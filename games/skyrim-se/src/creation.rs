// SPDX-License-Identifier: MIT
//! Cold native creation and Steam Present cooperation for Skyrim SE.
//!
//! The game creates its device and swapchain once through its discovered import thunk. Only
//! the game's own call, or the Display Tweaks relay of it to its discovered wrapper, may open the Runtime's
//! mode-3 creation scope around the complete original call; any outer wrapper keeps its arguments,
//! outputs and HRESULT. The Steam overlay forwarding is installed through the shared Steam facade
//! only while the game's renderer owner has no device (the cold owner). Anything else refuses
//! admission and the game's native creation and Present run unchanged.
use crate::discovery::{self, Site, rva};
use crate::layout::*;
use crate::platform::{CreationCall, CreationScope, NativeChain, Platform};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::rsf_game_host_services;
use std::ffi::{CStr, CString, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use rsf_game_sys::RSF_OK as OK;

/// `DXGI_SWAP_CHAIN_DESC` size and `OutputWindow` offset; `native_creation.c` asserts both.
const DESCRIPTION_SIZE: usize = 72;
const DESCRIPTION_WINDOW: usize = 48;

/// Win32 `VirtualQuery` state and protection bits.
const MEM_COMMIT: u32 = 0x1000;
const PAGE_NOACCESS: u32 = 0x01;
const PAGE_GUARD: u32 = 0x100;
const PAGE_READABLE: u32 = 0x02 | 0x04 | 0x08 | 0x20 | 0x40 | 0x80;
const PAGE_EXECUTABLE_READABLE: u32 = 0x20 | 0x40 | 0x80;

const RUNTIME_MODULE: &str = "ReScaleFrame.Runtime.dll";
const CREATION_BEGIN: &CStr = c"rsf_d3d11_host_creation_begin";
const CREATION_END: &CStr = c"rsf_d3d11_host_creation_end";

const CALLER_UNREADABLE: &CStr = c"Native creation caller unreadable.";
const CALLER_DIFFERS: &CStr =
    c"Native creation caller differs from native or verified Display Tweaks route.";
const DISPLAY_PROFILE: &CStr =
    c"Display Tweaks creation route refused: module pin failed or module changed.";
const DISPLAY_RELAY: &CStr =
    c"Display Tweaks creation relay, original thunk or call-return bytes refused.";
const THUNK_REFUSED: &CStr = c"Native creation thunk bytes refused.";
const UNEXPECTED_CALLER: &CStr = c"Native creation refused: unexpected guarded-route caller.";
const PREPARED: &CStr = c"Skyrim native creation root prepared: discovered game import thunk -> current D3D11 IAT slot; guarded native or discovered Display Tweaks call route; scoped inner creation, outer chain retained; no graphics created yet.";

/// Process-lived trampoline of the game thunk. Published before the hook can be enabled.
static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static LIVE: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU32 = AtomicU32::new(0);
static REPORTS: AtomicU32 = AtomicU32::new(0);
static SHARED: Mutex<Shared> = Mutex::new(Shared {
    services: None,
    begin: 0,
    end: 0,
});

/// Host services and Runtime creation entries, written by prepare while no call is in flight.
#[derive(Clone, Copy)]
struct Shared {
    services: Option<rsf_game_host_services>,
    begin: usize,
    end: usize,
}

// The services are the Runtime's process-lived D3D11 host session; their pointers are not
// thread-affine.
unsafe impl Send for Shared {}

fn lock() -> MutexGuard<'static, Shared> {
    // Hook callbacks must not unwind; a poisoned lock keeps its last consistent state.
    lock_or_recover(&SHARED)
}

fn shared() -> Shared {
    *lock()
}

fn thunk(platform: &Platform) -> usize {
    platform.main_module().wrapping_add(rva(Site::CreateThunk))
}

/// The return address of the game's creation call.
fn create_return(base: usize) -> usize {
    base.wrapping_add(rva(Site::CreateCall) + CREATE_CALL_LENGTH)
}

/// Readable committed memory for the whole range, optionally executable and inside `allocation`.
fn mapped(platform: &Platform, address: usize, size: usize, allocation: usize, code: bool) -> bool {
    let Some(region) = platform.query(address) else {
        return false;
    };
    let (Some(end), Some(region_end)) = (
        address.checked_add(size),
        region.base.checked_add(region.size),
    ) else {
        return false;
    };
    region.state == MEM_COMMIT
        && region.protect & (PAGE_NOACCESS | PAGE_GUARD) == 0
        && region.protect & PAGE_READABLE != 0
        && (!code || region.protect & PAGE_EXECUTABLE_READABLE != 0)
        && (allocation == 0 || region.allocation_base == allocation)
        && end <= region_end
}

