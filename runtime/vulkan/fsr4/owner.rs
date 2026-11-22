// SPDX-License-Identifier: MIT
//! Owner of the FSR 4 DX12 bridge behind the Vulkan SR path.
//!
//! Compiled into the rsf-fsr4-compat archive, which the integrated Runtime and
//! the standalone VulkanStoryFsr4.dll both link. bridge.cpp keeps the typed
//! FidelityFX descriptor and INT8 compatibility boundaries.

use crate::ffx_version::{self, MAX_VERSIONS, Version};
use core::ffi::{CStr, c_void};
use core::ptr;
use rsf_game_sys::windows_path::absolute_wide_path;
use std::alloc::{Layout, alloc};

#[path = "bridge_abi.rs"]
mod bridge_abi;
use bridge_abi::{
    RSF_FSR4_BRIDGE_ABI_MISMATCH as ABI_MISMATCH,
    RSF_FSR4_BRIDGE_ABI_VERSION as BRIDGE_ABI_VERSION,
    RSF_FSR4_BRIDGE_INVALID_ARGUMENT as INVALID_ARGUMENT, rsf_fsr4_bridge_create as BridgeCreate,
    rsf_fsr4_bridge_frame as BridgeFrame, rsf_fsr4_bridge_info as BridgeInfo,
};
#[path = "completion.rs"]
mod completion;
use completion::next_local_completion;
#[cfg(windows)]
#[path = "sdk.rs"]
mod sdk;
#[cfg(windows)]
#[path = "standalone.rs"]
mod standalone;
#[path = "../timeline_pair.rs"]
mod timeline_pair;

type Handle = *mut c_void;
type Native = *mut c_void;
type Step = unsafe extern "C" fn(Native) -> i32;

const PLATFORM_ABI_VERSION: u32 = 1;
const UNEXPECTED: i32 = 0x8000_ffff_u32 as i32; // E_UNEXPECTED
const DEVICE_REMOVED: i32 = 0x887a_0005_u32 as i32; // DXGI_ERROR_DEVICE_REMOVED

// Local result codes kept from the previous C++ owner.
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

// Native objects (bridge_platform.h).
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

// DXGI_FORMAT values the public frame accepts; bridge.cpp asserts them.
const RGBA16_FLOAT: u32 = 10;
const RG16_FLOAT: u32 = 34;
const R32_FLOAT: u32 = 41;

const AMD: u32 = 0x1002;
const NVIDIA: u32 = 0x10de;
const INTEL: u32 = 0x8086;
const FSR4_MAJOR: u32 = 4;
const PROVIDER_DLL: &str = "amd_fidelityfx_upscaler_dx12.dll";
const MAX_PATH: usize = 260;
const DEFAULT_FRAME_TIME_MS: f32 = 16.667;
// The previous C++ bound, 3.141593f, one f32 step above pi.
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

