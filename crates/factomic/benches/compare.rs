use std::hint::black_box;
use std::sync::LazyLock;
use std::time::Duration;

use criterion::measurement::WallTime;
use criterion::{
    BatchSize, BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main,
};
use factomic::{AttributeDef, AttributeId, CollectionKind, EntityId, Store, Type};
use fjall::{Database, Keyspace, KeyspaceCreateOptions, OwnedWriteBatch, PersistMode, Readable};
use heed::byteorder::BigEndian;
use heed::types::{Str, U64};
use heed::{CommitOrAbort, Database as HeedDatabase, Env, EnvOpenOptions};
use native_db::{Builder as NativeBuilder, Database as NativeDatabase, Models, ToKey, native_db};
use native_model::{Model, native_model};
use redb::{
    Database as RedbDatabase, Durability, ReadableDatabase, ReadableTable, TableDefinition,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

const REDB_TABLE: TableDefinition<u64, &str> = TableDefinition::new("kv");
const READ_ITEMS: u64 = 1_000;
const RANGE_END: u64 = 100;

#[derive(Serialize, Deserialize, Debug)]
#[native_model(id = 1, version = 1)]
#[native_db]
struct NativeItem {
    #[primary_key]
    id: u64,
    value: String,
}

static NATIVE_MODELS: LazyLock<Models> = LazyLock::new(|| {
    let mut models = Models::new();
    models.define::<NativeItem>().unwrap();
    models
});

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

fn value(index: u64) -> String {
    format!("value-{index:04}")
}

fn values(size: usize) -> Vec<String> {
    (0..size as u64).map(value).collect()
}

fn factomic_store() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (dir, store)
}

fn configure_writes(group: &mut BenchmarkGroup<'_, WallTime>) {
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
}

fn configure_reads(group: &mut BenchmarkGroup<'_, WallTime>) {
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
}

fn expect_point(result: Option<String>) -> Option<String> {
    assert_eq!(result.as_deref(), Some("value-0500"));
    result
}

fn expect_range(result: Vec<String>) -> Vec<String> {
    assert_eq!(result.len(), RANGE_END as usize);
    assert_eq!(result.first().map(String::as_str), Some("value-0000"));
    assert_eq!(result.last().map(String::as_str), Some("value-0099"));
    result
}

type FactomicCommitInput = (TempDir, Store, EntityId, AttributeId, Option<Vec<String>>);

fn factomic_commit_input(size: usize) -> FactomicCommitInput {
    let (dir, store) = factomic_store();
    let attr = store
        .declare_attribute(def("bench/value", Type::String, CollectionKind::Many))
        .unwrap();
    let entity = store.new_entity().unwrap();
    (dir, store, entity, attr, Some(values(size)))
}

fn factomic_commit() -> impl FnMut(&mut FactomicCommitInput) {
    move |(_dir, store, entity, attr, values): &mut FactomicCommitInput| {
        let mut tx = store.transact();
        for value in values.take().unwrap() {
            tx.assert(*entity, *attr, &value);
        }
        tx.commit().unwrap();
        black_box(())
    }
}

fn fjall_database() -> (TempDir, Database, Keyspace) {
    let dir = TempDir::new().unwrap();
    let db = Database::builder(dir.path()).open().unwrap();
    let keyspace = db.keyspace("kv", KeyspaceCreateOptions::default).unwrap();
    (dir, db, keyspace)
}

type FjallCommitInput = (TempDir, Database, Keyspace, Option<Vec<String>>);

fn fjall_commit_input(size: usize) -> FjallCommitInput {
    let (dir, db, keyspace) = fjall_database();
    (dir, db, keyspace, Some(values(size)))
}

fn fjall_commit(size: usize) -> impl FnMut(&mut FjallCommitInput) {
    move |(_dir, db, keyspace, values): &mut FjallCommitInput| {
        let mut batch =
            OwnedWriteBatch::with_capacity(db.clone(), size).durability(Some(PersistMode::SyncAll));
        for (index, value) in values.take().unwrap().into_iter().enumerate() {
            batch.insert(
                keyspace,
                (index as u64).to_be_bytes().to_vec(),
                value.into_bytes(),
            );
        }
        batch.commit().unwrap();
        black_box(())
    }
}

fn redb_database() -> (TempDir, RedbDatabase) {
    let dir = TempDir::new().unwrap();
    let db = RedbDatabase::create(dir.path().join("db.redb")).unwrap();
    (dir, db)
}

type RedbCommitInput = (TempDir, RedbDatabase, Option<Vec<String>>);

fn redb_commit_input(size: usize) -> RedbCommitInput {
    let (dir, db) = redb_database();
    (dir, db, Some(values(size)))
}

