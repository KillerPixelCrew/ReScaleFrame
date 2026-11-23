// SPDX-License-Identifier: MIT
use super::config::{BAD_SIZE, MAX_SIZE, parse};
use super::forwarding::InitOnce;
use super::{entry, load_library, load_system, module_path, null, null_mut};
use std::ffi::{CStr, CString, OsString, c_void};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::Foundation::{E_FAIL, FARPROC, GetLastError, HMODULE, SetLastError};
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY, FILE_NAME_NORMALIZED,
    FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, GetFinalPathNameByHandleW,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    GetThreadErrorMode, OutputDebugStringA, SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX,
    SetThreadErrorMode,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW, GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use windows_sys::Win32::System::Threading::{GetCurrentThreadId, INIT_ONCE};
use windows_sys::core::{BOOL, GUID, HRESULT};

static STARTUP: InitOnce = InitOnce::new();
static STARTING_THREAD: AtomicU32 = AtomicU32::new(0);
static SYSTEM: InitOnce = InitOnce::new();

fn quiet_load(path: *const u16, flags: u32) -> HMODULE {
    unsafe {
        let mut previous = 0;
        if SetThreadErrorMode(
            GetThreadErrorMode() | SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX,
            &mut previous,
        ) == 0
        {
            return null_mut();
        }
        let module = load_library(path, flags);
        let error = GetLastError();
        if SetThreadErrorMode(previous, null_mut()) == 0 {
            // A loaded DLL may already own process state. Keep it mapped even on refusal.
            return null_mut();
        }
        SetLastError(error);
        module
    }
}

fn note(root: &Path, message: &str) {
    let message = format!("loader: {message}\r\n");
    if let Ok(debug) = CString::new(message.as_str()) {
        unsafe { OutputDebugStringA(debug.as_ptr().cast()) };
    }
    if let Ok(mut file) = OpenOptions::new()
        .append(true)
        .create(true)
        .access_mode(FILE_APPEND_DATA)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(root.join("ReScaleFrame.Loader.log"))
    {
        let _ = file.write_all(message.as_bytes());
    }
}

struct CheckedFile {
    file: File,
    path: Vec<u16>,
    identity: (u32, u32, u32),
}

impl CheckedFile {
    fn open(name: &Path) -> Option<Self> {
        // Hold the read-only share through startup so checked files cannot be replaced.
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(name)
            .ok()?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
        {
            return None;
        }
        let mut path = vec![0; super::PATH_CAPACITY];
        let count = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                path.as_mut_ptr(),
                path.len() as u32,
                FILE_NAME_NORMALIZED,
            )
        } as usize;
        if count == 0 || count >= path.len() {
            return None;
        }
        path.truncate(count + 1);
        let identity = (
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        );
        Some(Self {
            file,
            path,
            identity,
        })
    }

    fn inside(&self, root: &[u16]) -> bool {
        let count = root.len() as i32;
        self.path.len() > root.len() + 1
            && unsafe { CompareStringOrdinal(self.path.as_ptr(), count, root.as_ptr(), count, 1) }
                == CSTR_EQUAL
    }
}

fn relative_module(name: &str) -> bool {
    !name
        .bytes()
        .any(|c| c < 32 || matches!(c, 127 | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'))
        && name
            .split(['/', '\\'])
            .all(|part| !part.is_empty() && !part.ends_with(['.', ' ']))
}

fn read_config(file: &File) -> Result<super::config::Config, &'static str> {
    let size = file.metadata().map_err(|_| BAD_SIZE)?.len();
    if size == 0 || size > MAX_SIZE {
        return Err(BAD_SIZE);
    }
    let mut bytes = vec![0; size as usize];
    let mut file = file;
    file.read_exact(&mut bytes)
        .map_err(|_| "configuration read failed")?;
    parse(&bytes)
}

fn activate(carrier: &CheckedFile, root_path: &Path, root: &[u16]) -> Result<String, String> {
    let config = CheckedFile::open(&root_path.join("ReScaleFrame.ini"))
        .filter(|file| file.inside(root))
        .ok_or("configuration missing or outside carrier directory")?;
    let settings = read_config(&config.file)?;
    if !settings.auto_start {
        return Ok("AutoStart disabled".into());
    }
    if !relative_module(&settings.runtime_module) || !relative_module(&settings.entry_module) {
        return Err("module path refused (must be contained relative paths)".into());
    }

    let checked = |name: &str| {
        CheckedFile::open(&root_path.join(name.replace('/', "\\")))
            .filter(|file| file.inside(root) && file.identity != carrier.identity)
            .ok_or("module missing, escaped carrier directory, or aliases another module")
    };
    let runtime = checked(&settings.runtime_module)?;
    let adapter = checked(&settings.entry_module)?;
    if runtime.identity == adapter.identity {
        return Err("module missing, escaped carrier directory, or aliases another module".into());
    }
    const FLAGS: u32 = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
    if quiet_load(runtime.path.as_ptr(), FLAGS).is_null() {
        return Err(format!("runtime load failed win32={}", unsafe {
            GetLastError()
        }));
    }
    let adapter_module = quiet_load(adapter.path.as_ptr(), FLAGS);
    if adapter_module.is_null() {
        return Err(format!("adapter load failed win32={}", unsafe {
            GetLastError()
        }));
    }
    // Loaded module references remain live even when the entry refuses activation.
    let start = entry(adapter_module, &settings.entry_point).ok_or("entry export missing")?;
    let result = unsafe { start(config.path.as_ptr()) };
    let status = if result == 0 { "ready" } else { "refused" };
    Ok(format!("entry {status} status={result}"))
}

fn startup() {
    let mut module = null_mut();
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            startup as *const () as *const u16,
            &mut module,
        )
    } == 0
    {
        return;
    }
    let Some(carrier) = module_path(module).and_then(|path| CheckedFile::open(&path)) else {
        return;
    };
    let Some(separator) = carrier
        .path
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\'))
    else {
        return;
    };
    let root = &carrier.path[..=separator];
    let root_path = PathBuf::from(OsString::from_wide(root));
    let message = activate(&carrier, &root_path, root).unwrap_or_else(|message| message);
    note(&root_path, &message);
}

