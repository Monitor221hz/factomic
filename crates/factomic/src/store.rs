use std::alloc::System;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use arc_swap::ArcSwap;
use fjall::{Database, Keyspace, KeyspaceCreateOptions, Readable};
use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::factom::CollectionKind;
use crate::id::{
    AttributeId, EntityId, FIRST_USER_ATTR, SYSTEM_PARTITION, TX_KIND_ATTR, TX_MOVE_ATTR,
    TX_TIMESTAMP_ATTR, TxnId, USER_PARTITION,
};
use crate::value::{Type, tag_for_type, type_for_tag};

mod query;
mod transact;

pub use transact::Transaction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDef {
    pub name: String,
    pub value_type: Type,
    pub kind: CollectionKind,
    pub key_type: Option<Type>,
    pub indexed: bool,
    pub is_component: bool,
}

const META_NEXT_ATTR: &[u8; 9] = b"next_attr";
const META_ENTITY_PREFIX: u8 = 0x02;

pub struct Db {
    txn: TxnId,
    as_of: Option<TxnId>,
    snapshot: fjall::Snapshot,
    attrs: Keyspace,
    easvt: Keyspace,
    aesvt: Keyspace,
    avest: Keyspace,
    vaest: Keyspace,
    easvt_h: Keyspace,
    aesvt_h: Keyspace,
    avest_h: Keyspace,
    vaest_h: Keyspace,
    txlog: Keyspace,
} //todo: multi reader/single writer affirming tests

impl Db {
    /// Returns a read-only view of this database as it existed at the moment after `txn` was appended.
    /// In other words, it returns all transactions before and including the cutoff.
    pub fn at(self: &Arc<Self>, txn: TxnId) -> Arc<Self> {
        Arc::new(Self {
            txn: self.txn,
            as_of: Some(txn),
            snapshot: self.snapshot.clone(),
            attrs: self.attrs.clone(),
            easvt: self.easvt.clone(),
            aesvt: self.aesvt.clone(),
            avest: self.avest.clone(),
            vaest: self.vaest.clone(),
            easvt_h: self.easvt_h.clone(),
            aesvt_h: self.aesvt_h.clone(),
            avest_h: self.avest_h.clone(),
            vaest_h: self.vaest_h.clone(),
            txlog: self.txlog.clone(),
        })
    }

    /// Returns an up-to-date snapshot as it existed after the most recent transaction.
    /// This returns an identical clone if the original self was already the latest snapshot.
    pub fn newest(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self {
            txn: self.txn,
            as_of: None,
            snapshot: self.snapshot.clone(),
            attrs: self.attrs.clone(),
            easvt: self.easvt.clone(),
            aesvt: self.aesvt.clone(),
            avest: self.avest.clone(),
            vaest: self.vaest.clone(),
            easvt_h: self.easvt_h.clone(),
            aesvt_h: self.aesvt_h.clone(),
            avest_h: self.avest_h.clone(),
            vaest_h: self.vaest_h.clone(),
            txlog: self.txlog.clone(),
        })
    }

    pub fn at_time(self: &Arc<Self>, time: SystemTime) -> Result<Option<Arc<Self>>> {
        if let Some(cutoff) = self.txn_at(time)? {
            Ok(Some(self.at(cutoff)))
        } else {
            Ok(None)
        }
    }

    pub fn at_time_else_current(self: &Arc<Self>, time: SystemTime) -> Result<Arc<Self>> {
        if let Some(cutoff) = self.txn_at(time)? {
            Ok(self.at(cutoff))
        } else {
            Ok(self.clone())
        }
    }

    pub fn attribute_def(&self, id: AttributeId) -> Result<AttributeDef> {
        let payload = self
            .snapshot
            .get(&self.attrs, by_attr_key(id))?
            .ok_or_else(|| Error::UnknownAttribute(format!("{id:?}")))?;

        if payload.len() < 4 {
            return Err(Error::CorruptKey {
                expected: 4,
                got: payload.len(),
            });
        }
        let key_type = match payload[payload.len() - 1] {
            0x00 => None,
            tag => Some(type_for_tag(tag)?),
        };
        let kind = CollectionKind::try_from(payload[payload.len() - 2])?;
        let flags = payload[payload.len() - 3];
        let indexed = flags & 1 != 0;
        let is_component = flags & 0b10 != 0;

        let value_type = type_for_tag(payload[payload.len() - 4])?;

        let name = String::from_utf8(payload[..payload.len() - 4].to_vec())
            .map_err(|_| Error::CorruptPrimitive(Type::String))?;

        Ok(AttributeDef {
            name,
            value_type,
            kind,
            key_type,
            indexed,
            is_component,
        })
    }

    pub fn named_attribute_def(&self, name: &str) -> Result<Option<AttributeDef>> {
        let by_name_key = by_name_key(name);
        if let Some(id_bytes) = self.attrs.get(&by_name_key)? {
            let id = AttributeId::try_from(id_bytes.as_ref())?;
            Ok(Some(self.attribute_def(id)?))
        } else {
            Ok(None)
        }
    }

    pub fn attribute_id(&self, name: &str) -> Result<Option<AttributeId>> {
        let by_name_key = by_name_key(name);
        if let Some(id_bytes) = self.attrs.get(&by_name_key)? {
            Ok(Some(AttributeId::try_from(id_bytes.as_ref())?))
        } else {
            Ok(None)
        }
    }
}

