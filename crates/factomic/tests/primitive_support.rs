use factomic::{DecodePrimitive, EncodePrimitive, Primitive, Value};
use std::fmt::Debug;

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
    let bytes = encoded(&value);
    let decoded =
        T::decode(&bytes).unwrap_or_else(|error| panic!("failed to decode {value:?}: {error:?}"));
    assert_eq!(decoded, value);
    assert_eq!(T::try_from(stored).unwrap(), value);
}

#[test]
fn f32_support_round_trips() {
    assert_round_trip(1.5f32, Value::Double(1.5));
    assert_round_trip(f32::INFINITY, Value::Double(f64::INFINITY));
    assert!(f32::try_from(Value::Double(f64::MAX)).is_err());
}

#[test]
fn char_and_network_support_round_trips() {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

    assert_round_trip('🦀', Value::String("🦀".to_owned()));
    assert_round_trip(
        IpAddr::from([127, 0, 0, 1]),
        Value::String("127.0.0.1".to_owned()),
    );
    assert_round_trip(
        Ipv4Addr::new(127, 0, 0, 1),
        Value::String("127.0.0.1".to_owned()),
    );
    assert_round_trip(Ipv6Addr::LOCALHOST, Value::String("::1".to_owned()));
    let socket = SocketAddr::from(([127, 0, 0, 1], 8080));
    assert_round_trip(socket, Value::String(socket.to_string()));
    let socket = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 8080);
    assert_round_trip(socket, Value::String(socket.to_string()));
    let socket = SocketAddrV6::new(Ipv6Addr::LOCALHOST, 8080, 0, 0);
    assert_round_trip(socket, Value::String(socket.to_string()));
}

#[test]
fn nonzero_integer_support_round_trips_and_validates() {
    use std::num::{
        NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroU8, NonZeroU16, NonZeroU32,
    };

    assert_round_trip(NonZeroI8::new(-8).unwrap(), Value::Long(-8));
    assert_round_trip(NonZeroI16::new(-16).unwrap(), Value::Long(-16));
    assert_round_trip(NonZeroI32::new(-32).unwrap(), Value::Long(-32));
    assert_round_trip(NonZeroI64::new(-64).unwrap(), Value::Long(-64));
    assert_round_trip(NonZeroU8::new(8).unwrap(), Value::Long(8));
    assert_round_trip(NonZeroU16::new(16).unwrap(), Value::Long(16));
    assert_round_trip(NonZeroU32::new(32).unwrap(), Value::Long(32));
    assert!(NonZeroI64::try_from(Value::Long(0)).is_err());
    assert!(NonZeroI8::try_from(Value::Long(128)).is_err());
}

#[test]
fn duration_support_is_lossless_and_ordered() {
    use std::time::Duration;

    let short = Duration::new(1, 2);
    let long = Duration::new(2, 0);
    assert_round_trip(short, Value::Bytes(encoded(&short)));
    assert_round_trip(
        Duration::new(u64::MAX, 999_999_999),
        Value::Bytes(encoded(&Duration::new(u64::MAX, 999_999_999))),
    );
    assert!(encoded(&short) < encoded(&long));
    assert!(Duration::decode(&[0; 11]).is_err());
}

#[test]
fn system_time_support_is_lossless_and_ordered() {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let before = UNIX_EPOCH - Duration::new(1, 500_000_000);
    let after = UNIX_EPOCH + Duration::new(1, 500_000_000);
    for value in [before, UNIX_EPOCH, after] {
        assert_round_trip(value, Value::Bytes(encoded(&value)));
    }
    assert!(encoded(&before) < encoded(&UNIX_EPOCH));
    assert!(encoded(&UNIX_EPOCH) < encoded(&after));
    assert!(SystemTime::decode(&[0; 15]).is_err());
}

#[cfg(feature = "camino")]
#[test]
fn camino_support_round_trips() {
    let value = camino::Utf8PathBuf::from("/tmp/example.txt");
    assert_round_trip(value.clone(), Value::String(value.to_string()));
}

#[cfg(all(feature = "derive", feature = "camino"))]
#[test]
fn camino_values_work_with_entity_derive() -> factomic::Result<()> {
    use std::sync::Arc;

    use factomic::{Schema, Store};

    #[allow(dead_code)]
    #[derive(factomic::Entity)]
    struct Document {
        #[factomic(value)]
        path: camino::Utf8PathBuf,
    }

    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(directory.path())?);
    DocumentSchema::declare(&store)?;
    let document = store.create::<Document>()?;
    let path = camino::Utf8PathBuf::from("/tmp/example.txt");
    document.transact().set_path(path.clone()).commit()?;
    assert_eq!(document.read().path()?, Some(path));
    Ok(())
}

#[cfg(feature = "url")]
#[test]
fn url_support_round_trips() {
    let value = url::Url::parse("https://example.com/path?q=1").unwrap();
    assert_round_trip(value.clone(), Value::String(value.to_string()));
}

#[cfg(feature = "bytes")]
#[test]
fn bytes_support_round_trips() {
    let value = bytes::Bytes::from_static(b"hello");
    assert_round_trip(value, Value::Bytes(b"hello".to_vec()));
}

