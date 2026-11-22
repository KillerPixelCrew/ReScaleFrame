// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::dx12_vulkan_interop::{
    BGRA8_UNORM as VK_BGRA8_UNORM, R32_FLOAT as VK_R32_FLOAT, RG16_FLOAT as VK_RG16_FLOAT, checked,
};
use crate::ffx_version::{self, MAX_VERSIONS, Version};
use crate::sr_session::has_capacity;
use rsf_game_sys::windows_path::absolute_wide_path;
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION,
    rsf_frame_record as Record,
};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering::SeqCst};

#[path = "../../vulkan/fsr4/completion.rs"]
mod completion;
use completion::next_local_completion;
#[path = "../../vulkan/fsr4-fg/sdk_commands.rs"]
mod commands;
#[path = "../../vulkan/dx12_bridge_commands.rs"]
mod dx12;
#[path = "../../vulkan/fsr4-fg/sdk.rs"]
mod sdk;
#[path = "fsr4_fg_bridge/state_fixture.rs"]
mod state_fixture;
#[path = "../../vulkan/timeline_pair.rs"]
mod timeline_pair;

type Handle = *mut c_void;

#[path = "../../vulkan/fsr4-fg/bridge_abi.rs"]
mod bridge_abi;
use bridge_abi::{RSF_FSR4FG_ABI_MISMATCH as ABI_MISMATCH, RSF_FSR4FG_INVALID_ARGUMENT as INVALID};
pub(super) use bridge_abi::{
    RSF_FSR4FG_BRIDGE_ABI_VERSION as BRIDGE_ABI, RSF_FSR4FG_NOT_SUPPORTED as UNSUPPORTED,
    rsf_fsr4fg_create as BridgeCreate, rsf_fsr4fg_frame as BridgeFrame,
    rsf_fsr4fg_info as BridgeInfo, rsf_fsr4fg_present_status as BridgeStatus,
};

const OUT_OF_MEMORY: i32 = 0x8007_000e_u32 as i32;
const UNEXPECTED: i32 = 0x8000_ffff_u32 as i32;

const DEVICE: u32 = 1 << 0;
const GENERATION: u32 = 1 << 1;
const CHAIN: u32 = 1 << 2;
const SWAPCHAIN: u32 = 1 << 3;
const LOCAL_FENCE: u32 = 1 << 4;
const SHARED_FENCE: u32 = 1 << 5;

const TEXTURE_2D: u32 = 3; // D3D12_RESOURCE_DIMENSION_TEXTURE2D
const BGRA8_UNORM: u32 = 87;
const R32_FLOAT: u32 = 41;
const RG16_FLOAT: u32 = 34;
const FEATURE_LEVEL_12_0: u32 = 0xc000;
const COMMAND_LIST_DIRECT: u32 = 0;
const QUEUE_PRIORITY_NORMAL: i32 = 0;
const QUEUE_FLAGS_NONE: u32 = 0;
const FENCE_FLAGS_NONE: u32 = 0;
const SWAPCHAIN_RENDER_TARGET_OUTPUT: u32 = 0x20;
const SWAPCHAIN_SCALING_STRETCH: u32 = 0;
const SWAPCHAIN_FLIP_DISCARD: u32 = 4;
const SWAPCHAIN_ALPHA_UNSPECIFIED: u32 = 0;
const MAX_DIRECTORY: usize = 32700;
const PROVIDER_DLL: &str = "amd_fidelityfx_framegeneration_dx12.dll";
const PROVIDER_PATH_CAPACITY: usize = MAX_DIRECTORY + 1 + PROVIDER_DLL.len() + 1;

#[repr(C)]
#[derive(Default)]
struct Texture {
    width: u64,
    height: u32,
    format: u32,
    dimension: u32,
    layers: u32,
    samples: u32,
}

#[repr(C)]
struct QueueDesc {
    list_type: u32,
    priority: i32,
    flags: u32,
    node_mask: u32,
}