pub struct Store {
    pub(crate) db: Database,
    pub(crate) meta: Keyspace,
    pub(crate) attrs: Keyspace,
    pub(crate) easvt: Keyspace,
    pub(crate) aesvt: Keyspace,
    pub(crate) avest: Keyspace,
    pub(crate) vaest: Keyspace,
    pub(crate) easvt_h: Keyspace,
    pub(crate) aesvt_h: Keyspace,
    pub(crate) avest_h: Keyspace,
    pub(crate) vaest_h: Keyspace,
    pub(crate) txlog: Keyspace,
    pub(crate) current: ArcSwap<Db>,
    pub(crate) transact_lock: Mutex<()>,
}

const BY_NAME_PREFIX: u8 = 0x01; // -> attr id
const BY_ATTR_PREFIX: u8 = 0x02; // -> name, tag, flags

#[inline]
fn by_attr_key(attr_id: AttributeId) -> Vec<u8> {
    let mut by_id_key = Vec::with_capacity(1 + size_of::<AttributeId>());
    by_id_key.push(BY_ATTR_PREFIX);
    by_id_key.extend_from_slice(&attr_id.to_be_bytes());
    by_id_key
}

#[inline]
fn by_attr_value(def: &AttributeDef) -> Vec<u8> {
    let mut by_id_value = Vec::with_capacity(def.name.len() + 4);
    by_id_value.extend_from_slice(def.name.as_bytes());
    by_id_value.push(tag_for_type(def.value_type));
    by_id_value.push(def.indexed as u8 | (def.is_component as u8) << 1);
    by_id_value.push(def.kind.to_byte());
    by_id_value.push(def.key_type.map_or(0x00, tag_for_type));
    by_id_value
}

#[inline]
fn by_name_key(name: &str) -> Vec<u8> {
    let mut by_name_key = Vec::with_capacity(1 + name.len());
    by_name_key.push(BY_NAME_PREFIX);
    by_name_key.extend_from_slice(name.as_bytes());
    by_name_key
}

#[inline]
fn by_name_value(attr_id: AttributeId) -> [u8; size_of::<AttributeId>()] {
    attr_id.to_be_bytes()
}

impl Store {
    pub fn db(&self) -> Arc<Db> {
        self.current.load().clone()
    }

    /// Returns a read-only database as it existed at the moment after `txn` was appended.
    /// In other words, it returns all transactions before and including the cutoff.
    pub fn db_at(&self, txn: TxnId) -> Arc<Db> {
        self.current.load().at(txn)
    }

    pub fn db_at_time(&self, time: SystemTime) -> Result<Option<Arc<Db>>> {
        self.current.load().at_time(time)
    }

    pub(crate) fn swap(&self, txn: TxnId) {
        self.current.store(
            Db {
                txn,
                as_of: None,
                snapshot: self.db.snapshot(),
                attrs: self.attrs.clone(),
                easvt: self.easvt.clone(),
                aesvt: self.aesvt.clone(),
                avest: self.avest.clone(),
                vaest: self.vaest.clone(),
                easvt_h: self.easvt_h.clone(),
                aesvt_h: self.aesvt_h.clone(),
                avest_h: self.avest_h.clone(),
                vaest_h: self.vaest_h.clone(),
                txlog: self.txlog.clone(),
            }
            .into(),
        );
    }

    pub(crate) fn refresh(&self) {
        self.swap(self.current.load().txn);
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::builder(path).open()?;
        let meta = db.keyspace("meta", KeyspaceCreateOptions::default)?;
        let attrs = db.keyspace("attrs", KeyspaceCreateOptions::default)?;
        let easvt = db.keyspace("easvt", KeyspaceCreateOptions::default)?;
        let aesvt = db.keyspace("aesvt", KeyspaceCreateOptions::default)?;
        let avest = db.keyspace("avest", KeyspaceCreateOptions::default)?;
        let vaest = db.keyspace("vaest", KeyspaceCreateOptions::default)?;
        let easvt_h = db.keyspace("easvt_h", KeyspaceCreateOptions::default)?;
        let aesvt_h = db.keyspace("aesvt_h", KeyspaceCreateOptions::default)?;
        let avest_h = db.keyspace("avest_h", KeyspaceCreateOptions::default)?;
        let vaest_h = db.keyspace("vaest_h", KeyspaceCreateOptions::default)?;
        let txlog = db.keyspace("txlog", KeyspaceCreateOptions::default)?;
        let snapshot = ArcSwap::from_pointee(Db {
            txn: TxnId::with_value(0),
            as_of: None,
            snapshot: db.snapshot(),
            attrs: attrs.clone(),
            easvt: easvt.clone(),
            aesvt: aesvt.clone(),
            avest: avest.clone(),
            vaest: vaest.clone(),
            easvt_h: easvt_h.clone(),
            aesvt_h: aesvt_h.clone(),
            avest_h: avest_h.clone(),
            vaest_h: vaest_h.clone(),
            txlog: txlog.clone(),
        });
        let store = Store {
            db,
            meta,
            attrs,
            easvt,
            aesvt,
            avest,
            vaest,
            easvt_h,
            aesvt_h,
            avest_h,
            vaest_h,
            txlog,
            current: snapshot,
            transact_lock: Mutex::new(()),
        };
        store.sync_system_attributes()?;
        Ok(store)
    }

