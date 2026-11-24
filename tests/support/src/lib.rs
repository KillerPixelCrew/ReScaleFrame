// SPDX-License-Identifier: MIT
//! Window, WARP device and check helpers the Windows fixture crates share.
#![cfg(windows)]

use std::ffi::{CStr, OsStr};
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{
    E_POINTER, FreeLibrary, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDevice, D3D11CreateDeviceAndSwapChain,
    ID3D11Device, ID3D11DeviceContext,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12CreateDevice, D3D12GetDebugInterface, ID3D12Debug, ID3D12Device,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_SWAP_CHAIN_DESC, IDXGIAdapter, IDXGIFactory4, IDXGISwapChain,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_FLAGS, LOAD_LIBRARY_SEARCH_SYSTEM32,
    LoadLibraryExW,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, UnregisterClassW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
};
use windows::core::{HSTRING, IUnknown, PCSTR, PCWSTR, Param, Result};

use ash::vk;
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record, rsf_game_cpu_event,
};

/// Ends the fixture process at the first failed condition, naming the file and line.
#[macro_export]
macro_rules! check {
    ($condition:expr) => {
        if !$condition {
            eprintln!(
                "check failed at {}:{}: {}",
                file!(),
                line!(),
                stringify!($condition)
            );
            std::process::exit(1);
        }
    };
}

/// A frame record with the ABI header, identity and sizes set and everything else zero. Callers
/// change the rest with struct update syntax.
pub fn frame_record(
    session_id: u64,
    frame_id: u64,
    view_id: u32,
    render: [u32; 2],
    output: [u32; 2],
) -> rsf_frame_record {
    rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        session_id,
        frame_id,
        view_id,
        render_width: render[0],
        render_height: render[1],
        output_width: output[0],
        output_height: output[1],
        ..Default::default()
    }
}

/// Gives `record` an inverted-depth camera with identity matrices and the record's own sizes.
pub fn sr_camera(record: &mut rsf_frame_record, near_plane: f32, far_plane: f32) {
    let sizes = [
        record.render_width,
        record.render_height,
        record.output_width,
        record.output_height,
    ];
    let camera = &mut record.camera;
    camera.struct_size = size_of_val(camera) as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    [
        camera.render_width,
        camera.render_height,
        camera.output_width,
        camera.output_height,
    ] = sizes;
    camera.depth_inverted = 1;
    camera.near_plane = near_plane;
    camera.far_plane = far_plane;
    camera.vertical_fov_radians = 1.0;
    for matrix in [
        &mut camera.view_to_clip,
        &mut camera.clip_to_view,
        &mut camera.view_to_world,
        &mut camera.world_to_view,
        &mut camera.clip_to_previous_clip,
    ] {
        for diagonal in [0, 5, 10, 15] {
            matrix[diagonal] = 1.0;
        }
    }
}

/// A CPU event with the struct size, identity and clock set, a rendered source expected, and the
/// event version, flags and input fields zero. Callers change the rest with struct update syntax.
pub fn cpu_event(
    session_id: u64,
    source_frame_id: u64,
    stage: u32,
    timestamp_qpc: u64,
    qpc_frequency: u64,
) -> rsf_game_cpu_event {
    rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage,
        session_id,
        source_frame_id,
        timestamp_qpc,
        qpc_frequency,
        render_expected: 1,
        ..Default::default()
    }
}

/// The performance counter.
pub fn qpc() -> u64 {
    let mut count = 0;
    let _ = unsafe { QueryPerformanceCounter(&mut count) };
    count as u64
}

/// The performance counter's frequency.
pub fn qpc_frequency() -> u64 {
    let mut frequency = 0;
    let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
    frequency as u64
}

/// `text` as the NUL-terminated UTF-16 a Win32 call takes.
pub fn wide_os(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

/// A Vulkan entry point (a `PFN_vk*` or `*const ()`) as the opaque function address the C resolver
/// ABI hands out. The Runtime casts it back to the matching signature before calling it. Windows x64
/// uses one calling convention for system and C procedure pointers.
pub fn vk_entry<F: Copy>(entry: F) -> Option<unsafe extern "C" fn()> {
    const { assert!(size_of::<F>() == size_of::<unsafe extern "C" fn()>()) };
    Some(unsafe { std::mem::transmute_copy(&entry) })
}

/// A `vkCmdPipelineBarrier` that records nothing, for fake Vulkan entry tables.
///
/// # Safety
///
/// It reads none of its arguments; it is unsafe only to match the Vulkan entry signature.
#[allow(clippy::too_many_arguments)]
pub unsafe extern "system" fn ignore_pipeline_barrier(
    _: vk::CommandBuffer,
    _: vk::PipelineStageFlags,
    _: vk::PipelineStageFlags,
    _: vk::DependencyFlags,
    _: u32,
    _: *const vk::MemoryBarrier<'_>,
    _: u32,
    _: *const vk::BufferMemoryBarrier<'_>,
    _: u32,
    _: *const vk::ImageMemoryBarrier<'_>,
) {
}

/// A D3D11 device and its immediate context on `driver`. Fixtures that prefer hardware call this
/// with the hardware driver first and [`warp_d3d11`] second.
pub fn d3d11_device(
    driver: D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
    levels: Option<&[D3D_FEATURE_LEVEL]>,
) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    d3d11_device_on(None, driver, flags, levels)
}