#[cfg(feature = "uuid")]
#[test]
fn uuid_support_round_trips() {
    let value = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    assert_round_trip(value, Value::Bytes(value.as_bytes().to_vec()));
}

#[cfg(feature = "rust_decimal")]
#[test]
fn rust_decimal_support_round_trips() {
    let value: rust_decimal::Decimal = "1234567890.12345".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
}

#[cfg(feature = "semver")]
#[test]
fn semver_support_round_trips() {
    let value = semver::Version::parse("1.2.3-alpha.1").unwrap();
    assert_round_trip(value.clone(), Value::String(value.to_string()));
}

#[cfg(feature = "ulid")]
#[test]
fn ulid_support_round_trips() {
    let value: ulid::Ulid = "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
}

#[cfg(feature = "ipnet")]
#[test]
fn ipnet_support_round_trips() {
    let value: ipnet::IpNet = "192.168.0.0/24".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value: ipnet::Ipv4Net = "192.168.0.0/24".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value: ipnet::Ipv6Net = "2001:db8::/32".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
}

#[cfg(feature = "bigdecimal")]
#[test]
fn bigdecimal_support_round_trips() {
    let value: bigdecimal::BigDecimal = "12345678901234567890.12345".parse().unwrap();
    assert_round_trip(value.clone(), Value::String(value.to_string()));
}

#[cfg(feature = "num-bigint")]
#[test]
fn num_bigint_support_round_trips() {
    let value: num_bigint::BigInt = "-123456789012345678901234567890".parse().unwrap();
    assert_round_trip(value.clone(), Value::String(value.to_string()));
    let value: num_bigint::BigUint = "123456789012345678901234567890".parse().unwrap();
    assert_round_trip(value.clone(), Value::String(value.to_string()));
}

#[cfg(feature = "http")]
#[test]
fn http_uri_support_round_trips() {
    let value: http::Uri = "https://example.com/path?q=1".parse().unwrap();
    assert_round_trip(value.clone(), Value::String(value.to_string()));
}

#[cfg(feature = "serde-json")]
#[test]
fn serde_json_support_round_trips() {
    let value = serde_json::json!({"name": "example", "values": [1, 2, 3]});
    assert_round_trip(value.clone(), Value::String(value.to_string()));
}

#[cfg(feature = "chrono")]
#[test]
fn chrono_support_round_trips() {
    use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

    let value = DateTime::<Utc>::from_timestamp(1_753_000_000, 123_456_000).unwrap();
    let timestamp = jiff::Timestamp::from_microsecond(value.timestamp_micros()).unwrap();
    assert_round_trip(value, Value::Timestamp(timestamp));

    let value = DateTime::parse_from_rfc3339("2024-07-11T01:14:00+05:30").unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value = NaiveDate::from_ymd_opt(2024, 7, 11).unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value = NaiveDateTime::parse_from_str("2024-07-11 01:14:00.123456", "%Y-%m-%d %H:%M:%S%.f")
        .unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value = NaiveTime::from_hms_micro_opt(1, 14, 0, 123_456).unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
}

#[cfg(feature = "time")]
#[test]
fn time_support_round_trips() {
    use time::format_description::well_known::Iso8601;
    use time::{Date, OffsetDateTime, PrimitiveDateTime, Time};

    let value = OffsetDateTime::from_unix_timestamp_nanos(1_753_000_000_123_456_000).unwrap();
    let timestamp = jiff::Timestamp::from_microsecond(1_753_000_000_123_456).unwrap();
    assert_round_trip(value, Value::Timestamp(timestamp));
    let value = Date::parse("2024-07-11", &Iso8601::DATE).unwrap();
    assert_round_trip(value, Value::String(value.format(&Iso8601::DATE).unwrap()));
    let value = Time::parse("01:14:00.123456", &Iso8601::TIME).unwrap();
    assert_round_trip(value, Value::String(value.format(&Iso8601::TIME).unwrap()));
    let value =
        PrimitiveDateTime::parse("2024-07-11T01:14:00.123456", &Iso8601::DATE_TIME).unwrap();
    assert_round_trip(
        value,
        Value::String(value.format(&Iso8601::DATE_TIME).unwrap()),
    );
}

#[cfg(feature = "jiff-extended")]
#[test]
fn extended_jiff_support_round_trips() {
    let value: jiff::civil::Date = "2024-07-11".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value: jiff::civil::DateTime = "2024-07-11T01:14:00.123456".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value: jiff::civil::Time = "01:14:00.123456".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value: jiff::SignedDuration = "PT1H2M3S".parse().unwrap();
    assert_round_trip(value, Value::String(value.to_string()));
    let value: jiff::ZonedDateTime = "2024-07-11T01:14:00-04:00[America/New_York]"
        .parse()
        .unwrap();
    assert_round_trip(value.clone(), Value::String(value.to_string()));

    let value: jiff::Span = "P1Y2M3DT4H5M6S".parse().unwrap();
    assert_eq!(<jiff::Span as Primitive>::TYPE, factomic::Type::String);
    let decoded = jiff::Span::decode(&encoded(&value)).unwrap();
    assert_eq!(decoded.fieldwise(), value.fieldwise());
    let decoded = jiff::Span::try_from(Value::String(value.to_string())).unwrap();
    assert_eq!(decoded.fieldwise(), value.fieldwise());
}
