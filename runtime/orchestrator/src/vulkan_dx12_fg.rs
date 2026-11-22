// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::sr_session::has_capacity;
use crate::sys::rsf_dx12vk_shared_image as SharedImage;
use crate::sys::{
    RSF_FGVK_ABI_VERSION, RSF_FGVK_FRAME_SLOTS, rsf_fgvk_frame as Frame,
    rsf_fgvk_frame_slot as Recording, rsf_fgvk_setup as Setup, rsf_fgvk_status as Status,
    rsf_fgvk_timeline as RecordingTimeline,
};
use crate::sys::{rsf_srvk_image as Image, rsf_srvk_setup as Graphics};
use crate::sys::{rsf_vkfg_image as OwnedImage, rsf_vkfg_platform as ImageOps};
use crate::vulkan_external_copy::{
    self, ALL_COMMANDS, Barrier, FAMILY_EXTERNAL, GENERAL, TRANSFER_DST, Transfer, UNDEFINED,
};
use crate::vulkan_fg_frames::*;
use crate::vulkan_fg_resources::{ensure_image, rsf_core_vkfg_release_image};
use crate::vulkan_sr::{SAMPLED_USAGE, TRANSFER_SRC_USAGE, valid_image};
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION,
    rsf_frame_record as Record,
};
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void};
use std::ptr;
use std::slice;

pub(super) const INVALID: i32 = -13001;
pub(super) const ABI_MISMATCH: i32 = -13002;
pub(super) const NOT_READY: i32 = 1;
const INCOMPLETE: i32 = 5;
const OUT_OF_MEMORY: i32 = -1;
const INIT_FAILED: i32 = -3;
const EXTENSION_NOT_PRESENT: i32 = -7;
pub(super) const FORMAT_UNSUPPORTED: i32 = -11;
pub(super) const UNKNOWN: i32 = -13;
pub(super) const INVALID_EXTERNAL_HANDLE: i32 = -1000072003;
const RGBA8: u32 = 37;
const BGRA8: u32 = 44;
const RG16F: u32 = 83;
const R32F: u32 = 100;
const SHADER_READ: u32 = 5;
const BLIT_SRC: u32 = 0x400;
const BLIT_DST: u32 = 0x800;
const SRGB_NONLINEAR: u32 = 0;
const EXCLUSIVE: u32 = 0;
const PRESENT_FIFO: u32 = 2;
const PRESENT_FIFO_RELAXED: u32 = 3;
const SWAPCHAIN_LATENCY_NV: u32 = 1_000_505_007;
const WAIT_NS: u64 = 10_000_000_000;
const BUFFER_COUNT: usize = 3;
const PRESENT_WAIT_LIMIT: usize = 16;
const SUBMIT_WAIT_CAPACITY: usize = PRESENT_WAIT_LIMIT + 1;
pub(super) const SLOT_COUNT: usize = RSF_FGVK_FRAME_SLOTS as usize;

mod platform;

pub(super) type Handle = *mut c_void;
pub(super) type Release = unsafe extern "C" fn(Handle) -> i32;

/// Raw facts the native reader copies from the borrowed VkSwapchainCreateInfoKHR.
/// `extension_count` saturates at 2; `extension_type` is the first chained sType or 0.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Swapchain {
    pub(super) width: u32,
    pub(super) height: u32,
    format: u32,
    usage: u32,
    color_space: u32,
    layers: u32,
    sharing_mode: u32,
    min_images: u32,
    flags: u32,
    present_mode: u32,
    extension_count: u32,
    extension_type: u32,
    old_chain: u64,
}

impl Swapchain {
    /// The owned chain accepts no extension, or only the NV low-latency create info,
    /// which the vendor latency path consumes.
    fn extensions_compatible(self) -> bool {
        self.extension_count == 0
            || (self.extension_count == 1 && self.extension_type == SWAPCHAIN_LATENCY_NV)
    }

    /// FIFO and FIFO_RELAXED present on vertical blank; the vendor presents with vsync then.
    pub(super) fn vsync(self) -> u32 {
        u32::from(matches!(
            self.present_mode,
            PRESENT_FIFO | PRESENT_FIFO_RELAXED
        ))
    }

    pub(super) fn supported(self) -> bool {
        self.old_chain == 0
            && self.layers == 1
            && self.sharing_mode == EXCLUSIVE
            && self.width != 0
            && self.height != 0
            && self.color_space == SRGB_NONLINEAR
            && matches!(self.format, RGBA8 | BGRA8)
            && self.extensions_compatible()
            && self.flags == 0
            && self.min_images <= BUFFER_COUNT as u32
    }
}

/// Reads the borrowed create info only after the caller admitted the setup and options.
pub(super) fn describe(ops: &GraphicsOps, setup: &Setup) -> Option<Swapchain> {
    if setup.create_info.is_null() {
        return None;
    }
    let mut swapchain = Swapchain::default();
    unsafe { (ops.describe)(setup.create_info, &mut swapchain) };
    Some(swapchain)
}

#[repr(C)]
pub(super) struct PresentInfo {
    chain: u64,
    index: u32,
    wait_count: u32,
    waits: *const u64,
    result: *mut i32,
}

#[repr(C)]
struct PresentInput {
    native: *const c_void,
    swapchain_count: u32,
    wait_count: u32,
    has_extension: u32,
    has_image: u32,
    has_waits: u32,
    has_result: u32,
}

type ReadPresent = unsafe extern "C" fn(*const c_void, u32, &mut u64, &mut u32, *mut u64);
type PresentCallback = unsafe extern "C" fn(Handle, &PresentInfo, *const Record, u32) -> i32;

