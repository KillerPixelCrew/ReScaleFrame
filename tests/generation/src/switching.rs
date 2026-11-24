// SPDX-License-Identifier: MIT
use crate::device::{self, HEIGHT, SKIP, WIDTH, Window};
use crate::session::{Raw, rsf_fg_session_create, rsf_fg_session_destroy, rsf_fg_session_select};
use crate::sys::*;
use rsf_game_sys::backend_result::{FEATURE_FAILED, INIT_FAILED, NOT_READY, OK};
use rsf_test_support::d3d12::wait_for_signal;
use rsf_test_support::hardware_adapter;
use std::ffi::{c_char, c_int};
use std::ptr::{from_mut, null, null_mut};
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW;
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Direct3D12::*,
        Dxgi::{Common::*, *},
    },
};
use windows::core::{Interface, w};

struct Host {
    plain: Option<IDXGISwapChain1>,
    fence: ID3D12Fence,
    queue: ID3D12CommandQueue,
    device: ID3D12Device,
    factory: IDXGIFactory4,
    borrowed: Raw,
    hwnd: HWND,
    sequence: u64,
}

unsafe extern "C" fn quiesce(user: Raw) -> i32 {
    let host = unsafe { &mut *user.cast::<Host>() };
    host.sequence += 1;
    if wait_for_signal(&host.queue, &host.fence, host.sequence, 15_000) {
        OK
    } else {
        FEATURE_FAILED
    }
}

extern "C" fn resume(_: Raw) {}

unsafe extern "C" fn release(user: Raw) -> i32 {
    let host = unsafe { &mut *user.cast::<Host>() };
    host.borrowed = null_mut();
    host.plain = None;
    OK
}

unsafe extern "C" fn adopt(user: Raw, chain: Raw) -> i32 {
    let host = unsafe { &mut *user.cast::<Host>() };
    if chain.is_null() || host.plain.is_some() || !host.borrowed.is_null() {
        return NOT_READY;
    }
    host.borrowed = chain;
    OK
}

unsafe extern "C" fn plain(user: Raw) -> i32 {
    let host = unsafe { &mut *user.cast::<Host>() };
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: WIDTH,
        Height: HEIGHT,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferCount: 3,
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        ..Default::default()
    };
    match unsafe {
        host.factory.CreateSwapChainForHwnd(
            &host.queue,
            host.hwnd,
            &desc,
            None,
            None::<&IDXGIOutput>,
        )
    } {
        Ok(chain) => {
            host.plain = Some(chain);
            OK
        }
        Err(_) => INIT_FAILED,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_fg_switching(argc: c_int, argv: *const *const c_char) -> i32 {
    if argc != 4 {
        println!("SKIP: supply FFX directory, XeFG directory and adapter vendor ID");
        return SKIP;
    }
    let argv = unsafe { std::slice::from_raw_parts(argv, 4) };
    let vendor = unsafe { device::numeric_arg(argv[3]) };
    let Ok(factory) = device::factory() else {
        return 1;
    };
    let adapter = match hardware_adapter(&factory, vendor) {
        Ok(Some(adapter)) => adapter,
        Ok(None) => return SKIP,
        Err(_) => return 1,
    };
    let Ok((device, queue)) = device::device_and_queue(&adapter) else {
        return 1;
    };
    let Ok(fence) = (unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }) else {
        return 1;
    };
    let Ok(window) = Window::new(
        w!("RSFFGSwitching"),
        w!("FG owner switching fixture"),
        WS_OVERLAPPEDWINDOW,
        WIDTH as i32,
        HEIGHT as i32,
    ) else {
        return 1;
    };
    let mut host = Host {
        plain: None,
        fence,
        queue,
        device,
        factory,
        borrowed: null_mut(),
        hwnd: window.handle,
        sequence: 0,
    };
    let callbacks = rsf_fg_host {
        struct_size: size_of::<rsf_fg_host>() as u32,
        abi_version: RSF_FG_ABI_VERSION,
        user: from_mut(&mut host).cast(),
        quiesce: Some(quiesce),
        resume: Some(resume),
        release: Some(release),
        adopt: Some(adopt),
        restore_plain: Some(plain),
    };
    let mut session = null_mut();
    if unsafe { rsf_fg_session_create(&callbacks, &mut session) } != OK {
        return 1;
    }
    let mut setup = device::setup(host.hwnd, host.device.as_raw(), host.queue.as_raw(), 1);
    let options = device::disabled_options();
    for backend in [
        RSF_FG_BACKEND_FSR3,
        RSF_FG_BACKEND_XESS,
        RSF_FG_BACKEND_FSR3,
    ] {
        setup.feature_major = backend;
        setup.runtime_directory_utf8 = if backend == RSF_FG_BACKEND_XESS {
            argv[2]
        } else {
            argv[1]
        };
        let provider = unsafe { device::rsf_fg_get_provider(backend) };
        if provider.is_null() {
            eprintln!("FAIL: missing provider for backend {backend}");
            std::process::exit(1);
        }
        let result = unsafe { rsf_fg_session_select(session, provider, &setup, &options) };
        println!("select backend={backend} result={result}");
        // If retirement was refused, the live session and native objects still belong together.
        if device::unavailable(result) {
            std::process::exit(SKIP);
        }
        if result != OK
            || unsafe { device::disabled_present(host.borrowed) }.is_none_or(|hr| hr < 0)
        {
            std::process::exit(1);
        }
    }
    if unsafe { rsf_fg_session_select(session, null(), null(), null()) } != OK
        || host.plain.is_none()
    {
        std::process::exit(1);
    }
    if unsafe { rsf_fg_session_destroy(session) } != OK {
        std::process::exit(1);
    }
    drop(host.plain.take());
    drop(window);
    drop(host);
    println!("FSR3 -> XeFG -> FSR3 -> plain ownership, disabled Present and teardown passed");
    0
}
