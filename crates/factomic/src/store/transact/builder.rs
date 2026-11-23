use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashSet, btree_map::Entry},
    sync::Arc,
    time::SystemTime,
};

use fjall::{OwnedWriteBatch, Readable};
use fractional_index::FractionalIndex;
use itertools::Itertools;

use crate::keys;
use crate::primitive::{DecodePrimitive, EncodePrimitive};
use crate::store::{AttributeDef, Store};
use crate::value::{TAG_REF, Type, Value};
use crate::{
    Db,
    id::{AttributeId, EntityId, TxnId},
};
use crate::{
    Factom,
    factom::{ASSERT, CollectionKind, MapKey, Op, RETRACT, Slot},
};
use crate::{
    error::{Error, Result},
    value,
};

use super::super::{by_attr_key, by_attr_value, by_name_key};
use super::{FactomRef, check_key, ref_target};

const EA_KEY_OFFSET: usize = size_of::<EntityId>() + size_of::<AttributeId>();
const INITIAL_TXN_BATCH_CAPACITY: usize = 24;

enum InsertPosition {
    Before(Slot),
    After(Slot),
}

enum ListEnd {
    Front,
    Back,
}

#[allow(dead_code)]
enum PlannedOp {
    Raw {
        op: Op,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        ty: Type,
        region: Vec<u8>,
    },
    Append {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        region: Vec<u8>,
        end: ListEnd,
    },
    ReplaceList {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        regions: Vec<Vec<u8>>,
    },
    ReplaceMany {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        regions: Vec<Vec<u8>>,
    },
    ReplaceListAt {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        ordinal: usize,
        region: Vec<u8>,
    },
    InsertListAt {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        ordinal: usize,
        region: Vec<u8>,
    },
    RemoveListAt {
        entity: EntityId,
        attr: AttributeId,
        ordinal: usize,
    },
    MoveListAt {
        entity: EntityId,
        attr: AttributeId,
        from_ordinal: usize,
        to_ordinal: usize,
    },
    ReplaceMap {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        entries: Vec<(MapKey, Vec<u8>)>,
    },
    Clear {
        entity: EntityId,
        attr: AttributeId,
    },
    RemoveSlot {
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
    },
    InsertAt {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        region: Vec<u8>,
        position: InsertPosition,
    },
    MoveSlot {
        entity: EntityId,
        attr: AttributeId,
        from: Slot,
        to: Slot,
    },
    Rebalance {
        entity: EntityId,
        attr: AttributeId,
    },
    RevertTo {
        txn: TxnId,
    },
    RevertToTime {
        time: SystemTime,
    },
    UndoTxn {
        txn: TxnId,
    },
    UndoEntityLatest {
        entity: EntityId,
    },
    RetractEntity {
        entity: EntityId,
    },
}

pub struct Transaction<'a> {
    pub(crate) store: &'a Store,
    ops: Vec<PlannedOp>,
}

impl Store {
    pub fn transact(&self) -> Transaction<'_> {
        Transaction {
            store: self,
            ops: Vec::new(),
        }
    }
}
/*snapshots?
 * store.db -> store.db()
* -> pub struct Db {
    txn: TxnId,
    snapshot: fjall::Snapshot,
}

*/