// Admit the borrowed VkPresentInfoKHR shape before the reader touches its arrays.
unsafe fn marshal_present(
    call: PresentCallback,
    owner: Handle,
    input: &PresentInput,
    read: ReadPresent,
    source: *const Record,
    explicit: u32,
    result: &mut i32,
) -> i32 {
    *result = INCOMPLETE;
    if input.native.is_null()
        || input.has_extension != 0
        || input.swapchain_count != 1
        || input.has_image == 0
        || input.wait_count as usize > PRESENT_WAIT_LIMIT
        || (input.wait_count != 0 && input.has_waits == 0)
    {
        return INIT_FAILED;
    }
    let mut waits = [0; PRESENT_WAIT_LIMIT];
    let (mut chain, mut index) = (0, 0);
    unsafe {
        read(
            input.native,
            input.wait_count,
            &mut chain,
            &mut index,
            waits.as_mut_ptr(),
        )
    };
    let info = PresentInfo {
        chain,
        index,
        wait_count: input.wait_count,
        waits: waits.as_ptr(),
        result: if input.has_result != 0 {
            ptr::from_mut(result)
        } else {
            ptr::null_mut()
        },
    };
    unsafe { call(owner, &info, source, explicit) }
}

#[repr(C)]
struct PresentSubmit {
    wait_count: u32,
    waits: [u64; SUBMIT_WAIT_CAPACITY],
    values: [u64; SUBMIT_WAIT_CAPACITY],
    stages: [u32; SUBMIT_WAIT_CAPACITY],
    signal: u64,
    ready: u64,
}

impl PresentSubmit {
    fn new(binary_waits: &[u64], timeline: u64, previous: u64, ready: u64) -> Self {
        let count = binary_waits.len();
        let mut submit = Self {
            wait_count: count as u32,
            waits: [0; SUBMIT_WAIT_CAPACITY],
            values: [0; SUBMIT_WAIT_CAPACITY],
            stages: [0; SUBMIT_WAIT_CAPACITY],
            signal: timeline,
            ready,
        };
        submit.waits[..count].copy_from_slice(binary_waits);
        submit.stages[..count].fill(ALL_COMMANDS);
        if previous != 0 {
            submit.waits[count] = timeline;
            submit.values[count] = previous;
            submit.stages[count] = ALL_COMMANDS;
            submit.wait_count += 1;
        }
        submit
    }
}

#[repr(C)]
pub(super) struct GraphicsOps {
    pub(super) adapter_luid: unsafe extern "C" fn(*const Graphics, *mut u64) -> i32,
    platform_allocate: unsafe extern "C" fn(*const Setup) -> Handle,
    platform_destroy: unsafe extern "C" fn(Handle),
    platform_resolve: unsafe extern "C" fn(Handle, *const Graphics) -> i32,
    format_features: unsafe extern "C" fn(Handle, u32) -> u32,
    initialize_capture: unsafe extern "C" fn(Handle, *const Graphics) -> i32,
    create_pool: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    destroy_pool: unsafe extern "C" fn(Handle, u64),
    images: *const ImageOps,
    allocate_commands: unsafe extern "C" fn(Handle, u64, *mut Handle) -> i32,
    begin: unsafe extern "C" fn(Handle, Handle) -> i32,
    end: unsafe extern "C" fn(Handle, Handle) -> i32,
    barrier: unsafe extern "C" fn(Handle, Handle, *const Barrier),
    copy: unsafe extern "C" fn(Handle, Handle, *const Transfer),
    blit: unsafe extern "C" fn(Handle, Handle, *const Transfer),
    clear: unsafe extern "C" fn(Handle, Handle, u64),
    acquire_submit: unsafe extern "C" fn(Handle, u64, u64) -> i32,
    present_submit: unsafe extern "C" fn(Handle, Handle, &PresentSubmit) -> i32,
    wait_timeline: unsafe extern "C" fn(Handle, u64, u64, u64) -> i32,
    create_fence: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    mark_submit: unsafe extern "C" fn(Handle, u64) -> i32,
    wait_fence: unsafe extern "C" fn(Handle, u64, u64) -> i32,
    destroy_fence: unsafe extern "C" fn(Handle, u64),
    idle: Release,
    capture_provider: unsafe extern "C" fn(Handle, u64, u32),
    capture_record:
        unsafe extern "C" fn(Handle, Handle, *const Image, *const Record, *const c_char, u64),
    capture_complete: unsafe extern "C" fn(Handle, u64),
    capture_discard: unsafe extern "C" fn(Handle, u64),
    capture_flush: Release,
    describe: unsafe extern "C" fn(*const c_void, *mut Swapchain),
}
const _: () = {
    assert!(size_of::<GraphicsOps>() == 30 * size_of::<Handle>());
    assert!(std::mem::offset_of!(GraphicsOps, describe) == 29 * size_of::<Handle>());
    assert!(std::mem::offset_of!(GraphicsOps, platform_resolve) == 3 * size_of::<Handle>());
    assert!(std::mem::offset_of!(GraphicsOps, images) == 8 * size_of::<Handle>());
    assert!(size_of::<Swapchain>() == 56);
    assert!(size_of::<PresentInfo>() == 32);
    assert!(size_of::<PresentInput>() == 32);
    assert!(std::mem::offset_of!(PresentInput, wait_count) == 12);
    assert!(size_of::<PresentSubmit>() == 368);
    assert!(std::mem::offset_of!(PresentSubmit, values) == 144);
    assert!(std::mem::offset_of!(PresentSubmit, signal) == 352);
};

