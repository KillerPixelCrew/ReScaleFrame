// SPDX-License-Identifier: MIT
use super::{
    FOLIAGE_MAX_GROUPS, FoliageConstants, FoliageItem, FrameIdentity, Rotation, TreeRecord, finite,
    lane,
};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::mem::MaybeUninit;
use std::ptr;

const POISON: f32 = f32::from_bits(0x7fc5_a5a5);
// Preserve the previous table's 90% cap on 2^19 slots.
const MAX_ITEMS: usize = ((1 << 19) * 9) / 10;
pub(super) type Rows = [[f32; 16]; FOLIAGE_MAX_GROUPS];
type Key = [u32; 5];

#[repr(C)]
pub(super) struct DetailRemap {
    pub(super) live: u32,
    pub(super) matched: u32,
    pub(super) unmatched: u32,
    pub(super) conflicts: u32,
    pub(super) route_found: bool,
    pub(super) previous: FoliageConstants,
    pub(super) items: [FoliageItem; FOLIAGE_MAX_GROUPS],
}

const _: () = {
    assert!(size_of::<TreeRecord>() == 104);
    assert!(size_of::<Rows>() == 3904);
    assert!(size_of::<DetailRemap>() == 4704);
    assert!(std::mem::offset_of!(DetailRemap, previous) == 20);
    assert!(std::mem::offset_of!(DetailRemap, items) == 68);
};

impl DetailRemap {
    pub(super) fn empty() -> Self {
        Self {
            live: 0,
            matched: 0,
            unmatched: 0,
            conflicts: 0,
            route_found: false,
            previous: FoliageConstants::default(),
            items: [FoliageItem::default(); FOLIAGE_MAX_GROUPS],
        }
    }
}

fn live_groups(rows: &Rows) -> &[[f32; 16]] {
    // The engine fills leading groups only. The poison marker is a NaN.
    let count = rows.iter().take_while(|group| finite(*group)).count();
    &rows[..count]
}

fn group_scale(group: &[f32; 16]) -> f32 {
    let [x, y, z] = [group[0], group[1], group[2]].map(f64::from);
    (x * x + y * y + z * z).sqrt() as f32
}

fn key(group: &[f32; 16]) -> Key {
    // World translation, then the engine's per-item sun and hemisphere values.
    [3, 7, 11, 12, 15].map(|lane| group[lane].to_bits())
}

fn mark_conflict<T: PartialEq>(slot: &mut Option<T>, next: &T) -> bool {
    if slot.as_ref().is_some_and(|old| old != next) {
        *slot = None;
        true
    } else {
        false
    }
}

#[derive(PartialEq)]
struct DetailItem {
    route: u64,
    bits: [u32; 16],
}

#[derive(Default)]
struct Frame {
    id: FrameIdentity,
    // A present key with None denotes a same-frame conflict.
    trees: HashMap<u64, Option<TreeRecord>>,
    routes: HashMap<u64, Option<FoliageConstants>>,
    items: HashMap<Key, Option<DetailItem>>,
}

#[derive(Default)]
struct View {
    current: Option<Frame>,
    previous: Option<Frame>,
}

impl View {
    fn begin(&mut self, id: FrameIdentity) -> bool {
        let mut new = match id.rotation(self.current.as_ref().map(|frame| frame.id)) {
            Rotation::Reuse => return self.previous.is_some(),
            Rotation::Advance => {
                let reusable = self.previous.take();
                self.previous = self.current.take();
                reusable.unwrap_or_default()
            }
            Rotation::Reset => {
                self.previous = None;
                self.current.take().unwrap_or_default()
            }
        };
        new.id = id;
        new.trees.clear();
        new.routes.clear();
        new.items.clear();
        self.current = Some(new);
        self.previous.is_some()
    }
}

#[derive(Default)]
pub(super) struct History {
    views: [View; 2],
    conflicts: u64,
    overflows: u64,
}

impl History {
    pub(super) fn begin(&mut self, view: u8, id: FrameIdentity) -> bool {
        self.views[lane(view)].begin(id)
    }

