// SPDX-License-Identifier: MIT
use super::graphics::sdk_device_proc;
use super::{FfxDispatch, Handle, Image, Setup};
use crate::sharpening_policy::ffx_sharpening;
use ash::vk;
use std::ptr;

pub(super) use crate::ffx_vk_abi as abi;
use abi::*;

pub(super) fn header(kind: u32, next: *mut ffxApiHeader) -> ffxApiHeader {
    ffxApiHeader {
        type_: u64::from(kind),
        pNext: next,
    }
}

/// Fills the backend and upscale create records, chaining `version` behind the backend.
pub(super) fn write_create_desc(
    setup: &Setup,
    flags: u32,
    version: *mut ffxApiHeader,
    backend: &mut ffxCreateBackendVKDesc,
    create: &mut ffxCreateContextDescUpscale,
) {
    *backend = ffxCreateBackendVKDesc {
        header: header(FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK, version),
        vkDevice: setup.device.cast(),
        vkPhysicalDevice: setup.physical_device.cast(),
        vkDeviceProcAddr: Some(sdk_device_proc),
    };
    let dimensions = FfxApiDimensions2D {
        width: setup.output_width,
        height: setup.output_height,
    };
    *create = ffxCreateContextDescUpscale {
        header: header(
            FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE,
            &raw mut backend.header,
        ),
        flags,
        maxRenderSize: dimensions,
        maxUpscaleSize: dimensions,
        fpMessage: None,
    };
}

pub(super) fn plan_desc(
    setup: &Setup,
    width: *mut u32,
    height: *mut u32,
) -> ffxQueryDescUpscaleGetRenderResolutionFromQualityMode {
    ffxQueryDescUpscaleGetRenderResolutionFromQualityMode {
        header: header(
            FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE,
            ptr::null_mut(),
        ),
        displayWidth: setup.output_width,
        displayHeight: setup.output_height,
        qualityMode: setup.quality,
        pOutRenderWidth: width,
        pOutRenderHeight: height,
    }
}

fn resource(image: &Image, output: bool) -> FfxApiResource {
    let vk_format = vk::Format::from_raw(image.format as i32);
    let format = match vk_format {
        vk::Format::R16G16B16A16_SFLOAT => FFX_API_SURFACE_FORMAT_R16G16B16A16_FLOAT,
        vk::Format::R16G16_SFLOAT => FFX_API_SURFACE_FORMAT_R16G16_FLOAT,
        vk::Format::R32_SFLOAT | vk::Format::D32_SFLOAT => FFX_API_SURFACE_FORMAT_R32_FLOAT,
        vk::Format::R8_UNORM => FFX_API_SURFACE_FORMAT_R8_UNORM,
        _ => FFX_API_SURFACE_FORMAT_UNKNOWN,
    };
    let usage = if vk_format == vk::Format::D32_SFLOAT {
        FFX_API_RESOURCE_USAGE_DEPTHTARGET
    } else if output {
        FFX_API_RESOURCE_USAGE_UAV
    } else {
        FFX_API_RESOURCE_USAGE_READ_ONLY
    };
    FfxApiResource {
        resource: ptr::with_exposed_provenance_mut(image.image as usize),
        description: FfxApiResourceDescription {
            type_: FFX_API_RESOURCE_TYPE_TEXTURE2D as u32,
            format: format as u32,
            __bindgen_anon_1: FfxApiResourceDescription__bindgen_ty_1 { width: image.width },
            __bindgen_anon_2: FfxApiResourceDescription__bindgen_ty_2 {
                height: image.height,
            },
            __bindgen_anon_3: FfxApiResourceDescription__bindgen_ty_3 { depth: 1 },
            mipCount: 1,
            flags: 0,
            usage: usage as u32,
        },
        state: if output {
            FFX_API_RESOURCE_STATE_UNORDERED_ACCESS
        } else {
            FFX_API_RESOURCE_STATE_COMPUTE_READ
        } as u32,
    }
}

/// # Safety
/// `call.frame.record` must be the record Vulkan SR admission validated for this evaluation.
pub(super) unsafe fn dispatch_desc(
    commands: Handle,
    call: &FfxDispatch<'_>,
) -> ffxDispatchDescUpscale {
    let frame = call.frame;
    // Vulkan SR admission validated this borrowed record for the evaluation.
    let record = unsafe { &*frame.record };
    let [jitter_x, jitter_y] = record.camera.jitter_pixels;
    let (enable_sharpening, sharpness) = ffx_sharpening(call.sharpness);
    ffxDispatchDescUpscale {
        header: header(FFX_API_DISPATCH_DESC_TYPE_UPSCALE, ptr::null_mut()),
        commandList: commands,
        color: resource(&frame.color, false),
        depth: resource(&frame.depth, false),
        motionVectors: resource(&frame.motion, false),
        reactive: resource(&call.reactive, false),
        transparencyAndComposition: resource(&call.composition, false),
        output: resource(&call.output, true),
        jitterOffset: FfxApiFloatCoords2D {
            x: jitter_x,
            y: jitter_y,
        },
        motionVectorScale: FfxApiFloatCoords2D {
            x: frame.motion_scale_x,
            y: frame.motion_scale_y,
        },
        preExposure: frame.pre_exposure,
        renderSize: FfxApiDimensions2D {
            width: record.render_width,
            height: record.render_height,
        },
        upscaleSize: FfxApiDimensions2D {
            width: record.output_width,
            height: record.output_height,
        },
        frameTimeDelta: record.frame_time_ms,
        cameraNear: record.camera.near_plane,
        cameraFar: call.far_plane,
        cameraFovAngleVertical: record.camera.vertical_fov_radians,
        viewSpaceToMetersFactor: 1.0,
        enableSharpening: enable_sharpening,
        sharpness,
        reset: call.reset != 0,
        ..ffxDispatchDescUpscale::default()
    }
}
