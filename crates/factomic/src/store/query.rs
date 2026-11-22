use std::time::SystemTime;

use fjall::Readable;
use itertools::{Either, Itertools};
use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::factom::{Factom, Op, Slot};
use crate::id::{AttributeId, EntityId, TX_TIMESTAMP_ATTR, TxnId};
use crate::primitive::EncodePrimitive;
use crate::store::Db;
use crate::value::{Type, tag_for_type};
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
                    Factom::with_easvt_pair(&key, &payload)
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
                Factom::with_easvt_pair(&key, &payload)
            }))
        }
    }

    pub fn entity_factoms(&self, entity: EntityId) -> impl Iterator<Item = Result<Factom>> + '_ {
        self.prefix_entity_factoms(keys::easvt_entity_prefix(entity))
    }

    pub fn entity_exists(&self, entity: EntityId) -> Result<bool> {
        Ok(self.entity_factoms(entity).next().transpose()?.is_some())
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
        let cutoff = self.as_of;
        self.snapshot
            .prefix(
                &self.easvt_h,
                keys::escape_identity(&keys::easvt_entity_prefix(entity)),
            )
            .filter_map(move |item| {
                let decoded = (|| -> Result<_> {
                    let (key, payload) = item.into_inner()?;
                    Factom::with_easvt_h_pair(&key, &payload)
                })();
                match decoded {
                    Ok(factom) if cutoff.is_some_and(|cutoff| factom.txn() > cutoff) => None,
                    other => Some(other),
                }
            })
    }

    pub fn entities_with_value<V: EncodePrimitive>(
        &self,
        attribute: AttributeId,
        value: &V,
    ) -> Result<Vec<EntityId>> {
        let def = self.attribute_def(attribute)?;
        if !def.indexed {
            return Err(Error::AttributeNotIndexed(def.name.clone()));
        }
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
            entities.dedup();
            return Ok(entities);
        }

        for item in self.snapshot.prefix(&self.avest, prefix) {
            let (key, _) = item.into_inner()?;
            let (_, _, entity, _) = keys::decode_avest_key(&key)?;
            entities.push(entity);
        }
        entities.dedup();
        Ok(entities)
    }

    pub fn referring_entities(
        &self,
        target: EntityId,
        attribute: AttributeId,
    ) -> Result<Vec<EntityId>> {
        let def = self.attribute_def(attribute)?;
        if def.value_type != Type::Ref {
            return Err(Error::TypeMismatch {
                attribute: def.name.clone(),
                expected: Type::Ref,
                got: def.value_type,
            });
        }
        let prefix = keys::vaest_target_attr_prefix(target, attribute);
        let mut entities = Vec::new();
        if let Some(cutoff) = self.as_of {
            for item in self.visible_vaest(prefix, cutoff) {
                let (key, _) = item?;
                let (_, _, source, _, _) = keys::decode_vaest_history_key(&key)?;
                entities.push(source);
            }
            entities.dedup();
            return Ok(entities);
        }

        for item in self.snapshot.prefix(&self.vaest, prefix) {
            let (key, _) = item.into_inner()?;
            let (_, _, source, _) = keys::decode_vaest_key(&key)?;
            entities.push(source);
        }
        entities.dedup();
        Ok(entities)
    }

    pub fn tx_factoms(&self, txn: TxnId) -> impl Iterator<Item = Result<Factom>> + '_ {
        if self.as_of.is_some_and(|cutoff| txn > cutoff) {
            return Either::Left(std::iter::empty());
        }
        Either::Right(
            self.snapshot
                .prefix(&self.txlog, keys::txlog_prefix(txn))
                .map(|item| {
                    let (key, payload) = item.into_inner()?;
                    Factom::with_txlog_pair(&key, &payload)
                }),
        )
    }

    fn prefix_latest_factom(&self, prefix: &[u8]) -> Result<Option<Factom>> {
        let current = match self.snapshot.prefix(&self.easvt, prefix).next() {
            Some(item) => {
                let (key, payload) = item.into_inner()?;
                let factom = Factom::with_easvt_pair(&key, &payload)?;
                if self.as_of.is_some_and(|cutoff| factom.txn() > cutoff) {
                    None
                } else {
                    Some(factom)
                }
            }
            None => None,
        };

        let decode_history = |item: fjall::Guard| -> Result<Factom> {
            let (key, payload) = item.into_inner()?;
            Factom::with_easvt_h_pair(&key, &payload)
        };
        let history_prefix = keys::escape_identity(prefix);
        let historical = if let Some(cutoff) = self.as_of {
            self.snapshot
                .prefix(&self.easvt_h, history_prefix)
                .rev()
                .find_map(|item| match decode_history(item) {
                    Ok(factom) if factom.txn() <= cutoff => Some(Ok(factom)),
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                })
        } else {
            self.snapshot
                .prefix(&self.easvt_h, history_prefix)
                .next_back()
                .map(decode_history)
        };
        let historical = match historical {
            Some(result) => Some(result?),
            None => None,
        };

        match (current, historical) {
            (Some(current), Some(historical)) if current.txn() >= historical.txn() => {
                Ok(Some(current))
            }
            (Some(_), Some(historical)) => Ok(Some(historical)),
            (Some(current), None) => Ok(Some(current)),
            (None, Some(historical)) => Ok(Some(historical)),
            (None, None) => Ok(None),
        }
    }
    /// Latest committed assertion or retraction for an identity
    pub fn identity_latest_factom(
        &self,
        entity: EntityId,
        attribute: AttributeId,
        slot: &Slot,
        value: &Value,
    ) -> Result<Option<Factom>> {
        self.prefix_latest_factom(&keys::easvt_entity_attr_slot_value_prefix(
            entity,
            attribute,
            slot,
            &value.region(),
        ))
    }

    pub fn entity_latest_factom(&self, entity: EntityId) -> Result<Option<Factom>> {
        self.prefix_latest_factom(&keys::easvt_entity_prefix(entity))
    }

    /// Helper method that internally calls [Db::latest_factom]
    pub fn latest_of_factom(&self, factom: &Factom) -> Result<Option<Factom>> {
        self.identity_latest_factom(
            factom.entity(),
            factom.attribute(),
            factom.slot(),
            factom.value(),
        )
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
            .flatten()
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
    fn history_and_txlog_respect_as_of() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let t1 = store.assert(entity, name, &"one").unwrap().unwrap();
        let t2 = store.assert(entity, name, &"two").unwrap().unwrap();
        store.retract(entity, name, &"two").unwrap().unwrap();
        let db = store.db();

        let at_t1 = db.at(t1);
        let history = at_t1
            .entity_history(entity)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].txn(), t1);
        assert_eq!(history[0].op(), Op::Assert);
        assert!(at_t1.tx_factoms(t2).next().is_none());
        assert!(at_t1.tx_factoms(t1).next().is_some());

        let history = db
            .at(t2)
            .entity_history(entity)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(history.len(), 3);
        assert!(history.iter().all(|factom| factom.txn() <= t2));
        assert!(
            history
                .iter()
                .any(|factom| factom.txn() == t2 && factom.op() == Op::Retract)
        );

        let history = db
            .entity_history(entity)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(history.len(), 4);
        assert!(db.tx_factoms(t2).next().is_some());
    }

    #[test]
    fn latest_operation_returns_assertion_or_retraction_by_txn() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let t1 = store.assert(entity, name, &"alice").unwrap().unwrap();
        let t2 = store.assert(entity, name, &"bob").unwrap().unwrap();
        let t3 = store.retract(entity, name, &"bob").unwrap().unwrap();
        let db = store.db();

        let latest = db
            .identity_latest_factom(entity, name, &Slot::Empty, &Value::String("bob".into()))
            .unwrap()
            .unwrap();
        assert_eq!(latest.op(), Op::Retract);
        assert_eq!(latest.txn(), t3);
        assert_eq!(latest.value(), &Value::String("bob".into()));

        let at_t2 = db.at(t2);
        let latest = at_t2
            .identity_latest_factom(entity, name, &Slot::Empty, &Value::String("bob".into()))
            .unwrap()
            .unwrap();
        assert_eq!(latest.op(), Op::Assert);
        assert_eq!(latest.txn(), t2);

        let at_t1 = db.at(t1);
        let latest = at_t1
            .identity_latest_factom(entity, name, &Slot::Empty, &Value::String("alice".into()))
            .unwrap()
            .unwrap();
        assert_eq!(latest.op(), Op::Assert);
        assert_eq!(latest.txn(), t1);

        let before_all = db.at(TxnId::with_value(0));
        assert!(
            before_all
                .identity_latest_factom(entity, name, &Slot::Empty, &Value::String("bob".into()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn latest_operation_returns_none_for_missing_identity() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        assert!(
            store
                .db()
                .identity_latest_factom(entity, name, &Slot::Empty, &Value::String("carol".into()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn non_indexed_queries_are_rejected() {
        let (_dir, store) = temp_store();
        let attr = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let target = store.new_entity().unwrap();

        assert!(matches!(
            store.db().entities_with_value(attr, &"Alice"),
            Err(Error::AttributeNotIndexed(_))
        ));
        assert!(matches!(
            store.db().referring_entities(target, attr),
            Err(Error::TypeMismatch { .. })
        ));
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
    fn reopen_preserves_indexes_history_and_time_views() {
        let (dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let first_target = store.new_entity().unwrap();
        let second_target = store.new_entity().unwrap();
        let [age, friend, items, scores] = store
            .declare_attributes([
                AttributeDef {
                    indexed: true,
                    ..def("person/age", Type::Long, CollectionKind::One)
                },
                def("person/friend", Type::Ref, CollectionKind::One),
                def("person/items", Type::String, CollectionKind::List),
                AttributeDef {
                    key_type: Some(Type::String),
                    ..def("person/scores", Type::Long, CollectionKind::Map)
                },
            ])
            .unwrap();
        let declaration = store.db().txn();
        let t1 = store
            .transact()
            .assert(entity, age, &30_i64)
            .assert(entity, friend, &first_target)
            .push(entity, items, &"a")
            .push(entity, items, &"c")
            .put(entity, scores, "math".into(), &90_i64)
            .commit()
            .unwrap()
            .unwrap();
        let declaration_time = tx_time(&store.db(), declaration);
        let t1_time = tx_time(&store.db(), t1);
        std::thread::sleep(Duration::from_millis(2));
        let anchor = store.db().slot_at(entity, items, 1).unwrap().unwrap();
        let t2 = store
            .transact()
            .assert(entity, age, &31_i64)
            .assert(entity, friend, &second_target)
            .put(entity, scores, "math".into(), &95_i64)
            .insert_before(entity, items, anchor, &"b")
            .commit()
            .unwrap()
            .unwrap();
        let t2_time = tx_time(&store.db(), t2);
        drop(store);

        let store = Store::open(dir.path()).unwrap();
        let db = store.db();
        assert_eq!(db.txn(), t2);
        assert_eq!(
            db.entity_values::<String>(entity, items)
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(
            db.entity_keyvalue::<i64>(entity, scores, "math").unwrap(),
            Some(95)
        );
        assert_eq!(db.entity_value::<i64>(entity, age).unwrap(), Some(31));
        assert_eq!(
            db.referring_entities(second_target, friend).unwrap(),
            vec![entity]
        );
        assert!(
            db.referring_entities(first_target, friend)
                .unwrap()
                .is_empty()
        );
        assert_eq!(db.entities_with_value(age, &31_i64).unwrap(), vec![entity]);
        assert!(db.entities_with_value(age, &30_i64).unwrap().is_empty());

        let at_t1 = db.at(t1);
        assert_eq!(
            at_t1
                .entity_values::<String>(entity, items)
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!["a".to_string(), "c".to_string()]
        );
        assert_eq!(at_t1.entity_value::<i64>(entity, age).unwrap(), Some(30));
        assert_eq!(
            at_t1
                .entity_keyvalue::<i64>(entity, scores, "math")
                .unwrap(),
            Some(90)
        );
        assert_eq!(
            at_t1.entity_value::<EntityId>(entity, friend).unwrap(),
            Some(first_target)
        );
        assert_eq!(
            at_t1.entities_with_value(age, &30_i64).unwrap(),
            vec![entity]
        );
        assert!(at_t1.entities_with_value(age, &31_i64).unwrap().is_empty());
        assert!(at_t1.tx_factoms(t2).next().is_none());
        assert!(
            at_t1
                .entity_history(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .iter()
                .all(|factom| factom.txn() <= t1)
        );

        let log = db.tx_factoms(t2).flatten().collect_vec();
        for attr in [age, friend, items, scores] {
            assert!(log.iter().any(|factom| factom.attribute() == attr));
        }
        assert_eq!(
            db.at_time(t1_time)
                .unwrap()
                .unwrap()
                .entity_value::<i64>(entity, age)
                .unwrap(),
            Some(30)
        );
        assert_eq!(
            db.at_time(t2_time)
                .unwrap()
                .unwrap()
                .entity_value::<i64>(entity, age)
                .unwrap(),
            Some(31)
        );
        assert!(
            db.at_time(declaration_time)
                .unwrap()
                .unwrap()
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );
        store.check_invariants().unwrap();
    }

    #[test]
    fn txn_at_respects_timestamp_bounds() {
        let (_dir, store) = temp_store();
        let entity = store.new_entity().unwrap();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::Many))
            .unwrap();
        let declaration = store.db().txn();
        std::thread::sleep(Duration::from_millis(2));
        let t1 = store.assert(entity, name, &"one").unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(2));
        let t2 = store.assert(entity, name, &"two").unwrap().unwrap();
        let db = store.db();
        let declaration_time = tx_time(&db, declaration);
        let t1_time = tx_time(&db, t1);
        let t2_time = tx_time(&db, t2);
        let between = t1_time + (t2_time.duration_since(t1_time).unwrap() / 2);

        assert_eq!(
            db.txn_at(declaration_time - Duration::from_micros(1))
                .unwrap(),
            None
        );
        assert_eq!(db.txn_at(declaration_time).unwrap(), Some(declaration));
        assert_eq!(
            db.txn_at(t1_time - Duration::from_micros(1)).unwrap(),
            Some(declaration)
        );
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
        let declaration = store.db().txn();
        std::thread::sleep(Duration::from_millis(2));
        let t1 = store.assert(entity, name, &"one").unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(2));
        let t2 = store.assert(entity, name, &"two").unwrap().unwrap();
        let db = store.db();
        let declaration_time = tx_time(&db, declaration);
        let t1_time = tx_time(&db, t1);
        let t2_time = tx_time(&db, t2);
        let before_all = declaration_time - Duration::from_micros(1);
        let before_t1 = t1_time - Duration::from_micros(1);
        let at_t1 = db.at(t1);

        assert!(db.at_time(before_all).unwrap().is_none());
        let declaration_view = db.at_time(before_t1).unwrap().unwrap();
        assert_eq!(declaration_view.as_of(), Some(declaration));
        assert!(
            declaration_view
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );
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
            db.at_time_else_current(before_all)
                .unwrap()
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()[0]
                .value(),
            &Value::String("two".into())
        );
        assert_eq!(
            at_t1
                .at_time_else_current(before_all)
                .unwrap()
                .entity_factoms(entity)
                .collect::<Result<Vec<_>>>()
                .unwrap()[0]
                .value(),
            &Value::String("one".into())
        );
    }
}
