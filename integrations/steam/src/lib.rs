// SPDX-License-Identifier: MIT
//! Steam overlay D3D11 swapchain forwarding, shared by the Skyrim SE and Fallout 4 adapters.
//!
//! This crate owns the exact gameoverlayrenderer64.dll profile: its fingerprint, the guarded
//! load sites and Present entries, the relay page, the forwarding handlers and the one-shot
//! installation. What a forwarded call means is the Runtime's decision through its
//! rsf_d3d11_host_dispatch_chain export. Native exception guards protect the two foreign calls.
#![cfg(windows)]

use rsf_game_sys::call_header::validate;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{
    RSF_D3D11_DISPATCH_PRESENT, RSF_D3D11_DISPATCH_PRESENT1, RSF_D3D11_DISPATCH_RELEASE,
    RSF_D3D11_DISPATCH_RESIZE_BUFFERS, RSF_D3D11_DISPATCH_RESIZE_BUFFERS1,
    RSF_D3D11_DISPATCH_SET_COLOR_SPACE1, RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE,
    RSF_D3D11_HOST_ABI_VERSION, rsf_d3d11_chain_dispatch,
    rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_1 as PresentArgs,
    rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_2 as Present1Args,
    rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_3 as FullscreenArgs,
    rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_4 as ResizeArgs,
    rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_5 as ColorSpaceArgs,
    rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_6 as Resize1Args,
};
use rsf_game_sys::{RSF_OK, host_log, rsf_game_host_services};
use rsf_windows::minhook;
use std::ffi::c_void;
use std::os::windows::ffi::OsStringExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_FREE, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ,
    PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect, VirtualQuery,
};
use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::core::{s, w};

const E_FAIL: i32 = 0x8000_4005_u32 as i32;

// The only recognized overlay build. Every RVA and byte below belongs to it.
const STEAM_SHA256: [u8; 32] = [
    0x21, 0xc0, 0xc1, 0xdd, 0xb4, 0x25, 0x9c, 0x11, 0x56, 0x35, 0x6b, 0xdf, 0xe8, 0x43, 0x2d, 0xff,
    0x50, 0x4d, 0x52, 0xc7, 0xb9, 0x6d, 0x19, 0x71, 0x86, 0x88, 0xcf, 0xf2, 0xd5, 0xa6, 0xe5, 0x2f,
];

// A complete MOV RAX/R10,[RIP+disp32] that loads Steam's saved original method, followed by the
// bytes that call or tail-jump through it.
struct Site {
    rva: usize,
    method: u32,
    load: [u8; 7],
    following: [u8; 16],
}

