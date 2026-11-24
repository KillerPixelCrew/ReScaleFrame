// SPDX-License-Identifier: MIT
//! Process-lived Community Shaders resolver and initializer transaction.

use super::{
    CONFIGURE, HDR_PROCESS, HdrCall, INITIALIZE, MAX_IMAGE_BYTES, Read, SHA256, allowed,
    before_device, excluded_byte, features_inactive, profile_matches,
};
use crate::platform::{self, Platform};
use rsf_game_sys::win32::module_path;
use std::ffi::{CStr, OsString, c_char, c_void};
use std::fs::OpenOptions;
use std::os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, io::AsRawHandle};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, FARPROC, FreeLibrary, GetLastError,
    HANDLE, HMODULE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_SHARE_READ, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExW, GetModuleHandleW,
};

use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READONLY,
    UnmapViewOfFile,
};

const MODULE_NAME: *const u16 = windows_sys::w!("CommunityShaders.dll");
const EXCEPTION: &CStr = c"CS initializer exception; RSF coexistence refused for this process.";
const ARMED: &CStr = c"CS initializer interception armed before module load; awaiting exact-profile initialization if present.";
const EXCLUDED: &CStr = c"CS 1.9.1 Upscaling lifecycle excluded; original configuration and other feature initialization forwarded; runtime coexistence unvalidated.";

type Withdraw = fn();

unsafe extern "system" {
    fn rsf_skyrim_cs_resolve(module: HMODULE, name: *const u8) -> FARPROC;
}

unsafe extern "C" {
    fn rsf_skyrim_cs_call_initializer(original: usize, skse: *const c_void) -> u32;
    fn rsf_skyrim_cs_configure(feature: *mut c_void, json: *mut c_void);
    fn rsf_skyrim_cs_hdr_process(
        manager: *mut c_void,
        input: u32,
        output: u32,
        arg4: usize,
        arg5: u8,
    );
}

use rsf_game_sys::win32::read_process as read;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Unarmed,
    Arming,
    Armed,
    Captured,
    Initializing,
    Excluded,
    Refused,
}

struct State {
    phase: Phase,
    reason: Option<&'static CStr>,
    module: usize,
    original: usize,
    callout: usize,
    withdraw: Option<Withdraw>,
}

impl State {
    fn capture(&mut self, module: usize, original: usize) -> bool {
        if self.phase == Phase::Armed {
            self.module = module;
            self.original = original;
            self.phase = Phase::Captured;
        }
        self.module == module && self.original == original
    }

    fn claim_initializer(&mut self) -> Option<(usize, usize, usize)> {
        if self.phase != Phase::Captured {
            return None;
        }
        self.phase = Phase::Initializing;
        Some((self.module, self.original, self.callout))
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    phase: Phase::Unarmed,
    reason: None,
    module: 0,
    original: 0,
    callout: 0,
    withdraw: None,
});
// Each published trampoline remains callable for the life of the pinned adapter.
static HOOKS: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn refuse(reason: &'static CStr, withdraw: bool) {
    let callback = {
        let mut state = state();
        state.phase = Phase::Refused;
        state.reason = Some(reason);
        state.withdraw
    };
    if withdraw && let Some(callback) = callback {
        callback();
    }
}

fn current_module() -> usize {
    unsafe { GetModuleHandleW(MODULE_NAME) as usize }
}

struct Module(usize);

impl Module {
    fn borrow(address: usize) -> Option<Self> {
        let (flags, name) = if address == 0 {
            (0, MODULE_NAME)
        } else {
            (
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                address as *const u16,
            )
        };
        let mut hold = ptr::null_mut();
        // A reference keeps the exact module loaded throughout the handoff.
        let loaded = unsafe { GetModuleHandleExW(flags, name, &mut hold) };
        (loaded != 0).then(|| Self(hold as usize))
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0 as _) };
    }
}

fn file_kind(path: &[u16]) -> u32 {
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes != INVALID_FILE_ATTRIBUTES {
        return if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            3
        } else {
            2
        };
    }
    match unsafe { GetLastError() } {
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => 1,
        _ => 0,
    }
}