#[repr(C)]
struct SwapchainDesc {
    width: u32,
    height: u32,
    format: u32,
    stereo: u32,
    sample_count: u32,
    sample_quality: u32,
    buffer_usage: u32,
    buffer_count: u32,
    scaling: u32,
    swap_effect: u32,
    alpha_mode: u32,
    flags: u32,
}

type GeneratedFn = unsafe extern "C" fn(Handle, i32, u32);
type StatusOp = unsafe extern "C" fn(Handle) -> i32;
type VoidOp = unsafe extern "C" fn(Handle);
type FenceOp = unsafe extern "C" fn(Handle, u64) -> i32;

#[repr(C)]
struct Platform {
    allocate: unsafe extern "C" fn(Handle, GeneratedFn) -> Handle,
    release: VoidOp,
    create_factory: unsafe extern "C" fn(Handle, u32) -> i32,
    find_adapter: unsafe extern "C" fn(Handle, u64) -> i32,
    get_adapter_vendor: unsafe extern "C" fn(Handle, *mut u32) -> i32,
    create_device: unsafe extern "C" fn(Handle, u32) -> i32,
    load: unsafe extern "C" fn(Handle, *const u16) -> i32,
    versions: unsafe extern "C" fn(Handle, *mut Version, *mut u64) -> i32,
    create_generation: unsafe extern "C" fn(Handle, *const BridgeCreate, u64) -> i32,
    get_provider: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    create_queue: unsafe extern "C" fn(Handle, *const QueueDesc) -> i32,
    create_allocator: unsafe extern "C" fn(Handle, u32) -> i32,
    create_command_list: unsafe extern "C" fn(Handle, u32, u32) -> i32,
    create_completion_fence: unsafe extern "C" fn(Handle, u64, u32) -> i32,
    create_event: unsafe extern "C" fn(Handle, u32, u32) -> i32,
    objects: unsafe extern "C" fn(Handle) -> u32,
    create_swapchain: unsafe extern "C" fn(Handle, Handle, *const SwapchainDesc, u32) -> i32,
    create_image: unsafe extern "C" fn(Handle, u32, u32, u32, *mut Handle, *mut Handle) -> i32,
    create_fence: unsafe extern "C" fn(Handle, *mut Handle) -> i32,
    describe_texture: unsafe extern "C" fn(Handle, *mut Texture),
    reset_commands: StatusOp,
    close_commands: StatusOp,
    configure: unsafe extern "C" fn(Handle, u64, u32, u32, u32, Handle) -> i32,
    shader_read: unsafe extern "C" fn(Handle, Handle, u32),
    prepare: unsafe extern "C" fn(Handle, *const BridgeFrame, u64, u32) -> i32,
    copy_to_backbuffer: unsafe extern "C" fn(Handle, Handle) -> i32,
    wait_shared: FenceOp,
    execute: VoidOp,
    present: unsafe extern "C" fn(Handle, u32) -> i32,
    wait_presents: StatusOp,
    signal: unsafe extern "C" fn(Handle, u64, u32) -> i32,
    wait_local: FenceOp,
    get_present_count: unsafe extern "C" fn(Handle, *mut u32) -> i32,
    destroy_generation: StatusOp,
    release_chain: VoidOp,
    destroy_swapchain: StatusOp,
}

const _: () = {
    assert!(size_of::<Platform>() == 36 * size_of::<Handle>());
    assert!(std::mem::offset_of!(Platform, create_factory) == 2 * size_of::<Handle>());
    assert!(std::mem::offset_of!(Platform, load) == 6 * size_of::<Handle>());
    assert!(std::mem::offset_of!(Platform, create_queue) == 10 * size_of::<Handle>());
    assert!(std::mem::offset_of!(Platform, create_swapchain) == 16 * size_of::<Handle>());
    assert!(std::mem::offset_of!(Platform, close_commands) == 21 * size_of::<Handle>());
    assert!(size_of::<BridgeCreate>() == 48);
    assert!(size_of::<BridgeInfo>() == 32);
    assert!(size_of::<BridgeFrame>() == 80);
    assert!(size_of::<BridgeStatus>() == 32);
    assert!(size_of::<Texture>() == 32);
    assert!(std::mem::offset_of!(Texture, dimension) == 16);
    assert!(size_of::<QueueDesc>() == 16 && align_of::<QueueDesc>() == 4);
    assert!(std::mem::offset_of!(QueueDesc, priority) == 4);
    assert!(std::mem::offset_of!(QueueDesc, node_mask) == 12);
    assert!(size_of::<SwapchainDesc>() == 48 && align_of::<SwapchainDesc>() == 4);
    assert!(std::mem::offset_of!(SwapchainDesc, sample_count) == 16);
    assert!(std::mem::offset_of!(SwapchainDesc, buffer_count) == 28);
    assert!(std::mem::offset_of!(SwapchainDesc, flags) == 44);
};

