// SPDX-License-Identifier: MIT
//! Streamline DLSS SR call leaves, device preparation, private images,
//! sharpening and capture of the Vulkan DLSS SR owner, formerly
//! native_dlss_vulkan.cpp. native_dlss_vulkan.c lists them in the production
//! platform table with the session invoke, the FrameDump parameter call and
//! the SDK preset policy.
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_create, rsf_core_capture_vk_destroy,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_initialize, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record, rsf_core_capture_vk_retire,
};
use crate::sys::rsf_vkfg_image as Image;
use crate::sys::{rsf_srvk_frame as Frame, rsf_srvk_image as SourceImage, rsf_srvk_setup as Setup};
use crate::sys::{
    rsf_vkdlss_init as Init, rsf_vkdlss_optimal as Optimal, rsf_vkdlss_options as Options,
    rsf_vkdlss_tag as Tag,
};
use crate::vulkan_fg_resources::device::{
    Device, rsf_core_vkfg_device_destroy, rsf_core_vkfg_device_memory, rsf_core_vkfg_load_device,
    rsf_core_vkfg_platform,
};
use crate::vulkan_fg_resources::{rsf_core_vkfg_ensure_image_usage, rsf_core_vkfg_release_image};
use crate::vulkan_sharpening::Host as Sharpen;
use crate::vulkan_sharpening::{
    rsf_sharpenvk_host_clear, rsf_sharpenvk_host_destroy, rsf_sharpenvk_host_retire,
    rsf_sharpenvk_host_run,
};
use crate::vulkan_streamline_camera::sdk_api::{
    self, EvaluateFeature, FEATURE_DLSS, FreeResources, GetOptimalSettings, OK as SDK_OK,
    SetConstants, SetOptions, SetTagForFrame, VALID_UNTIL_EVALUATE, guarded,
};
use crate::vulkan_streamline_camera::{
    image_resource, resource_tag, rsf_slvk_sdk_constants, rsf_slvk_sdk_resource, rsf_slvk_sdk_tag,
    with_previous_to_clip,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::{
    FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, MISSING_ENTRY_POINT, NOT_SUPPORTED, OK,
};
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::{c_char, c_void};
use std::ptr;

const SR_DLSS: u32 = 1;
const COMMAND_RETIREMENT: u32 = 1;
/// DLSS SR viewport. DLSS-G uses viewport zero in the shared session.
const VIEWPORT: u32 = 1;
const TAGS: usize = 5;

#[derive(Default)]
struct Sdk {
    set_constants: Option<SetConstants<rsf_slvk_sdk_constants>>,
    set_tags: Option<SetTagForFrame<rsf_slvk_sdk_tag>>,
    evaluate: Option<EvaluateFeature>,
    free_resources: Option<FreeResources>,
    optimal: Option<GetOptimalSettings<Options, Optimal>>,
    set_options: Option<SetOptions<Options>>,
}

struct Native {
    sharpen: Sharpen,
    setup: Setup,
    gpu: *mut Device,
    sdk: Sdk,
    capture: *mut Capture,
}

impl Drop for Native {
    fn drop(&mut self) {
        // The former C++ destructor body, then its members in reverse order.
        unsafe {
            rsf_sharpenvk_host_destroy(&mut self.sharpen);
            rsf_core_capture_vk_destroy(self.capture);
            rsf_core_vkfg_device_destroy(self.gpu);
        }
    }
}

fn native<'a>(handle: *mut c_void) -> &'a mut Native {
    unsafe { &mut *handle.cast::<Native>() }
}

fn feature_result(result: i32) -> i32 {
    if result == SDK_OK { OK } else { FEATURE_FAILED }
}

fn optimal_settings() -> Optimal {
    Optimal {
        base: sdk_api::DLSS_OPTIMAL,
        ..Default::default()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_open(setup: *const Setup, out: *mut *mut c_void) -> i32 {
    let native = Box::new(Native {
        sharpen: Sharpen::default(),
        setup: unsafe { *setup },
        gpu: ptr::null_mut(),
        sdk: Sdk::default(),
        capture: rsf_core_capture_vk_create(COMMAND_RETIREMENT),
    });
    unsafe { out.write(Box::into_raw(native).cast()) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_close(handle: *mut c_void) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle.cast::<Native>()) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_initialize(
    module: *mut c_void,
    _: usize,
    user: *mut c_void,
) -> i32 {
    let init = unsafe { &mut *user.cast::<Init>() };
    let native = native(init.native);
    let sdk = &mut native.sdk;
    let entries = unsafe { sdk_api::Entries::resolve(module) };
    sdk.set_constants = entries.set_constants;
    sdk.set_tags = entries.set_tag_for_frame;
    sdk.evaluate = entries.evaluate_feature;
    sdk.free_resources = entries.free_resources;
    let (Some(supported), Some(feature)) =
        (entries.is_feature_supported, entries.get_feature_function)
    else {
        return MISSING_ENTRY_POINT;
    };
    if sdk.set_constants.is_none()
        || sdk.set_tags.is_none()
        || sdk.evaluate.is_none()
        || sdk.free_resources.is_none()
    {
        return MISSING_ENTRY_POINT;
    }
    let adapter = sdk_api::adapter(native.setup.physical_device);
    if guarded(|| unsafe { supported(FEATURE_DLSS, &adapter) }) != SDK_OK {
        return NOT_SUPPORTED;
    }
    sdk.optimal = unsafe { sdk_api::feature(feature, FEATURE_DLSS, c"slDLSSGetOptimalSettings") };
    sdk.set_options = unsafe { sdk_api::feature(feature, FEATURE_DLSS, c"slDLSSSetOptions") };
    let (Some(optimal), Some(_)) = (sdk.optimal, sdk.set_options) else {
        return MISSING_ENTRY_POINT;
    };
    let mut plan = optimal_settings();
    let request = init.options;
    if guarded(|| unsafe { optimal(request, &mut plan) }) != SDK_OK
        || plan.width == 0
        || plan.height == 0
    {
        return NOT_SUPPORTED;
    }
    init.width = plan.width;
    init.height = plan.height;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_prepare(handle: *mut c_void) -> i32 {
    let native = native(handle);
    let device = unsafe { rsf_core_vkfg_load_device(&native.setup, ptr::null(), true) };
    if device.is_null() {
        return MISSING_ENTRY_POINT;
    }
    unsafe { rsf_core_vkfg_device_destroy(native.gpu) };
    native.gpu = device;
    let setup = &native.setup;
    let name = c"vkGetPhysicalDeviceMemoryProperties";
    let Some(properties) = setup
        .instance_proc
        .and_then(|lookup| unsafe { lookup(setup.instance, name.as_ptr()) })
    else {
        return MISSING_ENTRY_POINT;
    };
    let properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties =
        unsafe { std::mem::transmute(properties) };
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(setup.physical_device as u64),
            &mut memory,
        )
    };
    rsf_core_vkfg_device_memory(unsafe { &mut *device }, &memory);
    unsafe { rsf_core_capture_vk_provider(native.capture, 0, SR_DLSS, 0) };
    if unsafe {
        rsf_core_capture_vk_initialize(native.capture, setup.device, setup.device_proc, &memory)
    } == 0
    {
        return INIT_FAILED;
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_allocate(
    handle: *mut c_void,
    image: *mut Image,
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
) -> u32 {
    let gpu = native(handle).gpu;
    rsf_core_vkfg_ensure_image_usage(
        &rsf_core_vkfg_platform,
        gpu.cast(),
        unsafe { &mut *image },
        width,
        height,
        format,
        usage,
        1,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_free_image(handle: *mut c_void, image: *mut Image) {
    let gpu = native(handle).gpu;
    rsf_core_vkfg_release_image(&rsf_core_vkfg_platform, gpu.cast(), unsafe { &mut *image });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_constants(
    handle: *mut c_void,
    token: usize,
    input: *const rsf_slvk_sdk_constants,
) -> i32 {
    let Some(set_constants) = native(handle).sdk.set_constants else {
        return MISSING_ENTRY_POINT;
    };
    let Some(constants) = with_previous_to_clip(unsafe { &*input }) else {
        return INVALID_ARGUMENT;
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    let token = ptr::with_exposed_provenance(token);
    feature_result(guarded(|| unsafe {
        set_constants(&constants, token, &viewport)
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_options(
    handle: *mut c_void,
    input: *const Options,
) -> i32 {
    let Some(set_options) = native(handle).sdk.set_options else {
        return MISSING_ENTRY_POINT;
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    feature_result(guarded(|| unsafe { set_options(&viewport, input) }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_tag(
    handle: *mut c_void,
    token: usize,
    commands: *mut c_void,
    plan: *const Tag,
) -> i32 {
    let Some(set_tags) = native(handle).sdk.set_tags else {
        return MISSING_ENTRY_POINT;
    };
    let plan = unsafe { std::slice::from_raw_parts(plan, TAGS) };
    let mut resources: [rsf_slvk_sdk_resource; TAGS] = Default::default();
    let mut tags: [rsf_slvk_sdk_tag; TAGS] = Default::default();
    for ((item, resource), tag) in plan.iter().zip(&mut resources).zip(&mut tags) {
        let image = unsafe { item.image.as_ref() };
        if let Some(image) = image {
            *resource = image_resource(image, item.layout, image.usage);
        }
        let resource = image.map_or(ptr::null_mut(), |_| ptr::from_mut(resource).cast());
        *tag = resource_tag(
            resource,
            item.kind,
            VALID_UNTIL_EVALUATE,
            Some([item.width, item.height]),
        );
    }
    let viewport = sdk_api::viewport(VIEWPORT);
    let token = ptr::with_exposed_provenance(token);
    let count = TAGS as u32;
    feature_result(guarded(|| unsafe {
        set_tags(token, &viewport, tags.as_ptr(), count, commands)
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_evaluate(
    handle: *mut c_void,
    token: usize,
    commands: *mut c_void,
) -> i32 {
    let Some(evaluate) = native(handle).sdk.evaluate else {
        return MISSING_ENTRY_POINT;
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    let inputs = [ptr::from_ref(&viewport.base)];
    let token = ptr::with_exposed_provenance(token);
    feature_result(guarded(|| unsafe {
        evaluate(FEATURE_DLSS, token, inputs.as_ptr(), 1, commands)
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_capture_record(
    handle: *mut c_void,
    commands: *mut c_void,
    image: *const SourceImage,
    record: *const Record,
    role: *const c_char,
    reset: u32,
) {
    unsafe {
        rsf_core_capture_vk_record(
            native(handle).capture,
            commands,
            image,
            record,
            role,
            reset,
            1,
            ptr::null_mut(),
            0,
        )
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_sharpen(
    handle: *mut c_void,
    commands: *mut c_void,
    input: *const Frame,
    output: u64,
) {
    let native = native(handle);
    unsafe {
        rsf_sharpenvk_host_run(
            &mut native.sharpen,
            &native.setup,
            commands,
            &*input,
            output,
        )
    };
}

/// The release callback runs under the session invoke with the native as user.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_release(
    _: *mut c_void,
    _: usize,
    handle: *mut c_void,
) -> i32 {
    let Some(free_resources) = native(handle).sdk.free_resources else {
        return MISSING_ENTRY_POINT;
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    feature_result(guarded(|| unsafe {
        free_resources(FEATURE_DLSS, &viewport)
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_flush(handle: *mut c_void) {
    let native = native(handle);
    unsafe {
        rsf_core_capture_vk_flush(native.capture, 0, ptr::null_mut());
        rsf_sharpenvk_host_clear(&mut native.sharpen);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkdlss_sdk_retire(handle: *mut c_void, commands: *mut c_void) {
    let native = native(handle);
    unsafe {
        rsf_sharpenvk_host_retire(&mut native.sharpen, commands);
        rsf_core_capture_vk_retire(native.capture, commands);
    }
}

const _: () = {
    assert!(size_of::<Tag>() == 24);
    assert!(size_of::<Options>() == 88);
    assert!(size_of::<Optimal>() == 64);
};
