// SPDX-License-Identifier: MIT
mod detours;
mod probe;
mod route;

use crate::d3d11_observer_callbacks::Callbacks;
use crate::d3d11_observer_present::{present_calls, replace_present_size};
use crate::sys::rsf_observer_options as Options;
use crate::{d3d11_observer_callbacks, d3d11_observer_capture};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};

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
const NO_DEVICE: i32 = -4;

// Production reads and patches the live vtable slots and routes through `route`; unit tests call
// same-named fakes.
#[cfg(not(test))]
mod leaf {
    pub(super) use super::read_slot as read;
    pub(super) use super::route::{detour, remove};
    pub(super) use super::rsf_graphics_replace_vtable_slot as replace;
}
#[cfg(test)]
use tests as leaf;

const _: () = {
    assert!(size_of::<Slot>() == 2 * size_of::<Handle>());
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
    crate::sync::try_lock(&HOOKS)
}

impl Hooks {
    fn has_patches(&self) -> bool {
        self.patches.iter().any(Option::is_some)
            || ROUTES
                .iter()
                .any(|route| !route.target.load(Ordering::Relaxed).is_null())
    }

    fn patch(&mut self, index: usize, slot: Slot) -> bool {
        if slot.address.is_null() {
            return false;
        }
        let original = unsafe { leaf::read(slot.address) };
        if original.is_null() || original == slot.hook {
            return false;
        }
        ORIGINALS[index].store(original, Ordering::Release);
        if unsafe { leaf::replace(slot.address, original, slot.hook) } == 0 {
            ORIGINALS[index].store(null_mut(), Ordering::Release);
            return false;
        }
        self.patches[index] = Some(slot);
        true
    }

    fn retire(&mut self) -> bool {
        ACTIVE.store(false, Ordering::Release);
        d3d11_observer_callbacks::clear();
        for (index, patch) in self.patches.iter_mut().enumerate().rev() {
            let Some(Slot { address, hook }) = *patch else {
                continue;
            };
            let original = ORIGINALS[index].load(Ordering::Relaxed);
            let current = unsafe { leaf::read(address) };
            if current == original
                || (current == hook && unsafe { leaf::replace(address, hook, original) } != 0)
            {
                *patch = None;
                ORIGINALS[index].store(null_mut(), Ordering::Release);
            }
        }
        for route in ROUTES.iter().rev() {
            let target = route.target.load(Ordering::Relaxed);
            if !target.is_null() && unsafe { leaf::remove(target) } != 0 {
                // The genuine trampoline page is process-lifetime, as before.
                route.publish(null_mut(), null_mut(), null_mut());
            }
        }
        !self.has_patches()
    }

    fn install(&mut self, slots: &[Slot; SLOT_COUNT], has_present1: u32, user: Handle) -> i32 {
        let required = self.patch(TEXTURE, slots[TEXTURE])
            && (slots[BUFFER].address.is_null() || self.patch(BUFFER, slots[BUFFER]))
            && (unsafe { leaf::detour(user, 0) } != 0
                || (ROUTES[0].target.load(Ordering::Relaxed).is_null()
                    && self.patch(PRESENT, slots[PRESENT])));
        if !required {
            if self.retire() {
                d3d11_observer_capture::clear();
            }
            return PATCH_FAILED;
        }
        for (index, &slot) in slots.iter().enumerate().skip(LAYOUT) {
            self.patch(index, slot);
        }
        if has_present1 != 0 {
            unsafe { leaf::detour(user, 1) };
        }
        ACTIVE.store(true, Ordering::Release);
        OK
    }
}

#[cfg_attr(test, allow(dead_code))]
unsafe extern "C" fn rsf_graphics_replace_vtable_slot(
    address: *mut Handle,
    expected: Handle,
    replacement: Handle,
) -> i32 {
    let mut protection = 0;
    if unsafe {
        VirtualProtect(
            address.cast(),
            size_of::<Handle>(),
            PAGE_EXECUTE_READWRITE,
            &mut protection,
        )
    } == 0
    {
        return 0;
    }
    // The aligned slot belongs to a live table. Keep the expected-value check and
    // the write in one operation so a third-party replacement wins on contention.
    let slot = unsafe { AtomicPtr::from_ptr(address) };
    let changed = slot
        .compare_exchange(expected, replacement, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok();
    let mut restored = 0;
    unsafe {
        VirtualProtect(
            address.cast(),
            size_of::<Handle>(),
            protection,
            &mut restored,
        )
    };
    i32::from(changed)
}

unsafe extern "C" {
    static rsf_observer11_callback_guards: Callbacks;
}

#[cfg_attr(test, allow(dead_code))]
unsafe extern "C" fn read_slot(address: *mut Handle) -> Handle {
    unsafe { AtomicPtr::from_ptr(address) }.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_observer_install(options: *const Options) -> i32 {
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
    let Some(routes) = probe::native_slots(&mut slots) else {
        return NO_DEVICE;
    };
    let has_present1 = u32::from(!routes[1].table.is_null());
    let mut setup = route::Setup::new(routes);
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
    d3d11_observer_callbacks::configure(options, unsafe { &rsf_observer11_callback_guards });
    state.install(&slots, has_present1, std::ptr::from_mut(&mut setup).cast())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_observer_uninstall() -> i32 {
    uninstall()
}

fn uninstall() -> i32 {
    let Some(mut state) = lifecycle() else {
        return NOT_READY;
    };
    if !state.has_patches() || present_calls() != 0 {
        return NOT_READY;
    }
    if !state.retire() {
        return PATCH_FAILED;
    }
    d3d11_observer_capture::clear();
    replace_present_size(0, 0);
    OK
}

pub(super) fn installed() -> i32 {
    i32::from(ACTIVE.load(Ordering::Acquire))
}

fn original(slot: usize) -> Handle {
    ORIGINALS[slot].load(Ordering::Acquire)
}

pub(super) fn forward_target(method: u32, nested: u32) -> Handle {
    let route = &ROUTES[method as usize];
    let chain = route.chain.load(Ordering::Acquire);
    if nested != 0 {
        if chain.is_null() {
            null_mut()
        } else {
            route.genuine.load(Ordering::Relaxed)
        }
    } else if method == 0 && chain.is_null() {
        original(PRESENT)
    } else {
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::without_provenance_mut;

    const REFUSED: usize = 40;

    pub(super) unsafe extern "C" fn read(address: *mut Handle) -> Handle {
        unsafe { address.read() }
    }

    pub(super) unsafe extern "C" fn replace(
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

    pub(super) unsafe extern "C" fn detour(_: Handle, _: u32) -> i32 {
        assert_eq!(uninstall(), NOT_READY);
        0
    }
    pub(super) unsafe extern "C" fn remove(_: Handle) -> i32 {
        1
    }

    #[test]
    fn required_present_failure_restores_both_creation_hooks() {
        let mut state = lifecycle().unwrap();
        let native = without_provenance_mut(11);
        let mut table = [native; 3];
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
        assert_eq!(state.install(&slots, 0, null_mut()), PATCH_FAILED);
        assert_eq!(table, [native; 3]);
        assert!(!state.has_patches());
        assert!(original(TEXTURE).is_null());
        assert!(original(BUFFER).is_null());
    }
}
