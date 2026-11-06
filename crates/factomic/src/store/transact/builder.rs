use std::collections::{BTreeMap, HashSet};

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
        let mut effects = Effects::new();
        for op in ops {
            plan_op(store, op, &mut effects)?;
        }

        if effects
            .values()
            .all(|eff| eff.evictions.is_empty() && eff.inserts.is_empty())
        {
            return Ok(None);
        }

        let txn = store.alloc_txn_id()?;
        let mut batch = OwnedWriteBatch::with_capacity(store.db.clone(), 24);

        for ((_, attr), eff) in &effects {
            for (to_slot, region) in &eff.inserts {
                if region[0] != TAG_REF {
                    continue;
                }
                for (from_slot, old_region, _) in &eff.evictions {
                    if old_region == region {
                        store.record_tx_move(&mut batch, txn, *attr, from_slot, to_slot);
                    }
                }
            }
        }

        for eff in effects.values() {
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

    fn evict(&mut self, slot: Slot, region: Vec<u8>, txn: Option<TxnId>) {
        match txn {
            Some(old_txn) => self.evictions.push((slot, region, old_txn)),
            None => {
                if let Some(pos) = self
                    .inserts
                    .iter()
                    .position(|(s, r)| *s == slot && *r == region)
                {
                    self.inserts.remove(pos);
                }
            }
        }
    }
}

type Effects = BTreeMap<(EntityId, AttributeId), Effective>;

struct VisibleDatom {
    slot: Slot,
    region: Vec<u8>,
    txn: Option<TxnId>,
}

// easvt
fn order_key(slot: &Slot, region: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(slot.encoded_len() + region.len());
    slot.encode_key(&mut key);
    key.extend_from_slice(region);
    key
}

fn visible_datoms(
    store: &Store,
    effects: &Effects,
    entity: EntityId,
    attr: AttributeId,
) -> Result<Vec<VisibleDatom>> {
    let pending = effects.get(&(entity, attr));
    let mut datoms = Vec::new();
    for guard in store
        .easvt
        .prefix(keys::easvt_entity_attr_prefix(entity, attr))
    {
        let (key, payload) = guard.into_inner()?;
        let (slot, consumed) =
            Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
        let region = key[size_of::<EntityId>() + size_of::<AttributeId>() + consumed..].to_vec();
        if let Some(eff) = pending
            && eff
                .evictions
                .iter()
                .any(|(s, r, _)| *s == slot && *r == region)
        {
            continue;
        }
        let txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
        datoms.push(VisibleDatom {
            slot,
            region,
            txn: Some(txn),
        });
    }
    if let Some(eff) = pending {
        for (slot, region) in &eff.inserts {
            datoms.push(VisibleDatom {
                slot: slot.clone(),
                region: region.clone(),
                txn: None,
            });
        }
    }
    datoms.sort_by_key(|d| order_key(&d.slot, &d.region));
    Ok(datoms)
}

fn visible_txn(
    store: &Store,
    effects: &Effects,
    entity: EntityId,
    attr: AttributeId,
    slot: &Slot,
    region: &[u8],
) -> Result<Option<Option<TxnId>>> {
    if let Some(eff) = effects.get(&(entity, attr)) {
        if eff.inserts.iter().any(|(s, r)| s == slot && r == region) {
            return Ok(Some(None));
        }
        if eff
            .evictions
            .iter()
            .any(|(s, r, _)| s == slot && r == region)
        {
            return Ok(None);
        }
    }
    let key = keys::easvt_key(entity, attr, slot, region);
    let Some(payload) = store.easvt.get(&key)? else {
        return Ok(None);
    };
    Ok(Some(Some(TxnId::with_value(u64::from_be_bytes(
        payload[..8].try_into().unwrap(),
    )))))
}

