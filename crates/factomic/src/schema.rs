use std::sync::Arc;

use crate::Db;
use crate::EntityId;
use crate::Result;
use crate::Store;

pub trait Entity: Sized {
    type Schema: Schema;
}

pub trait Schema: Sized {
    fn declare(store: &Store) -> Result<Self>;
}

pub struct DbHandle<T: Entity> {
    db: Arc<Db>,
    schema: Arc<T::Schema>,
    entity: EntityId,
}

#[cfg(test)]
mod tests {
    use crate::{
        AttributeDef, AttributeId, CollectionKind, Result, Store, Type,
        schema::{DbHandle, Entity, Schema},
    };

    struct Person {
        name: String,
        age: i64,
        bucket_list: Vec<String>,
    }

    struct PersonSchema {
        name: AttributeId,
        age: AttributeId,
        bucket_list: AttributeId,
    }

    impl PersonSchema {
        fn name_def() -> AttributeDef {
            AttributeDef {
                name: "person/name".into(),
                value_type: Type::String,
                kind: CollectionKind::One,
                key_type: None,
                indexed: false,
                is_component: false,
            }
        }
        fn age_def() -> AttributeDef {
            AttributeDef {
                name: "person/age".into(),
                value_type: Type::Long,
                kind: CollectionKind::One,
                key_type: None,
                indexed: false,
                is_component: false,
            }
        }
        fn bucket_list_def() -> AttributeDef {
            AttributeDef {
                name: "person/bucket_list".into(),
                value_type: Type::String,
                kind: CollectionKind::List,
                key_type: None,
                indexed: false,
                is_component: false,
            }
        }
    }

    impl Entity for Person {
        type Schema = PersonSchema;
    }

    impl Schema for PersonSchema {
        fn declare(store: &Store) -> Result<Self> {
            Ok(Self {
                name: store.declare_attribute(PersonSchema::name_def())?,
                age: store.declare_attribute(PersonSchema::age_def())?,
                bucket_list: store.declare_attribute(PersonSchema::bucket_list_def())?,
            })
        }
    }

    pub trait PersonEntity {
        fn name(&self) -> Result<Option<String>>;
        fn age(&self) -> Result<Option<i64>>;
        fn bucket_list(&self) -> Result<Vec<String>>;
    }

    impl PersonEntity for DbHandle<Person> {
        //to-do: realize() fn to create Person?
        fn name(&self) -> Result<Option<String>> {
            self.db.entity_value(self.entity, self.schema.name)
        }
        fn age(&self) -> Result<Option<i64>> {
            self.db.entity_value(self.entity, self.schema.age)
        }
        fn bucket_list(&self) -> Result<Vec<String>> {
            self.db.entity_values(self.entity, self.schema.bucket_list)
        }
    }
}
