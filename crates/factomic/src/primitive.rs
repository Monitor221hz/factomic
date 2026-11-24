use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::id::EntityId;
use crate::value::Type;

mod integrations;

pub trait Primitive: Sized {
    const TYPE: Type;
}

/// For encoding primitives for use in indexes.
///
/// Note that [fjall] stores lexicographically by raw bytes, so the encoded bytes must sort in the same logical order as the underlying type. Implementation should be adjusted accordingly.
///
/// For example, the default encoding for [i64]:
/// ```ignore
/// buf.extend_from_slice(&(*self as u64 ^ 1 << 63).to_be_bytes());
/// ```
/// To prevent two's complement from breaking negative integer sorting in big endian.
pub trait EncodePrimitive: Primitive {
    fn encode(&self, buf: &mut Vec<u8>);
}

/// For decoding primitives for use in indexes. If your implementing type encodes via [EncodePrimitive] as an already decodable primitive type (e.g [Timestamp] encoding as [i64]), you do not need to implement this.
///
/// Note that [fjall] stores lexicographically by raw bytes, so if any byte magic was done to make encoded bytes sort in the same logical order, it must be properly reversed here.
///
/// For example, the default decoding for i64:
/// ```ignore
/// Ok(i64::from_be_bytes(bytes.try_into().unwrap(),) ^ (1 << 63)) // unwrap used for brevity only
/// ```
/// Which successfully counters the shift
pub trait DecodePrimitive<'a>: Sized + Primitive {
    fn decode(bytes: &'a [u8]) -> Result<Self>;
}

/// Implements primitive support for an owned type stored as a Factomic string.
#[macro_export]
macro_rules! impl_string_primitive {
    ($ty:ty, $as_str:path, $from_string:expr $(,)?) => {
        impl $crate::Primitive for $ty {
            const TYPE: $crate::Type = $crate::Type::String;
        }

        impl $crate::EncodePrimitive for $ty {
            fn encode(&self, buf: &mut ::std::vec::Vec<u8>) {
                $crate::EncodePrimitive::encode(&($as_str)(self), buf);
            }
        }

        impl $crate::DecodePrimitive<'_> for $ty {
            fn decode(bytes: &[u8]) -> $crate::Result<Self> {
                let value = <::std::string::String as $crate::DecodePrimitive>::decode(bytes)?;
                ($from_string)(value)
            }
        }

        impl ::core::convert::TryFrom<$crate::Value> for $ty {
            type Error = $crate::Error;

            fn try_from(value: $crate::Value) -> $crate::Result<Self> {
                let value =
                    <::std::string::String as ::core::convert::TryFrom<$crate::Value>>::try_from(
                        value,
                    )?;
                ($from_string)(value)
            }
        }
    };
}

/// Implements primitive support for an owned type stored as a Factomic long.
#[macro_export]
macro_rules! impl_long_primitive {
    ($ty:ty, $as_i64:expr, $from_i64:expr $(,)?) => {
        impl $crate::Primitive for $ty {
            const TYPE: $crate::Type = $crate::Type::Long;
        }

        impl $crate::EncodePrimitive for $ty {
            fn encode(&self, buf: &mut ::std::vec::Vec<u8>) {
                $crate::EncodePrimitive::encode(&($as_i64)(self), buf);
            }
        }

        impl $crate::DecodePrimitive<'_> for $ty {
            fn decode(bytes: &[u8]) -> $crate::Result<Self> {
                let value = <i64 as $crate::DecodePrimitive>::decode(bytes)?;
                ($from_i64)(value)
            }
        }

        impl ::core::convert::TryFrom<$crate::Value> for $ty {
            type Error = $crate::Error;

            fn try_from(value: $crate::Value) -> $crate::Result<Self> {
                let value = <i64 as ::core::convert::TryFrom<$crate::Value>>::try_from(value)?;
                ($from_i64)(value)
            }
        }
    };
}

