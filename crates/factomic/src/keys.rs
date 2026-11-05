use crate::error::{Error, Result};
use crate::id::{AttributeId, EntityId, TxnId};
use crate::value::Value;

pub fn eavt_key(e: EntityId, a: AttributeId, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(size_of::<EntityId>() + size_of::<AttributeId>() + v.len());
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(v);
    key
}

pub fn aevt_key(e: EntityId, a: AttributeId, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(size_of::<AttributeId>() + size_of::<EntityId>() + v.len());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(v);
    key
}

pub fn avet_key(e: EntityId, a: AttributeId, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(size_of::<AttributeId>() + v.len() + size_of::<EntityId>());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(v);
    key.extend_from_slice(&e.to_be_bytes());
    key
}

pub fn vaet_key(target: EntityId, a: AttributeId, source: EntityId) -> Vec<u8> {
    let mut key = Vec::with_capacity(
        size_of::<EntityId>() + size_of::<AttributeId>() + size_of::<EntityId>(),
    );
    key.extend_from_slice(&target.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(&source.to_be_bytes());
    key
}

pub fn eavt_history_key(e: EntityId, a: AttributeId, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = Vec::with_capacity(
        size_of::<EntityId>() + size_of::<AttributeId>() + v.len() + size_of::<TxnId>(),
    );
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(v);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

pub fn aevt_history_key(e: EntityId, a: AttributeId, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = Vec::with_capacity(
        size_of::<AttributeId>() + size_of::<EntityId>() + v.len() + size_of::<TxnId>(),
    );
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(v);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

pub fn avet_history_key(e: EntityId, a: AttributeId, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = Vec::with_capacity(
        size_of::<EntityId>() + size_of::<AttributeId>() + v.len() + size_of::<TxnId>(),
    );
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(v);
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(&t.to_be_bytes());
    key
}

pub fn vaet_history_key(target: EntityId, a: AttributeId, source: EntityId, t: TxnId) -> Vec<u8> {
    let mut key = Vec::with_capacity(
        size_of::<EntityId>()
            + size_of::<AttributeId>()
            + size_of::<EntityId>()
            + size_of::<TxnId>(),
    );
    key.extend_from_slice(&target.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(&source.to_be_bytes());
    key.extend_from_slice(&t.to_be_bytes());
    key
}

pub fn txlog_key(t: TxnId, e: EntityId, a: AttributeId, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(
        size_of::<TxnId>() + size_of::<EntityId>() + size_of::<AttributeId>() + v.len(),
    );
    key.extend_from_slice(&t.to_be_bytes());
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(v);
    key
}

pub fn decode_eavt_key(key: &[u8]) -> Result<(EntityId, AttributeId, Value)> {
    {
        let lim = size_of::<EntityId>() + size_of::<AttributeId>() + 1;
        if key.len() < lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        EntityId(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        Value::decode_key(&key[10..])?,
    );

    Ok(key)
}

#[allow(dead_code)]
pub fn decode_aevt_key(key: &[u8]) -> Result<(AttributeId, EntityId, Value)> {
    {
        let lim = size_of::<AttributeId>() + size_of::<EntityId>() + 1;
        if key.len() < lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        AttributeId::new(u16::from_be_bytes(key[0..2].try_into().unwrap())),
        EntityId(u64::from_be_bytes(key[2..10].try_into().unwrap())),
        Value::decode_key(&key[10..])?,
    );

    Ok(key)
}

pub fn decode_avet_key(key: &[u8]) -> Result<(AttributeId, Value, EntityId)> {
    {
        let lim = size_of::<AttributeId>() + size_of::<EntityId>() + 1;
        if key.len() < lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        AttributeId::new(u16::from_be_bytes(key[0..2].try_into().unwrap())),
        Value::decode_key(&key[2..key.len() - 8])?,
        EntityId(u64::from_be_bytes(key[key.len() - 8..].try_into().unwrap())),
    );

    Ok(key)
}

pub fn decode_vaet_key(key: &[u8]) -> Result<(EntityId, AttributeId, EntityId)> {
    {
        let lim = size_of::<EntityId>() + size_of::<AttributeId>() + size_of::<EntityId>();
        if key.len() != lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        EntityId(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        EntityId(u64::from_be_bytes(key[10..18].try_into().unwrap())),
    );

    Ok(key)
}

pub fn decode_eavt_history_key(key: &[u8]) -> Result<(EntityId, AttributeId, Value, TxnId)> {
    {
        let lim = size_of::<EntityId>() + size_of::<AttributeId>() + 1 + size_of::<TxnId>();
        if key.len() < lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        EntityId(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        Value::decode_key(&key[10..key.len() - 8])?,
        TxnId::new(u64::from_be_bytes(key[key.len() - 8..].try_into().unwrap())),
    );

    Ok(key)
}

#[allow(dead_code)]
pub fn decode_aevt_history_key(key: &[u8]) -> Result<(AttributeId, EntityId, Value, TxnId)> {
    {
        let lim = size_of::<AttributeId>() + size_of::<EntityId>() + 1 + size_of::<TxnId>();
        if key.len() < lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        AttributeId::new(u16::from_be_bytes(key[0..2].try_into().unwrap())),
        EntityId(u64::from_be_bytes(key[2..10].try_into().unwrap())),
        Value::decode_key(&key[10..key.len() - 8])?,
        TxnId::new(u64::from_be_bytes(key[key.len() - 8..].try_into().unwrap())),
    );

    Ok(key)
}

#[allow(dead_code)]
pub fn decode_avet_history_key(key: &[u8]) -> Result<(AttributeId, Value, EntityId, TxnId)> {
    {
        let lim = size_of::<AttributeId>() + 1 + size_of::<EntityId>() + size_of::<TxnId>();
        if key.len() < lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        AttributeId::new(u16::from_be_bytes(key[0..2].try_into().unwrap())),
        Value::decode_key(&key[2..key.len() - 16])?,
        EntityId(u64::from_be_bytes(
            key[key.len() - 16..key.len() - 8].try_into().unwrap(),
        )),
        TxnId::new(u64::from_be_bytes(key[key.len() - 8..].try_into().unwrap())),
    );

    Ok(key)
}

#[allow(dead_code)]
pub fn decode_vaet_history_key(key: &[u8]) -> Result<(EntityId, AttributeId, EntityId, TxnId)> {
    {
        let lim = size_of::<EntityId>()
            + size_of::<AttributeId>()
            + size_of::<EntityId>()
            + size_of::<TxnId>();
        if key.len() != lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        EntityId(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        EntityId(u64::from_be_bytes(key[10..18].try_into().unwrap())),
        TxnId::new(u64::from_be_bytes(key[18..26].try_into().unwrap())),
    );

    Ok(key)
}

pub fn decode_txlog_key(key: &[u8]) -> Result<(TxnId, EntityId, AttributeId, Value)> {
    {
        let lim = size_of::<TxnId>() + size_of::<EntityId>() + size_of::<AttributeId>() + 1;
        if key.len() < lim {
            return Err(Error::CorruptKey {
                expected: lim,
                got: key.len(),
            });
        }
    }
    let key = (
        TxnId::new(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        EntityId(u64::from_be_bytes(key[8..16].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[16..18].try_into().unwrap())),
        Value::decode_key(&key[18..])?,
    );

    Ok(key)
}

pub fn eavt_entity_prefix(e: EntityId) -> Vec<u8> {
    e.to_be_bytes().to_vec()
}

pub fn eavt_entity_attr_prefix(e: EntityId, a: AttributeId) -> Vec<u8> {
    let mut prefix = eavt_entity_prefix(e);
    prefix.extend_from_slice(&a.to_be_bytes());
    prefix
}

#[allow(dead_code)]
pub fn aevt_attr_prefix(a: AttributeId) -> Vec<u8> {
    a.to_be_bytes().to_vec()
}

pub fn avet_attr_prefix(a: AttributeId) -> Vec<u8> {
    a.to_be_bytes().to_vec()
}

pub fn avet_value_prefix(a: AttributeId, v: &[u8]) -> Vec<u8> {
    let mut prefix = avet_attr_prefix(a);
    prefix.extend_from_slice(v);
    prefix
}

pub fn vaet_target_prefix(target: EntityId) -> Vec<u8> {
    target.to_be_bytes().to_vec()
}

pub fn vaet_target_attr_prefix(target: EntityId, a: AttributeId) -> Vec<u8> {
    let mut prefix = vaet_target_prefix(target);
    prefix.extend_from_slice(&a.to_be_bytes());
    prefix
}

pub fn txlog_prefix(t: TxnId) -> Vec<u8> {
    t.to_be_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{SYSTEM_PARTITION, USER_PARTITION};

    fn e(index: u64) -> EntityId {
        EntityId::new(USER_PARTITION, index)
    }

    fn region(v: &Value) -> Vec<u8> {
        let mut buf = Vec::new();
        v.encode_key(&mut buf);
        buf
    }

    #[test]
    fn eavt_round_trip() {
        let (entity, attr, value) = (
            e(42),
            AttributeId::new(0x0100),
            Value::String("alice".into()),
        );
        let key = eavt_key(entity, attr, &region(&value));
        assert_eq!(decode_eavt_key(&key).unwrap(), (entity, attr, value));
    }

    #[test]
    fn aevt_round_trip() {
        let (entity, attr, value) = (e(42), AttributeId::new(0x0100), Value::Long(30));
        let key = aevt_key(entity, attr, &region(&value));
        assert_eq!(decode_aevt_key(&key).unwrap(), (attr, entity, value));
    }

    #[test]
    fn avet_round_trip() {
        let (entity, attr, value) = (e(42), AttributeId::new(0x0100), Value::Long(-5));
        let key = avet_key(entity, attr, &region(&value));
        assert_eq!(decode_avet_key(&key).unwrap(), (attr, value, entity));
    }

    #[test]
    fn vaet_round_trip() {
        let (target, attr, source) = (e(43), AttributeId::new(0x0102), e(42));
        let key = vaet_key(target, attr, source);
        assert_eq!(decode_vaet_key(&key).unwrap(), (target, attr, source));
    }

    #[test]
    fn history_round_trips() {
        let (entity, attr, value) = (e(42), AttributeId::new(0x0100), Value::Long(30));
        let txn = TxnId::new(EntityId::new(SYSTEM_PARTITION, 100).0);
        let region = region(&value);
        assert_eq!(
            decode_eavt_history_key(&eavt_history_key(entity, attr, &region, txn)).unwrap(),
            (entity, attr, value.clone(), txn)
        );
        assert_eq!(
            decode_aevt_history_key(&aevt_history_key(entity, attr, &region, txn)).unwrap(),
            (attr, entity, value.clone(), txn)
        );
        assert_eq!(
            decode_avet_history_key(&avet_history_key(entity, attr, &region, txn)).unwrap(),
            (attr, value, entity, txn)
        );
    }

    #[test]
    fn vaet_history_round_trip() {
        let (target, attr, source) = (e(43), AttributeId::new(0x0102), e(42));
        let txn = TxnId::new(EntityId::new(SYSTEM_PARTITION, 100).0);
        let key = vaet_history_key(target, attr, source, txn);
        assert_eq!(
            decode_vaet_history_key(&key).unwrap(),
            (target, attr, source, txn)
        );
    }

    #[test]
    fn txlog_round_trip() {
        let (entity, attr, value) = (e(42), AttributeId::new(0x0100), Value::Bool(true));
        let txn = TxnId::new(EntityId::new(SYSTEM_PARTITION, 100).0);
        let key = txlog_key(txn, entity, attr, &region(&value));
        assert_eq!(decode_txlog_key(&key).unwrap(), (txn, entity, attr, value));
    }

    #[test]
    fn eavt_sorts_by_entity_then_attribute() {
        let attr_a = AttributeId::new(0x0100);
        let attr_b = AttributeId::new(0x0101);
        let v = region(&Value::Long(1));
        assert!(eavt_key(e(1), attr_b, &v) < eavt_key(e(2), attr_a, &v));
        assert!(eavt_key(e(1), attr_a, &v) < eavt_key(e(1), attr_b, &v));
    }

    #[test]
    fn avet_sorts_by_value_within_attribute() {
        let attr = AttributeId::new(0x0100);
        let k29 = avet_key(e(2), attr, &region(&Value::Long(29)));
        let k30 = avet_key(e(1), attr, &region(&Value::Long(30)));
        assert!(k29 < k30);
        let neg = avet_key(e(1), attr, &region(&Value::Long(-1)));
        assert!(neg < k29);
    }

    #[test]
    fn avet_same_value_sorts_by_entity() {
        let attr = AttributeId::new(0x0100);
        let v = region(&Value::Long(30));
        assert!(avet_key(e(1), attr, &v) < avet_key(e(2), attr, &v));
    }

    #[test]
    fn vaet_sorts_by_target() {
        let attr = AttributeId::new(0x0102);
        assert!(vaet_key(e(1), attr, e(9)) < vaet_key(e(2), attr, e(1)));
    }

    #[test]
    fn history_sorts_by_txn_last() {
        let (entity, attr) = (e(42), AttributeId::new(0x0100));
        let v = region(&Value::Long(30));
        let t1 = TxnId::new(EntityId::new(SYSTEM_PARTITION, 1).0);
        let t2 = TxnId::new(EntityId::new(SYSTEM_PARTITION, 2).0);
        assert!(eavt_history_key(entity, attr, &v, t1) < eavt_history_key(entity, attr, &v, t2));
    }

    #[test]
    fn prefixes_match_their_keys() {
        let (entity, attr) = (e(42), AttributeId::new(0x0100));
        let v = region(&Value::Long(30));
        let txn = TxnId::new(EntityId::new(SYSTEM_PARTITION, 1).0);
        assert!(eavt_key(entity, attr, &v).starts_with(&eavt_entity_prefix(entity)));
        assert!(eavt_key(entity, attr, &v).starts_with(&eavt_entity_attr_prefix(entity, attr)));
        assert!(aevt_key(entity, attr, &v).starts_with(&aevt_attr_prefix(attr)));
        assert!(avet_key(entity, attr, &v).starts_with(&avet_attr_prefix(attr)));
        assert!(avet_key(entity, attr, &v).starts_with(&avet_value_prefix(attr, &v)));
        assert!(vaet_key(e(43), attr, entity).starts_with(&vaet_target_prefix(e(43))));
        assert!(vaet_key(e(43), attr, entity).starts_with(&vaet_target_attr_prefix(e(43), attr)));
        assert!(txlog_key(txn, entity, attr, &v).starts_with(&txlog_prefix(txn)));
    }

    #[test]
    fn decoders_reject_short_keys() {
        assert!(decode_eavt_key(&[0; 5]).is_err());
        assert!(decode_avet_key(&[0; 5]).is_err());
        assert!(decode_vaet_key(&[0; 17]).is_err());
        assert!(decode_eavt_history_key(&[0; 12]).is_err());
        assert!(decode_avet_history_key(&[0; 12]).is_err());
        assert!(decode_vaet_history_key(&[0; 25]).is_err());
        assert!(decode_txlog_key(&[0; 12]).is_err());
    }
}
