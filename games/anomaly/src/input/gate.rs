// SPDX-License-Identifier: MIT
//! Adapter policy at the engine's buffered DirectInput boundary.
use super::{KEYBOARD, MOUSE};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard};

// RSF_OVERLAY_CAPTURE_MOUSE / _KEYBOARD.
const CAPTURE_MOUSE: u32 = 0x1;
const CAPTURE_KEYBOARD: u32 = 0x2;
// dinput.h values for the ANSI/Unicode DirectInput 8 device boundary.
type HResult = i32;
const DI_OK: HResult = 0;
const DIERR_INPUTLOST: HResult = 0x8007_001E_u32 as i32;
const DIERR_NOTACQUIRED: HResult = 0x8007_000C_u32 as i32;
const DISCL_EXCLUSIVE: u32 = 0x1;
const DIGDD_PEEK: u32 = 0x1;
const DIMOFS_BUTTON0: u32 = 12;
const DIMOFS_BUTTON7: u32 = 19;
const INFINITE: u32 = u32::MAX;
// DIDEVICEOBJECTDATA on x64, and DIDEVICEOBJECTDATA_DX3 without uAppData.
const OBJECT_SIZE: u32 = 24;
const DX3_OBJECT_SIZE: u32 = 16;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetTickCount() -> u32;
}

type Capture = unsafe extern "C" fn(*mut c_void) -> u32;

/// DirectInput trampolines. The first argument is the IDirectInputDevice8 `this`.
#[repr(C)]
pub(crate) struct DeviceCalls {
    pub acquire: unsafe extern "system" fn(*mut c_void) -> HResult,
    pub unacquire: unsafe extern "system" fn(*mut c_void) -> HResult,
    pub get_state: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> HResult,
    pub get_data: unsafe extern "system" fn(*mut c_void, u32, *mut u8, *mut u32, u32) -> HResult,
}

/// Keys and buttons indexed by `dwOfs`: keyboard DIK_* codes, mouse DIMOFS_BUTTON0..7.
#[derive(Clone, Copy, Default)]
struct Keys([u64; 4]);

impl Keys {
    fn test(&self, ofs: usize) -> bool {
        self.0[ofs / 64] & (1 << (ofs % 64)) != 0
    }
    fn set(&mut self, ofs: usize, down: bool) {
        if down {
            self.0[ofs / 64] |= 1 << (ofs % 64);
        } else {
            self.0[ofs / 64] &= !(1 << (ofs % 64));
        }
    }
    fn none(&self) -> bool {
        self.0 == [0; 4]
    }
}

#[derive(Default)]
struct Entry {
    kind: u8,
    captured: bool,
    exclusive: bool,
    game_acquired: bool,
    gate_unacquired: bool,
    down: Keys,
    release_pending: Keys,
}

pub(crate) struct Gate {
    capture: Option<Capture>,
    user: usize,
    devices: Mutex<BTreeMap<usize, Entry>>,
}

fn well_formed(stride: u32) -> bool {
    stride == OBJECT_SIZE || stride == DX3_OBJECT_SIZE
}

fn input_lost(result: HResult) -> bool {
    result == DIERR_INPUTLOST || result == DIERR_NOTACQUIRED
}

fn at(base: *mut u8, index: u32, stride: u32) -> *mut u8 {
    base.wrapping_add(index as usize * stride as usize)
}

/// Flushes the device buffer: NULL data with INFINITE count.
///
/// # Safety
/// `device` and `calls` come from the hook that received the engine's call.
unsafe fn flush(device: *mut c_void, calls: &DeviceCalls, stride: u32) -> HResult {
    let mut count = INFINITE;
    unsafe { (calls.get_data)(device, stride, ptr::null_mut(), &mut count, 0) }
}

