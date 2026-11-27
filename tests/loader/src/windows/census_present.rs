// SPDX-License-Identifier: MIT
// Opt-in desktop fixture for an injected overlay alongside the vendor census.
use crate::sys::{RSF_CENSUS_ABI_VERSION, rsf_census_options};
use ::windows::Win32::UI::WindowsAndMessaging::{WS_OVERLAPPEDWINDOW, WS_VISIBLE};
use ::windows::Win32::{
    Foundation::{E_FAIL, HANDLE},
    Graphics::{
        Direct3D::D3D_FEATURE_LEVEL_11_0,
        Direct3D12::*,
        Dxgi::{Common::*, *},
    },
};
use ::windows::core::{Interface, PCSTR, w};
use rsf_test_support::Window;
use rsf_test_support::d3d12::transition_barrier;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use windows_sys::Win32::{
    Foundation::{CloseHandle, MAX_PATH, WAIT_OBJECT_0},
    Storage::FileSystem::GetTempPathA,
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{CreateEventW, WaitForSingleObject},
    },
    UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW},
};

const SLOTS: u32 = 3;
const MULTI_BUFFER_ID: i32 = 904;
static ERRORS: AtomicU32 = AtomicU32::new(0);
static MULTI_BUFFER: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn on_message(
    _: D3D12_MESSAGE_CATEGORY,
    severity: D3D12_MESSAGE_SEVERITY,
    id: D3D12_MESSAGE_ID,
    text: PCSTR,
    _: *mut c_void,
) {
    if severity.0 > D3D12_MESSAGE_SEVERITY_ERROR.0 {
        return;
    }
    if id.0 == MULTI_BUFFER_ID {
        MULTI_BUFFER.fetch_add(1, Relaxed);
    }
    if ERRORS.fetch_add(1, Relaxed) < 8 && !text.0.is_null() {
        eprintln!(
            "d3d12 error {}: {}",
            id.0,
            unsafe { CStr::from_ptr(text.0.cast()) }.to_string_lossy()
        );
    }
}

fn create_window() -> Option<Window> {
    let style = WS_VISIBLE | WS_OVERLAPPEDWINDOW;
    Window::new(
        w!("RsfCensusCoexistence"),
        w!("census coexistence"),
        style,
        1280,
        720,
    )
    .ok()
}
struct Event(HANDLE);
impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0.0) };
    }
}

struct ErrorObserver(ID3D12InfoQueue1, u32);
impl Drop for ErrorObserver {
    fn drop(&mut self) {
        let _ = unsafe { self.0.UnregisterMessageCallback(self.1) };
    }
}

fn hardware_device(factory: &IDXGIFactory4) -> Option<ID3D12Device> {
    for index in 0.. {
        let adapter = unsafe { factory.EnumAdapters1(index) }.ok()?;
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
            || desc.DedicatedVideoMemory <= 512 << 20
        {
            continue;
        }
        let mut device = None;
        if unsafe {
            D3D12CreateDevice::<_, ID3D12Device>(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device)
        }
        .is_ok()
            && device.is_some()
        {
            let end = desc
                .Description
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(desc.Description.len());
            println!(
                "adapter {}",
                String::from_utf16_lossy(&desc.Description[..end])
            );
            return device;
        }
    }
    None
}

fn handle(heap: &ID3D12DescriptorHeap, index: u32, step: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
    let mut handle = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
    handle.ptr += index as usize * step as usize;
    handle
}

fn wait(fence: &ID3D12Fence, value: u64, event: &Event) -> bool {
    unsafe { fence.SetEventOnCompletion(value, event.0) }.is_ok()
        && unsafe { WaitForSingleObject(event.0.0, 5000) } == WAIT_OBJECT_0
}

#[derive(Default)]
struct Counts {
    reset_failures: u32,
    present_failures: u32,
}

