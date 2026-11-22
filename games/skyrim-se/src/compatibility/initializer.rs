// SPDX-License-Identifier: MIT
//! Process-lived Community Shaders resolver and initializer transaction.

use super::{
    CONFIGURE, HDR_PROCESS, HdrCall, INITIALIZE, Read, allowed, before_device, excluded_byte,
    features_inactive,
};
use crate::platform::{self, Platform};
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

const PATH_LIMIT: usize = 32768;
const EXCEPTION: &CStr = c"CS initializer exception; RSF coexistence refused for this process.";
const ARMED: &CStr = c"CS initializer interception armed before module load; awaiting exact-profile initialization if present.";
const EXCLUDED: &CStr = c"CS 1.9.1 Upscaling lifecycle excluded; original configuration and other feature initialization forwarded; runtime coexistence unvalidated.";

type Withdraw = unsafe extern "C" fn();

#[repr(C)]
struct Leaves {
    read: Read,
    current_module: unsafe extern "C" fn() -> usize,
    borrow_module: unsafe extern "C" fn(usize) -> usize,
    release_module: unsafe extern "C" fn(usize),
    module_path: unsafe extern "C" fn(usize, *mut u16, u32) -> u32,
    file_kind: unsafe extern "C" fn(*const u16) -> u32,
    profile: unsafe extern "C" fn(*const u16, usize) -> bool,
    hook_initialize: unsafe extern "C" fn() -> bool,
    loader_callout: unsafe extern "C" fn(usize) -> bool,
    initialize: unsafe extern "C" fn(usize, *const c_void) -> u32,
    resolver_detour: *const c_void,
    initialize_detour: usize,
    configure_detour: *const c_void,
    hdr_detour: *const c_void,
}

const _: () = assert!(size_of::<Leaves>() == 112);

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
        unsafe { callback() };
    }
}

struct Module<'a> {
    value: usize,
    leaves: &'a Leaves,
}

impl<'a> Module<'a> {
    fn borrow(leaves: &'a Leaves, address: usize) -> Option<Self> {
        let value = unsafe { (leaves.borrow_module)(address) };
        (value != 0).then(|| Self { value, leaves })
    }
}

impl Drop for Module<'_> {
    fn drop(&mut self) {
        unsafe { (self.leaves.release_module)(self.value) };
    }
}

fn module_path(leaves: &Leaves, module: usize, buffer: &mut [u16; PATH_LIMIT]) -> Option<usize> {
    let len =
        unsafe { (leaves.module_path)(module, buffer.as_mut_ptr(), PATH_LIMIT as u32) } as usize;
    (len != 0 && len < PATH_LIMIT).then_some(len)
}

