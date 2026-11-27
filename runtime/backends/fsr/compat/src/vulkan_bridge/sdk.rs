// SPDX-License-Identifier: MIT

use super::{
    Context, DEPTH_INFINITE, DEPTH_INVERTED, Dispatch, FAILED, Handle, MAX_VERSIONS,
    MOTION_JITTERED, Version, ffx_version,
};
use crate::ffx_abi::*;
use core::ptr;

/// The directory of the module holding this bridge, with its trailing separator.
pub(super) fn own_directory() -> Option<Vec<u16>> {
    use windows::{
        Win32::{
            Foundation::{HMODULE, MAX_PATH},
            System::LibraryLoader::{
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleHandleExW,
            },
        },
        core::PCWSTR,
    };

    let mut module = HMODULE::default();
    let address = PCWSTR(own_directory as *const () as *const u16);
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address,
            &mut module,
        )
    }
    .is_err()
    {
        return None;
    }
    let mut path = [0_u16; MAX_PATH as usize];
    let name = unsafe { rsf_game_sys::win32::module_path_in(module.0, &mut path) }?;
    let separator = name
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\') || unit == u16::from(b'/'))?;
    Some(name[..=separator].to_vec())
}

// Function-like FidelityFX macros of the pinned 2.4.0 headers; see
// docs/research/rust-vulkan-ffx-v05.md.
const UPSCALE_CREATE: u32 = 0x10000;
const UPSCALE_DISPATCH: u32 = 0x10001;
const UPSCALE_PLAN: u32 = 0x10003;
const UPSCALE_VERSION: u32 = 0x1000b;

fn header(kind: u32, next: *mut ffxApiHeader) -> ffxApiHeader {
    ffxApiHeader {
        type_: u64::from(kind),
        pNext: next,
    }
}

pub(super) unsafe fn versions(
    query: PfnFfxQuery,
    device: Handle,
    output: *mut Version,
    count: &mut u64,
) -> i32 {
    let Some(query) = query else { return FAILED };
    let capacity = *count;
    if capacity > MAX_VERSIONS as u64 {
        return FAILED;
    }
    let mut ids = [0; MAX_VERSIONS];
    let mut names = [ptr::null(); MAX_VERSIONS];
    let mut request = ffxQueryDescGetVersions {
        header: header(FFX_API_QUERY_DESC_TYPE_GET_VERSIONS, ptr::null_mut()),
        createDescType: u64::from(UPSCALE_CREATE),
        device,
        outputCount: count,
        versionIds: ids.as_mut_ptr(),
        versionNames: names.as_mut_ptr(),
    };
    let result = unsafe { query(ptr::null_mut(), &raw mut request.header) } as i32;
    if result == 0 {
        // The owner rejects an overflow count after the call.
        let stored = (*count).min(capacity) as usize;
        for (index, (&id, &name)) in ids.iter().zip(&names).take(stored).enumerate() {
            unsafe { output.add(index).write(ffx_version::from_sdk(id, name)) };
        }
    }
    result
}

pub(super) unsafe fn provider(
    query: PfnFfxQuery,
    effect: *mut ffxContext,
    output: &mut Version,
    parsed: &mut bool,
) -> i32 {
    let Some(query) = query else { return FAILED };
    let mut desc = ffxQueryGetProviderVersion {
        header: header(
            FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION,
            ptr::null_mut(),
        ),
        ..Default::default()
    };
    let result = unsafe { query(effect, &raw mut desc.header) } as i32;
    if result == 0 {
        let (version, numeric) =
            unsafe { ffx_version::parse_sdk(desc.versionId, desc.versionName) };
        *output = version;
        *parsed = numeric;
    }
    result
}

pub(super) unsafe fn create(
    create: PfnFfxCreateContext,
    effect: *mut ffxContext,
    backend: *mut ffxApiHeader,
    api_version: u32,
    setup: &Context,
) -> i32 {
    let Some(create) = create else { return FAILED };
    let mut flags = FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE | FFX_UPSCALE_ENABLE_AUTO_EXPOSURE;
    if setup.flags & DEPTH_INVERTED != 0 {
        flags |= FFX_UPSCALE_ENABLE_DEPTH_INVERTED;
    }
    if setup.flags & DEPTH_INFINITE != 0 {
        flags |= FFX_UPSCALE_ENABLE_DEPTH_INFINITE;
    }
    if setup.flags & MOTION_JITTERED != 0 {
        flags |= FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION;
    }
    let mut selection = ffxOverrideVersion {
        header: header(FFX_API_DESC_TYPE_OVERRIDE_VERSION, ptr::null_mut()),
        versionId: setup.provider,
    };
    let mut version = ffxCreateContextDescUpscaleVersion {
        header: header(UPSCALE_VERSION, &raw mut selection.header),
        version: api_version,
    };
    unsafe { (*backend).pNext = &raw mut version.header };
    let mut desc = ffxCreateContextDescUpscale {
        header: header(UPSCALE_CREATE, backend),
        flags: flags as u32,
        maxRenderSize: FfxApiDimensions2D {
            width: setup.render_width,
            height: setup.render_height,
        },
        maxUpscaleSize: FfxApiDimensions2D {
            width: setup.output_width,
            height: setup.output_height,
        },
        ..Default::default()
    };
    let result = unsafe { create(effect, &raw mut desc.header, ptr::null()) } as i32;
    unsafe { (*backend).pNext = ptr::null_mut() };
    result
}

pub(super) unsafe fn plan(
    query: PfnFfxQuery,
    effect: *mut ffxContext,
    quality: u32,
    display_width: u32,
    display_height: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(query) = query else { return FAILED };
    let mut desc = ffxQueryDescUpscaleGetRenderResolutionFromQualityMode {
        header: header(UPSCALE_PLAN, ptr::null_mut()),
        displayWidth: display_width,
        displayHeight: display_height,
        qualityMode: quality,
        pOutRenderWidth: width,
        pOutRenderHeight: height,
    };
    unsafe { query(effect, &raw mut desc.header) as i32 }
}

/// The dispatch descriptor; the COM leaf adds resource descriptions from the
/// actual D3D12 images.
pub(super) fn dispatch(frame: &Dispatch, commands: Handle) -> ffxDispatchDescUpscale {
    ffxDispatchDescUpscale {
        header: header(UPSCALE_DISPATCH, ptr::null_mut()),
        commandList: commands,
        jitterOffset: FfxApiFloatCoords2D {
            x: frame.jitter_x,
            y: frame.jitter_y,
        },
        motionVectorScale: FfxApiFloatCoords2D {
            x: frame.motion_scale_x,
            y: frame.motion_scale_y,
        },
        renderSize: FfxApiDimensions2D {
            width: frame.render_width,
            height: frame.render_height,
        },
        upscaleSize: FfxApiDimensions2D {
            width: frame.output_width,
            height: frame.output_height,
        },
        // The owner admits finite values in [0, 1] and initializes to zero.
        enableSharpening: frame.sharpness > 0.0,
        sharpness: frame.sharpness,
        frameTimeDelta: frame.frame_time_ms,
        preExposure: frame.pre_exposure,
        reset: frame.reset != 0,
        cameraNear: frame.near_plane,
        cameraFar: frame.far_plane,
        cameraFovAngleVertical: frame.vertical_fov,
        viewSpaceToMetersFactor: 1.0,
        ..Default::default()
    }
}
