// SPDX-License-Identifier: MIT
// FidelityFX 3 Vulkan frame-generation owner. The native leaf in
// native_fg_vulkan.cpp keeps the SDK module, its contexts, the WSI hook bodies
// and their marshalling, command recording, capture and timing. This owner
// decides admission, including WSI registration and the process-wide route, slot reuse,
// completion evidence, presentation admission and retirement order.
use crate::allocate_handle;
use crate::sdk_module::{FIDELITYFX_DLL, Loader, SdkModule};
use crate::sr_session::has_capacity;
use crate::vulkan_fg_abi::{
    RSF_FGVK_ABI_VERSION, RSF_FGVK_FRAME_SLOTS, rsf_fgvk_frame as Frame,
    rsf_fgvk_frame_slot as Recording, rsf_fgvk_setup as Setup, rsf_fgvk_status as Status,
    rsf_fgvk_submit_plan as SubmitPlan, rsf_fgvk_timeline as RecordingTimeline,
};
use crate::vulkan_fg_frames::*;
use crate::vulkan_fg_resources::{
    rsf_core_vkfg_ensure_image, rsf_core_vkfg_fence_submission, rsf_core_vkfg_prepare_fences,
    rsf_core_vkfg_release_fences, rsf_core_vkfg_release_image,
};
use crate::vulkan_fg_resources_abi::{
    rsf_vkfg_fences as Fences, rsf_vkfg_image as PrivateImage, rsf_vkfg_platform as Resources,
};
use crate::vulkan_sr_abi::{RSF_SRVK_ABI_VERSION, rsf_srvk_image as Image};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record as Record,
};
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_COMPILED: i32 = -3;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const VK_SUCCESS: i32 = 0;
const VK_INCOMPLETE: i32 = 5;
const VK_SUBOPTIMAL: i32 = 1_000_001_003;
const VK_INITIALIZATION_FAILED: i32 = -3;
// rsfFfxSetVulkanWsiDispatch has no table for this device when it returns -3.
const WSI_FOREIGN_DEVICE: i32 = -3;
const RGBA8: u32 = 37;
const BGRA8: u32 = 44;
const RG16F: u32 = 83;
const R32F: u32 = 100;
const SHADER_READ: u32 = 5;
const SRGB_NONLINEAR: u32 = 0;
const SLOT_COUNT: usize = RSF_FGVK_FRAME_SLOTS as usize;

type Handle = *mut c_void;

/// Facts the native leaf reads from the borrowed VkSwapchainCreateInfoKHR.
#[repr(C)]
#[derive(Clone, Copy)]
struct Swapchain {
    width: u32,
    height: u32,
    format: u32,
    color_space: u32,
    layers: u32,
    structure_valid: u32,
    old_chain: u64,
}

impl Swapchain {
    fn supported(self) -> bool {
        self.structure_valid != 0
            && self.old_chain == 0
            && self.width != 0
            && self.height != 0
            && self.layers == 1
            && self.color_space == SRGB_NONLINEAR
            && matches!(self.format, RGBA8 | BGRA8)
    }
}

#[repr(C)]
struct Ops {
    loader: Loader,
    open: unsafe extern "C" fn(*const Setup, Handle, u32, u32, *mut Handle, *mut Handle) -> i32,
    close: unsafe extern "C" fn(Handle),
    set_wsi: unsafe extern "C" fn(Handle, u32) -> i32,
    create_swapchain: unsafe extern "C" fn(Handle, *mut u64, *mut u32) -> i32,
    query_swapchain: unsafe extern "C" fn(Handle) -> i32,
    create_effect: unsafe extern "C" fn(Handle, *mut u32) -> i32,
    query_provider: unsafe extern "C" fn(Handle) -> i32,
    destroy_effect: unsafe extern "C" fn(Handle) -> i32,
    destroy_chain: unsafe extern "C" fn(Handle),
    destroy_swapchain: unsafe extern "C" fn(Handle) -> i32,
    images: unsafe extern "C" fn(Handle, *mut u32, *mut u64) -> i32,
    acquire: unsafe extern "C" fn(Handle, u64, u64, u64, *mut u32, u64) -> i32,
    prepare: unsafe extern "C" fn(Handle, Handle, *const Frame) -> i32,
    copy: unsafe extern "C" fn(Handle, Handle, *const Image, u64, u32),
    configure: unsafe extern "C" fn(Handle, u32, u64, u64, *const c_char) -> i32,
    present: unsafe extern "C" fn(Handle, *const c_void, u64, *mut u64) -> i32,
    drain: unsafe extern "C" fn(Handle) -> i32,
    capture_wants: unsafe extern "C" fn(Handle, u64) -> u32,
    capture_record:
        unsafe extern "C" fn(Handle, Handle, *const Image, *const Record, *const c_char),
    capture_flush: unsafe extern "C" fn(Handle),
    fence_timing: unsafe extern "C" fn(Handle, u64),
    timing_begin: unsafe extern "C" fn() -> i64,
    timing_end: unsafe extern "C" fn(*const c_char, u64, i64),
    resources: &'static Resources,
}

const _: () = {
    assert!(size_of::<Swapchain>() == 32);
    assert!(size_of::<Ops>() == 26 * size_of::<Handle>());
};

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

fn slot_index(frame: u64) -> usize {
    (frame % SLOT_COUNT as u64) as usize
}

/// Nested diagnostics scope. The native leaf writes nothing unless the
/// opt-in Vulkan timing file is configured.
struct Timing {
    ops: &'static Ops,
    stage: &'static CStr,
    frame: u64,
    start: i64,
}

