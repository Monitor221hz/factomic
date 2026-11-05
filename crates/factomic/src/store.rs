use std::path::Path;

use fjall::{Database, Keyspace, KeyspaceCreateOptions};

use crate::error::{Error, Result};
use crate::id::{
    AttributeId, EntityId, FIRST_USER_ATTR, SYSTEM_PARTITION, TX_TIMESTAMP_ATTR, TxnId,
    USER_PARTITION,
};
use crate::value::{Type, tag_for_type, type_for_tag};

mod query;
mod transact;

pub use transact::Transaction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDef {
    pub name: String,
    pub value_type: Type,
    pub indexed: bool,
}

const META_NEXT_ATTR: &[u8; 9] = b"next_attr";
const META_ENTITY_PREFIX: u8 = 0x02;

pub struct Store {
    pub(crate) db: Database,
    pub(crate) meta: Keyspace,
    pub(crate) attrs: Keyspace,
    pub(crate) eavt: Keyspace,
    pub(crate) aevt: Keyspace,
    pub(crate) avet: Keyspace,
    pub(crate) vaet: Keyspace,
    pub(crate) eavt_h: Keyspace,
    pub(crate) aevt_h: Keyspace,
    pub(crate) avet_h: Keyspace,
    pub(crate) vaet_h: Keyspace,
    pub(crate) txlog: Keyspace,
}
const BY_NAME_PREFIX: u8 = 0x01; // -> attr id
const BY_ATTR_PREFIX: u8 = 0x02; // -> name, tag, flags

#[inline]
fn by_attr_key(attr_id: AttributeId) -> Vec<u8> {
    let mut by_id_key = vec![BY_ATTR_PREFIX];
    by_id_key.extend_from_slice(&attr_id.to_be_bytes());
    by_id_key
}

#[inline]
fn by_attr_value(name: &str, value_type: Type, indexed: bool) -> Vec<u8> {
    let mut by_id_value = name.as_bytes().to_vec();
    by_id_value.push(tag_for_type(value_type));
    by_id_value.push(if indexed { 0x01 } else { 0x00 });
    by_id_value
}

#[inline]
fn by_name_key(name: &str) -> Vec<u8> {
    let mut by_name_key = vec![BY_NAME_PREFIX];
    by_name_key.extend_from_slice(name.as_bytes());
    by_name_key
}

