use super::*;
use crate::id::{TX_KIND_ATTR, TX_MOVE_ATTR};
use crate::value::Value;
use itertools::Itertools;

use tempfile::TempDir;

fn temp_store() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (dir, store)
}

fn values(store: &Store, entity: EntityId) -> Vec<Value> {
    store
        .db()
        .entity_factoms(entity)
        .collect::<Result<Vec<_>>>()
        .unwrap()
        .iter()
        .map(|f| f.value().clone())
        .collect()
}

fn long_map(store: &Store, entity: EntityId, attr: AttributeId) -> Vec<(String, i64)> {
    store
        .db()
        .entity_keyvalues::<String, i64>(entity, attr)
        .collect::<Result<Vec<_>>>()
        .unwrap()
}

fn def(name: &str, value_type: Type, kind: CollectionKind) -> AttributeDef {
    AttributeDef {
        name: name.into(),
        value_type,
        kind,
        key_type: None,
        indexed: false,
        is_component: false,
    }
}

fn declare_list(store: &Store) -> AttributeId {
    store
        .declare_attribute(def("person/items", Type::String, CollectionKind::List))
        .unwrap()
}

fn declare_many(store: &Store) -> AttributeId {
    store
        .declare_attribute(def("person/tags", Type::String, CollectionKind::Many))
        .unwrap()
}

fn declare_map(store: &Store) -> AttributeId {
    store
        .declare_attribute(AttributeDef {
            key_type: Some(Type::String),
            ..def("person/scores", Type::Long, CollectionKind::Map)
        })
        .unwrap()
}

fn declare_component(store: &Store) -> AttributeId {
    store
        .declare_attribute(AttributeDef {
            is_component: true,
            ..def("person/child", Type::Ref, CollectionKind::One)
        })
        .unwrap()
}

#[test]
fn factom_key_round_trip() {
    let slots = vec![
        Slot::Empty,
        Slot::position(b"a0".to_vec()).unwrap(),
        Slot::Key(MapKey::Long(-1)),
        Slot::Key(MapKey::String("a\0b".into())),
        Slot::Key(MapKey::Ref(EntityId::with_value(1))),
    ];
    let region = vec![4, 0, 255];

    for slot in slots {
        for txn in [None, Some(TxnId::with_value(42))] {
            let key = sv_key(&slot, &region);
            let decoded = decode_sv_key(&key, txn).unwrap();
            assert_eq!(decoded.slot, slot);
            assert_eq!(decoded.region, region);
            assert_eq!(decoded.txn, txn);
        }
    }
}

#[test]
fn structural_order_matches_encoded_order() {
    let slots = vec![
        Slot::Empty,
        Slot::position(b"a".to_vec()).unwrap(),
        Slot::position(b"a0".to_vec()).unwrap(),
        Slot::position(b"b".to_vec()).unwrap(),
        Slot::Key(MapKey::Long(i64::MIN)),
        Slot::Key(MapKey::Long(-1)),
        Slot::Key(MapKey::Long(0)),
        Slot::Key(MapKey::Long(i64::MAX)),
        Slot::Key(MapKey::String(String::new())),
        Slot::Key(MapKey::String("a".into())),
        Slot::Key(MapKey::String("aa".into())),
        Slot::Key(MapKey::String("a\0".into())),
        Slot::Key(MapKey::String("a\0b".into())),
        Slot::Key(MapKey::String("b".into())),
        Slot::Key(MapKey::Ref(EntityId::with_value(1))),
        Slot::Key(MapKey::Ref(EntityId::with_value(2))),
    ];
    let regions: [&[u8]; 4] = [&[], &[0], &[1], &[0, 255]];

    for left_slot in &slots {
        for right_slot in &slots {
            for left_region in regions {
                for right_region in regions {
                    assert_eq!(
                        compare_factom_order(left_slot, left_region, right_slot, right_region,),
                        sv_key(left_slot, left_region).cmp(&sv_key(right_slot, right_region)),
                        "mismatch for ({left_slot:?}, {left_region:?}) and ({right_slot:?}, {right_region:?})",
                    );
                }
            }
        }
    }
}

#[test]
fn revert_to_restores_values_and_schema() {
    let (dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let entity = store.new_entity().unwrap();
    let first = store.assert(entity, name, &"alice").unwrap().unwrap();
    let second = store.assert(entity, name, &"bob").unwrap().unwrap();
    let age = store
        .declare_attribute(def("person/age", Type::Long, CollectionKind::One))
        .unwrap();
    let fourth = store.assert(entity, age, &30_i64).unwrap().unwrap();

    let restored = store.revert_to(second).unwrap().unwrap();
    assert!(restored > fourth);
    let db = store.db();
    assert_eq!(
        db.entity_value::<String>(entity, name).unwrap(),
        Some("bob".into())
    );
    assert_eq!(db.entity_value::<i64>(entity, age).unwrap(), None);
    assert_eq!(db.attribute_id("person/age").unwrap(), None);
    assert_eq!(
        db.at(fourth).entity_value::<i64>(entity, age).unwrap(),
        Some(30)
    );
    assert_eq!(
        db.at(second).entity_value::<String>(entity, name).unwrap(),
        Some("bob".into())
    );
    assert_eq!(
        db.at(first).entity_value::<String>(entity, name).unwrap(),
        Some("alice".into())
    );

    drop(db);
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    assert_eq!(
        reopened.db().entity_value::<String>(entity, name).unwrap(),
        Some("bob".into())
    );
    assert_eq!(reopened.db().attribute_id("person/age").unwrap(), None);
    reopened.check_invariants().unwrap();
}

#[test]
fn revert_to_latest_is_noop() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let entity = store.new_entity().unwrap();
    let txn = store.assert(entity, name, &"alice").unwrap().unwrap();

    assert_eq!(store.revert_to(txn).unwrap(), None);
    assert_eq!(store.db().txn(), txn);
    assert_eq!(
        store.db().entity_value::<String>(entity, name).unwrap(),
        Some("alice".into())
    );
}

