use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lsm_rust::memtable::Memtable;
use lsm_rust::store::Store;

const REPETICIONES: usize = 5;
const TAMANOS: [usize; 4] = [1000, 2000, 4000, 8000];

fn main() -> std::io::Result<()> {
    for n in TAMANOS {
        let mut tiempos: Vec<Duration> = Vec::new();
        for _r in 0..REPETICIONES {
            let mut memtable = Memtable::new();
            let mut claves: Vec<String> = Vec::new();
            let mut valores: Vec<String> = Vec::new();
            for i in 0..n {
                claves.push(format!("key{i:06}"));
                valores.push(format!("val{i:06}"));
            }

            let inicio = Instant::now();

            for i in 0..n {
                memtable.put(&claves[i], &valores[i]);
            }

            let t = inicio.elapsed();

            tiempos.push(t);
        }
        tiempos.sort();
        let mediana = tiempos[REPETICIONES / 2].as_secs_f64();
        println!("memtable_put  n={:5}  {:9.2} ms  {:7.2} us/op", n, mediana * 1000.0, mediana / n as f64 * 1e6);
    }

    for n in TAMANOS {
        let mut tiempos: Vec<Duration> = Vec::new();
        for r in 0..REPETICIONES {
            let d: PathBuf = std::env::temp_dir().join(format!("lsm-bench-store_put-{}-{}", n, r));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d)?;

            let mut store = Store::new(&d.join("data.wal"), None, Some(n + 1))?;
            let mut claves: Vec<String> = Vec::new();
            let mut valores: Vec<String> = Vec::new();
            for i in 0..n {
                claves.push(format!("key{i:06}"));
                valores.push(format!("val{i:06}"));
            }

            let inicio = Instant::now();

            for i in 0..n {
                store.put(&claves[i], &valores[i])?;
            }

            let t = inicio.elapsed();

            tiempos.push(t);
            fs::remove_dir_all(&d)?;
        }
        tiempos.sort();
        let mediana = tiempos[REPETICIONES / 2].as_secs_f64();
        println!("store_put  n={:5}  {:9.2} ms  {:7.2} us/op", n, mediana * 1000.0, mediana / n as f64 * 1e6);
    }

    for n in TAMANOS {
        let mut tiempos: Vec<Duration> = Vec::new();
        for r in 0..REPETICIONES {
            let d: PathBuf = std::env::temp_dir().join(format!("lsm-bench-get-{}-{}", n, r));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d)?;

            let mut store = Store::new(&d.join("data.wal"), None, Some(n / 4))?;
            let mut claves: Vec<String> = Vec::new();
            for i in 0..n {
                claves.push(format!("key{i:06}"));
                store.put(&format!("key{i:06}"),&format!("val{i:06}"))?;
            }
            store.compact()?;

            let inicio = Instant::now();

            for i in 0..n {
                std::hint::black_box(store.get(&claves[(i * 7919) % n])?);
            }

            let t = inicio.elapsed();

            tiempos.push(t);
            fs::remove_dir_all(&d)?;
        }
        tiempos.sort();
        let mediana = tiempos[REPETICIONES / 2].as_secs_f64();
        println!("get  n={:5}  {:9.2} ms  {:7.2} us/op", n, mediana * 1000.0, mediana / n as f64 * 1e6);
    }

    for n in TAMANOS {
        let mut tiempos: Vec<Duration> = Vec::new();
        for r in 0..REPETICIONES {
            let d: PathBuf = std::env::temp_dir().join(format!("lsm-bench-compact-{}-{}", n, r));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d)?;

            let mut store = Store::new(&d.join("data.wal"), None, Some(n / 4))?;
            for i in 0..n {
                store.put(&format!("key{i:06}"),&format!("val{i:06}"))?;
            }

            let inicio = Instant::now();

            store.compact()?;

            let t = inicio.elapsed();

            tiempos.push(t);
            fs::remove_dir_all(&d)?;
        }
        tiempos.sort();
        let mediana = tiempos[REPETICIONES / 2].as_secs_f64();
        println!("compact  n={:5}  {:9.2} ms  {:7.2} us/op", n, mediana * 1000.0, mediana / n as f64 * 1e6);
    }

    for n in TAMANOS {
        let mut tiempos: Vec<Duration> = Vec::new();
        for r in 0..REPETICIONES {
            let d: PathBuf = std::env::temp_dir().join(format!("lsm-bench-recovery-{}-{}", n, r));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d)?;

            let mut store = Store::new(&d.join("data.wal"), None, Some(n + 1))?;
            for i in 0..n {
                store.put(&format!("key{i:06}"),&format!("val{i:06}"))?;
            }

            let inicio = Instant::now();

            let _other_store = Store::new(&d.join("data.wal"), None, Some(n + 1))?;

            let t = inicio.elapsed();

            tiempos.push(t);
            fs::remove_dir_all(&d)?;
        }
        tiempos.sort();
        let mediana = tiempos[REPETICIONES / 2].as_secs_f64();
        println!("recovery  n={:5}  {:9.2} ms  {:7.2} us/op", n, mediana * 1000.0, mediana / n as f64 * 1e6);
    }
    Ok(())
}