use std::fs::OpenOptions;
use std::io::Write;

use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn write_entry(op_type: u8, key: &str, val: &str, path: &Path) -> std::io::Result<()> {
    let mut f: std::fs::File = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let bytes_type: [u8; 1] = op_type.to_le_bytes();
    let encoded_key: &[u8] = key.as_bytes();
    let len_key: u32 = encoded_key.len() as u32;
    let bytes_len_key: [u8; 4] = len_key.to_le_bytes();
    let encoded_val: &[u8] = val.as_bytes();
    let len_val = encoded_val.len() as u32;
    let bytes_len_val: [u8; 4] = len_val.to_le_bytes();
    f.write_all(&bytes_type)?;
    f.write_all(&bytes_len_key)?;
    f.write_all(encoded_key)?;
    f.write_all(&bytes_len_val)?;
    f.write_all(encoded_val)?;
    Ok(())
}

pub fn read_entry(f: &mut File) -> Option<(u8, String, String)> {
    let mut type_buf: [u8; 1] = [0u8; 1];
    let n: usize = f.read(&mut type_buf).ok()?;
    if n == 0 { return None; }
    let op_type = type_buf[0];
    let mut len_key_buf: [u8; 4] = [0u8; 4];
    f.read_exact(&mut len_key_buf).ok()?;
    let len_key: u32 = u32::from_le_bytes(len_key_buf);
    let mut key_buf: Vec<u8> = vec![0u8; len_key as usize];
    f.read_exact(&mut key_buf).ok()?;
    let key: String = String::from_utf8(key_buf).ok()?;
    let mut len_val_buf: [u8; 4] = [0u8; 4];
    f.read_exact(&mut len_val_buf).ok()?;
    let len_val: u32 = u32::from_le_bytes(len_val_buf);
    let mut val_buf: Vec<u8> = vec![0u8; len_val as usize];
    f.read_exact(&mut val_buf).ok()?;
    let val: String = String::from_utf8(val_buf).ok()?;
    Some((op_type, key, val))
}

pub fn read_wal(path: &Path) -> std::io::Result<Vec<(u8, String, String)>> {
    let mut f = File::open(path)?;
    let mut entries: Vec<(u8, String, String)> = Vec::new();
    while let Some(entry) = read_entry(&mut f) { entries.push(entry); }
    Ok(entries)
}