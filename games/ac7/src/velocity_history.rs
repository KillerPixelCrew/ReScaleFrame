// SPDX-License-Identifier: MIT
//! The translucent velocity pass's logical history: model and view identity, frame rotation,
//! history validity, record accounting, the view's written and commit state, and epoch
//! retirement. velocity_pass.cpp keeps the D3D11 buffers and textures at the slots assigned here,
//! performs every allocation, clear, rehash and copy these decisions ask for, and releases a
//! slot's resources when frame end retires it.

use std::collections::HashMap;
use std::ffi::c_void;
use std::hash::Hash;
use std::mem::offset_of;
use std::sync::{LazyLock, Mutex, MutexGuard};

const NO_SLOT: u32 = u32::MAX;
const NO_FRAME: u32 = u32::MAX;
const FIRST_EPOCH: u64 = 1;
// An entry survives the frame end it was used in and the one after.
const RETAINED_EPOCHS: u64 = 2;
// The smallest history, capture and zero-source buffer, in records. velocity_stream.rs shares it.
pub(crate) const MIN_CAPACITY: u32 = 1024;
// Records are 64 bytes and the hash table keeps twice the records. This also bounds one draw's
// stream-output records in velocity_stream.rs.
pub(crate) const RECORD_LIMIT: u32 = u32::MAX / 128;

// View actions.
const VIEW_UNAVAILABLE: u32 = 0;
const VIEW_SAME_FRAME: u32 = 1;
const VIEW_REUSE: u32 = 2;
const VIEW_ALLOCATE: u32 = 3;
pub(crate) const VIEW_COMMIT: u32 = 4;

// Retirement kinds.
const RETIRE_MODEL: u32 = 0;
const RETIRE_VIEW: u32 = 1;

/// A draw's model identity and its record count for this callback. `particle` drops the vertex
/// factory and draw offsets from the identity; `reset` is the scope's history reset.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // constructed by velocity_pass.cpp
struct ModelDraw {
    session_id: u64,
    history_key: u64,
    view_key: u64,
    primitive_key: u64,
    material_key: u64,
    vertex_factory_key: u64,
    vertex_hash: u32,
    geometry_hash: u32,
    start: u32,
    base_vertex: i32,
    particle: u32,
    frame: u32,
    reset: u32,
    records: u32,
}

/// `rotate`: swap previous and current and clear the new current. `capacity`: the current
/// buffer must hold at least this many records. Both apply only while `slot` is valid.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModelDecision {
    slot: u32,
    rotate: u32,
    usable: u32,
    capacity: u32,
}

/// A view's identity at a draw (its native depth extent) or at a commit (the motion target's
/// pass, extent unused).
#[repr(C)]
#[allow(dead_code)] // constructed by velocity_pass.cpp and velocity_commit.rs
pub(crate) struct ViewDraw {
    pub(crate) session_id: u64,
    pub(crate) history_key: u64,
    pub(crate) view_key: u64,
    pub(crate) family_key: u64,
    pub(crate) frame: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewDecision {
    pub(crate) slot: u32,
    pub(crate) action: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

const _: () = {
    assert!(size_of::<ModelDraw>() == 80 && offset_of!(ModelDraw, vertex_hash) == 48);
    assert!(offset_of!(ModelDraw, records) == 76);
    assert!(size_of::<ModelDecision>() == 16);
    assert!(size_of::<ViewDraw>() == 48 && offset_of!(ViewDraw, frame) == 32);
    assert!(size_of::<ViewDecision>() == 16);
};

const UNAVAILABLE_MODEL: ModelDecision = ModelDecision {
    slot: NO_SLOT,
    rotate: 0,
    usable: 0,
    capacity: 0,
};

pub(crate) const UNAVAILABLE_VIEW: ViewDecision = ViewDecision {
    slot: NO_SLOT,
    action: VIEW_UNAVAILABLE,
    width: 0,
    height: 0,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ModelKey {
    view: ViewKey,
    primitive: u64,
    material: u64,
    vertex_hash: u32,
    vertex_factory: u64,
    start: u32,
    base_vertex: i32,
    geometry_hash: u32,
}

/// The session and the history key, or the view key when the scope has none.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct ViewKey(u64, u64);

impl ViewKey {
    fn of(session_id: u64, history_key: u64, view_key: u64) -> Self {
        Self(
            session_id,
            if history_key != 0 {
                history_key
            } else {
                view_key
            },
        )
    }
}

struct Model {
    frame: u32,
    records: u32,
    epoch: u64,
    usable: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            frame: NO_FRAME,
            records: 0,
            epoch: 0,
            usable: false,
        }
    }
}

struct View {
    frame: u32,
    width: u32,
    height: u32,
    family: u64,
    view: u64,
    epoch: u64,
    written: bool,
    // Every native target of the slot exists at this extent.
    allocated: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            frame: NO_FRAME,
            width: 0,
            height: 0,
            family: 0,
            view: 0,
            epoch: 0,
            written: false,
            allocated: false,
        }
    }
}