/// One recorded command of a planned transfer, in submission order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Barrier(Barrier),
    Copy(Transfer),
    Blit(Transfer),
    Clear(u64),
}

fn copy_steps(family: u32, source: &Image, target: &SharedImage, initialized: bool) -> [Step; 5] {
    let transfer = Transfer {
        source: source.image,
        target: target.image,
        source_extent: [source.width, source.height],
        target_extent: [target.width, target.height],
    };
    let plan = vulkan_external_copy::plan(transfer, source.layout, family, initialized, family);
    [
        Step::Barrier(plan.before[0]),
        Step::Barrier(plan.before[1]),
        if source.format == target.format {
            Step::Copy(plan.transfer)
        } else {
            Step::Blit(plan.transfer)
        },
        Step::Barrier(plan.after[0]),
        Step::Barrier(plan.after[1]),
    ]
}

// A cleared placeholder discards its contents and is released like a copy target.
fn clear_steps(family: u32, image: u64) -> [Step; 3] {
    [
        Step::Barrier(Barrier::new(image, UNDEFINED, TRANSFER_DST, family, family)),
        Step::Clear(image),
        Step::Barrier(Barrier::new(
            image,
            TRANSFER_DST,
            GENERAL,
            family,
            FAMILY_EXTERNAL,
        )),
    ]
}

/// Presentation colour arrives as RGBA8 or BGRA8 and every shared colour
/// image is BGRA8, so RGBA8 swapchains need the format-converting blit.
fn blit_formats_supported(rgba_optimal: u32, bgra_optimal: u32) -> bool {
    rgba_optimal & BLIT_SRC != 0 && bgra_optimal & BLIT_DST != 0
}

impl GraphicsOps {
    fn open(&self, setup: &Setup) -> Result<Handle, i32> {
        let platform = unsafe { (self.platform_allocate)(setup) };
        if platform.is_null() {
            return Err(OUT_OF_MEMORY);
        }
        if let Err(result) = self.prepare(platform, &setup.graphics) {
            unsafe { (self.platform_destroy)(platform) };
            return Err(result);
        }
        Ok(platform)
    }

    fn prepare(&self, platform: Handle, graphics: &Graphics) -> Result<(), i32> {
        succeeded(unsafe { (self.platform_resolve)(platform, graphics) })?;
        let rgba = unsafe { (self.format_features)(platform, RGBA8) };
        let bgra = unsafe { (self.format_features)(platform, BGRA8) };
        if !blit_formats_supported(rgba, bgra) {
            return Err(FORMAT_UNSUPPORTED);
        }
        succeeded(unsafe { (self.initialize_capture)(platform, graphics) })
    }

    fn record(&self, vk: Handle, commands: Handle, steps: impl IntoIterator<Item = Step>) {
        for step in steps {
            unsafe {
                match step {
                    Step::Barrier(barrier) => (self.barrier)(vk, commands, &barrier),
                    Step::Copy(transfer) => (self.copy)(vk, commands, &transfer),
                    Step::Blit(transfer) => (self.blit)(vk, commands, &transfer),
                    Step::Clear(image) => (self.clear)(vk, commands, image),
                }
            }
        }
    }
}

pub(super) struct Start {
    pub(super) window: Handle,
    pub(super) generated_count: u32,
    pub(super) timeline_enabled: u32,
}

pub(super) struct PresentFrame<'a> {
    pub(super) record: Option<&'a Record>,
    pub(super) color: Handle,
    pub(super) depth: Handle,
    pub(super) motion: Handle,
    pub(super) hudless: Handle,
    pub(super) scales: [f32; 2],
    pub(super) ready: u64,
    pub(super) done: u64,
    pub(super) vsync: u32,
}

pub(super) struct PresentStatus {
    pub(super) frames_presented: u64,
    pub(super) enabled: u32,
    pub(super) result: i32,
    pub(super) terminal: bool,
    pub(super) generated_callbacks: u64,
}

pub(super) trait Submission {
    fn enabled(&self) -> bool;
    fn present(&mut self, frame: PresentFrame<'_>) -> (i32, PresentStatus);
}

pub(super) trait Vendor {
    const MAX_SOURCE_FRAME: u64 = u64::MAX;
    type Submission<'a>: Submission
    where
        Self: 'a;

