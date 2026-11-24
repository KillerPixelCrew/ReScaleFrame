// SPDX-License-Identifier: MIT
// FidelityFX 3 Vulkan frame-generation owner. The SDK leaf in sdk.rs keeps the
// SDK module entries, its contexts, hook entry points and capture. This owner
// decides admission, including WSI registration and the process-wide route, slot reuse,
// copy transitions, completion evidence, presentation admission and retirement order.
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, NOT_COMPILED, NOT_READY,
    NOT_SUPPORTED, OK,
};
#[cfg(feature = "test-fixtures")]
mod fixture;
mod sdk;

use crate::allocate_handle;
use crate::sdk_module::{FIDELITYFX_DLL, SdkModule};
use crate::sr_session::has_capacity;
use crate::sys::{
    RSF_FGVK_ABI_VERSION, rsf_fgvk_frame as Frame, rsf_fgvk_setup as Setup,
    rsf_fgvk_status as Status,
};
use crate::sys::{RSF_SRVK_ABI_VERSION, rsf_srvk_image as Image};
use crate::vulkan_fg_copy::{SHADER_READ, record_shader_read_copy};
use crate::vulkan_fg_frames::FRAME_SLOTS as RSF_FGVK_FRAME_SLOTS;
use crate::vulkan_fg_frames::FrameSlot as Recording;
use crate::vulkan_fg_frames::Timeline as RecordingTimeline;
use crate::vulkan_fg_frames::*;
use crate::vulkan_fg_resources::Fences;
use crate::vulkan_fg_resources::Image as PrivateImage;
use crate::vulkan_fg_resources::{
    SAMPLED_TRANSFER, ensure_image, fence_submission, prepare_fences, release_fences, release_image,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::rsf_frame_record as Record;
use sdk::Host;
use std::ffi::{CStr, c_void};
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

const VK_SUCCESS: i32 = vk::Result::SUCCESS.as_raw();
const VK_SUBOPTIMAL: i32 = vk::Result::SUBOPTIMAL_KHR.as_raw();
const VK_INITIALIZATION_FAILED: i32 = vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
// rsfFfxSetVulkanWsiDispatch has no table for this device when it returns -3.
const WSI_FOREIGN_DEVICE: i32 = -3;
const RGBA8: u32 = vk::Format::R8G8B8A8_UNORM.as_raw() as u32;
const BGRA8: u32 = vk::Format::B8G8R8A8_UNORM.as_raw() as u32;
const RG16F: u32 = vk::Format::R16G16_SFLOAT.as_raw() as u32;
const R32F: u32 = vk::Format::R32_SFLOAT.as_raw() as u32;
const SRGB_NONLINEAR: u32 = vk::ColorSpaceKHR::SRGB_NONLINEAR.as_raw() as u32;
const SLOT_COUNT: usize = RSF_FGVK_FRAME_SLOTS as usize;

type Handle = *mut c_void;

/// Facts read from the borrowed VkSwapchainCreateInfoKHR.
#[derive(Clone, Copy, Default)]
struct Swapchain {
    width: u32,
    height: u32,
    format: u32,
    color_space: u32,
    layers: u32,
    old_chain: u64,
}

impl Swapchain {
    unsafe fn read(info: *const c_void) -> Option<Self> {
        if unsafe { info.cast::<vk::StructureType>().read() }
            != vk::StructureType::SWAPCHAIN_CREATE_INFO_KHR
        {
            return None;
        }
        let info = unsafe { &*info.cast::<vk::SwapchainCreateInfoKHR<'_>>() };
        Some(Self {
            width: info.image_extent.width,
            height: info.image_extent.height,
            format: info.image_format.as_raw() as u32,
            color_space: info.image_color_space.as_raw() as u32,
            layers: info.image_array_layers,
            old_chain: info.old_swapchain.as_raw(),
        })
    }

    fn supported(self) -> bool {
        self.old_chain == 0
            && self.width != 0
            && self.height != 0
            && self.layers == 1
            && self.color_space == SRGB_NONLINEAR
            && matches!(self.format, RGBA8 | BGRA8)
    }
}

// The SDK leaf's steps (sdk.rs). Status-returning steps report rsf_backend_result and WSI steps
// return the VkResult unchanged.
use sdk::leaf;
/// The process-wide WSI route. The corrected SDK keeps one WSI dispatch table
/// whose hooks carry no user pointer, so one owner holds the route from
/// admission until its native leaf is closed. A hook borrows the published
/// leaf for one callback. The synchronous generation callback also uses this
/// route to look up its configured source and count successful dispatches.
/// This lock is never held across a native, Vulkan or SDK call.
struct Route {
    owner: usize,
    device: usize,
    native: usize,
    borrows: usize,
    closing: bool,
    source: Option<Record>,
    generated_callbacks: u64,
}

impl Route {
    const FREE: Self = Self {
        owner: 0,
        device: 0,
        native: 0,
        borrows: 0,
        closing: false,
        source: None,
        generated_callbacks: 0,
    };

    fn admit(&mut self, owner: usize, device: usize) -> bool {
        if self.owner != 0 {
            return false;
        }
        *self = Self {
            owner,
            device,
            ..Self::FREE
        };
        true
    }

    fn publish(&mut self, owner: usize, native: usize) {
        if self.owner == owner {
            self.native = native;
        }
    }

    fn borrow(&mut self, device: usize, match_device: bool) -> usize {
        if self.native == 0 || self.closing || (match_device && device != self.device) {
            return 0;
        }
        self.borrows += 1;
        self.native
    }

    fn give_back(&mut self) {
        self.borrows = self.borrows.saturating_sub(1);
    }

    fn matches_native(&self, native: Handle) -> bool {
        self.native != 0 && self.native == native.addr() && !self.closing
    }

    /// Refuses new borrows and reports whether none is still active.
    fn close(&mut self, owner: usize) -> bool {
        if self.owner != owner {
            return true;
        }
        self.closing = true;
        self.borrows == 0
    }

    fn release(&mut self, owner: usize) {
        if self.owner == owner {
            *self = Self::FREE;
        }
    }
}

static ROUTE: Mutex<Route> = Mutex::new(Route::FREE);

fn route() -> MutexGuard<'static, Route> {
    ROUTE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn presented(result: i32) -> bool {
    matches!(result, VK_SUCCESS | VK_SUBOPTIMAL)
}

/// Nested diagnostics scope. The native leaf writes nothing unless the
/// opt-in Vulkan timing file is configured.
struct Timing {
    stage: &'static str,
    frame: u64,
    start: i64,
}

impl Timing {
    fn new(stage: &'static str, frame: u64) -> Self {
        Self {
            stage,
            frame,
            start: leaf::timing_begin(),
        }
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        leaf::timing_end(self.stage, self.frame, self.start);
    }
}

struct Slot {
    state: Recording,
    fences: Fences,
    color: PrivateImage,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            state: unsafe { std::mem::zeroed() },
            fences: Fences::default(),
            color: PrivateImage::default(),
        }
    }
}