impl<'a> Transaction<'a> {
    pub fn assert<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Assert,
            entity,
            attr,
            slot: Slot::Empty,
            ty: V::TYPE,
            region: value::region(value),
        });
        self
    }

    pub fn retract<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Retract,
            entity,
            attr,
            slot: Slot::Empty,
            ty: V::TYPE,
            region: value::region(value),
        });
        self
    }
    /// Accepts and upserts at a slot.
    /// Use [replace_list_at](Transaction::replace_list_at) instead if there are pending changes to the same collection within the same transaction.
    pub fn assert_at<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Assert,
            entity,
            attr,
            slot,
            ty: V::TYPE,
            region: value::region(value),
        });
        self
    }

    pub fn retract_at<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Retract,
            entity,
            attr,
            slot,
            ty: V::TYPE,
            region: value::region(value),
        });
        self
    }

    pub fn assert_value(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &Value,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Assert,
            entity,
            attr,
            slot: Slot::Empty,
            ty: value.value_type(),
            region: value.region(),
        });
        self
    }

    pub fn retract_value(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &Value,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Retract,
            entity,
            attr,
            slot: Slot::Empty,
            ty: value.value_type(),
            region: value.region(),
        });
        self
    }

    pub fn assert_value_at(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &Value,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Assert,
            entity,
            attr,
            slot,
            ty: value.value_type(),
            region: value.region(),
        });
        self
    }

    pub fn retract_value_at(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &Value,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Retract,
            entity,
            attr,
            slot,
            ty: value.value_type(),
            region: value.region(),
        });
        self
    }

    pub fn clear(&mut self, entity: EntityId, attr: AttributeId) -> &mut Self {
        self.ops.push(PlannedOp::Clear { entity, attr });
        self
    }

    pub fn push<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Append {
            entity,
            attr,
            ty: V::TYPE,
            region: value::region(value),
            end: ListEnd::Back,
        });
        self
    }

    pub fn prepend<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::Append {
            entity,
            attr,
            ty: V::TYPE,
            region: value::region(value),
            end: ListEnd::Front,
        });
        self
    }

    pub fn replace_list<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        values: impl IntoIterator<Item = V>,
    ) -> &mut Self {
        let regions = values
            .into_iter()
            .map(|value| value::region(&value))
            .collect();
        self.ops.push(PlannedOp::ReplaceList {
            entity,
            attr,
            ty: V::TYPE,
            regions,
        });
        self
    }

    pub fn replace_many<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        values: impl IntoIterator<Item = V>,
    ) -> &mut Self {
        let regions = values
            .into_iter()
            .map(|value| value::region(&value))
            .collect();
        self.ops.push(PlannedOp::ReplaceMany {
            entity,
            attr,
            ty: V::TYPE,
            regions,
        });
        self
    }

    pub fn replace_list_at<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        ordinal: usize,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::ReplaceListAt {
            entity,
            attr,
            ty: V::TYPE,
            ordinal,
            region: value::region(value),
        });
        self
    }

    pub fn insert_list_at<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        ordinal: usize,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::InsertListAt {
            entity,
            attr,
            ty: V::TYPE,
            ordinal,
            region: value::region(value),
        });
        self
    }

    pub fn remove_list_at(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        ordinal: usize,
    ) -> &mut Self {
        self.ops.push(PlannedOp::RemoveListAt {
            entity,
            attr,
            ordinal,
        });
        self
    }

    pub fn move_list_at(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        from_ordinal: usize,
        to_ordinal: usize,
    ) -> &mut Self {
        self.ops.push(PlannedOp::MoveListAt {
            entity,
            attr,
            from_ordinal,
            to_ordinal,
        });
        self
    }

    pub fn replace_map<K: Into<MapKey>, V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        entries: impl IntoIterator<Item = (K, V)>,
    ) -> &mut Self {
        let entries = entries
            .into_iter()
            .map(|(key, value)| (key.into(), value::region(&value)))
            .collect();
        self.ops.push(PlannedOp::ReplaceMap {
            entity,
            attr,
            ty: V::TYPE,
            entries,
        });
        self
    }

    pub fn remove(&mut self, entity: EntityId, attr: AttributeId, slot: Slot) -> &mut Self {
        self.ops.push(PlannedOp::RemoveSlot { entity, attr, slot });
        self
    }

    pub fn put<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        key: MapKey,
        value: &V,
    ) -> &mut Self {
        self.assert_at(entity, attr, Slot::Key(key), value)
    }

    pub fn remove_key(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        key: impl Into<MapKey>,
    ) -> &mut Self {
        self.remove(entity, attr, Slot::Key(key.into()))
    }

    pub fn insert_before<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        anchor: Slot,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::InsertAt {
            entity,
            attr,
            ty: V::TYPE,
            region: value::region(value),
            position: InsertPosition::Before(anchor),
        });
        self
    }

    pub fn insert_after<V: EncodePrimitive>(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        anchor: Slot,
        value: &V,
    ) -> &mut Self {
        self.ops.push(PlannedOp::InsertAt {
            entity,
            attr,
            ty: V::TYPE,
            region: value::region(value),
            position: InsertPosition::After(anchor),
        });
        self
    }

    pub fn move_slot(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        from: Slot,
        to: Slot,
    ) -> &mut Self {
        self.ops.push(PlannedOp::MoveSlot {
            entity,
            attr,
            from,
            to,
        });
        self
    }

    pub fn rebalance(&mut self, entity: EntityId, attr: AttributeId) -> &mut Self {
        self.ops.push(PlannedOp::Rebalance { entity, attr });
        self
    }

    pub fn retract_entity(&mut self, entity: EntityId) -> &mut Self {
        self.ops.push(PlannedOp::RetractEntity { entity });
        self
    }

    pub fn revert_to(&mut self, txn: TxnId) -> &mut Self {
        self.ops.push(PlannedOp::RevertTo { txn });
        self
    }

    pub fn revert_to_time(&mut self, time: SystemTime) -> &mut Self {
        self.ops.push(PlannedOp::RevertToTime { time });
        self
    }

    /// Cherry-pick a committed transaction for undo.
    /// This only reverses the operations in `txn` that are part of the current fact identity.
    /// For point-in-time restoration of the entire store, use [revert_to](Transaction::revert_to) instead.
    pub fn undo_txn(&mut self, txn: TxnId) -> &mut Self {
        self.ops.push(PlannedOp::UndoTxn { txn });
        self
    }

    pub fn undo_entity_latest(&mut self, entity: EntityId) -> &mut Self {
        self.ops.push(PlannedOp::UndoEntityLatest { entity });
        self
    }

    // todo: mutex for single writer or rwlock when snapshots/query result cache (parking_lot?)
    pub fn commit(&mut self) -> Result<Option<TxnId>> {
        let store = self.store;
        let ops = std::mem::take(&mut self.ops);
        let _guard = store.transact_lock.lock();
        let mut changes = TransactionChanges::new();
        for op in ops {
            plan_op(store, op, &mut changes)?;
        }
        #[cfg(debug_assertions)]
        changes.assert_reference_inserts_consistent();

        if changes.is_empty() {
            return Ok(None);
        }

        let txn = store.alloc_txn_id()?;
        let mut batch = store.durable_batch(INITIAL_TXN_BATCH_CAPACITY);

        for attribute_changes in changes.values() {
            write_attribute_changes(store, &mut batch, txn, attribute_changes)?;
        }

        for attr in &changes.attribute_removals {
            let def = store.attribute_def(*attr)?;
            batch.insert(
                &store.attrs_h,
                by_attr_key(*attr),
                by_attr_value(store.db().attribute_declaration(*attr)?.1, &def),
            );
            batch.remove(&store.attrs, by_attr_key(*attr));
            batch.remove(&store.attrs, by_name_key(&def.name));
        }

        store.record_tx_timestamp(&mut batch, txn);
        store.commit_durable(batch)?;
        store.swap(txn);
        Ok(Some(txn))
    }
}

fn write_attribute_changes(
    store: &Store,
    batch: &mut OwnedWriteBatch,
    txn: TxnId,
    changes: &AttributeChanges,
) -> Result<()> {
    if changes.evictions.is_empty() && changes.inserts.is_empty() {
        return Ok(());
    }

    let evictions = changes
        .evictions
        .iter()
        .map(|(key, old_txn)| decode_sv_key(key, Some(*old_txn)).map(|factom| (factom, *old_txn)))
        .collect::<Result<Vec<_>>>()?;
    let inserts = changes
        .inserts
        .iter()
        .map(|key| decode_sv_key(key, None))
        .collect::<Result<Vec<_>>>()?;

    for insert in &inserts {
        if insert.region[0] != TAG_REF {
            continue;
        }
        for (eviction, _) in &evictions {
            if eviction.region == insert.region {
                store.record_tx_move(batch, txn, changes.attr, &eviction.slot, &insert.slot);
            }
        }
    }
    for (eviction, old_txn) in &evictions {
        let old = FactomRef {
            entity: changes.entity,
            attr: changes.attr,
            slot: &eviction.slot,
            region: &eviction.region,
        };
        store.remove_current(batch, &changes.def, old)?;
        store.write_history(batch, &changes.def, old, *old_txn, txn)?;
        batch.insert(
            &store.txlog,
            keys::txlog_key(
                txn,
                changes.entity,
                changes.attr,
                &eviction.slot,
                &eviction.region,
            ),
            [RETRACT],
        );
    }
    for insert in &inserts {
        let factom = FactomRef {
            entity: changes.entity,
            attr: changes.attr,
            slot: &insert.slot,
            region: &insert.region,
        };
        store.insert_current(batch, &changes.def, factom, txn)?;
        batch.insert(
            &store.txlog,
            keys::txlog_key(
                txn,
                changes.entity,
                changes.attr,
                &insert.slot,
                &insert.region,
            ),
            [ASSERT],
        );
    }
    if let Some(tag) = changes.tag {
        store.record_tx_kind(batch, txn, tag);
    }
    Ok(())
}