/// Implements primitive support for an owned type stored as a Factomic timestamp.
#[macro_export]
macro_rules! impl_timestamp_primitive {
    ($ty:ty, $as_microsecond:expr, $from_microsecond:expr $(,)?) => {
        impl $crate::Primitive for $ty {
            const TYPE: $crate::Type = $crate::Type::Timestamp;
        }

        impl $crate::EncodePrimitive for $ty {
            fn encode(&self, buf: &mut ::std::vec::Vec<u8>) {
                $crate::EncodePrimitive::encode(&($as_microsecond)(self), buf);
            }
        }

        impl $crate::DecodePrimitive<'_> for $ty {
            fn decode(bytes: &[u8]) -> $crate::Result<Self> {
                let micros = <i64 as $crate::DecodePrimitive>::decode(bytes)
                    .map_err(|_| $crate::Error::CorruptPrimitive($crate::Type::Timestamp))?;
                ($from_microsecond)(micros)
            }
        }

        impl ::core::convert::TryFrom<$crate::Value> for $ty {
            type Error = $crate::Error;

            fn try_from(value: $crate::Value) -> $crate::Result<Self> {
                let timestamp = match value {
                    $crate::Value::Timestamp(timestamp) => timestamp,
                    other => {
                        return ::core::result::Result::Err($crate::Error::ValueTypeMismatch {
                            expected: $crate::Type::Timestamp,
                            got: other.value_type(),
                        });
                    }
                };
                ($from_microsecond)(timestamp.as_microsecond())
            }
        }
    };
}

/// Implements primitive support for an owned type stored as Factomic bytes.
#[macro_export]
macro_rules! impl_bytes_primitive {
    ($ty:ty, $as_bytes:path, $from_bytes:expr $(,)?) => {
        impl $crate::Primitive for $ty {
            const TYPE: $crate::Type = $crate::Type::Bytes;
        }

        impl $crate::EncodePrimitive for $ty {
            fn encode(&self, buf: &mut ::std::vec::Vec<u8>) {
                buf.extend_from_slice(($as_bytes)(self));
            }
        }

        impl $crate::DecodePrimitive<'_> for $ty {
            fn decode(bytes: &[u8]) -> $crate::Result<Self> {
                ($from_bytes)(::std::vec::Vec::from(bytes))
            }
        }

        impl ::core::convert::TryFrom<$crate::Value> for $ty {
            type Error = $crate::Error;

            fn try_from(value: $crate::Value) -> $crate::Result<Self> {
                let value =
                    <::std::vec::Vec<u8> as ::core::convert::TryFrom<$crate::Value>>::try_from(
                        value,
                    )?;
                ($from_bytes)(value)
            }
        }
    };
}

impl Primitive for EntityId {
    const TYPE: Type = Type::Ref;
}

impl EncodePrimitive for EntityId {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.as_u64().to_be_bytes());
    }
}

impl DecodePrimitive<'_> for EntityId {
    fn decode(bytes: &'_ [u8]) -> Result<Self> {
        Ok(EntityId::with_value(u64::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| Error::CorruptPrimitive(Type::Ref))?,
        )))
    }
}

impl Primitive for Timestamp {
    const TYPE: Type = Type::Timestamp;
}

impl EncodePrimitive for Timestamp {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.as_microsecond().encode(buf);
    }
}

impl DecodePrimitive<'_> for Timestamp {
    fn decode(bytes: &'_ [u8]) -> Result<Self> {
        let micros = i64::decode(bytes).map_err(|_| Error::CorruptPrimitive(Type::Timestamp))?;
        Timestamp::from_microsecond(micros).map_err(|_| Error::CorruptPrimitive(Type::Timestamp))
    }
}

impl Primitive for bool {
    const TYPE: Type = Type::Bool;
}

impl EncodePrimitive for bool {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(*self as u8);
    }
}

