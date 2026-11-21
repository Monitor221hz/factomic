use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::measurement::WallTime;
use criterion::{
    BatchSize, BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main,
};
use factomic::{
    AttributeDef, AttributeId, CollectionKind, Db, EntityId, Result, Store, TxnId, Type,
};
use tempfile::TempDir;

fn def(name: &str, value_type: Type, kind: CollectionKind) -> AttributeDef {
    AttributeDef {
        name: name.into(),
        value_type,
        kind,
        key_type: None,
        indexed: false,
        is_component: false,
    }
}

fn indexed_def(name: &str, value_type: Type) -> AttributeDef {
    AttributeDef {
        indexed: true,
        ..def(name, value_type, CollectionKind::One)
    }
}

fn new_store() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (dir, store)
}

fn expensive(group: &mut BenchmarkGroup<'_, WallTime>) {
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
}

fn schema_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema");
    expensive(&mut group);
    group.bench_function("declare_single", |b| {
        b.iter_batched_ref(
            || {
                let (dir, store) = new_store();
                (
                    dir,
                    store,
                    Some(def("bench/name", Type::String, CollectionKind::One)),
                )
            },
            |(_dir, store, def)| black_box(store.declare_attribute(def.take().unwrap()).unwrap()),
            BatchSize::SmallInput,
        )
    });
    group.throughput(Throughput::Elements(16));
    group.bench_function("declare_batch_16", |b| {
        b.iter_batched_ref(
            || {
                let (dir, store) = new_store();
                let defs: [AttributeDef; 16] = std::array::from_fn(|index| {
                    def(
                        &format!("bench/attr-{index}"),
                        Type::String,
                        CollectionKind::One,
                    )
                });
                (dir, store, Some(defs))
            },
            |(_dir, store, defs)| {
                black_box(store.declare_attributes(defs.take().unwrap()).unwrap())
            },
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

fn entity_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("entity");
    expensive(&mut group);
    group.bench_function("new_entity", |b| {
        b.iter_batched_ref(
            new_store,
            |(_dir, store)| black_box(store.new_entity().unwrap()),
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

type WriteInput = (TempDir, Store, EntityId, AttributeId, Option<Vec<String>>);

fn write_input(size: usize) -> WriteInput {
    let (dir, store) = new_store();
    let attr = store
        .declare_attribute(def("bench/value", Type::String, CollectionKind::Many))
        .unwrap();
    let entity = store.new_entity().unwrap();
    let values = (0..size)
        .map(|index| format!("value-{index}"))
        .collect::<Vec<_>>();
    (dir, store, entity, attr, Some(values))
}

fn bench_assert(group: &mut BenchmarkGroup<'_, WallTime>, name: &'static str, size: usize) {
    group.throughput(Throughput::Elements(size as u64));
    group.bench_function(name, |b| {
        b.iter_batched_ref(
            || write_input(size),
            |(_dir, store, entity, attr, values)| {
                let mut tx = store.transact();
                for value in values.take().unwrap() {
                    tx.assert(*entity, *attr, &value);
                }
                let txn = tx.commit().unwrap();
                let persisted = store.db().entity_values::<String>(*entity, *attr).count();
                assert_eq!(persisted, size);
                black_box(txn)
            },
            BatchSize::SmallInput,
        )
    });
}

fn transaction_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("transactions");
    expensive(&mut group);
    bench_assert(&mut group, "assert_1", 1);
    bench_assert(&mut group, "assert_batch_10", 10);
    bench_assert(&mut group, "assert_batch_100", 100);
    group.throughput(Throughput::Elements(1));
    group.bench_function("overwrite_indexed", |b| {
        b.iter_batched_ref(
            || {
                let (dir, store) = new_store();
                let age = store
                    .declare_attribute(indexed_def("bench/age", Type::Long))
                    .unwrap();
                let entity = store.new_entity().unwrap();
                store.assert(entity, age, &30_i64).unwrap();
                (dir, store, entity, age)
            },
            |(_dir, store, entity, age)| black_box(store.assert(*entity, *age, &31_i64).unwrap()),
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

struct QueryFixture {
    _dir: TempDir,
    _store: Store,
    db: Arc<Db>,
    historical: Arc<Db>,
    entity: EntityId,
    age: AttributeId,
    friend: AttributeId,
    tags: AttributeId,
    items: AttributeId,
    scores: AttributeId,
    target: EntityId,
    latest_txn: TxnId,
}

fn query_fixture() -> QueryFixture {
    let (dir, store) = new_store();
    let entity = store.new_entity().unwrap();
    let target = store.new_entity().unwrap();
    let [age, friend, tags, items, scores] = store
        .declare_attributes([
            indexed_def("bench/age", Type::Long),
            def("bench/friend", Type::Ref, CollectionKind::One),
            def("bench/tags", Type::String, CollectionKind::Many),
            def("bench/items", Type::String, CollectionKind::List),
            AttributeDef {
                key_type: Some(Type::String),
                ..def("bench/scores", Type::Long, CollectionKind::Map)
            },
        ])
        .unwrap();
    let t1 = store
        .transact()
        .assert(entity, age, &30_i64)
        .assert(entity, friend, &target)
        .assert(entity, tags, &"x")
        .assert(entity, tags, &"y")
        .push(entity, items, &"a")
        .push(entity, items, &"c")
        .put(entity, scores, "math".into(), &90_i64)
        .commit()
        .unwrap()
        .unwrap();
    let anchor = store.db().slot_at(entity, items, 1).unwrap().unwrap();
    let latest_txn = store
        .transact()
        .assert(entity, age, &31_i64)
        .put(entity, scores, "math".into(), &95_i64)
        .insert_before(entity, items, anchor, &"b")
        .commit()
        .unwrap()
        .unwrap();
    let db = store.db();
    let historical = db.at(t1);
    QueryFixture {
        _dir: dir,
        _store: store,
        db,
        historical,
        entity,
        age,
        friend,
        tags,
        items,
        scores,
        target,
        latest_txn,
    }
}

struct LargeIndexedFixture {
    _dir: TempDir,
    _store: Store,
    db: Arc<Db>,
    age: AttributeId,
}

fn large_indexed_fixture() -> LargeIndexedFixture {
    let (dir, store) = new_store();
    let age = store
        .declare_attribute(indexed_def("bench/age", Type::Long))
        .unwrap();
    for _ in 0..10 {
        let mut tx = store.transact();
        for _ in 0..100 {
            let entity = store.new_entity().unwrap();
            tx.assert(entity, age, &31_i64);
        }
        tx.commit().unwrap();
    }
    let db = store.db();
    LargeIndexedFixture {
        _dir: dir,
        _store: store,
        db,
        age,
    }
}

struct DeepHistoryFixture {
    _dir: TempDir,
    _store: Store,
    historical: Arc<Db>,
    entity: EntityId,
    age: AttributeId,
}

fn deep_history_fixture() -> DeepHistoryFixture {
    let (dir, store) = new_store();
    let age = store
        .declare_attribute(indexed_def("bench/age", Type::Long))
        .unwrap();
    let entity = store.new_entity().unwrap();
    let first = store.assert(entity, age, &0_i64).unwrap().unwrap();
    for version in 1..=100_i64 {
        store.assert(entity, age, &version).unwrap();
    }
    let historical = store.db().at(first);
    DeepHistoryFixture {
        _dir: dir,
        _store: store,
        historical,
        entity,
        age,
    }
}

fn query_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("queries");
    let fixture = query_fixture();
    group.bench_function("entity_value", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .db
                    .entity_value::<i64>(fixture.entity, fixture.age)
                    .unwrap(),
            )
        })
    });
    group.bench_function("entity_values_many", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .db
                    .entity_values::<String>(fixture.entity, fixture.tags)
                    .collect::<Result<Vec<_>>>()
                    .unwrap(),
            )
        })
    });
    group.bench_function("map_keyvalue", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .db
                    .entity_keyvalue::<i64>(fixture.entity, fixture.scores, "math")
                    .unwrap(),
            )
        })
    });
    group.bench_function("indexed_lookup", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .db
                    .entities_with_value(fixture.age, &31_i64)
                    .unwrap(),
            )
        })
    });
    group.bench_function("reverse_reference", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .db
                    .referring_entities(fixture.target, fixture.friend)
                    .unwrap(),
            )
        })
    });
    group.bench_function("as_of_entity", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .historical
                    .entity_value::<i64>(fixture.entity, fixture.age)
                    .unwrap(),
            )
        })
    });
    group.bench_function("tx_factoms", |b| {
        b.iter(|| black_box(fixture.db.tx_factoms(fixture.latest_txn).unwrap()))
    });
    group.bench_function("entity_history", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .db
                    .entity_history(fixture.entity)
                    .collect::<Result<Vec<_>>>()
                    .unwrap(),
            )
        })
    });
    let large_indexed = large_indexed_fixture();
    group.bench_function("indexed_lookup_1000", |b| {
        b.iter(|| {
            let entities = large_indexed
                .db
                .entities_with_value(large_indexed.age, &31_i64)
                .unwrap();
            assert_eq!(entities.len(), 1000);
            black_box(entities)
        })
    });
    let deep_history = deep_history_fixture();
    group.bench_function("as_of_history_100", |b| {
        b.iter(|| {
            black_box(
                deep_history
                    .historical
                    .entity_value::<i64>(deep_history.entity, deep_history.age)
                    .unwrap(),
            )
        })
    });
    group.finish();
}

