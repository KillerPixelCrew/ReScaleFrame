// SPDX-License-Identifier: MIT
use crate::d3d11_observer_callbacks::snapshot;
use crate::d3d11_observer_hooks::rsf_core_observer11_installed;
use crate::scene_abi::rsf_observer_present_event as Event;
use crate::sr_session::has_capacity;
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

type Handle = *mut c_void;
type Update = unsafe extern "C" fn(Handle);
const TEST: u32 = 1;
const INVALID: i32 = -1;
const OK: i32 = 0;

#[repr(C)]
struct Platform {
    is_d3d11: unsafe extern "C" fn(Handle) -> i32,
    update: Update,
    forward: unsafe extern "C" fn(&Event, *const c_void, u32) -> i32,
}

const _: () = assert!(size_of::<Platform>() == 3 * size_of::<Handle>());

#[derive(Clone, Copy, PartialEq, Eq)]
struct ApplicationCall {
    chain: usize,
    method: u32,
    flags: u32,
}

static CALLS: AtomicU32 = AtomicU32::new(0);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static SIZE: AtomicU64 = AtomicU64::new(0);
static REENTRY_WARNED: AtomicBool = AtomicBool::new(false);
static APPLICATION: Mutex<Vec<ApplicationCall>> = Mutex::new(Vec::new());

thread_local! {
    static IN_HOOK: Cell<bool> = const { Cell::new(false) };
}

struct HookCall;

impl HookCall {
    fn begin() -> Self {
        CALLS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for HookCall {
    fn drop(&mut self) {
        CALLS.fetch_sub(1, Ordering::SeqCst);
    }
}

struct HookScope;

impl HookScope {
    fn enter() -> Option<Self> {
        if IN_HOOK.with(|flag| flag.replace(true)) {
            None
        } else {
            Some(Self)
        }
    }
}

impl Drop for HookScope {
    fn drop(&mut self) {
        IN_HOOK.with(|flag| flag.set(false));
    }
}

fn count_present(flags: u32, result: i32) {
    if flags & TEST == 0 && result == OK {
        FRAMES.fetch_add(1, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_present(
    event: &mut Event,
    parameters: *const c_void,
    platform: &Platform,
) -> i32 {
    let _call = HookCall::begin();
    if unsafe { (platform.is_d3d11)(event.swapchain) } == 0 {
        return unsafe { (platform.forward)(event, parameters, 0) };
    }
    let registration = (rsf_core_observer11_installed() != 0)
        .then(snapshot)
        .flatten();
    let Some(_scope) = HookScope::enter() else {
        if let Some(registration) = registration
            && !REENTRY_WARNED.swap(true, Ordering::Relaxed)
            && !registration.say(c"observer: Present re-entered from inside its own hook, so another hook forwards to this one. The inner call goes straight to the genuine Present where that is known, and is refused otherwise")
        {
            REENTRY_WARNED.store(false, Ordering::Relaxed);
        }
        return unsafe { (platform.forward)(event, parameters, 1) };
    };
    let Some(registration) = registration else {
        return unsafe { (platform.forward)(event, parameters, 0) };
    };

    registration.event(event);
    if event.flags & TEST == 0 {
        unsafe { (platform.update)(event.swapchain) };
        registration.present(event.swapchain);
    }
    let result = unsafe { (platform.forward)(event, parameters, 0) };
    count_present(event.flags, result);
    event.completed = 1;
    event.result = result;
    registration.event(event);
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_application_present(
    event: *const Event,
    update: Update,
) -> i32 {
    if !unsafe { has_capacity(event) } {
        return INVALID;
    }
    let event = unsafe { &*event };
    if event.swapchain.is_null() || event.completed > 1 {
        return INVALID;
    }
    let key = ApplicationCall {
        chain: event.swapchain.addr(),
        method: event.method,
        flags: event.flags,
    };
    if event.completed == 0 {
        {
            let mut calls = APPLICATION.lock().unwrap_or_else(PoisonError::into_inner);
            calls.push(key);
            CALLS.fetch_add(1, Ordering::SeqCst);
        }
        if event.flags & TEST == 0 {
            unsafe { update(event.swapchain) };
        }
    } else {
        let mut calls = APPLICATION.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(index) = calls.iter().rposition(|call| *call == key) else {
            return INVALID;
        };
        calls.swap_remove(index);
        count_present(event.flags, event.result);
        CALLS.fetch_sub(1, Ordering::SeqCst);
    }
    OK
}

pub(super) fn present_calls() -> u32 {
    CALLS.load(Ordering::SeqCst)
}

pub(super) fn frames() -> u32 {
    FRAMES.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_core_observer11_present_size() -> u64 {
    SIZE.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_core_observer11_replace_size(width: u32, height: u32) -> u64 {
    SIZE.swap(
        (u64::from(width) << 32) | u64::from(height),
        Ordering::AcqRel,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmatched_application_completion_keeps_the_real_call_live() {
        unsafe extern "C" fn update(_: Handle) {}
        let mut event = Event {
            struct_size: size_of::<Event>() as u32,
            completed: 1,
            swapchain: std::ptr::dangling_mut::<u8>().cast(),
            sync_interval: 0,
            flags: TEST,
            result: OK,
            method: 0,
        };
        let notify =
            |event: &Event| unsafe { rsf_core_observer11_application_present(event, update) };
        let before = present_calls();
        assert_eq!(notify(&event), INVALID);
        assert_eq!(present_calls(), before);
        event.completed = 0;
        assert_eq!(notify(&event), OK);
        event.completed = 1;
        event.method = 1;
        assert_eq!(notify(&event), INVALID);
        assert_eq!(present_calls(), before + 1);
        event.method = 0;
        assert_eq!(notify(&event), OK);
        assert_eq!(present_calls(), before);
    }
}
