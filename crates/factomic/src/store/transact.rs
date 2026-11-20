use fjall::OwnedWriteBatch;
use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::factom::{ASSERT, CollectionKind, MapKey, Op, RETRACT, Slot};
use crate::id::{AttributeId, EntityId, TX_KIND_ATTR, TX_MOVE_ATTR, TX_TIMESTAMP_ATTR, TxnId};
use crate::keys;
use crate::primitive::{DecodePrimitive, EncodePrimitive};
use crate::store::{AttributeDef, Store};
use crate::value::{TAG_REF, Type, Value, region};

fn check_type(store: &Store, attr: AttributeId, ty: Type) -> Result<AttributeDef> {
    let def = store.attribute_def(attr)?;
    if def.value_type != ty {
        return Err(Error::TypeMismatch {
            attribute: def.name.clone(),
            expected: def.value_type,
            got: ty,
        });
    }
    Ok(def)
}

fn ref_target(region: &[u8]) -> Result<Option<EntityId>> {
    if region[0] == TAG_REF {
        Ok(Some(EntityId::decode(&region[1..])?))
    } else {
        Ok(None)
    }
}

fn check_key(def: &AttributeDef, slot: &Slot) -> Result<()> {
    match def.kind {
        CollectionKind::One | CollectionKind::Many => {
            if !matches!(slot, Slot::Empty) {
                return Err(Error::UnexpectedKey(def.name.clone()));
            }
        }
        CollectionKind::List => match slot {
            Slot::Empty => return Err(Error::RequiresKey(def.name.clone())),
            Slot::Position(_) => {}
            Slot::Key(_) => return Err(Error::MismatchedKey(def.name.clone())),
        },
        CollectionKind::Map => match slot {
            Slot::Empty => return Err(Error::RequiresKey(def.name.clone())),
            Slot::Position(_) => return Err(Error::MismatchedKey(def.name.clone())),
            Slot::Key(key) => {
                if Some(key.value_type()) != def.key_type {
                    return Err(Error::MismatchedKey(def.name.clone()));
                }
            }
        },
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct FactomRef<'a> {
    entity: EntityId,
    attr: AttributeId,
    slot: &'a Slot,
    region: &'a [u8],
}

impl Store {
    fn insert_current(
        &self,
        batch: &mut OwnedWriteBatch,
        def: &AttributeDef,
        factom: FactomRef<'_>,
        txn: TxnId,
    ) -> Result<()> {
        let (entity, attr, k, region) = (factom.entity, factom.attr, factom.slot, factom.region);
        let payload = txn.to_be_bytes();
        batch.insert(
            &self.easvt,
            keys::easvt_key(entity, attr, k, region),
            payload,
        );
        batch.insert(
            &self.aesvt,
            keys::aesvt_key(entity, attr, k, region),
            payload,
        );
        if def.indexed {
            batch.insert(
                &self.avest,
                keys::avest_key(entity, attr, k, region),
                payload,
            );
        }
        if let Some(target) = ref_target(region)? {
            batch.insert(
                &self.vaest,
                keys::vaest_key(target, attr, entity, k),
                payload,
            );
        }
        Ok(())
    }

    fn remove_current(
        &self,
        batch: &mut OwnedWriteBatch,
        def: &AttributeDef,
        factom: FactomRef<'_>,
    ) -> Result<()> {
        let (entity, attr, k, region) = (factom.entity, factom.attr, factom.slot, factom.region);
        batch.remove(&self.easvt, keys::easvt_key(entity, attr, k, region));
        batch.remove(&self.aesvt, keys::aesvt_key(entity, attr, k, region));
        if def.indexed {
            batch.remove(&self.avest, keys::avest_key(entity, attr, k, region));
        }
        if let Some(target) = ref_target(region)? {
            batch.remove(&self.vaest, keys::vaest_key(target, attr, entity, k));
        }
        Ok(())
    }
    /// All up-to-date assertions exist in the current indexes and the txn log only.
    ///
    /// When an eviction (retraction) happens we backfill history with the original assertion using the same original txn id.
    ///
    /// The naive approach would be to write assert to history on first assertion and write only retract during eviction but then every factom would exist in both current and history indexes, so we should never do this.
    fn write_history(
        &self,
        batch: &mut OwnedWriteBatch,
        def: &AttributeDef,
        factom: FactomRef<'_>,
        asserted_at: TxnId,
        retracted_at: TxnId,
    ) -> Result<()> {
        let (entity, attr, k, region) = (factom.entity, factom.attr, factom.slot, factom.region);
        for (keyspace, key_fn) in [
            (
                &self.easvt_h,
                keys::easvt_history_key as fn(_, _, _, _, _) -> _,
            ),
            (&self.aesvt_h, keys::aesvt_history_key),
        ] {
            batch.insert(
                keyspace,
                key_fn(entity, attr, k, region, asserted_at),
                [ASSERT],
            );
            batch.insert(
                keyspace,
                key_fn(entity, attr, k, region, retracted_at),
                [RETRACT],
            );
        }
        if def.indexed {
            batch.insert(
                &self.avest_h,
                keys::avest_history_key(entity, attr, k, region, asserted_at),
                [ASSERT],
            );
            batch.insert(
                &self.avest_h,
                keys::avest_history_key(entity, attr, k, region, retracted_at),
                [RETRACT],
            );
        }
        if let Some(target) = ref_target(region)? {
            batch.insert(
                &self.vaest_h,
                keys::vaest_history_key(target, attr, entity, k, asserted_at),
                [ASSERT],
            );
            batch.insert(
                &self.vaest_h,
                keys::vaest_history_key(target, attr, entity, k, retracted_at),
                [RETRACT],
            );
        }
        Ok(())
    }

    fn record_tx_kind(&self, batch: &mut OwnedWriteBatch, txn: TxnId, kind: &str) {
        let region = region(&kind);
        let entity = txn.entity_id();
        let payload = txn.to_be_bytes();
        batch.insert(
            &self.easvt,
            keys::easvt_key(entity, TX_KIND_ATTR, &Slot::Empty, &region),
            payload,
        );
        batch.insert(
            &self.aesvt,
            keys::aesvt_key(entity, TX_KIND_ATTR, &Slot::Empty, &region),
            payload,
        );
        batch.insert(
            &self.txlog,
            keys::txlog_key(txn, entity, TX_KIND_ATTR, &Slot::Empty, &region),
            [ASSERT],
        );
    }

    fn record_tx_move(
        &self,
        batch: &mut OwnedWriteBatch,
        txn: TxnId,
        attr: AttributeId,
        from: &Slot,
        to: &Slot,
    ) {
        let mut bytes = Vec::with_capacity(2 + from.encoded_len() + to.encoded_len());
        bytes.extend_from_slice(&attr.to_be_bytes());
        from.encode_key(&mut bytes);
        to.encode_key(&mut bytes);
        let region = region(&bytes.as_slice());
        let entity = txn.entity_id();
        let payload = txn.to_be_bytes();
        batch.insert(
            &self.easvt,
            keys::easvt_key(entity, TX_MOVE_ATTR, &Slot::Empty, &region),
            payload,
        );
        batch.insert(
            &self.aesvt,
            keys::aesvt_key(entity, TX_MOVE_ATTR, &Slot::Empty, &region),
            payload,
        );
        batch.insert(
            &self.txlog,
            keys::txlog_key(txn, entity, TX_MOVE_ATTR, &Slot::Empty, &region),
            [ASSERT],
        );
    }

    fn record_tx_timestamp(&self, batch: &mut OwnedWriteBatch, txn: TxnId) {
        let region = region(&Timestamp::now());
        let entity = txn.entity_id();
        let payload = txn.to_be_bytes();
        batch.insert(
            &self.easvt,
            keys::easvt_key(entity, TX_TIMESTAMP_ATTR, &Slot::Empty, &region),
            payload,
        );
        batch.insert(
            &self.aesvt,
            keys::aesvt_key(entity, TX_TIMESTAMP_ATTR, &Slot::Empty, &region),
            payload,
        );
        batch.insert(
            &self.txlog,
            keys::txlog_key(txn, entity, TX_TIMESTAMP_ATTR, &Slot::Empty, &region),
            [ASSERT],
        );
        batch.insert(
            &self.avest,
            keys::avest_key(entity, TX_TIMESTAMP_ATTR, &Slot::Empty, &region),
            payload,
        );
    }
}

mod builder;

pub use builder::Transaction;

impl Store {
    pub fn assert<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact().assert(entity, attr, value).commit()
    }

    pub fn assert_value(
        &self,
        entity: EntityId,
        attr: AttributeId,
        value: &Value,
    ) -> Result<Option<TxnId>> {
        self.transact().assert_value(entity, attr, value).commit()
    }

    pub fn retract<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact().retract(entity, attr, value).commit()
    }

    pub fn retract_value(
        &self,
        entity: EntityId,
        attr: AttributeId,
        value: &Value,
    ) -> Result<Option<TxnId>> {
        self.transact().retract_value(entity, attr, value).commit()
    }

    pub fn assert_at<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact()
            .assert_at(entity, attr, slot, value)
            .commit()
    }

    pub fn retract_at<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact()
            .retract_at(entity, attr, slot, value)
            .commit()
    }

    pub fn push<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact().push(entity, attr, value).commit()
    }

    pub fn prepend<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact().prepend(entity, attr, value).commit()
    }

    pub fn remove(&self, entity: EntityId, attr: AttributeId, slot: Slot) -> Result<Option<TxnId>> {
        self.transact().remove(entity, attr, slot).commit()
    }

    pub fn put<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        key: impl Into<MapKey>,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact()
            .put(entity, attr, key.into(), value)
            .commit()
    }

    pub fn remove_key(
        &self,
        entity: EntityId,
        attr: AttributeId,
        key: MapKey,
    ) -> Result<Option<TxnId>> {
        self.transact().remove_key(entity, attr, key).commit()
    }

    pub fn insert_before<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        anchor: Slot,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact()
            .insert_before(entity, attr, anchor, value)
            .commit()
    }

    pub fn insert_after<V: EncodePrimitive>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        anchor: Slot,
        value: &V,
    ) -> Result<Option<TxnId>> {
        self.transact()
            .insert_after(entity, attr, anchor, value)
            .commit()
    }

    pub fn move_slot(
        &self,
        entity: EntityId,
        attr: AttributeId,
        from: Slot,
        to: Slot,
    ) -> Result<Option<TxnId>> {
        self.transact().move_slot(entity, attr, from, to).commit()
    }

    pub fn rebalance(&self, entity: EntityId, attr: AttributeId) -> Result<Option<TxnId>> {
        self.transact().rebalance(entity, attr).commit()
    }

    pub fn retract_entity(&self, entity: EntityId) -> Result<Option<TxnId>> {
        self.transact().retract_entity(entity).commit()
    }

    pub fn revert_txn(&self, txn: TxnId) -> Result<Option<TxnId>> {
        let mut tx = self.transact();
        for factom in self.db().tx_factoms(txn)? {
            if factom.attribute() == TX_TIMESTAMP_ATTR {
                continue;
            }
            match factom.op() {
                Op::Assert => tx.retract_value_at(
                    factom.entity(),
                    factom.attribute(),
                    factom.slot().clone(),
                    factom.value(),
                ),
                Op::Retract => tx.assert_value_at(
                    factom.entity(),
                    factom.attribute(),
                    factom.slot().clone(),
                    factom.value(),
                ),
            };
        }
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factom::{Factom, MapKey};
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
    fn assert_and_load_entity() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn = store.assert(e, name, &"alice").unwrap().unwrap();
        let factoms = store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(factoms.len(), 1);
        assert_eq!(factoms[0].entity(), e);
        assert_eq!(factoms[0].attribute(), name);
        assert_eq!(factoms[0].value(), &Value::String("alice".into()));
        assert_eq!(factoms[0].txn(), txn);
        assert_eq!(factoms[0].op(), Op::Assert);
    }

    #[test]
    fn noop_operations_return_none() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"alice").unwrap().unwrap();

        assert_eq!(store.assert(e, name, &"alice").unwrap(), None);
        assert_eq!(store.retract(e, name, &"nobody").unwrap(), None);
        let result = store
            .transact()
            .assert(e, name, &"alice")
            .retract(e, name, &"bob")
            .commit()
            .unwrap();

        assert_eq!(result, None);
        assert_eq!(
            store
                .db()
                .entity_factoms(e)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn overwrite_moves_old_value_to_history() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store.assert(e, name, &"alice").unwrap().unwrap();
        let txn2 = store.assert(e, name, &"bob").unwrap().unwrap();
        let current = store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].value(), &Value::String("bob".into()));
        let history = store.db().entity_history(e).flatten().collect::<Vec<_>>();
        assert!(history.contains(&Factom::new(
            e,
            name,
            Slot::Empty,
            Value::String("alice".into()),
            txn1,
            Op::Assert
        )));
        assert!(history.contains(&Factom::new(
            e,
            name,
            Slot::Empty,
            Value::String("alice".into()),
            txn2,
            Op::Retract
        )));
    }

    #[test]
    fn retract_removes_and_records_history() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store.assert(e, name, &"alice").unwrap().unwrap();
        let txn2 = store.retract(e, name, &"alice").unwrap().unwrap();
        assert!(
            store
                .db()
                .entity_factoms(e)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );
        let history = store.db().entity_history(e).flatten().collect::<Vec<_>>();
        assert!(history.contains(&Factom::new(
            e,
            name,
            Slot::Empty,
            Value::String("alice".into()),
            txn1,
            Op::Assert
        )));
        assert!(history.contains(&Factom::new(
            e,
            name,
            Slot::Empty,
            Value::String("alice".into()),
            txn2,
            Op::Retract
        )));
    }

    #[test]
    fn type_mismatch_is_rejected() {
        let (_dir, store) = temp_store();
        let age = store
            .declare_attribute(def("person/age", Type::Long, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let result = store.assert(e, age, &"thirty");
        assert!(matches!(result, Err(Error::TypeMismatch { .. })));
    }

    #[test]
    fn indexed_attr_supports_value_lookup() {
        let (_dir, store) = temp_store();
        let age = store
            .declare_attribute(AttributeDef {
                indexed: true,
                ..def("person/age", Type::Long, CollectionKind::One)
            })
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, age, &30i64).unwrap();
        assert_eq!(
            store.db().entities_with_value(age, &30i64).unwrap(),
            vec![e]
        );
        assert!(
            store
                .db()
                .entities_with_value(age, &31i64)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn ref_attr_supports_reverse_lookup() {
        let (_dir, store) = temp_store();
        let friend = store
            .declare_attribute(def("person/friend", Type::Ref, CollectionKind::One))
            .unwrap();
        let a = store.new_entity().unwrap();
        let b = store.new_entity().unwrap();
        store.assert(a, friend, &b).unwrap();
        assert_eq!(store.db().referring_entities(b, friend).unwrap(), vec![a]);
        assert!(store.db().referring_entities(a, friend).unwrap().is_empty());
    }

    #[test]
    fn txlog_records_the_whole_transaction() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn = store.assert(e, name, &"alice").unwrap().unwrap();
        let factoms = store.db().tx_factoms(txn).unwrap();
        assert_eq!(factoms.len(), 2);
        assert!(
            factoms
                .iter()
                .any(|d| d.entity() == e && d.attribute() == name && d.op() == Op::Assert)
        );
        assert!(
            factoms
                .iter()
                .any(|d| d.entity() == txn.entity_id() && d.attribute() == TX_TIMESTAMP_ATTR)
        );
    }

    #[test]
    fn revert_removes_asserted_value() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store.assert(e, name, &"alice").unwrap().unwrap();

        let reverted = store.revert_txn(txn1).unwrap().unwrap();
        assert!(
            store
                .db()
                .entity_factoms(e)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );

        let history = store.db().entity_history(e).flatten().collect::<Vec<_>>();
        assert!(history.contains(&Factom::new(
            e,
            name,
            Slot::Empty,
            Value::String("alice".into()),
            txn1,
            Op::Assert
        )));
        assert!(history.contains(&Factom::new(
            e,
            name,
            Slot::Empty,
            Value::String("alice".into()),
            reverted,
            Op::Retract
        )));
    }

    #[test]
    fn revert_overwrite_restores_previous_value() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"alice").unwrap().unwrap();
        let txn2 = store.assert(e, name, &"bob").unwrap().unwrap();

        store.revert_txn(txn2).unwrap();
        let current = store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].value(), &Value::String("alice".into()));
    }

    #[test]
    fn revert_retraction_restores_value() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"alice").unwrap().unwrap();
        let txn2 = store.retract(e, name, &"alice").unwrap().unwrap();

        store.revert_txn(txn2).unwrap();
        let current = store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].value(), &Value::String("alice".into()));
    }

    #[test]
    fn revert_keeps_tx_timestamp() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store.assert(e, name, &"alice").unwrap().unwrap();

        store.revert_txn(txn1).unwrap();
        assert_eq!(
            store
                .db()
                .entity_factoms(txn1.entity_id())
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn builder_commits_multiple_ops_as_one_txn() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let age = store
            .declare_attribute(def("person/age", Type::Long, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn = store
            .transact()
            .assert(e, name, &"alice")
            .assert(e, age, &30i64)
            .commit()
            .unwrap()
            .unwrap();
        let factoms = store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(factoms.len(), 2);
        assert!(factoms.iter().all(|f| f.txn() == txn));
        assert_eq!(store.db().tx_factoms(txn).unwrap().len(), 3);
    }

    #[test]
    fn builder_skips_noops_but_commits_effective_ops() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let age = store
            .declare_attribute(def("person/age", Type::Long, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"alice").unwrap().unwrap();
        let txn = store
            .transact()
            .assert(e, name, &"alice")
            .assert(e, age, &30i64)
            .commit()
            .unwrap()
            .unwrap();
        let logged = store.db().tx_factoms(txn).unwrap();
        assert_eq!(logged.len(), 2);
        assert!(
            logged
                .iter()
                .any(|f| f.attribute() == age && f.op() == Op::Assert)
        );
    }

    #[test]
    fn revert_multi_op_txn_is_one_atomic_txn() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let age = store
            .declare_attribute(def("person/age", Type::Long, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store
            .transact()
            .assert(e, name, &"alice")
            .assert(e, age, &30i64)
            .commit()
            .unwrap()
            .unwrap();
        let reverted = store.revert_txn(txn1).unwrap().unwrap();
        assert!(
            store
                .db()
                .entity_factoms(e)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );
        let logged = store.db().tx_factoms(reverted).unwrap();
        assert_eq!(logged.len(), 3);
        assert_eq!(logged.iter().filter(|f| f.op() == Op::Retract).count(), 2);
    }

    #[test]
    fn many_kind_keeps_all_values() {
        let (_dir, store) = temp_store();
        let tags = store
            .declare_attribute(def("person/tags", Type::String, CollectionKind::Many))
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, tags, &"a").unwrap().unwrap();
        store.assert(e, tags, &"b").unwrap().unwrap();
        assert_eq!(
            store
                .db()
                .entity_factoms(e)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(store.assert(e, tags, &"a").unwrap(), None);
        assert_eq!(
            store
                .db()
                .entity_factoms(e)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .len(),
            2
        );
        store.retract(e, tags, &"a").unwrap().unwrap();
        let factoms = store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(factoms.len(), 1);
        assert_eq!(factoms[0].value(), &Value::String("b".into()));
    }

    #[test]
    fn list_slots_hold_one_value_each() {
        let (_dir, store) = temp_store();
        let items = store
            .declare_attribute(def("person/items", Type::String, CollectionKind::List))
            .unwrap();
        let e = store.new_entity().unwrap();
        let (k1, k2) = (
            Slot::Position(b"a0".to_vec()),
            Slot::Position(b"a1".to_vec()),
        );
        store
            .assert_at(e, items, k1.clone(), &"a")
            .unwrap()
            .unwrap();
        store
            .assert_at(e, items, k2.clone(), &"b")
            .unwrap()
            .unwrap();
        store
            .assert_at(e, items, k1.clone(), &"c")
            .unwrap()
            .unwrap();
        let factoms = store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(factoms.len(), 2);
        assert_eq!(factoms[0].slot(), &k1);
        assert_eq!(factoms[0].value(), &Value::String("c".into()));
        assert_eq!(factoms[1].slot(), &k2);
        assert_eq!(factoms[1].value(), &Value::String("b".into()));
        store.retract_at(e, items, k2, &"b").unwrap().unwrap();
        assert_eq!(
            store
                .db()
                .entity_factoms(e)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn map_keys_hold_one_value_each() {
        let (_dir, store) = temp_store();
        let scores = store
            .declare_attribute(AttributeDef {
                key_type: Some(Type::String),
                ..def("person/scores", Type::Long, CollectionKind::Map)
            })
            .unwrap();
        let e = store.new_entity().unwrap();
        let ka = Slot::Key(MapKey::String("a".into()));
        let kb = Slot::Key(MapKey::String("b".into()));
        store
            .assert_at(e, scores, ka.clone(), &1i64)
            .unwrap()
            .unwrap();
        store
            .assert_at(e, scores, kb.clone(), &2i64)
            .unwrap()
            .unwrap();
        store
            .assert_at(e, scores, ka.clone(), &3i64)
            .unwrap()
            .unwrap();
        let factoms = store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(factoms.len(), 2);
        assert_eq!(factoms[0].slot(), &ka);
        assert_eq!(factoms[0].value(), &Value::Long(3));
        assert_eq!(factoms[1].slot(), &kb);
        assert_eq!(factoms[1].value(), &Value::Long(2));
    }

    #[test]
    fn plain_assert_on_list_is_rejected() {
        let (_dir, store) = temp_store();
        let items = store
            .declare_attribute(def("person/items", Type::String, CollectionKind::List))
            .unwrap();
        let e = store.new_entity().unwrap();
        let result = store.assert(e, items, &"x");
        assert!(matches!(result, Err(Error::RequiresKey(_))));
    }

    #[test]
    fn keyed_assert_on_one_is_rejected() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        let result = store
            .transact()
            .assert_at(e, name, Slot::Position(b"a0".to_vec()), &"x")
            .commit();
        assert!(matches!(result, Err(Error::UnexpectedKey(_))));
    }

    #[test]
    fn mismatched_key_variants_are_rejected() {
        let (_dir, store) = temp_store();
        let items = store
            .declare_attribute(def("person/items", Type::String, CollectionKind::List))
            .unwrap();
        let scores = store
            .declare_attribute(AttributeDef {
                key_type: Some(Type::String),
                ..def("person/scores", Type::Long, CollectionKind::Map)
            })
            .unwrap();
        let e = store.new_entity().unwrap();
        let on_list = store
            .transact()
            .assert_at(e, items, Slot::Key(MapKey::String("k".into())), &"x")
            .commit();
        assert!(matches!(on_list, Err(Error::MismatchedKey(_))));
        let on_map = store
            .transact()
            .assert_at(e, scores, Slot::Position(b"a0".to_vec()), &1i64)
            .commit();
        assert!(matches!(on_map, Err(Error::MismatchedKey(_))));
        let wrong_type = store
            .transact()
            .assert_at(e, scores, Slot::Key(MapKey::Long(1)), &1i64)
            .commit();
        assert!(matches!(wrong_type, Err(Error::MismatchedKey(_))));
    }

    #[test]
    fn declare_map_requires_valid_key_type() {
        let (_dir, store) = temp_store();
        let missing =
            store.declare_attribute(def("person/scores", Type::Long, CollectionKind::Map));
        assert!(matches!(missing, Err(Error::InvalidKeyType(_))));
        let bad_type = store.declare_attribute(AttributeDef {
            key_type: Some(Type::Bool),
            ..def("person/scores", Type::Long, CollectionKind::Map)
        });
        assert!(matches!(bad_type, Err(Error::InvalidKeyType(_))));
        let on_one = store.declare_attribute(AttributeDef {
            key_type: Some(Type::String),
            ..def("person/name", Type::String, CollectionKind::One)
        });
        assert!(matches!(on_one, Err(Error::InvalidKeyType(_))));
    }
}
