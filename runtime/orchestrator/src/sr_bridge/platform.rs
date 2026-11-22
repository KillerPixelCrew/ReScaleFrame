// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_d3d12::{
    arm_fence, borrow, close_commands, close_event, context_signal, context_wait, copy_resource,
    copy_transitioned, create_commands, create_event, create_output, create_texture12, execute,
    fence_d3d11, fence_d3d12, flush, query, queue_signal, queue_wait, reset_commands,
    surface_d3d11, surface_d3d12, surface_destroy, transition,
};
use crate::d3d12_fence::rsf_core_d3d12_wait_fence;
use crate::fg_leases::rsf_d3d12_fence_completed;
use crate::shared_surface::{
    ABI_VERSION, SurfaceSetup, rsf_shared_fence_create, rsf_shared_fence_destroy,
    rsf_shared_surface_create,
};
use crate::sr_session::*;
// The Rust definitions, not the declarations sr_session.h also provides through crate::sys.
use crate::sr_session::{
    rsf_sr_session_create, rsf_sr_session_destroy, rsf_sr_session_evaluate,
    rsf_sr_session_get_status, rsf_sr_session_select, rsf_sr_session_set_auto_exposure,
    rsf_sr_session_warm_up,
};
use std::ffi::c_char;
use std::ptr::null_mut;
use windows::Win32::Graphics::{
    Direct3D::D3D_FEATURE_LEVEL_11_0,
    Direct3D11::*,
    Direct3D12::*,
    Dxgi::{Common::*, IDXGIDevice},
};
use windows::core::Interface;

const _: () = {
    assert!(COLOR_FORMAT == DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32);
    assert!(MOTION_FORMAT == DXGI_FORMAT_R16G16_FLOAT.0 as u32);
    assert!(SCALAR_FORMAT == DXGI_FORMAT_R32_FLOAT.0 as u32);
    assert!(STATE_COMMON == D3D12_RESOURCE_STATE_COMMON.0 as u32);
    assert!(STATE_UNORDERED_ACCESS == D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32);
    assert!(STATE_SHADER_RESOURCE == D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32);
};

unsafe extern "C" {
    fn rsf_input_dump_poll();
    fn rsf_input_dump_requested() -> i32;
    fn rsf_input_dump_begin_record(record: *const rsf_frame_record, backend: u32) -> i32;
    fn rsf_input_dump_wants(record: *const rsf_frame_record) -> i32;
    fn rsf_input_dump_sr_frame(frame: *const rsf_sr_frame, backend: u32, sdr: u32);
    fn rsf_input_dump_result(
        record: *const rsf_frame_record,
        stage: *const c_char,
        backend: u32,
        result: i32,
    );
    fn rsf_input_dump_texture_record(
        context: Handle,
        record: *const rsf_frame_record,
        name: *const c_char,
        description: *const c_char,
        texture: Handle,
        view: u32,
        scale: f32,
    ) -> i32;
}

