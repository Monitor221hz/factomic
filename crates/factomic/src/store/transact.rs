use fjall::OwnedWriteBatch;
use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::factom::{ASSERT, Op, RETRACT};
use crate::id::{AttributeId, EntityId, TX_TIMESTAMP_ATTR, TxnId};
use crate::keys;
use crate::primitive::{DecodePrimitive, EncodePrimitive};
use crate::store::{AttributeDef, Store};
use crate::value::{TAG_REF, Type, Value, tag_for_type};

fn encode_region<V: EncodePrimitive>(value: &V) -> Vec<u8> {
    let mut region = vec![tag_for_type(V::TYPE)];
    value.encode(&mut region);
    region
}

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

#[derive(Clone, Copy)]
struct FactomRef<'a> {
    entity: EntityId,
    attr: AttributeId,
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
        let (entity, attr, region) = (factom.entity, factom.attr, factom.region);
        let payload = txn.to_be_bytes();
        batch.insert(&self.eavt, keys::eavt_key(entity, attr, region), payload);
        batch.insert(&self.aevt, keys::aevt_key(entity, attr, region), payload);
        if def.indexed {
            batch.insert(&self.avet, keys::avet_key(entity, attr, region), payload);
        }
        if let Some(target) = ref_target(region)? {
            batch.insert(&self.vaet, keys::vaet_key(target, attr, entity), payload);
        }
        Ok(())
    }

    fn remove_current(
        &self,
        batch: &mut OwnedWriteBatch,
        def: &AttributeDef,
        datom: FactomRef<'_>,
    ) -> Result<()> {
        let (entity, attr, region) = (datom.entity, datom.attr, datom.region);
        batch.remove(&self.eavt, keys::eavt_key(entity, attr, region));
        batch.remove(&self.aevt, keys::aevt_key(entity, attr, region));
        if def.indexed {
            batch.remove(&self.avet, keys::avet_key(entity, attr, region));
        }
        if let Some(target) = ref_target(region)? {
            batch.remove(&self.vaet, keys::vaet_key(target, attr, entity));
        }
        Ok(())
    }

    fn write_history(
        &self,
        batch: &mut OwnedWriteBatch,
        def: &AttributeDef,
        datom: FactomRef<'_>,
        asserted_at: TxnId,
        retracted_at: TxnId,
    ) -> Result<()> {
        let (entity, attr, region) = (datom.entity, datom.attr, datom.region);
        for (keyspace, key_fn) in [
            (&self.eavt_h, keys::eavt_history_key as fn(_, _, _, _) -> _),
            (&self.aevt_h, keys::aevt_history_key),
        ] {
            batch.insert(
                keyspace,
                key_fn(entity, attr, region, asserted_at),
                [ASSERT],
            );
            batch.insert(
                keyspace,
                key_fn(entity, attr, region, retracted_at),
                [RETRACT],
            );
        }
        if def.indexed {
            batch.insert(
                &self.avet_h,
                keys::avet_history_key(entity, attr, region, asserted_at),
                [ASSERT],
            );
            batch.insert(
                &self.avet_h,
                keys::avet_history_key(entity, attr, region, retracted_at),
                [RETRACT],
            );
        }
        if let Some(target) = ref_target(region)? {
            batch.insert(
                &self.vaet_h,
                keys::vaet_history_key(target, attr, entity, asserted_at),
                [ASSERT],
            );
            batch.insert(
                &self.vaet_h,
                keys::vaet_history_key(target, attr, entity, retracted_at),
                [RETRACT],
            );
        }
        Ok(())
    }

    fn record_tx_timestamp(&self, batch: &mut OwnedWriteBatch, txn: TxnId) {
        let region = encode_region(&Timestamp::now());
        let entity = txn.entity_id();
        let payload = txn.to_be_bytes();
        batch.insert(
            &self.eavt,
            keys::eavt_key(entity, TX_TIMESTAMP_ATTR, &region),
            payload,
        );
        batch.insert(
            &self.aevt,
            keys::aevt_key(entity, TX_TIMESTAMP_ATTR, &region),
            payload,
        );
        batch.insert(
            &self.txlog,
            keys::txlog_key(txn, entity, TX_TIMESTAMP_ATTR, &region),
            [ASSERT],
        );
    }
}

