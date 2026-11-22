// SPDX-License-Identifier: MIT
use crate::sr_session::has_capacity;
use crate::vulkan_dlss_fg_abi::{ChainSupport, Handle, Platform, Sample, SdkCall, Swapchain};
use crate::vulkan_fg_abi::{
    RSF_FGVK_ABI_VERSION, RSF_FGVK_FRAME_SLOTS, rsf_dlssgvk_status as Status,
    rsf_fgvk_frame as Frame, rsf_fgvk_frame_slot as Recording, rsf_fgvk_setup as Setup,
    rsf_fgvk_status as Presentation, rsf_fgvk_timeline as Timeline,
};
use crate::vulkan_fg_copy::{SHADER_READ, TRANSFER_SRC, record_shader_read_copy};
use crate::vulkan_fg_frames::*;
use crate::vulkan_fg_resources::{
    rsf_core_vkfg_ensure_image, rsf_core_vkfg_fence_submission, rsf_core_vkfg_prepare_fences,
    rsf_core_vkfg_release_fences, rsf_core_vkfg_release_image,
};
use crate::vulkan_fg_resources_abi::{
    rsf_vkfg_fences as Fences, rsf_vkfg_image as Image, rsf_vkfg_platform as Gpu,
};
use crate::vulkan_latency_abi::{rsf_slvk_call, rsf_slvk_callback};
use crate::vulkan_ownership::Claim;
use crate::vulkan_sr_abi::{RSF_SRVK_ABI_VERSION, rsf_srvk_image as SourceImage};
use rsf_game_sys::{RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record as Record};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

const RSF_DLSSGVK_ABI_VERSION: u32 = 1;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_COMPILED: i32 = -3;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;

const WSI_ERROR: i32 = -3;
const SUBOPTIMAL: i32 = 1000001003;
const USAGE_TRANSFER_SRC: u32 = 1;
const USAGE_TRANSFER_DST: u32 = 2;
const R32F: u32 = 100;
const RG16F: u32 = 83;
const RGBA8: u32 = 37;
const BGRA8: u32 = 44;
const SLOTS: usize = RSF_FGVK_FRAME_SLOTS as usize;

static OCCUPIED: AtomicBool = AtomicBool::new(false);
// Streamline's API error callback supplies neither a viewport nor a frame.
// The singleton owner consumes the latest code at its Present boundary.
static API_ERROR: AtomicI32 = AtomicI32::new(0);

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlgvk_report_api_error(error: i32) {
    API_ERROR.store(error, Ordering::SeqCst);
}

fn take_api_error() -> i32 {
    API_ERROR.swap(0, Ordering::SeqCst)
}

#[derive(Default)]
enum Inputs {
    #[default]
    Retired,
    Pending {
        semaphore: u64,
        value: u64,
    },
    Unknown,
}

impl Inputs {
    fn retire(&mut self, platform: &Platform, native: Handle) -> bool {
        match *self {
            Self::Retired => return true,
            Self::Unknown => return false,
            Self::Pending { semaphore, value } => {
                if unsafe { (platform.wait_inputs)(native, semaphore, value) } != OK {
                    return false;
                }
            }
        }
        *self = Self::Retired;
        true
    }
}

struct Slot {
    frame: Recording,
    images: [Image; 3],
    fences: Fences,
    inputs: Inputs,
}

impl Slot {
    fn new() -> Self {
        Self {
            // The ABI ledger starts with no records, pointer leases or pending work.
            frame: unsafe { std::mem::zeroed() },
            images: [Image::default(); 3],
            fences: Fences::default(),
            inputs: Inputs::Retired,
        }
    }

    fn group(&self, hudless: bool) -> u64 {
        self.frame.recording_sequence * 2 + u64::from(hudless)
    }

    fn capture_retired(&self, platform: &Platform, native: Handle) {
        for (submitted, hudless) in [
            (self.frame.submitted_prepare, false),
            (self.frame.submitted_hudless, true),
        ] {
            if submitted != 0 {
                unsafe { (platform.capture_group)(native, self.group(hudless), 1) };
            }
        }
    }
}

struct Owner {
    _claim: Claim,
    platform: &'static Platform,
    gpu_platform: &'static Gpu,
    native: Handle,
    gpu: Handle,
    chain: u64,
    backbuffers: u32,
    setup: Setup,
    timeline: Timeline,
    slots: [Slot; SLOTS],
    status: Status,
    requested: bool,
    configured: bool,
    resources: bool,
}