impl Timing {
    fn new(ops: &'static Ops, stage: &'static CStr, frame: u64) -> Self {
        Self {
            ops,
            stage,
            frame,
            start: unsafe { (ops.timing_begin)() },
        }
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        unsafe { (self.ops.timing_end)(self.stage.as_ptr(), self.frame, self.start) };
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
struct Owner {
    ops: &'static Ops,
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
    admitted: bool,
    wsi_registered: bool,
    timeline: RecordingTimeline,
    slots: [Slot; SLOT_COUNT],
    status: Status,
}

impl Owner {
    fn new(ops: &'static Ops, setup: &Setup, swapchain: Swapchain) -> Self {
        Self {
            ops,
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
            admitted: false,
            wsi_registered: false,
            timeline: unsafe { std::mem::zeroed() },
            slots: std::array::from_fn(|_| Slot::default()),
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

    fn id(&self) -> usize {
        ptr::from_ref(self).addr()
    }

    fn initialize(&mut self, setup: &Setup) -> i32 {
        let ops = self.ops;
        // One owner holds the WSI route before the module is loaded, as the
        // former native compare-exchange did at the start of open.
        if !route().admit(self.id(), self.device.addr()) {
            return NOT_SUPPORTED;
        }
        self.admitted = true;
        let module = match unsafe {
            SdkModule::open(
                ops.loader,
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
            (ops.open)(
                setup,
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
        if unsafe { (ops.set_wsi)(self.native, 1) } != OK {
            return INIT_FAILED;
        }
        self.wsi_registered = true;
        // The SDK calls the create, image and destroy hooks synchronously
        // inside swapchain-context creation and failure cleanup.
        route().publish(self.id(), self.native.expose_provenance());
        let mut context = 0;
        let created = unsafe { (ops.create_swapchain)(self.native, &mut self.chain, &mut context) };
        self.context = context != 0;
        if created != OK {
            return created;
        }
        if !self.context || self.chain == 0 {
            return INIT_FAILED;
        }
        let queried = unsafe { (ops.query_swapchain)(self.native) };
        if queried != OK {
            return queried;
        }
        let mut context = 0;
        let created = unsafe { (ops.create_effect)(self.native, &mut context) };
        self.effect = context != 0;
        if created != OK {
            return created;
        }
        if !self.effect {
            return INIT_FAILED;
        }
        let provider = unsafe { (ops.query_provider)(self.native) };
        if provider != OK {
            return provider;
        }
        self.status.available = 1;
        self.status.result = OK;
        OK
    }

    fn wants(&self, frame: u64) -> bool {
        unsafe { (self.ops.capture_wants)(self.native, frame) != 0 }
    }

    fn capture(&self, commands: Handle, image: &Image, record: &Record, role: &CStr) {
        unsafe { (self.ops.capture_record)(self.native, commands, image, record, role.as_ptr()) };
    }

    fn valid_record<'a>(&self, frame: &'a Frame) -> Option<&'a Record> {
        if !unsafe { has_capacity(frame.record) } {
            return None;
        }
        let record = unsafe { &*frame.record };
        let camera = &record.camera;
        if record.abi_version != RSF_GAME_FRAME_ABI_VERSION
            || (camera.struct_size as usize) < size_of_val(camera)
            || camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION
        {
            return None;
        }
        let finite = [
            &camera.view_to_clip,
            &camera.clip_to_view,
            &camera.view_to_world,
            &camera.world_to_view,
            &camera.clip_to_previous_clip,
        ]
        .into_iter()
        .flatten()
        .all(|value| value.is_finite());
        let positive = |value: f32| value.is_finite() && value > 0.0;
        let valid = finite
            && record.session_id != 0
            && record.frame_id != 0
            && record.render_width != 0
            && record.render_height != 0
            && record.render_width <= self.width
            && record.render_height <= self.height
            && record.output_width == self.width
            && record.output_height == self.height
            && camera.render_width == record.render_width
            && camera.render_height == record.render_height
            && camera.output_width == self.width
            && camera.output_height == self.height
            && positive(record.frame_time_ms)
            && positive(camera.near_plane)
            && camera.far_plane.is_finite()
            && camera.far_plane >= 0.0
            && (self.depth_infinite != 0 || camera.far_plane > camera.near_plane)
            && positive(camera.vertical_fov_radians)
            // The former `< 3.141593f` rounds to the f32 just above PI.
            && camera.vertical_fov_radians <= std::f32::consts::PI
            && camera.jitter_pixels.iter().all(|value| value.is_finite());
        valid.then_some(record)
    }

    fn guides_valid(&self, frame: &Frame, record: &Record) -> bool {
        let guide = |image: &Image, format: u32| {
            image.device == self.device
                && image.image != 0
                && image.format == format
                && image.width == record.render_width
                && image.height == record.render_height
                && image.layout == SHADER_READ
        };
        guide(&frame.depth, R32F)
            && guide(&frame.motion, RG16F)
            && [frame.motion_scale_x, frame.motion_scale_y]
                .into_iter()
                .all(|scale| scale.is_finite() && scale != 0.0)
            && record.camera.depth_inverted == self.depth_inverted
            && record.camera.motion_jittered == self.motion_jittered
    }

    fn images(&self, count: *mut u32, output: *mut u64) -> i32 {
        let images = self.ops.images;
        if output.is_null() {
            return unsafe { images(self.native, count, ptr::null_mut()) };
        }
        // FidelityFX FrameInterpolationSwapChainVK::getSwapchainImages leaves an
        // oversized capacity unchanged after filling its smaller image set. Query
        // the real count first so callers never consume uninitialized tail handles.
        let capacity = unsafe { count.read() };
        let mut available = 0;
        let result = unsafe { images(self.native, &mut available, ptr::null_mut()) };
        if result != VK_SUCCESS {
            unsafe { count.write(0) };
            return result;
        }
        unsafe { count.write(capacity.min(available)) };
        let filled = unsafe { images(self.native, count, output) };
        let written = unsafe { count.read() };
        if written > capacity || written > available {
            unsafe { count.write(0) };
            return VK_INITIALIZATION_FAILED;
        }
        if filled == VK_SUCCESS && capacity < available {
            VK_INCOMPLETE
        } else {
            filled
        }
    }

    fn prepare(&mut self, commands: Handle, frame: &Frame) -> i32 {
        let Some(record) = self.valid_record(frame) else {
            return INVALID;
        };
        let ops = self.ops;
        let _timing = Timing::new(ops, c"ffx_prepare_total", record.frame_id);
        if !self.enabled && !self.wants(record.frame_id) {
            return OK;
        }
        if !self.guides_valid(frame, record) {
            return INVALID;
        }
        let slot = &mut self.slots[slot_index(record.frame_id)];
        if rsf_core_fgvk_can_prepare(&slot.state) == 0 {
            return NOT_READY;
        }
        unsafe { (ops.fence_timing)(self.native, record.frame_id) };
        let fences = rsf_core_vkfg_prepare_fences(
            ops.resources,
            self.resources,
            &mut slot.state,
            &mut slot.fences,
        );
        unsafe { (ops.fence_timing)(self.native, 0) };
        if fences != OK {
            return fences;
        }
        rsf_core_fgvk_prepare_begin(&mut self.timeline, &mut slot.state, record, commands);
        // Dispatch can record GPU work before reporting an error. Submission still
        // owns that recording even when it never becomes an accepted FG input.
        slot.state.recorded_prepare = 1;
        if self.enabled {
            if unsafe { (ops.prepare)(self.native, commands, frame) } != OK {
                return FEATURE_FAILED;
            }
            slot.state.prepared = 1;
            self.status.prepared += 1;
        }
        rsf_core_fgvk_prepare_accepted(&mut self.timeline, &mut slot.state);
        self.capture(commands, &frame.depth, record, c"fg-depth");
        self.capture(commands, &frame.motion, record, c"fg-motion-uv");
        OK
    }

    fn hudless(&mut self, commands: Handle, frame: &Frame) -> i32 {
        let Some(record) = self.valid_record(frame) else {
            return INVALID;
        };
        if !self.enabled && !self.wants(record.frame_id) {
            return OK;
        }
        let ops = self.ops;
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
            || rsf_core_fgvk_hudless_begin(&mut slot.state, record, commands) == 0
        {
            return INVALID;
        }
        if rsf_core_vkfg_ensure_image(
            ops.resources,
            self.resources,
            &mut slot.color,
            width,
            height,
            RGBA8,
            0,
        ) == 0
        {
            return INIT_FAILED;
        }
        slot.state.recorded_hudless = 1;
        unsafe {
            (ops.copy)(
                self.native,
                commands,
                input,
                slot.color.image,
                slot.color.initialized,
            )
        };
        slot.color.initialized = 1;
        slot.state.hudless = 1;
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

    fn submitted(&mut self, commands: *const Handle, count: u32) {
        let states: [*mut Recording; SLOT_COUNT] =
            std::array::from_fn(|i| &raw mut self.slots[i].state);
        let mut plan = SubmitPlan {
            fences: 0,
            discarded_prepare: 0,
            discarded_hudless: 0,
        };
        unsafe { rsf_core_fgvk_submitted(states.as_ptr(), commands, count, &mut plan) };
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let bit = 1 << index;
            if plan.discarded_hudless & bit != 0 {
                slot.color.initialized = 0;
            }
            if plan.fences & bit != 0 {
                rsf_core_vkfg_fence_submission(
                    self.ops.resources,
                    self.resources,
                    &mut slot.state,
                    &slot.fences,
                    0,
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
            (true, true) => c"ffx_configure_update",
            (true, false) => c"ffx_configure_enable",
            (false, true) => c"ffx_configure_disable",
            (false, false) => c"ffx_configure_disabled",
        };
        let enabled = record.is_some();
        if unsafe {
            (self.ops.configure)(
                self.native,
                u32::from(enabled),
                color,
                frame,
                stage.as_ptr(),
            )
        } != OK
        {
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
        let ops = self.ops;
        let _timing = Timing::new(ops, c"ffx_present_total", source_frame);
        if chain == 0 || chain != self.chain {
            return VK_INITIALIZATION_FAILED;
        }
        let index = slot_index(source_frame);
        let identity = source.matches(&self.slots[index].state.record);
        // A camera reset discards dispatch history through the generation
        // callback, rather than disabling the presenter and draining its queues
        // and pacing threads. Keep activation stable while this frame's
        // submitted input leases are valid.
        let valid = rsf_core_fgvk_present_candidate(
            &self.timeline,
            &self.slots[index].state,
            source_frame,
            u32::from(identity),
            u32::from(self.enabled),
        ) != 0;
        let configured = self.configure(valid.then_some(index));
        if configured != OK {
            self.status.result = configured;
            return VK_INITIALIZATION_FAILED;
        }
        let result = unsafe {
            (ops.present)(
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
            rsf_core_vkfg_fence_submission(
                ops.resources,
                self.resources,
                &mut slot.state,
                &slot.fences,
                1,
            );
        }
        rsf_core_fgvk_presented(
            &mut self.timeline,
            &slot.state,
            source_frame,
            u32::from(identity),
        );
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
        self.slots
            .iter()
            .any(|slot| rsf_core_fgvk_unsubmitted(&slot.state) != 0)
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
        if unsafe { (self.ops.drain)(self.native) } != OK {
            return NOT_READY;
        }
        unsafe { (self.ops.capture_flush)(self.native) };
        OK
    }

    // Each step records its own completion, so a refused teardown can be
    // retried without repeating a release that already succeeded.
    fn retire(&mut self) -> i32 {
        if self.native.is_null() {
            // A failed open freed its own partial leaf and registered no hooks.
            self.module = None;
            self.withdraw();
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
        let (ops, native) = (self.ops, self.native);
        if self.effect {
            if unsafe { (ops.destroy_effect)(native) } != OK {
                return FEATURE_FAILED;
            }
            self.effect = false;
            let mut route = route();
            if route.matches_native(native) {
                route.source = None;
            }
        }
        if self.chain != 0 {
            unsafe { (ops.destroy_chain)(native) };
            self.chain = 0;
        }
        if self.context {
            if unsafe { (ops.destroy_swapchain)(native) } != OK {
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
            let cleared = unsafe { (ops.set_wsi)(native, 0) };
            if !matches!(cleared, OK | WSI_FOREIGN_DEVICE) {
                return FEATURE_FAILED;
            }
            self.wsi_registered = false;
        }
        for slot in &mut self.slots {
            rsf_core_vkfg_release_image(ops.resources, self.resources, &mut slot.color);
            rsf_core_vkfg_release_fences(ops.resources, self.resources, &mut slot.fences);
        }
        // Keep admission through module unload, after the WSI table no longer points at the host.
        unsafe { (ops.close)(native) };
        self.native = ptr::null_mut();
        self.module = None;
        self.withdraw();
        OK
    }

    fn withdraw(&mut self) {
        if self.admitted {
            route().release(self.id());
            self.admitted = false;
        }
    }
}

/// Scoped borrow for a native WSI hook. `match_device` is zero for the queue
/// hooks, which receive no device. Returns null without a published leaf,
/// once retirement has closed the route, or for another device. Each non-null
/// result is paired with one `rsf_core_ffxgvk_wsi_return`.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_ffxgvk_wsi_borrow(device: Handle, match_device: u32) -> Handle {
    ptr::with_exposed_provenance_mut(route().borrow(device.addr(), match_device != 0))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_ffxgvk_wsi_return() {
    route().give_back();
}

unsafe fn live<'a>(owner: *mut Owner) -> Option<&'a mut Owner> {
    unsafe { owner.as_mut() }.filter(|owner| !owner.retiring)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_create(
    ops: *const Ops,
    setup: *const Setup,
    swapchain: *const Swapchain,
    output: *mut *mut Owner,
    chain: *mut u64,
) -> i32 {
    if output.is_null() || chain.is_null() {
        return INVALID;
    }
    unsafe {
        output.write(ptr::null_mut());
        chain.write(0);
    }
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let graphics = &setup.graphics;
    if !unsafe { has_capacity(graphics) } {
        return INVALID;
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
        return INVALID;
    }
    // Four distinct queues of one family: graphics, compute, present, acquire.
    for (index, queue) in setup.queues.iter().enumerate() {
        if queue.is_null() || setup.queues[..index].contains(queue) {
            return INVALID;
        }
    }
    let Some(ops) = (unsafe { ops.as_ref() }) else {
        return NOT_COMPILED;
    };
    let Some(&swapchain) = (unsafe { swapchain.as_ref() }) else {
        return INVALID;
    };
    if !swapchain.supported() {
        return NOT_SUPPORTED;
    }
    let owner = allocate_handle(Owner::new(ops, setup, swapchain));
    if owner.is_null() {
        return INIT_FAILED;
    }
    let state = unsafe { &mut *owner };
    let result = state.initialize(setup);
    if result != OK {
        state.status.result = result;
        // A refused cleanup retains the owner for the documented destroy retry.
        if unsafe { rsf_core_ffxgvk_destroy(owner) } != OK {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_images(
    owner: *mut Owner,
    count: *mut u32,
    images: *mut u64,
) -> i32 {
    match unsafe { live(owner) } {
        Some(owner) if !count.is_null() => owner.images(count, images),
        _ => VK_INITIALIZATION_FAILED,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_acquire(
    owner: *mut Owner,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    match unsafe { live(owner) } {
        Some(owner) if !index.is_null() => unsafe {
            (owner.ops.acquire)(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_enable(owner: *mut Owner, enabled: u32) -> i32 {
    match unsafe { owner.as_mut() } {
        Some(owner) if enabled <= 1 => {
            owner.enabled = enabled != 0;
            OK
        }
        _ => INVALID,
    }
}

unsafe fn with_frame(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
    stage: fn(&mut Owner, Handle, &Frame) -> i32,
) -> i32 {
    if owner.is_null() || commands.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    match unsafe { live(owner) } {
        Some(owner) => stage(owner, commands, frame),
        None => INVALID,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_prepare(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe { with_frame(owner, commands, frame, Owner::prepare) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_hudless(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe { with_frame(owner, commands, frame, Owner::hudless) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_submitted(
    owner: *mut Owner,
    commands: *const Handle,
    count: u32,
) {
    if let Some(owner) = unsafe { live(owner) }
        && !commands.is_null()
    {
        owner.submitted(commands, count);
    }
}

#[unsafe(no_mangle)]
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

#[unsafe(no_mangle)]
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

#[unsafe(no_mangle)]
extern "C" fn rsf_core_ffxgvk_note_generated(native: Handle) {
    let mut route = route();
    if route.matches_native(native) {
        route.generated_callbacks = route.generated_callbacks.wrapping_add(1);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_status(owner: *const Owner, status: *mut Status) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(status) } {
        return INVALID;
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_drain(owner: *mut Owner) -> i32 {
    match unsafe { owner.as_mut() } {
        Some(owner) if !owner.native.is_null() => owner.drain(),
        _ => OK,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ffxgvk_destroy(owner: *mut Owner) -> i32 {
    let Some(state) = (unsafe { owner.as_mut() }) else {
        return OK;
    };
    let result = state.retire();
    if result == OK {
        drop(unsafe { Box::from_raw(owner) });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vulkan_sr_abi::rsf_vk_function;
    use std::cell::RefCell;

    const OUT_OF_DATE: i32 = -1_000_001_004;
    const NATIVE: usize = 0x40;

    #[derive(Default)]
    struct Calls {
        next: u64,
        opens: u32,
        closes: u32,
        configures: Vec<u64>,
        presents: u32,
        drains: u32,
        fence_submits: u32,
        effects_destroyed: u32,
        chains_destroyed: u32,
        swapchains_destroyed: u32,
        fail_provider: bool,
        fail_drain: bool,
        fail_effect_destroy: bool,
        fail_swapchain_destroy: bool,
        fail_open: bool,
        present_result: i32,
    }

    thread_local! { static CALLS: RefCell<Calls> = RefCell::default(); }
    // Owners share the process-wide WSI route, so these tests run one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());
    fn serial() -> MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn routed(device: usize) -> Handle {
        let native = rsf_core_ffxgvk_wsi_borrow(ptr::without_provenance_mut(device), 1);
        if !native.is_null() {
            rsf_core_ffxgvk_wsi_return();
        }
        native
    }
    fn calls<R>(f: impl FnOnce(&mut Calls) -> R) -> R {
        CALLS.with(|calls| f(&mut calls.borrow_mut()))
    }
    fn fail(failed: bool) -> i32 {
        if failed { FEATURE_FAILED } else { OK }
    }

    unsafe extern "C" fn open(
        _: *const Setup,
        _: Handle,
        _: u32,
        _: u32,
        out: *mut Handle,
        user: *mut Handle,
    ) -> i32 {
        // Admission precedes the native leaf, and nothing is routed yet.
        assert!(route().owner != 0 && route().native == 0);
        if calls(|c| {
            c.opens += 1;
            c.fail_open
        }) {
            return INIT_FAILED;
        }
        unsafe {
            out.write(ptr::without_provenance_mut(NATIVE));
            user.write(ptr::without_provenance_mut(NATIVE + 1));
        }
        OK
    }
    unsafe extern "C" fn load(_: *const u16) -> Handle {
        ptr::without_provenance_mut(NATIVE + 2)
    }
    unsafe extern "C" fn unload(_: Handle) {}
    unsafe extern "C" fn close(native: Handle) {
        assert_eq!(native.addr(), NATIVE);
        // The hooks are already refused, and admission outlives the native
        // close so no successor can register while it unloads.
        assert!(routed(DEVICE).is_null() && route().owner != 0 && route().closing);
        calls(|c| c.closes += 1);
    }
    unsafe extern "C" fn set_wsi(_: Handle, _: u32) -> i32 {
        OK
    }
    unsafe extern "C" fn create_swapchain(_: Handle, chain: *mut u64, context: *mut u32) -> i32 {
        unsafe {
            chain.write(3);
            context.write(1);
        }
        OK
    }
    unsafe extern "C" fn create_effect(_: Handle, context: *mut u32) -> i32 {
        unsafe { context.write(1) };
        OK
    }
    unsafe extern "C" fn step(_: Handle) -> i32 {
        OK
    }
    unsafe extern "C" fn query_provider(_: Handle) -> i32 {
        if calls(|c| c.fail_provider) {
            INIT_FAILED
        } else {
            OK
        }
    }
    unsafe extern "C" fn destroy_effect(_: Handle) -> i32 {
        calls(|c| {
            c.effects_destroyed += u32::from(!c.fail_effect_destroy);
            fail(c.fail_effect_destroy)
        })
    }
    unsafe extern "C" fn destroy_chain(_: Handle) {
        calls(|c| c.chains_destroyed += 1);
    }
    unsafe extern "C" fn destroy_swapchain(_: Handle) -> i32 {
        calls(|c| {
            c.swapchains_destroyed += u32::from(!c.fail_swapchain_destroy);
            fail(c.fail_swapchain_destroy)
        })
    }
    unsafe extern "C" fn images(_: Handle, count: *mut u32, _: *mut u64) -> i32 {
        unsafe { count.write(3) };
        OK
    }
    unsafe extern "C" fn acquire(_: Handle, _: u64, _: u64, _: u64, _: *mut u32, _: u64) -> i32 {
        OK
    }
    unsafe extern "C" fn prepare(_: Handle, _: Handle, _: *const Frame) -> i32 {
        OK
    }
    unsafe extern "C" fn copy(_: Handle, _: Handle, _: *const Image, _: u64, _: u32) {}
    unsafe extern "C" fn configure(
        _: Handle,
        enabled: u32,
        _: u64,
        frame: u64,
        _: *const c_char,
    ) -> i32 {
        calls(|c| c.configures.push(if enabled == 0 { 0 } else { frame }));
        OK
    }
    unsafe extern "C" fn present(_: Handle, _: *const c_void, _: u64, count: *mut u64) -> i32 {
        calls(|c| {
            c.presents += 1;
            unsafe { count.write(u64::from(c.presents)) };
            c.present_result
        })
    }
    unsafe extern "C" fn drain(_: Handle) -> i32 {
        calls(|c| {
            c.drains += 1;
            if c.fail_drain { NOT_READY } else { OK }
        })
    }
    unsafe extern "C" fn wants(_: Handle, _: u64) -> u32 {
        0
    }
    unsafe extern "C" fn record(
        _: Handle,
        _: Handle,
        _: *const Image,
        _: *const Record,
        _: *const c_char,
    ) {
    }
    unsafe extern "C" fn flush(_: Handle) {}
    unsafe extern "C" fn fence_timing(_: Handle, _: u64) {}
    unsafe extern "C" fn timing_begin() -> i64 {
        0
    }
    unsafe extern "C" fn timing_end(_: *const c_char, _: u64, _: i64) {}

    unsafe extern "C" fn local_types(_: Handle) -> u32 {
        1
    }
    unsafe extern "C" fn create_handle(
        _: Handle,
        _: u32,
        _: u32,
        _: u32,
        _: u32,
        out: *mut u64,
    ) -> i32 {
        unsafe {
            out.write(calls(|c| {
                c.next += 1;
                c.next
            }))
        };
        OK
    }
    unsafe extern "C" fn requirements(_: Handle, _: u64, size: *mut u64, types: *mut u32) {
        unsafe {
            size.write(4096);
            types.write(1);
        }
    }
    unsafe extern "C" fn allocate(_: Handle, _: u64, _: u32, out: *mut u64) -> i32 {
        unsafe { create_handle(ptr::null_mut(), 0, 0, 0, 0, out) }
    }
    unsafe extern "C" fn pair(_: Handle, _: u64, _: u64) -> i32 {
        OK
    }
    unsafe extern "C" fn release(_: Handle, _: u64) {}
    unsafe extern "C" fn fence(_: Handle, out: *mut u64) -> i32 {
        unsafe { allocate(ptr::null_mut(), 0, 0, out) }
    }
    unsafe extern "C" fn fence_ok(_: Handle, _: u64) -> i32 {
        OK
    }
    unsafe extern "C" fn submit_fence(_: Handle, _: u64) -> i32 {
        calls(|c| c.fence_submits += 1);
        OK
    }

    static RESOURCES: Resources = Resources {
        local_memory_types: Some(local_types),
        create_image: Some(create_handle),
        requirements: Some(requirements),
        allocate: Some(allocate),
        bind: Some(pair),
        create_view: None,
        destroy_view: Some(release),
        destroy_image: Some(release),
        free_memory: Some(release),
        create_fence: Some(fence),
        destroy_fence: Some(release),
        wait_fence: Some(fence_ok),
        reset_fence: Some(fence_ok),
        submit_fence: Some(submit_fence),
    };
    static OPS: Ops = Ops {
        loader: Loader { load, unload },
        open,
        close,
        set_wsi,
        create_swapchain,
        query_swapchain: step,
        create_effect,
        query_provider,
        destroy_effect,
        destroy_chain,
        destroy_swapchain,
        images,
        acquire,
        prepare,
        copy,
        configure,
        present,
        drain,
        capture_wants: wants,
        capture_record: record,
        capture_flush: flush,
        fence_timing,
        timing_begin,
        timing_end,
        resources: &RESOURCES,
    };

    unsafe extern "C" fn proc_address(_: *mut c_void, _: *const c_char) -> rsf_vk_function {
        None
    }
    unsafe extern "C" fn submit(_: *mut c_void, _: u32, _: *const c_void, _: u64) -> i32 {
        OK
    }
    unsafe extern "C" fn queue(_: *mut c_void) {}

    const DEVICE: usize = 1;
    const SWAPCHAIN: Swapchain = Swapchain {
        width: 640,
        height: 360,
        format: BGRA8,
        color_space: SRGB_NONLINEAR,
        layers: 1,
        structure_valid: 1,
        old_chain: 0,
    };

    fn setup() -> Setup {
        let mut setup: Setup = unsafe { std::mem::zeroed() };
        setup.struct_size = size_of::<Setup>() as u32;
        setup.abi_version = RSF_FGVK_ABI_VERSION;
        let graphics = &mut setup.graphics;
        graphics.struct_size = size_of_val(graphics) as u32;
        graphics.abi_version = RSF_SRVK_ABI_VERSION;
        graphics.instance = ptr::without_provenance_mut(7);
        graphics.physical_device = ptr::without_provenance_mut(8);
        graphics.device = ptr::without_provenance_mut(DEVICE);
        graphics.instance_proc = Some(proc_address);
        graphics.device_proc = Some(proc_address);
        graphics.runtime_directory_utf8 = c"C:\\fake".as_ptr();
        setup.queues = std::array::from_fn(|i| ptr::without_provenance_mut(20 + i));
        setup.create_info = ptr::without_provenance(9);
        setup.graphics_submit = Some(submit);
        setup.lock_queues = Some(queue);
        setup.unlock_queues = Some(queue);
        setup
    }

    fn create(setup: &Setup, swapchain: &Swapchain) -> (i32, *mut Owner, u64) {
        let (mut owner, mut chain) = (ptr::null_mut(), 0);
        let result =
            unsafe { rsf_core_ffxgvk_create(&OPS, setup, swapchain, &mut owner, &mut chain) };
        (result, owner, chain)
    }

    fn source(frame: u64) -> Record {
        let mut record: Record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<Record>() as u32;
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        (
            record.session_id,
            record.frame_id,
            record.resource_generation,
        ) = (7, frame, 1);
        (record.render_width, record.render_height) = (640, 360);
        (record.output_width, record.output_height) = (640, 360);
        record.frame_time_ms = 16.0;
        let camera = &mut record.camera;
        camera.struct_size = size_of_val(camera) as u32;
        camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        (camera.render_width, camera.render_height) = (640, 360);
        (camera.output_width, camera.output_height) = (640, 360);
        (
            camera.near_plane,
            camera.far_plane,
            camera.vertical_fov_radians,
        ) = (0.1, 100.0, 1.0);
        record
    }

    fn frame(record: &Record) -> Frame {
        let image = |format| Image {
            device: ptr::without_provenance_mut(DEVICE),
            image: 12,
            format,
            width: 640,
            height: 360,
            layout: SHADER_READ,
            copy_source: 1,
        };
        Frame {
            struct_size: size_of::<Frame>() as u32,
            abi_version: RSF_FGVK_ABI_VERSION,
            record,
            depth: image(R32F),
            motion: image(RG16F),
            hudless: image(RGBA8),
            motion_scale_x: 640.0,
            motion_scale_y: 360.0,
        }
    }

    fn command(value: usize) -> Handle {
        ptr::without_provenance_mut(value)
    }

    fn present_info() -> *const c_void {
        ptr::without_provenance(0x50)
    }

    #[test]
    fn admission_refuses_before_opening_the_native_leaf() {
        let mut duplicate = setup();
        duplicate.queues[3] = duplicate.queues[0];
        assert_eq!(create(&duplicate, &SWAPCHAIN).0, INVALID);
        let mut mismatch = setup();
        mismatch.abi_version += 1;
        assert_eq!(create(&mismatch, &SWAPCHAIN).0, ABI_MISMATCH);
        let layered = Swapchain {
            layers: 2,
            ..SWAPCHAIN
        };
        assert_eq!(create(&setup(), &layered).0, NOT_SUPPORTED);
        let recreated = Swapchain {
            old_chain: 5,
            ..SWAPCHAIN
        };
        assert_eq!(create(&setup(), &recreated).0, NOT_SUPPORTED);
        let (mut owner, mut chain) = (ptr::null_mut(), 0);
        let disabled = unsafe {
            rsf_core_ffxgvk_create(ptr::null(), &setup(), &SWAPCHAIN, &mut owner, &mut chain)
        };
        assert_eq!((disabled, owner.is_null(), chain), (NOT_COMPILED, true, 0));
        assert_eq!(calls(|c| c.opens), 0);
    }

    #[test]
    fn refused_cleanup_after_failed_creation_retains_the_owner() {
        let _serial = serial();
        calls(|c| (c.fail_provider, c.fail_swapchain_destroy) = (true, true));
        let (result, owner, chain) = create(&setup(), &SWAPCHAIN);
        assert_eq!((result, chain), (INIT_FAILED, 0));
        assert!(!owner.is_null());
        // The owner was never returned, so there is nothing to disable or drain.
        assert!(calls(|c| c.configures.is_empty() && c.drains == 0));
        assert_eq!(
            calls(|c| (c.effects_destroyed, c.chains_destroyed, c.closes)),
            (1, 1, 0)
        );
        assert_eq!(unsafe { rsf_core_ffxgvk_drain(owner) }, OK);
        assert_eq!(calls(|c| c.drains), 0);
        calls(|c| c.fail_swapchain_destroy = false);
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(owner) }, OK);
        assert_eq!(
            calls(|c| (
                c.effects_destroyed,
                c.chains_destroyed,
                c.swapchains_destroyed,
                c.closes
            )),
            (1, 1, 1, 1)
        );
    }

    #[test]
    fn configured_source_keeps_its_consumer_fence_when_present_reports_an_error() {
        let _serial = serial();
        let (result, owner, chain) = create(&setup(), &SWAPCHAIN);
        assert_eq!((result, chain), (OK, 3));
        let record = source(1);
        let input = frame(&record);
        let list = command(10);
        unsafe {
            assert_eq!(rsf_core_ffxgvk_enable(owner, 1), OK);
            assert_eq!(rsf_core_ffxgvk_prepare(owner, list, &input), OK);
            assert_eq!(rsf_core_ffxgvk_hudless(owner, list, &input), OK);
            rsf_core_ffxgvk_submitted(owner, &list, 1);
        }
        assert_eq!(calls(|c| c.fence_submits), 1);
        assert_eq!(
            unsafe { rsf_core_ffxgvk_present(owner, present_info(), 4, ptr::null(), 0) },
            VK_INITIALIZATION_FAILED
        );
        calls(|c| c.present_result = OUT_OF_DATE);
        assert_eq!(
            unsafe { rsf_core_ffxgvk_present(owner, present_info(), 3, ptr::null(), 0) },
            OUT_OF_DATE
        );
        let mut status: Status = unsafe { std::mem::zeroed() };
        (status.struct_size, status.abi_version) =
            (size_of::<Status>() as u32, RSF_FGVK_ABI_VERSION);
        assert_eq!(unsafe { rsf_core_ffxgvk_status(owner, &mut status) }, OK);
        assert_eq!(
            (status.enabled, status.result, status.source_frame),
            (1, FEATURE_FAILED, 1)
        );
        assert_eq!(
            calls(|c| (c.fence_submits, c.configures.clone())),
            (2, vec![1])
        );
        // The recording was consumed and cannot be generated again.
        calls(|c| c.present_result = OK);
        assert_eq!(
            unsafe { rsf_core_ffxgvk_present(owner, present_info(), 3, ptr::null(), 0) },
            OK
        );
        assert_eq!(
            calls(|c| (c.fence_submits, c.configures.clone())),
            (2, vec![1, 0])
        );
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(owner) }, OK);
    }

    #[test]
    fn teardown_requires_submission_and_drain_and_retries_without_repeating() {
        let _serial = serial();
        let (_, owner, _) = create(&setup(), &SWAPCHAIN);
        let record = source(2);
        let input = frame(&record);
        let list = command(11);
        unsafe {
            assert_eq!(rsf_core_ffxgvk_enable(owner, 1), OK);
            assert_eq!(rsf_core_ffxgvk_prepare(owner, list, &input), OK);
            assert_eq!(rsf_core_ffxgvk_destroy(owner), NOT_READY);
        }
        assert_eq!(calls(|c| (c.drains, c.effects_destroyed)), (0, 0));
        unsafe { rsf_core_ffxgvk_submitted(owner, &list, 1) };
        calls(|c| c.fail_drain = true);
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(owner) }, NOT_READY);
        assert_eq!(calls(|c| (c.drains, c.effects_destroyed)), (1, 0));
        calls(|c| (c.fail_drain, c.fail_effect_destroy) = (false, true));
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(owner) }, FEATURE_FAILED);
        assert_eq!(unsafe { rsf_core_ffxgvk_drain(owner) }, OK);
        assert_eq!(calls(|c| c.drains), 2);
        let mut count = 0;
        unsafe {
            assert_eq!(
                rsf_core_ffxgvk_present(owner, present_info(), 3, ptr::null(), 0),
                VK_INITIALIZATION_FAILED
            );
            assert_eq!(
                rsf_core_ffxgvk_images(owner, &mut count, ptr::null_mut()),
                VK_INITIALIZATION_FAILED
            );
            assert_eq!(rsf_core_ffxgvk_prepare(owner, list, &input), INVALID);
        }
        calls(|c| c.fail_effect_destroy = false);
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(owner) }, OK);
        assert_eq!(
            calls(|c| (
                c.drains,
                c.effects_destroyed,
                c.chains_destroyed,
                c.swapchains_destroyed,
                c.closes
            )),
            (2, 1, 1, 1, 1)
        );
    }

    #[test]
    fn one_owner_holds_the_wsi_route_and_a_borrow_retains_its_leaf() {
        let _serial = serial();
        let (result, first, _) = create(&setup(), &SWAPCHAIN);
        assert_eq!(result, OK);
        assert_eq!(routed(DEVICE).addr(), NATIVE);
        assert!(
            routed(DEVICE + 1).is_null(),
            "a foreign device is not routed"
        );
        // A second owner is refused before its native leaf opens.
        let (second, owner, chain) = create(&setup(), &SWAPCHAIN);
        assert_eq!((second, owner.is_null(), chain), (NOT_SUPPORTED, true, 0));
        assert_eq!(calls(|c| c.opens), 1);

        // A queue hook (no device) is inside its callback while the host
        // destroys the owner: every SDK context goes, but the leaf stays.
        let hook = rsf_core_ffxgvk_wsi_borrow(ptr::null_mut(), 0);
        assert_eq!(hook.addr(), NATIVE);
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(first) }, NOT_READY);
        assert_eq!(
            calls(|c| (c.effects_destroyed, c.swapchains_destroyed, c.closes)),
            (1, 1, 0)
        );
        assert!(routed(DEVICE).is_null(), "a closing route lends no leaf");
        assert!(rsf_core_ffxgvk_wsi_borrow(ptr::null_mut(), 0).is_null());
        assert_eq!(create(&setup(), &SWAPCHAIN).0, NOT_SUPPORTED);
        rsf_core_ffxgvk_wsi_return();

        // The retry repeats no SDK release and closes the leaf once.
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(first) }, OK);
        assert_eq!(
            calls(|c| (
                c.effects_destroyed,
                c.chains_destroyed,
                c.swapchains_destroyed,
                c.closes
            )),
            (1, 1, 1, 1)
        );
        assert!(routed(DEVICE).is_null());
        let (next, successor, _) = create(&setup(), &SWAPCHAIN);
        assert_eq!(next, OK);
        assert_eq!(routed(DEVICE).addr(), NATIVE);
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(successor) }, OK);
    }

    #[test]
    fn failed_open_releases_the_wsi_route() {
        let _serial = serial();
        calls(|c| c.fail_open = true);
        let (result, owner, _) = create(&setup(), &SWAPCHAIN);
        assert_eq!((result, owner.is_null()), (INIT_FAILED, true));
        assert_eq!(calls(|c| (c.opens, c.closes)), (1, 0));
        assert_eq!(route().owner, 0);
        calls(|c| c.fail_open = false);
        let (result, owner, _) = create(&setup(), &SWAPCHAIN);
        assert_eq!(result, OK);
        assert_eq!(unsafe { rsf_core_ffxgvk_destroy(owner) }, OK);
        assert_eq!(route().owner, 0);
    }
}