    fn create(
        &mut self,
        graphics: &Graphics,
        swapchain: &Swapchain,
        luid: u64,
        count: u32,
    ) -> Result<u64, i32>;
    fn start(&self, graphics: &Graphics, swapchain: &Swapchain, options: Start) -> Result<(), i32>;
    fn capture_backend(&self) -> u32;
    fn implicit_present_source(&self) -> Option<u64> {
        None
    }
    fn begin_present(&self, source_frame: u64, enabled: bool) -> Result<Self::Submission<'_>, i32>;
    fn set_generated(&self, count: u32) -> Result<(), i32>;
    fn drain(&self) -> Result<(), i32>;
    fn prepare_destroy(&mut self) -> Result<(), i32>;
    fn destroy(&mut self) -> Result<(), i32>;
    fn import(&self, graphics: &Graphics, luid: u64, enabled: u32) -> Result<Handle, i32>;
    fn importer_ops(&self) -> &InteropOps;
}

#[repr(C)]
pub(super) struct InteropOps {
    pub(super) image: unsafe extern "C" fn(Handle, u32, u32, u32, *mut SharedImage) -> i32,
    pub(super) timeline: unsafe extern "C" fn(Handle) -> u64,
    pub(super) destroy: unsafe extern "C" fn(Handle, u32) -> i32,
    pub(super) release_guides: unsafe extern "C" fn(Handle, u64, u64) -> i32,
}

impl InteropOps {
    fn image(
        &self,
        importer: Handle,
        format: u32,
        width: u32,
        height: u32,
        target: &mut SharedImage,
    ) -> Result<(), i32> {
        succeeded(unsafe { (self.image)(importer, format, width, height, target) })
    }
}

// The public WSI contract serializes accesses to Presentation. XeLL CPU
// leases borrow only Vendor, so an older frame can present during sleep.
pub(super) struct Owner<V> {
    pub(super) vendor: V,
    state: UnsafeCell<Presentation>,
}

pub(super) fn setup_valid(setup: &Setup, options: &Start) -> bool {
    let graphics = &setup.graphics;
    !graphics.instance.is_null()
        && graphics.instance_proc.is_some()
        && graphics.device_proc.is_some()
        && !graphics.device.is_null()
        && !graphics.physical_device.is_null()
        && setup.graphics_submit.is_some()
        && setup.lock_queues.is_some()
        && setup.unlock_queues.is_some()
        && !options.window.is_null()
        && options.timeline_enabled == 1
        && graphics.motion_jittered == 0
}

pub(super) unsafe fn create<V: Vendor>(
    vendor: V,
    ops: &'static GraphicsOps,
    setup: &Setup,
    swapchain: Swapchain,
    options: Start,
    output: *mut *mut Owner<V>,
    chain: *mut u64,
) -> i32 {
    if !swapchain.supported() {
        return FORMAT_UNSUPPORTED;
    }
    let owner = allocate_handle(Owner {
        vendor,
        state: UnsafeCell::new(Presentation::new(ops, setup, swapchain)),
    });
    if owner.is_null() {
        return OUT_OF_MEMORY;
    }
    let state = unsafe { &mut *owner };
    if let Err(result) = state
        .state
        .get_mut()
        .initialize(&mut state.vendor, setup, options)
    {
        if unsafe { destroy(owner) } != 0 {
            unsafe { output.write(owner) };
        }
        return result;
    }
    let identity = owner.addr() as u64;
    state.state.get_mut().chain = identity;
    unsafe {
        output.write(owner);
        chain.write(identity);
    }
    0
}

pub(super) fn succeeded(result: i32) -> Result<(), i32> {
    if result == 0 { Ok(()) } else { Err(result) }
}

#[derive(Default)]
struct Buffer {
    image: OwnedImage,
    commands: Handle,
    shared: SharedImage,
    done: u64,
    acquired: bool,
}

#[derive(Default)]
struct SourceFence {
    handle: u64,
    stages: u32,
}

struct Slot {
    state: Recording,
    depth: SharedImage,
    motion: SharedImage,
    hudless: SharedImage,
    fences: [SourceFence; 2],
    done: u64,
    scales: [f32; 2],
    guides_initialized: bool,
    hudless_initialized: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            state: unsafe { std::mem::zeroed() },
            depth: SharedImage::default(),
            motion: SharedImage::default(),
            hudless: SharedImage::default(),
            fences: std::array::from_fn(|_| SourceFence::default()),
            done: 0,
            scales: [0.0; 2],
            guides_initialized: false,
            hudless_initialized: false,
        }
    }
}

const GUIDES: u32 = 1;
const HUDLESS: u32 = 2;
fn slot_index(frame: u64) -> usize {
    (frame % SLOT_COUNT as u64) as usize
}

// Calls are serialized by the Vulkan WSI contract. The shared primary owner
// in vulkan_fg.rs and the presentation borrow gate remain the entry point.
struct Presentation {
    ops: &'static GraphicsOps,
    vk: Handle,
    device: Handle,
    family: u32,
    depth_inverted: u32,
    interop: Handle,
    pool: u64,
    timeline: u64,
    chain: u64,
    swapchain: Swapchain,
    buffers: [Buffer; BUFFER_COUNT],
    next: usize,
    slots: [Slot; SLOT_COUNT],
    recording: RecordingTimeline,
    fallback_depth: SharedImage,
    fallback_motion: SharedImage,
    // Zero marks first use for both the fallback pair and Buffer::done.
    // Successful GPU submission commits an even, nonzero timeline value.
    last_done: u64,
    requested: bool,
    terminal: bool,
    retiring: bool,
    status: Status,
}

impl Presentation {
    fn new(ops: &'static GraphicsOps, setup: &Setup, swapchain: Swapchain) -> Self {
        Self {
            ops,
            vk: ptr::null_mut(),
            device: setup.graphics.device,
            family: setup.queue_family,
            depth_inverted: setup.graphics.depth_inverted,
            interop: ptr::null_mut(),
            pool: 0,
            timeline: 0,
            chain: 0,
            swapchain,
            buffers: std::array::from_fn(|_| Buffer::default()),
            next: 0,
            slots: std::array::from_fn(|_| Slot::default()),
            recording: unsafe { std::mem::zeroed() },
            fallback_depth: SharedImage::default(),
            fallback_motion: SharedImage::default(),
            last_done: 0,
            requested: false,
            terminal: false,
            retiring: false,
            status: Status {
                struct_size: size_of::<Status>() as u32,
                abi_version: RSF_FGVK_ABI_VERSION,
                enabled: 0,
                available: 0,
                result: 0,
                prepared: 0,
                tagged: 0,
                generated_callbacks: 0,
                last_present_count: 0,
                source_frame: 0,
            },
        }
    }