// The shared primary owner in vulkan_fg.rs serializes every call. The SDK's
// generation callback runs inside the native present call and takes a copy
// of the configured source from the route without borrowing this owner.
pub(crate) struct Owner {
    host: &'static Host,
    native: Handle,
    module: Option<SdkModule>,
    resources: Handle,
    device: Handle,
    width: u32,
    height: u32,
    depth_inverted: u32,
    depth_infinite: u32,
    motion_jittered: u32,
    chain: u64,
    context: bool,
    effect: bool,
    enabled: bool,
    configured: bool,
    retiring: bool,
    wsi_registered: bool,
    timeline: RecordingTimeline,
    slots: [Slot; SLOT_COUNT],
    status: Status,
}

impl Owner {
    fn new(host: &'static Host, setup: &Setup, swapchain: Swapchain) -> Self {
        Self {
            host,
            native: ptr::null_mut(),
            module: None,
            resources: ptr::null_mut(),
            device: setup.graphics.device,
            width: swapchain.width,
            height: swapchain.height,
            depth_inverted: setup.graphics.depth_inverted,
            depth_infinite: setup.graphics.depth_infinite,
            motion_jittered: setup.graphics.motion_jittered,
            chain: 0,
            context: false,
            effect: false,
            enabled: false,
            configured: false,
            retiring: false,
            wsi_registered: false,
            timeline: unsafe { std::mem::zeroed() },
            slots: std::array::from_fn(|_| Slot::default()),
            status: Status {
                struct_size: size_of::<Status>() as u32,
                abi_version: RSF_FGVK_ABI_VERSION,
                ..Default::default()
            },
        }
    }

