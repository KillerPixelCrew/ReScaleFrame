// SPDX-License-Identifier: MIT
//! Graphics owner of the Unity package's D3D11 bridge (integrations/unity/native/bridge.cpp). That
//! bridge copies Unity's D3D11 textures into shared surfaces and records the render pass on a
//! D3D12 queue: the Runtime's D3D11 presentation device when it has one, else a device it creates
//! on Unity's adapter.
//!
//! One record owns the shared graphics and the command slots as a unit: Unity's D3D11 device and
//! immediate context, the D3D12 device and queue, the completion fence, the shared D3D11/D3D12
//! fence and one D3D12 allocator and list per ring slot. This module also owns the single creator,
//! the stopped state after a failed creation, the checked close and the release. Slot rotation and
//! fence values are the package bridge's submission ring (`bridge_interop.rs`).
//!
//! bridge.cpp keeps Unity interface access and the typed COM calls as leaves, which run outside
//! the state mutex. Its frame callbacks (`bridge_state.rs`) are the admission that keeps a
//! borrowed view alive: the package closes callback admission before it closes this owner.
//!
//! The C++ kept these objects as unguarded globals. It created the command slots lazily on the
//! render thread, kept partial objects after a failed creation until release, and let the
//! package's graphics lookup copy the D3D12 device and queue without any admission.
use crate::bridge_abi::RSF_UNITY_FRAMES_SLOTS;
use crate::bridge_interop::{
    rsf_unity_interop_d3d11_drained, rsf_unity_interop_drained, rsf_unity_interop_release,
};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

const SLOTS: usize = RSF_UNITY_FRAMES_SLOTS as usize;
// Initialization results, as d3d11_state.h defines them.
const DEVICE_BUSY: u32 = 0;
const DEVICE_FAILED: u32 = 1;
const DEVICE_READY: u32 = 2;
const DEVICE_STOPPED: u32 = 3;

/// Transferred COM references and the shared fence. A copy is a view borrowed under a frame
/// callback, the creation claim or the checked close.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeviceHandles {
    device11: *mut c_void,
    context11: *mut c_void,
    device12: *mut c_void,
    queue12: *mut c_void,
    completion: *mut c_void,
    sharing: *mut c_void,
}

const _: () = assert!(
    size_of::<DeviceHandles>() == 48
        && std::mem::offset_of!(DeviceHandles, device12) == 16
        && std::mem::offset_of!(DeviceHandles, sharing) == 40
);

impl DeviceHandles {
    const EMPTY: Self = Self {
        device11: ptr::null_mut(),
        context11: ptr::null_mut(),
        device12: ptr::null_mut(),
        queue12: ptr::null_mut(),
        completion: ptr::null_mut(),
        sharing: ptr::null_mut(),
    };

    fn complete(self) -> bool {
        [
            self.device11,
            self.context11,
            self.device12,
            self.queue12,
            self.completion,
            self.sharing,
        ]
        .iter()
        .all(|handle| !handle.is_null())
    }
}

/// One ring slot's transferred allocator and command list.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CommandSlot {
    allocator: *mut c_void,
    list: *mut c_void,
}

const _: () =
    assert!(size_of::<CommandSlot>() == 16 && std::mem::offset_of!(CommandSlot, list) == 8);

impl CommandSlot {
    const EMPTY: Self = Self {
        allocator: ptr::null_mut(),
        list: ptr::null_mut(),
    };

