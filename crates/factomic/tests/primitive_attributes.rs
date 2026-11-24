#![cfg(feature = "derive")]

use factomic::{DecodePrimitive, EncodePrimitive, Primitive, Value};
use std::convert::Infallible;
use std::fmt::{Debug, Display};

#[derive(Debug, PartialEq)]
#[factomic::primitive(string)]
struct Text(String);

impl Display for Text {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl std::str::FromStr for Text {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, PartialEq)]
#[factomic::primitive(long)]
struct Long(i64);

impl From<&Long> for i64 {
    fn from(value: &Long) -> Self {
        value.0
    }
}

impl TryFrom<i64> for Long {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

#[derive(Debug, PartialEq)]
#[factomic::primitive(timestamp)]
struct Instant(i64);

impl From<&Instant> for i64 {
    fn from(value: &Instant) -> Self {
        value.0
    }
}

impl TryFrom<i64> for Instant {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

#[derive(Debug, PartialEq)]
#[factomic::primitive(bytes)]
struct Bytes(Vec<u8>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for Bytes {
    type Error = ();

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

fn encoded(value: &impl EncodePrimitive) -> Vec<u8> {
    let mut buf = Vec::new();
    value.encode(&mut buf);
    buf
}

fn assert_round_trip<T>(value: T, stored: Value)
where
    T: Debug
        + PartialEq
        + Primitive
        + EncodePrimitive
        + for<'a> DecodePrimitive<'a>
        + TryFrom<Value>,
    <T as TryFrom<Value>>::Error: Debug,
{
    assert_eq!(T::TYPE, stored.value_type());
    assert_eq!(T::decode(&encoded(&value)).unwrap(), value);
    assert_eq!(T::try_from(stored).unwrap(), value);
}

#[test]
fn string_attribute_round_trips() {
    assert_round_trip(Text("hello".to_owned()), Value::String("hello".to_owned()));
}

#[test]
fn long_attribute_round_trips() {
    assert_round_trip(Long(-42), Value::Long(-42));
}

#[test]
fn timestamp_attribute_round_trips() {
    let micros = 1_753_000_000_000_000;
    assert_round_trip(
        Instant(micros),
        Value::Timestamp(jiff::Timestamp::from_microsecond(micros).unwrap()),
    );
}

#[test]
fn bytes_attribute_round_trips() {
    assert_round_trip(Bytes(vec![1, 2, 3]), Value::Bytes(vec![1, 2, 3]));
}
