# factomic

Factomic is a unitemporal, embedded fact database backed by [fjall](https://crates.io/crates/fjall) born out of a need for a Datomic-like model and all its audit-trail advantages, while fully supporting ordered list and map operations.

It is strongly derived from the [Datomic](https://www.datomic.com/rationale.html) information model, with the addition of Slots for ordinal lists and map keys. A slot uses relative fractional indexing rather than absolute indexing to ensure that facts remain immutable and independantly valid.

Factomic is **tentatively** ACID-compliant (real power-loss and mid-journal crashes have not been tested). It lacks any real-world usage, thus should be considered pre-production code and prone to **breaking changes**.

## Quick start

```rust
use std::sync::Arc;
use factomic::{Result, Store};

#[derive(factomic::Entity)]
struct Person {
    name: String,
    #[factomic(indexed)]
    age: i64,
    manager: Option<Arc<Person>>,
}

fn main() -> Result<()> {
    let store = Arc::new(Store::open("factomic.db")?);
    let manager = store.create::<Person>()?;
    let mut tx = manager.transact();
    tx.set_name("Alice").set_age(30).set_manager(&manager);
    tx.commit()?;

    let query = store.db().query::<Person>()?.expect("person schema");
    let people = query.find_by_age(30)?;
    assert_eq!(people.len(), 1);
    Ok(())
}
```

## Batteries Included

- `One`, `List`, `Map`, and `Many` collections.
- Optional single and map values with explicit clear operations.
- Primitive and entity references with borrowed-handle conversion.
- Incremental mutations, replacement, and ordinal list operations.
- Indexed exact and reverse-reference queries.
- Historical snapshots, retraction, and existence checks.
- Full-definition schema conflict checks, atomic multi-attribute declarations, and storage format versioning.

See `crates/factomic-derive/README.md` for field syntax and `crates/factomic-example/src/main.rs` for a complete example.

## ID allocation

Entity and Transaction IDs increase monotonically but are not guaranteed to be contiguous. Allocating an entity consumes its ID even if no facts are asserted.

Successful mutations provide the durability guarantee via `fsync`.

## Verification

```text
cargo test --workspace --all-features
cargo run -p factomic-example
```

## Benchmarking

Run the Criterion benchmarks with `cargo bench -p factomic`, or compile them without running using `cargo bench -p factomic --no-run`. Benchmarks use real temporary storage and normal `SyncAll` durability, so write latency depends heavily on the filesystem and hardware.

Reports are written to `target/criterion/report/index.html`. Criterion 0.8 and Rust 1.90 are used.

Save and compare baselines with:

```text
cargo bench -p factomic -- --save-baseline main
cargo bench -p factomic -- --baseline main
```

### Latest measured results

Measured on an AMD Ryzen 7 9800X3D with 16 threads, approximately 94 GiB RAM, Windows 11 Home 26200, and a Samsung SSD 990 PRO 1TB. Versions: Factomic 0.2.0, fjall 3.1.13, redb 5.0.0, rusqlite 0.43.0, heed 0.23.1, Criterion 0.8.2, and rustc 1.99.1. 

All writes used strict durability.

| Benchmark | Factomic | fjall | redb | rusqlite | heed |
|---|---:|---:|---:|---:|---:|
| `commit_1` | 1.271 ms | 1.372 ms | 1.348 ms | 1.373 ms | 193.6 µs |
| `commit_100` | 1.744 ms | 1.375 ms | 1.349 ms | 1.498 ms | 265.7 µs |
| `point_read` | 722 ns | 193 ns | 801 ns | 1.856 µs | 157 ns |
| `range_read_100` | 21.92 µs | 10.52 µs | 8.35 µs | 8.78 µs | 5.02 µs |
| `commit_100` throughput | 57.3K records/s | 72.7K records/s | 74.1K records/s | 66.8K records/s | 376.4K records/s |

Practical Factomic workloads:

| Operation | Result |
|---|---:|
| Create one complete record | 2.831 ms |
| Create 10 records in one transaction | 15.73 ms total, 636 records/s |
| Read a complete record | 5.22 µs, approximately 191K records/s |
| Update indexed field | 1.369 ms |
| Append list item | 1.465 ms |
| Update map entry | 1.395 ms |
| Retract entity | 1.434 ms |
| Assert one fact | 640 facts/s |
| Assert 10 facts in one transaction | 8.76K facts/s |
| Assert 100 facts in one transaction | 66.8K facts/s |
| Overwrite indexed fact | 743 overwrites/s |
| Declare one attribute | 1.603 ms |
| Declare 16 attributes in one transaction | 1.349 ms, approximately 11.9K attributes/s |
| Allocate an entity | 1.382 ms |
| Open a fresh store | 220.96 ms |
| Reopen the small populated store | 9.34 ms |
| Reopen the 1,000-fact store | 17.34 ms |

## Database comparisons

Run `cargo bench -p factomic --bench compare` for strict-durability comparisons with fjall 3.1.13, redb 5.0.0, rusqlite 0.43.0, and heed 0.23.1. Comparisons use fjall `SyncAll`, redb `Immediate`, SQLite WAL with `synchronous=FULL`, and normal durable LMDB commits.

## License

Apache-2.0. See `LICENSE`.
