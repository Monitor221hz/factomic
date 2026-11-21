use crate::{MapKey, value::Type};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("storage error")]
    Storage(#[from] fjall::Error),

    #[error("unknown attribute: {0}")]
    UnknownAttribute(String),

    #[error("attribute is not indexed: {0}")]
    AttributeNotIndexed(String),

    #[error("type mismatch on {attribute}: expected {expected:?}, got {got:?}")]
    TypeMismatch {
        attribute: String,
        expected: Type,
        got: Type,
    },

    #[error("attribute {0} already exists with a different definition")]
    SchemaConflict(String),

    #[error("unsupported storage format version: {0}")]
    UnsupportedFormatVersion(u64),

    #[error("value type mismatch: expected {expected:?}, got {got:?}")]
    ValueTypeMismatch { expected: Type, got: Type },

    #[error("map key type mismatch")]
    MapKeyTypeMismatch,

    #[error("corrupt value for type {0:?}")]
    CorruptPrimitive(Type),

    #[error("unknown value tag: {0:#04x}")]
    UnknownValueTag(u8),

    #[error("unknown op byte: {0:#04x}")]
    UnknownOp(u8),

    #[error("unknown collection kind: {0:#04x}")]
    UnknownKind(u8),

    #[error("attribute {0} requires a non-empty key")]
    RequiresKey(String),

    #[error("invalid key type for attribute {0}")]
    InvalidKeyType(String),

    #[error("attribute {0} does not accept a key")]
    UnexpectedKey(String),

    #[error("attribute {0}: key does not match the collection kind")]
    MismatchedKey(String),

    #[error("position too long: {0} bytes (max 254)")]
    PositionTooLong(usize),

    #[error("corrupt position")]
    CorruptPosition,

    #[error("slot not found on attribute {0}")]
    SlotNotFound(String),

    #[error("component conflict on attribute {0}")]
    ComponentConflict(String),

    #[error("invalid schema for attribute {0}")]
    InvalidSchema(String),

    #[error("corrupt key: expected at least {expected} bytes, got {got}")]
    CorruptKey { expected: usize, got: usize },

    #[error("utf 8 err")]
    StringError(#[from] std::str::Utf8Error),

    #[error("time conversion error: {0}")]
    TimeError(#[from] jiff::Error),

    #[error("slice error {0}")]
    SliceError(#[from] std::array::TryFromSliceError),
}

pub type Result<T> = std::result::Result<T, Error>;
