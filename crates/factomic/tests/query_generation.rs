#![cfg(feature = "derive")]
#![allow(dead_code)]

use std::{collections::HashMap, sync::Arc};

use factomic::{DbHandle, EntityId, Result, Store};
use tempfile::TempDir;

#[derive(factomic::Entity)]
struct Company {
    name: String,
}

#[derive(factomic::Entity)]
struct Employee {
    #[factomic(indexed)]
    name: String,
    #[factomic(indexed)]
    age: i64,
    #[factomic(indexed)]
    company: Company,
    bosses: Vec<Company>,
    previous: std::collections::HashSet<Company>,
    friends: HashMap<String, Company>,
    #[factomic(indexed)]
    manager: Option<Company>,
    #[factomic(indexed)]
    scores: HashMap<String, i64>,
}

fn temp_store() -> (TempDir, Arc<Store>) {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    (dir, store)
}

fn ids(handles: Vec<DbHandle<Employee>>) -> Vec<EntityId> {
    let mut ids: Vec<_> = handles.into_iter().map(|handle| handle.entity).collect();
    ids.sort();
    ids
}

#[test]
fn generated_finders_cover_indexes_references_dedup_and_history() -> Result<()> {
    let (_dir, store) = temp_store();
    assert!(store.db().query::<Employee>()?.is_none());
    let company = store.create::<Company>()?;
    let company_id = company.entity;
    let mut tx = company.transact();
    tx.set_name("Acme");
    tx.commit()?;
    let company_handle = store.db().entity::<Company>(company_id)?.expect("company");
    let other = store.create::<Company>()?;
    let other_id = other.entity;
    let mut tx = other.transact();
    tx.set_name("Other");
    tx.commit()?;
    let other_handle = store.db().entity::<Company>(other_id)?.expect("other");

    let first = store.create::<Employee>()?;
    let first_id = first.entity;
    let second = store.create::<Employee>()?;
    let second_id = second.entity;
    let mut setup_txn = None;
    for employee in [&first, &second] {
        let mut tx = employee.transact();
        let name = if employee.entity == first_id {
            "Alice"
        } else {
            "Bob"
        };
        tx.set_name(name)
            .set_age(30)
            .set_company(&company_handle)
            .set_bosses([company_id, company_id].into_iter())
            .set_previous([company_id, company_id].into_iter())
            .set_friends(
                [
                    ("a".to_string(), &company_handle),
                    ("b".to_string(), &company_handle),
                ]
                .into_iter(),
            )
            .set_manager(company_id)
            .set_scores([("x".to_string(), 7)].into_iter());
        let txn = tx.commit()?.expect("transaction");
        setup_txn = Some(txn);
    }
    let setup_txn = setup_txn.expect("first transaction");

    let query = store.db().query::<Employee>()?.expect("query");
    assert_eq!(ids(query.find_by_age(30)?), vec![first_id, second_id]);
    assert_eq!(ids(query.find_by_name("Alice")?), vec![first_id]);
    assert_eq!(
        ids(query.find_by_company(&company_handle)?),
        vec![first_id, second_id]
    );
    assert_eq!(
        ids(query.find_referrers_by_company(&company_handle)?),
        vec![first_id, second_id]
    );
    assert_eq!(
        ids(query.find_referrers_by_bosses(&company_handle)?),
        vec![first_id, second_id]
    );
    assert_eq!(
        ids(query.find_referrers_by_previous(&company_handle)?),
        vec![first_id, second_id]
    );
    assert_eq!(
        ids(query.find_referrers_by_friends(&company_handle)?),
        vec![first_id, second_id]
    );
    assert_eq!(
        ids(query.find_referrers_by_manager(&company_handle)?),
        vec![first_id, second_id]
    );
    assert_eq!(ids(query.find_by_scores(7)?), vec![first_id, second_id]);
    assert!(query.find_by_scores(99)?.is_empty());
    let schema_a = query.find_by_age(30)?.remove(0).schema;
    let schema_b = query.find_by_age(30)?.remove(0).schema;
    assert!(Arc::ptr_eq(&schema_a, &schema_b));

    let mut tx = first.transact();
    tx.set_age(31).set_company(&other_handle);
    tx.commit()?;
    let historical = store
        .db()
        .at(setup_txn)
        .query::<Employee>()?
        .expect("historical query");
    assert_eq!(ids(historical.find_by_age(30)?), vec![first_id, second_id]);
    assert_eq!(
        ids(historical.find_by_company(&company_handle)?),
        vec![first_id, second_id]
    );
    assert_eq!(historical.find_by_age(30)?.remove(0).age()?, Some(30));
    assert_eq!(
        historical
            .find_by_age(30)?
            .remove(0)
            .company()?
            .expect("company")
            .entity,
        company_id
    );
    let current_query = store.db().query::<Employee>()?.expect("current query");
    assert_eq!(ids(current_query.find_by_age(31)?), vec![first_id]);
    assert_eq!(
        ids(current_query.find_by_company(&other_handle)?),
        vec![first_id]
    );
    Ok(())
}
