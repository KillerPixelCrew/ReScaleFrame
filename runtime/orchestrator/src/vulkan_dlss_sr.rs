// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::vulkan_dlss_sr_abi::{rsf_vkdlss_images, rsf_vkdlss_platform as Platform};
use crate::vulkan_latency_abi::{RSF_SLVK_ABI_VERSION, rsf_slvk_call, rsf_slvk_callback};
use crate::vulkan_sr::{
    GENERAL, History, R32_FLOAT, RG16_FLOAT, RGBA16_FLOAT, SAMPLED_USAGE, STORAGE_USAGE,
    TRANSFER_DST_USAGE, TRANSFER_SRC_USAGE,
};
use crate::vulkan_sr_abi::{rsf_srvk_frame, rsf_srvk_image, rsf_srvk_setup};
use rsf_game_sys::rsf_frame_record;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

const INVALID: i32 = -1;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const R16: u32 = 76;
const INPUT_USAGE: u32 = SAMPLED_USAGE | TRANSFER_DST_USAGE;
const OUTPUT_USAGE: u32 = INPUT_USAGE | STORAGE_USAGE | TRANSFER_SRC_USAGE;

impl rsf_slvk_call {
    pub(super) fn for_record(
        record: Option<&rsf_frame_record>,
        callback: rsf_slvk_callback,
        user: *mut c_void,
    ) -> Self {
        let (session_id, frame_id) =
            record.map_or((0, 0), |record| (record.session_id, record.frame_id));
        Self {
            struct_size: size_of::<Self>() as u32,
            abi_version: RSF_SLVK_ABI_VERSION,
            session_id,
            frame_id,
            callback,
            user,
        }
    }
}

static OCCUPIED: AtomicBool = AtomicBool::new(false);

struct Claim;

impl Claim {
    fn acquire() -> Option<Self> {
        OCCUPIED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        OCCUPIED.store(false, Ordering::Release);
    }
}

pub(super) struct Owner {
    _claim: Claim,
    platform: &'static Platform,
    native: *mut c_void,
    width: u32,
    height: u32,
    images: rsf_vkdlss_images,
    history: History,
    preset: u32,
    resources: bool,
}

impl Drop for Owner {
    fn drop(&mut self) {
        if self.native.is_null() {
            return;
        }
        let platform = self.platform;
        for image in [
            &mut self.images.color,
            &mut self.images.depth,
            &mut self.images.motion,
            &mut self.images.output,
            &mut self.images.bias,
        ] {
            unsafe { (platform.free_image.unwrap())(self.native, image) };
        }
        unsafe { (platform.close.unwrap())(self.native) };
    }
}

impl Owner {
    fn allocate(&mut self, setup: &rsf_srvk_setup) -> bool {
        let allocate = self.platform.allocate.unwrap();
        for (image, format) in [
            (&mut self.images.color, RGBA16_FLOAT),
            (&mut self.images.depth, R32_FLOAT),
            (&mut self.images.motion, RG16_FLOAT),
            (&mut self.images.bias, R16),
        ] {
            if unsafe {
                allocate(
                    self.native,
                    image,
                    self.width,
                    self.height,
                    format,
                    INPUT_USAGE,
                )
            } == 0
            {
                return false;
            }
        }
        unsafe {
            allocate(
                self.native,
                &mut self.images.output,
                setup.output_width,
                setup.output_height,
                RGBA16_FLOAT,
                OUTPUT_USAGE,
            ) != 0
        }
    }

    fn release_effect(&mut self) -> i32 {
        if !self.resources {
            return 0;
        }
        let call = rsf_slvk_call::for_record(None, self.platform.release, self.native);
        let result = unsafe { (self.platform.invoke.unwrap())(&call) };
        if result == 0 {
            self.resources = false;
        }
        result
    }
}

struct Evaluation<'a> {
    owner: &'a mut Owner,
    commands: *mut c_void,
    input: &'a rsf_srvk_frame,
}

