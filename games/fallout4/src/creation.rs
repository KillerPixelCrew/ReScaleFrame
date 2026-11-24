// SPDX-License-Identifier: MIT
//! Cold native D3D11 creation and presentation binding.
//!
//! The engine's own D3D11CreateDeviceAndSwapChain call runs unchanged with its own arguments
//! through the hooked import thunk. When the exact caller creates the renderer owner's outputs
//! for its window, the call is bracketed by the Runtime's creation scope, and the final outer
//! swapchain is bound for presentation. Scene and input producers are admitted separately.
//! The original runs through the C creation call in host_boundary.c, which leaves a native throw
//! uncaught, so an engine throw ends the process.
use crate::build_layout::{
    RENDERER_CHAIN as CHAIN, RENDERER_CONTEXT as CONTEXT, RENDERER_DEVICE as DEVICE,
    RENDERER_POINTER as OWNER_RVA, RENDERER_WINDOW as WINDOW,
};
use crate::sys::{
    RSF_D3D11_HOST_ABI_VERSION, rsf_d3d11_creation_scope, rsf_d3d11_native_chain,
    rsf_d3d11_presentation_binding, rsf_fallout4_create_call,
};
use crate::{EXECUTABLE, EXECUTABLE_SHA256, Plain, RSF_ERROR_NOT_READY, RSF_OK};
use crate::{image, read};
use rsf_game_sys::win32::{module_path, typed_export};
use rsf_game_sys::windows_path::leaf_is;
use rsf_game_sys::{host_log, rsf_game_host_services};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows_sys::core::w;

pub(crate) const E_FAIL: i32 = 0x8000_4005_u32 as i32;

// AE1.11.240 disassembly: creation1824930+410 -> thunk22C4CD9;
// renderer owner38E13A0, native device48/context50/window58/chain70.
const CALL_RVA: usize = 0x1824d40;
const THUNK_RVA: usize = 0x22c4cd9;
// CALL rel32 to the thunk, and the thunk's JMP [RIP+disp32] to the imported creation.
const CALL_BYTES: [u8; 5] = [0xe8, 0x94, 0xff, 0xa9, 0x00];
const THUNK_BYTES: [u8; 6] = [0xff, 0x25, 0x79, 0xca, 0x17, 0x00];

// DXGI_SWAP_CHAIN_DESC as the engine passes it. Only OutputWindow is compared, but the whole
// description must be readable.
#[repr(C)]
#[derive(Clone, Copy)]
struct SwapChainDesc {
    buffer_desc: [u32; 7],
    sample_desc: [u32; 2],
    buffer_usage: u32,
    buffer_count: u32,
    output_window: usize,
    windowed: i32,
    swap_effect: u32,
    flags: u32,
}
const _: () = assert!(size_of::<SwapChainDesc>() == 72);
const _: () = assert!(std::mem::offset_of!(SwapChainDesc, output_window) == 48);
unsafe impl Plain for SwapChainDesc {}

pub(crate) type Begin = unsafe extern "C" fn(*mut c_void, *mut rsf_d3d11_creation_scope) -> i32;
pub(crate) type End = unsafe extern "C" fn(
    *mut c_void,
    *const rsf_d3d11_creation_scope,
    *const rsf_d3d11_native_chain,
) -> i32;
pub(crate) type Bind =
    unsafe extern "C" fn(*mut c_void, *const rsf_d3d11_presentation_binding) -> i32;

// The scoped services and Runtime creation exports, published by prepare before the hook is
// enabled. The detour copies them.
#[derive(Clone, Copy)]
struct Scoped {
    host: rsf_game_host_services,
    begin: Begin,
    end: End,
    bind: Bind,
}
// SAFETY: the services and exports are process-lived Runtime state.
unsafe impl Send for Scoped {}

static SCOPED: Mutex<Option<Scoped>> = Mutex::new(None);
static ENABLED: AtomicBool = AtomicBool::new(false);
static ENTERED: AtomicU32 = AtomicU32::new(0);
// The hook is created once and its trampoline stays process-lived.
static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe extern "C" {
    fn rsf_steam_d3d11_chain_prepare(
        host: *const rsf_game_host_services,
        cold_owner: Option<unsafe extern "C" fn() -> bool>,
    ) -> bool;
    fn rsf_fallout4_call_creation(
        original: *mut c_void,
        call: *const rsf_fallout4_create_call,
    ) -> i32;
}

