use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::id::EntityId;
use crate::value::Type;

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

    fn encoded(value: &impl EncodePrimitive) -> Vec<u8> {
        let mut buf = Vec::new();
        value.encode(&mut buf);
        buf
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
