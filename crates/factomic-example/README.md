# factomic-example

A runnable consumer of the Factomic library.

## Run

```text
cargo run -p factomic-example
```

The example creates a temporary database, derives `Car` and `Person` schemas, commits typed mutations, performs indexed and reverse-reference queries, reads historical snapshots, retracts an entity, and verifies persistence after reopening.

## License

Apache-2.0. See `LICENSE`.
