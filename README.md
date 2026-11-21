# Factomic

Factomic is a unitemporal, embedded fact database backed by [fjall](https://crates.io/crates/fjall) born out of a need for a Datomic-like model and all its audit-trail advantages, while fully supporting ordered list and map operations.

It is strongly derived from the [Datomic](https://www.datomic.com/rationale.html) information model, with the addition of Slots for ordinal lists and map keys. A slot uses relative fractional indexing rather than absolute indexing to ensure that facts remain immutable and independantly valid.

Factomic is **tentatively** ACID-compliant (real power-loss and mid-journal crashes have not been tested). It lacks any real-world usage, thus should be considered pre-production code and prone to **breaking changes**.

## Workspace

- `factomic`: runtime database, snapshots, entity handles, transactions, and queries.
- `factomic-derive`: `#[derive(factomic::Entity]` schema and typed API generation.
- `factomic-example`: runnable end-to-end example.

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

## ID allocation

Entity and Transaction IDs increase monotonically but are not guaranteed to be contiguous. Allocating an entity consumes its ID even if no facts are asserted.

Successful mutations provide the durability guarantee via `fsync`.

## Verification

```text
cargo test --workspace --all-features
cargo run -p factomic-example
```