#[derive(Default)]
struct Generated {
    count: AtomicU64,
    result: AtomicI32,
}

unsafe extern "C" fn generated(user: Handle, result: i32, count: u32) {
    let generated = unsafe { &*user.cast::<Generated>() };
    generated.result.store(result, SeqCst);
    if result == 0 {
        generated.count.fetch_add(u64::from(count), SeqCst);
    }
}

struct Bridge {
    ops: &'static Platform,
    native: Handle,
    // The SDK callback borrows only this allocation. It can run while the
    // presenting thread is mutating the rest of the bridge.
    generated: *mut Generated,
    luid: u64,
    provider: u64,
    vendor: u32,
    output: [u32; 2],
    depth_inverted: bool,
    depth_infinite: bool,
    motion_jittered: bool,
    admitted: bool,
    swapchain_attempted: bool,
    configured: bool,
    prepared_destroy: bool,
    sequence: u64,
    completion: u64,
    done: u64,
    last_source: u64,
    last_present_count: u32,
    history: bool,
    terminal: i32,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        // Owners with incomplete teardown are returned to C and never reach Drop.
        if !self.native.is_null() {
            unsafe { (self.ops.release)(self.native) };
        }
        drop(unsafe { Box::from_raw(self.generated) });
    }
}

impl Bridge {
    fn new(ops: &'static Platform, luid: u64) -> Result<Box<Self>, i32> {
        let telemetry = allocate_handle(Generated::default());
        if telemetry.is_null() {
            return Err(OUT_OF_MEMORY);
        }
        let bridge = allocate_handle(Self {
            ops,
            native: ptr::null_mut(),
            generated: telemetry,
            luid,
            provider: 0,
            vendor: 0,
            output: [0; 2],
            depth_inverted: false,
            depth_infinite: false,
            motion_jittered: false,
            admitted: false,
            swapchain_attempted: false,
            configured: false,
            prepared_destroy: false,
            sequence: 0,
            completion: 0,
            done: 0,
            last_source: 0,
            last_present_count: 0,
            history: false,
            terminal: 0,
        });
        if bridge.is_null() {
            return Err(OUT_OF_MEMORY);
        }
        let mut bridge = unsafe { Box::from_raw(bridge) };
        bridge.native = unsafe { (ops.allocate)(telemetry.cast(), generated) };
        if bridge.native.is_null() {
            return Err(OUT_OF_MEMORY);
        }
        Ok(bridge)
    }

    fn open_device(&mut self) -> Result<(), i32> {
        let p = self.ops;
        let native = self.native;
        unsafe {
            checked((p.create_factory)(native, 0))?;
            checked((p.find_adapter)(native, self.luid))?;
            checked((p.get_adapter_vendor)(native, &mut self.vendor))?;
            checked((p.create_device)(native, FEATURE_LEVEL_12_0))
        }
    }

    fn discover(&mut self, directory: *const u16) -> Result<(), i32> {
        let mut versions = [Version::default(); MAX_VERSIONS];
        let mut count = MAX_VERSIONS as u64;
        let ops = self.ops;
        self.open_device()?;
        let mut path = [0; PROVIDER_PATH_CAPACITY];
        provider_path(directory, &mut path)?;
        unsafe {
            checked((ops.load)(self.native, path.as_ptr()))?;
            checked((ops.versions)(
                self.native,
                versions.as_mut_ptr(),
                &mut count,
            ))?;
        }
        let provider = select_version(&versions, count);
        if provider == 0 {
            return Err(UNSUPPORTED);
        }
        self.provider = provider;
        Ok(())
    }

