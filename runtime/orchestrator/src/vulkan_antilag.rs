// SPDX-License-Identifier: MIT
use crate::latency_antilag::fps_from_interval;
use crate::latency_lease::rsf_native_latency_release_claim;
use crate::vulkan_sr_abi::rsf_vk_function;
use std::ffi::c_void;
use std::sync::{Condvar, Mutex};

const NOT_READY: i32 = -8;
const INPUT: u32 = 0;
const PRESENT: u32 = 1;
const MODE_ONLY: u32 = u32::MAX;
const FRAME_SLOTS: usize = 128;

type Invoke = unsafe extern "C" fn(*mut c_void, rsf_vk_function, u32, u64, u32, u32);

#[derive(Clone, Copy)]
struct Device {
    handle: *mut c_void,
    update: rsf_vk_function,
    invoke: Invoke,
}

// The opaque Vulkan handle is borrowed until all admitted SDK calls retire.
unsafe impl Send for Device {}

impl Device {
    fn call(self, stage: u32, frame: u64, enabled: bool, fps: u32) {
        unsafe {
            (self.invoke)(
                self.handle,
                self.update,
                stage,
                frame,
                u32::from(enabled),
                fps,
            )
        };
    }
}

#[derive(Clone, Copy)]
struct Key {
    frame: u64,
    input: bool,
    present_owed: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Status {
    available: u32,
    enabled: u32,
    input_updates: u64,
    present_updates: u64,
}

#[derive(PartialEq, Eq)]
enum Phase {
    Open,
    RetiringDevice,
    Closing,
}

struct State {
    device: Option<Device>,
    calls: usize,
    phase: Phase,
    configuring: bool,
    configured: Option<(bool, u32)>,
    session: u64,
    keys: [Option<Key>; FRAME_SLOTS],
    input_updates: u64,
    present_updates: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            device: None,
            calls: 0,
            phase: Phase::Open,
            configuring: false,
            configured: None,
            session: 0,
            keys: [None; FRAME_SLOTS],
            input_updates: 0,
            present_updates: 0,
        }
    }

    fn slot(&mut self, frame: u64) -> &mut Option<Key> {
        &mut self.keys[(frame % FRAME_SLOTS as u64) as usize]
    }

    fn for_device(&self, device: *mut c_void) -> bool {
        self.device.is_some_and(|binding| binding.handle == device)
    }

    fn enabled(&self) -> bool {
        self.configured.is_some_and(|(enabled, _)| enabled)
    }

    fn admit(&mut self, device: Device, stage: u32, frame: u64) -> Update {
        self.calls += 1;
        let (enabled, interval) = self.configured.unwrap_or_default();
        Update {
            device,
            stage,
            frame,
            enabled,
            fps: if enabled {
                fps_from_interval(interval)
            } else {
                0
            },
        }
    }

    fn frame_update(&mut self, session: u64, frame: u64, stage: u32) -> Option<Update> {
        let device = self.device?;
        if !self.enabled()
            || (session != 0 && session != self.session)
            || self.phase == Phase::Closing
            || (stage == INPUT && (self.phase != Phase::Open || self.configuring))
        {
            return None;
        }
        let key = self.slot(frame).as_mut().filter(|key| key.frame == frame)?;
        if stage == INPUT {
            if key.input {
                return None;
            }
            key.input = true;
            key.present_owed = true;
            self.input_updates += 1;
        } else {
            if !key.present_owed {
                return None;
            }
            key.present_owed = false;
            self.present_updates += 1;
        }
        Some(self.admit(device, stage, frame))
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static DRAINED: Condvar = Condvar::new();

struct Update {
    device: Device,
    stage: u32,
    frame: u64,
    enabled: bool,
    fps: u32,
}

impl Update {
    fn call(self) {
        self.device
            .call(self.stage, self.frame, self.enabled, self.fps);
    }
}

impl Drop for Update {
    fn drop(&mut self) {
        let mut state = STATE.lock().unwrap();
        state.calls -= 1;
        if self.stage == MODE_ONLY {
            state.configuring = false;
        }
        if state.calls == 0 {
            DRAINED.notify_all();
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_bind(
    handle: *mut c_void,
    update: rsf_vk_function,
    invoke: Invoke,
) -> i32 {
    let mut state = STATE.lock().unwrap();
    if state.phase != Phase::Open || (state.device.is_some() && !state.for_device(handle)) {
        return NOT_READY;
    }
    if state.device.is_none() {
        state.device = Some(Device {
            handle,
            update,
            invoke,
        });
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_status() -> Status {
    let state = STATE.lock().unwrap();
    Status {
        available: u32::from(state.device.is_some()),
        enabled: u32::from(state.enabled()),
        input_updates: state.input_updates,
        present_updates: state.present_updates,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_options(mode: u32, interval: u32) {
    let update = {
        let mut state = STATE.lock().unwrap();
        let Some(device) = state.device else {
            return;
        };
        let enabled = mode != 0;
        if state.phase != Phase::Open
            || state.configuring
            || state.configured == Some((enabled, interval))
        {
            return;
        }
        if enabled != state.enabled() {
            for key in state.keys.iter_mut().flatten() {
                key.present_owed = false;
            }
        }
        state.configured = Some((enabled, interval));
        state.configuring = true;
        state.admit(device, MODE_ONLY, 0)
    };
    update.call();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_begin(session: u64, frame: u64) {
    let mut state = STATE.lock().unwrap();
    if state.device.is_none() || state.phase != Phase::Open || session == 0 || frame == 0 {
        return;
    }
    if state.session != session {
        state.session = session;
        state.keys.fill(None);
    }
    let slot = state.slot(frame);
    if !slot.is_some_and(|key| key.frame == frame) {
        *slot = Some(Key {
            frame,
            input: false,
            present_owed: false,
        });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_cancel(session: u64, frame: u64) {
    let mut state = STATE.lock().unwrap();
    if state.session == session
        && let Some(key) = state.slot(frame).as_mut().filter(|key| key.frame == frame)
    {
        key.present_owed = false;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_frame(session: u64, frame: u64, stage: u32) {
    if !matches!(stage, INPUT | PRESENT) {
        return;
    }
    let update = STATE.lock().unwrap().frame_update(session, frame, stage);
    if let Some(update) = update {
        // An earlier Present must be able to proceed while a newer input sleeps.
        update.call();
    }
}

fn close() {
    let device = {
        let state = STATE.lock().unwrap();
        debug_assert!(state.phase == Phase::Closing && state.calls == 0);
        state.device.filter(|_| state.enabled())
    };
    if let Some(device) = device {
        device.call(MODE_ONLY, 0, false, 0);
    }
    let mut state = STATE.lock().unwrap();
    rsf_native_latency_release_claim(2);
    *state = State::new();
    DRAINED.notify_all();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_shutdown_begin() -> i32 {
    let mut state = STATE.lock().unwrap();
    if state.phase != Phase::Open || state.calls != 0 {
        return NOT_READY;
    }
    state.phase = Phase::Closing;
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_shutdown_end(committed: u32) {
    if committed != 0 {
        close();
    } else {
        STATE.lock().unwrap().phase = Phase::Open;
        DRAINED.notify_all();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_device_destroyed(device: *mut c_void) {
    let state = STATE.lock().unwrap();
    let mut state = DRAINED
        .wait_while(state, |state| {
            state.for_device(device) && state.phase != Phase::Open
        })
        .unwrap();
    if !state.for_device(device) {
        return;
    }
    state.phase = Phase::RetiringDevice;
    // The public void notification precedes the real vkDestroyDevice. Drain
    // outstanding borrows, allowing already-owed older Presents to finish.
    let mut state = DRAINED.wait_while(state, |state| state.calls != 0).unwrap();
    state.phase = Phase::Closing;
    drop(state);
    close();
}
