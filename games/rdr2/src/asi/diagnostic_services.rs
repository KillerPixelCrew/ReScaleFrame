// SPDX-License-Identifier: MIT
//! Adapt the ASI's linked diagnostic libraries to the RDR2 startup and sampling workers.
use super::services;
use crate::diagnostics::{
    CensusResult, DiagnosticsLeaves, DumpResult, path_units, rsf_rdr2_start_diagnostics,
    rsf_rdr2_start_frame_probe,
};
use crate::sys::*;
use std::ffi::{CString, c_char};
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Storage::FileSystem::CreateDirectoryA;

const SUCCESS: i32 = 0;
const INVALID_CENSUS_ARGUMENT: i32 = -1;
static CENSUS_INSTALLED: AtomicBool = AtomicBool::new(false);

pub(super) unsafe extern "C" fn install_frame_probe(
    library: *const u16,
    prefix: *const u16,
) -> i32 {
    let Some((initialise, trigger)) = services().frame_initialise.zip(services().frame_trigger)
    else {
        return -1;
    };
    unsafe { rsf_rdr2_start_frame_probe(library, prefix, initialise, trigger) }
}

pub(crate) unsafe extern "C" fn install_census(directory: *const u16, flags: u32) -> i32 {
    let Some(install) = services().census_install else {
        return INVALID_CENSUS_ARGUMENT;
    };
    let directory = String::from_utf16_lossy(unsafe { path_units(directory) });
    let directory = CString::new(directory).expect("terminated startup path has no inner NUL");
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: directory.as_ptr(),
        flags,
        ..Default::default()
    };
    // Census converts and copies the path during install.
    let result = unsafe { install(&options) };
    if result == SUCCESS {
        CENSUS_INSTALLED.store(true, Ordering::Release);
    }
    result
}

pub(crate) unsafe extern "C" fn start_diagnostics(modules: *const u16, dumps: *const u16) {
    const LEAVES: DiagnosticsLeaves = DiagnosticsLeaves {
        measure,
        dump,
        modules: module_list,
        census,
    };
    unsafe { rsf_rdr2_start_diagnostics(modules, dumps, &LEAVES) };
}

unsafe extern "C" fn measure() -> f64 {
    let mut entropy = 0.0;
    if let Some(measure) = services().measure {
        unsafe { measure(null(), &mut entropy) };
    }
    entropy
}

unsafe extern "C" fn dump(
    directory: *const c_char,
    threshold: f64,
    require_decrypted: u32,
    poll_ms: u32,
    timeout_ms: u32,
    stable_samples: u32,
    output: *mut DumpResult,
) {
    let Some(dump) = services().dump else { return };
    unsafe { CreateDirectoryA(directory.cast(), null()) };
    let options = rsf_dump_options {
        struct_size: size_of::<rsf_dump_options>() as u32,
        abi_version: RSF_MODULE_DUMP_ABI_VERSION,
        output_directory_utf8: directory,
        entropy_threshold: threshold,
        require_decrypted,
        ..Default::default()
    };
    let mut report = rsf_dump_report {
        struct_size: size_of::<rsf_dump_report>() as u32,
        ..Default::default()
    };
    let result = unsafe { dump(&options, poll_ms, timeout_ms, stable_samples, &mut report) };
    unsafe {
        output.write(DumpResult {
            result,
            sections: report.sections_written,
            imports: report.imports_described,
            iat_refs: report.iat_references,
            entropy: report.code_entropy,
            base: report.load_base,
            bytes: report.bytes_written,
        });
    }
}

unsafe extern "C" fn module_list(path: *const c_char, label: *const c_char) {
    if let Some(modules) = services().modules {
        unsafe { modules(path, label) };
    }
}

unsafe extern "C" fn census(output: *mut CensusResult) -> bool {
    let Some((flush, counters)) = services().census_flush.zip(services().census_counters) else {
        return false;
    };
    if !CENSUS_INSTALLED.load(Ordering::Acquire) || unsafe { flush() } != SUCCESS {
        return false;
    }
    let mut report = rsf_census_counters {
        struct_size: size_of::<rsf_census_counters>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        ..Default::default()
    };
    if unsafe { counters(&mut report) } != SUCCESS {
        return false;
    }
    unsafe {
        output.write(CensusResult {
            calls: report.calls_seen,
            written: report.records_written,
            presents: report.present_index,
            dropped: report.records_dropped_queue
                + report.records_dropped_budget
                + report.records_dropped_size,
            sites: report.sites,
            hooks: report.hooks_installed,
            conflicts: report.hook_conflicts,
        });
    }
    true
}
