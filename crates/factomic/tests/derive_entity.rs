#![cfg(feature = "derive")]
#![allow(dead_code)]

use std::{collections::HashMap, sync::Arc};

use factomic::{
    AttributeDef, CollectionKind, DecodePrimitive, EncodePrimitive, EntityId, EntityTransaction,
    Error, MapKey, Primitive, Result, Schema, Store, Type, Value,
};
use tempfile::TempDir;

#[derive(factomic::Entity)]
struct Car {
    name: String,
}

#[derive(factomic::Entity)]
struct Person {
    name: String,
    age: i64,
    bucket_list: Vec<String>,
    car: Car,
    ancestor: Arc<Person>,
    children: Vec<Person>,
    friends: HashMap<String, Person>,
    labels: std::collections::HashSet<String>,
}

#[derive(factomic::Entity)]
struct OptionalModel {
    nickname: Option<String>,
    car: Option<Car>,
    friends: HashMap<String, Option<Car>>,
    scores: HashMap<String, Option<i64>>,
    by_ref: HashMap<Arc<Car>, Option<String>>,
}

#[derive(factomic::Entity)]
struct SmallInts {
    signed: i8,
    medium: i32,
    unsigned: u32,
    ratio: f32,
    values: Vec<i16>,
    tags: std::collections::HashSet<u8>,
}

#[derive(factomic::Entity)]
struct Incremental {
    list: Vec<String>,
    tags: std::collections::HashSet<String>,
    scores: HashMap<String, i64>,
}

#[derive(factomic::Entity)]
#[factomic(namespace = "registry")]
struct Registry {
    enabled: bool,
    score: f64,
    created: jiff::Timestamp,
    codes: Vec<i64>,
    owners: std::collections::HashSet<Arc<Person>>,
    scores: HashMap<String, i64>,
    by_ref: HashMap<Arc<Person>, String>,
    #[factomic(name = "registry/primary", indexed)]
    primary: String,
    #[factomic(component)]
    parent: Arc<Person>,
}

mod option_shadow {
    pub struct Option<T>(pub T);

    #[derive(factomic::Entity)]
    pub struct Model {
        pub value: ::core::option::Option<String>,
        pub values: ::std::collections::HashMap<String, ::core::option::Option<String>>,
    }
}

#[derive(factomic::Entity)]
struct RawComponent {
    #[factomic(component)]
    parent: EntityId,
}

#[derive(factomic::Entity)]
#[factomic(namespace = "evolution")]
struct EvolutionV1 {
    #[factomic(name = "evolution/shared")]
    shared: String,
}

#[derive(factomic::Entity)]
#[factomic(namespace = "evolution")]
struct EvolutionV2 {
    #[factomic(name = "evolution/shared")]
    shared: String,
    added: i64,
}

#[derive(factomic::Entity)]
#[factomic(namespace = "evolution")]
struct EvolutionConflict {
    fresh: String,
    #[factomic(name = "evolution/shared", indexed)]
    shared: String,
}

#[derive(factomic::Entity)]
#[factomic(namespace = "evolution")]
struct EvolutionRenamed {
    #[factomic(name = "evolution/shared")]
    renamed: String,
}

#[derive(factomic::Entity)]
#[factomic(namespace = "evolution")]
struct EvolutionIndexMismatch {
    #[factomic(name = "evolution/shared")]
    shared: String,
    #[factomic(name = "evolution/added", indexed)]
    added: i64,
}

#[derive(factomic::Entity)]
#[factomic(namespace = "type_evolution")]
struct TypeEvolutionV1 {
    value: String,
}

#[derive(factomic::Entity)]
#[factomic(namespace = "type_evolution")]
struct TypeEvolutionV2 {
    value: i64,
}

mod shadowing {
    #![allow(non_camel_case_types)]

    pub struct Iterator;
    pub struct Into;
    pub struct Sized;
    pub struct usize;
    pub struct str;

