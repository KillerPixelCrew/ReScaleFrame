// SPDX-License-Identifier: MIT
//! The Unity package's presentation behind package.cpp: the package identity (the SDK root with
//! its streamline, fidelityfx and xess directories and the Streamline interposer, Unity's version
//! and the project id), the one generation install, and the provider request of every window
//! pass. The identity comes from [UnitySR] when the plugin loads and from rsfu_initialize for each
//! session; SR views read it for their contexts. package.cpp keeps the window guard and the DXGI
//! adapter lookup behind Auto, and lends rsf_fg12_install, the presentation host's calls and the
//! package log in the Runtime's form as leaves. The provider choice for Auto is the Runtime's rsf_fg_default_backend, reached
//! through that leaf; nothing here selects a vendor.
use super::{BACKEND_OK, Leaves, PACKAGE, Package};
use crate::generation::{self, Sources};
use crate::package_abi::{RSF_FG_OFF, rsf_unity_package_platform, rsf_unity_package_view_setup};
use crate::runtime_abi::{
    RSF_FG_BACKEND_AUTO, RSF_REFLEX_OFF, rsf_fg12_setup, rsf_native_fg_options,
};
use std::ffi::CString;
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::sync::{Mutex, PoisonError};

/// The package's SDK directories and Streamline identity, as UTF-8 C strings.
#[derive(Clone, Default)]
pub(super) struct Identity {
    pub(super) root: CString,
    pub(super) streamline: CString,
    pub(super) interposer: CString,
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

fn joined(base: &CString, leaf: &str) -> CString {
    text(&[base.as_bytes(), leaf.as_bytes()].concat())
}

impl Identity {
    /// The directories under the SDK root as the C++ made them, also for an empty root.
    pub(super) fn new(root: &[u8], version: &[u8], project: &[u8]) -> Self {
        let root = text(root);
        let streamline = joined(&root, "/streamline");
        Self {
            interposer: joined(&streamline, "/sl.interposer.dll"),
            fsr: joined(&root, "/fidelityfx"),
            xess: joined(&root, "/xess"),
            streamline,
            root,
            version: text(version),
            project: text(project),
        }
    }

    /// Without a root, a version and a project id there is nothing to install.
    fn installable(&self) -> bool {
        !self.root.is_empty() && !self.version.is_empty() && !self.project.is_empty()
    }

    /// Lends the identity to a view's setup for one leaf call.
    pub(super) fn lend(&self, setup: &mut rsf_unity_package_view_setup) {
        setup.root_utf8 = self.root.as_ptr();
        setup.streamline_utf8 = self.streamline.as_ptr();
        setup.interposer_utf8 = self.interposer.as_ptr();
        setup.fsr_utf8 = self.fsr.as_ptr();
        setup.xess_utf8 = self.xess.as_ptr();
        setup.engine_version_utf8 = self.version.as_ptr();
        setup.project_id_utf8 = self.project.as_ptr();
    }
}

pub(super) struct Presentation {
    /// None until the plugin loads, which reads as every string empty.
    identity: Mutex<Option<Identity>>,
    /// Whether rsf_fg12_install accepted the setup; held across the install.
    installed: Mutex<bool>,
    /// Auto's provider for Unity's adapter, kept for the process once it is non-zero.
    default_backend: AtomicU32,
}

impl Presentation {
    pub(super) fn device_retired(&self) {
        self.default_backend.store(0, SeqCst);
    }
    pub(super) const fn new() -> Self {
        Self {
            identity: Mutex::new(None),
            installed: Mutex::new(false),
            default_backend: AtomicU32::new(0),
        }
    }

    pub(super) fn identity(&self) -> Identity {
        self.identity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_default()
    }

    pub(super) fn set_identity(&self, identity: Identity) {
        *self.identity.lock().unwrap_or_else(PoisonError::into_inner) = Some(identity);
    }
}

// SAFETY for every call below: as for the other leaves, package.cpp's noexcept functions that
// borrow their pointer arguments for the call only.
impl Leaves {
    fn fg_install(&self, setup: &rsf_fg12_setup) -> i32 {
        unsafe { (self.fg_install)(setup) }
    }
    fn present_has_owner(&self) -> bool {
        unsafe { (self.present_has_owner)() != 0 }
    }
    fn present_request(&self, backend: u32) -> i32 {
        unsafe { (self.present_request)(backend) }
    }
    /// Auto's provider for Unity's adapter; None when the device or adapter cannot be read.
    fn fg_default_backend(&self) -> Option<u32> {
        let mut backend = 0u32;
        (unsafe { (self.fg_default_backend)(&mut backend) } != 0).then_some(backend)
    }
}

impl Package {
    /// The one install: the Auto provider with generation off until rsfu_configure, switchable
    /// at run time, from the SDK root's fidelityfx, xess and streamline directories. A refusal
    /// is retried by the next call. Its result is the generation result.
    pub(super) fn install(&self, leaves: &Leaves) {
        let identity = self.presentation.identity();
        let result = {
            let mut installed = self
                .presentation
                .installed
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
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
            let setup = generation::setup(
                RSF_FG_BACKEND_AUTO,
                options,
                &sources,
                leaves.accept_window,
                leaves.backend_log,
            );
            let result = leaves.fg_install(&setup);
            *installed = result == BACKEND_OK;
            result
        };
        self.lock().status.fg_result = result;
    }