#[test]
fn revert_to_declaration_is_noop() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let declaration = store.db().txn();

    assert_eq!(store.revert_to(declaration).unwrap(), None);
    assert_eq!(store.db().txn(), declaration);
    assert_eq!(store.db().attribute_id("person/name").unwrap(), Some(name));
}

#[test]
fn revert_to_unknown_transaction_errors() {
    let (_dir, store) = temp_store();
    let result = store.revert_to(TxnId::with_value(999));
    assert!(matches!(result, Err(Error::UnknownTransaction(_))));
    assert_eq!(store.db().txn(), TxnId::with_value(0));
}

#[test]
fn sorted_factom_searches_work() {
    let mut factoms = vec![
        FactomState::pending(Slot::Empty, vec![1]),
        FactomState::pending(Slot::position(b"a0".to_vec()).unwrap(), vec![1]),
        FactomState::pending(Slot::Key(MapKey::Long(1)), vec![1]),
        FactomState::pending(Slot::Key(MapKey::String("a".into())), vec![1]),
    ];
    factoms.sort_by(|left, right| {
        compare_factom_order(&left.slot, &left.region, &right.slot, &right.region)
    });

    for (index, factom) in factoms.iter().enumerate() {
        assert!(contains_sorted_factom(
            &factoms,
            &factom.slot,
            &factom.region
        ));
        assert!(!contains_sorted_factom(&factoms, &factom.slot, &[2]));
        assert_eq!(find_slot_index(&factoms, &factom.slot), Ok(index));
    }
    assert!(find_slot_index(&factoms, &Slot::Key(MapKey::Long(2))).is_err());
}

#[test]
fn push_appends_in_order_across_txns() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    store.push(e, items, &"b").unwrap().unwrap();
    store.push(e, items, &"c").unwrap().unwrap();
    assert_eq!(
        values(&store, e),
        vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into())
        ]
    );
}

#[test]
fn push_and_prepend_share_one_txn() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    let txn = store
        .transact()
        .push(e, items, &"x")
        .push(e, items, &"y")
        .prepend(e, items, &"z")
        .commit()
        .unwrap()
        .unwrap();
    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 3);
    assert!(factoms.iter().all(|f| f.txn() == txn));
    assert_eq!(
        values(&store, e),
        vec![
            Value::String("z".into()),
            Value::String("x".into()),
            Value::String("y".into())
        ]
    );
}

#[test]
fn remove_deletes_a_slot_without_knowing_its_value() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    store.push(e, items, &"b").unwrap().unwrap();
    store.push(e, items, &"c").unwrap().unwrap();
    let middle = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap()[1]
        .slot()
        .clone();
    store.remove(e, items, middle).unwrap().unwrap();
    assert_eq!(
        values(&store, e),
        vec![Value::String("a".into()), Value::String("c".into())]
    );
}

#[test]
fn push_after_remove_still_sorts_last() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    store.push(e, items, &"b").unwrap().unwrap();
    let middle = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap()[1]
        .slot()
        .clone();
    store.remove(e, items, middle).unwrap().unwrap();
    store.push(e, items, &"c").unwrap().unwrap();
    assert_eq!(
        values(&store, e),
        vec![Value::String("a".into()), Value::String("c".into())]
    );
}

#[test]
fn put_overwrites_and_remove_key_deletes() {
    let (_dir, store) = temp_store();
    let scores = declare_map(&store);
    let e = store.new_entity().unwrap();
    store
        .put(e, scores, MapKey::String("b".into()), &2i64)
        .unwrap()
        .unwrap();
    store
        .put(e, scores, MapKey::String("a".into()), &1i64)
        .unwrap()
        .unwrap();
    store
        .put(e, scores, MapKey::String("b".into()), &3i64)
        .unwrap()
        .unwrap();
    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 2);
    assert_eq!(factoms[0].slot(), &Slot::Key(MapKey::String("a".into())));
    assert_eq!(factoms[0].value(), &Value::Long(1));
    assert_eq!(factoms[1].slot(), &Slot::Key(MapKey::String("b".into())));
    assert_eq!(factoms[1].value(), &Value::Long(3));
    store
        .remove_key(e, scores, MapKey::String("a".into()))
        .unwrap()
        .unwrap();
    assert_eq!(values(&store, e), vec![Value::Long(3)]);
}