    #[derive(factomic::Entity)]
    pub struct Model {
        pub name: String,
        pub values: Vec<String>,
        pub car: super::Car,
    }
}

mod raw_visibility {
    #[derive(factomic::Entity)]
    pub(crate) struct Model {
        pub r#type: String,
        pub r#match: Vec<String>,
    }
}

#[derive(factomic::Entity)]
struct StandardPaths {
    optional: ::core::option::Option<::std::string::String>,
    list: ::std::vec::Vec<::core::primitive::i64>,
    set: ::std::collections::HashSet<::factomic::EntityId>,
    map: ::std::collections::HashMap<::std::string::String, ::std::sync::Arc<Car>>,
    created: ::jiff::Timestamp,
}

mod custom_types {
    #[derive(factomic::Entity)]
    pub struct String {}

    #[derive(factomic::Entity)]
    pub struct Timestamp {}
}

#[derive(factomic::Entity)]
struct CustomNamedTypes {
    string: custom_types::String,
    timestamp: custom_types::Timestamp,
}

#[derive(Debug, PartialEq)]
struct Score(i64);

impl Primitive for Score {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for Score {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.0.encode(buf);
    }
}

impl DecodePrimitive<'_> for Score {
    fn decode(bytes: &[u8]) -> Result<Self> {
        i64::decode(bytes).map(Self)
    }
}

impl TryFrom<Value> for Score {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        i64::try_from(value).map(Self)
    }
}

#[derive(Debug, PartialEq)]
struct GenericScore<T>(T);

factomic::impl_long_primitive!(
    GenericScore<i64>,
    |value: &GenericScore<i64>| value.0,
    |value: i64| Ok(GenericScore(value)),
);

#[derive(Debug, PartialEq)]
struct CustomMapKey(i64);

impl Primitive for CustomMapKey {
    const TYPE: Type = Type::Long;
}

impl EncodePrimitive for CustomMapKey {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.0.encode(buf);
    }
}

impl From<CustomMapKey> for MapKey {
    fn from(value: CustomMapKey) -> Self {
        Self::Long(value.0)
    }
}

impl TryFrom<MapKey> for CustomMapKey {
    type Error = Error;

    fn try_from(value: MapKey) -> Result<Self> {
        i64::try_from(value).map(Self)
    }
}

type Alias = i64;

#[derive(factomic::Entity)]
struct ExplicitTarget {
    name: String,
}

#[derive(factomic::Entity)]
struct CustomValues {
    #[factomic(value, indexed)]
    alias: Alias,
    #[factomic(value)]
    score: Score,
    #[factomic(value)]
    scores: Vec<Score>,
    #[factomic(value)]
    optional_score: Option<Score>,
    #[factomic(value)]
    generic_score: GenericScore<i64>,
    #[factomic(ref)]
    explicit: ExplicitTarget,
}

#[derive(factomic::Entity)]
struct CustomKeyOptionalMap {
    #[factomic(value)]
    entries: HashMap<CustomMapKey, Option<i64>>,
}

fn temp_store() -> (TempDir, Arc<Store>) {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    (dir, store)
}

