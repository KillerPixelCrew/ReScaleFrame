// SPDX-License-Identifier: MIT
//! Shared session owner for the integrated Runtime and standalone XeSS-FG DLL.

use std::alloc::{Layout, alloc};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::mem::MaybeUninit;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicI32, Ordering};

#[path = "capture.rs"]
mod capture;
#[path = "timing.rs"]
mod timing;

use capture::{Capture, Desc, Event, Footprint, Image};
use timing::{Phase as TimingPhase, Report as TimingReport};

type Handle = *mut c_void;
type Step = unsafe extern "C" fn(Handle) -> i32;

#[repr(C)]
struct Modules {
    fg: Handle,
    xell: Handle,
}

// Matches VulkanStoryXessFrame and the managed XessPresentationFrame layout.
#[repr(C)]
struct Frame {
    frame_id: u32,
    reset: u32,
    vsync: u32,
    _reserved: MaybeUninit<u32>,
    color: Handle,
    depth: Handle,
    motion: Handle,
    hudless: Handle,
    ui: Handle,
    view: [f32; 16],
    projection: [f32; 16],
    jitter: [f32; 2],
    motion_scale: [f32; 2],
    ready: u64,
    done: u64,
}

impl Frame {
    fn has_resources(&self) -> bool {
        !self.color.is_null()
            && !self.depth.is_null()
            && !self.motion.is_null()
            && !self.hudless.is_null()
    }
}

#[repr(C)]
struct IntegratedFrame {
    struct_size: u32,
    abi_version: u32,
    frame_id: u32,
    reset: u32,
    vsync: u32,
    reserved: u32,
    color: Handle,
    depth: Handle,
    motion: Handle,
    hudless: Handle,
    view: [f32; 16],
    projection: [f32; 16],
    jitter: [f32; 2],
    motion_scale: [f32; 2],
    ready: u64,
    done: u64,
}

impl From<&IntegratedFrame> for Frame {
    fn from(frame: &IntegratedFrame) -> Self {
        Self {
            frame_id: frame.frame_id,
            reset: frame.reset,
            vsync: frame.vsync,
            _reserved: MaybeUninit::zeroed(),
            color: frame.color,
            depth: frame.depth,
            motion: frame.motion,
            hudless: frame.hudless,
            ui: ptr::null_mut(),
            view: frame.view,
            projection: frame.projection,
            jitter: frame.jitter,
            motion_scale: frame.motion_scale,
            ready: frame.ready,
            done: frame.done,
        }
    }
}

#[derive(Default)]
#[repr(C)]
struct PresentResult {
    frames_presented: u32,
    fg_enabled: u32,
    fg_result: i32,
}

#[derive(Default, PartialEq, Eq)]
#[repr(C)]
struct PresentResourceDesc {
    width: u64,
    height: u32,
    format: u32,
}

#[repr(C)]
struct IntegratedStatus {
    struct_size: u32,
    abi_version: u32,
    result: PresentResult,
    _reserved: u32,
}