#[test]
fn same_txn_map_changes_merge_in_key_order() {
    let (_dir, store) = temp_store();
    let scores = declare_map(&store);
    let e = store.new_entity().unwrap();
    for (key, value) in [("b", 2i64), ("d", 4), ("f", 6)] {
        store
            .put(e, scores, MapKey::String(key.into()), &value)
            .unwrap()
            .unwrap();
    }

    store
        .transact()
        .put(e, scores, MapKey::String("e".into()), &5i64)
        .put(e, scores, MapKey::String("a".into()), &1i64)
        .put(e, scores, MapKey::String("d".into()), &40i64)
        .put(e, scores, MapKey::String("c".into()), &3i64)
        .commit()
        .unwrap()
        .unwrap();

    assert_eq!(
        values(&store, e),
        vec![
            Value::Long(1),
            Value::Long(2),
            Value::Long(3),
            Value::Long(40),
            Value::Long(5),
            Value::Long(6),
        ]
    );
}

#[test]
fn intents_reject_wrong_kinds() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let scores = declare_map(&store);
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    let push_on_map = store.push(e, scores, &1i64);
    assert!(matches!(push_on_map, Err(Error::MismatchedKey(_))));
    let put_on_list = store.put(e, items, MapKey::String("k".into()), &"x");
    assert!(matches!(put_on_list, Err(Error::MismatchedKey(_))));
    let remove_on_one = store.remove(e, name, Slot::Empty);
    assert!(matches!(remove_on_one, Err(Error::MismatchedKey(_))));
    let clear_on_map = store.transact().clear(e, scores).commit();
    assert!(matches!(clear_on_map, Err(Error::MismatchedKey(_))));
}

#[test]
fn mixed_prepend_and_push_bound_against_db_and_pending() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"b").unwrap().unwrap();
    store.push(e, items, &"c").unwrap().unwrap();
    store
        .transact()
        .prepend(e, items, &"a")
        .push(e, items, &"d")
        .prepend(e, items, &"z")
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(
        values(&store, e),
        vec![
            Value::String("z".into()),
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
            Value::String("d".into())
        ]
    );
}

#[test]
fn slot_at_addresses_list_by_ordinal() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    store.push(e, items, &"b").unwrap().unwrap();
    store.push(e, items, &"c").unwrap().unwrap();
    let second = store.db().slot_at(e, items, 1).unwrap().unwrap();
    store.insert_after(e, items, second, &"x").unwrap().unwrap();
    assert_eq!(
        values(&store, e),
        vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("x".into()),
            Value::String("c".into())
        ]
    );
    assert_eq!(store.db().slot_at(e, items, 4).unwrap(), None);
    assert_eq!(store.db().slot_at(e, items, 99).unwrap(), None);
}

#[test]
fn revert_push_removes_the_item() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    let txn = store.push(e, items, &"a").unwrap().unwrap();
    store.revert_txn(txn).unwrap();
    assert!(
        store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn revert_remove_restores_item_with_original_slot_and_value() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    store.push(e, items, &"b").unwrap().unwrap();
    let slot = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap()[0]
        .slot()
        .clone();
    let removed = store.remove(e, items, slot.clone()).unwrap().unwrap();
    store.revert_txn(removed).unwrap();
    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 2);
    assert_eq!(factoms[0].slot(), &slot);
    assert_eq!(factoms[0].value(), &Value::String("a".into()));
}

#[test]
fn insert_before_anchors_first_slot() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"b").unwrap().unwrap();
    let anchor = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap()[0]
        .slot()
        .clone();
    store
        .insert_before(e, items, anchor, &"a")
        .unwrap()
        .unwrap();
    assert_eq!(
        values(&store, e),
        vec![Value::String("a".into()), Value::String("b".into())]
    );
}

#[test]
fn insert_at_missing_anchor_errors() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    let ghost = Slot::position(b"zz".to_vec()).unwrap();
    let result = store.insert_after(e, items, ghost, &"b");
    assert!(matches!(result, Err(Error::SlotNotFound(_))));
}

#[test]
fn move_slot_preserves_value_and_records_annotation() {
    let (_dir, store) = temp_store();
    let items = store
        .declare_attribute(AttributeDef {
            value_type: Type::Ref,
            ..def("person/items", Type::Ref, CollectionKind::List)
        })
        .unwrap();
    let e = store.new_entity().unwrap();
    let target = store.new_entity().unwrap();
    store.push(e, items, &target).unwrap().unwrap();
    let from = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap()[0]
        .slot()
        .clone();
    let to = Slot::position(b"zz".to_vec()).unwrap();
    let txn = store
        .move_slot(e, items, from.clone(), to.clone())
        .unwrap()
        .unwrap();
    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 1);
    assert_eq!(factoms[0].slot(), &to);
    assert_eq!(factoms[0].value(), &Value::Ref(target));
    let mut expected = attr_bytes(items);
    from.encode_key(&mut expected);
    to.encode_key(&mut expected);
    let tx_factoms = store
        .db()
        .entity_factoms(txn.entity_id())
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert!(
        tx_factoms
            .iter()
            .any(|f| f.attribute() == TX_MOVE_ATTR && f.value() == &Value::Bytes(expected.clone()))
    );
}

