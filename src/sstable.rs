use std::fs::File;
use std::path::{Path, PathBuf};
use std::io::{Read, Seek, SeekFrom, Write, Error, ErrorKind, Result};
use crate::memtable::{Memtable, Value};
use crate::wal::read_entry;

pub fn write_sstable(memtable: &Memtable, path: &Path) -> Result<()>{
    let temp_path: PathBuf = path.with_extension("tmp");
    let mut index: Vec<(&str, u64)> = Vec::new();
    let mut f = File::create(&temp_path)?;
    for (key, val) in memtable.data.iter() {
        let pos: u64 = f.seek(SeekFrom::Current(0))?;
        index.push((key.as_str(), pos));
        let (op_type, val): (u8, &str) = match val{
            Value::Present(v) => (0, v.as_str()),
            Value::Deleted => (1, ""),
        };
        f.write_all(&[op_type])?;
        let encoded_key: &[u8] = key.as_bytes();
        let len_key: u32 = encoded_key.len() as u32;
        let bytes_len_key = len_key.to_le_bytes();
        f.write_all(&bytes_len_key)?;
        f.write_all(encoded_key)?;
        let encoded_val: &[u8] = val.as_bytes();
        let len_val: u32 = encoded_val.len() as u32;
        let bytes_len_val = len_val.to_le_bytes();
        f.write_all(&bytes_len_val)?;
        f.write_all(encoded_val)?;
    }
    let index_start: u64 = f.seek(SeekFrom::Current(0))?;
    for (key, pos) in index.iter() {
        let encoded_key: &[u8] = key.as_bytes();
        let len_key: u32 = encoded_key.len() as u32;
        let bytes_len_key = len_key.to_le_bytes();
        f.write_all(&bytes_len_key)?;
        f.write_all(encoded_key)?;
        f.write_all(&pos.to_le_bytes())?;
    }
    f.write_all(&index_start.to_le_bytes())?;
    std::fs::rename(temp_path, path)?;
    Ok(())
}

pub fn read_index(path: &Path) -> Result<Vec<(String, u64)>> {
    let mut index: Vec<(String, u64)> = Vec::new();
    let mut f: File = File::open(path)?;
    f.seek(SeekFrom::End(-8))?;
    let index_end: u64 = f.seek(SeekFrom::Current(0))?;
    let mut bytes_index_start: [u8; 8] = [0u8; 8];
    f.read_exact(&mut bytes_index_start)?;
    let index_start: u64 = u64::from_le_bytes(bytes_index_start);
    if index_start > index_end {
        return Err(Error::new(ErrorKind::InvalidData, "Invalid index start"));
    }
    f.seek(SeekFrom::Start(index_start))?;
    while f.seek(SeekFrom::Current(0))? < index_end {
        let mut bytes_len_key: [u8; 4] = [0u8; 4];
        f.read_exact(&mut bytes_len_key)?;
        let len_key: u32 = u32::from_le_bytes(bytes_len_key);
        let mut bytes_key: Vec<u8> = vec![0u8; len_key as usize];
        f.read_exact(&mut bytes_key)?;
        let key: String = String::from_utf8(bytes_key).map_err(|e: std::string::FromUtf8Error|Error::new(ErrorKind::InvalidData, e))?;
        let mut bytes_pos: [u8; 8] = [0u8; 8];
        f.read_exact(&mut bytes_pos)?;
        let pos: u64 = u64::from_le_bytes(bytes_pos);
        index.push((key, pos));
    }
    Ok(index)
}

