// SPDX-License-Identifier: MIT
use super::{Handle, OBSERVER, Observer, diagnostics, rsf_core_observer12_present};
use crate::com_ref::ComRef;
use std::ptr;

const FACTORY: u32 = 0;
const DEVICE: u32 = 1;
const QUEUE: u32 = 2;
const CHAIN: u32 = 3;
const DIRECT: u32 = 0;
const CREATE_SWAPCHAIN_FOR_HWND: usize = 15;
const CREATE_COMMAND_QUEUE: usize = 8;
const EXECUTE_COMMAND_LISTS: usize = 10;
const PRESENT: usize = 8;

type Query = unsafe extern "C" fn(Handle, &mut Handle) -> i32;

#[repr(C)]
struct Platform {
    query_factory: Query,
    query_device: Query,
    query_queue: Query,
    queue_type: unsafe extern "C" fn(Handle) -> u32,
    bind_interface: unsafe extern "C" fn(u32, Handle, usize) -> i32,
    register_debug: unsafe extern "C" fn(Handle),
    describe_window: unsafe extern "C" fn(Handle, &mut Handle) -> i32,
}

const _: () = assert!(size_of::<Platform>() == 7 * size_of::<Handle>());

fn query_interface(query: Query, pointer: Handle) -> Option<ComRef> {
    if pointer.is_null() {
        return None;
    }
    let mut object = ptr::null_mut();
    let result = unsafe { query(pointer, &mut object) };
    unsafe { ComRef::from_owned(object) }.filter(|_| result >= 0)
}

impl Platform {
    fn direct_queue(&self, pointer: Handle) -> Option<ComRef> {
        query_interface(self.query_queue, pointer)
            .filter(|queue| unsafe { (self.queue_type)(queue.as_ptr()) } == DIRECT)
    }

