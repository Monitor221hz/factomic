use crate::error::{Error, Result};
use crate::factom::Slot;
use crate::id::{AttributeId, EntityId, TxnId};
use crate::value::Value;

const E_LEN: usize = size_of::<EntityId>();
const A_LEN: usize = size_of::<AttributeId>();
const T_LEN: usize = size_of::<TxnId>();

pub fn easvt_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(E_LEN + A_LEN + slot.encoded_len() + v.len());
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    slot.encode_key(&mut key);
    key.extend_from_slice(v);
    key
}

pub fn aesvt_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(A_LEN + E_LEN + slot.encoded_len() + v.len());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(&e.to_be_bytes());
    slot.encode_key(&mut key);
    key.extend_from_slice(v);
    key
}

pub fn avest_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(A_LEN + v.len() + E_LEN + slot.encoded_len());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(v);
    key.extend_from_slice(&e.to_be_bytes());
    slot.encode_key(&mut key);
    key
}

pub fn vaest_key(target: EntityId, a: AttributeId, source: EntityId, slot: &Slot) -> Vec<u8> {
    let mut key = Vec::with_capacity(E_LEN + A_LEN + E_LEN + slot.encoded_len());
    key.extend_from_slice(&target.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(&source.to_be_bytes());
    slot.encode_key(&mut key);
    key
}

pub fn easvt_history_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = easvt_key(e, a, slot, v);
    key.reserve_exact(T_LEN);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

pub fn aesvt_history_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = aesvt_key(e, a, slot, v);
    key.reserve_exact(T_LEN);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

pub fn avest_history_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = avest_key(e, a, slot, v);
    key.reserve_exact(T_LEN);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

pub fn vaest_history_key(
    target: EntityId,
    a: AttributeId,
    source: EntityId,
    slot: &Slot,
    t: TxnId,
) -> Vec<u8> {
    let mut key = vaest_key(target, a, source, slot);
    key.reserve_exact(T_LEN);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

pub fn txlog_key(t: TxnId, e: EntityId, a: AttributeId, slot: &Slot, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(T_LEN + E_LEN + A_LEN + slot.encoded_len() + v.len());
    key.extend_from_slice(&t.to_be_bytes());
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    slot.encode_key(&mut key);
    key.extend_from_slice(v);
    key
}

fn corrupt(expected: usize, got: usize) -> Error {
    Error::CorruptKey { expected, got }
}

pub fn decode_easvt_key(key: &[u8]) -> Result<(EntityId, AttributeId, Slot, Value)> {
    let lim = E_LEN + A_LEN + 2;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, consumed) = Slot::decode_key(&key[E_LEN + A_LEN..])?;
    Ok((
        EntityId::with_value(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        s,
        Value::decode_key(&key[E_LEN + A_LEN + consumed..])?,
    ))
}

#[allow(dead_code)]
pub fn decode_aesvt_key(key: &[u8]) -> Result<(AttributeId, EntityId, Slot, Value)> {
    let lim = A_LEN + E_LEN + 2;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, consumed) = Slot::decode_key(&key[A_LEN + E_LEN..])?;
    Ok((
        AttributeId::new(u16::from_be_bytes(key[0..2].try_into().unwrap())),
        EntityId::with_value(u64::from_be_bytes(key[2..10].try_into().unwrap())),
        s,
        Value::decode_key(&key[A_LEN + E_LEN + consumed..])?,
    ))
}

pub fn decode_avest_key(key: &[u8]) -> Result<(AttributeId, Value, EntityId, Slot)> {
    let lim = A_LEN + 1 + E_LEN + 1;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, k_len) = Slot::decode_key_back(key)?;
    let e_start = key.len() - k_len - E_LEN;
    Ok((
        AttributeId::new(u16::from_be_bytes(key[0..2].try_into().unwrap())),
        Value::decode_key(&key[A_LEN..e_start])?,
        EntityId::with_value(u64::from_be_bytes(
            key[e_start..key.len() - k_len].try_into().unwrap(),
        )),
        s,
    ))
}

pub fn decode_vaest_key(key: &[u8]) -> Result<(EntityId, AttributeId, EntityId, Slot)> {
    let lim = E_LEN + A_LEN + E_LEN + 1;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, _) = Slot::decode_key(&key[E_LEN + A_LEN + E_LEN..])?;
    Ok((
        EntityId::with_value(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        EntityId::with_value(u64::from_be_bytes(key[10..18].try_into().unwrap())),
        s,
    ))
}