pub fn read_sstable(path: &Path) -> Result<Vec<(String, Value)>> {
    let mut key_vals: Vec<(String, Value)> = Vec::new();
    let index: Vec<(String, u64)> = read_index(path)?;
    let mut f: File = File::open(path)?;
    for (key, pos) in index.iter() {
        f.seek(SeekFrom::Start(*pos))?;
        let entry: (u8, String, String) = read_entry(&mut f).ok_or_else(|| Error::new(ErrorKind::UnexpectedEof, "Unexpected EOF"))?;
        key_vals.push(match entry.0 {
           0 => (key.clone(), Value::Present(entry.2)),
           1 => (key.clone(), Value::Deleted),
           _ => return Err(Error::new(ErrorKind::InvalidData, "Invalid op_type")),
        });
    }
    Ok(key_vals)
}

pub fn get_sstable(path: &Path, key: &str) -> Result<Option<Value>> {
    let mut f: File = File::open(path)?;
    let index: Vec<(String, u64)> = read_index(path)?;
    if index.is_empty() { return Ok(None); }
    let mut high: usize = index.len() - 1;
    let mut low: usize = 0;
    while low <= high {
        let mid: usize = (high + low) / 2;
        if index[mid].0 == *key {
            f.seek(SeekFrom::Start(index[mid].1))?;
            let entry: (u8, String, String) = read_entry(&mut f).ok_or_else(|| Error::new(ErrorKind::UnexpectedEof, "Unexpected EOF"))?;
            return Ok(Some(match entry.0 {
                0 => Value::Present(entry.2),
                1 => Value::Deleted,
                _ => return Err(Error::new(ErrorKind::InvalidData, "Invalid op_type")),
            }));
        }
        if index[mid].0.as_str() < key {
            low = mid + 1;
        } else {
            if mid == 0 { break; }
            high = mid - 1;
        }
    }
    Ok(None)
}