struct AttributeChanges {
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    evictions: BTreeMap<Vec<u8>, TxnId>,
    inserts: BTreeSet<Vec<u8>>,
    tag: Option<&'static str>,
}

impl AttributeChanges {
    fn new(def: AttributeDef, entity: EntityId, attr: AttributeId) -> Self {
        AttributeChanges {
            def,
            entity,
            attr,
            evictions: BTreeMap::new(),
            inserts: BTreeSet::new(),
            tag: None,
        }
    }

    fn insert(&mut self, key: Vec<u8>) {
        self.inserts.insert(key);
    }

    fn evict(&mut self, factom: FactomState) {
        let key = factom.encoded_key();
        match factom.txn {
            Some(txn) => {
                self.evictions.insert(key, txn);
            }
            None => {
                self.inserts.remove(&key);
            }
        }
    }
}

struct TransactionChanges {
    by_attribute: BTreeMap<(EntityId, AttributeId), AttributeChanges>,
    reference_inserts: BTreeSet<Vec<u8>>,
    attribute_removals: BTreeSet<AttributeId>,
    attribute_cache: BTreeMap<AttributeId, AttributeDef>,
}

impl TransactionChanges {
    fn new() -> Self {
        Self {
            by_attribute: BTreeMap::new(),
            reference_inserts: BTreeSet::new(),
            attribute_removals: BTreeSet::new(),
            attribute_cache: BTreeMap::new(),
        }
    }
    fn check_type(&mut self, store: &Store, attr: AttributeId, ty: Type) -> Result<AttributeDef> {
        let def = match self.attribute_cache.entry(attr) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let new_def = store.attribute_def(attr)?;
                entry.insert(new_def)
            }
        };

        if def.value_type != ty {
            return Err(Error::TypeMismatch {
                attribute: def.name.clone(),
                expected: def.value_type,
                got: ty,
            });
        }

        Ok(def.clone())
    }
    fn get(&self, entity: EntityId, attr: AttributeId) -> Option<&AttributeChanges> {
        self.by_attribute.get(&(entity, attr))
    }

    fn values(&self) -> impl Iterator<Item = &AttributeChanges> {
        self.by_attribute.values()
    }

    fn is_empty(&self) -> bool {
        self.values()
            .all(|changes| changes.evictions.is_empty() && changes.inserts.is_empty())
            && self.attribute_removals.is_empty()
    }

    fn remove_attribute(&mut self, attr: AttributeId) {
        self.attribute_removals.insert(attr);
    }

    fn pending_attrs(&self, entity: EntityId) -> Vec<AttributeId> {
        self.by_attribute
            .range((entity, AttributeId::new(u16::MIN))..=(entity, AttributeId::new(u16::MAX)))
            .map(|((_, attr), _)| *attr)
            .collect()
    }

    fn ensure(&mut self, def: AttributeDef, entity: EntityId, attr: AttributeId) {
        self.by_attribute
            .entry((entity, attr))
            .or_insert_with(|| AttributeChanges::new(def, entity, attr));
    }

    fn insert(
        &mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        region: Vec<u8>,
    ) -> Result<()> {
        let key = sv_key(&slot, &region);
        let reference_key =
            ref_target(&region)?.map(|target| keys::vaes_key(target, attr, entity, &slot));
        let attribute_changes = self
            .by_attribute
            .get_mut(&(entity, attr))
            .expect("attribute changes must be initialized before insertion");
        if attribute_changes.evictions.remove(&key).is_some() {
            return Ok(());
        }
        attribute_changes.insert(key);
        if let Some(key) = reference_key {
            self.reference_inserts.insert(key);
        }
        Ok(())
    }

    fn evict(&mut self, entity: EntityId, attr: AttributeId, factom: FactomState) -> Result<()> {
        let reference_key = match factom.txn {
            Some(_) => None,
            None => ref_target(&factom.region)?
                .map(|target| keys::vaes_key(target, attr, entity, &factom.slot)),
        };
        self.by_attribute
            .get_mut(&(entity, attr))
            .expect("attribute changes must be initialized before eviction")
            .evict(factom);
        if let Some(key) = reference_key {
            self.reference_inserts.remove(&key);
        }
        Ok(())
    }

    fn clear_inserts(&mut self, entity: EntityId, attr: AttributeId) -> Result<()> {
        let Some(changes) = self.by_attribute.get_mut(&(entity, attr)) else {
            return Ok(());
        };
        for key in &changes.inserts {
            let insert = decode_sv_key(key, None)?;
            if let Some(target) = ref_target(&insert.region)? {
                self.reference_inserts
                    .remove(&keys::vaes_key(target, attr, entity, &insert.slot));
            }
        }
        changes.inserts.clear();
        Ok(())
    }

    fn set_tag(&mut self, entity: EntityId, attr: AttributeId, tag: &'static str) {
        self.by_attribute
            .get_mut(&(entity, attr))
            .expect("attribute changes must be initialized before tagging")
            .tag = Some(tag);
    }

    fn pending_reference_keys(&self, target: EntityId) -> impl Iterator<Item = &Vec<u8>> {
        let prefix = keys::vaes_ref_prefix(target);
        self.reference_inserts
            .range(prefix.clone()..)
            .take_while(move |key| key.starts_with(&prefix))
    }

    #[cfg(debug_assertions)]
    fn assert_reference_inserts_consistent(&self) {
        let mut expected = BTreeSet::new();
        for ((entity, attr), changes) in &self.by_attribute {
            for key in &changes.inserts {
                let insert = decode_sv_key(key, None).unwrap();
                if let Some(target) = ref_target(&insert.region).unwrap() {
                    expected.insert(keys::vaes_key(target, *attr, *entity, &insert.slot));
                }
            }
        }
        assert_eq!(self.reference_inserts, expected);
    }
}