    fn create_commands(&self) -> Result<(), i32> {
        let queue = QueueDesc {
            list_type: COMMAND_LIST_DIRECT,
            priority: QUEUE_PRIORITY_NORMAL,
            flags: QUEUE_FLAGS_NONE,
            node_mask: 0,
        };
        let p = self.ops;
        let native = self.native;
        unsafe {
            checked((p.create_queue)(native, &queue))?;
            checked((p.create_allocator)(native, COMMAND_LIST_DIRECT))?;
            checked((p.create_command_list)(native, 0, COMMAND_LIST_DIRECT))?;
            checked((p.close_commands)(native))?;
            checked((p.create_completion_fence)(native, 0, FENCE_FLAGS_NONE))?;
            checked((p.create_event)(native, 0, 0))
        }
    }

    fn initialize(&mut self, setup: &BridgeCreate) -> Result<(), i32> {
        self.output = [setup.width, setup.height];
        self.depth_inverted = setup.depth_inverted != 0;
        self.depth_infinite = setup.depth_infinite != 0;
        self.motion_jittered = setup.motion_jittered != 0;
        self.discover(setup.runtime_directory)?;
        let mut actual = 0;
        let ops = self.ops;
        unsafe {
            checked((ops.create_generation)(self.native, setup, self.provider))?;
            if (ops.get_provider)(self.native, &mut actual) != 0 || actual != self.provider {
                return Err(UNSUPPORTED);
            }
        }
        self.create_commands()?;
        self.admitted = true;
        Ok(())
    }

    fn info(&self, out: &mut BridgeInfo) {
        out.adapter_luid = self.luid;
        out.provider_version = self.provider;
        out.vendor_id = self.vendor;
        out.max_generated = 1;
    }

    fn has(&self, objects: u32) -> bool {
        (unsafe { (self.ops.objects)(self.native) } & objects) == objects
    }

    fn start(&mut self, window: Handle) -> i32 {
        if window.is_null()
            || !self.admitted
            || !self.has(GENERATION)
            || self.has(CHAIN)
            || self.has(SWAPCHAIN)
            || self.terminal != 0
            || self.prepared_destroy
        {
            return INVALID;
        }
        let [width, height] = self.output;
        let desc = SwapchainDesc {
            width,
            height,
            format: BGRA8_UNORM,
            stereo: 0,
            sample_count: 1,
            sample_quality: 0,
            buffer_usage: SWAPCHAIN_RENDER_TARGET_OUTPUT,
            buffer_count: 3,
            scaling: SWAPCHAIN_SCALING_STRETCH,
            swap_effect: SWAPCHAIN_FLIP_DISCARD,
            alpha_mode: SWAPCHAIN_ALPHA_UNSPECIFIED,
            flags: 0,
        };
        // The SDK can write to the retained DXGI description. Refresh only the
        // fields the original bridge set on each attempt when retrying.
        let first_attempt = u32::from(!self.swapchain_attempted);
        self.swapchain_attempted = true;
        let code =
            unsafe { (self.ops.create_swapchain)(self.native, window, &desc, first_attempt) };
        if code == 0 && !self.has(CHAIN) {
            return UNEXPECTED;
        }
        code
    }

    fn generated(&self) -> &Generated {
        unsafe { &*self.generated }
    }

    fn configure(&mut self, enabled: u32, hudless: Handle) -> Result<(), i32> {
        let [width, height] = self.output;
        self.sequence = self.sequence.wrapping_add(1);
        checked(unsafe {
            (self.ops.configure)(self.native, self.sequence, width, height, enabled, hudless)
        })?;
        self.configured = true;
        if enabled == 0 {
            self.generated().result.store(0, SeqCst);
        }
        Ok(())
    }

    fn wait_presents(&self) -> Result<(), i32> {
        if self.has(SWAPCHAIN) {
            checked(unsafe { (self.ops.wait_presents)(self.native) })?;
        }
        Ok(())
    }

