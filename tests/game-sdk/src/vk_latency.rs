// SPDX-License-Identifier: MIT
use crate::sys::*;
use rsf_orchestrator::vulkan_latency_api::*;
use std::ffi::{c_char, c_void};
use std::ptr::{self, null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

#[path = "vk_amd.rs"]
mod amd;
#[path = "vk_streamline.rs"]
mod streamline;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
type Event = rsf_game_cpu_event;

static AMD_FIXTURE: AtomicBool = AtomicBool::new(false);
static CALLBACK_FAILURES: AtomicU32 = AtomicU32::new(0);

fn verify(value: bool) -> bool {
    CALLBACK_FAILURES.fetch_add(u32::from(!value), Relaxed);
    value
}

// The fixtures use non-null opaque identities without dereferencing them.
fn identity<T>(id: usize) -> *mut T {
    ptr::without_provenance_mut(id)
}

struct Block {
    armed: bool,
    entered: bool,
    released: bool,
}

struct Gate {
    state: Mutex<Block>,
    changed: Condvar,
}

impl Gate {
    const fn new() -> Self {
        Self {
            state: Mutex::new(Block {
                armed: false,
                entered: false,
                released: false,
            }),
            changed: Condvar::new(),
        }
    }

    fn arm(&self) {
        *self.state.lock().unwrap() = Block {
            armed: true,
            entered: false,
            released: false,
        };
    }

    fn pause(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.armed {
            return true;
        }
        state.entered = true;
        self.changed.notify_all();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(3), |state| !state.released)
            .unwrap();
        state.released
    }

    fn entered(&self) -> bool {
        let (state, _) = self
            .changed
            .wait_timeout_while(
                self.state.lock().unwrap(),
                Duration::from_secs(2),
                |state| !state.entered,
            )
            .unwrap();
        state.entered
    }

    fn release(&self) {
        let mut state = self.state.lock().unwrap();
        state.released = true;
        state.armed = false;
        self.changed.notify_all();
    }
}

fn status_request() -> rsf_latencyvk_status {
    rsf_latencyvk_status {
        struct_size: size_of::<rsf_latencyvk_status>() as u32,
        abi_version: RSF_LATENCYVK_ABI_VERSION,
        ..Default::default()
    }
}

fn event(
    session: u64,
    source: u64,
    stage: u32,
    stamp: u64,
    hz: u64,
    version: u32,
    flags: u32,
) -> Event {
    Event {
        struct_size: size_of::<Event>() as u32,
        stage,
        session_id: session,
        source_frame_id: source,
        timestamp_qpc: stamp,
        qpc_frequency: hz,
        render_expected: 1,
        input_kind: 0,
        message_id: 0,
        event_version: version,
        flags,
    }
}

struct PresentationLease {
    context: *mut rsf_fgvk,
    closing: bool,
}

impl PresentationLease {
    fn borrow() -> Self {
        let mut closing = 0;
        let context = unsafe { rsf_core_fgvk_presentation_borrow(&mut closing) };
        Self {
            context,
            closing: closing != 0,
        }
    }
}

impl Drop for PresentationLease {
    fn drop(&mut self) {
        if !self.context.is_null() {
            rsf_core_fgvk_presentation_return();
        }
    }
}

unsafe extern "C" fn fake_fgvk_backend(context: *const rsf_fgvk, backend: *mut u32) -> i32 {
    if AMD_FIXTURE.load(Relaxed) && context != identity(4) {
        return NOT_READY;
    }
    if context.is_null() || backend.is_null() {
        return INVALID;
    }
    unsafe { backend.write(RSF_FG_BACKEND_XESS) };
    OK
}

extern "C" fn fake_fgvk_latency_options(context: *mut rsf_fgvk, mode: u32, _: u32) -> i32 {
    if AMD_FIXTURE.load(Relaxed) {
        return if context == identity(4) {
            OK
        } else {
            NOT_READY
        };
    }
    streamline::xell_options(mode)
}

extern "C" fn fake_fgvk_latency_sleep(context: *mut rsf_fgvk, _: u64) -> i32 {
    if AMD_FIXTURE.load(Relaxed) {
        return NOT_READY;
    }
    streamline::xell_sleep(context)
}

extern "C" fn fake_fgvk_latency_marker(_: *mut rsf_fgvk, frame: u64, kind: u32) -> i32 {
    if AMD_FIXTURE.load(Relaxed) {
        return NOT_READY;
    }
    streamline::xell_marker(frame, kind)
}

