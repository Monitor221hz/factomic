use std::collections::BTreeSet;
use std::sync::{Arc, Barrier};
use std::thread;

use factomic::{AttributeDef, AttributeId, CollectionKind, EntityId, Result, Store, TxnId, Type};
use itertools::Itertools;
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

fn temp_store() -> (TempDir, Arc<Store>) {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    (dir, store)
}

fn paired_schema(store: &Store) -> (AttributeId, AttributeId) {
    let [name, version] = store
        .declare_attributes([
            def("person/name", Type::String, CollectionKind::One),
            def("person/version", Type::Long, CollectionKind::One),
        ])
        .unwrap();
    (name, version)
}

#[test]
fn multi_readers_see_only_complete_writer_snapshots() {
    let (_dir, store) = temp_store();
    let (name, version) = paired_schema(&store);
    let entity = store.new_entity().unwrap();
    store
        .transact()
        .assert(entity, name, &"v0")
        .assert(entity, version, &0_i64)
        .commit()
        .unwrap();
    let pinned = store.db();
    let start = Arc::new(Barrier::new(4));
    let end = Arc::new(Barrier::new(4));

    let observations = thread::scope(|scope| {
        let mut readers = Vec::new();
        for _ in 0..3 {
            let store = store.clone();
            let start = start.clone();
            let end = end.clone();
            readers.push(scope.spawn(move || {
                let mut observations = Vec::new();
                for _ in 0..3 {
                    start.wait();
                    let db = store.db();
                    let name = db.entity_value::<String>(entity, name);
                    let version = db.entity_value::<i64>(entity, version);
                    end.wait();
                    observations.push((name, version));
                }
                observations
            }));
        }

        let mut commits = Vec::new();
        for round in 1..=3_i64 {
            start.wait();
            let value = format!("v{round}");
            let commit = store
                .transact()
                .assert(entity, name, &value)
                .assert(entity, version, &round)
                .commit();
            end.wait();
            commits.push(commit);
        }
        for commit in commits {
            commit.unwrap();
        }
        readers
            .into_iter()
            .map(|reader| reader.join().unwrap())
            .collect::<Vec<_>>()
    });

    for reader in observations {
        for (name, version) in reader {
            let name = name.unwrap().unwrap();
            let version = version.unwrap().unwrap();
            assert_eq!(name, format!("v{version}"));
        }
    }
    assert_eq!(
        pinned.entity_value::<String>(entity, name).unwrap(),
        Some("v0".into())
    );
    assert_eq!(
        pinned
            .newest()
            .entity_value::<i64>(entity, version)
            .unwrap(),
        Some(0)
    );
    let latest = store.db();
    assert_eq!(
        latest.entity_value::<String>(entity, name).unwrap(),
        Some("v3".into())
    );
    assert_eq!(
        latest.entity_value::<i64>(entity, version).unwrap(),
        Some(3)
    );
}