fn redb_commit() -> impl FnMut(&mut RedbCommitInput) {
    move |(_dir, db, values): &mut RedbCommitInput| {
        let mut txn = db.begin_write().unwrap();
        txn.set_durability(Durability::Immediate).unwrap();
        {
            let mut table = txn.open_table(REDB_TABLE).unwrap();
            for (index, value) in values.take().unwrap().into_iter().enumerate() {
                table.insert(index as u64, value.as_str()).unwrap();
            }
        }
        txn.commit().unwrap();
        black_box(())
    }
}

fn sqlite_connection() -> (TempDir, Connection) {
    let dir = TempDir::new().unwrap();
    let conn = Connection::open(dir.path().join("db.sqlite")).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         CREATE TABLE kv (k INTEGER PRIMARY KEY, v TEXT NOT NULL);",
    )
    .unwrap();
    (dir, conn)
}

type SqliteCommitInput = (TempDir, Connection, Option<Vec<String>>);

fn sqlite_commit_input(size: usize) -> SqliteCommitInput {
    let (dir, conn) = sqlite_connection();
    (dir, conn, Some(values(size)))
}

fn sqlite_commit() -> impl FnMut(&mut SqliteCommitInput) {
    move |(_dir, conn, values): &mut SqliteCommitInput| {
        let txn = conn.transaction().unwrap();
        for (index, value) in values.take().unwrap().into_iter().enumerate() {
            txn.execute(
                "INSERT INTO kv (k, v) VALUES (?1, ?2)",
                params![index as i64, value],
            )
            .unwrap();
        }
        txn.commit().unwrap();
        black_box(())
    }
}

type HeedDb = HeedDatabase<'static, U64<BigEndian>, Str>;

fn heed_database() -> (TempDir, Env, HeedDb) {
    let dir = TempDir::new().unwrap();
    let env = unsafe {
        EnvOpenOptions::new()
            .max_dbs(1)
            .map_size(16 * 1024 * 1024)
            .open(dir.path())
            .unwrap()
    };
    let txn = env.write_txn().unwrap();
    let db = env
        .create_databases_and_commit(txn, |wdo, txn| {
            wdo.create_database::<U64<BigEndian>, Str>(txn, None)
                .map(CommitOrAbort::<_, ()>::Commit)
        })
        .unwrap()
        .unwrap_commit();
    (dir, env, db)
}

type HeedCommitInput = (TempDir, Env, HeedDb, Option<Vec<String>>);

fn heed_commit_input(size: usize) -> HeedCommitInput {
    let (dir, env, db) = heed_database();
    (dir, env, db, Some(values(size)))
}

fn heed_commit() -> impl FnMut(&mut HeedCommitInput) {
    move |(_dir, env, db, values): &mut HeedCommitInput| {
        let mut txn = env.write_txn().unwrap();
        for (index, value) in values.take().unwrap().into_iter().enumerate() {
            db.put(&mut txn, &(index as u64), value.as_str()).unwrap();
        }
        txn.commit().unwrap();
        black_box(())
    }
}

fn native_database() -> (TempDir, NativeDatabase<'static>) {
    let dir = TempDir::new().unwrap();
    let db = NativeBuilder::new()
        .create(&NATIVE_MODELS, dir.path().join("db.redb"))
        .unwrap();
    (dir, db)
}