#[test]
fn derive_supports_typed_handles_and_collections() -> Result<()> {
    let (_dir, store) = temp_store();
    let person_id = store.new_entity()?;
    assert!(store.db().entity::<Person>(person_id)?.is_none());
    CarSchema::declare(&store)?;
    PersonSchema::declare(&store)?;

    let car = store.create::<Car>()?;
    let car_id = car.entity;
    car.transact().set_name("Roadster").commit()?;
    let car_handle = store.db().entity::<Car>(car_id)?.expect("car handle");
    let first = store.new_entity()?;
    let second = store.new_entity()?;
    let person_schema = PersonSchema::declare(&store)?;
    store.assert(first, person_schema.name, &"first")?;
    store.assert(second, person_schema.name, &"second")?;
    let first_handle = store.db().entity::<Person>(first)?.expect("person handle");
    let person = store.create::<Person>()?;
    let person_id = person.entity;

    let mut tx: EntityTransaction<'_, Person> = person.transact();
    tx.set_name("Alice")
        .set_age(42)
        .set_bucket_list(["a", "b"].into_iter())
        .set_car(&car_handle)
        .set_ancestor(&first_handle)
        .set_children([first, second].into_iter())
        .set_children_at(1, &first_handle)
        .set_friends([("first".to_string(), second)].into_iter())
        .set_friends_seek("second", &first_handle)
        .set_labels(["x", "y", "x"].into_iter());
    let txn = tx.commit()?.expect("transaction");

    let person = store
        .db()
        .entity::<Person>(person_id)?
        .expect("person handle");
    assert_eq!(person.name()?, Some("Alice".into()));
    assert_eq!(person.age()?, Some(42));
    assert_eq!(
        person.bucket_list().collect::<Result<Vec<_>>>()?,
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(person.car()?.expect("car").name()?, Some("Roadster".into()));
    assert_eq!(person.ancestor()?.expect("ancestor").entity, first);
    assert_eq!(
        person
            .children()
            .map(|child| child.map(|child| child.entity))
            .collect::<Result<Vec<_>>>()?,
        vec![first, first]
    );
    assert_eq!(person.children_at(1)?.expect("child").entity, first);
    assert_eq!(
        person.friends_seek("first")?.expect("friend").entity,
        second
    );
    assert_eq!(
        person.friends_seek("second")?.expect("friend").entity,
        first
    );
    assert_eq!(
        person.labels().collect::<Result<Vec<_>>>()?,
        vec!["x".to_string(), "y".to_string()]
    );

    let historical = person.read_at(txn)?.expect("historical person");
    assert_eq!(historical.name()?, Some("Alice".into()));

    let person_mut = store
        .entity_mut::<Person>(person_id)?
        .expect("person schema");
    let mut update = person_mut.transact();
    update.set_name("Bob");
    update.commit()?;
    let updated = store
        .db()
        .entity::<Person>(person_id)?
        .expect("updated person");
    assert_eq!(updated.name()?, Some("Bob".into()));
    assert_eq!(historical.name()?, Some("Alice".into()));
    Ok(())
}

#[test]
fn derive_generates_expected_schema_metadata() -> Result<()> {
    let (_dir, store) = temp_store();
    CarSchema::declare(&store)?;
    let schema = PersonSchema::declare(&store)?;
    let friends = store.attribute_def(schema.friends)?;
    assert_eq!(friends.name, "person/friends");
    assert_eq!(friends.kind, factomic::CollectionKind::Map);
    assert_eq!(friends.key_type, Some(factomic::Type::String));
    assert_eq!(friends.value_type, factomic::Type::Ref);
    let labels = store.attribute_def(schema.labels)?;
    assert_eq!(labels.kind, factomic::CollectionKind::Many);
    assert_eq!(PersonSchema::declare(&store)?.labels, schema.labels);
    Ok(())
}

#[test]
fn derive_supports_reference_keys_many_refs_and_attributes() -> Result<()> {
    let (_dir, store) = temp_store();
    PersonSchema::declare(&store)?;
    RegistrySchema::declare(&store)?;
    let person_mut = store.create::<Person>()?;
    let person_id = person_mut.entity;
    let person_schema = PersonSchema::declare(&store)?;
    store.assert(person_id, person_schema.name, &"target")?;
    let person = store.db().entity::<Person>(person_id)?.expect("person");
    let registry = store.create::<Registry>()?;
    let registry_id = registry.entity;
    let created = jiff::Timestamp::new(1_700_000_000, 0)?;

    let mut tx = registry.transact();
    tx.set_enabled(true)
        .set_score(1.5)
        .set_created(created)
        .set_codes([2, 4].into_iter())
        .set_owners([&person].into_iter())
        .set_scores([("a".to_string(), 7)].into_iter())
        .set_by_ref([(&person, "owner")].into_iter())
        .set_primary("main")
        .set_parent(&person);
    tx.commit()?;

    let registry = store
        .db()
        .entity::<Registry>(registry_id)?
        .expect("registry");
    assert_eq!(registry.enabled()?, Some(true));
    assert_eq!(registry.score()?, Some(1.5));
    assert_eq!(registry.created()?, Some(created));
    assert_eq!(registry.codes().collect::<Result<Vec<_>>>()?, vec![2, 4]);
    assert_eq!(
        registry
            .owners()
            .map(|owner| owner.map(|owner| owner.entity))
            .collect::<Result<Vec<_>>>()?,
        vec![person_id]
    );
    assert_eq!(
        registry.scores().collect::<Result<Vec<_>>>()?,
        vec![("a".to_string(), 7)]
    );
    assert_eq!(
        registry
            .by_ref()
            .map(|entry| {
                let (owner, value) = entry?;
                Ok((owner.entity, value))
            })
            .collect::<Result<Vec<_>>>()?,
        vec![(person_id, "owner".to_string())]
    );
    assert_eq!(registry.by_ref_seek(&person)?, Some("owner".to_string()));
    assert_eq!(registry.primary()?, Some("main".to_string()));
    assert_eq!(registry.parent()?.expect("parent").entity, person_id);

    let schema = RegistrySchema::declare(&store)?;
    let primary = store.attribute_def(schema.primary)?;
    assert_eq!(primary.name, "registry/primary");
    assert!(primary.indexed);
    assert!(store.attribute_def(schema.parent)?.is_component);
    assert_eq!(
        store.attribute_def(schema.enabled)?.name,
        "registry/enabled"
    );
    Ok(())
}

#[test]
fn derive_supports_small_integer_types() -> Result<()> {
    let (_dir, store) = temp_store();
    SmallIntsSchema::declare(&store)?;
    let model = store.create::<SmallInts>()?;
    let id = model.entity;
    let mut tx = model.transact();
    tx.set_signed(-8)
        .set_medium(-123_456)
        .set_unsigned(42)
        .set_ratio(1.5)
        .set_values([-1, 2].into_iter())
        .set_tags([1, 2, 1].into_iter());
    tx.commit()?;

    let current = store.db().entity::<SmallInts>(id)?.expect("small ints");
    assert_eq!(current.signed()?, Some(-8));
    assert_eq!(current.medium()?, Some(-123_456));
    assert_eq!(current.unsigned()?, Some(42));
    assert_eq!(current.ratio()?, Some(1.5));
    assert_eq!(current.values().collect::<Result<Vec<_>>>()?, vec![-1, 2]);
    assert_eq!(current.tags().collect::<Result<Vec<_>>>()?, vec![1, 2]);
    Ok(())
}

#[test]
fn derive_supports_incremental_collection_mutations() -> Result<()> {
    let (_dir, store) = temp_store();
    IncrementalSchema::declare(&store)?;
    let model = store.create::<Incremental>()?;
    let id = model.entity;
    let mut tx = model.transact();
    tx.push_list("a")
        .prepend_list("start")
        .push_list("end")
        .insert_list_at(1, "middle")
        .remove_list_at(2)
        .move_list_at(0, 2)
        .rebalance_list()
        .add_tags("a")
        .add_tags("b")
        .remove_tags("a")
        .set_scores_seek("a", 1)
        .remove_scores_key("a");
    tx.commit()?;

    let current = store.db().entity::<Incremental>(id)?.expect("incremental");
    assert_eq!(
        current.list().collect::<Result<Vec<_>>>()?,
        vec!["middle".to_string(), "end".to_string(), "start".to_string(),]
    );
    assert_eq!(
        current.tags().collect::<Result<Vec<_>>>()?,
        vec!["b".to_string()]
    );
    assert!(current.scores().next().is_none());
    Ok(())
}

#[test]
fn derive_supports_optional_single_and_map_values() -> Result<()> {
    let (_dir, store) = temp_store();
    CarSchema::declare(&store)?;
    OptionalModelSchema::declare(&store)?;
    let car = store.create::<Car>()?;
    let car_id = car.entity;
    car.transact().set_name("Roadster").commit()?;
    let car_handle = store.db().entity::<Car>(car_id)?.expect("car handle");
    let model = store.create::<OptionalModel>()?;
    let model_id = model.entity;

    let mut tx = model.transact();
    tx.set_nickname("Al")
        .set_car(&car_handle)
        .set_friends(
            [
                ("a".to_string(), Some(&car_handle)),
                ("b".to_string(), Some(&car_handle)),
                ("a".to_string(), None),
            ]
            .into_iter(),
        )
        .set_scores(
            [
                ("a".to_string(), Some(1)),
                ("a".to_string(), None),
                ("b".to_string(), Some(2)),
            ]
            .into_iter(),
        )
        .set_by_ref([(&car_handle, Some("old")), (&car_handle, None)].into_iter());
    tx.commit()?;

    let current = store
        .db()
        .entity::<OptionalModel>(model_id)?
        .expect("model");
    assert_eq!(current.nickname()?, Some("Al".into()));
    assert_eq!(
        current.car()?.expect("car").name()?,
        Some("Roadster".into())
    );
    assert!(current.friends_seek("a")?.is_none());
    assert!(current.friends_seek("b")?.is_some());
    assert!(current.scores_seek("a")?.is_none());
    assert_eq!(current.scores_seek("b")?, Some(2));
    assert!(current.by_ref_seek(&car_handle)?.is_none());

    let mut tx = model.transact();
    tx.clear_nickname()
        .clear_car()
        .clear_friends_seek("b")
        .clear_scores_seek("b");
    tx.commit()?;
    let cleared = model.read();
    assert_eq!(cleared.nickname()?, None);
    assert!(cleared.car()?.is_none());
    assert!(cleared.friends_seek("b")?.is_none());
    assert!(cleared.scores_seek("b")?.is_none());

    let mut tx = model.transact();
    tx.set_friends(
        [
            ("a".to_string(), None),
            ("a".to_string(), Some(&car_handle)),
            ("b".to_string(), Some(&car_handle)),
        ]
        .into_iter(),
    )
    .set_by_ref([(&car_handle, None), (&car_handle, Some("new"))].into_iter());
    tx.commit()?;
    let replaced = store
        .db()
        .entity::<OptionalModel>(model_id)?
        .expect("replaced model");
    assert_eq!(
        replaced.friends_seek("a")?.expect("friend").name()?,
        Some("Roadster".into())
    );
    assert_eq!(
        replaced.friends_seek("b")?.expect("friend").name()?,
        Some("Roadster".into())
    );
    assert_eq!(replaced.by_ref_seek(&car_handle)?, Some("new".to_string()));
    Ok(())
}

#[test]
fn optional_map_custom_keys_do_not_require_eq_or_hash() -> Result<()> {
    let (_dir, store) = temp_store();
    let model = store.create::<CustomKeyOptionalMap>()?;
    model
        .transact()
        .set_entries(
            [
                (CustomMapKey(2), Some(20)),
                (CustomMapKey(1), Some(10)),
                (CustomMapKey(2), None),
                (CustomMapKey(1), Some(11)),
                (CustomMapKey(3), None),
                (CustomMapKey(3), Some(30)),
                (CustomMapKey(4), Some(40)),
                (CustomMapKey(4), None),
            ]
            .into_iter(),
        )
        .commit()?;
    let current = model.read();
    assert_eq!(current.entries_seek(CustomMapKey(1))?, Some(11));
    assert_eq!(current.entries_seek(CustomMapKey(2))?, None);
    assert_eq!(current.entries_seek(CustomMapKey(3))?, Some(30));
    assert_eq!(current.entries_seek(CustomMapKey(4))?, None);
    assert_eq!(
        current.entries().collect::<Result<Vec<_>>>()?,
        vec![(CustomMapKey(1), 11), (CustomMapKey(3), 30)]
    );
    Ok(())
}

#[test]
fn schema_evolution_is_additive_and_stable_across_field_renames() -> Result<()> {
    let (_dir, store) = temp_store();
    let v1 = EvolutionV1Schema::declare(&store)?;
    assert!(EvolutionV2Schema::get(&store.db())?.is_none());
    let v2 = EvolutionV2Schema::declare(&store)?;
    assert_eq!(v1.shared, v2.shared);
    assert_eq!(
        EvolutionV2Schema::get(&store.db())?.expect("v2").added,
        v2.added
    );
    let renamed = EvolutionRenamedSchema::declare(&store)?;
    assert_eq!(renamed.renamed, v1.shared);
    assert_eq!(
        EvolutionRenamedSchema::get(&store.db())?
            .expect("renamed")
            .renamed,
        v1.shared
    );
    Ok(())
}

#[test]
fn schema_get_rejects_full_definition_and_index_mismatches() -> Result<()> {
    let (_dir, store) = temp_store();
    EvolutionV2Schema::declare(&store)?;
    assert!(matches!(
        EvolutionIndexMismatchSchema::get(&store.db()),
        Err(Error::SchemaConflict(name)) if name == "evolution/added"
    ));

    let (_dir, store) = temp_store();
    TypeEvolutionV1Schema::declare(&store)?;
    assert!(matches!(
        TypeEvolutionV2Schema::get(&store.db()),
        Err(Error::SchemaConflict(name)) if name == "type_evolution/value"
    ));
    assert!(store.db().attribute_id("type_evolution/value")?.is_some());
    Ok(())
}

#[test]
fn declaration_preflight_prevents_partial_schema_writes() -> Result<()> {
    let (_dir, store) = temp_store();
    EvolutionV1Schema::declare(&store)?;
    assert!(matches!(
        EvolutionConflictSchema::declare(&store),
        Err(Error::SchemaConflict(name)) if name == "evolution/shared"
    ));
    assert!(store.db().attribute_id("evolution/fresh")?.is_none());
    assert!(EvolutionV1Schema::get(&store.db())?.is_some());
    Ok(())
}

#[test]
fn derived_multi_field_schema_survives_reopen() -> Result<()> {
    let (dir, store) = temp_store();
    let declared = EvolutionV2Schema::declare(&store)?;
    drop(store);

    let reopened = Arc::new(Store::open(dir.path())?);
    let loaded = EvolutionV2Schema::get(&reopened.db())?.expect("evolution schema");
    assert_eq!(loaded.shared, declared.shared);
    assert_eq!(loaded.added, declared.added);
    Ok(())
}

#[test]
fn derived_schema_respects_declaration_cutoff() -> Result<()> {
    let (_dir, store) = temp_store();
    let before = store.db().txn();
    let declared = EvolutionV2Schema::declare(&store)?;
    let declaration = store.db().txn();
    let latest = store.db();

    assert!(EvolutionV2Schema::get(&latest.at(before))?.is_none());
    let historical = EvolutionV2Schema::get(&latest.at(declaration))?.expect("declared schema");
    assert_eq!(historical.shared, declared.shared);
    assert_eq!(historical.added, declared.added);
    Ok(())
}

#[test]
fn generated_code_supports_shadowed_standard_names() -> Result<()> {
    use shadowing::{ModelEntity, ModelEntityMut};

    let (_dir, store) = temp_store();
    CarSchema::declare(&store)?;
    shadowing::ModelSchema::declare(&store)?;
    let car = store.create::<Car>()?;
    car.transact().set_name("car").commit()?;
    let model = store.create::<shadowing::Model>()?;
    model
        .transact()
        .set_name("shadowed")
        .set_values(["a", "b"].into_iter())
        .set_values_at(1, "c")
        .set_car(car.entity)
        .commit()?;
    let current = model.read();
    assert_eq!(current.name()?, Some("shadowed".to_string()));
    assert_eq!(current.values_at(1)?, Some("c".to_string()));
    assert_eq!(current.car()?.expect("car").entity, car.entity);
    Ok(())
}

#[test]
fn generated_code_supports_raw_identifiers_and_visibility() -> Result<()> {
    use raw_visibility::{ModelEntity, ModelEntityMut};

    let (_dir, store) = temp_store();
    raw_visibility::ModelSchema::declare(&store)?;
    let model = store.create::<raw_visibility::Model>()?;
    model
        .transact()
        .set_type("raw")
        .set_match(["a"].into_iter())
        .commit()?;
    assert_eq!(model.read().r#type()?, Some("raw".to_string()));
    assert_eq!(
        model.read().r#match().collect::<Result<Vec<_>>>()?,
        vec!["a".to_string()]
    );
    Ok(())
}

#[test]
fn standard_paths_and_custom_module_names_are_distinguished() -> Result<()> {
    let (_dir, store) = temp_store();
    CarSchema::declare(&store)?;
    StandardPathsSchema::declare(&store)?;
    custom_types::StringSchema::declare(&store)?;
    custom_types::TimestampSchema::declare(&store)?;
    let schema = CustomNamedTypesSchema::declare(&store)?;
    assert_eq!(store.attribute_def(schema.string)?.value_type, Type::Ref);
    assert_eq!(store.attribute_def(schema.timestamp)?.value_type, Type::Ref);
    Ok(())
}

#[test]
fn custom_values_and_explicit_references_generate_working_apis() -> Result<()> {
    use CustomValuesQuery as _;

    let (_dir, store) = temp_store();
    ExplicitTargetSchema::declare(&store)?;
    let schema = CustomValuesSchema::declare(&store)?;
    assert_eq!(store.attribute_def(schema.alias)?.value_type, Type::Long);
    assert!(store.attribute_def(schema.alias)?.indexed);
    assert_eq!(store.attribute_def(schema.score)?.kind, CollectionKind::One);
    assert_eq!(
        store.attribute_def(schema.scores)?.kind,
        CollectionKind::List
    );
    let target = store.create::<ExplicitTarget>()?;
    target.transact().set_name("target").commit()?;
    let model = store.create::<CustomValues>()?;
    model
        .transact()
        .set_alias(7)
        .set_score(Score(11))
        .set_scores([Score(1), Score(2)].into_iter())
        .set_optional_score(Score(3))
        .set_generic_score(GenericScore(17))
        .set_explicit(target.entity)
        .commit()?;
    let current = model.read();
    assert_eq!(current.alias()?, Some(7));
    assert_eq!(current.score()?, Some(Score(11)));
    assert_eq!(
        current.scores().collect::<Result<Vec<_>>>()?,
        vec![Score(1), Score(2)]
    );
    assert_eq!(current.optional_score()?, Some(Score(3)));
    assert_eq!(current.generic_score()?, Some(GenericScore(17)));
    assert_eq!(current.explicit()?.expect("target").entity, target.entity);
    let found = store
        .db()
        .query::<CustomValues>()?
        .expect("query")
        .find_by_alias(7)?;
    assert_eq!(found.len(), 1);
    Ok(())
}

#[test]
fn full_attribute_definition_comparison_does_not_require_public_fields() -> Result<()> {
    let (_dir, store) = temp_store();
    let id = store.declare_attribute(AttributeDef {
        name: "type_evolution/value".to_string(),
        value_type: Type::String,
        kind: CollectionKind::One,
        key_type: None,
        indexed: false,
        is_component: false,
    })?;
    assert_eq!(
        TypeEvolutionV1Schema::get(&store.db())?
            .expect("schema")
            .value,
        id
    );
    Ok(())
}

#[test]
fn raw_entity_id_component_fields_are_supported() -> Result<()> {
    let (_dir, store) = temp_store();
    let target = store.create::<RawComponent>()?;
    let target_id = target.entity;
    let child = store.create::<RawComponent>()?;
    let mut tx = child.transact();
    tx.set_parent(target_id);
    tx.commit()?;
    assert_eq!(child.read().parent()?, Some(target_id));
    Ok(())
}
