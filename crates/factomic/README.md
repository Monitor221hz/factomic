<p align="center">
  <img src="/logo.png" height="200">
</p>
<p align="center">
  <small>logo by <a href="https://lorcblog.blogspot.com/">Lorc</a> under <a href="https://creativecommons.org/licenses/by/3.0/">CC BY 3.0</a>, modified.</small>
</p>
<p align="center">
  <a href="https://github.com/Monitor221hz/factomic/actions/workflows/ci.yml">
    <img src="https://github.com/Monitor221hz/factomic/actions/workflows/ci.yml/badge.svg" alt="CI" />
  </a>
  <a href="https://docs.rs/factomic">
    <img src="https://img.shields.io/docsrs/factomic?color=green" alt="docs.rs" />
  </a>
  <a href="https://crates.io/crates/factomic">
    <img src="https://img.shields.io/crates/v/factomic?color=blue" alt="Crates.io" />
  </a>
  <a href="https://deps.rs/repo/github/Monitor221hz/factomic">
    <img src="https://deps.rs/repo/github/Monitor221hz/factomic/status.svg" alt="dependency status" />
  </a>
</p>

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

## Primitive support

In addition to the built-in numeric, string, entity-reference, and Jiff timestamp types, Factomic supports `f32`, `char`, nonzero integers representable by `i64`, standard IP/socket addresses, `Duration`, and `SystemTime`. Durations and system times use lossless binary representations.

Additional standard-library and external value types use `#[factomic(value)]` on entity fields.

For Primitive downstream types, use `#[factomic::primitive(string)]`, `long`, `timestamp`, or `bytes` on a non-generic local type with the corresponding conversion traits.

The `impl_string_primitive!`, `impl_long_primitive!`, `impl_timestamp_primitive!`, and `impl_bytes_primitive!` can be used for more non conversion trait conforming types.

External integrations are opt-in through Cargo features:

| Feature         | Types                                                                     | Stored representation                                      |
| --------------- | ------------------------------------------------------------------------- | ---------------------------------------------------------- |
| `bigdecimal`    | `bigdecimal::BigDecimal`                                                  | String                                                     |
| `bytes`         | `bytes::Bytes`                                                            | Bytes                                                      |
| `camino`        | `camino::Utf8PathBuf`                                                     | String                                                     |
| `chrono`        | `chrono::DateTime<Utc>`, `DateTime<FixedOffset>`, naive date/time types   | Timestamp for UTC; strings otherwise                       |
| `http`          | `http::Uri`                                                               | String                                                     |
| `ipnet`         | `ipnet::{IpNet, Ipv4Net, Ipv6Net}`                                        | String                                                     |
| `jiff-extended` | Jiff civil date/time types, `SignedDuration`, `Span`, and `ZonedDateTime` | String                                                     |
| `num-bigint`    | `num_bigint::{BigInt, BigUint}`                                           | String                                                     |
| `rust_decimal`  | `rust_decimal::Decimal`                                                   | String                                                     |
| `semver`        | `semver::Version`                                                         | String                                                     |
| `serde-json`    | `serde_json::Value`                                                       | String                                                     |
| `time`          | `time::OffsetDateTime`, `Date`, `PrimitiveDateTime`, and `Time`           | Timestamp for `OffsetDateTime`; ISO 8601 strings otherwise |
| `ulid`          | `ulid::Ulid`                                                              | String                                                     |
| `url`           | `url::Url`                                                                | String                                                     |
| `uuid`          | `uuid::Uuid`                                                              | Bytes                                                      |
| `versions`      | `versions::{SemVer, Version, Mess, Versioning, Requirement}`              | String                                                     |

## ID allocation

Entity and Transaction IDs increase monotonically but are not guaranteed to be contiguous. Allocating an entity consumes its ID even if no facts are asserted.

Successful mutations provide the durability guarantee via `fsync`.

## Verification

```text
cargo test --workspace --all-features
cargo run -p factomic-example
```

## Benchmarking

Run the Criterion benchmarks with `cargo bench -p factomic-benches`, or compile them without running using `cargo bench -p factomic-benches --no-run`.

Reports are written to `target/criterion/report/index.html`.

Save and compare baselines with:

```text
cargo bench -p factomic-benches -- --save-baseline main
cargo bench -p factomic-benches -- --baseline main
```

### Latest measured results

Specs: AMD Ryzen 7 9800X3D, 94 GiB RAM, Windows 11 Home 26200, Samsung SSD 990 PRO 1TB.

Versions:

- Factomic 0.3.0
- fjall 3.1.13
- redb 5.0.0
- rusqlite 0.43.0
- heed 0.23.1
- native_db 0.8.2

All writes used strict durability. Reads used fresh snapshots/transactions and owned `String` results.

It should be noted that Factomic writes also include indexes, history, and transaction logging.

#### Comparison

| Benchmark               |        Factomic |           fjall |            redb |        rusqlite |             heed |       native_db |
| ----------------------- | --------------: | --------------: | --------------: | --------------: | ---------------: | --------------: |
| `commit_1`              |        1.482 ms |        1.496 ms |        1.373 ms |        1.420 ms |         248.1 µs |        1.514 ms |
| `commit_100`            |        1.577 ms |        1.769 ms |        2.872 ms |        2.849 ms |         273.2 µs |        3.058 ms |
| `point_read`            |          785 ns |          314 ns |          755 ns |        2.565 µs |           149 ns |          824 ns |
| `range_read_100`        |        18.13 µs |        14.51 µs |         8.78 µs |         8.70 µs |          5.03 µs |        15.41 µs |
| `commit_100` throughput | 63.4K entries/s | 56.5K entries/s | 34.8K entries/s | 35.1K entries/s | 366.1K entries/s | 32.7K entries/s |

#### Workloads

| Operation                                 |                                     Result |
| ----------------------------------------- | -----------------------------------------: |
| Create one complete record                |                                   2.819 ms |
| Create 10 records in one fact transaction |              15.58 ms total, 642 records/s |
| Read a complete record                    |      4.39 µs, approximately 228K records/s |
| Update indexed field                      |                                   1.348 ms |
| Append list item                          |                                   1.391 ms |
| Update map entry                          |                                   1.607 ms |
| Retract entity                            |                                   1.251 ms |
| Revert database to transaction            |                                   1.481 ms |
| Revert database to time                   |                                   1.624 ms |
| Undo transaction                          |                                   1.466 ms |
| Undo latest entity transaction            |                                   1.404 ms |
| Assert one fact                           |                                698 facts/s |
| Assert 10 facts in one transaction        |                              7.26K facts/s |
| Assert 100 facts in one transaction       |                              60.9K facts/s |
| Overwrite indexed fact                    |                           705 overwrites/s |
| Declare one attribute                     |                                   1.566 ms |
| Declare 16 attributes in one transaction  | 1.438 ms, approximately 11.1K attributes/s |
| Allocate an entity                        |                                   1.403 ms |
| Open a fresh store                        |                                  234.68 ms |
| Reopen the small populated store          |                                    9.97 ms |
| Reopen the 1,000-fact store               |                                   21.68 ms |

## Database comparisons

Run `cargo bench -p factomic-benches --bench compare` for strict-durability comparisons.

Comparisons use fjall `SyncAll`, redb/native_db `Immediate`, SQLite WAL with `synchronous=FULL`, and normal durable LMDB commits.

## CI and publishing

Configure the `CARGO_REGISTRY_TOKEN` repository secret with a crates.io API token.

## License

Apache-2.0. See `LICENSE`.
