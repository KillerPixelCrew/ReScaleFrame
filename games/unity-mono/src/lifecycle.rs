// SPDX-License-Identifier: MIT
//! The plugin's renderer lifecycle. It owns the prepared/running state, the status reason, the
//! managed API table handed to the helper, and the order in which the native bridge and the Mono
//! helper are prepared, activated, drained and released.
use crate::mono;
use crate::plugin_abi::{rsf_unity_native_api, rsf_unity_plugin_platform};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_game_control_args, rsf_game_prepare_args, rsf_game_renderer_status,
    rsf_game_start_args, rsf_result,
};
use std::ffi::CStr;
use std::ptr;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};

pub(crate) const OK: rsf_result = 0;
pub(crate) const INVALID_ARGUMENT: rsf_result = -1;
pub(crate) const ABI_MISMATCH: rsf_result = -2;
const NOT_READY: rsf_result = -3;
const BUSY: rsf_result = -4;

/// Managed helper stages reported through the bridge (Bootstrap.cs).
const MANAGED_INSTALLED: u32 = 2;
const MANAGED_REFUSED: u32 = 3;

const PATH_CAPACITY: usize = 32768;
const HELPER: &str = "ReScaleFrame.Unity.Managed.dll";

const UNPREPARED: &CStr = c"Unity Mono adapter has not been prepared.";
const NO_HELPER_PATH: &CStr = c"Managed Unity helper path is unavailable.";
const ACTIVE: &CStr =
    c"Unity managed adapter active; native provider execution and image validation pending.";
const QUIESCED: &CStr = c"Unity producers quiesced; queued commands may still need to drain.";
const STOPPED: &CStr =
    c"Unity adapter stopped; managed assembly remains inert in the player domain.";
const CONTRACT_REFUSED: &CStr =
    c"Managed Unity adapter refused its pipeline contract; see diagnostics.";

struct State {
    prepared: bool,
    running: bool,
    reason: &'static CStr,
    /// Filled by the bridge and read by the helper's Start, which copies it.
    api: rsf_unity_native_api,
}

// The table holds bridge entry points and the bridge's render event, all of this DLL.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State {
    prepared: false,
    running: false,
    reason: UNPREPARED,
    api: rsf_unity_native_api {
        struct_size: 0,
        abi_version: 0,
        session_id: 0,
        log: None,
        config: None,
        enqueue: None,
        render_event: ptr::null_mut(),
        managed_state: None,
        cpu_event: None,
        cpu_fact: None,
    },
});

static PLATFORM: OnceLock<Bridge> = OnceLock::new();

/// The native bridge entry points, all present.
#[derive(Clone, Copy)]
struct Bridge {
    prepare: unsafe extern "C" fn(
        host: *const rsf_game_sys::rsf_game_host_services,
        api: *mut rsf_unity_native_api,
    ) -> u32,
    activate: unsafe extern "C" fn(enabled: u32),
    drained: unsafe extern "C" fn() -> u32,
    release: unsafe extern "C" fn(),
    managed_state: unsafe extern "C" fn() -> u32,
}

impl Bridge {
    fn complete(platform: &rsf_unity_plugin_platform) -> Option<Self> {
        Some(Self {
            prepare: platform.bridge_prepare?,
            activate: platform.bridge_activate?,
            drained: platform.bridge_drained?,
            release: platform.bridge_release?,
            managed_state: platform.bridge_managed_state?,
        })
    }
}

/// Keeps the first complete bridge table. False when the table is incomplete.
pub(crate) fn adopt(platform: &rsf_unity_plugin_platform) -> bool {
    match Bridge::complete(platform) {
        Some(bridge) => {
            let _ = PLATFORM.get_or_init(|| bridge);
            true
        }
        None => false,
    }
}

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A host structure must be at least as long as this plugin's version and carry its ABI.
unsafe fn validate<T>(value: *const T) -> Result<(), rsf_result> {
    let header = value.cast::<u32>();
    if value.is_null() || unsafe { header.read() } < size_of::<T>() as u32 {
        return Err(INVALID_ARGUMENT);
    }
    if unsafe { header.add(1).read() } != RSF_GAME_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    Ok(())
}

/// ReScaleFrame.Unity.Managed.dll beside this plugin, as UTF-8.
fn helper_path() -> Option<String> {
    let mut module: HMODULE = ptr::null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    // Any address inside this DLL names its module.
    let anchor = (helper_path as *const ()).cast::<u16>();
    if unsafe { GetModuleHandleExW(flags, anchor, &mut module) } == 0 {
        return None;
    }
    let mut path = vec![0u16; PATH_CAPACITY];
    let length =
        unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), PATH_CAPACITY as u32) } as usize;
    if length == 0 || length >= PATH_CAPACITY {
        return None;
    }
    let plugin = String::from_utf16(&path[..length]).ok()?;
    sibling(&plugin, HELPER)
}