fn hook_created() -> bool {
    !ORIGINAL.load(Ordering::Acquire).is_null()
}

fn create_hook() -> bool {
    let target = hook_address() as *mut c_void;
    let detour = creation_detour as *const () as *mut c_void;
    crate::create_hook(target, detour, &ORIGINAL)
}

fn hook_address() -> usize {
    image() + THUNK_RVA
}

fn renderer() -> Option<usize> {
    read::<usize>(image() + OWNER_RVA)
}

/// The renderer owner is absent or has not created its device.
extern "C" fn cold_owner() -> bool {
    match renderer() {
        Some(0) => true,
        Some(renderer) => read::<usize>(renderer.wrapping_add(DEVICE)) == Some(0),
        None => false,
    }
}

fn expected_creation() -> bool {
    read::<[u8; 5]>(image() + CALL_RVA) == Some(CALL_BYTES)
        && read::<[u8; 6]>(image() + THUNK_RVA) == Some(THUNK_BYTES)
}

/// The renderer owner's window, owned by this process.
pub(crate) fn accept_window(candidate: *mut c_void) -> bool {
    if candidate.is_null() {
        return false;
    }
    let window = match renderer() {
        Some(renderer) if renderer != 0 => read::<usize>(renderer.wrapping_add(WINDOW)),
        _ => None,
    };
    if window != Some(candidate as usize) {
        return false;
    }
    let mut process = 0;
    let owned = unsafe { GetWindowThreadProcessId(candidate, &mut process) } != 0;
    owned && process == unsafe { GetCurrentProcessId() }
}

/// Checks the executable name and recognized SHA-256 once for the preparing transaction.
pub(crate) fn admit_build() -> bool {
    unsafe extern "C" {
        fn rsf_common_sha256_file_hex(path: *const u16, output: *mut [u8; 64]) -> u32;
    }
    let Some(path) = (unsafe { module_path(std::ptr::null_mut()) }) else {
        return false;
    };
    if !leaf_is(path.units(), EXECUTABLE) {
        return false;
    }
    let mut digest = [0; 64];
    let hashed =
        unsafe { rsf_common_sha256_file_hex(path.terminated().as_ptr(), &raw mut digest) } != 0;
    hashed && digest == *EXECUTABLE_SHA256.to_bytes()
}

pub(crate) fn preflight() -> bool {
    !ENABLED.load(Ordering::Acquire)
        && ENTERED.load(Ordering::Acquire) == 0
        && expected_creation()
        && cold_owner()
}

pub(crate) fn busy() -> bool {
    ENTERED.load(Ordering::Acquire) != 0
}

/// Resolves the Runtime creation exports, prepares Steam's chain forwarding and creates the
/// disabled creation hook.
pub(crate) fn prepare(services: &rsf_game_host_services) -> bool {
    if !preflight() {
        return false;
    }
    let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
    if runtime.is_null() {
        return false;
    }
    // SAFETY: the process-lived Runtime exports these signatures in d3d11_host.h.
    let (begin, end, bind) = unsafe {
        (
            typed_export::<Begin>(runtime, c"rsf_d3d11_host_creation_begin"),
            typed_export::<End>(runtime, c"rsf_d3d11_host_creation_end"),
            typed_export::<Bind>(runtime, c"rsf_d3d11_host_bind_presentation"),
        )
    };
    let (Some(begin), Some(end), Some(bind)) = (begin, end, bind) else {
        return false;
    };
    if !prepare_steam_chain(services) {
        return false;
    }
    if !hook_created() && !create_hook() {
        return false;
    }
    let scoped = Scoped {
        host: *services,
        begin,
        end,
        bind,
    };
    *SCOPED.lock().unwrap_or_else(PoisonError::into_inner) = Some(scoped);
    let _ = crate::log_host(
        services,
        c"Fallout4 AE cold creation prepared at thunk22C4CD9, caller1824D40; native arguments and outer wrapper retained; scene admission remains separate.",
    );
    true
}

