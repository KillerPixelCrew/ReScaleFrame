// SPDX-License-Identifier: MIT
//! State of the Unity package's Vulkan bridge (integrations/unity/native/vulkan_bridge.cpp). That
//! bridge copies Unity's Vulkan images into same-adapter D3D12 resources, records the render pass
//! on its own D3D12 queue and replaces Unity's present with its own DXGI swap chain.
//!
//! This module owns the bridge's admission and lifecycle: the shared device's readiness and its
//! single creator, the permanent fault, the render events queued to Unity and their packets, the
//! present interceptions and surface updates in progress, the player window frame the present
//! renders under, the bridge's DXGI swap chain and resize, and the checked close and release.
//! One record owns the D3D12 device, queue and fence, the imported semaphore and commands, and
//! retains the Unity Vulkan device facts that shared images and commands need. Slot rotation and
//! shared fence values follow the package bridge's submission ring (`bridge_interop.rs`).
//!
//! The presentation state outlives a release, because Unity owns the swapchains and the window
//! stays: Unity's intercepted swapchains (their images and the present-ready semaphores the bridge
//! created for them), the presentation window's identity, and the presentation lease. The lease is
//! the one exclusive hold the C++ took as a mutex: present interceptions, swapchain retirement and
//! the swap chain's creation on the window thread run one at a time, a present keeps it through
//! the forwarded vkQueuePresentKHR, and retirement keeps it through vkDeviceWaitIdle and the
//! semaphore destruction, which need host access to the device's queues.
//!
//! vulkan_bridge.cpp keeps Unity interface access and typed graphics and Win32 calls. Native
//! callbacks use explicit handles and run outside the state mutex.
//! The orchestrator's Vulkan frame-generation swapchain owners have their own presentation
//! tables and this crate does not link them, so the registry here is the bridge's own.
//!
//! The C++ kept this as unguarded globals. Its release counted only queued render events, so it
//! could destroy the slots, images and swap chain a present interception was recording into on the
//! render thread, and it cleared the window frame identity while that thread copied it.
use crate::bridge_abi::{RSF_UNITY_FRAMES_PACKETS, RSF_UNITY_FRAMES_SLOTS, rsf_unity_packet};
use crate::bridge_interop::{rsf_unity_interop_drained, rsf_unity_interop_release};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::slice;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

const EVENTS: usize = RSF_UNITY_FRAMES_PACKETS as usize;
const SLOTS: usize = RSF_UNITY_FRAMES_SLOTS as usize;
// Event kinds and initialization results, as vulkan_state.h defines them.
const EVENT_HUDLESS: u32 = 2;
const DEVICE_BUSY: u32 = 0;
const DEVICE_FAILED: u32 = 1;
const DEVICE_READY: u32 = 2;
const SHARED_IMAGES: usize = 6;

#[repr(C)]
#[derive(Clone, Copy)]
struct Frame {
    packet: rsf_unity_packet,
    services: rsf_game_host_services,
}
const _: () = assert!(
    size_of::<Frame>() == size_of::<rsf_unity_packet>() + size_of::<rsf_game_host_services>()
        && std::mem::offset_of!(Frame, services) == size_of::<rsf_unity_packet>()
);

// SAFETY: both C records accept zero integers, floats and null pointers.
const EMPTY_FRAME: Frame = unsafe { std::mem::zeroed() };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Free,
    /// Handed to Unity's queue access, not yet called back.
    Queued,
    /// The queue callback is recording it.
    Running,
}

#[derive(Clone, Copy)]
struct Event {
    frame: Frame,
    kind: u32,
    reconfigure: bool,
    stage: Stage,
}

const FREE: Event = Event {
    frame: EMPTY_FRAME,
    kind: 0,
    reconfigure: false,
    stage: Stage::Free,
};

/// One started event: its slot, frame with services, kind and whether it applies a policy change.
type Started = (usize, Frame, u32, bool);

#[derive(Clone, Copy, PartialEq, Eq)]
struct ImageKey {
    width: u32,
    height: u32,
    format: u32,
    dx_format: u32,
}

#[derive(Clone, Copy)]
struct Image {
    key: ImageKey,
    handles: ImageHandles,
    initialized: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ImageHandles {
    resource: *mut c_void,
    image: u64,
    memory: u64,
}

const _: () = assert!(size_of::<ImageHandles>() == 24);

impl ImageHandles {
    const EMPTY: Self = Self {
        resource: ptr::null_mut(),
        image: 0,
        memory: 0,
    };

    fn complete(self) -> bool {
        !self.resource.is_null() && self.image != 0 && self.memory != 0
    }
}

/// The pool owns the two Vulkan buffers. This record also carries one Vulkan fence and two
/// transferred COM references. A copied record is a borrowed view under the presentation lease.
#[repr(C)]
#[derive(Clone, Copy)]
struct CommandSlot {
    allocator: *mut c_void,
    commands: *mut c_void,
    vk: [u64; 2],
    complete: u64,
}

const _: () = assert!(
    size_of::<CommandSlot>() == 40
        && std::mem::offset_of!(CommandSlot, commands) == 8
        && std::mem::offset_of!(CommandSlot, vk) == 16
        && std::mem::offset_of!(CommandSlot, complete) == 32
);

impl CommandSlot {
    const EMPTY: Self = Self {
        allocator: ptr::null_mut(),
        commands: ptr::null_mut(),
        vk: [0; 2],
        complete: 0,
    };

    fn complete(self) -> bool {
        !self.allocator.is_null()
            && !self.commands.is_null()
            && self.vk.iter().all(|&buffer| buffer != 0)
            && self.complete != 0
    }
}

/// The COM references and imported semaphore transfer to Rust. Unity owns the Vulkan device and
/// queue. The native leaves and bridge callbacks borrow this view for the duration of their hold.
#[repr(C)]
#[derive(Clone, Copy)]
struct DeviceHandles {
    device: *mut c_void,
    queue: *mut c_void,
    fence: *mut c_void,
    vk_device: u64,
    vk_physical_device: u64,
    vk_queue: u64,
    vk_semaphore: u64,
    vk_queue_family: u32,
}

const _: () = assert!(
    size_of::<DeviceHandles>() == 64
        && std::mem::offset_of!(DeviceHandles, vk_device) == 24
        && std::mem::offset_of!(DeviceHandles, vk_queue_family) == 56
);

impl DeviceHandles {
    const EMPTY: Self = Self {
        device: ptr::null_mut(),
        queue: ptr::null_mut(),
        fence: ptr::null_mut(),
        vk_device: 0,
        vk_physical_device: 0,
        vk_queue: 0,
        vk_semaphore: 0,
        vk_queue_family: 0,
    };