    fn wait_local(&self) -> Result<(), i32> {
        if self.completion != 0 {
            checked(unsafe { (self.ops.wait_local)(self.native, self.completion) })?;
        }
        Ok(())
    }

    fn signal_local(&mut self) -> Result<(), i32> {
        let Some(value) = next_local_completion(self.completion) else {
            self.terminal = UNEXPECTED;
            return Err(UNEXPECTED);
        };
        let code = unsafe { (self.ops.signal)(self.native, value, 0) };
        if code != 0 {
            self.terminal = code;
            return Err(code);
        }
        self.completion = value;
        Ok(())
    }

    fn handoff(&mut self, frame: &BridgeFrame, result: i32) -> i32 {
        if self.prepared_destroy
            || !timeline_pair::can_signal_done(frame.ready_value, frame.done_value, self.done)
            || !self.has(LOCAL_FENCE | SHARED_FENCE)
        {
            return result;
        }
        let wait = unsafe { (self.ops.wait_shared)(self.native, frame.ready_value) };
        if wait != 0 {
            self.terminal = wait;
            return wait;
        }
        let signal = unsafe { (self.ops.signal)(self.native, frame.done_value, 1) };
        if signal != 0 {
            self.terminal = signal;
            return signal;
        }
        self.done = frame.done_value;
        self.signal_local().err().unwrap_or(result)
    }

    fn texture_matches(&self, resource: Handle, format: u32, [width, height]: [u32; 2]) -> bool {
        if resource.is_null() {
            return false;
        }
        let mut desc = Texture::default();
        unsafe { (self.ops.describe_texture)(resource, &mut desc) };
        desc.dimension == TEXTURE_2D
            && desc.format == format
            && desc.width == u64::from(width)
            && desc.height == height
            && desc.layers == 1
            && desc.samples == 1
    }

    fn temporal_inputs(&self, frame: &BridgeFrame) -> Result<(), i32> {
        if !unsafe { has_capacity(frame.record) } {
            return Err(INVALID);
        }
        let record = unsafe { &*frame.record };
        let camera = &record.camera;
        let render = [record.render_width, record.render_height];
        let [width, height] = self.output;
        if record.abi_version != RSF_GAME_FRAME_ABI_VERSION
            || camera.struct_size < size_of_val(camera) as u32
            || camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION
            || record.session_id == 0
            || record.frame_id == 0
            || [record.output_width, record.output_height] != self.output
            || render.contains(&0)
            || render[0] > width
            || render[1] > height
            || !record.frame_time_ms.is_finite()
            || record.frame_time_ms <= 0.0
            || !finite_nonzero(frame.motion_scale_x)
            || !finite_nonzero(frame.motion_scale_y)
            || !camera.near_plane.is_finite()
            || camera.near_plane <= 0.0
            || !camera.far_plane.is_finite()
            || camera.far_plane < 0.0
            || (!self.depth_infinite && camera.far_plane <= camera.near_plane)
            || !camera.vertical_fov_radians.is_finite()
            || camera.vertical_fov_radians <= 0.0
            || camera.vertical_fov_radians > std::f32::consts::PI
            || !camera.jitter_pixels.into_iter().all(f32::is_finite)
            || (camera.depth_inverted != 0) != self.depth_inverted
            || (camera.motion_jittered != 0) != self.motion_jittered
            || !camera
                .view_to_world
                .into_iter()
                .chain(camera.view_to_clip)
                .all(f32::is_finite)
            || !self.texture_matches(frame.depth, R32_FLOAT, render)
            || !self.texture_matches(frame.motion, RG16_FLOAT, render)
            || !self.texture_matches(frame.hudless, BGRA8_UNORM, self.output)
            || frame.depth == frame.motion
            || frame.depth == frame.hudless
            || frame.motion == frame.hudless
            || frame.color == frame.depth
            || frame.color == frame.motion
            || frame.color == frame.hudless
        {
            return Err(INVALID);
        }
        Ok(())
    }