fn attr_bytes(attr: AttributeId) -> Vec<u8> {
    attr.to_be_bytes().to_vec()
}

#[test]
fn move_slot_missing_source_errors() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    let ghost = Slot::position(b"zz".to_vec()).unwrap();
    let dest = Slot::position(b"zy".to_vec()).unwrap();
    let result = store.move_slot(e, items, ghost, dest);
    assert!(matches!(result, Err(Error::SlotNotFound(_))));
}

#[test]
fn rebalance_preserves_values_and_tags_tx_kind() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    store.push(e, items, &"c").unwrap().unwrap();
    let anchor = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap()[1]
        .slot()
        .clone();
    store
        .insert_before(e, items, anchor.clone(), &"b")
        .unwrap()
        .unwrap();
    let fractional = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap()[1]
        .slot()
        .clone();
    assert_ne!(fractional, anchor);

    let txn = store
        .transact()
        .push(e, items, &"d")
        .rebalance(e, items)
        .commit()
        .unwrap()
        .unwrap();
    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 4);
    assert_eq!(
        factoms
            .iter()
            .map(|f| f.value().clone())
            .collect::<Vec<_>>(),
        vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
            Value::String("d".into())
        ]
    );
    let mut index = FractionalIndex::default();
    for factom in &factoms {
        assert_eq!(factom.slot(), &Slot::with_index(&index).unwrap());
        index = FractionalIndex::new_after(&index);
    }
    assert_ne!(factoms[1].slot(), &fractional);
    let tx_factoms = store
        .db()
        .entity_factoms(txn.entity_id())
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert!(
        tx_factoms
            .iter()
            .any(|f| f.attribute() == TX_KIND_ATTR
                && f.value() == &Value::String("rebalance".into()))
    );
}

#[test]
fn canonical_rebalance_is_noop_and_does_not_tag_mixed_txn() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    store.push(e, items, &"a").unwrap().unwrap();
    store.push(e, items, &"b").unwrap().unwrap();
    let before = store
        .db()
        .entity_attr_factoms(e, items)
        .collect::<Result<Vec<_>>>()
        .unwrap();

    assert_eq!(store.rebalance(e, items).unwrap(), None);
    let txn = store
        .transact()
        .rebalance(e, items)
        .assert(e, name, &"alice")
        .commit()
        .unwrap()
        .unwrap();
    let log = store.db().tx_factoms(txn).flatten().collect_vec();
    assert!(log.iter().all(|factom| factom.attribute() != TX_KIND_ATTR));
    let after = store
        .db()
        .entity_attr_factoms(e, items)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn retract_entity_removes_everything_and_external_refs() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let friend = store
        .declare_attribute(def("person/friend", Type::Ref, CollectionKind::One))
        .unwrap();
    let e1 = store.new_entity().unwrap();
    let e2 = store.new_entity().unwrap();
    store.assert(e1, name, &"alice").unwrap().unwrap();
    store.assert(e2, name, &"bob").unwrap().unwrap();
    store.assert(e2, friend, &e1).unwrap().unwrap();
    store.retract_entity(e1).unwrap().unwrap();
    assert!(
        store
            .db()
            .entity_factoms(e1)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    let e2_factoms = store
        .db()
        .entity_factoms(e2)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(e2_factoms.len(), 1);
    assert_eq!(e2_factoms[0].attribute(), name);
}

#[test]
fn retract_entity_cascades_component_children() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let child = store
        .declare_attribute(AttributeDef {
            is_component: true,
            ..def("person/child", Type::Ref, CollectionKind::One)
        })
        .unwrap();
    let parent = store.new_entity().unwrap();
    let kid = store.new_entity().unwrap();
    store.assert(kid, name, &"junior").unwrap().unwrap();
    store.assert(parent, child, &kid).unwrap().unwrap();
    store.retract_entity(parent).unwrap().unwrap();
    assert!(
        store
            .db()
            .entity_factoms(parent)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .db()
            .entity_factoms(kid)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn retract_entity_survives_component_cycle() {
    let (_dir, store) = temp_store();
    let partner = store
        .declare_attribute(AttributeDef {
            is_component: true,
            ..def("person/partner", Type::Ref, CollectionKind::One)
        })
        .unwrap();
    let a = store.new_entity().unwrap();
    let b = store.new_entity().unwrap();
    store.assert(a, partner, &b).unwrap().unwrap();
    store.assert(b, partner, &a).unwrap().unwrap();
    store.retract_entity(a).unwrap().unwrap();
    assert!(
        store
            .db()
            .entity_factoms(a)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .db()
            .entity_factoms(b)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn component_conflict_rejected() {
    let (_dir, store) = temp_store();
    let child = store
        .declare_attribute(AttributeDef {
            is_component: true,
            ..def("person/child", Type::Ref, CollectionKind::One)
        })
        .unwrap();
    let o1 = store.new_entity().unwrap();
    let o2 = store.new_entity().unwrap();
    let kid = store.new_entity().unwrap();
    store.assert(o1, child, &kid).unwrap().unwrap();
    let result = store.assert(o2, child, &kid);
    assert!(matches!(result, Err(Error::ComponentConflict(_))));
}

#[test]
fn component_requires_ref_type() {
    let (_dir, store) = temp_store();
    let result = store.declare_attribute(AttributeDef {
        is_component: true,
        ..def("person/child", Type::String, CollectionKind::One)
    });
    assert!(matches!(result, Err(Error::InvalidSchema(_))));
}

#[test]
fn same_txn_one_attr_last_write_wins() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    store
        .transact()
        .assert(e, name, &"a")
        .assert(e, name, &"b")
        .commit()
        .unwrap()
        .unwrap();
    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 1);
    assert_eq!(factoms[0].value(), &Value::String("b".into()));
}

#[test]
fn same_txn_list_slot_last_write_wins() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    let k = Slot::position(b"a0".to_vec()).unwrap();
    store
        .transact()
        .assert_at(e, items, k.clone(), &"a")
        .assert_at(e, items, k.clone(), &"b")
        .commit()
        .unwrap()
        .unwrap();
    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 1);
    assert_eq!(factoms[0].slot(), &k);
    assert_eq!(factoms[0].value(), &Value::String("b".into()));
}