struct PlannedOp {
    op: Op,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    region: Vec<u8>,
}

pub struct Transaction<'a> {
    store: &'a Store,
    ops: Vec<PlannedOp>,
}

impl Store {
    pub fn transact(&self) -> Transaction<'_> {
        Transaction {
            store: self,
            ops: Vec::new(),
        }
    }
}

impl<'a> Transaction<'a> {
    pub fn assert<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp {
            op: Op::Assert,
            entity,
            attr,
            ty: V::TYPE,
            region: encode_region(value),
        });
        self
    }

    pub fn retract<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp {
            op: Op::Retract,
            entity,
            attr,
            ty: V::TYPE,
            region: encode_region(value),
        });
        self
    }

    pub fn assert_value(mut self, entity: EntityId, attr: AttributeId, value: &Value) -> Self {
        let mut region = Vec::new();
        value.encode_key(&mut region);
        self.ops.push(PlannedOp {
            op: Op::Assert,
            entity,
            attr,
            ty: value.value_type(),
            region,
        });
        self
    }

    pub fn retract_value(mut self, entity: EntityId, attr: AttributeId, value: &Value) -> Self {
        let mut region = Vec::new();
        value.encode_key(&mut region);
        self.ops.push(PlannedOp {
            op: Op::Retract,
            entity,
            attr,
            ty: value.value_type(),
            region,
        });
        self
    }

    pub fn commit(self) -> Result<Option<TxnId>> {
        enum Plan {
            Skip,
            Effective {
                def: AttributeDef,
                evictions: Vec<(Vec<u8>, TxnId)>,
            },
        }

        let store = self.store;
        let mut plans = Vec::with_capacity(self.ops.len());
        for op in &self.ops {
            let def = check_type(store, op.attr, op.ty)?;
            match op.op {
                Op::Assert => {
                    let new_key = keys::eavt_key(op.entity, op.attr, &op.region);
                    let mut evictions = Vec::new();
                    let mut skip = false;
                    for guard in store
                        .eavt
                        .prefix(keys::eavt_entity_attr_prefix(op.entity, op.attr))
                    {
                        let (key, payload) = guard.into_inner()?;
                        if *key == *new_key {
                            skip = true;
                            break;
                        }
                        let old_txn =
                            TxnId::new(u64::from_be_bytes(payload[..8].try_into().unwrap()));
                        let region =
                            key[size_of::<EntityId>() + size_of::<AttributeId>()..].to_vec();
                        evictions.push((region, old_txn));
                    }
                    plans.push(if skip {
                        Plan::Skip
                    } else {
                        Plan::Effective { def, evictions }
                    });
                }
                Op::Retract => {
                    let key = keys::eavt_key(op.entity, op.attr, &op.region);
                    match store.eavt.get(&key)? {
                        None => plans.push(Plan::Skip),
                        Some(payload) => {
                            let old_txn =
                                TxnId::new(u64::from_be_bytes(payload[..8].try_into().unwrap()));
                            plans.push(Plan::Effective {
                                def,
                                evictions: vec![(op.region.clone(), old_txn)],
                            });
                        }
                    }
                }
            }
        }

        if plans.iter().all(|p| matches!(p, Plan::Skip)) {
            return Ok(None);
        }

        let txn = store.alloc_txn_id()?;
        let mut batch = OwnedWriteBatch::with_capacity(store.db.clone(), 24);

        for (op, plan) in self.ops.iter().zip(plans) {
            let Plan::Effective { def, evictions } = plan else {
                continue;
            };
            for (old_region, old_txn) in evictions {
                let old = FactomRef {
                    entity: op.entity,
                    attr: op.attr,
                    region: &old_region,
                };
                store.remove_current(&mut batch, &def, old)?;
                store.write_history(&mut batch, &def, old, old_txn, txn)?;
                batch.insert(
                    &store.txlog,
                    keys::txlog_key(txn, op.entity, op.attr, &old_region),
                    [RETRACT],
                );
            }
            if op.op == Op::Assert {
                let datom = FactomRef {
                    entity: op.entity,
                    attr: op.attr,
                    region: &op.region,
                };
                store.insert_current(&mut batch, &def, datom, txn)?;
                batch.insert(
                    &store.txlog,
                    keys::txlog_key(txn, op.entity, op.attr, &op.region),
                    [ASSERT],
                );
            }
        }

        store.record_tx_timestamp(&mut batch, txn);
        batch.commit()?;
        Ok(Some(txn))
    }
}

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

    pub fn revert_txn(&self, txn: TxnId) -> Result<Option<TxnId>> {
        let mut tx = self.transact();
        for factom in self.tx_factoms(txn)? {
            if factom.attribute() == TX_TIMESTAMP_ATTR {
                continue;
            }
            tx = match factom.op() {
                Op::Assert => tx.retract_value(factom.entity(), factom.attribute(), factom.value()),
                Op::Retract => tx.assert_value(factom.entity(), factom.attribute(), factom.value()),
            };
        }
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factom::Factom;
    use tempfile::TempDir;

    fn temp_store() -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn assert_and_load_entity() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn = store.assert(e, name, &"alice").unwrap().unwrap();
        let factoms = store.entity_factoms(e).unwrap();
        assert_eq!(factoms.len(), 1);
        assert_eq!(factoms[0].entity(), e);
        assert_eq!(factoms[0].attribute(), name);
        assert_eq!(factoms[0].value(), &Value::String("alice".into()));
        assert_eq!(factoms[0].txn(), txn);
        assert_eq!(factoms[0].op(), Op::Assert);
    }

    #[test]
    fn assert_is_idempotent() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"alice").unwrap().unwrap();
        let second = store.assert(e, name, &"alice").unwrap();
        assert_eq!(second, None);
        assert_eq!(store.entity_factoms(e).unwrap().len(), 1);
    }

    #[test]
    fn overwrite_moves_old_value_to_history() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store.assert(e, name, &"alice").unwrap().unwrap();
        let txn2 = store.assert(e, name, &"bob").unwrap().unwrap();
        let current = store.entity_factoms(e).unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].value(), &Value::String("bob".into()));
        let history = store.entity_history(e).unwrap();
        assert!(history.contains(&Factom::new(
            e,
            name,
            Value::String("alice".into()),
            txn1,
            Op::Assert
        )));
        assert!(history.contains(&Factom::new(
            e,
            name,
            Value::String("alice".into()),
            txn2,
            Op::Retract
        )));
    }

    #[test]
    fn retract_removes_and_records_history() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store.assert(e, name, &"alice").unwrap().unwrap();
        let txn2 = store.retract(e, name, &"alice").unwrap().unwrap();
        assert!(store.entity_factoms(e).unwrap().is_empty());
        let history = store.entity_history(e).unwrap();
        assert!(history.contains(&Factom::new(
            e,
            name,
            Value::String("alice".into()),
            txn1,
            Op::Assert
        )));
        assert!(history.contains(&Factom::new(
            e,
            name,
            Value::String("alice".into()),
            txn2,
            Op::Retract
        )));
    }

    #[test]
    fn retract_missing_value_is_noop() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        assert_eq!(store.retract(e, name, &"nobody").unwrap(), None);
    }

    #[test]
    fn type_mismatch_is_rejected() {
        let (_dir, store) = temp_store();
        let age = store
            .declare_attribute("person/age", Type::Long, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        let result = store.assert(e, age, &"thirty");
        assert!(matches!(result, Err(Error::TypeMismatch { .. })));
    }

    #[test]
    fn indexed_attr_supports_value_lookup() {
        let (_dir, store) = temp_store();
        let age = store
            .declare_attribute("person/age", Type::Long, true)
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, age, &30i64).unwrap();
        assert_eq!(store.entities_with_value(age, &30i64).unwrap(), vec![e]);
        assert!(store.entities_with_value(age, &31i64).unwrap().is_empty());
    }

    #[test]
    fn ref_attr_supports_reverse_lookup() {
        let (_dir, store) = temp_store();
        let friend = store
            .declare_attribute("person/friend", Type::Ref, false)
            .unwrap();
        let a = store.new_entity().unwrap();
        let b = store.new_entity().unwrap();
        store.assert(a, friend, &b).unwrap();
        assert_eq!(store.referring_entities(b, friend).unwrap(), vec![a]);
        assert!(store.referring_entities(a, friend).unwrap().is_empty());
    }

    #[test]
    fn txlog_records_the_whole_transaction() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn = store.assert(e, name, &"alice").unwrap().unwrap();
        let datoms = store.tx_factoms(txn).unwrap();
        assert_eq!(datoms.len(), 2);
        assert!(
            datoms
                .iter()
                .any(|d| d.entity() == e && d.attribute() == name && d.op() == Op::Assert)
        );
        assert!(
            datoms
                .iter()
                .any(|d| d.entity() == txn.entity_id() && d.attribute() == TX_TIMESTAMP_ATTR)
        );
    }

    #[test]
    fn revert_removes_asserted_value() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store.assert(e, name, &"alice").unwrap().unwrap();

        let reverted = store.revert_txn(txn1).unwrap().unwrap();
        assert!(store.entity_factoms(e).unwrap().is_empty());

        let history = store.entity_history(e).unwrap();
        assert!(history.contains(&Factom::new(
            e,
            name,
            Value::String("alice".into()),
            txn1,
            Op::Assert
        )));
        assert!(history.contains(&Factom::new(
            e,
            name,
            Value::String("alice".into()),
            reverted,
            Op::Retract
        )));
    }

    #[test]
    fn revert_overwrite_restores_previous_value() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"alice").unwrap().unwrap();
        let txn2 = store.assert(e, name, &"bob").unwrap().unwrap();

        store.revert_txn(txn2).unwrap();
        let current = store.entity_factoms(e).unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].value(), &Value::String("alice".into()));
    }

    #[test]
    fn revert_retraction_restores_value() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"alice").unwrap().unwrap();
        let txn2 = store.retract(e, name, &"alice").unwrap().unwrap();

        store.revert_txn(txn2).unwrap();
        let current = store.entity_factoms(e).unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].value(), &Value::String("alice".into()));
    }

    #[test]
    fn revert_keeps_tx_timestamp() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn1 = store.assert(e, name, &"alice").unwrap().unwrap();

        store.revert_txn(txn1).unwrap();
        assert_eq!(store.entity_factoms(txn1.entity_id()).unwrap().len(), 1);
    }

    #[test]
    fn builder_commits_multiple_ops_as_one_txn() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let age = store
            .declare_attribute("person/age", Type::Long, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        let txn = store
            .transact()
            .assert(e, name, &"alice")
            .assert(e, age, &30i64)
            .commit()
            .unwrap()
            .unwrap();
        let factoms = store.entity_factoms(e).unwrap();
        assert_eq!(factoms.len(), 2);
        assert!(factoms.iter().all(|f| f.txn() == txn));
        assert_eq!(store.tx_factoms(txn).unwrap().len(), 3);
    }

    #[test]
    fn builder_all_noops_returns_none() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"alice").unwrap().unwrap();
        let result = store
            .transact()
            .assert(e, name, &"alice")
            .retract(e, name, &"bob")
            .commit()
            .unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn builder_skips_noops_but_commits_effective_ops() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let age = store
            .declare_attribute("person/age", Type::Long, false)
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
        let logged = store.tx_factoms(txn).unwrap();
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
            .declare_attribute("person/name", Type::String, false)
            .unwrap();
        let age = store
            .declare_attribute("person/age", Type::Long, false)
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
        assert!(store.entity_factoms(e).unwrap().is_empty());
        let logged = store.tx_factoms(reverted).unwrap();
        assert_eq!(logged.len(), 3);
        assert_eq!(logged.iter().filter(|f| f.op() == Op::Retract).count(), 2);
    }
}
