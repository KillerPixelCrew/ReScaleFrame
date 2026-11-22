// SPDX-License-Identifier: MIT
//! Owner of the FSR 4 DX12 bridge behind the Vulkan SR path.
//!
//! Compiled into the rsf-fsr4-compat archive, which the integrated Runtime and
//! the standalone VulkanStoryFsr4.dll both link. The capsule in native.rs holds
//! the typed FidelityFX descriptors, COM owners and the INT8 compatibility lease.

use crate::ffx_version::{self, MAX_VERSIONS, Version};
use core::ffi::{CStr, c_void};
use core::ptr;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT,
};
use rsf_game_sys::windows_path::absolute_wide_path;
use rsf_game_sys::ffi_util::try_box;

use crate::sys::{
    RSF_FSR4_BRIDGE_ABI_MISMATCH as ABI_MISMATCH,
    RSF_FSR4_BRIDGE_ABI_VERSION as BRIDGE_ABI_VERSION,
    RSF_FSR4_BRIDGE_INVALID_ARGUMENT as INVALID_ARGUMENT, rsf_fsr4_bridge_create as BridgeCreate,
    rsf_fsr4_bridge_frame as BridgeFrame, rsf_fsr4_bridge_info as BridgeInfo,
};
use crate::dx12_bridge::{self as dx12, can_signal_done, next_local_completion};
mod native;
use native::Capsule;
mod sdk;
mod standalone;

type Handle = *mut c_void;
const UNEXPECTED: i32 = windows::Win32::Foundation::E_UNEXPECTED.0;
const DEVICE_REMOVED: i32 = windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED.0;

// Local result codes of the bridge ABI.
const FAILED: i32 = -1;
const OUT_OF_MEMORY: i32 = -2;
const WAIT_INCOMPLETE: i32 = -2;
const NO_DIRECTORY: i32 = -6;
const NO_FSR4_PROVIDER: i32 = -9;
const UNSUPPORTED_VENDOR: i32 = -11;
const COMPATIBILITY_REFUSED: i32 = -12;

const WAIT_MS: u32 = 10_000;
const SLOTS: usize = 3;
const MAX_QUALITY: u32 = 4;

// Objects currently held by the native capsule (native.rs).
const DEVICE: u32 = 1 << 0;
const QUEUE: u32 = 1 << 1;
const EFFECT: u32 = 1 << 2;
const LOCAL_FENCE: u32 = 1 << 3;
const EVENT: u32 = 1 << 4;
const SHARED_FENCE: u32 = 1 << 5;

// Create options; native maps them to FFX_UPSCALE_ENABLE_* flags.
const DEPTH_INVERTED: u32 = 1 << 0;
const DEPTH_INFINITE: u32 = 1 << 1;
const MOTION_JITTERED: u32 = 1 << 2;

// DXGI_FORMAT values the public frame accepts.
const RGBA16_FLOAT: u32 = DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32;
const RG16_FLOAT: u32 = DXGI_FORMAT_R16G16_FLOAT.0 as u32;
const R32_FLOAT: u32 = DXGI_FORMAT_R32_FLOAT.0 as u32;

const AMD: u32 = 0x1002;
const NVIDIA: u32 = 0x10de;
const INTEL: u32 = 0x8086;
const FSR4_MAJOR: u32 = 4;
const PROVIDER_DLL: &str = "amd_fidelityfx_upscaler_dx12.dll";
const MAX_PATH: usize = 260;
const DEFAULT_FRAME_TIME_MS: f32 = 16.667;
// 3.141593f, one f32 step above pi, so a pi FOV is admitted.
#[allow(clippy::approx_constant)]
const MAX_VERTICAL_FOV: f32 = 3.141_593;

/// rsf_vk_fsr4_frame, the managed fixed-layout frame.
#[repr(C)]
struct LegacyFrame {
    color: Handle,
    depth: Handle,
    motion: Handle,
    output: Handle,
    render_width: u32,
    render_height: u32,
    display_width: u32,
    display_height: u32,
    jitter_x: f32,
    jitter_y: f32,
    delta_ms: f32,
    near_plane: f32,
    far_plane: f32,
    fov: f32,
    reset: u32,
    ready_value: u64,
    done_value: u64,
}

