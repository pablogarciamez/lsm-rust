# lsm-rust

Reimplementación en Rust de la base de datos clave-valor LSM de [lsm](https://github.com/pablogarciamez/lsm) (Python): memtable ordenada, WAL, SSTables con índice, compactación y recuperación del estado al arrancar.

## Estructura

- `src/memtable.rs`: memtable ordenada sobre un `Vec`.
- `src/wal.rs`: write-ahead log.
- `src/sstable.rs`: escritura, lectura, búsqueda y fusión de SSTables.
- `src/store.rs`: `Store` con `put`/`get`/`delete`, flush, compactación y recuperación desde el WAL.
- `examples/bench.rs`: benchmarks (ver abajo).

## Tests

```
cargo test
```

22 tests: 6 de memtable, 10 de sstable (3 de ellos de merge) y 6 de store.

## Benchmarks: Python frente a Rust

### Qué se mide

El objetivo es comparar el rendimiento de las dos implementaciones de la misma estructura LSM, una en Python y otra en Rust. Se toman cinco benchmarks representativos. Cada uno se ejecuta con cuatro tamaños (n = 1000, 2000, 4000 y 8000) y cada combinación se repite cinco veces. Se toma la mediana para evitar el sesgo momentáneo de procesos externos del portátil.

| Benchmark | Preparación (fuera del cronómetro) | Lo que se mide |
|---|---|---|
| `memtable_put` | Memtable vacía y claves generadas | n `put` en la memtable, sin disco |
| `store_put` | Store vacío en un directorio nuevo, `max_length = n + 1` | n `put` en el Store (WAL + memtable) |
| `get` | n claves con `max_length = n / 4` (4 flushes) y `compact` | n `get` desde la SSTable compactada |
| `compact` | n claves con `max_length = n / 4` (4 SSTables) | una llamada a `compact` |
| `recovery` | n `put` con `max_length = n + 1` (todo queda en el WAL) | abrir un Store nuevo sobre ese WAL |

Los datos son idénticos en los dos lenguajes: claves `key000000`…, valores `val000000`…, y `get` lee las claves en el orden `(i * 7919) % n`.

Código: [`bench/bench.py`](https://github.com/pablogarciamez/lsm/blob/main/bench/bench.py) en lsm y `examples/bench.rs` en este repo.

### Entorno

- CPU Intel Core i5-1135G7 @ 2.40GHz, Ubuntu.
- Python 3.14.4.
- rustc 1.98.1, compilado con `--release`.

### Resultados

Mediana de 5 repeticiones, en ms. El ratio es el tiempo de Python dividido entre el de Rust: por encima de 1, Rust es más rápido.

| Benchmark | n | Python | Rust | Ratio |
|---|---|---|---|---|
| memtable_put | 1000 | 32,79 | 2,76 | 11,9 |
| | 2000 | 130,40 | 10,67 | 12,2 |
| | 4000 | 550,29 | 40,39 | 13,6 |
| | 8000 | 2183,19 | 161,62 | 13,5 |
| store_put | 1000 | 37,77 | 5,61 | 6,7 |
| | 2000 | 146,88 | 16,38 | 9,0 |
| | 4000 | 654,08 | 52,76 | 12,4 |
| | 8000 | 2289,49 | 192,51 | 11,9 |
| get | 1000 | 690,94 | 1120,78 | 0,62 |
| | 2000 | 2792,44 | 4411,39 | 0,63 |
| | 4000 | 11333,63 | 17606,33 | 0,64 |
| | 8000 | 46774,89 | 71257,55 | 0,66 |
| compact | 1000 | 73,49 | 10,88 | 6,8 |
| | 2000 | 273,39 | 31,67 | 8,6 |
| | 4000 | 1095,04 | 103,19 | 10,6 |
| | 8000 | 4500,10 | 367,16 | 12,3 |
| recovery | 1000 | 33,97 | 4,09 | 8,3 |
| | 2000 | 132,60 | 13,31 | 10,0 |
| | 4000 | 527,52 | 46,81 | 11,3 |
| | 8000 | 2195,42 | 173,58 | 12,7 |

Factor por el que se multiplica el tiempo de Rust al duplicar n:

| Benchmark | 1k→2k | 2k→4k | 4k→8k |
|---|---|---|---|
| memtable_put | 3,87 | 3,79 | 4,00 |
| store_put | 2,92 | 3,22 | 3,65 |
| get | 3,94 | 3,99 | 4,05 |
| compact | 2,91 | 3,26 | 3,56 |
| recovery | 3,25 | 3,52 | 3,71 |

En Python, los cinco benchmarks están entre ×3,5 y ×4,5 en todos los saltos.

### De dónde viene la diferencia

**Idea general.** Las dos implementaciones son prácticamente equivalentes: los algoritmos son iguales y, por tanto, la complejidad temporal también. El algoritmo decide la forma de la curva (cuánto crece el tiempo al duplicar n). El lenguaje decide la altura (lo rápido que se ejecuta cada operación). En `store_put`, `recovery` y `compact` hay una parte de lectura o escritura de archivos que crece de forma lineal y una parte de memtable que crece de forma cuadrática. En `get`, como se ve más abajo, también la lectura de archivos acaba siendo cuadrática.

**memtable_put.** La complejidad es cuadrática. Tal y como está hecho `put`, recorre la memtable buscando una clave mayor que la nueva. Como las claves llegan en orden, cada una acaba al final después de recorrer toda la lista, y al repetirlo n veces son unas n²/2 comparaciones (unos 32 millones con n = 8000). Este benchmark no hace ninguna petición al sistema operativo, solo operaciones simples, así que el ratio es bastante constante entre tamaños (entre 12 y 13,5). Python es más lento porque en él todo es un objeto: para una simple comparación tiene que acceder a las listas, comprobar si los tipos coinciden y después devolver el valor. En Rust los tipos están definidos al compilar y la comparación se reduce a unas pocas instrucciones del procesador. Si se divide el tiempo entre los 32 millones de comparaciones, sale unos 68 ns por comparación en Python y unos 5 ns en Rust.

**store_put.** Mide dos procesos distintos: la escritura en el WAL, que es lineal, y la inserción en la memtable, que es cuadrática. En Rust, como el segundo proceso es muy rápido, el primero se nota: con n pequeño no se llega a ×4 al duplicar n. Al crecer n, el coste cuadrático del segundo proceso predomina y el crecimiento se acerca a ×4. En Python no ocurre lo mismo: el proceso cuadrático es de por sí mucho más lento y la escritura en el archivo apenas se nota. Por eso el ratio sube con n (de 6,7 a 11,9) y se acerca al de `memtable_put`. Restando `memtable_put` a `store_put` en Rust queda el coste del WAL: unos 3 µs por put y casi constante (2,9; 2,9; 3,1 y 3,9 µs). Esa es la parte lineal. Con n = 1000 es la mitad del tiempo total de Rust, y con n = 8000 es el 16 %.

**recovery.** Sigue la misma lógica. La lectura del WAL es lineal, y la reconstrucción de la memtable dentro del Store, que hace un `put` por entrada, es cuadrática. Al subir n, esta segunda parte predomina en el resultado final.

**compact.** Mide lo rápido que se compactan 4 SSTables. Igual que antes, leer las SSTables y escribir la nueva es lineal, y fusionarlas, que hace `put` en dos pasadas, es cuadrático.

**get.** Es el más sorprendente, porque es el único en el que Rust es más lento (ratio 0,66). Primero, es cuadrático aunque la búsqueda sea binaria. `get_sstable` lee el índice entero (n entradas) en cada llamada, antes de buscar. Ese coste lineal por get, repetido n veces, da un total cuadrático y es mucho mayor que lo que tarda la búsqueda binaria. Segundo, Rust es más lento porque la lectura no está optimizada. En Python, `open()` crea un lector con búfer: al leer, pide al sistema operativo un bloque de varios KB, y las lecturas siguientes salen de ese bloque hasta que se acaba. En Rust, `File` no tiene búfer: cada `read_exact` y cada `seek` es una petición independiente al sistema operativo. Con n = 8000 son unas 32.000 peticiones por get en Rust (4 por cada entrada del índice) frente a unas 21 en Python (unos 168 KB de índice en bloques de 8 KB). Aun haciendo unas 1500 veces más peticiones, Rust solo es unas 1,5 veces más lento. Probablemente es porque procesar cada entrada del índice le cuesta mucho menos que a Python. **Esta explicación es una hipótesis argumentada, no medida** (ver "Qué queda abierto").

### Limitaciones de la medida

- Todo se midió en una sola máquina y un solo sistema operativo, con una sola ejecución del script por lenguaje.
- Las medianas se calculan sobre repeticiones contiguas. Si justo hay un proceso en paralelo, afecta a todas. De ahí la diferencia entre ejecuciones: `store_put` con n = 4000 en Python dio 531 ms en una ejecución y 654 ms en otra (+23 %).
- Es una implementación muy básica y simplificada. Usa algoritmos cuadráticos (inserción lineal en la memtable, índice recargado en cada lectura) donde las bases de datos clave-valor reales usan estructuras de coste log n o n·log n.

### Qué falló

- La primera versión del benchmark de `get` medía la memtable y no el disco. `compact` solo fusiona los `.sst` existentes y no hace flush de la memtable, y con `max_length = n + 1` nunca se llegaba a crear una SSTable. Se corrigió con `max_length = n / 4`.
- La primera versión de `compact` usaba `max_length = 100`, así que el número de SSTables a fusionar crecía con n (de 10 a 80) y variaban dos cosas a la vez. Se fijó en 4 SSTables con `max_length = n / 4`.

### Qué queda abierto

- Confirmar la hipótesis de `get`: envolver el archivo en un `std::io::BufReader` dentro de `read_index` y volver a medir. Si es correcta, Rust debería pasar a ser más rápido que Python en `get`.
- Repetir las mediciones varias veces y en otra máquina.

## Limitaciones conocidas de la implementación

- El WAL no tiene checksum. `read_entry` convierte cualquier error en `None`, así que una entrada truncada y una corrupta son indistinguibles.
- No se llama a `sync_all()` antes del rename.
- La compactación es siempre total, sin niveles.
- `compact` sobre un Store sin SSTables escribe una SSTable vacía.
- Sin test: más de 10 SSTables en `list_sstables`, una segunda compactación tras reabrir, y un WAL con una entrada truncada seguida de un put posterior.
- Una longitud de clave corrupta y enorme en `read_index` o `read_entry` podría abortar el proceso al reservar memoria.

## Reproducir los benchmarks

Python:

```
git clone https://github.com/pablogarciamez/lsm
cd lsm
pip install -e .
python bench/bench.py
```

Rust:

```
git clone https://github.com/pablogarciamez/lsm-rust
cd lsm-rust
cargo run --release --example bench
```

Cada script tarda varios minutos, sobre todo por `get` con n = 8000. No suspendas el equipo durante la ejecución.