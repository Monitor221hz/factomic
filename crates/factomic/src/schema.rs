use std::sync::Arc;
use std::sync::Weak;

use arc_swap::ArcSwap;

use crate::Db;
use crate::EntityId;
use crate::Result;
use crate::Store;
use crate::Transaction;
use crate::TxnId;

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

impl<T: Entity> From<DbHandle<T>> for EntityId {
    fn from(value: DbHandle<T>) -> Self {
        value.entity
    }
}

impl<T: Entity> From<&DbHandle<T>> for EntityId {
    fn from(value: &DbHandle<T>) -> Self {
        value.entity
    }
}

pub struct DbWeakHandle<T> {
    pub entity: EntityId,
    pub db: Arc<Db>,
    model: ArcSwap<T>,
}

impl<T: Entity> From<DbWeakHandle<T>> for EntityId {
    fn from(value: DbWeakHandle<T>) -> Self {
        value.entity
    }
}

impl<T> DbWeakHandle<T> {}

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
    // todo: entity, entity_at, entity_at_time
}

pub struct DbHandleMut<T: Entity> {
    pub db: Arc<Store>,
    pub schema: Arc<T::Schema>,
    pub entity: EntityId,
}

impl<T: Entity> From<DbHandleMut<T>> for EntityId {
    fn from(value: DbHandleMut<T>) -> Self {
        value.entity
    }
}

pub struct EntityTransaction<'a, T: Entity> {
    handle: &'a DbHandleMut<T>,
    transaction: Transaction<'a>,
}

impl<'a, T: Entity> EntityTransaction<'a, T> {
    pub fn commit(mut self) -> Result<Option<TxnId>> {
        self.transaction.commit()
    }
}

impl<T: Entity> DbHandleMut<T> {
    pub fn transact(&self) -> EntityTransaction<'_, T> {
        EntityTransaction {
            handle: self,
            transaction: self.db.transact(),
        }
    }

    pub fn read(&self) -> DbHandle<T> {
        DbHandle {
            db: self.db.db(),
            schema: self.schema.clone(),
            entity: self.entity,
        }
    }
    // todo: read, read_at, read_at_time?
}