#[repr(C)]
struct Request {
    luid: u64,
    directory: *const u16,
    render_width: u32,
    render_height: u32,
    output_width: u32,
    output_height: u32,
    flags: u32,
}

#[repr(C)]
struct Context {
    provider: u64,
    render_width: u32,
    render_height: u32,
    output_width: u32,
    output_height: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Default)]
struct Texture {
    width: u64,
    height: u32,
    format: u32,
}

#[repr(C)]
struct Dispatch {
    color: Handle,
    depth: Handle,
    motion: Handle,
    output: Handle,
    render_width: u32,
    render_height: u32,
    output_width: u32,
    output_height: u32,
    jitter_x: f32,
    jitter_y: f32,
    motion_scale_x: f32,
    motion_scale_y: f32,
    frame_time_ms: f32,
    pre_exposure: f32,
    near_plane: f32,
    far_plane: f32,
    vertical_fov: f32,
    sharpness: f32,
    reset: u32,
    slot: u32,
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<LegacyFrame>() == 96 && offset_of!(LegacyFrame, ready_value) == 80);
    assert!(size_of::<Request>() == 40 && offset_of!(Request, flags) == 32);
    assert!(size_of::<Context>() == 32 && offset_of!(Context, flags) == 24);
    assert!(size_of::<Texture>() == 16 && offset_of!(Texture, format) == 12);
    assert!(size_of::<Dispatch>() == 96 && offset_of!(Dispatch, render_width) == 32);
    assert!(offset_of!(Dispatch, frame_time_ms) == 64 && offset_of!(Dispatch, reset) == 88);
};

fn candidate(vendor: u32) -> bool {
    // AMD natively; NVIDIA and Intel only through the INT8 compatibility lease.
    matches!(vendor, AMD | NVIDIA | INTEL)
}

/// Whether the effective provider's display name identifies FSR 4.
///
/// `parsed` is the provider query's `%u.%u.%u` result for the name. A parsed
/// name, including family 0, is FSR 4 only for family 4. Labels without that
/// shape are accepted only when they say "FSR4" or "FSR 4".
fn is_fsr4(version: &Version, parsed: bool) -> bool {
    if parsed {
        return version.major == FSR4_MAJOR;
    }
    if version.name.is_null() {
        return false;
    }
    let name = unsafe { CStr::from_ptr(version.name) }.to_bytes();
    [b"FSR4".as_slice(), b"FSR 4".as_slice()]
        .iter()
        .any(|label| name.windows(label.len()).any(|window| window == *label))
}

fn select_provider(versions: &[Version], count: u64) -> Option<u64> {
    let count = usize::try_from(count)
        .ok()
        .filter(|count| *count <= versions.len())?;
    ffx_version::select(&versions[..count], FSR4_MAJOR, 0).map(|version| version.id)
}

/// Borrows a NUL-terminated UTF-16 string without its terminator.
///
/// # Safety
/// `text` must be non-null and NUL-terminated.
unsafe fn wide<'a>(text: *const u16) -> &'a [u16] {
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    unsafe { std::slice::from_raw_parts(text, length) }
}

fn provider_path(directory: *const u16) -> Option<Vec<u16>> {
    let mut path = if directory.is_null() {
        let mut own = [0; MAX_PATH];
        let length = sdk::own_directory(&mut own);
        own[..length.min(MAX_PATH)].to_vec()
    } else {
        unsafe { wide(directory) }.to_vec()
    };
    let last = *path.last()?;
    if last != u16::from(b'\\') && last != u16::from(b'/') {
        path.push(u16::from(b'\\'));
    }
    Some(rsf_game_sys::ffi_util::wide_path(&path, PROVIDER_DLL))
}

/// Checks a size-checked call structure. `abi_version` is read only after
/// `struct_size` proves the caller allocated at least `required` bytes.
///
/// # Safety
/// `header` must point to a readable `struct_size`, and the structure must be
/// readable for the size it states.
unsafe fn check_header(header: *const u32, required: usize) -> i32 {
    let size = unsafe { header.read() };
    if (size as usize) < required {
        return INVALID_ARGUMENT;
    }
    let version = unsafe { header.add(1).read() };
    if version == BRIDGE_ABI_VERSION {
        0
    } else {
        ABI_MISMATCH
    }
}