#[repr(C)]
struct Platform {
    struct_size: u32,
    abi_version: u32,
    allocate: unsafe extern "C" fn(*mut Session) -> Handle,
    release_native: unsafe extern "C" fn(Handle),
    load_runtime: unsafe extern "C" fn(Handle, *const u16, *mut Modules) -> i32,
    unload_module: unsafe extern "C" fn(Handle),
    create_factory: Step,
    find_adapter: unsafe extern "C" fn(Handle, u64) -> i32,
    create_device: Step,
    create_queue: Step,
    create_xell: Step,
    create_fg: unsafe extern "C" fn(Handle, Handle) -> i32,
    set_latency: Step,
    set_default_sleep_mode: Step,
    log_create_error: unsafe extern "C" fn(*const c_char, i32, i32),
    init_swapchain: unsafe extern "C" fn(Handle, Handle, u32, u32, u32, u32) -> i32,
    query_limit: unsafe extern "C" fn(Handle, *mut u32) -> i32,
    get_swapchain: Step,
    enable_ui: Step,
    create_completion_fence: Step,
    open_event: unsafe extern "C" fn() -> Handle,
    create_recorders: Step,
    // Stores the SDK diagnostic setter. Nonzero when the export is missing.
    find_diagnostics: unsafe extern "C" fn(Handle, Handle) -> i32,
    set_generated: unsafe extern "C" fn(Handle, u32) -> i32,
    read_completion: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    arm_completion: unsafe extern "C" fn(Handle, u64, Handle) -> i32,
    wait_event: unsafe extern "C" fn(Handle, u32) -> i32,
    disable_fg: Step,
    unregister_diagnostics: Step,
    release_swapchain: unsafe extern "C" fn(Handle),
    destroy_fg: Step,
    destroy_xell: Step,
    has_shared_queue: unsafe extern "C" fn(*const c_void) -> u32,
    describe_present_resource: unsafe extern "C" fn(Handle, *mut PresentResourceDesc),
    reset_commands: unsafe extern "C" fn(Handle, u32) -> i32,
    transition_present_resource: unsafe extern "C" fn(Handle, Handle, u32, u32),
    // Transfers one DXGI backbuffer reference to the session on success.
    acquire_backbuffer: unsafe extern "C" fn(Handle, *mut Handle) -> i32,
    copy_present_color: unsafe extern "C" fn(Handle, Handle, Handle),
    tag_present_resource: unsafe extern "C" fn(Handle, Handle, u32, u32) -> i32,
    release_backbuffer: unsafe extern "C" fn(Handle),
    close_commands: Step,
    tag_constants: unsafe extern "C" fn(Handle, *const Frame) -> i32,
    wait_shared: unsafe extern "C" fn(Handle, u64) -> i32,
    execute: unsafe extern "C" fn(Handle),
    set_present_id: unsafe extern "C" fn(Handle, u32) -> i32,
    present: unsafe extern "C" fn(Handle, u32) -> i32,
    signal_done: unsafe extern "C" fn(Handle, u64) -> i32,
    signal_completion: unsafe extern "C" fn(Handle, u64) -> i32,
    read_present_status: unsafe extern "C" fn(Handle, *mut PresentResult) -> i32,
    timing_enabled: unsafe extern "C" fn() -> u32,
    timing_now: unsafe extern "C" fn() -> i64,
    timing_frequency: unsafe extern "C" fn() -> i64,
    timing_report: unsafe extern "C" fn(&TimingReport),
    begin_gpu_timing: unsafe extern "C" fn(Handle, u32) -> u32,
    timestamp_copies: unsafe extern "C" fn(Handle, u32, u32),
    mark_present_enter: unsafe extern "C" fn(Handle, u32),
    finish_gpu_timing: unsafe extern "C" fn(Handle, u32),
    serialize_enabled: unsafe extern "C" fn() -> u32,
    // Lends the session's capture owner to the capsule as the SDK callback user.
    attach_capture: unsafe extern "C" fn(Handle, *const Capture),
    register_diagnostics: Step,
    capture_fingerprint: unsafe extern "C" fn(Handle) -> u32,
    capture_failure: unsafe extern "C" fn(*const u16, *const c_char),
    capture_describe: unsafe extern "C" fn(Handle, *mut Desc),
    capture_event: unsafe extern "C" fn(*const u16, *const Event) -> i32,
    // Writes the footprint, its byte size and a device reference. Nonzero
    // when the resource has no device.
    capture_footprint: unsafe extern "C" fn(Handle, *mut Footprint, *mut u64, *mut Handle) -> i32,
    // A COPY_DEST readback buffer with one reference, or null.
    capture_readback: unsafe extern "C" fn(Handle, u64) -> Handle,
    capture_release: unsafe extern "C" fn(Handle),
    capture_copy: unsafe extern "C" fn(Handle, Handle, Handle, *const Footprint, u32),
    // Maps one completed readback and writes its raw and JSON files.
    capture_publish: unsafe extern "C" fn(*const u16, *const Image) -> i32,
}

