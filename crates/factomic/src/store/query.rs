use std::time::SystemTime;

use fjall::Readable;
use itertools::{Either, Itertools};
use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::factom::{Factom, Op, Slot};
use crate::id::{AttributeId, EntityId, TX_TIMESTAMP_ATTR, TxnId};
use crate::primitive::EncodePrimitive;
use crate::store::Db;
use crate::value::tag_for_type;
use crate::{MapKey, Value, keys};

const TXN_LEN: usize = size_of::<TxnId>();

fn current_candidate(key: &[u8], payload: &[u8]) -> Result<(Vec<u8>, Op)> {
    let txn = TxnId::try_from(payload)?;
    let mut temporal_key = keys::encode_history_identity(key); // add 0x00 00
    temporal_key.reserve_exact(TXN_LEN);
    temporal_key.extend_from_slice(&txn.to_be_bytes());
    Ok((temporal_key, Op::Assert))
}

fn historical_candidate(key: &[u8], payload: &[u8]) -> Result<(Vec<u8>, Op)> {
    if key.len() < TXN_LEN {
        return Err(Error::CorruptKey {
            expected: TXN_LEN,
            got: key.len(),
        });
    }
    if payload.len() != 1 {
        return Err(Error::CorruptKey {
            expected: 1,
            got: payload.len(),
        });
    }
    Ok((key.to_vec(), Op::try_from(payload[0])?))
}

fn identity(temporal_key: &[u8]) -> &[u8] {
    &temporal_key[..temporal_key.len() - TXN_LEN]
}

fn txn(temporal_key: &[u8]) -> TxnId {
    TxnId::with_value(u64::from_be_bytes(
        temporal_key[temporal_key.len() - TXN_LEN..]
            .try_into()
            .unwrap(),
    ))
}

fn visible_at<C, H>(
    current: C,
    history: H,
    cutoff: TxnId,
) -> impl Iterator<Item = Result<(Vec<u8>, TxnId)>>
where
    C: Iterator<Item = Result<(Vec<u8>, Op)>>,
    H: Iterator<Item = Result<(Vec<u8>, Op)>>,
{
    current
        .merge_by(history, |a, b| match (a, b) {
            (Ok(a), Ok(b)) => a.0 <= b.0,
            (Err(_), _) => true,
            (_, Err(_)) => false,
        })
        .coalesce(move |winner, next| match (winner, next) {
            (Ok(winner), Ok(next)) => {
                if identity(&winner.0) == identity(&next.0) {
                    Ok(Ok(if txn(&next.0) <= cutoff { next } else { winner }))
                } else {
                    Err((Ok(winner), Ok(next)))
                }
            }
            (winner, next) => Err((winner, next)),
        })
        .filter_map(move |candidate| match candidate {
            Err(error) => Some(Err(error)),
            Ok((key, op)) => {
                let asserted_at = txn(&key);
                (op == Op::Assert && asserted_at <= cutoff).then_some(Ok((key, asserted_at)))
            }
        })
}

impl Db {
    fn visible_easvt(
        &self,
        prefix: Vec<u8>,
        cutoff: TxnId,
    ) -> impl Iterator<Item = Result<(Vec<u8>, TxnId)>> {
        let current = self
            .snapshot
            .prefix(&self.easvt, prefix.clone())
            .map(|item| {
                let (key, payload) = item.into_inner()?;
                current_candidate(&key, &payload)
            });
        let history = self
            .snapshot
            .prefix(&self.easvt_h, keys::escape_identity(&prefix))
            .map(|item| {
                let (key, payload) = item.into_inner()?;
                historical_candidate(&key, &payload)
            });
        visible_at(current, history, cutoff)
    }

    fn visible_avest(
        &self,
        prefix: Vec<u8>,
        cutoff: TxnId,
    ) -> impl Iterator<Item = Result<(Vec<u8>, TxnId)>> {
        let current = self
            .snapshot
            .prefix(&self.avest, prefix.clone())
            .map(|item| {
                let (key, payload) = item.into_inner()?;
                current_candidate(&key, &payload)
            });
        let history = self
            .snapshot
            .prefix(&self.avest_h, keys::escape_identity(&prefix))
            .map(|item| {
                let (key, payload) = item.into_inner()?;
                historical_candidate(&key, &payload)
            });
        visible_at(current, history, cutoff)
    }