// The host serializes every call on one bridge. Native owners stay behind
// `native` until checked destruction releases them.
pub struct Session {
    native: Box<Capsule>,
    adapter_luid: u64,
    provider_version: u64,
    vendor: u32,
    max_render: [u32; 2],
    max_output: [u32; 2],
    depth_infinite: bool,
    compatibility: bool,
    sharpness: f32,
    // Local completion-fence values: the latest submission and each allocator's.
    completion: u64,
    allocator_values: [u64; SLOTS],
    next_allocator: u32,
    // Highest shared-fence `done` value this queue has signalled.
    last_done: u64,
    // Terminal release failure. Ownership is retained once it is set.
    failure: i32,
    prepared: bool,
}

impl Session {
    fn new(native: Box<Capsule>) -> Self {
        Self {
            native,
            adapter_luid: 0,
            provider_version: 0,
            vendor: 0,
            max_render: [0; 2],
            max_output: [0; 2],
            depth_infinite: false,
            compatibility: false,
            sharpness: 0.0,
            completion: 0,
            allocator_values: [0; SLOTS],
            next_allocator: 0,
            last_done: 0,
            failure: 0,
            prepared: false,
        }
    }

    fn has(&self, objects: u32) -> bool {
        (self.native.objects() & objects) == objects
    }

    fn retain(&mut self, code: i32) -> i32 {
        self.failure = code;
        code
    }

    /// Creates the signed FSR 4 DX12 context on the adapter matching `luid`.
    ///
    /// NVIDIA and Intel take the INT8 compatibility lease before version
    /// enumeration, which filters providers by device support. The newest 4.x
    /// provider is forced and the effective provider must be that ID with an
    /// FSR 4 name; there is no fallback to an older provider.
    fn initialize(&mut self, request: &Request) -> i32 {
        let native = &mut self.native;
        let mut vendor = 0;
        let code = native.find_adapter(request.luid, &mut vendor);
        if code != 0 {
            return code;
        }
        if !candidate(vendor) {
            return UNSUPPORTED_VENDOR;
        }
        let code = native.create_device();
        if code != 0 {
            return code;
        }
        let Some(path) = provider_path(request.directory) else {
            return NO_DIRECTORY;
        };
        let code = native.load(path.as_ptr());
        if code != 0 {
            return code;
        }
        if matches!(vendor, NVIDIA | INTEL) {
            if !native.acquire_compatibility() {
                return COMPATIBILITY_REFUSED;
            }
            self.compatibility = true;
        }
        let mut versions = [Version::default(); MAX_VERSIONS];
        let mut count = MAX_VERSIONS as u64;
        let code = native.versions(&mut versions, &mut count);
        if code != 0 {
            return code;
        }
        let Some(selected) = select_provider(&versions, count) else {
            return NO_FSR4_PROVIDER;
        };
        let context = Context {
            provider: selected,
            render_width: request.render_width,
            render_height: request.render_height,
            output_width: request.output_width,
            output_height: request.output_height,
            flags: request.flags,
        };
        let code = native.create_context(&context);
        if code != 0 {
            return code;
        }
        let mut provider = Version::default();
        let mut parsed = false;
        let code = native.provider(&mut provider, &mut parsed);
        if code != 0 {
            return code;
        }
        // The signed loader can select a legacy FSR provider on older GPUs.
        // This option promises the ML upscaler, so reject that substitution.
        if provider.id != selected || !is_fsr4(&provider, parsed) {
            return NO_FSR4_PROVIDER;
        }
        self.adapter_luid = request.luid;
        self.provider_version = provider.id;
        self.vendor = vendor;
        self.max_render = [request.render_width, request.render_height];
        self.max_output = [request.output_width, request.output_height];
        self.depth_infinite = request.flags & DEPTH_INFINITE != 0;
        self.native.create_commands()
    }