    fn initialize(
        &mut self,
        vendor: &mut impl Vendor,
        setup: &Setup,
        options: Start,
    ) -> Result<(), i32> {
        let ops = self.ops;
        let graphics = &setup.graphics;
        let mut luid = 0;
        succeeded(unsafe { (ops.adapter_luid)(graphics, &mut luid) })?;
        self.vk = ops.open(setup)?;
        let version = vendor.create(graphics, &self.swapchain, luid, options.generated_count)?;
        self.interop = vendor.import(graphics, luid, options.timeline_enabled)?;
        unsafe { (ops.capture_provider)(self.vk, version, vendor.capture_backend()) };
        let importer = vendor.importer_ops();
        self.timeline = unsafe { (importer.timeline)(self.interop) };
        if self.timeline == 0 {
            return Err(INVALID_EXTERNAL_HANDLE);
        }
        succeeded(unsafe { (ops.create_pool)(self.vk, &mut self.pool) })?;
        for buffer in &mut self.buffers {
            ensure_image(
                unsafe { &*ops.images },
                self.vk,
                &mut buffer.image,
                [self.swapchain.width, self.swapchain.height],
                self.swapchain.format,
                self.swapchain.usage | TRANSFER_SRC_USAGE | SAMPLED_USAGE,
                false,
            )?;
            vendor.importer_ops().image(
                self.interop,
                BGRA8,
                self.swapchain.width,
                self.swapchain.height,
                &mut buffer.shared,
            )?;
            succeeded(unsafe {
                (ops.allocate_commands)(self.vk, self.pool, &mut buffer.commands)
            })?;
        }
        vendor
            .importer_ops()
            .image(self.interop, R32F, 1, 1, &mut self.fallback_depth)?;
        vendor
            .importer_ops()
            .image(self.interop, RG16F, 1, 1, &mut self.fallback_motion)?;
        vendor.start(graphics, &self.swapchain, options)?;
        self.status.available = 1;
        Ok(())
    }

    fn open(&self) -> bool {
        self.status.available != 0 && !self.terminal && !self.retiring
    }

    fn wait_timeline(&self, value: u64, timeout: u64) -> Result<(), i32> {
        if value == 0 {
            return Ok(());
        }
        succeeded(unsafe { (self.ops.wait_timeline)(self.vk, self.timeline, value, timeout) })
    }

    fn complete_source(ops: &GraphicsOps, vk: Handle, record: &Recording, fence: &mut SourceFence) {
        for stage in [GUIDES, HUDLESS] {
            if fence.stages & stage != 0 {
                unsafe { (ops.capture_complete)(vk, record.capture_group(stage == HUDLESS)) }
            }
        }
        fence.stages = 0;
    }

    fn wait_sources(ops: &GraphicsOps, vk: Handle, slot: &mut Slot) -> Result<(), i32> {
        for fence in &mut slot.fences {
            if fence.stages != 0 {
                succeeded(unsafe { (ops.wait_fence)(vk, fence.handle, WAIT_NS) })?;
                Self::complete_source(ops, vk, &slot.state, fence);
            }
        }
        Ok(())
    }

    fn frame_valid(&self, frame: &Frame, record: &Record) -> bool {
        let camera = &record.camera;
        record.abi_version == RSF_GAME_FRAME_ABI_VERSION
            && record.frame_id != 0
            && record.session_id != 0
            && record.render_width != 0
            && record.render_height != 0
            && record.output_width == self.swapchain.width
            && record.output_height == self.swapchain.height
            && camera.struct_size as usize >= size_of_val(camera)
            && camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
            && camera.motion_jittered == 0
            && camera.depth_inverted == self.depth_inverted
            && [frame.motion_scale_x, frame.motion_scale_y]
                .into_iter()
                .all(|scale| scale.is_finite() && scale != 0.0)
            && camera
                .world_to_view
                .iter()
                .chain(&camera.view_to_clip)
                .chain(&camera.jitter_pixels)
                .all(|value| value.is_finite())
    }

    fn image_valid(&self, image: &Image, format: u32, width: u32, height: u32) -> bool {
        image.copy_source != 0
            && valid_image(image, self.device, [width, height], SHADER_READ, &[format])
    }

    fn acquire(&mut self, timeout: u64, semaphore: u64, fence: u64) -> Result<u32, i32> {
        if !self.open() || (semaphore == 0 && fence == 0) {
            return Err(INIT_FAILED);
        }
        let index = self.next;
        let buffer = &self.buffers[index];
        if buffer.acquired {
            return Err(NOT_READY);
        }
        self.wait_timeline(buffer.done, timeout)?;
        succeeded(unsafe { (self.ops.acquire_submit)(self.vk, semaphore, fence) })?;
        self.buffers[index].acquired = true;
        self.next = (index + 1) % BUFFER_COUNT;
        Ok(index as u32)
    }

