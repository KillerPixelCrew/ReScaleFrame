// SPDX-License-Identifier: MIT
//! Observer hook installation, activation, and retryable disable. The session (`session.rs`)
//! owns the control record under its lock; native code keeps the typed MinHook trampolines.
use rsf_game_sys::{host_log, rsf_game_host_services};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::hook_set::HookSet;

const HOOKS: [&str; 5] = [
    "jitter",
    "process-main",
    "resolution",
    "renderer-end",
    "frame-upload",
];
#[derive(Default)]
pub(crate) struct Control {
    hooks: HookSet<{ HOOKS.len() }>,
    pinned: bool,
}

// The hook operations: the native MinHook leaves below, or the test fake.
#[cfg(test)]
use fake::{create, disable, enable, initialize, pin, remove};

impl Control {
    pub(crate) const fn new() -> Self {
        Self {
            hooks: HookSet::new(),
            pinned: false,
        }
    }

    /// Roll back only disabled hooks, retaining any whose removal refused.
    fn remove_created(&mut self) {
        self.hooks
            .remove((0..HOOKS.len()).rev(), |index| remove(index as u32));
    }

    pub(crate) fn prepare(&mut self) -> bool {
        if self.pinned {
            return self.hooks.first_unavailable().is_none();
        }
        self.remove_created();
        if !self.hooks.is_empty() || !initialize() {
            return false;
        }
        if !self.hooks.create(|index| create(index as u32)) {
            self.remove_created();
            return false;
        }
        if !pin() {
            self.remove_created();
            return false;
        }
        self.pinned = true;
        true
    }

    pub(crate) fn disable(&mut self, rollback: bool) -> bool {
        let count = HOOKS.len();
        let order = (0..count).map(|ordinal| {
            if rollback {
                count - 1 - ordinal
            } else {
                ordinal
            }
        });
        self.hooks.disable(order, |index| disable(index as u32))
    }

    pub(crate) fn start(&mut self, host: &rsf_game_host_services) -> bool {
        if !self.pinned || !self.hooks.is_complete() || !self.disable(true) {
            return false;
        }
        if let Err((index, status)) = self.hooks.enable(|index| enable(index as u32)) {
            let stage = HOOKS[index];
            host_log(
                host,
                &format!("Skyrim start refused at {stage}: MinHook status={status}"),
            );
            self.disable(true);
            return false;
        }
        true
    }
}

