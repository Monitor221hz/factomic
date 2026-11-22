use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::id::EntityId;
use crate::primitive::{DecodePrimitive, EncodePrimitive};

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

const TAG_BOOL: u8 = 0x01;
pub(crate) const TAG_LONG: u8 = 0x02;
const TAG_DOUBLE: u8 = 0x03;
pub(crate) const TAG_STRING: u8 = 0x04;
const TAG_BYTES: u8 = 0x05;
pub(crate) const TAG_REF: u8 = 0x06;
const TAG_INSTANT: u8 = 0x07;

pub(crate) fn tag_for_type(ty: Type) -> u8 {
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

pub(crate) fn type_for_tag(tag: u8) -> Result<Type> {
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

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Long(i64),
    Double(f64),
    String(String),
    Bytes(Vec<u8>),
    Ref(EntityId),
    Timestamp(Timestamp),
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
            Value::Timestamp(_) => Type::Timestamp,
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
            Value::Timestamp(v) => v.encode(buf),
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
            TAG_INSTANT => Ok(Value::Timestamp(Timestamp::decode(body)?)),
            _ => Err(Error::UnknownValueTag(tag)),
        }
    }

    pub fn region(&self) -> Vec<u8> {
        let mut region = Vec::with_capacity(1 + size_of_val(self));
        self.encode_key(&mut region);
        region
    }
}

pub(crate) fn region<V: EncodePrimitive>(value: &V) -> Vec<u8> {
    let mut region = Vec::with_capacity(1 + size_of_val(value));
    region.push(tag_for_type(V::TYPE));
    value.encode(&mut region);
    region
}

impl TryFrom<&Value> for bool {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self> {
        match value {
            Value::Bool(v) => Ok(*v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Bool,
                got: other.value_type(),
            }),
        }
    }
}

impl TryFrom<Value> for bool {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        match value {
            Value::Bool(v) => Ok(v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Bool,
                got: other.value_type(),
            }),
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl TryFrom<&Value> for i64 {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self> {
        match value {
            Value::Long(v) => Ok(*v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Long,
                got: other.value_type(),
            }),
        }
    }
}

impl TryFrom<Value> for i64 {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        match value {
            Value::Long(v) => Ok(v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Long,
                got: other.value_type(),
            }),
        }
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Long(value)
    }
}

macro_rules! impl_small_int {
    ($($ty:ty),* $(,)?) => {
        $(
            impl TryFrom<&Value> for $ty {
                type Error = Error;

                fn try_from(value: &Value) -> Result<Self> {
                    let value = i64::try_from(value)?;
                    <$ty>::try_from(value).map_err(|_| Error::CorruptPrimitive(Type::Long))
                }
            }

            impl TryFrom<Value> for $ty {
                type Error = Error;

                fn try_from(value: Value) -> Result<Self> {
                    let value = i64::try_from(value)?;
                    <$ty>::try_from(value).map_err(|_| Error::CorruptPrimitive(Type::Long))
                }
            }

            impl From<$ty> for Value {
                fn from(value: $ty) -> Self {
                    Value::Long(value as i64)
                }
            }
        )*
    };
}

impl_small_int!(i8, i16, i32, u8, u16, u32);

impl TryFrom<&Value> for f64 {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self> {
        match value {
            Value::Double(v) => Ok(*v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Double,
                got: other.value_type(),
            }),
        }
    }
}

impl TryFrom<Value> for f64 {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        match value {
            Value::Double(v) => Ok(v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Double,
                got: other.value_type(),
            }),
        }
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Double(value)
    }
}

impl TryFrom<&Value> for String {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self> {
        match value {
            Value::String(v) => Ok(v.clone()),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::String,
                got: other.value_type(),
            }),
        }
    }
}

impl TryFrom<Value> for String {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        match value {
            Value::String(v) => Ok(v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::String,
                got: other.value_type(),
            }),
        }
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl TryFrom<&Value> for Vec<u8> {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self> {
        match value {
            Value::Bytes(v) => Ok(v.clone()),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Bytes,
                got: other.value_type(),
            }),
        }
    }
}

impl TryFrom<Value> for Vec<u8> {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        match value {
            Value::Bytes(v) => Ok(v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Bytes,
                got: other.value_type(),
            }),
        }
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Value::Bytes(value)
    }
}

impl TryFrom<&Value> for EntityId {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self> {
        match value {
            Value::Ref(v) => Ok(*v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Ref,
                got: other.value_type(),
            }),
        }
    }
}

