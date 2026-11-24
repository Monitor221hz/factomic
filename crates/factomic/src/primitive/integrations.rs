use crate::{Error, Result};

fn to_string<T: ToString>(value: &T) -> String {
    value.to_string()
}

fn from_string<T: std::str::FromStr>(value: String) -> Result<T> {
    value
        .parse()
        .map_err(|_| Error::CorruptPrimitive(crate::Type::String))
}

mod standard {
    use super::{from_string, to_string};
    use crate::{DecodePrimitive, EncodePrimitive, Error, Primitive, Result, Type, Value};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
    use std::num::{
        NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroU8, NonZeroU16, NonZeroU32,
    };
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    crate::impl_string_primitive!(char, to_string, from_string);
    crate::impl_string_primitive!(IpAddr, to_string, from_string);
    crate::impl_string_primitive!(Ipv4Addr, to_string, from_string);
    crate::impl_string_primitive!(Ipv6Addr, to_string, from_string);
    crate::impl_string_primitive!(SocketAddr, to_string, from_string);
    crate::impl_string_primitive!(SocketAddrV4, to_string, from_string);
    crate::impl_string_primitive!(SocketAddrV6, to_string, from_string);

    macro_rules! impl_nonzero_primitive {
        ($ty:ty, $inner:ty) => {
            crate::impl_long_primitive!($ty, |value: &$ty| i64::from(value.get()), |value: i64| {
                let value =
                    <$inner>::try_from(value).map_err(|_| Error::CorruptPrimitive(Type::Long))?;
                <$ty>::new(value).ok_or(Error::CorruptPrimitive(Type::Long))
            },);
        };
    }

    impl_nonzero_primitive!(NonZeroI8, i8);
    impl_nonzero_primitive!(NonZeroI16, i16);
    impl_nonzero_primitive!(NonZeroI32, i32);
    impl_nonzero_primitive!(NonZeroI64, i64);
    impl_nonzero_primitive!(NonZeroU8, u8);
    impl_nonzero_primitive!(NonZeroU16, u16);
    impl_nonzero_primitive!(NonZeroU32, u32);

    fn f32_from_f64(value: f64) -> Result<f32> {
        let narrowed = value as f32;
        if value.is_finite() && narrowed.is_infinite() {
            Err(Error::CorruptPrimitive(Type::Double))
        } else {
            Ok(narrowed)
        }
    }

    impl Primitive for f32 {
        const TYPE: Type = Type::Double;
    }

    impl EncodePrimitive for f32 {
        fn encode(&self, buf: &mut Vec<u8>) {
            f64::from(*self).encode(buf);
        }
    }