    fn sync_system_attributes(&self) -> Result<()> {
        let system_attrs = [
            AttributeDef {
                name: "db/txTimestamp".into(),
                value_type: Type::Timestamp,
                kind: CollectionKind::One,
                key_type: None,
                indexed: true,
                is_component: false,
            },
            AttributeDef {
                name: "db/txMove".into(),
                value_type: Type::Bytes,
                kind: CollectionKind::Many,
                key_type: None,
                indexed: false,
                is_component: false,
            },
            AttributeDef {
                name: "db/txKind".into(),
                value_type: Type::String,
                kind: CollectionKind::One,
                key_type: None,
                indexed: false,
                is_component: false,
            },
        ];
        let ids = [TX_TIMESTAMP_ATTR, TX_MOVE_ATTR, TX_KIND_ATTR];
        for (id, def) in ids.into_iter().zip(system_attrs) {
            let by_id_key = by_attr_key(id);
            if self.attrs.get(&by_id_key)?.is_some() {
                continue;
            }
            self.attrs
                .insert(by_name_key(&def.name), by_name_value(id))?;
            self.attrs.insert(by_id_key, by_attr_value(&def))?;
        }
        Ok(())
    }

    pub fn declare_attribute(&self, def: AttributeDef) -> Result<AttributeId> {
        let _guard = self.transact_lock.lock();
        let valid_key_type = match def.kind {
            CollectionKind::Map => {
                matches!(def.key_type, Some(Type::String | Type::Long | Type::Ref))
            }
            _ => def.key_type.is_none(),
        };
        if !valid_key_type {
            return Err(Error::InvalidKeyType(def.name));
        }
        if def.is_component && def.value_type != Type::Ref {
            return Err(Error::InvalidSchema(def.name));
        }

        let by_name_key = by_name_key(&def.name);

        let attribute_id = if let Some(id_bytes) = self.attrs.get(&by_name_key)? {
            let id = AttributeId::new(u16::from_be_bytes(id_bytes[..2].try_into().unwrap()));
            let existing = self.attribute_def(id)?;
            if existing.value_type != def.value_type {
                return Err(Error::SchemaConflict(def.name));
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
        self.attrs.insert(by_id_key, by_attr_value(&def))?;
        self.refresh();
        Ok(attribute_id)
    }

    pub fn attribute_def(&self, id: AttributeId) -> Result<AttributeDef> {
        let payload = self
            .attrs
            .get(by_attr_key(id))?
            .ok_or_else(|| Error::UnknownAttribute(format!("{id:?}")))?;

        if payload.len() < 4 {
            return Err(Error::CorruptKey {
                expected: 4,
                got: payload.len(),
            });
        }
        let key_type = match payload[payload.len() - 1] {
            0x00 => None,
            tag => Some(type_for_tag(tag)?),
        };
        let kind = CollectionKind::try_from(payload[payload.len() - 2])?;
        let flags = payload[payload.len() - 3];
        let indexed = flags & 1 != 0;
        let is_component = flags & 0b10 != 0;

        let value_type = type_for_tag(payload[payload.len() - 4])?;

        let name = String::from_utf8(payload[..payload.len() - 4].to_vec())
            .map_err(|_| Error::CorruptPrimitive(Type::String))?;

        Ok(AttributeDef {
            name,
            value_type,
            kind,
            key_type,
            indexed,
            is_component,
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
        let _guard = self.transact_lock.lock();
        self.refresh();
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

    #[test]
    fn declare_and_reload_def() {
        let (_dir, store) = temp_store();
        let id = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
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
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let second = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn declare_conflicting_type_errors() {
        let (_dir, store) = temp_store();
        store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let result = store.declare_attribute(def("person/name", Type::Long, CollectionKind::One));
        assert!(matches!(result, Err(Error::SchemaConflict(_))));
    }

    #[test]
    fn registry_survives_reopen() {
        let (dir, store) = temp_store();
        let schema = AttributeDef {
            indexed: true,
            ..def("person/age", Type::Long, CollectionKind::One)
        };
        let id = store.declare_attribute(schema.clone()).unwrap();
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        let again = reopened.declare_attribute(schema).unwrap();
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
        assert!(store.db().entity_factoms(entity).unwrap().is_empty());
    }
}