    fn bind_queue(&self, queue: &ComRef) {
        unsafe { (self.bind_interface)(QUEUE, queue.as_ptr(), EXECUTE_COMMAND_LISTS) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer12_attach(
    observer: *const Observer,
    pointer: Handle,
    kind: u32,
    platform: &Platform,
) -> i32 {
    if !OBSERVER.get().is_some_and(|owner| ptr::eq(owner, observer)) {
        return -1;
    }
    let (query, slot) = match kind {
        FACTORY => (platform.query_factory, CREATE_SWAPCHAIN_FOR_HWND),
        DEVICE => (platform.query_device, CREATE_COMMAND_QUEUE),
        _ => return -1,
    };
    let Some(object) = query_interface(query, pointer) else {
        return -1;
    };
    if kind == DEVICE && diagnostics::enabled() {
        unsafe { (platform.register_debug)(object.as_ptr()) };
    }
    if unsafe { (platform.bind_interface)(kind, object.as_ptr(), slot) } != 0 {
        0
    } else {
        -3
    }
}

/// The native CreateSwapChainForHwnd has returned. Its output is readable on success.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer12_factory_result(
    platform: &Platform,
    result: i32,
    outer: u32,
    queue: Handle,
    window: Handle,
    chain: *const Handle,
) {
    if outer == 0 || result < 0 || queue.is_null() {
        return;
    }
    let Some(&chain) = (unsafe { chain.as_ref() }).filter(|chain| !chain.is_null()) else {
        return;
    };
    if let Some(owner) = OBSERVER.get() {
        observe_swapchain(owner, platform, queue, window, chain);
    }
}

fn observe_swapchain(
    owner: &Observer,
    platform: &Platform,
    queue: Handle,
    window: Handle,
    chain: Handle,
) {
    if !owner.accepts_window(window) {
        return;
    }
    let Some(queue) = platform.direct_queue(queue) else {
        return;
    };
    platform.bind_queue(&queue);
    unsafe { (platform.bind_interface)(CHAIN, chain, PRESENT) };
    if let Some(swapchain) = owner.setup.swapchain {
        unsafe { swapchain(owner.setup.user, queue.as_ptr(), chain, window) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer12_device_result(
    platform: &Platform,
    result: i32,
    queue: *const Handle,
) {
    if result >= 0
        && let Some(&queue) = unsafe { queue.as_ref() }
        && let Some(queue) = platform.direct_queue(queue)
    {
        platform.bind_queue(&queue);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer12_present_begin(
    platform: &Platform,
    chain: Handle,
    interval: u32,
    flags: u32,
) -> u32 {
    let mut window = ptr::null_mut();
    if unsafe { (platform.describe_window)(chain, &mut window) } == 0
        || !OBSERVER
            .get()
            .is_some_and(|owner| owner.accepts_window(window))
    {
        return 0;
    }
    rsf_core_observer12_present(chain, interval, flags, 1);
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{add_ref, fake, raw};
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::AtomicU64;

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Accept,
        Query,
        QueueType,
        Bind(u32, usize),
        Publish,
    }

    thread_local! {
        static CALLS: RefCell<Vec<Call>> = const { RefCell::new(Vec::new()) };
        static QUEUE_TYPE: Cell<u32> = const { Cell::new(0) };
    }

    fn record(call: Call) {
        CALLS.with_borrow_mut(|calls| calls.push(call));
    }

    unsafe extern "C" fn accept(_: Handle, window: Handle) -> i32 {
        record(Call::Accept);
        i32::from(window.addr() == 2)
    }

    unsafe extern "C" fn query(pointer: Handle, out: &mut Handle) -> i32 {
        record(Call::Query);
        unsafe { add_ref(pointer.cast()) };
        *out = pointer;
        0
    }

    unsafe extern "C" fn queue_type(_: Handle) -> u32 {
        record(Call::QueueType);
        QUEUE_TYPE.get()
    }

    unsafe extern "C" fn bind(kind: u32, _: Handle, slot: usize) -> i32 {
        record(Call::Bind(kind, slot));
        0
    }

    unsafe extern "C" fn publish(_: Handle, _: Handle, chain: Handle, window: Handle) {
        assert_eq!((chain.addr(), window.addr()), (3, 2));
        record(Call::Publish);
    }

    unsafe extern "C" fn register_debug(_: Handle) {}

    unsafe extern "C" fn describe_window(_: Handle, _: &mut Handle) -> i32 {
        0
    }

    const PLATFORM: Platform = Platform {
        query_factory: query,
        query_device: query,
        query_queue: query,
        queue_type,
        bind_interface: bind,
        register_debug,
        describe_window,
    };

    #[test]
    fn attachment_checks_window_then_queue_and_publishes_after_both_bind_attempts() {
        let observer = Observer {
            setup: super::super::ObserverSetup {
                struct_size: 48,
                abi_version: 1,
                user: ptr::null_mut(),
                accept_window: Some(accept),
                submit: None,
                present: None,
                swapchain: Some(publish),
            },
            serial: AtomicU64::new(0),
        };
        let take_calls = || CALLS.with_borrow_mut(std::mem::take);
        let queue = fake();
        let chain = ptr::without_provenance_mut(3);
        let window = ptr::without_provenance_mut(2);
        observe_swapchain(&observer, &PLATFORM, raw(&queue), ptr::null_mut(), chain);
        assert_eq!(take_calls(), [Call::Accept]);

        QUEUE_TYPE.set(2);
        observe_swapchain(&observer, &PLATFORM, raw(&queue), window, chain);
        assert_eq!(take_calls(), [Call::Accept, Call::Query, Call::QueueType]);
        assert_eq!(queue.refs.get(), 1);

        QUEUE_TYPE.set(0);
        observe_swapchain(&observer, &PLATFORM, raw(&queue), window, chain);
        assert_eq!(
            take_calls(),
            [
                Call::Accept,
                Call::Query,
                Call::QueueType,
                Call::Bind(2, 10),
                Call::Bind(3, 8),
                Call::Publish
            ],
        );
        assert_eq!(queue.refs.get(), 1);
    }
}