    impl DecodePrimitive<'_> for f32 {
        fn decode(bytes: &[u8]) -> Result<Self> {
            f32_from_f64(f64::decode(bytes)?)
        }
    }

    impl TryFrom<Value> for f32 {
        type Error = Error;

        fn try_from(value: Value) -> Result<Self> {
            f32_from_f64(f64::try_from(value)?)
        }
    }

    impl From<f32> for Value {
        fn from(value: f32) -> Self {
            Value::Double(f64::from(value))
        }
    }

    impl Primitive for Duration {
        const TYPE: Type = Type::Bytes;
    }

    impl EncodePrimitive for Duration {
        fn encode(&self, buf: &mut Vec<u8>) {
            buf.extend_from_slice(&self.as_secs().to_be_bytes());
            buf.extend_from_slice(&self.subsec_nanos().to_be_bytes());
        }
    }

    impl DecodePrimitive<'_> for Duration {
        fn decode(bytes: &[u8]) -> Result<Self> {
            let bytes: [u8; 12] = bytes
                .try_into()
                .map_err(|_| Error::CorruptPrimitive(Type::Bytes))?;
            let seconds = u64::from_be_bytes(bytes[..8].try_into().expect("fixed length"));
            let nanos = u32::from_be_bytes(bytes[8..].try_into().expect("fixed length"));
            if nanos >= 1_000_000_000 {
                return Err(Error::CorruptPrimitive(Type::Bytes));
            }
            Ok(Duration::new(seconds, nanos))
        }
    }

    impl TryFrom<Value> for Duration {
        type Error = Error;

        fn try_from(value: Value) -> Result<Self> {
            let bytes = Vec::<u8>::try_from(value)?;
            Duration::decode(&bytes)
        }
    }

    const NANOS_PER_SECOND: i128 = 1_000_000_000;
    const ORDERED_I128_SIGN: u128 = 1 << 127;

    fn system_time_nanos(value: &SystemTime) -> i128 {
        let (duration, negative) = match value.duration_since(UNIX_EPOCH) {
            Ok(duration) => (duration, false),
            Err(error) => (error.duration(), true),
        };
        let nanos =
            i128::from(duration.as_secs()) * NANOS_PER_SECOND + i128::from(duration.subsec_nanos());
        if negative { -nanos } else { nanos }
    }

    fn system_time_from_nanos(nanos: i128) -> Result<SystemTime> {
        let magnitude = nanos.unsigned_abs();
        let seconds = u64::try_from(magnitude / NANOS_PER_SECOND as u128)
            .map_err(|_| Error::CorruptPrimitive(Type::Bytes))?;
        let subsec_nanos = (magnitude % NANOS_PER_SECOND as u128) as u32;
        let duration = Duration::new(seconds, subsec_nanos);
        let value = if nanos < 0 {
            UNIX_EPOCH.checked_sub(duration)
        } else {
            UNIX_EPOCH.checked_add(duration)
        };
        value.ok_or(Error::CorruptPrimitive(Type::Bytes))
    }

    impl Primitive for SystemTime {
        const TYPE: Type = Type::Bytes;
    }

    impl EncodePrimitive for SystemTime {
        fn encode(&self, buf: &mut Vec<u8>) {
            let ordered = (system_time_nanos(self) as u128) ^ ORDERED_I128_SIGN;
            buf.extend_from_slice(&ordered.to_be_bytes());
        }
    }

    impl DecodePrimitive<'_> for SystemTime {
        fn decode(bytes: &[u8]) -> Result<Self> {
            let bytes: [u8; 16] = bytes
                .try_into()
                .map_err(|_| Error::CorruptPrimitive(Type::Bytes))?;
            let ordered = u128::from_be_bytes(bytes);
            system_time_from_nanos((ordered ^ ORDERED_I128_SIGN) as i128)
        }
    }

    impl TryFrom<Value> for SystemTime {
        type Error = Error;

        fn try_from(value: Value) -> Result<Self> {
            let bytes = Vec::<u8>::try_from(value)?;
            SystemTime::decode(&bytes)
        }
    }
}

#[cfg(feature = "camino")]
mod camino_support {
    crate::impl_string_primitive!(camino::Utf8PathBuf, super::to_string, super::from_string);
}

#[cfg(feature = "url")]
mod url_support {
    crate::impl_string_primitive!(url::Url, super::to_string, super::from_string);
}

#[cfg(feature = "bytes")]
mod bytes_support {
    fn as_bytes(value: &bytes::Bytes) -> &[u8] {
        value.as_ref()
    }

    crate::impl_bytes_primitive!(bytes::Bytes, as_bytes, |value: Vec<u8>| Ok(
        bytes::Bytes::from(value)
    ));
}

#[cfg(feature = "uuid")]
mod uuid_support {
    use crate::{Error, Result, Type};

    fn as_bytes(value: &uuid::Uuid) -> &[u8] {
        value.as_bytes()
    }

    fn from_bytes(value: Vec<u8>) -> Result<uuid::Uuid> {
        uuid::Uuid::from_slice(&value).map_err(|_| Error::CorruptPrimitive(Type::Bytes))
    }

    crate::impl_bytes_primitive!(uuid::Uuid, as_bytes, from_bytes);
}

#[cfg(feature = "rust_decimal")]
mod rust_decimal_support {
    crate::impl_string_primitive!(rust_decimal::Decimal, super::to_string, super::from_string);
}

#[cfg(feature = "semver")]
mod semver_support {
    crate::impl_string_primitive!(semver::Version, super::to_string, super::from_string);
}

#[cfg(feature = "ulid")]
mod ulid_support {
    crate::impl_string_primitive!(ulid::Ulid, super::to_string, super::from_string);
}

#[cfg(feature = "ipnet")]
mod ipnet_support {
    crate::impl_string_primitive!(ipnet::IpNet, super::to_string, super::from_string);
    crate::impl_string_primitive!(ipnet::Ipv4Net, super::to_string, super::from_string);
    crate::impl_string_primitive!(ipnet::Ipv6Net, super::to_string, super::from_string);
}

