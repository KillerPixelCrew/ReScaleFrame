// SPDX-License-Identifier: MIT
//! Graphics owner of the Unity package's D3D11 bridge (integrations/unity/native/bridge.cpp). That
//! bridge copies Unity's D3D11 textures into shared surfaces and records the render pass on a
//! D3D12 queue: the Runtime's D3D11 presentation device when it has one, else a device it creates
//! on Unity's adapter.
//!
//! One record owns the shared graphics, the command slots and the shared surfaces as a unit:
//! Unity's D3D11 device and immediate context, the D3D12 device and queue, the completion fence,
//! the shared D3D11/D3D12 fence, one D3D12 allocator and list per ring slot and the six
//! `rsf_shared_surface` slots. This module also owns the single creator, the stopped state after a
//! failed creation, each surface's key and its replacement, the surface lease, the checked close
//! and the release. Slot rotation and fence values are the package bridge's submission ring
//! (`bridge_interop.rs`).
//!
//! The D3D11 route also admits its packets here (`rsf_unity_d3d11_admit_packet`): the shared
//! packet store (`bridge_state.rs`) retains the packet's Unity textures through the typed leaves
//! before the stored address reaches Unity, holds the references through the render event that
//! takes the packet and releases them when that event ends or the store's release cancels it.
//! These references exist before the record is created and do not depend on it.
//!
//! The route, pass and surface plan of each render event, the shared surface format, the policy
//! change gate and the submission's retirement verdict are `d3d11_pass.rs`. Its change gate
//! waits through this record's wait leaf (`wait_completion`).
//!
//! bridge.cpp keeps Unity interface access and the typed COM calls as leaves, which run outside
//! the state mutex. Its frame callbacks (`bridge_state.rs`) are the admission that keeps a
//! borrowed view alive: the package closes callback admission before it closes this owner. Inside
//! a callback, the surface lease lends the surface table to one holder, so a surface view stays
//! valid until the holder ends its lease and close refuses while it is held.
//!
//! The C++ kept these objects as unguarded globals. It created the command slots lazily on the
//! render thread, kept partial objects after a failed creation until release, let the package's
//! graphics lookup copy the D3D12 device and queue without any admission, replaced surfaces
//! without any hold on the views a frame had already taken, and kept the queued packets' texture
//! references in a native array beside the Rust store.
use crate::bridge_abi::{RSF_UNITY_FRAMES_SLOTS, rsf_unity_packet};
use crate::bridge_interop::{
    rsf_unity_interop_d3d11_drained, rsf_unity_interop_drained, rsf_unity_interop_release,
    rsf_unity_interop_submitted,
};
use crate::bridge_state::{LeaseOps, admit_leased_packet};
use crate::d3d11_pass::shared_format;
use crate::device_creation::{Claim, Creation};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

const SLOTS: usize = RSF_UNITY_FRAMES_SLOTS as usize;
/// Colour, depth, motion, output, the HUD-less scene and the Editor preview source.
const SURFACES: usize = 6;
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
    wait: unsafe extern "C" fn(*const DeviceHandles, u64) -> bool,
    create_surface: unsafe extern "C" fn(
        *const DeviceHandles,
        *const rsf_game_host_services,
        u32,
        u32,
        u32,
        *mut *mut c_void,
    ) -> bool,
    destroy_surface: unsafe extern "C" fn(*mut c_void),
}

const _: () = assert!(size_of::<DeviceOps>() == 9 * size_of::<usize>());

/// The fence readers and the view they read, copied out for a check outside the state mutex.
type Proof = (DeviceHandles, DeviceOps);

/// The source texture description a surface was created for. The C++ compared the same three
/// fields of `D3D11_TEXTURE2D_DESC`; the shared format derived from them is not part of the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceKey {
    width: u32,
    height: u32,
    format: u32,
}

/// One owned `rsf_shared_surface` and the key it was created for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Surface {
    key: SurfaceKey,
    handle: *mut c_void,
}

struct SharedDevice {
    handles: DeviceHandles,
    ops: DeviceOps,
    slots: [CommandSlot; SLOTS],
    /// Created on a slot's first use and replaced when its source description changes.
    surfaces: [Option<Surface>; SURFACES],
}