pub(crate) fn start() -> bool {
    if !hook_created() || !expected_creation() || !cold_owner() {
        return false;
    }
    if !crate::enable_hook(hook_address()) {
        return false;
    }
    ENABLED.store(true, Ordering::Release);
    true
}

pub(crate) fn quiesce() {
    ENABLED.store(false, Ordering::Release);
}

pub(crate) fn stop() -> bool {
    quiesce();
    let disabled = !hook_created() || crate::disable_hook(hook_address());
    disabled && !busy()
}

struct Entry;

impl Entry {
    fn new() -> Self {
        ENTERED.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        ENTERED.fetch_sub(1, Ordering::AcqRel);
    }
}

// The exact engine call for the renderer owner's outputs and window, before its device exists.
fn owner_call(
    scoped: Option<&Scoped>,
    caller: usize,
    call: &rsf_fallout4_create_call,
) -> Option<usize> {
    if !ENABLED.load(Ordering::Acquire) || caller != image() + CALL_RVA + 5 {
        return None;
    }
    let renderer = renderer().filter(|&renderer| renderer != 0)?;
    if call.swapchain as usize != renderer.wrapping_add(CHAIN)
        || call.device as usize != renderer.wrapping_add(DEVICE)
        || call.immediate_context as usize != renderer.wrapping_add(CONTEXT)
    {
        return None;
    }
    let window = read::<usize>(renderer.wrapping_add(WINDOW)).filter(|&window| window != 0)?;
    if call.swapchain_desc.is_null() {
        return None;
    }
    let description = read::<SwapChainDesc>(call.swapchain_desc as usize)?;
    let owned = description.output_window == window
        && cold_owner()
        && scoped.is_some_and(|scoped| prepare_steam_chain(&scoped.host));
    owned.then_some(window)
}

// Closes an open scope after the complete native call. A successful matched close binds the
// final outer swapchain for presentation. The thread's last error is preserved.
fn close(
    scoped: &Scoped,
    scope: &rsf_d3d11_creation_scope,
    outputs: Option<&rsf_d3d11_native_chain>,
    native_result: i32,
) {
    let last_error = unsafe { GetLastError() };
    let user = scoped.host.user;
    let mut admitted = RSF_ERROR_NOT_READY;
    let mut bound = RSF_ERROR_NOT_READY;
    if let Ok(result) = crate::creation_end(scoped.end, user, scope, outputs) {
        admitted = result;
        if admitted == RSF_OK
            && let Some(swapchain) = outputs
                .and_then(|outputs| read::<usize>(outputs.swapchain as usize))
                .filter(|&swapchain| swapchain != 0)
        {
            let binding = rsf_d3d11_presentation_binding {
                struct_size: size_of::<rsf_d3d11_presentation_binding>() as u32,
                abi_version: RSF_D3D11_HOST_ABI_VERSION,
                // The caller-owned chain slot after the Runtime's scope end.
                swapchain: swapchain as *mut c_void,
            };
            if let Ok(result) = crate::bind_presentation(scoped.bind, user, &binding) {
                bound = result;
            }
        }
    }
    host_log(
        &scoped.host,
        &format!(
            "Fallout4 cold creation native_HRESULT=0x{:08x} scope_end={admitted} binding={bound}; outer outputs retained, no scene/input producers",
            native_result as u32
        ),
    );
    unsafe { SetLastError(last_error) };
}