    fn complete(self) -> bool {
        !self.allocator.is_null() && !self.list.is_null()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DeviceOps {
    create: unsafe extern "C" fn(*const rsf_game_host_services, *mut DeviceHandles) -> bool,
    destroy: unsafe extern "C" fn(*const DeviceHandles),
    create_slot: unsafe extern "C" fn(*const DeviceHandles, *mut CommandSlot) -> bool,
    destroy_slot: unsafe extern "C" fn(*const CommandSlot),
    completed: unsafe extern "C" fn(*const DeviceHandles) -> u64,
    shared_completed: unsafe extern "C" fn(*const DeviceHandles) -> u64,
}

const _: () = assert!(size_of::<DeviceOps>() == 6 * size_of::<usize>());

/// The fence readers and the view they read, copied out for a check outside the state mutex.
type Proof = (DeviceHandles, DeviceOps);

struct SharedDevice {
    handles: DeviceHandles,
    ops: DeviceOps,
    slots: [CommandSlot; SLOTS],
}

impl SharedDevice {
    fn new(ops: DeviceOps) -> Self {
        Self {
            handles: DeviceHandles::EMPTY,
            ops,
            slots: [CommandSlot::EMPTY; SLOTS],
        }
    }

    /// The device, then every slot. Stops at the first refusal; drop destroys what exists.
    fn prepare(&mut self, services: *const rsf_game_host_services) -> bool {
        if !unsafe { (self.ops.create)(services, &mut self.handles) } || !self.handles.complete() {
            return false;
        }
        self.slots.iter_mut().all(|slot| {
            let created = unsafe { (self.ops.create_slot)(&self.handles, slot) };
            created && slot.complete()
        })
    }
}

impl Drop for SharedDevice {
    fn drop(&mut self) {
        // The lists reference their allocators and the device; release them first.
        for slot in &self.slots {
            unsafe { (self.ops.destroy_slot)(slot) };
        }
        unsafe { (self.ops.destroy)(&self.handles) };
    }
}

/// How a caller of `initialize` proceeds after the state mutex is released.
#[derive(Debug, PartialEq, Eq)]
enum Claim {
    Ready(DeviceHandles),
    Create,
    Refused(u32),
}

struct State {
    /// Set before the first creation leaf runs and cleared after publication or rollback.
    creating: bool,
    device: Option<SharedDevice>,
    /// A creation failed: refuse further attempts until release, as the C++ `failed11` did.
    failed: bool,
    /// The package closed admission. Only retirement may use the record.
    closed: bool,
}

// The record holds COM references whose use is serialized by frame callbacks, the creation claim
// and the package lifecycle. The pointers themselves are only copied under the mutex.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State::new());

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

impl State {
    const fn new() -> Self {
        Self {
            creating: false,
            device: None,
            failed: false,
            closed: false,
        }
    }

    fn claim(&mut self) -> Claim {
        if self.closed || self.creating {
            return Claim::Refused(DEVICE_BUSY);
        }
        if let Some(device) = &self.device {
            return Claim::Ready(device.handles);
        }
        if self.failed {
            return Claim::Refused(DEVICE_STOPPED);
        }
        self.creating = true;
        Claim::Create
    }

    /// Ends a refused claim after its rollback.
    fn creation_failed(&mut self) {
        self.creating = false;
        self.failed = true;
    }

    fn view(&self) -> Option<DeviceHandles> {
        if self.closed {
            return None;
        }
        self.device.as_ref().map(|device| device.handles)
    }

    fn slot(&self, index: usize) -> Option<CommandSlot> {
        if self.closed {
            return None;
        }
        self.device.as_ref()?.slots.get(index).copied()
    }

    /// Closes admission and copies the fence readers. None while a creation is in progress.
    fn close(&mut self) -> Option<Option<Proof>> {
        if self.creating {
            return None;
        }
        self.closed = true;
        Some(
            self.device
                .as_ref()
                .map(|device| (device.handles, device.ops)),
        )
    }

    /// The record to destroy outside the mutex, once closed and not creating.
    fn take_for_release(&mut self) -> Option<Option<SharedDevice>> {
        if !self.closed || self.creating {
            return None;
        }
        Some(self.device.take())
    }

    /// Forgets the failure. Admission stays closed until the next package session opens it.
    fn released(&mut self) {
        debug_assert!(self.closed && !self.creating && self.device.is_none());
        *self = Self {
            closed: true,
            ..Self::new()
        };
    }

    fn open(&mut self) {
        self.closed = false;
    }
}

/// The checked close's verdict, in the C++ order: a completion read of UINT64_MAX is device
/// removal, the ring must have reached its last completion and the last D3D11 copy its fence.
fn retired(completed: u64, shared: u64) -> bool {
    completed != u64::MAX
        && rsf_unity_interop_drained(completed) != 0
        && rsf_unity_interop_d3d11_drained(shared) != 0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_d3d11_initialize(
    services: *const rsf_game_host_services,
    ops: *const DeviceOps,
    view: *mut DeviceHandles,
) -> u32 {
    let Some(ops) = (unsafe { ops.as_ref() }) else {
        return DEVICE_FAILED;
    };
    if view.is_null() {
        return DEVICE_FAILED;
    }
    // Bind first, so the state guard ends before any arm runs.
    let claim = lock().claim();
    match claim {
        Claim::Ready(handles) => {
            unsafe { view.write(handles) };
            return DEVICE_READY;
        }
        Claim::Refused(result) => return result,
        Claim::Create => {}
    }

    let mut device = SharedDevice::new(*ops);
    if !device.prepare(services) {
        // Roll back before ending the claim, so a later caller never sees partial objects.
        drop(device);
        lock().creation_failed();
        return DEVICE_FAILED;
    }
    let handles = device.handles;
    {
        // Only the claim holder publishes and close refuses while it runs. Swap with the empty
        // slot so even an old owner could never drop with the state mutex held.
        let mut pending = Some(device);
        let mut state = lock();
        std::mem::swap(&mut state.device, &mut pending);
        state.creating = false;
    }
    unsafe { view.write(handles) };
    DEVICE_READY
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_d3d11_device_view(view: *mut DeviceHandles) -> bool {
    if view.is_null() {
        return false;
    }
    let Some(handles) = lock().view() else {
        return false;
    };
    unsafe { view.write(handles) };
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_d3d11_command_slot(index: u32, view: *mut CommandSlot) -> bool {
    if view.is_null() {
        return false;
    }
    let Some(slot) = lock().slot(index as usize) else {
        return false;
    };
    unsafe { view.write(slot) };
    true
}

/// Fence reads run outside the state mutex. The closed callback admission and this close keep
/// the record alive and prevent new submissions.
#[unsafe(no_mangle)]
extern "C" fn rsf_unity_d3d11_try_close() -> bool {
    let Some(proof) = lock().close() else {
        return false;
    };
    let (completed, shared) = proof.map_or((0, 0), |(handles, ops)| unsafe {
        ((ops.completed)(&handles), (ops.shared_completed)(&handles))
    });
    retired(completed, shared)
}

/// The package's checked close still holds the proof. The destruction leaves are infallible.
#[unsafe(no_mangle)]
extern "C" fn rsf_unity_d3d11_release() -> bool {
    let Some(device) = lock().take_for_release() else {
        return false;
    };
    drop(device);
    rsf_unity_interop_release();
    lock().released();
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_d3d11_open() {
    lock().open();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        /// The leaves each test's fake ops called, in order. Tests run on their own threads.
        static CALLS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
        /// The leaf that refuses, if any.
        static REFUSE: RefCell<&'static str> = const { RefCell::new("") };
    }

    fn handle(value: usize) -> *mut c_void {
        ptr::without_provenance_mut(value)
    }

    fn called(name: &'static str) -> bool {
        CALLS.with(|calls| calls.borrow_mut().push(name));
        REFUSE.with(|refuse| *refuse.borrow() != name)
    }

    fn calls() -> Vec<&'static str> {
        CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
    }

    unsafe extern "C" fn create(_: *const rsf_game_host_services, out: *mut DeviceHandles) -> bool {
        let out = unsafe { &mut *out };
        out.device11 = handle(1);
        out.context11 = handle(2);
        out.device12 = handle(3);
        if !called("create") {
            // A partial transfer, as the native leaf makes on a refused queue.
            return false;
        }
        out.queue12 = handle(4);
        out.completion = handle(5);
        out.sharing = handle(6);
        true
    }

    unsafe extern "C" fn destroy(shared: *const DeviceHandles) {
        called(if unsafe { (*shared).queue12.is_null() } {
            "destroy partial"
        } else {
            "destroy"
        });
    }

    unsafe extern "C" fn create_slot(_: *const DeviceHandles, out: *mut CommandSlot) -> bool {
        let out = unsafe { &mut *out };
        out.allocator = handle(7);
        if !called("create_slot") {
            return false;
        }
        out.list = handle(8);
        true
    }

    unsafe extern "C" fn destroy_slot(slot: *const CommandSlot) {
        let slot = unsafe { *slot };
        called(if slot.complete() {
            "destroy_slot"
        } else if slot.allocator.is_null() {
            "destroy_slot empty"
        } else {
            "destroy_slot partial"
        });
    }

    unsafe extern "C" fn completed(_: *const DeviceHandles) -> u64 {
        0
    }

    const OPS: DeviceOps = DeviceOps {
        create,
        destroy,
        create_slot,
        destroy_slot,
        completed,
        shared_completed: completed,
    };

    fn refuse(name: &'static str) {
        REFUSE.with(|refuse| *refuse.borrow_mut() = name);
    }

    #[test]
    fn the_device_and_every_slot_are_created_together() {
        let mut device = SharedDevice::new(OPS);
        assert!(device.prepare(ptr::null()));
        assert!(device.handles.complete());
        assert!(device.slots.iter().all(|slot| slot.complete()));
        assert_eq!(
            calls(),
            ["create", "create_slot", "create_slot", "create_slot"]
        );
        drop(device);
        assert_eq!(
            calls(),
            ["destroy_slot", "destroy_slot", "destroy_slot", "destroy"]
        );
    }

    #[test]
    fn a_refused_slot_rolls_back_the_whole_record() {
        refuse("create_slot");
        let mut device = SharedDevice::new(OPS);
        assert!(!device.prepare(ptr::null()));
        assert_eq!(calls(), ["create", "create_slot"]);
        drop(device);
        // The partial slot and the complete device are destroyed; untouched slots are empty.
        assert_eq!(
            calls(),
            [
                "destroy_slot partial",
                "destroy_slot empty",
                "destroy_slot empty",
                "destroy"
            ]
        );
    }

    #[test]
    fn a_refused_device_destroys_its_partial_transfer() {
        refuse("create");
        let mut device = SharedDevice::new(OPS);
        assert!(!device.prepare(ptr::null()));
        assert_eq!(calls(), ["create"]);
        drop(device);
        assert_eq!(
            calls(),
            [
                "destroy_slot empty",
                "destroy_slot empty",
                "destroy_slot empty",
                "destroy partial"
            ]
        );
    }

    #[test]
    fn one_caller_creates_and_a_failure_stops_until_release() {
        let mut state = State::new();
        assert_eq!(state.claim(), Claim::Create);
        assert_eq!(state.claim(), Claim::Refused(DEVICE_BUSY));
        // Close refuses while a creation runs.
        assert!(state.close().is_none());
        state.creation_failed();
        assert_eq!(state.claim(), Claim::Refused(DEVICE_STOPPED));
        assert!(state.view().is_none());
        assert!(state.take_for_release().is_none());
        assert!(state.close().unwrap().is_none());
        assert!(state.take_for_release().unwrap().is_none());
        state.released();
        assert!(!state.failed);
        assert_eq!(state.claim(), Claim::Refused(DEVICE_BUSY));
        state.open();
        assert_eq!(state.claim(), Claim::Create);
    }

    #[test]
    fn a_published_record_is_lent_until_close() {
        let mut state = State::new();
        assert_eq!(state.claim(), Claim::Create);
        let mut device = SharedDevice::new(OPS);
        assert!(device.prepare(ptr::null()));
        let handles = device.handles;
        state.device = Some(device);
        state.creating = false;
        assert_eq!(state.claim(), Claim::Ready(handles));
        assert_eq!(state.view(), Some(handles));
        assert!(state.slot(SLOTS - 1).unwrap().complete());
        assert!(state.slot(SLOTS).is_none());
        let (proof, _) = state.close().unwrap().unwrap();
        assert_eq!(proof, handles);
        assert!(state.view().is_none());
        assert!(state.slot(0).is_none());
        assert_eq!(state.claim(), Claim::Refused(DEVICE_BUSY));
        calls();
        let taken = state.take_for_release().unwrap();
        assert!(state.device.is_none());
        drop(taken);
        assert_eq!(calls().last(), Some(&"destroy"));
        state.released();
        assert!(state.closed);
    }

    #[test]
    fn exports_check_their_pointers() {
        assert_eq!(
            unsafe { rsf_unity_d3d11_initialize(ptr::null(), ptr::null(), ptr::null_mut()) },
            DEVICE_FAILED
        );
        let mut view = DeviceHandles::EMPTY;
        assert_eq!(
            unsafe { rsf_unity_d3d11_initialize(ptr::null(), &OPS, ptr::null_mut()) },
            DEVICE_FAILED
        );
        assert!(!unsafe { rsf_unity_d3d11_device_view(ptr::null_mut()) });
        assert!(!unsafe { rsf_unity_d3d11_command_slot(0, ptr::null_mut()) });
        // Nothing was published by these refusals.
        assert!(!unsafe { rsf_unity_d3d11_device_view(&mut view) });
        assert_eq!(view, DeviceHandles::EMPTY);
    }
}