#[rustfmt::skip]
const SITES: [Site; 10] = [
    Site { rva: 0x9c32f, method: RSF_D3D11_DISPATCH_RELEASE, load: [0x48,0x8b,0x05,0x7a,0x71,0x0e,0x00], following: [0x48,0x8b,0xf9,0xff,0xd0,0x8b,0xd8,0x85,0xc0,0x74,0x42,0x48,0x8b,0xd7,0x48,0x8d] },
    Site { rva: 0x9c375, method: RSF_D3D11_DISPATCH_RELEASE, load: [0x48,0x8b,0x05,0x34,0x71,0x0e,0x00], following: [0x48,0x8b,0xcf,0xff,0xd0,0x8b,0xd8,0x85,0xdb,0x75,0x2d,0x48,0x8b,0xd7,0x48,0x8d] },
    Site { rva: 0x9c154, method: RSF_D3D11_DISPATCH_PRESENT, load: [0x48,0x8b,0x05,0x5d,0x73,0x0e,0x00], following: [0xff,0xd0,0xe9,0x9b,0x01,0x00,0x00,0x4c,0x8d,0x25,0x7f,0x73,0x0e,0x00,0x4c,0x89] },
    Site { rva: 0x9c23e, method: RSF_D3D11_DISPATCH_PRESENT, load: [0x48,0x8b,0x05,0x73,0x72,0x0e,0x00], following: [0x44,0x8b,0xc6,0x8b,0xd5,0x49,0x8b,0xcf,0xff,0xd0,0x8b,0xf0,0x48,0x85,0xff,0x74] },
    Site { rva: 0x9c6cf, method: RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE, load: [0x48,0x8b,0x05,0xf2,0x6d,0x0e,0x00], following: [0x4d,0x8b,0xc6,0x41,0x8b,0xd7,0x48,0x8b,0xce,0x48,0x83,0xc4,0x28,0x41,0x5f,0x41] },
    Site { rva: 0x9c5bc, method: RSF_D3D11_DISPATCH_RESIZE_BUFFERS, load: [0x4c,0x8b,0x15,0xfd,0x6e,0x0e,0x00], following: [0x45,0x8b,0xc7,0x41,0x8b,0xd4,0x48,0x8b,0xcd,0x48,0x83,0xc4,0x30,0x41,0x5f,0x41] },
    Site { rva: 0x9bf72, method: RSF_D3D11_DISPATCH_PRESENT1, load: [0x48,0x8b,0x05,0x57,0x75,0x0e,0x00], following: [0xff,0xd0,0xe9,0x92,0x01,0x00,0x00,0x4c,0x8d,0x2d,0x61,0x75,0x0e,0x00,0x4c,0x89] },
    Site { rva: 0x9c05d, method: RSF_D3D11_DISPATCH_PRESENT1, load: [0x48,0x8b,0x05,0x6c,0x74,0x0e,0x00], following: [0x4d,0x8b,0xcc,0x45,0x8b,0xc6,0x8b,0xd5,0x49,0x8b,0xcf,0xff,0xd0,0x8b,0xe8,0x48] },
    Site { rva: 0x9c630, method: RSF_D3D11_DISPATCH_SET_COLOR_SPACE1, load: [0x48,0x8b,0x05,0xa1,0x6e,0x0e,0x00], following: [0x8b,0xd3,0x48,0x8b,0xce,0x48,0x83,0xc4,0x28,0x5f,0x5e,0x5d,0x5b,0x48,0xff,0xe0] },
    Site { rva: 0x9c4df, method: RSF_D3D11_DISPATCH_RESIZE_BUFFERS1, load: [0x4c,0x8b,0x15,0xfa,0x6f,0x0e,0x00], following: [0x45,0x8b,0xc4,0x48,0x89,0x9c,0x24,0xf8,0x00,0x00,0x00,0x8b,0xd5,0x48,0x89,0xbc] },
];

// Steam's saved original methods, indexed by dispatch method - 1.
const SLOTS: [usize; 7] = [
    0x1834b0, 0x1834b8, 0x1834c8, 0x1834c0, 0x1834d0, 0x1834d8, 0x1834e0,
];
// Steam's Present and Present1 handlers, entered before they take Steam's global Present lock.
const PRESENT_ENTRIES: [usize; 2] = [0x9c130, 0x9bf50];
#[rustfmt::skip]
const PRESENT_ENTRY_BYTES: [[u8; 16]; 2] = [
    [0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x6c,0x24,0x10,0x56,0x57,0x41,0x54,0x41,0x56],
    [0x40,0x53,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x83,0xec],
];
const HOOK_COUNT: usize = SITES.len() + PRESENT_ENTRIES.len();

// Relay page layout: one 32-byte stub per site, then the handler cells the stubs load.
const PAGE_BYTES: usize = 4096;
const STUB_BYTES: usize = 32;
const CELL_OFFSET: usize = 512;
const NEAR_LIMIT: usize = 0x7000_0000;
const _: () = assert!(SITES.len() * STUB_BYTES <= CELL_OFFSET);
const _: () = assert!(CELL_OFFSET + SLOTS.len() * size_of::<usize>() <= PAGE_BYTES);
const _: () = assert!(size_of::<rsf_d3d11_chain_dispatch>() == 72);

