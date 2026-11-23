// SPDX-License-Identifier: MIT
//! Streamline DLSS-G call leaves, Vulkan device preparation and capture
//! routing of the owner in vulkan_dlss_fg.rs, and `LEAVES`, the production platform table that
//! adds the session invoke and the Streamline worker epoch.
use super::rsf_core_dlgvk_report_api_error;
use super::sdk_fg_options::{self, Packed, State};
use crate::dlss_vulkan_capture::leaves::LEAVES as CAPTURE_LEAVES;
use crate::dlss_vulkan_capture::{
    Owner as PresentCapture, rsf_core_dlss_capture_close, rsf_core_dlss_capture_detach,
    rsf_core_dlss_capture_flush, rsf_core_dlss_capture_open, rsf_core_dlss_capture_remember,
};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_complete_group, rsf_core_capture_vk_create,
    rsf_core_capture_vk_destroy, rsf_core_capture_vk_discard_group, rsf_core_capture_vk_enabled,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_initialize, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record,
};
use crate::sys::rsf_srvk_image as SourceImage;
use crate::sys::{rsf_dlssgvk_status as Status, rsf_fgvk_setup as Setup};
use crate::vulkan_dlss_fg_abi::{Handle, Sample, Sdk, SdkCall};
use crate::vulkan_fg_resources::device::{
    Device, rsf_core_vkfg_device_destroy, rsf_core_vkfg_device_memory, rsf_core_vkfg_load_device,
};
use crate::vulkan_streamline_camera::sdk_api::{
    self, FEATURE_DLSS_G, GetFeatureFunction, IsFeatureSupported, OK as SDK_OK, guarded,
};
use crate::vulkan_streamline_camera::{
    image_resource, resource_tag, rsf_slvk_sdk_resource, rsf_slvk_sdk_tag, with_previous_to_clip,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::{
    FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, MISSING_ENTRY_POINT, NOT_READY, NOT_SUPPORTED,
    OK,
};
use rsf_game_sys::{RSF_FRAME_FLAG_RESET, rsf_frame_record as Record};
use std::ptr;

const SR_DLSS: u32 = 1;
const SHARED_SUBMISSION: u32 = 0;
const VIEWPORT: u32 = 0;
const TRUE: u8 = 1;

const _: () = {
    assert!(vk::ImageUsageFlags::TRANSFER_SRC.as_raw() == 1);
    assert!(vk::ImageUsageFlags::TRANSFER_DST.as_raw() == 2);
    assert!(vk::Format::R8G8B8A8_UNORM.as_raw() == 37 && vk::Format::B8G8R8A8_UNORM.as_raw() == 44);
};

/// The native owner. `sdk` comes first: the native fixture's initialize
/// leaf writes its entry points there.
#[repr(C)]
struct Native {
    sdk: Sdk,
    setup: Setup,
    gpu: *mut Device,
    capture: *mut Capture,
    present: *mut PresentCapture,
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            rsf_core_dlss_capture_close(self.present);
            rsf_core_capture_vk_destroy(self.capture);
            rsf_core_vkfg_device_destroy(self.gpu);
        }
    }
}

fn call<'a>(user: Handle) -> &'a mut SdkCall {
    unsafe { &mut *user.cast::<SdkCall>() }
}

fn native<'a>(handle: Handle) -> &'a mut Native {
    unsafe { &mut *handle.cast::<Native>() }
}

fn sdk_result(call: &mut SdkCall, result: i32) -> i32 {
    call.result = result;
    if result == SDK_OK { OK } else { FEATURE_FAILED }
}