    /// Waits up to ten seconds for a local completion-fence value.
    ///
    /// Device removal is a failure, not completion. A retained release failure
    /// is returned before any new wait.
    fn wait(&mut self, target: u64) -> i32 {
        if self.failure != 0 {
            return self.failure;
        }
        if target == 0 {
            return 0;
        }
        if !self.has(LOCAL_FENCE) {
            return self.retain(UNEXPECTED);
        }
        let completed = self.native.completed();
        if completed == u64::MAX {
            return self.retain(DEVICE_REMOVED);
        }
        if completed >= target {
            return 0;
        }
        if !self.has(EVENT) {
            return FAILED;
        }
        let code = self.native.wait_event(target, WAIT_MS);
        if code != 0 {
            return code;
        }
        // An earlier timed-out wait can also have signalled the shared event.
        let completed = self.native.completed();
        if completed == u64::MAX {
            return self.retain(DEVICE_REMOVED);
        }
        if completed >= target {
            0
        } else {
            WAIT_INCOMPLETE
        }
    }

    /// Drains tracked DX12 work and destroys the SDK consumer before the host
    /// retires imported resources. Failure is terminal and keeps ownership.
    fn prepare(&mut self) -> i32 {
        if self.failure != 0 {
            return self.failure;
        }
        if self.prepared {
            return 0;
        }
        let idle = self.wait(self.completion);
        if idle != 0 {
            return self.retain(idle);
        }
        if self.has(EFFECT) {
            let code = self.native.destroy_context();
            if code != 0 {
                return self.retain(code);
            }
        }
        self.prepared = true;
        0
    }

    fn admits(&self, frame: &BridgeFrame) -> bool {
        let floats = [
            frame.jitter_x,
            frame.jitter_y,
            frame.motion_scale_x,
            frame.motion_scale_y,
            frame.delta_ms,
            frame.pre_exposure,
            frame.near_plane,
            frame.far_plane,
            frame.vertical_fov,
        ];
        let [max_render_width, max_render_height] = self.max_render;
        let [max_output_width, max_output_height] = self.max_output;
        let valid = !frame.color.is_null()
            && !frame.depth.is_null()
            && !frame.motion.is_null()
            && !frame.output.is_null()
            && frame.render_width != 0
            && frame.render_height != 0
            && frame.output_width != 0
            && frame.output_height != 0
            && frame.reset <= 1
            && frame.render_width <= max_render_width
            && frame.render_height <= max_render_height
            && frame.output_width <= max_output_width
            && frame.output_height <= max_output_height
            && frame.ready_value != 0
            && frame.ready_value > self.last_done
            && frame.done_value > frame.ready_value
            && floats.iter().all(|value| value.is_finite())
            && frame.motion_scale_x != 0.0
            && frame.motion_scale_y != 0.0
            && frame.delta_ms > 0.0
            && frame.pre_exposure > 0.0
            && frame.near_plane > 0.0
            && frame.far_plane >= 0.0
            && (self.depth_infinite || frame.far_plane > frame.near_plane)
            && frame.vertical_fov > 0.0
            && frame.vertical_fov < MAX_VERTICAL_FOV;
        if !valid {
            return false;
        }
        let describe = native::describe;
        let color = describe(frame.color);
        let depth = describe(frame.depth);
        let motion = describe(frame.motion);
        let output = describe(frame.output);
        color.width == u64::from(frame.render_width)
            && color.height == frame.render_height
            && color.format == RGBA16_FLOAT
            && depth.width == color.width
            && depth.height == color.height
            && depth.format == R32_FLOAT
            && motion.width == color.width
            && motion.height == color.height
            && motion.format == RG16_FLOAT
            && output.width == u64::from(frame.output_width)
            && output.height == frame.output_height
            && output.format == RGBA16_FLOAT
    }

