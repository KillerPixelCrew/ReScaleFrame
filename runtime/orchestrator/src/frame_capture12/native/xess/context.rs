// SPDX-License-Identifier: MIT
use super::{Handle, readback_buffer};
use windows::Win32::{
    Foundation::{E_UNEXPECTED, GENERIC_ALL, HANDLE, LUID},
    Graphics::{
        Direct3D::D3D_FEATURE_LEVEL_11_0,
        Direct3D12::*,
        Dxgi::{
            Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
            CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_DEVICE_REMOVED,
            DXGI_FEATURE_PRESENT_ALLOW_TEARING, DXGI_PRESENT, DXGI_PRESENT_ALLOW_TEARING,
            DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING,
            DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter1,
            IDXGIFactory4, IDXGIFactory5, IDXGISwapChain4,
        },
    },
};
use windows::core::{Interface, PCWSTR, Result};

mod timestamps;

unsafe fn borrow<T: Interface>(raw: &Handle) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live XeSS bridge graphics interface")
}

fn status(result: Result<()>) -> i32 {
    result.map_or_else(|error| error.code().0, |()| 0)
}

fn publish<T: Interface>(result: Result<T>, out: &mut Handle) -> i32 {
    match result {
        Ok(value) => {
            *out = value.into_raw();
            0
        }
        Err(error) => error.code().0,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_dx12_factory(out: &mut Handle) -> i32 {
    let result = unsafe { CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0)) };
    if publish(result, out) < 0 { -4 } else { 0 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_adapter(
    factory: Handle,
    luid: u64,
    out: &mut Handle,
) -> i32 {
    let factory = unsafe { borrow::<IDXGIFactory4>(&factory) };
    let luid = LUID {
        LowPart: luid as u32,
        HighPart: (luid >> 32) as i32,
    };
    let result = unsafe { factory.EnumAdapterByLuid::<IDXGIAdapter1>(luid) };
    if publish(result, out) < 0 { -5 } else { 0 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_device(adapter: Handle, out: &mut Handle) -> i32 {
    let adapter = unsafe { borrow::<IDXGIAdapter1>(&adapter) };
    let mut device = None::<ID3D12Device>;
    if unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.is_err() {
        return -6;
    }
    let Some(device) = device else { return -6 };
    *out = device.into_raw();
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_queue(device: Handle, out: &mut Handle) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    let result = unsafe { device.CreateCommandQueue::<ID3D12CommandQueue>(&desc) };
    if publish(result, out) < 0 { -7 } else { 0 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_tearing(factory: Handle) -> u32 {
    let factory = unsafe { borrow::<IDXGIFactory4>(&factory) };
    let Ok(factory) = factory.cast::<IDXGIFactory5>() else {
        return 0;
    };
    let mut supported = 0i32;
    let result = unsafe {
        factory.CheckFeatureSupport(
            DXGI_FEATURE_PRESENT_ALLOW_TEARING,
            (&raw mut supported).cast(),
            size_of::<i32>() as u32,
        )
    };
    u32::from(result.is_ok() && supported != 0)
}

const _: () = assert!(size_of::<DXGI_SWAP_CHAIN_DESC1>() == 48);

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_dx12_swapchain_desc(
    out: &mut DXGI_SWAP_CHAIN_DESC1,
    width: u32,
    height: u32,
    tearing: u32,
) {
    *out = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 3,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        Flags: if tearing != 0 {
            DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32
        } else {
            0
        },
        ..Default::default()
    };
}

const SLOTS: usize = rsf_vk_xess_fg::session_records_abi::RSF_XESS_BRIDGE_ALLOCATOR_COUNT as usize;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_recorders(
    device: Handle,
    allocators: &mut [Handle; SLOTS],
    list: &mut Handle,
) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    for slot in allocators.iter_mut() {
        let result = unsafe {
            device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
        };
        let code = publish(result, slot);
        if code < 0 {
            return code;
        }
    }
    let allocator = unsafe { borrow::<ID3D12CommandAllocator>(&allocators[0]) };
    let commands: ID3D12GraphicsCommandList = match unsafe {
        device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            allocator,
            None::<&ID3D12PipelineState>,
        )
    } {
        Ok(commands) => commands,
        Err(error) => return error.code().0,
    };
    let code = status(unsafe { commands.Close() });
    *list = commands.into_raw();
    code
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_reset(allocator: Handle, list: Handle) -> i32 {
    let allocator = unsafe { borrow::<ID3D12CommandAllocator>(&allocator) };
    let list = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    status(unsafe {
        allocator
            .Reset()
            .and_then(|()| list.Reset(allocator, None::<&ID3D12PipelineState>))
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_close(list: Handle) -> i32 {
    status(unsafe { borrow::<ID3D12GraphicsCommandList>(&list).Close() })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_backbuffer(swapchain: Handle, out: &mut Handle) -> i32 {
    let swapchain = unsafe { borrow::<IDXGISwapChain4>(&swapchain) };
    publish(
        unsafe { swapchain.GetBuffer::<ID3D12Resource>(swapchain.GetCurrentBackBufferIndex()) },
        out,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_present(
    swapchain: Handle,
    vsync: u32,
    tearing: u32,
) -> i32 {
    let swapchain = unsafe { borrow::<IDXGISwapChain4>(&swapchain) };
    let flags = if vsync == 0 && tearing != 0 {
        DXGI_PRESENT_ALLOW_TEARING
    } else {
        DXGI_PRESENT(0)
    };
    unsafe { swapchain.Present(u32::from(vsync != 0), flags) }.0
}

unsafe fn execute(queue: &ID3D12CommandQueue, list: &ID3D12GraphicsCommandList) {
    // The inherited ID3D12CommandList pointer is borrowed for the native call.
    let list =
        std::mem::ManuallyDrop::new([Some(unsafe { ID3D12CommandList::from_raw(list.as_raw()) })]);
    unsafe { queue.ExecuteCommandLists(&*list) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_execute(queue: Handle, list: Handle) {
    unsafe {
        execute(
            borrow::<ID3D12CommandQueue>(&queue),
            borrow::<ID3D12GraphicsCommandList>(&list),
        )
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_fence(
    device: Handle,
    shared: u32,
    out: &mut Handle,
) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    let flags = if shared != 0 {
        D3D12_FENCE_FLAG_SHARED
    } else {
        D3D12_FENCE_FLAG_NONE
    };
    publish(unsafe { device.CreateFence::<ID3D12Fence>(0, flags) }, out)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_completed(fence: Handle, completed: &mut u64) -> i32 {
    let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&fence) }) else {
        return E_UNEXPECTED.0;
    };
    *completed = unsafe { fence.GetCompletedValue() };
    if *completed == u64::MAX {
        DXGI_ERROR_DEVICE_REMOVED.0
    } else {
        0
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_arm(fence: Handle, target: u64, event: Handle) -> i32 {
    status(unsafe { borrow::<ID3D12Fence>(&fence).SetEventOnCompletion(target, HANDLE(event)) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_wait(queue: Handle, fence: Handle, target: u64) -> i32 {
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    status(unsafe { queue.Wait(borrow::<ID3D12Fence>(&fence), target) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_signal(queue: Handle, fence: Handle, target: u64) -> i32 {
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    status(unsafe { queue.Signal(borrow::<ID3D12Fence>(&fence), target) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_share_fence(
    device: Handle,
    fence: Handle,
    out: &mut Handle,
) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    let fence = unsafe { borrow::<ID3D12Fence>(&fence) };
    match unsafe { device.CreateSharedHandle(fence, None, GENERIC_ALL.0, PCWSTR::null()) } {
        Ok(handle) => {
            *out = handle.0;
            0
        }
        Err(error) => error.code().0,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_vulkan_luid(physical: Handle, out: &mut u64) -> i32 {
    use ash::vk::{self, Handle as _};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    let loader_name: Vec<_> = "vulkan-1.dll\0".encode_utf16().collect();
    let loader = unsafe { GetModuleHandleW(loader_name.as_ptr()) };
    if loader.is_null() {
        return -10;
    }
    let Some(address) =
        (unsafe { GetProcAddress(loader, c"vkGetPhysicalDeviceProperties2".as_ptr().cast()) })
    else {
        return -10;
    };
    let query: vk::PFN_vkGetPhysicalDeviceProperties2 = unsafe { std::mem::transmute(address) };
    let mut identity = vk::PhysicalDeviceIDProperties::default();
    let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut identity);
    unsafe {
        query(
            vk::PhysicalDevice::from_raw(physical as u64),
            &mut properties,
        )
    };
    if identity.device_luid_valid == 0 {
        return -11;
    }
    *out = u64::from_ne_bytes(identity.device_luid);
    0
}
