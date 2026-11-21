use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use fjall::{Keyspace, Readable, Snapshot};

use super::*;
use crate::factom::{Op, Slot};
use crate::value::Value;

type Records = BTreeMap<Vec<u8>, Vec<u8>>;
type CheckResult<T> = std::result::Result<T, String>;

fn entries(snapshot: &Snapshot, keyspace: &Keyspace) -> CheckResult<Records> {
    snapshot
        .iter(keyspace)
        .map(|guard| {
            let (key, value) = guard.into_inner().map_err(|error| error.to_string())?;
            Ok((key.to_vec(), value.to_vec()))
        })
        .collect()
}

fn compare(label: &str, expected: &Records, actual: &Records) -> CheckResult<()> {
    if expected == actual {
        return Ok(());
    }
    for (key, value) in expected {
        match actual.get(key) {
            None => return Err(format!("{label}: missing entry {key:?}")),
            Some(actual) if actual != value => {
                return Err(format!("{label}: payload mismatch for {key:?}"));
            }
            _ => {}
        }
    }
    for key in actual.keys() {
        if !expected.contains_key(key) {
            return Err(format!("{label}: unexpected entry {key:?}"));
        }
    }
    Err(format!("{label}: mismatch"))
}

fn insert(records: &mut Records, key: Vec<u8>, value: Vec<u8>, label: &str) -> CheckResult<()> {
    if records.insert(key.clone(), value).is_some() {
        return Err(format!("{label}: duplicate entry {key:?}"));
    }
    Ok(())
}

fn validate_schema(def: &AttributeDef) -> CheckResult<()> {
    let valid_key_type = match def.kind {
        CollectionKind::Map => matches!(def.key_type, Some(Type::String | Type::Long | Type::Ref)),
        _ => def.key_type.is_none(),
    };
    if !valid_key_type {
        return Err(format!("schema: invalid key type for {}", def.name));
    }
    if def.is_component && def.value_type != Type::Ref {
        return Err(format!("schema: non-ref component {}", def.name));
    }
    Ok(())
}

fn validate_fact(
    def: &AttributeDef,
    declared_at: TxnId,
    txn: TxnId,
    slot: &Slot,
    value: &Value,
) -> CheckResult<()> {
    if declared_at > txn {
        return Err(format!(
            "fact: declaration {declared_at:?} follows assertion {txn:?}"
        ));
    }
    if def.value_type != value.value_type() {
        return Err(format!("fact: value type mismatch for {}", def.name));
    }
    match def.kind {
        CollectionKind::One | CollectionKind::Many if *slot != Slot::Empty => {
            Err(format!("fact: unexpected slot for {}", def.name))
        }
        CollectionKind::List if !matches!(slot, Slot::Position(_)) => {
            Err(format!("fact: invalid list slot for {}", def.name))
        }
        CollectionKind::Map => match slot {
            Slot::Key(key) if Some(key.value_type()) == def.key_type => Ok(()),
            _ => Err(format!("fact: invalid map key for {}", def.name)),
        },
        CollectionKind::One | CollectionKind::Many | CollectionKind::List => Ok(()),
    }
}

fn check_counter(meta: &Records, partition: u8, maximum: u64) -> CheckResult<()> {
    let key = [META_ENTITY_PREFIX, partition];
    match meta.get(key.as_slice()) {
        Some(value) if value.len() == 8 => {
            let next = u64::from_be_bytes(value.as_slice().try_into().unwrap());
            if next <= maximum {
                return Err(format!(
                    "meta: counter {partition} is {next}, maximum is {maximum}"
                ));
            }
        }
        Some(_) => return Err(format!("meta: invalid counter {partition}")),
        None if maximum != 0 => return Err(format!("meta: missing counter {partition}")),
        None => {}
    }
    Ok(())
}

impl Store {
    pub(crate) fn next_counter(&self, partition: u8) -> Option<u64> {
        self.meta
            .get([META_ENTITY_PREFIX, partition])
            .unwrap()
            .map(|value| u64::from_be_bytes(value.as_ref().try_into().unwrap()))
    }