// IDXGISwapChain, IDXGISwapChain1 and IDXGISwapChain3 methods as Steam calls them. Enumerations
// and BOOL travel as their 32-bit values.
type Receiver = *mut c_void;
type ReleaseFn = unsafe extern "system" fn(Receiver) -> u32;
type PresentFn = unsafe extern "system" fn(Receiver, u32, u32) -> i32;
type FullscreenFn = unsafe extern "system" fn(Receiver, i32, *mut c_void) -> i32;
type ResizeFn = unsafe extern "system" fn(Receiver, u32, u32, u32, u32, u32) -> i32;
type Present1Fn = unsafe extern "system" fn(Receiver, u32, u32, *const c_void) -> i32;
type ColorSpaceFn = unsafe extern "system" fn(Receiver, u32) -> i32;
type Resize1Fn = unsafe extern "system" fn(
    Receiver,
    u32,
    u32,
    u32,
    u32,
    u32,
    *const u32,
    *const *mut c_void,
) -> i32;

type Service = unsafe extern "C" fn(*mut c_void, *mut rsf_d3d11_chain_dispatch) -> i32;
type ColdOwner = unsafe extern "C" fn() -> bool;

unsafe extern "C" {
    fn rsf_steam_dispatch_guard(
        service: Service,
        user: *mut c_void,
        request: *mut rsf_d3d11_chain_dispatch,
    ) -> i32;
}

struct Relay {
    service: Service,
    user: *mut c_void,
    base: usize,
}

// Runtime owns the opaque user, guarantees concurrent dispatch, and is pinned before publication.
unsafe impl Send for Relay {}
unsafe impl Sync for Relay {}

// Published before any hooks are created. All hooks and relays borrow this process-lived record.
static RELAY: OnceLock<Relay> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);
// Serializes installation and records that an attempt published state.
static ATTEMPTED: Mutex<bool> = Mutex::new(false);
// MinHook writes the trampolines here before the hooks are enabled.
static ORIGINAL_PRESENT_ENTRY: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static ORIGINAL_PRESENT1_ENTRY: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

/// # Safety
/// `address` must be the entry point of a function with signature F.
unsafe fn function<F: Copy>(address: usize) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    unsafe { std::mem::transmute_copy(&address) }
}

fn read_bytes<const N: usize>(address: usize) -> Option<[u8; N]> {
    rsf_game_sys::win32::read(address)
}

fn handlers() -> [usize; 7] {
    [
        forward_release as ReleaseFn as usize,
        forward_present as PresentFn as usize,
        forward_fullscreen as FullscreenFn as usize,
        forward_resize as ResizeFn as usize,
        forward_present1 as Present1Fn as usize,
        forward_color as ColorSpaceFn as usize,
        forward_resize1 as Resize1Fn as usize,
    ]
}

fn request_for(method: u32, receiver: Receiver) -> rsf_d3d11_chain_dispatch {
    // SAFETY: the C request is plain data; all-zero is its value-initialized state.
    let mut request: rsf_d3d11_chain_dispatch = unsafe { std::mem::zeroed() };
    request.struct_size = size_of::<rsf_d3d11_chain_dispatch>() as u32;
    request.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    request.method = method;
    request.receiver = receiver;
    request
}

fn relay() -> &'static Relay {
    RELAY.get().expect("Steam hooks require a published relay")
}

// True when the Runtime answered the call. A failed service fails closed without native forwarding.
fn handled(request: &mut rsf_d3d11_chain_dispatch) -> bool {
    let relay = relay();
    let status = unsafe { rsf_steam_dispatch_guard(relay.service, relay.user, request) };
    if status != RSF_OK {
        // Release has no HRESULT channel. Fail closed without native forwarding.
        if request.method == RSF_D3D11_DISPATCH_RELEASE {
            request.result.references = 0;
        } else {
            request.result.hresult = E_FAIL;
        }
        return true;
    }
    request.handled != 0
}