fn plan_op(store: &Store, op: PlannedOp, effects: &mut Effects) -> Result<()> {
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
                Op::Assert => plan_assert(store, effects, def, entity, attr, slot, region),
                Op::Retract => plan_retract(store, effects, def, entity, attr, slot, region),
            }
        }
        PlannedOp::Push {
            entity,
            attr,
            ty,
            region,
        } => plan_append(store, effects, entity, attr, ty, region, true),
        PlannedOp::Prepend {
            entity,
            attr,
            ty,
            region,
        } => plan_append(store, effects, entity, attr, ty, region, false),
        PlannedOp::RemoveSlot { entity, attr, slot } => {
            let def = store.attribute_def(attr)?;
            if !matches!(def.kind, CollectionKind::List | CollectionKind::Map) {
                return Err(Error::MismatchedKey(def.name.clone()));
            }
            check_key(&def, &slot)?;
            plan_remove(store, effects, def, entity, attr, slot)
        }
        PlannedOp::InsertAt {
            entity,
            attr,
            ty,
            region,
            anchor,
            before,
        } => plan_insert_at(store, effects, entity, attr, ty, region, anchor, before),
        PlannedOp::MoveSlot {
            entity,
            attr,
            from,
            to,
        } => plan_move(store, effects, entity, attr, from, to),
        PlannedOp::Rebalance { entity, attr } => plan_rebalance(store, effects, entity, attr),
        PlannedOp::RetractEntity { entity } => {
            let mut visited = HashSet::new();
            plan_retract_entity(store, effects, entity, &mut visited)
        }
    }
}

fn plan_assert(
    store: &Store,
    effects: &mut Effects,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
    region: Vec<u8>,
) -> Result<()> {
    check_component_owner(store, effects, &def, entity, attr, &region)?;
    match def.kind {
        CollectionKind::One => {
            let visible = visible_datoms(store, effects, entity, attr)?;
            if visible.iter().any(|d| d.slot == slot && d.region == region) {
                return Ok(());
            }
            let eff = effects
                .entry((entity, attr))
                .or_insert_with(|| Effective::new(def, entity, attr));
            for d in visible {
                eff.evict(d.slot, d.region, d.txn);
            }
            eff.inserts.push((slot, region));
        }
        CollectionKind::Many => {
            if visible_txn(store, effects, entity, attr, &slot, &region)?.is_some() {
                return Ok(());
            }
            effects
                .entry((entity, attr))
                .or_insert_with(|| Effective::new(def, entity, attr))
                .inserts
                .push((slot, region));
        }
        CollectionKind::List | CollectionKind::Map => {
            let visible = visible_datoms(store, effects, entity, attr)?;
            if visible.iter().any(|d| d.slot == slot && d.region == region) {
                return Ok(());
            }
            let eff = effects
                .entry((entity, attr))
                .or_insert_with(|| Effective::new(def, entity, attr));
            for d in visible.into_iter().filter(|d| d.slot == slot) {
                eff.evict(d.slot, d.region, d.txn);
            }
            eff.inserts.push((slot, region));
        }
    }
    Ok(())
}

fn plan_retract(
    store: &Store,
    effects: &mut Effects,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
    region: Vec<u8>,
) -> Result<()> {
    let Some(txn) = visible_txn(store, effects, entity, attr, &slot, &region)? else {
        return Ok(());
    };
    effects
        .entry((entity, attr))
        .or_insert_with(|| Effective::new(def, entity, attr))
        .evict(slot, region, txn);
    Ok(())
}