    fn validate_frame(&self, frame: &BridgeFrame) -> Result<(), i32> {
        if !self.has(CHAIN | SHARED_FENCE)
            || self.prepared_destroy
            || frame.enabled > 1
            || frame.vsync > 1
            || frame.ready_value == 0
            || frame.ready_value <= self.done
            || frame.done_value <= frame.ready_value
            || !self.texture_matches(frame.color, BGRA8_UNORM, self.output)
        {
            return Err(INVALID);
        }
        if frame.enabled != 0 {
            self.temporal_inputs(frame)?;
        }
        Ok(())
    }

    fn record_frame(&mut self, frame: &BridgeFrame) -> Result<(), i32> {
        let p = self.ops;
        let native = self.native;
        // The SDK requires Configure on every frame before PrepareV2, including resets.
        self.configure(frame.enabled, frame.hudless)?;
        if frame.enabled != 0 {
            let record = unsafe { &*frame.record };
            let reset = !self.history
                || record.frame_id != self.last_source.wrapping_add(1)
                || record.flags & RSF_FRAME_FLAG_RESET != 0;
            for resource in [frame.depth, frame.motion, frame.hudless] {
                unsafe { (p.shader_read)(native, resource, 1) };
            }
            let code = unsafe { (p.prepare)(native, frame, self.sequence, u32::from(reset)) };
            if code != 0 {
                self.history = false;
                return Err(code);
            }
            for resource in [frame.depth, frame.motion] {
                unsafe { (p.shader_read)(native, resource, 0) };
            }
        }
        checked(unsafe { (p.copy_to_backbuffer)(native, frame.color) })
    }

    fn record_commands(&mut self, frame: &BridgeFrame) -> Result<(), i32> {
        checked(unsafe { (self.ops.reset_commands)(self.native) })?;
        let recorded = self.record_frame(frame);
        let closed = checked(unsafe { (self.ops.close_commands)(self.native) });
        recorded.and(closed)
    }

    fn submit_frame(&mut self, frame: &BridgeFrame) -> Result<i32, i32> {
        let p = self.ops;
        let native = self.native;
        checked(unsafe { (p.wait_shared)(native, frame.ready_value) })?;
        unsafe { (p.execute)(native) };
        let presented = unsafe { (p.present)(native, frame.vsync) };

        // The SDK can still read HUD-less color after the DXGI Present call.
        self.wait_presents()?;
        self.signal_local()?;
        self.wait_local()?;
        if frame.enabled != 0 {
            checked(unsafe { (p.reset_commands)(native) })?;
            unsafe { (p.shader_read)(native, frame.hudless, 0) };
            checked(unsafe { (p.close_commands)(native) })?;
            unsafe { (p.execute)(native) };
        }
        Ok(presented)
    }

    fn present(&mut self, frame: &BridgeFrame, status: &mut BridgeStatus) -> i32 {
        if self.terminal != 0 {
            return self.terminal;
        }
        if let Err(code) = self.validate_frame(frame) {
            return self.handoff(frame, code);
        }
        if let Err(code) = self.wait_local() {
            self.terminal = code;
            return code;
        }
        if let Err(code) = self.record_commands(frame) {
            return self.handoff(frame, code);
        }
        let presented = match self.submit_frame(frame) {
            Ok(presented) => presented,
            Err(code) => {
                self.terminal = code;
                return code;
            }
        };
        self.history = frame.enabled != 0 && presented == 0;
        if let Some(source) = source_id(frame.record) {
            self.last_source = source;
        }
        let reported = if presented != 0 {
            presented
        } else {
            self.generated().result.load(SeqCst)
        };
        let result = self.handoff(frame, reported);
        let mut count = 0;
        if unsafe { (self.ops.get_present_count)(self.native, &mut count) } == 0 {
            status.frames_presented = present_count_delta(self.last_present_count, count);
            self.last_present_count = count;
        }
        status.fg_enabled = frame.enabled;
        status.generated_callbacks = self.generated().count.load(SeqCst);
        result
    }

    fn drain(&mut self) -> Result<(), i32> {
        if self.prepared_destroy {
            return Ok(());
        }
        checked(self.terminal)?;
        self.wait_presents()?;
        if self.has(LOCAL_FENCE) {
            self.signal_local()?;
        }
        self.wait_local()
    }