    fn prepare(
        &mut self,
        vendor: &impl Vendor,
        commands: Handle,
        frame: &Frame,
        record: &Record,
    ) -> Result<(), i32> {
        let (width, height) = (record.render_width, record.render_height);
        if !self.image_valid(&frame.depth, R32F, width, height)
            || !self.image_valid(&frame.motion, RG16F, width, height)
        {
            return Err(INVALID);
        }
        let index = slot_index(record.frame_id);
        let slot = &self.slots[index];
        if rsf_core_fgvk_can_prepare(&slot.state) == 0 {
            return Err(NOT_READY);
        }
        self.wait_timeline(slot.done, WAIT_NS)?;
        let slot = &mut self.slots[index];
        Self::wait_sources(self.ops, self.vk, slot)?;
        let importer = vendor.importer_ops();
        let resized = [&slot.depth, &slot.motion]
            .iter()
            .any(|guide| guide.image != 0 && [guide.width, guide.height] != [width, height]);
        if resized {
            // The Vulkan source fences and DX12 consumer timeline have retired.
            succeeded(unsafe {
                (importer.release_guides)(self.interop, slot.depth.image, slot.motion.image)
            })?;
            slot.depth = SharedImage::default();
            slot.motion = SharedImage::default();
            slot.guides_initialized = false;
            // A failed replacement must leave the previous frame ineligible.
            slot.state = unsafe { std::mem::zeroed() };
        }
        let ops = self.ops;
        for (target, format, width, height) in [
            (&mut slot.depth, R32F, width, height),
            (&mut slot.motion, RG16F, width, height),
            (
                &mut slot.hudless,
                BGRA8,
                self.swapchain.width,
                self.swapchain.height,
            ),
        ] {
            if target.image == 0 {
                importer.image(self.interop, format, width, height, target)?;
            }
        }
        for fence in &mut slot.fences {
            if fence.handle == 0 {
                succeeded(unsafe { (ops.create_fence)(self.vk, &mut fence.handle) })?;
            }
        }
        slot.done = 0;
        slot.scales = [frame.motion_scale_x, frame.motion_scale_y];
        let previous = &self.recording.last_prepared;
        let previous_extent = [previous.render_width, previous.render_height];
        rsf_core_fgvk_prepare_begin(&mut self.recording, &mut slot.state, record, commands);
        if previous_extent != [width, height] {
            slot.state.record.flags |= RSF_FRAME_FLAG_RESET;
        }
        for (source, target, role) in [
            (&frame.depth, &slot.depth, c"fg-depth"),
            (&frame.motion, &slot.motion, c"fg-motion-uv"),
        ] {
            let steps = copy_steps(self.family, source, target, slot.guides_initialized);
            ops.record(self.vk, commands, steps);
            unsafe {
                (ops.capture_record)(
                    self.vk,
                    commands,
                    source,
                    &slot.state.record,
                    role.as_ptr(),
                    slot.state.capture_group(false),
                );
            }
        }
        slot.state.recorded_prepare = 1;
        slot.state.prepared = 1;
        rsf_core_fgvk_prepare_accepted(&mut self.recording, &mut slot.state);
        self.status.prepared += 1;
        Ok(())
    }

    fn hudless(&mut self, commands: Handle, frame: &Frame, record: &Record) -> Result<(), i32> {
        if !self.image_valid(
            &frame.hudless,
            RGBA8,
            self.swapchain.width,
            self.swapchain.height,
        ) {
            return Err(INVALID);
        }
        let slot = &mut self.slots[slot_index(record.frame_id)];
        if slot.state.record.render_width != record.render_width
            || slot.state.record.render_height != record.render_height
            || rsf_core_fgvk_hudless_begin(&mut slot.state, record, commands) == 0
        {
            return Err(INVALID);
        }
        let steps = copy_steps(
            self.family,
            &frame.hudless,
            &slot.hudless,
            slot.hudless_initialized,
        );
        self.ops.record(self.vk, commands, steps);
        unsafe {
            (self.ops.capture_record)(
                self.vk,
                commands,
                &frame.hudless,
                &slot.state.record,
                c"fg-hudless-color".as_ptr(),
                slot.state.capture_group(true),
            );
        }
        slot.state.recorded_hudless = 1;
        slot.state.hudless = 1;
        self.status.tagged += 1;
        Ok(())
    }

