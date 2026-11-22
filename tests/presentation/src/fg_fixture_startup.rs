// SPDX-License-Identifier: MIT
//! Startup and retirement for the linked shared FG executable.
use crate::fg_bridge::{rsf_fg_bridge_fixture_allocate, rsf_fg_bridge_fixture_commands};
use crate::fg_fixture_callbacks::*;
use crate::fg_fixture_focus::rsf_fg_bridge_fixture_focus_end;
use crate::fg_fixture_loop::rsf_fg_fixture_run_loop;
use crate::fixture_device::{rsf_present_fixture_debug_errors, rsf_present_fixture_debug12};
use crate::present_fixture_startup::*;
use crate::sys::rsf_fg_fixture_inputs;
use crate::sys::*;
use std::ffi::{CStr, CString, c_char};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Direct3D11::{ID3D11RenderTargetView, ID3D11Texture2D};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::CreateEventW;
use windows::core::{BOOL, Interface, PCWSTR, w};

unsafe extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
}

// The provider can retain callback inputs after a failed GPU retirement.
static PROCESS_STATE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());

extern "C" fn release_state() {
    unsafe { rsf_fg_fixture_state_destroy(PROCESS_STATE.swap(null_mut(), Ordering::AcqRel)) };
}

unsafe extern "C" fn log(_: Raw, message: *const c_char) {
    println!(
        "SDK: {}",
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    );
}

unsafe extern "C" fn accept(_: Raw, _: Raw) -> i32 {
    1
}

struct Focus(Raw);
impl Drop for Focus {
    fn drop(&mut self) {
        unsafe { rsf_fg_bridge_fixture_focus_end(self.0) };
    }
}

struct Mode {
    overlay: bool,
    mixed: bool,
    mfg: bool,
    hot: bool,
    flip: bool,
    legacy_hot: bool,
    service: bool,
    native: bool,
}

impl Mode {
    fn new(api: &[u8]) -> Self {
        let overlay = api == b"12-overlay";
        let mixed = api == b"12-mixed" || overlay;
        let mfg = api == b"12-mfg";
        let hot = api == b"12-hot" || mixed || mfg;
        let flip = api == b"11-flip";
        let legacy_hot = api == b"11-hot" || flip;
        let service = hot || api == b"12-service";
        let native = api != b"11" && !legacy_hot;
        Self {
            overlay,
            mixed,
            mfg,
            hot,
            flip,
            legacy_hot,
            service,
            native,
        }
    }

    fn install(&self, directories: &Directories, backend: u32, explicit: bool, state: Raw) -> bool {
        let (engine_type, engine_version_utf8, project_id_utf8) = if self.native {
            (2, c"6000.3", c"57a42c7e-faf0-4bda-a9f9-892870948ac1")
        } else {
            (1, c"4.18.3", c"a3ed1f08-3542-4698-b85c-e1a9908e861a")
        };
        let max_generated_frames = if self.mfg { 5 } else { 3 };
        if self.service {
            let mut setup = rsf_fg12_setup {
                struct_size: size_of::<rsf_fg12_setup>() as u32,
                backend,
                max_generated_frames,
                runtime_directory_utf8: directories.primary,
                options: rsf_native_fg_options {
                    struct_size: size_of::<rsf_native_fg_options>() as u32,
                    mode: RSF_FG_FIXED,
                    generated_frames: 1,
                    reflex_mode: RSF_REFLEX_OFF,
                    ..Default::default()
                },
                depth_inverted: 1,
                units_to_meters: 1.0,
                log: Some(log),
                accept_window: Some(accept),
                engine_type,
                engine_version_utf8: engine_version_utf8.as_ptr(),
                project_id_utf8: project_id_utf8.as_ptr(),
                ..Default::default()
            };
            if self.hot {
                setup.runtime_switching = 1;
                setup.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
                setup.fsr3_directory_utf8 = directories.primary;
                setup.fsr4_directory_utf8 = directories.primary;
                setup.xess_directory_utf8 = directories.xess;
                setup.streamline_directory_utf8 = directories.extra;
            }
            let install = if explicit {
                rsf_fg12_install_explicit
            } else {
                rsf_fg12_install
            };
            return unsafe { install(&setup) } == 0;
        }
        let mut setup = rsf_d3d11_present_setup {
            struct_size: size_of::<rsf_d3d11_present_setup>() as u32,
            runtime_directory_utf8: directories.primary,
            backend,
            max_generated_frames,
            log: Some(log),
            user: state,
            accept_window: Some(accept),
            prepare: Some(rsf_fg_fixture_prepare),
            latency_event: Some(rsf_fg_fixture_latency),
            retire: Some(rsf_fg_fixture_retire),
            depth_inverted: 1,
            depth_infinite: u32::from(!self.native),
            units_to_meters: if self.native { 1.0 } else { 0.01 },
            engine_type,
            engine_version_utf8: engine_version_utf8.as_ptr(),
            project_id_utf8: project_id_utf8.as_ptr(),
            ..Default::default()
        };
        if self.legacy_hot {
            setup.runtime_switching = 1;
            setup.streamline_directory_utf8 = directories.primary;
            setup.fsr3_directory_utf8 = directories.legacy_fsr();
            setup.fsr4_directory_utf8 = directories.legacy_fsr();
            setup.xess_directory_utf8 = directories.xess;
        }
        let install = if explicit {
            rsf_d3d11_present_prepare
        } else {
            rsf_d3d11_present_install
        };
        unsafe { install(&setup) != 0 }
    }

