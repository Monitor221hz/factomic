use fractional_index::FractionalIndex;

use crate::error::{Error, Result};
use crate::id::{AttributeId, EntityId, TxnId};
use crate::primitive::{DecodePrimitive, EncodePrimitive};
use crate::value::{TAG_LONG, TAG_REF, TAG_STRING, Type, Value};

pub mod kvp;

const TAG_POSITION: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionKind {
    One,
    Many,
    List,
    Map,
}

impl CollectionKind {
    pub fn to_byte(self) -> u8 {
        match self {
            CollectionKind::One => 0x00,
            CollectionKind::Many => 0x01,
            CollectionKind::List => 0x02,
            CollectionKind::Map => 0x03,
        }
    }
}

impl TryFrom<u8> for CollectionKind {
    type Error = Error;

    fn try_from(byte: u8) -> Result<Self> {
        match byte {
            0x00 => Ok(CollectionKind::One),
            0x01 => Ok(CollectionKind::Many),
            0x02 => Ok(CollectionKind::List),
            0x03 => Ok(CollectionKind::Map),
            _ => Err(Error::UnknownKind(byte)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapKey {
    String(String),
    Long(i64),
    Ref(EntityId),
    // todo: bytes, timestamps, composites (low prio)
}

impl MapKey {
    pub fn value_type(&self) -> Type {
        match self {
            MapKey::String(_) => Type::String,
            MapKey::Long(_) => Type::Long,
            MapKey::Ref(_) => Type::Ref,
        }
    }
}

impl TryFrom<MapKey> for String {
    type Error = Error;

    fn try_from(value: MapKey) -> Result<Self> {
        match value {
            MapKey::String(s) => Ok(s),
            _ => Err(Error::MapKeyTypeMismatch),
        }
    }
}

impl TryFrom<MapKey> for EntityId {
    type Error = Error;

    fn try_from(value: MapKey) -> Result<Self> {
        match value {
            MapKey::Ref(e) => Ok(e),
            _ => Err(Error::MapKeyTypeMismatch),
        }
    }
}

impl TryFrom<MapKey> for i64 {
    type Error = Error;
    fn try_from(value: MapKey) -> Result<Self> {
        match value {
            MapKey::Long(i) => Ok(i),
            _ => Err(Error::MapKeyTypeMismatch),
        }
    }
}

impl From<String> for MapKey {
    fn from(value: String) -> Self {
        MapKey::String(value)
    }
}

impl From<&str> for MapKey {
    fn from(value: &str) -> Self {
        MapKey::String(value.into())
    }
}

impl From<i64> for MapKey {
    fn from(value: i64) -> Self {
        MapKey::Long(value)
    }
}

impl From<EntityId> for MapKey {
    fn from(value: EntityId) -> Self {
        MapKey::Ref(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    Empty,
    Position(Vec<u8>),
    Key(MapKey),
}

const TERMINATOR: u8 = 0x00;
const ESCAPED: u8 = 0xFF;
const FIXED_FRAME_LEN: usize = 10;
const MAX_FRAME_LEN: usize = 255;

impl Slot {
    pub fn position(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() || bytes.len() + 3 > MAX_FRAME_LEN {
            return Err(Error::PositionTooLong(bytes.len()));
        }
        Ok(Slot::Position(bytes))
    }

    pub(crate) fn with_index(index: &FractionalIndex) -> Result<Self> {
        Self::position(index.as_bytes().to_vec())
    }

    pub(crate) fn encoded_len(&self) -> usize {
        match self {
            Slot::Empty => 2,
            Slot::Position(pos) => 3 + pos.len(),
            Slot::Key(MapKey::String(s)) => {
                3 + s.as_bytes().iter().filter(|&&b| b == 0x00).count() + s.len()
            }
            Slot::Key(MapKey::Long(_) | MapKey::Ref(_)) => FIXED_FRAME_LEN,
        }
    }

    pub fn try_to_map_key(self) -> Result<MapKey> {
        match self {
            Slot::Key(map_key) => Ok(map_key),
            _ => Err(Error::CorruptPosition),
        }
    }

    pub fn encode_key(&self, buf: &mut Vec<u8>) {
        match self {
            Slot::Empty => {
                buf.push(0x00);
                buf.push(0x02);
            }
            Slot::Position(pos) => {
                buf.push(TAG_POSITION);
                buf.extend_from_slice(pos);
                buf.push(TERMINATOR);
                buf.push((pos.len() + 3) as u8);
            }
            Slot::Key(MapKey::String(s)) => {
                buf.push(TAG_STRING);
                for &b in s.as_bytes() {
                    if b == 0x00 {
                        buf.push(TERMINATOR);
                        buf.push(ESCAPED);
                    } else {
                        buf.push(b);
                    }
                }
                buf.push(TERMINATOR);
                buf.push((self.encoded_len()) as u8);
            }
            Slot::Key(MapKey::Long(v)) => {
                buf.push(TAG_LONG);
                v.encode(buf);
                buf.push(FIXED_FRAME_LEN as u8);
            }
            Slot::Key(MapKey::Ref(v)) => {
                buf.push(TAG_REF);
                v.encode(buf);
                buf.push(FIXED_FRAME_LEN as u8);
            }
        }
    }

    pub fn len_key(bytes: &[u8]) -> Result<usize> {
        let Some(&kind) = bytes.first() else {
            return Err(Error::CorruptKey {
                expected: 1,
                got: 0,
            });
        };
        match kind {
            0x00 => {
                if bytes.len() < 2 || bytes[1] != 0x02 {
                    return Err(Error::CorruptKey {
                        expected: 2,
                        got: bytes.len(),
                    });
                }
                Ok(2)
            }
            TAG_POSITION => {
                let Some(end) = bytes[1..].iter().position(|&b| b == TERMINATOR) else {
                    return Err(Error::CorruptKey {
                        expected: 4,
                        got: bytes.len(),
                    });
                };
                let frame = end + 3;
                if bytes.len() < frame || bytes[frame - 1] as usize != frame {
                    return Err(Error::CorruptKey {
                        expected: frame,
                        got: bytes.len(),
                    });
                }
                Ok(frame)
            }
            TAG_STRING => {
                let mut content = Vec::new();
                let mut i = 1;
                loop {
                    let Some(&b) = bytes.get(i) else {
                        return Err(Error::CorruptKey {
                            expected: 4,
                            got: bytes.len(),
                        });
                    };
                    match b {
                        ESCAPED => content.push(b),
                        TERMINATOR => match bytes.get(i + 1) {
                            Some(&ESCAPED) => {
                                content.push(0x00);
                                i += 1;
                            }
                            _ => break,
                        },
                        _ => content.push(b),
                    }
                    i += 1;
                }
                let frame = i + 2;
                if bytes.len() < frame || bytes[frame - 1] as usize != frame {
                    return Err(Error::CorruptKey {
                        expected: frame,
                        got: bytes.len(),
                    });
                }
                Ok(frame)
            }
            TAG_LONG | TAG_REF => {
                if bytes.len() < FIXED_FRAME_LEN
                    || bytes[FIXED_FRAME_LEN - 1] as usize != FIXED_FRAME_LEN
                {
                    return Err(Error::CorruptKey {
                        expected: FIXED_FRAME_LEN,
                        got: bytes.len(),
                    });
                }
                Ok(FIXED_FRAME_LEN)
            }
            _ => Err(Error::UnknownValueTag(kind)),
        }
    }

    pub fn decode_key(bytes: &[u8]) -> Result<(Self, usize)> {
        let Some(&kind) = bytes.first() else {
            return Err(Error::CorruptKey {
                expected: 1,
                got: 0,
            });
        };
        match kind {
            0x00 => {
                if bytes.len() < 2 || bytes[1] != 0x02 {
                    return Err(Error::CorruptKey {
                        expected: 2,
                        got: bytes.len(),
                    });
                }
                Ok((Slot::Empty, 2))
            }
            TAG_POSITION => {
                let Some(end) = bytes[1..].iter().position(|&b| b == TERMINATOR) else {
                    return Err(Error::CorruptKey {
                        expected: 4,
                        got: bytes.len(),
                    });
                };
                let frame = end + 3;
                if bytes.len() < frame || bytes[frame - 1] as usize != frame {
                    return Err(Error::CorruptKey {
                        expected: frame,
                        got: bytes.len(),
                    });
                }
                Ok((Slot::Position(bytes[1..end + 1].to_vec()), frame))
            }
            TAG_STRING => {
                let mut content = Vec::new();
                let mut i = 1;
                loop {
                    let Some(&b) = bytes.get(i) else {
                        return Err(Error::CorruptKey {
                            expected: 4,
                            got: bytes.len(),
                        });
                    };
                    match b {
                        ESCAPED => content.push(b),
                        TERMINATOR => match bytes.get(i + 1) {
                            Some(&ESCAPED) => {
                                content.push(0x00);
                                i += 1;
                            }
                            _ => break,
                        },
                        _ => content.push(b),
                    }
                    i += 1;
                }
                let frame = i + 2;
                if bytes.len() < frame || bytes[frame - 1] as usize != frame {
                    return Err(Error::CorruptKey {
                        expected: frame,
                        got: bytes.len(),
                    });
                }
                Ok((Slot::Key(MapKey::String(String::decode(&content)?)), frame))
            }
            TAG_LONG | TAG_REF => {
                if bytes.len() < FIXED_FRAME_LEN
                    || bytes[FIXED_FRAME_LEN - 1] as usize != FIXED_FRAME_LEN
                {
                    return Err(Error::CorruptKey {
                        expected: FIXED_FRAME_LEN,
                        got: bytes.len(),
                    });
                }
                let body = &bytes[1..FIXED_FRAME_LEN - 1];
                let slot = if kind == TAG_LONG {
                    Slot::Key(MapKey::Long(i64::decode(body)?))
                } else {
                    Slot::Key(MapKey::Ref(EntityId::decode(body)?))
                };
                Ok((slot, FIXED_FRAME_LEN))
            }
            _ => Err(Error::UnknownValueTag(kind)),
        }
    }

    pub fn decode_key_back(bytes: &[u8]) -> Result<(Self, usize)> {
        let Some(&last) = bytes.last() else {
            return Err(Error::CorruptKey {
                expected: 1,
                got: 0,
            });
        };
        let frame = last as usize;
        if frame < 2 || bytes.len() < frame {
            return Err(Error::CorruptKey {
                expected: frame,
                got: bytes.len(),
            });
        }
        let start = bytes.len() - frame;
        let (slot, consumed) = Self::decode_key(&bytes[start..])?;
        if consumed != frame {
            return Err(Error::CorruptKey {
                expected: frame,
                got: consumed,
            });
        }
        Ok((slot, consumed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Assert,
    Retract,
}

pub(crate) const ASSERT: u8 = 0x00;
pub(crate) const RETRACT: u8 = 0x01;

impl Op {
    pub fn to_byte(self) -> u8 {
        match self {
            Op::Assert => ASSERT,
            Op::Retract => RETRACT,
        }
    }
}

impl TryFrom<u8> for Op {
    type Error = Error;

    fn try_from(byte: u8) -> Result<Self> {
        match byte {
            ASSERT => Ok(Op::Assert),
            RETRACT => Ok(Op::Retract),
            _ => Err(Error::UnknownOp(byte)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Factom {
    entity: EntityId,
    attribute: AttributeId,
    slot: Slot,
    value: Value,
    txn: TxnId,
    op: Op,
}

impl Factom {
    pub fn new(
        entity: EntityId,
        attribute: AttributeId,
        slot: Slot,
        value: Value,
        txn: TxnId,
        op: Op,
    ) -> Self {
        Factom {
            entity,
            attribute,
            slot,
            value,
            txn,
            op,
        }
    }

    pub fn entity(&self) -> EntityId {
        self.entity
    }

    pub fn attribute(&self) -> AttributeId {
        self.attribute
    }

    pub fn slot(&self) -> &Slot {
        &self.slot
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn txn(&self) -> TxnId {
        self.txn
    }

    pub fn op(&self) -> Op {
        self.op
    }

    pub fn try_from_value<'a, T>(&'a self) -> Result<T>
    where
        T: TryFrom<&'a Value, Error = crate::Error>,
    {
        T::try_from(self.value())
    }

    pub fn try_to_value<T>(self) -> Result<T>
    where
        T: TryFrom<Value, Error = crate::Error>,
    {
        T::try_from(self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{SYSTEM_PARTITION, USER_PARTITION};

    fn round_trip(slot: &Slot) {
        let mut buf = Vec::new();
        slot.encode_key(&mut buf);
        let (front, front_consumed) = Slot::decode_key(&buf).unwrap();
        assert_eq!(&front, slot);
        assert_eq!(front_consumed, buf.len());
        let (back, back_consumed) = Slot::decode_key_back(&buf).unwrap();
        assert_eq!(&back, slot);
        assert_eq!(back_consumed, buf.len());
    }

    #[test]
    fn slot_round_trips_all_variants() {
        round_trip(&Slot::Empty);
        round_trip(&Slot::Position(b"a0".to_vec()));
        round_trip(&Slot::Key(MapKey::String("hello".into())));
        round_trip(&Slot::Key(MapKey::String(String::new())));
        round_trip(&Slot::Key(MapKey::Long(42)));
        round_trip(&Slot::Key(MapKey::Ref(EntityId::new(USER_PARTITION, 7))));
    }

    #[test]
    fn slot_back_parse_handles_zero_bytes_in_content() {
        round_trip(&Slot::Key(MapKey::Long(256)));
        round_trip(&Slot::Key(MapKey::String("\u{2}\u{0}".into())));
    }

    #[test]
    fn slot_back_parse_ignores_preceding_bytes() {
        let mut buf = vec![0xAA; 8];
        Slot::Key(MapKey::Long(256)).encode_key(&mut buf);
        let (slot, consumed) = Slot::decode_key_back(&buf).unwrap();
        assert_eq!(slot, Slot::Key(MapKey::Long(256)));
        assert_eq!(consumed, buf.len() - 8);
    }

    #[test]
    fn slot_empty_sorts_first() {
        let mut empty = Vec::new();
        Slot::Empty.encode_key(&mut empty);
        let mut position = Vec::new();
        Slot::Position(b"a0".to_vec()).encode_key(&mut position);
        let mut key = Vec::new();
        Slot::Key(MapKey::String("a".into())).encode_key(&mut key);
        assert!(empty < position);
        assert!(empty < key);
    }

    #[test]
    fn position_constructor_validates_length() {
        assert!(matches!(
            Slot::position(Vec::new()),
            Err(Error::PositionTooLong(0))
        ));
        assert!(matches!(
            Slot::position(vec![0; 255]),
            Err(Error::PositionTooLong(255))
        ));
        assert!(Slot::position(b"a0".to_vec()).is_ok());
    }

    #[test]
    fn collection_kind_byte_round_trip() {
        for kind in [
            CollectionKind::One,
            CollectionKind::Many,
            CollectionKind::List,
            CollectionKind::Map,
        ] {
            assert_eq!(CollectionKind::try_from(kind.to_byte()).unwrap(), kind);
        }
        assert!(matches!(
            CollectionKind::try_from(0x04),
            Err(Error::UnknownKind(0x04))
        ));
    }

    #[test]
    fn op_byte_round_trip() {
        assert_eq!(Op::try_from(Op::Assert.to_byte()).unwrap(), Op::Assert);
        assert_eq!(Op::try_from(Op::Retract.to_byte()).unwrap(), Op::Retract);
        assert!(matches!(Op::try_from(0x02), Err(Error::UnknownOp(0x02))));
    }

    #[test]
    fn factom_accessors() {
        let e = EntityId::new(USER_PARTITION, 1);
        let a = AttributeId::new(0x0100);
        let t = TxnId::new(SYSTEM_PARTITION, 1);
        let slot = Slot::Empty;
        let factom = Factom::new(e, a, slot.clone(), Value::Long(30), t, Op::Assert);
        assert_eq!(factom.entity(), e);
        assert_eq!(factom.attribute(), a);
        assert_eq!(factom.slot(), &slot);
        assert_eq!(factom.value(), &Value::Long(30));
        assert_eq!(factom.txn(), t);
        assert_eq!(factom.op(), Op::Assert);
    }
}
