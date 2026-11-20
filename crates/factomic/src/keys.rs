//! Defines key/prefix encodings and key decodings in various for use in byte-ordered indexes.
//!
//! Each field on the [Factom](crate::factom::Factom) is stored as a separate component on the key, except the last component, which becomes the value.
//!
//! | Letter | Name            | Example            | Length (bytes) |
//! |--------|-----------------|--------------------|----------------|
//! | E      | [EntityId]      | 0x2000000000000001 | 8              |
//! | A      | [AttributeId]   | 0x01               | 2              |
//! | S      | [Slot]          | 0x00               | variable       |
//! | V      | [Value]         | "Bob"              | variable       |
//! | T      | [TxnId]         | 0x1000000000000001 | 8              |
//!
//! The example column of the above demonstrates a single fact, decomposed:
//!
//! > Entity Person changed to "Bob" in "person/name" at One of via transaction.
//!
//! Because [fjall] sorts its keys by bytes, searches are `O(log n + k)` complexity where `k` is the number of results to read.
//!
//! Since orders are only cheap to scan by prefix, various indexes are kept of components in different orders.
//!
//! All indexes escape zero bytes with 0xFF (0x00 becomes 0x00_FF)
//!
//! Historical indexes terminate complete identities with 0x00_00.
//!
//! An identiy refers to the components that identify a fact independent of time and assertion.
//!
//! `E A S V || 0x00 00 || T`
//!
//! `A E S V || 0x00 00 || T`
//!
//! `A V E S || 0x00 00 || T`
//!
//! `V A E S || 0x00 00 || T`
//!
//!
//! This is to prevent T from being compared with anything other than T.
//!
//! Otherwise, for example, a fact with a longer V can compare against a shorter fact's T unintentionally.
//!
//! Temporal queries like [at](crate::store::Db::at) normalize current entries to the history representation in memory.

use crate::error::{Error, Result};
use crate::factom::Slot;
use crate::id::{AttributeId, EntityId, TxnId};
use crate::value::Value;

const E_LEN: usize = size_of::<EntityId>();
const A_LEN: usize = size_of::<AttributeId>();
const T_LEN: usize = size_of::<TxnId>();

const ZERO: u8 = 0x00;
const ESCAPED_ZERO: u8 = 0xFF;

pub(crate) fn escape_identity(identity: &[u8]) -> Vec<u8> {
    let zeroes = identity.iter().filter(|&&byte| byte == ZERO).count();
    let mut encoded = Vec::with_capacity(identity.len() + zeroes);
    for &byte in identity {
        encoded.push(byte);
        if byte == ZERO {
            encoded.push(ESCAPED_ZERO);
        }
    }
    encoded
}

pub(crate) fn encode_history_identity(identity: &[u8]) -> Vec<u8> {
    let mut encoded = escape_identity(identity);
    encoded.reserve_exact(2);
    encoded.extend_from_slice(&[ZERO, ZERO]);
    encoded
}

fn decode_identity(key: &[u8]) -> Result<Vec<u8>> {
    let mut identity = Vec::with_capacity(key.len());
    let mut index = 0;
    while index < key.len() {
        if key[index] != ZERO {
            identity.push(key[index]);
            index += 1;
            continue;
        }

        let Some(&escape) = key.get(index + 1) else {
            return Err(corrupt(key.len() + 1, key.len()));
        };
        match escape {
            ZERO => {
                if index + 2 != key.len() {
                    return Err(corrupt(index + 2, key.len()));
                }
                return Ok(identity);
            }
            ESCAPED_ZERO => {
                identity.push(ZERO);
                index += 2;
            }
            _ => return Err(corrupt(key.len() + 1, key.len())),
        }
    }
    Err(corrupt(key.len() + 2, key.len()))
}

/// For entity scans by everything else.
pub fn easvt_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(E_LEN + A_LEN + slot.encoded_len() + v.len());
    key.extend_from_slice(&e.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    slot.encode_key(&mut key);
    key.extend_from_slice(v);
    key
}

/// For attribute scans by entity; which attributes are contained in which entities.
pub fn aesvt_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(A_LEN + E_LEN + slot.encoded_len() + v.len());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(&e.to_be_bytes());
    slot.encode_key(&mut key);
    key.extend_from_slice(v);
    key
}

/// For attribute scans by value; which attributes have a value, or are within a range of values (since v is still ordered). Usually opt-in by flag.
pub fn avest_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(A_LEN + v.len() + E_LEN + slot.encoded_len());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(v);
    key.extend_from_slice(&e.to_be_bytes());
    slot.encode_key(&mut key);
    key
}