#[test]
fn same_txn_assert_then_retract_nets_to_nothing() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    let result = store
        .transact()
        .assert(e, name, &"a")
        .retract(e, name, &"a")
        .commit()
        .unwrap();
    assert_eq!(result, None);
    assert!(
        store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    assert!(store.db().entity_history(e).collect::<Vec<_>>().is_empty());
}

#[test]
fn same_txn_retract_then_assert_keeps_factom() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    let original = store.assert(e, name, &"a").unwrap().unwrap();
    let result = store
        .transact()
        .retract(e, name, &"a")
        .assert(e, name, &"a")
        .commit()
        .unwrap();
    assert_eq!(result, None);

    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 1);
    assert_eq!(factoms[0].value(), &Value::String("a".into()));
    assert_eq!(factoms[0].txn(), original);
    assert!(
        store
            .db()
            .entity_history(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    let historical = store
        .db_at(original)
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(historical.len(), 1);
    assert_eq!(historical[0].txn(), original);
}

#[test]
fn same_txn_restored_fact_can_be_retracted_again() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    let original = store.assert(e, name, &"a").unwrap().unwrap();
    let retracted = store
        .transact()
        .retract(e, name, &"a")
        .assert(e, name, &"a")
        .retract(e, name, &"a")
        .commit()
        .unwrap()
        .unwrap();

    assert!(
        store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    let history = store
        .db()
        .entity_history(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(history.len(), 2);
    assert!(
        history
            .iter()
            .any(|factom| factom.txn() == original && factom.op() == Op::Assert)
    );
    assert!(
        history
            .iter()
            .any(|factom| factom.txn() == retracted && factom.op() == Op::Retract)
    );
    assert_eq!(
        store
            .db_at(original)
            .entity_value::<String>(e, name)
            .unwrap(),
        Some("a".into())
    );
    assert_eq!(
        store
            .db_at(retracted)
            .entity_value::<String>(e, name)
            .unwrap(),
        None
    );
}

#[test]
fn same_txn_replacing_then_restoring_original_is_noop() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    let original = store.assert(e, name, &"a").unwrap().unwrap();
    let result = store
        .transact()
        .retract(e, name, &"a")
        .assert(e, name, &"b")
        .assert(e, name, &"a")
        .commit()
        .unwrap();
    assert_eq!(result, None);

    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 1);
    assert_eq!(factoms[0].value(), &Value::String("a".into()));
    assert_eq!(factoms[0].txn(), original);
    assert!(
        store
            .db()
            .entity_history(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn same_txn_restoration_only_logs_effective_attributes() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let nickname = store
        .declare_attribute(def("person/nickname", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    let original = store.assert(e, name, &"a").unwrap().unwrap();
    let txn = store
        .transact()
        .retract(e, name, &"a")
        .assert(e, name, &"a")
        .assert(e, nickname, &"ally")
        .commit()
        .unwrap()
        .unwrap();

    let log = store.db().tx_factoms(txn).flatten().collect_vec();
    assert!(log.iter().all(|factom| factom.attribute() != name));
    assert!(log.iter().any(|factom| factom.attribute() == nickname));
    let names = store
        .db()
        .entity_attr_factoms(e, name)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].txn(), original);
    assert!(
        store
            .db()
            .entity_history(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn same_txn_reference_move_round_trip_is_noop() {
    let (_dir, store) = temp_store();
    let links = store
        .declare_attribute(def("person/links", Type::Ref, CollectionKind::List))
        .unwrap();
    let source = store.new_entity().unwrap();
    let target = store.new_entity().unwrap();
    let original = store.push(source, links, &target).unwrap().unwrap();
    let from = store.db().slot_at(source, links, 0).unwrap().unwrap();
    let to = Slot::position(b"zz".to_vec()).unwrap();
    let result = store
        .transact()
        .move_slot(source, links, from.clone(), to.clone())
        .move_slot(source, links, to, from.clone())
        .commit()
        .unwrap();
    assert_eq!(result, None);

    let factoms = store
        .db()
        .entity_factoms(source)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 1);
    assert_eq!(factoms[0].slot(), &from);
    assert_eq!(factoms[0].txn(), original);
    assert_eq!(factoms[0].value(), &Value::Ref(target));
    assert_eq!(
        store.db().referring_entities(target, links).unwrap(),
        vec![source]
    );
    assert!(
        store
            .db()
            .entity_history(source)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn same_txn_remove_cancels_pending_insert() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    let k = Slot::position(b"a0".to_vec()).unwrap();
    let result = store
        .transact()
        .assert_at(e, items, k.clone(), &"a")
        .remove(e, items, k.clone())
        .commit()
        .unwrap();
    assert_eq!(result, None);
    assert!(
        store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn same_txn_insert_after_pending_anchor() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    let k = Slot::with_index(&FractionalIndex::default()).unwrap();
    store
        .transact()
        .assert_at(e, items, k.clone(), &"a")
        .insert_after(e, items, k.clone(), &"b")
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(
        values(&store, e),
        vec![Value::String("a".into()), Value::String("b".into())]
    );
}

#[test]
fn same_txn_move_of_pending_slot() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    let from = Slot::position(b"a0".to_vec()).unwrap();
    let to = Slot::position(b"zz".to_vec()).unwrap();
    store
        .transact()
        .assert_at(e, items, from.clone(), &"x")
        .move_slot(e, items, from.clone(), to.clone())
        .commit()
        .unwrap()
        .unwrap();
    let factoms = store
        .db()
        .entity_factoms(e)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 1);
    assert_eq!(factoms[0].slot(), &to);
    assert_eq!(factoms[0].value(), &Value::String("x".into()));
}

#[test]
fn replace_list_at_replaces_committed_and_pending_ordinals() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    for value in ["a", "b", "c"] {
        store.push(e, items, &value).unwrap().unwrap();
    }

    store
        .transact()
        .replace_list_at(e, items, 1, &"x")
        .push(e, items, &"d")
        .replace_list_at(e, items, 3, &"y")
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(
        values(&store, e),
        vec![
            Value::String("a".into()),
            Value::String("x".into()),
            Value::String("c".into()),
            Value::String("y".into()),
        ]
    );

    let result = store
        .transact()
        .replace_list_at(e, items, 99, &"z")
        .commit();
    assert!(matches!(result, Err(Error::SlotNotFound(_))));
}

#[test]
fn replace_map_rebuilds_smaller_empty_and_larger_maps() {
    let (_dir, store) = temp_store();
    let scores = declare_map(&store);
    let e = store.new_entity().unwrap();
    for (key, value) in [("a", 1), ("b", 2), ("c", 3)] {
        store.put(e, scores, key, &value).unwrap().unwrap();
    }

    store
        .transact()
        .replace_map(e, scores, [("x", 9)])
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(long_map(&store, e, scores), vec![("x".into(), 9)]);

    store
        .transact()
        .replace_map(e, scores, std::iter::empty::<(&str, i64)>())
        .commit()
        .unwrap()
        .unwrap();
    assert!(long_map(&store, e, scores).is_empty());

    store
        .transact()
        .replace_map(e, scores, [("y", 10), ("z", 11)])
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(
        long_map(&store, e, scores),
        vec![("y".into(), 10), ("z".into(), 11)]
    );
}

#[test]
fn replace_map_accounts_for_pending_operations_and_duplicate_keys() {
    let (_dir, store) = temp_store();
    let scores = declare_map(&store);
    let e = store.new_entity().unwrap();

    store
        .transact()
        .put(e, scores, "discarded".into(), &0)
        .replace_map(e, scores, [("a", 1), ("b", 2), ("a", 3)])
        .put(e, scores, "c".into(), &4)
        .commit()
        .unwrap()
        .unwrap();

    assert_eq!(
        long_map(&store, e, scores),
        vec![("a".into(), 3), ("b".into(), 2), ("c".into(), 4)]
    );
}

#[test]
fn replace_many_rebuilds_smaller_empty_and_larger_sets() {
    let (_dir, store) = temp_store();
    let tags = declare_many(&store);
    let e = store.new_entity().unwrap();
    for value in ["c", "a", "b"] {
        store.assert(e, tags, &value).unwrap().unwrap();
    }

    store
        .transact()
        .replace_many(e, tags, ["x"])
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(values(&store, e), vec![Value::String("x".into())]);

    store
        .transact()
        .replace_many(e, tags, std::iter::empty::<&str>())
        .commit()
        .unwrap()
        .unwrap();
    assert!(values(&store, e).is_empty());

    store
        .transact()
        .replace_many(e, tags, ["z", "y"])
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(
        values(&store, e),
        vec![Value::String("y".into()), Value::String("z".into())]
    );
}

#[test]
fn replace_many_accounts_for_surrounding_pending_operations_and_duplicate_values() {
    let (_dir, store) = temp_store();
    let tags = declare_many(&store);
    let e = store.new_entity().unwrap();

    store
        .transact()
        .assert(e, tags, &"discarded")
        .replace_many(e, tags, ["b", "a", "b"])
        .assert(e, tags, &"c")
        .commit()
        .unwrap()
        .unwrap();

    assert_eq!(
        values(&store, e),
        vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
        ]
    );
}

#[test]
fn clear_removes_committed_value_and_is_idempotent() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    store.assert(e, name, &"committed").unwrap().unwrap();

    store.transact().clear(e, name).commit().unwrap().unwrap();
    assert_eq!(store.db().entity_value::<String>(e, name).unwrap(), None);
    let history = store.db().entity_history(e).flatten().collect::<Vec<_>>();
    assert!(history.iter().any(|factom| {
        factom.op() == Op::Retract && factom.value() == &Value::String("committed".into())
    }));
    assert_eq!(store.transact().clear(e, name).commit().unwrap(), None);
}

#[test]
fn clear_handles_pending_assertions_and_followup_values() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();

    let result = store
        .transact()
        .assert(e, name, &"pending")
        .clear(e, name)
        .commit()
        .unwrap();
    assert_eq!(result, None);
    assert!(
        store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );

    store
        .transact()
        .clear(e, name)
        .assert(e, name, &"after")
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(
        store.db().entity_value::<String>(e, name).unwrap(),
        Some("after".into())
    );
}