pub fn decode_easvt_history_key(key: &[u8]) -> Result<(EntityId, AttributeId, Slot, Value, TxnId)> {
    let lim = E_LEN + A_LEN + 2 + T_LEN;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, consumed) = Slot::decode_key(&key[E_LEN + A_LEN..])?;
    let v_end = key.len() - T_LEN;
    Ok((
        EntityId::with_value(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        s,
        Value::decode_key(&key[E_LEN + A_LEN + consumed..v_end])?,
        TxnId::with_value(u64::from_be_bytes(key[v_end..].try_into().unwrap())),
    ))
}

#[allow(dead_code)]
pub fn decode_aesvt_history_key(key: &[u8]) -> Result<(AttributeId, EntityId, Slot, Value, TxnId)> {
    let lim = A_LEN + E_LEN + 2 + T_LEN;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, consumed) = Slot::decode_key(&key[A_LEN + E_LEN..])?;
    let v_end = key.len() - T_LEN;
    Ok((
        AttributeId::new(u16::from_be_bytes(key[0..2].try_into().unwrap())),
        EntityId::with_value(u64::from_be_bytes(key[2..10].try_into().unwrap())),
        s,
        Value::decode_key(&key[A_LEN + E_LEN + consumed..v_end])?,
        TxnId::with_value(u64::from_be_bytes(key[v_end..].try_into().unwrap())),
    ))
}

#[allow(dead_code)]
pub fn decode_avest_history_key(key: &[u8]) -> Result<(AttributeId, Value, EntityId, Slot, TxnId)> {
    let lim = A_LEN + 1 + E_LEN + 1 + T_LEN;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let body = &key[..key.len() - T_LEN];
    let (s, k_len) = Slot::decode_key_back(body)?;
    let e_start = body.len() - k_len - E_LEN;
    Ok((
        AttributeId::new(u16::from_be_bytes(key[0..2].try_into().unwrap())),
        Value::decode_key(&body[A_LEN..e_start])?,
        EntityId::with_value(u64::from_be_bytes(
            body[e_start..body.len() - k_len].try_into().unwrap(),
        )),
        s,
        TxnId::with_value(u64::from_be_bytes(
            key[key.len() - T_LEN..].try_into().unwrap(),
        )),
    ))
}

#[allow(dead_code)]
pub fn decode_vaest_history_key(
    key: &[u8],
) -> Result<(EntityId, AttributeId, EntityId, Slot, TxnId)> {
    let lim = E_LEN + A_LEN + E_LEN + 1 + T_LEN;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, consumed) = Slot::decode_key(&key[E_LEN + A_LEN + E_LEN..])?;
    let t_start = E_LEN + A_LEN + E_LEN + consumed;
    if key.len() != t_start + T_LEN {
        return Err(corrupt(t_start + T_LEN, key.len()));
    }
    Ok((
        EntityId::with_value(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        EntityId::with_value(u64::from_be_bytes(key[10..18].try_into().unwrap())),
        s,
        TxnId::with_value(u64::from_be_bytes(key[t_start..].try_into().unwrap())),
    ))
}

pub fn decode_txlog_key(key: &[u8]) -> Result<(TxnId, EntityId, AttributeId, Slot, Value)> {
    let lim = T_LEN + E_LEN + A_LEN + 2;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, consumed) = Slot::decode_key(&key[T_LEN + E_LEN + A_LEN..])?;
    Ok((
        TxnId::with_value(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        EntityId::with_value(u64::from_be_bytes(key[8..16].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[16..18].try_into().unwrap())),
        s,
        Value::decode_key(&key[T_LEN + E_LEN + A_LEN + consumed..])?,
    ))
}

pub fn easvt_entity_prefix(e: EntityId) -> Vec<u8> {
    e.to_be_bytes().to_vec()
}

pub fn easvt_entity_attr_prefix(e: EntityId, a: AttributeId) -> Vec<u8> {
    let mut prefix = easvt_entity_prefix(e);
    prefix.extend_from_slice(&a.to_be_bytes());
    prefix
}

#[allow(dead_code)]
pub fn aesvt_attr_prefix(a: AttributeId) -> Vec<u8> {
    a.to_be_bytes().to_vec()
}

pub fn avest_attr_prefix(a: AttributeId) -> Vec<u8> {
    a.to_be_bytes().to_vec()
}

pub fn avest_value_prefix(a: AttributeId, v: &[u8]) -> Vec<u8> {
    let mut prefix = avest_attr_prefix(a);
    prefix.extend_from_slice(v);
    prefix
}

pub fn vaest_target_prefix(target: EntityId) -> Vec<u8> {
    target.to_be_bytes().to_vec()
}

pub fn vaest_target_attr_prefix(target: EntityId, a: AttributeId) -> Vec<u8> {
    let mut prefix = vaest_target_prefix(target);
    prefix.extend_from_slice(&a.to_be_bytes());
    prefix
}