/// The verified return address of the guarded creation call: the game's own call, or the
/// Display Tweaks relay to its wrapper's call of the unchanged thunk.
fn creation_caller(platform: &Platform) -> Result<usize, &'static CStr> {
    let base = platform.main_module();
    let mut call = [0; CREATE_CALL_LENGTH];
    if !platform.read(base.wrapping_add(rva(Site::CreateCall)), &mut call) {
        return Err(CALLER_UNREADABLE);
    }
    // The discovered call site, its displacement either the game's own or a relay's.
    if !discovery::verify(platform, base, Site::CreateCall) {
        return Err(CALLER_DIFFERS);
    }
    let target = rel32_target(create_return(base), [call[1], call[2], call[3], call[4]]);
    if target == thunk(platform) {
        return Ok(create_return(base));
    }
    let module = platform.module_handle(DISPLAY_TWEAKS_MODULE);
    if module == 0 || call[0] != 0xe8 {
        return Err(CALLER_DIFFERS);
    }
    let route = match DISPLAY.get_or_init(|| display_route(platform, module)) {
        Ok(route) if route.module == module => route,
        Ok(_) => return Err(DISPLAY_PROFILE),
        Err(refusal) => return Err(refusal.as_c_str()),
    };
    let sites = route.sites;
    // Display Tweaks patches only the game's E8 displacement. Its SKSE relay jumps to Display
    // pre/post work, which calls the unchanged native thunk.
    let relay = target;

    let mut relay_bytes = [0; RELAY_LENGTH];
    if !mapped(platform, relay, RELAY_LENGTH, 0, true)
        || !platform.read(relay, &mut relay_bytes)
        || relay_bytes[..RELAY_JUMP.len()] != RELAY_JUMP
    {
        return Err(CALLER_DIFFERS);
    }
    let mut destination = [0; size_of::<usize>()];
    destination.copy_from_slice(&relay_bytes[RELAY_JUMP.len()..]);
    let destination = usize::from_le_bytes(destination);
    let invoke = module.wrapping_add(sites.call);
    let retained = module.wrapping_add(sites.retained);
    let [wrapper_bytes, call_bytes] = discovery::DISPLAY_SPANS;
    if destination != module.wrapping_add(sites.wrapper)
        || !mapped(platform, destination, wrapper_bytes, module, true)
        || !mapped(platform, invoke, call_bytes, module, true)
        || !mapped(platform, retained, size_of::<usize>(), module, false)
        || !discovery::display_tweaks_current(platform, module, &sites)
        || platform.read_usize(retained) != Some(thunk(platform))
    {
        return Err(DISPLAY_RELAY);
    }
    Ok(module.wrapping_add(sites.call_return))
}

/// The Display Tweaks route discovered in its loaded module, pinned for the process, with the
/// messages that name it; the module's file hash is logged only.
struct DisplayRoute {
    module: usize,
    sites: discovery::DisplayTweaks,
    matched: CString,
}

static DISPLAY: OnceLock<Result<DisplayRoute, CString>> = OnceLock::new();

fn display_route(platform: &Platform, module: usize) -> Result<DisplayRoute, CString> {
    let message = |text: String| CString::new(text).unwrap_or_default();
    let digest = platform.module_sha256(module).map_or_else(
        || "unreadable".into(),
        |digest| String::from_utf8_lossy(&digest).into_owned(),
    );
    let sites = discovery::display_tweaks(platform, module).map_err(|reason| {
        message(format!(
            "Display Tweaks creation route refused: {reason} in SSEDisplayTweaks.dll SHA-256 {digest}."
        ))
    })?;
    if platform.pin_module(module) != module {
        return Err(DISPLAY_PROFILE.into());
    }
    Ok(DisplayRoute {
        module,
        sites,
        matched: message(format!(
            "Skyrim native creation matched the Display Tweaks pre/native/post route discovered in SSEDisplayTweaks.dll SHA-256 {digest}; native thunk and game outputs retained."
        )),
    })
}

