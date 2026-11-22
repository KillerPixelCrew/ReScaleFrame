// SPDX-License-Identifier: MIT
//! Admission and lifetime of the optional F9 renderer-root observers.

#[cfg(all(windows, not(test)))]
mod native;

use crate::sites::{POSTPROCESS_ENTRY, POSTPROCESS_RVA, WIDGET_QUEUE_ENTRY, WIDGET_QUEUE_RVA};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};

const RHI_WRITE_RVA: usize = 0x120e67c;
const RHI_WRITE: &[u8] = &[0x48, 0x89, 0x05, 0xd5, 0x9c, 0xa6, 0x02];
const CURRENT_COMMAND_RVA: usize = 0x3c78358;
const VELOCITY_SIZE_RVA: usize = 0x10fdd09;
const VELOCITY_SIZE_NATIVE: &[u8] = b"\x0f\x2f\xdd\x76\x38";
const VELOCITY_SIZE_PATCHED: &[u8] = b"\x0f\x2f\xdd\x90\x90";

// Order matches the typed Should, Has and History originals in the native capture.
const ENGINE_SITES: [(usize, &[u8]); 3] = [
    (
        0x10fdc00,
        b"\x40\x53\x48\x83\xec\x20\x4c\x8b\xda\x48\x8b\xd9\x45\x84\xc0",
    ),
    (
        0x1183820,
        b"\x40\x53\x48\x83\xec\x60\x80\xb9\x34\x0c\x00\x00",
    ),
    (
        0x10ee670,
        b"\x40\x53\x48\x83\xec\x20\x49\x8b\xd8\x48\x85\xd2\x74\x3a",
    ),
];

struct RootSite {
    rva: usize,
    expected: &'static [u8],
    native_owned: bool,
}

// Order matches the typed detour and original arrays in motion_capture_trampolines.c. Short stable
// entry sequences omit the security-cookie displacement. MinHook relocates full instructions.
const ROOT_SITES: [RootSite; 6] = [
    RootSite {
        // Post-process
        rva: POSTPROCESS_RVA as usize,
        expected: POSTPROCESS_ENTRY,
        native_owned: true,
    },
    RootSite {
        // Widget prepare
        rva: 0x4d69a0,
        expected: &[
            0x4c, 0x8b, 0xdc, 0x55, 0x48, 0x81, 0xec, 0x10, 0x03, 0x00, 0x00,
        ],
        native_owned: false,
    },
    RootSite {
        // Widget queue
        rva: WIDGET_QUEUE_RVA as usize,
        expected: WIDGET_QUEUE_ENTRY.split_at(15).0,
        native_owned: true,
    },
    RootSite {
        // Main temporal construction
        rva: 0xff6ad0,
        expected: &[
            0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x7c, 0x24, 0x18, 0x55, 0x41, 0x54, 0x41,
            0x55, 0x41, 0x56, 0x41, 0x57,
        ],
        native_owned: false,
    },
    RootSite {
        // Main temporal execution
        rva: 0x10aeca0,
        expected: &[
            0x40, 0x55, 0x53, 0x56, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x8d,
            0xac, 0x24, 0xf0, 0xfd, 0xff, 0xff,
        ],
        native_owned: false,
    },
    RootSite {
        // Recursive graph execution
        rva: 0x10b3f10,
        expected: &[
            0x4c, 0x89, 0x44, 0x24, 0x18, 0x48, 0x89, 0x4c, 0x24, 0x08, 0x53, 0x48, 0x83, 0xec,
            0x30,
        ],
        native_owned: false,
    },
];

enum RootInstall {
    AlreadyInstalled,
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
        let mut actual = [0u8; 20];
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
    if base.is_null() || !effects.matches(base.wrapping_add(RHI_WRITE_RVA), RHI_WRITE) {
        return RootInstall::RhiRefused;
    }

    let native_owner = NATIVE_OWNER.load(Ordering::Acquire);
    let mut targets = [null_mut(); ROOT_SITES.len()];
    for (target, site) in targets.iter_mut().zip(ROOT_SITES) {
        if native_owner && site.native_owned {
            continue;
        }
        let address = base.wrapping_add(site.rva);
        if !effects.matches(address, site.expected) {
            return RootInstall::EntryRefused;
        }
        *target = address.cast();
    }

    let result = registration.install(targets, effects);
    if registration.installed {
        CURRENT_COMMAND.store(
            base.wrapping_add(CURRENT_COMMAND_RVA).cast(),
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
    let size = base.wrapping_add(VELOCITY_SIZE_RVA);
    if base.is_null()
        || !(effects.matches(size, VELOCITY_SIZE_NATIVE)
            || (NATIVE_OWNER.load(Ordering::Acquire)
                && effects.matches(size, VELOCITY_SIZE_PATCHED)))
    {
        return RootInstall::EntryRefused;
    }
    let mut targets = [null_mut(); ENGINE_SITES.len()];
    for (target, (rva, expected)) in targets.iter_mut().zip(ENGINE_SITES) {
        let address = base.wrapping_add(rva);
        if !effects.matches(address, expected) {
            return RootInstall::EntryRefused;
        }
        *target = address.cast();
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