pub fn txlog_prefix(t: TxnId) -> Vec<u8> {
    t.to_be_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factom::MapKey;
    use crate::id::{SYSTEM_PARTITION, USER_PARTITION};

    fn e(index: u64) -> EntityId {
        EntityId::new(USER_PARTITION, index)
    }

    fn region(v: &Value) -> Vec<u8> {
        let mut buf = Vec::new();
        v.encode_key(&mut buf);
        buf
    }

    fn position() -> Slot {
        Slot::Position(b"a0".to_vec())
    }

    #[test]
    fn easvt_round_trip() {
        let (entity, attr, k, value) = (
            e(42),
            AttributeId::new(0x0100),
            position(),
            Value::String("alice".into()),
        );
        let key = easvt_key(entity, attr, &k, &region(&value));
        assert_eq!(decode_easvt_key(&key).unwrap(), (entity, attr, k, value));
    }

    #[test]
    fn aesvt_round_trip() {
        let (entity, attr, k, value) = (
            e(42),
            AttributeId::new(0x0100),
            Slot::Empty,
            Value::Long(30),
        );
        let key = aesvt_key(entity, attr, &k, &region(&value));
        assert_eq!(decode_aesvt_key(&key).unwrap(), (attr, entity, k, value));
    }

    #[test]
    fn avest_round_trip() {
        let (entity, attr, k, value) =
            (e(42), AttributeId::new(0x0100), position(), Value::Long(-5));
        let key = avest_key(entity, attr, &k, &region(&value));
        assert_eq!(decode_avest_key(&key).unwrap(), (attr, value, entity, k));
    }

    #[test]
    fn avest_round_trip_with_string_value_and_map_key() {
        let (entity, attr) = (e(42), AttributeId::new(0x0100));
        let s = Slot::Key(MapKey::String("key".into()));
        let value = Value::String("variable length value".into());
        let key = avest_key(entity, attr, &s, &region(&value));
        assert_eq!(decode_avest_key(&key).unwrap(), (attr, value, entity, s));
    }

    #[test]
    fn vaest_round_trip() {
        let (target, attr, source, k) = (e(43), AttributeId::new(0x0102), e(42), position());
        let key = vaest_key(target, attr, source, &k);
        assert_eq!(decode_vaest_key(&key).unwrap(), (target, attr, source, k));
    }

    #[test]
    fn history_round_trips() {
        let (entity, attr, k, value) =
            (e(42), AttributeId::new(0x0100), position(), Value::Long(30));
        let txn = TxnId::new(SYSTEM_PARTITION, 100);
        let region = region(&value);
        assert_eq!(
            decode_easvt_history_key(&easvt_history_key(entity, attr, &k, &region, txn)).unwrap(),
            (entity, attr, k.clone(), value.clone(), txn)
        );
        assert_eq!(
            decode_aesvt_history_key(&aesvt_history_key(entity, attr, &k, &region, txn)).unwrap(),
            (attr, entity, k.clone(), value.clone(), txn)
        );
        assert_eq!(
            decode_avest_history_key(&avest_history_key(entity, attr, &k, &region, txn)).unwrap(),
            (attr, value, entity, k, txn)
        );
    }

    #[test]
    fn vaest_history_round_trip() {
        let (target, attr, source, k) = (e(43), AttributeId::new(0x0102), e(42), Slot::Empty);
        let txn = TxnId::new(SYSTEM_PARTITION, 100);
        let key = vaest_history_key(target, attr, source, &k, txn);
        assert_eq!(
            decode_vaest_history_key(&key).unwrap(),
            (target, attr, source, k, txn)
        );
    }

    #[test]
    fn txlog_round_trip() {
        let (entity, attr, k, value) = (
            e(42),
            AttributeId::new(0x0100),
            position(),
            Value::Bool(true),
        );
        let txn = TxnId::new(SYSTEM_PARTITION, 100);
        let key = txlog_key(txn, entity, attr, &k, &region(&value));
        assert_eq!(
            decode_txlog_key(&key).unwrap(),
            (txn, entity, attr, k, value)
        );
    }

    #[test]
    fn easvt_sorts_by_entity_then_attribute() {
        let attr_a = AttributeId::new(0x0100);
        let attr_b = AttributeId::new(0x0101);
        let v = region(&Value::Long(1));
        assert!(
            easvt_key(e(1), attr_b, &Slot::Empty, &v) < easvt_key(e(2), attr_a, &Slot::Empty, &v)
        );
        assert!(
            easvt_key(e(1), attr_a, &Slot::Empty, &v) < easvt_key(e(1), attr_b, &Slot::Empty, &v)
        );
    }

    #[test]
    fn easvt_sorts_by_k_within_attribute() {
        let attr = AttributeId::new(0x0100);
        let v = region(&Value::Long(1));
        let empty = easvt_key(e(1), attr, &Slot::Empty, &v);
        let first = easvt_key(e(1), attr, &Slot::Position(b"a0".to_vec()), &v);
        let second = easvt_key(e(1), attr, &Slot::Position(b"a1".to_vec()), &v);
        assert!(empty < first);
        assert!(first < second);
    }

    #[test]
    fn avest_sorts_by_value_within_attribute() {
        let attr = AttributeId::new(0x0100);
        let k29 = avest_key(e(2), attr, &Slot::Empty, &region(&Value::Long(29)));
        let k30 = avest_key(e(1), attr, &Slot::Empty, &region(&Value::Long(30)));
        assert!(k29 < k30);
        let neg = avest_key(e(1), attr, &Slot::Empty, &region(&Value::Long(-1)));
        assert!(neg < k29);
    }

    #[test]
    fn avest_same_value_sorts_by_entity_then_k() {
        let attr = AttributeId::new(0x0100);
        let v = region(&Value::Long(30));
        assert!(avest_key(e(1), attr, &Slot::Empty, &v) < avest_key(e(2), attr, &Slot::Empty, &v));
        let e1_first = avest_key(e(1), attr, &Slot::Position(b"a0".to_vec()), &v);
        let e1_second = avest_key(e(1), attr, &Slot::Position(b"a1".to_vec()), &v);
        assert!(e1_first < e1_second);
    }

    #[test]
    fn vaest_sorts_by_target() {
        let attr = AttributeId::new(0x0102);
        assert!(
            vaest_key(e(1), attr, e(9), &Slot::Empty) < vaest_key(e(2), attr, e(1), &Slot::Empty)
        );
    }

    #[test]
    fn history_sorts_by_txn_last() {
        let (entity, attr) = (e(42), AttributeId::new(0x0100));
        let v = region(&Value::Long(30));
        let t1 = TxnId::new(SYSTEM_PARTITION, 1);
        let t2 = TxnId::new(SYSTEM_PARTITION, 2);
        assert!(
            easvt_history_key(entity, attr, &Slot::Empty, &v, t1)
                < easvt_history_key(entity, attr, &Slot::Empty, &v, t2)
        );
    }

    #[test]
    fn prefixes_match_their_keys() {
        let (entity, attr) = (e(42), AttributeId::new(0x0100));
        let v = region(&Value::Long(30));
        let txn = TxnId::new(SYSTEM_PARTITION, 1);
        assert!(
            easvt_key(entity, attr, &Slot::Empty, &v).starts_with(&easvt_entity_prefix(entity))
        );
        assert!(
            easvt_key(entity, attr, &Slot::Empty, &v)
                .starts_with(&easvt_entity_attr_prefix(entity, attr))
        );
        assert!(aesvt_key(entity, attr, &Slot::Empty, &v).starts_with(&aesvt_attr_prefix(attr)));
        assert!(avest_key(entity, attr, &Slot::Empty, &v).starts_with(&avest_attr_prefix(attr)));
        assert!(
            avest_key(entity, attr, &Slot::Empty, &v).starts_with(&avest_value_prefix(attr, &v))
        );
        assert!(
            vaest_key(e(43), attr, entity, &Slot::Empty).starts_with(&vaest_target_prefix(e(43)))
        );
        assert!(
            vaest_key(e(43), attr, entity, &Slot::Empty)
                .starts_with(&vaest_target_attr_prefix(e(43), attr))
        );
        assert!(txlog_key(txn, entity, attr, &Slot::Empty, &v).starts_with(&txlog_prefix(txn)));
    }

    #[test]
    fn decoders_reject_short_keys() {
        assert!(decode_easvt_key(&[0; 5]).is_err());
        assert!(decode_avest_key(&[0; 5]).is_err());
        assert!(decode_vaest_key(&[0; 17]).is_err());
        assert!(decode_easvt_history_key(&[0; 12]).is_err());
        assert!(decode_avest_history_key(&[0; 12]).is_err());
        assert!(decode_vaest_history_key(&[0; 25]).is_err());
        assert!(decode_txlog_key(&[0; 12]).is_err());
    }

    #[test]
    fn keys_reserve_exact_capacity() {
        let entity = e(42);
        let attr = AttributeId::new(0x0100);
        let slot = position();
        let v = region(&Value::String("a longer value".into()));
        let txn = TxnId::new(SYSTEM_PARTITION, 1);
        for key in [
            easvt_key(entity, attr, &slot, &v),
            aesvt_key(entity, attr, &slot, &v),
            avest_key(entity, attr, &slot, &v),
            vaest_key(e(43), attr, entity, &slot),
            easvt_history_key(entity, attr, &slot, &v, txn),
            txlog_key(txn, entity, attr, &slot, &v),
        ] {
            assert_eq!(key.capacity(), key.len());
        }
    }
}