#[test]
fn clear_retracts_committed_and_cancels_pending_overwrite() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    store.assert(e, name, &"committed").unwrap().unwrap();

    store
        .transact()
        .assert(e, name, &"pending")
        .clear(e, name)
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(store.db().entity_value::<String>(e, name).unwrap(), None);
    let history = store.db().entity_history(e).flatten().collect::<Vec<_>>();
    assert!(history.iter().any(|factom| {
        factom.op() == Op::Retract && factom.value() == &Value::String("committed".into())
    }));
    assert!(!history.iter().any(|factom| {
        factom.op() == Op::Assert && factom.value() == &Value::String("pending".into())
    }));
}

#[test]
fn incremental_list_operations_work_with_pending_changes() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    for value in ["a", "b", "d"] {
        store.push(e, items, &value).unwrap().unwrap();
    }

    store
        .transact()
        .insert_list_at(e, items, 2, &"c")
        .remove_list_at(e, items, 0)
        .insert_list_at(e, items, 3, &"e")
        .move_list_at(e, items, 0, 3)
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(
        values(&store, e),
        vec![
            Value::String("c".into()),
            Value::String("d".into()),
            Value::String("e".into()),
            Value::String("b".into()),
        ]
    );

    assert!(matches!(
        store.transact().insert_list_at(e, items, 99, &"x").commit(),
        Err(Error::SlotNotFound(_))
    ));
    assert!(matches!(
        store.transact().remove_list_at(e, items, 99).commit(),
        Err(Error::SlotNotFound(_))
    ));
    assert!(matches!(
        store.transact().move_list_at(e, items, 0, 99).commit(),
        Err(Error::SlotNotFound(_))
    ));

    let empty = store.new_entity().unwrap();
    store
        .transact()
        .insert_list_at(empty, items, 0, &"x")
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(values(&store, empty), vec![Value::String("x".into())]);
}

