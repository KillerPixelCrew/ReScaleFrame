// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::frame_capture_vk::sr_capture_inputs;
use rsf_game_sys::backend_result::{INIT_FAILED, INVALID_ARGUMENT, MISSING_ENTRY_POINT, NOT_READY};
mod commands;
mod leaves;

/// The Streamline leaf table when CMake verified the corrected Streamline Vulkan runtime (the
/// `streamline-vk` feature); without it DLSS SR reports NOT_COMPILED.
pub(crate) const fn native_leaves() -> Option<&'static Platform> {
    if cfg!(feature = "streamline-vk") {
        Some(&leaves::NATIVE)
    } else {
        None
    }
}
use self::commands::Dispatch;
use crate::sys::{RSF_SLVK_ABI_VERSION, rsf_slvk_call, rsf_slvk_callback};
use crate::sys::{rsf_slvk_sdk_constants, rsf_vkdlss_options};
use crate::sys::{rsf_srvk_frame, rsf_srvk_image, rsf_srvk_setup};
use crate::vulkan_fg_resources::Image as rsf_vkfg_image;
use crate::vulkan_ownership::Claim;
use crate::vulkan_sr::{
    GENERAL, History, R32_FLOAT, RG16_FLOAT, RGBA16_FLOAT, SAMPLED_USAGE, SHADER_READ,
    STORAGE_USAGE, TRANSFER_DST_USAGE, TRANSFER_SRC_USAGE,
};
use crate::vulkan_streamline_camera::{camera_constants, rsf_slvk_sdk_header};
use rsf_game_sys::rsf_frame_record;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::AtomicBool;

/// The owner's private images.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Images {
    pub(crate) color: rsf_vkfg_image,
    pub(crate) depth: rsf_vkfg_image,
    pub(crate) motion: rsf_vkfg_image,
    pub(crate) bias: rsf_vkfg_image,
    pub(crate) output: rsf_vkfg_image,
}

/// One Streamline resource tag the `tag` leaf records.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Tag {
    pub(crate) image: *const rsf_vkfg_image,
    pub(crate) kind: u32,
    pub(crate) layout: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The `initialize` callback's user record.
#[repr(C)]
pub(crate) struct Init {
    pub(crate) native: *mut c_void,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) options: *const rsf_vkdlss_options,
}

/// The owner's Streamline, device, image, sharpening and capture leaves: `leaves::NATIVE` in
/// production, fakes in vulkan_sr/owner_tests.rs.
pub(crate) struct Platform {
    pub(crate) open: unsafe extern "C" fn(*const rsf_srvk_setup, *mut *mut c_void) -> i32,
    pub(crate) close: unsafe extern "C" fn(*mut c_void),
    pub(crate) invoke: unsafe extern "C" fn(*const rsf_slvk_call) -> i32,
    pub(crate) initialize: rsf_slvk_callback,
    pub(crate) prepare: unsafe extern "C" fn(*mut c_void) -> i32,
    pub(crate) allocate:
        unsafe extern "C" fn(*mut c_void, *mut rsf_vkfg_image, u32, u32, u32, u32) -> u32,
    pub(crate) free_image: unsafe extern "C" fn(*mut c_void, *mut rsf_vkfg_image),
    pub(crate) constants:
        unsafe extern "C" fn(*mut c_void, usize, *const rsf_slvk_sdk_constants) -> i32,
    pub(crate) options: unsafe extern "C" fn(*mut c_void, *const rsf_vkdlss_options) -> i32,
    pub(crate) tag: unsafe extern "C" fn(*mut c_void, usize, *mut c_void, *const Tag) -> i32,
    pub(crate) evaluate: unsafe extern "C" fn(*mut c_void, usize, *mut c_void) -> i32,
    pub(crate) capture_parameters: unsafe extern "C" fn(*mut c_void, *const rsf_srvk_frame, u32),
    pub(crate) capture_record: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *const rsf_srvk_image,
        *const rsf_frame_record,
        *const std::ffi::c_char,
        u32,
    ),
    pub(crate) sharpen: unsafe extern "C" fn(*mut c_void, *mut c_void, *const rsf_srvk_frame, u64),
    pub(crate) release: rsf_slvk_callback,
    pub(crate) flush: unsafe extern "C" fn(*mut c_void),
    pub(crate) retire: unsafe extern "C" fn(*mut c_void, *mut c_void),
    pub(crate) preset_valid: unsafe extern "C" fn(u32) -> i32,
}
const R16: u32 = 76;
const INPUT_USAGE: u32 = SAMPLED_USAGE | TRANSFER_DST_USAGE;
const OUTPUT_USAGE: u32 = INPUT_USAGE | STORAGE_USAGE | TRANSFER_SRC_USAGE;
// Streamline's DLAA, Quality, Balanced, Performance and Ultra Performance.
const SDK_MODES: [u32; 5] = [6, 3, 2, 1, 4];

