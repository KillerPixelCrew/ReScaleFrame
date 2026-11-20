// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::sr_session::has_capacity;
use crate::vulkan_dlss_sr_abi::{rsf_vkdlss_images, rsf_vkdlss_platform as Platform};
use crate::vulkan_latency_abi::{RSF_SLVK_ABI_VERSION, rsf_slvk_call};
use crate::vulkan_sr::History;
use crate::vulkan_sr_abi::{rsf_srvk_frame, rsf_srvk_setup};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

const INVALID: i32 = -1;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const RGBA16: u32 = 97;
const R32: u32 = 100;
const RG16: u32 = 83;
const R16: u32 = 76;
const INPUT_USAGE: u32 = 0x6; // Sampled | transfer destination.
const OUTPUT_USAGE: u32 = 0xf; // Storage | sampled | transfer source/destination.

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

struct Owner {
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
            (&mut self.images.color, RGBA16),
            (&mut self.images.depth, R32),
            (&mut self.images.motion, RG16),
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
                RGBA16,
                OUTPUT_USAGE,
            ) != 0
        }
    }

    fn release_effect(&mut self) -> i32 {
        if !self.resources {
            return 0;
        }
        let call = rsf_slvk_call {
            struct_size: size_of::<rsf_slvk_call>() as u32,
            abi_version: RSF_SLVK_ABI_VERSION,
            session_id: 0,
            frame_id: 0,
            callback: self.platform.release,
            user: self.native,
        };
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
    unsafe {
        (platform.capture.unwrap())(native, *commands, *input, &images.output, reset);
        (platform.sharpen.unwrap())(native, *commands, *input, images.output.image);
        (platform.publish.unwrap())(native, *commands, *input, images.output.image);
    }
    owner.history.commit(record);
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_create(
    platform: &'static Platform,
    setup: *const rsf_srvk_setup,
    out: *mut *mut Owner,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.quality > 4 || setup.output_width == 0 || setup.output_height == 0 {
        return INVALID;
    }
    let Some(claim) = Claim::acquire() else {
        return NOT_READY;
    };
    let raw = allocate_handle(Owner {
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
        return INIT_FAILED;
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
        return result;
    }
    if owner.native.is_null() || owner.width == 0 || owner.height == 0 || !owner.allocate(setup) {
        return INIT_FAILED;
    }
    unsafe { out.write(Box::into_raw(owner)) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_plan(
    owner: *const Owner,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if width.is_null() || height.is_null() {
        return INVALID;
    }
    unsafe {
        width.write(owner.width);
        height.write(owner.height);
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_evaluate(
    owner: *mut Owner,
    commands: *mut c_void,
    input: *const rsf_srvk_frame,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    if commands.is_null() || !unsafe { has_capacity(input) } {
        return INVALID;
    }
    let input = unsafe { &*input };
    if !unsafe { has_capacity(input.record) }
        || input.color.copy_source == 0
        || input.depth.copy_source == 0
        || input.motion.copy_source == 0
        || input.depth.format != R32
        || (input.bias_current_color.image != 0 && input.bias_current_color.copy_source == 0)
    {
        return INVALID;
    }
    let record = unsafe { &*input.record };
    // The outer SRVK owner normalizes and validates the leased render facts.
    let invoke = owner.platform.invoke.unwrap();
    let mut evaluation = Evaluation {
        owner,
        commands,
        input,
    };
    let call = rsf_slvk_call {
        struct_size: size_of::<rsf_slvk_call>() as u32,
        abi_version: RSF_SLVK_ABI_VERSION,
        session_id: record.session_id,
        frame_id: record.frame_id,
        callback: Some(evaluate),
        user: ptr::from_mut(&mut evaluation).cast(),
    };
    unsafe { invoke(&call) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_preset(owner: *mut Owner, preset: u32) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    if owner.preset == preset {
        return 0;
    }
    let result = owner.release_effect();
    if result == 0 {
        owner.preset = preset;
        owner.history = History::default();
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_destroy(owner: *mut Owner) -> i32 {
    let Some(active) = (unsafe { owner.as_mut() }) else {
        return 0;
    };
    let result = active.release_effect();
    if result != 0 {
        return result;
    }
    unsafe {
        (active.platform.flush.unwrap())(active.native);
        drop(Box::from_raw(owner));
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_retire(owner: *mut Owner, commands: *mut c_void) {
    if let Some(owner) = unsafe { owner.as_mut() } {
        unsafe { (owner.platform.retire.unwrap())(owner.native, commands) };
    }
}
