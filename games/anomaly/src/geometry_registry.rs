// SPDX-License-Identifier: MIT
use crate::sys::{
    rsf_anomaly_geometry_draw as Draw, rsf_anomaly_geometry_key as Key,
    rsf_anomaly_geometry_probe as Probe, rsf_anomaly_geometry_retired as Retired, *,
};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ffi::c_void;
use std::mem;
use std::sync::Mutex;

#[cfg(test)]
mod tests;

pub(crate) type IdentityRegistry = Mutex<Registry>;
pub(crate) type RetirementQueue = Mutex<Queue>;

#[derive(Default)]
pub(crate) struct Registry {
    // Index by visual first so a draw with no pool can check all owners.
    live: BTreeMap<(u64, u64), u64>,
    generation: u64,
}

impl Registry {
    pub(crate) const fn new() -> Self {
        Self {
            live: BTreeMap::new(),
            generation: 0,
        }
    }
}

#[derive(Default)]
pub(crate) struct Queue {
    pending: Vec<Key>,
    dropped: u64,
}

impl Queue {
    pub(crate) fn enqueue(&mut self, key: Key) {
        if self.pending.try_reserve(1).is_ok() {
            self.pending.push(key);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    pub(crate) fn take(&mut self) -> Vec<Key> {
        mem::take(&mut self.pending)
    }
}

unsafe fn deliver(
    keys: impl ExactSizeIterator<Item = Key>,
    retired: Retired,
    user: *mut c_void,
) -> usize {
    let count = keys.len();
    if let Some(retired) = retired {
        for key in keys {
            unsafe { retired(user, &key) };
        }
    }
    count
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_registry_create() -> *mut IdentityRegistry {
    Box::into_raw(Box::default())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_registry_destroy(registry: *mut IdentityRegistry) {
    drop(unsafe { Box::from_raw(registry) });
}

pub(crate) fn register(registry: &IdentityRegistry, owner: u64, visual: u64) -> u64 {
    if owner == 0 || visual == 0 {
        return 0;
    }
    let mut registry = registry.lock().unwrap();
    let Some(generation) = registry.generation.checked_add(1) else {
        return 0;
    };
    let Entry::Vacant(slot) = registry.live.entry((visual, owner)) else {
        return 0;
    };
    slot.insert(generation);
    registry.generation = generation;
    generation
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_register(
    registry: &IdentityRegistry,
    owner: u64,
    visual: u64,
) -> u64 {
    register(registry, owner, visual)
}

pub(crate) fn find(registry: &IdentityRegistry, owner: u64, visual: u64) -> u64 {
    registry
        .lock()
        .unwrap()
        .live
        .get(&(visual, owner))
        .copied()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_find(
    registry: &IdentityRegistry,
    owner: u64,
    visual: u64,
) -> u64 {
    find(registry, owner, visual)
}

pub(crate) fn probe_visual(registry: &IdentityRegistry, visual: u64) -> Probe {
    let state = registry.lock().unwrap();
    let mut matches = state.live.range((visual, 0)..=(visual, u64::MAX));
    match (matches.next(), matches.next()) {
        (Some(((_, owner), generation)), None) => Probe {
            lookup: RSF_ANOMALY_VISUAL_UNIQUE as u32,
            key: Key {
                owner: *owner,
                visual,
                generation: *generation,
            },
        },
        (Some(_), Some(_)) => Probe {
            lookup: RSF_ANOMALY_VISUAL_AMBIGUOUS as u32,
            ..Default::default()
        },
        _ => Probe {
            lookup: RSF_ANOMALY_VISUAL_MISSING as u32,
            ..Default::default()
        },
    }
}

pub(crate) fn retire(registry: &IdentityRegistry, owner: u64, visual: u64) -> u64 {
    registry
        .lock()
        .unwrap()
        .live
        .remove(&(visual, owner))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_retire(
    registry: &IdentityRegistry,
    owner: u64,
    visual: u64,
) -> u64 {
    retire(registry, owner, visual)
}

// Invalidate the entire batch and release the registry lock before delivering callbacks.
pub(crate) fn take_live(registry: &IdentityRegistry) -> impl ExactSizeIterator<Item = Key> {
    let live = mem::take(&mut registry.lock().unwrap().live);
    live.into_iter().map(|((visual, owner), generation)| Key {
        owner,
        visual,
        generation,
    })
}

pub(crate) fn classify(draw: &Draw) -> i32 {
    if draw.pass == RSF_ANOMALY_PASS_SHADOW {
        return RSF_ANOMALY_DRAW_IGNORED_SHADOW;
    }
    if draw.capturing == 0 {
        return RSF_ANOMALY_DRAW_UNIDENTIFIED_NO_CAPTURE;
    }
    match draw.lookup {
        RSF_ANOMALY_VISUAL_UNIQUE => return RSF_ANOMALY_DRAW_IDENTIFIED,
        RSF_ANOMALY_VISUAL_AMBIGUOUS => return RSF_ANOMALY_DRAW_UNIDENTIFIED_AMBIGUOUS,
        _ => {}
    }
    let tree = matches!(
        draw.visual_type,
        RSF_ANOMALY_TYPE_TREE_ST | RSF_ANOMALY_TYPE_TREE_PM
    );
    if tree && draw.foliage_identity != 0 {
        return RSF_ANOMALY_DRAW_IDENTIFIED_FOLIAGE;
    }
    if matches!(
        draw.visual_type,
        RSF_ANOMALY_TYPE_SKELETON_ST | RSF_ANOMALY_TYPE_SKELETON_PM
    ) {
        return RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC;
    }
    if draw.world_readable == 0 {
        return RSF_ANOMALY_DRAW_UNIDENTIFIED_UNREADABLE;
    }
    if tree {
        return RSF_ANOMALY_DRAW_UNIDENTIFIED_DEFORMED;
    }
    if draw.world_identity != 0 {
        RSF_ANOMALY_DRAW_IGNORED_STATIC_WORLD
    } else {
        RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_retirements_create() -> *mut RetirementQueue {
    Box::into_raw(Box::default())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_retirements_destroy(queue: *mut RetirementQueue) {
    drop(unsafe { Box::from_raw(queue) });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_enqueue(queue: &RetirementQueue, key: &Key) {
    queue.lock().unwrap().enqueue(*key);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_drain(
    queue: &RetirementQueue,
    retired: Retired,
    user: *mut c_void,
) -> usize {
    let batch = queue.lock().unwrap().take();
    unsafe { deliver(batch.into_iter(), retired, user) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_dropped(queue: &RetirementQueue) -> u64 {
    queue.lock().unwrap().dropped
}