    fn submitted(&mut self, commands: &[*mut c_void]) {
        let plan = submission_plan(self.slots.each_mut().map(|slot| &mut slot.state), commands);
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let bit = 1 << index;
            for (discarded, stage) in [
                (plan.discarded_prepare, GUIDES),
                (plan.discarded_hudless, HUDLESS),
            ] {
                if discarded & bit != 0 {
                    unsafe {
                        (self.ops.capture_discard)(
                            self.vk,
                            slot.state.capture_group(stage == HUDLESS),
                        )
                    };
                }
            }
            if plan.fences & bit == 0 {
                continue;
            }
            // A joint notification uses one marker. Split notifications use
            // separate fences so neither has to wait in the submission callback.
            let submitted =
                (slot.state.submitted_prepare * GUIDES) | (slot.state.submitted_hudless * HUDLESS);
            let pending = slot.fences[0].stages | slot.fences[1].stages;
            let fence = if slot.fences[0].stages == 0 {
                &mut slot.fences[0]
            } else {
                &mut slot.fences[1]
            };
            let result = unsafe { (self.ops.mark_submit)(self.vk, fence.handle) };
            if result != 0 {
                slot.state.completion_unknown = 1;
                self.terminal = true;
                self.status.result = result;
                return;
            }
            fence.stages = submitted & !pending;
            slot.guides_initialized |= slot.state.submitted_prepare != 0;
            slot.hudless_initialized |= slot.state.submitted_hudless != 0;
        }
    }

    fn present(
        &mut self,
        vendor: &impl Vendor,
        info: &PresentInfo,
        source: PresentSource,
    ) -> Result<(), i32> {
        if !self.open()
            || info.chain != self.chain
            || info.wait_count as usize > PRESENT_WAIT_LIMIT
            || (info.wait_count != 0 && info.waits.is_null())
        {
            return Err(INIT_FAILED);
        }
        let index = info.index as usize;
        if !self
            .buffers
            .get(index)
            .is_some_and(|buffer| buffer.acquired)
        {
            return Err(INIT_FAILED);
        }
        let observed = match source {
            PresentSource::Implicit => vendor.implicit_present_source(),
            PresentSource::Explicit(_) => None,
        };
        let source_frame =
            observed.unwrap_or_else(|| source.frame(self.recording.last_prepared.frame_id));
        let slot_index = slot_index(source_frame);
        let slot = &self.slots[slot_index];
        let identity = source.matches(&slot.state.record);
        let valid = rsf_core_fgvk_present_candidate(
            &self.recording,
            &slot.state,
            source_frame,
            u32::from(identity),
            u32::from(self.requested && source.allows_fg()),
        ) != 0;

        let matched_source =
            identity && source_frame != 0 && slot.state.record.frame_id == source_frame;
        let mut submission = vendor.begin_present(
            if matched_source || observed.is_some() {
                source_frame
            } else {
                0
            },
            valid,
        )?;
        let valid = submission.enabled();
        let ops = self.ops;
        let buffer = &mut self.buffers[index];
        let source_image = Image {
            device: self.device,
            image: buffer.image.image,
            format: self.swapchain.format,
            width: self.swapchain.width,
            height: self.swapchain.height,
            layout: SHADER_READ,
            copy_source: 1,
        };
        succeeded(unsafe { (ops.begin)(self.vk, buffer.commands) })?;
        if self.last_done == 0 {
            for image in [&self.fallback_depth, &self.fallback_motion] {
                let steps = clear_steps(self.family, image.image);
                ops.record(self.vk, buffer.commands, steps);
            }
        }
        let steps = copy_steps(self.family, &source_image, &buffer.shared, buffer.done != 0);
        ops.record(self.vk, buffer.commands, steps);
        succeeded(unsafe { (ops.end)(self.vk, buffer.commands) })?;

        // The SDK queue signals done after restoring HUD-less ownership.
        // A later ready signal must never bypass the prior done, across slots.
        let done = self.last_done.checked_add(2).ok_or(UNKNOWN)?;
        let ready = done - 1;
        let waits = if info.wait_count == 0 {
            &[]
        } else {
            // The marshaler owns this bounded array through the callback.
            unsafe { slice::from_raw_parts(info.waits, info.wait_count as usize) }
        };
        let plan = PresentSubmit::new(waits, self.timeline, self.last_done, ready);
        let result = unsafe { (ops.present_submit)(self.vk, buffer.commands, &plan) };
        if result != 0 {
            self.terminal = true;
            return Err(result);
        }
        buffer.done = done;
        self.last_done = done;

        let slot = &mut self.slots[slot_index];
        if valid {
            slot.done = done;
        }
        let (depth, motion, hudless, scales) = if valid {
            (
                slot.depth.dx12_resource,
                slot.motion.dx12_resource,
                slot.hudless.dx12_resource,
                slot.scales,
            )
        } else {
            // The SDK receives these placeholders only in its disabled path.
            (
                self.fallback_depth.dx12_resource,
                self.fallback_motion.dx12_resource,
                buffer.shared.dx12_resource,
                [1.0; 2],
            )
        };
        let frame = PresentFrame {
            record: matched_source.then_some(&slot.state.record),
            color: buffer.shared.dx12_resource,
            depth,
            motion,
            hudless,
            scales,
            ready,
            done,
            vsync: self.swapchain.vsync(),
        };
        let (result, status) = submission.present(frame);
        self.terminal = status.terminal;
        buffer.acquired = false;
        rsf_core_fgvk_presented(
            &mut self.recording,
            &slot.state,
            source_frame,
            u32::from(identity),
        );
        self.status.source_frame = if matches!(source, PresentSource::Implicit) || valid {
            source_frame
        } else {
            0
        };
        self.status.last_present_count = status.frames_presented;
        self.status.enabled = status.enabled;
        self.status.result = if result != 0 { result } else { status.result };
        self.status.generated_callbacks = status.generated_callbacks;
        let wsi_result = if result != 0 { UNKNOWN } else { 0 };
        if !info.result.is_null() {
            unsafe { info.result.write(wsi_result) };
        }
        succeeded(wsi_result)
    }

    fn drain(&mut self, vendor: &impl Vendor) -> Result<(), i32> {
        if self.retiring {
            return Ok(());
        }
        if self.terminal
            || self
                .slots
                .iter()
                .any(|slot| rsf_core_fgvk_unsubmitted(&slot.state) != 0)
        {
            return Err(NOT_READY);
        }
        if self.vk.is_null() {
            return Ok(());
        }
        self.wait_timeline(self.last_done, WAIT_NS)?;
        succeeded(unsafe { (self.ops.idle)(self.vk) })?;
        vendor.drain()?;
        for slot in &mut self.slots {
            for fence in &mut slot.fences {
                Self::complete_source(self.ops, self.vk, &slot.state, fence);
            }
        }
        succeeded(unsafe { (self.ops.capture_flush)(self.vk) })
    }

    fn retire(&mut self, vendor: &mut impl Vendor) -> Result<(), i32> {
        self.drain(vendor)?;
        self.retiring = true;
        let ops = self.ops;
        vendor.prepare_destroy()?;
        if self.pool != 0 {
            unsafe { (ops.destroy_pool)(self.vk, std::mem::take(&mut self.pool)) };
        }
        if !self.vk.is_null() {
            for buffer in &mut self.buffers {
                rsf_core_vkfg_release_image(unsafe { &*ops.images }, self.vk, &mut buffer.image);
            }
        }
        for slot in &mut self.slots {
            for fence in &mut slot.fences {
                if fence.handle != 0 {
                    unsafe { (ops.destroy_fence)(self.vk, std::mem::take(&mut fence.handle)) };
                }
            }
        }
        if !self.interop.is_null() {
            succeeded(unsafe { (vendor.importer_ops().destroy)(self.interop, 1) })?;
            self.interop = ptr::null_mut();
        }
        vendor.destroy()?;
        if !self.vk.is_null() {
            unsafe { (ops.platform_destroy)(std::mem::replace(&mut self.vk, ptr::null_mut())) };
        }
        Ok(())
    }
}

