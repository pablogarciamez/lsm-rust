use std::fs::File;
use std::path::{Path};
use std::io::{Read, Seek, SeekFrom, Write, Error, ErrorKind, Result};
use crate::memtable::{Memtable, Value};
use crate::wal::read_entry;

pub fn write_sstable(memtable: &Memtable, path: &Path) -> Result<()>{
    let temp_path = path.parent().unwrap().join("temp.sst");
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