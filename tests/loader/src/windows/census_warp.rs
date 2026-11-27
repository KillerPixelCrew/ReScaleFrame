// SPDX-License-Identifier: MIT
// Real WARP calls for the census fixture. The input payload and caller probe stay native.
use super::{cast, named};
use ::windows::{
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct3D::{D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0},
            Direct3D12::{
                D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, ID3D12CommandAllocator,
                ID3D12CommandList, ID3D12CommandQueue, ID3D12Device, ID3D12GraphicsCommandList,
                ID3D12PipelineState,
            },
            Dxgi::{
                Common::{
                    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, DXGI_PRESENT_TEST, DXGI_SWAP_CHAIN_DESC1,
                DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
                DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIFactory4, IDXGIOutput,
                IDXGISwapChain3,
            },
        },
    },
    core::{GUID, Interface},
};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows_sys::{
    Win32::{
        System::LibraryLoader::LoadLibraryW,
        UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_OVERLAPPED},
    },
    core::w,
};

type Ptr = *mut c_void;
type Factory = unsafe extern "system" fn(*const GUID, *mut Ptr) -> i32;
type Device = unsafe extern "system" fn(Ptr, D3D_FEATURE_LEVEL, *const GUID, *mut Ptr) -> i32;
type Dispatch = unsafe extern "C" fn(*const c_void);

struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0.0) };
    }
}

fn fixture_window() -> Option<Window> {
    let window = unsafe {
        CreateWindowExW(
            0,
            w!("STATIC"),
            w!("census"),
            WS_OVERLAPPED,
            0,
            0,
            640,
            360,
            null_mut(),
            null_mut(),
            null_mut(),
            null(),
        )
    };
    (!window.is_null()).then(|| Window(HWND(window)))
}

fn open_warp() -> Option<(IDXGIFactory4, IDXGIAdapter, ID3D12Device)> {
    let dxgi = unsafe { LoadLibraryW(w!("dxgi.dll")) };
    let d3d12 = unsafe { LoadLibraryW(w!("d3d12.dll")) };
    if dxgi.is_null() || d3d12.is_null() {
        return None;
    }
    let create_factory = cast::<Factory>(named(dxgi, c"CreateDXGIFactory1")?);
    let create_device = cast::<Device>(named(d3d12, c"D3D12CreateDevice")?);
    let mut raw = null_mut();
    if unsafe { create_factory(&IDXGIFactory4::IID, &mut raw) } < 0 || raw.is_null() {
        return None;
    }
    let factory = unsafe { IDXGIFactory4::from_raw(raw) };
    let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.ok()?;
    let mut raw = null_mut();
    if unsafe {
        create_device(
            adapter.as_raw(),
            D3D_FEATURE_LEVEL_11_0,
            &ID3D12Device::IID,
            &mut raw,
        )
    } < 0
        || raw.is_null()
    {
        return None;
    }
    let device = unsafe { ID3D12Device::from_raw(raw) };
    Some((factory, adapter, device))
}

fn checked<T>(
    result: ::windows::core::Result<T>,
    failures: &mut i32,
    operation: &str,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("FAIL {operation}: {error}");
            *failures += 1;
            None
        }
    }
}

fn frames(
    payload: *const c_void,
    dispatch: Dispatch,
    calls: &mut u64,
    failures: &mut i32,
) -> Option<()> {
    let (factory, _adapter, device) = open_warp()?;
    let queue: ID3D12CommandQueue = checked(
        unsafe {
            device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                ..Default::default()
            })
        },
        failures,
        "CreateCommandQueue",
    )?;
    let allocator: ID3D12CommandAllocator = checked(
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) },
        failures,
        "CreateCommandAllocator",
    )?;
    let list: ID3D12GraphicsCommandList = checked(
        unsafe {
            device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None::<&ID3D12PipelineState>,
            )
        },
        failures,
        "CreateCommandList",
    )?;
    let _ = unsafe { list.Close() };
    let lists = [Some(ID3D12CommandList::from(list))];
    let window = fixture_window()?;
    let description = DXGI_SWAP_CHAIN_DESC1 {
        Width: 640,
        Height: 360,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        ..Default::default()
    };
    let chain = checked(
        unsafe {
            factory.CreateSwapChainForHwnd(
                &queue,
                window.0,
                &description,
                None,
                None::<&IDXGIOutput>,
            )
        },
        failures,
        "CreateSwapChainForHwnd",
    )?;
    let color_chain = chain.cast::<IDXGISwapChain3>().ok();
    if let Some(chain) = &color_chain {
        let _ = unsafe { chain.SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709) };
    }
    for frame in 0..3 {
        unsafe {
            dispatch(payload);
            dispatch(payload);
            *calls += 2;
            queue.ExecuteCommandLists(&lists);
            queue.ExecuteCommandLists(&lists);
        }
        let present = if frame == 1 {
            unsafe { chain.Present1(0, DXGI_PRESENT(0), &DXGI_PRESENT_PARAMETERS::default()) }
        } else {
            unsafe { chain.Present(0, DXGI_PRESENT(0)) }
        };
        let _ = checked(present.ok(), failures, "Present");
    }
    let _ = unsafe { chain.Present(0, DXGI_PRESENT_TEST) };
    if let Some(chain) = &color_chain {
        let _ = checked(
            unsafe {
                chain.ResizeBuffers(0, 320, 180, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
            },
            failures,
            "ResizeBuffers",
        );
    }
    Some(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_warp(
    payload: *const c_void,
    dispatch: Dispatch,
    calls: &mut u64,
    failures: &mut i32,
) -> bool {
    frames(payload, dispatch, calls, failures).is_some()
}
