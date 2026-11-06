use std::collections::{BTreeMap, HashMap, HashSet};

use fjall::OwnedWriteBatch;
use fractional_index::FractionalIndex;

use crate::error::{Error, Result};
use crate::factom::{ASSERT, CollectionKind, MapKey, Op, RETRACT, Slot};
use crate::id::{AttributeId, EntityId, TxnId};
use crate::keys;
use crate::primitive::{DecodePrimitive, EncodePrimitive};
use crate::store::{AttributeDef, Store};
use crate::value::{TAG_REF, Type, Value};

use super::{FactomRef, check_key, check_type, encode_region, value_region};

enum PlannedOp {
    Raw {
        op: Op,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        ty: Type,
        region: Vec<u8>,
    },
    Push {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        region: Vec<u8>,
    },
    Prepend {
        entity: EntityId,
        attr: AttributeId,
        ty: Type,
        region: Vec<u8>,
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
        anchor: Slot,
        before: bool,
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
    store: &'a Store,
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
        mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Assert,
            entity,
            attr,
            slot: Slot::Empty,
            ty: V::TYPE,
            region: encode_region(value),
        });
        self
    }

    pub fn retract<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Retract,
            entity,
            attr,
            slot: Slot::Empty,
            ty: V::TYPE,
            region: encode_region(value),
        });
        self
    }

    pub fn assert_at<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Assert,
            entity,
            attr,
            slot,
            ty: V::TYPE,
            region: encode_region(value),
        });
        self
    }

    pub fn retract_at<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Retract,
            entity,
            attr,
            slot,
            ty: V::TYPE,
            region: encode_region(value),
        });
        self
    }

    pub fn assert_value(mut self, entity: EntityId, attr: AttributeId, value: &Value) -> Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Assert,
            entity,
            attr,
            slot: Slot::Empty,
            ty: value.value_type(),
            region: value_region(value),
        });
        self
    }

    pub fn retract_value(mut self, entity: EntityId, attr: AttributeId, value: &Value) -> Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Retract,
            entity,
            attr,
            slot: Slot::Empty,
            ty: value.value_type(),
            region: value_region(value),
        });
        self
    }

    pub fn assert_value_at(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &Value,
    ) -> Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Assert,
            entity,
            attr,
            slot,
            ty: value.value_type(),
            region: value_region(value),
        });
        self
    }

    pub fn retract_value_at(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        slot: Slot,
        value: &Value,
    ) -> Self {
        self.ops.push(PlannedOp::Raw {
            op: Op::Retract,
            entity,
            attr,
            slot,
            ty: value.value_type(),
            region: value_region(value),
        });
        self
    }

    pub fn push<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp::Push {
            entity,
            attr,
            ty: V::TYPE,
            region: encode_region(value),
        });
        self
    }

    pub fn prepend<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp::Prepend {
            entity,
            attr,
            ty: V::TYPE,
            region: encode_region(value),
        });
        self
    }

    pub fn remove(mut self, entity: EntityId, attr: AttributeId, slot: Slot) -> Self {
        self.ops.push(PlannedOp::RemoveSlot { entity, attr, slot });
        self
    }

    pub fn put<V: EncodePrimitive>(
        self,
        entity: EntityId,
        attr: AttributeId,
        key: MapKey,
        value: &V,
    ) -> Self {
        self.assert_at(entity, attr, Slot::Key(key), value)
    }

    pub fn remove_key(self, entity: EntityId, attr: AttributeId, key: MapKey) -> Self {
        self.remove(entity, attr, Slot::Key(key))
    }

    pub fn insert_before<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        anchor: Slot,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp::InsertAt {
            entity,
            attr,
            ty: V::TYPE,
            region: encode_region(value),
            anchor,
            before: true,
        });
        self
    }

    pub fn insert_after<V: EncodePrimitive>(
        mut self,
        entity: EntityId,
        attr: AttributeId,
        anchor: Slot,
        value: &V,
    ) -> Self {
        self.ops.push(PlannedOp::InsertAt {
            entity,
            attr,
            ty: V::TYPE,
            region: encode_region(value),
            anchor,
            before: false,
        });
        self
    }

    pub fn move_slot(mut self, entity: EntityId, attr: AttributeId, from: Slot, to: Slot) -> Self {
        self.ops.push(PlannedOp::MoveSlot {
            entity,
            attr,
            from,
            to,
        });
        self
    }

    pub fn rebalance(mut self, entity: EntityId, attr: AttributeId) -> Self {
        self.ops.push(PlannedOp::Rebalance { entity, attr });
        self
    }

    pub fn retract_entity(mut self, entity: EntityId) -> Self {
        self.ops.push(PlannedOp::RetractEntity { entity });
        self
    }
    // todo: mutex for single writer or rwlock when snapshots/query result cache (parking_lot?)
    pub fn commit(self) -> Result<Option<TxnId>> {
        let Transaction { store, ops } = self;
        let mut effectives = Vec::new();
        let mut pending = HashMap::new();
        for op in ops {
            effectives.extend(plan_op(store, op, &mut pending)?);
        }

        if effectives.is_empty() {
            return Ok(None);
        }

        let txn = store.alloc_txn_id()?;
        let mut batch = OwnedWriteBatch::with_capacity(store.db.clone(), 24);

        for eff in &effectives {
            for (to_slot, region) in &eff.inserts {
                if region[0] != TAG_REF {
                    continue;
                }
                for other in &effectives {
                    if other.attr != eff.attr || other.entity != eff.entity {
                        continue;
                    }
                    for (from_slot, old_region, _) in &other.evictions {
                        if old_region == region {
                            store.record_tx_move(&mut batch, txn, eff.attr, from_slot, to_slot);
                        }
                    }
                }
            }
        }

        for eff in &effectives {
            for (slot, old_region, old_txn) in &eff.evictions {
                let old = FactomRef {
                    entity: eff.entity,
                    attr: eff.attr,
                    slot,
                    region: old_region,
                };
                store.remove_current(&mut batch, &eff.def, old)?;
                store.write_history(&mut batch, &eff.def, old, *old_txn, txn)?;
                batch.insert(
                    &store.txlog,
                    keys::txlog_key(txn, eff.entity, eff.attr, slot, old_region),
                    [RETRACT],
                );
            }
            for (slot, region) in &eff.inserts {
                let datom = FactomRef {
                    entity: eff.entity,
                    attr: eff.attr,
                    slot,
                    region,
                };
                store.insert_current(&mut batch, &eff.def, datom, txn)?;
                batch.insert(
                    &store.txlog,
                    keys::txlog_key(txn, eff.entity, eff.attr, slot, region),
                    [ASSERT],
                );
            }
            if let Some(tag) = eff.tag {
                store.record_tx_kind(&mut batch, txn, tag);
            }
        }

        store.record_tx_timestamp(&mut batch, txn);
        batch.commit()?;
        Ok(Some(txn))
    }
}