// Steam's saved original method, unless it is missing or would re-enter this handler.
fn continuation(method: u32) -> Option<usize> {
    let index = method as usize - 1;
    let slot = relay().base.wrapping_add(SLOTS[index]);
    let mut next = 0usize;
    let copied = unsafe {
        rsf_game_sys::win32::read_process(slot, (&raw mut next).cast(), size_of::<usize>())
    };
    (copied && next != 0 && next != handlers()[index]).then_some(next)
}

fn dispatch_present(receiver: Receiver, interval: u32, flags: u32) -> Option<i32> {
    let mut request = request_for(RSF_D3D11_DISPATCH_PRESENT, receiver);
    request.args.present = PresentArgs {
        sync_interval: interval,
        flags,
    };
    handled(&mut request).then_some(unsafe { request.result.hresult })
}

fn dispatch_present1(
    receiver: Receiver,
    interval: u32,
    flags: u32,
    parameters: *const c_void,
) -> Option<i32> {
    let mut request = request_for(RSF_D3D11_DISPATCH_PRESENT1, receiver);
    request.args.present1 = Present1Args {
        sync_interval: interval,
        flags,
        parameters,
    };
    handled(&mut request).then_some(unsafe { request.result.hresult })
}

// A facade must enter RSF before Steam acquires its global Present lock.
// XeSS's physical presenter runs on another thread and needs that same lock.
// Native receivers retain Steam's entire handler, including its overlay work.
unsafe extern "system" fn present_entry(receiver: Receiver, interval: u32, flags: u32) -> i32 {
    if let Some(result) = dispatch_present(receiver, interval, flags) {
        return result;
    }
    match ORIGINAL_PRESENT_ENTRY.load(Ordering::Acquire) as usize {
        0 => E_FAIL,
        next => unsafe { function::<PresentFn>(next)(receiver, interval, flags) },
    }
}

unsafe extern "system" fn present1_entry(
    receiver: Receiver,
    interval: u32,
    flags: u32,
    parameters: *const c_void,
) -> i32 {
    if let Some(result) = dispatch_present1(receiver, interval, flags, parameters) {
        return result;
    }
    match ORIGINAL_PRESENT1_ENTRY.load(Ordering::Acquire) as usize {
        0 => E_FAIL,
        next => unsafe { function::<Present1Fn>(next)(receiver, interval, flags, parameters) },
    }
}

unsafe extern "system" fn forward_release(receiver: Receiver) -> u32 {
    let mut request = request_for(RSF_D3D11_DISPATCH_RELEASE, receiver);
    if handled(&mut request) {
        return unsafe { request.result.references };
    }
    // Never AddRef or touch the object after Release.
    match continuation(request.method) {
        Some(next) => unsafe { function::<ReleaseFn>(next)(receiver) },
        None => 0,
    }
}

unsafe extern "system" fn forward_present(receiver: Receiver, interval: u32, flags: u32) -> i32 {
    if let Some(result) = dispatch_present(receiver, interval, flags) {
        return result;
    }
    match continuation(RSF_D3D11_DISPATCH_PRESENT) {
        Some(next) => unsafe { function::<PresentFn>(next)(receiver, interval, flags) },
        None => E_FAIL,
    }
}

unsafe extern "system" fn forward_fullscreen(
    receiver: Receiver,
    fullscreen: i32,
    target: *mut c_void,
) -> i32 {
    let mut request = request_for(RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE, receiver);
    request.args.fullscreen = FullscreenArgs {
        fullscreen: fullscreen as u32,
        target,
    };
    if handled(&mut request) {
        return unsafe { request.result.hresult };
    }
    match continuation(request.method) {
        Some(next) => unsafe { function::<FullscreenFn>(next)(receiver, fullscreen, target) },
        None => E_FAIL,
    }
}

