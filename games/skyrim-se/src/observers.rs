// SPDX-License-Identifier: MIT
//! Observer hook installation, activation, and retryable disable. Native code serializes calls
//! with the lifecycle mutex and keeps typed MinHook trampolines.
use rsf_game_sys::rsf_game_host_services;
use std::ffi::{CString, c_char};

const HOOKS: [&str; 5] = [
    "jitter",
    "process-main",
    "resolution",
    "renderer-end",
    "frame-upload",
];
const ALL: u32 = (1 << HOOKS.len()) - 1;

#[repr(C)]
#[derive(Default)]
struct Control {
    created: u32,
    active: u32,
    pinned: u32,
}

#[repr(C)]
struct Leaves {
    initialize: unsafe extern "C" fn() -> bool,
    create: unsafe extern "C" fn(u32) -> bool,
    remove: unsafe extern "C" fn(u32) -> bool,
    enable: unsafe extern "C" fn(u32) -> i32,
    disable: unsafe extern "C" fn(u32) -> bool,
    pin: unsafe extern "C" fn() -> bool,
    log: unsafe extern "C" fn(*const rsf_game_host_services, *const c_char),
}

const _: () = {
    assert!(size_of::<Control>() == 12);
    assert!(size_of::<Leaves>() == 56);
};

impl Control {
    /// Roll back only disabled hooks, retaining any whose removal refused.
    fn remove_created(&mut self, leaves: &Leaves) {
        for index in (0..HOOKS.len() as u32).rev() {
            let bit = 1 << index;
            if self.created & bit != 0 && unsafe { (leaves.remove)(index) } {
                self.created &= !bit;
            }
        }
    }

    fn prepare(&mut self, leaves: &Leaves) -> bool {
        if self.pinned != 0 {
            return self.created == ALL && self.active == 0;
        }
        self.remove_created(leaves);
        if self.created != 0 || !unsafe { (leaves.initialize)() } {
            return false;
        }
        for index in 0..HOOKS.len() as u32 {
            if !unsafe { (leaves.create)(index) } {
                self.remove_created(leaves);
                return false;
            }
            self.created |= 1 << index;
        }
        if !unsafe { (leaves.pin)() } {
            self.remove_created(leaves);
            return false;
        }
        self.pinned = 1;
        true
    }

    fn disable(&mut self, leaves: &Leaves, rollback: bool) -> bool {
        let count = HOOKS.len() as u32;
        for ordinal in 0..count {
            let index = if rollback {
                count - 1 - ordinal
            } else {
                ordinal
            };
            let bit = 1 << index;
            if self.active & bit != 0 && unsafe { (leaves.disable)(index) } {
                self.active &= !bit;
            }
        }
        self.active == 0
    }

    fn start(&mut self, leaves: &Leaves, host: &rsf_game_host_services) -> bool {
        if self.pinned == 0 || self.created != ALL || !self.disable(leaves, true) {
            return false;
        }
        for (index, stage) in HOOKS.into_iter().enumerate() {
            let status = unsafe { (leaves.enable)(index as u32) };
            if status != 0 {
                if let Ok(message) = CString::new(format!(
                    "Skyrim start refused at {stage}: MinHook status={status}"
                )) {
                    unsafe { (leaves.log)(host, message.as_ptr()) };
                }
                self.disable(leaves, true);
                return false;
            }
            self.active |= 1 << index;
        }
        true
    }
}

// The C facade lends these tables for the call and exclusively lends Control.
#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observers_prepare(control: &mut Control, leaves: &Leaves) -> bool {
    control.prepare(leaves)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observers_start(
    control: &mut Control,
    leaves: &Leaves,
    host: &rsf_game_host_services,
) -> bool {
    control.start(leaves, host)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_observers_disable(
    control: &mut Control,
    leaves: &Leaves,
    rollback: bool,
) -> bool {
    control.disable(leaves, rollback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static DISABLE_REFUSES: Cell<bool> = const { Cell::new(true) };
    }

    extern "C" fn yes() -> bool {
        true
    }

    extern "C" fn hook_yes(_: u32) -> bool {
        true
    }

    extern "C" fn enable(index: u32) -> i32 {
        i32::from(index == 1)
    }

    extern "C" fn disable(index: u32) -> bool {
        index != 0 || !DISABLE_REFUSES.get()
    }

    extern "C" fn log(_: *const rsf_game_host_services, _: *const c_char) {}

    #[test]
    fn a_failed_start_keeps_hooks_whose_disable_refuses() {
        let leaves = Leaves {
            initialize: yes,
            create: hook_yes,
            remove: hook_yes,
            enable,
            disable,
            pin: yes,
            log,
        };
        let mut control = Control::default();
        // The fake log does not read the host. Zero is an empty GameSDK services table.
        let host = unsafe { std::mem::zeroed() };
        assert!(control.prepare(&leaves));
        assert!(!control.start(&leaves, &host));
        assert_eq!(control.active, 1);
        assert!(!control.disable(&leaves, false));
        DISABLE_REFUSES.set(false);
        assert!(control.disable(&leaves, false));
        assert_eq!(control.active, 0);
    }
}
