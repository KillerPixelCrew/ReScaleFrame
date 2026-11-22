// SPDX-License-Identifier: MIT
use super::*;
use ::windows::Win32::{
    Foundation::{HANDLE, HWND},
    Graphics::{
        Direct3D12::*,
        Dxgi::{Common::*, *},
    },
};
use ::windows::core::{IUnknown, Interface};
use rsf_runtime_submission::{FenceStatus, fence_status};
use std::mem::ManuallyDrop;
use windows_sys::Win32::{
    Foundation::{CloseHandle, S_OK},
    System::Threading::CreateEventW,
};

const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
const BUFFER_COUNT: u32 = SLOTS as u32;

// Each leaf receives a live reference from the caller or graphics owner.
unsafe fn borrow<T: Interface>(raw: &*mut c_void) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live graphics reference")
}

unsafe extern "C" fn bind(device: *mut c_void, queue: *mut c_void, out: *mut Gpu) -> bool {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    let queue: &ID3D12CommandQueue = unsafe { borrow(&queue) };
    if unsafe { queue.GetDesc() }.Type != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return false;
    }
    let mut queue_device: Option<ID3D12Device> = None;
    if unsafe { queue.GetDevice(&mut queue_device) }.is_err()
        || queue_device.as_ref() != Some(device)
    {
        return false;
    }
    let Ok(fence) = (unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }) else {
        return false;
    };
    unsafe {
        out.write(Gpu {
            device: device.clone().into_raw(),
            queue: queue.clone().into_raw(),
            fence: fence.into_raw(),
            chain: ptr::null_mut(),
        })
    };
    true
}

unsafe extern "C" fn create_chain(
    gpu: *const Gpu,
    hwnd: *mut c_void,
    width: u32,
    height: u32,
    out: *mut *mut c_void,
) -> bool {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: FORMAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferCount: BUFFER_COUNT,
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        ..Default::default()
    };
    let opened = unsafe {
        let queue: &ID3D12CommandQueue = borrow(&(*gpu).queue);
        CreateDXGIFactory1::<IDXGIFactory2>()
            .and_then(|factory| {
                factory.CreateSwapChainForHwnd(queue, HWND(hwnd), &desc, None, None::<&IDXGIOutput>)
            })
            .and_then(|chain| chain.cast::<IDXGISwapChain3>())
    };
    let Ok(chain) = opened else {
        return false;
    };
    unsafe { out.write(chain.into_raw()) };
    true
}

unsafe extern "C" fn extent(chain: *mut c_void, width: *mut u32, height: *mut u32) -> bool {
    let chain: &IDXGISwapChain3 = unsafe { borrow(&chain) };
    let Ok(desc) = (unsafe { chain.GetDesc1() }) else {
        return false;
    };
    unsafe {
        width.write(desc.Width);
        height.write(desc.Height);
    }
    true
}

unsafe extern "C" fn resize(chain: *mut c_void, width: u32, height: u32) -> bool {
    let chain: &IDXGISwapChain3 = unsafe { borrow(&chain) };
    unsafe { chain.ResizeBuffers(BUFFER_COUNT, width, height, FORMAT, DXGI_SWAP_CHAIN_FLAG(0)) }
        .is_ok()
}

unsafe extern "C" fn describe(source: *mut c_void, output: *mut Texture) {
    let source: &ID3D12Resource = unsafe { borrow(&source) };
    let desc = unsafe { source.GetDesc() };
    let compatible = desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && desc.DepthOrArraySize == 1
        && desc.MipLevels == 1
        && desc.SampleDesc.Count == 1
        && desc.SampleDesc.Quality == 0
        && matches!(
            desc.Format,
            DXGI_FORMAT_R8G8B8A8_UNORM
                | DXGI_FORMAT_R8G8B8A8_TYPELESS
                | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        );
    unsafe {
        output.write(Texture {
            width: desc.Width,
            height: desc.Height,
            compatible,
        })
    };
}

unsafe extern "C" fn completed(fence: *mut c_void) -> u64 {
    let fence: &ID3D12Fence = unsafe { borrow(&fence) };
    unsafe { fence.GetCompletedValue() }
}

unsafe extern "C" fn wait(fence: *mut c_void, value: u64) -> bool {
    let fence: &ID3D12Fence = unsafe { borrow(&fence) };
    match fence_status(unsafe { fence.GetCompletedValue() }, value) {
        FenceStatus::Complete => return true,
        FenceStatus::Removed => return false,
        FenceStatus::Pending => {}
    }
    let event = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
    if event.is_null() {
        return false;
    }
    let result = crate::d3d12_fence::wait_for_value(
        value,
        10_000,
        || unsafe { fence.GetCompletedValue() },
        || {
            unsafe { fence.SetEventOnCompletion(value, HANDLE(event)) }
                .map_or_else(|e| e.code().0, |()| S_OK)
        },
        |remaining| unsafe { crate::d3d12_fence::wait_event(event, remaining) },
    );
    unsafe { CloseHandle(event) };
    result == S_OK
}

