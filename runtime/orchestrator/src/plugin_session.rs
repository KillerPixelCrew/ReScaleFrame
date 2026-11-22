// SPDX-License-Identifier: MIT
use crate::sdk_module::SdkModule;
use crate::sync::lock;
use rsf_game_sys::windows_path::absolute_wide_path;
use rsf_game_sys::*;
use rsf_game_sys::{RSF_PLUGIN_SESSION_ABI_VERSION, rsf_plugin_session_options};
use std::ffi::CStr;
use std::ptr;
use std::sync::{Mutex, RwLock};
use windows_sys::Win32::{
    Foundation::FARPROC,
    System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
    },
};

const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;
const NATIVE_REFUSED: i32 = -5;
const GAME_RECOGNIZED: rsf_detection = 1;

const CONTROL_ARGS: rsf_game_control_args = rsf_game_control_args {
    struct_size: size_of::<rsf_game_control_args>() as u32,
    abi_version: RSF_GAME_ABI_VERSION,
};

type Control = Option<unsafe extern "C" fn(*const rsf_game_control_args) -> i32>;
unsafe extern "C" {
    fn rsf_plugin_call_query(
        entry: rsf_get_game_plugin_api_fn,
        version: u32,
        api: *mut rsf_game_plugin_api,
    ) -> i32;
    fn rsf_plugin_call_detect(
        detect: rsf_detect_game_fn,
        probe: *const rsf_game_probe,
        detection: *mut rsf_detection,
    ) -> i32;
    fn rsf_plugin_call_prepare(
        hooks: *const rsf_game_hooks_api,
        args: *const rsf_game_prepare_args,
    ) -> i32;
    fn rsf_plugin_call_start(
        hooks: *const rsf_game_hooks_api,
        args: *const rsf_game_start_args,
    ) -> i32;
    fn rsf_plugin_call_control(callback: Control, args: *const rsf_game_control_args) -> i32;
    fn rsf_plugin_call_status(
        hooks: *const rsf_game_hooks_api,
        out: *mut rsf_game_renderer_status,
    ) -> i32;
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    // Not startable: prepare has not succeeded, or a quiesce began and has not finished.
    Closed,
    Prepared,
    Started,
    Quiesced,
}

// GameSDK lifecycle callbacks have no instance handle. Reserve the loaded
// module before any plugin calls and keep that reservation through final stop.
static ADMITTED_MODULES: Mutex<Vec<usize>> = Mutex::new(Vec::new());

struct PluginModule(SdkModule);

impl PluginModule {
    fn admit(module: SdkModule) -> Result<Self, i32> {
        let identity = module.as_ptr() as usize;
        let mut modules = lock(&ADMITTED_MODULES);
        if modules.contains(&identity) {
            return Err(BUSY);
        }
        modules.try_reserve(1).map_err(|_| NOT_READY)?;
        modules.push(identity);
        Ok(Self(module))
    }
}

impl Drop for PluginModule {
    fn drop(&mut self) {
        let identity = self.0.as_ptr() as usize;
        let mut modules = lock(&ADMITTED_MODULES);
        modules.retain(|&admitted| admitted != identity);
    }
}