fn loader_callout(address: usize) -> bool {
    // Checked ntdll RtlIsThreadWithinLoaderCallout export.
    let query = unsafe { std::mem::transmute::<usize, unsafe extern "system" fn() -> u8>(address) };
    unsafe { query() != 0 }
}

struct Mapping(HANDLE);

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct View(MEMORY_MAPPED_VIEW_ADDRESS);

impl Drop for View {
    fn drop(&mut self) {
        unsafe { UnmapViewOfFile(self.0) };
    }
}

fn inspect_profile(path: &[u16], module: usize, read: Read) -> Option<bool> {
    let path = OsString::from_wide(path);
    let path = std::path::Path::new(&path);
    // This read-only handle denies writes and replacement through fingerprinting and mapping.
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .ok()?;
    let length = usize::try_from(file.metadata().ok()?.len()).ok()?;
    if !(1..=MAX_IMAGE_BYTES).contains(&length)
        || platform::hex_sha256(path).as_ref() != Some(SHA256)
    {
        return Some(false);
    }
    let mapping = unsafe {
        CreateFileMappingW(
            file.as_raw_handle().cast(),
            ptr::null(),
            PAGE_READONLY,
            0,
            0,
            ptr::null(),
        )
    };
    if mapping.is_null() {
        return None;
    }
    let mapping = Mapping(mapping);
    let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_READ, 0, 0, length) };
    if view.Value.is_null() {
        return None;
    }
    let view = View(view);
    let bytes = unsafe { std::slice::from_raw_parts(view.0.Value.cast::<u8>(), length) };
    Some(profile_matches(bytes, module, read))
}

fn installed_profile() -> Result<(), &'static CStr> {
    if let Some(module) = Module::borrow(0) {
        return if unsafe { module_path(module.0 as _) }
            .is_some_and(|path| inspect_profile(path.units(), module.0, read) == Some(true))
        {
            Err(c"Community Shaders 1.9.1 already present before interception; prior initialization cannot be excluded.")
        } else {
            Err(c"Community Shaders refused: loaded module differs from the guarded 1.9.1 profile or cannot be inspected.")
        };
    }
    let unavailable = c"Community Shaders presence check failed: executable directory unavailable.";
    let path = unsafe { module_path(ptr::null_mut()) }.ok_or(unavailable)?;
    let path = path.units();
    let directory = path
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\'))
        .ok_or(unavailable)?
        + 1;
    let suffix = "Data\\SKSE\\Plugins\\CommunityShaders.dll";
    let mut candidate = Vec::new();
    candidate
        .try_reserve_exact(directory + suffix.len() + 1)
        .map_err(|_| unavailable)?;
    candidate.extend_from_slice(&path[..directory]);
    candidate.extend(suffix.encode_utf16());
    candidate.push(0);
    match file_kind(&candidate) {
        1 => Ok(()), // absent file or path
        2 if inspect_profile(&candidate[..candidate.len() - 1], 0, read) == Some(true) => Ok(()),
        0 => Err(c"Community Shaders presence check failed: installed plugin path unreadable."),
        _ => Err(
            c"Community Shaders installed profile unknown or unreadable; early handover refused.",
        ),
    }
}

fn create_hook(platform: &Platform, slot: usize, target: usize, detour: *const c_void) -> bool {
    let Some(original) = platform.hook_create(target, detour) else {
        return false;
    };
    HOOKS[slot].store(original.addr(), Ordering::Release);
    true
}

fn arm(platform: &Platform) -> Result<(), &'static CStr> {
    installed_profile()?;
    if !rsf_windows::minhook::initialize() {
        return Err(c"CS initializer interception could not initialize native hooks.");
    }
    let target = platform.export_address(platform.module_handle("kernel32.dll"), c"GetProcAddress");
    let callout = platform.export_address(
        platform.module_handle("ntdll.dll"),
        c"RtlIsThreadWithinLoaderCallout",
    );
    if target == 0
        || callout == 0
        || loader_callout(callout)
        || platform.pin_module(rsf_skyrim_cs_resolve as *const () as usize) == 0
        || !create_hook(platform, 0, target, rsf_skyrim_cs_resolve as *const c_void)
    {
        return Err(c"CS initializer interception unavailable outside loader callout.");
    }
    {
        let mut state = state();
        state.phase = Phase::Armed;
        state.reason = Some(ARMED);
        state.callout = callout;
    }
    if !platform.hook_enable(target) {
        let reason = c"CS initializer interception could not be enabled.";
        refuse(reason, false);
        platform.hook_disable(target);
        return Err(reason);
    }
    if current_module() != 0 {
        let mut state = state();
        if state.phase == Phase::Armed {
            let reason = c"CS appeared before initializer interception completed; startup ordering ambiguous.";
            state.phase = Phase::Refused;
            state.reason = Some(reason);
            return Err(reason);
        }
    }
    Ok(())
}

