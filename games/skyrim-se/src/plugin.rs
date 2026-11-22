// SPDX-License-Identifier: MIT
//! GameSDK identity and the exact-build admission of the native observers.
use crate::layout::{END, JITTER, PROCESS, PROCESS_CALL, RESET, RESOLUTION, UPLOAD, VIEWPORT};
use crate::platform;
use crate::session::{self, Leaves};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detection, rsf_game_hooks_api, rsf_game_info, rsf_game_plugin_api,
    rsf_game_probe, rsf_result,
};
use std::ffi::{CStr, c_char, c_void};
use std::slice;

pub(crate) const INVALID_ARGUMENT: rsf_result = -1;
pub(crate) const ABI_MISMATCH: rsf_result = -2;
const NOT_READY: rsf_result = -3;
const AMD64: u16 = 0x8664;
const EXECUTABLE: &[u8] = b"SkyrimSE.exe";
pub(crate) const SHA256: &CStr =
    c"846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const STATUS: &CStr =
    c"Experimental native-resolution SR; reduced-resolution SR, FG and latency remain incomplete.";

#[repr(C)]
struct ObserverTargets {
    jitter: *mut c_void,
    process: *mut c_void,
    resolution: *mut c_void,
    end: *mut c_void,
    upload: *mut c_void,
}

const _: () = assert!(size_of::<ObserverTargets>() == 40);

type Guard = unsafe extern "C" fn() -> i32;

fn executable(path: &[u16]) -> bool {
    let leaf = path.rsplit(|&unit| unit == u16::from(b'\\')).next();
    leaf.is_some_and(|leaf| {
        leaf.len() == EXECUTABLE.len()
            && leaf.iter().zip(EXECUTABLE).all(|(&unit, byte)| {
                u8::try_from(unit).is_ok_and(|unit| unit.eq_ignore_ascii_case(byte))
            })
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_executable_sha256() -> *const c_char {
    SHA256.as_ptr()
}

/// The supplied path is readable for `length` units, the callbacks are process-lived, and
/// `out` is null or writable for one record. The public lifecycle serializes admission.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_observer_admit(
    path: *const u16,
    length: usize,
    sampling: Guard,
    presentation_inputs: Guard,
    out: *mut ObserverTargets,
) -> i32 {
    if path.is_null() || out.is_null() || length == 0 || length >= 32_768 {
        return 0;
    }
    if !executable(unsafe { slice::from_raw_parts(path, length) }) {
        return 0;
    }
    let Some(platform) = platform::get() else {
        return 0;
    };
    let base = platform.main_module();
    if base == 0
        || platform
            .module_sha256(base)
            .as_ref()
            .map(|digest| digest.as_slice())
            != Some(SHA256.to_bytes())
        || !platform.matches(base.wrapping_add(JITTER.rva), JITTER.bytes)
    {
        return 0;
    }
    for guard in [sampling, presentation_inputs] {
        let result = unsafe { guard() };
        if result != 1 {
            return result;
        }
    }
    if ![
        PROCESS,
        PROCESS_CALL,
        RESET,
        RESOLUTION,
        END,
        VIEWPORT,
        UPLOAD,
    ]
    .into_iter()
    .all(|site| platform.matches(base.wrapping_add(site.rva), site.bytes))
    {
        return 0;
    }
    unsafe {
        out.write(ObserverTargets {
            jitter: base.wrapping_add(JITTER.rva) as *mut c_void,
            process: base.wrapping_add(PROCESS.rva) as *mut c_void,
            resolution: base.wrapping_add(RESOLUTION.rva) as *mut c_void,
            end: base.wrapping_add(END.rva) as *mut c_void,
            upload: base.wrapping_add(UPLOAD.rva) as *mut c_void,
        })
    };
    1
}

unsafe fn matches_ascii(string: *const c_char, expected: &[u8]) -> bool {
    !string.is_null()
        && unsafe { CStr::from_ptr(string) }
            .to_bytes()
            .eq_ignore_ascii_case(expected)
}

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    if probe.is_null()
        || (unsafe { probe.cast::<u32>().read() } as usize) < size_of::<rsf_game_probe>()
    {
        return 0;
    }
    let probe = unsafe { &*probe };
    if probe.pe_machine == AMD64
        && unsafe { matches_ascii(probe.executable_name_utf8, EXECUTABLE) }
        && unsafe { matches_ascii(probe.sha256_hex, SHA256.to_bytes()) }
    {
        1
    } else {
        0
    }
}

/// Publishes the GameSDK table backed by the Rust session. A short output may contain only size.
///
/// # Safety
/// `out` is null or a caller-owned table readable for its size prefix. `leaves` is null or a
/// table that, when accepted, lives for the process.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_publish_api(
    requested: u32,
    out: *mut rsf_game_plugin_api,
    leaves: *const Leaves,
) -> rsf_result {
    if out.is_null()
        || (unsafe { out.cast::<u32>().read() } as usize) < size_of::<rsf_game_plugin_api>()
    {
        return INVALID_ARGUMENT;
    }
    if requested != RSF_GAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if !unsafe { session::publish(leaves) } {
        return NOT_READY;
    }
    let hooks = rsf_game_hooks_api {
        prepare: Some(session::prepare),
        start: Some(session::start),
        quiesce: Some(session::quiesce),
        stop: Some(session::stop),
        status: Some(session::status),
    };
    unsafe {
        out.write(rsf_game_plugin_api {
            struct_size: size_of::<rsf_game_plugin_api>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            info: rsf_game_info {
                id: c"skyrim-se".as_ptr(),
                name: c"Skyrim Special Edition".as_ptr(),
                version: VERSION.as_ptr().cast(),
                rendering_ready: 0,
                status: STATUS.as_ptr(),
            },
            detect: Some(detect),
            hooks,
        })
    };
    0
}