struct FactomState {
    slot: Slot,
    /// [Value] as bytes
    region: Vec<u8>,
    txn: Option<TxnId>,
}

impl FactomState {
    fn new(slot: Slot, region: Vec<u8>, txn: Option<TxnId>) -> Self {
        Self { slot, region, txn }
    }

    fn pending(slot: Slot, region: Vec<u8>) -> Self {
        Self::new(slot, region, None)
    }

    fn committed(slot: Slot, region: Vec<u8>, txn: TxnId) -> Self {
        Self::new(slot, region, Some(txn))
    }

    fn encoded_key(&self) -> Vec<u8> {
        sv_key(&self.slot, &self.region)
    }
}

// sv
fn sv_key(slot: &Slot, region: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(slot.encoded_len() + region.len());
    slot.encode_key(&mut key);
    key.extend_from_slice(region);
    key
}

fn decode_txn(payload: &[u8]) -> TxnId {
    TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()))
}

fn decode_sv_key(key: &[u8], txn: Option<TxnId>) -> Result<FactomState> {
    let (slot, consumed) = Slot::decode_key(key)?;
    let region = key[consumed..].to_vec();
    Ok(FactomState::new(slot, region, txn))
}

fn compare_map_keys(left: &MapKey, right: &MapKey) -> Ordering {
    match (left, right) {
        (MapKey::Long(left), MapKey::Long(right)) => left.cmp(right),
        (MapKey::Long(_), _) => Ordering::Less,
        (_, MapKey::Long(_)) => Ordering::Greater,
        (MapKey::String(left), MapKey::String(right)) => left.cmp(right),
        (MapKey::String(_), MapKey::Ref(_)) => Ordering::Less,
        (MapKey::Ref(_), MapKey::String(_)) => Ordering::Greater,
        (MapKey::Ref(left), MapKey::Ref(right)) => left.cmp(right),
    }
}

fn compare_slots(left: &Slot, right: &Slot) -> Ordering {
    match (left, right) {
        (Slot::Empty, Slot::Empty) => Ordering::Equal,
        (Slot::Empty, _) => Ordering::Less,
        (_, Slot::Empty) => Ordering::Greater,
        (Slot::Position(left), Slot::Position(right)) => left.cmp(right),
        (Slot::Position(_), Slot::Key(_)) => Ordering::Less,
        (Slot::Key(_), Slot::Position(_)) => Ordering::Greater,
        (Slot::Key(left), Slot::Key(right)) => compare_map_keys(left, right),
    }
}

fn compare_factom_order(
    left_slot: &Slot,
    left_region: &[u8],
    right_slot: &Slot,
    right_region: &[u8],
) -> Ordering {
    compare_slots(left_slot, right_slot).then_with(|| left_region.cmp(right_region))
}

fn contains_sorted_factom(factoms: &[FactomState], slot: &Slot, region: &[u8]) -> bool {
    factoms
        .binary_search_by(|factom| compare_factom_order(&factom.slot, &factom.region, slot, region))
        .is_ok()
}

fn scan_slot_factom(
    factoms: impl Iterator<Item = Result<FactomState>>,
    slot: &Slot,
) -> Result<Option<FactomState>> {
    for factom in factoms {
        let factom = factom?;

        if compare_slots(&factom.slot, slot) == Ordering::Equal {
            return Ok(Some(factom));
        }
    }
    Ok(None)
}
/// Returns a view of current assertions, which includes:
/// - Already present assertions in the db that will not be evicted within the current transaction
/// - Pending assertions within the current transaction
fn visible_assertions<'a>(
    store: &'a Store,
    changes: &'a TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
) -> impl Iterator<Item = Result<FactomState>> + 'a {
    let pending = changes.get(entity, attr);

    let committed_pending = pending;

    let mut committed = store
        .easvt
        .prefix(keys::easv_entity_attr_prefix(entity, attr))
        .map(move |guard| -> Result<Option<_>> {
            let (key, payload) = guard.into_inner()?;
            let factom_key = &key[EA_KEY_OFFSET..];

            if let Some(changes) = committed_pending
                && changes.evictions.contains_key(factom_key)
            {
                return Ok(None);
            }

            let txn = decode_txn(&payload);
            let factom = decode_sv_key(factom_key, Some(txn))?;
            Ok(Some((key, factom)))
        })
        .filter_map(|result| result.transpose())
        .peekable();

    let mut inserts = pending
        .into_iter()
        .flat_map(|changes| changes.inserts.iter())
        .peekable();

    let mut finished = false;

    std::iter::from_fn(move || {
        if finished {
            return None;
        }

        let take_committed = match (committed.peek(), inserts.peek()) {
            (Some(Ok((key, _))), Some(insert_key)) => {
                &key[EA_KEY_OFFSET..] <= insert_key.as_slice()
            }

            (Some(_), _) => true,
            (None, Some(_)) => false,
            (None, None) => {
                finished = true;
                return None;
            }
        };

        let next = if take_committed {
            committed.next().unwrap().map(|(_, factom)| factom)
        } else {
            decode_sv_key(inserts.next().unwrap(), None)
        };

        if next.is_err() {
            finished = true;
        }

        Some(next)
    })
}

enum FactomVisibility {
    Absent,
    Pending,
    Committed(TxnId),
}

fn factom_visibility(
    store: &Store,
    changes: &TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    slot: &Slot,
    region: &[u8],
) -> Result<FactomVisibility> {
    if let Some(changes) = changes.get(entity, attr) {
        let key = sv_key(slot, region);
        if changes.inserts.contains(&key) {
            return Ok(FactomVisibility::Pending);
        }
        if changes.evictions.contains_key(&key) {
            return Ok(FactomVisibility::Absent);
        }
    }
    let key = keys::easv_key(entity, attr, slot, region);
    let Some(payload) = store.easvt.get(&key)? else {
        return Ok(FactomVisibility::Absent);
    };
    Ok(FactomVisibility::Committed(decode_txn(&payload)))
}