/// For value scans by attribute. This is only used for Ref values, which are entity ids, to find attributes/entities that reference a particular entity.
pub fn vaest_key(target: EntityId, a: AttributeId, source: EntityId, slot: &Slot) -> Vec<u8> {
    let mut key = Vec::with_capacity(E_LEN + A_LEN + E_LEN + slot.encoded_len());
    key.extend_from_slice(&target.to_be_bytes());
    key.extend_from_slice(&a.to_be_bytes());
    key.extend_from_slice(&source.to_be_bytes());
    slot.encode_key(&mut key);
    key
}

/// Same as [easvt_key] but where t is encoded since [Op](crate::factom::Op) is stored as value instead in the key value pair.
pub fn easvt_history_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = encode_history_identity(&easvt_key(e, a, slot, v));
    key.reserve_exact(T_LEN);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

/// Same as [aesvt_key] but where t is encoded since [Op](crate::factom::Op) is stored as value instead in the key value pair.
pub fn aesvt_history_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = encode_history_identity(&aesvt_key(e, a, slot, v));
    key.reserve_exact(T_LEN);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

/// Same as [avest_key] but where t is encoded since [Op](crate::factom::Op) is stored as value instead in the key value pair.
pub fn avest_history_key(e: EntityId, a: AttributeId, slot: &Slot, v: &[u8], t: TxnId) -> Vec<u8> {
    let mut key = encode_history_identity(&avest_key(e, a, slot, v));
    key.reserve_exact(T_LEN);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

/// Same as [vaest_key] but where t is encoded since [Op](crate::factom::Op) is stored as value instead in the key value pair.
pub fn vaest_history_key(
    target: EntityId,
    a: AttributeId,
    source: EntityId,
    slot: &Slot,
    t: TxnId,
) -> Vec<u8> {
    let mut key = encode_history_identity(&vaest_key(target, a, source, slot));
    key.reserve_exact(T_LEN);
    key.extend_from_slice(&t.to_be_bytes());
    key
}

// TEASV where E is sometimes the transaction itself.
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

fn decode_easvt_key_raw(key: &[u8]) -> Result<(EntityId, AttributeId, Slot, Value)> {
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

fn decode_aesvt_key_raw(key: &[u8]) -> Result<(AttributeId, EntityId, Slot, Value)> {
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

fn decode_avest_key_raw(key: &[u8]) -> Result<(AttributeId, Value, EntityId, Slot)> {
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

fn decode_vaest_key_raw(key: &[u8]) -> Result<(EntityId, AttributeId, EntityId, Slot)> {
    let lim = E_LEN + A_LEN + E_LEN + 1;
    if key.len() < lim {
        return Err(corrupt(lim, key.len()));
    }
    let (s, consumed) = Slot::decode_key(&key[E_LEN + A_LEN + E_LEN..])?;
    let expected = E_LEN + A_LEN + E_LEN + consumed;
    if key.len() != expected {
        return Err(corrupt(expected, key.len()));
    }
    Ok((
        EntityId::with_value(u64::from_be_bytes(key[0..8].try_into().unwrap())),
        AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap())),
        EntityId::with_value(u64::from_be_bytes(key[10..18].try_into().unwrap())),
        s,
    ))
}

pub fn decode_easvt_key(key: &[u8]) -> Result<(EntityId, AttributeId, Slot, Value)> {
    decode_easvt_key_raw(key)
}

#[allow(dead_code)]
pub fn decode_aesvt_key(key: &[u8]) -> Result<(AttributeId, EntityId, Slot, Value)> {
    decode_aesvt_key_raw(key)
}

pub fn decode_avest_key(key: &[u8]) -> Result<(AttributeId, Value, EntityId, Slot)> {
    decode_avest_key_raw(key)
}

pub fn decode_vaest_key(key: &[u8]) -> Result<(EntityId, AttributeId, EntityId, Slot)> {
    decode_vaest_key_raw(key)
}

fn decode_history_identity(key: &[u8]) -> Result<(Vec<u8>, TxnId)> {
    if key.len() < T_LEN + 2 {
        return Err(corrupt(T_LEN + 2, key.len()));
    }
    let txn_start = key.len() - T_LEN;
    let identity = decode_identity(&key[..txn_start])?;
    let txn = TxnId::with_value(u64::from_be_bytes(key[txn_start..].try_into().unwrap()));
    Ok((identity, txn))
}

pub fn decode_easvt_history_key(key: &[u8]) -> Result<(EntityId, AttributeId, Slot, Value, TxnId)> {
    let (identity, txn) = decode_history_identity(key)?;
    let (e, a, s, v) = decode_easvt_key_raw(&identity)?;
    Ok((e, a, s, v, txn))
}

#[allow(dead_code)]
pub fn decode_aesvt_history_key(key: &[u8]) -> Result<(AttributeId, EntityId, Slot, Value, TxnId)> {
    let (identity, txn) = decode_history_identity(key)?;
    let (a, e, s, v) = decode_aesvt_key_raw(&identity)?;
    Ok((a, e, s, v, txn))
}

#[allow(dead_code)]
pub fn decode_avest_history_key(key: &[u8]) -> Result<(AttributeId, Value, EntityId, Slot, TxnId)> {
    let (identity, txn) = decode_history_identity(key)?;
    let (a, v, e, s) = decode_avest_key_raw(&identity)?;
    Ok((a, v, e, s, txn))
}

#[allow(dead_code)]
pub fn decode_vaest_history_key(
    key: &[u8],
) -> Result<(EntityId, AttributeId, EntityId, Slot, TxnId)> {
    let (identity, txn) = decode_history_identity(key)?;
    let (v, a, e, s) = decode_vaest_key_raw(&identity)?;
    Ok((v, a, e, s, txn))
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
    let mut prefix = easvt_entity_prefix(e); // todo: prealloc
    prefix.extend_from_slice(&a.to_be_bytes());
    prefix
}

pub fn easvt_entity_attr_slot_prefix(e: EntityId, a: AttributeId, s: &Slot) -> Vec<u8> {
    let mut prefix = easvt_entity_prefix(e); // todo: prealloc
    prefix.extend_from_slice(&a.to_be_bytes());
    s.encode_key(&mut prefix);
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
    fn current_keys_remain_raw_and_history_encodes_identity() {
        let (entity, attr) = (e(42), AttributeId::new(0x0100));
        let slot = Slot::Empty;
        let region = region(&Value::String("a\0".into()));
        let mut raw = Vec::new();
        raw.extend_from_slice(&entity.to_be_bytes());
        raw.extend_from_slice(&attr.to_be_bytes());
        slot.encode_key(&mut raw);
        raw.extend_from_slice(&region);

        let current = easvt_key(entity, attr, &slot, &region);
        assert_eq!(current, raw);

        let txn = TxnId::new(SYSTEM_PARTITION, 1);
        let history = easvt_history_key(entity, attr, &slot, &region, txn);
        assert_eq!(
            history[..history.len() - T_LEN],
            encode_history_identity(&raw)
        );
        assert_eq!(&history[history.len() - T_LEN..], &txn.to_be_bytes());
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
    fn prefix_free_identity_round_trip() {
        for identity in [
            Vec::new(),
            vec![0],
            vec![0, 0],
            vec![0, 0xFF, 1, 0, 2],
            b"ordinary bytes".to_vec(),
        ] {
            assert_eq!(
                decode_identity(&encode_history_identity(&identity)).unwrap(),
                identity
            );
        }
        assert!(decode_identity(&[0, 0, 1]).is_err());
        assert!(decode_identity(&[0, 1]).is_err());
        assert!(decode_identity(&[1, 0]).is_err());
    }

    #[test]
    fn identity_encoding_is_prefix_free_and_order_preserving() {
        let mut identities = vec![Vec::new()];
        for len in 1..=3 {
            let previous = identities.clone();
            for prefix in previous.into_iter().filter(|key| key.len() == len - 1) {
                for byte in [0, 1, 0xFF] {
                    let mut identity = prefix.clone();
                    identity.push(byte);
                    identities.push(identity);
                }
            }
        }

        for left in &identities {
            for right in &identities {
                let encoded_left = encode_history_identity(left);
                let encoded_right = encode_history_identity(right);
                assert_eq!(encoded_left.cmp(&encoded_right), left.cmp(right));
                if left != right {
                    assert!(!encoded_left.starts_with(&encoded_right));
                }
            }
        }
    }

    #[test]
    fn prefix_values_do_not_produce_prefix_identities() {
        let (entity, attr) = (e(42), AttributeId::new(0x0100));
        let short = encode_history_identity(&easvt_key(
            entity,
            attr,
            &Slot::Empty,
            &region(&Value::String("a".into())),
        ));
        let long = encode_history_identity(&easvt_key(
            entity,
            attr,
            &Slot::Empty,
            &region(&Value::String("a\0".into())),
        ));
        assert!(!long.starts_with(&short));
        assert!(short < long);

        let t1 = TxnId::new(SYSTEM_PARTITION, 1);
        let t2 = TxnId::new(SYSTEM_PARTITION, 2);
        let t3 = TxnId::new(SYSTEM_PARTITION, 3);
        let short_region = region(&Value::String("a".into()));
        let long_region = region(&Value::String("a\0".into()));
        let short_t1 = easvt_history_key(entity, attr, &Slot::Empty, &short_region, t1);
        let short_t3 = easvt_history_key(entity, attr, &Slot::Empty, &short_region, t3);
        let long_t2 = easvt_history_key(entity, attr, &Slot::Empty, &long_region, t2);
        let mut events = vec![short_t3.clone(), long_t2.clone(), short_t1.clone()];
        events.sort();
        assert_eq!(events, vec![short_t1, short_t3, long_t2]);
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