fn installed_profile(leaves: &Leaves) -> Result<(), &'static CStr> {
    let mut path = [0; PATH_LIMIT];
    if let Some(module) = Module::borrow(leaves, 0) {
        return if module_path(leaves, module.value, &mut path).is_some()
            && unsafe { (leaves.profile)(path.as_ptr(), module.value) }
        {
            Err(c"Community Shaders 1.9.1 already present before interception; prior initialization cannot be excluded.")
        } else {
            Err(c"Community Shaders refused: loaded module differs from the guarded 1.9.1 profile or cannot be inspected.")
        };
    }
    let unavailable = c"Community Shaders presence check failed: executable directory unavailable.";
    let length = module_path(leaves, 0, &mut path).ok_or(unavailable)?;
    let directory = path[..length]
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
    match unsafe { (leaves.file_kind)(candidate.as_ptr()) } {
        1 => Ok(()), // absent file or path
        2 if unsafe { (leaves.profile)(candidate.as_ptr(), 0) } => Ok(()),
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

fn arm(platform: &Platform, leaves: &Leaves) -> Result<(), &'static CStr> {
    installed_profile(leaves)?;
    if !unsafe { (leaves.hook_initialize)() } {
        return Err(c"CS initializer interception could not initialize native hooks.");
    }
    let target = platform.export_address(platform.module_handle("kernel32.dll"), c"GetProcAddress");
    let callout = platform.export_address(
        platform.module_handle("ntdll.dll"),
        c"RtlIsThreadWithinLoaderCallout",
    );
    if target == 0
        || callout == 0
        || unsafe { (leaves.loader_callout)(callout) }
        || platform.pin_module(leaves.resolver_detour.addr()) == 0
        || !create_hook(platform, 0, target, leaves.resolver_detour)
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
    if unsafe { (leaves.current_module)() } != 0 {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cs_checkpoint(
    platform: *const Platform,
    leaves: &Leaves,
    withdraw: Option<Withdraw>,
    refusal: &mut *const c_char,
) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
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
    if let Err(reason) = arm(platform, leaves) {
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

/// The native resolver lends a Win32 name or ordinal after calling the original on its own frame.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cs_resolved(
    leaves: &Leaves,
    module: usize,
    name: *const c_char,
    result: usize,
) -> usize {
    if result == 0
        || module == 0
        || name.addr() <= 0xffff
        || unsafe { CStr::from_ptr(name) } != c"SKSEPlugin_Load"
        || module != unsafe { (leaves.current_module)() }
    {
        return result;
    }
    // This may run in a loader callout. Publish scalars only and keep all effects outside the lock.
    let mut state = state();
    if state.phase == Phase::Armed {
        state.module = module;
        state.original = result;
        state.phase = Phase::Captured;
    }
    if state.module == module && state.original == result {
        leaves.initialize_detour
    } else {
        result
    }
}

fn call_initializer(leaves: &Leaves, original: usize, skse: *const c_void) -> bool {
    match unsafe { (leaves.initialize)(original, skse) } {
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
    leaves: &Leaves,
    reason: &'static CStr,
    original: usize,
    skse: *const c_void,
) -> bool {
    refuse(reason, true);
    before_device(platform.main_module(), leaves.read) == Some(true)
        && call_initializer(leaves, original, skse)
}

fn initialize(platform: &Platform, leaves: &Leaves, skse: *const c_void) -> bool {
    let (module, original, callout) = {
        let mut state = state();
        if state.phase != Phase::Captured {
            return false;
        }
        state.phase = Phase::Initializing;
        (state.module, state.original, state.callout)
    };
    if callout == 0 || unsafe { (leaves.loader_callout)(callout) } {
        refuse(
            c"CS handover refused: initializer entered from a loader callout.",
            false,
        );
        return false;
    }
    let borrowed = if module == 0 {
        None
    } else {
        Module::borrow(leaves, module)
    };
    let Some(_module) = borrowed.filter(|hold| hold.value == module) else {
        refuse(
            c"CS initializer module no longer available; ownership refused.",
            false,
        );
        return false;
    };
    let mut path = [0; PATH_LIMIT];
    let length = module_path(leaves, module, &mut path);
    if module.checked_add(INITIALIZE) != Some(original)
        || length.is_none()
        || !unsafe { (leaves.profile)(path.as_ptr(), module) }
        || features_inactive(module, false, leaves.read) != Some(true)
        || before_device(platform.main_module(), leaves.read) != Some(true)
    {
        return retain_original(platform, leaves,
            c"CS profile/dormancy/startup guard failed; RSF withdrawn, original CS retained only before device creation.", original, skse);
    }
    if platform.pin_module(module) == 0 {
        return retain_original(
            platform,
            leaves,
            c"CS handover module lifetime unavailable; RSF withdrawn.",
            original,
            skse,
        );
    }
    let configure = module + CONFIGURE;
    let hdr = module + HDR_PROCESS;
    if !create_hook(platform, 1, configure, leaves.configure_detour) {
        return retain_original(
            platform,
            leaves,
            c"CS Feature configuration hook unavailable; RSF withdrawn.",
            original,
            skse,
        );
    }
    let hdr_created = create_hook(platform, 2, hdr, leaves.hdr_detour);
    if !hdr_created || !platform.hook_enable(configure) || !platform.hook_enable(hdr) {
        let reason = c"CS configuration/HDR chain hook activation failed; RSF withdrawn.";
        refuse(reason, false);
        // Disabled or still reachable, every created trampoline stays valid. Refused detours
        // forward without excluding the feature. There is no remapping after this terminal result.
        platform.hook_disable(configure);
        if hdr_created {
            platform.hook_disable(hdr);
        }
        return retain_original(platform, leaves, reason, original, skse);
    }
    let result = match unsafe { (leaves.initialize)(original, skse) } {
        2 => {
            refuse(EXCEPTION, true);
            return false;
        }
        status => status == 1,
    };
    if !result || features_inactive(module, false, leaves.read) != Some(true) {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cs_initialize(
    platform: *const Platform,
    leaves: &Leaves,
    skse: *const c_void,
) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    initialize(platform, leaves, skse)
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

fn render_allowed(leaves: &Leaves, module: usize) -> bool {
    let (phase, admitted) = {
        let state = state();
        (state.phase, state.module)
    };
    if module == 0 {
        return phase != Phase::Refused;
    }
    phase == Phase::Excluded
        && module == admitted
        && features_inactive(module, true, leaves.read) == Some(true)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_render_allowed(leaves: &Leaves) -> bool {
    render_allowed(leaves, unsafe { (leaves.current_module)() })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_status(leaves: &Leaves) -> *const c_char {
    let module = unsafe { (leaves.current_module)() };
    let (phase, reason) = {
        let state = state();
        (state.phase, state.reason)
    };
    if phase == Phase::Excluded && !render_allowed(leaves, module) {
        return c"CS HDR chain preserved; float16 scene readers, PQ/HDR setup metadata and post-composite HUD-less lease still required; RSF HDR processing refused.".as_ptr();
    }
    if phase == Phase::Refused || module != 0 {
        reason.map_or(ptr::null(), CStr::as_ptr)
    } else {
        ptr::null()
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_postprocess_allowed(game: usize, call: &HdrCall, read: Read) -> bool {
    let (phase, module) = {
        let state = state();
        (state.phase, state.module)
    };
    allowed(game, module, phase == Phase::Excluded, call, read).is_some()
}