pub(super) struct Session {
    _module: PluginModule,
    hooks: rsf_game_hooks_api,
    services: rsf_game_host_services,
    phase: RwLock<Phase>,
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_plugin_session_prepare(
    options: *const rsf_plugin_session_options,
    out: *mut *mut Session,
) -> i32 {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    if options.is_null()
        || (unsafe { options.cast::<u32>().read() } as usize)
            < size_of::<rsf_plugin_session_options>()
    {
        return INVALID_ARGUMENT;
    }
    let options = unsafe { &*options };
    if !unsafe { absolute_wide_path(options.plugin_path, 32767) }
        || options.probe.is_null()
        || (unsafe { options.probe.cast::<u32>().read() } as usize) < size_of::<rsf_game_probe>()
        || (options.services.struct_size as usize) < size_of::<rsf_game_host_services>()
        || options.services.session_id == 0
    {
        return INVALID_ARGUMENT;
    }
    if options.abi_version != RSF_PLUGIN_SESSION_ABI_VERSION
        || options.services.abi_version != RSF_GAME_ABI_VERSION
    {
        return ABI_MISMATCH;
    }

    let handle = unsafe {
        LoadLibraryExW(
            options.plugin_path,
            ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    let Some(module) = SdkModule::adopt_system(handle) else {
        return NOT_READY;
    };
    let module = match PluginModule::admit(module) {
        Ok(module) => module,
        Err(result) => return result,
    };
    let name = CStr::from_bytes_with_nul(RSF_GAME_ENTRY_POINT).expect("GameSDK entry point");
    // This export uses the checked GameSDK signature on Windows x64.
    let entry = unsafe {
        std::mem::transmute::<FARPROC, rsf_get_game_plugin_api_fn>(module.0.symbol(name))
    };
    if entry.is_none() {
        return ABI_MISMATCH;
    }
    let mut api: rsf_game_plugin_api = unsafe { std::mem::zeroed() };
    api.struct_size = size_of_val(&api) as u32;
    let result = unsafe { rsf_plugin_call_query(entry, RSF_GAME_ABI_VERSION, &mut api) };
    if result != 0 {
        return result;
    }
    if (api.struct_size as usize) < size_of_val(&api) || api.abi_version != RSF_GAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if api.detect.is_none() {
        return NATIVE_REFUSED;
    }
    let mut detection = 0;
    let result = unsafe { rsf_plugin_call_detect(api.detect, options.probe, &mut detection) };
    if result != 0 {
        return result;
    }
    if detection != GAME_RECOGNIZED {
        return NATIVE_REFUSED;
    }
    let h = &api.hooks;
    if h.prepare.is_none()
        || h.start.is_none()
        || h.quiesce.is_none()
        || h.stop.is_none()
        || h.status.is_none()
    {
        return ABI_MISMATCH;
    }

    // A plugin may retain the services address until stop succeeds. Allocate
    // its final owner before prepare and preserve recoverable allocation failure.
    let raw = crate::allocate_handle(Session {
        _module: module,
        hooks: api.hooks,
        services: options.services,
        phase: RwLock::new(Phase::Closed),
    });
    if raw.is_null() {
        return NOT_READY;
    }
    // Keep the allocation at the same address throughout the plugin's lease.
    let session = unsafe { &*raw };
    let args = rsf_game_prepare_args {
        struct_size: size_of::<rsf_game_prepare_args>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        host: &session.services,
    };
    let result = unsafe { rsf_plugin_call_prepare(&session.hooks, &args) };
    let result = if result == 0 {
        *session.phase.write().unwrap() = Phase::Prepared;
        0
    } else if unsafe { rsf_plugin_session_quiesce(raw) } == 0
        && unsafe { rsf_plugin_session_stop(raw) } == 0
    {
        return result;
    } else {
        BUSY
    };
    unsafe { out.write(raw) };
    result
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_plugin_session_start(raw: *const Session) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    let Ok(mut phase) = session.phase.try_write() else {
        return BUSY;
    };
    match *phase {
        Phase::Started => return 0,
        Phase::Prepared => (),
        _ => return NOT_READY,
    }
    let args = rsf_game_start_args {
        struct_size: size_of::<rsf_game_start_args>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
    };
    let result = unsafe { rsf_plugin_call_start(&session.hooks, &args) };
    if result == 0 {
        *phase = Phase::Started;
    }
    result
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_plugin_session_quiesce(raw: *const Session) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    let Ok(mut phase) = session.phase.try_write() else {
        return BUSY;
    };
    if *phase == Phase::Quiesced {
        return 0;
    }
    // A refusal may leave some producers closed. Only another quiesce can
    // finish the transition. Do not tell a caller that this session started.
    *phase = Phase::Closed;
    let result = unsafe { rsf_plugin_call_control(session.hooks.quiesce, &CONTROL_ARGS) };
    if result == 0 {
        *phase = Phase::Quiesced;
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_plugin_session_status(
    raw: *const Session,
    out: *mut rsf_game_renderer_status,
) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if out.is_null()
        || (unsafe { out.cast::<u32>().read() } as usize) < size_of::<rsf_game_renderer_status>()
    {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*out).abi_version } != RSF_GAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Ok(_reader) = session.phase.try_read() else {
        return BUSY;
    };
    unsafe { rsf_plugin_call_status(&session.hooks, out) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_plugin_session_stop(raw: *mut Session) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return 0;
    };
    let Ok(phase) = session.phase.try_write() else {
        return BUSY;
    };
    if *phase != Phase::Quiesced {
        return BUSY;
    }
    let result = unsafe { rsf_plugin_call_control(session.hooks.stop, &CONTROL_ARGS) };
    if result == 0 {
        // The caller serializes final stop with any new uses of this handle.
        drop(phase);
        drop(unsafe { Box::from_raw(raw) });
    }
    result
}
