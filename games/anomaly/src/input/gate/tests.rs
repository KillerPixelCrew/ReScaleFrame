// SPDX-License-Identifier: MIT
use super::*;
use std::cell::Cell;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
struct Record {
    offset: u32,
    data: u32,
    timestamp: u32,
    sequence: u32,
    app_data: usize,
}

fn record(offset: u32, data: u32) -> Record {
    Record {
        offset,
        data,
        ..Record::default()
    }
}

struct Device {
    events: VecDeque<Record>,
    acquired: bool,
    acquires: u32,
    unacquires: u32,
    flushes: u32,
    state: u32,
}

impl Device {
    fn new() -> Self {
        Self {
            events: VecDeque::new(),
            acquired: true,
            acquires: 0,
            unacquires: 0,
            flushes: 0,
            state: 9,
        }
    }
}

unsafe extern "system" fn acquire(device: *mut c_void) -> HResult {
    let device = unsafe { &mut *device.cast::<Device>() };
    device.acquired = true;
    device.acquires += 1;
    DI_OK
}

unsafe extern "system" fn unacquire(device: *mut c_void) -> HResult {
    let device = unsafe { &mut *device.cast::<Device>() };
    device.acquired = false;
    device.unacquires += 1;
    DI_OK
}

unsafe extern "system" fn get_state(device: *mut c_void, size: u32, data: *mut c_void) -> HResult {
    let device = unsafe { &mut *device.cast::<Device>() };
    if !device.acquired {
        return DIERR_NOTACQUIRED;
    }
    assert_eq!(size, size_of::<u32>() as u32);
    unsafe { data.cast::<u32>().write(std::mem::take(&mut device.state)) };
    DI_OK
}

unsafe extern "system" fn get_data(
    device: *mut c_void,
    size: u32,
    data: *mut u8,
    count: *mut u32,
    flags: u32,
) -> HResult {
    let device = unsafe { &mut *device.cast::<Device>() };
    if !device.acquired {
        return DIERR_NOTACQUIRED;
    }
    let count = unsafe { &mut *count };
    if data.is_null() {
        if *count == INFINITE {
            device.events.clear();
            device.flushes += 1;
        }
        return DI_OK;
    }
    assert_eq!(size as usize, size_of::<Record>());
    let copied = (*count as usize).min(device.events.len());
    for (index, event) in device.events.iter().take(copied).enumerate() {
        unsafe { data.cast::<Record>().add(index).write(*event) };
    }
    if flags & DIGDD_PEEK == 0 {
        device.events.drain(..copied);
    }
    *count = copied as u32;
    DI_OK
}

const CALLS: DeviceCalls = DeviceCalls {
    acquire,
    unacquire,
    get_state,
    get_data,
};

thread_local! {
    static OBSERVED: Cell<Option<u8>> = const { Cell::new(None) };
    static OBSERVER_GATE: Cell<*const Gate> = const { Cell::new(ptr::null()) };
}

unsafe extern "C" fn observe(kind: u8) {
    // A callback must be able to inspect the gate without locking recursively.
    let gate = unsafe { &*OBSERVER_GATE.get() };
    let devices = gate
        .devices
        .try_lock()
        .expect("observer runs outside the gate lock");
    assert!(devices.values().any(|entry| entry.kind == kind));
    assert_eq!(OBSERVED.replace(Some(kind)), None);
}

unsafe extern "C" fn observer() -> Option<Observer> {
    Some(observe)
}

unsafe extern "C" fn capture(user: *mut c_void) -> u32 {
    unsafe { &*user.cast::<Cell<u32>>() }.get()
}

fn gate(bits: &Cell<u32>) -> Gate {
    Gate {
        capture: Some(capture),
        user: ptr::from_ref(bits).cast_mut().cast(),
        devices: Mutex::default(),
    }
}

fn read(
    gate: &Gate,
    device: &mut Device,
    records: &mut [Record],
    flags: u32,
) -> (HResult, u32, Option<u8>) {
    OBSERVER_GATE.set(ptr::from_ref(gate));
    let mut count = records.len() as u32;
    let result = unsafe {
        gate_get_data(
            gate,
            ptr::from_mut(device).cast(),
            &CALLS,
            OBJECT_SIZE,
            records.as_mut_ptr().cast(),
            &mut count,
            flags,
            Some(observer),
        )
    };
    OBSERVER_GATE.set(ptr::null());
    (result, count, OBSERVED.take())
}