impl Gate {
    #[cfg(windows)]
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn new(capture: Capture) -> Self {
        Self {
            capture: Some(capture),
            user: 0,
            devices: Mutex::new(BTreeMap::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<usize, Entry>> {
        crate::lock(&self.devices)
    }

    /// Runs on the polling thread before the call is answered and returns whether the device kind
    /// is captured now. Both edges drop whatever is buffered.
    ///
    /// # Safety
    /// As for `flush`.
    unsafe fn update(&self, entry: &mut Entry, device: *mut c_void, calls: &DeviceCalls) -> bool {
        let bits = match self.capture {
            Some(capture) => unsafe { capture(std::ptr::with_exposed_provenance_mut(self.user)) },
            None => 0,
        };
        let wanted = if entry.kind == MOUSE {
            CAPTURE_MOUSE
        } else {
            CAPTURE_KEYBOARD
        };
        let now = bits & wanted != 0;
        if now == entry.captured {
            return now;
        }
        entry.captured = now;
        let _ = unsafe { flush(device, calls, OBJECT_SIZE) };
        if now {
            for (pending, down) in entry.release_pending.0.iter_mut().zip(entry.down.0) {
                *pending |= down;
            }
            entry.down = Keys::default();
            if entry.kind == MOUSE
                && entry.exclusive
                && entry.game_acquired
                && !entry.gate_unacquired
            {
                let _ = unsafe { (calls.unacquire)(device) };
                entry.gate_unacquired = true;
            }
        } else if entry.gate_unacquired {
            entry.gate_unacquired = false;
            // A refusal (window not foreground) leaves the engine to see NOTACQUIRED and acquire itself.
            if entry.game_acquired {
                let _ = unsafe { (calls.acquire)(device) };
            }
        }
        now
    }
}

/// Writes the synthetic releases owed for keys the engine saw pressed when capture began.
///
/// # Safety
/// `out` holds `capacity` records of `stride` bytes.
unsafe fn emit_pending(
    entry: &mut Entry,
    out: *mut u8,
    capacity: u32,
    stride: u32,
    consume: bool,
) -> u32 {
    if entry.release_pending.none() || out.is_null() || capacity == 0 || !well_formed(stride) {
        return 0;
    }
    let mut written = 0;
    #[cfg(windows)]
    let now = unsafe { GetTickCount() };
    #[cfg(not(windows))]
    let now = 0;
    for ofs in 0..256 {
        if written == capacity {
            break;
        }
        if !entry.release_pending.test(ofs) {
            continue;
        }
        let slot = at(out, written, stride);
        // dwOfs, dwData (bit 7 clear: released), dwTimeStamp; the remainder stays zero.
        unsafe {
            ptr::write_bytes(slot, 0, stride as usize);
            slot.cast::<u32>().write_unaligned(ofs as u32);
            slot.add(8).cast::<u32>().write_unaligned(now);
        }
        if consume {
            entry.release_pending.set(ofs, false);
        }
        written += 1;
    }
    written
}

/// Records which keys and buttons the engine has now seen pressed.
///
/// # Safety
/// `events` holds `count` records of `stride` bytes.
unsafe fn track(entry: &mut Entry, events: *const u8, count: u32, stride: u32) {
    if !well_formed(stride) {
        return;
    }
    for i in 0..count {
        let event = events.wrapping_add(i as usize * stride as usize);
        let (ofs, data) = unsafe {
            (
                event.cast::<u32>().read_unaligned(),
                event.add(4).cast::<u32>().read_unaligned(),
            )
        };
        let relevant = if entry.kind == KEYBOARD {
            ofs < 256
        } else {
            (DIMOFS_BUTTON0..=DIMOFS_BUTTON7).contains(&ofs)
        };
        if relevant {
            entry.down.set(ofs as usize, data & 0x80 != 0);
        }
    }
}

/// The same address may be reused by a later device; adding it again replaces the old record.
pub(crate) fn gate_add(gate: &Gate, device: *mut c_void, kind: u8) {
    if device.is_null() {
        return;
    }
    let entry = Entry {
        kind,
        ..Entry::default()
    };
    gate.lock().insert(device as usize, entry);
}

pub(crate) fn gate_cooperative_level(gate: &Gate, device: *mut c_void, flags: u32) {
    if let Some(entry) = gate.lock().get_mut(&(device as usize)) {
        entry.exclusive = flags & DISCL_EXCLUSIVE != 0;
    }
}

#[cfg(all(windows, feature = "overlay-device-fixture"))]
pub(crate) fn tracked(gate: &Gate, device: *mut c_void) -> bool {
    gate.lock().contains_key(&(device as usize))
}

type Observer = unsafe extern "C" fn(u8);
type ObserverQuery = unsafe extern "C" fn() -> Option<Observer>;

/// # Safety
/// `device` and `calls` come from the hook that received the engine's Acquire.
pub(crate) unsafe fn gate_acquire(
    gate: &Gate,
    device: *mut c_void,
    calls: &DeviceCalls,
) -> HResult {
    let mut devices = gate.lock();
    let Some(entry) = devices.get_mut(&(device as usize)) else {
        drop(devices);
        return unsafe { (calls.acquire)(device) };
    };
    entry.game_acquired = true;
    let captured = unsafe { gate.update(entry, device, calls) };
    // The engine believes it holds the mouse; the window keeps it until the panel closes.
    if entry.gate_unacquired {
        return DI_OK;
    }
    if captured && entry.kind == MOUSE && entry.exclusive {
        entry.gate_unacquired = true;
        return DI_OK;
    }
    unsafe { (calls.acquire)(device) }
}

/// # Safety
/// As for `gate_acquire`.
pub(crate) unsafe fn gate_unacquire(
    gate: &Gate,
    device: *mut c_void,
    calls: &DeviceCalls,
) -> HResult {
    if let Some(entry) = gate.lock().get_mut(&(device as usize)) {
        entry.game_acquired = false;
        entry.gate_unacquired = false;
    }
    unsafe { (calls.unacquire)(device) }
}

/// # Safety
/// As for `gate_acquire`; `data` holds `size` bytes or is null.
pub(crate) unsafe fn gate_get_state(
    gate: &Gate,
    device: *mut c_void,
    calls: &DeviceCalls,
    size: u32,
    data: *mut c_void,
) -> HResult {
    let mut devices = gate.lock();
    let Some(entry) = devices.get_mut(&(device as usize)) else {
        drop(devices);
        return unsafe { (calls.get_state)(device, size, data) };
    };
    if !unsafe { gate.update(entry, device, calls) } || data.is_null() || size == 0 {
        return unsafe { (calls.get_state)(device, size, data) };
    }
    // Read through so relative axes do not pile up behind the panel, then hide the result.
    let result = if entry.gate_unacquired {
        DIERR_NOTACQUIRED
    } else {
        unsafe { (calls.get_state)(device, size, data) }
    };
    if !entry.gate_unacquired && result < 0 {
        return result;
    }
    unsafe { ptr::write_bytes(data.cast::<u8>(), 0, size as usize) };
    DI_OK
}

/// While captured the engine sees no records, real input is drained and genuine loss still reaches
/// it. Notify the observer only for hardware records consumed by the engine, after dropping the
/// device lock. Peeks and synthetic releases do not generate input facts.
///
/// # Safety
/// As for `gate_acquire`; `data` holds `*count` records of `object_size` bytes
/// or is null; `count` is writable or null.
#[allow(clippy::too_many_arguments)] // GetDeviceData's arguments plus the gate, the methods and the fact.
pub(crate) unsafe fn gate_get_data(
    gate: &Gate,
    device: *mut c_void,
    calls: &DeviceCalls,
    object_size: u32,
    data: *mut u8,
    count: *mut u32,
    flags: u32,
    query: Option<ObserverQuery>,
) -> HResult {
    let mut devices = gate.lock();
    let Some(entry) = devices.get_mut(&(device as usize)) else {
        drop(devices);
        return unsafe { (calls.get_data)(device, object_size, data, count, flags) };
    };
    let captured = unsafe { gate.update(entry, device, calls) };
    // NULL data is a flush or query by the engine, not a read; the real device answers it.
    if data.is_null() || count.is_null() {
        return unsafe { (calls.get_data)(device, object_size, data, count, flags) };
    }
    let peek = flags & DIGDD_PEEK != 0;
    let capacity = unsafe { count.read() };
    if !well_formed(object_size) {
        if !captured {
            return unsafe { (calls.get_data)(device, object_size, data, count, flags) };
        }
        let _ = unsafe { flush(device, calls, object_size) };
        unsafe { count.write(0) };
        return DI_OK;
    }
    let pending = unsafe { emit_pending(entry, data, capacity, object_size, !peek) };
    if captured {
        let drained = if !peek && !entry.gate_unacquired {
            unsafe { flush(device, calls, object_size) }
        } else {
            DI_OK
        };
        unsafe { count.write(pending) };
        if pending != 0 {
            return DI_OK;
        }
        return if !entry.gate_unacquired && input_lost(drained) {
            drained
        } else {
            DI_OK
        };
    }
    if pending >= capacity {
        unsafe { count.write(pending) };
        return DI_OK;
    }
    let mut received = capacity - pending;
    let records = at(data, pending, object_size);
    let result = unsafe { (calls.get_data)(device, object_size, records, &mut received, flags) };
    if result < 0 {
        if pending == 0 {
            return result;
        }
        unsafe { count.write(pending) };
        return DI_OK;
    }
    if !peek {
        unsafe { track(entry, records, received, object_size) };
    }
    let consumed = (!peek && received != 0).then_some(entry.kind);
    unsafe { count.write(pending + received) };
    drop(devices);
    if let Some(kind) = consumed
        && let Some(observer) = query.and_then(|query| unsafe { query() })
    {
        unsafe { observer(kind) };
    }
    result
}

#[cfg(test)]
mod tests;
