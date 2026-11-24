// SPDX-License-Identifier: MIT
use crate::sys::{
    RSF_FG_BACKEND_ALL as ALL, RSF_FG_BACKEND_AUTO as AUTO, RSF_FG_BACKEND_DLSS as DLSS,
    RSF_FG_BACKEND_FSR3 as FSR3, RSF_FG_BACKEND_XESS as XESS, rsf_fg_choice,
};
use rsf_game_sys::backend_result::{FEATURE_FAILED, NOT_SUPPORTED, OK};
use std::ptr::null;
use windows_sys::Win32::Storage::FileSystem::{DeleteFileW, GetTempFileNameW, GetTempPathW};
use windows_sys::Win32::System::WindowsProgramming::WritePrivateProfileStringW;
use windows_sys::core::w;

unsafe extern "C" {
    fn rsf_fg_choice_start(path: *const u16, fallback: u32, choices: u32) -> u32;
    fn rsf_fg_choice_save(backend: u32) -> i32;
    fn rsf_fg_choice_get() -> rsf_fg_choice;
    fn rsf_fg_choice_resolve_auto(backend: u32);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_fg_choice(default_backend: unsafe extern "C" fn(u32, u32) -> u32) -> i32 {
    let mut directory = [0_u16; 260];
    let mut path = [0_u16; 260];
    unsafe {
        let len = GetTempPathW(directory.len() as u32, directory.as_mut_ptr());
        if len == 0
            || len as usize >= directory.len()
            || GetTempFileNameW(directory.as_ptr(), w!("rsf"), 0, path.as_mut_ptr()) == 0
        {
            eprintln!("FAIL: could not create an FG choice fixture");
            return 1;
        }
        let path = path.as_ptr();
        let mut ok = rsf_fg_choice_start(path, DLSS, 0x3b) == DLSS;
        ok &= rsf_fg_choice_save(FSR3) == OK;
        let choice = rsf_fg_choice_get();
        ok &= choice.backend == FSR3 && choice.last_result == OK;
        ok &= rsf_fg_choice_start(path, DLSS, 0x3b) == FSR3;
        ok &= rsf_fg_choice_save(0) == OK;
        ok &= rsf_fg_choice_start(path, DLSS, 0x3b) == 0;
        ok &= rsf_fg_choice_save(2) == NOT_SUPPORTED;
        ok &= rsf_fg_choice_get().backend == 0;
        ok &= rsf_fg_choice_start(path, 0, 0x39) == 0;
        ok &= rsf_fg_choice_save(DLSS) == NOT_SUPPORTED;
        ok &= rsf_fg_choice_save(XESS) == OK;
        ok &= rsf_fg_choice_start(path, 0, 0x39) == XESS;
        ok &=
            WritePrivateProfileStringW(w!("Rendering"), w!("FrameGeneration"), w!("2"), path) != 0;
        ok &= rsf_fg_choice_start(path, FSR3, 0x39) == FSR3;
        ok &= (ALL | 1) == 0x3b;
        ok &= rsf_fg_choice_start(path, 9, ALL) == 0;
        rsf_fg_choice_start(null(), DLSS, 0x3b);
        ok &= rsf_fg_choice_save(FSR3) == FEATURE_FAILED;
        let choice = rsf_fg_choice_get();
        ok &= choice.backend == DLSS && choice.last_result == FEATURE_FAILED;
        DeleteFileW(path);

        ok &= default_backend(0x8086, 0) == XESS;
        ok &= default_backend(0x10de, 0) == DLSS;
        ok &= default_backend(0x1002, 0) == FSR3;
        ok &= default_backend(0x8086, 1) == 0 && default_backend(0xffff, 0) == 0;
        ok &= rsf_fg_choice_start(path, AUTO, ALL) == AUTO;
        rsf_fg_choice_resolve_auto(XESS);
        ok &= rsf_fg_choice_get().backend == XESS;
        ok &= rsf_fg_choice_start(path, AUTO, ALL) == AUTO;
        ok &= rsf_fg_choice_save(FSR3) == OK;
        ok &= rsf_fg_choice_start(path, AUTO, ALL) == FSR3;
        rsf_fg_choice_resolve_auto(XESS);
        ok &= rsf_fg_choice_get().backend == FSR3;
        DeleteFileW(path);
        if ok {
            println!(
                "PASS: saved/reloaded provider, Off, renderer mask, corrupt value and failed save"
            );
        } else {
            eprintln!("FAIL: generation provider persistence");
        }
        i32::from(!ok)
    }
}