impl DecodePrimitive<'_> for bool {
    fn decode(bytes: &'_ [u8]) -> Result<Self> {
        match bytes {
            [0x00] => Ok(false),
            [0x01] => Ok(true),
            _ => Err(Error::CorruptPrimitive(Type::Bool)),
        }
    }
}

impl Primitive for i64 {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for i64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&(*self as u64 ^ 1 << 63).to_be_bytes());
    }
}

impl DecodePrimitive<'_> for i64 {
    fn decode(bytes: &'_ [u8]) -> Result<Self> {
        Ok(i64::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| Error::CorruptPrimitive(Type::Long))?,
        ) ^ (1 << 63))
    }
}

impl Primitive for f64 {
    const TYPE: Type = Type::Double;
}

impl EncodePrimitive for f64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        let bits = self.to_bits();
        let bits = if bits & (1 << 63) == 0 {
            bits ^ (1 << 63)
        } else {
            !bits
        };
        buf.extend_from_slice(&bits.to_be_bytes());
    }
}

impl DecodePrimitive<'_> for f64 {
    fn decode(bytes: &'_ [u8]) -> Result<Self> {
        let bits = u64::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| Error::CorruptPrimitive(Type::Double))?,
        );
        let bits = if bits & (1 << 63) != 0 {
            bits ^ (1 << 63)
        } else {
            !bits
        };
        Ok(f64::from_bits(bits))
    }
}

impl Primitive for &str {
    const TYPE: Type = Type::String;
}

impl EncodePrimitive for &str {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

impl<'a> DecodePrimitive<'a> for &'a str {
    fn decode(bytes: &'a [u8]) -> Result<Self> {
        Ok(str::from_utf8(bytes)?)
    }
}

impl Primitive for String {
    const TYPE: Type = Type::String;
}

impl EncodePrimitive for String {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

impl DecodePrimitive<'_> for String {
    fn decode(bytes: &'_ [u8]) -> Result<Self> {
        Ok(str::from_utf8(bytes)
            .map_err(|_| Error::CorruptPrimitive(Type::String))?
            .to_owned())
    }
}

