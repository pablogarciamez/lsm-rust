use std::path::{PathBuf, Path};
use crate::memtable::{Value, Memtable};
use crate::wal::{read_wal, write_entry};
use crate::sstable::{get_sstable, write_sstable, merge_sstables};

use std::io::{Result, Error, ErrorKind};
use std::fs::{File, read_dir, remove_file};

pub struct Store {
    wal_filename: PathBuf,
    memtable: Memtable,
    directory: PathBuf,
    sstables: Vec<PathBuf>,
    counter: u64,
    max_length: usize,
}

fn list_sstables(directory: &Path) -> Result<Vec<(u64, PathBuf)>> {
    let mut found: Vec<(u64, PathBuf)> = Vec::new();
    for entry in read_dir(directory)? {
        let path: PathBuf = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sst") {
            continue;
        }
        let number = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidData,
                format!("sstable name is not numeric: {}", path.display()),
            )
        })?;
        found.push((number, path));
    }
    found.sort_by_key(|(n, _)| *n);
    Ok(found)
}

impl Store {
    pub fn new(wal_filename: &Path, directory: Option<&Path>, max_length: Option<usize>) -> Result<Store> {
        let directory = match directory {
            Some(d) => d,
            None => wal_filename
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        };

        let max_length: usize = match max_length {
            Some(d) => d,
            None => 100,
        };

        let found: Vec<(u64, PathBuf)> = list_sstables(directory)?;
        let counter: u64 = found.last().map_or(0, |(n,_)| n + 1);
        let sstables: Vec<PathBuf> = found.into_iter().map(|(_, p)| p).collect();

        let entries = match read_wal(wal_filename) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };

        let mut memtable = Memtable::new();
        for (op, key, val) in entries {
            if op == 0 {
                memtable.put(&key, &val);
            } else {
                memtable.delete(&key);
            }
        }

