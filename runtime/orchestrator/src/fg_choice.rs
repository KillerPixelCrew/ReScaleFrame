// SPDX-License-Identifier: MIT
use crate::settings_store::{rsf_settings_store_read_integer, rsf_settings_store_write_integer};
use crate::sys::{
    RSF_FG_BACKEND_ALL as PROVIDERS, RSF_FG_BACKEND_AUTO as AUTO, rsf_fg_choice as Choice,
};
use crate::sys::{
    RSF_FG_BACKEND_DLSS as DLSS, RSF_FG_BACKEND_FSR3 as FSR3, RSF_FG_BACKEND_FSR4 as FSR4,
    RSF_FG_BACKEND_XESS as XESS, rsf_fg_backend_info,
};
use rsf_game_sys::backend_result::{FEATURE_FAILED, NOT_SUPPORTED};
use std::ffi::{CStr, c_int};
use std::sync::Mutex;
use windows_sys::core::{PCWSTR, w};

const SECTION: PCWSTR = w!("Rendering");
const KEY_UTF8: &CStr = c"FrameGeneration";

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fg_default_backend(vendor: u32, software: u32) -> u32 {
    if software != 0 {
        return 0;
    }
    match vendor {
        0x10de => DLSS,
        0x8086 => XESS,
        0x1002 => FSR3,
        _ => 0,
    }
}

struct BackendInfo(rsf_fg_backend_info);

// SAFETY: The immutable records only hold pointers to static C string literals.
unsafe impl Sync for BackendInfo {}

impl BackendInfo {
    const fn new(
        id: u32,
        ini_key: &'static CStr,
        env_var: &'static CStr,
        default_subdir: &'static CStr,
    ) -> Self {
        Self(rsf_fg_backend_info {
            id,
            ini_key: ini_key.as_ptr(),
            env_var: env_var.as_ptr(),
            default_subdir: default_subdir.as_ptr(),
        })
    }
}

// The legacy INI labels are returned to callers as metadata.
static BACKENDS: [BackendInfo; 4] = [
    BackendInfo::new(
        DLSS,
        c"Streamline",
        c"RSF_STREAMLINE_BIN",
        c"ReScaleFrame\\streamline",
    ),
    BackendInfo::new(FSR3, c"FSR3", c"RSF_FSR3_BIN", c"ReScaleFrame\\fidelityfx"),
    BackendInfo::new(FSR4, c"FSR4", c"RSF_FSR4_BIN", c"ReScaleFrame\\fidelityfx"),
    BackendInfo::new(XESS, c"XeSS", c"RSF_XESS_BIN", c"ReScaleFrame\\xess"),
];

#[unsafe(no_mangle)]
extern "C" fn rsf_fg_backend_info_for(id: u32) -> *const rsf_fg_backend_info {
    BACKENDS
        .iter()
        .find(|info| info.0.id == id)
        .map_or(std::ptr::null(), |info| &info.0)
}

pub(crate) fn known(backend: u32) -> bool {
    1u32.checked_shl(backend).unwrap_or(0) & PROVIDERS != 0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fg_backend_known(id: u32) -> c_int {
    c_int::from(known(id))
}

struct Settings {
    // Empty when settings are memory-only, otherwise terminated UTF-16.
    path: Vec<u16>,
    choice: Choice,
}

static SETTINGS: Mutex<Settings> = Mutex::new(Settings {
    path: Vec::new(),
    choice: Choice {
        backend: 0,
        choices: 0,
        last_result: 0,
    },
});

// Auto needs every provider among `choices`; zero (Off) needs bit 0.
pub(crate) fn allowed(backend: u32, choices: u32) -> bool {
    if backend == AUTO {
        return choices & PROVIDERS == PROVIDERS;
    }
    let bit = 1u32.checked_shl(backend).unwrap_or(0);
    bit & choices & (PROVIDERS | 1) != 0
}

/// FG Auto among the providers a host can serve (`choices`, bit n = provider n): the adapter's
/// `preferred` provider (`rsf_fg_default_backend`, zero for none) when served, otherwise FSR3 when
/// served, otherwise zero. The private `fg_auto_choice.h` declares the C entry point for
/// statically linked consumers.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fg_select_auto_backend(preferred: u32, choices: u32) -> u32 {
    [preferred, FSR3]
        .into_iter()
        .find(|&backend| backend != 0 && backend != AUTO && allowed(backend, choices))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_choice_start(path: PCWSTR, fallback: u32, choices: u32) -> u32 {
    let mut settings = SETTINGS.lock().unwrap();
    let mut length = 0;
    if !path.is_null() {
        while unsafe { *path.add(length) } != 0 {
            length += 1;
        }
    }
    let saved = if length == 0 {
        settings.path.clear();
        fallback
    } else {
        // Keep the previous session intact if copying the caller's path fails.
        let additional = (length + 1).saturating_sub(settings.path.len());
        if settings.path.try_reserve(additional).is_err() {
            return fallback;
        }
        settings.path.clear();
        settings
            .path
            .extend_from_slice(unsafe { std::slice::from_raw_parts(path, length + 1) });
        unsafe { rsf_settings_store_read_integer(path, SECTION, KEY_UTF8.as_ptr(), fallback) }
    };
    let choices = choices | 1;
    let backend = [saved, fallback]
        .into_iter()
        .find(|&id| allowed(id, choices))
        .unwrap_or(0);
    settings.choice = Choice {
        backend,
        choices,
        last_result: 0,
    };
    backend
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_fg_choice_save(backend: u32) -> i32 {
    let mut settings = SETTINGS.lock().unwrap();
    let result = if !allowed(backend, settings.choice.choices) {
        NOT_SUPPORTED
    } else if settings.path.is_empty() {
        FEATURE_FAILED
    } else {
        if unsafe {
            rsf_settings_store_write_integer(
                settings.path.as_ptr(),
                SECTION,
                KEY_UTF8.as_ptr(),
                backend,
            )
        } != 0
        {
            FEATURE_FAILED
        } else {
            settings.choice.backend = backend;
            0
        }
    };
    settings.choice.last_result = result;
    result
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fg_choice_get() -> Choice {
    SETTINGS.lock().unwrap().choice
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fg_choice_resolve_auto(backend: u32) {
    let mut settings = SETTINGS.lock().unwrap();
    if settings.choice.backend == AUTO && allowed(backend, settings.choice.choices) {
        settings.choice.backend = backend;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(ids: &[u32]) -> u32 {
        ids.iter().map(|&id| 1u32 << id).sum()
    }

    #[test]
    fn auto_takes_the_served_preference_then_fsr3() {
        let all = bits(&[DLSS, FSR3, FSR4, XESS]);
        for preferred in [DLSS, FSR3, XESS] {
            assert_eq!(rsf_fg_select_auto_backend(preferred, all), preferred);
        }
        // No preference (software or unknown adapter), or one the host cannot serve.
        assert_eq!(rsf_fg_select_auto_backend(0, all), FSR3);
        assert_eq!(rsf_fg_select_auto_backend(DLSS, bits(&[FSR3, XESS])), FSR3);
        assert_eq!(rsf_fg_select_auto_backend(XESS, bits(&[DLSS])), 0);
        // Off, Auto and unknown IDs are never chosen.
        assert_eq!(rsf_fg_select_auto_backend(0, 1), 0);
        assert_eq!(rsf_fg_select_auto_backend(AUTO, all), FSR3);
        assert_eq!(rsf_fg_select_auto_backend(2, all | 4), FSR3);
        assert_eq!(rsf_fg_select_auto_backend(DLSS, 0), 0);
    }
}
