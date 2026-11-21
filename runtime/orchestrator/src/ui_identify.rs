// SPDX-License-Identifier: MIT
use crate::sr_session::has_capacity;
use crate::ui_identify_abi::*;
use std::ffi::{c_int, c_void};
use std::ptr::{null, null_mut};

const OK: rsf_ui_result = 0;
const INVALID: rsf_ui_result = -1;
const FULL: rsf_ui_result = -3;
const LAYOUTS: usize = RSF_UI_MAX_LAYOUTS as usize;
const WIDGET_TARGETS: usize = RSF_UI_MAX_WIDGET_TARGETS as usize;
const NAMED_SHADERS: usize = RSF_UI_MAX_NAMED_SHADERS as usize;
// Indexed by rsf_ui_set from ui_identify.h.
const CAPACITIES: [usize; 5] = [
    LAYOUTS,
    LAYOUTS,
    WIDGET_TARGETS,
    NAMED_SHADERS,
    NAMED_SHADERS,
];

type Identity = *mut c_void;

// Entries are borrowed identities. Inline storage keeps views contiguous and mutations
// allocation-free inside resource-creation and draw callbacks.
struct Set {
    entries: [Identity; WIDGET_TARGETS],
    count: usize,
}

impl Set {
    fn entries(&self) -> &[Identity] {
        &self.entries[..self.count]
    }

    fn remove(&mut self, index: usize) {
        // Filling the gap with the last entry would make the newest member look older.
        self.entries.copy_within(index + 1..self.count, index);
        self.count -= 1;
        self.entries[self.count] = null_mut();
    }
}

struct Registry {
    sets: [Set; CAPACITIES.len()],
    counters: rsf_ui_registry_counters,
}

impl Registry {
    fn record(&mut self, set: usize, object: Identity) {
        let target = &mut self.sets[set];
        target.entries[target.count] = object;
        target.count += 1;
        self.counters.recorded[set] = self.counters.recorded[set].wrapping_add(1);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ui_registry_create(abi_version: u32) -> *mut Registry {
    if abi_version != RSF_UI_IDENTIFY_ABI_VERSION {
        return null_mut();
    }
    crate::allocate_handle(Registry {
        sets: [const {
            Set {
                entries: [null_mut(); WIDGET_TARGETS],
                count: 0,
            }
        }; CAPACITIES.len()],
        counters: rsf_ui_registry_counters {
            struct_size: size_of::<rsf_ui_registry_counters>() as u32,
            refused_full: [0; CAPACITIES.len()],
            forgotten_on_reuse: 0,
            recorded: [0; CAPACITIES.len()],
            evicted: [0; CAPACITIES.len()],
        },
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_registry_destroy(registry: *mut Registry) {
    if !registry.is_null() {
        drop(unsafe { Box::from_raw(registry) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_registry_add(
    registry: *mut Registry,
    set: rsf_ui_set,
    object: Identity,
) -> rsf_ui_result {
    let set = set as usize;
    if object.is_null() || set >= CAPACITIES.len() {
        return INVALID;
    }
    let Some(registry) = (unsafe { registry.as_mut() }) else {
        return INVALID;
    };
    let target = &registry.sets[set];
    if target.entries().contains(&object) {
        return OK;
    }
    if target.count == CAPACITIES[set] {
        registry.counters.refused_full[set] = registry.counters.refused_full[set].wrapping_add(1);
        return FULL;
    }
    registry.record(set, object);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_registry_note_recent(
    registry: *mut Registry,
    set: rsf_ui_set,
    object: Identity,
) -> c_int {
    let set = set as usize;
    if object.is_null() || set >= CAPACITIES.len() {
        return 0;
    }
    let Some(registry) = (unsafe { registry.as_mut() }) else {
        return 0;
    };
    let target = &mut registry.sets[set];
    // Repeated draws to the current target usually hit this fast path.
    if target.entries().last() == Some(&object) {
        return 0;
    }
    if let Some(index) = target.entries().iter().position(|&entry| entry == object) {
        target.entries[index..target.count].rotate_left(1);
        return 0;
    }
    if target.count == CAPACITIES[set] {
        target.remove(0);
        registry.counters.evicted[set] = registry.counters.evicted[set].wrapping_add(1);
    }
    registry.record(set, object);
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_registry_forget(registry: *mut Registry, object: Identity) {
    if object.is_null() {
        return;
    }
    let Some(registry) = (unsafe { registry.as_mut() }) else {
        return;
    };
    for target in &mut registry.sets {
        if let Some(index) = target.entries().iter().position(|&entry| entry == object) {
            target.remove(index);
            registry.counters.forgotten_on_reuse =
                registry.counters.forgotten_on_reuse.wrapping_add(1);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_registry_contains(
    registry: *const Registry,
    set: rsf_ui_set,
    object: *const c_void,
) -> c_int {
    if object.is_null() {
        return 0;
    }
    let target = unsafe { registry.as_ref() }.and_then(|registry| registry.sets.get(set as usize));
    c_int::from(target.is_some_and(|target| target.entries().contains(&object.cast_mut())))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_registry_view(
    registry: *const Registry,
    set: rsf_ui_set,
    count_out: *mut u32,
) -> *const Identity {
    let target = unsafe { registry.as_ref() }.and_then(|registry| registry.sets.get(set as usize));
    if !count_out.is_null() {
        unsafe { count_out.write(target.map_or(0, |target| target.count as u32)) };
    }
    target.map_or(null(), |target| target.entries.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_registry_get_counters(
    registry: *const Registry,
    counters: *mut rsf_ui_registry_counters,
) -> rsf_ui_result {
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(counters) } {
        return INVALID;
    }
    let mut snapshot = registry.counters;
    snapshot.struct_size = unsafe { counters.cast::<u32>().read() };
    unsafe { counters.write(snapshot) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_shader_hash(bytecode: *const c_void, bytes: u32) -> u32 {
    if bytecode.is_null() || bytes == 0 {
        return 0;
    }
    let data = unsafe { std::slice::from_raw_parts(bytecode.cast::<u8>(), bytes as usize) };
    crate::shader_hash::crc32c(data)
}