/// `leaf` in the directory of `path`. None when `path` has no directory.
pub(crate) fn sibling(path: &str, leaf: &str) -> Option<String> {
    let separator = path.rfind(['/', '\\'])?;
    Some(format!("{}{leaf}", &path[..=separator]))
}

pub(crate) unsafe extern "C" fn prepare(args: *const rsf_game_prepare_args) -> rsf_result {
    if let Err(error) = unsafe { validate(args) } {
        return error;
    }
    let host = unsafe { (*args).host };
    if let Err(error) = unsafe { validate(host) } {
        return error;
    }
    let host_services = unsafe { &*host };
    if host_services.session_id == 0 {
        return INVALID_ARGUMENT;
    }
    let Some(bridge) = PLATFORM.get() else {
        return NOT_READY;
    };
    let mut state = lock();
    if state.prepared {
        return BUSY;
    }
    if unsafe { (bridge.prepare)(host, &raw mut state.api) } == 0 {
        return NOT_READY;
    }
    let started = match helper_path() {
        Some(helper) => mono::start(&helper, &raw mut state.api),
        None => Err(NO_HELPER_PATH),
    };
    state.reason = match started {
        Ok(reason) | Err(reason) => reason,
    };
    state.prepared = started.is_ok();
    if state.prepared {
        return OK;
    }
    if let Some(log) = host_services.log {
        unsafe { log(host_services.user, state.reason.as_ptr()) };
    }
    unsafe { (bridge.release)() };
    NOT_READY
}

pub(crate) unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    if let Err(error) = unsafe { validate(args) } {
        return error;
    }
    let mut state = lock();
    let Some(bridge) = PLATFORM.get().filter(|_| state.prepared) else {
        return NOT_READY;
    };
    unsafe { (bridge.activate)(1) };
    state.running = true;
    state.reason = ACTIVE;
    OK
}

pub(crate) unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    if let Err(error) = unsafe { validate(args) } {
        return error;
    }
    let mut state = lock();
    if let Some(bridge) = PLATFORM.get() {
        unsafe { (bridge.activate)(0) };
    }
    state.running = false;
    state.reason = QUIESCED;
    OK
}

pub(crate) unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    if let Err(error) = unsafe { validate(args) } {
        return error;
    }
    let mut state = lock();
    if state.running {
        return BUSY;
    }
    if let Some(bridge) = PLATFORM.get() {
        // Retire only after every callback, packet and Unity submission has completed.
        if unsafe { (bridge.drained)() } == 0 || !mono::stop() {
            return BUSY;
        }
        unsafe { (bridge.release)() };
    }
    state.prepared = false;
    state.reason = STOPPED;
    OK
}

pub(crate) unsafe extern "C" fn status(out: *mut rsf_game_renderer_status) -> rsf_result {
    if let Err(error) = unsafe { validate(out) } {
        return error;
    }
    let state = lock();
    let stage = PLATFORM
        .get()
        .map_or(0, |bridge| unsafe { (bridge.managed_state)() });
    let out = unsafe { &mut *out };
    out.prepared = u32::from(state.prepared);
    out.active = u32::from(state.running && stage == MANAGED_INSTALLED);
    out.rendering_ready = 0;
    let reason = if stage == MANAGED_REFUSED {
        CONTRACT_REFUSED
    } else {
        state.reason
    };
    out.reason = reason.as_ptr();
    OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_sits_beside_the_plugin() {
        assert_eq!(
            sibling(
                r"C:\Game\ReScaleFrame\ReScaleFrame.Game.UnityMono.dll",
                HELPER
            )
            .as_deref(),
            Some(r"C:\Game\ReScaleFrame\ReScaleFrame.Unity.Managed.dll")
        );
        assert_eq!(sibling("plugin.dll", HELPER), None);
    }

    #[test]
    fn lifecycle_refuses_short_and_foreign_structures() {
        let mut args = rsf_game_start_args {
            struct_size: size_of::<rsf_game_start_args>() as u32 - 1,
            abi_version: RSF_GAME_ABI_VERSION,
        };
        assert_eq!(unsafe { validate(&args) }, Err(INVALID_ARGUMENT));
        args.struct_size += 1;
        args.abi_version += 1;
        assert_eq!(unsafe { validate(&args) }, Err(ABI_MISMATCH));
        args.abi_version -= 1;
        assert_eq!(unsafe { validate(&args) }, Ok(()));
        assert_eq!(
            unsafe { validate(ptr::null::<rsf_game_start_args>()) },
            Err(INVALID_ARGUMENT)
        );
    }
}