    fn chain(
        &self,
        factory: &IDXGIFactory4,
        producer: Raw,
        window: HWND,
        explicit: bool,
    ) -> Result<IDXGISwapChain, i32> {
        let flip = self.native || self.flip;
        let desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC {
                Width: if self.mixed { 2560 } else { 1280 },
                Height: if self.mixed { 1600 } else { 720 },
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ..Default::default()
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: if flip { 3 } else { 1 },
            OutputWindow: window,
            Windowed: BOOL(1),
            SwapEffect: if flip {
                DXGI_SWAP_EFFECT_FLIP_DISCARD
            } else {
                DXGI_SWAP_EFFECT_DISCARD
            },
            Flags: if self.hot && self.native {
                DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32
            } else {
                0
            },
        };
        if !explicit {
            return create_chain(factory, producer, &desc);
        }
        let explicit_desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: desc.BufferDesc.Width,
            Height: desc.BufferDesc.Height,
            Format: desc.BufferDesc.Format,
            SampleDesc: desc.SampleDesc,
            BufferUsage: desc.BufferUsage,
            BufferCount: desc.BufferCount,
            SwapEffect: desc.SwapEffect,
            Flags: desc.Flags,
            ..Default::default()
        };
        let mut created = null_mut();
        let result = unsafe {
            rsf_d3d11_present_create_for_hwnd(
                factory.as_raw(),
                producer,
                window.0,
                (&raw const explicit_desc).cast(),
                &mut created,
            )
        };
        if result < 0 || created.is_null() {
            return Err(1);
        }
        unsafe { IDXGISwapChain1::from_raw(created) }
            .cast()
            .map_err(|_| 1)
    }
}

struct Directories {
    primary: *const c_char,
    xess: *const c_char,
    extra: *const c_char,
}
impl Directories {
    fn legacy_fsr(&self) -> *const c_char {
        if self.extra.is_null() {
            self.primary
        } else {
            self.extra
        }
    }
}

fn raw<T: Interface>(object: &Option<T>) -> Raw {
    object.as_ref().map_or(null_mut(), Interface::as_raw)
}

unsafe fn attach_sr(gpu: Raw, directory: *const c_char) -> Result<*mut rsf_sr12, i32> {
    let mut bytes = unsafe { CStr::from_ptr(directory) }.to_bytes().to_vec();
    bytes.extend_from_slice(b"\\sl.interposer.dll");
    let interposer = CString::new(bytes).map_err(|_| 1)?;
    let mut sr = rsf_sr12_setup {
        struct_size: size_of::<rsf_sr12_setup>() as u32,
        abi_version: 1,
        device: gpu,
        backend: 1,
        output_width: 1280,
        output_height: 720,
        inverted_depth: 1,
        streamline_host: unsafe { rsf_d3d11_present_host() }.cast(),
        ..Default::default()
    };
    sr.dlss.struct_size = size_of::<rsf_dlss_setup>() as u32;
    sr.dlss.abi_version = RSF_DLSS_ABI_VERSION;
    sr.dlss.interposer_path_utf8 = interposer.as_ptr();
    sr.dlss.plugin_directory_utf8 = directory;
    sr.dlss.engine = 2; // RSF_DLSS_ENGINE_UNITY.
    sr.dlss.engine_version_utf8 = c"6000.3".as_ptr();
    sr.dlss.project_id_utf8 = c"57a42c7e-faf0-4bda-a9f9-892870948ac1".as_ptr();
    sr.dlss.require_signature = 1;
    sr.dlss.log = Some(log);
    let mut result = null_mut();
    if unsafe { rsf_sr12_create(&sr, &mut result) } != 0 {
        return Err(1);
    }
    println!("PASS: native DLSS SR registered before runtime FSR/XeSS switches");
    Ok(result)
}