fn plan_remove(
    store: &Store,
    effects: &mut Effects,
    def: AttributeDef,
    entity: EntityId,
    attr: AttributeId,
    slot: Slot,
) -> Result<()> {
    let visible = visible_datoms(store, effects, entity, attr)?;
    let Some(d) = visible.into_iter().find(|d| d.slot == slot) else {
        return Ok(());
    };
    effects
        .entry((entity, attr))
        .or_insert_with(|| Effective::new(def, entity, attr))
        .evict(d.slot, d.region, d.txn);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn plan_insert_at(
    store: &Store,
    effects: &mut Effects,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    region: Vec<u8>,
    anchor: Slot,
    before: bool,
) -> Result<()> {
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    if !matches!(anchor, Slot::Position(_)) {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_datoms(store, effects, entity, attr)?;
    let mut anchor_found = false;
    let mut neighbor = None;
    for d in &visible {
        if before {
            if d.slot == anchor {
                anchor_found = true;
                break;
            }
            neighbor = Some(d.slot.clone());
        } else {
            if anchor_found {
                neighbor = Some(d.slot.clone());
                break;
            }
            if d.slot == anchor {
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
    plan_assert(store, effects, def, entity, attr, slot, region)
}

fn plan_move(
    store: &Store,
    effects: &mut Effects,
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
    let visible = visible_datoms(store, effects, entity, attr)?;
    let Some(d) = visible.into_iter().find(|d| d.slot == from) else {
        return Err(Error::SlotNotFound(def.name.clone()));
    };
    let region = d.region.clone();
    plan_remove(store, effects, def.clone(), entity, attr, from)?;
    plan_assert(store, effects, def, entity, attr, to, region)
}

// todo: auto rebalance
fn plan_rebalance(
    store: &Store,
    effects: &mut Effects,
    entity: EntityId,
    attr: AttributeId,
) -> Result<()> {
    let def = store.attribute_def(attr)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_datoms(store, effects, entity, attr)?;
    if visible.len() < 2 {
        return Ok(());
    }
    let eff = effects
        .entry((entity, attr))
        .or_insert_with(|| Effective::new(def, entity, attr));
    eff.tag = Some("rebalance");
    let mut index = FractionalIndex::default();
    for d in visible {
        let region = d.region.clone();
        eff.evict(d.slot, d.region, d.txn);
        eff.inserts
            .push((Slot::position(index.as_bytes().to_vec())?, region));
        index = FractionalIndex::new_after(&index);
    }
    Ok(())
}

fn plan_retract_entity(
    store: &Store,
    effects: &mut Effects,
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
        let (slot, consumed) =
            Slot::decode_key(&key[size_of::<EntityId>() + size_of::<AttributeId>()..])?;
        let region = key[size_of::<EntityId>() + size_of::<AttributeId>() + consumed..].to_vec();
        if let Some(eff) = effects.get(&(entity, attr))
            && eff
                .evictions
                .iter()
                .any(|(s, r, _)| *s == slot && *r == region)
        {
            continue;
        }
        let old_txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
        let def = store.attribute_def(attr)?;
        if def.is_component {
            components.push(EntityId::decode(&region[1..])?);
        }
        effects
            .entry((entity, attr))
            .or_insert_with(|| Effective::new(def, entity, attr))
            .evictions
            .push((slot, region, old_txn));
    }
    // component links still cascade?
    let pending_attrs: Vec<AttributeId> = effects
        .range((entity, AttributeId::new(u16::MIN))..=(entity, AttributeId::new(u16::MAX)))
        .map(|((_, attr), _)| *attr)
        .collect();
    for attr in pending_attrs {
        let eff = effects.get_mut(&(entity, attr)).unwrap();
        if eff.def.is_component {
            for (_, region) in &eff.inserts {
                components.push(EntityId::decode(&region[1..])?);
            }
        }
        eff.inserts.clear();
    }
    for child in components {
        plan_retract_entity(store, effects, child, visited)?;
    }
    // todo: refs asserted earlier same transaction not visible
    for guard in store.vaest.prefix(keys::vaest_target_prefix(entity)) {
        let (key, payload) = guard.into_inner()?;
        let (_, attr, source, slot) = keys::decode_vaest_key(&key)?;
        let mut region = Vec::with_capacity(9);
        Value::Ref(entity).encode_key(&mut region);
        if let Some(eff) = effects.get(&(source, attr))
            && eff
                .evictions
                .iter()
                .any(|(s, r, _)| *s == slot && *r == region)
        {
            continue;
        }
        let old_txn = TxnId::with_value(u64::from_be_bytes(payload[..8].try_into().unwrap()));
        let def = store.attribute_def(attr)?;
        effects
            .entry((source, attr))
            .or_insert_with(|| Effective::new(def, source, attr))
            .evictions
            .push((slot, region, old_txn));
    }
    Ok(())
}

fn check_component_owner(
    store: &Store,
    effects: &Effects,
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
        let (_, other_attr, source, other_slot) = keys::decode_vaest_key(&key)?;
        if source == entity && other_attr == attr {
            continue;
        }
        //todo: refs asserted earlier same transaction not visible
        if let Some(eff) = effects.get(&(source, other_attr)) {
            let mut other_region = Vec::with_capacity(9);
            Value::Ref(target).encode_key(&mut other_region);
            if eff
                .evictions
                .iter()
                .any(|(s, r, _)| *s == other_slot && *r == other_region)
            {
                continue;
            }
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
    effects: &mut Effects,
    entity: EntityId,
    attr: AttributeId,
    ty: Type,
    region: Vec<u8>,
    after: bool,
) -> Result<()> {
    let def = check_type(store, attr, ty)?;
    if def.kind != CollectionKind::List {
        return Err(Error::MismatchedKey(def.name.clone()));
    }
    let visible = visible_datoms(store, effects, entity, attr)?;
    let boundary = if after {
        visible.last().map(|d| d.slot.clone())
    } else {
        visible.first().map(|d| d.slot.clone())
    };
    let slot = mint_position(boundary, after)?;
    plan_assert(store, effects, def, entity, attr, slot, region)
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
        let factoms = store.entity_factoms(e).unwrap();
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
        let factoms = store.entity_factoms(e).unwrap();
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
        assert!(store.entity_factoms(e).unwrap().is_empty());
        assert!(store.entity_history(e).unwrap().is_empty());
    }

    #[test]
    fn same_txn_retract_then_assert_keeps_datom() {
        let (_dir, store) = temp_store();
        let name = store
            .declare_attribute(def("person/name", Type::String, CollectionKind::One))
            .unwrap();
        let e = store.new_entity().unwrap();
        store.assert(e, name, &"a").unwrap().unwrap();
        store
            .transact()
            .retract(e, name, &"a")
            .assert(e, name, &"a")
            .commit()
            .unwrap()
            .unwrap();
        let factoms = store.entity_factoms(e).unwrap();
        assert_eq!(factoms.len(), 1);
        assert_eq!(factoms[0].value(), &Value::String("a".into()));
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
        assert!(store.entity_factoms(e).unwrap().is_empty());
    }

    #[test]
    fn same_txn_insert_after_pending_anchor() {
        let (_dir, store) = temp_store();
        let items = declare_list(&store);
        let e = store.new_entity().unwrap();
        let k = Slot::position(FractionalIndex::default().as_bytes().to_vec()).unwrap();
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
        let factoms = store.entity_factoms(e).unwrap();
        assert_eq!(factoms.len(), 1);
        assert_eq!(factoms[0].slot(), &to);
        assert_eq!(factoms[0].value(), &Value::String("x".into()));
    }

    #[test]
    fn same_txn_rebalance_includes_pending_inserts() {
        let (_dir, store) = temp_store();
        let items = declare_list(&store);
        let e = store.new_entity().unwrap();
        store.push(e, items, &"a").unwrap().unwrap();
        store
            .transact()
            .push(e, items, &"b")
            .rebalance(e, items)
            .commit()
            .unwrap()
            .unwrap();
        let factoms = store.entity_factoms(e).unwrap();
        assert_eq!(
            values(&store, e),
            vec![Value::String("a".into()), Value::String("b".into())]
        );
        let mut index = FractionalIndex::default();
        for factom in &factoms {
            assert_eq!(
                factom.slot(),
                &Slot::position(index.as_bytes().to_vec()).unwrap()
            );
            index = FractionalIndex::new_after(&index);
        }
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
        let factoms = store.entity_factoms(o2).unwrap();
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
        assert!(store.entity_factoms(e).unwrap().is_empty());
    }
}
