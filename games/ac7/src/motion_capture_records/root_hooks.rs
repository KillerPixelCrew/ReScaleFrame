// SPDX-License-Identifier: MIT
//! Admission and lifetime of the optional F9 renderer-root observers.

use crate::sites::{POSTPROCESS_ENTRY, POSTPROCESS_RVA, WIDGET_QUEUE_ENTRY, WIDGET_QUEUE_RVA};
use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};

const RHI_WRITE_RVA: usize = 0x120e67c;
const RHI_WRITE: &[u8] = &[0x48, 0x89, 0x05, 0xd5, 0x9c, 0xa6, 0x02];
const CURRENT_COMMAND_RVA: usize = 0x3c78358;

struct RootSite {
    rva: usize,
    expected: &'static [u8],
    prefix: Option<usize>,
    native_owned: bool,
}

// Order matches the typed detour and original arrays in motion_capture.cpp. Short stable
// entry sequences omit the security-cookie displacement. MinHook relocates full instructions.
const ROOT_SITES: [RootSite; 6] = [
    RootSite {
        // Post-process
        rva: POSTPROCESS_RVA as usize,
        expected: POSTPROCESS_ENTRY,
        prefix: None,
        native_owned: true,
    },
    RootSite {
        // Widget prepare
        rva: 0x4d69a0,
        expected: &[
            0x4c, 0x8b, 0xdc, 0x55, 0x48, 0x81, 0xec, 0x10, 0x03, 0x00, 0x00,
        ],
        prefix: None,
        native_owned: false,
    },
    RootSite {
        // Widget queue
        rva: WIDGET_QUEUE_RVA as usize,
        expected: WIDGET_QUEUE_ENTRY,
        prefix: Some(15),
        native_owned: true,
    },
    RootSite {
        // Main temporal construction
        prefix: None,
        rva: 0xff6ad0,
        expected: &[
            0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x7c, 0x24, 0x18, 0x55, 0x41, 0x54, 0x41,
            0x55, 0x41, 0x56, 0x41, 0x57,
        ],
        native_owned: false,
    },
    RootSite {
        // Main temporal execution
        prefix: None,
        rva: 0x10aeca0,
        expected: &[
            0x40, 0x55, 0x53, 0x56, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x8d,
            0xac, 0x24, 0xf0, 0xfd, 0xff, 0xff,
        ],
        native_owned: false,
    },
    RootSite {
        // Recursive graph execution
        prefix: None,
        rva: 0x10b3f10,
        expected: &[
            0x4c, 0x89, 0x44, 0x24, 0x18, 0x48, 0x89, 0x4c, 0x24, 0x08, 0x53, 0x48, 0x83, 0xec,
            0x30,
        ],
        native_owned: false,
    },
];

#[repr(u32)]
enum RootInstall {
    AlreadyInstalled,
    RhiRefused,
    EntryRefused,
    RolledBack,
    Installed,
    PartialTeardown,
}

// Private ABI mirrored in motion_capture_records.h. The native leaves keep typed trampolines
// and all MinHook effects, including rollback. The target array is borrowed for each call.
#[repr(C)]
struct RootHookEffects {
    copy: unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool,
    native_owner: unsafe extern "C" fn() -> bool,
    install: unsafe extern "C" fn(*mut *mut c_void, u32) -> bool,
    remove: unsafe extern "C" fn(*mut *mut c_void, u32),
}

const _: () = {
    assert!(size_of::<RootHookEffects>() == 32 && offset_of!(RootHookEffects, remove) == 24);
};

struct RootTargets {
    entries: [*mut c_void; ROOT_SITES.len()],
    installed: bool,
}

// SAFETY: These process-wide hook identities are only passed to native MinHook leaves
// under the registration mutex. Rust never dereferences them.
unsafe impl Send for RootTargets {}

// Serialize root install and removal. Draws and finish stats can read the published address
// without taking this lock, and the native wrapper logs after the transition returns.
static REGISTRATION: Mutex<Option<RootTargets>> = Mutex::new(None);
static CURRENT_COMMAND: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

fn registration() -> MutexGuard<'static, Option<RootTargets>> {
    REGISTRATION
        .lock()
        .unwrap_or_else(|error| error.into_inner())
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_install_roots(
    base: *mut u8,
    effects: *const RootHookEffects,
) -> RootInstall {
    let mut registration = registration();
    if let Some(targets) = registration.as_ref() {
        return if targets.installed {
            RootInstall::AlreadyInstalled
        } else {
            RootInstall::PartialTeardown
        };
    }
    // SAFETY: The native wrapper passes its live table of synchronous capture leaves.
    let effects = unsafe { &*effects };
    if base.is_null() || !effects.matches(base.wrapping_add(RHI_WRITE_RVA), RHI_WRITE) {
        return RootInstall::RhiRefused;
    }

    // SAFETY: The leaf reads the native-owner flag. Use the same value to guard and bind.
    let native_owner = unsafe { (effects.native_owner)() };
    let mut targets = RootTargets {
        entries: [null_mut(); ROOT_SITES.len()],
        installed: false,
    };
    for (target, site) in targets.entries.iter_mut().zip(ROOT_SITES) {
        if native_owner && site.native_owned {
            continue;
        }
        let address = base.wrapping_add(site.rva);
        let expected = &site.expected[..site.prefix.unwrap_or(site.expected.len())];
        if !effects.matches(address, expected) {
            return RootInstall::EntryRefused;
        }
        *target = address.cast();
    }

    // SAFETY: All selected addresses passed the guarded reads. The native batch borrows
    // the live array, preserves the omitted slots and rolls back on failure.
    if !unsafe { (effects.install)(targets.entries.as_mut_ptr(), targets.entries.len() as u32) } {
        if targets.entries.iter().any(|target| !target.is_null()) {
            *registration = Some(targets);
            return RootInstall::PartialTeardown;
        }
        return RootInstall::RolledBack;
    }
    targets.installed = true;
    *registration = Some(targets);
    CURRENT_COMMAND.store(
        base.wrapping_add(CURRENT_COMMAND_RVA).cast(),
        Ordering::Release,
    );
    RootInstall::Installed
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_remove_roots(effects: *const RootHookEffects) -> bool {
    let mut registration = registration();
    if let Some(targets) = registration.as_mut() {
        // SAFETY: The native wrapper passes its live effects table. The leaf uses the exact
        // target array borrowed from the registration and skips omitted slots.
        unsafe { ((*effects).remove)(targets.entries.as_mut_ptr(), targets.entries.len() as u32) };
        CURRENT_COMMAND.store(null_mut(), Ordering::Release);
        targets.installed = false;
        if targets.entries.iter().any(|target| !target.is_null()) {
            return false;
        }
        *registration = None;
    }
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_current_command_address() -> *const c_void {
    CURRENT_COMMAND.load(Ordering::Acquire)
}
