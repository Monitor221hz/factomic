//! A temporal, embedded database model specialized for records-as-data applications.
//!
//! A factomic database is a set of immutable atomic facts called [Factom]s. Factoms are only appended to the database, ensuring an audit trail and the capability
//! to query the database state at different points in time, much like its primary inspiration, Datomic.
//!
//! A fact is, in Datomic's words, something done at a point in time that is known to have happened.
//!
//! The typical datom relates:
//! (E)ntity to a (V)alue via an (A)ttribute in (T)xn (which points to an entity that contains point in time *t* in its attribute tx/Instant)
//!
//! Factomic diverges from Datomic in the introduction of Slots.
//!
//! A [Factom] relates:
//! (Entity) to a (V)alue via an (A)ttribute at (S)lot in (T)xn (which points to an entity that contains point in time *t* in its attribute tx/Timestamp)

#![forbid(unsafe_code)]

mod error;
mod factom;
mod id;
mod keys;
mod primitive;
mod schema;
mod store;
mod value;

pub use error::{Error, Result};
pub use factom::{CollectionKind, Factom, MapKey, Op, Slot};
pub use id::{AttributeId, EntityId, TxnId};
pub use primitive::{DecodePrimitive, EncodePrimitive, Primitive};
pub use store::{AttributeDef, Db, Store, Transaction};
pub use value::{Type, Value};
