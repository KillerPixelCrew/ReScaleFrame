// SPDX-License-Identifier: MIT
use super::{address, fail, load, named, null_mut, ordinal, require, system_path};
use std::ffi::c_void;
use std::ptr::fn_addr_eq;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MOD_NOT_FOUND, GetLastError};
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XINPUT_VIBRATION};
use windows_sys::core::BOOL;

const LOAD_FLAGS: u32 = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;

#[unsafe(no_mangle)]
extern "C" fn rsf_test_shared_shim_exports() -> i32 {
    let args: Vec<_> = std::env::args_os().collect();
    let [_, path] = args.as_slice() else {
        return 2;
    };
    let shim = load(path, LOAD_FLAGS);
    require(!shim.is_null(), "load shared shim");
    require(
        ordinal(shim, 1).is_some_and(|proc| fn_addr_eq(proc, address(shim, c"DirectInput8Create"))),
        "DirectInput8Create has ordinal 1",
    );
    for name in [
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
    ] {
        address(shim, name);
    }
    let system_version = system_path("version.dll");
    let system_kernel = system_path("kernel32.dll");
    let original = unsafe {
        LoadLibraryExW(
            system_version.as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    require(!original.is_null(), "load real System32 version DLL");
    type Size = unsafe extern "system" fn(*const u16, *mut u32) -> u32;
    type Info = unsafe extern "system" fn(*const u16, u32, u32, *mut c_void) -> BOOL;
    type Query =
        unsafe extern "system" fn(*const c_void, *const u16, *mut *mut c_void, *mut u32) -> BOOL;
    let size = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, Size>(address(
            shim,
            c"GetFileVersionInfoSizeW",
        ))
    };
    let original_size = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, Size>(address(
            original,
            c"GetFileVersionInfoSizeW",
        ))
    };
    let info = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, Info>(address(
            shim,
            c"GetFileVersionInfoW",
        ))
    };
    let query = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, Query>(address(
            shim,
            c"VerQueryValueW",
        ))
    };
    let mut ignored = 0;
    let count = unsafe { size(system_kernel.as_ptr(), &mut ignored) };
    require(
        count != 0 && count == unsafe { original_size(system_kernel.as_ptr(), &mut ignored) },
        "forward version resource size",
    );
    let mut data = vec![0u8; count as usize];
    require(
        unsafe { info(system_kernel.as_ptr(), 0, count, data.as_mut_ptr().cast()) } != 0,
        "forward version info",
    );
    let mut value = null_mut();
    let mut bytes = 0;
    require(
        unsafe {
            query(
                data.as_ptr().cast(),
                [u16::from(b'\\'), 0].as_ptr(),
                &mut value,
                &mut bytes,
            )
        } != 0
            && !value.is_null()
            && bytes >= 52,
        "forward version resource root lookup",
    );
    // Startup may still be running. Keep the carrier loaded.
    println!(
        "PASS: DirectInput ordinal 1, named version exports and real version-resource forwarding."
    );
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_xinput_carrier() -> i32 {
    let args: Vec<_> = std::env::args_os().collect();
    let [_, path] = args.as_slice() else {
        return 2;
    };
    let carrier = load(path, LOAD_FLAGS);
    require(!carrier.is_null(), "load XInput carrier");
    for (index, name) in [
        c"DllMain",
        c"XInputGetState",
        c"XInputSetState",
        c"XInputGetCapabilities",
        c"XInputEnable",
        c"XInputGetDSoundAudioDeviceGuids",
        c"XInputGetBatteryInformation",
        c"XInputGetKeystroke",
    ]
    .into_iter()
    .enumerate()
    {
        require(
            named(carrier, name)
                .zip(ordinal(carrier, index as u16 + 1))
                .is_some_and(|(named, ordinal)| fn_addr_eq(named, ordinal)),
            "matching named and ordinal XInput export",
        );
    }
    for number in 100..104 {
        require(
            ordinal(carrier, number).is_some(),
            "unnamed compatibility ordinal",
        );
    }

    let system = system_path("xinput1_3.dll");
    let original =
        unsafe { LoadLibraryExW(system.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if original.is_null() {
        let error = unsafe { GetLastError() };
        eprintln!("System XInput 1.3 unavailable: Win32 error {error}; carrier exports checked.");
        return if matches!(error, ERROR_MOD_NOT_FOUND | ERROR_FILE_NOT_FOUND) {
            77
        } else {
            1
        };
    }
    type GetState = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;
    type SetState = unsafe extern "system" fn(u32, *mut XINPUT_VIBRATION) -> u32;
    let (Some(state), Some(native_state), Some(vibration), Some(native_vibration)) = (
        ordinal(carrier, 2),
        ordinal(original, 2),
        ordinal(carrier, 3),
        ordinal(original, 3),
    ) else {
        fail("resolve carrier and system state and vibration exports");
    };
    let state =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, GetState>(state) };
    let native_state = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, GetState>(native_state)
    };
    let vibration =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, SetState>(vibration) };
    let native_vibration = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, SetState>(native_vibration)
    };
    let mut input = XINPUT_STATE::default();
    let mut output = XINPUT_VIBRATION::default();
    // An invalid user index verifies native forwarding without touching a controller.
    require(
        unsafe {
            state(u32::MAX, &mut input) == native_state(u32::MAX, &mut input)
                && vibration(u32::MAX, &mut output) == native_vibration(u32::MAX, &mut output)
        },
        "state and vibration match native XInput behavior",
    );
    println!("PASS: XInput names/ordinals and native state/vibration forwarding.");
    0
}
