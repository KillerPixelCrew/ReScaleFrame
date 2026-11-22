// SPDX-License-Identifier: MIT
//! Sized GameSDK records shared by native plugin carriers and exact-build detectors.
use crate::ffi_util::has_capacity;
use crate::{
    RSF_ERROR_ABI_MISMATCH, RSF_ERROR_INVALID_ARGUMENT, RSF_GAME_ABI_VERSION, RSF_GAME_RECOGNIZED,
    RSF_GAME_UNKNOWN, RSF_OK, rsf_detection, rsf_game_hooks_api, rsf_game_host_services,
    rsf_game_info, rsf_game_log_fn, rsf_game_plugin_api, rsf_game_probe, rsf_result,
};
use crate::{RSF_PLUGIN_SESSION_ABI_VERSION, rsf_plugin_session_options};
use core::ffi::CStr;

#[cfg(windows)]
pub mod carrier_log;

#[derive(Clone, Copy)]
pub struct Probe<'a> {
    pub machine: u16,
    pub executable: &'a CStr,
    pub sha256: &'a CStr,
}

impl<'a> Probe<'a> {
    /// Borrows a full size-only probe and its NUL-terminated identity fields.
    ///
    /// # Safety
    /// A non-null pointer must be aligned and readable through the size field.
    /// If the size is sufficient, the full probe and its non-null C strings must
    /// be readable and remain immutable through the returned borrow.
    pub unsafe fn from_raw(raw: *const rsf_game_probe) -> Option<Self> {
        if raw.is_null() || unsafe { raw.cast::<u32>().read() } < size_of::<rsf_game_probe>() as u32
        {
            return None;
        }
        let raw = unsafe { &*raw };
        if raw.executable_name_utf8.is_null() || raw.sha256_hex.is_null() {
            return None;
        }
        Some(Self {
            machine: raw.pe_machine,
            executable: unsafe { CStr::from_ptr(raw.executable_name_utf8) },
            sha256: unsafe { CStr::from_ptr(raw.sha256_hex) },
        })
    }

    pub fn matches(self, executable: &CStr, sha256: &CStr) -> bool {
        self.executable
            .to_bytes()
            .eq_ignore_ascii_case(executable.to_bytes())
            && self
                .sha256
                .to_bytes()
                .eq_ignore_ascii_case(sha256.to_bytes())
    }

    /// The `detect` entry of a plugin: `RSF_GAME_RECOGNIZED` when `raw` is a full probe of an
    /// AMD64 image for which `recognizes` accepts the executable name and SHA-256.
    ///
    /// # Safety
    /// As for [`Probe::from_raw`].
    pub unsafe fn detect(
        raw: *const rsf_game_probe,
        recognizes: impl FnOnce(Probe<'_>) -> bool,
    ) -> rsf_detection {
        match unsafe { Self::from_raw(raw) } {
            Some(probe) if probe.machine == PE_MACHINE_AMD64 && recognizes(probe) => {
                RSF_GAME_RECOGNIZED
            }
            _ => RSF_GAME_UNKNOWN,
        }
    }
}

const PE_MACHINE_AMD64: u16 = 0x8664;

/// The body of a plugin's exported `rsf_get_game_plugin_api`: checks the caller's table size and
/// ABI version, then writes the table.
///
/// # Safety
/// `out` is null or points to the caller's writable sized API table; a short one may contain only
/// its size.
pub unsafe fn publish_api(
    requested_abi: u32,
    out: *mut rsf_game_plugin_api,
    info: rsf_game_info,
    detect: unsafe extern "C" fn(*const rsf_game_probe) -> rsf_detection,
    hooks: rsf_game_hooks_api,
) -> rsf_result {
    if !unsafe { has_capacity(out.cast_const()) } {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    if requested_abi != RSF_GAME_ABI_VERSION {
        return RSF_ERROR_ABI_MISMATCH;
    }
    unsafe {
        out.write(rsf_game_plugin_api {
            struct_size: size_of::<rsf_game_plugin_api>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            info,
            detect: Some(detect),
            hooks,
        })
    };
    RSF_OK
}

/// Initializes sized host services with only session identity and logging.
pub fn host_services(tick: u64, log: rsf_game_log_fn) -> rsf_game_host_services {
    // Other fields are nullable C pointers and callbacks.
    let mut services: rsf_game_host_services = unsafe { core::mem::zeroed() };
    services.struct_size = size_of::<rsf_game_host_services>() as u32;
    services.abi_version = RSF_GAME_ABI_VERSION;
    services.session_id = tick.saturating_add(1);
    services.log = log;
    services
}

/// Lends the sized request and probe to a synchronous session operation.
/// The path is a Windows wide string with a terminating NUL.
pub fn with_session_options<R>(
    path: &[u16],
    probe: Probe<'_>,
    services: &rsf_game_host_services,
    call: impl FnOnce(&rsf_plugin_session_options) -> R,
) -> R {
    let probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: probe.machine,
        reserved: 0,
        executable_name_utf8: probe.executable.as_ptr(),
        sha256_hex: probe.sha256.as_ptr(),
    };
    let request = rsf_plugin_session_options {
        struct_size: size_of::<rsf_plugin_session_options>() as u32,
        abi_version: RSF_PLUGIN_SESSION_ABI_VERSION,
        plugin_path: path.as_ptr(),
        probe: &probe,
        services: *services,
    };
    call(&request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null;

    #[test]
    fn probe_checks_size_and_null_fields_before_borrowing() {
        assert!(unsafe { Probe::from_raw(null()) }.is_none());
        let mut raw = rsf_game_probe {
            struct_size: 4,
            pe_machine: 0x8664,
            reserved: 0,
            executable_name_utf8: c"Game.exe".as_ptr(),
            sha256_hex: c"ABC".as_ptr(),
        };
        assert!(unsafe { Probe::from_raw(&raw) }.is_none());
        raw.struct_size = size_of::<rsf_game_probe>() as u32;
        let probe = unsafe { Probe::from_raw(&raw) }.unwrap();
        assert_eq!(probe.machine, 0x8664);
        assert!(probe.matches(c"game.EXE", c"abc"));
        raw.sha256_hex = null();
        assert!(unsafe { Probe::from_raw(&raw) }.is_none());
    }

    #[test]
    fn detect_needs_amd64_and_an_accepted_build() {
        let mut raw = rsf_game_probe {
            struct_size: size_of::<rsf_game_probe>() as u32,
            pe_machine: 0x8664,
            reserved: 0,
            executable_name_utf8: c"Game.exe".as_ptr(),
            sha256_hex: c"ABC".as_ptr(),
        };
        let accepts = |probe: Probe<'_>| probe.matches(c"game.EXE", c"abc");
        assert_eq!(unsafe { Probe::detect(&raw, accepts) }, RSF_GAME_RECOGNIZED);
        raw.pe_machine = 0x14c;
        assert_eq!(unsafe { Probe::detect(&raw, accepts) }, RSF_GAME_UNKNOWN);
        raw.pe_machine = 0x8664;
        assert_eq!(unsafe { Probe::detect(&raw, |_| false) }, RSF_GAME_UNKNOWN);
        assert_eq!(unsafe { Probe::detect(null(), accepts) }, RSF_GAME_UNKNOWN);
    }
}