unsafe extern "C" fn on_error(error: *const i32) {
    if let Some(&error) = unsafe { error.as_ref() } {
        rsf_core_dlgvk_report_api_error(error);
    }
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_initialize(module: Handle, _: usize, user: Handle) -> i32 {
    let call = call(user);
    let native = native(call.native);
    let supported =
        unsafe { sdk_api::export::<IsFeatureSupported>(module, c"slIsFeatureSupported") };
    let feature = unsafe { sdk_api::export::<GetFeatureFunction>(module, c"slGetFeatureFunction") };
    native.sdk.set_constants = unsafe { sdk_api::export(module, c"slSetConstants") };
    native.sdk.set_tags = unsafe { sdk_api::export(module, c"slSetTagForFrame") };
    native.sdk.free_resources = unsafe { sdk_api::export(module, c"slFreeResources") };
    let (Some(supported), Some(feature), Some(_), Some(_), Some(_)) = (
        supported,
        feature,
        native.sdk.set_constants,
        native.sdk.set_tags,
        native.sdk.free_resources,
    ) else {
        return MISSING_ENTRY_POINT;
    };
    let adapter = sdk_api::adapter(native.setup.graphics.physical_device);
    let support = guarded(|| unsafe { supported(FEATURE_DLSS_G, &adapter) });
    call.result = support;
    if support != SDK_OK {
        return NOT_SUPPORTED;
    }
    native.sdk.set_options =
        unsafe { sdk_api::feature(feature, FEATURE_DLSS_G, c"slDLSSGSetOptions") };
    native.sdk.get_state = unsafe { sdk_api::feature(feature, FEATURE_DLSS_G, c"slDLSSGGetState") };
    if native.sdk.set_options.is_none() || native.sdk.get_state.is_none() {
        return MISSING_ENTRY_POINT;
    }
    OK
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_configure(_: Handle, _: usize, user: Handle) -> i32 {
    let call = call(user);
    let Some(set_options) = native(call.native).sdk.set_options else {
        return MISSING_ENTRY_POINT;
    };
    let callback: sdk_api::ApiErrorCallback = on_error;
    let options = Packed {
        // The slot holds PFunOnAPIErrorCallback, which receives `const APIError&`.
        reserved_callback: Some(unsafe {
            std::mem::transmute::<sdk_api::ApiErrorCallback, unsafe extern "C" fn()>(callback)
        }),
        ..unsafe { *call.options }
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    let result = guarded(|| unsafe { set_options(&viewport, &options) });
    sdk_result(call, result)
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_sample(_: Handle, _: usize, user: Handle) -> i32 {
    let call = call(user);
    let Some(get_state) = native(call.native).sdk.get_state else {
        return MISSING_ENTRY_POINT;
    };
    let mut state: State = sdk_fg_options::state();
    let viewport = sdk_api::viewport(VIEWPORT);
    let result = guarded(|| unsafe { get_state(&viewport, &mut state, ptr::null()) });
    let result = sdk_result(call, result);
    if result == OK {
        call.sample = Sample {
            semaphore: state.inputs_fence.addr() as u64,
            value: state.inputs_fence_value,
            actual_presents: state.actual_presents,
            max_generated: state.max_generated,
            status: state.status,
            min_dimension: state.min_dimension,
            vsync: u32::from(state.vsync == TRUE),
            dynamic_mfg: u32::from(state.dynamic_mfg == TRUE),
        };
    }
    result
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_constants(_: Handle, token: usize, user: Handle) -> i32 {
    let call = call(user);
    if token == 0 {
        return NOT_READY;
    }
    let Some(set_constants) = native(call.native).sdk.set_constants else {
        return MISSING_ENTRY_POINT;
    };
    let Some(constants) = with_previous_to_clip(unsafe { &*call.camera }) else {
        return INVALID_ARGUMENT;
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    let token = ptr::with_exposed_provenance(token);
    let result = guarded(|| unsafe { set_constants(&constants, token, &viewport) });
    sdk_result(call, result)
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_tag(_: Handle, token: usize, user: Handle) -> i32 {
    let call = call(user);
    let Some(set_tags) = native(call.native).sdk.set_tags else {
        return MISSING_ENTRY_POINT;
    };
    let plans = unsafe { std::slice::from_raw_parts(call.tags, 2) };
    // Streamline copies the staged guides on the leased recording; the
    // resource records stay live for the call.
    let usage = (vk::ImageUsageFlags::SAMPLED
        | vk::ImageUsageFlags::TRANSFER_SRC
        | vk::ImageUsageFlags::TRANSFER_DST)
        .as_raw();
    let layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32;
    let mut resources: [rsf_slvk_sdk_resource; 2] = Default::default();
    let mut tags: [rsf_slvk_sdk_tag; 2] = Default::default();
    for ((plan, resource), tag) in plans.iter().zip(&mut resources).zip(&mut tags) {
        let image = unsafe { plan.image.as_ref() };
        if let Some(image) = image {
            *resource = image_resource(image, layout, usage);
        }
        *tag = resource_tag(
            image.map_or(ptr::null_mut(), |_| ptr::from_mut(resource).cast()),
            plan.kind,
            plan.lifetime,
            image.map(|_| [plan.width, plan.height]),
        );
    }
    let viewport = sdk_api::viewport(VIEWPORT);
    let token = ptr::with_exposed_provenance(token);
    let result = guarded(|| unsafe { set_tags(token, &viewport, tags.as_ptr(), 2, call.commands) });
    sdk_result(call, result)
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_release(_: Handle, _: usize, user: Handle) -> i32 {
    let call = call(user);
    let Some(free_resources) = native(call.native).sdk.free_resources else {
        return MISSING_ENTRY_POINT;
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    let result = guarded(|| unsafe { free_resources(FEATURE_DLSS_G, &viewport) });
    sdk_result(call, result)
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_open(setup: *const Setup, out: *mut Handle) -> i32 {
    let native = Box::new(Native {
        sdk: Sdk::default(),
        setup: unsafe { *setup },
        gpu: ptr::null_mut(),
        capture: rsf_core_capture_vk_create(SHARED_SUBMISSION),
        present: ptr::null_mut(),
    });
    unsafe { out.write(Box::into_raw(native).cast()) };
    OK
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_close(handle: Handle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle.cast::<Native>()) });
    }
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_prepare_chain(
    handle: Handle,
    capture_enabled: *mut u32,
    gpu: *mut Handle,
) -> i32 {
    let native = native(handle);
    let graphics = &native.setup.graphics;
    let name = c"vkGetPhysicalDeviceMemoryProperties";
    let Some(properties) = graphics
        .instance_proc
        .and_then(|lookup| unsafe { lookup(graphics.instance, name.as_ptr()) })
    else {
        return MISSING_ENTRY_POINT;
    };
    let properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties =
        unsafe { std::mem::transmute(properties) };
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(graphics.physical_device as u64),
            &mut memory,
        )
    };
    let device = unsafe { rsf_core_vkfg_load_device(graphics, &native.setup, true) };
    if device.is_null() {
        return MISSING_ENTRY_POINT;
    }
    unsafe { rsf_core_vkfg_device_destroy(native.gpu) };
    native.gpu = device;
    rsf_core_vkfg_device_memory(unsafe { &mut *device }, &memory);
    unsafe { rsf_core_capture_vk_provider(native.capture, 0, SR_DLSS, 1) };
    let graphics = &native.setup.graphics;
    if unsafe {
        rsf_core_capture_vk_initialize(
            native.capture,
            graphics.device,
            graphics.device_proc,
            &memory,
        )
    } == 0
    {
        return INIT_FAILED;
    }
    unsafe {
        capture_enabled.write(u32::from(rsf_core_capture_vk_enabled(native.capture) != 0));
        gpu.write(device.cast());
    }
    OK
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_start_capture(handle: Handle) -> i32 {
    let native = native(handle);
    unsafe { rsf_core_dlss_capture_close(native.present) };
    native.present = ptr::null_mut();
    unsafe { rsf_core_dlss_capture_open(&CAPTURE_LEAVES, &native.setup, &mut native.present) }
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_capture(
    handle: Handle,
    commands: Handle,
    image: *const SourceImage,
    record: *const Record,
    role: u32,
    group: u64,
) {
    const ROLES: [&std::ffi::CStr; 3] = [c"fg-depth", c"fg-motion-uv", c"fg-hudless-color"];
    let Some(role) = ROLES.get(role as usize) else {
        return;
    };
    let reset = unsafe { (*record).flags } & RSF_FRAME_FLAG_RESET;
    unsafe {
        rsf_core_capture_vk_record(
            native(handle).capture,
            commands,
            image,
            record,
            role.as_ptr(),
            reset,
            1,
            ptr::null_mut(),
            group,
        )
    };
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_remember(
    handle: Handle,
    record: *const Record,
    status: *const Status,
    configured: u32,
) {
    let count = unsafe { (*status).requested_generated_frames };
    unsafe {
        rsf_core_dlss_capture_remember(native(handle).present, record, count, status, configured)
    };
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_capture_group(handle: Handle, group: u64, complete: u32) {
    let capture = native(handle).capture;
    unsafe {
        if complete != 0 {
            rsf_core_capture_vk_complete_group(capture, group);
        } else {
            rsf_core_capture_vk_discard_group(capture, group);
        }
    }
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_flush(handle: Handle) {
    let native = native(handle);
    unsafe {
        rsf_core_capture_vk_flush(native.capture, 0, ptr::null_mut());
        rsf_core_dlss_capture_flush(native.present);
    }
}

unsafe extern "C" fn rsf_core_dlgvk_sdk_detach_capture(handle: Handle) -> i32 {
    unsafe { rsf_core_dlss_capture_detach(native(handle).present) }
}

unsafe extern "C" {
    // rsf-streamline-vk's bridge export (sl.def), linked from the same Runtime archive.
    fn rsf_vk_sl_worker_timeout_epoch() -> u64;
}

unsafe extern "C" fn timeout_epoch() -> u64 {
    unsafe { rsf_vk_sl_worker_timeout_epoch() }
}

// SAFETY: the table holds function pointers and the address of an immutable static.
unsafe impl Sync for crate::vulkan_dlss_fg_abi::Platform {}

/// The production Streamline table (`rsf_dlssgvk_create` with the `streamline-vk` feature).
/// fixture/sdk.rs copies it and replaces the SDK, session invoke and worker epoch entries.
pub(super) static LEAVES: crate::vulkan_dlss_fg_abi::Platform =
    crate::vulkan_dlss_fg_abi::Platform {
        gpu: &crate::vulkan_fg_resources::device::DEVICE_PLATFORM,
        open: rsf_core_dlgvk_sdk_open,
        close: rsf_core_dlgvk_sdk_close,
        prepare_chain: rsf_core_dlgvk_sdk_prepare_chain,
        start_capture: rsf_core_dlgvk_sdk_start_capture,
        invoke: crate::vulkan_latency::rsf_slvk_invoke,
        initialize: Some(rsf_core_dlgvk_sdk_initialize),
        configure: Some(rsf_core_dlgvk_sdk_configure),
        sample: Some(rsf_core_dlgvk_sdk_sample),
        constants: Some(rsf_core_dlgvk_sdk_constants),
        tag: Some(rsf_core_dlgvk_sdk_tag),
        release: Some(rsf_core_dlgvk_sdk_release),
        capture: rsf_core_dlgvk_sdk_capture,
        remember: rsf_core_dlgvk_sdk_remember,
        capture_group: rsf_core_dlgvk_sdk_capture_group,
        flush: rsf_core_dlgvk_sdk_flush,
        detach_capture: rsf_core_dlgvk_sdk_detach_capture,
        timeout_epoch,
    };