unsafe extern "system" fn forward_resize(
    receiver: Receiver,
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
) -> i32 {
    let mut request = request_for(RSF_D3D11_DISPATCH_RESIZE_BUFFERS, receiver);
    request.args.resize_buffers = ResizeArgs {
        buffer_count: count,
        width,
        height,
        format,
        flags,
    };
    if handled(&mut request) {
        return unsafe { request.result.hresult };
    }
    match continuation(request.method) {
        Some(next) => unsafe {
            function::<ResizeFn>(next)(receiver, count, width, height, format, flags)
        },
        None => E_FAIL,
    }
}

unsafe extern "system" fn forward_present1(
    receiver: Receiver,
    interval: u32,
    flags: u32,
    parameters: *const c_void,
) -> i32 {
    if let Some(result) = dispatch_present1(receiver, interval, flags, parameters) {
        return result;
    }
    match continuation(RSF_D3D11_DISPATCH_PRESENT1) {
        Some(next) => unsafe {
            function::<Present1Fn>(next)(receiver, interval, flags, parameters)
        },
        None => E_FAIL,
    }
}

unsafe extern "system" fn forward_color(receiver: Receiver, color: u32) -> i32 {
    let mut request = request_for(RSF_D3D11_DISPATCH_SET_COLOR_SPACE1, receiver);
    request.args.color_space = ColorSpaceArgs { color_space: color };
    if handled(&mut request) {
        return unsafe { request.result.hresult };
    }
    match continuation(request.method) {
        Some(next) => unsafe { function::<ColorSpaceFn>(next)(receiver, color) },
        None => E_FAIL,
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn forward_resize1(
    receiver: Receiver,
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
    node_masks: *const u32,
    queues: *const *mut c_void,
) -> i32 {
    let mut request = request_for(RSF_D3D11_DISPATCH_RESIZE_BUFFERS1, receiver);
    request.args.resize_buffers1 = Resize1Args {
        buffer_count: count,
        width,
        height,
        format,
        flags,
        node_masks,
        present_queues: queues,
    };
    if handled(&mut request) {
        return unsafe { request.result.hresult };
    }
    match continuation(request.method) {
        Some(next) => unsafe {
            function::<Resize1Fn>(next)(
                receiver, count, width, height, format, flags, node_masks, queues,
            )
        },
        None => E_FAIL,
    }
}

fn pin(address: *const c_void) -> bool {
    let mut module = null_mut();
    let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN;
    unsafe { GetModuleHandleExW(flags, address.cast(), &mut module) != 0 }
}

// A free allocation granule within rel32 reach of the site, or null.
fn near_page(site: usize) -> *mut u8 {
    let mut info = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut info) };
    let step = info.dwAllocationGranularity as usize;
    if step == 0 {
        return null_mut();
    }
    let centre = site & !(step - 1);
    let mut delta = step;
    while delta < NEAR_LIMIT {
        let below = centre.saturating_sub(delta);
        for candidate in [centre.wrapping_add(delta), below] {
            if candidate == 0 {
                continue;
            }
            let mut region = MEMORY_BASIC_INFORMATION::default();
            let size = size_of::<MEMORY_BASIC_INFORMATION>();
            if unsafe { VirtualQuery(candidate as *const c_void, &mut region, size) } != 0
                && region.State == MEM_FREE
            {
                let page = unsafe {
                    VirtualAlloc(
                        candidate as *const c_void,
                        PAGE_BYTES,
                        MEM_RESERVE | MEM_COMMIT,
                        PAGE_READWRITE,
                    )
                };
                if !page.is_null() {
                    return page.cast();
                }
            }
        }
        delta += step;
    }
    null_mut()
}

// Every Present entry and load site still holds the recognized bytes.
fn guarded(base: usize) -> bool {
    PRESENT_ENTRIES
        .iter()
        .zip(&PRESENT_ENTRY_BYTES)
        .all(|(&rva, expected)| read_bytes::<16>(base + rva).as_ref() == Some(expected))
        && SITES.iter().all(|site| {
            read_bytes::<7>(base + site.rva) == Some(site.load)
                && read_bytes::<16>(base + site.rva + 7) == Some(site.following)
        })
}