pub fn merge_sstables(paths: &[&Path]) -> Result<Memtable> {
    let mut memtable: Memtable = Memtable::new();
    for path in paths.iter() {
        let key_vals: Vec<(String, Value)> = read_sstable(path)?;
        for (key, val) in key_vals.iter() {
            match val {
                Value::Present(v) => memtable.put(key, v),
                Value::Deleted => memtable.delete(key),
            }
        }
    }
    let mut filtered_memtable: Memtable = Memtable::new();
    for (key, val) in memtable.data.iter() {
        if let Value::Present(v) = val {
            filtered_memtable.put(key, v);
        }
    }
    Ok(filtered_memtable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::read_dir;

    fn temp_dir(name: &str) -> PathBuf {
        let dir: PathBuf = std::env::temp_dir().join(format!("lsm_rust_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_then_read_roundtrip() -> Result<()> {
        let path: PathBuf = temp_dir("write_then_read_roundtrip");
        let mut memtable: Memtable = Memtable::new();
        memtable.put("a", "1");
        memtable.put("b", "4");
        memtable.put("c", "9");
        memtable.delete("d");
        let sst_path:PathBuf = path.join("0.sst");
        write_sstable(&memtable, sst_path.as_path())?;
        let key_vals: Vec<(String, Value)> = read_sstable(sst_path.as_path())?;
        let real_key_vals: Vec<(String, Value)> = memtable.data;
        assert_eq!(key_vals, real_key_vals);
        Ok(())
    }

    #[test]
    fn get_sstable_finds_every_key() -> Result<()> {
        let path: PathBuf = temp_dir("get_sstable_finds_every_key");
        let mut memtable: Memtable = Memtable::new();
        memtable.put("a", "1");
        memtable.put("b", "4");
        memtable.put("c", "9");
        memtable.put("d", "16");
        memtable.put("e", "25");
        memtable.put("f", "36");
        memtable.put("g", "49");
        let sst_path:PathBuf = path.join("0.sst");
        write_sstable(&memtable, sst_path.as_path())?;
        assert_eq!(get_sstable(sst_path.as_path(), "a")?, Some(Value::Present("1".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "b")?, Some(Value::Present("4".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "c")?, Some(Value::Present("9".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "d")?, Some(Value::Present("16".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "e")?, Some(Value::Present("25".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "f")?, Some(Value::Present("36".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "g")?, Some(Value::Present("49".to_string())));
        Ok(())
    }

    #[test]
    fn get_sstable_missing_key_returns_none() -> Result<()> {
        let path: PathBuf = temp_dir("get_sstable_missing_key_returns_none");
        let mut memtable: Memtable = Memtable::new();
        memtable.put("b", "4");
        memtable.put("c", "9");
        memtable.delete("e");
        memtable.put("f", "36");
        let sst_path:PathBuf = path.join("0.sst");
        write_sstable(&memtable, sst_path.as_path())?;
        assert_eq!(get_sstable(sst_path.as_path(), "a")?, None);
        assert_eq!(get_sstable(sst_path.as_path(), "d")?, None);
        assert_eq!(get_sstable(sst_path.as_path(), "g")?, None);
        assert_eq!(get_sstable(sst_path.as_path(), "e")?, Some(Value::Deleted));
        Ok(())
    }

    #[test]
    fn unicode_and_empty_values_roundtrip() -> Result<()> {
        let path: PathBuf = temp_dir("unicode_and_empty_values_roundtrip");
        let mut memtable: Memtable = Memtable::new();
        memtable.put("abc", "ñ");
        memtable.put("ñ", "日本語");
        memtable.put("日本語", "");
        memtable.put("dw", "❤️");
        let sst_path:PathBuf = path.join("0.sst");
        write_sstable(&memtable, sst_path.as_path())?;
        let key_vals: Vec<(String, Value)> = read_sstable(sst_path.as_path())?;
        let real_key_vals: Vec<(String, Value)> = memtable.data;
        assert_eq!(key_vals, real_key_vals);
        assert_eq!(get_sstable(sst_path.as_path(), "abc")?, Some(Value::Present("ñ".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "ñ")?, Some(Value::Present("日本語".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "日本語")?, Some(Value::Present("".to_string())));
        assert_eq!(get_sstable(sst_path.as_path(), "dw")?, Some(Value::Present("❤️".to_string())));
        Ok(())
    }

    #[test]
    fn write_leaves_no_tmp_file() -> Result<()> {
        let path: PathBuf = temp_dir("write_leaves_no_tmp_file");
        let mut memtable: Memtable = Memtable::new();
        memtable.put("a", "1");
        let sst_path:PathBuf = path.join("0.sst");
        write_sstable(&memtable, sst_path.as_path())?;
        let mut entries = read_dir(path).unwrap();
        let entry = entries.next().unwrap().unwrap();
        assert!(entries.next().is_none(), "¡Hay más de un elemento en el directorio!");
        assert_eq!(entry.file_name().into_string().unwrap(), "0.sst");
        Ok(())
    }

    #[test]
    fn empty_sstable_has_defined_behavior() -> Result<()> {
        let path: PathBuf = temp_dir("empty_sstable_has_defined_behavior");
        let memtable: Memtable = Memtable::new();
        let sst_path:PathBuf = path.join("0.sst");
        write_sstable(&memtable, sst_path.as_path())?;
        assert_eq!(read_sstable(sst_path.as_path())?, Vec::new());
        assert_eq!(get_sstable(sst_path.as_path(), "abc")?, None);
        assert_eq!(read_index(sst_path.as_path())?, Vec::new());
        Ok(())
    }

    #[test]
    fn read_index_rejects_corrupt_footer() -> Result<()> {
        let path: PathBuf = temp_dir("read_index_rejects_corrupt_footer");
        let short_path: PathBuf = path.join("short.sst");
        std::fs::write(&short_path, [1u8, 2, 3])?;
        let eight_path: PathBuf = path.join("eight.sst");
        std::fs::write(&eight_path, 1000u64.to_le_bytes())?;
        let big_path: PathBuf = path.join("big.sst");
        std::fs::write(&big_path, u64::MAX.to_le_bytes())?;
        let result:std::prelude::v1::Result<Vec<(String, u64)>, Error> = read_index(short_path.as_path());
        assert!(result.is_err());
        let result:std::prelude::v1::Result<Vec<(String, u64)>, Error> = read_index(eight_path.as_path());
        assert!(result.is_err());
        let result:std::prelude::v1::Result<Vec<(String, u64)>, Error> = read_index(big_path.as_path());
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn merge_disjoint_keys() -> Result<()> {
        let dir: PathBuf = temp_dir("merge_disjoint_keys");
        let mut m0: Memtable = Memtable::new();
        m0.put("a", "1");
        m0.put("c", "3");
        let mut m1: Memtable = Memtable::new();
        m1.put("b", "2");
        m1.put("d", "4");
        let p0: PathBuf = dir.join("0.sst");
        let p1: PathBuf = dir.join("1.sst");
        write_sstable(&m0, p0.as_path())?;
        write_sstable(&m1, p1.as_path())?;
        let merged: Memtable = merge_sstables(&[p0.as_path(), p1.as_path()])?;
        let expected: Vec<(String, Value)> = vec![
            ("a".to_string(), Value::Present("1".to_string())),
            ("b".to_string(), Value::Present("2".to_string())),
            ("c".to_string(), Value::Present("3".to_string())),
            ("d".to_string(), Value::Present("4".to_string())),
        ];
        assert_eq!(merged.data, expected);
        Ok(())
    }

    #[test]
    fn merge_later_path_wins() -> Result<()> {
        let dir: PathBuf = temp_dir("merge_later_path_wins");
        let mut m0: Memtable = Memtable::new();
        m0.put("a", "viejo");
        m0.put("b", "solo_en_0");
        let mut m1: Memtable = Memtable::new();
        m1.put("a", "nuevo");
        m1.put("c", "solo_en_1");
        let p0: PathBuf = dir.join("0.sst");
        let p1: PathBuf = dir.join("1.sst");
        write_sstable(&m0, p0.as_path())?;
        write_sstable(&m1, p1.as_path())?;

        let merged: Memtable = merge_sstables(&[p0.as_path(), p1.as_path()])?;
        let expected: Vec<(String, Value)> = vec![
            ("a".to_string(), Value::Present("nuevo".to_string())),
            ("b".to_string(), Value::Present("solo_en_0".to_string())),
            ("c".to_string(), Value::Present("solo_en_1".to_string())),
        ];
        assert_eq!(merged.data, expected);

        let merged_rev: Memtable = merge_sstables(&[p1.as_path(), p0.as_path()])?;
        let expected_rev: Vec<(String, Value)> = vec![
            ("a".to_string(), Value::Present("viejo".to_string())),
            ("b".to_string(), Value::Present("solo_en_0".to_string())),
            ("c".to_string(), Value::Present("solo_en_1".to_string())),
        ];
        assert_eq!(merged_rev.data, expected_rev);
        Ok(())
    }

    #[test]
    fn merge_tombstone_removes_older_value() -> Result<()> {
        let dir: PathBuf = temp_dir("merge_tombstone_removes_older_value");
        let mut m0: Memtable = Memtable::new();
        m0.put("a", "1");
        m0.put("b", "2");
        m0.delete("d");
        let mut m1: Memtable = Memtable::new();
        m1.delete("a");
        m1.delete("z");
        m1.put("c", "3");
        m1.put("d", "4");
        let p0: PathBuf = dir.join("0.sst");
        let p1: PathBuf = dir.join("1.sst");
        write_sstable(&m0, p0.as_path())?;
        write_sstable(&m1, p1.as_path())?;
        let merged: Memtable = merge_sstables(&[p0.as_path(), p1.as_path()])?;
        let expected: Vec<(String, Value)> = vec![
            ("b".to_string(), Value::Present("2".to_string())),
            ("c".to_string(), Value::Present("3".to_string())),
            ("d".to_string(), Value::Present("4".to_string())),
        ];
        assert_eq!(merged.data, expected);
        Ok(())
    }
}