unsafe extern "C" fn evaluate(_: *mut c_void, token: usize, user: *mut c_void) -> i32 {
    let evaluation = unsafe { &mut *user.cast::<Evaluation<'_>>() };
    let Evaluation {
        owner,
        commands,
        input,
    } = evaluation;
    if token == 0 {
        return INVALID;
    }
    let record = unsafe { &*input.record };
    let camera = &record.camera;
    if [
        &camera.view_to_clip,
        &camera.clip_to_view,
        &camera.clip_to_previous_clip,
        &camera.view_to_world,
    ]
    .into_iter()
    .flatten()
    .any(|value| !value.is_finite())
    {
        return INVALID;
    }
    let platform = owner.platform;
    let native = owner.native;
    let reset = u32::from(owner.history.needs_reset(record));
    let result = unsafe { (platform.constants.unwrap())(native, token, *input, reset) };
    if result != 0 {
        return result;
    }
    let result = unsafe { (platform.options.unwrap())(native, *input, owner.preset) };
    if result != 0 {
        return result;
    }

    let images = &mut owner.images;
    let stage = platform.stage.unwrap();
    for (source, image) in [
        (&input.color, &mut images.color),
        (&input.depth, &mut images.depth),
        (&input.motion, &mut images.motion),
    ] {
        unsafe { stage(native, *commands, source, image, 0) };
        image.initialized = 1;
    }
    if input.bias_current_color.image != 0 {
        unsafe {
            stage(
                native,
                *commands,
                &input.bias_current_color,
                &mut images.bias,
                1,
            )
        };
        images.bias.initialized = 1;
    }
    if images.output.initialized == 0 {
        unsafe { (platform.initialize_output.unwrap())(native, *commands, &images.output) };
        images.output.initialized = 1;
    }
    let result = unsafe { (platform.tag.unwrap())(native, token, *commands, *input, images) };
    if result != 0 {
        return result;
    }
    owner.resources = true;
    let result = unsafe { (platform.evaluate.unwrap())(native, token, *commands) };
    if result != 0 {
        owner.history = History::default();
        return result;
    }
    unsafe { (platform.capture_parameters.unwrap())(native, *input, reset) };
    let capture = platform.capture_record.unwrap();
    for (image, role) in [
        (&input.color, c"sr-source-jittered"),
        (&input.color, c"sr-linear-color"),
        (&input.depth, c"sr-depth"),
        (&input.motion, c"sr-motion-uv"),
        (&input.bias_current_color, c"sr-bias-mask"),
    ] {
        if image.image != 0 {
            unsafe { capture(native, *commands, image, record, role.as_ptr(), reset) };
        }
    }
    let output = rsf_srvk_image {
        device: input.output.device,
        image: images.output.image,
        format: RGBA16_FLOAT,
        width: record.output_width,
        height: record.output_height,
        layout: GENERAL,
        copy_source: 1,
    };
    unsafe {
        // Preserve the pre-sharpening SDK output in the capture.
        capture(
            native,
            *commands,
            &output,
            record,
            c"sr-output-linear".as_ptr(),
            reset,
        );
        (platform.sharpen.unwrap())(native, *commands, *input, images.output.image);
        (platform.publish.unwrap())(native, *commands, *input, images.output.image);
    }
    owner.history.commit(record);
    0
}

impl Owner {
    pub(super) fn create(
        platform: &'static Platform,
        setup: &rsf_srvk_setup,
    ) -> Result<Box<Self>, i32> {
        let claim = Claim::acquire().ok_or(NOT_READY)?;
        let raw = allocate_handle(Self {
            _claim: claim,
            platform,
            native: ptr::null_mut(),
            width: 0,
            height: 0,
            images: rsf_vkdlss_images::default(),
            history: History::default(),
            preset: 0,
            resources: false,
        });
        if raw.is_null() {
            return Err(INIT_FAILED);
        }
        let mut owner = unsafe { Box::from_raw(raw) };
        let result = unsafe {
            (platform.open.unwrap())(
                setup,
                &mut owner.native,
                &mut owner.width,
                &mut owner.height,
            )
        };
        if result != 0 {
            return Err(result);
        }
        if owner.native.is_null() || owner.width == 0 || owner.height == 0 || !owner.allocate(setup)
        {
            return Err(INIT_FAILED);
        }
        Ok(owner)
    }

    pub(super) fn plan(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    pub(super) fn evaluate(
        &mut self,
        commands: *mut c_void,
        input: &rsf_srvk_frame,
        record: &rsf_frame_record,
    ) -> i32 {
        if input.color.copy_source == 0
            || input.depth.copy_source == 0
            || input.motion.copy_source == 0
            || input.depth.format != R32_FLOAT
            || (input.bias_current_color.image != 0 && input.bias_current_color.copy_source == 0)
        {
            return INVALID;
        }
        let invoke = self.platform.invoke.unwrap();
        let mut evaluation = Evaluation {
            owner: self,
            commands,
            input,
        };
        let call = rsf_slvk_call::for_record(
            Some(record),
            Some(evaluate),
            ptr::from_mut(&mut evaluation).cast(),
        );
        unsafe { invoke(&call) }
    }

    pub(super) fn set_preset(&mut self, preset: u32) -> i32 {
        let Some(preset_valid) = self.platform.preset_valid else {
            return INVALID;
        };
        if unsafe { preset_valid(preset) } == 0 {
            return INVALID;
        }
        if self.preset == preset {
            return 0;
        }
        let result = self.release_effect();
        if result == 0 {
            self.preset = preset;
            self.history = History::default();
        }
        result
    }

    pub(super) fn destroy(&mut self) -> i32 {
        let result = self.release_effect();
        if result == 0 {
            unsafe { (self.platform.flush.unwrap())(self.native) };
        }
        result
    }

    pub(super) fn retire_commands(&self, commands: *mut c_void) {
        unsafe { (self.platform.retire.unwrap())(self.native, commands) };
    }
}