const _: () = {
    assert!(size_of::<Modules>() == 16);
    assert!(size_of::<Frame>() == 216 && align_of::<Frame>() == 8);
    assert!(std::mem::offset_of!(Frame, color) == 16);
    assert!(std::mem::offset_of!(Frame, view) == 56);
    assert!(std::mem::offset_of!(Frame, ready) == 200);
    assert!(size_of::<IntegratedFrame>() == 216 && align_of::<IntegratedFrame>() == 8);
    assert!(std::mem::offset_of!(IntegratedFrame, struct_size) == 0);
    assert!(std::mem::offset_of!(IntegratedFrame, abi_version) == 4);
    assert!(std::mem::offset_of!(IntegratedFrame, color) == 24);
    assert!(std::mem::offset_of!(IntegratedFrame, view) == 56);
    assert!(std::mem::offset_of!(IntegratedFrame, ready) == 200);
    assert!(size_of::<PresentResult>() == 12);
    assert!(size_of::<PresentResourceDesc>() == 16 && align_of::<PresentResourceDesc>() == 8);
    assert!(std::mem::offset_of!(PresentResourceDesc, height) == 8);
    assert!(std::mem::offset_of!(PresentResourceDesc, format) == 12);
    assert!(size_of::<IntegratedStatus>() == 24);
    assert!(std::mem::offset_of!(IntegratedStatus, struct_size) == 0);
    assert!(std::mem::offset_of!(IntegratedStatus, abi_version) == 4);
    assert!(std::mem::offset_of!(IntegratedStatus, result) == 8);
    assert!(std::mem::offset_of!(IntegratedStatus, _reserved) == 20);
    assert!(size_of::<Platform>() == 544 && align_of::<Platform>() == 8);
    assert!(std::mem::offset_of!(Platform, allocate) == 8);
    assert!(std::mem::offset_of!(Platform, init_swapchain) == 112);
    assert!(std::mem::offset_of!(Platform, find_diagnostics) == 168);
    assert!(std::mem::offset_of!(Platform, read_completion) == 184);
    assert!(std::mem::offset_of!(Platform, unregister_diagnostics) == 216);
    assert!(std::mem::offset_of!(Platform, destroy_xell) == 240);
    assert!(std::mem::offset_of!(Platform, has_shared_queue) == 248);
    assert!(std::mem::offset_of!(Platform, describe_present_resource) == 256);
    assert!(std::mem::offset_of!(Platform, transition_present_resource) == 272);
    assert!(std::mem::offset_of!(Platform, acquire_backbuffer) == 280);
    assert!(std::mem::offset_of!(Platform, tag_present_resource) == 296);
    assert!(std::mem::offset_of!(Platform, release_backbuffer) == 304);
    assert!(std::mem::offset_of!(Platform, wait_shared) == 328);
    assert!(std::mem::offset_of!(Platform, read_present_status) == 376);
    assert!(std::mem::offset_of!(Platform, serialize_enabled) == 448);
    assert!(std::mem::offset_of!(Platform, attach_capture) == 456);
    assert!(std::mem::offset_of!(Platform, capture_footprint) == 504);
    assert!(std::mem::offset_of!(Platform, capture_publish) == 536);
};

const PLATFORM_ABI_VERSION: u32 = 5;
const BRIDGE_ABI_VERSION: u32 = 1;
const INVALID_ARGUMENT: i32 = -10001;
const ABI_MISMATCH: i32 = -10002;
const WAIT_MS: u32 = 10_000;
const WAIT_INCOMPLETE: i32 = -3;
const ALLOCATOR_COUNT: u32 = 3;
const CAPTURE_FLUSH_FAILED: i32 = -7010;
const CAPTURE_START_FAILED: i32 = -7011;

// Values are checked against the native headers in bridge.cpp.
const DXGI_FORMAT_R16G16_FLOAT: u32 = 34;
const DXGI_FORMAT_D32_FLOAT: u32 = 40;
const DXGI_FORMAT_R32_FLOAT: u32 = 41;
const DXGI_FORMAT_B8G8R8A8_UNORM: u32 = 87;
const STATE_COMMON: u32 = 0;
const STATE_PRESENT: u32 = 0;
const STATE_COPY_DEST: u32 = 0x400;
const STATE_COPY_SOURCE: u32 = 0x800;

#[repr(u32)]
enum InputRole {
    Hudless = 0,
    Depth = 1,
    Motion = 2,
    Ui = 3,
}

thread_local! {
    static CREATE_STAGE: Cell<&'static CStr> = const { Cell::new(c"not attempted") };
}

#[derive(PartialEq, Eq)]
enum Phase {
    Creating,
    Ready,
    Starting,
    PresenterReady,
    Started,
    Retiring,
    Prepared,
}

// The caller guarantees the size word. The version is readable only after the
// declared size covers the whole record.
unsafe fn check_present_abi<T>(record: *const T) -> Result<(), i32> {
    let header = record.cast::<u32>();
    if (unsafe { header.read() } as usize) < size_of::<T>() {
        return Err(INVALID_ARGUMENT);
    }
    if unsafe { header.add(1).read() } != BRIDGE_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    Ok(())
}

fn success(code: i32) -> Result<(), i32> {
    if code < 0 { Err(code) } else { Ok(()) }
}

fn advance_timing(ops: &Platform, phase: TimingPhase, start: Option<i64>) -> Option<i64> {
    start.map(|previous| {
        let now = unsafe { (ops.timing_now)() };
        timing::rsf_core_xess_timing_cpu(phase as u32, previous, now, unsafe {
            (ops.timing_frequency)()
        });
        now
    })
}

// The host serializes startup, presentation-side calls and retirement. The
// native XeLL sleep and marker paths borrow their typed bindings independently.
struct Session {
    platform: &'static Platform,
    native: Handle,
    // Owned. The SDK callbacks and the public capture calls borrow it
    // through the capsule while the session itself may be borrowed mutably.
    capture: NonNull<Capture>,
    modules: Modules,
    event: Option<OwnedHandle>,
    max_generated: u32,
    width: u32,
    height: u32,
    backbuffer_hudless: bool,
    next_allocator: u32,
    allocator_completion: [u64; ALLOCATOR_COUNT as usize],
    completion_value: u64,
    last_done_signalled: u64,
    phase: Phase,
    failure: AtomicI32,
}