impl SharedDevice {
    fn new(ops: DeviceOps) -> Self {
        Self {
            handles: DeviceHandles::EMPTY,
            ops,
            slots: [CommandSlot::EMPTY; SLOTS],
            surfaces: [None; SURFACES],
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
        // Surfaces first, while the devices they were created on are still referenced, as the C++
        // release did. Then the lists, which reference their allocators and the device.
        for surface in self.surfaces.iter().flatten() {
            unsafe { (self.ops.destroy_surface)(surface.handle) };
        }
        for slot in &self.slots {
            unsafe { (self.ops.destroy_slot)(slot) };
        }
        unsafe { (self.ops.destroy)(&self.handles) };
    }
}

/// How `prepare_surface` proceeds after the state mutex is released.
#[derive(Debug, PartialEq, Eq)]
enum SurfaceClaim {
    /// The slot's surface matches the key; the lease now holds its view.
    Borrowed(*mut c_void),
    /// Create a surface for the slot. `retire` when it replaces one that submissions may read.
    Create {
        retire: bool,
    },
    Refused,
}

struct State {
    creation: Creation<SharedDevice>,
    /// The package closed admission. Only retirement may use the record.
    closed: bool,
    /// One frame callback holds the surface table. Only it may borrow or replace a surface.
    leased: bool,
    /// The surface slots the current lease borrowed. Their views stay valid until it ends.
    borrowed: u8,
}

// The record holds COM references whose use is serialized by frame callbacks, the creation claim,
// the surface lease and the package lifecycle. The pointers themselves are only copied under the
// mutex.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State::new());

fn lock() -> MutexGuard<'static, State> {
    lock_state(&STATE)
}