fn observe(caller: usize, call: &rsf_fallout4_create_call) -> i32 {
    let _entry = Entry::new();
    let entry_error = unsafe { GetLastError() };
    let scoped = *SCOPED.lock().unwrap_or_else(PoisonError::into_inner);
    let mut scope = rsf_d3d11_creation_scope {
        struct_size: size_of::<rsf_d3d11_creation_scope>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        window: std::ptr::null_mut(),
        token: 0,
    };
    let mut open = None;
    if let Some(window) = owner_call(scoped.as_ref(), caller, call)
        && let Some(scoped) = scoped.as_ref()
    {
        scope.window = window as *mut c_void;
        let begun = crate::creation_begin(scoped.begin, scoped.host.user, &mut scope);
        if begun.is_ok_and(|result| result == RSF_OK) {
            open = Some(scoped);
        }
    }
    unsafe { SetLastError(entry_error) };
    let original = ORIGINAL.load(Ordering::Acquire);
    let result = if original.is_null() {
        E_FAIL
    } else {
        // The native function runs without a handler: an engine throw ends the process.
        unsafe { rsf_fallout4_call_creation(original, call) }
    };
    let native_error = unsafe { GetLastError() };
    if let Some(scoped) = open {
        let mut outputs = None;
        if result >= 0 && ENABLED.load(Ordering::Acquire) {
            let device = read::<usize>(call.device as usize).filter(|&value| value != 0);
            let context = device
                .and_then(|_| read::<usize>(call.immediate_context as usize))
                .filter(|&value| value != 0);
            let chain = context
                .and_then(|_| read::<usize>(call.swapchain as usize))
                .filter(|&value| value != 0);
            if let (Some(device), Some(context), Some(_)) = (device, context, chain) {
                outputs = Some(rsf_d3d11_native_chain {
                    struct_size: size_of::<rsf_d3d11_native_chain>() as u32,
                    abi_version: RSF_D3D11_HOST_ABI_VERSION,
                    device: device as *mut c_void,
                    immediate_context: context as *mut c_void,
                    swapchain: call.swapchain,
                });
            }
        }
        unsafe { SetLastError(native_error) };
        close(scoped, &scope, outputs.as_ref(), result);
    }
    unsafe { SetLastError(native_error) };
    result
}

// Win64 passes driver_type and flags in the low halves of RDX and R9. Carry the
// actual return address in their unused high halves to the private Rust entry.
// This leaf touches no stack slot or nonvolatile register. The eight stack
// arguments, caller's return address and home space reach the Rust entry unchanged.
#[unsafe(naked)]
unsafe extern "system" fn creation_detour(
    _adapter: *mut c_void,
    _driver_type: i32,
    _software: *mut c_void,
    _flags: u32,
    _feature_levels: *const i32,
    _feature_level_count: u32,
    _sdk_version: u32,
    _swapchain_desc: *const c_void,
    _swapchain: *mut *mut c_void,
    _device: *mut *mut c_void,
    _feature_level: *mut i32,
    _immediate_context: *mut *mut c_void,
) -> i32 {
    std::arch::naked_asm!(
        "mov r10, [rsp]",
        "mov r11, r10",
        "shl r10, 32",
        "shr r11, 32",
        "shl r11, 32",
        "mov edx, edx",
        "mov r9d, r9d",
        "or rdx, r10",
        "or r9, r11",
        "jmp {observe}",
        observe = sym observe_create,
    );
}

/// Called only by the Win64 entry relay with the captured return address.
unsafe extern "system" fn observe_create(
    adapter: *mut c_void,
    driver_type_and_caller_low: u64,
    software: *mut c_void,
    flags_and_caller_high: u64,
    feature_levels: *const i32,
    feature_level_count: u32,
    sdk_version: u32,
    swapchain_desc: *const c_void,
    swapchain: *mut *mut c_void,
    device: *mut *mut c_void,
    feature_level: *mut i32,
    immediate_context: *mut *mut c_void,
) -> i32 {
    let caller =
        (flags_and_caller_high & 0xffff_ffff_0000_0000) | (driver_type_and_caller_low >> 32);
    let call = rsf_fallout4_create_call {
        adapter,
        driver_type: driver_type_and_caller_low as i32,
        software,
        flags: flags_and_caller_high as u32,
        feature_levels,
        feature_level_count,
        sdk_version,
        swapchain_desc,
        swapchain,
        device,
        feature_level,
        immediate_context,
    };
    observe(caller as usize, &call)
}

pub(crate) unsafe extern "C" fn rsf_fallout4_accept_window(
    _user: *mut c_void,
    window: *mut c_void,
) -> i32 {
    i32::from(accept_window(window))
}

fn prepare_steam_chain(host: &rsf_game_host_services) -> bool {
    // The linked Steam owner borrows this host and the callback synchronously.
    unsafe { rsf_steam_d3d11_chain_prepare(host, Some(cold_owner)) }
}