#[test]
fn concurrent_writers_preserve_all_independent_insertions() {
    let (_dir, store) = temp_store();
    let tags = store
        .declare_attribute(def("person/tags", Type::String, CollectionKind::Many))
        .unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let results = thread::scope(|scope| {
        let mut writers = Vec::new();
        for index in 0..4 {
            let store = store.clone();
            let barrier = barrier.clone();
            writers.push(scope.spawn(move || {
                barrier.wait();
                let entity = store.new_entity().unwrap();
                let value = format!("writer-{index}");
                let txn = store.assert(entity, tags, &value).unwrap().unwrap();
                (entity, value, txn)
            }));
        }
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(
        results
            .iter()
            .map(|(entity, _, _)| *entity)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(
        results
            .iter()
            .map(|(_, _, txn)| *txn)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    let db = store.db();
    assert_eq!(
        db.txn(),
        results.iter().map(|(_, _, txn)| *txn).max().unwrap()
    );
    for (entity, value, txn) in results {
        assert_eq!(
            db.entity_values::<String>(entity, tags)
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec![value]
        );
        assert!(db.tx_factoms(txn).flatten().collect_vec().len() >= 2);
    }
}

#[test]
fn contended_writers_publish_complete_pairs_and_logs() {
    let (dir, store) = temp_store();
    let (name, version) = paired_schema(&store);
    let entity = store.new_entity().unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let results = thread::scope(|scope| {
        let mut writers = Vec::new();
        for index in 0..4_i64 {
            let store = store.clone();
            let barrier = barrier.clone();
            writers.push(scope.spawn(move || {
                barrier.wait();
                let value = format!("writer-{index}");
                let txn = store
                    .transact()
                    .assert(entity, name, &value)
                    .assert(entity, version, &index)
                    .commit()
                    .unwrap()
                    .unwrap();
                (index, value, txn)
            }));
        }
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(
        results
            .iter()
            .map(|(_, _, txn)| *txn)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    let db = store.db();
    assert_eq!(
        db.txn(),
        results.iter().map(|(_, _, txn)| *txn).max().unwrap()
    );
    let final_name = db.entity_value::<String>(entity, name).unwrap().unwrap();
    let final_version = db.entity_value::<i64>(entity, version).unwrap().unwrap();
    assert_eq!(final_name, format!("writer-{final_version}"));
    for (index, value, txn) in results {
        let historical = db.at(txn);
        assert_eq!(
            historical.entity_value::<String>(entity, name).unwrap(),
            Some(value)
        );
        assert_eq!(
            historical.entity_value::<i64>(entity, version).unwrap(),
            Some(index)
        );
        let log = db.tx_factoms(txn).flatten().collect_vec();
        assert!(log.iter().any(|factom| factom.attribute() == name));
        assert!(log.iter().any(|factom| factom.attribute() == version));
    }
    drop(db);
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    assert_eq!(
        reopened.db().entity_value::<String>(entity, name).unwrap(),
        Some(final_name)
    );
    assert_eq!(
        reopened.db().entity_value::<i64>(entity, version).unwrap(),
        Some(final_version)
    );
}

#[test]
fn concurrent_unique_declarations_allocate_unique_ids_and_txns() {
    let (_dir, store) = temp_store();
    let initial = store.db().txn();
    let barrier = Arc::new(Barrier::new(4));
    let results = thread::scope(|scope| {
        let mut writers = Vec::new();
        for index in 0..4 {
            let store = store.clone();
            let barrier = barrier.clone();
            writers.push(scope.spawn(move || {
                barrier.wait();
                let name = format!("person/attr-{index}");
                let id = store
                    .declare_attribute(def(&name, Type::String, CollectionKind::One))
                    .unwrap();
                (name, id)
            }));
        }
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(
        results
            .iter()
            .map(|(_, id)| *id)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(
        store.db().txn().entity_id().index(),
        initial.entity_id().index() + 4
    );
    let db = store.db();
    for (name, _) in results {
        assert!(db.attribute_id(&name).unwrap().is_some());
    }
}

#[test]
fn concurrent_identical_declaration_creates_one_transaction() {
    let (_dir, store) = temp_store();
    let initial = store.db().txn();
    let barrier = Arc::new(Barrier::new(4));
    let ids = thread::scope(|scope| {
        let mut writers = Vec::new();
        for _ in 0..4 {
            let store = store.clone();
            let barrier = barrier.clone();
            writers.push(scope.spawn(move || {
                barrier.wait();
                store
                    .declare_attribute(def("person/shared", Type::String, CollectionKind::One))
                    .unwrap()
            }));
        }
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert!(ids.iter().all(|id| *id == ids[0]));
    assert_eq!(
        store.db().txn().entity_id().index(),
        initial.entity_id().index() + 1
    );
    assert_eq!(
        store.db().attribute_id("person/shared").unwrap(),
        Some(ids[0])
    );
}

#[test]
fn transaction_ids_are_send_sync_compatible() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Store>();
    assert_send_sync::<EntityId>();
    assert_send_sync::<TxnId>();
    assert_send_sync::<AttributeId>();
}