fn options(mode: u32, width: u32, height: u32) -> rsf_vkdlss_options {
    rsf_vkdlss_options {
        base: rsf_slvk_sdk_header {
            next: ptr::null_mut(),
            type_lo: 0x6ac8_26e4,
            type_mid: 0x4c61,
            type_hi: 0x4101,
            type_tail: [0xa9, 0x2d, 0x63, 0x8d, 0x42, 0x10, 0x57, 0xb8],
            version: 3,
        },
        mode,
        width,
        height,
        hdr: 1,
        pre_exposure: 1.0,
        exposure_scale: 1.0,
        ..Default::default()
    }
}

pub(super) fn slvk_call(
    record: Option<&rsf_frame_record>,
    callback: rsf_slvk_callback,
    user: *mut c_void,
) -> rsf_slvk_call {
    let (session_id, frame_id) =
        record.map_or((0, 0), |record| (record.session_id, record.frame_id));
    rsf_slvk_call {
        struct_size: size_of::<rsf_slvk_call>() as u32,
        abi_version: RSF_SLVK_ABI_VERSION,
        session_id,
        frame_id,
        callback,
        user,
    }
}

static OCCUPIED: AtomicBool = AtomicBool::new(false);

pub(super) struct Owner {
    _claim: Claim,
    platform: &'static Platform,
    native: *mut c_void,
    dispatch: Option<Dispatch>,
    width: u32,
    height: u32,
    mode: u32,
    images: Images,
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
            // A refused create allocated nothing; only images it holds are released.
            if image.image != 0 || image.memory != 0 || image.view != 0 {
                unsafe { (platform.free_image)(self.native, image) };
            }
        }
        unsafe { (platform.close)(self.native) };
    }
}

impl Owner {
    fn allocate(&mut self, setup: &rsf_srvk_setup) -> bool {
        let allocate = self.platform.allocate;
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
        let call = slvk_call(None, self.platform.release, self.native);
        let result = unsafe { (self.platform.invoke)(&call) };
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
        return INVALID_ARGUMENT;
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
        return INVALID_ARGUMENT;
    }
    let platform = owner.platform;
    let native = owner.native;
    let reset = u32::from(owner.history.needs_reset(record));
    let constants = camera_constants(
        record,
        [input.motion_scale_x, input.motion_scale_y],
        reset != 0,
    );
    let result = unsafe { (platform.constants)(native, token, &constants) };
    if result != 0 {
        return result;
    }
    let mut evaluate_options = options(owner.mode, record.output_width, record.output_height);
    evaluate_options.auto_exposure = 1;
    evaluate_options.pre_exposure = input.pre_exposure;
    evaluate_options.presets = [owner.preset; 6];
    let result = unsafe { (platform.options)(native, &evaluate_options) };
    if result != 0 {
        return result;
    }

