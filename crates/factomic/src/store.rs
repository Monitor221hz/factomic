use std::alloc::System;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use arc_swap::ArcSwap;
use fjall::{Database, Keyspace, KeyspaceCreateOptions, OwnedWriteBatch, PersistMode, Readable};
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

const FORMAT_VERSION: u64 = 1;
const META_FORMAT_VERSION: &[u8] = b"format_version";
const META_NEXT_ATTR: &[u8; 9] = b"next_attr";
const META_ENTITY_PREFIX: u8 = 0x02;

#[derive(Clone)]
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
        let Some(id) = self.attribute_id(name)? else {
            return Ok(None);
        };
        Ok(Some(self.attribute_def(id)?))
    }

    pub fn attribute_id(&self, name: &str) -> Result<Option<AttributeId>> {
        let by_name_key = by_name_key(name);
        if let Some(id_bytes) = self.snapshot.get(&self.attrs, by_name_key)? {
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

    pub(crate) fn durable_batch(&self, capacity: usize) -> OwnedWriteBatch {
        OwnedWriteBatch::with_capacity(self.db.clone(), capacity)
            .durability(Some(PersistMode::SyncAll))
    }

    pub(crate) fn persist_durable(&self) -> Result<()> {
        self.db.persist(PersistMode::SyncAll)?;
        Ok(())
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::builder(path).open()?;
        let meta = db.keyspace("meta", KeyspaceCreateOptions::default)?;
        let initialize_format = if let Some(version) = meta.get(META_FORMAT_VERSION)? {
            let version = u64::from_be_bytes(version.as_ref().try_into()?);
            if version != FORMAT_VERSION {
                return Err(Error::UnsupportedFormatVersion(version));
            }
            false
        } else {
            true
        };
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
        store.sync_system_attributes(initialize_format)?;
        Ok(store)
    }

    fn sync_system_attributes(&self, initialize_format: bool) -> Result<()> {
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
        let mut batch = self.durable_batch(1 + ids.len() * 2);
        if initialize_format {
            batch.insert(
                &self.meta,
                META_FORMAT_VERSION,
                FORMAT_VERSION.to_be_bytes(),
            );
        }
        for (id, def) in ids.into_iter().zip(system_attrs) {
            let by_name_key = by_name_key(&def.name);
            let by_id_key = by_attr_key(id);
            let name_mapping = self.attrs.get(&by_name_key)?;
            let definition = self.attrs.get(&by_id_key)?;
            if let Some(id_bytes) = &name_mapping {
                if AttributeId::try_from(id_bytes.as_ref())? != id {
                    return Err(Error::SchemaConflict(def.name));
                }
            }
            if definition.is_some() && self.attribute_def(id)? != def {
                return Err(Error::SchemaConflict(def.name));
            }
            if name_mapping.is_none() || definition.is_none() {
                batch.insert(&self.attrs, by_name_key, by_name_value(id));
                batch.insert(&self.attrs, by_id_key, by_attr_value(&def));
            }
        }
        batch.commit()?;
        self.refresh();
        Ok(())
    }

    pub fn declare_attribute(&self, def: AttributeDef) -> Result<AttributeId> {
        let [id] = self.declare_attributes([def])?;
        Ok(id)
    }

    pub fn declare_attributes<const N: usize>(
        &self,
        defs: [AttributeDef; N],
    ) -> Result<[AttributeId; N]> {
        let _guard = self.transact_lock.lock();
        for def in &defs {
            let valid_key_type = match def.kind {
                CollectionKind::Map => {
                    matches!(def.key_type, Some(Type::String | Type::Long | Type::Ref))
                }
                _ => def.key_type.is_none(),
            };
            if !valid_key_type {
                return Err(Error::InvalidKeyType(def.name.clone()));
            }
            if def.is_component && def.value_type != Type::Ref {
                return Err(Error::InvalidSchema(def.name.clone()));
            }
        }

        let mut next_id = self.meta.get(META_NEXT_ATTR)?.map_or_else(
            || AttributeId::new(FIRST_USER_ATTR),
            |id| AttributeId::new(u16::from_be_bytes(id[..2].try_into().unwrap())),
        );
        let initial_next_id = next_id;
        let mut pending = std::collections::HashMap::new();
        let mut ids = Vec::with_capacity(N);
        let mut batch = self.durable_batch(1 + N * 2);

        for def in defs {
            if let Some((id, pending_def)) = pending.get(&def.name) {
                if pending_def != &def {
                    return Err(Error::SchemaConflict(def.name));
                }
                ids.push(*id);
                continue;
            }

            let by_name_key = by_name_key(&def.name);
            let id = if let Some(id_bytes) = self.attrs.get(&by_name_key)? {
                let id = AttributeId::try_from(id_bytes.as_ref())?;
                let existing = self.attribute_def(id)?;
                if existing != def {
                    return Err(Error::SchemaConflict(def.name));
                }
                id
            } else {
                let id = next_id;
                next_id = AttributeId::new(next_id.as_u16() + 1);
                batch.insert(&self.attrs, &by_name_key, by_name_value(id));
                batch.insert(&self.attrs, by_attr_key(id), by_attr_value(&def));
                pending.insert(def.name.clone(), (id, def.clone()));
                id
            };
            ids.push(id);
        }

        if next_id != initial_next_id {
            batch.insert(&self.meta, META_NEXT_ATTR, next_id.to_be_bytes());
        }
        batch.commit()?;
        self.refresh();
        Ok(ids.try_into().unwrap())
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
        let id = self.alloc_entity_id(USER_PARTITION)?;
        self.persist_durable()?;
        Ok(id)
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

    fn format_version(store: &Store) -> u64 {
        u64::from_be_bytes(
            store
                .meta
                .get(META_FORMAT_VERSION)
                .unwrap()
                .unwrap()
                .as_ref()
                .try_into()
                .unwrap(),
        )
    }

    #[test]
    fn open_stamps_missing_format_version() {
        let dir = TempDir::new().unwrap();
        {
            let db = Database::builder(dir.path()).open().unwrap();
            db.keyspace("meta", KeyspaceCreateOptions::default).unwrap();
        }
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(format_version(&store), FORMAT_VERSION);
    }

    #[test]
    fn reopen_preserves_format_version() {
        let (dir, store) = temp_store();
        assert_eq!(format_version(&store), FORMAT_VERSION);
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(format_version(&reopened), FORMAT_VERSION);
    }

    #[test]
    fn open_rejects_unsupported_format_version() {
        let dir = TempDir::new().unwrap();
        {
            let db = Database::builder(dir.path()).open().unwrap();
            let meta = db.keyspace("meta", KeyspaceCreateOptions::default).unwrap();
            meta.insert(META_FORMAT_VERSION, 2_u64.to_be_bytes())
                .unwrap();
        }
        let result = Store::open(dir.path());
        assert!(matches!(result, Err(Error::UnsupportedFormatVersion(2))));
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
    fn system_attributes_are_visible_immediately_after_open() {
        let (_dir, store) = temp_store();
        let db = store.db();
        for name in ["db/txTimestamp", "db/txMove", "db/txKind"] {
            assert!(db.named_attribute_def(name).unwrap().is_some());
        }
    }

    #[test]
    fn declare_attributes_is_atomic_across_snapshots_and_reopen() {
        let (dir, store) = temp_store();
        let historical = store.db();
        let definitions = [
            def("person/name", Type::String, CollectionKind::One),
            AttributeDef {
                indexed: true,
                ..def("person/age", Type::Long, CollectionKind::One)
            },
        ];

        let [name, age] = store.declare_attributes(definitions.clone()).unwrap();
        assert_eq!(name.as_u16(), FIRST_USER_ATTR);
        assert_eq!(age.as_u16(), FIRST_USER_ATTR + 1);
        for definition in &definitions {
            assert_eq!(historical.attribute_id(&definition.name).unwrap(), None);
        }
        let current = store.db();
        assert_eq!(current.attribute_id("person/name").unwrap(), Some(name));
        assert_eq!(current.attribute_id("person/age").unwrap(), Some(age));
        assert_eq!(
            current.named_attribute_def("person/age").unwrap(),
            Some(definitions[1].clone())
        );

        drop(current);
        drop(historical);
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(
            reopened.declare_attributes(definitions).unwrap(),
            [name, age]
        );
        let next = reopened
            .declare_attribute(def("person/nickname", Type::String, CollectionKind::One))
            .unwrap();
        assert_eq!(next.as_u16(), FIRST_USER_ATTR + 2);
    }

    #[test]
    fn declare_attributes_late_validation_failure_writes_nothing() {
        let (_dir, store) = temp_store();
        let result = store.declare_attributes([
            def("person/name", Type::String, CollectionKind::One),
            AttributeDef {
                key_type: Some(Type::String),
                ..def("person/invalid", Type::String, CollectionKind::One)
            },
        ]);
        assert!(matches!(result, Err(Error::InvalidKeyType(name)) if name == "person/invalid"));
        assert_eq!(store.db().attribute_id("person/name").unwrap(), None);
        let next = store
            .declare_attribute(def("person/next", Type::String, CollectionKind::One))
            .unwrap();
        assert_eq!(next.as_u16(), FIRST_USER_ATTR);
    }

    #[test]
    fn declare_attributes_late_conflict_writes_nothing() {
        let (_dir, store) = temp_store();
        let existing = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let result = store.declare_attributes([
            def("person/fresh", Type::String, CollectionKind::One),
            def("person/name", Type::Long, CollectionKind::One),
        ]);
        assert!(matches!(result, Err(Error::SchemaConflict(name)) if name == "person/name"));
        assert_eq!(store.db().attribute_id("person/fresh").unwrap(), None);
        assert_eq!(
            store.db().attribute_id("person/name").unwrap(),
            Some(existing)
        );
        let next = store
            .declare_attribute(def("person/next", Type::String, CollectionKind::One))
            .unwrap();
        assert_eq!(next.as_u16(), existing.as_u16() + 1);
    }

    #[test]
    fn declare_attributes_identical_duplicates_allocate_once() {
        let (_dir, store) = temp_store();
        let definition = def("person/name", Type::String, CollectionKind::One);
        let [first, second] = store
            .declare_attributes([definition.clone(), definition])
            .unwrap();
        assert_eq!(first, second);
        let next = store
            .declare_attribute(def("person/next", Type::String, CollectionKind::One))
            .unwrap();
        assert_eq!(next.as_u16(), first.as_u16() + 1);
    }

    #[test]
    fn declare_attributes_conflicting_duplicates_write_nothing() {
        let (_dir, store) = temp_store();
        let result = store.declare_attributes([
            def("person/name", Type::String, CollectionKind::One),
            def("person/name", Type::Long, CollectionKind::One),
        ]);
        assert!(matches!(result, Err(Error::SchemaConflict(name)) if name == "person/name"));
        assert_eq!(store.db().attribute_id("person/name").unwrap(), None);
        let next = store
            .declare_attribute(def("person/next", Type::String, CollectionKind::One))
            .unwrap();
        assert_eq!(next.as_u16(), FIRST_USER_ATTR);
    }

    #[test]
    fn named_attribute_def_uses_one_snapshot() {
        let (_dir, store) = temp_store();
        let historical = store.db();
        let schema = AttributeDef {
            indexed: true,
            ..def("person/name", Type::String, CollectionKind::One)
        };

        assert_eq!(historical.named_attribute_def(&schema.name).unwrap(), None);

        let id = store.declare_attribute(schema.clone()).unwrap();

        assert_eq!(historical.attribute_id(&schema.name).unwrap(), None);
        assert_eq!(historical.named_attribute_def(&schema.name).unwrap(), None);

        let current = store.db();
        assert_eq!(current.attribute_id(&schema.name).unwrap(), Some(id));
        assert_eq!(
            current.named_attribute_def(&schema.name).unwrap(),
            Some(schema)
        );
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
    fn declare_conflicting_non_value_type_errors() {
        let (_dir, store) = temp_store();
        store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let result = store.declare_attribute(AttributeDef {
            indexed: true,
            ..def("person/name", Type::String, CollectionKind::One)
        });
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
        assert!(
            store
                .db()
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );
    }
}
