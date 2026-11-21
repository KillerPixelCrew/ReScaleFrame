// SPDX-License-Identifier: MIT
use crate::engine_guards::Image;
use crate::input::rsf_anomaly_input_activate;
use std::ffi::c_void;
use std::ptr::{null_mut, with_exposed_provenance, with_exposed_provenance_mut};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

const INPUT: usize = 0;
const POSITION: usize = 1;
const SLOT_BYTES: usize = size_of::<usize>();
const PAGE_READWRITE: u32 = 4;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualProtect(
        address: *mut c_void,
        length: usize,
        protection: u32,
        previous: *mut u32,
    ) -> i32;
}

type Read = unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool;
type Exchange =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut *mut c_void) -> bool;

struct Slot {
    address: usize,
    original: usize,
    replacement: usize,
    counter: usize,
    read: Read,
    exchange: Exchange,
    pending_protection: Option<u32>,
}

static SLOTS: Mutex<[Option<Slot>; 2]> = Mutex::new([None, None]);
// In-flight detours can still read their original after the slot is restored.
static ORIGINALS: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
static INPUT_COUNTER: AtomicUsize = AtomicUsize::new(0);
static POSITION_ACTIVE: AtomicBool = AtomicBool::new(false);

fn slots() -> MutexGuard<'static, [Option<Slot>; 2]> {
    SLOTS.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Slot {
    fn restore_protection(&mut self, address: *mut c_void) -> bool {
        if let Some(protection) = self.pending_protection {
            let mut ignored = 0;
            if unsafe { VirtualProtect(address, SLOT_BYTES, protection, &mut ignored) } == 0 {
                return false;
            }
            self.pending_protection = None;
        }
        true
    }

    fn set(&mut self, enabled: bool) -> bool {
        let address = with_exposed_provenance_mut(self.address);
        if !self.restore_protection(address) {
            return false;
        }
        let (expected, wanted) = if enabled {
            (self.original, self.replacement)
        } else {
            (self.replacement, self.original)
        };
        let mut current = null_mut::<c_void>();
        if !unsafe { (self.read)((&raw mut current).cast(), address, SLOT_BYTES) } {
            return false;
        }
        if current.addr() == wanted {
            return true;
        }
        if current.addr() != expected {
            return false;
        }
        let mut old = 0;
        if unsafe { VirtualProtect(address, SLOT_BYTES, PAGE_READWRITE, &mut old) } == 0 {
            return false;
        }
        self.pending_protection = Some(old);
        let exchanged = unsafe {
            (self.exchange)(
                address,
                with_exposed_provenance_mut(expected),
                with_exposed_provenance_mut(wanted),
                &mut current,
            )
        };
        let found = current.addr();
        self.restore_protection(address) && exchanged && (found == expected || found == wanted)
    }
}

fn publish(kind: usize, enabled: bool) {
    match kind {
        INPUT => rsf_anomaly_input_activate(enabled),
        POSITION => POSITION_ACTIVE.store(enabled, Ordering::Release),
        _ => {}
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_slot_install(
    kind: u32,
    image: &Image,
    slot_rva: u32,
    function_rva: u32,
    counter_rva: u32,
    replacement: *mut c_void,
    exchange: Exchange,
) -> bool {
    let kind = kind as usize;
    let (Some(address), Some(original)) = (
        image.address(slot_rva, SLOT_BYTES),
        image.address(function_rva, 1),
    ) else {
        return false;
    };
    if !address.cast::<usize>().is_aligned() || replacement.is_null() {
        return false;
    }
    let counter = if kind == INPUT {
        let Some(counter) = image.address(counter_rva, size_of::<u32>()) else {
            return false;
        };
        counter.expose_provenance()
    } else {
        0
    };
    let (address, original, replacement) = (
        address.expose_provenance(),
        original.expose_provenance(),
        replacement.expose_provenance(),
    );
    let mut slots = slots();
    let Some(entry) = slots.get_mut(kind) else {
        return false;
    };
    if entry.is_none() {
        if image.copy(slot_rva) != Some(original.to_ne_bytes()) {
            return false;
        }
        *entry = Some(Slot {
            address,
            original,
            replacement,
            counter,
            read: image.read,
            exchange,
            pending_protection: None,
        });
        if kind == INPUT {
            INPUT_COUNTER.store(counter, Ordering::Release);
        }
        ORIGINALS[kind].store(original, Ordering::Release);
    }
    let slot = entry.as_mut().unwrap();
    if [slot.address, slot.original, slot.replacement, slot.counter]
        != [address, original, replacement, counter]
    {
        return false;
    }
    let enabled = slot.set(true);
    publish(kind, enabled);
    enabled
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_slot_set(kind: u32, enabled: bool) -> bool {
    let kind = kind as usize;
    let mut slots = slots();
    let Some(entry) = slots.get_mut(kind) else {
        return false;
    };
    if !enabled {
        publish(kind, false);
    }
    let Some(slot) = entry else { return true };
    let changed = slot.set(enabled);
    if enabled {
        publish(kind, changed);
    }
    changed
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_slot_original(kind: u32) -> *mut c_void {
    ORIGINALS.get(kind as usize).map_or(null_mut(), |original| {
        with_exposed_provenance_mut(original.load(Ordering::Acquire))
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_position_slot_active() -> bool {
    POSITION_ACTIVE.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_input_slot_counter() -> *const u32 {
    with_exposed_provenance(INPUT_COUNTER.load(Ordering::Acquire))
}