type Evictions = Vec<(Slot, Vec<u8>, TxnId)>;

struct Effective {
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    evictions: Evictions,
    inserts: Vec<(Slot, Vec<u8>)>,
    tag: Option<&'static str>,
}

impl Effective {
    fn new(def: AttributeDef, entity: EntityId, attr: AttributeId) -> Self {
        Effective {
            def,
            entity,
            attr,
            evictions: vec![],
            inserts: vec![],
            tag: None,
        }
    }
}

fn plan_op(
    store: &Store,
    op: PlannedOp,
    pending: &mut HashMap<(EntityId, AttributeId), (Slot, Slot)>,
) -> Result<Vec<Effective>> {
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
                Op::Assert => plan_assert(store, def, entity, attr, slot, region),
                Op::Retract => plan_retract(store, def, entity, attr, slot, region),
            }
        }
        PlannedOp::Push {
            entity,
            attr,
            ty,
            region,
        } => plan_append(store, entity, attr, ty, region, true, pending),
        PlannedOp::Prepend {
            entity,
            attr,
            ty,
            region,
        } => plan_append(store, entity, attr, ty, region, false, pending),
        PlannedOp::RemoveSlot { entity, attr, slot } => {
            let def = store.attribute_def(attr)?;
            if !matches!(def.kind, CollectionKind::List | CollectionKind::Map) {
                return Err(Error::MismatchedKey(def.name.clone()));
            }
            check_key(&def, &slot)?;
            plan_remove(store, def, entity, attr, slot)
        }
        PlannedOp::InsertAt {
            entity,
            attr,
            ty,
            region,
            anchor,
            before,
        } => plan_insert_at(store, entity, attr, ty, region, anchor, before),
        PlannedOp::MoveSlot {
            entity,
            attr,
            from,
            to,
        } => plan_move(store, entity, attr, from, to),
        PlannedOp::Rebalance { entity, attr } => {
            pending.remove(&(entity, attr));
            plan_rebalance(store, entity, attr)
        }
        PlannedOp::RetractEntity { entity } => {
            let mut visited = HashSet::new();
            plan_retract_entity(store, entity, &mut visited)
        }
    }
}

