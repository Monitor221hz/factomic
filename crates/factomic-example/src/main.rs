use std::{collections::HashMap, sync::Arc};

use factomic::{DbHandle, DbHandleMut, EntityId, Result, Store};
use tempfile::TempDir;

#[derive(factomic::Entity)]
struct Car {
    name: String,
}

#[derive(factomic::Entity)]
struct Person {
    name: String,
    #[factomic(indexed)]
    age: i64,
    nickname: Option<String>,
    car: Car,
    manager: Option<Arc<Person>>,
    children: Vec<Arc<Person>>,
    friends: HashMap<String, Arc<Person>>,
    #[factomic(indexed)]
    active: bool,
}

fn car(store: &Arc<Store>, name: &str) -> Result<DbHandle<Car>> {
    let car = store.create::<Car>()?;
    let mut tx = car.transact();
    tx.set_name(name);
    tx.commit()?;
    Ok(car.read())
}

fn person(store: &Arc<Store>, name: &str) -> Result<DbHandleMut<Person>> {
    let person = store.create::<Person>()?;
    let mut tx = person.transact();
    tx.set_name(name);
    tx.commit()?;
    Ok(person)
}

fn ids<T: factomic::Entity>(handles: Vec<DbHandle<T>>) -> Vec<EntityId> {
    handles.into_iter().map(|handle| handle.entity).collect()
}

fn main() -> Result<()> {
    let dir = TempDir::new().expect("temporary directory");
    let store = Arc::new(Store::open(dir.path())?);
    let car = car(&store, "Roadster")?;
    let car_id = car.entity;
    let manager = person(&store, "Manager")?;
    let child = person(&store, "Child")?;
    let person = store.create::<Person>()?;
    let person_id = person.entity;
    let manager_id = manager.entity;
    let child_id = child.entity;

    let mut tx = person.transact();
    tx.set_name("Alice")
        .set_age(30)
        .set_nickname("Ally")
        .set_car(&car)
        .set_manager(&manager)
        .set_children([&child].into_iter())
        .set_friends([("best".to_string(), &child)].into_iter())
        .set_active(true);
    let initial_txn = tx.commit()?.expect("initial commit");

    let current = person.read();
    println!("name: {:?}", current.name()?);
    println!("age: {:?}", current.age()?);
    println!("nickname: {:?}", current.nickname()?);
    println!("car: {:?}", current.car()?.expect("car").name()?);
    println!("manager: {:?}", current.manager()?.expect("manager").entity);
    println!(
        "children: {:?}",
        current
            .children()
            .map(|child| child.map(|child| child.entity))
            .collect::<Result<Vec<_>>>()?
    );
    println!(
        "friends: {:?}",
        current
            .friends()
            .map(|entry| entry.map(|(name, friend)| (name, friend.entity)))
            .collect::<Result<Vec<_>>>()?
    );
    assert_eq!(current.name()?, Some("Alice".into()));
    assert_eq!(current.age()?, Some(30));
    assert_eq!(current.nickname()?, Some("Ally".into()));
    assert_eq!(
        current.car()?.expect("car").name()?,
        Some("Roadster".into())
    );
    assert_eq!(current.car()?.expect("car").entity, car_id);
    assert_eq!(current.manager()?.expect("manager").entity, manager_id);
    assert_eq!(
        current
            .children()
            .next()
            .transpose()?
            .expect("child")
            .entity,
        child_id
    );
    assert_eq!(current.children().count(), 1);
    assert_eq!(current.friends().count(), 1);
    assert_eq!(current.active()?, Some(true));

    let query = store.db().query::<Person>()?.expect("person schema");
    assert_eq!(ids(query.find_by_age(30)?), vec![person_id]);
    assert_eq!(
        ids(query.find_referrers_by_manager(&manager)?),
        vec![person_id]
    );
    assert_eq!(ids(query.find_referrers_by_car(&car)?), vec![person_id]);
    assert_eq!(ids(query.find_by_active(true)?), vec![person_id]);

    let mut tx = person.transact();
    tx.clear_nickname().set_age(31);
    tx.commit()?;
    let updated = person.read();
    assert_eq!(updated.age()?, Some(31));
    assert_eq!(updated.nickname()?, None);

    let historical = person.read_at(initial_txn)?.expect("historical person");
    assert_eq!(historical.age()?, Some(30));
    assert_eq!(historical.nickname()?, Some("Ally".into()));
    let historical_query = store
        .db()
        .at(initial_txn)
        .query::<Person>()?
        .expect("historical query");
    assert_eq!(ids(historical_query.find_by_age(30)?), vec![person_id]);

    person.retract()?;
    assert!(!person.exists()?);
    assert!(store.db().entity::<Person>(person_id)?.is_none());
    assert!(historical.exists()?);

    drop(current);
    drop(updated);
    drop(historical);
    drop(query);
    drop(historical_query);
    drop(person);
    drop(manager);
    drop(child);
    drop(car);
    drop(store);
    let reopened = Arc::new(Store::open(dir.path())?);
    assert!(reopened.db().entity::<Person>(person_id)?.is_none());
    assert!(!reopened.db().entity_exists(person_id)?);
    println!("example completed");
    Ok(())
}