fn fail(host: &rsf_game_host_services, message: &str) -> bool {
    host_log(host, message);
    false
}

fn recognized(module: HMODULE) -> bool {
    let Some(path) = (unsafe { rsf_game_sys::win32::module_path(module) }) else {
        return false;
    };
    let path = std::ffi::OsString::from_wide(path.units());
    rsf_windows::sha256_file(path.as_ref()) == Some(STEAM_SHA256)
}

fn prepare(host: &rsf_game_host_services, cold_owner: impl FnOnce() -> bool) -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let mut attempted = lock_or_recover(&ATTEMPTED);
    if INSTALLED.load(Ordering::Relaxed) {
        return true;
    }
    if *attempted {
        return fail(
            host,
            "Steam swapchain forwarding installation previously refused; process restart required.",
        );
    }
    let module = unsafe { GetModuleHandleW(w!("gameoverlayrenderer64.dll")) };
    if module.is_null() {
        return true; // Rechecked at the guarded cold creation call.
    }
    if !cold_owner() {
        return fail(
            host,
            "Steam swapchain forwarding refused late installation after native device creation.",
        );
    }
    if !recognized(module) {
        return fail(
            host,
            "Steam swapchain forwarding refused: unrecognized overlay fingerprint.",
        );
    }
    let base = module as usize;
    if !guarded(base) {
        return fail(
            host,
            "Steam swapchain forwarding refused: load/call continuation bytes differ.",
        );
    }
    let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
    let service = if runtime.is_null() {
        None
    } else {
        unsafe { GetProcAddress(runtime, s!("rsf_d3d11_host_dispatch_chain")) }
    };
    let Some(service) = service else {
        return fail(
            host,
            "Steam swapchain forwarding refused: typed chain service unavailable.",
        );
    };
    let service_address = service as *const c_void;
    let adapter = forward_present as PresentFn as *const c_void;
    if !pin(module) || !pin(adapter) || !pin(service_address) {
        return fail(
            host,
            "Steam swapchain forwarding refused: typed chain service or process-lifetime modules unavailable.",
        );
    }
    let page = near_page(base + SITES[0].rva);
    if page.is_null() {
        return fail(
            host,
            "Steam swapchain forwarding refused: no near relay page.",
        );
    }
    // Relays replace only their complete MOV RAX/R10,[RIP+disp32]. Steam retains
    // its original calls/tail jumps, stack arguments, register setup and flags.
    let release = |page: *mut u8| unsafe { VirtualFree(page.cast(), 0, MEM_RELEASE) };
    let cells = page.wrapping_add(CELL_OFFSET);
    let handlers = handlers();
    unsafe {
        ptr::copy_nonoverlapping(
            handlers.as_ptr().cast::<u8>(),
            cells,
            size_of_val(&handlers),
        )
    };
    let mut targets = [null_mut::<c_void>(); HOOK_COUNT];
    for (index, site) in SITES.iter().enumerate() {
        let stub = page.wrapping_add(index * STUB_BYTES);
        let target = base + site.rva;
        targets[index] = target as *mut c_void;
        let reach = (stub as isize)
            .wrapping_sub(target as isize)
            .wrapping_sub(5);
        if i32::try_from(reach).is_err() {
            release(page);
            return fail(
                host,
                "Steam swapchain forwarding refused: relay outside rel32 reach.",
            );
        }
        let cell = cells.wrapping_add((site.method as usize - 1) * size_of::<usize>());
        let displacement = (cell as isize - (stub as isize + 7)) as i32;
        let mut code = [0u8; 21];
        code[..3].copy_from_slice(&site.load[..3]);
        code[3..7].copy_from_slice(&displacement.to_le_bytes());
        code[7..13].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
        code[13..].copy_from_slice(&(target + 7).to_le_bytes());
        unsafe { ptr::copy_nonoverlapping(code.as_ptr(), stub, code.len()) };
    }
    let mut old = 0;
    let published = unsafe {
        VirtualProtect(page.cast(), PAGE_BYTES, PAGE_EXECUTE_READ, &mut old) != 0
            && FlushInstructionCache(GetCurrentProcess(), page.cast(), PAGE_BYTES) != 0
    };
    if !published {
        release(page);
        return fail(
            host,
            "Steam swapchain forwarding refused: relay publication failed.",
        );
    }
    let record = Relay {
        service: unsafe { function::<Service>(service as usize) },
        user: host.user,
        base,
    };
    if RELAY.set(record).is_err() {
        release(page);
        return fail(
            host,
            "Steam swapchain forwarding refused: relay already published.",
        );
    }
    *attempted = true; // Relays/modules/trampolines remain process-lived after hook publication.
    let mut created = 0;
    while created < SITES.len()
        && unsafe {
            minhook::create(
                targets[created],
                page.wrapping_add(created * STUB_BYTES).cast(),
                null_mut(),
            )
        }
    {
        created += 1;
    }
    if created == SITES.len() {
        targets[created] = (base + PRESENT_ENTRIES[0]) as *mut c_void;
        let detour = present_entry as PresentFn as *mut c_void;
        let mut original = null_mut();
        if unsafe { minhook::create(targets[created], detour, &mut original) } {
            ORIGINAL_PRESENT_ENTRY.store(original, Ordering::Release);
            created += 1;
        }
    }
    if created == SITES.len() + 1 {
        targets[created] = (base + PRESENT_ENTRIES[1]) as *mut c_void;
        let detour = present1_entry as Present1Fn as *mut c_void;
        let mut original = null_mut();
        if unsafe { minhook::create(targets[created], detour, &mut original) } {
            ORIGINAL_PRESENT1_ENTRY.store(original, Ordering::Release);
            created += 1;
        }
    }
    let mut queued = created == HOOK_COUNT;
    if queued {
        for &target in &targets {
            if !unsafe { minhook::queue(target, true) } {
                queued = false;
            }
        }
    }
    if !queued || !unsafe { minhook::apply() } {
        // Queue removal too: a failed Apply may have enabled only the first site.
        // Keep all backing memory valid even if rollback itself refuses.
        for &target in &targets[..created] {
            let _ = unsafe { minhook::queue(target, false) };
        }
        let _ = unsafe { minhook::apply() };
        return fail(
            host,
            "Steam swapchain forwarding refused: complete hook set enable failed; no facade admitted.",
        );
    }
    INSTALLED.store(true, Ordering::Release);
    host_log(
        host,
        "Steam swapchain forwarding installed at seven methods/ten guarded loads and two pre-lock Present entries; native receivers retain Steam handlers; original installer/global untouched; process-lived relays.",
    );
    true
}

/// Prepares the Steam overlay's D3D11 chain hooks. Exported for games/fallout4/src/creation.rs,
/// which links this crate as the CMake archive rsf_steam_d3d11_chain; Skyrim, which depends on the
/// crate, calls it by path.
///
/// # Safety
///
/// MinHook must already be initialized and `host` is null or a live host services record. The
/// callback is borrowed for the duration of the call and must return without unwinding. The
/// dispatch service's opaque user stays valid for hooks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_steam_d3d11_chain_prepare(
    host: *const rsf_game_host_services,
    cold_owner: Option<ColdOwner>,
) -> bool {
    // The ABI prefix must be readable; the full host is borrowed only after validation.
    if unsafe { validate(host) } != RSF_OK {
        return false;
    }
    let cold_owner = || cold_owner.is_some_and(|cold| unsafe { cold() });
    catch_unwind(AssertUnwindSafe(|| prepare(unsafe { &*host }, cold_owner))).unwrap_or(false)
}