    /// The window pass's request, in the C++ order: nothing without a presentation owner, the
    /// stored provider with Auto resolved for Unity's adapter (nothing while it cannot be read),
    /// nothing while that provider is active, else the request, whose result is recorded.
    pub(super) fn request_generation(&self, leaves: &Leaves) {
        if !leaves.present_has_owner() {
            return;
        }
        let mut requested = self.lock().settings.fg_backend;
        if requested == RSF_FG_BACKEND_AUTO {
            requested = self.presentation.default_backend.load(SeqCst);
            if requested == 0 {
                let Some(resolved) = leaves.fg_default_backend() else {
                    return;
                };
                self.presentation.default_backend.store(resolved, SeqCst);
                requested = resolved;
            }
        }
        if requested == leaves.present_backend() {
            return;
        }
        let result = leaves.present_request(requested);
        self.lock().status.fg_result = result;
    }
}

/// Plugin load. Reads the identity from [UnitySR]: Runtime resolved like every [UnitySR] path,
/// EngineVersion and ProjectId as written, at most 255 characters each as the C++ read them.
/// Then the session services open the [UnitySR] Log path, resolved the same way, and the player
/// installs generation (services.rs).
///
/// # Safety
/// `ini` is null or a NUL-terminated UTF-16 path; `platform` is null or package.cpp's table.
#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_load(
    ini: *const u16,
    editor: u32,
    platform: *const rsf_unity_package_platform,
) {
    use crate::runtime_config::{Ini, path_setting, string_setting, utf8};
    use windows_sys::core::w;
    const PROFILE_TEXT: usize = 256;

    let Some(leaves) = (unsafe { Leaves::adopt(platform) }) else {
        return;
    };
    let mut log = Vec::new();
    if let Some(ini) = unsafe { Ini::new(ini) } {
        let root = utf8(&path_setting(&ini, w!("Runtime")));
        let version = utf8(&string_setting(&ini, w!("EngineVersion"), PROFILE_TEXT));
        let project = utf8(&string_setting(&ini, w!("ProjectId"), PROFILE_TEXT));
        PACKAGE
            .presentation
            .set_identity(Identity::new(&root, &version, &project));
        log = path_setting(&ini, w!("Log"));
    }
    PACKAGE.loaded(&leaves, editor != 0, &log);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_install(platform: *const rsf_unity_package_platform) {
    if let Some(leaves) = unsafe { Leaves::adopt(platform) } {
        PACKAGE.install(&leaves);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_request_generation(
    platform: *const rsf_unity_package_platform,
) {
    if let Some(leaves) = unsafe { Leaves::adopt(platform) } {
        PACKAGE.request_generation(&leaves);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_derives_the_sdk_directories() {
        let identity = Identity::new(b"C:/sdk", b"6000.0.1", b"p\0ignored");
        let pairs = [
            (&identity.root, "C:/sdk"),
            (&identity.streamline, "C:/sdk/streamline"),
            (&identity.interposer, "C:/sdk/streamline/sl.interposer.dll"),
            (&identity.fsr, "C:/sdk/fidelityfx"),
            (&identity.xess, "C:/sdk/xess"),
            (&identity.version, "6000.0.1"),
            (&identity.project, "p"),
        ];
        for (value, expected) in pairs {
            assert_eq!(value.to_str().unwrap(), expected);
        }
        assert!(identity.installable());
        // An empty root still names its directories, as the C++ paths() did.
        let rootless = Identity::new(b"", b"1", b"p");
        assert_eq!(rootless.fsr.to_str().unwrap(), "/fidelityfx");
        assert!(!rootless.installable());
        assert!(!Identity::new(b"r", b"", b"p").installable());
        assert!(!Identity::new(b"r", b"1", b"").installable());
        // Before the plugin loads every string is empty.
        let unloaded = Presentation::new().identity();
        assert!(unloaded.streamline.is_empty() && !unloaded.installable());
    }

    #[test]
    fn a_view_borrows_the_identity() {
        let identity = Identity::new(b"R", b"6000", b"P");
        let mut setup = rsf_unity_package_view_setup::default();
        identity.lend(&mut setup);
        let text = |pointer| {
            unsafe { std::ffi::CStr::from_ptr(pointer) }
                .to_str()
                .unwrap()
        };
        assert_eq!(
            [
                text(setup.root_utf8),
                text(setup.streamline_utf8),
                text(setup.interposer_utf8),
                text(setup.fsr_utf8),
                text(setup.xess_utf8),
                text(setup.engine_version_utf8),
                text(setup.project_id_utf8),
            ],
            [
                "R",
                "R/streamline",
                "R/streamline/sl.interposer.dll",
                "R/fidelityfx",
                "R/xess",
                "6000",
                "P"
            ]
        );
    }

    #[test]
    fn entries_refuse_an_incomplete_table() {
        unsafe {
            rsf_unity_package_install(std::ptr::null());
            rsf_unity_package_request_generation(std::ptr::null());
        }
    }
}
