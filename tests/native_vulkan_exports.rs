// SPDX-License-Identifier: MIT
// Validate the shipped DLL in a separate process using the Windows loader.
use rsf_test_support::Library;
use std::ffi::{CStr, c_void};
use std::process::ExitCode;
use windows::Win32::System::LibraryLoader::LOAD_LIBRARY_FLAGS;

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

// No Vulkan handles are needed to check the production DLL's output-clearing rules.
fn split_refuses_null_calls(runtime: &Library) -> bool {
    type Plan = unsafe extern "C" fn(*const c_void, *mut u32, *mut u32) -> i32;
    type Destroy = unsafe extern "C" fn(*mut c_void) -> i32;
    let (Some(plan), Some(destroy)) = (
        runtime.symbol(c"rsf_srvk_fsr4_plan"),
        runtime.symbol(c"rsf_srvk_fsr4_destroy"),
    ) else {
        return false;
    };
    let plan: Plan = unsafe { std::mem::transmute(plan) };
    let destroy: Destroy = unsafe { std::mem::transmute(destroy) };
    let (mut width, mut height) = (9, 9);
    let result = unsafe { plan(std::ptr::null(), &mut width, &mut height) };
    let destroyed = unsafe { destroy(std::ptr::null_mut()) };
    (result, width, height, destroyed) == (-1, 0, 0, 0)
}

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next().filter(|_| args.next().is_none()) else {
        return ExitCode::from(2);
    };
    let runtime = match Library::open(&path, LOAD_LIBRARY_FLAGS(0)) {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Runtime load failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut result = ExitCode::SUCCESS;
    for name in EXPORTS {
        if runtime.symbol(name).is_none() {
            eprintln!("Missing native Vulkan export: {}", name.to_string_lossy());
            result = ExitCode::FAILURE;
        }
    }
    if !split_refuses_null_calls(&runtime) {
        eprintln!("Vulkan split SR DLL admission check failed");
        result = ExitCode::FAILURE;
    }
    result
}
