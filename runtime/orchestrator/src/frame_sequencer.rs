// SPDX-License-Identifier: MIT
use rsf_game_sys::RSF_FRAME_SOURCE_RENDERED;
use std::alloc::{Layout, alloc};
use std::ptr;
use std::sync::Mutex;

const INVALID_ARGUMENT: i32 = -1;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const ALL_MARKERS: u32 = 0b111_1111;
const RENDER_MARKERS: u32 = 0b111_1000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SequenceState {
    struct_size: u32,
    frame_id: u64,
    marker_mask: u32,
    failed: u32,
    timestamps: [u64; 7],
    source_provenance: u32,
    source_qpc: u64,
}

struct Ledger {
    capacity: usize,
    last_begin: u64,
    slots: [SequenceState; 64],
}

impl Ledger {
    fn slot(&mut self, id: u64) -> &mut SequenceState {
        &mut self.slots[id as usize % self.capacity]
    }
}

type Handle = Mutex<Ledger>;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_create(capacity: u32, out: *mut *mut Handle) -> i32 {
    if out.is_null() || !(1..=64).contains(&capacity) {
        return INVALID_ARGUMENT;
    }
    // Allocation failure is a recoverable result in the public C API.
    let handle = unsafe { alloc(Layout::new::<Handle>()).cast::<Handle>() };
    if handle.is_null() {
        unsafe { out.write(ptr::null_mut()) };
        return INIT_FAILED;
    }
    unsafe {
        handle.write(Mutex::new(Ledger {
            capacity: capacity as usize,
            last_begin: 0,
            slots: [SequenceState::default(); 64],
        }));
        out.write(handle);
    }
    0
}

fn begin(handle: Option<&Handle>, id: u64, source: u64) -> i32 {
    let Some(handle) = handle else {
        return INVALID_ARGUMENT;
    };
    if id == 0 {
        return INVALID_ARGUMENT;
    }
    let mut ledger = handle.lock().unwrap();
    if id <= ledger.last_begin || ledger.slot(id).frame_id != 0 {
        return NOT_READY;
    }
    *ledger.slot(id) = SequenceState {
        struct_size: size_of::<SequenceState>() as u32,
        frame_id: id,
        source_provenance: if source == 0 {
            0
        } else {
            RSF_FRAME_SOURCE_RENDERED
        },
        source_qpc: source,
        ..Default::default()
    };
    ledger.last_begin = id;
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_begin(handle: *const Handle, id: u64) -> i32 {
    begin(unsafe { handle.as_ref() }, id, 0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_begin_rendered(
    handle: *const Handle,
    id: u64,
    source: u64,
) -> i32 {
    if source == 0 {
        return INVALID_ARGUMENT;
    }
    begin(unsafe { handle.as_ref() }, id, source)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_marker(
    handle: *const Handle,
    id: u64,
    marker: u32,
    timestamp: u64,
) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    let index = match marker {
        6 => 0, // Input sample precedes the simulation, render and present pairs.
        0..=5 => marker as usize + 1,
        _ => return INVALID_ARGUMENT,
    };
    if id == 0 || timestamp == 0 {
        return INVALID_ARGUMENT;
    }
    let mut ledger = handle.lock().unwrap();
    let slot = ledger.slot(id);
    if slot.frame_id != id || slot.failed != 0 {
        return NOT_READY;
    }
    let rendered = slot.source_provenance == RSF_FRAME_SOURCE_RENDERED;
    let required = if rendered {
        RENDER_MARKERS
    } else {
        ALL_MARKERS
    };
    let bit = 1 << index;
    if required & bit == 0
        || slot.marker_mask != (bit - 1) & required
        || timestamp < slot.source_qpc
        || (index > 0 && timestamp < slot.timestamps[index - 1])
    {
        slot.failed = 1;
        return INVALID_ARGUMENT;
    }
    slot.timestamps[index] = timestamp;
    slot.marker_mask |= bit;
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_get(
    handle: *const Handle,
    id: u64,
    out: *mut SequenceState,
) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    // A short output may contain just the size field.
    if id == 0
        || out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<SequenceState>() as u32
    {
        return INVALID_ARGUMENT;
    }
    let mut ledger = handle.lock().unwrap();
    let slot = ledger.slot(id);
    if slot.frame_id != id {
        return NOT_READY;
    }
    unsafe { out.write(*slot) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_finish(handle: *const Handle, id: u64) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if id == 0 {
        return INVALID_ARGUMENT;
    }
    let mut ledger = handle.lock().unwrap();
    let slot = ledger.slot(id);
    if slot.frame_id != id {
        return NOT_READY;
    }
    let frame = std::mem::take(slot);
    let required = if frame.source_provenance == RSF_FRAME_SOURCE_RENDERED {
        RENDER_MARKERS
    } else {
        ALL_MARKERS
    };
    if frame.failed == 0 && frame.marker_mask == required {
        0
    } else {
        NOT_READY
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_destroy(handle: *mut Handle) {
    if !handle.is_null() {
        // The caller has retired every user of the allocation returned by create.
        drop(unsafe { Box::from_raw(handle) });
    }
}
