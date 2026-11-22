// SPDX-License-Identifier: MIT
use super::{load_library, load_system, null, null_mut};
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_void};
use windows_sys::Win32::Foundation::{
    ERROR_PROC_NOT_FOUND, FARPROC, HMODULE, STATUS_ILLEGAL_INSTRUCTION, SetLastError,
};
use windows_sys::Win32::System::Diagnostics::Debug::RaiseException;
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
use windows_sys::Win32::System::SystemServices::EXCEPTION_NONCONTINUABLE;
use windows_sys::Win32::System::Threading::{
    INIT_ONCE, INIT_ONCE_STATIC_INIT, InitOnceExecuteOnce, PINIT_ONCE_FN,
};
use windows_sys::core::BOOL;

pub(super) struct InitOnce(UnsafeCell<INIT_ONCE>);

// Windows synchronizes every access to the wrapped INIT_ONCE.
unsafe impl Sync for InitOnce {}

impl InitOnce {
    pub const fn new() -> Self {
        Self(UnsafeCell::new(INIT_ONCE_STATIC_INIT))
    }

    pub fn run(&self, init: PINIT_ONCE_FN) -> HMODULE {
        let mut module = null_mut();
        if unsafe { InitOnceExecuteOnce(self.0.get(), init, null_mut(), &mut module) } == 0 {
            return null_mut();
        }
        module
    }
}

static VERSION: InitOnce = InitOnce::new();
static XINPUT: InitOnce = InitOnce::new();

unsafe extern "system" fn version_init(
    _: *mut INIT_ONCE,
    _: *mut c_void,
    context: *mut *mut c_void,
) -> BOOL {
    let module = load_system("version.dll", load_library);
    // A module handle preserves the low reserved context bits as zero.
    unsafe { context.write(module) };
    i32::from(!module.is_null())
}

unsafe extern "system" fn xinput_init(
    _: *mut INIT_ONCE,
    _: *mut c_void,
    context: *mut *mut c_void,
) -> BOOL {
    let module = load_system("xinput1_3.dll", load_library);
    unsafe { context.write(module) };
    i32::from(!module.is_null())
}

fn required_export(module: HMODULE, name_or_ordinal: *const u8) -> FARPROC {
    if !module.is_null()
        && let found @ Some(_) = unsafe { GetProcAddress(module, name_or_ordinal) }
    {
        return found;
    }
    unsafe {
        SetLastError(ERROR_PROC_NOT_FOUND);
        RaiseException(
            STATUS_ILLEGAL_INSTRUCTION as u32,
            EXCEPTION_NONCONTINUABLE,
            0,
            null(),
        );
    }
    None
}

#[unsafe(no_mangle)]
extern "C" fn rsf_version_resolve(index: u32) -> FARPROC {
    const NAMES: [&CStr; 17] = [
        c"GetFileVersionInfoA",
        c"GetFileVersionInfoByHandle",
        c"GetFileVersionInfoExA",
        c"GetFileVersionInfoExW",
        c"GetFileVersionInfoSizeA",
        c"GetFileVersionInfoSizeExA",
        c"GetFileVersionInfoSizeExW",
        c"GetFileVersionInfoSizeW",
        c"GetFileVersionInfoW",
        c"VerFindFileA",
        c"VerFindFileW",
        c"VerInstallFileA",
        c"VerInstallFileW",
        c"VerLanguageNameA",
        c"VerLanguageNameW",
        c"VerQueryValueA",
        c"VerQueryValueW",
    ];
    let Some(name) = NAMES.get(index as usize) else {
        return required_export(null_mut(), null());
    };
    required_export(VERSION.run(Some(version_init)), name.as_ptr().cast())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xinput_resolve(index: u32) -> FARPROC {
    const ORDINALS: [u16; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 100, 101, 102, 103];
    let Some(&ordinal) = ORDINALS.get(index as usize) else {
        return required_export(null_mut(), null());
    };
    required_export(
        XINPUT.run(Some(xinput_init)),
        usize::from(ordinal) as *const u8,
    )
}
