// SPDX-License-Identifier: MIT
//! Admission and lifetime of the optional F9 renderer-root observers.

#[cfg(all(windows, not(test)))]
mod native;

use crate::discovery::{self, POSTPROCESS, Site, WIDGET_QUEUE};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};

/// `comiss xmm3,xmm5; jbe`: the size gate the native renderer's temporal velocity patch opens.
const VELOCITY_SIZE_NATIVE: &[u8] = b"\x0f\x2f\xdd\x76\x38";
const VELOCITY_SIZE_PATCHED: &[u8] = b"\x0f\x2f\xdd\x90\x90";

// Order matches the typed Should, Has and History originals in the native capture.
const ENGINE_SITES: [Site; 3] = [
    Site::ShouldRenderVelocity,
    Site::HasVelocity,
    Site::VelocityHistory,
];

/// A discovered entry and whether the native renderer hooks it itself.
struct RootSite {
    site: usize,
    native_owned: bool,
}

// Order matches the typed detour and original arrays in motion_capture_trampolines.c. MinHook
// relocates full instructions.
const ROOT_SITES: [RootSite; 6] = [
    RootSite {
        site: POSTPROCESS,
        native_owned: true,
    },
    RootSite {
        site: Site::PrepareSlateWindow as usize,
        native_owned: false,
    },
    RootSite {
        site: WIDGET_QUEUE,
        native_owned: true,
    },
    RootSite {
        // Main temporal construction
        site: Site::AddTemporalAa as usize,
        native_owned: false,
    },
    RootSite {
        // Main temporal execution
        site: Site::TemporalAaProcess as usize,
        native_owned: false,
    },
    RootSite {
        // Recursive graph execution
        site: Site::RecursivelyProcess as usize,
        native_owned: false,
    },
];
enum RootInstall {
    AlreadyInstalled,
    /// A site did not resolve; the text names it.
    Undiscovered(String),
    RhiRefused,
    EntryRefused,
    RolledBack,
    Installed,
    PartialTeardown,
}

// The native leaves keep typed trampolines and MinHook effects, including rollback.
// The target array is borrowed for each call.
struct RootHookEffects {
    copy: unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool,
    install: unsafe extern "C" fn(*mut *mut c_void, u32) -> bool,
    remove: unsafe extern "C" fn(*mut *mut c_void, u32),
}

struct HookTargets<const N: usize> {
    entries: [*mut c_void; N],
    installed: bool,
}

// SAFETY: These process-wide hook identities are only passed to native MinHook leaves
// under the registration mutex. Rust never dereferences them.
unsafe impl<const N: usize> Send for HookTargets<N> {}

impl<const N: usize> HookTargets<N> {
    const fn new() -> Self {
        Self {
            entries: [null_mut(); N],
            installed: false,
        }
    }

    fn status(&self) -> Option<RootInstall> {
        if self.installed {
            Some(RootInstall::AlreadyInstalled)
        } else if self.entries.iter().any(|target| !target.is_null()) {
            Some(RootInstall::PartialTeardown)
        } else {
            None
        }
    }

    fn install(&mut self, mut targets: [*mut c_void; N], effects: &RootHookEffects) -> RootInstall {
        // SAFETY: All non-null targets passed the guarded reads. The leaf borrows the array
        // and leaves only the slots it still owns on return.
        self.installed = unsafe { (effects.install)(targets.as_mut_ptr(), N as u32) };
        self.entries = targets;
        if self.installed {
            RootInstall::Installed
        } else {
            self.status().unwrap_or(RootInstall::RolledBack)
        }
    }

    fn remove(&mut self, effects: &RootHookEffects) -> bool {
        if self.status().is_some() {
            // SAFETY: This is the mutable array of targets retained by the install/removal leaf.
            unsafe { (effects.remove)(self.entries.as_mut_ptr(), N as u32) };
        }
        self.installed = false;
        self.status().is_none()
    }
}

// Serialize root install and removal. Draws and finish stats can read the published address
// without taking this lock, and the native wrapper logs after the transition returns.
static REGISTRATION: Mutex<HookTargets<{ ROOT_SITES.len() }>> = Mutex::new(HookTargets::new());
static ENGINE: Mutex<HookTargets<{ ENGINE_SITES.len() }>> = Mutex::new(HookTargets::new());
static NATIVE_OWNER: AtomicBool = AtomicBool::new(false);
static CURRENT_COMMAND: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

fn lock<const N: usize>(hooks: &Mutex<HookTargets<N>>) -> MutexGuard<'_, HookTargets<N>> {
    hooks.lock().unwrap_or_else(|error| error.into_inner())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_motion_capture_native_owner(enabled: u32) {
    NATIVE_OWNER.store(enabled != 0, Ordering::Release);
}

