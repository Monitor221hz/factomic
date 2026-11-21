use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::measurement::WallTime;
use criterion::{
    BatchSize, BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main,
};
use factomic::{AttributeDef, AttributeId, CollectionKind, Db, EntityId, Store, Type};
use fjall::{Database, Keyspace, KeyspaceCreateOptions, OwnedWriteBatch, PersistMode, Readable};
use heed::byteorder::BigEndian;
use heed::types::{Str, U32};
use heed::{CommitOrAbort, Database as HeedDatabase, Env, EnvOpenOptions};
use redb::{
    Database as RedbDatabase, Durability, ReadableDatabase, ReadableTable, TableDefinition,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

const REDB_TABLE: TableDefinition<u64, &str> = TableDefinition::new("kv");

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

fn factomic_store() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (dir, store)
}

fn configure_writes(group: &mut BenchmarkGroup<'_, WallTime>) {
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
}

fn configure_reads(group: &mut BenchmarkGroup<'_, WallTime>) {
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
}

type FactomicCommitInput = (TempDir, Store, EntityId, AttributeId, Option<Vec<String>>);

fn factomic_commit_input(size: usize) -> FactomicCommitInput {
    let (dir, store) = factomic_store();
    let attr = store
        .declare_attribute(def("bench/value", Type::String, CollectionKind::Many))
        .unwrap();
    let entity = store.new_entity().unwrap();
    let values = (0..size)
        .map(|index| format!("value-{index}"))
        .collect::<Vec<_>>();
    (dir, store, entity, attr, Some(values))
}

fn factomic_commit(size: usize) -> impl FnMut(&mut FactomicCommitInput) -> Option<factomic::TxnId> {
    move |(_dir, store, entity, attr, values): &mut FactomicCommitInput| {
        let mut tx = store.transact();
        for value in values.take().unwrap() {
            tx.assert(*entity, *attr, &value);
        }
        let txn = tx.commit().unwrap();
        assert_eq!(
            store.db().entity_values::<String>(*entity, *attr).count(),
            size
        );
        black_box(txn)
    }
}

fn fjall_database() -> (TempDir, Database, Keyspace) {
    let dir = TempDir::new().unwrap();
    let db = Database::builder(dir.path()).open().unwrap();
    let keyspace = db.keyspace("kv", KeyspaceCreateOptions::default).unwrap();
    (dir, db, keyspace)
}

fn fjall_commit(size: usize) -> impl FnMut(&mut (TempDir, Database, Keyspace)) {
    move |(_dir, db, keyspace): &mut (TempDir, Database, Keyspace)| {
        let mut batch =
            OwnedWriteBatch::with_capacity(db.clone(), size).durability(Some(PersistMode::SyncAll));
        for index in 0..size as u64 {
            batch.insert(keyspace, index.to_be_bytes().to_vec(), b"value".to_vec());
        }
        black_box(batch.commit().unwrap())
    }
}

fn redb_database() -> (TempDir, RedbDatabase) {
    let dir = TempDir::new().unwrap();
    let db = RedbDatabase::create(dir.path().join("db.redb")).unwrap();
    (dir, db)
}

fn redb_commit(size: usize) -> impl FnMut(&mut (TempDir, RedbDatabase)) {
    move |(_dir, db): &mut (TempDir, RedbDatabase)| {
        let mut txn = db.begin_write().unwrap();
        txn.set_durability(Durability::Immediate).unwrap();
        {
            let mut table = txn.open_table(REDB_TABLE).unwrap();
            for index in 0..size as u64 {
                table.insert(index, "value").unwrap();
            }
        }
        black_box(txn.commit().unwrap())
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

fn sqlite_commit(size: usize) -> impl FnMut(&mut (TempDir, Connection)) {
    move |(_dir, conn): &mut (TempDir, Connection)| {
        let txn = conn.transaction().unwrap();
        for index in 0..size as i64 {
            txn.execute("INSERT INTO kv (k, v) VALUES (?1, 'value')", params![index])
                .unwrap();
        }
        black_box(txn.commit().unwrap())
    }
}

type HeedDb = HeedDatabase<'static, U32<BigEndian>, Str>;

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
            wdo.create_database::<U32<BigEndian>, Str>(txn, None)
                .map(CommitOrAbort::<_, ()>::Commit)
        })
        .unwrap()
        .unwrap_commit();
    (dir, env, db)
}