trait Aged {
    fn epoch(&self) -> u64;
}

impl Aged for Model {
    fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Aged for View {
    fn epoch(&self) -> u64 {
        self.epoch
    }
}

/// Keyed entries at dense native slots. A retired slot is reused only after frame end has
/// reported it, so its native resources are already released.
struct Table<K, V> {
    slots: HashMap<K, u32>,
    entries: Vec<Option<V>>,
    free: Vec<u32>,
}

impl<K, V> Default for Table<K, V> {
    fn default() -> Self {
        Self {
            slots: HashMap::new(),
            entries: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<K: Copy + Eq + Hash, V: Default + Aged> Table<K, V> {
    fn find(&self, key: &K) -> Option<(u32, &V)> {
        let slot = *self.slots.get(key)?;
        Some((slot, self.entries.get(slot as usize)?.as_ref()?))
    }

    fn find_or_insert(&mut self, key: K) -> Option<(u32, &mut V)> {
        let slot = match self.slots.get(&key) {
            Some(&slot) => slot,
            None => {
                self.slots.try_reserve(1).ok()?;
                let slot = match self.free.pop() {
                    Some(slot) => slot,
                    None => {
                        let slot = u32::try_from(self.entries.len())
                            .ok()
                            .filter(|&slot| slot != NO_SLOT)?;
                        // Keep room to free every slot without allocating at frame end.
                        self.entries.try_reserve(1).ok()?;
                        self.free.try_reserve(self.entries.len() + 1).ok()?;
                        self.entries.push(None);
                        slot
                    }
                };
                self.entries[slot as usize] = Some(V::default());
                self.slots.insert(key, slot);
                slot
            }
        };
        Some((slot, self.entries.get_mut(slot as usize)?.as_mut()?))
    }

    fn get_mut(&mut self, slot: u32) -> Option<&mut V> {
        self.entries.get_mut(slot as usize)?.as_mut()
    }

    fn retire(&mut self, epoch: u64, kind: u32, retired: &mut Vec<(u32, u32)>) {
        let (entries, free) = (&mut self.entries, &mut self.free);
        self.slots.retain(|_, &mut slot| {
            let entry = &mut entries[slot as usize];
            if entry
                .as_ref()
                .is_some_and(|value| value.epoch() + RETAINED_EPOCHS >= epoch)
            {
                return true;
            }
            *entry = None;
            free.push(slot);
            retired.push((kind, slot));
            false
        });
    }

    fn len(&self) -> usize {
        self.slots.len()
    }
}

struct History {
    models: Table<ModelKey, Model>,
    views: Table<ViewKey, View>,
    epoch: u64,
}

impl Default for History {
    fn default() -> Self {
        Self {
            models: Table::default(),
            views: Table::default(),
            epoch: FIRST_EPOCH,
        }
    }
}

impl History {
    /// Rotation and validity happen at the first draw of a native frame, and the epoch is
    /// refreshed even when the record count is then refused.
    fn begin_model(&mut self, draw: &ModelDraw) -> (bool, ModelDecision) {
        let particle = draw.particle != 0;
        let key = ModelKey {
            view: ViewKey::of(draw.session_id, draw.history_key, draw.view_key),
            primitive: draw.primitive_key,
            material: draw.material_key,
            vertex_hash: draw.vertex_hash,
            vertex_factory: if particle { 0 } else { draw.vertex_factory_key },
            start: if particle { 0 } else { draw.start },
            base_vertex: if particle { 0 } else { draw.base_vertex },
            geometry_hash: draw.geometry_hash,
        };
        let epoch = self.epoch;
        let Some((slot, model)) = self.models.find_or_insert(key) else {
            return (false, UNAVAILABLE_MODEL);
        };
        let mut decision = ModelDecision {
            slot,
            ..UNAVAILABLE_MODEL
        };
        if model.frame != draw.frame {
            // History is valid only for the next native frame without a reset.
            model.usable = model.frame != NO_FRAME
                && model.frame.wrapping_add(1) == draw.frame
                && draw.reset == 0;
            model.frame = draw.frame;
            model.records = 0;
            decision.rotate = 1;
        }
        model.epoch = epoch;
        decision.usable = u32::from(model.usable);
        let count = draw.records;
        if count > u32::MAX - model.records || model.records + count > RECORD_LIMIT {
            return (false, decision);
        }
        model.records += count;
        let mut capacity = MIN_CAPACITY;
        while capacity < model.records * 2 {
            if capacity > u32::MAX / 2 {
                return (false, decision);
            }
            capacity *= 2;
        }
        decision.capacity = capacity;
        (true, decision)
    }

    /// `width` and `height` are the native depth extent the draw already matched.
    fn begin_view(&mut self, draw: &ViewDraw) -> ViewDecision {
        let key = ViewKey::of(draw.session_id, draw.history_key, draw.view_key);
        let Some((slot, view)) = self.views.find_or_insert(key) else {
            return UNAVAILABLE_VIEW;
        };
        let action = if view.frame == draw.frame {
            VIEW_SAME_FRAME
        } else if !view.allocated || view.width != draw.width || view.height != draw.height {
            // The native slot is discarded; it stays unready until every target exists.
            *view = View {
                width: draw.width,
                height: draw.height,
                ..View::default()
            };
            VIEW_ALLOCATE
        } else {
            VIEW_REUSE
        };
        ViewDecision {
            slot,
            action,
            width: view.width,
            height: view.height,
        }
    }

    /// The slot's targets exist and the draw's depth was copied into them.
    fn ready_view(&mut self, slot: u32, draw: &ViewDraw) {
        let epoch = self.epoch;
        if let Some(view) = self.views.get_mut(slot) {
            view.allocated = true;
            view.frame = draw.frame;
            view.family = draw.family_key;
            view.view = draw.view_key;
            view.epoch = epoch;
            view.written = false;
        }
    }

    fn mark_written(&mut self, slot: u32, written: bool) {
        if let Some(view) = self.views.get_mut(slot) {
            view.written = written;
        }
    }

    /// A commit needs translucent motion written for this pass's frame, view and family.
    fn commit_view(&self, pass: &ViewDraw) -> ViewDecision {
        let key = ViewKey::of(pass.session_id, pass.history_key, pass.view_key);
        match self.views.find(&key) {
            Some((slot, view))
                if view.written
                    && view.frame == pass.frame
                    && view.view == pass.view_key
                    && view.family == pass.family_key =>
            {
                ViewDecision {
                    slot,
                    action: VIEW_COMMIT,
                    width: view.width,
                    height: view.height,
                }
            }
            _ => UNAVAILABLE_VIEW,
        }
    }

    fn end_frame(&mut self, retired: &mut Vec<(u32, u32)>) {
        self.epoch += 1;
        let epoch = self.epoch;
        self.models.retire(epoch, RETIRE_MODEL, retired);
        self.views.retire(epoch, RETIRE_VIEW, retired);
    }
}

static HISTORY: LazyLock<Mutex<History>> = LazyLock::new(Mutex::default);

fn lock() -> MutexGuard<'static, History> {
    HISTORY.lock().unwrap_or_else(|error| error.into_inner())
}

type RetireSlot = unsafe extern "C" fn(*mut c_void, u32, u32);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_model_begin(
    draw: *const ModelDraw,
    decision: *mut ModelDecision,
) -> i32 {
    // SAFETY: The draw callback lends both for this call.
    let (Some(draw), Some(decision)) = (unsafe { draw.as_ref() }, unsafe { decision.as_mut() })
    else {
        return 0;
    };
    let (admitted, made) = lock().begin_model(draw);
    *decision = made;
    i32::from(admitted)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_view_begin(
    draw: *const ViewDraw,
    decision: *mut ViewDecision,
) {
    // SAFETY: The draw callback lends both for this call.
    let (Some(draw), Some(decision)) = (unsafe { draw.as_ref() }, unsafe { decision.as_mut() })
    else {
        return;
    };
    *decision = lock().begin_view(draw);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_view_ready(slot: u32, draw: *const ViewDraw) {
    // SAFETY: The draw callback lends its identity for this call.
    if let Some(draw) = unsafe { draw.as_ref() } {
        lock().ready_view(slot, draw);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_velocity_view_written(slot: u32) {
    lock().mark_written(slot, true);
}

/// The commit's view lookup; velocity_commit.rs admits the pass and builds its identity.
pub(crate) fn commit(pass: &ViewDraw) -> ViewDecision {
    lock().commit_view(pass)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_velocity_view_committed(slot: u32) {
    lock().mark_written(slot, false);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_velocity_model_count() -> usize {
    lock().models.len()
}

/// Advances the epoch and reports every retired slot after the lock is released.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_history_frame_end(
    user: *mut c_void,
    retire: Option<RetireSlot>,
) {
    let mut retired = Vec::new();
    {
        let mut history = lock();
        let total = history.models.len() + history.views.len();
        // Without room for the report the entries wait for the next frame end.
        if retired.try_reserve(total).is_err() {
            history.epoch += 1;
            return;
        }
        history.end_frame(&mut retired);
    }
    if let Some(retire) = retire {
        for (kind, slot) in retired {
            // SAFETY: The frame-end caller lends its retirement leaf and context for this call.
            unsafe { retire(user, kind, slot) };
        }
    }
}

/// The native slots were discarded with their renderer (`velocity_renderer.rs`).
pub(crate) fn reset() {
    *lock() = History::default();
}

#[cfg(test)]
mod tests;
