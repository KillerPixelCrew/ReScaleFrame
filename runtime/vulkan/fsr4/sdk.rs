// SPDX-License-Identifier: MIT

use super::{
    Context, DEPTH_INFINITE, DEPTH_INVERTED, Dispatch, FAILED, Handle, MAX_VERSIONS,
    MOTION_JITTERED, Version, ffx_version,
};
use core::ptr;

#[path = "sdk_abi.rs"]
mod abi;
use abi::*;

#[cfg(windows)]
#[path = "sdk_commands.rs"]
mod commands;
#[cfg(windows)]
#[path = "../dx12_bridge_commands.rs"]
mod dx12;
#[cfg(windows)]
#[path = "../dx12_provider_module.rs"]
mod provider_module;
// The lease entry points live in the archive's non-test native module.
#[cfg(all(windows, not(test)))]
#[path = "native.rs"]
mod native;
#[cfg(windows)]
use windows::Win32::Foundation::FARPROC;

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_own_directory(out: *mut u16, capacity: u32) -> u32 {
    use windows::{
        Win32::{
            Foundation::{HMODULE, MAX_PATH},
            System::LibraryLoader::{
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
                GetModuleHandleExW,
            },
        },
        core::PCWSTR,
    };

    let mut module = HMODULE::default();
    let address = PCWSTR(rsf_core_fsr4sr_own_directory as *const () as *const u16);
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address,
            &mut module,
        )
    }
    .is_err()
    {
        return 0;
    }
    let mut path = [0_u16; MAX_PATH as usize];
    let length = unsafe { GetModuleFileNameW(Some(module), &mut path) } as usize;
    if length == 0 || length >= path.len() {
        return 0;
    }
    let Some(separator) = path[..length]
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\') || unit == u16::from(b'/'))
    else {
        return 0;
    };
    let directory = separator + 1;
    if directory > capacity as usize {
        return 0;
    }
    unsafe { ptr::copy_nonoverlapping(path.as_ptr(), out, directory) };
    directory as u32
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_load_provider(
    path: *const u16,
    out: &mut Handle,
    create: &mut PfnFfxCreateContext,
    destroy: &mut PfnFfxDestroyContext,
    dispatch: &mut PfnFfxDispatch,
    query: &mut PfnFfxQuery,
) -> i32 {
    let Ok(module) = (unsafe { provider_module::load(path) }) else {
        return -7;
    };
    *out = module.0;
    // Each optional export uses its SDK signature on Windows x64.
    unsafe {
        *create = core::mem::transmute::<FARPROC, PfnFfxCreateContext>(provider_module::export(
            module,
            c"ffxCreateContext",
        ));
        *destroy = core::mem::transmute::<FARPROC, PfnFfxDestroyContext>(provider_module::export(
            module,
            c"ffxDestroyContext",
        ));
        *dispatch = core::mem::transmute::<FARPROC, PfnFfxDispatch>(provider_module::export(
            module,
            c"ffxDispatch",
        ));
        *query = core::mem::transmute::<FARPROC, PfnFfxQuery>(provider_module::export(
            module,
            c"ffxQuery",
        ));
    }
    if create.is_some() && destroy.is_some() && dispatch.is_some() && query.is_some() {
        0
    } else {
        -8
    }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_release_provider_objects(
    objects: &[Handle; 10],
    event: Handle,
    module: Handle,
) {
    unsafe { provider_module::release(objects, event, module) };
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_sdk_versions(
    query: PfnFfxQuery,
    device: Handle,
    output: *mut Version,
    count: *mut u64,
) -> i32 {
    let Some(query) = query else { return FAILED };
    let capacity = unsafe { count.read() };
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
        let stored = unsafe { count.read() }.min(capacity) as usize;
        for (index, (&id, &name)) in ids.iter().zip(&names).take(stored).enumerate() {
            unsafe { output.add(index).write(ffx_version::from_sdk(id, name)) };
        }
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_sdk_provider(
    query: PfnFfxQuery,
    effect: *mut ffxContext,
    output: *mut Version,
    parsed: *mut u32,
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
        unsafe {
            output.write(version);
            parsed.write(u32::from(numeric));
        }
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_sdk_create(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_sdk_plan(
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

/// The COM leaf writes resource descriptions from the actual D3D12 images.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_sdk_dispatch(
    frame: &Dispatch,
    commands: Handle,
    output: *mut ffxDispatchDescUpscale,
) {
    let desc = ffxDispatchDescUpscale {
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
    };
    unsafe { output.write(desc) };
}
