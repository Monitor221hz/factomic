use crate::value::Type;

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

    #[error("value type mismatch: expected {expected:?}, got {got:?}")]
    ValueTypeMismatch { expected: Type, got: Type },

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