impl Session {
    fn capture(&self) -> &Capture {
        unsafe { self.capture.as_ref() }
    }

    fn create_step(&self, stage: &'static CStr, step: Step) -> i32 {
        CREATE_STAGE.set(stage);
        unsafe { step(self.native) }
    }

    fn create(&mut self, luid: u64, directory: *const u16) -> i32 {
        let ops = self.platform;
        let native = self.native;
        CREATE_STAGE.set(c"runtime DLL/export loading");
        let code = unsafe { (ops.load_runtime)(native, directory, &mut self.modules) };
        if code != 0 {
            return code;
        }
        let code = self.create_step(c"CreateDXGIFactory2", ops.create_factory);
        if code != 0 {
            return code;
        }
        CREATE_STAGE.set(c"EnumAdapterByLuid");
        let code = unsafe { (ops.find_adapter)(native, luid) };
        if code != 0 {
            return code;
        }
        for (stage, step) in [
            (c"D3D12CreateDevice", ops.create_device),
            (c"CreateCommandQueue", ops.create_queue),
            (c"xellD3D12CreateContext", ops.create_xell),
        ] {
            let code = self.create_step(stage, step);
            if code != 0 {
                return code;
            }
        }
        CREATE_STAGE.set(c"xefgSwapChainD3D12CreateContext");
        let code = unsafe { (ops.create_fg)(native, self.modules.fg) };
        if code != 0 {
            return code;
        }
        let code = self.create_step(c"xefgSwapChainSetLatencyReduction", ops.set_latency);
        if code != 0 {
            return code;
        }
        self.create_step(c"xellSetSleepMode", ops.set_default_sleep_mode)
    }

    fn start(
        &mut self,
        window: Handle,
        width: u32,
        height: u32,
        inverted_depth: u32,
        generated: u32,
        unsupported: i32,
    ) -> i32 {
        let integrated = generated != 0;
        if window.is_null() || width == 0 || height == 0 || self.phase != Phase::Ready {
            return if integrated { INVALID_ARGUMENT } else { -1 };
        }
        let ops = self.platform;
        let native = self.native;
        if integrated {
            let mut max = 0;
            let code = unsafe { (ops.query_limit)(native, &mut max) };
            if code < 0 {
                return code;
            }
            if generated > max {
                return unsupported;
            }
        }
        // Once the SDK is given the HWND, a partial start must be retired.
        self.phase = Phase::Starting;
        let code = unsafe {
            (ops.init_swapchain)(
                native,
                window,
                width,
                height,
                u32::from(integrated),
                inverted_depth,
            )
        };
        if code < 0 {
            return code;
        }
        self.width = width;
        self.height = height;
        self.backbuffer_hudless = integrated;
        let code = unsafe { (ops.query_limit)(native, &mut self.max_generated) };
        if code < 0 || self.max_generated == 0 {
            return if code < 0 { code } else { -4 };
        }
        for step in [
            ops.get_swapchain,
            ops.enable_ui,
            ops.create_completion_fence,
        ] {
            let code = unsafe { step(native) };
            if code < 0 {
                return code;
            }
        }
        // CreateEventW transfers a Win32 event reference and returns null on failure.
        let event = unsafe { (ops.open_event)() };
        if event.is_null() {
            return -3;
        }
        self.event = Some(unsafe { OwnedHandle::from_raw_handle(event) });
        let code = unsafe { (ops.create_recorders)(native) };
        if code < 0 {
            return code;
        }
        self.phase = Phase::PresenterReady;
        if !self.capture().initialize(native, self.modules.fg) {
            return CAPTURE_START_FAILED;
        }
        let code = if integrated {
            unsafe { (ops.set_generated)(native, generated) }
        } else {
            0
        };
        if code >= 0 {
            self.phase = Phase::Started;
        }
        code
    }

    fn event(&self) -> Handle {
        self.event
            .as_ref()
            .map_or(ptr::null_mut(), AsRawHandle::as_raw_handle)
    }

    fn retain_failure(&self, code: i32) -> i32 {
        self.failure
            .compare_exchange(0, code, Ordering::Relaxed, Ordering::Relaxed)
            .map_or_else(|first| first, |_| code)
    }

    fn read_completion(&self) -> Result<u64, i32> {
        let mut completed = 0;
        let code = unsafe { (self.platform.read_completion)(self.native, &mut completed) };
        if code != 0 {
            return Err(self.retain_failure(code));
        }
        Ok(completed)
    }