type NativeCommitInput = (TempDir, NativeDatabase<'static>, Option<Vec<NativeItem>>);

fn native_commit_input(size: usize) -> NativeCommitInput {
    let (dir, db) = native_database();
    let items = (0..size as u64)
        .map(|id| NativeItem {
            id,
            value: value(id),
        })
        .collect();
    (dir, db, Some(items))
}

fn native_commit() -> impl FnMut(&mut NativeCommitInput) {
    move |(_dir, db, items): &mut NativeCommitInput| {
        let txn = db.rw_transaction().unwrap();
        for item in items.take().unwrap() {
            txn.insert(item).unwrap();
        }
        txn.commit().unwrap();
        black_box(())
    }
}

fn commit_benchmarks(c: &mut Criterion, name: &'static str, size: usize) {
    let mut group = c.benchmark_group(name);
    configure_writes(&mut group);
    group.throughput(Throughput::Elements(size as u64));
    group.bench_function("factomic", |b| {
        b.iter_batched_ref(
            || factomic_commit_input(size),
            factomic_commit(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("fjall", |b| {
        b.iter_batched_ref(
            || fjall_commit_input(size),
            fjall_commit(size),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("redb", |b| {
        b.iter_batched_ref(
            || redb_commit_input(size),
            redb_commit(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("rusqlite", |b| {
        b.iter_batched_ref(
            || sqlite_commit_input(size),
            sqlite_commit(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("heed", |b| {
        b.iter_batched_ref(
            || heed_commit_input(size),
            heed_commit(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("native_db", |b| {
        b.iter_batched_ref(
            || native_commit_input(size),
            native_commit(),
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

struct FactomicRead {
    _dir: TempDir,
    store: Store,
    entities: Vec<EntityId>,
    point: AttributeId,
    range: AttributeId,
    range_entity: EntityId,
}

fn factomic_read() -> FactomicRead {
    let (dir, store) = factomic_store();
    let point = store
        .declare_attribute(def("bench/point", Type::String, CollectionKind::One))
        .unwrap();
    let range = store
        .declare_attribute(def("bench/range", Type::String, CollectionKind::Many))
        .unwrap();
    let range_entity = store.new_entity().unwrap();
    let entities = (0..READ_ITEMS)
        .map(|_| store.new_entity().unwrap())
        .collect::<Vec<_>>();
    let mut tx = store.transact();
    for (index, entity) in entities.iter().enumerate() {
        tx.assert(*entity, point, &value(index as u64));
    }
    for index in 0..READ_ITEMS {
        tx.assert(range_entity, range, &value(index));
    }
    tx.commit().unwrap();
    FactomicRead {
        _dir: dir,
        store,
        entities,
        point,
        range,
        range_entity,
    }
}

struct FjallRead {
    _dir: TempDir,
    db: Database,
    keyspace: Keyspace,
}

fn fjall_read() -> FjallRead {
    let (_dir, db, keyspace) = fjall_database();
    let mut batch = OwnedWriteBatch::with_capacity(db.clone(), READ_ITEMS as usize)
        .durability(Some(PersistMode::SyncAll));
    for index in 0..READ_ITEMS {
        batch.insert(
            &keyspace,
            index.to_be_bytes().to_vec(),
            value(index).into_bytes(),
        );
    }
    batch.commit().unwrap();
    FjallRead { _dir, db, keyspace }
}

fn redb_read() -> (TempDir, RedbDatabase) {
    let (dir, db) = redb_database();
    let mut txn = db.begin_write().unwrap();
    txn.set_durability(Durability::Immediate).unwrap();
    {
        let mut table = txn.open_table(REDB_TABLE).unwrap();
        for index in 0..READ_ITEMS {
            table.insert(index, value(index).as_str()).unwrap();
        }
    }
    txn.commit().unwrap();
    (dir, db)
}

fn sqlite_read() -> (TempDir, Connection) {
    let (dir, mut conn) = sqlite_connection();
    let txn = conn.transaction().unwrap();
    for index in 0..READ_ITEMS {
        txn.execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2)",
            params![index as i64, value(index)],
        )
        .unwrap();
    }
    txn.commit().unwrap();
    (dir, conn)
}

fn heed_read() -> (TempDir, Env, HeedDb) {
    let (dir, env, db) = heed_database();
    let mut txn = env.write_txn().unwrap();
    for index in 0..READ_ITEMS {
        db.put(&mut txn, &index, value(index).as_str()).unwrap();
    }
    txn.commit().unwrap();
    (dir, env, db)
}

fn native_read() -> (TempDir, NativeDatabase<'static>) {
    let (dir, db) = native_database();
    let txn = db.rw_transaction().unwrap();
    for id in 0..READ_ITEMS {
        txn.insert(NativeItem {
            id,
            value: value(id),
        })
        .unwrap();
    }
    txn.commit().unwrap();
    (dir, db)
}

fn point_read_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("point_read");
    configure_reads(&mut group);
    let factomic = factomic_read();
    group.bench_function("factomic", |b| {
        b.iter(|| {
            let result = factomic
                .store
                .db()
                .entity_value::<String>(factomic.entities[500], factomic.point)
                .unwrap();
            black_box(expect_point(result))
        })
    });
    let fjall = fjall_read();
    group.bench_function("fjall", |b| {
        b.iter(|| {
            let result = fjall
                .db
                .snapshot()
                .get(&fjall.keyspace, 500_u64.to_be_bytes())
                .unwrap()
                .map(|value| String::from_utf8(value.as_ref().to_vec()).unwrap());
            black_box(expect_point(result))
        })
    });
    let (_dir, redb) = redb_read();
    group.bench_function("redb", |b| {
        b.iter(|| {
            let txn = redb.begin_read().unwrap();
            let table = txn.open_table(REDB_TABLE).unwrap();
            let result = table
                .get(500_u64)
                .unwrap()
                .map(|value| value.value().to_string());
            black_box(expect_point(result))
        })
    });
    let (_dir, conn) = sqlite_read();
    group.bench_function("rusqlite", |b| {
        b.iter(|| {
            let txn = conn.unchecked_transaction().unwrap();
            let mut statement = txn.prepare_cached("SELECT v FROM kv WHERE k = ?1").unwrap();
            let result = statement
                .query_row([500_i64], |row| row.get::<_, String>(0))
                .optional()
                .unwrap();
            black_box(expect_point(result))
        })
    });
    let (_dir, env, db) = heed_read();
    group.bench_function("heed", |b| {
        b.iter(|| {
            let txn = env.read_txn().unwrap();
            let result = db.get(&txn, &500_u64).unwrap().map(str::to_string);
            black_box(expect_point(result))
        })
    });
    let (_dir, native) = native_read();
    group.bench_function("native_db", |b| {
        b.iter(|| {
            let txn = native.r_transaction().unwrap();
            let result = txn
                .get()
                .primary(500_u64)
                .unwrap()
                .map(|item: NativeItem| item.value);
            black_box(expect_point(result))
        })
    });
    group.finish();
}

fn range_read_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_read_100");
    configure_reads(&mut group);
    group.throughput(Throughput::Elements(RANGE_END));
    let factomic = factomic_read();
    group.bench_function("factomic", |b| {
        b.iter(|| {
            let result = factomic
                .store
                .db()
                .entity_values::<String>(factomic.range_entity, factomic.range)
                .take(RANGE_END as usize)
                .collect::<factomic::Result<Vec<_>>>()
                .unwrap();
            black_box(expect_range(result))
        })
    });
    let fjall = fjall_read();
    group.bench_function("fjall", |b| {
        b.iter(|| {
            let result = fjall
                .db
                .snapshot()
                .range(
                    &fjall.keyspace,
                    0_u64.to_be_bytes().to_vec()..RANGE_END.to_be_bytes().to_vec(),
                )
                .map(|guard| {
                    let (_, value) = guard.into_inner().unwrap();
                    String::from_utf8(value.as_ref().to_vec()).unwrap()
                })
                .collect::<Vec<_>>();
            black_box(expect_range(result))
        })
    });
    let (_dir, redb) = redb_read();
    group.bench_function("redb", |b| {
        b.iter(|| {
            let txn = redb.begin_read().unwrap();
            let table = txn.open_table(REDB_TABLE).unwrap();
            let result = table
                .range(0_u64..RANGE_END)
                .unwrap()
                .map(|entry| entry.unwrap().1.value().to_string())
                .collect::<Vec<_>>();
            black_box(expect_range(result))
        })
    });
    let (_dir, conn) = sqlite_read();
    group.bench_function("rusqlite", |b| {
        b.iter(|| {
            let txn = conn.unchecked_transaction().unwrap();
            let mut statement = txn
                .prepare_cached("SELECT v FROM kv WHERE k >= 0 AND k < 100 ORDER BY k")
                .unwrap();
            let mut rows = statement.query([]).unwrap();
            let mut result = Vec::with_capacity(RANGE_END as usize);
            while let Some(row) = rows.next().unwrap() {
                result.push(row.get::<_, String>(0).unwrap());
            }
            black_box(expect_range(result))
        })
    });
    let (_dir, env, db) = heed_read();
    group.bench_function("heed", |b| {
        b.iter(|| {
            let txn = env.read_txn().unwrap();
            let result = db
                .range(&txn, &(0_u64..RANGE_END))
                .unwrap()
                .map(|entry| entry.unwrap().1.to_string())
                .collect::<Vec<_>>();
            black_box(expect_range(result))
        })
    });
    let (_dir, native) = native_read();
    group.bench_function("native_db", |b| {
        b.iter(|| {
            let txn = native.r_transaction().unwrap();
            let result = txn
                .scan()
                .primary::<NativeItem>()
                .unwrap()
                .range(0_u64..RANGE_END)
                .unwrap()
                .map(|item| item.unwrap().value)
                .collect::<Vec<_>>();
            black_box(expect_range(result))
        })
    });
    group.finish();
}

fn print_versions(_: &mut Criterion) {
    println!(
        "factomic={} fjall=3.1.13 redb=5.0.0 rusqlite=0.43.0 heed=0.23.1 native_db=0.8.2 criterion=0.8.2",
        env!("CARGO_PKG_VERSION")
    );
}

criterion_group!(
    benches,
    print_versions,
    commit_1,
    commit_100,
    point_read_benchmarks,
    range_read_benchmarks
);
criterion_main!(benches);

fn commit_1(c: &mut Criterion) {
    commit_benchmarks(c, "commit_1", 1)
}

fn commit_100(c: &mut Criterion) {
    commit_benchmarks(c, "commit_100", 100)
}
