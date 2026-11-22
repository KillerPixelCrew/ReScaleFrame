// SPDX-License-Identifier: MIT
use super::{Dispatch, Handle, abi::*, dx12, rsf_core_fsr4sr_sdk_dispatch};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};

type Describe = unsafe extern "C" fn(Handle, u32, *mut FfxApiResource);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_shared_image(
    device: Handle,
    width: u32,
    height: u32,
    vk_format: u32,
    writable: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    let (format, mut flags) = match vk_format {
        97 => (DXGI_FORMAT_R16G16B16A16_FLOAT, D3D12_RESOURCE_FLAG_NONE),
        83 => (DXGI_FORMAT_R16G16_FLOAT, D3D12_RESOURCE_FLAG_NONE),
        100 => (DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_FLAG_NONE),
        126 => (
            DXGI_FORMAT_D32_FLOAT,
            D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        ),
        _ => return -2,
    };
    if writable != 0 {
        if flags != D3D12_RESOURCE_FLAG_NONE {
            return -3;
        }
        flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    }
    let device = unsafe { dx12::borrow(&device) };
    match dx12::create_shared_texture(device, width, height, format, flags) {
        Ok((shared, image)) => {
            unsafe {
                handle.write(shared);
                resource.write(image);
            }
            0
        }
        Err(error) => error.code().0,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_record(
    commands: Handle,
    allocator: Handle,
    frame: &Dispatch,
    context: *mut ffxContext,
    dispatch: PfnFfxDispatch,
    describe: Describe,
) -> i32 {
    let Some(dispatch) = dispatch else { return -1 };
    let list: &ID3D12GraphicsCommandList = unsafe { dx12::borrow(&commands) };
    let allocator: &ID3D12CommandAllocator = unsafe { dx12::borrow(&allocator) };
    if let Err(error) = unsafe { allocator.Reset().and_then(|()| list.Reset(allocator, None)) } {
        return error.code().0;
    }
    const COMMON: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COMMON;
    const READ: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    const WRITE: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    let resources = [
        (frame.color, READ),
        (frame.depth, READ),
        (frame.motion, READ),
        (frame.output, WRITE),
    ];
    for (image, state) in &resources {
        dx12::transition(list, unsafe { dx12::borrow(image) }, COMMON, *state);
    }

    let mut desc = ffxDispatchDescUpscale::default();
    unsafe {
        rsf_core_fsr4sr_sdk_dispatch(frame, commands, &mut desc);
        // FidelityFX owns the DXGI format and resource-description conventions.
        const COMPUTE_READ: u32 = FFX_API_RESOURCE_STATE_COMPUTE_READ as u32;
        const UNORDERED_ACCESS: u32 = FFX_API_RESOURCE_STATE_UNORDERED_ACCESS as u32;
        describe(frame.color, COMPUTE_READ, &mut desc.color);
        describe(frame.depth, COMPUTE_READ, &mut desc.depth);
        describe(frame.motion, COMPUTE_READ, &mut desc.motionVectors);
        describe(frame.output, UNORDERED_ACCESS, &mut desc.output);
    }
    let code = unsafe { dispatch(context, &raw const desc.header) } as i32;
    if code != 0 {
        let _ = unsafe { list.Close() };
        return code;
    }
    for (image, state) in &resources {
        dx12::transition(list, unsafe { dx12::borrow(image) }, *state, COMMON);
    }
    dx12::status(unsafe { list.Close() })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_execute(queue: Handle, commands: Handle) {
    unsafe { dx12::execute(queue, commands) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_queue_fence(
    queue: Handle,
    fence: Handle,
    value: u64,
    signal: u32,
) -> i32 {
    unsafe { dx12::queue_fence(queue, fence, value, signal != 0) }
}