#[repr(C)]
struct Platform {
    struct_size: u32,
    abi_version: u32,
    allocate: unsafe extern "C" fn() -> Native,
    release: unsafe extern "C" fn(Native),
    objects: unsafe extern "C" fn(Native) -> u32,
    find_adapter: unsafe extern "C" fn(Native, u64, *mut u32) -> i32,
    create_device: Step,
    own_directory: unsafe extern "C" fn(*mut u16, u32) -> u32,
    load: unsafe extern "C" fn(Native, *const u16) -> i32,
    acquire_compatibility: unsafe extern "C" fn(Native) -> u32,
    release_compatibility: unsafe extern "C" fn(Native),
    versions: unsafe extern "C" fn(Native, *mut Version, *mut u64) -> i32,
    create_context: unsafe extern "C" fn(Native, *const Context) -> i32,
    provider: unsafe extern "C" fn(Native, *mut Version, *mut u32) -> i32,
    create_commands: Step,
    plan: unsafe extern "C" fn(Native, u32, u32, u32, *mut u32, *mut u32) -> i32,
    create_image: unsafe extern "C" fn(Native, u32, u32, u32, u32, *mut Handle, *mut Handle) -> i32,
    create_fence: unsafe extern "C" fn(Native, *mut Handle) -> i32,
    describe: unsafe extern "C" fn(Handle, *mut Texture),
    completed: unsafe extern "C" fn(Native) -> u64,
    wait_event: unsafe extern "C" fn(Native, u64, u32) -> i32,
    record: unsafe extern "C" fn(Native, *const Dispatch) -> i32,
    wait_shared: unsafe extern "C" fn(Native, u64) -> i32,
    execute: unsafe extern "C" fn(Native),
    signal: unsafe extern "C" fn(Native, u64, u32) -> i32,
    destroy_context: Step,
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<LegacyFrame>() == 96 && offset_of!(LegacyFrame, ready_value) == 80);
    assert!(size_of::<Request>() == 40 && offset_of!(Request, flags) == 32);
    assert!(size_of::<Context>() == 32 && offset_of!(Context, flags) == 24);
    assert!(size_of::<Texture>() == 16 && offset_of!(Texture, format) == 12);
    assert!(size_of::<Dispatch>() == 96 && offset_of!(Dispatch, render_width) == 32);
    assert!(offset_of!(Dispatch, frame_time_ms) == 64 && offset_of!(Dispatch, reset) == 88);
    assert!(size_of::<Platform>() == 8 + 24 * size_of::<Handle>());
    assert!(offset_of!(Platform, allocate) == 8 && offset_of!(Platform, create_context) == 88);
    assert!(offset_of!(Platform, destroy_context) == 192);
};

fn candidate(vendor: u32) -> bool {
    // AMD natively; NVIDIA and Intel only through the INT8 compatibility lease.
    matches!(vendor, AMD | NVIDIA | INTEL)
}

/// Whether the effective provider's display name identifies FSR 4.
///
/// `parsed` is the native leaf's `%u.%u.%u` result for the name. A parsed
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

fn provider_path(ops: &Platform, directory: *const u16) -> Option<Vec<u16>> {
    let mut path = if directory.is_null() {
        let mut own = [0; MAX_PATH];
        let length = unsafe { (ops.own_directory)(own.as_mut_ptr(), MAX_PATH as u32) } as usize;
        own[..length.min(MAX_PATH)].to_vec()
    } else {
        unsafe { wide(directory) }.to_vec()
    };
    let last = *path.last()?;
    if last != u16::from(b'\\') && last != u16::from(b'/') {
        path.push(u16::from(b'\\'));
    }
    path.extend(PROVIDER_DLL.encode_utf16());
    path.push(0);
    Some(path)
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

/// # Safety
/// A non-null table must stay valid for the process lifetime.
unsafe fn platform(ops: *const Platform) -> Option<&'static Platform> {
    let ops = unsafe { ops.as_ref() }?;
    (ops.struct_size as usize >= size_of::<Platform>() && ops.abi_version == PLATFORM_ABI_VERSION)
        .then_some(ops)
}