type RecordInput = (TempDir, Store, [AttributeId; 5], EntityId);

fn record_input() -> RecordInput {
    let (dir, store) = new_store();
    let target = store.new_entity().unwrap();
    let attributes = store
        .declare_attributes([
            indexed_def("bench/age", Type::Long),
            def("bench/friend", Type::Ref, CollectionKind::One),
            def("bench/tags", Type::String, CollectionKind::Many),
            def("bench/items", Type::String, CollectionKind::List),
            AttributeDef {
                key_type: Some(Type::String),
                ..def("bench/scores", Type::Long, CollectionKind::Map)
            },
        ])
        .unwrap();
    (dir, store, attributes, target)
}

fn create_records(
    store: &Store,
    [age, friend, tags, items, scores]: [AttributeId; 5],
    target: EntityId,
    count: usize,
) {
    let mut tx = store.transact();
    for _ in 0..count {
        let entity = store.new_entity().unwrap();
        tx.assert(entity, age, &30_i64)
            .assert(entity, friend, &target)
            .assert(entity, tags, &"x")
            .assert(entity, tags, &"y")
            .push(entity, items, &"a")
            .push(entity, items, &"c")
            .put(entity, scores, "math".into(), &90_i64);
    }
    black_box(tx.commit().unwrap());
}

