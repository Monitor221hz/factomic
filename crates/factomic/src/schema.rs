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
    fn get(db: &Arc<Db>) -> Result<Option<Self>>;
}

pub struct DbHandle<T: Entity> {
    pub db: Arc<Db>,
    pub schema: Arc<T::Schema>,
    pub entity: EntityId,
}

impl Db {
    pub fn entity<T>(self: &Arc<Self>, entity: EntityId) -> Result<Option<DbHandle<T>>>
    where
        T: Entity,
    {
        let Some(schema) = T::Schema::get(self)? else {
            return Ok(None);
        };
        Ok(Some(DbHandle {
            db: self.clone(),
            schema: schema.into(),
            entity,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        AttributeDef, AttributeId, CollectionKind, EntityId, Result, Store, Type,
        schema::{DbHandle, Entity, Schema},
    };

    struct Person {
        name: String,
        age: i64,
        bucket_list: Vec<String>,
        ancestor: Arc<Person>,
        children: Vec<Person>,
    }

    struct PersonSchema {
        name: AttributeId,
        age: AttributeId,
        bucket_list: AttributeId,
        ancestor: AttributeId,
        children: AttributeId,
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
        fn ancestor_def() -> AttributeDef {
            AttributeDef {
                name: "person/ancestor".into(),
                value_type: Type::String,
                kind: CollectionKind::One,
                key_type: None,
                indexed: false,
                is_component: false,
            }
        }
        fn children_def() -> AttributeDef {
            AttributeDef {
                name: "person/children".into(),
                value_type: Type::Ref,
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
                ancestor: store.declare_attribute(PersonSchema::ancestor_def())?,
                children: store.declare_attribute(PersonSchema::children_def())?,
            })
        }

        fn get(db: &std::sync::Arc<crate::Db>) -> Result<Option<Self>> {
            let Some(name) = db.attribute_id("person/name")? else {
                // todo: bevy_reflect type names instead of writing them raw?
                return Ok(None);
            };
            let Some(age) = db.attribute_id("person/age")? else {
                return Ok(None);
            };
            let Some(bucket_list) = db.attribute_id("person/bucket_list")? else {
                return Ok(None);
            };
            let Some(ancestor) = db.attribute_id("person/ancestor")? else {
                return Ok(None);
            };
            let Some(children) = db.attribute_id("person/children")? else {
                return Ok(None);
            };
            Ok(Some(Self {
                name,
                age,
                bucket_list,
                ancestor,
                children,
            }))
        }
    }

    pub trait PersonEntity {
        fn name(&self) -> Result<Option<String>>;
        fn age(&self) -> Result<Option<i64>>;
        fn bucket_list(&self) -> Result<Vec<String>>;
        fn ancestor(&self) -> Result<Option<DbHandle<Person>>>;
        fn children(&self) -> Result<Vec<DbHandle<Person>>>;
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

        fn ancestor(&self) -> Result<Option<DbHandle<Person>>> {
            let Some(entity) = self
                .db
                .entity_value::<EntityId>(self.entity, self.schema.ancestor)?
            else {
                return Ok(None);
            };
            self.db.entity(entity)
        }

        fn children(&self) -> Result<Vec<DbHandle<Person>>> {
            self.db
                .entity_values::<EntityId>(self.entity, self.schema.ancestor)?
                .into_iter()
                .filter_map(|e| self.db.entity(e).transpose())
                .collect()
        }
    }
}