impl Store {
    pub fn entity_mut<T>(self: &Arc<Self>, entity: EntityId) -> Result<Option<DbHandleMut<T>>>
    where
        T: Entity,
    {
        let Some(schema) = T::Schema::get(&self.db())? else {
            return Ok(None);
        };
        Ok(Some(DbHandleMut {
            db: self.clone(),
            schema: schema.into(),
            entity,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use tempfile::TempDir;

    use crate::{
        AttributeDef, AttributeId, CollectionKind, EntityId, Result, Store, Type,
        schema::{DbHandle, Entity, EntityTransaction, Schema},
    };

    fn temp_store() -> (TempDir, Arc<Store>) {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(Store::open(dir.path()).unwrap());
        (dir, store)
    }

    pub struct Person {
        name: String,
        age: i64,
        bucket_list: Vec<String>,
        ancestor: Arc<Person>,
        children: Vec<Person>,
        friends: HashMap<String, Person>,
    }

    pub struct PersonSchema {
        name: AttributeId,
        age: AttributeId,
        bucket_list: AttributeId,
        ancestor: AttributeId,
        children: AttributeId,
        friends: AttributeId,
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
                value_type: Type::Ref,
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
        fn friends_def() -> AttributeDef {
            AttributeDef {
                name: "person/friends".into(),
                value_type: Type::Ref,
                kind: CollectionKind::Map,
                key_type: Some(Type::String),
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
                friends: store.declare_attribute(PersonSchema::friends_def())?,
            })
        }

        fn get(db: &std::sync::Arc<crate::Db>) -> Result<Option<Self>> {
            let Some(name) = db.attribute_id("person/name")? else {
                // todo: derive should
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
            let Some(friends) = db.attribute_id("person/friends")? else {
                return Ok(None);
            };
            Ok(Some(Self {
                name,
                age,
                bucket_list,
                ancestor,
                children,
                friends,
            }))
        }
    }

    pub trait PersonEntity {
        fn name(&self) -> Result<Option<String>>;
        fn age(&self) -> Result<Option<i64>>;
        fn bucket_list(&self) -> impl Iterator<Item = Result<String>> + '_;
        fn ancestor(&self) -> Result<Option<DbHandle<Person>>>;
        fn children(&self) -> impl Iterator<Item = Result<DbHandle<Person>>> + '_;
        fn children_at(&self, ordinal: usize) -> Result<Option<DbHandle<Person>>>;
        fn friends(&self) -> impl Iterator<Item = Result<(String, DbHandle<Person>)>> + '_;
        fn friends_seek(&self, key: &str) -> Result<Option<DbHandle<Person>>>;
    }

    impl PersonEntity for DbHandle<Person> {
        //to-do: realize() fn to create Person?
        fn name(&self) -> Result<Option<String>> {
            self.db.entity_value(self.entity, self.schema.name)
        }

        fn age(&self) -> Result<Option<i64>> {
            self.db.entity_value(self.entity, self.schema.age)
        }

        fn bucket_list(&self) -> impl Iterator<Item = Result<String>> + '_ {
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

        fn children(&self) -> impl Iterator<Item = Result<DbHandle<Person>>> + '_ {
            self.db
                .entity_values::<EntityId>(self.entity, self.schema.children)
                .filter_map(|result| match result {
                    Ok(entity) => self.db.entity(entity).transpose(),
                    Err(error) => Some(Err(error)),
                })
        }

        fn children_at(&self, ordinal: usize) -> Result<Option<DbHandle<Person>>> {
            let Some(entity) =
                self.db
                    .entity_value_at::<EntityId>(self.entity, self.schema.children, ordinal)?
            else {
                return Ok(None);
            };
            self.db.entity(entity)
        }

        fn friends_seek(&self, key: &str) -> Result<Option<DbHandle<Person>>> {
            let Some(entity) =
                self.db
                    .entity_keyvalue::<EntityId>(self.entity, self.schema.friends, key)?
            else {
                return Ok(None);
            };
            self.db.entity(entity)
        }

        fn friends(&self) -> impl Iterator<Item = Result<(String, DbHandle<Person>)>> + '_ {
            self.db
                .entity_keyvalues::<String, EntityId>(self.entity, self.schema.friends)
                .into_iter()
                .filter_map(|r| match r {
                    Ok(p) => {
                        let Some(entity) = self.db.entity(p.1).transpose() else {
                            return None;
                        };
                        Some((p.0, entity))
                    }
                    Err(_) => None,
                })
                .map(|(name, friend)| Ok((name, friend?)))
        }
    }

    pub trait PersonEntityMut: Sized {
        fn set_name(&mut self, name: &str) -> &mut Self;
        fn set_age(&mut self, age: i64) -> &mut Self;
        fn set_bucket_list<'a>(&mut self, bucket_list: impl Iterator<Item = &'a str>) -> &mut Self;
        fn set_ancestor(&mut self, ancestor: impl Into<EntityId>) -> &mut Self;
        fn set_children(
            &mut self,
            children: impl Iterator<Item = impl Into<EntityId>>,
        ) -> &mut Self;
        fn set_children_at(&mut self, ordinal: usize, value: impl Into<EntityId>) -> &mut Self;
        fn set_friends(
            &mut self,
            kvp: impl Iterator<Item = (String, impl Into<EntityId>)>,
        ) -> &mut Self;
        fn set_friends_seek(&mut self, key: &str, value: impl Into<EntityId>) -> &mut Self;
    }

    impl<'a> PersonEntityMut for EntityTransaction<'a, Person> {
        fn set_name(&mut self, name: &str) -> &mut Self {
            self.transaction.assert_value(
                self.handle.entity,
                self.handle.schema.name,
                &name.into(),
            );
            self
        }

        fn set_age(&mut self, age: i64) -> &mut Self {
            self.transaction
                .assert(self.handle.entity, self.handle.schema.age, &age);
            self
        }

        fn set_bucket_list<'b>(&mut self, bucket_list: impl Iterator<Item = &'b str>) -> &mut Self {
            self.transaction.replace_list(
                self.handle.entity,
                self.handle.schema.bucket_list,
                bucket_list,
            );
            self
        }

        fn set_ancestor(&mut self, ancestor: impl Into<EntityId>) -> &mut Self {
            let ancestor = ancestor.into();
            self.transaction
                .assert(self.handle.entity, self.handle.schema.ancestor, &ancestor);
            self
        }

        fn set_children(
            &mut self,
            children: impl Iterator<Item = impl Into<EntityId>>,
        ) -> &mut Self {
            self.transaction.replace_list(
                self.handle.entity,
                self.handle.schema.children,
                children.map(Into::into),
            );
            self
        }

        fn set_children_at(&mut self, ordinal: usize, value: impl Into<EntityId>) -> &mut Self {
            let value = value.into();
            self.transaction.replace_list_at(
                self.handle.entity,
                self.handle.schema.children,
                ordinal,
                &value,
            );
            self
        }

        fn set_friends(
            &mut self,
            kvp: impl Iterator<Item = (String, impl Into<EntityId>)>,
        ) -> &mut Self {
            self.transaction.replace_map(
                self.handle.entity,
                self.handle.schema.friends,
                kvp.map(|(name, friend)| (name, friend.into())),
            );
            self
        }

        fn set_friends_seek(&mut self, key: &str, value: impl Into<EntityId>) -> &mut Self {
            let value = value.into();
            self.transaction.put(
                self.handle.entity,
                self.handle.schema.friends,
                key.into(),
                &value,
            );
            self
        }
    }

    #[test]
    fn typed_mutations_accept_entity_ids_and_borrowed_handles() {
        let (_dir, store) = temp_store();
        PersonSchema::declare(&store).unwrap();
        let person_id = store.new_entity().unwrap();
        let first = store.new_entity().unwrap();
        let second = store.new_entity().unwrap();
        let person = store.entity_mut::<Person>(person_id).unwrap().unwrap();
        let first_handle = store.db().entity::<Person>(first).unwrap().unwrap();

        let mut tx = person.transact();
        tx.set_ancestor(&first_handle)
            .set_children([first, second].into_iter())
            .set_children_at(1, &first_handle)
            .set_friends([("first".to_string(), second)].into_iter())
            .set_friends_seek("second", &first_handle);
        tx.commit().unwrap().unwrap();

        let person = store.db().entity::<Person>(person_id).unwrap().unwrap();
        assert_eq!(person.ancestor().unwrap().unwrap().entity, first);
        assert_eq!(
            person
                .children()
                .map(|child| child.unwrap().entity)
                .collect::<Vec<_>>(),
            vec![first, first]
        );
        assert_eq!(person.children_at(1).unwrap().unwrap().entity, first);
        assert_eq!(
            person.friends_seek("first").unwrap().unwrap().entity,
            second
        );
        assert_eq!(
            person.friends_seek("second").unwrap().unwrap().entity,
            first
        );
        assert_eq!(
            person
                .friends()
                .map(|friend| {
                    let (name, friend) = friend.unwrap();
                    (name, friend.entity)
                })
                .collect::<Vec<_>>(),
            vec![("first".into(), second), ("second".into(), first)]
        );
    }
}
