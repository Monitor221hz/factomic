use std::cmp::Ordering;
use std::collections::HashMap;

use fjall::Readable;

use crate::error::{Error, Result};
use crate::factom::{Factom, Op, Slot};
use crate::id::{AttributeId, EntityId, TxnId};
use crate::primitive::EncodePrimitive;
use crate::store::Db;
use crate::value::tag_for_type;
use crate::{Value, keys};

//todo: zero copy identities?
struct Candidate {
    identity: Vec<u8>, // key without txn component
    txn: TxnId,
    op: Op,
}

fn candidate_cmp(a: &Candidate, b: &Candidate) -> Ordering {
    a.identity.cmp(&b.identity).then_with(|| a.txn.cmp(&b.txn)) //uuuugh idk maybe keep compound keys and lexo-compare. do we even need Candidate
}

struct Fact {
    identity: Vec<u8>,
    asserted_at: TxnId,
}

impl Db {
    fn current_candidates(&self, entity: EntityId) -> impl Iterator<Item = Result<Candidate>> {
        self.snapshot
            .prefix(&self.easvt, keys::easvt_entity_prefix(entity))
            .map(|item| {
                let (key, payload) = item.into_inner()?;
                Ok(Candidate {
                    identity: key.to_vec(),
                    txn: TxnId::from_bytes(&payload),
                    op: Op::Assert,
                })
            })
    }

    fn historical_candidates(&self, entity: EntityId) -> impl Iterator<Item = Result<Candidate>> {
        self.snapshot
            .prefix(&self.easvt, keys::easvt_entity_prefix(entity))
            .map(|item| {
                let (key, payload) = item.into_inner()?;

                let txn_start = key.len() - size_of::<TxnId>();

                Ok(Candidate {
                    identity: key[..txn_start].to_vec(),
                    txn: TxnId::from_bytes(&key[txn_start..]),
                    op: Op::try_from(payload[0])?,
                })
            })
    }

    fn merge_candidates(
        current: impl Iterator<Item = Result<Candidate>>,
        history: impl Iterator<Item = Result<Candidate>>,
    ) -> impl Iterator<Item = Result<Candidate>> {
        let mut current = current.peekable();
        let mut history = history.peekable();

        std::iter::from_fn(move || {
            let take_current = match (current.peek(), history.peek()) {
                (None, None) => return None,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some(Ok(c)), Some(Ok(h))) => candidate_cmp(c, h).is_le(),
                (Some(Err(_)), _) => true,
                (_, Some(Err(_))) => false,
            };
            if take_current {
                current.next()
            } else {
                history.next()
            }
        })
    }

    pub fn slot_at(
        &self,
        entity: EntityId,
        attr: AttributeId,
        ordinal: usize,
    ) -> Result<Option<Slot>> {
        let Some(guard) = self
            .snapshot
            .prefix(&self.easvt, keys::easvt_entity_attr_prefix(entity, attr))
            .nth(ordinal)
        else {
            return Ok(None);
        };
        let (key, _) = guard.into_inner()?;
        let (slot, _) = Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
        Ok(Some(slot))
    }

    pub fn entity_factoms(&self, entity: EntityId) -> Result<Vec<Factom>> {
        let mut factoms = Vec::new();
        for item in self
            .snapshot
            .prefix(&self.easvt, keys::easvt_entity_prefix(entity))
        {
            let (key, payload) = item.into_inner()?;
            let (e, a, k, v) = keys::decode_easvt_key(&key)?;
            let txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
            factoms.push(Factom::new(e, a, k, v, txn, Op::Assert));
        }
        Ok(factoms)
    }

    pub fn entity_history(&self, entity: EntityId) -> Result<Vec<Factom>> {
        let mut factoms = Vec::new();
        for item in self
            .snapshot
            .prefix(&self.easvt_h, keys::easvt_entity_prefix(entity))
        {
            let (key, payload) = item.into_inner()?;
            let (e, a, k, v, txn) = keys::decode_easvt_history_key(&key)?;
            let op = Op::try_from(payload[0])?;
            factoms.push(Factom::new(e, a, k, v, txn, op));
        }
        Ok(factoms)
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
        let mut entities = Vec::new();
        for item in self
            .snapshot
            .prefix(&self.avest, keys::avest_value_prefix(attribute, &region))
        {
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
        let mut entities = Vec::new();
        for item in self.snapshot.prefix(
            &self.vaest,
            keys::vaest_target_attr_prefix(target, attribute),
        ) {
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
