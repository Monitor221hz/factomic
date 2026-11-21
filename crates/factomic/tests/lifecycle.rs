#![cfg(feature = "derive")]

use std::sync::Arc;

use factomic::{EntityId, Result, Schema, Store, TxnId};
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[derive(factomic::Entity)]
struct Person {
    name: Option<String>,
}

fn temp_store() -> (TempDir, Arc<Store>) {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    (dir, store)
}

#[test]
fn create_only_allocates_until_first_assertion() -> Result<()> {
    let (_dir, store) = temp_store();
    let person = store.create::<Person>()?;
    let id = person.entity;
    assert!(!person.exists()?);
    assert!(store.db().entity::<Person>(id)?.is_none());
    assert!(store.entity_mut::<Person>(id)?.is_none());

    let mut tx = person.transact();
    tx.set_name("Alice");
    tx.commit()?;
    assert!(person.exists()?);
    let current = store.db().entity::<Person>(id)?.expect("person");
    assert_eq!(current.name()?, Some("Alice".into()));
    assert!(store.entity_mut::<Person>(id)?.is_some());
    Ok(())
}

#[test]
fn schema_and_entity_existence_are_distinct() -> Result<()> {
    let (_dir, store) = temp_store();
    let id = store.new_entity()?;
    assert!(!store.db().entity_exists(id)?);
    assert!(store.db().entity::<Person>(id)?.is_none());
    let schema = PersonSchema::declare(&store)?;
    assert!(!store.db().entity_exists(id)?);
    assert!(store.db().entity::<Person>(id)?.is_none());

    store.assert(id, schema.name, &"raw")?;
    assert!(store.db().entity_exists(id)?);
    assert!(store.db().entity::<Person>(id)?.is_some());
    Ok(())
}

#[test]
fn clearing_last_user_field_removes_existence() -> Result<()> {
    let (_dir, store) = temp_store();
    let person = store.create::<Person>()?;
    let mut tx = person.transact();
    tx.set_name("Alice");
    tx.commit()?;
    assert!(person.exists()?);

    let mut tx = person.transact();
    tx.clear_name();
    tx.commit()?;
    assert!(!person.exists()?);
    assert!(store.db().entity::<Person>(person.entity)?.is_none());
    Ok(())
}

#[test]
fn typed_retraction_removes_current_existence_and_is_idempotent() -> Result<()> {
    let (_dir, store) = temp_store();
    let person = store.create::<Person>()?;
    let id = person.entity;
    let mut tx = person.transact();
    tx.set_name("Alice");
    let assertion = tx.commit()?.expect("assertion");

    let mut tx = person.transact();
    tx.retract();
    tx.commit()?.expect("retraction");
    assert!(!person.exists()?);
    assert!(store.db().entity::<Person>(id)?.is_none());
    assert!(store.entity_mut::<Person>(id)?.is_none());
    assert_eq!(person.retract()?, None);

    let historical = person.read_at(assertion)?.expect("historical person");
    assert_eq!(historical.name()?, Some("Alice".into()));
    assert!(historical.exists()?);

    let mut tx = person.transact();
    tx.set_name("Bob");
    tx.commit()?;
    assert!(person.exists()?);
    assert_eq!(person.read().name()?, Some("Bob".into()));
    Ok(())
}

#[test]
fn successful_mutations_survive_process_exit_without_drop() -> TestResult {
    const CHILD: &str = "FACTOMIC_DURABILITY_TEST_CHILD";
    const DB_PATH: &str = "FACTOMIC_DURABILITY_TEST_DB";
    const IDS_PATH: &str = "FACTOMIC_DURABILITY_TEST_IDS";

    if std::env::var_os(CHILD).is_some() {
        let store = Arc::new(Store::open(std::env::var_os(DB_PATH).unwrap())?);
        let person = store.create::<Person>()?;
        let mut tx = person.transact();
        tx.set_name("Alice");
        let txn = tx.commit()?.expect("committed transaction");
        std::fs::write(
            std::env::var_os(IDS_PATH).unwrap(),
            format!("{}\n{}", person.entity.as_u64(), txn.as_u64()),
        )?;
        std::process::exit(0);
    }

    let db_dir = TempDir::new()?;
    let ids_dir = TempDir::new()?;
    let ids_path = ids_dir.path().join("ids");
    let status = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "successful_mutations_survive_process_exit_without_drop",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(DB_PATH, db_dir.path())
        .env(IDS_PATH, &ids_path)
        .status()?;
    assert!(status.success());

    let ids = std::fs::read_to_string(ids_path)?;
    let mut ids = ids.lines();
    let entity = EntityId::with_value(ids.next().unwrap().parse()?);
    let txn = TxnId::with_value(ids.next().unwrap().parse()?);
    assert!(ids.next().is_none());

    let store = Store::open(db_dir.path())?;
    let db = store.db();
    let schema = PersonSchema::get(&db)?.expect("person schema");
    let person = db.entity::<Person>(entity)?.expect("person");
    assert_eq!(person.name()?, Some("Alice".into()));
    let historical = db
        .at(txn)
        .entity::<Person>(entity)?
        .expect("historical person");
    assert_eq!(historical.name()?, Some("Alice".into()));
    assert!(
        db.tx_factoms(txn)?
            .iter()
            .any(|factom| factom.entity() == entity && factom.attribute() == schema.name)
    );

    let next_entity = store.new_entity()?;
    assert!(next_entity.index() > entity.index());
    let next_txn = store
        .assert(entity, schema.name, &"Bob")?
        .expect("next transaction");
    assert!(next_txn > txn);
    assert_eq!(
        store.db().entity::<Person>(entity)?.unwrap().name()?,
        Some("Bob".into())
    );
    Ok(())
}