fn plan_assert(
    store: &Store,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
    region: Vec<u8>,
) -> Result<Vec<Effective>> {
    check_component_owner(store, &def, entity, attr, &region)?;
    let new_key = keys::easvt_key(entity, attr, &slot, &region);
    let mut effective = Effective::new(def, entity, attr);
    match effective.def.kind {
        CollectionKind::One => {
            for guard in store
                .easvt
                .prefix(keys::easvt_entity_attr_prefix(entity, attr))
            {
                let (key, payload) = guard.into_inner()?;
                if *key == *new_key {
                    return Ok(vec![]);
                }
                let (old_slot, consumed) =
                    Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
                let old_txn =
                    TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
                let region =
                    key[size_of::<EntityId>() + size_of::<AttributeId>() + consumed..].to_vec();
                effective.evictions.push((old_slot, region, old_txn));
            }
        }
        CollectionKind::Many => {
            if store.easvt.get(&new_key)?.is_some() {
                return Ok(vec![]);
            }
        }
        CollectionKind::List | CollectionKind::Map => {
            for guard in store
                .easvt
                .prefix(keys::easvt_entity_attr_prefix(entity, attr))
            {
                let (key, payload) = guard.into_inner()?;
                if *key == *new_key {
                    return Ok(vec![]);
                }
                let (s, consumed) =
                    Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
                if s == slot {
                    let old_txn =
                        TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
                    let region =
                        key[size_of::<EntityId>() + size_of::<AttributeId>() + consumed..].to_vec();
                    effective.evictions.push((s, region, old_txn));
                }
            }
        }
    }
    effective.inserts.push((slot, region));
    Ok(vec![effective])
}

fn plan_retract(
    store: &Store,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
    region: Vec<u8>,
) -> Result<Vec<Effective>> {
    let key = keys::easvt_key(entity, attr, &slot, &region);
    let Some(payload) = store.easvt.get(&key)? else {
        return Ok(vec![]);
    };
    let old_txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
    let mut effective = Effective::new(def, entity, attr);
    effective.evictions.push((slot, region, old_txn));
    Ok(vec![effective])
}

fn plan_remove(
    store: &Store,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
) -> Result<Vec<Effective>> {
    for guard in store
        .easvt
        .prefix(keys::easvt_entity_attr_prefix(entity, attr))
    {
        let (key, payload) = guard.into_inner()?;
        let (s, consumed) =
            Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
        if s == slot {
            let old_txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
            let region =
                key[size_of::<EntityId>() + size_of::<AttributeId>() + consumed..].to_vec();
            let mut effective = Effective::new(def, entity, attr);
            effective.evictions.push((s, region, old_txn));
            return Ok(vec![effective]);
        }
    }
    Ok(vec![])
}

