// SPDX-License-Identifier: MIT
use crate::device::{self, HEIGHT, SKIP, WIDTH, Window};
use crate::session::{OK, Raw};
use crate::sys::rsf_fg_retirement;
use crate::sys::*;
use std::ffi::{CStr, c_char, c_int};
use std::mem::MaybeUninit;
use std::ptr::null_mut;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::IDXGIAdapter1};
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW;
use windows::core::{Interface, PCSTR, w};

// An ABI-identical raw-pointer view of exports another module declares with references.
#[allow(clashing_extern_declarations)]
unsafe extern "C" {
    fn rsf_streamline_host_create(setup: *const rsf_streamline_host_setup, host: *mut Raw) -> i32;
    fn rsf_streamline_host_graphics(host: Raw, graphics: *mut rsf_streamline_graphics) -> i32;
    fn rsf_streamline_host_destroy(host: Raw) -> i32;
}

unsafe extern "C" fn log_message(_: Raw, message: *const c_char) {
    if !message.is_null() {
        println!(
            "SDK: {}",
            unsafe { CStr::from_ptr(message) }.to_string_lossy()
        );
    }
}

unsafe extern "system" fn debug_message(
    _: D3D12_MESSAGE_CATEGORY,
    severity: D3D12_MESSAGE_SEVERITY,
    _: D3D12_MESSAGE_ID,
    message: PCSTR,
    _: Raw,
) {
    if severity.0 <= D3D12_MESSAGE_SEVERITY_WARNING.0 && !message.0.is_null() {
        println!(
            "D3D12: {}",
            unsafe { CStr::from_ptr(message.0.cast()) }.to_string_lossy()
        );
    }
}

struct DebugLog {
    queue: ID3D12InfoQueue1,
    cookie: u32,
}

impl DebugLog {
    fn register(device: &ID3D12Device) -> Option<Self> {
        let queue = device.cast::<ID3D12InfoQueue1>().ok()?;
        let mut cookie = 0;
        unsafe {
            let _ = queue.SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_CORRUPTION, false);
            let _ = queue.SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_ERROR, false);
            queue
                .RegisterMessageCallback(
                    Some(debug_message),
                    D3D12_MESSAGE_CALLBACK_FLAG_NONE,
                    null_mut(),
                    &mut cookie,
                )
                .ok()?;
        }
        Some(Self { queue, cookie })
    }
}

impl Drop for DebugLog {
    fn drop(&mut self) {
        let _ = unsafe { self.queue.UnregisterMessageCallback(self.cookie) };
    }
}

fn dump_last_messages(device: &ID3D12Device) {
    let Ok(info) = device.cast::<ID3D12InfoQueue>() else {
        return;
    };
    let count = unsafe { info.GetNumStoredMessages() };
    for index in count.saturating_sub(12)..count {
        let mut bytes = 0;
        let _ = unsafe { info.GetMessage(index, None, &mut bytes) };
        if bytes < size_of::<D3D12_MESSAGE>() {
            continue;
        }
        // Use word-aligned storage for both the descriptor and the appended string.
        let words = bytes.div_ceil(size_of::<usize>());
        let mut storage = vec![MaybeUninit::<usize>::uninit(); words];
        let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        if unsafe { info.GetMessage(index, Some(message), &mut bytes) }.is_ok() {
            let description = unsafe { (*message).pDescription };
            if !description.is_null() {
                println!(
                    "D3D12: {}",
                    unsafe { CStr::from_ptr(description.cast()) }.to_string_lossy()
                );
            }
        }
    }
}

enum GraphicsOwner {
    Native {
        _queue: ID3D12CommandQueue,
        _device: ID3D12Device,
    },
    Streamline(Raw),
}

impl GraphicsOwner {
    fn host(&self) -> Raw {
        match self {
            Self::Streamline(host) => *host,
            Self::Native { .. } => null_mut(),
        }
    }

    fn finish(self) -> i32 {
        match self {
            Self::Streamline(host) => unsafe { rsf_streamline_host_destroy(host) },
            Self::Native { .. } => OK,
        }
    }
}

fn graphics(
    backend: u32,
    directory: *const c_char,
    adapter: &IDXGIAdapter1,
) -> Result<(GraphicsOwner, rsf_streamline_graphics), i32> {
    let mut info = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let owner = if backend == RSF_FG_BACKEND_DLSS {
        let setup = rsf_streamline_host_setup {
            struct_size: size_of::<rsf_streamline_host_setup>() as u32,
            abi_version: RSF_STREAMLINE_HOST_ABI_VERSION,
            runtime_directory_utf8: directory,
            dxgi_adapter: adapter.as_raw(),
            engine_type: 0,
            engine_version_utf8: c"ReScaleFrame fixture".as_ptr(),
            project_id_utf8: c"a3ed1f08-3542-4698-b85c-e1a9908e861a".as_ptr(),
            require_signature: 1,
            development_runtime: 0,
            log: Some(log_message),
            log_user: null_mut(),
            profile: RSF_SL_PROFILE_DLSS_FG,
            load_dlss_sr: 1,
        };
        let mut host = null_mut();
        let result = unsafe { rsf_streamline_host_create(&setup, &mut host) };
        println!("Streamline early host result={result}");
        if result != OK {
            return Err(if device::unavailable(result) { SKIP } else { 1 });
        }
        if unsafe { rsf_streamline_host_graphics(host, &mut info) } != OK {
            // The foreign context may still own graphics even though the snapshot was refused.
            std::process::exit(1);
        }
        GraphicsOwner::Streamline(host)
    } else {
        let (device, queue) = device::device_and_queue(adapter).map_err(|_| 1)?;
        info.device = device.as_raw();
        info.queue = queue.as_raw();
        info.native_device = info.device;
        info.native_queue = info.queue;
        GraphicsOwner::Native {
            _device: device,
            _queue: queue,
        }
    };
    Ok((owner, info))
}