#[test]
fn replace_list_rebuilds_shorter_empty_and_longer_lists() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    for value in ["a", "b", "c"] {
        store.push(e, items, &value).unwrap().unwrap();
    }

    store
        .transact()
        .replace_list(e, items, ["x"])
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(values(&store, e), vec![Value::String("x".into())]);

    store
        .transact()
        .replace_list(e, items, std::iter::empty::<&str>())
        .commit()
        .unwrap()
        .unwrap();
    assert!(values(&store, e).is_empty());

    store
        .transact()
        .replace_list(e, items, ["y", "z"])
        .commit()
        .unwrap()
        .unwrap();
    assert_eq!(
        values(&store, e),
        vec![Value::String("y".into()), Value::String("z".into())]
    );
}

#[test]
fn replace_list_accounts_for_surrounding_pending_operations() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();

    store
        .transact()
        .push(e, items, &"discarded")
        .replace_list(e, items, ["a", "b"])
        .push(e, items, &"c")
        .commit()
        .unwrap()
        .unwrap();

    assert_eq!(
        values(&store, e),
        vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
        ]
    );
}

#[test]
fn same_txn_rebalance_preserves_duplicate_pending_values() {
    let (_dir, store) = temp_store();
    let items = declare_list(&store);
    let e = store.new_entity().unwrap();
    let index = FractionalIndex::default();
    let before = FractionalIndex::new_before(&index);
    let first = Slot::with_index(&before).unwrap();
    let second = Slot::with_index(&index).unwrap();

    store
        .transact()
        .assert_at(e, items, first, &"same")
        .assert_at(e, items, second, &"same")
        .rebalance(e, items)
        .commit()
        .unwrap()
        .unwrap();

    assert_eq!(
        values(&store, e),
        vec![Value::String("same".into()), Value::String("same".into())]
    );
}

#[test]
fn same_txn_ownership_transfer_avoids_false_conflict() {
    let (_dir, store) = temp_store();
    let child = store
        .declare_attribute(AttributeDef {
            is_component: true,
            ..def("person/child", Type::Ref, CollectionKind::One)
        })
        .unwrap();
    let o1 = store.new_entity().unwrap();
    let o2 = store.new_entity().unwrap();
    let kid = store.new_entity().unwrap();
    store.assert(o1, child, &kid).unwrap().unwrap();
    store
        .transact()
        .retract(o1, child, &kid)
        .assert(o2, child, &kid)
        .commit()
        .unwrap()
        .unwrap();
    let factoms = store
        .db()
        .entity_factoms(o2)
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(factoms.len(), 1);
    assert_eq!(factoms[0].value(), &Value::Ref(kid));
}

