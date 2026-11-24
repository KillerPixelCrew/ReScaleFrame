// SPDX-License-Identifier: MIT
//! Steam overlay D3D11 swapchain forwarding, shared by the Skyrim SE and Fallout 4 adapters.
//!
//! This crate owns the gameoverlayrenderer64.dll sites, found by signature in the loaded overlay
//! (discovery.rs; its file hash is logged only), the relay page, the forwarding handlers and the
//! one-shot installation. What a forwarded call means is the Runtime's decision through its
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

mod discovery;

use discovery::{LOAD_BYTES, LOAD_METHODS, LOADS, METHODS};

const HOOK_COUNT: usize = LOADS + 2;

// Relay page layout: one 32-byte stub per site, then the handler cells the stubs load.
const PAGE_BYTES: usize = 4096;
const STUB_BYTES: usize = 32;
const CELL_OFFSET: usize = 512;
const NEAR_LIMIT: usize = 0x7000_0000;
const _: () = assert!(LOADS * STUB_BYTES <= CELL_OFFSET);
const _: () = assert!(CELL_OFFSET + METHODS * size_of::<usize>() <= PAGE_BYTES);
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
    // Steam's saved original methods, indexed by dispatch method - 1.
    slots: [usize; METHODS],
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

fn handlers() -> [usize; METHODS] {
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
    rsf_d3d11_chain_dispatch {
        struct_size: size_of::<rsf_d3d11_chain_dispatch>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        method,
        receiver,
        ..Default::default()
    }
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
    let next = rsf_game_sys::win32::read::<usize>(relay().slots[index])?;
    (next != 0 && next != handlers()[index]).then_some(next)
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

fn fail(host: &rsf_game_host_services, message: &str) -> bool {
    host_log(host, message);
    false
}

/// The overlay file's SHA-256 in lowercase hex, for the log only.
fn digest(module: HMODULE) -> String {
    let path = unsafe { rsf_game_sys::win32::module_path(module) };
    let digest = path.and_then(|path| {
        rsf_windows::sha256_file(std::ffi::OsString::from_wide(path.units()).as_ref())
    });
    digest.map_or_else(
        || "unreadable".into(),
        |digest| digest.iter().map(|byte| format!("{byte:02x}")).collect(),
    )
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
    let base = module as usize;
    // Evidence only: the hash names the overlay build in the log, it admits nothing.
    let sites = match discovery::discover(base) {
        Ok(sites) => sites,
        Err(reason) => {
            return fail(
                host,
                &format!(
                    "Steam swapchain forwarding refused: {reason} in gameoverlayrenderer64.dll SHA-256 {}.",
                    digest(module)
                ),
            );
        }
    };
    host_log(
        host,
        &format!(
            "Steam overlay sites discovered in gameoverlayrenderer64.dll SHA-256 {}.",
            digest(module)
        ),
    );
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
    let page = near_page(base + sites.loads[0]);
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
    for (slot, rva) in targets[LOADS..].iter_mut().zip(sites.entries) {
        *slot = (base + rva) as *mut c_void;
    }
    for (index, (&load, &method)) in sites.loads.iter().zip(&LOAD_METHODS).enumerate() {
        let stub = page.wrapping_add(index * STUB_BYTES);
        let target = base + load;
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
        let cell = cells.wrapping_add((method as usize - 1) * size_of::<usize>());
        let displacement = (cell as isize - (stub as isize + LOAD_BYTES as isize)) as i32;
        // The load's own REX, opcode and ModRM: a discovered pattern fixes them.
        let Some(opcode) = read_bytes::<3>(target) else {
            release(page);
            return fail(
                host,
                "Steam swapchain forwarding refused: load site unreadable.",
            );
        };
        let mut code = [0u8; 21];
        code[..3].copy_from_slice(&opcode);
        code[3..7].copy_from_slice(&displacement.to_le_bytes());
        code[7..13].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
        code[13..].copy_from_slice(&(target + LOAD_BYTES).to_le_bytes());
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
    // The expected bytes are checked again at every found site just before the hooks.
    if let Some(site) = discovery::changed(base, &sites) {
        release(page);
        return fail(
            host,
            &format!("Steam swapchain forwarding refused: site '{site}' changed since discovery."),
        );
    }
    let record = Relay {
        service: unsafe { function::<Service>(service as usize) },
        user: host.user,
        slots: sites.slots.map(|slot| base + slot),
    };
    if RELAY.set(record).is_err() {
        release(page);
        return fail(
            host,
            "Steam swapchain forwarding refused: relay already published.",
        );
    }
    *attempted = true; // Relays/modules/trampolines remain process-lived after hook publication.
    // The Present entries keep their trampolines; the sites only run their relay stubs.
    let entries: [(*mut c_void, &AtomicPtr<c_void>); 2] = [
        (
            present_entry as PresentFn as *mut c_void,
            &ORIGINAL_PRESENT_ENTRY,
        ),
        (
            present1_entry as Present1Fn as *mut c_void,
            &ORIGINAL_PRESENT1_ENTRY,
        ),
    ];
    let mut created = 0;
    while created < HOOK_COUNT {
        let mut original = null_mut();
        let entry = created.checked_sub(LOADS).map(|index| entries[index]);
        let detour = match entry {
            Some((detour, _)) => detour,
            None => page.wrapping_add(created * STUB_BYTES).cast(),
        };
        let out = if entry.is_some() {
            &mut original
        } else {
            null_mut()
        };
        if !unsafe { minhook::create(targets[created], detour, out) } {
            break;
        }
        if let Some((_, trampoline)) = entry {
            trampoline.store(original, Ordering::Release);
        }
        created += 1;
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
        "Steam swapchain forwarding installed at seven methods/ten discovered loads and two pre-lock Present entries; native receivers retain Steam handlers; original installer/global untouched; process-lived relays.",
    );
    true
}

/// Prepares the Steam overlay's D3D11 chain hooks. Exported for games/fallout4/src/creation.rs,
/// which declares it over the C ABI with its own bindings; Skyrim calls it by path. Both embed
/// this crate in their archive.
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