fn plan_op(store: &Store, op: PlannedOp, changes: &mut TransactionChanges) -> Result<()> {
    match op {
        PlannedOp::Raw {
            op,
            entity,
            attr,
            slot,
            ty,
            region,
        } => {
            let def = changes.check_type(store, attr, ty)?;
            check_key(&def, &slot)?;
            match op {
                Op::Assert => plan_assert(store, changes, def, entity, attr, slot, region),
                Op::Retract => plan_retract(store, changes, def, entity, attr, slot, region),
            }
        }
        PlannedOp::Append {
            entity,
            attr,
            ty,
            region,
            end,
        } => plan_append(store, changes, entity, attr, ty, region, end),
        PlannedOp::ReplaceList {
            entity,
            attr,
            ty,
            regions,
        } => plan_replace_list(store, changes, entity, attr, ty, regions),
        PlannedOp::ReplaceMany {
            entity,
            attr,
            ty,
            regions,
        } => plan_replace_many(store, changes, entity, attr, ty, regions),
        PlannedOp::ReplaceListAt {
            entity,
            attr,
            ty,
            ordinal,
            region,
        } => plan_replace_list_at(store, changes, entity, attr, ty, ordinal, region),
        PlannedOp::InsertListAt {
            entity,
            attr,
            ty,
            ordinal,
            region,
        } => plan_insert_list_at(store, changes, entity, attr, ty, ordinal, region),
        PlannedOp::RemoveListAt {
            entity,
            attr,
            ordinal,
        } => plan_remove_list_at(store, changes, entity, attr, ordinal),
        PlannedOp::MoveListAt {
            entity,
            attr,
            from_ordinal,
            to_ordinal,
        } => plan_move_list_at(store, changes, entity, attr, from_ordinal, to_ordinal),
        PlannedOp::ReplaceMap {
            entity,
            attr,
            ty,
            entries,
        } => plan_replace_map(store, changes, entity, attr, ty, entries),
        PlannedOp::Clear { entity, attr } => plan_clear(store, changes, entity, attr),
        PlannedOp::RemoveSlot { entity, attr, slot } => {
            let def = store.attribute_def(attr)?;
            if !matches!(def.kind, CollectionKind::List | CollectionKind::Map) {
                return Err(Error::MismatchedKey(def.name.clone()));
            }
            check_key(&def, &slot)?;
            plan_remove(store, changes, def, entity, attr, slot)
        }
        PlannedOp::InsertAt {
            entity,
            attr,
            ty,
            region,
            position,
        } => plan_insert_at(store, changes, entity, attr, ty, region, position),
        PlannedOp::MoveSlot {
            entity,
            attr,
            from,
            to,
        } => plan_move(store, changes, entity, attr, from, to),
        PlannedOp::Rebalance { entity, attr } => plan_rebalance(store, changes, entity, attr),
        PlannedOp::RevertTo { txn } => plan_revert_to(store, changes, txn),
        PlannedOp::RevertToTime { time } => plan_revert_to_time(store, changes, time),
        PlannedOp::UndoTxn { txn } => plan_revert_txn(store, changes, txn),
        PlannedOp::UndoEntityLatest { entity } => plan_undo_entity_latest(store, changes, entity),
        PlannedOp::RetractEntity { entity } => {
            let mut visited = HashSet::new();
            plan_retract_entity(store, changes, entity, &mut visited)
        }
    }
}

fn plan_assert(
    store: &Store,
    changes: &mut TransactionChanges,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
    region: Vec<u8>,
) -> Result<()> {
    check_component_owner(store, changes, &def, entity, attr, &region)?;
    match def.kind {
        CollectionKind::Many => {
            if !matches!(
                factom_visibility(store, changes, entity, attr, &slot, &region)?,
                FactomVisibility::Absent
            ) {
                return Ok(());
            }
            changes.ensure(def, entity, attr);
            changes.insert(entity, attr, slot, region)?;
        }
        CollectionKind::One | CollectionKind::List | CollectionKind::Map => {
            let visible =
                visible_assertions(store, changes, entity, attr).collect::<Result<Vec<_>>>()?;
            if contains_sorted_factom(&visible, &slot, &region) {
                return Ok(());
            }
            let replace_all = def.kind == CollectionKind::One;
            changes.ensure(def, entity, attr);
            for factom in visible {
                if replace_all || factom.slot == slot {
                    changes.evict(entity, attr, factom)?;
                }
            }
            changes.insert(entity, attr, slot, region)?;
        }
    }
    Ok(())
}

fn plan_retract(
    store: &Store,
    changes: &mut TransactionChanges,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
    region: Vec<u8>,
) -> Result<()> {
    let txn = match factom_visibility(store, changes, entity, attr, &slot, &region)? {
        FactomVisibility::Absent => return Ok(()),
        FactomVisibility::Pending => None,
        FactomVisibility::Committed(txn) => Some(txn),
    };
    changes.ensure(def, entity, attr);
    changes.evict(entity, attr, FactomState::new(slot, region, txn))
}

fn plan_remove(
    store: &Store,
    changes: &mut TransactionChanges,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
) -> Result<()> {
    let Some(factom) = scan_slot_factom(visible_assertions(store, changes, entity, attr), &slot)?
    else {
        return Ok(());
    };

    changes.ensure(def, entity, attr);
    changes.evict(entity, attr, factom)
}

