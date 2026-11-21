// SPDX-License-Identifier: MIT
mod probe;

use crate::d3d11_observer_abi::rsf_observer_options as Options;
use crate::d3d11_observer_callbacks::Callbacks;
use crate::d3d11_observer_present::{present_calls, rsf_core_observer11_replace_size};
use crate::{d3d11_observer_callbacks, d3d11_observer_capture};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};

type Handle = *mut c_void;
const TEXTURE: usize = 0;
const BUFFER: usize = 1;
const PRESENT: usize = 2;
const LAYOUT: usize = 3;
const STAGE_BASE: usize = 4;
const SLOT_COUNT: usize = 11;
const OK: i32 = 0;
const ALREADY_INSTALLED: i32 = -3;
const PATCH_FAILED: i32 = -5;
const NOT_READY: i32 = -6;

#[repr(C)]
#[derive(Clone, Copy)]
struct Slot {
    address: *mut Handle,
    hook: Handle,
}

const EMPTY_SLOT: Slot = Slot {
    address: null_mut(),
    hook: null_mut(),
};
type Probe = unsafe extern "C" fn(Handle, &mut [Slot; SLOT_COUNT], &mut u32) -> i32;

#[repr(C)]
struct Platform {
    read: unsafe extern "C" fn(*mut Handle) -> Handle,
    replace: unsafe extern "C" fn(*mut Handle, Handle, Handle) -> i32,
    detour: unsafe extern "C" fn(Handle, u32) -> i32,
    remove: unsafe extern "C" fn(Handle) -> i32,
}

const _: () = {
    assert!(size_of::<Slot>() == 2 * size_of::<Handle>());
    assert!(size_of::<Platform>() == 4 * size_of::<Handle>());
};

struct Route {
    target: AtomicPtr<c_void>,
    chain: AtomicPtr<c_void>,
    genuine: AtomicPtr<c_void>,
}

impl Route {
    const fn new() -> Self {
        Self {
            target: AtomicPtr::new(null_mut()),
            chain: AtomicPtr::new(null_mut()),
            genuine: AtomicPtr::new(null_mut()),
        }
    }

    fn publish(&self, target: Handle, chain: Handle, genuine: Handle) {
        if chain.is_null() {
            self.chain.store(null_mut(), Ordering::Release);
            self.target.store(target, Ordering::Relaxed);
            self.genuine.store(genuine, Ordering::Relaxed);
        } else {
            self.target.store(target, Ordering::Relaxed);
            self.genuine.store(genuine, Ordering::Relaxed);
            // Native code can enter immediately after MinHook enables the target.
            self.chain.store(chain, Ordering::Release);
        }
    }
}

struct Hooks {
    patches: [Option<Slot>; SLOT_COUNT],
}

// Only the lifecycle lock reads slot addresses and changes hook ownership.
unsafe impl Send for Hooks {}

static HOOKS: Mutex<Hooks> = Mutex::new(Hooks {
    patches: [None; SLOT_COUNT],
});
static ACTIVE: AtomicBool = AtomicBool::new(false);
static ORIGINALS: [AtomicPtr<c_void>; SLOT_COUNT] =
    [const { AtomicPtr::new(null_mut()) }; SLOT_COUNT];
static ROUTES: [Route; 2] = [const { Route::new() }; 2];