    fn wait(&self, target: u64) -> Result<(), i32> {
        let failure = self.failure.load(Ordering::Relaxed);
        if failure != 0 {
            return Err(failure);
        }
        if target == 0 || self.read_completion()? >= target {
            return Ok(());
        }
        let event = self.event();
        if event.is_null() {
            return Err(-2);
        }
        let code = unsafe { (self.platform.arm_completion)(self.native, target, event) };
        if code < 0 {
            return Err(code);
        }
        let code = unsafe { (self.platform.wait_event)(event, WAIT_MS) };
        if code != 0 || self.read_completion()? < target {
            return Err(WAIT_INCOMPLETE);
        }
        Ok(())
    }

    fn next_completion(&mut self) -> u64 {
        self.completion_value = self.completion_value.wrapping_add(1);
        self.completion_value
    }

    fn signal_done(&mut self, done: u64) -> Result<(), i32> {
        success(unsafe { (self.platform.signal_done)(self.native, done) })?;
        self.last_done_signalled = done;
        Ok(())
    }

    fn present_resources_match(&self, frame: &Frame) -> bool {
        let describe = |resource: Handle| {
            let mut desc = PresentResourceDesc::default();
            unsafe { (self.platform.describe_present_resource)(resource, &mut desc) };
            desc
        };
        let color = describe(frame.color);
        let hudless = describe(frame.hudless);
        let ui = (!frame.ui.is_null()).then(|| describe(frame.ui));
        let motion = describe(frame.motion);
        let depth = describe(frame.depth);
        let output = PresentResourceDesc {
            width: u64::from(self.width),
            height: self.height,
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
        };
        let depth_format = if self.backbuffer_hudless {
            DXGI_FORMAT_R32_FLOAT
        } else {
            DXGI_FORMAT_D32_FLOAT
        };
        color == output
            && hudless == output
            && (self.backbuffer_hudless || ui.as_ref() == Some(&output))
            && motion.width == depth.width
            && motion.height == depth.height
            && motion.format == DXGI_FORMAT_R16G16_FLOAT
            && depth.format == depth_format
    }

    fn record_present_inputs(&self, frame: &Frame) -> Result<(), i32> {
        let ops = self.platform;
        let native = self.native;
        let transition = |resource, before, after| unsafe {
            (ops.transition_present_resource)(native, resource, before, after);
        };
        // A fallback may use final color as HUD-less. Transition each distinct
        // pointer in first-use order and keep every non-null SDK role below.
        let mut inputs = [
            frame.color,
            frame.depth,
            frame.motion,
            frame.hudless,
            frame.ui,
        ];
        for index in 1..inputs.len() {
            if inputs[..index].contains(&inputs[index]) {
                inputs[index] = ptr::null_mut();
            }
        }
        let unique_inputs = inputs.into_iter().filter(|input| !input.is_null());
        for input in unique_inputs.clone() {
            transition(input, STATE_COMMON, STATE_COPY_SOURCE);
        }

        let mut backbuffer = ptr::null_mut();
        let code = unsafe { (ops.acquire_backbuffer)(native, &mut backbuffer) };
        if code < 0 {
            unsafe { (ops.close_commands)(native) };
            return Err(code);
        }
        transition(backbuffer, STATE_PRESENT, STATE_COPY_DEST);
        unsafe { (ops.copy_present_color)(native, backbuffer, frame.color) };

        for (resource, role) in [
            (frame.depth, InputRole::Depth),
            (frame.motion, InputRole::Motion),
            (frame.hudless, InputRole::Hudless),
            (frame.ui, InputRole::Ui),
        ] {
            if resource.is_null() {
                continue;
            }
            let code = unsafe {
                (ops.tag_present_resource)(native, resource, role as u32, frame.frame_id)
            };
            if code < 0 {
                unsafe {
                    (ops.close_commands)(native);
                    (ops.release_backbuffer)(backbuffer);
                }
                return Err(code);
            }
        }

        for input in unique_inputs {
            transition(input, STATE_COPY_SOURCE, STATE_COMMON);
        }
        transition(backbuffer, STATE_COPY_DEST, STATE_PRESENT);
        unsafe { (ops.release_backbuffer)(backbuffer) };
        Ok(())
    }