// The host serializes every call on one bridge. Native owners stay behind
// `native` until checked destruction releases them.
struct Session {
    ops: &'static Platform,
    native: Native,
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
    fn new(ops: &'static Platform, native: Native) -> Self {
        Self {
            ops,
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
        (unsafe { (self.ops.objects)(self.native) } & objects) == objects
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
        let ops = self.ops;
        let native = self.native;
        let mut vendor = 0;
        let code = unsafe { (ops.find_adapter)(native, request.luid, &mut vendor) };
        if code != 0 {
            return code;
        }
        if !candidate(vendor) {
            return UNSUPPORTED_VENDOR;
        }
        let code = unsafe { (ops.create_device)(native) };
        if code != 0 {
            return code;
        }
        let Some(path) = provider_path(ops, request.directory) else {
            return NO_DIRECTORY;
        };
        let code = unsafe { (ops.load)(native, path.as_ptr()) };
        if code != 0 {
            return code;
        }
        if matches!(vendor, NVIDIA | INTEL) {
            if unsafe { (ops.acquire_compatibility)(native) } == 0 {
                return COMPATIBILITY_REFUSED;
            }
            self.compatibility = true;
        }
        let mut versions = [Version::default(); MAX_VERSIONS];
        let mut count = MAX_VERSIONS as u64;
        let code = unsafe { (ops.versions)(native, versions.as_mut_ptr(), &mut count) };
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
        let code = unsafe { (ops.create_context)(native, &context) };
        if code != 0 {
            return code;
        }
        let mut provider = Version::default();
        let mut parsed = 0;
        let code = unsafe { (ops.provider)(native, &mut provider, &mut parsed) };
        if code != 0 {
            return code;
        }
        // The signed loader can select a legacy FSR provider on older GPUs.
        // This option promises the ML upscaler, so reject that substitution.
        if provider.id != selected || !is_fsr4(&provider, parsed != 0) {
            return NO_FSR4_PROVIDER;
        }
        self.adapter_luid = request.luid;
        self.provider_version = provider.id;
        self.vendor = vendor;
        self.max_render = [request.render_width, request.render_height];
        self.max_output = [request.output_width, request.output_height];
        self.depth_infinite = request.flags & DEPTH_INFINITE != 0;
        unsafe { (ops.create_commands)(native) }
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
        let ops = self.ops;
        let completed = unsafe { (ops.completed)(self.native) };
        if completed == u64::MAX {
            return self.retain(DEVICE_REMOVED);
        }
        if completed >= target {
            return 0;
        }
        if !self.has(EVENT) {
            return FAILED;
        }
        let code = unsafe { (ops.wait_event)(self.native, target, WAIT_MS) };
        if code != 0 {
            return code;
        }
        // An earlier timed-out wait can also have signalled the shared event.
        let completed = unsafe { (ops.completed)(self.native) };
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
            let code = unsafe { (self.ops.destroy_context)(self.native) };
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
        let describe = |resource| {
            let mut texture = Texture::default();
            unsafe { (self.ops.describe)(resource, &mut texture) };
            texture
        };
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
        let ops = self.ops;
        let code = unsafe { (ops.record)(self.native, &dispatch) };
        if code != 0 {
            return code;
        }
        let code = unsafe { (ops.wait_shared)(self.native, ready) };
        if code != 0 {
            return code;
        }
        unsafe { (ops.execute)(self.native) };
        self.completion = completed;
        self.allocator_values[slot] = completed;
        let handoff = unsafe { (ops.signal)(self.native, done, 1) };
        if handoff == 0 {
            self.last_done = done;
        }
        let retired = unsafe { (ops.signal)(self.native, completed, 0) };
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
        if !timeline_pair::can_signal_done(ready, done, self.last_done)
            || !self.has(QUEUE | SHARED_FENCE)
        {
            return result;
        }
        let ops = self.ops;
        let wait = unsafe { (ops.wait_shared)(self.native, ready) };
        if wait != 0 {
            return self.retain(wait);
        }
        let handoff = unsafe { (ops.signal)(self.native, done, 1) };
        if handoff == 0 {
            self.last_done = done;
        }
        let Some(completed) = next_local_completion(self.completion) else {
            return self.retain(UNEXPECTED);
        };
        self.completion = completed;
        let retired = unsafe { (ops.signal)(self.native, completed, 0) };
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
/// `ops` must be the static native table; `output` must be writable.
unsafe fn create(ops: *const Platform, request: &Request, output: *mut *mut Session) -> i32 {
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
    let Some(ops) = (unsafe { platform(ops) }) else {
        return FAILED;
    };
    let native = unsafe { (ops.allocate)() };
    if native.is_null() {
        return OUT_OF_MEMORY;
    }
    let session = unsafe { alloc(Layout::new::<Session>()) }.cast::<Session>();
    if session.is_null() {
        unsafe { (ops.release)(native) };
        return OUT_OF_MEMORY;
    }
    unsafe { session.write(Session::new(ops, native)) };
    let code = unsafe { &mut *session }.initialize(request);
    if code != 0 {
        // A failed checked cleanup returns the retained owner to the caller.
        if unsafe { rsf_core_fsr4sr_destroy(session) } != 0 {
            unsafe { output.write(session) };
        }
        return code;
    }
    unsafe { output.write(session) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fsr4sr_candidate_vendor(vendor: u32) -> u32 {
    u32::from(candidate(vendor))
}

/// # Safety
/// `request` and `output` follow rsf_vk_fsr4_create; `ops` is the static table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_create(
    ops: *const Platform,
    request: *const Request,
    output: *mut *mut Session,
) -> i32 {
    let Some(request) = (unsafe { request.as_ref() }) else {
        return FAILED;
    };
    unsafe { create(ops, request, output) }
}

/// # Safety
/// `setup` and `output` follow RsfFsr4BridgeCreate; `ops` is the static table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_create_public(
    ops: *const Platform,
    setup: *const BridgeCreate,
    output: *mut *mut Session,
) -> i32 {
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
    unsafe { create(ops, &request, output) }
}

/// # Safety
/// Non-null pointers must be a live bridge and a writable info structure.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_info(session: *const Session, info: *mut BridgeInfo) -> i32 {
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
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_sharpness(session: *mut Session, amount: f32) -> i32 {
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
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_plan(
    session: *mut Session,
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(session) = (unsafe { session.as_ref() }) else {
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
    unsafe {
        (session.ops.plan)(
            session.native,
            quality,
            output_width,
            output_height,
            width,
            height,
        )
    }
}

/// # Safety
/// Non-null pointers must be a live bridge and writable outputs.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_create_image(
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
    unsafe {
        (session.ops.create_image)(
            session.native,
            width,
            height,
            format,
            writable,
            handle,
            resource,
        )
    }
}

/// # Safety
/// Non-null pointers must be a live bridge and a writable handle.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_create_fence(
    session: *mut Session,
    handle: *mut Handle,
) -> i32 {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return FAILED;
    };
    if handle.is_null() {
        return FAILED;
    }
    unsafe { handle.write(ptr::null_mut()) };
    if !session.has(DEVICE) {
        return FAILED;
    }
    unsafe { (session.ops.create_fence)(session.native, handle) }
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
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_evaluate_legacy(
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
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_evaluate(
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
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_wait_idle(session: *mut Session) -> i32 {
    unsafe { session.as_mut() }.map_or(0, |session| session.wait(session.completion))
}

/// # Safety
/// A non-null `session` must be a live bridge.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_prepare_destroy(session: *mut Session) -> i32 {
    unsafe { session.as_mut() }.map_or(0, Session::prepare)
}

/// Prepares, drops the INT8 lease after the SDK context, then releases the
/// native owners and the bridge. The caller must already have drained Vulkan
/// users and disposed of imported shared images and fence imports. Failure
/// keeps the bridge attached.
///
/// # Safety
/// A non-null `session` must be a live bridge with no other users.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_destroy(session: *mut Session) -> i32 {
    let Some(owner) = (unsafe { session.as_mut() }) else {
        return 0;
    };
    let code = owner.prepare();
    if code != 0 {
        return code;
    }
    // The SDK context is destroyed; dropping the lease restores native capability answers.
    unsafe { (owner.ops.release_compatibility)(owner.native) };
    owner.compatibility = false;
    unsafe { (owner.ops.release)(owner.native) };
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
        let path = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<_>>();
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

    #[test]
    fn null_bridges_and_vendor_policy() {
        unsafe {
            assert_eq!(
                rsf_core_fsr4sr_sharpness(ptr::null_mut(), 0.5),
                INVALID_ARGUMENT
            );
            assert_eq!(rsf_core_fsr4sr_wait_idle(ptr::null_mut()), 0);
            assert_eq!(rsf_core_fsr4sr_prepare_destroy(ptr::null_mut()), 0);
            assert_eq!(rsf_core_fsr4sr_destroy(ptr::null_mut()), 0);
            assert_eq!(
                rsf_core_fsr4sr_evaluate_legacy(ptr::null_mut(), ptr::null()),
                FAILED
            );
        }
        assert_eq!(rsf_core_fsr4sr_candidate_vendor(0x10de), 1);
        assert_eq!(rsf_core_fsr4sr_candidate_vendor(0x5143), 0);
    }
}
