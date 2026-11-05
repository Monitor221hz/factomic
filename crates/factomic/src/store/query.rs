use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::factom::{Factom, Op};
use crate::id::{AttributeId, EntityId, TxnId};
use crate::primitive::EncodePrimitive;
use crate::store::Store;
use crate::value::tag_for_type;
use crate::{Value, keys};

pub struct EntityView {
    id: EntityId,
    map: HashMap<u16, Value>,
}

impl Store {
    pub fn entity_factoms(&self, entity: EntityId) -> Result<Vec<Factom>> {
        let mut factoms = Vec::new();
        for item in self.eavt.prefix(keys::eavt_entity_prefix(entity)) {
            let (key, payload) = item.into_inner()?;
            let (e, a, v) = keys::decode_eavt_key(&key)?;
            let txn = TxnId::new(u64::from_be_bytes(payload[..8].try_into().unwrap()));
            factoms.push(Factom::new(e, a, v, txn, Op::Assert));
        }
        Ok(factoms)
    }

    pub fn entity(&self, entity: EntityId) -> Result<EntityView> {
        let mut map: HashMap<u16, Value> = HashMap::new();
        for item in self.eavt.prefix(keys::eavt_entity_prefix(entity)) {
            let (key, _) = item.into_inner()?;
            let (_, a, v) = keys::decode_eavt_key(&key)?;
            map.insert(a.as_u16(), v);
        }
        Ok(EntityView { id: entity, map })
    }

    pub fn entity_history(&self, entity: EntityId) -> Result<Vec<Factom>> {
        let mut factoms = Vec::new();
        for item in self.eavt_h.prefix(keys::eavt_entity_prefix(entity)) {
            let (key, payload) = item.into_inner()?;
            let (e, a, v, txn) = keys::decode_eavt_history_key(&key)?;
            let op = Op::from_byte(payload[0])?;
            factoms.push(Factom::new(e, a, v, txn, op));
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
        let mut region = vec![tag_for_type(V::TYPE)];
        value.encode(&mut region);
        let mut entities = Vec::new();
        for item in self
            .avet
            .prefix(keys::avet_value_prefix(attribute, &region))
        {
            let (key, _) = item.into_inner()?;
            let (_, _, entity) = keys::decode_avet_key(&key)?;
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
        for item in self
            .vaet
            .prefix(keys::vaet_target_attr_prefix(target, attribute))
        {
            let (key, _) = item.into_inner()?;
            let (_, _, source) = keys::decode_vaet_key(&key)?;
            entities.push(source);
        }
        Ok(entities)
    }

    pub fn tx_factoms(&self, txn: TxnId) -> Result<Vec<Factom>> {
        let mut factoms = Vec::new();
        for item in self.txlog.prefix(keys::txlog_prefix(txn)) {
            let (key, payload) = item.into_inner()?;
            let (txn, entity, attribute, value) = keys::decode_txlog_key(&key)?;
            let op = Op::from_byte(payload[0])?;
            factoms.push(Factom::new(entity, attribute, value, txn, op));
        }
        Ok(factoms)
    }
}