impl TryFrom<Value> for EntityId {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        match value {
            Value::Ref(v) => Ok(v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Ref,
                got: other.value_type(),
            }),
        }
    }
}

impl From<EntityId> for Value {
    fn from(value: EntityId) -> Self {
        Value::Ref(value)
    }
}

impl TryFrom<&Value> for Timestamp {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self> {
        match value {
            Value::Timestamp(v) => Ok(*v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Timestamp,
                got: other.value_type(),
            }),
        }
    }
}

impl TryFrom<Value> for Timestamp {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        match value {
            Value::Timestamp(v) => Ok(v),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Timestamp,
                got: other.value_type(),
            }),
        }
    }
}

impl From<Timestamp> for Value {
    fn from(value: Timestamp) -> Self {
        Value::Timestamp(value)
    }
}

impl<'a> TryFrom<&'a Value> for &'a str {
    type Error = Error;

    fn try_from(value: &'a Value) -> Result<Self> {
        match value {
            Value::String(v) => Ok(v.as_str()),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::String,
                got: other.value_type(),
            }),
        }
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_string())
    }
}

impl<'a> TryFrom<&'a Value> for &'a [u8] {
    type Error = Error;

    fn try_from(value: &'a Value) -> Result<Self> {
        match value {
            Value::Bytes(v) => Ok(v.as_slice()),
            other => Err(Error::ValueTypeMismatch {
                expected: Type::Bytes,
                got: other.value_type(),
            }),
        }
    }
}

impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Self {
        Value::Bytes(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::USER_PARTITION;

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
            (Value::Timestamp(Timestamp::UNIX_EPOCH), Type::Timestamp),
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
            Value::Timestamp(Timestamp::UNIX_EPOCH),
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
    fn try_from_extracts_owned_primitives() {
        let id = EntityId::new(USER_PARTITION, 7);
        assert!(bool::try_from(&Value::Bool(true)).unwrap());
        assert_eq!(i64::try_from(&Value::Long(-42)).unwrap(), -42);
        assert_eq!(f64::try_from(&Value::Double(1.5)).unwrap(), 1.5);
        assert_eq!(
            String::try_from(&Value::String("hello".into())).unwrap(),
            "hello"
        );
        assert_eq!(
            Vec::<u8>::try_from(&Value::Bytes(vec![1, 2, 3])).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(EntityId::try_from(&Value::Ref(id)).unwrap(), id);
        assert_eq!(
            Timestamp::try_from(&Value::Timestamp(Timestamp::UNIX_EPOCH)).unwrap(),
            Timestamp::UNIX_EPOCH
        );
    }

    #[test]
    fn try_from_extracts_borrowed_views() {
        let string = Value::String("hello".into());
        assert_eq!(<&str>::try_from(&string).unwrap(), "hello");
        let bytes = Value::Bytes(vec![1, 2, 3]);
        assert_eq!(<&[u8]>::try_from(&bytes).unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn small_ints_round_trip_through_value() {
        assert_eq!(i8::try_from(&Value::from(-8i8)).unwrap(), -8);
        assert_eq!(i16::try_from(&Value::from(-16i16)).unwrap(), -16);
        assert_eq!(i32::try_from(&Value::from(-32i32)).unwrap(), -32);
        assert_eq!(u8::try_from(&Value::from(8u8)).unwrap(), 8);
        assert_eq!(u16::try_from(&Value::from(16u16)).unwrap(), 16);
        assert_eq!(u32::try_from(&Value::from(32u32)).unwrap(), 32);
        assert_eq!(Value::from(42u32), Value::Long(42));
    }

    #[test]
    fn small_int_narrowing_rejects_overflow() {
        assert!(matches!(
            i8::try_from(&Value::Long(128)),
            Err(Error::CorruptPrimitive(Type::Long))
        ));
        assert!(matches!(
            u8::try_from(&Value::Long(-1)),
            Err(Error::CorruptPrimitive(Type::Long))
        ));
        assert!(matches!(
            u32::try_from(&Value::Long(i64::MAX)),
            Err(Error::CorruptPrimitive(Type::Long))
        ));
    }

    #[test]
    fn try_from_mismatch_reports_expected_and_got() {
        let value = Value::String("hello".into());
        assert!(matches!(
            i64::try_from(&value),
            Err(Error::ValueTypeMismatch {
                expected: Type::Long,
                got: Type::String,
            })
        ));
        assert!(matches!(
            <&str>::try_from(&Value::Long(1)),
            Err(Error::ValueTypeMismatch {
                expected: Type::String,
                got: Type::Long,
            })
        ));
    }
}
