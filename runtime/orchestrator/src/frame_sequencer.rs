// SPDX-License-Identifier: MIT
use rsf_game_sys::{RSF_FRAME_SOURCE_INPUT, RSF_FRAME_SOURCE_RENDERED};
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

impl SequenceState {
    fn required(&self) -> u32 {
        if self.source_provenance == RSF_FRAME_SOURCE_RENDERED {
            RENDER_MARKERS
        } else {
            ALL_MARKERS
        }
    }
}

pub(super) struct Ledger {
    capacity: usize,
    last_begin: u64,
    slots: [SequenceState; 64],
}

impl Ledger {
    pub(super) fn new(capacity: usize) -> Self {
        assert!((1..=64).contains(&capacity));
        Self {
            capacity,
            last_begin: 0,
            slots: [SequenceState::default(); 64],
        }
    }

    fn index(&self, id: u64) -> usize {
        (id % self.capacity as u64) as usize
    }

    fn get(&self, id: u64) -> Option<&SequenceState> {
        let slot = &self.slots[self.index(id)];
        (id != 0 && slot.frame_id == id).then_some(slot)
    }

    #[cfg(windows)]
    pub(super) fn busy(&self) -> bool {
        self.slots[..self.capacity]
            .iter()
            .any(|slot| slot.frame_id != 0)
    }

    #[cfg(windows)]
    pub(super) fn source(&self, id: u64) -> Option<u32> {
        self.get(id).map(|slot| slot.source_provenance)
    }

    #[cfg(windows)]
    pub(super) fn prepare_source(&self, id: u64) -> Option<(u32, u64)> {
        let slot = self.get(id)?;
        // Prepare after submit starts or ends, before Present starts.
        let submit_start = slot.required() & 0b000_1111;
        let submit_end = slot.required() & 0b001_1111;
        if slot.failed != 0 || (slot.marker_mask != submit_start && slot.marker_mask != submit_end)
        {
            return None;
        }
        let timestamp = if slot.source_provenance == RSF_FRAME_SOURCE_RENDERED {
            slot.source_qpc
        } else {
            slot.timestamps[0]
        };
        Some((slot.source_provenance, timestamp))
    }

    pub(super) fn begin(&mut self, id: u64, source: Option<u64>) -> i32 {
        if id == 0 || source == Some(0) {
            return INVALID_ARGUMENT;
        }
        let index = self.index(id);
        if id <= self.last_begin || self.slots[index].frame_id != 0 {
            return NOT_READY;
        }
        self.slots[index] = SequenceState {
            struct_size: size_of::<SequenceState>() as u32,
            frame_id: id,
            source_provenance: if source.is_some() {
                RSF_FRAME_SOURCE_RENDERED
            } else {
                RSF_FRAME_SOURCE_INPUT
            },
            source_qpc: source.unwrap_or(0),
            ..Default::default()
        };
        self.last_begin = id;
        0
    }

    pub(super) fn marker(&mut self, id: u64, marker: u32, timestamp: u64) -> i32 {
        let index = match marker {
            6 => 0, // Input sample precedes the simulation, render and present pairs.
            0..=5 => marker as usize + 1,
            _ => return INVALID_ARGUMENT,
        };
        if id == 0 || timestamp == 0 {
            return INVALID_ARGUMENT;
        }
        let index_in_ledger = self.index(id);
        let slot = &mut self.slots[index_in_ledger];
        if slot.frame_id != id || slot.failed != 0 {
            return NOT_READY;
        }
        let required = slot.required();
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

    pub(super) fn finish(&mut self, id: u64) -> i32 {
        if id == 0 {
            return INVALID_ARGUMENT;
        }
        let index = self.index(id);
        if self.slots[index].frame_id != id {
            return NOT_READY;
        }
        let frame = std::mem::take(&mut self.slots[index]);
        if frame.failed == 0 && frame.marker_mask == frame.required() {
            0
        } else {
            NOT_READY
        }
    }
}

type Handle = Mutex<Ledger>;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_create(capacity: u32, out: *mut *mut Handle) -> i32 {
    if out.is_null() || !(1..=64).contains(&capacity) {
        return INVALID_ARGUMENT;
    }
    let handle = crate::allocate_handle(Mutex::new(Ledger::new(capacity as usize)));
    unsafe { out.write(handle) };
    if handle.is_null() { INIT_FAILED } else { 0 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_begin(handle: *const Handle, id: u64) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    handle.lock().unwrap().begin(id, None)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_begin_rendered(
    handle: *const Handle,
    id: u64,
    source: u64,
) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    handle.lock().unwrap().begin(id, Some(source))
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
    handle.lock().unwrap().marker(id, marker, timestamp)
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
    if id == 0
        || out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<SequenceState>() as u32
    {
        return INVALID_ARGUMENT;
    }
    let ledger = handle.lock().unwrap();
    let Some(slot) = ledger.get(id) else {
        return NOT_READY;
    };
    unsafe { out.write(*slot) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_finish(handle: *const Handle, id: u64) -> i32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    handle.lock().unwrap().finish(id)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_sequencer_destroy(handle: *mut Handle) {
    if !handle.is_null() {
        // The caller has retired every user of the allocation returned by create.
        drop(unsafe { Box::from_raw(handle) });
    }
}