#[test]
fn buffered_capture_releases_keys_and_reports_only_consumed_hardware() {
    let bits = Cell::new(0);
    let gate = gate(&bits);
    let mut keyboard = Device::new();
    let mut other = Device::new();
    gate_add(&gate, ptr::from_mut(&mut keyboard).cast(), KEYBOARD);
    let mut output = [Record::default(); 4];
    keyboard.events.extend([record(17, 0x80), record(30, 0x80)]);
    bits.set(CAPTURE_MOUSE);
    assert_eq!(
        read(&gate, &mut keyboard, &mut output, DIGDD_PEEK),
        (DI_OK, 2, None)
    );
    assert_eq!(
        read(&gate, &mut keyboard, &mut output, 0),
        (DI_OK, 2, Some(KEYBOARD))
    );
    assert_eq!(output[0], record(17, 0x80));
    keyboard.events.push_back(record(30, 0));
    bits.set(CAPTURE_KEYBOARD);
    assert_eq!(
        read(&gate, &mut keyboard, &mut output[..1], DIGDD_PEEK),
        (DI_OK, 1, None)
    );
    assert_eq!((output[0].offset, output[0].data), (17, 0));
    assert_eq!(
        read(&gate, &mut keyboard, &mut output[..1], 0),
        (DI_OK, 1, None)
    );
    assert_eq!(output[0].offset, 17);
    assert_eq!(read(&gate, &mut keyboard, &mut output, 0), (DI_OK, 1, None));
    assert_eq!((output[0].offset, output[0].data), (30, 0));
    assert_eq!(read(&gate, &mut keyboard, &mut output, 0), (DI_OK, 0, None));
    assert!(keyboard.events.is_empty());
    keyboard.acquired = false;
    assert_eq!(
        read(&gate, &mut keyboard, &mut output, 0).0,
        DIERR_NOTACQUIRED
    );
    other.events.push_back(record(5, 0x80));
    assert_eq!(read(&gate, &mut other, &mut output, 0), (DI_OK, 1, None));
    assert_eq!(other.flushes, 0);

    keyboard.acquired = true;
    keyboard.events.push_back(record(17, 0x80));
    let device = ptr::from_mut(&mut keyboard).cast();
    let mut all = INFINITE;
    assert_eq!(
        unsafe {
            gate_get_data(
                &gate,
                device,
                &CALLS,
                OBJECT_SIZE,
                ptr::null_mut(),
                &mut all,
                0,
                None,
            )
        },
        DI_OK
    );
    assert!(keyboard.events.is_empty());

    keyboard.events.push_back(record(17, 0x80));
    bits.set(0);
    assert_eq!(read(&gate, &mut keyboard, &mut output, 0), (DI_OK, 0, None));
    keyboard.events.push_back(record(30, 0x80));
    assert_eq!(
        read(&gate, &mut keyboard, &mut output, 0),
        (DI_OK, 1, Some(KEYBOARD))
    );
}

#[test]
fn mouse_capture_respects_acquisition_and_drains_state() {
    let bits = Cell::new(0);
    let gate = gate(&bits);
    let mut mouse = Device::new();
    let device = ptr::from_mut(&mut mouse).cast();
    gate_add(&gate, device, MOUSE);
    gate_cooperative_level(&gate, device, DISCL_EXCLUSIVE);
    assert_eq!(unsafe { gate_acquire(&gate, device, &CALLS) }, DI_OK);
    let mut output = [Record::default(); 4];
    mouse.events.push_back(record(DIMOFS_BUTTON0, 0x80));
    assert_eq!(
        read(&gate, &mut mouse, &mut output, 0),
        (DI_OK, 1, Some(MOUSE))
    );
    bits.set(CAPTURE_MOUSE);
    assert_eq!(read(&gate, &mut mouse, &mut output, 0), (DI_OK, 1, None));
    assert_eq!((output[0].offset, output[0].data), (DIMOFS_BUTTON0, 0));
    assert!(!mouse.acquired);
    assert_eq!(mouse.unacquires, 1);
    let device = ptr::from_mut(&mut mouse).cast();
    assert_eq!(unsafe { gate_acquire(&gate, device, &CALLS) }, DI_OK);
    assert_eq!(mouse.acquires, 1);
    bits.set(0);
    assert_eq!(read(&gate, &mut mouse, &mut output, 0), (DI_OK, 0, None));
    assert!(mouse.acquired);
    assert_eq!(mouse.acquires, 2);

    let device = ptr::from_mut(&mut mouse).cast();
    gate_cooperative_level(&gate, device, 0);
    bits.set(CAPTURE_MOUSE);
    let mut state = u32::MAX;
    assert_eq!(
        unsafe { gate_get_state(&gate, device, &CALLS, 4, ptr::from_mut(&mut state).cast(),) },
        DI_OK
    );
    assert_eq!((state, mouse.state, mouse.unacquires), (0, 0, 1));
    assert!(mouse.acquired);

    let device = ptr::from_mut(&mut mouse).cast();
    assert_eq!(unsafe { gate_unacquire(&gate, device, &CALLS) }, DI_OK);
    bits.set(0);
    assert_eq!(read(&gate, &mut mouse, &mut output, 0).0, DIERR_NOTACQUIRED);
    assert_eq!(mouse.acquires, 2);
}