    /// Records and submits reconstruction after a GPU wait for `ready`.
    ///
    /// Shared images enter and leave COMMON. The local completion value is
    /// tracked immediately after submission; the shared `done` signal follows.
    fn submit(&mut self, mut dispatch: Dispatch, ready: u64, done: u64) -> i32 {
        if dispatch.color.is_null()
            || dispatch.depth.is_null()
            || dispatch.motion.is_null()
            || dispatch.output.is_null()
            || ready == 0
            || done == 0
            || !self.has(SHARED_FENCE)
        {
            return FAILED;
        }
        let Some(completed) = next_local_completion(self.completion) else {
            return UNEXPECTED;
        };
        let slot = (self.next_allocator % SLOTS as u32) as usize;
        self.next_allocator = self.next_allocator.wrapping_add(1);
        let idle = self.wait(self.allocator_values[slot]);
        if idle != 0 {
            return idle;
        }
        dispatch.slot = slot as u32;
        dispatch.sharpness = self.sharpness;
        let code = self.native.record(&dispatch);
        if code != 0 {
            return code;
        }
        let code = self.native.wait_shared(ready);
        if code != 0 {
            return code;
        }
        self.native.execute();
        self.completion = completed;
        self.allocator_values[slot] = completed;
        let handoff = self.native.signal(done, true);
        if handoff == 0 {
            self.last_done = done;
        }
        let retired = self.native.signal(completed, false);
        if retired != 0 {
            return self.retain(retired);
        }
        if handoff != 0 {
            return self.retain(handoff);
        }
        0
    }

    /// Releases a host continuation that may already wait for `done`.
    ///
    /// Every failure still waits for producer readiness before signalling.
    fn finish(&mut self, ready: u64, done: u64, result: i32) -> i32 {
        if !can_signal_done(ready, done, self.last_done)
            || !self.has(QUEUE | SHARED_FENCE)
        {
            return result;
        }
        let wait = self.native.wait_shared(ready);
        if wait != 0 {
            return self.retain(wait);
        }
        let handoff = self.native.signal(done, true);
        if handoff == 0 {
            self.last_done = done;
        }
        let Some(completed) = next_local_completion(self.completion) else {
            return self.retain(UNEXPECTED);
        };
        self.completion = completed;
        let retired = self.native.signal(completed, false);
        if retired != 0 {
            return self.retain(retired);
        }
        if handoff != 0 {
            return self.retain(handoff);
        }
        result
    }

    fn evaluate(&mut self, dispatch: Dispatch, ready: u64, done: u64) -> i32 {
        let result = self.submit(dispatch, ready, done);
        self.finish(ready, done, result)
    }
}

/// # Safety
/// `output` must be writable.
unsafe fn create(request: &Request, output: *mut *mut Session) -> i32 {
    if request.luid == 0
        || output.is_null()
        || request.render_width == 0
        || request.render_height == 0
        || request.output_width == 0
        || request.output_height == 0
    {
        return FAILED;
    }
    unsafe { output.write(ptr::null_mut()) };
    let Some(native) = try_box(Capsule::EMPTY) else {
        return OUT_OF_MEMORY;
    };
    let Some(session) = try_box(Session::new(native)) else {
        return OUT_OF_MEMORY;
    };
    let session = Box::into_raw(session);
    let code = unsafe { &mut *session }.initialize(request);
    if code != 0 {
        // A failed checked cleanup returns the retained owner to the caller.
        if unsafe { destroy(session) } != 0 {
            unsafe { output.write(session) };
        }
        return code;
    }
    unsafe { output.write(session) };
    0
}

/// # Safety
/// `setup` and `output` follow RsfFsr4BridgeCreate.
pub unsafe fn create_public(setup: *const BridgeCreate, output: *mut *mut Session) -> i32 {
    if output.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { output.write(ptr::null_mut()) };
    if setup.is_null() {
        return INVALID_ARGUMENT;
    }
    let check = unsafe { check_header(setup.cast(), size_of::<BridgeCreate>()) };
    if check != 0 {
        return check;
    }
    let setup = unsafe { &*setup };
    if !unsafe { absolute_wide_path(setup.runtime_directory, 32767) }
        || setup.reserved != 0
        || setup.depth_inverted > 1
        || setup.depth_infinite > 1
        || setup.motion_jittered > 1
    {
        return INVALID_ARGUMENT;
    }
    let mut flags = 0;
    if setup.depth_inverted != 0 {
        flags |= DEPTH_INVERTED;
    }
    if setup.depth_infinite != 0 {
        flags |= DEPTH_INFINITE;
    }
    if setup.motion_jittered != 0 {
        flags |= MOTION_JITTERED;
    }
    let request = Request {
        luid: setup.adapter_luid,
        directory: setup.runtime_directory,
        render_width: setup.render_width,
        render_height: setup.render_height,
        output_width: setup.output_width,
        output_height: setup.output_height,
        flags,
    };
    unsafe { create(&request, output) }
}