    fn id(&self) -> usize {
        ptr::from_ref(self).addr()
    }

    fn initialize(&mut self, setup: &Setup) -> i32 {
        // One owner holds the WSI route before the module is loaded.
        if !route().admit(self.id(), self.device.addr()) {
            return NOT_SUPPORTED;
        }
        let module = match unsafe {
            SdkModule::open(
                self.host.loader,
                setup.graphics.runtime_directory_utf8,
                FIDELITYFX_DLL,
            )
        } {
            Ok(module) => module,
            Err(result) => return result,
        };
        let library = module.as_ptr();
        self.module = Some(module);
        let opened = unsafe {
            leaf::open(
                setup,
                self.host,
                library,
                self.width,
                self.height,
                &mut self.native,
                &mut self.resources,
            )
        };
        if opened != OK {
            return opened;
        }
        if self.native.is_null() || self.resources.is_null() {
            return INIT_FAILED;
        }
        if unsafe { leaf::set_wsi(self.native, 1) } != OK {
            return INIT_FAILED;
        }
        self.wsi_registered = true;
        // The SDK calls the create, image and destroy hooks synchronously
        // inside swapchain-context creation and failure cleanup.
        route().publish(self.id(), self.native.expose_provenance());
        let mut context = 0;
        let created = unsafe { leaf::create_swapchain(self.native, &mut self.chain, &mut context) };
        self.context = context != 0;
        if created != OK {
            return created;
        }
        if !self.context || self.chain == 0 {
            return INIT_FAILED;
        }
        let queried = unsafe { leaf::query_swapchain(self.native) };
        if queried != OK {
            return queried;
        }
        let mut context = 0;
        let created = unsafe { leaf::create_effect(self.native, &mut context) };
        self.effect = context != 0;
        if created != OK {
            return created;
        }
        if !self.effect {
            return INIT_FAILED;
        }
        let provider = unsafe { leaf::query_provider(self.native) };
        if provider != OK {
            return provider;
        }
        self.status.available = 1;
        self.status.result = OK;
        OK
    }

    fn wants(&self, frame: u64) -> bool {
        unsafe { leaf::capture_wants(self.native, frame) != 0 }
    }

    fn capture(&self, commands: Handle, image: &Image, record: &Record, role: &CStr) {
        unsafe { leaf::capture_record(self.native, commands, image, record, role.as_ptr()) };
    }