unsafe extern "C" fn acquire(chain: *mut c_void, output: *mut *mut c_void) -> bool {
    let chain: &IDXGISwapChain3 = unsafe { borrow(&chain) };
    let Ok(buffer) =
        (unsafe { chain.GetBuffer::<ID3D12Resource>(chain.GetCurrentBackBufferIndex()) })
    else {
        return false;
    };
    unsafe { output.write(buffer.into_raw()) };
    true
}

unsafe extern "C" fn create_slot(device: *mut c_void, output: *mut Slot) -> bool {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    let Ok(allocator) = (unsafe {
        device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
    }) else {
        return false;
    };
    let Ok(list) = (unsafe {
        device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &allocator,
            None::<&ID3D12PipelineState>,
        )
    }) else {
        return false;
    };
    if unsafe { list.Close() }.is_err() {
        return false;
    }
    unsafe {
        output.write(Slot {
            allocator: allocator.into_raw(),
            list: list.into_raw(),
            source: ptr::null_mut(),
        })
    };
    true
}

fn barrier(
    resource: &ID3D12Resource,
    from: D3D12_RESOURCE_STATES,
    to: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // The union borrows the caller's live reference.
                pResource: ManuallyDrop::new(Some(unsafe {
                    ID3D12Resource::from_raw(resource.as_raw())
                })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: from,
                StateAfter: to,
            }),
        },
    }
}

unsafe extern "C" fn record(
    slot: *const Slot,
    buffer: *mut c_void,
    texture: *mut c_void,
    common: bool,
) -> u32 {
    let slot = unsafe { &*slot };
    let allocator: &ID3D12CommandAllocator = unsafe { borrow(&slot.allocator) };
    let list: &ID3D12GraphicsCommandList = unsafe { borrow(&slot.list) };
    let source: &ID3D12Resource = unsafe { borrow(&texture) };
    let destination: &ID3D12Resource = unsafe { borrow(&buffer) };
    let mut source_device: Option<ID3D12Device> = None;
    let mut destination_device: Option<ID3D12Device> = None;
    if source == destination
        || unsafe { source.GetDevice(&mut source_device) }.is_err()
        || unsafe { destination.GetDevice(&mut destination_device) }.is_err()
        || source_device != destination_device
        || unsafe { allocator.Reset() }.is_err()
    {
        return RECORD_REFUSED;
    }
    if unsafe { list.Reset(allocator, None::<&ID3D12PipelineState>) }.is_err() {
        return RECORD_BROKEN;
    }
    let state = if common {
        D3D12_RESOURCE_STATE_COMMON
    } else {
        D3D12_RESOURCE_STATE_RENDER_TARGET
    };
    unsafe {
        list.ResourceBarrier(&[
            barrier(source, state, D3D12_RESOURCE_STATE_COPY_SOURCE),
            barrier(
                destination,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_COPY_DEST,
            ),
        ]);
        list.CopyResource(destination, source);
        list.ResourceBarrier(&[
            barrier(
                destination,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_PRESENT,
            ),
            barrier(source, D3D12_RESOURCE_STATE_COPY_SOURCE, state),
        ]);
    }
    if unsafe { list.Close() }.is_ok() {
        RECORD_CLOSED
    } else {
        RECORD_BROKEN
    }
}

unsafe extern "C" fn execute(queue: *mut c_void, list: *mut c_void) {
    let queue: &ID3D12CommandQueue = unsafe { borrow(&queue) };
    let list = ManuallyDrop::new(Some(unsafe { ID3D12CommandList::from_raw(list) }));
    unsafe { queue.ExecuteCommandLists(std::slice::from_ref(&*list)) };
}

unsafe extern "C" fn signal(queue: *mut c_void, fence: *mut c_void, value: u64) -> bool {
    let queue: &ID3D12CommandQueue = unsafe { borrow(&queue) };
    let fence: &ID3D12Fence = unsafe { borrow(&fence) };
    unsafe { queue.Signal(fence, value) }.is_ok()
}

unsafe extern "C" fn present(
    chain: *mut c_void,
    source: *const rsf_game_render_pass,
    observer: Observer,
) -> bool {
    let mut pass = unsafe { *source };
    pass.swapchain = chain;
    pass.role = rsf_game_sys::RSF_GAME_RENDER_WINDOW;
    if let Some(observe) = observer
        && unsafe { observe(&raw const pass) } == 0
    {
        return false;
    }
    let chain: &IDXGISwapChain3 = unsafe { borrow(&chain) };
    unsafe { chain.Present(0, DXGI_PRESENT(0)) }.0 == S_OK
}

unsafe extern "C" fn retain(object: *mut c_void) {
    let reference: &IUnknown = unsafe { borrow(&object) };
    let _ = reference.clone().into_raw();
}

unsafe extern "C" fn release(object: *mut c_void) {
    drop(unsafe { IUnknown::from_raw(object) });
}

pub(super) const LEAVES: Leaves = Leaves {
    bind_gpu: bind,
    create_chain,
    chain_extent: extent,
    resize_chain: resize,
    describe,
    completed,
    wait,
    acquire,
    create_slot,
    record,
    execute,
    signal,
    present,
    retain,
    release,
};