impl Drop for Owner {
    fn drop(&mut self) {
        if !self.gpu.is_null() {
            for slot in &mut self.slots {
                for image in &mut slot.images {
                    rsf_core_vkfg_release_image(self.gpu_platform, self.gpu, image);
                }
                rsf_core_vkfg_release_fences(self.gpu_platform, self.gpu, &mut slot.fences);
            }
        }
        if self.chain != 0 {
            unsafe { (self.platform.destroy_chain)(self.native, self.chain) };
        }
        unsafe { (self.platform.close)(self.native) };
    }
}

fn invoke(
    platform: &Platform,
    callback: rsf_slvk_callback,
    user: Handle,
    source: Option<&Record>,
) -> i32 {
    let call = rsf_slvk_call::for_record(source, callback, user);
    // The Streamline worker invokes the callback synchronously with the exact token.
    unsafe { (platform.invoke)(&call) }
}

struct RecordWork<'a> {
    platform: &'a Platform,
    sdk: SdkCall,
    images: &'a mut [Image],
    sources: &'a [SourceImage],
    first_role: u32,
    group: u64,
    staged: bool,
}

unsafe extern "C" fn record_inputs(module: Handle, token: usize, user: Handle) -> i32 {
    let work = unsafe { &mut *user.cast::<RecordWork<'_>>() };
    if token == 0 {
        return NOT_READY;
    }
    let sdk = &mut work.sdk;
    if work.first_role == 0 {
        let result =
            unsafe { (work.platform.constants.unwrap())(module, token, ptr::from_mut(sdk).cast()) };
        if result != OK {
            return result;
        }
    }
    for (source, image) in work.sources.iter().zip(work.images.iter_mut()) {
        unsafe {
            record_shader_read_copy(
                work.platform.barrier,
                work.platform.copy,
                sdk.native,
                sdk.commands,
                source.image,
                image,
                [source.width, source.height],
            )
        };
        image.initialized = 1;
    }
    work.staged = true;
    sdk.images = work.images.as_ptr();
    sdk.count = work.images.len() as u32;
    let result = unsafe { (work.platform.tag.unwrap())(module, token, ptr::from_mut(sdk).cast()) };
    if result == OK {
        for (index, source) in work.sources.iter().enumerate() {
            unsafe {
                (work.platform.capture)(
                    sdk.native,
                    sdk.commands,
                    source,
                    sdk.record,
                    work.first_role + index as u32,
                    work.group,
                )
            };
        }
    }
    result
}

impl Owner {
    fn create_chain(&mut self) -> i32 {
        let mut support = ChainSupport::default();
        let prepared =
            unsafe { (self.platform.prepare_chain)(self.native, &mut support, &mut self.gpu) };
        if prepared != OK {
            return prepared;
        }
        if self.gpu.is_null() {
            return INIT_FAILED;
        }
        // Generated frames need a WSI copy destination. Capture also needs a
        // copy source. Keep source usage when available for capture tooling.
        let required = USAGE_TRANSFER_DST
            | if support.capture_enabled != 0 {
                USAGE_TRANSFER_SRC
            } else {
                0
            };
        if support.usage & required != required {
            return NOT_SUPPORTED;
        }
        let usage = required | (support.usage & USAGE_TRANSFER_SRC);
        let mut chain = 0;
        let result = unsafe { (self.platform.create_chain)(self.native, usage, &mut chain) };
        if result != OK || chain == 0 {
            return INIT_FAILED;
        }
        // From here on a refused enumeration or capture registration rolls back the chain.
        self.chain = chain;
        let enumerated = unsafe {
            (self.platform.images)(self.native, chain, &mut self.backbuffers, ptr::null_mut())
        };
        if enumerated != OK || self.backbuffers == 0 {
            return INIT_FAILED;
        }
        unsafe { (self.platform.start_capture)(self.native) }
    }

    fn sdk(
        &mut self,
        callback: rsf_slvk_callback,
        count: u32,
        source: Option<&Record>,
    ) -> (i32, Sample) {
        let mut call = SdkCall {
            native: self.native,
            result: self.status.sdk_result,
            count,
            backbuffers: self.backbuffers,
            ..Default::default()
        };
        let result = invoke(
            self.platform,
            callback,
            ptr::from_mut(&mut call).cast(),
            source,
        );
        self.status.sdk_result = call.result;
        (result, call.sample)
    }

    fn configure(&mut self, enable: bool, source: Option<&Record>) -> i32 {
        let count = if enable {
            self.status.requested_generated_frames
        } else {
            0
        };
        let (result, _) = self.sdk(self.platform.configure, count, source);
        if result == OK {
            self.configured = enable;
            self.resources |= enable;
        }
        result
    }

    fn sample(&mut self) -> (i32, Sample) {
        let (result, sample) = self.sdk(self.platform.sample, 0, None);
        if result == OK {
            self.status.presentation.last_present_count += u64::from(sample.actual_presents);
            self.status.max_generated_frames = sample.max_generated;
            self.status.sdk_status = sample.status;
            self.status.min_dimension = sample.min_dimension;
            self.status.vsync_supported = sample.vsync;
            self.status.dynamic_mfg_supported = sample.dynamic_mfg;
        }
        (result, sample)
    }

    fn valid_record(&self, record: &Record) -> bool {
        let graphics = &self.setup.graphics;
        valid_source_record(
            record,
            graphics.output_width,
            graphics.output_height,
            graphics.depth_infinite != 0,
        ) && record.camera.depth_inverted == graphics.depth_inverted
            && record.camera.motion_jittered == graphics.motion_jittered
    }

    fn valid_image(&self, image: &SourceImage, width: u32, height: u32, format: u32) -> bool {
        image.device == self.setup.graphics.device
            && image.image != 0
            && image.copy_source != 0
            && image.width == width
            && image.height == height
            && image.format == format
            && image.layout == SHADER_READ
    }

    fn record(&mut self, commands: Handle, input: &Frame, record: &Record, hudless: bool) -> i32 {
        if !self.valid_record(record) {
            return INVALID;
        }
        if !self.requested {
            return OK;
        }
        let width = self.setup.graphics.output_width;
        let height = self.setup.graphics.output_height;
        let index = slot_index(record.frame_id);
        if hudless {
            if !self.valid_image(&input.hudless, width, height, RGBA8)
                || rsf_core_fgvk_hudless_begin(&mut self.slots[index].frame, record, commands) == 0
            {
                return INVALID;
            }
        } else {
            let rw = record.render_width;
            let rh = record.render_height;
            if !self.valid_image(&input.depth, rw, rh, R32F)
                || !self.valid_image(&input.motion, rw, rh, RG16F)
                || [input.motion_scale_x, input.motion_scale_y]
                    .iter()
                    .any(|x| !x.is_finite() || *x == 0.0)
            {
                return INVALID;
            }
            let slot = &mut self.slots[index];
            if rsf_core_fgvk_can_prepare(&slot.frame) == 0
                || !slot.inputs.retire(self.platform, self.native)
            {
                return NOT_READY;
            }
            let result = rsf_core_vkfg_prepare_fences(
                self.gpu_platform,
                self.gpu,
                &mut slot.frame,
                &mut slot.fences,
            );
            if result != OK {
                return result;
            }
            slot.capture_retired(self.platform, self.native);
            for (image, (w, h, format)) in slot.images.iter_mut().zip([
                (rw, rh, R32F),
                (rw, rh, RG16F),
                (width, height, RGBA8),
            ]) {
                if rsf_core_vkfg_ensure_image(self.gpu_platform, self.gpu, image, w, h, format, 1)
                    == 0
                {
                    return INIT_FAILED;
                }
            }
            rsf_core_fgvk_prepare_begin(&mut self.timeline, &mut slot.frame, record, commands);
        }

        let slot = &mut self.slots[index];
        let group = slot.group(hudless);
        let guides = [input.depth, input.motion];
        let (images, sources, first_role) = if hudless {
            (
                &mut slot.images[2..],
                std::slice::from_ref(&input.hudless),
                2,
            )
        } else {
            (&mut slot.images[..2], guides.as_slice(), 0)
        };
        let mut work = RecordWork {
            platform: self.platform,
            sdk: SdkCall {
                native: self.native,
                result: self.status.sdk_result,
                commands,
                input,
                record: &slot.frame.record,
                ..Default::default()
            },
            images,
            sources,
            first_role,
            group,
            staged: false,
        };
        let result = invoke(
            self.platform,
            Some(record_inputs),
            ptr::from_mut(&mut work).cast(),
            Some(record),
        );
        self.status.sdk_result = work.sdk.result;
        if hudless {
            slot.frame.recorded_hudless = u32::from(work.staged);
            if result == OK {
                slot.frame.hudless = 1;
                self.status.presentation.tagged += 1;
            }
        } else {
            slot.frame.recorded_prepare = u32::from(work.staged);
            if result == OK {
                slot.frame.prepared = 1;
                rsf_core_fgvk_prepare_accepted(&mut self.timeline, &mut slot.frame);
                self.status.presentation.prepared += 1;
                unsafe {
                    (self.platform.remember)(
                        self.native,
                        &slot.frame.record,
                        &self.status,
                        u32::from(self.configured),
                    )
                };
            }
        }
        self.status.presentation.result = result;
        result
    }

    fn submitted(&mut self, commands: &[*mut c_void]) {
        let plan = submission_plan(self.slots.each_mut().map(|slot| &mut slot.frame), commands);
        for (i, slot) in self.slots.iter_mut().enumerate() {
            let bit = 1 << i;
            for (discarded, hudless, images) in [
                (plan.discarded_prepare, false, 0..2),
                (plan.discarded_hudless, true, 2..3),
            ] {
                if discarded & bit != 0 {
                    for image in &mut slot.images[images] {
                        image.initialized = 0;
                    }
                    unsafe { (self.platform.capture_group)(self.native, slot.group(hudless), 0) };
                }
            }
            if plan.fences & bit != 0 {
                rsf_core_vkfg_fence_submission(
                    self.gpu_platform,
                    self.gpu,
                    &mut slot.frame,
                    &slot.fences,
                    0,
                );
            }
        }
    }

    fn present(&mut self, info: *const c_void, source: PresentSource) -> i32 {
        if info.is_null() || unsafe { (self.platform.valid_present)(self.chain, info) } == 0 {
            return WSI_ERROR;
        }
        let id = source.frame(self.timeline.last_prepared.frame_id);
        let index = slot_index(id);
        let stored = self.slots[index].frame.record;
        let identity = u32::from(source.matches(&stored));
        let candidate = rsf_core_fgvk_present_candidate(
            &self.timeline,
            &self.slots[index].frame,
            id,
            identity,
            u32::from(self.requested && source.allows_fg()),
        ) != 0;
        let configuration = self.configure(candidate, candidate.then_some(&stored));
        let configuration_sdk = self.status.sdk_result;
        let active = candidate && configuration == OK;
        if configuration != OK {
            self.status.presentation.result = configuration;
            self.status.presentation.enabled = 0;
            self.status.presentation.source_frame = 0;
            let off = self.configure(false, None);
            self.status.sdk_result = configuration_sdk;
            if off != OK {
                self.status.asynchronous_present_result = take_api_error();
                return WSI_ERROR;
            }
        }
        if active {
            unsafe { (self.platform.remember)(self.native, &stored, &self.status, 1) };
        }
        // The SDK proxy may wait on its presenter. The actual WSI calls take
        // the broker inside the proxy, without an outer host queue lock.
        let result = unsafe { (self.platform.present)(self.native, info) };
        let accepted = matches!(result, OK | SUBOPTIMAL);
        if active && accepted {
            let slot = &mut self.slots[index];
            rsf_core_vkfg_fence_submission(
                self.gpu_platform,
                self.gpu,
                &mut slot.frame,
                &slot.fences,
                1,
            );
        }
        let (state, sample) = self.sample();
        let slot = &mut self.slots[index];
        if active {
            slot.inputs = if accepted && state == OK && sample.semaphore != 0 {
                Inputs::Pending {
                    semaphore: sample.semaphore,
                    value: sample.value,
                }
            } else {
                // An error at the WSI boundary does not prove the SDK skipped
                // its input work. A subsequent query could describe another Present.
                Inputs::Unknown
            };
        }
        if configuration != OK {
            self.status.sdk_result = configuration_sdk;
        }
        let error = take_api_error();
        self.status.asynchronous_present_result = error;
        rsf_core_fgvk_presented(&mut self.timeline, &slot.frame, id, identity);
        self.status.presentation.source_frame = if active { id } else { 0 };
        self.status.presentation.enabled = u32::from(
            active
                && accepted
                && state == OK
                && sample.semaphore != 0
                && slot.frame.completion_unknown == 0
                && self.status.sdk_status == 0
                && error == 0,
        );
        self.status.presentation.result =
            if !accepted || (active && self.status.sdk_status != 0) || error != 0 {
                FEATURE_FAILED
            } else if configuration != OK {
                configuration
            } else if active
                && state == OK
                && (sample.semaphore == 0 || slot.frame.completion_unknown != 0)
            {
                NOT_READY
            } else {
                state
            };
        result
    }

    fn drain(&mut self) -> i32 {
        // An unsubmitted command buffer can still reference private and SDK resources.
        if self
            .slots
            .iter()
            .any(|slot| rsf_core_fgvk_unsubmitted(&slot.frame) != 0)
        {
            return NOT_READY;
        }
        let epoch = unsafe { (self.platform.timeout_epoch)() };
        let disabled = self.configure(false, None);
        if disabled != OK {
            return disabled;
        }
        let idle = unsafe {
            let user = self.setup.queue_user;
            (self.setup.lock_queues.unwrap())(user);
            let result = (self.platform.idle)(self.native);
            (self.setup.unlock_queues.unwrap())(user);
            result
        };
        if idle != OK || epoch != unsafe { (self.platform.timeout_epoch)() } {
            return NOT_READY;
        }
        for slot in &mut self.slots {
            // Failed Present cannot provide a trustworthy input semaphore.
            // Disabling FG and reaching device idle without a queue timeout
            // establishes retirement independently of that semaphore.
            if matches!(slot.inputs, Inputs::Unknown) {
                slot.inputs = Inputs::Retired;
            }
            if !slot.inputs.retire(self.platform, self.native) {
                return NOT_READY;
            }
        }
        for slot in &mut self.slots {
            slot.capture_retired(self.platform, self.native);
            slot.frame.source_pending = 0;
            slot.frame.consumer_pending = 0;
        }
        unsafe { (self.platform.flush)(self.native) };
        OK
    }
}

fn slot_index(frame: u64) -> usize {
    (frame % SLOTS as u64) as usize
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_create(
    platform: *const Platform,
    setup: *const Setup,
    out: *mut *mut Owner,
    chain: *mut u64,
) -> i32 {
    if !out.is_null() {
        unsafe { out.write(ptr::null_mut()) };
    }
    if !chain.is_null() {
        unsafe { chain.write(0) };
    }
    if out.is_null() || chain.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    let graphics = &setup.graphics;
    if (graphics.struct_size as usize) < size_of_val(graphics) {
        return INVALID;
    }
    if setup.abi_version != RSF_FGVK_ABI_VERSION || graphics.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if graphics.instance.is_null()
        || graphics.physical_device.is_null()
        || graphics.device.is_null()
        || graphics.instance_proc.is_none()
        || graphics.device_proc.is_none()
        || setup.create_info.is_null()
        || setup.queues[0].is_null()
        || setup.graphics_submit.is_none()
        || setup.lock_queues.is_none()
        || setup.unlock_queues.is_none()
        || graphics.output_width == 0
        || graphics.output_height == 0
    {
        return INVALID;
    }
    let Some(platform) = (unsafe { platform.as_ref() }) else {
        return NOT_COMPILED;
    };
    let Some(gpu_platform) = (unsafe { platform.gpu.as_ref() }) else {
        return NOT_COMPILED;
    };
    let mut description = Swapchain::default();
    unsafe { (platform.describe)(setup.create_info, &mut description) };
    if description.has_surface == 0
        || [description.width, description.height]
            != [graphics.output_width, graphics.output_height]
        || !matches!(description.format, RGBA8 | BGRA8)
        || description.layers != 1
    {
        return INVALID;
    }
    let Some(claim) = Claim::acquire(&OCCUPIED) else {
        return NOT_READY;
    };
    let mut native = ptr::null_mut();
    let opened = unsafe { (platform.open)(setup, &mut native) };
    if opened != OK {
        return opened;
    }
    if native.is_null() {
        return INIT_FAILED;
    }
    let mut owner = Box::new(Owner {
        _claim: claim,
        platform,
        gpu_platform,
        native,
        gpu: ptr::null_mut(),
        chain: 0,
        backbuffers: 0,
        // The C host keeps the allocator and callback contexts alive for this owner.
        setup: unsafe { ptr::read(setup) },
        timeline: unsafe { std::mem::zeroed() },
        slots: std::array::from_fn(|_| Slot::new()),
        status: Status {
            struct_size: size_of::<Status>() as u32,
            abi_version: RSF_DLSSGVK_ABI_VERSION,
            presentation: Presentation {
                struct_size: size_of::<Presentation>() as u32,
                abi_version: RSF_FGVK_ABI_VERSION,
                ..unsafe { std::mem::zeroed() }
            },
            requested_generated_frames: 1,
            ..unsafe { std::mem::zeroed() }
        },
        requested: false,
        configured: false,
        resources: false,
    });
    let (initialized, _) = owner.sdk(platform.initialize, 0, None);
    if initialized != OK {
        return initialized;
    }
    let (result, sample) = owner.sample();
    if result != OK {
        return result;
    }
    if sample.max_generated == 0
        || graphics.output_width < sample.min_dimension
        || graphics.output_height < sample.min_dimension
    {
        return NOT_SUPPORTED;
    }
    let result = owner.create_chain();
    if result != OK {
        return result;
    }
    owner.status.presentation.available = 1;
    take_api_error();
    unsafe {
        chain.write(owner.chain);
        out.write(Box::into_raw(owner));
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_images(
    owner: *mut Owner,
    count: *mut u32,
    images: *mut u64,
) -> i32 {
    match unsafe { owner.as_ref() } {
        Some(owner) if !count.is_null() => unsafe {
            bounded_swapchain_images(count, images, |count, images| {
                (owner.platform.images)(owner.native, owner.chain, count, images)
            })
        },
        _ => WSI_ERROR,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_acquire(
    owner: *mut Owner,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return WSI_ERROR;
    };
    if index.is_null() {
        return WSI_ERROR;
    }
    unsafe { (owner.platform.acquire)(owner.native, owner.chain, timeout, semaphore, fence, index) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_image_layout(owner: *const Owner, layout: *mut u32) -> i32 {
    if owner.is_null() || layout.is_null() {
        return INVALID;
    }
    // Streamline reads its proxy images as copy sources, including in Off mode.
    unsafe { layout.write(TRANSFER_SRC) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_enable(owner: *mut Owner, enabled: u32, count: u32) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    if enabled > 1 || count == 0 {
        return INVALID;
    }
    if count > owner.status.max_generated_frames {
        return NOT_SUPPORTED;
    }
    owner.requested = enabled != 0;
    owner.status.requested_generated_frames = count;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_record(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
    hudless: u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    if commands.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if !unsafe { has_capacity(frame.record) } {
        return INVALID;
    }
    let record = unsafe { &*frame.record };
    if record.abi_version != RSF_GAME_FRAME_ABI_VERSION {
        return INVALID;
    }
    owner.record(commands, frame, record, hudless != 0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_submitted(
    owner: *mut Owner,
    commands: *const Handle,
    count: u32,
) {
    if let Some(owner) = unsafe { owner.as_mut() }
        && !commands.is_null()
    {
        owner.submitted(unsafe { std::slice::from_raw_parts(commands, count as usize) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_present(
    owner: *mut Owner,
    info: *const c_void,
    source: *const Record,
    explicit: u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return WSI_ERROR;
    };
    let Some(source) = (unsafe { PresentSource::read(source, explicit) }) else {
        return WSI_ERROR;
    };
    owner.present(info, source)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_status(owner: *mut Owner, status: *mut Status) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(status) } {
        return INVALID;
    }
    if unsafe { (*status).abi_version } != RSF_DLSSGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    unsafe { status.write(owner.status) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_drain(owner: *mut Owner) -> i32 {
    unsafe { owner.as_mut() }.map_or(OK, Owner::drain)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlgvk_destroy(owner: *mut Owner) -> i32 {
    let Some(live) = (unsafe { owner.as_mut() }) else {
        return OK;
    };
    let drained = live.drain();
    if drained != OK {
        return drained;
    }
    // A refused callback detachment must leave the SDK feature and capture
    // allocations alive. Successful detachment keeps its state for a retry
    // if the subsequent SDK release refuses.
    let detached = unsafe { (live.platform.detach_capture)(live.native) };
    if detached != OK {
        return detached;
    }
    if live.resources {
        let (released, _) = live.sdk(live.platform.release, 0, None);
        if released != OK {
            return released;
        }
    }
    drop(unsafe { Box::from_raw(owner) });
    OK
}

#[cfg(test)]
mod owner_tests;