#[cfg(feature = "bigdecimal")]
mod bigdecimal_support {
    crate::impl_string_primitive!(bigdecimal::BigDecimal, super::to_string, super::from_string);
}

#[cfg(feature = "num-bigint")]
mod num_bigint_support {
    crate::impl_string_primitive!(num_bigint::BigInt, super::to_string, super::from_string);
    crate::impl_string_primitive!(num_bigint::BigUint, super::to_string, super::from_string);
}

#[cfg(feature = "http")]
mod http_support {
    crate::impl_string_primitive!(http::Uri, super::to_string, super::from_string);
}

#[cfg(feature = "serde-json")]
mod serde_json_support {
    crate::impl_string_primitive!(serde_json::Value, super::to_string, super::from_string);
}

#[cfg(feature = "chrono")]
mod chrono_support {
    use crate::{Error, Type};
    use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

    fn datetime_from_string(value: String) -> crate::Result<NaiveDateTime> {
        value
            .replacen(' ', "T", 1)
            .parse()
            .map_err(|_| Error::CorruptPrimitive(Type::String))
    }

    crate::impl_timestamp_primitive!(
        DateTime<Utc>,
        |value: &DateTime<Utc>| value.timestamp_micros(),
        |micros: i64| {
            DateTime::from_timestamp_micros(micros).ok_or(Error::CorruptPrimitive(Type::Timestamp))
        },
    );
    crate::impl_string_primitive!(
        DateTime<chrono::FixedOffset>,
        super::to_string,
        super::from_string
    );
    crate::impl_string_primitive!(NaiveDate, super::to_string, super::from_string);
    crate::impl_string_primitive!(NaiveDateTime, super::to_string, datetime_from_string);
    crate::impl_string_primitive!(NaiveTime, super::to_string, super::from_string);
}

#[cfg(feature = "time")]
mod time_support {
    use crate::{Error, Result, Type};
    use time::format_description::well_known::Iso8601;
    use time::{Date, OffsetDateTime, PrimitiveDateTime, Time};

    fn date_to_string(value: &Date) -> String {
        value.format(&Iso8601::DATE).expect("valid ISO 8601 date")
    }

    fn date_from_string(value: String) -> Result<Date> {
        Date::parse(&value, &Iso8601::DATE).map_err(|_| Error::CorruptPrimitive(Type::String))
    }

    fn time_to_string(value: &Time) -> String {
        value.format(&Iso8601::TIME).expect("valid ISO 8601 time")
    }

    fn time_from_string(value: String) -> Result<Time> {
        Time::parse(&value, &Iso8601::TIME).map_err(|_| Error::CorruptPrimitive(Type::String))
    }

    fn datetime_to_string(value: &PrimitiveDateTime) -> String {
        value
            .format(&Iso8601::DATE_TIME)
            .expect("valid ISO 8601 datetime")
    }

    fn datetime_from_string(value: String) -> Result<PrimitiveDateTime> {
        PrimitiveDateTime::parse(&value, &Iso8601::DATE_TIME)
            .map_err(|_| Error::CorruptPrimitive(Type::String))
    }

    crate::impl_timestamp_primitive!(
        OffsetDateTime,
        |value: &OffsetDateTime| (value.unix_timestamp_nanos() / 1_000) as i64,
        |micros: i64| {
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(micros) * 1_000)
                .map_err(|_| Error::CorruptPrimitive(Type::Timestamp))
        },
    );
    crate::impl_string_primitive!(Date, date_to_string, date_from_string);
    crate::impl_string_primitive!(PrimitiveDateTime, datetime_to_string, datetime_from_string);
    crate::impl_string_primitive!(Time, time_to_string, time_from_string);
}

#[cfg(feature = "jiff-extended")]
mod jiff_support {
    crate::impl_string_primitive!(jiff::civil::Date, super::to_string, super::from_string);
    crate::impl_string_primitive!(jiff::civil::DateTime, super::to_string, super::from_string);
    crate::impl_string_primitive!(jiff::civil::Time, super::to_string, super::from_string);
    crate::impl_string_primitive!(jiff::SignedDuration, super::to_string, super::from_string);
    crate::impl_string_primitive!(jiff::Span, super::to_string, super::from_string);
    crate::impl_string_primitive!(jiff::ZonedDateTime, super::to_string, super::from_string);
}
