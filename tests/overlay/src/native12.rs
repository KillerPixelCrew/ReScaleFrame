// SPDX-License-Identifier: MIT
// Block the real WARP queue, fill all three overlay slots, then allow their fences to complete.
use super::{Handle, Window, check, require};
use crate::sys::{rsf_overlay_intent, rsf_overlay_stats};
use std::{ffi::CStr, thread, time::Duration};
use windows::{
    Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0},
        Graphics::{
            Direct3D::D3D_FEATURE_LEVEL_11_0,
            Direct3D12::{
                D3D12_COMMAND_QUEUE_DESC, D3D12_FENCE_FLAG_NONE, D3D12_MESSAGE,
                D3D12_MESSAGE_SEVERITY_ERROR, D3D12CreateDevice, D3D12GetDebugInterface,
                ID3D12CommandQueue, ID3D12Debug, ID3D12Device, ID3D12Fence, ID3D12InfoQueue,
            },
            Dxgi::{
                Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
                CreateDXGIFactory1, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_DISCARD,
                DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIFactory4, IDXGIOutput,
            },
        },
        System::Threading::{CreateEventW, WaitForSingleObject},
    },
    core::{Interface, PCWSTR, w},
};

type Present = unsafe extern "C" fn(
    device: Handle,
    queue: Handle,
    chain: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
) -> i32;

fn debug_device() -> Option<(IDXGIFactory4, ID3D12Device)> {
    let mut debug: Option<ID3D12Debug> = None;
    unsafe { D3D12GetDebugInterface(&mut debug) }.ok()?;
    unsafe { debug?.EnableDebugLayer() };
    let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory1() }.ok()?;
    let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.ok()?;
    let mut device = None;
    unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.ok()?;
    Some((factory, device?))
}

fn verify_no_debug_errors(device: &ID3D12Device) {
    let info: ID3D12InfoQueue = check(device.cast(), "get D3D12 debug queue");
    for index in 0..unsafe { info.GetNumStoredMessages() } {
        let mut bytes = 0;
        check(
            unsafe { info.GetMessage(index, None, &mut bytes) },
            "read message size",
        );
        require(
            bytes >= size_of::<D3D12_MESSAGE>(),
            "debug message is complete",
        );
        // The SDK writes a header followed by its description. Use pointer-aligned storage.
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        check(
            unsafe { info.GetMessage(index, Some(message), &mut bytes) },
            "read D3D12 message",
        );
        let message = unsafe { &*message };
        if message.Severity.0 <= D3D12_MESSAGE_SEVERITY_ERROR.0 {
            let description = unsafe { CStr::from_ptr(message.pDescription.cast()) };
            eprintln!("{}", description.to_string_lossy());
            require(false, "D3D12 debug queue contains an error");
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_overlay_native12(draw: Present) -> i32 {
    let mut args = std::env::args_os();
    require(args.len() == 2, "pass the deterministic overlay panel DLL");
    // Set before creation of the D3D runtime and its worker threads.
    unsafe { std::env::set_var("RSF_OVERLAY_DLL", args.nth(1).expect("panel DLL path")) };
    let Some((factory, device)) = debug_device() else {
        return 77;
    };
    let queue: ID3D12CommandQueue = check(
        unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default()) },
        "create D3D12 queue",
    );
    let window = Window::new(w!("RSF UI backpressure"), 100, 100);
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: 80,
        Height: 80,
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
    let chain = check(
        unsafe {
            factory.CreateSwapChainForHwnd(&queue, window.hwnd, &desc, None, None::<&IDXGIOutput>)
        },
        "create D3D12 swap chain",
    );
    let stats = rsf_overlay_stats {
        struct_size: size_of::<rsf_overlay_stats>() as u32,
        show_performance_hud: 1,
        ..Default::default()
    };
    let mut intent = rsf_overlay_intent {
        struct_size: size_of::<rsf_overlay_intent>() as u32,
        ..Default::default()
    };
    let mut present = || unsafe {
        draw(
            device.as_raw(),
            queue.as_raw(),
            chain.as_raw(),
            &stats,
            &mut intent,
        ) != 0
    };

    let blocked: ID3D12Fence = check(
        unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) },
        "create blocking fence",
    );
    check(unsafe { queue.Wait(&blocked, 1) }, "block queue");
    for _ in 0..3 {
        if !present() {
            let _ = unsafe { blocked.Signal(1) };
            require(false, "submit one overlay frame per slot");
        }
    }
    // CPU signals only this test-owned fence. The fourth Present waits for its own submitted work.
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        check(unsafe { blocked.Signal(1) }, "unblock queue");
    });
    let drawn = present();
    check(
        release.join().map_err(|_| "queue release thread panicked"),
        "join queue release",
    );

    let drained: ID3D12Fence = check(
        unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) },
        "create drain fence",
    );
    check(unsafe { queue.Signal(&drained, 1) }, "signal drain fence");
    let event = check(
        unsafe { CreateEventW(None, false, false, PCWSTR::null()) },
        "create drain event",
    );
    let event = unsafe { windows::core::Owned::<HANDLE>::new(event) };
    check(
        unsafe { drained.SetEventOnCompletion(1, *event) },
        "arm drain event",
    );
    let waited = unsafe { WaitForSingleObject(*event, 5_000) };
    require(
        drawn && waited == WAIT_OBJECT_0,
        "fourth overlay submission drawn and retired",
    );
    verify_no_debug_errors(&device);
    println!("Four UI submissions retained across a full GPU ring; D3D12 validation passed.");
    0
}
