use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashSet},
};

use fjall::OwnedWriteBatch;
use fractional_index::FractionalIndex;

use crate::factom::{ASSERT, CollectionKind, MapKey, Op, RETRACT, Slot};
use crate::id::{AttributeId, EntityId, TxnId};
use crate::keys;
use crate::primitive::{DecodePrimitive, EncodePrimitive};
use crate::store::{AttributeDef, Store};
use crate::value::{TAG_REF, Type, Value};
use crate::{
    error::{Error, Result},
    value,
};

use super::{FactomRef, check_key, check_type, ref_target};

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
    RetractEntity {
        entity: EntityId,
    },
}

pub struct Transaction<'a> {
    pub(crate) store: &'a Store,
    pub(crate) ops: Vec<PlannedOp>,
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

        store.record_tx_timestamp(&mut batch, txn);
        batch.commit()?;
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
}

impl TransactionChanges {
    fn new() -> Self {
        Self {
            by_attribute: BTreeMap::new(),
            reference_inserts: BTreeSet::new(),
        }
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
            ref_target(&region)?.map(|target| keys::vaest_key(target, attr, entity, &slot));
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
                .map(|target| keys::vaest_key(target, attr, entity, &factom.slot)),
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
                    .remove(&keys::vaest_key(target, attr, entity, &insert.slot));
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
        let prefix = keys::vaest_target_prefix(target);
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
                    expected.insert(keys::vaest_key(target, *attr, *entity, &insert.slot));
                }
            }
        }
        assert_eq!(self.reference_inserts, expected);
    }
}

struct FactomState {
    slot: Slot,
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

fn find_slot_index(factoms: &[FactomState], slot: &Slot) -> std::result::Result<usize, usize> {
    factoms.binary_search_by(|factom| compare_slots(&factom.slot, slot))
}

fn visible_factoms(
    store: &Store,
    changes: &TransactionChanges,
    entity: EntityId,
    attr: AttributeId,
) -> Result<Vec<FactomState>> {
    let pending = changes.get(entity, attr);
    let mut committed = store
        .easvt
        .prefix(keys::easvt_entity_attr_prefix(entity, attr))
        .map(|guard| -> Result<Option<_>> {
            let (key, payload) = guard.into_inner()?;
            let factom_key = &key[EA_KEY_OFFSET..];
            if let Some(changes) = pending
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
    let mut factoms = Vec::new();

    loop {
        let take_committed = match (committed.peek(), inserts.peek()) {
            (Some(Ok((key, _))), Some(insert_key)) => {
                &key[EA_KEY_OFFSET..] <= insert_key.as_slice()
            }
            (Some(_), _) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take_committed {
            let (_, factom) = committed.next().unwrap()?;
            factoms.push(factom);
        } else {
            let insert_key = inserts.next().unwrap();
            factoms.push(decode_sv_key(insert_key, None)?);
        }
    }

    Ok(factoms)
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
    let key = keys::easvt_key(entity, attr, slot, region);
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
            let def = check_type(store, attr, ty)?;
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
            let visible = visible_factoms(store, changes, entity, attr)?;
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
    let mut visible = visible_factoms(store, changes, entity, attr)?;
    let Ok(index) = find_slot_index(&visible, &slot) else {
        return Ok(());
    };
    changes.ensure(def, entity, attr);
    changes.evict(entity, attr, visible.swap_remove(index))
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
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    if !matches!(anchor, Slot::Position(_)) {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_factoms(store, changes, entity, attr)?;
    let Ok(anchor_index) = find_slot_index(&visible, anchor) else {
        return Err(Error::SlotNotFound(def.name.clone()));
    };
    let (left, right) = match position {
        InsertPosition::Before(_) => (anchor_index.checked_sub(1), Some(anchor_index)),
        InsertPosition::After(_) => (Some(anchor_index), anchor_index.checked_add(1)),
    };
    let left = left.and_then(|index| visible.get(index));
    let right = right.and_then(|index| visible.get(index));
    let slot = mint_between(
        left.map(|factom| &factom.slot),
        right.map(|factom| &factom.slot),
    )?;
    plan_assert(store, changes, def, entity, attr, slot, region)
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
    let visible = visible_factoms(store, changes, entity, attr)?;
    let Ok(index) = find_slot_index(&visible, &from) else {
        return Err(Error::SlotNotFound(def.name.clone()));
    };
    let region = visible[index].region.clone();
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
    let visible = visible_factoms(store, changes, entity, attr)?;
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
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::Many {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    for region in &regions {
        check_component_owner(store, changes, &def, entity, attr, region)?;
    }

    let replacements: BTreeSet<_> = regions.into_iter().collect();
    let visible = visible_factoms(store, changes, entity, attr)?;
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
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    for region in &regions {
        check_component_owner(store, changes, &def, entity, attr, region)?;
    }

    let visible = visible_factoms(store, changes, entity, attr)?;
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
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_factoms(store, changes, entity, attr)?;
    let Some(factom) = visible.get(ordinal) else {
        return Err(Error::SlotNotFound(def.name.clone()));
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
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_factoms(store, changes, entity, attr)?;
    if ordinal > visible.len() {
        return Err(Error::SlotNotFound(def.name.clone()));
    }
    let left = ordinal.checked_sub(1).and_then(|index| visible.get(index));
    let right = visible.get(ordinal);
    let slot = mint_between(
        left.map(|factom| &factom.slot),
        right.map(|factom| &factom.slot),
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
    let visible = visible_factoms(store, changes, entity, attr)?;
    let Some(factom) = visible.get(ordinal) else {
        return Err(Error::SlotNotFound(def.name.clone()));
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
    let visible = visible_factoms(store, changes, entity, attr)?;
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
    let def = check_type(store, attr, ty)?;
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

    let visible = visible_factoms(store, changes, entity, attr)?;
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
    for guard in store.vaest.prefix(keys::vaest_target_prefix(target)) {
        let (key, payload) = guard.into_inner()?;
        let (_, attr, source, slot) = keys::decode_vaest_key(&key)?;
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
        let (_, attr, source, slot) = keys::decode_vaest_key(key)?;
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
    for guard in store.easvt.prefix(keys::easvt_entity_prefix(entity)) {
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
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_factoms(store, changes, entity, attr)?;
    let slot = match end {
        ListEnd::Front => mint_between(None, visible.first().map(|factom| &factom.slot))?,
        ListEnd::Back => mint_between(visible.last().map(|factom| &factom.slot), None)?,
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
mod tests {
    use super::*;
    use crate::id::{TX_KIND_ATTR, TX_MOVE_ATTR};
    use crate::value::Value;
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
                .any(|f| f.attribute() == TX_MOVE_ATTR
                    && f.value() == &Value::Bytes(expected.clone()))
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
            tx_factoms.iter().any(|f| f.attribute() == TX_KIND_ATTR
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
        let log = store.db().tx_factoms(txn).unwrap();
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

        let log = store.db().tx_factoms(txn).unwrap();
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
}