impl Primitive for i32 {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for i32 {
    fn encode(&self, buf: &mut Vec<u8>) {
        i64::from(*self).encode(buf)
    }
}

impl Primitive for i16 {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for i16 {
    fn encode(&self, buf: &mut Vec<u8>) {
        i64::from(*self).encode(buf)
    }
}

impl Primitive for i8 {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for i8 {
    fn encode(&self, buf: &mut Vec<u8>) {
        i64::from(*self).encode(buf)
    }
}

impl Primitive for u32 {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for u32 {
    fn encode(&self, buf: &mut Vec<u8>) {
        i64::from(*self).encode(buf)
    }
}

impl Primitive for u16 {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for u16 {
    fn encode(&self, buf: &mut Vec<u8>) {
        i64::from(*self).encode(buf)
    }
}

impl Primitive for u8 {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for u8 {
    fn encode(&self, buf: &mut Vec<u8>) {
        i64::from(*self).encode(buf)
    }
}

impl Primitive for &[u8] {
    const TYPE: Type = Type::Bytes;
}

impl EncodePrimitive for &[u8] {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self);
    }
}

impl<'a> DecodePrimitive<'a> for &'a [u8] {
    fn decode(bytes: &'a [u8]) -> Result<Self> {
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    fn encoded(value: &impl EncodePrimitive) -> Vec<u8> {
        let mut buf = Vec::new();
        value.encode(&mut buf);
        buf
    }

    #[derive(Debug, PartialEq)]
    struct StringPrimitive(String);

    impl StringPrimitive {
        fn as_str(&self) -> &str {
            self.0.as_str()
        }
    }

    impl_string_primitive!(
        StringPrimitive,
        StringPrimitive::as_str,
        |value: String| Ok(StringPrimitive(value)),
    );

    #[derive(Debug, PartialEq)]
    struct LongPrimitive(i64);

    impl_long_primitive!(
        LongPrimitive,
        |value: &LongPrimitive| value.0,
        |value: i64| Ok(LongPrimitive(value)),
    );

    #[derive(Debug, PartialEq)]
    struct TimestampPrimitive(Timestamp);

    impl_timestamp_primitive!(
        TimestampPrimitive,
        |value: &TimestampPrimitive| value.0.as_microsecond(),
        |micros: i64| {
            Timestamp::from_microsecond(micros)
                .map(TimestampPrimitive)
                .map_err(|_| Error::CorruptPrimitive(Type::Timestamp))
        },
    );

    #[derive(Debug, PartialEq)]
    struct BytesPrimitive(Vec<u8>);

    impl BytesPrimitive {
        fn as_slice(&self) -> &[u8] {
            self.0.as_slice()
        }

        fn from_bytes(value: Vec<u8>) -> Result<Self> {
            Ok(Self(value))
        }
    }

    impl_bytes_primitive!(
        BytesPrimitive,
        BytesPrimitive::as_slice,
        BytesPrimitive::from_bytes,
    );

    #[test]
    fn string_primitive_macro_round_trip() {
        let value = StringPrimitive("hello".to_owned());
        assert_eq!(StringPrimitive::TYPE, Type::String);
        assert_eq!(encoded(&value), encoded(&value.0));
        assert_eq!(StringPrimitive::decode(&encoded(&value)).unwrap(), value);
        assert_eq!(
            StringPrimitive::try_from(Value::String("hello".to_owned())).unwrap(),
            value
        );
    }

    #[test]
    fn long_primitive_macro_round_trip() {
        let value = LongPrimitive(-42);
        assert_eq!(LongPrimitive::TYPE, Type::Long);
        assert_eq!(encoded(&value), encoded(&value.0));
        assert_eq!(LongPrimitive::decode(&encoded(&value)).unwrap(), value);
        assert_eq!(LongPrimitive::try_from(Value::Long(-42)).unwrap(), value);
    }

    #[test]
    fn timestamp_primitive_macro_round_trip() {
        let timestamp = Timestamp::from_microsecond(1_753_000_000_000_000).unwrap();
        let value = TimestampPrimitive(timestamp);
        assert_eq!(TimestampPrimitive::TYPE, Type::Timestamp);
        assert_eq!(encoded(&value), encoded(&timestamp));
        assert_eq!(TimestampPrimitive::decode(&encoded(&value)).unwrap(), value);
        assert_eq!(
            TimestampPrimitive::try_from(Value::Timestamp(timestamp)).unwrap(),
            value
        );
    }

    #[test]
    fn bytes_primitive_macro_round_trip() {
        let value = BytesPrimitive(vec![1, 2, 3]);
        assert_eq!(BytesPrimitive::TYPE, Type::Bytes);
        assert_eq!(encoded(&value), vec![1, 2, 3]);
        assert_eq!(BytesPrimitive::decode(&encoded(&value)).unwrap(), value);
        assert_eq!(
            BytesPrimitive::try_from(Value::Bytes(vec![1, 2, 3])).unwrap(),
            value
        );
    }

    #[test]
    fn bool_round_trip() {
        assert!(bool::decode(&encoded(&true)).unwrap());
        assert!(!bool::decode(&encoded(&false)).unwrap());
    }

    #[test]
    fn bool_rejects_garbage() {
        assert!(matches!(
            bool::decode(&[0x02]),
            Err(Error::CorruptPrimitive(Type::Bool))
        ));
        assert!(bool::decode(&[]).is_err());
    }

    #[test]
    fn i64_round_trip() {
        for x in [i64::MIN, -42, -1, 0, 1, 42, i64::MAX] {
            assert_eq!(i64::decode(&encoded(&x)).unwrap(), x);
        }
    }

    #[test]
    fn i64_encoding_preserves_order() {
        let values = [i64::MIN, -42, -1, 0, 1, 42, i64::MAX];
        for pair in values.windows(2) {
            assert!(encoded(&pair[0]) < encoded(&pair[1]));
        }
    }

    #[test]
    fn small_ints_encode_as_i64() {
        assert_eq!(encoded(&42i8), encoded(&42i64));
        assert_eq!(encoded(&42i16), encoded(&42i64));
        assert_eq!(encoded(&42i32), encoded(&42i64));
        assert_eq!(encoded(&42u8), encoded(&42i64));
        assert_eq!(encoded(&42u16), encoded(&42i64));
        assert_eq!(encoded(&42u32), encoded(&42i64));
    }

    #[test]
    fn f64_round_trip() {
        for x in [
            f64::NEG_INFINITY,
            -1.5,
            -0.0,
            0.0,
            1.5,
            f64::MAX,
            f64::INFINITY,
            f64::NAN,
        ] {
            assert_eq!(f64::decode(&encoded(&x)).unwrap().to_bits(), x.to_bits());
        }
    }

    #[test]
    fn f64_encoding_preserves_order() {
        let values = [
            f64::NEG_INFINITY,
            -1.5,
            -0.0,
            0.0,
            1.5,
            f64::MAX,
            f64::INFINITY,
        ];
        for pair in values.windows(2) {
            assert!(encoded(&pair[0]) < encoded(&pair[1]));
        }
    }

    #[test]
    fn str_round_trip() {
        let buf = encoded(&"hello");
        assert_eq!(<&str>::decode(&buf).unwrap(), "hello");
        assert_eq!(String::decode(&buf).unwrap(), "hello");
    }

    #[test]
    fn bytes_round_trip() {
        let buf = encoded(&&[1u8, 2, 3][..]);
        assert_eq!(<&[u8]>::decode(&buf).unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn entity_id_round_trip() {
        let id = EntityId::with_value(42);
        assert_eq!(EntityId::decode(&encoded(&id)).unwrap(), id);
        assert!(matches!(
            EntityId::decode(&[0; 7]),
            Err(Error::CorruptPrimitive(Type::Ref))
        ));
    }

    #[test]
    fn entity_id_encoding_preserves_order() {
        assert!(encoded(&EntityId::with_value(1)) < encoded(&EntityId::with_value(2)));
    }

    #[test]
    fn timestamp_round_trip() {
        for ts in [Timestamp::MIN, Timestamp::UNIX_EPOCH] {
            assert_eq!(Timestamp::decode(&encoded(&ts)).unwrap(), ts);
        }
        let near_max = Timestamp::from_microsecond(253402207199999999).unwrap();
        assert_eq!(Timestamp::decode(&encoded(&near_max)).unwrap(), near_max);
        for micros in [-1, 0, 1, 1_753_000_000_000_000] {
            let ts = Timestamp::from_microsecond(micros).unwrap();
            assert_eq!(Timestamp::decode(&encoded(&ts)).unwrap(), ts);
        }
    }

    #[test]
    fn timestamp_truncates_sub_microseconds() {
        let ts = Timestamp::new(5, 123_456_789).unwrap();
        let decoded = Timestamp::decode(&encoded(&ts)).unwrap();
        assert_eq!(decoded, Timestamp::from_microsecond(5_123_456).unwrap());
    }

    #[test]
    fn timestamp_encoding_preserves_order() {
        let before = Timestamp::from_microsecond(-1).unwrap();
        let epoch = Timestamp::UNIX_EPOCH;
        let after = Timestamp::from_microsecond(1).unwrap();
        assert!(encoded(&before) < encoded(&epoch));
        assert!(encoded(&epoch) < encoded(&after));
    }

    #[test]
    fn timestamp_rejects_out_of_range() {
        let garbage = encoded(&i64::MAX);
        assert!(matches!(
            Timestamp::decode(&garbage),
            Err(Error::CorruptPrimitive(Type::Timestamp))
        ));
    }
}
