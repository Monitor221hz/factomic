#![cfg(feature = "derive")]

use std::sync::Arc;

use factomic::{
    AttributeDef, AttributeId, CollectionKind, EntityId, Result, Schema, Store, TxnId, Type,
};
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

fn test_def(name: &str, value_type: Type) -> AttributeDef {
    AttributeDef {
        name: name.into(),
        value_type,
        kind: CollectionKind::One,
        key_type: None,
        indexed: false,
        is_component: false,
    }
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

const CHILD: &str = "FACTOMIC_DURABILITY_TEST_CHILD";
const DB_PATH: &str = "FACTOMIC_DURABILITY_TEST_DB";
const IDS_PATH: &str = "FACTOMIC_DURABILITY_TEST_IDS";
const ABORT: &str = "FACTOMIC_DURABILITY_TEST_ABORT";

fn run_successful_mutations(test_name: &str, abort: bool) -> TestResult {
    if std::env::var_os(CHILD).is_some() {
        let store = Arc::new(Store::open(std::env::var_os(DB_PATH).unwrap())?);
        let person = store.create::<Person>()?;
        let declaration = store.db().txn();
        let mut tx = person.transact();
        tx.set_name("Alice");
        let txn = tx.commit()?.expect("committed transaction");
        std::fs::write(
            std::env::var_os(IDS_PATH).unwrap(),
            format!(
                "{}\n{}\n{}",
                person.entity.as_u64(),
                declaration.as_u64(),
                txn.as_u64()
            ),
        )?;
        if std::env::var_os(ABORT).is_some() {
            std::process::abort();
        }
        std::process::exit(0);
    }

    let db_dir = TempDir::new()?;
    let ids_dir = TempDir::new()?;
    let ids_path = ids_dir.path().join("ids");
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD, "1")
        .env(DB_PATH, db_dir.path())
        .env(IDS_PATH, &ids_path);
    if abort {
        command.env(ABORT, "1");
    }
    let status = command.status()?;
    assert_eq!(!status.success(), abort);

    let ids = std::fs::read_to_string(ids_path)?;
    let mut ids = ids.lines();
    let entity = EntityId::with_value(ids.next().unwrap().parse()?);
    let declaration = TxnId::with_value(ids.next().unwrap().parse()?);
    let txn = TxnId::with_value(ids.next().unwrap().parse()?);
    assert!(ids.next().is_none());

    let store = Store::open(db_dir.path())?;
    let db = store.db();
    assert_eq!(db.txn(), txn);
    assert_eq!(db.as_of(), None);
    let schema = PersonSchema::get(&db)?.expect("person schema");
    let person = db.entity::<Person>(entity)?.expect("person");
    assert_eq!(person.name()?, Some("Alice".into()));
    let declaration_view = db.at(declaration);
    assert_eq!(declaration_view.txn(), txn);
    assert_eq!(declaration_view.as_of(), Some(declaration));
    assert!(declaration_view.entity::<Person>(entity)?.is_none());
    assert_eq!(db.tx_factoms(declaration)?.len(), 1);
    assert!(
        db.tx_factoms(txn)?
            .iter()
            .any(|factom| factom.entity() == entity && factom.attribute() == schema.name)
    );

    assert_eq!(store.assert(entity, schema.name, &"Alice")?, None);
    assert_eq!(store.db().txn(), txn);
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

#[test]
fn successful_mutations_survive_process_exit_without_drop() -> TestResult {
    run_successful_mutations(
        "successful_mutations_survive_process_exit_without_drop",
        false,
    )
}

#[test]
fn successful_mutations_survive_abort() -> TestResult {
    run_successful_mutations("successful_mutations_survive_abort", true)
}

#[test]
fn declaration_only_survives_abort() -> TestResult {
    if std::env::var_os(CHILD).is_some() {
        let store = Store::open(std::env::var_os(DB_PATH).unwrap())?;
        let [name, age] = store.declare_attributes([
            test_def("person/name", Type::String),
            test_def("person/age", Type::Long),
        ])?;
        let declaration = store.db().txn();
        std::fs::write(
            std::env::var_os(IDS_PATH).unwrap(),
            format!(
                "{}\n{}\n{}",
                name.as_u16(),
                age.as_u16(),
                declaration.as_u64()
            ),
        )?;
        std::process::abort();
    }

    let db_dir = TempDir::new()?;
    let ids_dir = TempDir::new()?;
    let ids_path = ids_dir.path().join("ids");
    let status = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "declaration_only_survives_abort", "--nocapture"])
        .env(CHILD, "1")
        .env(DB_PATH, db_dir.path())
        .env(IDS_PATH, &ids_path)
        .env(ABORT, "1")
        .status()?;
    assert!(!status.success());

    let ids = std::fs::read_to_string(ids_path)?;
    let mut ids = ids.lines();
    let name = AttributeId::new(ids.next().unwrap().parse()?);
    let age = AttributeId::new(ids.next().unwrap().parse()?);
    let declaration = TxnId::with_value(ids.next().unwrap().parse()?);
    let store = Store::open(db_dir.path())?;
    assert_eq!(store.db().txn(), declaration);
    assert_eq!(store.db().attribute_id("person/name")?, Some(name));
    assert_eq!(store.db().attribute_id("person/age")?, Some(age));
    assert_eq!(store.db().tx_factoms(declaration)?.len(), 1);
    assert_eq!(
        store.declare_attributes([
            test_def("person/name", Type::String),
            test_def("person/age", Type::Long),
        ])?,
        [name, age]
    );
    assert_eq!(store.db().txn(), declaration);
    let next = store.declare_attribute(test_def("person/next", Type::String))?;
    assert!(next.as_u16() > name.as_u16().max(age.as_u16()));
    Ok(())
}

#[test]
fn allocated_entity_without_facts_survives_abort() -> TestResult {
    if std::env::var_os(CHILD).is_some() {
        let store = Store::open(std::env::var_os(DB_PATH).unwrap())?;
        let entity = store.new_entity()?;
        std::fs::write(
            std::env::var_os(IDS_PATH).unwrap(),
            entity.as_u64().to_string(),
        )?;
        std::process::abort();
    }

    let db_dir = TempDir::new()?;
    let ids_dir = TempDir::new()?;
    let ids_path = ids_dir.path().join("ids");
    let status = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "allocated_entity_without_facts_survives_abort",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(DB_PATH, db_dir.path())
        .env(IDS_PATH, &ids_path)
        .env(ABORT, "1")
        .status()?;
    assert!(!status.success());

    let entity = EntityId::with_value(std::fs::read_to_string(ids_path)?.parse()?);
    let store = Store::open(db_dir.path())?;
    assert_eq!(store.db().txn(), TxnId::with_value(0));
    assert!(!store.db().entity_exists(entity)?);
    assert!(store.new_entity()?.index() > entity.index());
    Ok(())
}
