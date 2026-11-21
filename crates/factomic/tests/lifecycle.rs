#![cfg(feature = "derive")]

use std::sync::Arc;

use factomic::{Result, Schema, Store};
use tempfile::TempDir;

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