unsafe extern "system" fn start_once(
    _: *mut INIT_ONCE,
    _: *mut c_void,
    _: *mut *mut c_void,
) -> BOOL {
    STARTING_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    if std::panic::catch_unwind(startup).is_err() {
        unsafe {
            OutputDebugStringA(
                c"loader: startup panic; system forwarding preserved\r\n"
                    .as_ptr()
                    .cast(),
            )
        };
    }
    STARTING_THREAD.store(0, Ordering::Relaxed);
    // A refusal is final for this carrier mapping.
    1
}

unsafe extern "system" fn system_once(
    _: *mut INIT_ONCE,
    _: *mut c_void,
    context: *mut *mut c_void,
) -> BOOL {
    unsafe { context.write(load_system("dinput8.dll", quiet_load)) };
    1
}

fn system_proc(name: &CStr) -> FARPROC {
    let module = SYSTEM.run(Some(system_once));
    if module.is_null() {
        None
    } else {
        unsafe { GetProcAddress(module, name.as_ptr().cast()) }
    }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn rsf_thin_create(
    instance: HMODULE,
    version: u32,
    iid: *const GUID,
    output: *mut *mut c_void,
    outer: *mut c_void,
) -> HRESULT {
    // A same-thread create from the adapter entry must skip the in-progress startup.
    if STARTING_THREAD.load(Ordering::Relaxed) != unsafe { GetCurrentThreadId() } {
        STARTUP.run(Some(start_once));
    }
    unsafe { system_create(instance, version, iid, output, outer) }
}

/// The system DirectInput8Create, which both DirectInput carriers forward to.
pub(super) unsafe fn system_create(
    instance: HMODULE,
    version: u32,
    iid: *const GUID,
    output: *mut *mut c_void,
    outer: *mut c_void,
) -> HRESULT {
    type Create = unsafe extern "system" fn(
        HMODULE,
        u32,
        *const GUID,
        *mut *mut c_void,
        *mut c_void,
    ) -> HRESULT;
    system_proc(c"DirectInput8Create").map_or(E_FAIL, |address| unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, Create>(address)(
            instance, version, iid, output, outer,
        )
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn rsf_thin_get_class_object(
    clsid: *const GUID,
    iid: *const GUID,
    output: *mut *mut c_void,
) -> HRESULT {
    type GetClass =
        unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    system_proc(c"DllGetClassObject").map_or(E_FAIL, |address| unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, GetClass>(address)(
            clsid, iid, output,
        )
    })
}

fn forward_status(name: &CStr) -> HRESULT {
    type Status = unsafe extern "system" fn() -> HRESULT;
    system_proc(name).map_or(E_FAIL, |address| unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, Status>(address)()
    })
}

#[unsafe(no_mangle)]
extern "system" fn rsf_thin_can_unload() -> HRESULT {
    forward_status(c"DllCanUnloadNow")
}

#[unsafe(no_mangle)]
extern "system" fn rsf_thin_register_server() -> HRESULT {
    forward_status(c"DllRegisterServer")
}

#[unsafe(no_mangle)]
extern "system" fn rsf_thin_unregister_server() -> HRESULT {
    forward_status(c"DllUnregisterServer")
}

#[unsafe(no_mangle)]
extern "system" fn rsf_thin_joystick_format() -> *const c_void {
    type Format = unsafe extern "system" fn() -> *const c_void;
    system_proc(c"GetdfDIJoystick").map_or(null(), |address| unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, Format>(address)()
    })
}
