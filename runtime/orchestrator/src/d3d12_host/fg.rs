// SPDX-License-Identifier: MIT
use super::*;
use crate::sys::rsf_fg12_texture_desc;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain3;
use windows::core::IUnknown;

pub(crate) unsafe extern "C" fn rsf_fg12_platform_buffer_index(chain: Raw) -> u32 {
    unsafe { IUnknown::from_raw_borrowed(&chain) }
        .and_then(|chain| receive::<IDXGISwapChain3>(|iid, out| unsafe { chain.query(iid, out) }))
        .map_or(u32::MAX, |chain| unsafe {
            chain.GetCurrentBackBufferIndex()
        })
}

pub(crate) unsafe extern "C" fn rsf_fg12_platform_texture_desc(
    resource: Raw,
    output: *mut rsf_fg12_texture_desc,
) {
    let desc = unsafe { borrow::<ID3D12Resource>(&resource).GetDesc() };
    unsafe {
        output.write(rsf_fg12_texture_desc {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
        })
    };
}

fn same_shape(a: &D3D12_RESOURCE_DESC, b: &D3D12_RESOURCE_DESC) -> bool {
    a.Dimension == b.Dimension
        && a.Alignment == b.Alignment
        && a.Width == b.Width
        && a.Height == b.Height
        && a.DepthOrArraySize == b.DepthOrArraySize
        && a.MipLevels == b.MipLevels
        && a.Format == b.Format
        && a.Layout == b.Layout
        && a.SampleDesc == b.SampleDesc
}

pub(crate) unsafe extern "C" fn rsf_fg12_platform_texture(
    list: Raw,
    resource: Raw,
    reuse: Raw,
) -> Raw {
    let mut desc = unsafe { borrow::<ID3D12Resource>(&resource).GetDesc() };
    let Some(device) = device_of(unsafe { borrow::<ID3D12GraphicsCommandList>(&list) }) else {
        return null_mut();
    };
    if let Some(previous) = unsafe { ID3D12Resource::from_raw_borrowed(&reuse) }
        && same_shape(&desc, &unsafe { previous.GetDesc() })
        && owned_by(previous, device.as_raw())
    {
        return previous.clone().into_raw();
    }
    desc.Flags = D3D12_RESOURCE_FLAG_NONE;
    committed_resource(
        &device,
        D3D12_HEAP_TYPE_DEFAULT,
        &desc,
        D3D12_RESOURCE_STATE_COMMON,
    )
}

pub(crate) unsafe extern "C" fn rsf_fg12_platform_copy(
    list: Raw,
    source: Raw,
    source_state: u32,
    target: Raw,
    target_state: u32,
) {
    unsafe {
        rsf_core_d3d12_copy_transitioned(
            list,
            target,
            target_state,
            D3D12_RESOURCE_STATE_COMMON.0 as u32,
            source,
            source_state,
            source_state,
        )
    };
}

pub(crate) unsafe extern "C" fn rsf_fg12_platform_transition(
    list: Raw,
    resource: Raw,
    before: u32,
    after: u32,
) {
    unsafe { rsf_core_d3d12_transition(list, resource, before, after) };
}

pub(crate) unsafe extern "C" fn rsf_fg12_platform_fence(queue: Raw) -> Raw {
    let Some(device) = device_of(unsafe { borrow::<ID3D12CommandQueue>(&queue) }) else {
        return null_mut();
    };
    receive::<ID3D12Fence>(|iid, out| unsafe {
        (device.vtable().CreateFence)(device.as_raw(), 0, D3D12_FENCE_FLAG_NONE, iid, out)
    })
    .map_or(null_mut(), Interface::into_raw)
}

pub(crate) unsafe extern "C" fn rsf_fg12_platform_signal(
    queue: Raw,
    fence: Raw,
    value: u64,
) -> i32 {
    let queue: &ID3D12CommandQueue = unsafe { borrow(&queue) };
    let fence: &ID3D12Fence = unsafe { borrow(&fence) };
    i32::from(unsafe { queue.Signal(fence, value) }.is_ok())
}