fn record_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("records");
    expensive(&mut group);
    group.bench_function("create", |b| {
        b.iter_batched_ref(
            record_input,
            |(_dir, store, attributes, target)| create_records(store, *attributes, *target, 1),
            BatchSize::SmallInput,
        )
    });
    group.throughput(Throughput::Elements(10));
    group.bench_function("create_batch_10", |b| {
        b.iter_batched_ref(
            record_input,
            |(_dir, store, attributes, target)| create_records(store, *attributes, *target, 10),
            BatchSize::SmallInput,
        )
    });
    let fixture = query_fixture();
    group.throughput(Throughput::Elements(1));
    group.bench_function("read", |b| {
        b.iter(|| {
            let age = fixture
                .db
                .entity_value::<i64>(fixture.entity, fixture.age)
                .unwrap();
            let friend = fixture
                .db
                .entity_value::<EntityId>(fixture.entity, fixture.friend)
                .unwrap();
            let tags = fixture
                .db
                .entity_values::<String>(fixture.entity, fixture.tags)
                .collect::<Result<Vec<_>>>()
                .unwrap();
            let items = fixture
                .db
                .entity_values::<String>(fixture.entity, fixture.items)
                .collect::<Result<Vec<_>>>()
                .unwrap();
            let score = fixture
                .db
                .entity_keyvalue::<i64>(fixture.entity, fixture.scores, "math")
                .unwrap();
            black_box((age, friend, tags, items, score))
        })
    });
    group.bench_function("update_indexed", |b| {
        b.iter_batched_ref(
            query_fixture,
            |fixture| {
                black_box(
                    fixture
                        ._store
                        .assert(fixture.entity, fixture.age, &32_i64)
                        .unwrap(),
                )
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("append_list", |b| {
        b.iter_batched_ref(
            query_fixture,
            |fixture| {
                black_box(
                    fixture
                        ._store
                        .push(fixture.entity, fixture.items, &"d")
                        .unwrap(),
                )
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("update_map", |b| {
        b.iter_batched_ref(
            query_fixture,
            |fixture| {
                black_box(
                    fixture
                        ._store
                        .put(fixture.entity, fixture.scores, "math", &96_i64)
                        .unwrap(),
                )
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("retract", |b| {
        b.iter_batched_ref(
            query_fixture,
            |fixture| black_box(fixture._store.retract_entity(fixture.entity).unwrap()),
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

fn lifecycle_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("lifecycle");
    expensive(&mut group);
    group.bench_function("open_fresh", |b| {
        b.iter_batched_ref(
            || (TempDir::new().unwrap(), None),
            |(_dir, store)| {
                let opened = Store::open(_dir.path()).unwrap();
                let txn = opened.db().txn();
                store.replace(opened);
                black_box(txn)
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("reopen_populated", |b| {
        b.iter_batched_ref(
            || {
                let QueryFixture { _dir, .. } = query_fixture();
                (_dir, None)
            },
            |(_dir, store)| {
                let opened = Store::open(_dir.path()).unwrap();
                let txn = opened.db().txn();
                store.replace(opened);
                black_box(txn)
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("reopen_populated_1000", |b| {
        b.iter_batched_ref(
            || {
                let LargeIndexedFixture { _dir, .. } = large_indexed_fixture();
                (_dir, None)
            },
            |(_dir, store)| {
                let opened = Store::open(_dir.path()).unwrap();
                let txn = opened.db().txn();
                store.replace(opened);
                black_box(txn)
            },
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

criterion_group!(
    benches,
    schema_benchmarks,
    entity_benchmarks,
    transaction_benchmarks,
    query_benchmarks,
    record_benchmarks,
    lifecycle_benchmarks
);
criterion_main!(benches);
