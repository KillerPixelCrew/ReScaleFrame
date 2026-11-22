// SPDX-License-Identifier: MIT
//! Device creation shared by the two linked presentation fixtures.
use std::ffi::{c_char, c_void};
use std::ptr;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP,
    D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12CreateDevice,
    D3D12GetDebugInterface, ID3D12CommandQueue, ID3D12Debug, ID3D12Device,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND, IDXGIAdapter,
    IDXGIAdapter1, IDXGIFactory1, IDXGIFactory2, IDXGIFactory4,
};
use windows::Win32::Storage::FileSystem::CreateDirectoryW;
use windows::Win32::System::Environment::SetEnvironmentVariableW;
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress, LoadLibraryA};
use windows::Win32::System::Threading::Sleep;
use windows::core::{IUnknown, Interface, PCSTR, PCWSTR, w};

type Raw = *mut c_void;

fn adapter_for_vendor(factory: &IDXGIFactory1, vendor: u32) -> Result<IDXGIAdapter1, i32> {
    for index in 0.. {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return Err(77),
            Err(_) => return Err(1),
        };
        let desc = unsafe { adapter.GetDesc1() }.map_err(|_| 1)?;
        if desc.VendorId == vendor && desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
            return Ok(adapter);
        }
    }
    unreachable!()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_present_fixture_adapter(
    vendor: u32,
    factory4: u32,
    out_factory: *mut Raw,
    out_adapter: *mut Raw,
) -> i32 {
    let selected = unsafe {
        if factory4 != 0 {
            CreateDXGIFactory1::<IDXGIFactory4>()
                .map_err(|_| 1)
                .and_then(|factory| {
                    adapter_for_vendor(&factory, vendor)
                        .map(|adapter| (factory.into_raw(), adapter))
                })
        } else {
            CreateDXGIFactory1::<IDXGIFactory2>()
                .map_err(|_| 1)
                .and_then(|factory| {
                    adapter_for_vendor(&factory, vendor)
                        .map(|adapter| (factory.into_raw(), adapter))
                })
        }
    };
    match selected {
        Ok((factory, adapter)) => {
            unsafe {
                out_factory.write(factory);
                out_adapter.write(adapter.into_raw());
            }
            0
        }
        Err(code) => code,
    }
}

fn create11(
    adapter: Option<&IDXGIAdapter>,
    driver: D3D_DRIVER_TYPE,
) -> Option<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    // These linked runners pass no feature-level list and no creation flags.
    unsafe {
        D3D11CreateDevice(
            adapter,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .ok()?;
    Some((device?, context?))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_present_fixture_device11(
    adapter: Raw,
    device: *mut Raw,
    context: *mut Raw,
) -> i32 {
    let adapter = unsafe { IDXGIAdapter::from_raw_borrowed(&adapter) };
    let Some((created, immediate)) = create11(adapter, D3D_DRIVER_TYPE_UNKNOWN) else {
        return 1;
    };
    unsafe {
        device.write(created.into_raw());
        context.write(immediate.into_raw());
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_present_fixture_device12(
    adapter: Raw,
    device: *mut Raw,
    queue: *mut Raw,
) -> i32 {
    let adapter = unsafe { IUnknown::from_raw_borrowed(&adapter) };
    let mut created: Option<ID3D12Device> = None;
    if unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut created) }.is_err() {
        return 1;
    }
    let Some(created) = created else {
        return 1;
    };
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    let Ok(direct) = (unsafe { created.CreateCommandQueue::<ID3D12CommandQueue>(&desc) }) else {
        return 1;
    };
    unsafe {
        device.write(created.into_raw());
        queue.write(direct.into_raw());
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_present_fixture_debug12() -> i32 {
    let mut layer: Option<ID3D12Debug> = None;
    if unsafe { D3D12GetDebugInterface(&mut layer) }.is_err() {
        return 1;
    }
    let Some(layer) = layer else {
        return 1;
    };
    unsafe { layer.EnableDebugLayer() };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_proxy_smoke(proxy_path: *const c_char) -> i32 {
    let mut logs = [0_u16; 260];
    let path_length = unsafe { GetModuleFileNameW(None, &mut logs) } as usize;
    if path_length == 0 || path_length >= logs.len() {
        return 1;
    }
    let Some(leaf) = logs[..path_length]
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\'))
    else {
        return 1;
    };
    let suffix = unsafe { w!("\\proxy-smoke-logs").as_wide() };
    if leaf + suffix.len() + 1 > logs.len() {
        return 1;
    }
    logs[leaf..leaf + suffix.len()].copy_from_slice(suffix);
    logs[leaf + suffix.len()] = 0;
    let dir = PCWSTR(logs.as_ptr());
    let _ = unsafe { CreateDirectoryW(dir, None) };
    if unsafe { SetEnvironmentVariableW(w!("RSF_DUMP_DIR"), dir) }.is_err() {
        return 1;
    }
    let proxy = match unsafe { LoadLibraryA(PCSTR(proxy_path.cast())) } {
        Ok(proxy) => proxy,
        Err(error) => {
            eprintln!("proxy load error={}", error.code().0 as u32 & 0xffff);
            return 1;
        }
    };
    let named = unsafe { GetProcAddress(proxy, PCSTR(c"DirectInput8Create".as_ptr().cast())) };
    let ordinal = unsafe { GetProcAddress(proxy, PCSTR(ptr::without_provenance(1))) };
    if !matches!((named, ordinal), (Some(named), Some(ordinal)) if std::ptr::fn_addr_eq(named, ordinal))
        || unsafe { GetProcAddress(proxy, PCSTR(c"GetFileVersionInfoW".as_ptr().cast())) }.is_some()
    {
        return 1;
    }
    // Carrier hooks and its worker are process-owned. Leave the DLL loaded.
    unsafe { Sleep(250) };
    let Ok(_factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory2>() }) else {
        return 1;
    };
    if create11(None, D3D_DRIVER_TYPE_HARDWARE)
        .or_else(|| create11(None, D3D_DRIVER_TYPE_WARP))
        .is_none()
    {
        return 1;
    }
    println!(
        "PASS: AC7-only carrier loads, DirectInput ordinal 1 matches, D3D11 observer/device startup succeeds"
    );
    0
}