pub(super) unsafe fn images<V: Vendor>(
    owner: *const Owner<V>,
    count: *mut u32,
    output: *mut u64,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INIT_FAILED;
    };
    let state = unsafe { &*owner.state.get() };
    if count.is_null() || state.status.available == 0 || state.retiring {
        return INIT_FAILED;
    }
    if output.is_null() {
        unsafe { count.write(BUFFER_COUNT as u32) };
        return 0;
    }
    let copied = (unsafe { count.read() } as usize).min(BUFFER_COUNT);
    for (i, buffer) in state.buffers[..copied].iter().enumerate() {
        unsafe { output.add(i).write(buffer.image.image) };
    }
    unsafe { count.write(copied as u32) };
    if copied == BUFFER_COUNT {
        0
    } else {
        INCOMPLETE
    }
}

pub(super) unsafe fn acquire<V: Vendor>(
    owner: *mut Owner<V>,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INIT_FAILED;
    };
    if index.is_null() {
        return INIT_FAILED;
    }
    match unsafe { &mut *owner.state.get() }.acquire(timeout, semaphore, fence) {
        Ok(value) => {
            unsafe { index.write(value) };
            0
        }
        Err(result) => result,
    }
}

unsafe fn with_frame<V: Vendor>(
    owner: *mut Owner<V>,
    commands: Handle,
    frame: *const Frame,
    record_stage: fn(&mut Presentation, &V, Handle, &Frame, &Record) -> Result<(), i32>,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    let state = unsafe { &mut *owner.state.get() };
    if !state.open() || commands.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_FGVK_ABI_VERSION || !unsafe { has_capacity(frame.record) } {
        return INVALID;
    }
    let record = unsafe { &*frame.record };
    if record.frame_id > V::MAX_SOURCE_FRAME || !state.frame_valid(frame, record) {
        return INVALID;
    }
    record_stage(state, &owner.vendor, commands, frame, record)
        .err()
        .unwrap_or(0)
}

pub(super) unsafe fn prepare<V: Vendor>(
    owner: *mut Owner<V>,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe {
        with_frame(
            owner,
            commands,
            frame,
            |state, vendor, commands, frame, record| state.prepare(vendor, commands, frame, record),
        )
    }
}

pub(super) unsafe fn hudless<V: Vendor>(
    owner: *mut Owner<V>,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe {
        with_frame(
            owner,
            commands,
            frame,
            |state, _, commands, frame, record| state.hudless(commands, frame, record),
        )
    }
}

pub(super) unsafe fn submitted<V: Vendor>(
    owner: *mut Owner<V>,
    commands: *const Handle,
    count: u32,
) {
    if let Some(owner) = unsafe { owner.as_ref() }
        && !commands.is_null()
    {
        let state = unsafe { &mut *owner.state.get() };
        if state.open() {
            state.submitted(unsafe { slice::from_raw_parts(commands, count as usize) });
        }
    }
}

pub(super) unsafe fn enable_options<V: Vendor>(
    owner: *mut Owner<V>,
    enabled: u32,
    count: u32,
) -> i32 {
    unsafe { enable(owner, enabled, Some(count)) }
}

pub(super) unsafe fn enable<V: Vendor>(
    owner: *mut Owner<V>,
    enabled: u32,
    count: Option<u32>,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    let state = unsafe { &mut *owner.state.get() };
    if !state.open() || enabled > 1 {
        return INVALID;
    }
    if let Some(count) = count
        && let Err(result) = owner.vendor.set_generated(count)
    {
        return result;
    }
    state.requested = enabled == 1;
    0
}

pub(super) unsafe fn present<V: Vendor>(
    owner: *mut Owner<V>,
    info: *const PresentInfo,
    source: *const Record,
    explicit: u32,
) -> i32 {
    let (Some(owner), Some(info)) = (unsafe { (owner.as_ref(), info.as_ref()) }) else {
        return INIT_FAILED;
    };
    let Some(source) = (unsafe { PresentSource::read(source, explicit) }) else {
        return INIT_FAILED;
    };
    unsafe { &mut *owner.state.get() }
        .present(&owner.vendor, info, source)
        .err()
        .unwrap_or(0)
}

pub(super) unsafe fn status<V: Vendor>(owner: *const Owner<V>, status: *mut Status) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(status) } {
        return INVALID;
    }
    if unsafe { (*status).abi_version } != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    unsafe { status.write((*owner.state.get()).status) };
    0
}

pub(super) unsafe fn drain<V: Vendor>(owner: *mut Owner<V>) -> i32 {
    unsafe { owner.as_ref() }.map_or(0, |owner| {
        unsafe { &mut *owner.state.get() }
            .drain(&owner.vendor)
            .err()
            .unwrap_or(0)
    })
}

pub(super) unsafe fn destroy<V: Vendor>(owner: *mut Owner<V>) -> i32 {
    let Some(owner_ref) = (unsafe { owner.as_mut() }) else {
        return 0;
    };
    if let Err(result) = owner_ref.state.get_mut().retire(&mut owner_ref.vendor) {
        return result;
    }
    drop(unsafe { Box::from_raw(owner) });
    0
}

#[cfg(test)]
mod tests;
