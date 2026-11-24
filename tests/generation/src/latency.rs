// SPDX-License-Identifier: MIT
use crate::device::{self, SKIP, Window};
use crate::session::Raw;
use crate::sys::*;
use crate::sys::{
    RSF_STREAMLINE_HOST_ABI_VERSION, rsf_streamline_graphics, rsf_streamline_host_setup,
};
use rsf_game_sys::backend_result::{NOT_COMPILED, NOT_READY, NOT_SUPPORTED, OK};
use rsf_test_support::d3d12::transition_barrier;
use std::ffi::{CStr, c_char, c_int};
use std::ptr::null_mut;
use windows::Win32::Graphics::{
    Direct3D12::*,
    Dxgi::{Common::DXGI_FORMAT_R8G8B8A8_UNORM, *},
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SPACE};
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW;
use windows::core::{Interface, w};

unsafe extern "C" {
    fn rsf_streamline_host_create(setup: &rsf_streamline_host_setup, out: &mut Raw) -> i32;
    fn rsf_streamline_host_graphics(host: Raw, out: &mut rsf_streamline_graphics) -> i32;
    fn rsf_streamline_host_upgrade_chain(host: Raw, chain: &mut Raw) -> i32;
    fn rsf_streamline_host_reflex(host: Raw, mode: u32, interval: u32) -> i32;
    fn rsf_streamline_host_acquire(host: Raw, id: u64) -> i32;
    fn rsf_streamline_host_latency_status(
        host: Raw,
        out: &mut rsf_streamline_latency_status,
    ) -> i32;
    fn rsf_streamline_host_marker(host: Raw, id: u64, marker: u32, controller: u32) -> i32;
    fn rsf_streamline_host_input(host: Raw, id: u64, kinds: u32, message: u32) -> i32;
    fn rsf_streamline_host_sleep(host: Raw, id: u64) -> i32;
    fn rsf_streamline_host_reset_sources(host: Raw) -> i32;
    fn rsf_streamline_host_abort(host: Raw, id: u64) -> i32;
    fn rsf_streamline_host_destroy(host: Raw) -> i32;
}

unsafe extern "C" fn log_message(_: Raw, message: *const c_char) {
    println!(
        "SDK: {}",
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    );
}

fn status(host: Raw) -> Result<rsf_streamline_latency_status, i32> {
    let mut state = rsf_streamline_latency_status {
        struct_size: size_of::<rsf_streamline_latency_status>() as u32,
        ..Default::default()
    };
    check(unsafe { rsf_streamline_host_latency_status(host, &mut state) })?;
    Ok(state)
}

fn check(result: i32) -> Result<(), i32> {
    if result == OK { Ok(()) } else { Err(1) }
}

// The RSF_LATENCY_* values of rescaleframe/game_frame.h (casts in macros, which bindgen skips).
const LATENCY_SIMULATION_START: i32 = 0;
const LATENCY_SIMULATION_END: i32 = 1;
const LATENCY_RENDER_SUBMIT_START: i32 = 2;
const LATENCY_RENDER_SUBMIT_END: i32 = 3;
const LATENCY_PRESENT_START: i32 = 4;
const LATENCY_PRESENT_END: i32 = 5;
const LATENCY_INPUT_SAMPLE: i32 = 6;

fn marker(host: Raw, id: u64, marker: i32, controller: u32) -> Result<(), i32> {
    check(unsafe { rsf_streamline_host_marker(host, id, marker as u32, controller) })
}

fn record(
    list: &ID3D12GraphicsCommandList,
    buffer: &ID3D12Resource,
    rtv: D3D12_CPU_DESCRIPTOR_HANDLE,
    simulation: f32,
) {
    let transition = |from, to| transition_barrier(buffer, from, to);
    unsafe {
        list.ResourceBarrier(&[transition(
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        )]);
        list.ClearRenderTargetView(rtv, &[simulation, 0.1, 0.2, 1.0], None);
        list.ResourceBarrier(&[transition(
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_PRESENT,
        )]);
    }
}