#[cfg(not(test))]
pub(crate) fn checkpoint(refusal: &mut *const c_char, arm: bool) -> bool {
    let withdraw: Option<Withdraw> = arm.then_some(crate::session::withdraw as Withdraw);
    checkpoint_with(refusal, withdraw)
}

fn checkpoint_with(refusal: &mut *const c_char, withdraw: Option<Withdraw>) -> bool {
    let Some(platform) = platform::get() else {
        return false;
    };
    {
        let mut state = state();
        if withdraw.is_some() {
            state.withdraw = withdraw
        }
        match state.phase {
            Phase::Unarmed => state.phase = Phase::Arming,
            Phase::Arming => {
                *refusal = c"CS initializer interception is already being armed.".as_ptr();
                return false;
            }
            Phase::Refused => {
                *refusal = state.reason.map_or(ptr::null(), CStr::as_ptr);
                return false;
            }
            _ => return true,
        }
    }
    if let Err(reason) = arm(platform) {
        let mut state = state();
        if state.phase == Phase::Arming {
            state.phase = Phase::Unarmed
        }
        *refusal = reason.as_ptr();
        return false;
    }
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_original(slot: u32) -> usize {
    HOOKS
        .get(slot as usize)
        .map_or(0, |original| original.load(Ordering::Acquire))
}

/// The native resolver lends the original address and a Win32 name or ordinal.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cs_resolved(
    module: usize,
    name: *const c_char,
    result: usize,
) -> usize {
    if result == 0
        || module == 0
        || name.addr() <= 0xffff
        || unsafe { CStr::from_ptr(name) } != c"SKSEPlugin_Load"
        || module != current_module()
    {
        return result;
    }
    // Loader callout: publish scalars only. Initialization and all effects run later.
    if state().capture(module, result) {
        initialize_detour as *const () as usize
    } else {
        result
    }
}

fn call_initializer(original: usize, skse: *const c_void) -> bool {
    match unsafe { rsf_skyrim_cs_call_initializer(original, skse) } {
        1 => true,
        2 => {
            refuse(EXCEPTION, true);
            false
        }
        _ => false,
    }
}

fn retain_original(
    platform: &Platform,
    reason: &'static CStr,
    original: usize,
    skse: *const c_void,
) -> bool {
    refuse(reason, true);
    before_device(platform.main_module(), read) == Some(true) && call_initializer(original, skse)
}