fn commands(
    gpu: Raw,
    used: bool,
) -> Result<Option<(ID3D12CommandAllocator, ID3D12GraphicsCommandList)>, i32> {
    if !used {
        return Ok(None);
    }
    let (mut allocator, mut list) = (null_mut(), null_mut());
    if unsafe { rsf_fg_bridge_fixture_commands(gpu, &mut allocator, &mut list) } != 0 {
        return Err(1);
    }
    Ok(Some(unsafe {
        (
            ID3D12CommandAllocator::from_raw(allocator),
            ID3D12GraphicsCommandList::from_raw(list),
        )
    }))
}

unsafe fn run(argv: &[*const c_char]) -> Result<i32, i32> {
    if !matches!(argv.len(), 5..=7) {
        println!(
            "SKIP: backend, SDK directory, vendor ID, API(11/12/12-hot), optional XeSS and Streamline directories"
        );
        return Ok(77);
    }
    let (backend, vendor) = unsafe { (number(argv[1]), number(argv[3])) };
    let debug = env_present(c"RSF_UNITY_GPU_DEBUG");
    if debug && rsf_present_fixture_debug12() != 0 {
        return Ok(77);
    }
    let mode = Mode::new(unsafe { CStr::from_ptr(argv[4]) }.to_bytes());
    let explicit = env_present(c"RSF_EXPLICIT_SWAPCHAIN");
    if explicit && !mode.native {
        return Err(1);
    }
    let (factory, adapter) = adapter::<IDXGIFactory4>(vendor, true)?;
    let class = w!("RSFSharedFG");
    let instance = register_window(class)?;
    let focus_test = mode.mfg && env_present(c"RSF_MFG_FOCUS_TEST");
    let hwnd = window(class, w!("Shared generation fixture"), instance)?;
    let mut focus = Focus(null_mut());
    let state = rsf_fg_fixture_state_create(
        u32::from(mode.service),
        u32::from(mode.hot),
        u32::from(mode.flip),
        u32::from(mode.mfg),
    );
    PROCESS_STATE.store(state, Ordering::Release);
    // On CRT registration failure, the resource lease still lives until process exit.
    let _ = unsafe { atexit(release_state) };
    let directories = Directories {
        primary: argv[2],
        xess: argv.get(5).copied().unwrap_or(argv[2]),
        extra: argv.get(6).copied().unwrap_or(null()),
    };
    if !mode.install(&directories, backend, explicit, state) {
        return Err(1);
    }

    let (dx11, context, _dx12, queue) = if mode.native {
        let (device, direct) = device12(&adapter)?;
        (None, None, Some(device), Some(direct))
    } else {
        let (device, immediate) = device11(&adapter)?;
        (Some(device), Some(immediate), None, None)
    };
    let producer = if let Some(queue) = &queue {
        queue.as_raw()
    } else {
        dx11.as_ref().unwrap().as_raw()
    };
    let chain = mode.chain(&factory, producer, hwnd, explicit)?;
    if unsafe { rsf_d3d11_present_has_owner() } == 0 {
        println!("SKIP: provider refused, original engine chain retained");
        return Ok(77);
    }
    // Prepare the deferred provider outside the interception call.
    let latency_chain = chain.cast::<IDXGISwapChain2>().map_err(|_| 1)?;
    unsafe { latency_chain.GetDesc1() }.map_err(|_| 1)?;
    let mut cached_rtv: Option<ID3D11RenderTargetView> = None;
    if mode.flip {
        let back = unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.map_err(|_| 1)?;
        unsafe {
            dx11.as_ref()
                .unwrap()
                .CreateRenderTargetView(&back, None, Some(&mut cached_rtv))
        }
        .map_err(|_| 1)?;
    }
    let application_wait = if mode.hot && mode.native {
        unsafe { latency_chain.SetMaximumFrameLatency(1) }.map_err(|_| 1)?;
        let handle = unsafe { latency_chain.GetFrameLatencyWaitableObject() };
        if handle.0.is_null() {
            return Err(1);
        }
        handle
    } else {
        HANDLE::default()
    };
    if backend == RSF_FG_BACKEND_AUTO
        && rsf_fg_fixture_expect_auto(unsafe { rsf_fg_default_backend(vendor, 0) }) == 0
    {
        return Err(1);
    }
    let mut graphics = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..Default::default()
    };
    if unsafe { rsf_d3d11_present_graphics(&mut graphics) } == 0 {
        return Err(1);
    }
    let device = graphics.native_device;
    let gpu = unsafe { ID3D12Device::from_raw_borrowed(&device) }.ok_or(1)?;
    let fsr4_directory = if mode.legacy_hot {
        directories.legacy_fsr()
    } else {
        directories.primary
    };
    let fsr4_supported = unsafe { rsf_generation_fsr4_supported(device, fsr4_directory) };
    let backend_choices = unsafe { rsf_d3d11_present_backend_choices() };
    let dlss_sr = if mode.hot && !directories.extra.is_null() {
        unsafe { attach_sr(device, directories.extra) }?
    } else {
        null_mut()
    };
    let mut inputs: rsf_fg_fixture_inputs = unsafe { std::mem::zeroed() };
    if unsafe {
        rsf_fg_bridge_fixture_allocate(
            device,
            u32::from(mode.mixed),
            u32::from(mode.hot),
            &mut inputs,
        )
    } != 0
    {
        return Err(1);
    }
    unsafe { rsf_fg_fixture_state_inputs(state, &inputs) };
    let fence =
        unsafe { gpu.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }.map_err(|_| 1)?;
    let done = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.map_err(|_| 1)?;
    let source = commands(device, mode.service)?;
    let finish = commands(device, mode.overlay)?;
    let (allocator, commands) = source
        .as_ref()
        .map_or((null_mut(), null_mut()), |(a, l)| (a.as_raw(), l.as_raw()));
    let (finish_allocator, finish_commands) = finish
        .as_ref()
        .map_or((null_mut(), null_mut()), |(a, l)| (a.as_raw(), l.as_raw()));
    let run = rsf_fg_loop {
        chain: chain.as_raw(),
        gpu: device,
        queue: raw(&queue),
        graphics_queue: graphics.queue,
        fence: fence.as_raw(),
        done: done.0,
        application_wait: application_wait.0,
        window: hwnd.0,
        device11: raw(&dx11),
        context11: raw(&context),
        cached_rtv: raw(&cached_rtv),
        allocator,
        commands,
        finish_allocator,
        finish_commands,
        backend,
        vendor,
        native12: u32::from(mode.native),
        service: u32::from(mode.service),
        hot: u32::from(mode.hot),
        legacy_hot: u32::from(mode.legacy_hot),
        flip11: u32::from(mode.flip),
        mfg: u32::from(mode.mfg),
        overlay: u32::from(mode.overlay),
        with_streamline: u32::from(!directories.extra.is_null()),
        focus_test: u32::from(focus_test),
        fsr4_supported,
        backend_choices,
        log: Some(log),
        state: state.cast(),
    };
    let result = unsafe { rsf_fg_fixture_run_loop(&run, &mut focus.0) };
    // Keep the original failure path: an incomplete GPU loop does not ask the SDK to tear down.
    if result < 0 {
        return Err(1);
    }
    let _ = unsafe { CloseHandle(done) };
    if result == 0 && mode.flip {
        println!("PASS: DX11 cached buffer-zero view reached every physical presentation");
    }
    if !application_wait.0.is_null() {
        let _ = unsafe { CloseHandle(application_wait) };
        if result == 0 {
            println!("PASS: one application latency handle survived provider transitions");
        }
    }
    unsafe { rsf_sr12_destroy(dlss_sr) };
    if mode.overlay {
        unsafe { rsf_overlay_d3d12_stop() };
    }
    if debug {
        let accepted = unsafe { rsf_present_fixture_debug_errors(device) } == 0 && result == 0;
        println!(
            "D3D12 overlay/presentation validation={}",
            u32::from(accepted)
        );
        if !accepted {
            return Err(1);
        }
    }
    Ok(result)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_fixture_main(argc: i32, argv: *const *const c_char) -> i32 {
    unsafe { run(args(argc, argv)) }.unwrap_or_else(|status| status)
}