fn heed_commit(size: usize) -> impl FnMut(&mut (TempDir, Env, HeedDb)) {
    move |(_dir, env, db): &mut (TempDir, Env, HeedDb)| {
        let mut txn = env.write_txn().unwrap();
        for index in 0..size as u32 {
            db.put(&mut txn, &index, "value").unwrap();
        }
        black_box(txn.commit().unwrap())
    }
}

fn commit_benchmarks(c: &mut Criterion, name: &'static str, size: usize) {
    let mut group = c.benchmark_group(name);
    configure_writes(&mut group);
    group.throughput(Throughput::Elements(size as u64));
    group.bench_function("factomic", |b| {
        b.iter_batched_ref(
            || factomic_commit_input(size),
            factomic_commit(size),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("fjall", |b| {
        b.iter_batched_ref(fjall_database, fjall_commit(size), BatchSize::SmallInput)
    });
    group.bench_function("redb", |b| {
        b.iter_batched_ref(redb_database, redb_commit(size), BatchSize::SmallInput)
    });
    group.bench_function("rusqlite", |b| {
        b.iter_batched_ref(
            sqlite_connection,
            sqlite_commit(size),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("heed", |b| {
        b.iter_batched_ref(heed_database, heed_commit(size), BatchSize::SmallInput)
    });
    group.finish();
}

struct FactomicRead {
    _dir: TempDir,
    _store: Store,
    db: Arc<Db>,
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
    let entities = (0..1000)
        .map(|_| store.new_entity().unwrap())
        .collect::<Vec<_>>();
    let mut tx = store.transact();
    for (index, entity) in entities.iter().enumerate() {
        tx.assert(*entity, point, &format!("value-{index}"));
    }
    for index in 0..1000 {
        tx.assert(range_entity, range, &format!("value-{index:04}"));
    }
    tx.commit().unwrap();
    let db = store.db();
    FactomicRead {
        _dir: dir,
        _store: store,
        db,
        entities,
        point,
        range,
        range_entity,
    }
}

struct FjallRead {
    _dir: TempDir,
    _db: Database,
    snapshot: fjall::Snapshot,
    keyspace: Keyspace,
}

fn fjall_read() -> FjallRead {
    let (_dir, db, keyspace) = fjall_database();
    let mut batch =
        OwnedWriteBatch::with_capacity(db.clone(), 1000).durability(Some(PersistMode::SyncAll));
    for index in 0..1000_u64 {
        batch.insert(
            &keyspace,
            index.to_be_bytes().to_vec(),
            format!("value-{index}"),
        );
    }
    batch.commit().unwrap();
    let snapshot = db.snapshot();
    FjallRead {
        _dir,
        _db: db,
        snapshot,
        keyspace,
    }
}

fn redb_read() -> (TempDir, RedbDatabase) {
    let (dir, db) = redb_database();
    let mut txn = db.begin_write().unwrap();
    txn.set_durability(Durability::Immediate).unwrap();
    {
        let mut table = txn.open_table(REDB_TABLE).unwrap();
        for index in 0..1000_u64 {
            table
                .insert(index, format!("value-{index}").as_str())
                .unwrap();
        }
    }
    txn.commit().unwrap();
    (dir, db)
}

fn sqlite_read() -> (TempDir, Connection) {
    let (dir, mut conn) = sqlite_connection();
    let txn = conn.transaction().unwrap();
    for index in 0..1000_i64 {
        txn.execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2)",
            params![index, format!("value-{index}")],
        )
        .unwrap();
    }
    txn.commit().unwrap();
    (dir, conn)
}

fn heed_read() -> (TempDir, Env, HeedDb) {
    let (dir, env, db) = heed_database();
    let mut txn = env.write_txn().unwrap();
    for index in 0..1000_u32 {
        db.put(&mut txn, &index, format!("value-{index}").as_str())
            .unwrap();
    }
    txn.commit().unwrap();
    (dir, env, db)
}

fn point_read_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("point_read");
    configure_reads(&mut group);
    let factomic = factomic_read();
    group.bench_function("factomic", |b| {
        b.iter(|| {
            black_box(
                factomic
                    .db
                    .entity_value::<String>(factomic.entities[500], factomic.point)
                    .unwrap(),
            )
        })
    });
    let fjall = fjall_read();
    group.bench_function("fjall", |b| {
        b.iter(|| {
            black_box(
                fjall
                    .snapshot
                    .get(&fjall.keyspace, 500_u64.to_be_bytes())
                    .unwrap(),
            )
        })
    });
    let (_dir, redb) = redb_read();
    group.bench_function("redb", |b| {
        b.iter(|| {
            let txn = redb.begin_read().unwrap();
            let table = txn.open_table(REDB_TABLE).unwrap();
            black_box(
                table
                    .get(500_u64)
                    .unwrap()
                    .map(|value| value.value().to_string()),
            )
        })
    });
    let (_dir, conn) = sqlite_read();
    group.bench_function("rusqlite", |b| {
        b.iter(|| {
            let mut statement = conn
                .prepare_cached("SELECT v FROM kv WHERE k = ?1")
                .unwrap();
            black_box(
                statement
                    .query_row([500_i64], |row| row.get::<_, String>(0))
                    .unwrap(),
            )
        })
    });
    let (_dir, env, db) = heed_read();
    group.bench_function("heed", |b| {
        b.iter(|| {
            let txn = env.read_txn().unwrap();
            black_box(
                db.get(&txn, &500_u32)
                    .unwrap()
                    .map(|value| value.to_string()),
            )
        })
    });
    group.finish();
}

fn range_read_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_read_100");
    configure_reads(&mut group);
    let factomic = factomic_read();
    group.bench_function("factomic", |b| {
        b.iter(|| {
            black_box(
                factomic
                    .db
                    .entity_values::<String>(factomic.range_entity, factomic.range)
                    .take(100)
                    .collect::<factomic::Result<Vec<_>>>()
                    .unwrap(),
            )
        })
    });
    let fjall = fjall_read();
    group.bench_function("fjall", |b| {
        b.iter(|| {
            black_box(
                fjall
                    .snapshot
                    .range(
                        &fjall.keyspace,
                        0_u64.to_be_bytes().to_vec()..100_u64.to_be_bytes().to_vec(),
                    )
                    .map(|guard| guard.into_inner().unwrap())
                    .collect::<Vec<_>>(),
            )
        })
    });
    let (_dir, redb) = redb_read();
    group.bench_function("redb", |b| {
        b.iter(|| {
            let txn = redb.begin_read().unwrap();
            let table = txn.open_table(REDB_TABLE).unwrap();
            black_box(
                table
                    .range(0_u64..100_u64)
                    .unwrap()
                    .map(|entry| {
                        let (key, value) = entry.unwrap();
                        (key.value(), value.value().to_string())
                    })
                    .collect::<Vec<_>>(),
            )
        })
    });
    let (_dir, conn) = sqlite_read();
    group.bench_function("rusqlite", |b| {
        b.iter(|| {
            let mut statement = conn
                .prepare_cached("SELECT k, v FROM kv WHERE k >= 0 AND k < 100 ORDER BY k")
                .unwrap();
            let mut rows = statement.query([]).unwrap();
            let mut values = Vec::new();
            while let Some(row) = rows.next().unwrap() {
                values.push((
                    row.get::<_, i64>(0).unwrap(),
                    row.get::<_, String>(1).unwrap(),
                ));
            }
            black_box(values)
        })
    });
    let (_dir, env, db) = heed_read();
    group.bench_function("heed", |b| {
        b.iter(|| {
            let txn = env.read_txn().unwrap();
            black_box(
                db.range(&txn, &(0_u32..100_u32))
                    .unwrap()
                    .map(|entry| {
                        let (key, value) = entry.unwrap();
                        (key, value.to_string())
                    })
                    .collect::<Vec<_>>(),
            )
        })
    });
    group.finish();
}

fn print_versions(_: &mut Criterion) {
    println!("factomic=0.2.0 fjall=3.1.13 redb=5.0.0 rusqlite=0.43.0 heed=0.23.1 criterion=0.8.2");
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