fn plan_insert_at(
    store: &Store,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    region: Vec<u8>,
    anchor: Slot,
    before: bool,
) -> Result<Vec<Effective>> {
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    if !matches!(anchor, Slot::Position(_)) {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let mut anchor_found = false;
    let mut neighbor = None;
    for guard in store
        .easvt
        .prefix(keys::easvt_entity_attr_prefix(entity, attr))
    {
        let (key, _) = guard.into_inner()?;
        let (s, _) = Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
        if before {
            if s == anchor {
                anchor_found = true;
                break;
            }
            neighbor = Some(s);
        } else {
            if anchor_found {
                neighbor = Some(s);
                break;
            }
            if s == anchor {
                anchor_found = true;
            }
        }
    }
    if !anchor_found {
        return Err(Error::SlotNotFound(def.name.clone()));
    }
    let slot = if before {
        mint_between(neighbor.as_ref(), Some(&anchor))?
    } else {
        mint_between(Some(&anchor), neighbor.as_ref())?
    };
    plan_assert(store, def, entity, attr, slot, region)
}

fn plan_move(
    store: &Store,
    entity: EntityId,
    attr: AttributeId,
    from: Slot,
    to: Slot,
) -> Result<Vec<Effective>> {
    let def = store.attribute_def(attr)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    for slot in [&from, &to] {
        if !matches!(slot, Slot::Position(_)) {
            return Err(Error::MismatchedKey(def.name.clone()));
        }
    }
    let mut removed = plan_remove(store, def.clone(), entity, attr, from)?;
    let Some(eff) = removed.pop() else {
        return Err(Error::SlotNotFound(def.name.clone()));
    };
    let region = eff.evictions[0].1.clone();
    let mut asserted = plan_assert(store, def, entity, attr, to, region)?;
    removed.push(eff);
    removed.append(&mut asserted);
    Ok(removed)
}
// todo: auto rebalance
fn plan_rebalance(store: &Store, entity: EntityId, attr: AttributeId) -> Result<Vec<Effective>> {
    let def = store.attribute_def(attr)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let mut rows = Vec::new();
    for guard in store
        .easvt
        .prefix(keys::easvt_entity_attr_prefix(entity, attr))
    {
        let (key, payload) = guard.into_inner()?;
        let (slot, consumed) =
            Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
        let old_txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
        let region = key[size_of::<EntityId>() + size_of::<AttributeId>() + consumed..].to_vec();
        rows.push((slot, region, old_txn));
    }
    if rows.len() < 2 {
        return Ok(vec![]);
    }
    let mut effective = Effective::new(def, entity, attr);
    effective.tag = Some("rebalance");
    let mut index = FractionalIndex::default();
    for (slot, region, old_txn) in rows {
        effective.evictions.push((slot, region.clone(), old_txn));
        effective
            .inserts
            .push((Slot::position(index.as_bytes().to_vec())?, region));
        index = FractionalIndex::new_after(&index);
    }
    Ok(vec![effective])
}

fn plan_retract_entity(
    store: &Store,
    entity: EntityId,
    visited: &mut HashSet<EntityId>,
) -> Result<Vec<Effective>> {
    if !visited.insert(entity) {
        return Ok(vec![]);
    }
    let mut effectives = Vec::new();
    let mut by_attr: BTreeMap<AttributeId, (AttributeDef, Evictions)> = BTreeMap::new();
    let mut components = Vec::new();
    for guard in store.easvt.prefix(keys::easvt_entity_prefix(entity)) {
        let (key, payload) = guard.into_inner()?;
        let attr = AttributeId::new(u16::from_be_bytes(key[8..10].try_into().unwrap()));
        let (slot, consumed) =
            Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
        let region = key[size_of::<EntityId>() + size_of::<AttributeId>() + consumed..].to_vec();
        let old_txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
        let def = store.attribute_def(attr)?;
        if def.is_component {
            components.push(EntityId::decode(&region[1..])?);
        }
        by_attr
            .entry(attr)
            .or_insert_with(|| (def, Vec::new()))
            .1
            .push((slot, region, old_txn));
    }
    for (attr, (def, evictions)) in by_attr {
        effectives.push(Effective {
            def,
            entity,
            attr,
            evictions,
            inserts: vec![],
            tag: None,
        });
    }
    for child in components {
        effectives.extend(plan_retract_entity(store, child, visited)?);
    }
    for guard in store.vaest.prefix(keys::vaest_target_prefix(entity)) {
        let (key, payload) = guard.into_inner()?;
        let (_, attr, source, slot) = keys::decode_vaest_key(&key)?;
        let old_txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
        let def = store.attribute_def(attr)?;
        let mut region = Vec::with_capacity(9);
        Value::Ref(entity).encode_key(&mut region);
        effectives.push(Effective {
            def,
            entity: source,
            attr,
            evictions: vec![(slot, region, old_txn)],
            inserts: vec![],
            tag: None,
        });
    }
    Ok(effectives)
}

fn check_component_owner(
    store: &Store,
    def: &AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    region: &[u8],
) -> Result<()> {
    if !def.is_component {
        return Ok(());
    }
    let target = EntityId::decode(&region[1..])?;
    for guard in store.vaest.prefix(keys::vaest_target_prefix(target)) {
        let (key, _) = guard.into_inner()?;
        let (_, other_attr, source, _) = keys::decode_vaest_key(&key)?;
        if source == entity && other_attr == attr {
            continue;
        }
        let other_def = store.attribute_def(other_attr)?;
        if other_def.is_component {
            return Err(Error::ComponentConflict(def.name.clone()));
        }
    }
    Ok(())
}

fn plan_append(
    store: &Store,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    region: Vec<u8>,
    after: bool,
    pending: &mut HashMap<(EntityId, AttributeId), (Slot, Slot)>,
) -> Result<Vec<Effective>> {
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let key = (entity, attr);
    let slot = match pending.get_mut(&key) {
        Some((first, last)) => {
            let boundary = if after { last.clone() } else { first.clone() };
            let slot = mint_position(Some(boundary), after)?;
            if after {
                *last = slot.clone();
            } else {
                *first = slot.clone();
            }
            slot
        }
        None => {
            let db_first = store.boundary_slot(entity, attr, true)?;
            let db_last = store.boundary_slot(entity, attr, false)?;
            let boundary = if after {
                db_last.clone()
            } else {
                db_first.clone()
            };
            let slot = mint_position(boundary, after)?;
            let first = if after {
                db_first.unwrap_or_else(|| slot.clone())
            } else {
                slot.clone()
            };
            let last = if after {
                slot.clone()
            } else {
                db_last.unwrap_or_else(|| slot.clone())
            };
            pending.insert(key, (first, last));
            slot
        }
    };
    plan_assert(store, def, entity, attr, slot, region)
}

impl Store {
    fn boundary_slot(
        &self,
        entity: EntityId,
        attr: AttributeId,
        first: bool,
    ) -> Result<Option<Slot>> {
        let mut iter = self
            .easvt
            .prefix(keys::easvt_entity_attr_prefix(entity, attr));
        let guard = if first { iter.next() } else { iter.next_back() };
        let Some(guard) = guard else {
            return Ok(None);
        };
        let (key, _) = guard.into_inner()?;
        let (slot, _) = Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
        Ok(Some(slot))
    }
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
    Slot::position(index.as_bytes().to_vec())
}

fn mint_position(boundary: Option<Slot>, after: bool) -> Result<Slot> {
    match (boundary, after) {
        (None, _) => mint_between(None, None),
        (Some(b), true) => mint_between(Some(&b), None),
        (Some(b), false) => mint_between(None, Some(&b)),
    }
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
            .entity_factoms(entity)
            .unwrap()
            .iter()
            .map(|f| f.value().clone())
            .collect()
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

    fn declare_map(store: &Store) -> AttributeId {
        store
            .declare_attribute(AttributeDef {
                key_type: Some(Type::String),
                ..def("person/scores", Type::Long, CollectionKind::Map)
            })
            .unwrap()
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
    fn prepend_goes_first() {
        let (_dir, store) = temp_store();
        let items = declare_list(&store);
        let e = store.new_entity().unwrap();
        store.push(e, items, &"b").unwrap().unwrap();
        store.prepend(e, items, &"a").unwrap().unwrap();
        assert_eq!(
            values(&store, e),
            vec![Value::String("a".into()), Value::String("b".into())]
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
        let factoms = store.entity_factoms(e).unwrap();
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
        let middle = store.entity_factoms(e).unwrap()[1].slot().clone();
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
        let middle = store.entity_factoms(e).unwrap()[1].slot().clone();
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
        let factoms = store.entity_factoms(e).unwrap();
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
        let second = store.slot_at(e, items, 1).unwrap().unwrap();
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
        assert_eq!(store.slot_at(e, items, 4).unwrap(), None);
        assert_eq!(store.slot_at(e, items, 99).unwrap(), None);
    }

    #[test]
    fn revert_push_removes_the_item() {
        let (_dir, store) = temp_store();
        let items = declare_list(&store);
        let e = store.new_entity().unwrap();
        let txn = store.push(e, items, &"a").unwrap().unwrap();
        store.revert_txn(txn).unwrap();
        assert!(store.entity_factoms(e).unwrap().is_empty());
    }

    #[test]
    fn revert_remove_restores_item_with_original_slot_and_value() {
        let (_dir, store) = temp_store();
        let items = declare_list(&store);
        let e = store.new_entity().unwrap();
        store.push(e, items, &"a").unwrap().unwrap();
        store.push(e, items, &"b").unwrap().unwrap();
        let slot = store.entity_factoms(e).unwrap()[0].slot().clone();
        let removed = store.remove(e, items, slot.clone()).unwrap().unwrap();
        store.revert_txn(removed).unwrap();
        let factoms = store.entity_factoms(e).unwrap();
        assert_eq!(factoms.len(), 2);
        assert_eq!(factoms[0].slot(), &slot);
        assert_eq!(factoms[0].value(), &Value::String("a".into()));
    }

    #[test]
    fn insert_after_anchors_mid_list() {
        let (_dir, store) = temp_store();
        let items = declare_list(&store);
        let e = store.new_entity().unwrap();
        store.push(e, items, &"a").unwrap().unwrap();
        store.push(e, items, &"c").unwrap().unwrap();
        let anchor = store.entity_factoms(e).unwrap()[0].slot().clone();
        store.insert_after(e, items, anchor, &"b").unwrap().unwrap();
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
    fn insert_before_anchors_first_slot() {
        let (_dir, store) = temp_store();
        let items = declare_list(&store);
        let e = store.new_entity().unwrap();
        store.push(e, items, &"b").unwrap().unwrap();
        let anchor = store.entity_factoms(e).unwrap()[0].slot().clone();
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
        let from = store.entity_factoms(e).unwrap()[0].slot().clone();
        let to = Slot::position(b"zz".to_vec()).unwrap();
        let txn = store
            .move_slot(e, items, from.clone(), to.clone())
            .unwrap()
            .unwrap();
        let factoms = store.entity_factoms(e).unwrap();
        assert_eq!(factoms.len(), 1);
        assert_eq!(factoms[0].slot(), &to);
        assert_eq!(factoms[0].value(), &Value::Ref(target));
        let mut expected = attr_bytes(items);
        from.encode_key(&mut expected);
        to.encode_key(&mut expected);
        let tx_factoms = store.entity_factoms(txn.entity_id()).unwrap();
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
        let anchor = store.entity_factoms(e).unwrap()[1].slot().clone();
        store
            .insert_before(e, items, anchor.clone(), &"b")
            .unwrap()
            .unwrap();
        let fractional = store.entity_factoms(e).unwrap()[1].slot().clone();
        assert_ne!(fractional, anchor);

        let txn = store.rebalance(e, items).unwrap().unwrap();
        let factoms = store.entity_factoms(e).unwrap();
        assert_eq!(factoms.len(), 3);
        assert_eq!(
            factoms
                .iter()
                .map(|f| f.value().clone())
                .collect::<Vec<_>>(),
            vec![
                Value::String("a".into()),
                Value::String("b".into()),
                Value::String("c".into())
            ]
        );
        let mut index = FractionalIndex::default();
        for factom in &factoms {
            assert_eq!(
                factom.slot(),
                &Slot::position(index.as_bytes().to_vec()).unwrap()
            );
            index = FractionalIndex::new_after(&index);
        }
        assert_ne!(factoms[1].slot(), &fractional);
        let tx_factoms = store.entity_factoms(txn.entity_id()).unwrap();
        assert!(
            tx_factoms.iter().any(|f| f.attribute() == TX_KIND_ATTR
                && f.value() == &Value::String("rebalance".into()))
        );
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
        assert!(store.entity_factoms(e1).unwrap().is_empty());
        let e2_factoms = store.entity_factoms(e2).unwrap();
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
        assert!(store.entity_factoms(parent).unwrap().is_empty());
        assert!(store.entity_factoms(kid).unwrap().is_empty());
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
        assert!(store.entity_factoms(a).unwrap().is_empty());
        assert!(store.entity_factoms(b).unwrap().is_empty());
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
}