    fn present_frame(&mut self, frame: &Frame) -> Result<(PresentResult, Option<i64>), i32> {
        if frame.frame_id == 0
            || frame.ready == 0
            || frame.done <= frame.ready
            || !frame.has_resources()
            || (!self.backbuffer_hudless && frame.ui.is_null())
        {
            return Err(-1);
        }
        if !self.present_resources_match(frame) {
            return Err(-2);
        }
        let ops = self.platform;
        let native = self.native;
        let timed = unsafe { (ops.timing_enabled)() } != 0;
        let phase_start = timed.then(|| unsafe { (ops.timing_now)() });
        let slot = self.next_allocator % ALLOCATOR_COUNT;
        self.next_allocator = self.next_allocator.wrapping_add(1);
        self.wait(self.allocator_completion[slot as usize])?;
        let phase_start = advance_timing(ops, TimingPhase::AllocatorWait, phase_start);
        let gpu_timed = timed && unsafe { (ops.begin_gpu_timing)(native, slot) } != 0;
        success(unsafe { (ops.reset_commands)(native, slot) })?;
        if gpu_timed {
            unsafe { (ops.timestamp_copies)(native, slot, 0) };
        }
        self.record_present_inputs(frame)?;
        if gpu_timed {
            unsafe { (ops.timestamp_copies)(native, slot, 1) };
        }
        success(unsafe { (ops.close_commands)(native) })?;
        success(unsafe { (ops.tag_constants)(native, frame) })?;
        let phase_start = advance_timing(ops, TimingPhase::Record, phase_start);

        success(unsafe { (ops.wait_shared)(native, frame.ready) })?;
        unsafe { (ops.execute)(native) };
        let present_id = success(unsafe { (ops.set_present_id)(native, frame.frame_id) });
        let phase_start = advance_timing(ops, TimingPhase::Submit, phase_start);
        if gpu_timed {
            unsafe { (ops.mark_present_enter)(native, slot) };
        }
        let presented =
            present_id.and_then(|()| success(unsafe { (ops.present)(native, frame.vsync) }));
        let phase_start = advance_timing(ops, TimingPhase::Present, phase_start);
        if gpu_timed {
            unsafe { (ops.finish_gpu_timing)(native, slot) };
        }

        // The copy/tag list already owns the shared inputs. Queue both signals
        // even when the SDK ID or DXGI Present failed.
        let handoff = self.signal_done(frame.done);
        let completion = self.next_completion();
        let retire = success(unsafe { (ops.signal_completion)(native, completion) });
        self.allocator_completion[slot as usize] = completion;
        if let Err(code) = retire {
            self.retain_failure(code);
        }
        // Opt-in diagnostic wait. Its result does not certify retirement.
        if unsafe { (ops.serialize_enabled)() } != 0 && retire.is_ok() {
            let event = self.event();
            if unsafe { (ops.arm_completion)(native, completion, event) } >= 0 {
                unsafe { (ops.wait_event)(event, WAIT_MS) };
            }
        }
        handoff?;
        retire?;
        presented?;
        let mut status = PresentResult::default();
        success(unsafe { (ops.read_present_status)(native, &mut status) })?;
        Ok((status, phase_start))
    }

    fn recover_handoff(&mut self, frame: &Frame) -> Result<(), i32> {
        let ops = self.platform;
        let native = self.native;
        // Vulkan can already be waiting on done. Queue ready first so the
        // shared timeline stays ordered even when the original call refused.
        success(unsafe { (ops.wait_shared)(native, frame.ready) })?;
        let completion = self.next_completion();
        let handoff = self.signal_done(frame.done);
        success(unsafe { (ops.signal_completion)(native, completion) })?;
        handoff
    }

    fn present(
        &mut self,
        frame: &Frame,
        outputs_valid: bool,
    ) -> Result<(PresentResult, Option<i64>), i32> {
        if self.phase != Phase::Started
            || unsafe { (self.platform.has_shared_queue)(self.native) } == 0
        {
            return Err(-1);
        }
        let result = if outputs_valid {
            self.present_frame(frame)
        } else {
            Err(-1)
        };
        if frame.done > self.last_done_signalled
            && frame.done > frame.ready
            && let Err(code) = self.recover_handoff(frame)
        {
            self.retain_failure(code);
            return Err(code);
        }
        result
    }

    fn finish_present(&self, phase_start: Option<i64>) {
        if advance_timing(self.platform, TimingPhase::Retire, phase_start).is_some()
            && let Some(report) = timing::end_present()
        {
            unsafe { (self.platform.timing_report)(&report) };
        }
    }

    fn retire_consumers(&mut self) -> Result<(), i32> {
        let ops = self.platform;
        let native = self.native;
        let disable = matches!(self.phase, Phase::PresenterReady | Phase::Started);
        self.phase = Phase::Retiring;
        if disable {
            let code = unsafe { (ops.disable_fg)(native) };
            if code < 0 {
                return Err(code);
            }
        }
        self.wait(self.completion_value)?;
        // Publication is gated on the drained queue and the completed fence.
        if !self.capture().flush(native) {
            return Err(CAPTURE_FLUSH_FAILED);
        }
        // The SDK refuses destruction with an outstanding DXGI wrapper reference.
        unsafe { (ops.release_swapchain)(native) };
        for step in [ops.destroy_fg, ops.destroy_xell] {
            let code = unsafe { step(native) };
            if code < 0 {
                return Err(code);
            }
        }
        Ok(())
    }

