// SPDX-License-Identifier: MIT
// Validate the shipped DLL in a separate process using the Windows loader.
use std::ffi::{CStr, c_char, c_void};
use std::os::windows::ffi::OsStrExt;
use std::process::ExitCode;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(path: *const u16) -> *mut c_void;
    fn GetProcAddress(
        module: *mut c_void,
        name: *const c_char,
    ) -> Option<unsafe extern "system" fn() -> isize>;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

const EXPORTS: &[&CStr] = &[
    c"rsf_get_runtime_version",
    c"rsf_slvk_initialize",
    c"rsf_latencyvk_instance_proc",
    c"rsf_latencyvk_options",
    c"rsf_latencyvk_cpu",
    c"rsf_latencyvk_render",
    c"rsf_latencyvk_present",
    c"rsf_latencyvk_execution",
    c"rsf_latencyvk_shutdown",
    c"rsf_srvk_create",
    c"rsf_srvk_plan",
    c"rsf_srvk_evaluate",
    c"rsf_srvk_retire_commands",
    c"rsf_srvk_destroy",
    c"rsf_srvk_fsr4_create",
    c"rsf_srvk_fsr4_plan",
    c"rsf_srvk_fsr4_evaluate",
    c"rsf_srvk_fsr4_info",
    c"rsf_srvk_fsr4_submitted",
    c"rsf_srvk_fsr4_retire_commands",
    c"rsf_srvk_fsr4_destroy",
    c"rsf_fgvk_create_with_host",
    c"rsf_fgvk_images",
    c"rsf_fgvk_acquire",
    c"rsf_fgvk_enable_options",
    c"rsf_fgvk_prepare",
    c"rsf_fgvk_hudless",
    c"rsf_fgvk_submitted",
    c"rsf_fgvk_present",
    c"rsf_fgvk_destroy",
    c"rsf_fgvk_present_source",
];

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next().filter(|_| args.next().is_none()) else {
        return ExitCode::from(2);
    };
    let wide: Vec<u16> = path.encode_wide().chain([0]).collect();
    let runtime = unsafe { LoadLibraryW(wide.as_ptr()) };
    if runtime.is_null() {
        eprintln!("Runtime load failed: {}", std::io::Error::last_os_error());
        return ExitCode::FAILURE;
    }

    let mut result = ExitCode::SUCCESS;
    for name in EXPORTS {
        if unsafe { GetProcAddress(runtime, name.as_ptr()) }.is_none() {
            eprintln!("Missing native Vulkan export: {}", name.to_string_lossy());
            result = ExitCode::FAILURE;
        }
    }
    unsafe { FreeLibrary(runtime) };
    result
}
