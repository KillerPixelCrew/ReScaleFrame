// SPDX-License-Identifier: MIT
use super::{PATH_CAPACITY, entry, load_library, module_path, null, null_mut, spawn_startup, wide};
use rsf_game_sys::ffi_util::ascii_wide;
use rsf_game_sys::session::carrier_log::CarrierLog;
use rsf_game_sys::win32::module_path_in;
use std::cell::UnsafeCell;
use std::ffi::{CString, OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::Foundation::{GetLastError, HMODULE};
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW,
};
use windows_sys::core::{BOOL, GUID, HRESULT};

const LOADER: [u16; 7] = ascii_wide(b"Loader\0");
const FLAGS: u32 = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;

fn profile(key: &str, capacity: usize, configuration: &[u16]) -> Option<Vec<u16>> {
    let mut value = vec![0; capacity + 1];
    let count = unsafe {
        GetPrivateProfileStringW(
            LOADER.as_ptr(),
            wide(key).as_ptr(),
            null(),
            value.as_mut_ptr(),
            value.len() as u32,
            configuration.as_ptr(),
        )
    } as usize;
    if count == 0 || count >= capacity {
        return None;
    }
    value.truncate(count);
    Some(value)
}

fn configure(module: HMODULE) -> Option<u32> {
    let path = module_path(module)?;
    let root = path.parent()?;
    let directory = root.join("ReScaleFrame");
    let configuration = wide(root.join("ReScaleFrame.ini"));
    let log = CarrierLog::new();
    log.set_path(&wide(directory.join("loader.log")));
    let note = |message: &str, result: u32| {
        log.write(unsafe { GetTickCount64() }, format!("{message}: {result}"))
    };
    if unsafe {
        GetPrivateProfileIntW(
            LOADER.as_ptr(),
            wide("AutoStart").as_ptr(),
            1,
            configuration.as_ptr(),
        )
    } == 0
    {
        return Some(0);
    }
    let (Some(adapter), Some(name)) = (
        profile("EntryModule", PATH_CAPACITY, &configuration),
        profile("EntryPoint", 128, &configuration),
    ) else {
        return Some(0);
    };
    let path = OsString::from_wide(&adapter);
    // The version shim keeps the installed profile's absolute-path support.
    let adapter =
        if adapter.len() > 1 && (adapter[1] == u16::from(b':') || adapter[0] == u16::from(b'\\')) {
            wide(path)
        } else {
            let mut relative = wide(root);
            relative.pop();
            if relative.last() != Some(&u16::from(b'\\')) {
                relative.push(u16::from(b'\\'));
            }
            relative.extend(adapter.into_iter().chain([0]));
            relative
        };

    if load_library(
        wide(directory.join("ReScaleFrame.Runtime.dll")).as_ptr(),
        FLAGS,
    )
    .is_null()
    {
        note("Runtime DLL load refused", unsafe { GetLastError() });
        return None;
    }
    let adapter = load_library(adapter.as_ptr(), FLAGS);
    if adapter.is_null() {
        note("Integration DLL load refused", unsafe { GetLastError() });
        return None;
    }
    let name = CString::new(String::from_utf16_lossy(&name)).ok()?;
    let Some(start) = entry(adapter, &name) else {
        note("Integration entry point missing", unsafe { GetLastError() });
        return None;
    };
    let result = unsafe { start(configuration.as_ptr()) };
    note("Integration startup result", result);
    // DLLs and registered callbacks remain mapped for process lifetime.
    Some(result)
}

unsafe extern "system" fn startup(module: *mut c_void) -> u32 {
    std::panic::catch_unwind(|| configure(module))
        .ok()
        .flatten()
        .unwrap_or(1)
}

// The loader lock serializes process attach. Large static storage keeps Unity's
// small dependency-loader thread stacks viable.
struct AttachPath(UnsafeCell<[u16; PATH_CAPACITY]>);
unsafe impl Sync for AttachPath {}
static EXECUTABLE: AttachPath = AttachPath(UnsafeCell::new([0; PATH_CAPACITY]));

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shim_attach(instance: HMODULE, event: u32) -> BOOL {
    if event != DLL_PROCESS_ATTACH {
        return 1;
    }
    rsf_game_sys::panic_record::install();
    let storage = unsafe { &mut *EXECUTABLE.0.get() };
    if let Some(path) = unsafe { module_path_in(null_mut(), storage) } {
        let leaf = path
            .rsplit(|&unit| unit == u16::from(b'\\'))
            .next()
            .unwrap_or(path);
        let matches =
            |name| unsafe { CompareStringOrdinal(leaf.as_ptr(), -1, name, -1, 1) == CSTR_EQUAL };
        // Unity's crash reporter imports this shim for version forwarding.
        const CRASH64: [u16; 24] = ascii_wide(b"UnityCrashHandler64.exe\0");
        const CRASH32: [u16; 24] = ascii_wide(b"UnityCrashHandler32.exe\0");
        if matches(CRASH64.as_ptr()) || matches(CRASH32.as_ptr()) {
            unsafe { DisableThreadLibraryCalls(instance) };
            return 1;
        }
    }
    spawn_startup(instance, Some(startup));
    1
}

/// The shim's DirectInput8Create (shim_exports.def). The shim starts from process attach, so the
/// export only forwards to the system DirectInput.
#[unsafe(no_mangle)]
unsafe extern "system" fn rsf_shim_create(
    instance: HMODULE,
    version: u32,
    iid: *const GUID,
    output: *mut *mut c_void,
    outer: *mut c_void,
) -> HRESULT {
    unsafe { super::thin::system_create(instance, version, iid, output, outer) }
}