/// # Safety
/// Non-null pointers must be a live bridge and a writable info structure.
pub unsafe fn info(session: *const Session, info: *mut BridgeInfo) -> i32 {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if info.is_null() {
        return INVALID_ARGUMENT;
    }
    let check = unsafe { check_header(info.cast(), size_of::<BridgeInfo>()) };
    if check != 0 {
        return check;
    }
    let info = unsafe { &mut *info };
    info.adapter_luid = session.adapter_luid;
    info.provider_version = session.provider_version;
    info.vendor_id = session.vendor;
    info.int8_compatibility = u32::from(session.compatibility);
    0
}

/// # Safety
/// A non-null `session` must be a live bridge.
pub unsafe fn sharpness(session: *mut Session, amount: f32) -> i32 {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    if !(0.0..=1.0).contains(&amount) {
        return INVALID_ARGUMENT;
    }
    session.sharpness = amount;
    0
}

/// # Safety
/// Non-null pointers must be a live bridge and writable outputs.
pub unsafe fn plan(
    session: *mut Session,
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    if width.is_null() || height.is_null() || quality > MAX_QUALITY {
        return INVALID_ARGUMENT;
    }
    unsafe {
        width.write(0);
        height.write(0);
    }
    let [output_width, output_height] = session.max_output;
    session
        .native
        .plan(quality, output_width, output_height, width, height)
}

/// # Safety
/// Non-null pointers must be a live bridge and writable outputs.
pub unsafe fn create_image(
    session: *mut Session,
    width: u32,
    height: u32,
    format: u32,
    writable: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return FAILED;
    };
    if width == 0 || height == 0 || handle.is_null() || resource.is_null() {
        return FAILED;
    }
    unsafe {
        handle.write(ptr::null_mut());
        resource.write(ptr::null_mut());
    }
    if !session.has(DEVICE) {
        return FAILED;
    }
    session.native.create_image(width, height, format, writable, handle, resource)
}

/// # Safety
/// Non-null pointers must be a live bridge and a writable handle.
pub unsafe fn create_fence(
    session: *mut Session,
    handle: *mut Handle,
) -> i32 {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return FAILED;
    };
    if handle.is_null() {
        return FAILED;
    }
    unsafe { handle.write(ptr::null_mut()) };
    if !session.has(DEVICE) {
        return FAILED;
    }
    session.native.create_fence(handle)
}

fn frame_time_ms(delta: f32) -> f32 {
    if delta > 0.0 {
        delta
    } else {
        DEFAULT_FRAME_TIME_MS
    }
}

/// # Safety
/// Non-null pointers must be a live bridge and a readable legacy frame.
unsafe fn evaluate_legacy(
    session: *mut Session,
    frame: *const LegacyFrame,
) -> i32 {
    let (Some(session), Some(frame)) = (unsafe { session.as_mut() }, unsafe { frame.as_ref() })
    else {
        return FAILED;
    };
    // A prepared bridge has drained and destroyed its SDK consumer.
    if session.prepared {
        return FAILED;
    }
    let dispatch = Dispatch {
        color: frame.color,
        depth: frame.depth,
        motion: frame.motion,
        output: frame.output,
        render_width: frame.render_width,
        render_height: frame.render_height,
        output_width: frame.display_width,
        output_height: frame.display_height,
        jitter_x: frame.jitter_x,
        jitter_y: frame.jitter_y,
        motion_scale_x: 1.0,
        motion_scale_y: 1.0,
        frame_time_ms: frame_time_ms(frame.delta_ms),
        pre_exposure: 1.0,
        near_plane: frame.near_plane,
        far_plane: frame.far_plane,
        vertical_fov: frame.fov,
        sharpness: 0.0,
        reset: frame.reset,
        slot: 0,
    };
    session.evaluate(dispatch, frame.ready_value, frame.done_value)
}

