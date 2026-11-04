use jiff::Timestamp;

pub mod transact;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    Bool,
    Long,
    Double,
    String,
    Bytes,
    Ref,
    Timestamp,
    // uuid (feat.)
    // keyword/enum values
    // tuple
    // bg
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("storage error")]
    Storage(#[from] fjall::Error),

    #[error("unknown attribute: {0}")]
    UnknownAttribute(String),

    #[error("type mismatch on {attribute}: expected {expected:?}, got {got:?}")]
    TypeMismatch {
        attribute: String,
        expected: Type,
        got: Type,
    },

    #[error("attribute {0} already exists with a different value type")]
    SchemaConflict(String),

    #[error("corrupt value for type {0:?}")]
    CorruptPrimitive(Type),

    #[error("unknown value tag: {0:#04x}")]
    UnknownValueTag(u8),

    #[error("unknown op byte: {0:#04x}")]
    UnknownOp(u8),

    #[error("corrupt key: expected at least {expected} bytes, got {got}")]
    CorruptKey { expected: usize, got: usize },

    #[error("utf 8 err")]
    StringError(#[from] std::str::Utf8Error),
}

pub type Result<T> = std::result::Result<T, Error>;
pub trait Primitive {
    const TYPE: Type;
}

pub trait EncodePrimitive: Primitive {
    fn encode(&self, buf: &mut Vec<u8>);
}

pub trait DecodePrimitive<'a>: Sized + Primitive {
    fn decode(bytes: &'a [u8]) -> Result<Self>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttributeId(u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TxnId(u64);

pub const SYSTEM_PARTITION: u8 = 0x01;
pub const USER_PARTITION: u8 = 0x02;
pub const TX_TIMESTAMP_ATTR: AttributeId = AttributeId(0x0001);
pub const FIRST_USER_ATTR: u16 = 0x0100;

impl EntityId {
    pub fn new(partition: u8, index: u64) -> Self {
        assert!(index < (1 << 56));
        let shifted = (partition as u64) << 56;
        EntityId(shifted | index)
    }

    pub fn partition(&self) -> u8 {
        (self.0 >> 56) as u8
    }

    pub fn index(&self) -> u64 {
        self.0 & 0x00FF_FFFF_FFFF_FFFF
    }

    pub fn to_be_bytes(&self) -> [u8; 8] {
        self.0.to_be_bytes()
    }
}

impl AttributeId {
    pub fn new(id: u16) -> Self {
        AttributeId(id)
    }

    pub fn as_u16(&self) -> u16 {
        self.0
    }

    pub fn to_be_bytes(&self) -> [u8; size_of::<Self>()] {
        self.as_u16().to_be_bytes()
    }
}

impl TxnId {
    pub fn new(id: u64) -> Self {
        TxnId(id)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn to_be_bytes(&self) -> [u8; size_of::<Self>()] {
        self.as_u64().to_be_bytes()
    }

    pub fn entity_id(&self) -> EntityId {
        EntityId(self.as_u64())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Long(i64),
    Double(f64),
    String(String),
    Bytes(Vec<u8>),
    Ref(EntityId),
    Instant(Timestamp),
}

pub const TAG_BOOL: u8 = 0x01;
pub const TAG_LONG: u8 = 0x02;
pub const TAG_DOUBLE: u8 = 0x03;
pub const TAG_STRING: u8 = 0x04;
pub const TAG_BYTES: u8 = 0x05;
pub const TAG_REF: u8 = 0x06;
pub const TAG_INSTANT: u8 = 0x07;

pub fn tag_for_type(ty: Type) -> u8 {
    match ty {
        Type::Bool => TAG_BOOL,
        Type::Long => TAG_LONG,
        Type::Double => TAG_DOUBLE,
        Type::String => TAG_STRING,
        Type::Bytes => TAG_BYTES,
        Type::Ref => TAG_REF,
        Type::Timestamp => TAG_INSTANT,
    }
}

pub fn type_for_tag(tag: u8) -> Result<Type> {
    match tag {
        TAG_BOOL => Ok(Type::Bool),
        TAG_LONG => Ok(Type::Long),
        TAG_DOUBLE => Ok(Type::Double),
        TAG_STRING => Ok(Type::String),
        TAG_BYTES => Ok(Type::Bytes),
        TAG_REF => Ok(Type::Ref),
        TAG_INSTANT => Ok(Type::Timestamp),
        _ => Err(Error::UnknownValueTag(tag)),
    }
}

impl Value {
    pub fn value_type(&self) -> Type {
        match self {
            Value::Bool(_) => Type::Bool,
            Value::Long(_) => Type::Long,
            Value::Double(_) => Type::Double,
            Value::String(_) => Type::String,
            Value::Bytes(_) => Type::Bytes,
            Value::Ref(_) => Type::Ref,
            Value::Instant(_) => Type::Timestamp,
        }
    }

    pub fn tag(&self) -> u8 {
        tag_for_type(self.value_type())
    }

    pub fn encode_key(&self, buf: &mut Vec<u8>) {
        buf.push(self.tag());
        match self {
            Value::Bool(v) => v.encode(buf),
            Value::Long(v) => v.encode(buf),
            Value::Double(v) => v.encode(buf),
            Value::String(v) => v.encode(buf),
            Value::Bytes(v) => buf.extend_from_slice(v),
            Value::Ref(v) => v.encode(buf),
            Value::Instant(v) => v.encode(buf),
        };
    }

    pub fn decode_key(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::CorruptKey {
                expected: 1,
                got: 0,
            });
        }

        let tag = bytes[0];
        let body = &bytes[1..];
        match tag {
            TAG_BOOL => Ok(Value::Bool(bool::decode(body)?)),
            TAG_LONG => Ok(Value::Long(i64::decode(body)?)),
            TAG_DOUBLE => Ok(Value::Double(f64::decode(body)?)),
            TAG_STRING => Ok(Value::String(String::decode(body)?)),
            TAG_BYTES => Ok(Value::Bytes(body.to_vec())),
            TAG_REF => Ok(Value::Ref(EntityId::decode(body)?)),
            TAG_INSTANT => Ok(Value::Instant(Timestamp::decode(body)?)),
            _ => Err(Error::UnknownValueTag(tag)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Assert,
    Retract,
}

pub const ASSERT: u8 = 0x00;
pub const RETRACT: u8 = 0x01;

impl Op {
    pub fn to_byte(self) -> u8 {
        match self {
            Op::Assert => ASSERT,
            Op::Retract => RETRACT,
        }
    }

    pub fn from_byte(byte: u8) -> Result<Self> {
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
    value: Value,
    txn: TxnId,
    op: Op,
}

impl Factom {
    pub fn new(entity: EntityId, attribute: AttributeId, value: Value, txn: TxnId, op: Op) -> Self {
        Factom {
            entity,
            attribute,
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

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn txn(&self) -> TxnId {
        self.txn
    }

    pub fn op(&self) -> Op {
        self.op
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_partition_round_trip() {
        let id = EntityId::new(USER_PARTITION, 42);
        assert_eq!(id.partition(), USER_PARTITION);
        assert_eq!(id.index(), 42);
    }

    #[test]
    #[should_panic]
    fn entity_id_index_overflow_panics() {
        let _ = EntityId::new(USER_PARTITION, 1 << 56);
    }

    #[test]
    fn entity_id_partition_sorts_before_index() {
        let system = EntityId::new(SYSTEM_PARTITION, u64::MAX >> 8);
        let user = EntityId::new(USER_PARTITION, 1);
        assert!(system.0.to_be_bytes() < user.0.to_be_bytes());
    }

    #[test]
    fn attribute_id_bytes_round_trip() {
        let id = AttributeId::new(0x0100);
        assert_eq!(u16::from_be_bytes(id.to_be_bytes()), id.as_u16());
    }

    #[test]
    fn txn_id_is_an_entity_id() {
        let txn = TxnId::new(EntityId::new(SYSTEM_PARTITION, 7).0);
        assert_eq!(txn.entity_id().partition(), SYSTEM_PARTITION);
        assert_eq!(txn.entity_id().index(), 7);
    }

    #[test]
    fn op_byte_round_trip() {
        assert_eq!(Op::from_byte(Op::Assert.to_byte()).unwrap(), Op::Assert);
        assert_eq!(Op::from_byte(Op::Retract.to_byte()).unwrap(), Op::Retract);
        assert!(matches!(Op::from_byte(0x02), Err(Error::UnknownOp(0x02))));
    }

    #[test]
    fn tags_round_trip_through_types() {
        for ty in [
            Type::Bool,
            Type::Long,
            Type::Double,
            Type::String,
            Type::Bytes,
            Type::Ref,
            Type::Timestamp,
        ] {
            assert_eq!(type_for_tag(tag_for_type(ty)).unwrap(), ty);
        }
        assert!(matches!(
            type_for_tag(0x00),
            Err(Error::UnknownValueTag(0x00))
        ));
    }

    #[test]
    fn value_type_matches_variant() {
        let cases = [
            (Value::Bool(true), Type::Bool),
            (Value::Long(1), Type::Long),
            (Value::Double(1.0), Type::Double),
            (Value::String("s".into()), Type::String),
            (Value::Bytes(vec![1]), Type::Bytes),
            (Value::Ref(EntityId::new(USER_PARTITION, 1)), Type::Ref),
            (Value::Instant(Timestamp::UNIX_EPOCH), Type::Timestamp),
        ];
        for (value, ty) in cases {
            assert_eq!(value.value_type(), ty);
            assert_eq!(value.tag(), tag_for_type(ty));
        }
    }

    #[test]
    fn value_key_round_trip() {
        let values = [
            Value::Bool(true),
            Value::Long(-42),
            Value::Double(1.5),
            Value::String("hello".into()),
            Value::Bytes(vec![1, 2, 3]),
            Value::Ref(EntityId::new(USER_PARTITION, 7)),
            Value::Instant(Timestamp::UNIX_EPOCH),
        ];
        for value in values {
            let mut buf = Vec::new();
            value.encode_key(&mut buf);
            assert_eq!(Value::decode_key(&buf).unwrap(), value);
        }
    }

    #[test]
    fn value_key_rejects_unknown_tag() {
        assert!(matches!(
            Value::decode_key(&[0x7F, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(Error::UnknownValueTag(0x7F))
        ));
        assert!(Value::decode_key(&[]).is_err());
    }

    #[test]
    fn value_key_preserves_order_within_type() {
        let mut negatives = Vec::new();
        Value::Long(-1).encode_key(&mut negatives);
        let mut zero = Vec::new();
        Value::Long(0).encode_key(&mut zero);
        let mut one = Vec::new();
        Value::Long(1).encode_key(&mut one);
        assert!(negatives < zero);
        assert!(zero < one);
    }

    #[test]
    fn factom_accessors() {
        let e = EntityId::new(USER_PARTITION, 1);
        let a = AttributeId::new(0x0100);
        let t = TxnId::new(EntityId::new(SYSTEM_PARTITION, 1).0);
        let factom = Factom::new(e, a, Value::Long(30), t, Op::Assert);
        assert_eq!(factom.entity(), e);
        assert_eq!(factom.attribute(), a);
        assert_eq!(factom.value(), &Value::Long(30));
        assert_eq!(factom.txn(), t);
        assert_eq!(factom.op(), Op::Assert);
    }
}