    fn visible_vaest(
        &self,
        prefix: Vec<u8>,
        cutoff: TxnId,
    ) -> impl Iterator<Item = Result<(Vec<u8>, TxnId)>> {
        let current = self
            .snapshot
            .prefix(&self.vaest, prefix.clone())
            .map(|item| {
                let (key, payload) = item.into_inner()?;
                current_candidate(&key, &payload)
            });
        let history = self
            .snapshot
            .prefix(&self.vaest_h, keys::escape_identity(&prefix))
            .map(|item| {
                let (key, payload) = item.into_inner()?;
                historical_candidate(&key, &payload)
            });
        visible_at(current, history, cutoff)
    }

    pub fn slot_at(
        &self,
        entity: EntityId,
        attr: AttributeId,
        ordinal: usize,
    ) -> Result<Option<Slot>> {
        let prefix = keys::easvt_entity_attr_prefix(entity, attr);
        if let Some(cutoff) = self.as_of {
            let Some(item) = self.visible_easvt(prefix.clone(), cutoff).nth(ordinal) else {
                return Ok(None);
            };
            let (key, _) = item?;
            let (_, _, slot, _, _) = keys::decode_easvt_history_key(&key)?;
            return Ok(Some(slot));
        }

        let Some(guard) = self.snapshot.prefix(&self.easvt, prefix).nth(ordinal) else {
            return Ok(None);
        };
        let (key, _) = guard.into_inner()?;
        let (_, _, slot, _) = keys::decode_easvt_key(&key)?;
        Ok(Some(slot))
    }

    pub fn txn_at(&self, time: SystemTime) -> Result<Option<TxnId>> {
        let expected = Timestamp::try_from(time)?;
        let region = Value::Timestamp(expected).region();

        let start = keys::avest_attr_prefix(TX_TIMESTAMP_ATTR);

        let mut end = keys::avest_value_prefix(TX_TIMESTAMP_ATTR, &region);
        end.extend_from_slice(&EntityId::with_value(u64::MAX).to_be_bytes());
        Slot::Empty.encode_key(&mut end);

        let entries = self.snapshot.range(&self.avest, start..=end);

        for entry in entries.rev() {
            let (key, _) = entry.into_inner()?;
            let (_, _, txn_entity, _) = keys::decode_avest_key(&key)?;
            let txn = TxnId::with_entity(txn_entity);

            if self.as_of.is_some_and(|cutoff| txn > cutoff) {
                continue;
            }

            return Ok(Some(txn));
        }

        Ok(None)
    }

    fn prefix_entity_factom(&self, prefix: &[u8]) -> Result<Option<Factom>> {
        if let Some(cutoff) = self.as_of {
            self.visible_easvt(prefix.to_vec(), cutoff)
                .next()
                .map(|item| {
                    let (key, asserted_at) = item?;
                    let (e, a, k, v, _) = keys::decode_easvt_history_key(&key)?;
                    Ok(Factom::new(e, a, k, v, asserted_at, Op::Assert))
                })
                .transpose()
        } else {
            self.snapshot
                .prefix(&self.easvt, prefix)
                .next()
                .map(|item| {
                    let (key, payload) = item.into_inner()?;
                    let (e, a, k, v) = keys::decode_easvt_key(&key)?;
                    let txn = TxnId::try_from(payload.as_ref())?;
                    Ok(Factom::new(e, a, k, v, txn, Op::Assert))
                })
                .transpose()
        }
    }

