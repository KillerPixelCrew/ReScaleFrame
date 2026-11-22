// SPDX-License-Identifier: MIT
use crate::sr_session::has_capacity;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

pub(super) mod attachment;
mod detours;
pub(super) mod diagnostics;
mod foreign;

type Handle = *mut c_void;
const KINDS: usize = 4;
const CAPACITY: usize = 8;

pub(crate) use crate::sys::rsf_d3d12_observer_setup as ObserverSetup;

// The graphics target links the same MinHook instance as D3D11.
use rsf_minhook_sys::{MH_CreateHook, MH_EnableHook, MH_Initialize, MH_RemoveHook};

const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;

const _: () = assert!(
    size_of::<ObserverSetup>() == 48 && std::mem::offset_of!(ObserverSetup, swapchain) == 40
);

pub(super) struct Observer {
    setup: ObserverSetup,
    serial: AtomicU64,
}

// The process-lifetime setup is copied at creation. Its callbacks and user
// context are supplied for invocation on the observed COM methods' threads.
unsafe impl Send for Observer {}
unsafe impl Sync for Observer {}

impl Observer {
    fn accepts_window(&self, window: Handle) -> bool {
        let accept = self.setup.accept_window.expect("validated observer setup");
        unsafe { accept(self.setup.user, window) != 0 }
    }

    fn notify_submit(&self, queue: Handle, id: u64, count: u32, lists: *const Handle, begin: u32) {
        if let Some(submit) = self.setup.submit {
            unsafe { submit(self.setup.user, queue, id, count, lists, begin) };
        }
    }
}

struct Binding {
    target: AtomicPtr<c_void>,
    original: AtomicPtr<c_void>,
}

impl Binding {
    const fn new() -> Self {
        Self {
            target: AtomicPtr::new(ptr::null_mut()),
            original: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

static OBSERVER: OnceLock<Observer> = OnceLock::new();
static INSTALL: Mutex<()> = Mutex::new(());
static BINDINGS: [[Binding; CAPACITY]; KINDS] =
    [const { [const { Binding::new() }; CAPACITY] }; KINDS];

thread_local! {
    static DEPTH: Cell<[u32; KINDS]> = const { Cell::new([0; KINDS]) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d12_observer_create(
    setup: *const ObserverSetup,
    out: *mut *mut Observer,
) -> i32 {
    if out.is_null() || !unsafe { has_capacity(setup) } {
        return -1;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != 1 || setup.accept_window.is_none() {
        return -1;
    }
    unsafe { out.write(ptr::null_mut()) };
    let _install = INSTALL.lock().unwrap_or_else(PoisonError::into_inner);
    if OBSERVER.get().is_some() {
        return -2;
    }
    if !matches!(
        unsafe { MH_Initialize() },
        MH_OK | MH_ERROR_ALREADY_INITIALIZED
    ) {
        return -3;
    }
    let observer = OBSERVER.get_or_init(|| Observer {
        setup: *setup,
        serial: AtomicU64::new(0),
    });
    unsafe { out.write(ptr::from_ref(observer).cast_mut()) };
    0
}

unsafe fn bind(kind: u32, target: Handle, wrappers: &[Handle; CAPACITY]) -> i32 {
    let Some(bindings) = BINDINGS.get(kind as usize) else {
        return 0;
    };
    if target.is_null() {
        return 0;
    }
    let _install = INSTALL.lock().unwrap_or_else(PoisonError::into_inner);
    if bindings
        .iter()
        .any(|entry| entry.target.load(Ordering::Relaxed) == target)
    {
        return 1;
    }
    let Some((entry, &wrapper)) = bindings
        .iter()
        .zip(wrappers)
        .find(|(entry, _)| entry.target.load(Ordering::Relaxed).is_null())
    else {
        return 0;
    };
    let mut original = ptr::null_mut();
    if unsafe { MH_CreateHook(target, wrapper, &mut original) } != MH_OK {
        return 0;
    }
    // A detour can run as soon as the native patch is enabled.
    entry.target.store(target, Ordering::Relaxed);
    entry.original.store(original, Ordering::Release);
    if unsafe { MH_EnableHook(target) } == MH_OK {
        return 1;
    }
    unsafe { MH_RemoveHook(target) };
    entry.original.store(ptr::null_mut(), Ordering::Release);
    entry.target.store(ptr::null_mut(), Ordering::Relaxed);
    0
}

fn original(kind: u32, index: usize) -> Handle {
    BINDINGS[kind as usize][index]
        .original
        .load(Ordering::Acquire)
}

fn enter(kind: u32) -> i32 {
    DEPTH.with(|depth| {
        let mut current = depth.get();
        let value = &mut current[kind as usize];
        let outer = *value == 0;
        *value += 1;
        depth.set(current);
        i32::from(outer)
    })
}

fn leave(kind: u32) {
    DEPTH.with(|depth| {
        let mut current = depth.get();
        current[kind as usize] -= 1;
        depth.set(current);
    });
}

fn submit_begin(queue: Handle, count: u32, lists: *const Handle) -> u64 {
    OBSERVER.get().map_or(0, |owner| {
        let id = owner.serial.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        owner.notify_submit(queue, id, count, lists, 1);
        id
    })
}

fn submit_end(queue: Handle, id: u64, count: u32, lists: *const Handle) {
    if let Some(owner) = OBSERVER.get() {
        owner.notify_submit(queue, id, count, lists, 0);
    }
}

fn notify_present(chain: Handle, interval: u32, flags: u32, begin: u32) {
    if let Some(owner) = OBSERVER.get()
        && let Some(present) = owner.setup.present
    {
        unsafe { present(owner.setup.user, chain, interval, flags, begin) };
    }
}