#[test]
fn same_txn_retract_entity_cancels_pending_and_committed() {
    let (_dir, store) = temp_store();
    let name = store
        .declare_attribute(def("person/name", Type::String, CollectionKind::One))
        .unwrap();
    let nick = store
        .declare_attribute(def("person/nick", Type::String, CollectionKind::One))
        .unwrap();
    let e = store.new_entity().unwrap();
    store.assert(e, name, &"alice").unwrap().unwrap();
    store
        .transact()
        .assert(e, nick, &"ally")
        .retract_entity(e)
        .commit()
        .unwrap()
        .unwrap();
    assert!(
        store
            .db()
            .entity_factoms(e)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn same_txn_retract_entity_cancels_pending_external_ref() {
    let (_dir, store) = temp_store();
    let friend = store
        .declare_attribute(def("person/friend", Type::Ref, CollectionKind::One))
        .unwrap();
    let source = store.new_entity().unwrap();
    let target = store.new_entity().unwrap();

    let result = store
        .transact()
        .assert(source, friend, &target)
        .retract_entity(target)
        .commit()
        .unwrap();

    assert_eq!(result, None);
    assert!(
        store
            .db()
            .entity_factoms(source)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn same_txn_pending_component_owner_conflicts() {
    let (_dir, store) = temp_store();
    let child = declare_component(&store);
    let o1 = store.new_entity().unwrap();
    let o2 = store.new_entity().unwrap();
    let kid = store.new_entity().unwrap();

    let result = store
        .transact()
        .assert(o1, child, &kid)
        .assert(o2, child, &kid)
        .commit();

    assert!(matches!(result, Err(Error::ComponentConflict(_))));
}

#[test]
fn same_txn_cancelled_pending_owner_does_not_conflict() {
    let (_dir, store) = temp_store();
    let child = declare_component(&store);
    let o1 = store.new_entity().unwrap();
    let o2 = store.new_entity().unwrap();
    let kid = store.new_entity().unwrap();

    store
        .transact()
        .assert(o1, child, &kid)
        .retract(o1, child, &kid)
        .assert(o2, child, &kid)
        .commit()
        .unwrap()
        .unwrap();

    assert!(
        store
            .db()
            .entity_factoms(o1)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    assert_eq!(values(&store, o2), vec![Value::Ref(kid)]);
}

#[test]
fn same_txn_overwritten_pending_owner_does_not_conflict() {
    let (_dir, store) = temp_store();
    let child = declare_component(&store);
    let o1 = store.new_entity().unwrap();
    let o2 = store.new_entity().unwrap();
    let kid = store.new_entity().unwrap();
    let other = store.new_entity().unwrap();

    store
        .transact()
        .assert(o1, child, &kid)
        .assert(o1, child, &other)
        .assert(o2, child, &kid)
        .commit()
        .unwrap()
        .unwrap();

    assert_eq!(values(&store, o1), vec![Value::Ref(other)]);
    assert_eq!(values(&store, o2), vec![Value::Ref(kid)]);
}

#[test]
fn same_txn_reasserted_component_owner_still_conflicts() {
    let (_dir, store) = temp_store();
    let child = declare_component(&store);
    let o1 = store.new_entity().unwrap();
    let o2 = store.new_entity().unwrap();
    let kid = store.new_entity().unwrap();
    store.assert(o1, child, &kid).unwrap().unwrap();

    let result = store
        .transact()
        .retract(o1, child, &kid)
        .assert(o1, child, &kid)
        .assert(o2, child, &kid)
        .commit();

    assert!(matches!(result, Err(Error::ComponentConflict(_))));
}

#[test]
fn same_txn_moved_pending_ref_is_retracted_with_target() {
    let (_dir, store) = temp_store();
    let items = store
        .declare_attribute(AttributeDef {
            is_component: true,
            ..def("person/items", Type::Ref, CollectionKind::List)
        })
        .unwrap();
    let source = store.new_entity().unwrap();
    let target = store.new_entity().unwrap();
    let from = Slot::position(b"a0".to_vec()).unwrap();
    let to = Slot::position(b"zz".to_vec()).unwrap();

    let result = store
        .transact()
        .assert_at(source, items, from.clone(), &target)
        .move_slot(source, items, from, to)
        .retract_entity(target)
        .commit()
        .unwrap();

    assert_eq!(result, None);
    assert!(
        store
            .db()
            .entity_factoms(source)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn same_txn_retract_parent_cascades_pending_component() {
    let (_dir, store) = temp_store();
    let child = declare_component(&store);
    let parent = store.new_entity().unwrap();
    let kid = store.new_entity().unwrap();

    let result = store
        .transact()
        .assert(parent, child, &kid)
        .retract_entity(parent)
        .commit()
        .unwrap();

    assert_eq!(result, None);
    assert!(
        store
            .db()
            .entity_factoms(parent)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .db()
            .entity_factoms(kid)
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
}