fn plan_clear(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
) -> Result<()> {
    let def = store.attribute_def(attr)?;
    if def.kind != CollectionKind::One {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    plan_remove(store, changes, def, entity, attr, Slot::Empty)
}

fn plan_insert_at(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    region: Vec<u8>,
    position: InsertPosition,
) -> Result<()> {
    let anchor = match &position {
        InsertPosition::Before(anchor) | InsertPosition::After(anchor) => anchor,
    };
    let def = changes.check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    if !matches!(anchor, Slot::Position(_)) {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let mut visible = visible_assertions(store, changes, entity, attr);
    let mut prev = None;
    while let Some(result) = visible.next() {
        let factom = result?;

        if compare_slots(&factom.slot, anchor) == Ordering::Equal {
            let (left, right) = match position {
                InsertPosition::Before(_) => (prev, Some(factom)),
                InsertPosition::After(_) => (Some(factom), visible.next().transpose()?),
            };
            let left = left.map(|factom| factom.slot);
            let right = right.map(|factom| factom.slot);
            drop(visible);
            let slot = mint_between(left.as_ref(), right.as_ref())?;
            return plan_assert(store, changes, def, entity, attr, slot, region);
        }
        prev = Some(factom);
    }
    Err(Error::SlotNotFound(def.name))
}

fn plan_move(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    from: Slot,
    to: Slot,
) -> Result<()> {
    let def = store.attribute_def(attr)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    for slot in [&from, &to] {
        if !matches!(slot, Slot::Position(_)) {
            return Err(Error::MismatchedKey(def.name.clone()));
        }
    }
    let Some(factom) = scan_slot_factom(visible_assertions(store, changes, entity, attr), &from)?
    else {
        return Err(Error::SlotNotFound(def.name.clone()));
    };
    let region = factom.region.clone();
    plan_remove(store, changes, def.clone(), entity, attr, from)?;
    plan_assert(store, changes, def, entity, attr, to, region)
}

// todo: auto rebalance
fn plan_rebalance(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
) -> Result<()> {
    let def = store.attribute_def(attr)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_assertions(store, changes, entity, attr).collect::<Result<Vec<_>>>()?;
    if visible.len() < 2 {
        return Ok(());
    }
    changes.ensure(def, entity, attr);
    changes.set_tag(entity, attr, "rebalance");
    let mut index = FractionalIndex::default();
    let mut rebalanced = Vec::with_capacity(visible.len());
    for factom in visible {
        let region = factom.region.clone();
        changes.evict(entity, attr, factom)?;
        rebalanced.push((Slot::with_index(&index)?, region));
        index = FractionalIndex::new_after(&index);
    }
    for (slot, region) in rebalanced {
        changes.insert(entity, attr, slot, region)?;
    }
    Ok(())
}

fn plan_replace_many(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    regions: Vec<Vec<u8>>,
) -> Result<()> {
    let def = changes.check_type(store, attr, ty)?;
    if def.kind != CollectionKind::Many {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    for region in &regions {
        check_component_owner(store, changes, &def, entity, attr, region)?;
    }

    let replacements: BTreeSet<_> = regions.into_iter().collect();
    let visible = visible_assertions(store, changes, entity, attr).collect::<Result<Vec<_>>>()?;
    if visible.len() == replacements.len()
        && visible
            .iter()
            .zip(&replacements)
            .all(|(current, region)| current.slot == Slot::Empty && current.region == *region)
    {
        return Ok(());
    }

    changes.ensure(def, entity, attr);
    for factom in visible {
        changes.evict(entity, attr, factom)?;
    }
    for region in replacements {
        changes.insert(entity, attr, Slot::Empty, region)?;
    }
    Ok(())
}

fn plan_replace_list(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    regions: Vec<Vec<u8>>,
) -> Result<()> {
    let def = changes.check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    for region in &regions {
        check_component_owner(store, changes, &def, entity, attr, region)?;
    }

    let visible = visible_assertions(store, changes, entity, attr).collect::<Result<Vec<_>>>()?;
    let mut index = FractionalIndex::default();
    let mut replacements = Vec::with_capacity(regions.len());
    for region in regions {
        replacements.push((Slot::with_index(&index)?, region));
        index = FractionalIndex::new_after(&index);
    }

    if visible.len() == replacements.len()
        && visible
            .iter()
            .zip(&replacements)
            .all(|(current, (slot, region))| current.slot == *slot && current.region == *region)
    {
        return Ok(());
    }

    changes.ensure(def, entity, attr);
    for factom in visible {
        changes.evict(entity, attr, factom)?;
    }
    for (slot, region) in replacements {
        changes.insert(entity, attr, slot, region)?;
    }
    Ok(())
}

fn plan_replace_list_at(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    ordinal: usize,
    region: Vec<u8>,
) -> Result<()> {
    let def = changes.check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name));
    }
    let factom = itertools::process_results(
        visible_assertions(store, changes, entity, attr),
        |mut visible| visible.nth(ordinal),
    )?;

    let Some(factom) = factom else {
        return Err(Error::SlotNotFound(def.name));
    };
    let slot = factom.slot.clone();
    plan_assert(store, changes, def, entity, attr, slot, region)
}

fn plan_insert_list_at(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    ordinal: usize,
    region: Vec<u8>,
) -> Result<()> {
    let def = changes.check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name));
    }
    let sub_ordinal = ordinal.checked_sub(1);

    let (left, right) = itertools::process_results(
        visible_assertions(store, changes, entity, attr),
        |visible| {
            let mut immediate = visible
                .enumerate()
                .skip_while(|(index, _)| sub_ordinal.is_some_and(|ordinal| *index < ordinal));

            let left = sub_ordinal
                .and_then(|_| immediate.next())
                .map(|(_, factom)| factom);

            let right = immediate.next().map(|(_, factom)| factom);

            (left, right)
        },
    )?;

    if ordinal > 0 && left.is_none() {
        return Err(Error::SlotNotFound(def.name));
    }

    let slot = mint_between(
        left.as_ref().map(|factom| &factom.slot),
        right.as_ref().map(|factom| &factom.slot),
    )?;

    plan_assert(store, changes, def, entity, attr, slot, region)
}

fn plan_remove_list_at(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    ordinal: usize,
) -> Result<()> {
    let def = store.attribute_def(attr)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let factom = itertools::process_results(
        visible_assertions(store, changes, entity, attr),
        |mut visible| visible.nth(ordinal),
    )?;

    let Some(factom) = factom else {
        return Err(Error::SlotNotFound(def.name));
    };
    let slot = factom.slot.clone();
    plan_remove(store, changes, def, entity, attr, slot)
}

fn plan_move_list_at(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    from_ordinal: usize,
    to_ordinal: usize,
) -> Result<()> {
    let def = store.attribute_def(attr)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_assertions(store, changes, entity, attr).collect::<Result<Vec<_>>>()?;
    let Some(factom) = visible.get(from_ordinal) else {
        return Err(Error::SlotNotFound(def.name.clone()));
    };
    if to_ordinal >= visible.len() {
        return Err(Error::SlotNotFound(def.name.clone()));
    }
    if from_ordinal == to_ordinal {
        return Ok(());
    }
    let region = factom.region.clone();
    let from_slot = factom.slot.clone();
    let mut remaining = visible;
    remaining.remove(from_ordinal);
    let left = to_ordinal
        .checked_sub(1)
        .and_then(|index| remaining.get(index));
    let right = remaining.get(to_ordinal);
    let to_slot = mint_between(
        left.map(|factom| &factom.slot),
        right.map(|factom| &factom.slot),
    )?;
    plan_remove(store, changes, def.clone(), entity, attr, from_slot)?;
    plan_assert(store, changes, def, entity, attr, to_slot, region)
}