fn render(host: Raw, profile: u32) -> Result<(), i32> {
    let mut graphics: rsf_streamline_graphics = unsafe { std::mem::zeroed() };
    graphics.struct_size = size_of::<rsf_streamline_graphics>() as u32;
    check(unsafe { rsf_streamline_host_graphics(host, &mut graphics) })?;
    let device = unsafe { ID3D12Device::from_raw_borrowed(&graphics.device) }.ok_or(1)?;
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&graphics.queue) }.ok_or(1)?;
    let factory = unsafe { IDXGIFactory2::from_raw_borrowed(&graphics.factory) }.ok_or(1)?;
    let window = Window::new(
        w!("RSFLatencyFixture"),
        w!("Latency service fixture"),
        WS_OVERLAPPEDWINDOW,
        256,
        256,
    )
    .map_err(|_| 1)?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: 256,
        Height: 256,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferCount: 3,
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        ..Default::default()
    };
    let first = unsafe { factory.CreateSwapChainForHwnd(queue, window.handle, &desc, None, None) }
        .map_err(|_| 1)?;
    let mut raw = first.into_raw();
    let upgraded = unsafe { rsf_streamline_host_upgrade_chain(host, &mut raw) };
    if raw.is_null() {
        return Err(1);
    }
    let chain = unsafe { IDXGISwapChain1::from_raw(raw) };
    check(upgraded)?;
    let swap: IDXGISwapChain4 = chain.cast().map_err(|_| 1)?;
    drop(chain);
    let allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.map_err(|_| 1)?;
    let list: ID3D12GraphicsCommandList =
        unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }
            .map_err(|_| 1)?;
    unsafe { list.Close() }.map_err(|_| 1)?;
    let command: ID3D12CommandList = list.cast().map_err(|_| 1)?;
    let fence: ID3D12Fence =
        unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.map_err(|_| 1)?;
    let heap: ID3D12DescriptorHeap = unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: 1,
            ..Default::default()
        })
    }
    .map_err(|_| 1)?;
    let rtv = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
    let mut simulation = 0.0;
    for frame in 0..12 {
        if profile == RSF_SL_PROFILE_REFLEX && frame % 4 == 0 {
            let result = unsafe { rsf_streamline_host_reflex(host, frame / 4, 0) };
            if result != OK {
                println!("Reflex mode {} refused={result}", frame / 4);
                return Err(if result == NOT_SUPPORTED { SKIP } else { 1 });
            }
        }
        let id = (1u64 << 32) + u64::from(frame) + 1;
        check(unsafe { rsf_streamline_host_acquire(host, id) })?;
        let message = status(host)?.pcl_message_id;
        if message == 0 {
            return Err(1);
        }
        // Exercise both controller marker routes, with the ping still attached to the receipt.
        if frame % 2 == 0 {
            marker(host, id, LATENCY_INPUT_SAMPLE, 1)?;
        }
        for _ in 0..2 {
            check(unsafe {
                rsf_streamline_host_input(
                    host,
                    id,
                    rsf_game_sys::RSF_GAME_INPUT_CONTROLLER,
                    message,
                )
            })?;
        }
        marker(host, id, LATENCY_INPUT_SAMPLE, 1)?;
        check(unsafe { rsf_streamline_host_sleep(host, id) })?;
        let pressed = unsafe { GetAsyncKeyState(i32::from(VK_SPACE.0)) } < 0;
        marker(host, id, LATENCY_INPUT_SAMPLE, 0)?;
        marker(host, id, LATENCY_SIMULATION_START, 0)?;
        simulation += if pressed { 0.02 } else { 0.01 };
        marker(host, id, LATENCY_SIMULATION_END, 0)?;
        marker(host, id, LATENCY_RENDER_SUBMIT_START, 0)?;
        let buffer: ID3D12Resource =
            unsafe { swap.GetBuffer(swap.GetCurrentBackBufferIndex()) }.map_err(|_| 1)?;
        unsafe {
            allocator.Reset().map_err(|_| 1)?;
            list.Reset(&allocator, None).map_err(|_| 1)?;
            device.CreateRenderTargetView(&buffer, None, rtv);
        }
        record(&list, &buffer, rtv, simulation);
        unsafe {
            list.Close().map_err(|_| 1)?;
            queue.ExecuteCommandLists(&[Some(command.clone())]);
        }
        let presented = (|| {
            marker(host, id, LATENCY_RENDER_SUBMIT_END, 0)?;
            marker(host, id, LATENCY_PRESENT_START, 0)?;
            unsafe { swap.Present(0, DXGI_PRESENT(0)).ok() }.map_err(|_| 1)?;
            marker(host, id, LATENCY_PRESENT_END, 0)
        })();
        if !device::drain(queue, &fence, u64::from(frame) + 1) {
            std::process::exit(1);
        }
        presented?;
    }
    let state = status(host)?;
    println!(
        "profile={} sleepCalls={} markerCalls={} lastID={} ReflexAvailable={} latencyReport={}",
        state.profile,
        state.sleep_calls,
        state.marker_calls,
        state.last_begin_id,
        state.low_latency_available,
        state.latency_report_available,
    );
    if state.sleep_calls != if profile == RSF_SL_PROFILE_PCL { 0 } else { 12 }
        || state.marker_calls != 96
    {
        return Err(1);
    }
    check(unsafe { rsf_streamline_host_reset_sources(host) })?;
    check(unsafe { rsf_streamline_host_acquire(host, 1) })?;
    let reset = unsafe { rsf_streamline_host_reset_sources(host) };
    check(unsafe { rsf_streamline_host_abort(host, 1) })?;
    if reset != NOT_READY {
        return Err(1);
    }
    check(unsafe { rsf_streamline_host_reset_sources(host) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_latency_host(argc: c_int, argv: *const *const c_char) -> i32 {
    if argc != 4 {
        println!("SKIP: supply runtime directory, vendor ID and profile (1 Reflex / 2 PCL)");
        return SKIP;
    }
    let args = unsafe { std::slice::from_raw_parts(argv, 4) };
    let vendor = unsafe { device::numeric_arg(args[2]) };
    let profile = unsafe { device::numeric_arg(args[3]) };
    let Ok(factory) = device::factory() else {
        return 1;
    };
    let adapter = match device::adapter(&factory, vendor) {
        Ok(Some(adapter)) => adapter,
        Ok(None) => return SKIP,
        Err(_) => return 1,
    };
    let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
        return 1;
    };
    println!(
        "adapter vendor={:04x} device={:04x}",
        desc.VendorId, desc.DeviceId
    );
    let setup = rsf_streamline_host_setup {
        struct_size: size_of::<rsf_streamline_host_setup>() as u32,
        abi_version: RSF_STREAMLINE_HOST_ABI_VERSION,
        runtime_directory_utf8: args[1],
        dxgi_adapter: adapter.as_raw(),
        engine_type: 0,
        engine_version_utf8: c"ReScaleFrame latency fixture".as_ptr(),
        project_id_utf8: c"a3ed1f08-3542-4698-b85c-e1a9908e861a".as_ptr(),
        require_signature: 1,
        development_runtime: 0,
        log: Some(log_message),
        log_user: null_mut(),
        profile,
        load_dlss_sr: 0,
    };
    let mut host = null_mut();
    let result = unsafe { rsf_streamline_host_create(&setup, &mut host) };
    println!("host result={result} profile={profile}");
    if result != OK {
        return if result == NOT_COMPILED { SKIP } else { 1 };
    }
    let result = render(host, profile);
    let destroyed = unsafe { rsf_streamline_host_destroy(host) };
    if let Err(exit_code) = result {
        return exit_code;
    }
    if destroyed != OK {
        return 1;
    }
    println!(
        "Independent latency service API counts, 64-bit lineage and plain Present passed; ETW/physical latency NOT TESTED"
    );
    0
}