    fn prepare_destroy(&mut self) -> Result<(), i32> {
        if self.prepared_destroy {
            return Ok(());
        }
        checked(self.terminal)?;
        if self.has(CHAIN) && self.configured {
            self.configure(0, ptr::null_mut())?;
        }
        self.drain()?;
        let p = self.ops;
        let native = self.native;
        if self.has(GENERATION) {
            checked(unsafe { (p.destroy_generation)(native) })?;
        }
        if self.has(CHAIN) {
            unsafe { (p.release_chain)(native) };
        }
        if self.has(SWAPCHAIN) {
            checked(unsafe { (p.destroy_swapchain)(native) })?;
        }
        self.prepared_destroy = true;
        Ok(())
    }
}

pub(crate) fn present_count_delta(previous: u32, current: u32) -> u32 {
    if current >= previous || previous > 0xf000_0000 {
        current.wrapping_sub(previous)
    } else {
        current
    }
}

#[cfg(test)]
#[test]
fn present_count_delta_handles_reset_and_rollover() {
    assert_eq!(present_count_delta(100, 103), 3);
    assert_eq!(present_count_delta(100, 1), 1);
    assert_eq!(present_count_delta(u32::MAX - 1, 1), 3);
}

fn finite_nonzero(value: f32) -> bool {
    value.is_finite() && value != 0.0
}

fn source_id(record: *const Record) -> Option<u64> {
    const END: usize = std::mem::offset_of!(Record, frame_id) + size_of::<u64>();
    if record.is_null() || (unsafe { record.cast::<u32>().read() } as usize) < END {
        return None;
    }
    Some(unsafe { ptr::addr_of!((*record).frame_id).read() })
}

fn provider_path(
    directory: *const u16,
    path: &mut [u16; PROVIDER_PATH_CAPACITY],
) -> Result<(), i32> {
    // The C caller provides a NUL-terminated directory borrowed during admission.
    let Some(length) = (0..MAX_DIRECTORY).find(|&i| unsafe { directory.add(i).read() == 0 }) else {
        return Err(INVALID);
    };
    if length == 0 {
        return Err(INVALID);
    }
    unsafe { ptr::copy_nonoverlapping(directory, path.as_mut_ptr(), length) };
    let filename = if matches!(path[length - 1], 47 | 92) {
        length
    } else {
        path[length] = u16::from(b'\\');
        length + 1
    };
    for (output, unit) in path[filename..]
        .iter_mut()
        .zip(PROVIDER_DLL.encode_utf16().chain([0]))
    {
        *output = unit;
    }
    Ok(())
}

unsafe fn check<T>(input: *const T) -> Result<(), i32> {
    if !unsafe { has_capacity(input) } {
        return Err(INVALID);
    }
    if unsafe { input.cast::<u32>().add(1).read() } != BRIDGE_ABI {
        return Err(ABI_MISMATCH);
    }
    Ok(())
}