impl RootHookEffects {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        let mut actual = [0u8; 8];
        let actual = &mut actual[..expected.len()];
        // SAFETY: The native leaf guards the source read and writes only the supplied length.
        (unsafe { (self.copy)(actual.as_mut_ptr().cast(), address.cast(), actual.len()) })
            && actual == expected
    }
}

fn install_roots(base: *mut u8, effects: &RootHookEffects) -> RootInstall {
    let mut registration = lock(&REGISTRATION);
    if let Some(status) = registration.status() {
        return status;
    }
    let sites = ROOT_SITES.iter().map(|root| root.site);
    let globals = [Site::RhiWrite, Site::CurrentCommand].map(|site| site as usize);
    if let Err(reason) = discovery::admit(sites.chain(globals)) {
        return RootInstall::Undiscovered(reason);
    }
    if base.is_null() || !discovery::verify(base.addr(), Site::RhiWrite as usize) {
        return RootInstall::RhiRefused;
    }

    let native_owner = NATIVE_OWNER.load(Ordering::Acquire);
    let mut targets = [null_mut(); ROOT_SITES.len()];
    for (target, root) in targets.iter_mut().zip(ROOT_SITES) {
        if native_owner && root.native_owned {
            continue;
        }
        if !discovery::verify(base.addr(), root.site) {
            return RootInstall::EntryRefused;
        }
        *target = base.wrapping_add(discovery::rva(root.site)).cast();
    }

    let result = registration.install(targets, effects);
    if registration.installed {
        CURRENT_COMMAND.store(
            base.wrapping_add(discovery::rva(Site::CurrentCommand as usize))
                .cast(),
            Ordering::Release,
        );
    }
    result
}

fn remove_roots(effects: &RootHookEffects) -> bool {
    let mut registration = lock(&REGISTRATION);
    let removed = registration.remove(effects);
    CURRENT_COMMAND.store(null_mut(), Ordering::Release);
    removed
}

fn install_engine(base: *mut u8, effects: &RootHookEffects) -> RootInstall {
    let mut engine = lock(&ENGINE);
    if let Some(status) = engine.status() {
        return status;
    }
    let sites = ENGINE_SITES.map(|site| site as usize);
    if let Err(reason) = discovery::admit(sites.into_iter().chain([Site::VelocitySize as usize])) {
        return RootInstall::Undiscovered(reason);
    }
    let size = base.wrapping_add(discovery::rva(Site::VelocitySize as usize));
    if base.is_null()
        || !(effects.matches(size, VELOCITY_SIZE_NATIVE)
            || (NATIVE_OWNER.load(Ordering::Acquire)
                && effects.matches(size, VELOCITY_SIZE_PATCHED)))
    {
        return RootInstall::EntryRefused;
    }
    let mut targets = [null_mut(); ENGINE_SITES.len()];
    for (target, site) in targets.iter_mut().zip(sites) {
        if !discovery::verify(base.addr(), site) {
            return RootInstall::EntryRefused;
        }
        *target = base.wrapping_add(discovery::rva(site)).cast();
    }
    engine.install(targets, effects)
}

fn remove_engine(effects: &RootHookEffects) -> bool {
    lock(&ENGINE).remove(effects)
}

pub(crate) fn engine_installed() -> bool {
    lock(&ENGINE).installed
}

pub(crate) fn current_command_address() -> *const c_void {
    CURRENT_COMMAND.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_rollback_keeps_only_the_owned_targets_for_retry() {
        let first = std::ptr::without_provenance_mut(1);
        let second = std::ptr::without_provenance_mut(2);
        let effects = RootHookEffects {
            copy: unused_copy,
            install: partial_install,
            remove: failed_remove,
        };
        let mut hooks = HookTargets::new();
        assert!(matches!(
            hooks.install([first, second], &effects),
            RootInstall::PartialTeardown
        ));
        assert_eq!(hooks.entries, [null_mut(), second]);
        assert!(!hooks.remove(&effects));
        assert!(matches!(hooks.status(), Some(RootInstall::PartialTeardown)));
        let effects = RootHookEffects {
            remove: successful_remove,
            ..effects
        };
        assert!(hooks.remove(&effects));
        assert!(hooks.status().is_none());
    }

    extern "C" fn unused_copy(_: *mut c_void, _: *const c_void, _: usize) -> bool {
        false
    }

    unsafe extern "C" fn partial_install(targets: *mut *mut c_void, _: u32) -> bool {
        // SAFETY: The fixture registered two entries and lends its mutable array.
        unsafe { targets.write(null_mut()) };
        false
    }

    extern "C" fn failed_remove(_: *mut *mut c_void, _: u32) {}

    unsafe extern "C" fn successful_remove(targets: *mut *mut c_void, count: u32) {
        // SAFETY: The fixture lends the full mutable array with the given entry count.
        let targets = unsafe { std::slice::from_raw_parts_mut(targets, count as usize) };
        targets.fill(null_mut());
    }
}