/// # Safety
/// Non-null pointers must be a live bridge and a readable frame header.
pub unsafe fn evaluate(
    session: *mut Session,
    frame: *const BridgeFrame,
) -> i32 {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    if frame.is_null() {
        return INVALID_ARGUMENT;
    }
    let check = unsafe { check_header(frame.cast(), size_of::<BridgeFrame>()) };
    if check != 0 {
        return check;
    }
    let frame = unsafe { &*frame };
    if session.prepared {
        return INVALID_ARGUMENT;
    }
    if !session.admits(frame) {
        return session.finish(frame.ready_value, frame.done_value, INVALID_ARGUMENT);
    }
    let dispatch = Dispatch {
        color: frame.color,
        depth: frame.depth,
        motion: frame.motion,
        output: frame.output,
        render_width: frame.render_width,
        render_height: frame.render_height,
        output_width: frame.output_width,
        output_height: frame.output_height,
        jitter_x: frame.jitter_x,
        jitter_y: frame.jitter_y,
        motion_scale_x: frame.motion_scale_x,
        motion_scale_y: frame.motion_scale_y,
        frame_time_ms: frame_time_ms(frame.delta_ms),
        pre_exposure: frame.pre_exposure,
        near_plane: frame.near_plane,
        // Zero far means infinite depth.
        far_plane: if frame.far_plane > 0.0 {
            frame.far_plane
        } else {
            f32::MAX
        },
        vertical_fov: frame.vertical_fov,
        sharpness: 0.0,
        reset: frame.reset,
        slot: 0,
    };
    session.evaluate(dispatch, frame.ready_value, frame.done_value)
}

/// # Safety
/// A non-null `session` must be a live bridge.
unsafe fn wait_idle(session: *mut Session) -> i32 {
    unsafe { session.as_mut() }.map_or(0, |session| session.wait(session.completion))
}

/// # Safety
/// A non-null `session` must be a live bridge.
pub unsafe fn prepare_destroy(session: *mut Session) -> i32 {
    unsafe { session.as_mut() }.map_or(0, Session::prepare)
}