fn select_version(versions: &[Version], count: u64) -> u64 {
    if count > versions.len() as u64 {
        return 0;
    }
    ffx_version::select(&versions[..count as usize], 4, 0).map_or(0, |version| version.id)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_probe(
    ops: &'static Platform,
    luid: u64,
    directory: *const u16,
    out: *mut BridgeInfo,
) -> i32 {
    if let Err(code) = unsafe { check(out) } {
        return code;
    }
    let info = unsafe { &mut *out };
    info.adapter_luid = 0;
    info.provider_version = 0;
    info.vendor_id = 0;
    info.max_generated = 0;
    if luid == 0 || !unsafe { absolute_wide_path(directory, MAX_DIRECTORY) } {
        return INVALID;
    }
    let result = Bridge::new(ops, luid).and_then(|mut bridge| {
        bridge.discover(directory)?;
        bridge.info(info);
        Ok(())
    });
    result.err().unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_create(
    ops: &'static Platform,
    setup: *const BridgeCreate,
    output: *mut *mut Bridge,
) -> i32 {
    if output.is_null() {
        return INVALID;
    }
    unsafe { output.write(ptr::null_mut()) };
    if let Err(code) = unsafe { check(setup) } {
        return code;
    }
    let setup = unsafe { &*setup };
    if !unsafe { absolute_wide_path(setup.runtime_directory, MAX_DIRECTORY) }
        || setup.adapter_luid == 0
        || setup.width == 0
        || setup.height == 0
        || setup.width > i32::MAX as u32
        || setup.height > i32::MAX as u32
        || setup.generated_count != 1
        || setup.depth_inverted > 1
        || setup.depth_infinite > 1
        || setup.motion_jittered > 1
    {
        return INVALID;
    }
    let mut bridge = match Bridge::new(ops, setup.adapter_luid) {
        Ok(bridge) => bridge,
        Err(code) => return code,
    };
    if let Err(code) = bridge.initialize(setup) {
        if bridge.prepare_destroy().is_err() {
            unsafe { output.write(Box::into_raw(bridge)) };
        }
        return code;
    }
    unsafe { output.write(Box::into_raw(bridge)) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgInfo(bridge: *const Bridge, out: *mut BridgeInfo) -> i32 {
    if bridge.is_null() {
        return INVALID;
    }
    if let Err(code) = unsafe { check(out) } {
        return code;
    }
    unsafe { (*bridge).info(&mut *out) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgStart(bridge: *mut Bridge, window: Handle) -> i32 {
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return INVALID;
    };
    bridge.start(window)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgCreateImage(
    bridge: Handle,
    width: u32,
    height: u32,
    format: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    if handle.is_null() || resource.is_null() {
        return INVALID;
    }
    unsafe {
        handle.write(ptr::null_mut());
        resource.write(ptr::null_mut());
    }
    let Some(bridge) = (unsafe { bridge.cast::<Bridge>().as_ref() }) else {
        return INVALID;
    };
    if !bridge.has(DEVICE) || width == 0 || height == 0 || bridge.prepared_destroy {
        return INVALID;
    }
    let dxgi = match format {
        VK_BGRA8_UNORM => BGRA8_UNORM,
        VK_R32_FLOAT => R32_FLOAT,
        VK_RG16_FLOAT => RG16_FLOAT,
        _ => return INVALID,
    };
    unsafe { (bridge.ops.create_image)(bridge.native, width, height, dxgi, handle, resource) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgCreateFence(bridge: Handle, handle: *mut Handle) -> i32 {
    if handle.is_null() {
        return INVALID;
    }
    unsafe { handle.write(ptr::null_mut()) };
    let Some(bridge) = (unsafe { bridge.cast::<Bridge>().as_ref() }) else {
        return INVALID;
    };
    if !bridge.has(DEVICE) || bridge.has(SHARED_FENCE) || bridge.prepared_destroy {
        return INVALID;
    }
    unsafe { (bridge.ops.create_fence)(bridge.native, handle) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgPresent(
    bridge: *mut Bridge,
    frame: *const BridgeFrame,
    status: *mut BridgeStatus,
) -> i32 {
    if let Err(code) = unsafe { check(status) } {
        return code;
    }
    let status = unsafe { &mut *status };
    status.frames_presented = 0;
    status.fg_enabled = 0;
    status.generated_callbacks = 0;
    status.terminal = 0;
    let result = if let Some(bridge) = unsafe { bridge.as_mut() } {
        status.generated_callbacks = bridge.generated().count.load(SeqCst);
        let result = match unsafe { check(frame) } {
            Ok(()) => bridge.present(unsafe { &*frame }, status),
            Err(code) => code,
        };
        status.terminal = u32::from(bridge.terminal != 0);
        result
    } else {
        INVALID
    };
    status.result = result;
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgDrain(bridge: *mut Bridge) -> i32 {
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return 0;
    };
    bridge.drain().err().unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgPrepareDestroy(bridge: *mut Bridge) -> i32 {
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return 0;
    };
    bridge.prepare_destroy().err().unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgDestroy(bridge: *mut Bridge) -> i32 {
    let code = unsafe { RsfFsr4FgPrepareDestroy(bridge) };
    if code == 0 && !bridge.is_null() {
        drop(unsafe { Box::from_raw(bridge) });
    }
    code
}