fn plan_replace_map(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    entries: Vec<(MapKey, Vec<u8>)>,
) -> Result<()> {
    let def = changes.check_type(store, attr, ty)?;
    if def.kind != CollectionKind::Map {
        return Err(Error::MismatchedKey(def.name.clone()));
    }

    let mut replacements = BTreeMap::new();
    for (key, region) in entries {
        let slot = Slot::Key(key);
        check_key(&def, &slot)?;
        let mut encoded_slot = Vec::with_capacity(slot.encoded_len());
        slot.encode_key(&mut encoded_slot);
        replacements.insert(encoded_slot, (slot, region));
    }
    let replacements: Vec<_> = replacements.into_values().collect();
    for (_, region) in &replacements {
        check_component_owner(store, changes, &def, entity, attr, region)?;
    }

    let visible = visible_assertions(store, changes, entity, attr).collect::<Result<Vec<_>>>()?;
    if visible.len() == replacements.len()
        && visible
            .iter()
            .zip(&replacements)
            .all(|(current, (slot, region))| current.slot == *slot && current.region == *region)
    {
        return Ok(());
    }

    changes.ensure(def, entity, attr);
    for factom in visible {
        changes.evict(entity, attr, factom)?;
    }
    for (slot, region) in replacements {
        changes.insert(entity, attr, slot, region)?;
    }
    Ok(())
}

fn plan_revert_to(store: &Store, changes: &mut TransactionChanges, txn: TxnId) -> Result<()> {
    let db = store.db();
    let mut operations = db.tx_factoms(txn).peekable();
    if operations.peek().is_none() {
        return Err(Error::UnknownTransaction(txn));
    }

    if db.txn() == txn {
        return Ok(());
    }
    let past_db = db.at(txn);
    let mut identities = BTreeSet::new();
    for item in past_db.snapshot.iter(&past_db.easvt) {
        let (key, _) = item.into_inner()?;
        let (entity, attr, _, _) = keys::decode_easv_key(&key)?;
        identities.insert((entity, attr));
    }

    for item in past_db.snapshot.iter(&past_db.easvt_h) {
        let (key, _) = item.into_inner()?;
        let (entity, attr, _, _, _) = keys::decode_easvt_key(&key)?;
        identities.insert((entity, attr));
    }

    for item in store.easvt.iter() {
        let (key, _) = item.into_inner()?;
        let (entity, attr, _, _) = keys::decode_easv_key(&key)?;
        identities.insert((entity, attr));
    }

    for item in db.attrs.iter() {
        let (key, _) = item.into_inner()?;
        if key[0] != super::super::BY_ATTR_PREFIX {
            continue;
        }

        let id = AttributeId::try_from(&key[1..])?;
        let (def, declared_at) = db.attribute_declaration(id)?;
        if declared_at <= txn {
            continue;
        }

        match past_db.named_attribute_def(&def.name)? {
            Some(target_def) if target_def == def => {}
            Some(_) => return Err(Error::SchemaConflict(def.name)),
            None => changes.remove_attribute(id),
        }
    }

    for (entity, attr) in identities {
        let target = past_db
            .entity_attr_factoms(entity, attr)
            .collect::<Result<Vec<Factom>>>()?;

        let target_by_identity: BTreeMap<Vec<u8>, Factom> = target
            .into_iter()
            .map(|factom| (sv_key(factom.slot(), &factom.value().region()), factom))
            .collect();

        let latest_by_identity: BTreeMap<Vec<u8>, FactomState> =
            visible_assertions(store, changes, entity, attr)
                .map_ok(|state| (sv_key(&state.slot, &state.region), state))
                .collect::<Result<BTreeMap<_, _>>>()?;
        let target_def = store.attribute_def(attr)?;

        for (identity, target_factom) in &target_by_identity {
            match latest_by_identity.get(identity) {
                Some(current)
                    if &current.slot == target_factom.slot()
                        && current.region == target_factom.value().region() =>
                {
                    // skip, exists both past and present
                }
                Some(current) => {
                    // changed between past and present
                    plan_retract(
                        store,
                        changes,
                        target_def.clone(),
                        entity,
                        attr,
                        current.slot.clone(),
                        current.region.clone(),
                    )?;

                    plan_assert(
                        store,
                        changes,
                        target_def.clone(),
                        entity,
                        attr,
                        target_factom.slot().clone(),
                        target_factom.value().region(),
                    )?;
                }
                None => {
                    // exists in past but not in present
                    plan_assert(
                        store,
                        changes,
                        target_def.clone(),
                        entity,
                        attr,
                        target_factom.slot().clone(),
                        target_factom.value().region(),
                    )?;
                }
            }
        }

        for (identity, current) in &latest_by_identity {
            if !target_by_identity.contains_key(identity) {
                // exists in present but not in past
                plan_retract(
                    store,
                    changes,
                    target_def.clone(),
                    entity,
                    attr,
                    current.slot.clone(),
                    current.region.clone(),
                )?;
            }
        }
    }
    Ok(())
}

fn plan_revert_to_time(
    store: &Store,
    changes: &mut TransactionChanges,
    time: SystemTime,
) -> Result<()> {
    let Some(txn) = store.db().txn_at(time)? else {
        return Err(Error::NoTransactionBeforeTimeError);
    };
    plan_revert_to(store, changes, txn)
}

fn revert_tx_factoms(
    store: &Store,
    db: &Arc<Db>,
    changes: &mut TransactionChanges,
    txn: TxnId,
    tx_factoms: impl Iterator<Item = Result<Factom>>,
) -> Result<()> {
    for result in tx_factoms {
        let factom = result?;
        let entity = factom.entity();
        let attr = factom.attribute();
        let Some(latest) = db.latest_of_factom(&factom)? else {
            continue;
        };
        if latest.txn() != txn || latest.op() != factom.op() {
            continue;
        }
        let region = factom.value().region();
        let identity = sv_key(factom.slot(), &region);

        let has_pending_operation = changes.get(entity, attr).is_some_and(|pending| {
            pending.inserts.contains(&identity) || pending.evictions.contains_key(&identity)
        });

        if has_pending_operation {
            continue;
        }
        let def = changes.check_type(store, factom.attribute(), factom.value().value_type())?;
        check_key(&def, factom.slot())?;

        match factom.op() {
            Op::Assert => plan_retract(
                store,
                changes,
                def,
                entity,
                attr,
                factom.slot().clone(),
                region,
            )?,
            Op::Retract => plan_assert(
                store,
                changes,
                def,
                entity,
                attr,
                factom.slot().clone(),
                region,
            )?,
        }
    }
    Ok(())
}