    fn tree(&mut self, view: u8, visual: u64, record: &TreeRecord) -> Option<&TreeRecord> {
        let slot = &mut self.views[lane(view)];
        if visual == 0 {
            return None;
        }
        match slot.current.as_mut()?.trees.entry(visual) {
            Entry::Occupied(mut entry) => {
                self.conflicts += u64::from(mark_conflict(entry.get_mut(), record));
                if entry.get().is_none() {
                    return None;
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(Some(*record));
            }
        }
        slot.previous.as_ref()?.trees.get(&visual)?.as_ref()
    }

    fn detail(
        &mut self,
        view: u8,
        route: u64,
        constants: &FoliageConstants,
        rows: &Rows,
        out: &mut DetailRemap,
    ) {
        let slot = &mut self.views[lane(view)];
        let Some(current) = &mut slot.current else {
            return;
        };
        let live = live_groups(rows);
        out.live = live.len() as u32;
        let current_route = current.routes.entry(route).or_insert(Some(*constants));
        self.conflicts += u64::from(mark_conflict(current_route, constants));

        // Record the whole batch before choosing any previous rows. Both copies
        // of a contradictory item in one batch must be invalid.
        let mut keys = [[0; 5]; FOLIAGE_MAX_GROUPS];
        for (group, item_key) in live.iter().zip(&mut keys) {
            *item_key = key(group);
            let item = DetailItem {
                route,
                bits: group.map(f32::to_bits),
            };
            if let Some(old) = current.items.get_mut(item_key) {
                let conflict = mark_conflict(old, &item);
                out.conflicts += u32::from(conflict);
                self.conflicts += u64::from(conflict);
            } else if current.items.len() < MAX_ITEMS && current.items.try_reserve(1).is_ok() {
                current.items.insert(*item_key, Some(item));
            } else {
                self.overflows += 1;
            }
        }
        out.unmatched = out.live;
        let Some(previous) = &slot.previous else {
            return;
        };
        let Some(Some(previous_route)) = previous.routes.get(&route) else {
            return;
        };
        if current_route.is_none() {
            return;
        }
        out.route_found = true;
        out.previous = *previous_route;
        for (item_key, mapped) in keys.iter().zip(&mut out.items).take(live.len()) {
            if !matches!(current.items.get(item_key), Some(Some(_))) {
                continue;
            }
            let Some(Some(item)) = previous.items.get(item_key) else {
                continue;
            };
            if item.route != route {
                continue;
            }
            let rows = item.bits.map(f32::from_bits);
            let scale = group_scale(&rows);
            if !scale.is_finite() || scale <= 0.0 {
                continue;
            }
            *mapped = FoliageItem {
                valid: true,
                scale,
                fade: 1.0,
                rows,
            };
            out.matched += 1;
            out.unmatched -= 1;
        }
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_active(history: Option<&History>, view: u8) -> bool {
    history.is_some_and(|h| h.views[lane(view)].current.is_some())
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_reset(history: Option<&mut History>) {
    if let Some(history) = history {
        history.views = Default::default();
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_tree(
    history: Option<&mut History>,
    view: u8,
    visual: u64,
    record: &TreeRecord,
) -> *const TreeRecord {
    history
        .and_then(|h| h.tree(view, visual, record))
        .map_or(ptr::null(), ptr::from_ref)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_detail(
    history: Option<&mut History>,
    view: u8,
    route: u64,
    constants: &FoliageConstants,
    rows: &Rows,
    out: &mut MaybeUninit<DetailRemap>,
) {
    let out = out.write(DetailRemap::empty());
    if let Some(history) = history {
        history.detail(view, route, constants, rows, out);
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_conflicts(history: Option<&History>) -> u64 {
    history.map_or(0, |h| h.conflicts)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_overflows(history: Option<&History>) -> u64 {
    history.map_or(0, |h| h.overflows)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_recorded(
    history: Option<&History>,
    view: u8,
) -> usize {
    history
        .and_then(|h| h.views[lane(view)].current.as_ref())
        .map_or(0, |frame| frame.items.len())
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_count(rows: &Rows) -> u32 {
    live_groups(rows).len() as u32
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_poison(rows: &mut MaybeUninit<Rows>) {
    rows.write([[POISON; 16]; FOLIAGE_MAX_GROUPS]);
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_foliage_scale(group: &[f32; 16]) -> f32 {
    group_scale(group)
}
