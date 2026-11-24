#[allow(dead_code)]
mod downstream {
    // These names ensure exported macros do not depend on invocation-site imports.
    pub struct String;
    pub struct Vec;
    pub struct Result;
    pub struct Error;
    pub struct Type;
    pub struct Value;
    pub struct Primitive;
    pub struct EncodePrimitive;
    pub struct DecodePrimitive;

    #[derive(Debug, PartialEq)]
    pub struct Text(pub std::string::String);

    impl Text {
        pub fn as_str(&self) -> &str {
            self.0.as_str()
        }
    }

    factomic::impl_string_primitive!(Text, Text::as_str, |value: std::string::String| Ok(Text(
        value
    )),);

    #[derive(Debug, PartialEq)]
    pub struct Long(pub i64);

    factomic::impl_long_primitive!(Long, |value: &Long| value.0, |value: i64| Ok(Long(value)),);

    #[derive(Debug, PartialEq)]
    pub struct Instant(pub i64);

    factomic::impl_timestamp_primitive!(Instant, |value: &Instant| value.0, |value: i64| Ok(
        Instant(value)
    ),);

    #[derive(Debug, PartialEq)]
    pub struct Bytes(pub std::vec::Vec<u8>);

    impl Bytes {
        pub fn as_slice(&self) -> &[u8] {
            self.0.as_slice()
        }

        pub fn from_bytes(value: std::vec::Vec<u8>) -> factomic::Result<Self> {
            Ok(Self(value))
        }
    }

    factomic::impl_bytes_primitive!(Bytes, Bytes::as_slice, Bytes::from_bytes);
}

fn encoded(value: &impl factomic::EncodePrimitive) -> Vec<u8> {
    let mut buf = Vec::new();
    value.encode(&mut buf);
    buf
}

#[test]
fn exported_string_macro_is_hygienic() {
    use downstream::Text;
    use factomic::{DecodePrimitive, Primitive, Type, Value};

    let value = Text("hello".to_owned());
    assert_eq!(Text::TYPE, Type::String);
    assert_eq!(Text::decode(&encoded(&value)).unwrap(), value);
    assert_eq!(
        Text::try_from(Value::String("hello".to_owned())).unwrap(),
        value
    );
}

#[test]
fn exported_long_macro_is_hygienic() {
    use downstream::Long;
    use factomic::{DecodePrimitive, Primitive, Type, Value};

    let value = Long(-42);
    assert_eq!(Long::TYPE, Type::Long);
    assert_eq!(Long::decode(&encoded(&value)).unwrap(), value);
    assert_eq!(Long::try_from(Value::Long(-42)).unwrap(), value);
}

#[test]
fn exported_timestamp_macro_is_hygienic() {
    use downstream::Instant;
    use factomic::{DecodePrimitive, Primitive, Type, Value};

    let value = Instant(1_753_000_000_000_000);
    assert_eq!(Instant::TYPE, Type::Timestamp);
    assert_eq!(Instant::decode(&encoded(&value)).unwrap(), value);
    assert_eq!(
        Instant::try_from(Value::Timestamp(
            jiff::Timestamp::from_microsecond(value.0).unwrap()
        ))
        .unwrap(),
        value
    );
}

#[test]
fn exported_bytes_macro_is_hygienic() {
    use downstream::Bytes;
    use factomic::{DecodePrimitive, Primitive, Type, Value};

    let value = Bytes(vec![1, 2, 3]);
    assert_eq!(Bytes::TYPE, Type::Bytes);
    assert_eq!(Bytes::decode(&encoded(&value)).unwrap(), value);
    assert_eq!(Bytes::try_from(Value::Bytes(vec![1, 2, 3])).unwrap(), value);
}