    fn complete(self) -> bool {
        !self.device.is_null()
            && !self.queue.is_null()
            && !self.fence.is_null()
            && self.vk_device != 0
            && self.vk_physical_device != 0
            && self.vk_queue != 0
            && self.vk_semaphore != 0
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DeviceOps {
    create: unsafe extern "C" fn(*const rsf_game_host_services, *mut DeviceHandles) -> bool,
    destroy: unsafe extern "C" fn(*const DeviceHandles),
    create_pool: unsafe extern "C" fn(*const DeviceHandles) -> u64,
    destroy_pool: unsafe extern "C" fn(*const DeviceHandles, u64),
    create_slot: unsafe extern "C" fn(*const DeviceHandles, u64, *mut CommandSlot) -> bool,
    destroy_slot: unsafe extern "C" fn(*const DeviceHandles, *const CommandSlot),
    create_image:
        unsafe extern "C" fn(*const DeviceHandles, u32, u32, u32, u32, *mut ImageHandles) -> bool,
    destroy_image: unsafe extern "C" fn(*const DeviceHandles, *const ImageHandles),
    fence_ready: unsafe extern "C" fn(*const DeviceHandles, u64) -> bool,
    shared_completed: unsafe extern "C" fn(*const DeviceHandles) -> u64,
    drain_images: unsafe extern "C" fn(*const DeviceHandles) -> bool,
    report_created: unsafe extern "C" fn(*const rsf_game_host_services),
    ring_begin: extern "C" fn() -> u64,
    ring_retired: extern "C" fn(u64),
}

const _: () = assert!(size_of::<DeviceOps>() == 14 * size_of::<usize>());

struct SharedDevice {
    handles: DeviceHandles,
    ops: DeviceOps,
    pool: u64,
    slots: [CommandSlot; SLOTS],
    epoch: u64,
    recordings: [u64; SLOTS],
}

impl SharedDevice {
    fn new(ops: DeviceOps) -> Self {
        Self {
            handles: DeviceHandles::EMPTY,
            ops,
            pool: 0,
            slots: [CommandSlot::EMPTY; SLOTS],
            epoch: 0,
            recordings: [0; SLOTS],
        }
    }

    fn prepare(&mut self, services: *const rsf_game_host_services) -> bool {
        if !unsafe { (self.ops.create)(services, &mut self.handles) } || !self.handles.complete() {
            return false;
        }
        self.pool = unsafe { (self.ops.create_pool)(&self.handles) };
        self.pool != 0
            && self.slots.iter_mut().all(|slot| {
                let created = unsafe { (self.ops.create_slot)(&self.handles, self.pool, slot) };
                created && slot.complete()
            })
    }

    fn retire_commands(&mut self) {
        if self.pool != 0 {
            for slot in &self.slots {
                unsafe { (self.ops.destroy_slot)(&self.handles, slot) };
            }
            unsafe { (self.ops.destroy_pool)(&self.handles, std::mem::take(&mut self.pool)) };
        }
    }
}

impl Drop for SharedDevice {
    fn drop(&mut self) {
        self.retire_commands();
        unsafe { (self.ops.destroy)(&self.handles) };
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SurfaceOps {
    create: unsafe extern "C" fn(*const DeviceHandles, *mut c_void, u32, u32) -> *mut c_void,
    extent: unsafe extern "C" fn(*mut c_void, *mut u32, *mut u32),
    drain: unsafe extern "C" fn(*const DeviceHandles) -> bool,
    resize: unsafe extern "C" fn(*mut c_void, u32, u32) -> bool,
    destroy: unsafe extern "C" fn(*mut c_void),
}

const _: () = assert!(size_of::<SurfaceOps>() == 5 * size_of::<usize>());

struct Surface {
    chain: *mut c_void,
    ops: SurfaceOps,
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: Creation transferred one IDXGISwapChain3 reference to this owner.
        unsafe { (self.ops.destroy)(self.chain) };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PresentIdentity {
    queue: u64,
    swapchain: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Admission {
    Open,
    Retiring,
    Closed,
}

struct State {
    /// Set before invoking the first creation leaf and cleared after publication or rollback.
    creating: bool,
    device: Option<SharedDevice>,
    /// A failed step stopped the bridge until release.
    faulted: bool,
    /// Retiring refuses new entries; Closed allows only retirement to access the resources.
    admission: Admission,
    events: [Event; EVENTS],
    /// Present interceptions and surface updates in progress.
    entries: u32,
    /// One player window frame bound to a native queue and swapchain publication.
    window: Option<(PresentIdentity, Frame)>,
    surface: Option<Surface>,
    images: [Option<Image>; SHARED_IMAGES],
    /// Images whose barriers are recorded in the current command buffer.
    recorded_images: u32,
    /// Vulkan accepted an upload, but no Vulkan slot fence covers it yet.
    partial_submission: bool,
}

// Packets hold resource addresses as identity only. Host code and user data are process-lived.
// Owned device, image and command handles cross during an entry, event, creation claim or checked
// close. Recording, submission and retirement also hold the exclusive presentation lease.
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
            faulted: false,
            admission: Admission::Open,
            events: [FREE; EVENTS],
            entries: 0,
            window: None,
            surface: None,
            images: [None; SHARED_IMAGES],
            recorded_images: 0,
            partial_submission: false,
        }
    }

    fn open(&self) -> bool {
        !self.faulted && self.admission != Admission::Closed
    }

    fn set_window(&mut self, identity: PresentIdentity, frame: &Frame) -> bool {
        if !self.open() || frame.packet.frame_id == 0 {
            return false;
        }
        self.window = Some((identity, *frame));
        true
    }

    fn take_window(&mut self, identity: PresentIdentity) -> Option<Frame> {
        if !self.open() || self.window.as_ref()?.0 != identity {
            return None;
        }
        self.window.take().map(|(_, frame)| frame)
    }

    fn admit_event(&mut self, frame: &Frame, kind: u32, reconfigure: bool) -> Option<usize> {
        if !self.open() || kind > EVENT_HUDLESS {
            return None;
        }
        let slot = self
            .events
            .iter()
            .position(|event| event.stage == Stage::Free)?;
        self.events[slot] = Event {
            frame: *frame,
            kind,
            reconfigure,
            stage: Stage::Queued,
        };
        Some(slot)
    }

    fn event_address(&self, slot: usize) -> *mut c_void {
        ptr::from_ref(&self.events[slot].frame).cast_mut().cast()
    }

    /// The queued event at address, now running. A faulted bridge frees it instead.
    fn start_event(&mut self, address: *const c_void) -> Option<Started> {
        let slot = (0..EVENTS).find(|&slot| {
            self.events[slot].stage == Stage::Queued
                && ptr::eq(self.event_address(slot).cast_const(), address)
        })?;
        if self.faulted {
            self.events[slot] = FREE;
            return None;
        }
        let event = &mut self.events[slot];
        event.stage = Stage::Running;
        Some((slot, event.frame, event.kind, event.reconfigure))
    }

    fn finish_event(&mut self, slot: usize) {
        if let Some(event) = self.events.get_mut(slot) {
            *event = FREE;
        }
    }

    fn enter(&mut self) -> bool {
        if self.admission != Admission::Open {
            return false;
        }
        self.entries += 1;
        true
    }

    fn leave(&mut self) {
        self.entries = self.entries.saturating_sub(1);
    }

    fn claim_device(&mut self) -> bool {
        if self.admission == Admission::Closed || self.creating || self.device.is_some() {
            return false;
        }
        self.creating = true;
        true
    }

    fn can_borrow_device(&self) -> bool {
        self.admission != Admission::Closed
            && (self.entries != 0
                || self
                    .events
                    .iter()
                    .any(|event| event.stage == Stage::Running))
    }

    /// Stops the bridge; true for the first fault.
    fn fault(&mut self) -> bool {
        let first = !self.faulted;
        self.faulted = true;
        first
    }

    fn idle(&self) -> bool {
        self.entries == 0
            && !self.creating
            && !self.partial_submission
            && self.events.iter().all(|event| event.stage == Stage::Free)
    }

    fn record_image(&mut self, index: usize) -> bool {
        let Some(image) = self.images.get(index).copied().flatten() else {
            return false;
        };
        self.recorded_images |= 1 << index;
        image.initialized
    }

    fn accept_recorded_images(&mut self) {
        let mask = std::mem::take(&mut self.recorded_images);
        for (index, image) in self.images.iter_mut().enumerate() {
            if mask & (1 << index) != 0
                && let Some(image) = image
            {
                image.initialized = true;
            }
        }
    }

    fn upload_submitted(&mut self) {
        self.accept_recorded_images();
        self.partial_submission = true;
    }

    fn handback_fenced(&mut self) {
        self.accept_recorded_images();
        self.partial_submission = false;
    }

    fn close(&mut self) -> bool {
        if self.admission == Admission::Closed || !self.idle() {
            return false;
        }
        self.admission = Admission::Closed;
        true
    }

    fn reopen(&mut self) {
        self.admission = Admission::Open;
    }

    fn release(&mut self) {
        debug_assert_eq!(self.admission, Admission::Closed);
        debug_assert!(self.device.is_none() && self.surface.is_none());
        debug_assert!(self.images.iter().all(Option::is_none));
        *self = Self {
            admission: Admission::Closed,
            ..Self::new()
        };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_set_window(
    frame: *const Frame,
    instance: u64,
    device: *mut c_void,
    queue: u64,
    parent: *mut c_void,
) -> u32 {
    let Some(frame) = (unsafe { frame.as_ref() }) else {
        return 0;
    };
    let registry = presentation();
    let mut state = lock();
    // A new event supersedes any previous receipt, including when the target changed.
    state.window = None;
    if instance == 0 || device.is_null() || queue == 0 || parent.is_null() {
        return 0;
    }
    let Some(swapchain) = registry.window_target(instance, device as usize, parent as usize) else {
        return 0;
    };
    u32::from(state.set_window(PresentIdentity { queue, swapchain }, frame))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_admit_event(
    frame: *const Frame,
    kind: u32,
    reconfigure: u32,
    slot: *mut u32,
) -> *mut c_void {
    let Some(frame) = (unsafe { frame.as_ref() }) else {
        return ptr::null_mut();
    };
    if slot.is_null() {
        return ptr::null_mut();
    }
    let mut state = lock();
    let Some(index) = state.admit_event(frame, kind, reconfigure != 0) else {
        return ptr::null_mut();
    };
    unsafe { slot.write(index as u32) };
    state.event_address(index)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_start_event(
    address: *const c_void,
    frame: *mut Frame,
    kind: *mut u32,
    reconfigure: *mut u32,
    slot: *mut u32,
) -> u32 {
    if frame.is_null() || kind.is_null() || reconfigure.is_null() || slot.is_null() {
        return 0;
    }
    let Some((index, stored, event_kind, applies)) = lock().start_event(address) else {
        return 0;
    };
    unsafe {
        frame.write(stored);
        kind.write(event_kind);
        reconfigure.write(u32::from(applies));
        slot.write(index as u32);
    }
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_finish_event(slot: u32) {
    lock().finish_event(slot as usize);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_enter() -> u32 {
    u32::from(lock().enter())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_leave() {
    lock().leave();
}

/// The caller holds an entry or running event. While a claim is active, reentrant callers see
/// busy and close refuses. All the native creation, reporting and rollback run outside the lock.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_initialize(
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
    {
        let mut state = lock();
        if !state.can_borrow_device() {
            return DEVICE_BUSY;
        }
        if let Some(device) = &state.device {
            unsafe { view.write(device.handles) };
            return DEVICE_READY;
        }
        if !state.claim_device() {
            return DEVICE_BUSY;
        }
    }

    let mut device = SharedDevice::new(*ops);
    if !device.prepare(services) {
        drop(device);
        lock().creating = false;
        return DEVICE_FAILED;
    }
    device.epoch = (ops.ring_begin)();
    unsafe { (ops.report_created)(services) };
    let handles = device.handles;
    {
        // Only the creator can publish, and close refuses while creating. Swap with the
        // empty slot so even an old owner could never drop with the state mutex held.
        let mut pending = Some(device);
        let mut state = lock();
        std::mem::swap(&mut state.device, &mut pending);
        state.creating = false;
    }
    unsafe { view.write(handles) };
    DEVICE_READY
}

/// Copies a view under admission. The caller must retain the entry through its use or AddRef.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_device_view(view: *mut DeviceHandles) -> bool {
    let state = lock();
    if view.is_null() || !state.can_borrow_device() {
        return false;
    }
    let Some(device) = &state.device else {
        return false;
    };
    unsafe { view.write(device.handles) };
    true
}

/// The caller holds the presentation lease through recording and submission.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_command_slot(index: u32, view: *mut CommandSlot) -> bool {
    let state = lock();
    if view.is_null() || !state.open() || state.partial_submission {
        return false;
    }
    let Some(slot) = state
        .device
        .as_ref()
        .and_then(|device| device.slots.get(index as usize))
    else {
        return false;
    };
    unsafe { view.write(*slot) };
    true
}

/// Fence reads run outside the state mutex. The retirement close and presentation lease keep
/// the copied fences alive and prevent new submissions.
fn commands_retired() -> bool {
    let proof = {
        let state = lock();
        if state.admission != Admission::Closed
            || (state.device.is_none() && state.images.iter().any(Option::is_some))
        {
            return false;
        }
        state.device.as_ref().map(|device| {
            (
                device.handles,
                device.slots.map(|slot| slot.complete),
                device.ops.fence_ready,
                device.ops.shared_completed,
            )
        })
    };
    let Some((handles, fences, fence_ready, shared_completed)) = proof else {
        return rsf_unity_interop_drained(0) != 0;
    };
    if !fences
        .into_iter()
        .all(|fence| unsafe { fence_ready(&handles, fence) })
    {
        return false;
    }
    // D3D12 reports device removal as UINT64_MAX. It is not a completion value.
    let completed = unsafe { shared_completed(&handles) };
    completed != u64::MAX && rsf_unity_interop_drained(completed) != 0
}

/// The caller holds the presentation lease throughout native replacement.
fn prepare_image(state: &Mutex<State>, index: usize, key: ImageKey) -> Option<ImageHandles> {
    let lock = || state.lock().unwrap_or_else(PoisonError::into_inner);
    let (device, ops, replacing) = {
        let state = lock();
        if !state.open() || state.partial_submission {
            return None;
        }
        let device = state.device.as_ref()?;
        let current = state.images.get(index)?;
        if let Some(image) = current.filter(|image| image.key == key) {
            return Some(image.handles);
        }
        (device.handles, device.ops, current.is_some())
    };
    // Every slot shares this image. The current slot's fence only makes its command
    // resources reusable. Drain both APIs before replacing an image in the shared store.
    if replacing && !unsafe { (ops.drain_images)(&device) } {
        return None;
    }
    let mut handles = ImageHandles::EMPTY;
    if !unsafe {
        (ops.create_image)(
            &device,
            key.width,
            key.height,
            key.format,
            key.dx_format,
            &mut handles,
        )
    } || !handles.complete()
    {
        unsafe { (ops.destroy_image)(&device, &handles) };
        return None;
    }
    let previous = {
        let mut state = lock();
        if !state.open() || state.partial_submission {
            drop(state);
            unsafe { (ops.destroy_image)(&device, &handles) };
            return None;
        }
        state.images[index].replace(Image {
            key,
            handles,
            initialized: false,
        })
    };
    if let Some(previous) = previous {
        unsafe { (ops.destroy_image)(&device, &previous.handles) };
    }
    Some(handles)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_prepare_image(
    index: u32,
    width: u32,
    height: u32,
    format: u32,
    dx_format: u32,
    view: *mut ImageHandles,
) -> bool {
    if view.is_null() {
        return false;
    }
    let key = ImageKey {
        width,
        height,
        format,
        dx_format,
    };
    let Some(handles) = prepare_image(&STATE, index as usize, key) else {
        return false;
    };
    unsafe { view.write(handles) };
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_record_image(index: u32) -> bool {
    lock().record_image(index as usize)
}

/// Called after the Vulkan slot fence completed and all command buffers and lists reset.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_begin_recording(index: u32, epoch: *mut u64) -> u64 {
    let mut state = lock();
    state.recorded_images = 0;
    let Some(device) = &mut state.device else {
        return 0;
    };
    let Some(serial) = device.recordings.get_mut(index as usize) else {
        return 0;
    };
    let Some(next) = serial.checked_add(1) else {
        return 0;
    };
    *serial = next;
    unsafe { epoch.write(device.epoch) };
    next
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_upload_submitted() {
    lock().upload_submitted();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_handback_fenced() {
    lock().handback_fenced();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_fault() -> u32 {
    u32::from(lock().fault())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_faulted() -> u32 {
    u32::from(lock().faulted)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_stop_admission() {
    let mut state = lock();
    if state.admission == Admission::Open {
        state.admission = Admission::Retiring;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_try_close() -> bool {
    {
        let mut state = lock();
        if state.admission != Admission::Closed && !state.close() {
            return false;
        }
    }
    if rsf_unity_vulkan_presentation_try_begin() == 0 {
        return false;
    }
    if commands_retired() {
        return true;
    }
    end_lease();
    false
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_finish_retirement(commit: bool) {
    if commit {
        release_closed();
    }
    end_lease();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_open() {
    lock().reopen();
}

/// The close and lease still hold the proof. The destruction leaves are infallible.
fn release_closed() {
    let (images, surface, mut device) = {
        let mut state = lock();
        (
            std::mem::take(&mut state.images),
            state.surface.take(),
            state.device.take(),
        )
    };
    if let Some(device) = &mut device {
        if device.epoch != 0 {
            (device.ops.ring_retired)(device.epoch);
        }
        for image in images.into_iter().flatten() {
            unsafe { (device.ops.destroy_image)(&device.handles, &image.handles) };
        }
        device.retire_commands();
    }
    // Drop the chain before the shared queue and device it was created on.
    drop(surface);
    drop(device);
    rsf_unity_interop_release();
    lock().release();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_surface_ready() -> u32 {
    u32::from(lock().surface.is_some())
}

/// An entry keeps the shared device alive and the presentation lease serializes creation,
/// resize and every use of the borrowed chain. No native operation runs under a state lock.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_configure_surface(
    width: u32,
    height: u32,
    ops: *const SurfaceOps,
) -> *mut c_void {
    let Some(ops) = (unsafe { ops.as_ref() }) else {
        return ptr::null_mut();
    };
    let window = presentation().window;
    if window.is_null() {
        return ptr::null_mut();
    }
    let (device, current) = {
        let state = lock();
        if !state.can_borrow_device() {
            return ptr::null_mut();
        }
        let Some(device) = &state.device else {
            return ptr::null_mut();
        };
        (
            device.handles,
            state
                .surface
                .as_ref()
                .map(|surface| (surface.chain, surface.ops)),
        )
    };
    if let Some((chain, ops)) = current {
        let (mut current_width, mut current_height) = (0, 0);
        unsafe { (ops.extent)(chain, &mut current_width, &mut current_height) };
        if (width, height) == (current_width, current_height)
            || (unsafe { (ops.drain)(&device) } && unsafe { (ops.resize)(chain, width, height) })
        {
            return chain;
        }
        // A refused drain or resize keeps the owned chain for the next Present to retry.
        return ptr::null_mut();
    }
    let chain = unsafe { (ops.create)(&device, window, width, height) };
    if !chain.is_null() {
        // Only the holder of the presentation lease can publish a surface.
        let mut pending = Some(Surface { chain, ops: *ops });
        std::mem::swap(&mut lock().surface, &mut pending);
    }
    chain
}

/// One intercepted Unity swapchain: its extent, its images and the present-ready semaphore the
/// bridge created for each image. Handles are Vulkan's, kept as identity only.
struct Swapchain {
    device: usize,
    chain: u64,
    detached: bool,
    active: bool,
    surface: u64,
    publication: u64,
    width: u32,
    height: u32,
    images: Vec<u64>,
    present_ready: Vec<u64>,
}

/// The presented image of a leased swapchain (rsf_unity_vulkan_present_source). Written for
/// vulkan_bridge.cpp; only the tests read it here.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
struct PresentSource {
    image: u64,
    present_ready: u64,
    width: u32,
    height: u32,
}
const _: () = assert!(size_of::<PresentSource>() == 24);

struct Presentation {
    /// The presentation window, created once on the window thread and kept for the process.
    window: *mut c_void,
    /// The presentation lease is held.
    leased: bool,
    swapchains: Vec<Swapchain>,
    surfaces: Vec<(u64, u64, usize)>,
    next_publication: u64,
}

// The window is a Win32 handle kept as identity; this module never dereferences it.
unsafe impl Send for Presentation {}

static PRESENTATION: Mutex<Presentation> = Mutex::new(Presentation::new());
/// Wakes the threads waiting for the presentation lease.
static LEASE_ENDED: Condvar = Condvar::new();

fn presentation() -> MutexGuard<'static, Presentation> {
    PRESENTATION.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The presentation once no lease is held. Waits as the C++ mutex did.
fn unleased() -> MutexGuard<'static, Presentation> {
    let mut presentation = presentation();
    while presentation.leased {
        presentation = LEASE_ENDED
            .wait(presentation)
            .unwrap_or_else(PoisonError::into_inner);
    }
    presentation
}

fn end_lease() {
    presentation().leased = false;
    LEASE_ENDED.notify_all();
}

impl Presentation {
    const fn new() -> Self {
        Self {
            window: ptr::null_mut(),
            leased: false,
            swapchains: Vec::new(),
            surfaces: Vec::new(),
            next_publication: 1,
        }
    }

    /// Keeps the first window; true when window is the kept one.
    fn set_window(&mut self, window: *mut c_void) -> bool {
        if window.is_null() {
            return false;
        }
        if self.window.is_null() {
            self.window = window;
        }
        self.window == window
    }

    /// Publishes a complete swapchain: one present-ready semaphore per image, a new handle.
    fn register(&mut self, mut swapchain: Swapchain) -> bool {
        if self.next_publication == 0
            || swapchain.chain == 0
            || swapchain.images.is_empty()
            || swapchain.images.len() != swapchain.present_ready.len()
            || self.swapchains.iter().any(|known| {
                !known.detached
                    && known.device == swapchain.device
                    && known.chain == swapchain.chain
            })
            || self.swapchains.try_reserve(1).is_err()
        {
            return false;
        }
        swapchain.publication = self.next_publication;
        self.next_publication = self.next_publication.wrapping_add(1);
        self.swapchains.push(swapchain);
        true
    }

    fn window_target(&self, instance: u64, device: usize, parent: usize) -> Option<u64> {
        let mut active = self.swapchains.iter().filter(|chain| {
            !chain.detached
                && chain.active
                && chain.device == device
                && self.surfaces.contains(&(instance, chain.surface, parent))
        });
        let target = active.next()?;
        active.next().is_none().then_some(target.publication)
    }

    fn source(&self, device: usize, chain: u64, index: u32) -> Option<(u64, PresentSource)> {
        let swapchain = self.swapchains.iter().find(|known| {
            !known.detached && known.active && known.device == device && known.chain == chain
        })?;
        let index = index as usize;
        Some((
            swapchain.publication,
            PresentSource {
                image: *swapchain.images.get(index)?,
                present_ready: swapchain.present_ready[index],
                width: swapchain.width,
                height: swapchain.height,
            },
        ))
    }

    /// The native swapchain can disappear even if an idle wait fails. Its semaphores remain
    /// owned, but it can no longer supply a present source or block reuse of its handle.
    fn detach(&mut self, device: usize, chain: u64) {
        for known in &mut self.swapchains {
            if known.device == device && known.chain == chain {
                known.detached = true;
            }
        }
    }
}

/// Vulkan idle wait, followed by destruction of each detached batch on success or device loss.
type WaitIdle = unsafe extern "C" fn(device: *mut c_void) -> i32;
type DestroyReady = unsafe extern "C" fn(device: *mut c_void, ready: *const u64, count: u32);
const VK_SUCCESS: i32 = 0;
const VK_ERROR_DEVICE_LOST: i32 = -4;

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_set_surface_window(window: *mut c_void) -> u32 {
    u32::from(presentation().set_window(window))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_surface_window() -> *mut c_void {
    presentation().window
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_surface_register(instance: u64, surface: u64, parent: *mut c_void) {
    let mut registry = presentation();
    if registry.surfaces.try_reserve(1).is_ok() {
        registry.surfaces.push((instance, surface, parent as usize));
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_surface_forget(instance: u64, surface: u64) {
    presentation()
        .surfaces
        .retain(|&(owner, handle, _)| owner != instance || handle != surface);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_swapchain_supersede(device: *mut c_void, chain: u64) {
    if chain == 0 {
        return;
    }
    let mut registry = unleased();
    for known in &mut registry.swapchains {
        if known.device == device as usize && known.chain == chain {
            known.active = false;
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_swapchain_register(
    device: *mut c_void,
    chain: u64,
    surface: u64,
    width: u32,
    height: u32,
    images: *const u64,
    present_ready: *const u64,
    count: u32,
) -> u32 {
    if device.is_null() || images.is_null() || present_ready.is_null() || count == 0 {
        return 0;
    }
    let count = count as usize;
    let mut image_copy = Vec::new();
    let mut semaphore_copy = Vec::new();
    if image_copy.try_reserve_exact(count).is_err()
        || semaphore_copy.try_reserve_exact(count).is_err()
    {
        return 0;
    }
    // Allocation refusal leaves the native rollback owner in charge of every handle.
    image_copy.extend_from_slice(unsafe { slice::from_raw_parts(images, count) });
    semaphore_copy.extend_from_slice(unsafe { slice::from_raw_parts(present_ready, count) });
    let mut registry = unleased();
    let swapchain = Swapchain {
        device: device as usize,
        chain,
        detached: false,
        active: true,
        surface,
        publication: 0,
        width,
        height,
        images: image_copy,
        present_ready: semaphore_copy,
    };
    u32::from(registry.register(swapchain))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_swapchain_retire(
    device: *mut c_void,
    chain: u64,
    wait_idle: WaitIdle,
    destroy: DestroyReady,
) -> i32 {
    let mut registry = unleased();
    registry.detach(device as usize, chain);
    if !registry
        .swapchains
        .iter()
        .any(|known| known.device == device as usize && known.detached)
    {
        return VK_SUCCESS;
    }
    registry.leased = true;
    drop(registry);

    let result = unsafe { wait_idle(device) };
    // Device loss permits explicit teardown. An allocation failure or any other result leaves
    // the detached records available for a later retirement attempt on this device.
    if matches!(result, VK_SUCCESS | VK_ERROR_DEVICE_LOST) {
        loop {
            let retired = {
                let mut registry = presentation();
                registry
                    .swapchains
                    .iter()
                    .position(|known| known.device == device as usize && known.detached)
                    .map(|index| registry.swapchains.swap_remove(index))
            };
            let Some(retired) = retired else { break };
            unsafe {
                destroy(
                    device,
                    retired.present_ready.as_ptr(),
                    retired.present_ready.len() as u32,
                )
            };
        }
    }
    end_lease();
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_present_begin(
    device: *mut c_void,
    queue: u64,
    chain: u64,
    index: u32,
    source: *mut PresentSource,
    frame: *mut Frame,
) -> u32 {
    let mut registry = unleased();
    registry.leased = true;
    if source.is_null() || frame.is_null() {
        return 0;
    }
    let Some((swapchain, found)) = registry.source(device as usize, chain, index) else {
        return 0;
    };
    let Some(receipt) = lock().take_window(PresentIdentity { queue, swapchain }) else {
        return 0;
    };
    unsafe {
        source.write(found);
        frame.write(receipt);
    }
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_presentation_try_begin() -> u32 {
    let mut presentation = presentation();
    if presentation.leased {
        return 0;
    }
    presentation.leased = true;
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_presentation_end() {
    end_lease();
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENT_SR: u32 = 0;
    const IDENTITY: PresentIdentity = PresentIdentity {
        queue: 42,
        swapchain: 1,
    };

    fn work(frame_id: u64) -> Frame {
        let mut frame = EMPTY_FRAME;
        frame.packet.frame_id = frame_id;
        frame.services.session_id = frame_id + 100;
        frame
    }

    #[test]
    fn shared_image_resize_preserves_old_owner_while_another_slot_is_outstanding() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

        static VULKAN_IDLE: AtomicBool = AtomicBool::new(false);
        static D3D12_IDLE: AtomicBool = AtomicBool::new(false);
        static DESTROYED: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn drain(_: *const DeviceHandles) -> bool {
            VULKAN_IDLE.load(Relaxed) && D3D12_IDLE.load(Relaxed)
        }
        unsafe extern "C" fn create_image(
            _: *const DeviceHandles,
            width: u32,
            _: u32,
            _: u32,
            _: u32,
            output: *mut ImageHandles,
        ) -> bool {
            unsafe {
                output.write(ImageHandles {
                    resource: ptr::dangling_mut::<u8>().cast(),
                    image: u64::from(width),
                    memory: 1,
                });
            }
            true
        }
        unsafe extern "C" fn destroy_image(_: *const DeviceHandles, image: *const ImageHandles) {
            DESTROYED.store(unsafe { (*image).image } as usize, Relaxed);
        }
        let ops = DeviceOps {
            create: unused_create,
            destroy: unused_device,
            create_pool: unused_pool,
            destroy_pool: unused_handle,
            create_slot: unused_slot,
            destroy_slot: unused_destroy_slot,
            create_image,
            destroy_image,
            fence_ready: unused_fence,
            shared_completed: unused_pool,
            drain_images: drain,
            report_created: unused_report,
            ring_begin: unused_ring_begin,
            ring_retired: unused_ring_retired,
        };
        unsafe extern "C" fn unused_create(
            _: *const rsf_game_host_services,
            _: *mut DeviceHandles,
        ) -> bool {
            false
        }
        unsafe extern "C" fn unused_device(_: *const DeviceHandles) {}
        unsafe extern "C" fn unused_pool(_: *const DeviceHandles) -> u64 {
            0
        }
        unsafe extern "C" fn unused_handle(_: *const DeviceHandles, _: u64) {}
        unsafe extern "C" fn unused_slot(
            _: *const DeviceHandles,
            _: u64,
            _: *mut CommandSlot,
        ) -> bool {
            false
        }
        unsafe extern "C" fn unused_destroy_slot(_: *const DeviceHandles, _: *const CommandSlot) {}
        unsafe extern "C" fn unused_fence(_: *const DeviceHandles, _: u64) -> bool {
            false
        }
        unsafe extern "C" fn unused_report(_: *const rsf_game_host_services) {}
        extern "C" fn unused_ring_begin() -> u64 {
            0
        }
        extern "C" fn unused_ring_retired(_: u64) {}

        let state = Mutex::new(State {
            device: Some(SharedDevice::new(ops)),
            ..State::new()
        });
        let old = ImageKey {
            width: 1280,
            height: 720,
            format: 1,
            dx_format: 1,
        };
        let new = ImageKey { width: 1920, ..old };
        assert_eq!(prepare_image(&state, 0, old).unwrap().image, 1280);

        // Slot 1 is free to record. Slot 0 still holds both Vulkan and D3D12 users.
        assert!(prepare_image(&state, 0, new).is_none());
        VULKAN_IDLE.store(true, Relaxed);
        assert!(prepare_image(&state, 0, new).is_none());
        assert_eq!(prepare_image(&state, 0, old).unwrap().image, 1280);
        assert_eq!(DESTROYED.load(Relaxed), 0);
        D3D12_IDLE.store(true, Relaxed);
        assert_eq!(prepare_image(&state, 0, new).unwrap().image, 1920);
        assert_eq!(DESTROYED.load(Relaxed), 1280);
    }

    #[test]
    fn an_accepted_upload_publishes_image_layout_and_blocks_release_until_fenced() {
        let mut state = State::new();
        state.images[0] = Some(Image {
            key: ImageKey {
                width: 400,
                height: 300,
                format: 97,
                dx_format: 10,
            },
            handles: ImageHandles::EMPTY,
            initialized: false,
        });
        assert!(!state.record_image(0));
        assert!(!state.images[0].unwrap().initialized);
        state.upload_submitted();
        assert!(state.images[0].unwrap().initialized);
        assert!(!state.idle());
        assert!(!state.close());
        state.handback_fenced();
        assert!(state.close());
    }

    #[test]
    fn events_stay_outstanding_until_their_callback_finishes() {
        let mut state = State::new();
        let slot = state.admit_event(&work(4), EVENT_HUDLESS, false).unwrap();
        let address = state.event_address(slot);
        assert!(state.set_window(IDENTITY, &work(5)));
        assert!(!state.idle());
        let (started, stored, kind, reconfigure) = state.start_event(address).unwrap();
        assert_eq!(
            (started, stored.packet.frame_id, kind, reconfigure),
            (slot, 4, EVENT_HUDLESS, false)
        );
        assert_eq!(stored.services.session_id, 104);
        assert_eq!(
            state.take_window(IDENTITY).unwrap().services.session_id,
            105
        );
        // A running event is not started twice and still blocks retirement.
        assert!(state.start_event(address).is_none());
        assert!(!state.idle());
        state.finish_event(slot);
        assert!(state.idle());
        assert!(state.start_event(address).is_none());
        assert!(
            state
                .admit_event(&work(5), EVENT_HUDLESS + 1, false)
                .is_none()
        );
    }

    #[test]
    fn a_full_table_refuses_events() {
        let mut state = State::new();
        for frame in 0..EVENTS {
            assert_eq!(
                state.admit_event(&work(frame as u64 + 1), EVENT_SR, true),
                Some(frame)
            );
        }
        assert!(state.admit_event(&work(99), EVENT_SR, false).is_none());
        state.finish_event(EVENTS);
        state.finish_event(3);
        assert_eq!(state.admit_event(&work(99), EVENT_SR, false), Some(3));
    }

    #[test]
    fn a_fault_discards_queued_events_and_the_window_frame() {
        let mut state = State::new();
        assert!(state.set_window(IDENTITY, &work(8)));
        let slot = state.admit_event(&work(9), EVENT_SR, false).unwrap();
        assert!(state.fault());
        assert!(!state.fault());
        assert!(state.take_window(IDENTITY).is_none());
        assert!(!state.set_window(IDENTITY, &work(10)));
        assert!(state.admit_event(&work(11), EVENT_SR, false).is_none());
        assert!(state.start_event(state.event_address(slot)).is_none());
        assert!(state.idle());
        assert!(state.close());
        state.release();
        assert!(!state.faulted);
        assert!(state.take_window(IDENTITY).is_none());
    }

    #[test]
    fn the_window_frame_needs_a_frame_identity() {
        let mut state = State::new();
        assert!(state.take_window(IDENTITY).is_none());
        assert!(!state.set_window(IDENTITY, &work(0)));
        assert!(state.take_window(IDENTITY).is_none());
        assert!(state.set_window(IDENTITY, &work(12)));
        assert!(
            state
                .take_window(PresentIdentity {
                    queue: 43,
                    ..IDENTITY
                })
                .is_none()
        );
        assert!(
            state
                .take_window(PresentIdentity {
                    swapchain: 2,
                    ..IDENTITY
                })
                .is_none()
        );
        assert_eq!(state.take_window(IDENTITY).unwrap().packet.frame_id, 12);
        assert!(state.take_window(IDENTITY).is_none());
    }

    #[test]
    fn one_thread_creates_the_device() {
        let mut state = State::new();
        assert!(state.claim_device());
        assert!(!state.claim_device());
        assert!(!state.idle());
        assert!(!state.close());
        state.creating = false;
        assert!(state.idle());
        assert!(state.claim_device());
    }

    #[test]
    fn close_waits_for_entries_and_blocks_new_work() {
        let mut state = State::new();
        assert!(state.enter());
        state.admission = Admission::Retiring;
        assert!(!state.enter());
        assert!(!state.close());
        state.leave();
        assert!(state.close());
        assert!(!state.close());
        assert!(!state.enter());
        assert!(!state.claim_device());
        assert!(state.admit_event(&work(1), EVENT_SR, false).is_none());
        assert!(!state.set_window(IDENTITY, &work(1)));
        state.reopen();
        assert!(state.enter());
        state.leave();
        assert!(state.close());
        state.release();
        assert!(!state.enter());
        state.reopen();
        assert!(state.claim_device());
    }

    // The only test that touches the process-wide state.
    #[test]
    fn exports_check_their_pointers() {
        let frame = work(21);
        let mut slot = u32::MAX;
        let address = unsafe { rsf_unity_vulkan_admit_event(&frame, EVENT_SR, 1, &mut slot) };
        assert!(!address.is_null());
        assert_eq!(slot, 0);
        assert!(
            unsafe { rsf_unity_vulkan_admit_event(ptr::null(), EVENT_SR, 0, &mut slot) }.is_null()
        );
        let (mut out, mut kind, mut reconfigure, mut index) = (EMPTY_FRAME, 9, 9, 9);
        assert_eq!(
            unsafe {
                rsf_unity_vulkan_start_event(
                    address,
                    ptr::null_mut(),
                    &mut kind,
                    &mut reconfigure,
                    &mut index,
                )
            },
            0
        );
        assert_eq!(
            unsafe {
                rsf_unity_vulkan_start_event(
                    address,
                    &mut out,
                    &mut kind,
                    &mut reconfigure,
                    &mut index,
                )
            },
            1
        );
        assert_eq!(
            (out.packet.frame_id, kind, reconfigure, index),
            (21, EVENT_SR, 1, 0)
        );
        assert!(!lock().idle());
        rsf_unity_vulkan_finish_event(index);
        assert!(lock().idle());
        assert_eq!(rsf_unity_vulkan_enter(), 1);
        assert!(!lock().close());
        rsf_unity_vulkan_leave();
        assert_eq!(
            unsafe {
                rsf_unity_vulkan_set_window(ptr::null(), 0, ptr::null_mut(), 0, ptr::null_mut())
            },
            0
        );
        assert!(lock().close());
        lock().release();
        assert_eq!(rsf_unity_vulkan_enter(), 0);
        rsf_unity_vulkan_open();
    }

    fn swapchain(chain: u64, count: u64) -> Swapchain {
        Swapchain {
            device: 1,
            chain,
            detached: false,
            active: true,
            surface: 33,
            publication: 0,
            width: 640,
            height: 360,
            images: (1..=count).map(|image| chain * 100 + image).collect(),
            present_ready: (1..=count).map(|ready| chain * 1000 + ready).collect(),
        }
    }

    #[test]
    fn swapchains_publish_complete_and_once() {
        let mut presentation = Presentation::new();
        assert!(!presentation.register(swapchain(0, 2)));
        assert!(!presentation.register(swapchain(3, 0)));
        let mut uneven = swapchain(3, 2);
        uneven.present_ready.pop();
        assert!(!presentation.register(uneven));
        assert!(presentation.register(swapchain(3, 2)));
        assert!(!presentation.register(swapchain(3, 3)));
        assert_eq!(
            presentation.source(1, 3, 1),
            Some((
                1,
                PresentSource {
                    image: 302,
                    present_ready: 3002,
                    width: 640,
                    height: 360,
                }
            ))
        );
        assert!(presentation.source(1, 3, 2).is_none());
        assert!(presentation.source(1, 4, 0).is_none());
        presentation.surfaces.push((55, 33, 99));
        assert_eq!(presentation.window_target(55, 1, 99), Some(1));
        assert_eq!(presentation.window_target(56, 1, 99), None);
        assert_eq!(presentation.window_target(55, 1, 100), None);
        presentation.detach(1, 3);
        assert!(presentation.register(swapchain(3, 2)));
        assert_eq!(presentation.window_target(55, 1, 99), Some(2));
        presentation.detach(1, 3);
        assert!(presentation.source(1, 3, 0).is_none());
    }

    #[test]
    fn the_first_window_is_kept() {
        let mut presentation = Presentation::new();
        let (first, second) = (
            ptr::without_provenance_mut(8),
            ptr::without_provenance_mut(16),
        );
        assert!(!presentation.set_window(ptr::null_mut()));
        assert!(presentation.set_window(first));
        assert!(presentation.set_window(first));
        assert!(!presentation.set_window(second));
        assert_eq!(presentation.window, first);
    }

    static RETIRED: Mutex<(Vec<u64>, bool)> = Mutex::new((Vec::new(), false));

    unsafe extern "C" fn retire(_: *mut c_void, present_ready: *const u64, count: u32) {
        let handles = unsafe { slice::from_raw_parts(present_ready, count as usize) }.to_vec();
        // The lease is held while the semaphores are destroyed.
        let leased = rsf_unity_vulkan_presentation_try_begin() == 0;
        *RETIRED.lock().unwrap() = (handles, leased);
    }

    // The only test that touches the process-wide presentation.
    #[test]
    fn the_presentation_lease_is_exclusive() {
        let device = ptr::without_provenance_mut(1);
        let (images, ready) = ([71, 72], [81, 82]);
        assert_eq!(
            unsafe {
                rsf_unity_vulkan_swapchain_register(
                    device,
                    7,
                    33,
                    4,
                    2,
                    images.as_ptr(),
                    ready.as_ptr(),
                    2,
                )
            },
            1
        );
        assert_eq!(
            unsafe {
                rsf_unity_vulkan_swapchain_register(
                    device,
                    8,
                    33,
                    4,
                    2,
                    ptr::null(),
                    ready.as_ptr(),
                    2,
                )
            },
            0
        );
        let mut source = PresentSource::default();
        let mut frame = EMPTY_FRAME;
        // A present without a receipt still holds the lease while forwarding.
        assert_eq!(
            unsafe { rsf_unity_vulkan_present_begin(device, 42, 7, 1, &mut source, &mut frame) },
            0
        );
        assert_eq!(rsf_unity_vulkan_presentation_try_begin(), 0);
        rsf_unity_vulkan_presentation_end();
        unsafe extern "C" fn failed(_: *mut c_void) -> i32 {
            -1
        }
        unsafe extern "C" fn lost(_: *mut c_void) -> i32 {
            VK_ERROR_DEVICE_LOST
        }
        assert_eq!(
            unsafe { rsf_unity_vulkan_swapchain_retire(device, 7, failed, retire) },
            -1
        );
        assert!(RETIRED.lock().unwrap().0.is_empty());
        assert!(presentation().source(1, 7, 1).is_none());
        assert_eq!(
            unsafe { rsf_unity_vulkan_swapchain_retire(device, 7, lost, retire) },
            VK_ERROR_DEVICE_LOST
        );
        assert_eq!(*RETIRED.lock().unwrap(), (vec![81, 82], true));
        assert_eq!(rsf_unity_vulkan_presentation_try_begin(), 1);
        rsf_unity_vulkan_presentation_end();
    }
}