fn initialize(platform: &Platform, skse: *const c_void) -> bool {
    let claim = state().claim_initializer();
    let Some((module, original, callout)) = claim else {
        return false;
    };
    if callout == 0 || loader_callout(callout) {
        refuse(
            c"CS handover refused: initializer entered from a loader callout.",
            false,
        );
        return false;
    }
    let borrowed = if module == 0 {
        None
    } else {
        Module::borrow(module)
    };
    let Some(_module) = borrowed.filter(|hold| hold.0 == module) else {
        refuse(
            c"CS initializer module no longer available; ownership refused.",
            false,
        );
        return false;
    };
    let path = unsafe { module_path(module as _) };
    if module.checked_add(INITIALIZE) != Some(original)
        || path.is_none_or(|path| inspect_profile(path.units(), module, read) != Some(true))
        || features_inactive(module, false, read) != Some(true)
        || before_device(platform.main_module(), read) != Some(true)
    {
        return retain_original(platform,
            c"CS profile/dormancy/startup guard failed; RSF withdrawn, original CS retained only before device creation.", original, skse);
    }
    if platform.pin_module(module) == 0 {
        return retain_original(
            platform,
            c"CS handover module lifetime unavailable; RSF withdrawn.",
            original,
            skse,
        );
    }
    let configure = module + CONFIGURE;
    let hdr = module + HDR_PROCESS;
    if !create_hook(
        platform,
        1,
        configure,
        rsf_skyrim_cs_configure as *const c_void,
    ) {
        return retain_original(
            platform,
            c"CS Feature configuration hook unavailable; RSF withdrawn.",
            original,
            skse,
        );
    }
    let hdr_created = create_hook(platform, 2, hdr, rsf_skyrim_cs_hdr_process as *const c_void);
    if !hdr_created || !platform.hook_enable(configure) || !platform.hook_enable(hdr) {
        let reason = c"CS configuration/HDR chain hook activation failed; RSF withdrawn.";
        refuse(reason, false);
        // Disabled or still reachable, every created trampoline stays valid. Refused detours
        // forward without excluding the feature. There is no remapping after this terminal result.
        platform.hook_disable(configure);
        if hdr_created {
            platform.hook_disable(hdr);
        }
        return retain_original(platform, reason, original, skse);
    }
    let result = match unsafe { rsf_skyrim_cs_call_initializer(original, skse) } {
        2 => {
            refuse(EXCEPTION, true);
            return false;
        }
        status => status == 1,
    };
    if !result || features_inactive(module, false, read) != Some(true) {
        refuse(
            c"CS handover refused: original initialization failed or Upscaling exclusion was lost.",
            true,
        );
        return result;
    }
    let mut state = state();
    if state.phase == Phase::Initializing {
        state.phase = Phase::Excluded;
        state.reason = Some(EXCLUDED);
    }
    result
}

unsafe extern "C" fn initialize_detour(skse: *const c_void) -> bool {
    let Some(platform) = platform::get() else {
        return false;
    };
    initialize(platform, skse)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_excluded_byte(feature: usize) -> usize {
    let state = state();
    if matches!(state.phase, Phase::Initializing | Phase::Excluded) {
        excluded_byte(state.module, feature)
    } else {
        0
    }
}

fn module_allowed(module: usize) -> bool {
    let (phase, admitted) = {
        let state = state();
        (state.phase, state.module)
    };
    if module == 0 {
        return phase != Phase::Refused;
    }
    phase == Phase::Excluded
        && module == admitted
        && features_inactive(module, true, read) == Some(true)
}

pub(crate) fn render_allowed() -> bool {
    module_allowed(current_module())
}

pub(crate) fn status() -> *const c_char {
    let module = current_module();
    let (phase, reason) = {
        let state = state();
        (state.phase, state.reason)
    };
    if phase == Phase::Excluded && !module_allowed(module) {
        return c"CS HDR chain preserved; float16 scene readers, PQ/HDR setup metadata and post-composite HUD-less lease still required; RSF HDR processing refused.".as_ptr();
    }
    if phase == Phase::Refused || module != 0 {
        reason.map_or(ptr::null(), CStr::as_ptr)
    } else {
        ptr::null()
    }
}

pub(crate) fn matches_postprocess(call: &HdrCall) -> bool {
    let Some(platform) = platform::get() else {
        return false;
    };
    let game = platform.main_module();
    let (phase, module) = {
        let state = state();
        (state.phase, state.module)
    };
    allowed(game, module, phase == Phase::Excluded, call, read).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_initializer_lookups_remain_wrapped_with_one_invocation() {
        let mut state = State {
            phase: Phase::Armed,
            reason: None,
            module: 0,
            original: 0,
            callout: 3,
            withdraw: None,
        };
        assert!(state.capture(7, 11));
        assert!(state.capture(7, 11));
        assert!(!state.capture(8, 11));
        assert!(!state.capture(7, 12));
        assert_eq!(state.claim_initializer(), Some((7, 11, 3)));
        assert!(state.capture(7, 11));
        assert!(state.claim_initializer().is_none());
        state.phase = Phase::Excluded;
        assert!(state.capture(7, 11));
        assert!(state.claim_initializer().is_none());
        state.phase = Phase::Refused;
        assert!(state.capture(7, 11));
        assert!(state.claim_initializer().is_none());
    }
}