unsafe extern "C" fn fake_fgvk_latency_get_status(
    _: *mut rsf_fgvk,
    out: *mut rsf_fgvk_latency_status,
) -> i32 {
    if AMD_FIXTURE.load(Relaxed) {
        return NOT_READY;
    }
    unsafe { out.write(streamline::xell_status()) };
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_antilag_vulkan_fixture() -> i32 {
    AMD_FIXTURE.store(true, Relaxed);
    crate::run(amd::run)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_latency_vulkan_fixture() -> i32 {
    crate::run(streamline::run)
}

// The latency owner with this fixture's FG endpoints and, outside the Anti-Lag case, the fake
// Streamline; the public names below call it as the Runtime's exports call the production table.
static STREAMLINE_PLATFORM: Platform = Platform {
    streamline: &streamline::SDK,
    backend: Some(fake_fgvk_backend),
    options: Some(fake_fgvk_latency_options),
    sleep: Some(fake_fgvk_latency_sleep),
    marker: Some(fake_fgvk_latency_marker),
    status: Some(fake_fgvk_latency_get_status),
};
static AMD_PLATFORM: Platform = Platform {
    streamline: null(),
    ..STREAMLINE_PLATFORM
};

fn platform() -> &'static Platform {
    if AMD_FIXTURE.load(Relaxed) {
        &AMD_PLATFORM
    } else {
        &STREAMLINE_PLATFORM
    }
}

fn amd() -> bool {
    AMD_FIXTURE.load(Relaxed)
}

unsafe fn rsf_latencyvk_initialize(setup: *const rsf_latencyvk_setup) -> i32 {
    unsafe { rsf_core_latencyvk_initialize(platform(), setup) }
}
unsafe fn rsf_slvk_initialize(setup: *const rsf_slvk_setup) -> i32 {
    unsafe { rsf_core_slvk_initialize(platform(), setup) }
}
unsafe fn rsf_slvk_invoke(call: *const rsf_slvk_call) -> i32 {
    unsafe { rsf_core_slvk_invoke(platform(), call) }
}
unsafe fn rsf_latencyvk_options(mode: u32, limit: u32) -> i32 {
    rsf_core_latencyvk_options(platform(), mode, limit)
}
unsafe fn rsf_latencyvk_presentation(context: *mut c_void) -> i32 {
    rsf_core_latencyvk_presentation(platform(), context.cast())
}
unsafe fn rsf_latencyvk_submit_end(frame: u64) {
    rsf_core_latencyvk_submit_end(platform(), frame);
}
unsafe fn rsf_latencyvk_cpu(event: *const rsf_game_cpu_event) {
    unsafe { rsf_core_latencyvk_cpu(platform(), event) };
}
unsafe fn rsf_latencyvk_input(frame: u64, kind: u32, message: u32) -> i32 {
    rsf_core_latencyvk_input(platform(), 0, frame, kind, message)
}
unsafe fn rsf_latencyvk_render(frame: u64) {
    rsf_core_latencyvk_render(platform(), frame);
}
unsafe fn rsf_latencyvk_present(begin: u32) {
    rsf_core_latencyvk_present(platform(), begin);
}
unsafe fn rsf_latencyvk_execution(pass: *const rsf_game_render_pass, begin: u32) -> i32 {
    unsafe { rsf_core_latencyvk_submit(platform(), pass, begin) }
}
unsafe fn rsf_latencyvk_get_status(status: *mut rsf_latencyvk_status) -> i32 {
    unsafe { rsf_core_latencyvk_get_status(platform(), status) }
}
unsafe fn rsf_latencyvk_shutdown() -> i32 {
    rsf_core_latencyvk_shutdown(platform())
}
unsafe fn rsf_slvk_without_dlssg(callback: rsf_vk_wsi_call, user: *mut c_void) -> i32 {
    let scope = if amd() {
        None
    } else {
        streamline::WITHOUT_DLSS_G
    };
    unsafe { rsf_core_slvk_without_dlssg(scope, callback, user) }
}
// The Anti-Lag fixture ran the device entries with the Vulkan headers present, the latency
// fixture without.
unsafe fn rsf_latencyvk_require_device(
    query: *const rsf_latencyvk_device_query,
    out: *mut rsf_latencyvk_device_requirements,
) -> i32 {
    unsafe { rsf_core_amd_vk_require(u32::from(amd()), query, out) }
}
unsafe fn rsf_latencyvk_device_created(facts: *const rsf_latencyvk_created_device) -> i32 {
    unsafe { rsf_core_amd_vk_device_created(u32::from(amd()), facts) }
}
unsafe fn rsf_latencyvk_device_destroyed(device: *mut c_void) {
    rsf_core_amd_vk_device_destroyed(device);
}