fn guard(platform: &Platform) -> Result<usize, &'static CStr> {
    if !discovery::verify(platform, platform.main_module(), Site::CreateThunk) {
        return Err(THUNK_REFUSED);
    }
    creation_caller(platform)
}

#[cfg(not(test))]
use rsf_steam::rsf_steam_d3d11_chain_prepare;

fn prepare_steam(host: &rsf_game_host_services) -> bool {
    // Observer preparation initialized MinHook. Steam borrows the callback and retains the
    // process-lived Runtime dispatch used by this session.
    #[cfg(not(test))]
    return unsafe { rsf_steam_d3d11_chain_prepare(host, Some(cold_owner)) };
    #[cfg(test)]
    {
        let _ = host;
        false
    }
}

/// The game's renderer owner is absent or has no device yet.
fn cold(platform: &Platform) -> bool {
    before_device(platform.main_module(), |address| {
        platform.read_usize(address)
    }) == Some(true)
}

/// Cold-owner callback for the Steam facade, which may call it again on a later prepare.
#[cfg(not(test))]
unsafe extern "C" fn cold_owner() -> bool {
    cold(&Platform)
}

pub(crate) fn prepare(platform: &Platform, host: &rsf_game_host_services) -> bool {
    if CALLS.load(Ordering::Acquire) != 0 || LIVE.load(Ordering::Acquire) {
        return false;
    }
    if let Err(refusal) = guard(platform) {
        platform.log(host, refusal);
        return false;
    }
    if !cold(platform) || !prepare_steam(host) {
        return false;
    }
    let runtime = platform.module_handle(RUNTIME_MODULE);
    let (begin, end) = if runtime == 0 {
        (0, 0)
    } else {
        (
            platform.export_address(runtime, CREATION_BEGIN),
            platform.export_address(runtime, CREATION_END),
        )
    };
    if begin == 0 || end == 0 {
        return false;
    }
    if ORIGINAL.load(Ordering::Acquire).is_null() {
        let Some(original) = platform.hook_create(thunk(platform), platform.create_detour()) else {
            return false;
        };
        ORIGINAL.store(original, Ordering::Release);
    }
    *lock() = Shared {
        services: Some(*host),
        begin,
        end,
    };
    platform.log(host, PREPARED);
    true
}

pub(crate) fn start() -> bool {
    let platform = &Platform;
    let shared = shared();
    if ORIGINAL.load(Ordering::Acquire).is_null() || shared.begin == 0 || shared.end == 0 {
        return false;
    }
    if let Err(refusal) = guard(platform) {
        if let Some(services) = &shared.services {
            platform.log(services, refusal);
        }
        return false;
    }
    if !platform.hook_enable(thunk(platform)) {
        return false;
    }
    LIVE.store(true, Ordering::Release);
    true
}

pub(crate) fn quiesce() {
    LIVE.store(false, Ordering::Release);
}

pub(crate) fn busy() -> bool {
    CALLS.load(Ordering::Acquire) != 0
}

pub(crate) fn stop() -> bool {
    quiesce();
    if ORIGINAL.load(Ordering::Acquire).is_null() {
        return true;
    }
    let platform = &Platform;
    platform.hook_disable(thunk(platform)) && !busy()
}

/// Opens the call's lease and returns the original routine for the detour to call.
pub(crate) fn enter_lease() -> *mut c_void {
    CALLS.fetch_add(1, Ordering::SeqCst);
    ORIGINAL.load(Ordering::Acquire)
}

pub(crate) fn leave_lease() {
    CALLS.fetch_sub(1, Ordering::SeqCst);
}

/// Verified renderer owner and window when this call may open the Runtime creation scope.
fn owned_call(
    platform: &Platform,
    call: &CreationCall,
    services: &rsf_game_host_services,
) -> Option<(usize, usize)> {
    let owner = platform
        .read_usize(platform.main_module().wrapping_add(rva(Site::Owner)))
        .filter(|&owner| owner != 0)?;
    // Only this verified game owner may admit inner creation. The original call still uses the
    // current IAT and all original arguments, including outputs.
    if call.swapchain as usize != owner.wrapping_add(OWNER_SWAPCHAIN)
        || call.device as usize != owner.wrapping_add(OWNER_DEVICE)
        || call.context as usize != owner.wrapping_add(OWNER_CONTEXT)
        || call.description.is_null()
    {
        return None;
    }
    let mut description = [0; DESCRIPTION_SIZE];
    if !platform.read(call.description as usize, &mut description) {
        return None;
    }
    let window = platform
        .read_usize(owner.wrapping_add(OWNER_WINDOW))
        .filter(|&window| window != 0)?;
    let mut output_window = [0; size_of::<usize>()];
    output_window
        .copy_from_slice(&description[DESCRIPTION_WINDOW..DESCRIPTION_WINDOW + size_of::<usize>()]);
    (usize::from_le_bytes(output_window) == window
        && platform.render_allowed()
        && prepare_steam(services))
    .then_some((owner, window))
}