fn render(window: &Window, frames: u32) -> ::windows::core::Result<Counts> {
    let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }?;
    let device = hardware_device(&factory)
        .ok_or_else(|| ::windows::core::Error::new(E_FAIL, "no hardware D3D12 adapter"))?;
    let _observer = device.cast::<ID3D12InfoQueue1>().ok().and_then(|info| {
        let mut cookie = 0;
        unsafe {
            info.RegisterMessageCallback(
                Some(on_message),
                D3D12_MESSAGE_CALLBACK_FLAG_NONE,
                null_mut(),
                &mut cookie,
            )
        }
        .ok()?;
        Some(ErrorObserver(info, cookie))
    });
    let queue: ID3D12CommandQueue = unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        })
    }?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: 1280,
        Height: 720,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT | DXGI_USAGE_SHADER_INPUT,
        BufferCount: SLOTS,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
        ..Default::default()
    };
    let swapchain: IDXGISwapChain3 = unsafe {
        factory.CreateSwapChainForHwnd(&queue, window.handle, &desc, None, None::<&IDXGIOutput>)
    }?
    .cast()?;
    let rtvs: ID3D12DescriptorHeap = unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: SLOTS,
            ..Default::default()
        })
    }?;
    let step = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) };
    let mut buffers = Vec::new();
    let mut allocators = Vec::new();
    for index in 0..SLOTS {
        let resource: ID3D12Resource = unsafe { swapchain.GetBuffer(index) }?;
        unsafe { device.CreateRenderTargetView(&resource, None, handle(&rtvs, index, step)) };
        buffers.push(resource);
        allocators.push(unsafe {
            device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
        }?);
    }
    let list: ID3D12GraphicsCommandList = unsafe {
        device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &allocators[0],
            None::<&ID3D12PipelineState>,
        )
    }?;
    unsafe { list.Close() }?;
    let commands: ID3D12CommandList = list.cast()?;
    let commands = [Some(commands)];
    let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }?;
    let event = unsafe { CreateEventW(null(), 0, 0, null()) };
    if event.is_null() {
        return Err(::windows::core::Error::from_thread());
    }
    let event = Event(HANDLE(event));
    let mut slots = [0; SLOTS as usize];
    let mut next = 1;
    let mut counts = Counts::default();
    for frame in 0..frames {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe { DispatchMessageW(&message) };
        }
        let index = unsafe { swapchain.GetCurrentBackBufferIndex() };
        let slot = index as usize;
        if slot >= slots.len() {
            counts.reset_failures += 1;
            break;
        }
        if unsafe { fence.GetCompletedValue() } < slots[slot] && !wait(&fence, slots[slot], &event)
        {
            counts.reset_failures += 1;
            break;
        }
        if unsafe { allocators[slot].Reset() }.is_err()
            || unsafe { list.Reset(&allocators[slot], None::<&ID3D12PipelineState>) }.is_err()
        {
            counts.reset_failures += 1;
            break;
        }
        let before = D3D12_RESOURCE_STATE_PRESENT;
        let after = D3D12_RESOURCE_STATE_RENDER_TARGET;
        let colour = [0.1, 0.2 + 0.3 * (frame % 60) as f32 / 60.0, 0.4, 1.0];
        unsafe {
            list.ResourceBarrier(&[transition_barrier(&buffers[slot], before, after)]);
            list.ClearRenderTargetView(handle(&rtvs, index, step), &colour, None);
            list.ResourceBarrier(&[transition_barrier(&buffers[slot], after, before)]);
            list.Close()?;
            queue.ExecuteCommandLists(&commands);
        }
        if unsafe { swapchain.Present(1, DXGI_PRESENT(0)) }.is_err() {
            counts.present_failures += 1;
        }
        slots[slot] = next;
        unsafe { queue.Signal(&fence, next) }?;
        next += 1;
    }
    unsafe { queue.Signal(&fence, next) }?;
    if !wait(&fence, next, &event) {
        counts.reset_failures += 1;
    }
    Ok(counts)
}

type Install = unsafe extern "C" fn(*const rsf_census_options) -> i32;
type Flush = unsafe extern "C" fn() -> i32;

fn install_census(install: Install) -> bool {
    let mut folder = [0u8; MAX_PATH as usize];
    let count = unsafe { GetTempPathA(MAX_PATH, folder.as_mut_ptr()) } as usize;
    let suffix = c"rsf-census-coexistence".to_bytes_with_nul();
    if count == 0 || count + suffix.len() > folder.len() {
        return false;
    }
    folder[count..count + suffix.len()].copy_from_slice(suffix);
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: folder.as_ptr().cast(),
        ..Default::default()
    };
    if unsafe { install(&options) } != 0 {
        return false;
    }
    let dir = CStr::from_bytes_until_nul(&folder).expect("terminated census path");
    println!("census installed, output {}", dir.to_string_lossy());
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_present(
    argc: i32,
    argv: *const *const c_char,
    install: Install,
    flush: Flush,
) -> i32 {
    let with_census = argc > 1 && unsafe { CStr::from_ptr(*argv.add(1)) } == c"census";
    let frames = if argc > 2 {
        unsafe { CStr::from_ptr(*argv.add(2)) }
            .to_string_lossy()
            .parse::<i32>()
            .unwrap_or(0) as u32
    } else {
        900
    };
    if with_census && !install_census(install) {
        eprintln!("census install failed");
        return 2;
    }
    let mut debug = None;
    if unsafe { D3D12GetDebugInterface::<ID3D12Debug>(&mut debug) }.is_ok()
        && let Some(debug) = debug
    {
        unsafe { debug.EnableDebugLayer() };
    }
    let Some(window) = create_window() else {
        return 2;
    };
    let result = render(&window, frames);
    if with_census {
        unsafe { flush() };
    }
    let counts = match result {
        Ok(counts) => counts,
        Err(error) => {
            eprintln!("D3D12 fixture failed: {error}");
            return 2;
        }
    };
    let rtss = !unsafe { GetModuleHandleW(w!("RTSSHooks64.dll").as_ptr()) }.is_null();
    let mode = if with_census { "census" } else { "plain" };
    let errors = ERRORS.load(Relaxed);
    let multi = MULTI_BUFFER.load(Relaxed);
    let Counts {
        reset_failures,
        present_failures,
    } = counts;
    println!(
        "mode={mode} frames={frames} rtss_loaded={} d3d12_errors={errors} multi_buffer_904={multi} reset_failures={reset_failures} present_failures={present_failures}",
        i32::from(rtss)
    );
    i32::from(errors != 0 || reset_failures != 0 || present_failures != 0)
}
