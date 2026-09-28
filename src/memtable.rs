#[derive(Debug, PartialEq)]
pub enum Value {
    Present(String),
    Deleted,
}

pub struct Memtable {
    pub data: Vec<(String, Value)>,
}

impl Memtable {
    pub fn new() -> Self {
        Memtable { data: Vec::new() }
    }

    pub fn put(&mut self, key: &str, val: &str) {
        let mut is_put: bool = false;
        for i in 0..self.data.len() {
            if self.data[i].0.as_str() > key {
                self.data.insert(i, (key.to_string(), Value::Present(val.to_string())));
                is_put = true;
                break;
            } else if self.data[i].0 == *key {
                self.data[i].1 = Value::Present(val.to_string());
                is_put = true;
                break;
            }
        }
        if !is_put {
            self.data.push((key.to_string(), Value::Present(val.to_string())));
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        if self.data.is_empty() {
            return None;
        }
        let mut low: usize = 0;
        let mut high: usize = self.data.len() - 1;
        while low <= high {
            let mid: usize = (low + high) / 2;
            if self.data[mid].0 == key {
                return Some(&self.data[mid].1);
            } else if self.data[mid].0.as_str() < key {
                low = mid + 1;
            } else {
                if mid == 0 { break; }
                high = mid - 1;
            }
        }
        None
    }

    pub fn delete(&mut self, key: &str) {
        let mut is_deleted: bool = false;
        for i in 0..self.data.len() {
            if self.data[i].0.as_str() > key {
                self.data.insert(i, (key.to_string(), Value::Deleted));
                is_deleted = true;
                break;
            } else if self.data[i].0 == key {
                self.data[i].1 = Value::Deleted;
                is_deleted = true;
                break;
            }
        }
        if !is_deleted {
            self.data.push((key.to_string(), Value::Deleted));
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_and_get() {
        let mut m: Memtable = Memtable::new();
        m.put("key", "val");
        assert_eq!(m.get("key"), Some(&Value::Present("val".to_string())));
    }

    #[test]
    fn get_nonexistent_key() {
        let m: Memtable = Memtable::new();
        assert_eq!(m.get("key"), None);
    }

    #[test]
    fn test_overwrite_same_key() {
        let mut m: Memtable = Memtable::new();
        m.put("key", "val1");
        m.put("key", "val2");
        assert_eq!(m.get("key"), Some(&Value::Present("val2".to_string())));
        assert_eq!(m.data.len(), 1);
    }

    #[test]
    fn test_delete_marks_as_deleted() {
        let mut m: Memtable = Memtable::new();
        m.put("key", "val");
        m.delete("key");
        assert_eq!(m.get("key"), Some(&Value::Deleted));
    }

    #[test]
    fn test_delete_nonexistent_key() {
        let mut m: Memtable = Memtable::new();
        m.delete("key");
        assert_eq!(m.get("key"), Some(&Value::Deleted));
    }

    #[test]
    fn test_empty_memtable_get() {
        let m: Memtable = Memtable::new();
        assert_eq!(m.get("key"), None);
    }
}