    fn valid_record<'a>(&self, frame: &'a Frame) -> Option<&'a Record> {
        if !unsafe { has_capacity(frame.record) } {
            return None;
        }
        let record = unsafe { &*frame.record };
        let valid = valid_source_record(record, self.width, self.height, self.depth_infinite != 0)
            && record
                .camera
                .world_to_view
                .iter()
                .all(|value| value.is_finite());
        valid.then_some(record)
    }

    fn guides_valid(&self, frame: &Frame, record: &Record) -> bool {
        let guide = |image: &Image, format: u32| {
            crate::vulkan_sr::valid_image(
                image,
                self.device,
                [record.render_width, record.render_height],
                SHADER_READ,
                &[format],
            )
        };
        guide(&frame.depth, R32F)
            && guide(&frame.motion, RG16F)
            && [frame.motion_scale_x, frame.motion_scale_y]
                .into_iter()
                .all(|scale| scale.is_finite() && scale != 0.0)
            && record.camera.depth_inverted == self.depth_inverted
            && record.camera.motion_jittered == self.motion_jittered
    }

    unsafe fn images(&self, count: *mut u32, output: *mut u64) -> i32 {
        unsafe {
            bounded_swapchain_images(count, output, |count, images| {
                leaf::images(self.native, count, images)
            })
        }
    }

    fn prepare(&mut self, commands: Handle, frame: &Frame) -> i32 {
        let Some(record) = self.valid_record(frame) else {
            return INVALID_ARGUMENT;
        };
        let _timing = Timing::new("ffx_prepare_total", record.frame_id);
        if !self.enabled && !self.wants(record.frame_id) {
            return OK;
        }
        if !self.guides_valid(frame, record) {
            return INVALID_ARGUMENT;
        }
        let slot = &mut self.slots[slot_index(record.frame_id)];
        if !slot.state.can_prepare() {
            return NOT_READY;
        }
        unsafe { leaf::fence_timing(self.native, record.frame_id) };
        let fences = prepare_fences(
            leaf::resources(),
            self.resources,
            &mut slot.state,
            &mut slot.fences,
        );
        unsafe { leaf::fence_timing(self.native, 0) };
        if fences != OK {
            return fences;
        }
        self.timeline
            .prepare_begin(&mut slot.state, record, commands);
        // Dispatch can record GPU work before reporting an error. Submission still
        // owns that recording even when it never becomes an accepted FG input.
        slot.state.recorded_prepare = true;
        if self.enabled {
            if unsafe { leaf::prepare(self.native, commands, frame) } != OK {
                return FEATURE_FAILED;
            }
            slot.state.prepared = true;
            self.status.prepared += 1;
        }
        self.timeline.prepare_accepted(&mut slot.state);
        self.capture(commands, &frame.depth, record, c"fg-depth");
        self.capture(commands, &frame.motion, record, c"fg-motion-uv");
        OK
    }

    fn hudless(&mut self, commands: Handle, frame: &Frame) -> i32 {
        let Some(record) = self.valid_record(frame) else {
            return INVALID_ARGUMENT;
        };
        if !self.enabled && !self.wants(record.frame_id) {
            return OK;
        }
        let (device, width, height) = (self.device, self.width, self.height);
        let input = &frame.hudless;
        let slot = &mut self.slots[slot_index(record.frame_id)];
        if input.device != device
            || input.image == 0
            || input.copy_source == 0
            || input.format != RGBA8
            || input.width != width
            || input.height != height
            || input.layout != SHADER_READ
            || !slot.state.hudless_begin(record, commands)
        {
            return INVALID_ARGUMENT;
        }
        if ensure_image(
            leaf::resources(),
            self.resources,
            &mut slot.color,
            [width, height],
            RGBA8,
            SAMPLED_TRANSFER,
            false,
        )
        .is_err()
        {
            return INIT_FAILED;
        }
        slot.state.recorded_hudless = true;
        unsafe {
            record_shader_read_copy(
                leaf::barrier,
                leaf::copy,
                self.native,
                commands,
                input.image,
                &slot.color,
                [width, height],
            )
        };
        slot.color.initialized = 1;
        slot.state.hudless = true;
        let image = Image {
            device,
            image: slot.color.image,
            format: RGBA8,
            width,
            height,
            layout: SHADER_READ,
            copy_source: 1,
        };
        if self.enabled {
            self.status.tagged += 1;
        }
        self.capture(commands, &image, record, c"fg-hudless-color");
        OK
    }

    fn submitted(&mut self, commands: &[*mut c_void]) {
        let plan = submission_plan(self.slots.each_mut().map(|slot| &mut slot.state), commands);
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let bit = 1 << index;
            if plan.discarded_hudless & bit != 0 {
                slot.color.initialized = 0;
            }
            if plan.fences & bit != 0 {
                fence_submission(
                    leaf::resources(),
                    self.resources,
                    &mut slot.state,
                    &slot.fences,
                    false,
                );
            }
        }
    }

    fn configure(&mut self, slot: Option<usize>) -> i32 {
        let (record, color, frame) = match slot {
            Some(index) => {
                let slot = &self.slots[index];
                (
                    Some(slot.state.record),
                    slot.color.image,
                    slot.state.record.frame_id,
                )
            }
            None => (None, 0, self.timeline.last_prepared.frame_id),
        };
        let stage = match (slot.is_some(), self.configured) {
            (true, true) => "ffx_configure_update",
            (true, false) => "ffx_configure_enable",
            (false, true) => "ffx_configure_disable",
            (false, false) => "ffx_configure_disabled",
        };
        let enabled = record.is_some();
        if unsafe { leaf::configure(self.native, u32::from(enabled), color, frame, stage) } != OK {
            return FEATURE_FAILED;
        }
        let mut route = route();
        if route.matches_native(self.native) {
            route.source = record;
        }
        self.configured = enabled;
        OK
    }

    fn present(&mut self, info: *const c_void, chain: u64, source: &PresentSource) -> i32 {
        let source_frame = source.frame(self.timeline.last_prepared.frame_id);
        let _timing = Timing::new("ffx_present_total", source_frame);
        if chain == 0 || chain != self.chain {
            return VK_INITIALIZATION_FAILED;
        }
        let index = slot_index(source_frame);
        let identity = source.matches(&self.slots[index].state.record);
        // A camera reset discards dispatch history through the generation
        // callback, rather than disabling the presenter and draining its queues
        // and pacing threads. Keep activation stable while this frame's
        // submitted input leases are valid.
        let valid = self.timeline.present_candidate(
            &self.slots[index].state,
            source_frame,
            identity,
            self.enabled && source.allows_fg(),
        );
        let configured = self.configure(valid.then_some(index));
        if configured != OK {
            self.status.result = configured;
            return VK_INITIALIZATION_FAILED;
        }
        let result = unsafe {
            leaf::present(
                self.native,
                info,
                source_frame,
                &mut self.status.last_present_count,
            )
        };
        let slot = &mut self.slots[index];
        if valid {
            // FrameInterpolationSwapChainVK::presentInterpolated submits the
            // interpolation reading this HUD-less copy, then returns the result
            // of an earlier asynchronous present. Every configured source needs
            // its consumer fence, whatever that result is.
            fence_submission(
                leaf::resources(),
                self.resources,
                &mut slot.state,
                &slot.fences,
                true,
            );
        }
        self.timeline.presented(&slot.state, source_frame, identity);
        self.status.source_frame = if matches!(source, PresentSource::Explicit(_)) && !valid {
            0
        } else {
            source_frame
        };
        self.status.enabled = u32::from(valid);
        self.status.result = if presented(result) {
            OK
        } else {
            FEATURE_FAILED
        };
        result
    }

    fn unsubmitted(&self) -> bool {
        self.slots.iter().any(|slot| slot.state.unsubmitted())
    }

    fn drain(&mut self) -> i32 {
        // Once retirement has started, graphics are already drained. The SDK
        // swapchain may have been destroyed even while other cleanup is pending.
        if self.retiring || self.status.available == 0 {
            return OK;
        }
        // Device idle cannot cover commands the host has not submitted yet.
        if self.unsubmitted() {
            return NOT_READY;
        }
        if unsafe { leaf::drain(self.native) } != OK {
            return NOT_READY;
        }
        unsafe { leaf::capture_flush(self.native) };
        OK
    }

    // Each step records its own completion, so a refused teardown can be
    // retried without repeating a release that already succeeded.
    fn retire(&mut self) -> i32 {
        if self.native.is_null() {
            // A failed open freed its own partial leaf and registered no hooks.
            self.module = None;
            route().release(self.id());
            return OK;
        }
        if !self.retiring {
            if self.unsubmitted() {
                return NOT_READY;
            }
            // An owner that never became available has no engine work to drain.
            if self.status.available != 0 {
                if self.effect && self.configure(None) != OK {
                    return FEATURE_FAILED;
                }
                if self.chain != 0 && self.drain() != OK {
                    return NOT_READY;
                }
            }
            self.retiring = true;
            self.status.available = 0;
            self.status.enabled = 0;
        }
        if self.chain != 0 && !self.context {
            // A replacement chain without its SDK context cannot be passed to either destructor.
            // Keep the provider and WSI route live for this unproven SDK allocation.
            return NOT_READY;
        }
        let native = self.native;
        if self.effect {
            if unsafe { leaf::destroy_effect(native) } != OK {
                return FEATURE_FAILED;
            }
            self.effect = false;
            let mut route = route();
            if route.matches_native(native) {
                route.source = None;
            }
        }
        if self.chain != 0 {
            unsafe { leaf::destroy_chain(native) };
            self.chain = 0;
        }
        if self.context {
            if unsafe { leaf::destroy_swapchain(native) } != OK {
                return FEATURE_FAILED;
            }
            self.context = false;
        }
        // Destroying the chain joined the SDK interpolation and presenter
        // threads (FrameInterpolationSwapchainVK.cpp destroySwapchain), so no
        // SDK thread of this owner can enter a hook. Unregistering alone would
        // not prove that: the bridge copies its table entry under its lock and
        // calls the hook after releasing it. Refuse new borrows here and keep
        // the leaf while a callback still holds one.
        if !route().close(self.id()) {
            return NOT_READY;
        }
        if self.wsi_registered {
            let cleared = unsafe { leaf::set_wsi(native, 0) };
            if !matches!(cleared, OK | WSI_FOREIGN_DEVICE) {
                return FEATURE_FAILED;
            }
            self.wsi_registered = false;
        }
        for slot in &mut self.slots {
            release_image(leaf::resources(), self.resources, &mut slot.color);
            release_fences(leaf::resources(), self.resources, &mut slot.fences);
        }
        // Keep admission through module unload, after the WSI table no longer points at the host.
        unsafe { leaf::close(native) };
        self.native = ptr::null_mut();
        self.module = None;
        route().release(self.id());
        OK
    }
}

