// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_observer_callbacks::snapshot;
use crate::d3d11_observer_capture::{adopt_device, dump_presenter, needs_presenter};
use crate::d3d11_observer_hooks::{forward_target, installed};
use crate::sr_session::has_capacity;
use crate::sync::lock;
use crate::sys::rsf_observer_present_event as Event;
use rsf_game_sys::backend_result::{INVALID_ARGUMENT, OK};
use std::cell::Cell;
use std::ffi::{CString, c_void};
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

type Handle = *mut c_void;

const TEST: u32 = 1;

#[repr(C)]
pub(super) struct Platform {
    describe: unsafe extern "C" fn(Handle, &mut u32, &mut u32) -> i32,
    device: unsafe extern "C" fn(Handle, &mut Handle) -> i32,
    forward: unsafe extern "C" fn(&Event, *const c_void, Handle) -> i32,
}

unsafe extern "C" {
    pub(super) static rsf_observer11_present_platform: Platform;
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

impl Platform {
    fn d3d11_device(&self, chain: Handle) -> Option<ComRef> {
        let mut out = null_mut();
        let result = unsafe { (self.device)(chain, &mut out) };
        unsafe { ComRef::from_owned(out) }.filter(|_| result >= 0)
    }

    fn forward(&self, event: &Event, parameters: *const c_void, nested: u32) -> i32 {
        let target = forward_target(event.method, nested);
        if target.is_null() {
            return if nested != 0 {
                OK
            } else {
                0x8000_4005u32 as i32
            };
        }
        unsafe { (self.forward)(event, parameters, target) }
    }
}

fn say(message: String) {
    if let Some(registration) = snapshot()
        && let Ok(message) = CString::new(message)
    {
        registration.say(&message);
    }
}

fn update_presenter(chain: Handle, device: Option<ComRef>, platform: &Platform) {
    // Resize can follow the first frame. Refresh the swap chain dimensions every Present.
    let (mut width, mut height) = (0, 0);
    if unsafe { (platform.describe)(chain, &mut width, &mut height) } != 0 && width != 0 {
        let current = (u64::from(width) << 32) | u64::from(height);
        if present_size() != current {
            let previous = replace_present_size(width, height);
            let (previous_width, previous_height) = ((previous >> 32) as u32, previous as u32);
            if previous_width != 0 && previous != current {
                say(format!(
                    "observer: the swap chain is now {width}x{height}, was {previous_width}x{previous_height}"
                ));
            }
        }
    }
    if !needs_presenter() {
        return;
    }
    if let Some(device) = device.or_else(|| platform.d3d11_device(chain)) {
        if let Some(previous) = adopt_device(device.clone()) {
            say(format!(
                "observer: adopted presenting device {:p}, replacing provisional device {:p}",
                device.as_ptr(),
                previous.as_ptr(),
            ));
        }
        dump_presenter(&device);
    }
}

pub(super) unsafe fn present(
    event: &mut Event,
    parameters: *const c_void,
    platform: &Platform,
) -> i32 {
    let _call = HookCall::begin();
    // D3D12 Presents share DXGI and bypass D3D11 callbacks.
    let Some(device) = platform.d3d11_device(event.swapchain) else {
        return platform.forward(event, parameters, 0);
    };
    let registration = (installed() != 0).then(snapshot).flatten();
    let Some(_scope) = HookScope::enter() else {
        if let Some(registration) = registration
            && !REENTRY_WARNED.swap(true, Ordering::Relaxed)
            && !registration.say(c"observer: Present re-entered from inside its own hook, so another hook forwards to this one. The inner call goes straight to the genuine Present where that is known, and is refused otherwise")
        {
            REENTRY_WARNED.store(false, Ordering::Relaxed);
        }
        return platform.forward(event, parameters, 1);
    };
    let Some(registration) = registration else {
        return platform.forward(event, parameters, 0);
    };

    registration.event(event);
    if event.flags & TEST == 0 {
        update_presenter(event.swapchain, Some(device), platform);
        registration.present(event.swapchain);
    }
    let result = platform.forward(event, parameters, 0);
    count_present(event.flags, result);
    event.completed = 1;
    event.result = result;
    registration.event(event);
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_observer_notify_application_present(event: *const Event) -> i32 {
    unsafe {
        application_present(event, |chain| {
            update_presenter(chain, None, &rsf_observer11_present_platform)
        })
    }
}

unsafe fn application_present(event: *const Event, update: impl FnOnce(Handle)) -> i32 {
    if !unsafe { has_capacity(event) } {
        return INVALID_ARGUMENT;
    }
    let event = unsafe { &*event };
    if event.swapchain.is_null() || event.completed > 1 {
        return INVALID_ARGUMENT;
    }
    let key = ApplicationCall {
        chain: event.swapchain.addr(),
        method: event.method,
        flags: event.flags,
    };
    if event.completed == 0 {
        {
            let mut calls = lock(&APPLICATION);
            calls.push(key);
            CALLS.fetch_add(1, Ordering::SeqCst);
        }
        if event.flags & TEST == 0 {
            update(event.swapchain);
        }
    } else {
        let mut calls = lock(&APPLICATION);
        let Some(index) = calls.iter().rposition(|call| *call == key) else {
            return INVALID_ARGUMENT;
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

pub(super) fn present_size() -> u64 {
    SIZE.load(Ordering::Acquire)
}

pub(super) fn replace_present_size(width: u32, height: u32) -> u64 {
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
        let mut event = Event {
            struct_size: size_of::<Event>() as u32,
            completed: 1,
            swapchain: std::ptr::dangling_mut::<u8>().cast(),
            sync_interval: 0,
            flags: TEST,
            result: OK,
            method: 0,
        };
        let notify = |event: &Event| unsafe { application_present(event, |_| {}) };
        let before = present_calls();
        assert_eq!(notify(&event), INVALID_ARGUMENT);
        assert_eq!(present_calls(), before);
        event.completed = 0;
        assert_eq!(notify(&event), OK);
        event.completed = 1;
        event.method = 1;
        assert_eq!(notify(&event), INVALID_ARGUMENT);
        assert_eq!(present_calls(), before + 1);
        event.method = 0;
        assert_eq!(notify(&event), OK);
        assert_eq!(present_calls(), before);
    }
}