#[inline]
fn by_name_value(attr_id: AttributeId) -> [u8; size_of::<AttributeId>()] {
    attr_id.to_be_bytes()
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::builder(path).open()?;
        let meta = db.keyspace("meta", KeyspaceCreateOptions::default)?;
        let attrs = db.keyspace("attrs", KeyspaceCreateOptions::default)?;
        let eavt = db.keyspace("eavt", KeyspaceCreateOptions::default)?;
        let aevt = db.keyspace("aevt", KeyspaceCreateOptions::default)?;
        let avet = db.keyspace("avet", KeyspaceCreateOptions::default)?;
        let vaet = db.keyspace("vaet", KeyspaceCreateOptions::default)?;
        let eavt_h = db.keyspace("eavt_h", KeyspaceCreateOptions::default)?;
        let aevt_h = db.keyspace("aevt_h", KeyspaceCreateOptions::default)?;
        let avet_h = db.keyspace("avet_h", KeyspaceCreateOptions::default)?;
        let vaet_h = db.keyspace("vaet_h", KeyspaceCreateOptions::default)?;
        let txlog = db.keyspace("txlog", KeyspaceCreateOptions::default)?;
        let store = Store {
            db,
            meta,
            attrs,
            eavt,
            aevt,
            avet,
            vaet,
            eavt_h,
            aevt_h,
            avet_h,
            vaet_h,
            txlog,
        };
        store.sync_system_attributes()?;
        Ok(store)
    }

    fn sync_system_attributes(&self) -> Result<()> {
        let by_id_key = by_attr_key(TX_TIMESTAMP_ATTR);
        if self.attrs.get(&by_id_key)?.is_some() {
            return Ok(());
        }
        let name = "db/txTimestamp";
        self.attrs
            .insert(by_name_key(name), by_name_value(TX_TIMESTAMP_ATTR))?;

        self.attrs
            .insert(by_id_key, by_attr_value(name, Type::Timestamp, false))?;
        Ok(())
    }

    pub fn declare_attribute(
        &self,
        name: &str,
        value_type: Type,
        indexed: bool,
    ) -> Result<AttributeId> {
        let mut by_name_key = vec![BY_NAME_PREFIX];
        by_name_key.extend_from_slice(name.as_bytes());

        let attribute_id = if let Some(id_bytes) = self.attrs.get(&by_name_key)? {
            let id = AttributeId::new(u16::from_be_bytes(id_bytes[..2].try_into().unwrap()));
            let def = self.attribute_def(id)?;
            if def.value_type != value_type {
                return Err(Error::SchemaConflict(name.to_string()));
            }
            id
        } else {
            let new_id = self.meta.get(META_NEXT_ATTR)?.map_or_else(
                || AttributeId::new(FIRST_USER_ATTR),
                |id| AttributeId::new(u16::from_be_bytes(id[..2].try_into().unwrap())),
            );
            self.meta.insert(
                META_NEXT_ATTR,
                (AttributeId::new(new_id.as_u16() + 1)).to_be_bytes(),
            )?;
            new_id
        };
        self.attrs
            .insert(&by_name_key, attribute_id.to_be_bytes())?;
        let by_id_key = by_attr_key(attribute_id);
        self.attrs
            .insert(by_id_key, by_attr_value(name, value_type, indexed))?;
        Ok(attribute_id)
    }

    pub fn attribute_def(&self, id: AttributeId) -> Result<AttributeDef> {
        let payload = self
            .attrs
            .get(by_attr_key(id))?
            .ok_or_else(|| Error::UnknownAttribute(format!("{id:?}")))?;

        if payload.len() < 2 {
            return Err(Error::CorruptKey {
                expected: 2,
                got: payload.len(),
            });
        }
        let flags = payload[payload.len() - 1];
        let indexed = flags & 1 != 0;

        let value_type = type_for_tag(payload[payload.len() - 2])?;

        let name = String::from_utf8(payload[..payload.len() - 2].to_vec())
            .map_err(|_| Error::CorruptPrimitive(Type::String))?;

        Ok(AttributeDef {
            name,
            value_type,
            indexed,
        })
    }

    pub(crate) fn alloc_txn_id(&self) -> Result<TxnId> {
        Ok(TxnId::with_value(
            self.alloc_entity_id(SYSTEM_PARTITION)?.as_u64(),
        ))
    }

    fn alloc_entity_id(&self, partition: u8) -> Result<EntityId> {
        let key = [META_ENTITY_PREFIX, partition];
        let counter = self
            .meta
            .get(key)?
            .map_or_else(|| 1, |id| u64::from_be_bytes(id[..8].try_into().unwrap()));
        self.meta.insert(key, (counter + 1).to_be_bytes())?;
        Ok(EntityId::new(partition, counter))
    }

    pub fn new_entity(&self) -> Result<EntityId> {
        self.alloc_entity_id(USER_PARTITION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::USER_PARTITION;
    use tempfile::TempDir;

    fn temp_store() -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn declare_and_reload_def() {
        let (_dir, store) = temp_store();
        let id = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let def = store.attribute_def(id).unwrap();
        assert_eq!(def.name, "person/name");
        assert_eq!(def.value_type, Type::String);
        assert!(!def.indexed);
    }

    #[test]
    fn declare_twice_returns_same_id() {
        let (_dir, store) = temp_store();
        let first = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let second = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn declare_conflicting_type_errors() {
        let (_dir, store) = temp_store();
        store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let result = store.declare_attribute("person/name", Type::Long, false);
        assert!(matches!(result, Err(Error::SchemaConflict(_))));
    }

    #[test]
    fn registry_survives_reopen() {
        let (dir, store) = temp_store();
        let id = store
            .declare_attribute("person/age", Type::Long, true)
            .unwrap();
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        let again = reopened
            .declare_attribute("person/age", Type::Long, true)
            .unwrap();
        assert_eq!(id, again);
        assert!(reopened.attribute_def(id).unwrap().indexed);
    }

    #[test]
    fn entity_ids_are_in_user_partition_and_increase() {
        let (_dir, store) = temp_store();
        let first = store.new_entity().unwrap();
        let second = store.new_entity().unwrap();
        assert_eq!(first.partition(), USER_PARTITION);
        assert!(first.index() < second.index());
    }

    #[test]
    fn fresh_entity_has_no_factoms() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        assert!(store.entity_factoms(entity).unwrap().is_empty());
    }
}
