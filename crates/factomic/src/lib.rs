pub mod om {

    #[derive(Debug)]
    pub enum Type {
        Bool,
        Long,
        Double,
        String,
        Bytes,
        Ref,
        Instant,
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

        #[error("utf 8 err")]
        StringError(#[from] std::str::Utf8Error),
    }

    pub type Result<T> = std::result::Result<T, Error>;
    trait Primitive {
        const TYPE: Type;
    }

    trait EncodePrimitive: Primitive {
        fn encode(&self, buf: &mut Vec<u8>);
    }

    trait DecodePrimitive<'a>: Sized + Primitive {
        fn decode(bytes: &'a [u8]) -> Result<Self>;
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
            ))
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
            buf.extend_from_slice(&self.as_bytes());
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
}