fn plan_revert_txn(store: &Store, changes: &mut TransactionChanges, txn: TxnId) -> Result<()> {
    let db = store.db();
    let mut tx_factoms = db.tx_factoms(txn).peekable();
    if (tx_factoms.peek()).is_none() {
        return Err(Error::UnknownTransaction(txn));
    }
    revert_tx_factoms(store, &db, changes, txn, tx_factoms)
}

fn plan_undo_entity_latest(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
) -> Result<()> {
    let Some(factom) = store.db().entity_latest_factom(entity)? else {
        return Err(Error::NoTransactionBeforeTimeError);
    };
    let txn = factom.txn();
    let db = store.db();
    let mut tx_factoms = db
        .tx_factoms(txn)
        .filter_ok(|f| f.entity() == entity)
        .peekable();
    if (tx_factoms.peek()).is_none() {
        return Err(Error::UnknownTransaction(txn));
    };
    revert_tx_factoms(store, &db, changes, txn, tx_factoms)
}

struct VisibleReference {
    source: EntityId,
    attr: AttributeId,
    factom: FactomState,
}

fn for_each_visible_reference(
    store: &Store,
    changes: &TransactionChanges,
    target: EntityId,
    mut visit: impl FnMut(VisibleReference) -> Result<()>,
) -> Result<()> {
    for guard in store.vaest.prefix(keys::vaes_ref_prefix(target)) {
        let (key, payload) = guard.into_inner()?;
        let (_, attr, source, slot) = keys::decode_vaes_key(&key)?;
        let region = Value::Ref(target).region();
        if let Some(changes) = changes.get(source, attr)
            && changes.evictions.contains_key(&sv_key(&slot, &region))
        {
            continue;
        }
        let txn = decode_txn(&payload);
        visit(VisibleReference {
            source,
            attr,
            factom: FactomState::committed(slot, region, txn),
        })?;
    }
    for key in changes.pending_reference_keys(target) {
        let (_, attr, source, slot) = keys::decode_vaes_key(key)?;
        let region = Value::Ref(target).region();
        visit(VisibleReference {
            source,
            attr,
            factom: FactomState::pending(slot, region),
        })?;
    }
    Ok(())
}

fn plan_retract_entity(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    visited: &mut HashSet<EntityId>,
) -> Result<()> {
    if !visited.insert(entity) {
        return Ok(());
    }
    let mut components = Vec::new();
    for guard in store.easvt.prefix(keys::easv_entity_prefix(entity)) {
        let (key, payload) = guard.into_inner()?;
        let attr = AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap()));
        let factom_key = &key[EA_KEY_OFFSET..];
        if let Some(changes) = changes.get(entity, attr)
            && changes.evictions.contains_key(factom_key)
        {
            continue;
        }
        let old_txn = decode_txn(&payload);
        let factom = decode_sv_key(factom_key, Some(old_txn))?;
        let def = store.attribute_def(attr)?;
        if def.is_component {
            components.push(EntityId::decode(&factom.region[1..])?);
        }
        changes.ensure(def, entity, attr);
        changes.evict(entity, attr, factom)?;
    }

    for attr in changes.pending_attrs(entity) {
        if let Some(changes) = changes.get(entity, attr)
            && changes.def.is_component
        {
            for key in &changes.inserts {
                let insert = decode_sv_key(key, None)?;
                components.push(EntityId::decode(&insert.region[1..])?);
            }
        }
        changes.clear_inserts(entity, attr)?;
    }
    for child in components {
        plan_retract_entity(store, changes, child, visited)?;
    }
    let mut incoming = Vec::new();
    for_each_visible_reference(store, changes, entity, |reference| {
        incoming.push(reference);
        Ok(())
    })?;
    for reference in incoming {
        let def = store.attribute_def(reference.attr)?;
        changes.ensure(def, reference.source, reference.attr);
        changes.evict(reference.source, reference.attr, reference.factom)?;
    }
    Ok(())
}

fn check_component_owner(
    store: &Store,
    changes: &TransactionChanges,
    def: &AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    region: &[u8],
) -> Result<()> {
    if !def.is_component {
        return Ok(());
    }
    let target = EntityId::decode(&region[1..])?;
    for_each_visible_reference(store, changes, target, |reference| {
        if reference.source == entity && reference.attr == attr {
            return Ok(());
        }
        let other_def = store.attribute_def(reference.attr)?;
        if other_def.is_component {
            return Err(Error::ComponentConflict(def.name.clone()));
        }
        Ok(())
    })
}

fn plan_append(
    store: &Store,
    changes: &mut TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    region: Vec<u8>,
    end: ListEnd,
) -> Result<()> {
    let def = changes.check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name));
    }
    let slot = match end {
        ListEnd::Front => {
            let first = visible_assertions(store, changes, entity, attr)
                .next()
                .transpose()?;
            mint_between(None, first.as_ref().map(|factom| &factom.slot))?
        }
        ListEnd::Back => {
            let last = itertools::process_results(
                visible_assertions(store, changes, entity, attr),
                |visible| visible.last(),
            )?;
            mint_between(last.as_ref().map(|factom| &factom.slot), None)?
        }
    };
    plan_assert(store, changes, def, entity, attr, slot, region)
}

fn mint_between(left: Option<&Slot>, right: Option<&Slot>) -> Result<Slot> {
    fn to_index(slot: &Slot) -> Result<FractionalIndex> {
        match slot {
            Slot::Position(bytes) => {
                FractionalIndex::from_bytes(bytes.clone()).map_err(|_| Error::CorruptPosition)
            }
            _ => Err(Error::CorruptPosition),
        }
    }
    let index = match (left, right) {
        (None, None) => FractionalIndex::default(),
        (Some(l), None) => FractionalIndex::new_after(&to_index(l)?),
        (None, Some(r)) => FractionalIndex::new_before(&to_index(r)?),
        (Some(l), Some(r)) => FractionalIndex::new(Some(&to_index(l)?), Some(&to_index(r)?))
            .ok_or(Error::CorruptPosition)?,
    };
    Slot::with_index(&index)
}

#[cfg(test)]
mod tests;
