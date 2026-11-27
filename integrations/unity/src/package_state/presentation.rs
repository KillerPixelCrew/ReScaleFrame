// SPDX-License-Identifier: MIT
//! The Unity package's presentation behind package.c: the package identity (the SDK root with
//! its streamline, fidelityfx and xess directories, Unity's version
//! and the project id) and the one generation install. The identity comes from [UnitySR] when
//! the plugin loads and from rsfu_initialize for each session; the Runtime host each session
//! attaches reads it for its SR contexts. Provider requests are the host's.
use super::{Package, raw};
use crate::generation::{self, Sources};
use crate::sys::RSF_FG_OFF;
use crate::sys::{RSF_FG_BACKEND_AUTO, RSF_REFLEX_OFF, rsf_fg12_setup, rsf_native_fg_options};
use rsf_game_sys::backend_result;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::vendor_paths::VendorPaths;
use std::ffi::CString;
use std::sync::Mutex;

/// The package's SDK directories and Streamline identity, as UTF-8 C strings.
#[derive(Clone, Default)]
pub(super) struct Identity {
    pub(super) root: CString,
    pub(super) streamline: CString,
    pub(super) fsr: CString,
    pub(super) xess: CString,
    pub(super) version: CString,
    pub(super) project: CString,
}

/// UTF-8 text up to its first NUL, as a std::string's c_str() handed it on.
fn text(bytes: &[u8]) -> CString {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap_or_default()
}

impl Identity {
    /// The directories under the SDK root, also for an empty root.
    pub(super) fn new(root: &[u8], version: &[u8], project: &[u8]) -> Self {
        let root = text(root);
        let vendors = VendorPaths::new(&root);
        Self {
            fsr: vendors.fidelityfx,
            xess: vendors.xess,
            streamline: vendors.streamline,
            root,
            version: text(version),
            project: text(project),
        }
    }

    /// Without a root, a version and a project id there is nothing to install.
    fn installable(&self) -> bool {
        !self.root.is_empty() && !self.version.is_empty() && !self.project.is_empty()
    }
}

pub(super) struct Presentation {
    /// None until the plugin loads, which reads as every string empty.
    identity: Mutex<Option<Identity>>,
    /// Whether rsf_fg12_install accepted the setup; held across the install.
    installed: Mutex<bool>,
}

impl Presentation {
    pub(super) const fn new() -> Self {
        Self {
            identity: Mutex::new(None),
            installed: Mutex::new(false),
        }
    }

    pub(super) fn identity(&self) -> Identity {
        lock_or_recover(&self.identity).clone().unwrap_or_default()
    }

    pub(super) fn set_identity(&self, identity: Identity) {
        *lock_or_recover(&self.identity) = Some(identity);
    }
}

impl Package {
    /// The one install: the Auto provider with generation off until rsfu_configure, switchable
    /// at run time, from the SDK root's fidelityfx, xess and streamline directories. A refusal
    /// is retried by the next call.
    pub(super) fn install(&self) {
        let identity = self.presentation.identity();
        {
            let mut installed = lock_or_recover(&self.presentation.installed);
            if *installed || !identity.installable() {
                return;
            }
            let options = rsf_native_fg_options {
                struct_size: size_of::<rsf_native_fg_options>() as u32,
                mode: RSF_FG_OFF,
                generated_frames: 1,
                reflex_mode: RSF_REFLEX_OFF,
                frame_limit_us: 0,
                debug: 0,
            };
            let sources = Sources {
                runtime: identity.fsr.as_ptr(),
                fsr3: identity.fsr.as_ptr(),
                fsr4: identity.fsr.as_ptr(),
                xess: identity.xess.as_ptr(),
                streamline: identity.streamline.as_ptr(),
                engine_version: identity.version.as_ptr(),
                project_id: identity.project.as_ptr(),
            };
            let setup: rsf_fg12_setup = generation::setup(
                RSF_FG_BACKEND_AUTO,
                options,
                &sources,
                super::routing::accept_window,
                super::services::backend_log,
            );
            *installed = unsafe { raw::fg_install(&setup) } == backend_result::OK;
        }
    }
}

/// Plugin load. Reads the identity from [UnitySR]: Runtime resolved like every [UnitySR] path,
/// EngineVersion and ProjectId as written, at most 255 characters each. Then the session
/// services open the [UnitySR] Log path, resolved the same way, and the player installs
/// generation (services.rs).
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
pub(super) fn load() {
    use super::PACKAGE;
    use crate::runtime_config::{Ini, path_setting, string_setting, utf8};
    use windows_sys::core::w;
    const PROFILE_TEXT: usize = 256;

    let (editor, ini) = super::windows::startup_paths();
    let mut log = Vec::new();
    if let Some(ini) = unsafe { Ini::new(ini.as_ptr()) } {
        let root = utf8(&path_setting(&ini, w!("Runtime")));
        let version = utf8(&string_setting(&ini, w!("EngineVersion"), PROFILE_TEXT));
        let project = utf8(&string_setting(&ini, w!("ProjectId"), PROFILE_TEXT));
        PACKAGE
            .presentation
            .set_identity(Identity::new(&root, &version, &project));
        log = path_setting(&ini, w!("Log"));
    }
    PACKAGE.loaded(editor, &log);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_derives_the_sdk_directories() {
        let identity = Identity::new(b"C:/sdk", b"6000.0.1", b"p\0ignored");
        let pairs = [
            (&identity.root, "C:/sdk"),
            (&identity.streamline, r"C:\sdk\streamline"),
            (&identity.fsr, r"C:\sdk\fidelityfx"),
            (&identity.xess, r"C:\sdk\xess"),
            (&identity.version, "6000.0.1"),
            (&identity.project, "p"),
        ];
        for (value, expected) in pairs {
            assert_eq!(value.to_str().unwrap(), expected);
        }
        assert!(identity.installable());
        // An empty root still names its directories.
        let rootless = Identity::new(b"", b"1", b"p");
        assert_eq!(rootless.fsr.to_str().unwrap(), r"\fidelityfx");
        assert!(!rootless.installable());
        assert!(!Identity::new(b"r", b"", b"p").installable());
        assert!(!Identity::new(b"r", b"1", b"").installable());
        // Before the plugin loads every string is empty.
        let unloaded = Presentation::new().identity();
        assert!(unloaded.streamline.is_empty() && !unloaded.installable());
    }
}