    let images = &mut owner.images;
    let dispatch = owner.dispatch.as_ref().unwrap();
    for (source, image) in [
        (&input.color, &mut images.color),
        (&input.depth, &mut images.depth),
        (&input.motion, &mut images.motion),
    ] {
        dispatch.stage(*commands, source, image, false);
        image.initialized = 1;
    }
    if input.bias_current_color.image != 0 {
        dispatch.stage(*commands, &input.bias_current_color, &images.bias, true);
        images.bias.initialized = 1;
    }
    if images.output.initialized == 0 {
        dispatch.initialize_output(*commands, &images.output);
        images.output.initialized = 1;
    }
    let render_tag = |image, kind| Tag {
        image,
        kind,
        layout: SHADER_READ,
        width: images.color.width,
        height: images.color.height,
    };
    let tags = [
        render_tag(&images.color, 3),
        render_tag(&images.depth, 0),
        render_tag(&images.motion, 1),
        Tag {
            image: &images.output,
            kind: 4,
            layout: GENERAL,
            width: record.output_width,
            height: record.output_height,
        },
        // An explicit null tag clears a mask used on an earlier frame.
        render_tag(
            if input.bias_current_color.image != 0 {
                &images.bias
            } else {
                ptr::null()
            },
            29,
        ),
    ];
    let result = unsafe { (platform.tag)(native, token, *commands, tags.as_ptr()) };
    if result != 0 {
        return result;
    }
    owner.resources = true;
    let result = unsafe { (platform.evaluate)(native, token, *commands) };
    if result != 0 {
        owner.history = History::default();
        return result;
    }
    unsafe { (platform.capture_parameters)(native, *input, reset) };
    let capture = platform.capture_record;
    for (image, role) in sr_capture_inputs(input)
        .into_iter()
        .chain([(&input.bias_current_color, c"sr-bias-mask")])
    {
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
        (platform.sharpen)(native, *commands, *input, images.output.image);
    }
    dispatch.publish(
        *commands,
        images.output.image,
        input.output.image,
        record.output_width,
        record.output_height,
    );
    owner.history.commit(record);
    0
}

impl Owner {
    pub(super) fn create(
        platform: &'static Platform,
        setup: &rsf_srvk_setup,
    ) -> Result<Box<Self>, i32> {
        let mode = *SDK_MODES
            .get(setup.quality as usize)
            .ok_or(INVALID_ARGUMENT)?;
        let claim = Claim::acquire(&OCCUPIED).ok_or(NOT_READY)?;
        let raw = allocate_handle(Self {
            _claim: claim,
            platform,
            native: ptr::null_mut(),
            dispatch: None,
            width: 0,
            height: 0,
            mode,
            images: Images::default(),
            history: History::default(),
            preset: 0,
            resources: false,
        });
        if raw.is_null() {
            return Err(INIT_FAILED);
        }
        let mut owner = unsafe { Box::from_raw(raw) };
        let result = unsafe { (platform.open)(setup, &mut owner.native) };
        if result != 0 {
            return Err(result);
        }
        if owner.native.is_null() {
            return Err(INIT_FAILED);
        }
        let request = options(mode, setup.output_width, setup.output_height);
        let mut init = Init {
            native: owner.native,
            width: 0,
            height: 0,
            options: &request,
        };
        let call = slvk_call(None, platform.initialize, ptr::from_mut(&mut init).cast());
        let result = unsafe { (platform.invoke)(&call) };
        if result != 0 {
            return Err(result);
        }
        owner.width = init.width;
        owner.height = init.height;
        let dispatch = Dispatch::load(setup).ok_or(MISSING_ENTRY_POINT)?;
        let result = unsafe { (platform.prepare)(owner.native) };
        if result != 0 {
            return Err(result);
        }
        owner.dispatch = Some(dispatch);
        if owner.width == 0 || owner.height == 0 || !owner.allocate(setup) {
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
            return INVALID_ARGUMENT;
        }
        let invoke = self.platform.invoke;
        let mut evaluation = Evaluation {
            owner: self,
            commands,
            input,
        };
        let call = slvk_call(
            Some(record),
            Some(evaluate),
            ptr::from_mut(&mut evaluation).cast(),
        );
        unsafe { invoke(&call) }
    }

    pub(super) fn set_preset(&mut self, preset: u32) -> i32 {
        if unsafe { (self.platform.preset_valid)(preset) } == 0 {
            return INVALID_ARGUMENT;
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
            unsafe { (self.platform.flush)(self.native) };
        }
        result
    }

    pub(super) fn retire_commands(&self, commands: *mut c_void) {
        unsafe { (self.platform.retire)(self.native, commands) };
    }
}