static CALLS: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_enter() {
    CALLS.fetch_add(1, Ordering::AcqRel);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_leave() {
    CALLS.fetch_sub(1, Ordering::AcqRel);
}

#[cfg(not(test))]
pub(crate) fn busy() -> bool {
    CALLS.load(Ordering::Acquire) != 0
}

// Admission and MinHook control run under the session lock. Trampolines are published
// before enable and remain callable until a successful removal of a disabled hook.
static TARGETS: [AtomicUsize; HOOKS.len()] = [const { AtomicUsize::new(0) }; HOOKS.len()];
static ORIGINALS: [AtomicPtr<c_void>; HOOKS.len()] =
    [const { AtomicPtr::new(ptr::null_mut()) }; HOOKS.len()];

unsafe extern "C" {
    fn rsf_skyrim_observe_jitter(state: *mut c_void);
    fn rsf_skyrim_observe_process(
        manager: *mut c_void,
        input: u32,
        output: u32,
        arg4: usize,
        arg5: u8,
    ) -> usize;
    fn rsf_skyrim_observe_resolution(state: *mut c_void);
    fn rsf_skyrim_observe_end() -> usize;
    fn rsf_skyrim_observe_upload(renderer: *mut c_void, flags: u8);
}

const _: () = assert!(HOOKS.len() == crate::sys::RSF_SKYRIM_OBSERVER_HOOK_COUNT as usize);

const DETOURS: [*const c_void; HOOKS.len()] = {
    use crate::sys::{
        RSF_SKYRIM_OBSERVER_END, RSF_SKYRIM_OBSERVER_JITTER, RSF_SKYRIM_OBSERVER_PROCESS,
        RSF_SKYRIM_OBSERVER_RESOLUTION, RSF_SKYRIM_OBSERVER_UPLOAD,
    };
    let mut detours = [ptr::null(); HOOKS.len()];
    detours[RSF_SKYRIM_OBSERVER_JITTER as usize] = rsf_skyrim_observe_jitter as *const c_void;
    detours[RSF_SKYRIM_OBSERVER_PROCESS as usize] = rsf_skyrim_observe_process as *const c_void;
    detours[RSF_SKYRIM_OBSERVER_RESOLUTION as usize] =
        rsf_skyrim_observe_resolution as *const c_void;
    detours[RSF_SKYRIM_OBSERVER_END as usize] = rsf_skyrim_observe_end as *const c_void;
    detours[RSF_SKYRIM_OBSERVER_UPLOAD as usize] = rsf_skyrim_observe_upload as *const c_void;
    detours
};

pub(crate) fn bind_targets(targets: [usize; HOOKS.len()]) {
    for (slot, target) in TARGETS.iter().zip(targets) {
        slot.store(target, Ordering::Relaxed);
    }
}

fn target(index: usize) -> *mut c_void {
    ptr::with_exposed_provenance_mut(TARGETS[index].load(Ordering::Relaxed))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observer_original(index: u32) -> *mut c_void {
    ORIGINALS[index as usize].load(Ordering::Acquire)
}

#[cfg(not(test))]
fn initialize() -> bool {
    rsf_windows::minhook::initialize()
}

#[cfg(not(test))]
fn create(index: u32) -> bool {
    let detour = DETOURS[index as usize];
    let mut original = ptr::null_mut();
    if !unsafe {
        rsf_windows::minhook::create(target(index as usize), detour.cast_mut(), &mut original)
    } {
        return false;
    }
    ORIGINALS[index as usize].store(original, Ordering::Release);
    true
}

#[cfg(not(test))]
fn remove(index: u32) -> bool {
    if !unsafe { rsf_windows::minhook::remove_idempotent(target(index as usize)) } {
        return false;
    }
    ORIGINALS[index as usize].store(ptr::null_mut(), Ordering::Release);
    true
}

#[cfg(not(test))]
fn enable(index: u32) -> i32 {
    unsafe { rsf_windows::minhook::enable_status(target(index as usize)) }
}

#[cfg(not(test))]
fn disable(index: u32) -> bool {
    unsafe { rsf_windows::minhook::disable(target(index as usize)) }
}

#[cfg(not(test))]
fn pin() -> bool {
    let address = DETOURS[0] as usize;
    crate::platform::Platform.pin_module(address) != 0
}

/// The test hook leaves: every operation succeeds; `ENABLE_REFUSES` names an index whose enable
/// fails and `DISABLE_REFUSES` makes hook 0 refuse disable. `DISABLED` counts disable calls.
#[cfg(test)]
pub(crate) mod fake {
    use std::cell::Cell;

    thread_local! {
        pub(crate) static ENABLE_REFUSES: Cell<Option<u32>> = const { Cell::new(None) };
        pub(crate) static DISABLE_REFUSES: Cell<bool> = const { Cell::new(false) };
        pub(crate) static DISABLED: Cell<u32> = const { Cell::new(0) };
    }

    pub(super) fn initialize() -> bool {
        true
    }
    pub(super) fn create(_: u32) -> bool {
        true
    }
    pub(super) fn remove(_: u32) -> bool {
        true
    }
    pub(super) fn enable(index: u32) -> i32 {
        i32::from(ENABLE_REFUSES.get() == Some(index))
    }
    pub(super) fn disable(index: u32) -> bool {
        DISABLED.set(DISABLED.get() + 1);
        index != 0 || !DISABLE_REFUSES.get()
    }
    pub(super) fn pin() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_start_keeps_hooks_whose_disable_refuses() {
        fake::ENABLE_REFUSES.set(Some(1));
        fake::DISABLE_REFUSES.set(true);
        let mut control = Control::default();
        // A zeroed services table has no log callback.
        let host = unsafe { std::mem::zeroed() };
        assert!(control.prepare());
        assert!(!control.start(&host));
        assert_eq!(control.hooks.first_unavailable(), Some(0));
        assert!(!control.disable(false));
        fake::DISABLE_REFUSES.set(false);
        assert!(control.disable(false));
        assert!(control.hooks.is_disabled());
    }
}
