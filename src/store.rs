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
    pub fn new(&self, wal_filename: &Path, directory: Option<&Path>, max_length: Option<usize>) -> Result<Store> {
        let directory: &Path = match directory {
            Some(d) => d,
            None => wal_filename.parent().unwrap_or(Path::new(".")),
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
            memtable: Memtable::new(),
            directory: directory.to_path_buf(),
            sstables: sstables,
            counter: counter,
            max_length: max_length,
        })
    }

    pub fn put(&mut self, key: &str, val: &str) -> Result<()> {
        write_entry(0, key, val, &self.wal_filename.to_path_buf())?;
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
        write_sstable(&self.memtable, &sst_filename.as_path())?;
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
        write_sstable(&merge_sstables(&sstable_paths)?, &sst_filename.clone())?;
        self.counter += 1;
        self.sstables.push(sst_filename.clone());
        for path in &self.sstables[..self.sstables.len() - 1] {
            remove_file(path)?;
        }
        self.sstables = vec![sst_filename];
        Ok(())
    }
}