unsafe fn exercise(
    provider: &rsf_generation_provider,
    setup: &rsf_generation_setup,
    device12: &ID3D12Device,
    queue: &ID3D12CommandQueue,
) -> i32 {
    let (Some(create), Some(status), Some(configure), Some(after), Some(retire), Some(destroy)) = (
        provider.create,
        provider.status,
        provider.configure,
        provider.after_present,
        provider.retirement,
        provider.destroy,
    ) else {
        return 1;
    };
    let (mut context, mut chain) = (null_mut(), null_mut());
    let result = unsafe { create(setup, &mut context, &mut chain) };
    println!(
        "provider create result={result} chain={}",
        u32::from(!chain.is_null())
    );
    if result != OK {
        return if device::unavailable(result) { SKIP } else { 1 };
    }
    if context.is_null() {
        std::process::exit(1);
    }

    let mut state = rsf_fg_status {
        struct_size: size_of::<rsf_fg_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let result = unsafe { status(context, &mut state) };
    let version = state
        .version_name
        .iter()
        .map(|&byte| byte as u8)
        .take_while(|&byte| byte != 0)
        .collect::<Vec<_>>();
    println!(
        "status result={result} supported={} maxGenerated={} dynamic={} minDimension={} version={}",
        state.supported,
        state.max_generated_frames,
        state.dynamic_supported,
        state.min_dimension,
        String::from_utf8_lossy(&version)
    );
    let mut exit_code = i32::from(
        result != OK
            || chain.is_null()
            || unsafe { configure(context, &device::disabled_options()) } != OK,
    );
    if exit_code == 0 {
        let Some(presented) = (unsafe { device::disabled_present(chain) }) else {
            std::process::exit(1);
        };
        println!("disabled Present HRESULT={:08x}", presented as u32);
        let after_present = unsafe { after(context) };
        if presented < 0 || after_present != OK {
            exit_code = 1;
        }
    }

    let fence = unsafe { device12.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) };
    if !fence
        .as_ref()
        .is_ok_and(|fence| device::drain(queue, fence, 1))
    {
        let removed = unsafe { device12.GetDeviceRemovedReason() }
            .map_or_else(|error| error.code().0, |()| 0);
        println!(
            "FAIL: GPU did not retire, removed={:08x}; context retained until process exits",
            removed as u32
        );
        dump_last_messages(device12);
        std::process::exit(1);
    }
    let mut retirement = rsf_fg_retirement {
        struct_size: size_of::<rsf_fg_retirement>() as u32,
        fence: null_mut(),
        value: 0,
    };
    if unsafe { retire(context, &mut retirement) } != OK {
        std::process::exit(1);
    }
    if let Some(fence) = unsafe { ID3D12Fence::from_raw_borrowed(&retirement.fence) } {
        let completed = unsafe { fence.GetCompletedValue() };
        if completed == u64::MAX || completed < retirement.value {
            // Keep all provider and host resources alive until process exit.
            std::process::exit(1);
        }
    }
    unsafe { destroy(context) };
    exit_code
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_fg_provider(argc: c_int, argv: *const *const c_char) -> i32 {
    if argc != 4 {
        println!("SKIP: supply backend (1/3/4/5), runtime directory, vendor ID");
        return SKIP;
    }
    let args = unsafe { std::slice::from_raw_parts(argv, 4) };
    let backend = unsafe { device::numeric_arg(args[1]) };
    let vendor = unsafe { device::numeric_arg(args[3]) };
    let provider = unsafe { device::rsf_fg_get_provider(backend).as_ref() };
    let Some(provider) = provider else { return 1 };

    let mut debug: Option<ID3D12Debug> = None;
    if unsafe { D3D12GetDebugInterface(&mut debug) }.is_ok()
        && let Some(debug) = &debug
    {
        unsafe { debug.EnableDebugLayer() };
    }
    let Ok(factory) = device::factory() else {
        return 1;
    };
    let adapter = match device::adapter(&factory, vendor) {
        Ok(Some(adapter)) => adapter,
        Ok(None) => {
            println!("SKIP: selected hardware unavailable");
            return SKIP;
        }
        Err(_) => return 1,
    };
    let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
        return 1;
    };
    println!(
        "device vendor={:04x} device={:04x} backend={backend}",
        desc.VendorId, desc.DeviceId
    );
    let (owner, graphics) = match graphics(backend, args[2], &adapter) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (Some(native_device), Some(native_queue)) = (unsafe {
        (
            ID3D12Device::from_raw_borrowed(&graphics.native_device),
            ID3D12CommandQueue::from_raw_borrowed(&graphics.native_queue),
        )
    }) else {
        std::process::exit(1)
    };
    let debug_log = DebugLog::register(native_device);
    let Ok(window) = Window::new(
        w!("RSFFGFixture"),
        w!("FG lifecycle fixture"),
        WS_OVERLAPPEDWINDOW,
        WIDTH as i32,
        HEIGHT as i32,
    ) else {
        drop(debug_log);
        let _ = owner.finish();
        return 1;
    };
    let mut setup = device::setup(window.handle, graphics.device, graphics.queue, 5);
    setup.feature_major = backend;
    setup.runtime_directory_utf8 = args[2];
    setup.streamline_host = owner.host();
    setup.chain.log = Some(log_message);
    let mut result = unsafe { exercise(provider, &setup, native_device, native_queue) };
    drop(window);
    drop(debug_log);
    if owner.finish() != OK {
        result = 1;
    }
    println!("lifecycle result={result}; disabled Present coverage only");
    result
}