    pub(crate) fn logical_digest(&self) -> u64 {
        let _guard = self.transact_lock.lock();
        let snapshot = self.db.snapshot();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for (name, keyspace) in [
            ("attrs", &self.attrs),
            ("easvt", &self.easvt),
            ("aesvt", &self.aesvt),
            ("avest", &self.avest),
            ("vaest", &self.vaest),
            ("easvt_h", &self.easvt_h),
            ("aesvt_h", &self.aesvt_h),
            ("avest_h", &self.avest_h),
            ("vaest_h", &self.vaest_h),
            ("txlog", &self.txlog),
        ] {
            name.hash(&mut hasher);
            for (key, value) in entries(&snapshot, keyspace).unwrap() {
                key.hash(&mut hasher);
                value.hash(&mut hasher);
            }
        }
        for key in [META_FORMAT_VERSION, META_NEXT_ATTR.as_slice()] {
            key.hash(&mut hasher);
            self.meta.get(key).unwrap().hash(&mut hasher);
        }
        hasher.finish()
    }

    pub(crate) fn check_invariants(&self) -> CheckResult<()> {
        let _guard = self.transact_lock.lock();
        let snapshot = self.db.snapshot();
        let meta = entries(&snapshot, &self.meta)?;
        let attrs = entries(&snapshot, &self.attrs)?;
        let current = entries(&snapshot, &self.easvt)?;
        let current_aesvt = entries(&snapshot, &self.aesvt)?;
        let current_avest = entries(&snapshot, &self.avest)?;
        let current_vaest = entries(&snapshot, &self.vaest)?;
        let history = entries(&snapshot, &self.easvt_h)?;
        let history_aesvt = entries(&snapshot, &self.aesvt_h)?;
        let history_avest = entries(&snapshot, &self.avest_h)?;
        let history_vaest = entries(&snapshot, &self.vaest_h)?;
        let txlog = entries(&snapshot, &self.txlog)?;

        if meta.get(META_FORMAT_VERSION) != Some(&FORMAT_VERSION.to_be_bytes().to_vec()) {
            return Err("meta: incorrect format version".into());
        }

        let mut names = BTreeMap::new();
        let mut definitions = BTreeMap::new();
        for (key, value) in &attrs {
            let Some(prefix) = key.first().copied() else {
                return Err("attrs: empty key".into());
            };
            match prefix {
                BY_NAME_PREFIX => {
                    let name = String::from_utf8(key[1..].to_vec())
                        .map_err(|error| format!("attrs: invalid name: {error}"))?;
                    let id = AttributeId::try_from(value.as_slice())
                        .map_err(|error| format!("attrs: invalid name mapping: {error}"))?;
                    names.insert(name, id);
                }
                BY_ATTR_PREFIX => {
                    let id = AttributeId::try_from(&key[1..])
                        .map_err(|error| format!("attrs: invalid ID key: {error}"))?;
                    let (def, declared_at) = decode_attr_value(value)
                        .map_err(|error| format!("attrs: invalid definition: {error}"))?;
                    validate_schema(&def)?;
                    definitions.insert(id, (def, declared_at));
                }
                prefix => return Err(format!("attrs: unknown prefix {prefix}")),
            }
        }
        if names.len() + definitions.len() != attrs.len() {
            return Err("attrs: duplicate or incomplete mappings".into());
        }
        for (name, id) in &names {
            let (def, _) = definitions
                .get(id)
                .ok_or_else(|| format!("attrs: missing definition for {name}"))?;
            if &def.name != name {
                return Err(format!("attrs: name mismatch for {name}"));
            }
        }
        for (id, (def, _)) in &definitions {
            if names.get(&def.name) != Some(id) {
                return Err(format!("attrs: missing reverse mapping for {}", def.name));
            }
        }

        for (id, name, value_type, kind, indexed) in [
            (
                TX_TIMESTAMP_ATTR,
                "db/txTimestamp",
                Type::Timestamp,
                CollectionKind::One,
                true,
            ),
            (
                TX_MOVE_ATTR,
                "db/txMove",
                Type::Bytes,
                CollectionKind::Many,
                false,
            ),
            (
                TX_KIND_ATTR,
                "db/txKind",
                Type::String,
                CollectionKind::One,
                false,
            ),
        ] {
            let (def, declared_at) = definitions
                .get(&id)
                .ok_or_else(|| format!("attrs: missing system attribute {name}"))?;
            if def.name != name
                || def.value_type != value_type
                || def.kind != kind
                || def.indexed != indexed
                || def.is_component
                || def.key_type.is_some()
                || *declared_at != TxnId::with_value(0)
            {
                return Err(format!("attrs: invalid system attribute {name}"));
            }
        }

        if let Some(next) = meta.get(META_NEXT_ATTR.as_slice()) {
            let next = AttributeId::try_from(next.as_slice())
                .map_err(|error| format!("meta: invalid next attribute: {error}"))?;
            for (id, _) in definitions
                .iter()
                .filter(|(id, _)| id.as_u16() >= FIRST_USER_ATTR)
            {
                if next <= *id {
                    return Err("meta: next attribute does not exceed allocated IDs".into());
                }
            }
        } else if definitions.keys().any(|id| id.as_u16() >= FIRST_USER_ATTR) {
            return Err("meta: missing next attribute".into());
        }

        let mut expected_aesvt = Records::new();
        let mut expected_avest = Records::new();
        let mut expected_vaest = Records::new();
        let mut expected_log = Records::new();
        let mut current_txns = BTreeMap::new();
        let mut one_values = BTreeSet::new();
        let mut maximum_entity = 0_u64;
        for (key, payload) in &current {
            let (entity, attr, slot, value) = crate::keys::decode_easvt_key(key)
                .map_err(|error| format!("easvt: invalid key: {error}"))?;
            let txn = TxnId::try_from(payload.as_slice())
                .map_err(|error| format!("easvt: invalid transaction: {error}"))?;
            let (def, declared_at) = definitions
                .get(&attr)
                .ok_or_else(|| format!("easvt: unknown attribute {attr:?}"))?;
            validate_fact(def, *declared_at, txn, &slot, &value)?;
            if crate::keys::easvt_key(entity, attr, &slot, &value.region()) != *key {
                return Err("easvt: noncanonical key".into());
            }
            if def.kind == CollectionKind::One && !one_values.insert((entity, attr)) {
                return Err("easvt: multiple values for One attribute".into());
            }
            if entity.partition() == USER_PARTITION {
                maximum_entity = maximum_entity.max(entity.index());
            }
            if let Value::Ref(target) = value
                && target.partition() == USER_PARTITION
            {
                maximum_entity = maximum_entity.max(target.index());
            }
            insert(
                &mut expected_aesvt,
                crate::keys::aesvt_key(entity, attr, &slot, &value.region()),
                payload.clone(),
                "aesvt",
            )?;
            if def.indexed {
                insert(
                    &mut expected_avest,
                    crate::keys::avest_key(entity, attr, &slot, &value.region()),
                    payload.clone(),
                    "avest",
                )?;
            }
            if let Value::Ref(target) = value {
                insert(
                    &mut expected_vaest,
                    crate::keys::vaest_key(target, attr, entity, &slot),
                    payload.clone(),
                    "vaest",
                )?;
            }
            insert(
                &mut expected_log,
                crate::keys::txlog_key(txn, entity, attr, &slot, &value.region()),
                vec![Op::Assert.to_byte()],
                "txlog current",
            )?;
            current_txns.insert(key.clone(), txn);
        }
        compare("aesvt", &expected_aesvt, &current_aesvt)?;
        compare("avest", &expected_avest, &current_avest)?;
        compare("vaest", &expected_vaest, &current_vaest)?;

        let mut expected_history_aesvt = Records::new();
        let mut expected_history_avest = Records::new();
        let mut expected_history_vaest = Records::new();
        for (key, payload) in &history {
            let (entity, attr, slot, value, txn) = crate::keys::decode_easvt_history_key(key)
                .map_err(|error| format!("easvt_h: invalid key: {error}"))?;
            if payload.len() != 1 {
                return Err("easvt_h: invalid operation payload".into());
            }
            Op::try_from(payload[0]).map_err(|error| error.to_string())?;
            let (def, declared_at) = definitions
                .get(&attr)
                .ok_or_else(|| format!("easvt_h: unknown attribute {attr:?}"))?;
            validate_fact(def, *declared_at, txn, &slot, &value)?;
            if crate::keys::easvt_history_key(entity, attr, &slot, &value.region(), txn) != *key {
                return Err("easvt_h: noncanonical key".into());
            }
            if entity.partition() == USER_PARTITION {
                maximum_entity = maximum_entity.max(entity.index());
            }
            if let Value::Ref(target) = value
                && target.partition() == USER_PARTITION
            {
                maximum_entity = maximum_entity.max(target.index());
            }
            insert(
                &mut expected_history_aesvt,
                crate::keys::aesvt_history_key(entity, attr, &slot, &value.region(), txn),
                payload.clone(),
                "aesvt_h",
            )?;
            if def.indexed {
                insert(
                    &mut expected_history_avest,
                    crate::keys::avest_history_key(entity, attr, &slot, &value.region(), txn),
                    payload.clone(),
                    "avest_h",
                )?;
            }
            if let Value::Ref(target) = value {
                insert(
                    &mut expected_history_vaest,
                    crate::keys::vaest_history_key(target, attr, entity, &slot, txn),
                    payload.clone(),
                    "vaest_h",
                )?;
            }
            insert(
                &mut expected_log,
                crate::keys::txlog_key(txn, entity, attr, &slot, &value.region()),
                payload.clone(),
                "txlog history",
            )?;
        }
        compare("aesvt_h", &expected_history_aesvt, &history_aesvt)?;
        compare("avest_h", &expected_history_avest, &history_avest)?;
        compare("vaest_h", &expected_history_vaest, &history_vaest)?;
        compare("txlog", &expected_log, &txlog)?;

        let mut events: BTreeMap<Vec<u8>, Vec<(TxnId, Op)>> = BTreeMap::new();
        let mut timestamps: BTreeMap<TxnId, usize> = BTreeMap::new();
        let mut maximum_txn = TxnId::with_value(0);
        for (key, payload) in &txlog {
            let (txn, entity, attr, slot, value) = crate::keys::decode_txlog_key(key)
                .map_err(|error| format!("txlog: invalid key: {error}"))?;
            if payload.len() != 1 {
                return Err("txlog: invalid operation payload".into());
            }
            let op = Op::try_from(payload[0]).map_err(|error| error.to_string())?;
            let identity = crate::keys::easvt_key(entity, attr, &slot, &value.region());
            events.entry(identity).or_default().push((txn, op));
            maximum_txn = maximum_txn.max(txn);
            if attr == TX_TIMESTAMP_ATTR
                && entity == txn.entity_id()
                && slot == Slot::Empty
                && op == Op::Assert
                && matches!(value, Value::Timestamp(_))
            {
                *timestamps.entry(txn).or_default() += 1;
            }
            if entity.partition() == USER_PARTITION {
                maximum_entity = maximum_entity.max(entity.index());
            }
            if let Value::Ref(target) = value
                && target.partition() == USER_PARTITION
            {
                maximum_entity = maximum_entity.max(target.index());
            }
        }
        for (identity, events) in events {
            for (index, (txn, op)) in events.iter().enumerate() {
                if index == 0 && *op != Op::Assert {
                    return Err("txlog: identity starts with retraction".into());
                }
                if index > 0 && events[index - 1].1 == *op {
                    return Err("txlog: operations do not alternate".into());
                }
                let (entity, attr, slot, value) = crate::keys::decode_easvt_key(&identity)
                    .map_err(|error| format!("txlog: invalid identity: {error}"))?;
                let history_key =
                    crate::keys::easvt_history_key(entity, attr, &slot, &value.region(), *txn);
                let is_live_assertion = index + 1 == events.len() && *op == Op::Assert;
                if is_live_assertion {
                    if current_txns.get(&identity) != Some(txn) {
                        return Err("txlog: latest assertion is not current".into());
                    }
                    if history.contains_key(&history_key) {
                        return Err("txlog: live assertion is also historical".into());
                    }
                } else if history.get(&history_key) != Some(&vec![op.to_byte()]) {
                    return Err("txlog: event missing from history".into());
                }
            }
        }
        let logged_txns: BTreeSet<_> = txlog
            .keys()
            .map(|key| TxnId::with_value(u64::from_be_bytes(key[..8].try_into().unwrap())))
            .collect();
        for txn in logged_txns {
            if timestamps.get(&txn) != Some(&1) {
                return Err(format!(
                    "txlog: transaction {txn:?} has invalid timestamp count"
                ));
            }
        }
        check_counter(&meta, SYSTEM_PARTITION, maximum_txn.entity_id().index())?;
        check_counter(&meta, USER_PARTITION, maximum_entity)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> (tempfile::TempDir, Store) {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let [name, age, friend] = store
            .declare_attributes([
                AttributeDef {
                    name: "person/name".into(),
                    value_type: Type::String,
                    kind: CollectionKind::One,
                    key_type: None,
                    indexed: false,
                    is_component: false,
                },
                AttributeDef {
                    name: "person/age".into(),
                    value_type: Type::Long,
                    kind: CollectionKind::One,
                    key_type: None,
                    indexed: true,
                    is_component: false,
                },
                AttributeDef {
                    name: "person/friend".into(),
                    value_type: Type::Ref,
                    kind: CollectionKind::One,
                    key_type: None,
                    indexed: false,
                    is_component: false,
                },
            ])
            .unwrap();
        let entity = store.new_entity().unwrap();
        let first = store.new_entity().unwrap();
        let second = store.new_entity().unwrap();
        store.assert(entity, name, &"alice").unwrap();
        store.assert(entity, age, &30_i64).unwrap();
        store.assert(entity, friend, &first).unwrap();
        store.assert(entity, age, &31_i64).unwrap();
        store.assert(entity, friend, &second).unwrap();
        store.check_invariants().unwrap();
        (dir, store)
    }

    fn first_key(keyspace: &Keyspace) -> Vec<u8> {
        keyspace
            .iter()
            .next()
            .unwrap()
            .into_inner()
            .unwrap()
            .0
            .to_vec()
    }

    #[test]
    fn checker_detects_independent_corruption() {
        let mutations: [(&str, fn(&Store)); 8] = [
            ("missing aesvt", |store| {
                store.aesvt.remove(first_key(&store.aesvt)).unwrap();
            }),
            ("orphan avest", |store| {
                store.avest.insert(b"orphan".to_vec(), [0; 8]).unwrap();
            }),
            ("missing vaest history", |store| {
                store.vaest_h.remove(first_key(&store.vaest_h)).unwrap();
            }),
            ("invalid history op", |store| {
                let key = first_key(&store.easvt_h);
                store.easvt_h.insert(key, [0xFF]).unwrap();
            }),
            ("missing txlog retraction", |store| {
                let key = store
                    .txlog
                    .iter()
                    .map(|guard| guard.into_inner().unwrap())
                    .find(|(_, value)| value.as_ref() == [Op::Retract.to_byte()])
                    .unwrap()
                    .0;
                store.txlog.remove(key).unwrap();
            }),
            ("live assertion in history", |store| {
                let (key, payload) = store.easvt.iter().next().unwrap().into_inner().unwrap();
                let (entity, attr, slot, value) = crate::keys::decode_easvt_key(&key).unwrap();
                let txn = TxnId::try_from(payload.as_ref()).unwrap();
                store
                    .easvt_h
                    .insert(
                        crate::keys::easvt_history_key(entity, attr, &slot, &value.region(), txn),
                        [Op::Assert.to_byte()],
                    )
                    .unwrap();
            }),
            ("broken schema mapping", |store| {
                store
                    .attrs
                    .insert(
                        super::by_name_key("person/name"),
                        AttributeId::new(u16::MAX).to_be_bytes(),
                    )
                    .unwrap();
            }),
            ("missing timestamp", |store| {
                let key = store
                    .txlog
                    .iter()
                    .map(|guard| guard.into_inner().unwrap())
                    .find(|(key, _)| {
                        crate::keys::decode_txlog_key(key).unwrap().2 == TX_TIMESTAMP_ATTR
                    })
                    .unwrap()
                    .0;
                store.txlog.remove(key).unwrap();
            }),
        ];

        for (label, mutate) in mutations {
            let (_dir, store) = seed();
            mutate(&store);
            let error = store.check_invariants().unwrap_err();
            assert!(!error.is_empty(), "{label}");
        }
    }

    fn attribute(store: &Store, name: &str) -> AttributeId {
        store.db().attribute_id(name).unwrap().unwrap()
    }

    fn user_entity(store: &Store) -> EntityId {
        store
            .easvt
            .iter()
            .map(|guard| guard.into_inner().unwrap())
            .find_map(|(key, _)| {
                let (entity, ..) = crate::keys::decode_easvt_key(&key).unwrap();
                (entity.partition() == USER_PARTITION).then_some(entity)
            })
            .unwrap()
    }

    fn simple_def(name: &str, value_type: Type) -> AttributeDef {
        AttributeDef {
            name: name.into(),
            value_type,
            kind: CollectionKind::One,
            key_type: None,
            indexed: false,
            is_component: false,
        }
    }

    fn allocated_txn(store: &Store) -> TxnId {
        TxnId::new(
            SYSTEM_PARTITION,
            store.next_counter(SYSTEM_PARTITION).unwrap() - 1,
        )
    }

    #[test]
    fn fact_validation_failure_preserves_logical_state_and_counters() {
        let (_dir, store) = seed();
        let entity = user_entity(&store);
        let age = attribute(&store, "person/age");
        let name = attribute(&store, "person/name");
        let digest = store.logical_digest();
        let counter = store.next_counter(SYSTEM_PARTITION);
        let txn = store.db().txn();
        let mut tx = store.transact();
        tx.assert(entity, age, &32_i64)
            .assert(entity, name, &32_i64);

        assert!(matches!(tx.commit(), Err(Error::TypeMismatch { .. })));
        assert_eq!(store.logical_digest(), digest);
        assert_eq!(store.next_counter(SYSTEM_PARTITION), counter);
        assert_eq!(store.db().txn(), txn);
        store.check_invariants().unwrap();
        assert_eq!(tx.commit().unwrap(), None);
        tx.assert(entity, age, &33_i64);
        tx.commit().unwrap().unwrap();
        assert_eq!(
            store.db().entity_value::<i64>(entity, age).unwrap(),
            Some(33)
        );
        store.check_invariants().unwrap();
    }

    #[test]
    fn schema_conflict_preserves_schema_but_may_consume_txn() {
        let (_dir, store) = seed();
        let digest = store.logical_digest();
        let counter = store.next_counter(SYSTEM_PARTITION).unwrap();
        let txn = store.db().txn();
        let result = store.declare_attributes([
            simple_def("person/fresh", Type::String),
            simple_def("person/name", Type::Long),
        ]);

        assert!(matches!(result, Err(Error::SchemaConflict(_))));
        assert_eq!(store.logical_digest(), digest);
        assert_eq!(store.next_counter(SYSTEM_PARTITION), Some(counter + 1));
        assert_eq!(store.db().txn(), txn);
        assert_eq!(store.db().attribute_id("person/fresh").unwrap(), None);
        store.check_invariants().unwrap();
    }

    #[test]
    fn fact_precommit_error_is_atomic() {
        let (dir, store) = seed();
        let entity = user_entity(&store);
        let target = store.new_entity().unwrap();
        let age = attribute(&store, "person/age");
        let friend = attribute(&store, "person/friend");
        let digest = store.logical_digest();
        let txn = store.db().txn();
        fail_next_commit(CommitFailure::BeforeCommit);
        let result = store
            .transact()
            .assert(entity, age, &32_i64)
            .assert(entity, friend, &target)
            .commit();

        assert!(matches!(result, Err(Error::Storage(_))));
        assert_eq!(store.logical_digest(), digest);
        assert_eq!(store.db().txn(), txn);
        store.check_invariants().unwrap();
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(reopened.logical_digest(), digest);
        reopened.check_invariants().unwrap();
    }

    #[test]
    fn schema_precommit_error_is_atomic() {
        let (dir, store) = seed();
        let digest = store.logical_digest();
        let txn = store.db().txn();
        fail_next_commit(CommitFailure::BeforeCommit);
        let result = store.declare_attributes([
            simple_def("person/fresh", Type::String),
            simple_def("person/other", Type::Long),
        ]);

        assert!(matches!(result, Err(Error::Storage(_))));
        assert_eq!(store.logical_digest(), digest);
        assert_eq!(store.db().txn(), txn);
        assert_eq!(store.db().attribute_id("person/fresh").unwrap(), None);
        assert_eq!(store.db().attribute_id("person/other").unwrap(), None);
        store.check_invariants().unwrap();
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(reopened.logical_digest(), digest);
        reopened.check_invariants().unwrap();
    }

    #[test]
    fn fact_postcommit_error_recovers_complete_transaction() {
        let (dir, store) = seed();
        let entity = user_entity(&store);
        let target = store.new_entity().unwrap();
        let age = attribute(&store, "person/age");
        let friend = attribute(&store, "person/friend");
        let previous = store.db().txn();
        fail_next_commit(CommitFailure::AfterCommit);
        let result = store
            .transact()
            .assert(entity, age, &32_i64)
            .assert(entity, friend, &target)
            .commit();

        assert!(matches!(result, Err(Error::Storage(_))));
        assert_eq!(store.db().txn(), previous);
        store.check_invariants().unwrap();
        let committed = allocated_txn(&store);
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(reopened.db().txn(), committed);
        assert_eq!(
            reopened.db().entity_value::<i64>(entity, age).unwrap(),
            Some(32)
        );
        assert_eq!(
            reopened
                .db()
                .entity_value::<EntityId>(entity, friend)
                .unwrap(),
            Some(target)
        );
        assert_eq!(
            reopened
                .db_at(previous)
                .entity_value::<i64>(entity, age)
                .unwrap(),
            Some(31)
        );
        reopened.check_invariants().unwrap();
    }

    #[test]
    fn schema_postcommit_error_recovers_complete_declaration() {
        let (dir, store) = seed();
        let previous = store.db().txn();
        fail_next_commit(CommitFailure::AfterCommit);
        let result = store.declare_attributes([
            simple_def("person/fresh", Type::String),
            simple_def("person/other", Type::Long),
        ]);

        assert!(matches!(result, Err(Error::Storage(_))));
        assert_eq!(store.db().txn(), previous);
        store.check_invariants().unwrap();
        let committed = allocated_txn(&store);
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(reopened.db().txn(), committed);
        assert!(
            reopened
                .db()
                .attribute_id("person/fresh")
                .unwrap()
                .is_some()
        );
        assert!(
            reopened
                .db()
                .attribute_id("person/other")
                .unwrap()
                .is_some()
        );
        assert!(
            reopened
                .db()
                .tx_factoms(committed)
                .unwrap()
                .iter()
                .any(|factom| factom.attribute() == TX_TIMESTAMP_ATTR)
        );
        reopened.check_invariants().unwrap();
    }
}