fn lock_state(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

impl State {
    const fn new() -> Self {
        Self {
            creation: Creation::new(),
            closed: false,
            leased: false,
            borrowed: 0,
        }
    }

    fn claim(&mut self) -> Claim<DeviceHandles> {
        self.creation.claim(!self.closed, |device| device.handles)
    }

    fn creation_failed(&mut self) {
        self.creation.rolled_back(true);
    }

    fn view(&self) -> Option<DeviceHandles> {
        if self.closed {
            return None;
        }
        self.creation.device.as_ref().map(|device| device.handles)
    }

    /// The published view and its leaves for a frame callback's completion wait.
    fn waiter(&self) -> Option<Proof> {
        if self.closed {
            return None;
        }
        self.creation
            .device
            .as_ref()
            .map(|device| (device.handles, device.ops))
    }

    fn slot(&self, index: usize) -> Option<CommandSlot> {
        if self.closed {
            return None;
        }
        self.creation.device.as_ref()?.slots.get(index).copied()
    }

    /// Lends the surface table to one frame callback. Refused when closed, unpublished or held.
    fn begin_surfaces(&mut self) -> bool {
        if self.closed || self.creation.creating || self.leased || self.creation.device.is_none() {
            return false;
        }
        self.leased = true;
        self.borrowed = 0;
        true
    }

    fn end_surfaces(&mut self) {
        self.leased = false;
        self.borrowed = 0;
    }

    /// The lease holder's admission for surface slot `index` with a source description `key`.
    fn surface_claim(&mut self, index: usize, key: SurfaceKey) -> SurfaceClaim {
        if !self.leased || self.closed || index >= SURFACES {
            return SurfaceClaim::Refused;
        }
        let Some(device) = &self.creation.device else {
            return SurfaceClaim::Refused;
        };
        let current = device.surfaces[index];
        let bit = 1u8 << index;
        match current {
            Some(surface) if surface.key == key => {
                self.borrowed |= bit;
                SurfaceClaim::Borrowed(surface.handle)
            }
            // Replacing would destroy a view this lease already lent.
            _ if self.borrowed & bit != 0 => SurfaceClaim::Refused,
            current => SurfaceClaim::Create {
                retire: current.is_some(),
            },
        }
    }

    /// Installs the lease holder's new surface and returns the one it replaces, or hands the new
    /// surface back when the lease or record is gone.
    fn publish_surface(
        &mut self,
        index: usize,
        surface: Surface,
    ) -> Result<Option<Surface>, Surface> {
        if !self.leased || self.closed {
            return Err(surface);
        }
        let Some(entry) = self
            .creation
            .device
            .as_mut()
            .and_then(|device| device.surfaces.get_mut(index))
        else {
            return Err(surface);
        };
        let previous = entry.replace(surface);
        self.borrowed |= 1 << index;
        Ok(previous)
    }

    /// Closes admission and copies the fence readers. None while a creation is in progress or a
    /// frame callback holds the surface lease.
    fn close(&mut self) -> Option<Option<Proof>> {
        if self.creation.creating || self.leased {
            return None;
        }
        self.closed = true;
        Some(
            self.creation
                .device
                .as_ref()
                .map(|device| (device.handles, device.ops)),
        )
    }

    /// The record to destroy outside the mutex, once closed and not creating.
    fn take_for_release(&mut self) -> Option<Option<SharedDevice>> {
        if !self.closed || self.creation.creating || self.leased {
            return None;
        }
        Some(self.creation.device.take())
    }

    /// Forgets the failure. Admission stays closed until the next package session opens it.
    fn released(&mut self) {
        debug_assert!(
            self.closed
                && !self.creation.creating
                && !self.leased
                && self.creation.device.is_none()
        );
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
        Claim::Busy => return DEVICE_BUSY,
        Claim::Stopped => return DEVICE_STOPPED,
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
    let previous = lock().creation.publish(device);
    drop(previous);
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

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_d3d11_surfaces_begin() -> bool {
    lock().begin_surfaces()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_d3d11_surfaces_end() {
    lock().end_surfaces();
}

/// Borrows or replaces one surface for the lease holder. The leaves run outside the state mutex;
/// the lease keeps the record published, since close and release refuse while it is held.
fn prepare_surface(
    state: &Mutex<State>,
    index: usize,
    services: *const rsf_game_host_services,
    key: SurfaceKey,
    shared_format: u32,
) -> Option<*mut c_void> {
    let (retire, handles, ops) = {
        let mut state = lock_state(state);
        let retire = match state.surface_claim(index, key) {
            SurfaceClaim::Borrowed(surface) => return Some(surface),
            SurfaceClaim::Refused => return None,
            SurfaceClaim::Create { retire } => retire,
        };
        let device = state.creation.device.as_ref()?;
        (retire, device.handles, device.ops)
    };
    // Every submission that may still read the replaced surface took a completion value no later
    // than the last one handed out, so waiting for that value retires it. A first creation has
    // nothing to retire.
    if retire && !unsafe { (ops.wait)(&handles, rsf_unity_interop_submitted()) } {
        return None;
    }
    let mut created: *mut c_void = ptr::null_mut();
    let accepted = unsafe {
        (ops.create_surface)(
            &handles,
            services,
            key.width,
            key.height,
            shared_format,
            &mut created,
        )
    };
    if !accepted || created.is_null() {
        // The previous surface stays published, as the C++ kept it.
        unsafe { (ops.destroy_surface)(created) };
        return None;
    }
    let published = lock_state(state).publish_surface(
        index,
        Surface {
            key,
            handle: created,
        },
    );
    match published {
        Ok(previous) => {
            if let Some(previous) = previous {
                unsafe { (ops.destroy_surface)(previous.handle) };
            }
            Some(created)
        }
        Err(refused) => {
            unsafe { (ops.destroy_surface)(refused.handle) };
            None
        }
    }
}

/// The shared format is chosen from the slot and the source format (`d3d11_pass::shared_format`).
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_d3d11_prepare_surface(
    index: u32,
    services: *const rsf_game_host_services,
    width: u32,
    height: u32,
    source_format: u32,
    view: *mut *mut c_void,
) -> bool {
    if view.is_null() {
        return false;
    }
    let key = SurfaceKey {
        width,
        height,
        format: source_format,
    };
    let shared = shared_format(index, source_format);
    let Some(surface) = prepare_surface(&STATE, index as usize, services, key, shared) else {
        return false;
    };
    unsafe { view.write(surface) };
    true
}

/// A frame callback's wait for the completion fence to reach `value`, through the record's wait
/// leaf outside the state mutex. The callback keeps the record published. False without one.
pub(crate) fn wait_completion(value: u64) -> bool {
    let Some((handles, ops)) = lock().waiter() else {
        return false;
    };
    unsafe { (ops.wait)(&handles, value) }
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
/// Surfaces go first, then the slots and the shared graphics (`SharedDevice::drop`).
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

/// `ops` is `rsf_unity_d3d11_texture_ops`. The store runs the retain leaf under its mutex while it
/// publishes the packet, so the address is never visible without the lease.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_d3d11_admit_packet(
    packet: *const rsf_unity_packet,
    session_id: u64,
    ops: *const LeaseOps,
) -> *mut c_void {
    let Some(ops) = (unsafe { ops.as_ref() }) else {
        return ptr::null_mut();
    };
    unsafe { admit_leased_packet(packet, session_id, *ops) }
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

    unsafe extern "C" fn wait(_: *const DeviceHandles, _: u64) -> bool {
        called("wait")
    }

    /// Each surface's handle is its width, so a test can tell replacements apart.
    unsafe extern "C" fn create_surface(
        _: *const DeviceHandles,
        _: *const rsf_game_host_services,
        width: u32,
        _: u32,
        _: u32,
        out: *mut *mut c_void,
    ) -> bool {
        if !called("create_surface") {
            return false;
        }
        unsafe { out.write(handle(width as usize)) };
        true
    }

    unsafe extern "C" fn destroy_surface(surface: *mut c_void) {
        called(if surface.is_null() {
            "destroy_surface empty"
        } else {
            "destroy_surface"
        });
    }

    const OPS: DeviceOps = DeviceOps {
        create,
        destroy,
        create_slot,
        destroy_slot,
        completed,
        shared_completed: completed,
        wait,
        create_surface,
        destroy_surface,
    };

    fn key(width: u32) -> SurfaceKey {
        SurfaceKey {
            width,
            height: 4,
            format: 10,
        }
    }

    /// A state with a published record and no calls recorded.
    fn published() -> Mutex<State> {
        let mut state = State::new();
        let mut device = SharedDevice::new(OPS);
        assert!(device.prepare(ptr::null()));
        state.creation.device = Some(device);
        calls();
        Mutex::new(state)
    }

    fn surface(state: &Mutex<State>, index: usize, width: u32) -> Option<*mut c_void> {
        prepare_surface(state, index, ptr::null(), key(width), 2)
    }

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
        assert_eq!(state.claim(), Claim::Busy);
        // Close refuses while a creation runs.
        assert!(state.close().is_none());
        state.creation_failed();
        assert_eq!(state.claim(), Claim::Stopped);
        assert!(state.view().is_none());
        assert!(state.take_for_release().is_none());
        assert!(state.close().unwrap().is_none());
        assert!(state.take_for_release().unwrap().is_none());
        state.released();
        assert_eq!(state.claim(), Claim::Busy);
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
        assert!(state.creation.publish(device).is_none());
        assert_eq!(state.claim(), Claim::Ready(handles));
        assert_eq!(state.view(), Some(handles));
        assert!(state.slot(SLOTS - 1).unwrap().complete());
        assert!(state.slot(SLOTS).is_none());
        assert_eq!(state.waiter().map(|(view, _)| view), Some(handles));
        let (proof, _) = state.close().unwrap().unwrap();
        assert_eq!(proof, handles);
        assert!(state.view().is_none());
        assert!(state.waiter().is_none());
        assert!(state.slot(0).is_none());
        assert_eq!(state.claim(), Claim::Busy);
        calls();
        let taken = state.take_for_release().unwrap();
        assert!(state.creation.device.is_none());
        drop(taken);
        assert_eq!(calls().last(), Some(&"destroy"));
        state.released();
        assert!(state.closed);
    }

    #[test]
    fn a_surface_is_created_borrowed_and_replaced_after_the_ring_retires() {
        let state = published();
        // Without the lease nothing is lent or created.
        assert_eq!(surface(&state, 0, 16), None);
        assert!(lock_state(&state).begin_surfaces());
        // The first creation has nothing to retire.
        assert_eq!(surface(&state, 0, 16), Some(handle(16)));
        assert_eq!(calls(), ["create_surface"]);
        // A matching key borrows without a leaf.
        assert_eq!(surface(&state, 0, 16), Some(handle(16)));
        assert!(calls().is_empty());
        lock_state(&state).end_surfaces();
        assert!(lock_state(&state).begin_surfaces());
        // A new key waits for the ring, creates, publishes, then destroys the old surface.
        assert_eq!(surface(&state, 0, 32), Some(handle(32)));
        assert_eq!(calls(), ["wait", "create_surface", "destroy_surface"]);
        lock_state(&state).end_surfaces();
        assert_eq!(
            lock_state(&state)
                .creation
                .device
                .as_ref()
                .unwrap()
                .surfaces[0],
            Some(Surface {
                key: key(32),
                handle: handle(32)
            })
        );
    }

    #[test]
    fn a_refused_replacement_keeps_the_published_surface() {
        let state = published();
        assert!(lock_state(&state).begin_surfaces());
        assert_eq!(surface(&state, 3, 16), Some(handle(16)));
        lock_state(&state).end_surfaces();
        calls();
        assert!(lock_state(&state).begin_surfaces());
        refuse("wait");
        assert_eq!(surface(&state, 3, 32), None);
        assert_eq!(calls(), ["wait"]);
        refuse("create_surface");
        assert_eq!(surface(&state, 3, 32), None);
        assert_eq!(calls(), ["wait", "create_surface", "destroy_surface empty"]);
        refuse("");
        // The old surface is still lent for its own key.
        assert_eq!(surface(&state, 3, 16), Some(handle(16)));
        assert!(calls().is_empty());
    }

    #[test]
    fn a_lease_never_replaces_a_view_it_lent() {
        let state = published();
        assert!(lock_state(&state).begin_surfaces());
        assert_eq!(surface(&state, 4, 16), Some(handle(16)));
        calls();
        assert_eq!(surface(&state, 4, 32), None);
        assert!(calls().is_empty());
        // Another slot is independent.
        assert_eq!(surface(&state, 5, 32), Some(handle(32)));
        assert_eq!(surface(&state, SURFACES, 32), None);
        lock_state(&state).end_surfaces();
        calls();
    }

    #[test]
    fn the_surface_lease_has_one_holder_and_blocks_close() {
        let mut state = State::new();
        // Nothing to lend before publication.
        assert!(!state.begin_surfaces());
        let mut device = SharedDevice::new(OPS);
        assert!(device.prepare(ptr::null()));
        state.creation.device = Some(device);
        assert!(state.begin_surfaces());
        assert!(!state.begin_surfaces());
        assert!(state.close().is_none());
        assert!(!state.closed);
        state.end_surfaces();
        assert!(state.close().unwrap().is_some());
        assert!(!state.begin_surfaces());
        assert_eq!(state.surface_claim(0, key(16)), SurfaceClaim::Refused);
        assert_eq!(
            state.publish_surface(
                0,
                Surface {
                    key: key(16),
                    handle: handle(16)
                }
            ),
            Err(Surface {
                key: key(16),
                handle: handle(16)
            })
        );
        calls();
    }

    #[test]
    fn release_destroys_surfaces_before_slots_and_device() {
        let state = published();
        assert!(lock_state(&state).begin_surfaces());
        assert!(surface(&state, 1, 16).is_some());
        assert!(surface(&state, 5, 32).is_some());
        lock_state(&state).end_surfaces();
        calls();
        let taken = {
            let mut state = lock_state(&state);
            assert!(state.close().is_some());
            state.take_for_release().unwrap()
        };
        drop(taken);
        assert_eq!(
            calls(),
            [
                "destroy_surface",
                "destroy_surface",
                "destroy_slot",
                "destroy_slot",
                "destroy_slot",
                "destroy"
            ]
        );
        lock_state(&state).released();
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
        assert!(!unsafe {
            rsf_unity_d3d11_prepare_surface(0, ptr::null(), 1, 1, 10, ptr::null_mut())
        });
        // No record, so no surface lease.
        assert!(!rsf_unity_d3d11_surfaces_begin());
        // Without leaves or a packet nothing is admitted.
        assert!(unsafe { rsf_unity_d3d11_admit_packet(ptr::null(), 0, ptr::null()) }.is_null());
        // Nothing was published by these refusals.
        assert!(!unsafe { rsf_unity_d3d11_device_view(&mut view) });
        assert_eq!(view, DeviceHandles::EMPTY);
    }
}