fn lifecycle() -> Option<MutexGuard<'static, Hooks>> {
    if d3d11_observer_callbacks::in_creation_callback() {
        return None;
    }
    match HOOKS.try_lock() {
        Ok(state) => Some(state),
        Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

impl Hooks {
    fn has_patches(&self) -> bool {
        self.patches.iter().any(Option::is_some)
            || ROUTES
                .iter()
                .any(|route| !route.target.load(Ordering::Relaxed).is_null())
    }

    fn patch(&mut self, platform: &Platform, index: usize, slot: Slot) -> bool {
        if slot.address.is_null() {
            return false;
        }
        let original = unsafe { (platform.read)(slot.address) };
        if original.is_null() || original == slot.hook {
            return false;
        }
        ORIGINALS[index].store(original, Ordering::Release);
        if unsafe { (platform.replace)(slot.address, original, slot.hook) } == 0 {
            ORIGINALS[index].store(null_mut(), Ordering::Release);
            return false;
        }
        self.patches[index] = Some(slot);
        true
    }

    fn retire(&mut self, platform: &Platform) -> bool {
        ACTIVE.store(false, Ordering::Release);
        d3d11_observer_callbacks::clear();
        for (index, patch) in self.patches.iter_mut().enumerate().rev() {
            let Some(Slot { address, hook }) = *patch else {
                continue;
            };
            let original = ORIGINALS[index].load(Ordering::Relaxed);
            let current = unsafe { (platform.read)(address) };
            if current == original
                || (current == hook && unsafe { (platform.replace)(address, hook, original) } != 0)
            {
                *patch = None;
                ORIGINALS[index].store(null_mut(), Ordering::Release);
            }
        }
        for route in ROUTES.iter().rev() {
            let target = route.target.load(Ordering::Relaxed);
            if !target.is_null() && unsafe { (platform.remove)(target) } != 0 {
                // The genuine trampoline page is process-lifetime, as before.
                route.publish(null_mut(), null_mut(), null_mut());
            }
        }
        !self.has_patches()
    }

    fn install(
        &mut self,
        slots: &[Slot; SLOT_COUNT],
        has_present1: u32,
        user: Handle,
        platform: &Platform,
    ) -> i32 {
        let required = self.patch(platform, TEXTURE, slots[TEXTURE])
            && (slots[BUFFER].address.is_null() || self.patch(platform, BUFFER, slots[BUFFER]))
            && (unsafe { (platform.detour)(user, 0) } != 0
                || (ROUTES[0].target.load(Ordering::Relaxed).is_null()
                    && self.patch(platform, PRESENT, slots[PRESENT])));
        if !required {
            if self.retire(platform) {
                d3d11_observer_capture::clear();
            }
            return PATCH_FAILED;
        }
        for (index, &slot) in slots.iter().enumerate().skip(LAYOUT) {
            self.patch(platform, index, slot);
        }
        if has_present1 != 0 {
            unsafe { (platform.detour)(user, 1) };
        }
        ACTIVE.store(true, Ordering::Release);
        OK
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_install(
    options: *const Options,
    callbacks: &'static Callbacks,
    user: Handle,
    probe: Probe,
    platform: &Platform,
) -> i32 {
    let validated = unsafe { d3d11_observer_callbacks::validate(options) };
    if validated != OK {
        return validated;
    }
    let Some(mut state) = lifecycle() else {
        return NOT_READY;
    };
    if state.has_patches() {
        return ALREADY_INSTALLED;
    }

    let mut slots = [EMPTY_SLOT; SLOT_COUNT];
    let mut has_present1 = 0;
    let result = unsafe { probe(user, &mut slots, &mut has_present1) };
    if result != OK {
        return result;
    }
    let options = unsafe { &*options };
    if options.constant_buffer_max_bytes == 0 {
        slots[BUFFER] = EMPTY_SLOT;
    }
    if options.on_layout.is_none() {
        slots[LAYOUT] = EMPTY_SLOT;
    }
    if options.on_shader.is_none() {
        slots[STAGE_BASE..].fill(EMPTY_SLOT);
    }
    d3d11_observer_callbacks::configure(options, callbacks);
    state.install(&slots, has_present1, user, platform)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_uninstall_hooks(platform: &Platform) -> i32 {
    let Some(mut state) = lifecycle() else {
        return NOT_READY;
    };
    if !state.has_patches() || present_calls() != 0 {
        return NOT_READY;
    }
    if !state.retire(platform) {
        return PATCH_FAILED;
    }
    d3d11_observer_capture::clear();
    rsf_core_observer11_replace_size(0, 0);
    OK
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_core_observer11_installed() -> i32 {
    i32::from(ACTIVE.load(Ordering::Acquire))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_original(slot: usize) -> Handle {
    ORIGINALS[slot].load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_forward(method: u32, nested: u32) -> Handle {
    let route = &ROUTES[method as usize];
    let chain = route.chain.load(Ordering::Acquire);
    if nested != 0 {
        if chain.is_null() {
            null_mut()
        } else {
            route.genuine.load(Ordering::Relaxed)
        }
    } else if method == 0 && chain.is_null() {
        rsf_core_observer11_original(PRESENT)
    } else {
        chain
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_publish_route(
    method: u32,
    target: Handle,
    chain: Handle,
    genuine: Handle,
) {
    ROUTES[method as usize].publish(target, chain, genuine);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::without_provenance_mut;

    const REFUSED: usize = 40;

    unsafe extern "C" fn read(address: *mut Handle) -> Handle {
        unsafe { address.read() }
    }

    unsafe extern "C" fn replace(
        address: *mut Handle,
        expected: Handle,
        replacement: Handle,
    ) -> i32 {
        if replacement.addr() == REFUSED || unsafe { address.read() } != expected {
            return 0;
        }
        unsafe { address.write(replacement) };
        1
    }

    unsafe extern "C" fn detour(_: Handle, _: u32) -> i32 {
        assert_eq!(rsf_core_observer11_uninstall_hooks(&PLATFORM), NOT_READY);
        0
    }
    unsafe extern "C" fn remove(_: Handle) -> i32 {
        1
    }

    static PLATFORM: Platform = Platform {
        read,
        replace,
        detour,
        remove,
    };

    #[test]
    fn required_present_failure_restores_both_creation_hooks() {
        let mut state = lifecycle().unwrap();
        let original = without_provenance_mut(11);
        let mut table = [original; 3];
        let mut slots = [Slot {
            address: null_mut(),
            hook: null_mut(),
        }; SLOT_COUNT];
        for (index, hook) in [20, 30, REFUSED].into_iter().enumerate() {
            slots[index] = Slot {
                address: &mut table[index],
                hook: without_provenance_mut(hook),
            };
        }
        assert_eq!(
            state.install(&slots, 0, null_mut(), &PLATFORM),
            PATCH_FAILED
        );
        assert_eq!(table, [original; 3]);
        assert!(!state.has_patches());
        assert!(rsf_core_observer11_original(TEXTURE).is_null());
        assert!(rsf_core_observer11_original(BUFFER).is_null());
    }
}