/// [`d3d11_device`] on a chosen adapter, which needs `D3D_DRIVER_TYPE_UNKNOWN`.
pub fn d3d11_device_on(
    adapter: Option<&IDXGIAdapter>,
    driver: D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
    levels: Option<&[D3D_FEATURE_LEVEL]>,
) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            adapter,
            driver,
            HMODULE::default(),
            flags,
            levels,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }?;
    device.zip(context).ok_or_else(|| E_POINTER.into())
}

pub fn warp_d3d11(
    flags: D3D11_CREATE_DEVICE_FLAG,
    levels: Option<&[D3D_FEATURE_LEVEL]>,
) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    d3d11_device(D3D_DRIVER_TYPE_WARP, flags, levels)
}

/// A D3D11 device, its context and a swap chain for `desc.OutputWindow` on `driver`.
pub fn d3d11_swapchain(
    driver: D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
    desc: &DXGI_SWAP_CHAIN_DESC,
) -> Result<(ID3D11Device, ID3D11DeviceContext, IDXGISwapChain)> {
    let (mut chain, mut device, mut context) = (None, None, None);
    unsafe {
        D3D11CreateDeviceAndSwapChain(
            None::<&IDXGIAdapter>,
            driver,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(desc),
            Some(&mut chain),
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }?;
    match (device, context, chain) {
        (Some(device), Some(context), Some(chain)) => Ok((device, context, chain)),
        _ => Err(E_POINTER.into()),
    }
}

pub fn warp_swapchain(
    flags: D3D11_CREATE_DEVICE_FLAG,
    desc: &DXGI_SWAP_CHAIN_DESC,
) -> Result<(ID3D11Device, ID3D11DeviceContext, IDXGISwapChain)> {
    d3d11_swapchain(D3D_DRIVER_TYPE_WARP, flags, desc)
}

/// Turns the D3D12 debug layer on; false when it is not installed.
pub fn enable_d3d12_debug_layer() -> bool {
    let mut layer: Option<ID3D12Debug> = None;
    if unsafe { D3D12GetDebugInterface(&mut layer) }.is_err() {
        return false;
    }
    layer.is_some_and(|layer| {
        unsafe { layer.EnableDebugLayer() };
        true
    })
}

/// A feature level 11.0 D3D12 device on `adapter`.
pub fn d3d12_device(adapter: impl Param<IUnknown>) -> Result<ID3D12Device> {
    let mut device = None;
    unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }?;
    device.ok_or_else(|| E_POINTER.into())
}

/// A D3D12 device on the WARP adapter, with its factory.
pub fn warp_d3d12() -> Result<(IDXGIFactory4, ID3D12Device)> {
    let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory1() }?;
    let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }?;
    let device = d3d12_device(&adapter)?;
    Ok((factory, device))
}

/// The windows crate wraps DefWindowProcW in a Rust function; a window class needs the system ABI.
///
/// # Safety
///
/// Only the system calls it, as a window class procedure, with the arguments of a dispatched message.
pub unsafe extern "system" fn default_window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// A window of its own class, destroyed and unregistered on drop.
pub struct Window {
    pub handle: HWND,
    class: PCWSTR,
    instance: HINSTANCE,
}

impl Window {
    pub fn new(
        class: PCWSTR,
        title: PCWSTR,
        style: WINDOW_STYLE,
        width: i32,
        height: i32,
    ) -> Result<Self> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(PCWSTR::null())? }.0);
        let description = WNDCLASSW {
            lpfnWndProc: Some(default_window_procedure),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&description) } == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let handle = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                title,
                style,
                0,
                0,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )
        };
        match handle {
            Ok(handle) => Ok(Self {
                handle,
                class,
                instance,
            }),
            Err(error) => {
                let _ = unsafe { UnregisterClassW(class, Some(instance)) };
                Err(error)
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.handle);
            let _ = UnregisterClassW(self.class, Some(self.instance));
        }
    }
}

/// A DLL loaded for the life of the value, for fixtures that call the exports of a shipped binary.
pub struct Library(HMODULE);

impl Library {
    /// Loads `path`; `LOAD_LIBRARY_FLAGS(0)` is the default search order.
    pub fn open(path: &OsStr, flags: LOAD_LIBRARY_FLAGS) -> Result<Self> {
        Ok(Self(unsafe {
            LoadLibraryExW(&HSTRING::from(path), None, flags)
        }?))
    }

    pub fn symbol(&self, name: &CStr) -> Option<unsafe extern "system" fn() -> isize> {
        unsafe { GetProcAddress(self.0, PCSTR(name.as_ptr().cast())) }
    }

    /// The Vulkan loader, searched for in System32 only when `system32` is set.
    pub fn vulkan(system32: bool) -> Result<Self> {
        let flags = if system32 {
            LOAD_LIBRARY_SEARCH_SYSTEM32
        } else {
            LOAD_LIBRARY_FLAGS(0)
        };
        Self::open(OsStr::new("vulkan-1.dll"), flags)
    }

    /// `vkGetInstanceProcAddr` of the Vulkan loader.
    pub fn vulkan_instance_proc(&self) -> Option<vk::PFN_vkGetInstanceProcAddr> {
        let address = self.symbol(c"vkGetInstanceProcAddr")?;
        Some(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, vk::PFN_vkGetInstanceProcAddr>(
                address,
            )
        })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(self.0) };
    }
}
