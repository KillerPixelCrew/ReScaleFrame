// SPDX-License-Identifier: MIT
//! Keep RAGE's supplied backbuffer index when Social Club records into RAGE's command list.
//! The guarded renderer bytes and continuation addresses are from rdr2-renderer.md.
use crate::asi::{self, AsiPlatform, MH_ERROR_ALREADY_INITIALIZED, MH_OK};
use crate::layout::{asi::SOCIAL_CLUB_DISPATCHER, social_club as profile};
use crate::platform::{lock, minhook::MH_EnableHook, trampoline};
use crate::runtime_worker::log;
use std::ffi::c_void;
use std::ptr::{self, null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
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

type CurrentIndex = unsafe extern "C" fn(*mut c_void, *mut u32) -> bool;
type Dispatch = unsafe extern "C" fn(*mut c_void) -> usize;
type Render =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, u32) -> usize;

struct Options {
    current_index: CurrentIndex,
    trace: bool,
}

static ORIGINAL_DISPATCH: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static ORIGINAL_RENDER: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

unsafe extern "C" fn dispatch(context: *mut c_void) -> usize {
    observe_dispatch();
    unsafe { trampoline::<Dispatch>(&ORIGINAL_DISPATCH).map_or(0, |native| native(context)) }
}

unsafe extern "C" fn trace_render(
    renderer: *mut c_void,
    draws: *mut c_void,
    unused: *mut c_void,
    list: *mut c_void,
    index: u32,
) -> usize {
    observe_trace(renderer, list, index);
    unsafe {
        trampoline::<Render>(&ORIGINAL_RENDER)
            .map_or(0, |native| native(renderer, draws, unused, list, index))
    }
}

fn create(
    asi: &AsiPlatform,
    target: usize,
    detour: *const c_void,
    original: &AtomicPtr<c_void>,
) -> i32 {
    // Publish the trampoline inside MH_CreateHook before a separate census caller can enable it.
    unsafe { (asi.hook_create)(target, detour, original.as_ptr()) }
}

fn enable(target: usize) -> i32 {
    unsafe { MH_EnableHook(target as *mut c_void) }
}

static OPTIONS: OnceLock<Options> = OnceLock::new();
static ATTEMPTED: Mutex<bool> = Mutex::new(false);
static REPAIRED: AtomicBool = AtomicBool::new(false);
static OBSERVED: AtomicU32 = AtomicU32::new(0);
static MISMATCHED: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_rdr2_social_club_install(
    current_index: CurrentIndex,
    trace: bool,
) -> bool {
    let Some(asi) = asi::platform() else {
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
    if OPTIONS
        .set(Options {
            current_index,
            trace,
        })
        .is_err()
    {
        return false;
    }
    create(asi, site, dispatch as *const c_void, &ORIGINAL_DISPATCH) == MH_OK
        && enable(site) == MH_OK
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

fn observe_dispatch() {
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
            status = enable(target);
        }
    }
    let repaired = registered && matches!(status, MH_OK | MH_ERROR_ENABLED);
    REPAIRED.store(repaired, Ordering::Release);
    log(format!(
        "Social Club backbuffer fix at renderer RVA 0x4b89: {}",
        asi.status_text(status)
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
        let mut status = create(asi, target, trace_render as *const c_void, &ORIGINAL_RENDER);
        if status == MH_OK {
            status = enable(target);
        }
        log(format!(
            "Social Club frame trace: {}",
            asi.status_text(status)
        ));
    }
}

fn observe_trace(renderer: *const c_void, list: *const c_void, index: u32) {
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
        && unsafe { (options.current_index)(swapchain as *mut c_void, &mut current) };
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
