// SPDX-License-Identifier: MIT
use super::{Module, run};
use rsf_game_sys::RSF_OK;
use std::ffi::CStr;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::System::{
    Diagnostics::Debug::{IMAGE_DIRECTORY_ENTRY_IMPORT, ImageDirectoryEntryToData},
    LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleHandleExW, GetProcAddress,
    },
    SystemServices::{IMAGE_IMPORT_DESCRIPTOR, IMAGE_ORDINAL_FLAG64},
};

fn imports_on12(module: &Module) -> bool {
    let base = module.0.cast::<u8>();
    let mut bytes = 0;
    let directory = unsafe {
        ImageDirectoryEntryToData(base.cast(), true, IMAGE_DIRECTORY_ENTRY_IMPORT, &mut bytes)
    };
    if directory.is_null() {
        return false;
    }
    let descriptors = unsafe {
        std::slice::from_raw_parts(
            directory.cast::<IMAGE_IMPORT_DESCRIPTOR>(),
            bytes as usize / size_of::<IMAGE_IMPORT_DESCRIPTOR>(),
        )
    };
    for descriptor in descriptors
        .iter()
        .take_while(|descriptor| descriptor.Name != 0)
    {
        let rva = unsafe { descriptor.Anonymous.OriginalFirstThunk };
        if rva == 0 {
            continue;
        }
        let mut thunk = unsafe { base.add(rva as usize).cast::<u64>() };
        loop {
            let entry = unsafe { thunk.read() };
            if entry == 0 {
                break;
            }
            if entry & IMAGE_ORDINAL_FLAG64 == 0 {
                // An IMAGE_IMPORT_BY_NAME starts with its 16-bit hint.
                let name = unsafe { CStr::from_ptr(base.add(entry as usize + 2).cast()) };
                if name == c"D3D11On12CreateDevice" {
                    return true;
                }
            }
            thunk = unsafe { thunk.add(1) };
        }
    }
    false
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_orchestrator_domains() -> i32 {
    run(|| {
        let runtime = Module::beside_executable(Path::new("ReScaleFrame.Runtime.dll"));
        assert!(!imports_on12(&runtime), "D3D11-on-12 must resolve lazily");
        for name in [
            c"rsf_render_host_create",
            c"rsf_callback_host_start",
            c"rsf_render_settings_get",
        ] {
            assert!(
                unsafe { GetProcAddress(runtime.0, name.as_ptr().cast()) }.is_some(),
                "{name:?}"
            );
        }
        for name in [
            c"rsf_unity_sr_start",
            c"rsf_unity_fg_start",
            c"rsf_unity_sr_stop",
            c"rsf_rdr2_sr_host_create",
            c"rsf_rdr2_settings_get",
            c"rsf_rage_sr_host_create",
        ] {
            assert!(
                unsafe { GetProcAddress(runtime.0, name.as_ptr().cast()) }.is_none(),
                "{name:?}"
            );
        }
        let adapter = Module::beside_executable(Path::new("ReScaleFrame.Game.UnityMono.dll"));
        for name in [
            c"rsf_unity_start",
            c"rsf_unity_sr_start",
            c"rsf_unity_fg_start",
            c"rsf_unity_sr_stop",
        ] {
            let address = unsafe { GetProcAddress(adapter.0, name.as_ptr().cast()) }
                .unwrap_or_else(|| panic!("{name:?}"));
            let mut owner = null_mut();
            let result = unsafe {
                GetModuleHandleExW(
                    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                        | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    address as *const u16,
                    &mut owner,
                )
            };
            assert!(result != 0 && owner == adapter.0, "{name:?}");
        }
        RSF_OK
    })
}
