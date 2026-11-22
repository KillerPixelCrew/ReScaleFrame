// SPDX-License-Identifier: MIT

use super::{
    BridgeCreate, BridgeFrame, Handle, INVALID, MAX_VERSIONS, Record, UNSUPPORTED, Version,
    ffx_version,
};
use std::ptr;

#[path = "sdk_abi.rs"]
mod abi;
use abi::*;

#[path = "native.rs"]
mod native;
#[path = "../dx12_provider_module.rs"]
mod provider_module;
use windows::Win32::Foundation::FARPROC;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_load_provider(
    path: *const u16,
    out: &mut Handle,
    create: &mut PfnFfxCreateContext,
    destroy: &mut PfnFfxDestroyContext,
    configure: &mut PfnFfxConfigure,
    dispatch: &mut PfnFfxDispatch,
    query: &mut PfnFfxQuery,
) -> i32 {
    let module = match unsafe { provider_module::load(path) } {
        Ok(module) => module,
        Err(error) => return error.code().0,
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
        *configure = core::mem::transmute::<FARPROC, PfnFfxConfigure>(provider_module::export(
            module,
            c"ffxConfigure",
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
    if [
        create.is_some(),
        destroy.is_some(),
        configure.is_some(),
        dispatch.is_some(),
        query.is_some(),
    ]
    .into_iter()
    .all(|bound| bound)
    {
        0
    } else {
        windows::Win32::Foundation::E_NOINTERFACE.0
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_release_provider_objects(
    graphics: &super::commands::Graphics,
    module: Handle,
) {
    let objects = [
        graphics.commands,
        graphics.allocator,
        graphics.completion,
        graphics.shared,
        graphics.queue,
        graphics.device,
        graphics.adapter,
        graphics.factory,
    ];
    unsafe { provider_module::release(&objects, graphics.event, module) };
}

// Bindgen skips the SDK's function-like descriptor macros. These are the pinned
// 2.4.0 values; docs/research/rust-vulkan-ffx-v05.md records how they were read.
const FG_CREATE: u32 = 0x20001;
const FG_CONFIGURE: u32 = 0x20002;
const FG_PREPARE_V2: u32 = 0x2000c;
const FG_VERSION: u32 = 0x2000e;
const SWAPCHAIN_WAIT_PRESENTS: u32 = 0x30007;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_sdk_wait_presents(
    dispatch: PfnFfxDispatch,
    context: *mut ffxContext,
) -> i32 {
    let Some(dispatch) = dispatch else {
        return INVALID;
    };
    let wait = header(SWAPCHAIN_WAIT_PRESENTS);
    unsafe { dispatch(context, &wait) as i32 }
}

fn header(type_: u32) -> ffxApiHeader {
    ffxApiHeader {
        type_: u64::from(type_),
        pNext: ptr::null_mut(),
    }
}

/// Queries candidates on the matching DX12 device without creating a context.
/// The SDK owns the returned names for the duration of provider selection.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_sdk_versions(
    query: PfnFfxQuery,
    device: Handle,
    output: *mut Version,
    count: *mut u64,
) -> i32 {
    let Some(query) = query else { return INVALID };
    if device.is_null() || output.is_null() || count.is_null() {
        return INVALID;
    }
    let capacity = unsafe { count.read() };
    if capacity > MAX_VERSIONS as u64 {
        return INVALID;
    }
    let mut ids = [0; MAX_VERSIONS];
    let mut names = [ptr::null(); MAX_VERSIONS];
    let mut request = ffxQueryDescGetVersions {
        header: header(FFX_API_QUERY_DESC_TYPE_GET_VERSIONS),
        createDescType: u64::from(FG_CREATE),
        device,
        outputCount: count,
        versionIds: ids.as_mut_ptr(),
        versionNames: names.as_mut_ptr(),
    };
    if unsafe { query(ptr::null_mut(), &raw mut request.header) } != 0 {
        return UNSUPPORTED;
    }
    let actual = unsafe { count.read() };
    if actual > capacity {
        return UNSUPPORTED;
    }
    for (index, (&id, &name)) in ids.iter().zip(&names).take(actual as usize).enumerate() {
        unsafe { output.add(index).write(ffx_version::from_sdk(id, name)) };
    }
    0
}

/// Writes the retained generation descriptors in their final native storage.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_sdk_generation(
    setup: &BridgeCreate,
    provider: u64,
    api_version: u32,
    format: u32,
    backend: *mut ffxApiHeader,
    selection: *mut ffxOverrideVersion,
    version: *mut ffxCreateContextDescFrameGenerationVersion,
    description: *mut ffxCreateContextDescFrameGeneration,
) {
    let dimensions = FfxApiDimensions2D {
        width: setup.width,
        height: setup.height,
    };
    let flags = |enabled: u32, flag: FfxApiCreateContextFramegenerationFlags| {
        if enabled != 0 { flag as u32 } else { 0 }
    };
    unsafe {
        selection.write(ffxOverrideVersion {
            header: header(FFX_API_DESC_TYPE_OVERRIDE_VERSION),
            versionId: provider,
        });
        (*backend).pNext = &raw mut (*selection).header;
        version.write(ffxCreateContextDescFrameGenerationVersion {
            header: ffxApiHeader {
                pNext: backend,
                ..header(FG_VERSION)
            },
            version: api_version,
        });
        description.write(ffxCreateContextDescFrameGeneration {
            header: ffxApiHeader {
                pNext: &raw mut (*version).header,
                ..header(FG_CREATE)
            },
            flags: flags(
                setup.depth_inverted,
                FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED,
            ) | flags(
                setup.depth_infinite,
                FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE,
            ) | flags(
                setup.motion_jittered,
                FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION,
            ),
            displaySize: dimensions,
            maxRenderSize: dimensions,
            backBufferFormat: format,
        });
    }
}

type DescribeResource = unsafe extern "C" fn(Handle, u32, *mut FfxApiResource);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_sdk_configure(
    swapchain: Handle,
    sequence: u64,
    width: u32,
    height: u32,
    enabled: u32,
    generate: FfxApiFrameGenerationDispatchFunc,
    context: Handle,
    effect: *mut ffxContext,
    configure: PfnFfxConfigure,
    hudless: Handle,
    describe: DescribeResource,
) -> i32 {
    let Some(configure) = configure else {
        return INVALID;
    };
    let mut desc = ffxConfigureDescFrameGeneration {
        header: header(FG_CONFIGURE),
        swapChain: swapchain,
        frameID: sequence,
        frameGenerationEnabled: enabled != 0,
        frameGenerationCallback: generate,
        frameGenerationCallbackUserContext: context,
        generationRect: FfxApiRect2D {
            left: 0,
            top: 0,
            width: width as i32,
            height: height as i32,
        },
        ..Default::default()
    };
    if enabled != 0 {
        unsafe {
            describe(
                hudless,
                FFX_API_RESOURCE_STATE_COMPUTE_READ as u32,
                &mut desc.HUDLessColor,
            )
        };
    }
    unsafe { configure(effect, &raw const desc.header) as i32 }
}

/// Queries the provider selected by the SDK on the live effect.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_sdk_provider(
    query: PfnFfxQuery,
    context: *mut ffxContext,
    version: *mut u64,
) -> i32 {
    let Some(query) = query else { return INVALID };
    if context.is_null() || version.is_null() {
        return INVALID;
    }
    let mut request = ffxQueryGetProviderVersion {
        header: header(FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION),
        ..Default::default()
    };
    let code = unsafe { query(context, &raw mut request.header) } as i32;
    if code == 0 {
        unsafe { version.write(request.versionId) };
    }
    code
}

fn prepare(
    frame: &BridgeFrame,
    record: &Record,
    sequence: u64,
    reset: bool,
) -> ffxDispatchDescFrameGenerationPrepareV2 {
    let camera = &record.camera;
    let world = &camera.view_to_world;
    let sign = if camera.view_to_clip[11] < 0.0 {
        -1.0
    } else {
        1.0
    };
    ffxDispatchDescFrameGenerationPrepareV2 {
        header: header(FG_PREPARE_V2),
        frameID: sequence,
        renderSize: FfxApiDimensions2D {
            width: record.render_width,
            height: record.render_height,
        },
        jitterOffset: FfxApiFloatCoords2D {
            x: camera.jitter_pixels[0],
            y: camera.jitter_pixels[1],
        },
        motionVectorScale: FfxApiFloatCoords2D {
            x: frame.motion_scale_x,
            y: frame.motion_scale_y,
        },
        frameTimeDelta: record.frame_time_ms,
        reset,
        cameraNear: camera.near_plane,
        cameraFar: camera.far_plane,
        cameraFovAngleVertical: camera.vertical_fov_radians,
        viewSpaceToMetersFactor: 1.0,
        cameraPosition: [world[12], world[13], world[14]],
        cameraUp: [world[4], world[5], world[6]],
        cameraRight: [world[0], world[1], world[2]],
        cameraForward: [sign * world[8], sign * world[9], sign * world[10]],
        ..Default::default()
    }
}

/// Records the frame preparation on the command list loaned by the capsule.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_sdk_prepare(
    frame: &BridgeFrame,
    sequence: u64,
    reset: u32,
    commands: Handle,
    effect: *mut ffxContext,
    dispatch: PfnFfxDispatch,
    describe: DescribeResource,
) -> i32 {
    let Some(dispatch) = dispatch else {
        return INVALID;
    };
    let record = unsafe { &*frame.record };
    let mut desc = prepare(frame, record, sequence, reset != 0);
    desc.commandList = commands;
    let read = FFX_API_RESOURCE_STATE_COMPUTE_READ as u32;
    unsafe {
        describe(frame.depth, read, &mut desc.depth);
        describe(frame.motion, read, &mut desc.motionVectors);
        dispatch(effect, &raw const desc.header) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsr4_fg_bridge::{ABI_MISMATCH, BRIDGE_ABI, check, select_version};
    use std::cell::Cell;

    thread_local! {
        static MODE: Cell<u32> = const { Cell::new(0) };
    }

    fn fake_handle(value: usize) -> Handle {
        ptr::without_provenance_mut(value)
    }

    unsafe extern "C" fn query(
        context: *mut ffxContext,
        input: *mut ffxQueryDescHeader,
    ) -> ffxReturnCode_t {
        let mode = MODE.get();
        unsafe {
            if (*input).type_ == u64::from(FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION) {
                assert!(!context.is_null());
                (*input.cast::<ffxQueryGetProviderVersion>()).versionId =
                    if mode == 4 { 3 } else { 42 };
                return u32::from(mode == 5);
            }
            assert!(context.is_null());
            assert_eq!(
                (*input).type_,
                u64::from(FFX_API_QUERY_DESC_TYPE_GET_VERSIONS)
            );
            let request = &mut *input.cast::<ffxQueryDescGetVersions>();
            assert_eq!(request.device, fake_handle(123));
            assert_eq!(request.createDescType, u64::from(FG_CREATE));
            if mode == 2 {
                return 1;
            }
            if mode == 3 {
                request.outputCount.write((MAX_VERSIONS + 1) as u64);
                return 0;
            }
            let versions = [
                (3, c"3.1.6"),
                (41, c"4.0.0"),
                (42, c"4.0.1"),
                (50, c"5.0.0"),
            ];
            let selected = if mode == 0 {
                &versions[..1]
            } else {
                &versions[..]
            };
            for (i, &(id, name)) in selected.iter().enumerate() {
                request.versionIds.add(i).write(id);
                request.versionNames.add(i).write(name.as_ptr());
            }
            request.outputCount.write(selected.len() as u64);
        }
        0
    }

    #[test]
    fn exact_provider_queries_preserve_selection_and_refusals() {
        let discover = |mode, query, device| {
            MODE.set(mode);
            let mut versions = [Version::default(); MAX_VERSIONS];
            let mut count = MAX_VERSIONS as u64;
            let code = unsafe {
                rsf_core_fsr4fg_sdk_versions(query, device, versions.as_mut_ptr(), &mut count)
            };
            if code != 0 {
                return Err(code);
            }
            match select_version(&versions, count) {
                0 => Err(UNSUPPORTED),
                id => Ok(id),
            }
        };
        let device = fake_handle(123);
        assert_eq!(discover(0, Some(query), device), Err(UNSUPPORTED));
        assert_eq!(discover(1, Some(query), device), Ok(42));
        let mut context = fake_handle(1);
        let mut version = 0;
        assert_eq!(
            unsafe { rsf_core_fsr4fg_sdk_provider(Some(query), &mut context, &mut version) },
            0
        );
        assert_eq!(version, 42);
        MODE.set(4);
        assert_eq!(
            unsafe { rsf_core_fsr4fg_sdk_provider(Some(query), &mut context, &mut version) },
            0
        );
        assert_eq!(version, 3);
        MODE.set(5);
        assert_eq!(
            unsafe { rsf_core_fsr4fg_sdk_provider(Some(query), &mut context, &mut version) },
            1
        );
        assert_eq!(version, 3);
        assert_eq!(discover(2, Some(query), device), Err(UNSUPPORTED));
        assert_eq!(discover(3, Some(query), device), Err(UNSUPPORTED));
        assert_eq!(discover(1, None, device), Err(INVALID));
        assert_eq!(discover(1, Some(query), ptr::null_mut()), Err(INVALID));
    }

    #[test]
    fn prepare_v2_keeps_camera_conventions_and_bridge_abi_validation() {
        let mut record: Record = unsafe { std::mem::zeroed() };
        record.frame_id = 12;
        record.render_width = 426;
        record.render_height = 240;
        record.frame_time_ms = 16.5;
        let camera = &mut record.camera;
        for index in (0..16).step_by(5) {
            camera.view_to_world[index] = 1.0;
        }
        camera.view_to_world[12..15].copy_from_slice(&[2.0, 3.0, 4.0]);
        camera.view_to_clip[11] = -1.0;
        camera.near_plane = 0.05;
        camera.far_plane = 4000.0;
        camera.vertical_fov_radians = 1.3;
        camera.jitter_pixels = [0.25, -0.125];
        let mut frame = BridgeFrame {
            struct_size: size_of::<BridgeFrame>() as u32,
            abi_version: BRIDGE_ABI,
            record: &raw const record,
            motion_scale_x: 426.0,
            motion_scale_y: 240.0,
            ..unsafe { std::mem::zeroed() }
        };
        let desc = prepare(&frame, &record, 70, false);
        assert_eq!(desc.header.type_, u64::from(FG_PREPARE_V2));
        assert!(desc.frameID == 70 && !desc.reset);
        assert_eq!([desc.renderSize.width, desc.renderSize.height], [426, 240]);
        assert_eq!(desc.cameraPosition, [2.0, 3.0, 4.0]);
        assert_eq!(desc.cameraRight, [1.0, 0.0, 0.0]);
        assert_eq!(desc.cameraUp, [0.0, 1.0, 0.0]);
        assert_eq!(desc.cameraForward, [0.0, 0.0, -1.0]);
        assert_eq!(desc.frameTimeDelta, 16.5);
        assert_eq!(
            [desc.motionVectorScale.x, desc.motionVectorScale.y],
            [426.0, 240.0]
        );
        assert_eq!([desc.jitterOffset.x, desc.jitterOffset.y], [0.25, -0.125]);
        assert_eq!(
            [
                desc.cameraNear,
                desc.cameraFar,
                desc.cameraFovAngleVertical,
                desc.viewSpaceToMetersFactor
            ],
            [0.05, 4000.0, 1.3, 1.0]
        );
        record.camera.view_to_clip[11] = 1.0;
        frame.record = &raw const record;
        let desc = prepare(&frame, &record, 71, true);
        assert_eq!(desc.frameID, 71);
        assert!(desc.reset && desc.cameraForward[2] == 1.0);
        frame.struct_size -= 1;
        frame.abi_version = 99;
        assert_eq!(unsafe { check(&frame) }, Err(INVALID));
        frame.struct_size += 1;
        assert_eq!(unsafe { check(&frame) }, Err(ABI_MISMATCH));
    }
}