unsafe extern "C" fn session_create(setup: *const rsf_sr_session_setup, out: *mut Handle) -> i32 {
    unsafe { rsf_sr_session_create(setup, out.cast()) }
}
unsafe extern "C" fn session_select(
    session: Handle,
    backend: u32,
    quality: u32,
    version: u64,
) -> i32 {
    unsafe { rsf_sr_session_select(session.cast(), backend, quality, version) }
}
unsafe extern "C" fn session_set_auto_exposure(session: Handle, enabled: u32) -> i32 {
    unsafe { rsf_sr_session_set_auto_exposure(session.cast(), enabled) }
}
unsafe extern "C" fn session_evaluate(
    session: Handle,
    context: Handle,
    frame: *const rsf_sr_frame,
) -> i32 {
    unsafe { rsf_sr_session_evaluate(session.cast(), context, frame) }
}
unsafe extern "C" fn session_warm_up(
    session: Handle,
    context: Handle,
    frame: *const rsf_sr_frame,
) -> i32 {
    unsafe { rsf_sr_session_warm_up(session.cast(), context, frame) }
}
unsafe extern "C" fn session_get_status(
    session: Handle,
    status: *mut rsf_sr_session_status,
) -> i32 {
    unsafe { rsf_sr_session_get_status(session.cast(), status) }
}
unsafe extern "C" fn session_destroy(session: Handle) {
    unsafe { rsf_sr_session_destroy(session.cast()) };
}
unsafe extern "C" fn immediate_context(device: Handle) -> Handle {
    unsafe { borrow::<ID3D11Device>(&device).GetImmediateContext() }
        .map_or(null_mut(), Interface::into_raw)
}
unsafe extern "C" fn create_device12(device: Handle) -> Handle {
    let result = || -> windows::core::Result<_> {
        let dxgi = unsafe { borrow::<ID3D11Device>(&device) }.cast::<IDXGIDevice>()?;
        let adapter = unsafe { dxgi.GetAdapter() }?;
        let mut device = None::<ID3D12Device>;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }?;
        Ok(device.map_or(null_mut(), Interface::into_raw))
    };
    result().unwrap_or(null_mut())
}
unsafe extern "C" fn create_queue(device: Handle) -> Handle {
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    unsafe { borrow::<ID3D12Device>(&device).CreateCommandQueue::<ID3D12CommandQueue>(&desc) }
        .map_or(null_mut(), Interface::into_raw)
}
unsafe extern "C" fn create_texture(
    device: Handle,
    width: u32,
    height: u32,
    format: u32,
) -> Handle {
    unsafe {
        create_texture12(
            device,
            width,
            height,
            format,
            D3D12_RESOURCE_FLAG_NONE,
            D3D12_RESOURCE_STATE_COMMON,
        )
    }
}
unsafe extern "C" fn create_fence(device: Handle) -> Handle {
    unsafe { borrow::<ID3D12Device>(&device).CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }
        .map_or(null_mut(), Interface::into_raw)
}
unsafe extern "C" fn describe(texture: Handle, device: Handle, out: *mut TextureDesc) -> i32 {
    let texture = unsafe { borrow::<ID3D11Texture2D>(&texture) };
    let Ok(owner) = (unsafe { texture.GetDevice() }) else {
        return 0;
    };
    if owner.as_raw() != device {
        return 0;
    }
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
        out.write(TextureDesc {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
            samples: desc.SampleDesc.Count,
            array_size: desc.ArraySize,
        });
    }
    1
}
unsafe extern "C" fn same_context(context: Handle, context4: Handle) -> i32 {
    let found = unsafe { borrow::<ID3D11DeviceContext>(&context) }.cast::<ID3D11DeviceContext4>();
    i32::from(found.is_ok_and(|found| found.as_raw() == context4))
}
unsafe extern "C" fn wait_fence(fence: Handle, value: u64, event: Handle, timeout_ms: u32) -> i32 {
    i32::from(
        unsafe {
            rsf_core_d3d12_wait_fence(
                fence,
                value,
                event,
                timeout_ms,
                rsf_d3d12_fence_completed,
                arm_fence,
            )
        } >= 0,
    )
}
unsafe extern "C" fn copy_region(
    context: Handle,
    destination: Handle,
    source: Handle,
    width: u32,
    height: u32,
) {
    let rect = D3D11_BOX {
        right: width,
        bottom: height,
        back: 1,
        ..Default::default()
    };
    unsafe {
        borrow::<ID3D11DeviceContext4>(&context).CopySubresourceRegion(
            borrow::<ID3D11Resource>(&destination),
            0,
            0,
            0,
            0,
            borrow::<ID3D11Resource>(&source),
            0,
            Some(&rect),
        );
    }
}
unsafe extern "C" fn submit(queue: Handle, list: Handle) -> i32 {
    if unsafe { close_commands(list) } == 0 {
        return 0;
    }
    unsafe { execute(queue, list) };
    1
}
unsafe extern "C" fn device_ready(device: Handle) -> i32 {
    i32::from(unsafe { borrow::<ID3D12Device>(&device).GetDeviceRemovedReason() }.is_ok())
}
unsafe extern "C" fn surface_create(
    device11: Handle,
    device12: Handle,
    width: u32,
    height: u32,
    format: u32,
    log: Log,
    log_user: Handle,
) -> Handle {
    let setup = SurfaceSetup {
        struct_size: size_of::<SurfaceSetup>() as u32,
        abi_version: ABI_VERSION,
        width,
        height,
        format,
        render_target: 0,
        log,
        log_user,
        unordered_access: 0,
    };
    let mut surface = null_mut();
    if unsafe { rsf_shared_surface_create(device11, device12, &setup, &mut surface) } == 0 {
        surface.cast()
    } else {
        null_mut()
    }
}
unsafe extern "C" fn fence_create(
    device11: Handle,
    device12: Handle,
    log: Log,
    user: Handle,
) -> Handle {
    let mut fence = null_mut();
    if unsafe { rsf_shared_fence_create(device11, device12, log, user, &mut fence) } == 0 {
        fence.cast()
    } else {
        null_mut()
    }
}
unsafe extern "C" fn fence_destroy(fence: Handle) {
    unsafe { rsf_shared_fence_destroy(fence.cast()) };
}
unsafe extern "C" fn dump_poll_requested() -> i32 {
    unsafe {
        rsf_input_dump_poll();
        rsf_input_dump_requested()
    }
}
const RAW: u32 = 0;
const HDR: u32 = 3;
const DUMP_DEPTH: u32 = 4;
const SCALAR: u32 = 5;
const DENSE_MOTION: u32 = 7;
unsafe extern "C" fn dump_inputs(
    context: Handle,
    frame: *const rsf_sr_frame,
    backend: u32,
    hdr: u32,
) {
    let frame = unsafe { &*frame };
    if unsafe { rsf_input_dump_begin_record(frame.record, backend) } == 0 {
        return;
    }
    let colour = if hdr != 0 { HDR } else { RAW };
    unsafe { rsf_input_dump_sr_frame(frame, backend, u32::from(hdr == 0)) };
    let inputs = [
        (
            c"sr_source_jittered",
            c"D3D11 SR source colour",
            frame.color.resource,
            colour,
            1.0,
        ),
        (
            c"sr_color",
            c"D3D11 SR backend colour",
            frame.color.resource,
            colour,
            1.0,
        ),
        (
            c"sr_depth",
            c"D3D11 SR device depth",
            frame.depth.resource,
            DUMP_DEPTH,
            1.0,
        ),
        (
            c"sr_motion",
            c"D3D11 dense pixel displacement",
            frame.motion.resource,
            DENSE_MOTION,
            1.0 / 32.0,
        ),
        (
            c"sr_exposure",
            c"D3D11 SR exposure",
            frame.exposure.resource,
            SCALAR,
            1.0,
        ),
    ];
    for (name, description, texture, view, scale) in inputs {
        unsafe {
            rsf_input_dump_texture_record(
                context,
                frame.record,
                name.as_ptr(),
                description.as_ptr(),
                texture,
                view,
                scale,
            )
        };
    }
}
unsafe extern "C" fn dump_result(
    context: Handle,
    frame: *const rsf_sr_frame,
    backend: u32,
    result: i32,
    hdr: u32,
) {
    let frame = unsafe { &*frame };
    unsafe { rsf_input_dump_result(frame.record, c"sr".as_ptr(), backend, result) };
    if result == OK {
        unsafe {
            rsf_input_dump_texture_record(
                context,
                frame.record,
                c"sr_output".as_ptr(),
                c"Reconstructed D3D11 scene before tonemapping".as_ptr(),
                frame.output.resource,
                if hdr != 0 { HDR } else { RAW },
                1.0,
            )
        };
    }
}

pub(super) static WINDOWS: Platform = Platform {
    session_create,
    session_select,
    session_set_auto_exposure,
    session_evaluate,
    session_warm_up,
    session_get_status,
    session_destroy,
    validate_frame: rsf_core_validate_sr_frame,
    immediate_context,
    query_context4: query::<ID3D11DeviceContext, ID3D11DeviceContext4>,
    create_device12,
    create_queue,
    create_commands,
    create_output,
    create_texture,
    create_fence,
    create_event,
    close_event,
    describe,
    same_context,
    wait_fence,
    flush,
    copy_region,
    copy_resource,
    context_signal,
    context_wait,
    reset_commands,
    transition,
    copy_transitioned,
    submit,
    queue_wait,
    queue_signal,
    device_ready,
    surface_create,
    surface_destroy,
    surface_d3d11,
    surface_d3d12,
    fence_create,
    fence_destroy,
    fence_d3d11,
    fence_d3d12,
    dump_poll_requested,
    dump_inputs,
    dump_wants: rsf_input_dump_wants,
    dump_result,
};
