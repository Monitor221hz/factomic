# factomic-derive

Procedural macros for Factomic's typed entity schemas and handles.

## Usage

```rust
use std::sync::Arc;

#[derive(factomic::Entity)]
struct Person {
    name: String,
    #[factomic(indexed)]
    age: i64,
    manager: Option<Arc<Person>>,
}
```

The derive feature is enabled by default. It generates the schema, `Entity` and `Schema` implementations, typed read/write handle traits, and query methods.

## Field forms

- `String`, `bool`, `i64`, `f64`, `i8`/`i16`/`i32`, `u8`/`u16`/`u32`, `EntityId`, and `jiff::Timestamp` map to `One`.
- `Vec<T>` maps to `List`; `HashSet<T>` maps to `Many`; `HashMap<K, V>` maps to `Map`.
- `Option<T>` and `HashMap<K, Option<V>>` use absence and key removal for optional values.
- `Arc<T>` and direct custom entity types map to references.
- `#[factomic(indexed)]` enables exact indexed finders; reference fields also support reverse-reference finders.
- `#[factomic(namespace = "...")]`, `#[factomic(name = "...")]`, `#[factomic(component)]`, `#[factomic(value)]`, and `#[factomic(ref)]` customize schema and type behavior.

See `crates/factomic/README.md` for the runtime API and `crates/factomic-example/src/main.rs` for a complete example.

## License

Apache-2.0. See `LICENSE`.