    fn prepare_destroy(&mut self) -> i32 {
        let failure = self.failure.load(Ordering::Relaxed);
        if failure != 0 || self.phase == Phase::Prepared {
            return failure;
        }
        match self.retire_consumers() {
            Ok(()) => {
                self.phase = Phase::Prepared;
                0
            }
            Err(code) => self.retain_failure(code),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.native.is_null() {
            unsafe { (self.platform.release_native)(self.native) };
        }
        // The capsule no longer lends the capture, and checked retirement
        // unregistered the SDK callbacks. Remaining readbacks are released.
        drop(unsafe { Box::from_raw(self.capture.as_ptr()) });
        drop(self.event.take());
        // COM and the native callback storage are gone before the SDK DLLs unload.
        for module in [self.modules.fg, self.modules.xell] {
            if !module.is_null() {
                unsafe { (self.platform.unload_module)(module) };
            }
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_bridge_create(
    platform: *const Platform,
    luid: u64,
    directory: *const u16,
    output: *mut Handle,
) -> i32 {
    CREATE_STAGE.set(c"argument validation");
    if output.is_null() || luid == 0 {
        return -1;
    }
    unsafe { output.write(ptr::null_mut()) };
    if platform.is_null() || unsafe { (*platform).struct_size } < size_of::<Platform>() as u32 {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*platform).abi_version } != PLATFORM_ABI_VERSION {
        return ABI_MISMATCH;
    }
    // Both C++ consumers lend a static table with every callback populated.
    let platform = unsafe { &*platform };
    CREATE_STAGE.set(c"native context allocation");
    let session = unsafe { alloc(Layout::new::<Session>()).cast::<Session>() };
    if session.is_null() {
        return -2;
    }
    let capture = Box::new(Capture::from_environment(platform));
    unsafe {
        session.write(Session {
            platform,
            native: ptr::null_mut(),
            capture: NonNull::from(Box::leak(capture)),
            modules: Modules {
                fg: ptr::null_mut(),
                xell: ptr::null_mut(),
            },
            event: None,
            max_generated: 0,
            width: 0,
            height: 0,
            backbuffer_hudless: false,
            next_allocator: 0,
            allocator_completion: [0; ALLOCATOR_COUNT as usize],
            completion_value: 0,
            last_done_signalled: 0,
            phase: Phase::Creating,
            failure: AtomicI32::new(0),
        });
    }
    let native = unsafe { (platform.allocate)(session) };
    if native.is_null() {
        drop(unsafe { Box::from_raw(session) });
        return -2;
    }
    let owner = unsafe { &mut *session };
    owner.native = native;
    unsafe { (platform.attach_capture)(native, owner.capture.as_ptr()) };
    let code = owner.create(luid, directory);
    if code != 0 {
        let stage = CREATE_STAGE.get().as_ptr();
        unsafe { (platform.log_create_error)(stage, code, 0) };
        let cleanup = owner.prepare_destroy();
        if cleanup == 0 {
            drop(unsafe { Box::from_raw(session) });
        } else {
            unsafe { output.write(native) };
            unsafe { (platform.log_create_error)(stage, code, cleanup) };
        }
        return code;
    }
    owner.phase = Phase::Ready;
    unsafe { output.write(native) };
    CREATE_STAGE.set(c"ready");
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_create_stage() -> *const c_char {
    CREATE_STAGE.get().as_ptr()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_note_vulkan_lookup() {
    CREATE_STAGE.set(c"Vulkan adapter LUID lookup");
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_start(
    owner: &mut Session,
    window: Handle,
    width: u32,
    height: u32,
    inverted_depth: u32,
    generated: u32,
    unsupported: i32,
) -> i32 {
    owner.start(
        window,
        width,
        height,
        inverted_depth,
        generated,
        unsupported,
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_started(owner: &Session) -> u32 {
    u32::from(owner.phase == Phase::Started)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_max_generated(owner: &Session) -> u32 {
    owner.max_generated
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_wait_idle(owner: &Session) -> i32 {
    owner.wait(owner.completion_value).err().unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_bridge_present(
    owner: *mut Session,
    frame: *const Frame,
    frames_presented: *mut u32,
    fg_result: *mut i32,
    fg_enabled: *mut u32,
) -> i32 {
    if !frames_presented.is_null() {
        unsafe { frames_presented.write(0) };
    }
    if !fg_result.is_null() {
        unsafe { fg_result.write(0) };
    }
    if !fg_enabled.is_null() {
        unsafe { fg_enabled.write(0) };
    }
    let (Some(owner), Some(frame)) = (unsafe { owner.as_mut() }, unsafe { frame.as_ref() }) else {
        return -1;
    };
    let outputs_valid =
        !frames_presented.is_null() && !fg_result.is_null() && !fg_enabled.is_null();
    match owner.present(frame, outputs_valid) {
        Ok((status, phase_start)) => {
            unsafe {
                frames_presented.write(status.frames_presented);
                fg_result.write(status.fg_result);
                fg_enabled.write(status.fg_enabled);
            }
            owner.finish_present(phase_start);
            0
        }
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_bridge_present_integrated(
    owner: *mut Session,
    frame: *const IntegratedFrame,
    status: *mut IntegratedStatus,
) -> i32 {
    if frame.is_null() || status.is_null() {
        return INVALID_ARGUMENT;
    }
    let admission = unsafe { check_present_abi(frame).and_then(|()| check_present_abi(status)) };
    if let Err(code) = admission {
        return code;
    }
    let output = unsafe { ptr::addr_of_mut!((*status).result) };
    unsafe { output.write(PresentResult::default()) };
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    let input = unsafe { &*frame };
    if !owner.backbuffer_hudless
        || input.reserved != 0
        || input.reset > 1
        || input.vsync > 1
        || input.frame_id == 0
        || input.color.is_null()
        || input.depth.is_null()
        || input.motion.is_null()
        || input.hudless.is_null()
        || input.ready == 0
        || input.ready <= owner.last_done_signalled
        || input.done <= input.ready
    {
        return INVALID_ARGUMENT;
    }
    match owner.present(&Frame::from(input), true) {
        Ok((result, phase_start)) => {
            unsafe { output.write(result) };
            owner.finish_present(phase_start);
            0
        }
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_prepare_destroy(owner: &mut Session) -> i32 {
    owner.prepare_destroy()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_bridge_destroy(session: *mut Session) -> i32 {
    let code = unsafe { &mut *session }.prepare_destroy();
    if code == 0 {
        drop(unsafe { Box::from_raw(session) });
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc,
        VirtualFree, VirtualProtect,
    };
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

    #[test]
    fn integrated_present_checks_both_headers_before_writing_status() {
        let mut system = MaybeUninit::<SYSTEM_INFO>::uninit();
        unsafe { GetSystemInfo(system.as_mut_ptr()) };
        let page = unsafe { system.assume_init() }.dwPageSize as usize;
        let pages = unsafe {
            VirtualAlloc(
                ptr::null(),
                2 * page,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        assert!(!pages.is_null());
        let second_page = unsafe { pages.cast::<u8>().add(page) };
        let mut protection = 0;
        let guarded =
            unsafe { VirtualProtect(second_page.cast(), page, PAGE_NOACCESS, &mut protection) };
        if guarded == 0 {
            unsafe { VirtualFree(pages, 0, MEM_RELEASE) };
            panic!("could not protect the page after struct_size");
        }
        let prefix = unsafe { second_page.cast::<u32>().sub(1) };
        let prefix_size = size_of::<u32>() as u32;
        unsafe { prefix.write(prefix_size) };

        let mut frame: IntegratedFrame = unsafe { std::mem::zeroed() };
        frame.struct_size = size_of::<IntegratedFrame>() as u32;
        frame.abi_version = BRIDGE_ABI_VERSION;
        let mut status = IntegratedStatus {
            struct_size: size_of::<IntegratedStatus>() as u32,
            abi_version: BRIDGE_ABI_VERSION,
            result: PresentResult {
                frames_presented: 7,
                fg_enabled: 1,
                fg_result: -9,
            },
            _reserved: 0,
        };
        let short_frame = unsafe {
            rsf_core_xess_bridge_present_integrated(ptr::null_mut(), prefix.cast(), &mut status)
        };
        let short_status = unsafe {
            rsf_core_xess_bridge_present_integrated(ptr::null_mut(), &frame, prefix.cast())
        };
        let prefix_after = unsafe { prefix.read() };
        unsafe { VirtualFree(pages, 0, MEM_RELEASE) };

        assert_eq!(short_frame, INVALID_ARGUMENT);
        assert_eq!(short_status, INVALID_ARGUMENT);
        assert_eq!(prefix_after, prefix_size);

        frame.abi_version += 1;
        let wrong_frame = unsafe {
            rsf_core_xess_bridge_present_integrated(ptr::null_mut(), &frame, &mut status)
        };
        frame.abi_version = BRIDGE_ABI_VERSION;
        status.abi_version += 1;
        let wrong_status = unsafe {
            rsf_core_xess_bridge_present_integrated(ptr::null_mut(), &frame, &mut status)
        };
        assert_eq!(wrong_frame, ABI_MISMATCH);
        assert_eq!(wrong_status, ABI_MISMATCH);
        assert_eq!(status.result.frames_presented, 7);
        assert_eq!(status.result.fg_enabled, 1);
        assert_eq!(status.result.fg_result, -9);
    }
}