/// Scoped borrow for a native WSI hook. `match_device` is zero for the queue
/// hooks, which receive no device. Returns null without a published leaf,
/// once retirement has closed the route, or for another device. Each non-null
/// result is paired with one `rsf_core_ffxgvk_wsi_return`.
extern "C" fn rsf_core_ffxgvk_wsi_borrow(device: Handle, match_device: u32) -> Handle {
    ptr::with_exposed_provenance_mut(route().borrow(device.addr(), match_device != 0))
}

extern "C" fn rsf_core_ffxgvk_wsi_return() {
    route().give_back();
}

unsafe fn live<'a>(owner: *mut Owner) -> Option<&'a mut Owner> {
    unsafe { owner.as_mut() }.filter(|owner| !owner.retiring)
}

/// The provider entry: the production host when CMake found the FidelityFX Vulkan headers (the
/// `ffx-vk` feature), otherwise the disabled build.
pub(crate) unsafe extern "C" fn rsf_ffxgvk_create(
    setup: *const Setup,
    output: *mut *mut Owner,
    chain: *mut u64,
) -> i32 {
    let host = cfg!(feature = "ffx-vk").then_some(&sdk::NATIVE_HOST);
    unsafe { create(host, setup, output, chain) }
}

/// Creates an owner over the SDK leaf with `host`, which outlives every owner (`sdk::NATIVE_HOST`,
/// or a fixture's table). `host` is None in the disabled build, which validates the arguments and
/// reports NOT_COMPILED./// `host` is None in the disabled build.
unsafe fn create(
    host: Option<&'static Host>,
    setup: *const Setup,
    output: *mut *mut Owner,
    chain: *mut u64,
) -> i32 {
    if output.is_null() || chain.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe {
        output.write(ptr::null_mut());
        chain.write(0);
    }
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let graphics = &setup.graphics;
    if !unsafe { has_capacity(graphics) } {
        return INVALID_ARGUMENT;
    }
    if graphics.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if graphics.device.is_null()
        || graphics.physical_device.is_null()
        || graphics.instance.is_null()
        || graphics.device_proc.is_none()
        || graphics.instance_proc.is_none()
        || graphics.runtime_directory_utf8.is_null()
        || setup.create_info.is_null()
        || setup.graphics_submit.is_none()
        || setup.lock_queues.is_none()
        || setup.unlock_queues.is_none()
        || graphics.depth_inverted > 1
        || graphics.depth_infinite > 1
        || graphics.motion_jittered > 1
    {
        return INVALID_ARGUMENT;
    }
    // Four distinct queues of one family: graphics, compute, present, acquire.
    for (index, queue) in setup.queues.iter().enumerate() {
        if queue.is_null() || setup.queues[..index].contains(queue) {
            return INVALID_ARGUMENT;
        }
    }
    let Some(host) = host else {
        return NOT_COMPILED;
    };
    let Some(swapchain) =
        unsafe { Swapchain::read(setup.create_info) }.filter(|swapchain| swapchain.supported())
    else {
        return NOT_SUPPORTED;
    };
    let owner = allocate_handle(Owner::new(host, setup, swapchain));
    if owner.is_null() {
        return INIT_FAILED;
    }
    let state = unsafe { &mut *owner };
    let result = state.initialize(setup);
    if result != OK {
        state.status.result = result;
        // A refused cleanup retains the owner for the documented destroy retry.
        if unsafe { rsf_ffxgvk_destroy(owner) } != OK {
            unsafe { output.write(owner) };
        }
        return result;
    }
    unsafe {
        output.write(owner);
        chain.write(state.chain);
    }
    OK
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_images(
    owner: *mut Owner,
    count: *mut u32,
    images: *mut u64,
) -> i32 {
    match unsafe { live(owner) } {
        Some(owner) if !count.is_null() => unsafe { owner.images(count, images) },
        _ => VK_INITIALIZATION_FAILED,
    }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_acquire(
    owner: *mut Owner,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    match unsafe { live(owner) } {
        Some(owner) if !index.is_null() => unsafe {
            leaf::acquire(
                owner.native,
                timeout,
                semaphore,
                fence,
                index,
                owner.timeline.last_prepared.frame_id,
            )
        },
        _ => VK_INITIALIZATION_FAILED,
    }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_enable(owner: *mut Owner, enabled: u32) -> i32 {
    match unsafe { owner.as_mut() } {
        Some(owner) if enabled <= 1 => {
            owner.enabled = enabled != 0;
            OK
        }
        _ => INVALID_ARGUMENT,
    }
}

unsafe fn with_frame(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
    stage: fn(&mut Owner, Handle, &Frame) -> i32,
) -> i32 {
    if owner.is_null() || commands.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID_ARGUMENT;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    match unsafe { live(owner) } {
        Some(owner) => stage(owner, commands, frame),
        None => INVALID_ARGUMENT,
    }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_prepare(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe { with_frame(owner, commands, frame, Owner::prepare) }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_hudless(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe { with_frame(owner, commands, frame, Owner::hudless) }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_submitted(
    owner: *mut Owner,
    commands: *const Handle,
    count: u32,
) {
    if let Some(owner) = unsafe { live(owner) }
        && !commands.is_null()
    {
        owner.submitted(unsafe { std::slice::from_raw_parts(commands, count as usize) });
    }
}

unsafe fn present_native(
    owner: *mut Owner,
    info: *const c_void,
    source: *const Record,
    explicit_source: u32,
) -> i32 {
    let chain = unsafe { crate::vulkan_fg_wsi::single_present_chain(info) }
        .map_or(0, |chain| chain.as_raw());
    unsafe { rsf_core_ffxgvk_present(owner, info, chain, source, explicit_source) }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_present(owner: *mut Owner, info: *const c_void) -> i32 {
    unsafe { present_native(owner, info, ptr::null(), 0) }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_present_source(
    owner: *mut Owner,
    info: *const c_void,
    source: *const Record,
) -> i32 {
    unsafe { present_native(owner, info, source, 1) }
}

unsafe extern "C" fn rsf_core_ffxgvk_present(
    owner: *mut Owner,
    info: *const c_void,
    chain: u64,
    source: *const Record,
    explicit: u32,
) -> i32 {
    let Some(source) = (unsafe { PresentSource::read(source, explicit) }) else {
        return VK_INITIALIZATION_FAILED;
    };
    match unsafe { live(owner) } {
        Some(owner) if !info.is_null() => owner.present(info, chain, &source),
        _ => VK_INITIALIZATION_FAILED,
    }
}

unsafe extern "C" fn rsf_core_ffxgvk_generation_record(
    native: Handle,
    frame: u64,
    out: *mut Record,
) -> u32 {
    let route = route();
    if out.is_null() || !route.matches_native(native) {
        return 0;
    }
    let Some(record) = route.source.filter(|record| record.frame_id == frame) else {
        return 0;
    };
    unsafe { out.write(record) };
    1
}

extern "C" fn rsf_core_ffxgvk_note_generated(native: Handle) {
    let mut route = route();
    if route.matches_native(native) {
        route.generated_callbacks = route.generated_callbacks.wrapping_add(1);
    }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_get_status(
    owner: *const Owner,
    status: *mut Status,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(status) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*status).abi_version } != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let mut value = owner.status;
    {
        let route = route();
        if route.owner == owner.id() {
            value.generated_callbacks = route.generated_callbacks;
        }
    }
    unsafe { status.write(value) };
    OK
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_drain(owner: *mut Owner) -> i32 {
    match unsafe { owner.as_mut() } {
        Some(owner) if !owner.native.is_null() => owner.drain(),
        _ => OK,
    }
}

pub(crate) unsafe extern "C" fn rsf_ffxgvk_destroy(owner: *mut Owner) -> i32 {
    let Some(state) = (unsafe { owner.as_mut() }) else {
        return OK;
    };
    let result = state.retire();
    if result == OK {
        drop(unsafe { Box::from_raw(owner) });
    }
    result
}