/// Prepares, drops the INT8 lease after the SDK context, then releases the
/// native owners and the bridge. The caller must already have drained Vulkan
/// users and disposed of imported shared images and fence imports. Failure
/// keeps the bridge attached.
///
/// # Safety
/// A non-null `session` must be a live bridge with no other users.
pub unsafe fn destroy(session: *mut Session) -> i32 {
    let Some(owner) = (unsafe { session.as_mut() }) else {
        return 0;
    };
    let code = owner.prepare();
    if code != 0 {
        return code;
    }
    // The SDK context is destroyed; dropping the capsule releases the lease first,
    // which restores native capability answers, then the native owners.
    drop(unsafe { Box::from_raw(session) });
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: u64, name: &'static CStr, major: u32) -> Version {
        Version {
            id,
            name: name.as_ptr(),
            major,
            ..Version::default()
        }
    }

    #[test]
    fn accepts_only_fsr4_provider_names() {
        assert!(is_fsr4(&version(1, c"4.0.2", 4), true));
        assert!(is_fsr4(&version(1, c"AMD FSR4 upscaler", 0), false));
        assert!(is_fsr4(&version(1, c"FSR 4", 0), false));
        assert!(!is_fsr4(&version(1, c"3.1.4", 3), true));
        assert!(!is_fsr4(&version(1, c"FSR3", 0), false));
        assert!(!is_fsr4(&Version::default(), false));
        // A parsed family-0 name stays family 0 whatever label follows it.
        assert!(!is_fsr4(&version(1, c"0.1.2 FSR4", 0), true));
        assert!(!is_fsr4(&version(1, c"0.0.0 FSR 4", 0), true));
    }

    #[test]
    fn reads_the_abi_version_only_after_the_size_admits_it() {
        // Only struct_size is allocated: a short size must not read past it.
        let short = [7_u32];
        assert_eq!(unsafe { check_header(short.as_ptr(), 8) }, INVALID_ARGUMENT);
        let mismatch = [8_u32, 2];
        assert_eq!(unsafe { check_header(mismatch.as_ptr(), 8) }, ABI_MISMATCH);
        let current = [8_u32, BRIDGE_ABI_VERSION];
        assert_eq!(unsafe { check_header(current.as_ptr(), 8) }, 0);
    }

    #[test]
    fn selects_the_newest_family_four_provider_within_the_answer() {
        let mut versions = [Version::default(); MAX_VERSIONS];
        versions[0] = Version {
            id: 7,
            major: 4,
            minor: 0,
            patch: 3,
            ..Version::default()
        };
        versions[1] = Version {
            id: 9,
            major: 4,
            minor: 1,
            patch: 0,
            ..Version::default()
        };
        versions[2] = Version {
            id: 11,
            major: 3,
            minor: 9,
            patch: 9,
            ..Version::default()
        };
        assert_eq!(select_provider(&versions, 3), Some(9));
        assert_eq!(select_provider(&versions, 1), Some(7));
        assert_eq!(select_provider(&versions, 0), None);
        assert_eq!(select_provider(&versions, MAX_VERSIONS as u64 + 1), None);
    }

    #[test]
    fn requires_an_absolute_directory() {
        let path = rsf_game_sys::ffi_util::wide;
        for good in ["C:\\Game", "d:/runtime", "\\\\server\\share"] {
            assert!(
                unsafe { absolute_wide_path(path(good).as_ptr(), 32767) },
                "{good}"
            );
        }
        for bad in ["", "relative", "C:", "C:relative", "\\single"] {
            assert!(
                !unsafe { absolute_wide_path(path(bad).as_ptr(), 32767) },
                "{bad}"
            );
        }
        assert!(!unsafe { absolute_wide_path(ptr::null(), 32767) });
    }

    unsafe extern "system" {
        fn RaiseException(code: u32, flags: u32, count: u32, arguments: *const usize);
    }

    /// A fake ffxQuery that throws: MSVC raises a C++ throw as SEH code 0xE06D7363.
    unsafe extern "C-unwind" fn throwing_query(
        _: *mut crate::ffx_abi::ffxContext,
        _: *mut crate::ffx_abi::ffxQueryDescHeader,
    ) -> crate::ffx_abi::ffxReturnCode_t {
        unsafe { RaiseException(0xE06D_7363, 0, 0, ptr::null()) };
        0
    }

    struct Unwound<'a>(&'a mut bool);

    impl Drop for Unwound<'_> {
        fn drop(&mut self) {
            *self.0 = true;
        }
    }

    #[test]
    fn the_runtime_guard_contains_an_exception_thrown_by_the_sdk() {
        // The Runtime calls the bridge through this guard (vulkan_fsr4_sr.rs).
        const THROWN: i32 = -77;
        let (mut width, mut height) = (7_u32, 7_u32);
        let mut unwound = false;
        let result = rsf_streamline_vk::sdk_api::guarded_or(THROWN, || {
            let _frame = Unwound(&mut unwound);
            unsafe {
                sdk::plan(
                    Some(throwing_query),
                    ptr::null_mut(),
                    2,
                    1920,
                    1080,
                    &mut width,
                    &mut height,
                )
            }
        });
        assert_eq!(result, THROWN);
        // The Rust frames between the guard and the SDK entry ran their cleanup.
        assert!(unwound);
    }

    #[test]
    fn null_bridges_and_vendor_policy() {
        unsafe {
            assert_eq!(
                sharpness(ptr::null_mut(), 0.5),
                INVALID_ARGUMENT
            );
            assert_eq!(wait_idle(ptr::null_mut()), 0);
            assert_eq!(prepare_destroy(ptr::null_mut()), 0);
            assert_eq!(destroy(ptr::null_mut()), 0);
            assert_eq!(
                evaluate_legacy(ptr::null_mut(), ptr::null()),
                FAILED
            );
        }
        assert!(candidate(0x10de));
        assert!(!candidate(0x5143));
    }
}
