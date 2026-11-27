// SPDX-License-Identifier: MIT
//! The plugin's renderer lifecycle. It owns the prepared/running state, the status reason, the
//! managed API table handed to the helper, and the order in which the native bridge and the Mono
//! helper are prepared, activated, drained and released.
use crate::mono;
use crate::sys::rsf_unity_native_api;
use rsf_game_sys::call_header::validate;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::win32::module_path;
use rsf_game_sys::{
    rsf_game_control_args, rsf_game_prepare_args, rsf_game_renderer_status, rsf_game_start_args,
    rsf_result,
};
use rsf_unity::bridge_native::{mono_prepare, mono_release, mono_try_close};
use std::ffi::CStr;
use std::ptr;
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW,
};

use rsf_game_sys::{
    RSF_ERROR_BUSY as BUSY, RSF_ERROR_INVALID_ARGUMENT as INVALID_ARGUMENT,
    RSF_ERROR_NOT_READY as NOT_READY, RSF_OK as OK,
};

/// Managed helper stages reported through the bridge (Bootstrap.cs).
const MANAGED_INSTALLED: u32 = 2;
const MANAGED_REFUSED: u32 = 3;

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
    bridge_owned: bool,
    prepared: bool,
    running: bool,
    reason: &'static CStr,
    /// Filled by the bridge and read by the helper's Start, which copies it.
    api: rsf_unity_native_api,
}

// The table holds bridge entry points and the bridge's render event, all of this DLL.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State {
    bridge_owned: false,
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

fn lock() -> MutexGuard<'static, State> {
    lock_or_recover(&STATE)
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
    let path = unsafe { module_path(module) }?;
    sibling(&String::from_utf16(path.units()).ok()?, HELPER)
}

/// `leaf` in the directory of `path`. None when `path` has no directory.
pub(crate) fn sibling(path: &str, leaf: &str) -> Option<String> {
    let separator = path.rfind(['/', '\\'])?;
    Some(format!("{}{leaf}", &path[..=separator]))
}

pub(crate) unsafe extern "C" fn prepare(args: *const rsf_game_prepare_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let host = unsafe { (*args).host };
    let valid = unsafe { validate(host) };
    if valid != OK {
        return valid;
    }
    let host_services = unsafe { &*host };
    if host_services.session_id == 0 {
        return INVALID_ARGUMENT;
    }
    let mut state = lock();
    if state.bridge_owned {
        return BUSY;
    }
    if !unsafe { mono_prepare(host, &raw mut state.api) } {
        return NOT_READY;
    }
    state.bridge_owned = true;
    let started = match helper_path() {
        Some(helper) => unsafe { mono::start(&helper, &raw mut state.api) },
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
        unsafe {
            rsf_game_sys::rsf_game_log_guarded(Some(log), host_services.user, state.reason.as_ptr())
        };
    }
    if mono_try_close() {
        mono_release();
        state.bridge_owned = false;
        NOT_READY
    } else {
        // Keep ownership until the host retries stop and the failed preparation drains.
        BUSY
    }
}

pub(crate) unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let mut state = lock();
    if !state.prepared {
        return NOT_READY;
    }
    crate::bridge_state::rsf_unity_frames_activate(1);
    state.running = true;
    state.reason = ACTIVE;
    OK
}

pub(crate) unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let mut state = lock();
    crate::bridge_state::rsf_unity_frames_activate(0);
    state.running = false;
    state.reason = QUIESCED;
    OK
}

pub(crate) unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let mut state = lock();
    if state.running {
        return BUSY;
    }
    // Close admission before releasing any of the host's borrowed services.
    if state.bridge_owned {
        if !mono_try_close() || !mono::stop() {
            return BUSY;
        }
        mono_release();
        state.bridge_owned = false;
    }
    state.prepared = false;
    state.reason = STOPPED;
    OK
}

pub(crate) unsafe extern "C" fn status(out: *mut rsf_game_renderer_status) -> rsf_result {
    let valid = unsafe { validate(out) };
    if valid != OK {
        return valid;
    }
    let state = lock();
    let stage = crate::bridge_state::rsf_unity_frames_managed_stage();
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
}