        Ok(Store{
            wal_filename: wal_filename.to_path_buf(),
            memtable,
            directory: directory.to_path_buf(),
            sstables,
            counter,
            max_length: max_length,
        })
    }

    pub fn put(&mut self, key: &str, val: &str) -> Result<()> {
        write_entry(0, key, val, &self.wal_filename)?;
        self.memtable.put(key, val);
        if self.memtable.data.len() >= self.max_length {
            self.flush()?;
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        if let Some(value) = self.memtable.get(key) {
            return Ok(match value {
                Value::Present(v) => Some(v.clone()),
                Value::Deleted => None,
            });
        }
        for sstable in self.sstables.iter().rev() {
            if let Some(value) = get_sstable(sstable, key)? {
                return Ok(match value {
                    Value::Present(v) => Some(v),
                    Value::Deleted => None,
                });
            }
        }
        Ok(None)
    }

    pub fn delete(&mut self, key: &str) -> Result<()> {
        write_entry(1, key, "", &self.wal_filename)?;
        self.memtable.delete(key);
        if self.memtable.data.len() >= self.max_length {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        let sst_filename: PathBuf = self.directory.join(format!("{}.sst", self.counter));
        write_sstable(&self.memtable, &sst_filename)?;
        self.counter += 1;
        self.sstables.push(sst_filename);
        self.memtable = Memtable::new();
        File::create(&self.wal_filename)?;
        Ok(())
    }

    pub fn compact(&mut self) -> Result<()>{
        let sst_filename: PathBuf = self.directory.join(format!("{}.sst", self.counter));
        let found: Vec<(u64, PathBuf)> = list_sstables(&self.directory)?;
        let sstable_paths: Vec<&Path> = found.iter().map(|(_, p)| p.as_path()).collect();
        write_sstable(&merge_sstables(&sstable_paths)?, &sst_filename)?;
        self.counter += 1;
        self.sstables.push(sst_filename.clone());
        for path in &self.sstables[..self.sstables.len() - 1] {
            remove_file(path)?;
        }
        self.sstables = vec![sst_filename];
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir: PathBuf = std::env::temp_dir().join(format!("lsm_rust_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn put_get_delete_roundtrip() -> Result<()> {
        let dir: PathBuf = temp_dir("store_put_get_delete_roundtrip");
        let wal: PathBuf = dir.join("wal.log");
        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(100))?;
        store.put("a", "1")?;
        store.put("b", "2")?;
        assert_eq!(store.get("a")?, Some("1".to_string()));
        assert_eq!(store.get("b")?, Some("2".to_string()));
        assert_eq!(store.get("z")?, None);
        store.put("b", "22")?;
        assert_eq!(store.get("b")?, Some("22".to_string()));
        store.delete("a")?;
        assert_eq!(store.get("a")?, None);
        assert_eq!(store.get("b")?, Some("22".to_string()));
        Ok(())
    }

    #[test]
    fn auto_flush_and_newer_sstable_shadows() -> Result<()> {
        let dir: PathBuf = temp_dir("store_auto_flush_and_newer_sstable_shadows");
        let wal: PathBuf = dir.join("wal.log");
        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(2))?;

        // Primer flush: 0.sst con a=1, b=2
        store.put("a", "1")?;
        assert!(!dir.join("0.sst").exists());
        store.put("b", "2")?;
        assert!(dir.join("0.sst").exists());
        assert_eq!(std::fs::metadata(&wal)?.len(), 0);

        // Segundo flush: 1.sst con a=11, c=3 (a sobrescribe la de 0.sst)
        store.put("a", "11")?;
        store.put("c", "3")?;
        assert!(dir.join("1.sst").exists());
        assert_eq!(store.get("a")?, Some("11".to_string()));
        assert_eq!(store.get("b")?, Some("2".to_string()));
        assert_eq!(store.get("c")?, Some("3".to_string()));

        // Tercer flush: 2.sst con b borrado, d=4
        store.delete("b")?;
        store.put("d", "4")?;
        assert!(dir.join("2.sst").exists());
        assert_eq!(store.get("b")?, None);
        assert_eq!(store.get("a")?, Some("11".to_string()));
        assert_eq!(store.get("d")?, Some("4".to_string()));
        Ok(())
    }

    #[test]
    fn reopen_recovers_from_wal() -> Result<()> {
        let dir: PathBuf = temp_dir("store_reopen_recovers_from_wal");
        let wal: PathBuf = dir.join("wal.log");

        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(100))?;
        store.put("a", "1")?;
        store.put("b", "2")?;
        store.put("b", "22")?;
        store.delete("a")?;
        assert!(std::fs::metadata(&wal)?.len() > 0);
        drop(store);

        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(100))?;
        assert_eq!(store.get("a")?, None);
        assert_eq!(store.get("b")?, Some("22".to_string()));
        assert_eq!(store.get("z")?, None);

        // Escribir tras recuperar y reabrir otra vez
        store.put("c", "3")?;
        drop(store);
        let store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(100))?;
        assert_eq!(store.get("a")?, None);
        assert_eq!(store.get("b")?, Some("22".to_string()));
        assert_eq!(store.get("c")?, Some("3".to_string()));
        Ok(())
    }

    #[test]
    fn reopen_recovers_from_sstables() -> Result<()> {
        let dir: PathBuf = temp_dir("store_reopen_recovers_from_sstables");
        let wal: PathBuf = dir.join("wal.log");

        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(2))?;
        store.put("a", "1")?;
        store.put("b", "2")?;      // flush -> 0.sst (a=1, b=2)
        store.delete("b")?;
        store.put("c", "3")?;      // flush -> 1.sst (b borrado, c=3)
        store.put("d", "4")?;      // se queda en memtable y WAL
        assert!(dir.join("0.sst").exists());
        assert!(dir.join("1.sst").exists());
        assert!(!dir.join("2.sst").exists());
        drop(store);

        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(2))?;
        assert_eq!(store.counter, 2);
        assert_eq!(store.sstables.len(), 2);
        assert_eq!(store.get("a")?, Some("1".to_string()));
        assert_eq!(store.get("b")?, None);
        assert_eq!(store.get("c")?, Some("3".to_string()));
        assert_eq!(store.get("d")?, Some("4".to_string()));

        // Un flush tras reabrir debe crear 2.sst sin pisar los anteriores
        store.put("e", "5")?;      // memtable llega a 2 -> flush -> 2.sst (d=4, e=5)
        assert!(dir.join("2.sst").exists());
        assert_eq!(store.get("a")?, Some("1".to_string()));
        assert_eq!(store.get("b")?, None);
        assert_eq!(store.get("c")?, Some("3".to_string()));
        assert_eq!(store.get("d")?, Some("4".to_string()));
        assert_eq!(store.get("e")?, Some("5".to_string()));
        Ok(())
    }

    #[test]
    fn compact_keeps_latest_and_removes_old() -> Result<()> {
        use crate::sstable::read_sstable;
        let dir: PathBuf = temp_dir("store_compact_keeps_latest_and_removes_old");
        let wal: PathBuf = dir.join("wal.log");
        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(2))?;

        store.put("a", "1")?;
        store.put("b", "2")?;      // flush -> 0.sst (a=1, b=2)
        store.put("a", "11")?;
        store.put("c", "3")?;      // flush -> 1.sst (a=11, c=3)
        store.delete("b")?;
        store.put("d", "4")?;      // flush -> 2.sst (b borrado, d=4)
        store.put("e", "5")?;      // se queda en memtable y WAL
        assert_eq!(store.sstables.len(), 3);
        assert_eq!(store.counter, 3);

        store.compact()?;

        let sst_files: Vec<String> = read_dir(&dir)?
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".sst"))
            .collect();
        assert_eq!(sst_files, vec!["3.sst".to_string()]);
        assert_eq!(store.sstables.len(), 1);
        assert_eq!(store.counter, 4);

        let expected: Vec<(String, Value)> = vec![
            ("a".to_string(), Value::Present("11".to_string())),
            ("c".to_string(), Value::Present("3".to_string())),
            ("d".to_string(), Value::Present("4".to_string())),
        ];
        assert_eq!(read_sstable(dir.join("3.sst").as_path())?, expected);

        assert_eq!(store.get("a")?, Some("11".to_string()));
        assert_eq!(store.get("b")?, None);
        assert_eq!(store.get("c")?, Some("3".to_string()));
        assert_eq!(store.get("d")?, Some("4".to_string()));
        assert_eq!(store.get("e")?, Some("5".to_string()));
        Ok(())
    }

    #[test]
    fn compact_then_reopen() -> Result<()> {
        let dir: PathBuf = temp_dir("store_compact_then_reopen");
        let wal: PathBuf = dir.join("wal.log");
        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(2))?;

        store.put("a", "1")?;
        store.put("b", "2")?;      // flush -> 0.sst (a=1, b=2)
        store.put("a", "11")?;
        store.put("c", "3")?;      // flush -> 1.sst (a=11, c=3)
        store.delete("b")?;
        store.put("d", "4")?;      // flush -> 2.sst (b borrado, d=4)
        store.put("e", "5")?;      // se queda en memtable y WAL
        store.compact()?;          // -> 3.sst, borra 0, 1 y 2
        drop(store);

        let mut store: Store = Store::new(wal.as_path(), Some(dir.as_path()), Some(2))?;
        assert_eq!(store.counter, 4);
        assert_eq!(store.sstables.len(), 1);
        assert_eq!(store.get("a")?, Some("11".to_string()));
        assert_eq!(store.get("b")?, None);
        assert_eq!(store.get("c")?, Some("3".to_string()));
        assert_eq!(store.get("d")?, Some("4".to_string()));
        assert_eq!(store.get("e")?, Some("5".to_string()));

        // Flush tras reabrir: memtable {e} + f llega a 2 -> 4.sst
        store.put("f", "6")?;
        assert!(dir.join("3.sst").exists());
        assert!(dir.join("4.sst").exists());
        assert!(!dir.join("0.sst").exists());
        assert_eq!(store.get("a")?, Some("11".to_string()));
        assert_eq!(store.get("b")?, None);
        assert_eq!(store.get("e")?, Some("5".to_string()));
        assert_eq!(store.get("f")?, Some("6".to_string()));
        Ok(())
    }
}