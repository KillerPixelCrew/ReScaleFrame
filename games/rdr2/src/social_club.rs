// SPDX-License-Identifier: MIT
//! Keep RAGE's supplied backbuffer index when Social Club records into RAGE's command list.
//! The guarded renderer bytes and continuation addresses are from rdr2-renderer.md.
use crate::asi::{self, AsiPlatform, MH_ERROR_ALREADY_INITIALIZED, MH_OK};
use crate::layout::{asi::SOCIAL_CLUB_DISPATCHER, social_club as profile};
use crate::platform::lock;
use crate::runtime_worker::log;
use std::ffi::{CStr, c_void};
use std::ptr::{self, null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_READWRITE, VirtualAlloc,
    VirtualFree, VirtualProtect,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::core::w;

// The native leaf asserts these status values against the pinned MinHook header.
const MH_ERROR_ENABLED: i32 = 5;
const MH_ERROR_MEMORY_PROTECT: i32 = 10;

#[repr(C)]
#[derive(Clone, Copy)]
struct Platform {
    create_dispatch: unsafe extern "C" fn(usize) -> i32,
    create_trace: unsafe extern "C" fn(usize) -> i32,
    enable: unsafe extern "C" fn(usize) -> i32,
    current_index: unsafe extern "C" fn(*mut c_void, *mut u32) -> bool,
}

const _: () = assert!(size_of::<Platform>() == 4 * size_of::<usize>());

struct Options {
    platform: Platform,
    trace: bool,
}

static OPTIONS: OnceLock<Options> = OnceLock::new();
static ATTEMPTED: Mutex<bool> = Mutex::new(false);
static REPAIRED: AtomicBool = AtomicBool::new(false);
static OBSERVED: AtomicU32 = AtomicU32::new(0);
static MISMATCHED: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_social_club_install(platform: *const Platform, trace: bool) -> bool {
    let (Some(asi), Some(platform)) = (asi::platform(), unsafe { platform.as_ref() }) else {
        return false;
    };
    let image = unsafe { (asi.main_module)() };
    let Some(site) = image
        .checked_add(SOCIAL_CLUB_DISPATCHER as usize)
        .filter(|_| image != 0)
    else {
        return false;
    };
    if !asi.matches(site, &profile::DISPATCH_BYTES) {
        return false;
    }
    let initialized = unsafe { (asi.hook_initialize)() };
    if initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED {
        return false;
    }
    // Publish the leaves before the dispatch detour can run.
    if OPTIONS
        .set(Options {
            platform: *platform,
            trace,
        })
        .is_err()
    {
        return false;
    }
    unsafe { (platform.create_dispatch)(site) == MH_OK && (platform.enable)(site) == MH_OK }
}

fn renderer_matches(asi: &AsiPlatform, base: usize) -> bool {
    asi::pe_header(asi, base).is_some_and(|(_, signature, timestamp, size)| {
        signature == asi::PE_SIGNATURE
            && timestamp == profile::IMAGE_TIMESTAMP
            && size == profile::IMAGE_SIZE
    }) && asi.matches(base + profile::GATE, &profile::GATE_BYTES)
        && asi.matches(base + profile::ENTRY, &profile::ENTRY_BYTES)
}

fn relay(base: usize) -> [u8; 43] {
    let mut code = profile::RELAY;
    let continuation = ((base + profile::AFTER_COMPARISON) as u64).to_le_bytes();
    code[14..22].copy_from_slice(&continuation);
    code[35..43].copy_from_slice(&continuation);
    code
}

fn status_text(asi: &AsiPlatform, status: i32) -> String {
    let text = unsafe { (asi.hook_status)(status) };
    if text.is_null() {
        return status.to_string();
    }
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_social_club_dispatch() {
    let (Some(asi), Some(options)) = (asi::platform(), OPTIONS.get()) else {
        return;
    };
    let mut attempted = lock(&ATTEMPTED);
    if *attempted {
        return;
    }
    let mut module = null_mut();
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_PIN,
            w!("SocialClubD3D12Renderer.dll"),
            &mut module,
        )
    } == 0
    {
        return; // The dispatcher can run before the renderer module is loaded.
    }
    *attempted = true;
    let base = module as usize;
    if !renderer_matches(asi, base) {
        log(
            "Social Club backbuffer fix refused: unsupported renderer image or modified instructions.",
        );
        return;
    }

    let code = relay(base);
    let page =
        unsafe { VirtualAlloc(null(), code.len(), MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
    if page.is_null() {
        log("Social Club backbuffer fix refused: gate allocation failed.");
        return;
    }
    let mut old = 0;
    let published = unsafe {
        ptr::copy_nonoverlapping(code.as_ptr(), page.cast(), code.len());
        VirtualProtect(page, code.len(), PAGE_EXECUTE_READ, &mut old) != 0
            && FlushInstructionCache(GetCurrentProcess(), page, code.len()) != 0
    };
    let target = base + profile::GATE;
    let mut status = MH_ERROR_MEMORY_PROTECT;
    let mut registered = false;
    if published {
        status = unsafe { (asi.hook_create)(target, page, null_mut()) };
        if status == MH_OK {
            registered = true;
            status = unsafe { (options.platform.enable)(target) };
        }
    }
    let repaired = registered && matches!(status, MH_OK | MH_ERROR_ENABLED);
    REPAIRED.store(repaired, Ordering::Release);
    log(format!(
        "Social Club backbuffer fix at renderer RVA 0x4b89: {}",
        status_text(asi, status)
    ));
    if !repaired {
        if registered {
            // Census can enable hooks concurrently. A registered gate remains process-lived
            // after a refused enable so an already entered callback cannot jump to freed code.
            unsafe { (asi.hook_disable)(target) };
            unsafe { (asi.hook_remove)(target) };
        } else {
            unsafe { VirtualFree(page, 0, MEM_RELEASE) };
        }
        return;
    }
    if options.trace {
        let target = base + profile::ENTRY;
        let mut status = unsafe { (options.platform.create_trace)(target) };
        if status == MH_OK {
            status = unsafe { (options.platform.enable)(target) };
        }
        log(format!(
            "Social Club frame trace: {}",
            status_text(asi, status)
        ));
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_social_club_trace(renderer: *const c_void, list: *const c_void, index: u32) {
    let (Some(asi), Some(options)) = (asi::platform(), OPTIONS.get()) else {
        return;
    };
    let renderer = renderer as usize;
    let swapchain = asi
        .read_value::<usize>(renderer.wrapping_add(profile::SWAPCHAIN))
        .unwrap_or(0);
    let mut current = index;
    let queries_current = swapchain != 0
        && asi
            .read_value::<u8>(renderer.wrapping_add(profile::QUERY_CURRENT_INDEX))
            .unwrap_or(0)
            != 0
        && unsafe { (options.platform.current_index)(swapchain as *mut c_void, &mut current) };
    let call = OBSERVED.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    let mismatch = !list.is_null() && queries_current && current != index;
    let differences = if mismatch {
        MISMATCHED.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    } else {
        MISMATCHED.load(Ordering::Relaxed)
    };
    if call <= 8 || (mismatch && differences.is_power_of_two()) {
        let list = list as usize;
        let queried = u32::from(queries_current);
        let repaired = u32::from(REPAIRED.load(Ordering::Acquire));
        log(format!(
            "Social Club frame: call={call} list={list:016X} supplied={index} current={current} queries_current={queried} repaired={repaired} mismatches={differences}"
        ));
    }
}