/// Before the original call: report a refused route and open the Runtime scope for the owner.
pub(crate) fn enter(platform: &Platform, call: &mut CreationCall) {
    let entry_error = platform.last_error();
    call.scope = CreationScope {
        struct_size: size_of::<CreationScope>() as u32,
        abi_version: rsf_game_sys::RSF_D3D11_HOST_ABI_VERSION,
        window: ptr::null_mut(),
        token: 0,
    };
    call.outputs = NativeChain {
        struct_size: size_of::<NativeChain>() as u32,
        abi_version: rsf_game_sys::RSF_D3D11_HOST_ABI_VERSION,
        device: ptr::null_mut(),
        immediate_context: ptr::null_mut(),
        swapchain: ptr::null_mut(),
    };
    let shared = shared();
    let route = creation_caller(platform);
    let live = LIVE.load(Ordering::Acquire);
    let routed = route.ok() == Some(call.caller);
    if let Some(services) = shared.services.as_ref().filter(|s| s.log.is_some())
        && live
        && !routed
        && REPORTS.fetch_add(1, Ordering::Relaxed) < 4
    {
        platform.log(services, route.err().unwrap_or(UNEXPECTED_CALLER));
    }
    if let Some(services) = shared.services.as_ref()
        && live
        && routed
        && shared.begin != 0
        && let Some((owner, window)) = owned_call(platform, call, services)
    {
        if call.caller != create_return(platform.main_module())
            && let Some(Ok(route)) = DISPLAY.get()
        {
            platform.log(services, &route.matched);
        }
        call.owner = owner;
        call.window = window as *mut c_void;
        call.scope.window = call.window;
        let begun = platform.creation_begin(shared.begin, services.user, &mut call.scope);
        call.begun = u8::from(begun == OK);
    }
    platform.set_last_error(entry_error);
}

/// After the original call returned or unwound: borrow the final outer outputs when the owner and
/// window still match, close the scope and restore the native last error.
pub(crate) fn leave(platform: &Platform, call: &mut CreationCall) {
    let native_error = platform.last_error();
    if call.begun != 0 {
        let shared = shared();
        if call.returned != 0
            && call.native_result >= 0
            && LIVE.load(Ordering::Acquire)
            && platform.render_allowed()
        {
            let base = platform.main_module();
            let read = |address: *mut *mut c_void| {
                platform
                    .read_usize(address as usize)
                    .filter(|&value| value != 0)
            };
            if platform.read_usize(base.wrapping_add(rva(Site::Owner))) == Some(call.owner)
                && platform.read_usize(call.owner.wrapping_add(OWNER_WINDOW))
                    == Some(call.window as usize)
                && let Some(device) = read(call.device)
                && let Some(context) = read(call.context)
                && let Some(outer) = read(call.swapchain)
            {
                call.outputs.device = device as *mut c_void;
                call.outputs.immediate_context = context as *mut c_void;
                call.outputs.swapchain = call.swapchain;
                call.outer = outer as *mut c_void;
                call.matched = 1;
            }
        }
        if let Some(services) = shared.services.as_ref()
            && shared.end != 0
        {
            let outputs = (call.matched != 0).then_some(&call.outputs);
            let admitted = platform.creation_end(shared.end, services.user, &call.scope, outputs);
            if services.log.is_some() {
                platform.log_text(
                    services,
                    &report(call.native_result, admitted, call.outer as usize),
                );
            }
        }
    }
    platform.set_last_error(native_error);
}

/// The scoped-end receipt, with the pointer printed as MSVC `%p` does.
pub(crate) fn report(native_result: i32, admission: i32, outer: usize) -> String {
    format!(
        "Skyrim native creation scoped-end native_HRESULT=0x{:08x} admission={admission} outer={outer:016X}; outer outputs and wrapper Present preserved",
        native_result as u32
    )
}

#[cfg(test)]
mod tests;
