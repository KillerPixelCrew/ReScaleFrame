// SPDX-License-Identifier: MIT
//! GameSDK identity and the exact-build admission of the native observers.
use crate::layout::{END, JITTER, PROCESS, PROCESS_CALL, RESET, RESOLUTION, UPLOAD, VIEWPORT};
use crate::platform;
use crate::session;
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detection, rsf_game_hooks_api, rsf_game_info, rsf_game_plugin_api,
    rsf_game_probe, rsf_result,
};
use std::ffi::{CStr, c_char};

pub(crate) const INVALID_ARGUMENT: rsf_result = -1;
pub(crate) const ABI_MISMATCH: rsf_result = -2;
const AMD64: u16 = 0x8664;
const EXECUTABLE: &[u8] = b"SkyrimSE.exe";
pub(crate) const SHA256: &CStr =
    c"846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f";
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const STATUS: &CStr =
    c"Experimental native-resolution SR; reduced-resolution SR, FG and latency remain incomplete.";

fn executable(path: &[u16]) -> bool {
    let leaf = path.rsplit(|&unit| unit == u16::from(b'\\')).next();
    leaf.is_some_and(|leaf| {
        leaf.len() == EXECUTABLE.len()
            && leaf.iter().zip(EXECUTABLE).all(|(&unit, byte)| {
                u8::try_from(unit).is_ok_and(|unit| unit.eq_ignore_ascii_case(byte))
            })
    })
}

/// The public lifecycle serializes observer admission and installation.
pub(crate) fn admit_observers() -> i32 {
    let mut path = [0u16; 32_768];
    let length = platform::module_path(0, &mut path);
    if length == 0 || length >= path.len() || !executable(&path[..length]) {
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
    if !crate::sampling::sampling_guard() || !crate::presentation::presentation_guard() {
        return 0;
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
    crate::observers::bind_targets(
        [JITTER, PROCESS, RESOLUTION, END, UPLOAD].map(|site| base.wrapping_add(site.rva)),
    );
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
/// `out` is null or points to the caller's writable sized API table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_publish_api(
    requested: u32,
    out: *mut rsf_game_plugin_api,
) -> rsf_result {
    if out.is_null()
        || (unsafe { out.cast::<u32>().read() } as usize) < size_of::<rsf_game_plugin_api>()
    {
        return INVALID_ARGUMENT;
    }
    if requested != RSF_GAME_ABI_VERSION {
        return ABI_MISMATCH;
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
