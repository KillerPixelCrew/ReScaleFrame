// SPDX-License-Identifier: MIT
use rsf_game_sys::*;
use std::ffi::c_void;
use std::ptr;
use std::sync::RwLock;

const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;

#[repr(C)]
struct Options {
    size: u32,
    version: u32,
    path: *const u16,
    probe: *const rsf_game_probe,
    services: rsf_game_host_services,
}

struct Module(*mut c_void);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> rsf_get_game_plugin_api_fn;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

type Control = Option<unsafe extern "C" fn(*const rsf_game_control_args) -> i32>;
unsafe extern "C" {
    fn rsf_plugin_call_query(
        entry: rsf_get_game_plugin_api_fn,
        api: *mut rsf_game_plugin_api,
    ) -> i32;
    fn rsf_plugin_call_detect(detect: rsf_detect_game_fn, probe: *const rsf_game_probe) -> i32;
    fn rsf_plugin_call_prepare(
        hooks: *const rsf_game_hooks_api,
        services: *const rsf_game_host_services,
    ) -> i32;
    fn rsf_plugin_call_start(hooks: *const rsf_game_hooks_api) -> i32;
    fn rsf_plugin_call_control(callback: Control) -> i32;
    fn rsf_plugin_call_status(
        hooks: *const rsf_game_hooks_api,
        out: *mut rsf_game_renderer_status,
    ) -> i32;
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Partial,
    Prepared,
    Started,
    Quiesced,
}

struct Session {
    _module: Module,
    hooks: rsf_game_hooks_api,
    services: rsf_game_host_services,
    phase: RwLock<Phase>,
}

// The caller supplies a terminated UTF-16 path. Only inspect its prefix.
unsafe fn absolute(path: *const u16) -> bool {
    if path.is_null() || unsafe { *path } == 0 {
        return false;
    }
    let second = unsafe { *path.add(1) };
    (second == u16::from(b':') && matches!(unsafe { *path.add(2) }, 47 | 92))
        || (unsafe { *path } == u16::from(b'\\') && second == u16::from(b'\\'))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_plugin_session_prepare(
    options: *const Options,
    out: *mut *mut Session,
) -> i32 {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    if options.is_null()
        || (unsafe { options.cast::<u32>().read() } as usize) < size_of::<Options>()
    {
        return INVALID_ARGUMENT;
    }
    let options = unsafe { &*options };
    if !unsafe { absolute(options.path) }
        || options.probe.is_null()
        || (unsafe { options.probe.cast::<u32>().read() } as usize) < size_of::<rsf_game_probe>()
        || (options.services.struct_size as usize) < size_of::<rsf_game_host_services>()
        || options.services.session_id == 0
    {
        return INVALID_ARGUMENT;
    }
    if options.version != 1 || options.services.abi_version != RSF_GAME_ABI_VERSION {
        return ABI_MISMATCH;
    }

    // LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS
    let handle = unsafe { LoadLibraryExW(options.path, ptr::null_mut(), 0x100 | 0x1000) };
    if handle.is_null() {
        return NOT_READY;
    }
    let module = Module(handle);
    let entry = unsafe { GetProcAddress(handle, RSF_GAME_ENTRY_POINT.as_ptr()) };
    if entry.is_none() {
        return ABI_MISMATCH;
    }
    let mut api: rsf_game_plugin_api = unsafe { std::mem::zeroed() };
    api.struct_size = size_of_val(&api) as u32;
    let result = unsafe { rsf_plugin_call_query(entry, &mut api) };
    if result != 0 {
        return result;
    }
    if (api.struct_size as usize) < size_of_val(&api) || api.abi_version != RSF_GAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let result = unsafe { rsf_plugin_call_detect(api.detect, options.probe) };
    if result != 0 {
        return result;
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
        phase: RwLock::new(Phase::Partial),
    });
    if raw.is_null() {
        return NOT_READY;
    }
    // Keep the allocation at the same address throughout the plugin's lease.
    let session = unsafe { &*raw };
    let result = unsafe { rsf_plugin_call_prepare(&session.hooks, &session.services) };
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
unsafe extern "C" fn rsf_plugin_session_start(raw: *const Session) -> i32 {
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
    let result = unsafe { rsf_plugin_call_start(&session.hooks) };
    if result == 0 {
        *phase = Phase::Started;
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_plugin_session_quiesce(raw: *const Session) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    let Ok(mut phase) = session.phase.try_write() else {
        return BUSY;
    };
    if *phase == Phase::Quiesced {
        return 0;
    }
    let result = unsafe { rsf_plugin_call_control(session.hooks.quiesce) };
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
unsafe extern "C" fn rsf_plugin_session_stop(raw: *mut Session) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return 0;
    };
    let Ok(phase) = session.phase.try_write() else {
        return BUSY;
    };
    if *phase != Phase::Quiesced {
        return BUSY;
    }
    let result = unsafe { rsf_plugin_call_control(session.hooks.stop) };
    if result == 0 {
        // The caller serializes final stop with any new uses of this handle.
        drop(phase);
        drop(unsafe { Box::from_raw(raw) });
    }
    result
}