    fn prefix_entity_factoms(&self, prefix: Vec<u8>) -> impl Iterator<Item = Result<Factom>> + '_ {
        if let Some(cutoff) = self.as_of {
            Either::Left(self.visible_easvt(prefix, cutoff).map(|item| {
                let (key, asserted_at) = item?;
                let (e, a, k, v, _) = keys::decode_easvt_history_key(&key)?;
                Ok(Factom::new(e, a, k, v, asserted_at, Op::Assert))
            }))
        } else {
            Either::Right(self.snapshot.prefix(&self.easvt, prefix).map(|item| {
                let (key, payload) = item.into_inner()?;
                let (e, a, k, v) = keys::decode_easvt_key(&key)?;
                let txn = TxnId::try_from(payload.as_ref())?;
                Ok(Factom::new(e, a, k, v, txn, Op::Assert))
            }))
        }
    }

    pub fn entity_factoms(&self, entity: EntityId) -> impl Iterator<Item = Result<Factom>> + '_ {
        self.prefix_entity_factoms(keys::easvt_entity_prefix(entity))
    }

    pub fn entity_attr_factoms(
        &self,
        entity: EntityId,
        attr: AttributeId,
    ) -> impl Iterator<Item = Result<Factom>> + '_ {
        self.prefix_entity_factoms(keys::easvt_entity_attr_prefix(entity, attr))
    }

    pub fn entity_value<T>(&self, entity: EntityId, attr: AttributeId) -> Result<Option<T>>
    where
        T: for<'a> TryFrom<&'a Value, Error = crate::Error>,
    {
        self.prefix_entity_factom(&keys::easvt_entity_attr_prefix(entity, attr))?
            .map(|f| f.try_from_value())
            .transpose()
    }

    pub fn entity_values<T>(
        &self,
        entity: EntityId,
        attr: AttributeId,
    ) -> impl Iterator<Item = Result<T>> + '_
    where
        T: for<'a> TryFrom<&'a Value, Error = crate::Error>,
    {
        self.prefix_entity_factoms(keys::easvt_entity_attr_prefix(entity, attr))
            .map(|f| f.and_then(|f| f.try_from_value::<T>()))
    }

    pub fn entity_value_at<T>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        ordinal: usize,
    ) -> Result<Option<T>>
    where
        T: for<'a> TryFrom<&'a Value, Error = crate::Error>,
    {
        let Some(slot) = self.slot_at(entity, attr, ordinal)? else {
            return Ok(None);
        };
        self.prefix_entity_factom(&keys::easvt_entity_attr_slot_prefix(entity, attr, &slot))?
            .map(|f| f.try_from_value())
            .transpose()
    }

    pub fn entity_keyvalues<K, V>(
        &self,
        entity: EntityId,
        attr: AttributeId,
    ) -> impl Iterator<Item = Result<(K, V)>> + '_
    where
        K: for<'a> TryFrom<&'a MapKey, Error = crate::Error>,
        V: for<'a> TryFrom<&'a Value, Error = crate::Error>,
    {
        self.prefix_entity_factoms(keys::easvt_entity_attr_prefix(entity, attr))
            .map(|f| {
                let f = f?;
                Ok((f.try_key_from_slot::<K>()?, f.try_from_value::<V>()?))
            })
    }

    pub fn entity_keyvalue<V>(
        &self,
        entity: EntityId,
        attr: AttributeId,
        key: impl Into<MapKey>,
    ) -> Result<Option<V>>
    where
        V: for<'a> TryFrom<&'a Value, Error = crate::Error>,
    {
        let slot = Slot::Key(key.into());

        self.prefix_entity_factom(&keys::easvt_entity_attr_slot_prefix(entity, attr, &slot))?
            .map(|factom| factom.try_from_value())
            .transpose()
    }

    pub fn entity_history(&self, entity: EntityId) -> impl Iterator<Item = Result<Factom>> + '_ {
        self.snapshot
            .prefix(
                &self.easvt_h,
                keys::escape_identity(&keys::easvt_entity_prefix(entity)),
            )
            .map(|item| {
                let (key, payload) = item.into_inner()?;
                let (e, a, k, v, txn) = keys::decode_easvt_history_key(&key)?;
                let op = Op::try_from(payload[0])?;
                Ok(Factom::new(e, a, k, v, txn, op))
            })
    }

    pub fn entities_with_value<V: EncodePrimitive>(
        &self,
        attribute: AttributeId,
        value: &V,
    ) -> Result<Vec<EntityId>> {
        let def = self.attribute_def(attribute)?;
        if V::TYPE != def.value_type {
            return Err(Error::TypeMismatch {
                attribute: def.name.clone(),
                expected: def.value_type,
                got: V::TYPE,
            });
        }
        let mut region = Vec::with_capacity(9);
        region.push(tag_for_type(V::TYPE));
        value.encode(&mut region);
        let prefix = keys::avest_value_prefix(attribute, &region);
        let mut entities = Vec::new();
        if let Some(cutoff) = self.as_of {
            for item in self.visible_avest(prefix, cutoff) {
                let (key, _) = item?;
                let (_, _, entity, _, _) = keys::decode_avest_history_key(&key)?;
                entities.push(entity);
            }
            return Ok(entities);
        }

        for item in self.snapshot.prefix(&self.avest, prefix) {
            let (key, _) = item.into_inner()?;
            let (_, _, entity, _) = keys::decode_avest_key(&key)?;
            entities.push(entity);
        }
        Ok(entities)
    }

    pub fn referring_entities(
        &self,
        target: EntityId,
        attribute: AttributeId,
    ) -> Result<Vec<EntityId>> {
        let prefix = keys::vaest_target_attr_prefix(target, attribute);
        let mut entities = Vec::new();
        if let Some(cutoff) = self.as_of {
            for item in self.visible_vaest(prefix, cutoff) {
                let (key, _) = item?;
                let (_, _, source, _, _) = keys::decode_vaest_history_key(&key)?;
                entities.push(source);
            }
            return Ok(entities);
        }

        for item in self.snapshot.prefix(&self.vaest, prefix) {
            let (key, _) = item.into_inner()?;
            let (_, _, source, _) = keys::decode_vaest_key(&key)?;
            entities.push(source);
        }
        Ok(entities)
    }

    pub fn tx_factoms(&self, txn: TxnId) -> Result<Vec<Factom>> {
        let mut factoms = Vec::new();
        for item in self.snapshot.prefix(&self.txlog, keys::txlog_prefix(txn)) {
            let (key, payload) = item.into_inner()?;
            let (txn, entity, attribute, k, value) = keys::decode_txlog_key(&key)?;
            let op = Op::try_from(payload[0])?;
            factoms.push(Factom::new(entity, attribute, k, value, txn, op));
        }
        Ok(factoms)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::factom::CollectionKind;
    use crate::store::{AttributeDef, Store};
    use crate::value::{Type, Value};

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

    fn tx_time(db: &Db, txn: TxnId) -> SystemTime {
        let timestamp = db
            .tx_factoms(txn)
            .unwrap()
            .into_iter()
            .find(|factom| factom.attribute() == TX_TIMESTAMP_ATTR)
            .map(|factom| match factom.value() {
                Value::Timestamp(timestamp) => *timestamp,
                _ => panic!(),
            })
            .unwrap();
        SystemTime::try_from(timestamp).unwrap()
    }

    #[test]
    fn entity_factoms_as_of_reconstructs_reasserted_fact() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::Many))
            .unwrap();

        let t1 = store.assert(entity, name, &"alice").unwrap().unwrap();
        let t2 = store.retract(entity, name, &"alice").unwrap().unwrap();
        let t3 = store.assert(entity, name, &"alice").unwrap().unwrap();
        let db = store.db();
        let at_t1 = db.at(t1);

        let before_t1 = db.at(TxnId::with_value(t1.as_u64() - 1));
        assert!(
            before_t1
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );

        let facts = at_t1
            .entity_factoms(entity)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].txn(), t1);
        assert_eq!(facts[0].value(), &Value::String("alice".into()));
        assert!(
            db.at(t2)
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );

        let facts = db
            .at(t3)
            .entity_factoms(entity)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].txn(), t3);

        store.retract(entity, name, &"alice").unwrap().unwrap();
        assert_eq!(
            at_t1
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .len(),
            1
        );
        assert!(
            store
                .db()
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn as_of_preserves_easvt_order_for_prefix_values() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let tags = store
            .declare_attribute(def("person/tags", Type::String, CollectionKind::Many))
            .unwrap();

        store.assert(entity, tags, &"a").unwrap().unwrap();
        let t2 = store.assert(entity, tags, &"a\0").unwrap().unwrap();
        store.retract(entity, tags, &"a").unwrap().unwrap();
        store.retract(entity, tags, &"a\0").unwrap().unwrap();
        store.assert(entity, tags, &"a").unwrap().unwrap();
        store.assert(entity, tags, &"a\0").unwrap().unwrap();

        let values = store
            .db()
            .at(t2)
            .entity_factoms(entity)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|factom| factom.value().clone())
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![Value::String("a".into()), Value::String("a\0".into())]
        );
    }

    #[test]
    fn slot_at_as_of_counts_only_visible_slots() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let items = store
            .declare_attribute(def("person/items", Type::String, CollectionKind::List))
            .unwrap();

        let t1 = store.push(entity, items, &"a").unwrap().unwrap();
        let first = store.db().slot_at(entity, items, 0).unwrap().unwrap();
        let t2 = store.push(entity, items, &"b").unwrap().unwrap();
        let second = store.db().slot_at(entity, items, 1).unwrap().unwrap();
        store
            .remove(entity, items, second.clone())
            .unwrap()
            .unwrap();
        let db = store.db();

        assert_eq!(db.at(t1).slot_at(entity, items, 0).unwrap(), Some(first));
        assert_eq!(db.at(t1).slot_at(entity, items, 1).unwrap(), None);
        assert_eq!(db.at(t2).slot_at(entity, items, 1).unwrap(), Some(second));
        assert_eq!(db.slot_at(entity, items, 1).unwrap(), None);
    }

    #[test]
    fn indexed_value_queries_respect_as_of() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let age = store
            .declare_attribute(AttributeDef {
                indexed: true,
                ..def("person/age", Type::Long, CollectionKind::One)
            })
            .unwrap();

        let t1 = store.assert(entity, age, &30i64).unwrap().unwrap();
        let t2 = store.assert(entity, age, &31i64).unwrap().unwrap();
        let db = store.db();

        assert_eq!(
            db.at(t1).entities_with_value(age, &30i64).unwrap(),
            vec![entity]
        );
        assert!(
            db.at(t1)
                .entities_with_value(age, &31i64)
                .unwrap()
                .is_empty()
        );
        assert!(
            db.at(t2)
                .entities_with_value(age, &30i64)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            db.at(t2).entities_with_value(age, &31i64).unwrap(),
            vec![entity]
        );
    }

    #[test]
    fn reverse_reference_queries_respect_as_of() {
        let (_dir, store) = temp_store();
        let source = store.new_entity().unwrap();
        let first_target = store.new_entity().unwrap();
        let second_target = store.new_entity().unwrap();
        let friend = store
            .declare_attribute(def("person/friend", Type::Ref, CollectionKind::One))
            .unwrap();

        let t1 = store
            .assert(source, friend, &first_target)
            .unwrap()
            .unwrap();
        let t2 = store
            .assert(source, friend, &second_target)
            .unwrap()
            .unwrap();
        let db = store.db();

        assert_eq!(
            db.at(t1).referring_entities(first_target, friend).unwrap(),
            vec![source]
        );
        assert!(
            db.at(t1)
                .referring_entities(second_target, friend)
                .unwrap()
                .is_empty()
        );
        assert!(
            db.at(t2)
                .referring_entities(first_target, friend)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            db.at(t2).referring_entities(second_target, friend).unwrap(),
            vec![source]
        );
    }

    #[test]
    fn txn_at_respects_timestamp_bounds() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::Many))
            .unwrap();
        let t1 = store.assert(entity, name, &"one").unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(2));
        let t2 = store.assert(entity, name, &"two").unwrap().unwrap();
        let db = store.db();
        let t1_time = tx_time(&db, t1);
        let t2_time = tx_time(&db, t2);
        let between = t1_time + (t2_time.duration_since(t1_time).unwrap() / 2);

        assert_eq!(db.txn_at(t1_time - Duration::from_micros(1)).unwrap(), None);
        assert_eq!(db.txn_at(t1_time).unwrap(), Some(t1));
        assert_eq!(db.txn_at(between).unwrap(), Some(t1));
        assert_eq!(db.txn_at(t2_time).unwrap(), Some(t2));
        assert_eq!(
            db.txn_at(t2_time + Duration::from_millis(1)).unwrap(),
            Some(t2)
        );
    }

    #[test]
    fn time_views_respect_cutoffs_and_fallbacks() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let t1 = store.assert(entity, name, &"one").unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(2));
        let t2 = store.assert(entity, name, &"two").unwrap().unwrap();
        let db = store.db();
        let t1_time = tx_time(&db, t1);
        let t2_time = tx_time(&db, t2);
        let before = t1_time - Duration::from_micros(1);
        let at_t1 = db.at(t1);

        assert!(db.at_time(before).unwrap().is_none());
        assert_eq!(at_t1.txn_at(t2_time).unwrap(), Some(t1));
        assert_eq!(
            at_t1
                .at_time(t2_time)
                .unwrap()
                .unwrap()
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()[0]
                .value(),
            &Value::String("one".into())
        );
        assert_eq!(
            db.at_time_else_current(before)
                .unwrap()
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()[0]
                .value(),
            &Value::String("two".into())
        );
        assert_eq!(
            at_t1
                .at_time_else_current(before)
                .unwrap()
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()[0]
                .value(),
            &Value::String("one".into())
        );
    }
}
