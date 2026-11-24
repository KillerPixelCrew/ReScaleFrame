// SPDX-License-Identifier: MIT
//! State of the Unity package's Vulkan bridge, whose Unity SDK calls and foreign guards are the
//! C11 leaf integrations/unity/native/vulkan_bridge.c. That bridge copies Unity's Vulkan images
//! into same-adapter D3D12 resources, records the render pass on its own D3D12 queue and replaces
//! Unity's present with its own DXGI swap chain.
//!
//! This module owns the bridge's admission and lifecycle: the shared device's readiness and its
//! single creator, the permanent fault, the render events queued to Unity and their packets, the
//! present interceptions and surface updates in progress, the player window frame the present
//! renders under, the bridge's DXGI swap chain and resize, and the checked close and release.
//! One record owns the D3D12 device, queue and fence, the imported semaphore and commands, and
//! retains the Unity Vulkan device facts that shared images and commands need. Slot rotation and
//! shared fence values follow the Vulkan route's Runtime submission handle.
//!
//! The presentation state outlives a release, because Unity owns the swapchains and the window
//! stays: Unity's intercepted swapchains (their images and the present-ready semaphores the bridge
//! created for them), the presentation window's identity, and the presentation lease. The lease is
//! the one exclusive hold: present interceptions, swapchain retirement and
//! the swap chain's creation on the window thread run one at a time, a present keeps it through
//! the forwarded vkQueuePresentKHR, and retirement keeps it through vkDeviceWaitIdle and the
//! semaphore destruction, which need host access to the device's queues.
//!
//! vulkan_bridge.rs runs frame and HWND presentation through the typed platform owner.
//! The native boundary calls Unity's versioned interface and guards foreign callbacks.
//! Callbacks run outside the state mutex.
//! The orchestrator's Vulkan frame-generation swapchain owners have their own presentation
//! tables and this crate does not link them, so the registry here is the bridge's own.
use crate::bridge_interop::VULKAN;
use crate::device_creation::{Claim, Creation};
use crate::sys::{RSF_UNITY_FRAMES_PACKETS, RSF_UNITY_FRAMES_SLOTS, rsf_unity_packet};
pub(super) use crate::sys::{
    rsf_unity_vulkan_command_handles as CommandSlot,
    rsf_unity_vulkan_device_handles as DeviceHandles,
    rsf_unity_vulkan_image_handles as ImageHandles,
};
use crate::vulkan_resolver::{DEVICE_LOST, SUCCESS};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

// Vulkan routing over the Unity SDK leaves in integrations/unity/native, which need the untracked
// Unity native headers; unit tests cover the packet state above it.
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
#[path = "vulkan_bridge.rs"]
pub(crate) mod bridge;

const EVENTS: usize = RSF_UNITY_FRAMES_PACKETS as usize;
const SLOTS: usize = RSF_UNITY_FRAMES_SLOTS as usize;
const EVENT_HUDLESS: u32 = 2;
const SHARED_IMAGES: usize = 6;

#[derive(Clone, Copy)]
struct Frame {
    packet: rsf_unity_packet,
    services: rsf_game_host_services,
}

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
    #[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
    textures: bridge::Textures,
}

const FREE: Event = Event {
    frame: EMPTY_FRAME,
    kind: 0,
    reconfigure: false,
    stage: Stage::Free,
    #[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
    textures: bridge::Textures::EMPTY,
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
    #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
    initialized: bool,
}

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

// The pool owns the two Vulkan buffers. The slot owns the fence and the COM references.
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

// The COM references, dispatch and imported semaphore transfer to Rust. Unity lends the
// Vulkan device and queue for the duration of the shared-device owner.
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
        dispatch: ptr::null_mut(),
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

// The device and surface leaves: bridge::ops in production, the test fakes under test.
#[cfg(not(test))]
use bridge::ops;
#[cfg(test)]
use tests::ops;

struct SharedDevice {
    handles: DeviceHandles,
    pool: u64,
    slots: [CommandSlot; SLOTS],
    epoch: u64,
    #[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
    recordings: [u64; SLOTS],
}

impl SharedDevice {
    fn new() -> Self {
        Self {
            handles: DeviceHandles::EMPTY,
            pool: 0,
            slots: [CommandSlot::EMPTY; SLOTS],
            epoch: 0,
            #[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
            recordings: [0; SLOTS],
        }
    }

    fn prepare(&mut self, services: *const rsf_game_host_services) -> bool {
        if !unsafe { ops::create_device(services, &mut self.handles) } || !self.handles.complete() {
            return false;
        }
        self.pool = unsafe { ops::create_pool(&self.handles) };
        self.pool != 0
            && self.slots.iter_mut().all(|slot| {
                let created = unsafe { ops::create_slot(&self.handles, self.pool, slot) };
                created && slot.complete()
            })
    }

    fn retire_commands(&mut self) {
        if self.pool != 0 {
            for slot in &self.slots {
                unsafe { ops::destroy_slot(&self.handles, slot) };
            }
            unsafe { ops::destroy_pool(&self.handles, std::mem::take(&mut self.pool)) };
        }
    }
}

impl Drop for SharedDevice {
    fn drop(&mut self) {
        self.retire_commands();
        unsafe { ops::destroy_device(&self.handles) };
    }
}

struct Surface {
    chain: *mut c_void,
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: Creation transferred one IDXGISwapChain3 reference to this owner.
        unsafe { ops::chain_destroy(self.chain) };
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
    creation: Creation<SharedDevice>,
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
    #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
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
    lock_or_recover(&STATE)
}

impl State {
    const fn new() -> Self {
        Self {
            creation: Creation::new(),
            faulted: false,
            admission: Admission::Open,
            events: [FREE; EVENTS],
            entries: 0,
            window: None,
            surface: None,
            images: [None; SHARED_IMAGES],
            #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
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
            #[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
            textures: bridge::Textures::EMPTY,
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

    fn claim_device(&mut self) -> Claim<DeviceHandles> {
        let admitted = self.can_borrow_device();
        self.creation.claim(admitted, |device| device.handles)
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
            && !self.creation.creating
            && !self.partial_submission
            && self.events.iter().all(|event| event.stage == Stage::Free)
    }

    #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
    fn record_image(&mut self, index: usize) -> bool {
        let Some(image) = self.images.get(index).copied().flatten() else {
            return false;
        };
        self.recorded_images |= 1 << index;
        image.initialized
    }

    #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
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

    #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
    fn upload_submitted(&mut self) {
        self.accept_recorded_images();
        self.partial_submission = true;
    }

    #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
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
        debug_assert!(self.creation.device.is_none() && self.surface.is_none());
        debug_assert!(self.images.iter().all(Option::is_none));
        *self = Self {
            admission: Admission::Closed,
            ..Self::new()
        };
    }
}

fn publish_window(
    frame: &Frame,
    instance: u64,
    device: *mut c_void,
    queue: u64,
    parent: *mut c_void,
) -> bool {
    let registry = presentation();
    let mut state = lock();
    // A new event supersedes any previous receipt, including when the target changed.
    state.window = None;
    if instance == 0 || device.is_null() || queue == 0 || parent.is_null() {
        return false;
    }
    let Some(swapchain) = registry.window_target(instance, device as usize, parent as usize) else {
        return false;
    };
    state.set_window(PresentIdentity { queue, swapchain }, frame)
}

enum Device {
    Ready(DeviceHandles),
    Busy,
    Failed,
}

/// The shared device for the caller, who holds an entry or running event. While a claim is
/// active, reentrant callers see busy and close refuses. All the native creation, reporting and
/// rollback run outside the lock.
fn initialize_device(services: *const rsf_game_host_services) -> Device {
    let claim = lock().claim_device();
    match claim {
        Claim::Ready(handles) => return Device::Ready(handles),
        Claim::Busy => return Device::Busy,
        Claim::Stopped => return Device::Failed,
        Claim::Create => {}
    }

    let mut device = SharedDevice::new();
    if !device.prepare(services) {
        drop(device);
        lock().creation.rolled_back(false);
        return Device::Failed;
    }
    device.epoch = ops::ring_begin();
    unsafe { ops::report_created(services) };
    let handles = device.handles;
    let previous = lock().creation.publish(device);
    drop(previous);
    Device::Ready(handles)
}

/// A view borrowed under admission. The caller must retain the entry through its use or AddRef.
fn device_view() -> Option<DeviceHandles> {
    let state = lock();
    if !state.can_borrow_device() {
        return None;
    }
    state.creation.device.as_ref().map(|device| device.handles)
}

/// The caller holds the presentation lease through recording and submission.
fn command_slot(index: u32) -> Option<CommandSlot> {
    let state = lock();
    if !state.open() || state.partial_submission {
        return None;
    }
    state
        .creation
        .device
        .as_ref()
        .and_then(|device| device.slots.get(index as usize))
        .copied()
}

/// Fence reads run outside the state mutex. The retirement close and presentation lease keep
/// the copied fences alive and prevent new submissions.
fn commands_retired() -> bool {
    let proof = {
        let state = lock();
        if state.admission != Admission::Closed
            || (state.creation.device.is_none() && state.images.iter().any(Option::is_some))
        {
            return false;
        }
        state
            .creation
            .device
            .as_ref()
            .map(|device| (device.handles, device.slots.map(|slot| slot.complete)))
    };
    let Some((handles, fences)) = proof else {
        return VULKAN.lock().drained(0);
    };
    if !fences
        .into_iter()
        .all(|fence| unsafe { ops::fence_ready(&handles, fence) })
    {
        return false;
    }
    // D3D12 reports device removal as UINT64_MAX. It is not a completion value.
    let completed = unsafe { ops::shared_completed(&handles) };
    VULKAN.lock().drained(completed)
}

/// The caller holds the presentation lease throughout native replacement.
fn prepare_image(state: &Mutex<State>, index: usize, key: ImageKey) -> Option<ImageHandles> {
    let lock = || lock_or_recover(state);
    let (device, replacing) = {
        let state = lock();
        if !state.open() || state.partial_submission {
            return None;
        }
        let device = state.creation.device.as_ref()?;
        let current = state.images.get(index)?;
        if let Some(image) = current.filter(|image| image.key == key) {
            return Some(image.handles);
        }
        (device.handles, current.is_some())
    };
    // Every slot shares this image. The current slot's fence only makes its command
    // resources reusable. Drain both APIs before replacing an image in the shared store.
    if replacing && !unsafe { ops::drain(&device) } {
        return None;
    }
    let mut handles = ImageHandles::EMPTY;
    if !unsafe {
        ops::create_image(
            &device,
            key.width,
            key.height,
            key.format,
            key.dx_format,
            &mut handles,
        )
    } || !handles.complete()
    {
        unsafe { ops::destroy_image(&device, &handles) };
        return None;
    }
    let previous = {
        let mut state = lock();
        if !state.open() || state.partial_submission {
            drop(state);
            unsafe { ops::destroy_image(&device, &handles) };
            return None;
        }
        state.images[index].replace(Image {
            key,
            handles,
            #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
            initialized: false,
        })
    };
    if let Some(previous) = previous {
        unsafe { ops::destroy_image(&device, &previous.handles) };
    }
    Some(handles)
}

#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
pub(super) fn record_image(index: u32) -> bool {
    lock().record_image(index as usize)
}

/// Called after the Vulkan slot fence completed and all command buffers and lists reset.
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
pub(super) fn begin_recording(index: u32) -> Option<(u64, u64)> {
    let mut state = lock();
    state.recorded_images = 0;
    let device = state.creation.device.as_mut()?;
    let serial = device.recordings.get_mut(index as usize)?;
    *serial = serial.checked_add(1)?;
    (device.epoch != 0).then_some((device.epoch, *serial))
}

#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
pub(super) fn upload_submitted() {
    lock().upload_submitted();
}

#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
pub(super) fn handback_fenced() {
    lock().handback_fenced();
}

fn faulted() -> bool {
    lock().faulted
}

pub(crate) fn stop_admission() {
    let mut state = lock();
    if state.admission == Admission::Open {
        state.admission = Admission::Retiring;
    }
}

pub(crate) fn try_close() -> bool {
    {
        let mut state = lock();
        if state.admission != Admission::Closed && !state.close() {
            return false;
        }
    }
    if !try_begin_lease() {
        return false;
    }
    if commands_retired() {
        return true;
    }
    end_lease();
    false
}

pub(crate) fn finish_retirement(commit: bool) {
    if commit {
        release_closed();
    }
    end_lease();
}

pub(crate) fn open() {
    lock().reopen();
}

/// The close and lease still hold the proof. The destruction leaves are infallible.
fn release_closed() {
    let (images, surface, mut device) = {
        let mut state = lock();
        (
            std::mem::take(&mut state.images),
            state.surface.take(),
            state.creation.device.take(),
        )
    };
    if let Some(device) = &mut device {
        if device.epoch != 0 {
            ops::ring_retired(device.epoch);
        }
        for image in images.into_iter().flatten() {
            unsafe { ops::destroy_image(&device.handles, &image.handles) };
        }
        device.retire_commands();
    }
    // Drop the chain before the shared queue and device it was created on.
    drop(surface);
    drop(device);
    VULKAN.reset();
    lock().release();
}

fn surface_ready() -> bool {
    lock().surface.is_some()
}

/// An entry keeps the shared device alive and the presentation lease serializes creation,
/// resize and every use of the borrowed chain. No native operation runs under a state lock.
fn configure_surface(width: u32, height: u32) -> *mut c_void {
    let window = presentation().window;
    if window.is_null() {
        return ptr::null_mut();
    }
    let (device, current) = {
        let state = lock();
        if !state.can_borrow_device() {
            return ptr::null_mut();
        }
        let Some(device) = &state.creation.device else {
            return ptr::null_mut();
        };
        (
            device.handles,
            state.surface.as_ref().map(|surface| surface.chain),
        )
    };
    if let Some(chain) = current {
        let (mut current_width, mut current_height) = (0, 0);
        unsafe { ops::chain_extent(chain, &mut current_width, &mut current_height) };
        if (width, height) == (current_width, current_height)
            || (unsafe { ops::drain(&device) }
                && unsafe { ops::chain_resize(chain, width, height) })
        {
            return chain;
        }
        // A refused drain or resize keeps the owned chain for the next Present to retry.
        return ptr::null_mut();
    }
    let chain = unsafe { ops::chain_create(&device, window, width, height) };
    if !chain.is_null() {
        // Only the holder of the presentation lease can publish a surface.
        let mut pending = Some(Surface { chain });
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

/// The presented image of a leased swapchain and the semaphore for forwarding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PresentSource {
    image: u64,
    present_ready: u64,
    width: u32,
    height: u32,
}

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
    lock_or_recover(&PRESENTATION)
}

/// The presentation once no lease is held.
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
    fn register(&mut self, mut swapchain: Swapchain) -> Result<(), Vec<u64>> {
        if swapchain.device == 0
            || self.next_publication == 0
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
            return Err(swapchain.present_ready);
        }
        swapchain.publication = self.next_publication;
        self.next_publication = self.next_publication.wrapping_add(1);
        self.swapchains.push(swapchain);
        Ok(())
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

    fn source(&self, device: usize, chain: u64, index: u32) -> Option<(u64, u64, PresentSource)> {
        let swapchain = self.swapchains.iter().find(|known| {
            !known.detached && known.active && known.device == device && known.chain == chain
        })?;
        let index = index as usize;
        Some((
            swapchain.publication,
            swapchain.surface,
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

fn set_surface_window(window: *mut c_void) -> bool {
    presentation().set_window(window)
}

fn surface_window() -> *mut c_void {
    presentation().window
}

pub(crate) fn register_win32_surface(instance: u64, surface: u64, parent: *mut c_void) {
    let mut registry = presentation();
    if registry.surfaces.try_reserve(1).is_ok() {
        registry.surfaces.push((instance, surface, parent as usize));
    }
}

pub(crate) fn forget_win32_surface(instance: u64, surface: u64) {
    presentation()
        .surfaces
        .retain(|&(owner, handle, _)| owner != instance || handle != surface);
}

pub(crate) fn supersede_swapchain(device: *mut c_void, chain: u64) {
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

/// Publishes a complete batch. A refusal returns the semaphores to the caller for rollback;
/// the native swapchain owns its images and destroys them with the swapchain.
pub(crate) fn register_swapchain(
    device: *mut c_void,
    chain: u64,
    surface: u64,
    extent: (u32, u32),
    images: Vec<u64>,
    present_ready: Vec<u64>,
) -> Result<(), Vec<u64>> {
    let mut registry = unleased();
    let swapchain = Swapchain {
        device: device as usize,
        chain,
        detached: false,
        active: true,
        surface,
        publication: 0,
        width: extent.0,
        height: extent.1,
        images,
        present_ready,
    };
    registry.register(swapchain)
}

pub(crate) fn retire_swapchain(
    device: *mut c_void,
    chain: u64,
    wait_idle: impl FnOnce() -> i32,
    mut destroy: impl FnMut(u64),
) -> i32 {
    let mut registry = unleased();
    registry.detach(device as usize, chain);
    if !registry
        .swapchains
        .iter()
        .any(|known| known.device == device as usize && known.detached)
    {
        return SUCCESS;
    }
    registry.leased = true;
    drop(registry);

    let result = wait_idle();
    // Device loss permits explicit teardown. An allocation failure or any other result leaves
    // the detached records available for a later retirement attempt on this device.
    if matches!(result, SUCCESS | DEVICE_LOST) {
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
            for semaphore in retired.present_ready {
                destroy(semaphore);
            }
        }
    }
    end_lease();
    result
}

/// What a present interception found for its target. It lives for one present call, so the frame
/// stays inline rather than taking a heap allocation per present.
#[allow(clippy::large_enum_variant)]
enum Target {
    /// Another target: forward untouched.
    Other,
    /// The player's own target without a window frame: the native player window stays visible.
    Native,
    /// A window frame bound to this target, with the presented image.
    Window(PresentSource, Frame),
}

/// Waits for and takes the lease, which the caller always ends with `end_lease`, even for
/// `Other`. Consumes the window frame bound to the presenting queue and swapchain.
fn present_begin(
    instance: u64,
    device: *mut c_void,
    queue: u64,
    chain: u64,
    index: u32,
    parent: *mut c_void,
) -> Target {
    let mut registry = unleased();
    registry.leased = true;
    let Some((swapchain, surface, found)) = registry.source(device as usize, chain, index) else {
        return Target::Other;
    };
    if !registry
        .surfaces
        .contains(&(instance, surface, parent as usize))
    {
        return Target::Other;
    }
    match lock().take_window(PresentIdentity { queue, swapchain }) {
        Some(frame) => Target::Window(found, frame),
        None => Target::Native,
    }
}

fn try_begin_lease() -> bool {
    let mut presentation = presentation();
    !std::mem::replace(&mut presentation.leased, true)
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

    /// The device and surface leaves. Only the shared-image test reaches them: its drain needs
    /// both APIs idle, and each image's handle is its width.
    pub(super) mod ops {
        use super::*;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

        pub(crate) static VULKAN_IDLE: AtomicBool = AtomicBool::new(false);
        pub(crate) static D3D12_IDLE: AtomicBool = AtomicBool::new(false);
        pub(crate) static DESTROYED: AtomicUsize = AtomicUsize::new(0);

        pub(crate) unsafe fn drain(_: *const DeviceHandles) -> bool {
            VULKAN_IDLE.load(Relaxed) && D3D12_IDLE.load(Relaxed)
        }
        pub(crate) unsafe fn create_image(
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
        pub(crate) unsafe fn destroy_image(_: *const DeviceHandles, image: *const ImageHandles) {
            DESTROYED.store(unsafe { (*image).image } as usize, Relaxed);
        }
        pub(crate) unsafe fn create_device(
            _: *const rsf_game_host_services,
            _: *mut DeviceHandles,
        ) -> bool {
            false
        }
        pub(crate) unsafe fn destroy_device(_: *const DeviceHandles) {}
        pub(crate) unsafe fn create_pool(_: *const DeviceHandles) -> u64 {
            0
        }
        pub(crate) unsafe fn destroy_pool(_: *const DeviceHandles, _: u64) {}
        pub(crate) unsafe fn create_slot(
            _: *const DeviceHandles,
            _: u64,
            _: *mut CommandSlot,
        ) -> bool {
            false
        }
        pub(crate) unsafe fn destroy_slot(_: *const DeviceHandles, _: *const CommandSlot) {}
        pub(crate) unsafe fn fence_ready(_: *const DeviceHandles, _: u64) -> bool {
            false
        }
        pub(crate) unsafe fn shared_completed(_: *const DeviceHandles) -> u64 {
            0
        }
        pub(crate) unsafe fn report_created(_: *const rsf_game_host_services) {}
        pub(crate) fn ring_begin() -> u64 {
            0
        }
        pub(crate) fn ring_retired(_: u64) {}
        pub(crate) unsafe fn chain_create(
            _: *const DeviceHandles,
            _: *mut c_void,
            _: u32,
            _: u32,
        ) -> *mut c_void {
            ptr::null_mut()
        }
        pub(crate) unsafe fn chain_extent(_: *mut c_void, _: *mut u32, _: *mut u32) {}
        pub(crate) unsafe fn chain_resize(_: *mut c_void, _: u32, _: u32) -> bool {
            false
        }
        pub(crate) unsafe fn chain_destroy(_: *mut c_void) {}
    }
    #[test]
    fn shared_image_resize_preserves_old_owner_while_another_slot_is_outstanding() {
        use ops::{D3D12_IDLE, DESTROYED, VULKAN_IDLE};
        use std::sync::atomic::Ordering::Relaxed;

        let state = Mutex::new(State {
            creation: {
                let mut creation = Creation::new();
                creation.device = Some(SharedDevice::new());
                creation
            },
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
    fn one_borrower_creates_at_a_time_and_vulkan_can_retry() {
        let mut state = State::new();
        assert!(matches!(state.claim_device(), Claim::Busy));
        assert!(state.enter());
        assert!(matches!(state.claim_device(), Claim::Create));
        assert!(matches!(state.claim_device(), Claim::Busy));
        state.leave();
        assert!(!state.idle());
        assert!(!state.close());
        state.creation.rolled_back(false);
        assert!(state.idle());
        assert!(state.enter());
        assert!(matches!(state.claim_device(), Claim::Create));
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
        assert!(matches!(state.claim_device(), Claim::Busy));
        assert!(state.admit_event(&work(1), EVENT_SR, false).is_none());
        assert!(!state.set_window(IDENTITY, &work(1)));
        state.reopen();
        assert!(state.enter());
        state.leave();
        assert!(state.close());
        state.release();
        assert!(!state.enter());
        state.reopen();
        assert!(state.enter());
        assert!(matches!(state.claim_device(), Claim::Create));
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
        assert!(presentation.register(swapchain(0, 2)).is_err());
        assert!(presentation.register(swapchain(3, 0)).is_err());
        let mut uneven = swapchain(3, 2);
        uneven.present_ready.pop();
        assert!(presentation.register(uneven).is_err());
        assert!(presentation.register(swapchain(3, 2)).is_ok());
        assert!(presentation.register(swapchain(3, 3)).is_err());
        assert_eq!(
            presentation.source(1, 3, 1),
            Some((
                1,
                33,
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
        assert!(presentation.register(swapchain(3, 2)).is_ok());
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

    // The only test that touches the process-wide presentation.
    #[test]
    fn the_presentation_lease_is_exclusive() {
        let device = ptr::without_provenance_mut(1);
        let parent = ptr::without_provenance_mut(99);
        register_win32_surface(55, 33, parent);
        assert!(register_swapchain(device, 7, 33, (4, 2), vec![71, 72], vec![81, 82]).is_ok());
        // A present without a receipt still holds the lease while forwarding.
        assert!(matches!(
            present_begin(55, device, 42, 7, 1, parent),
            Target::Native
        ));
        assert!(!try_begin_lease());
        end_lease();
        let mut retired = Vec::new();
        assert_eq!(
            retire_swapchain(device, 7, || -1, |sem| retired.push(sem)),
            -1
        );
        assert!(retired.is_empty());
        assert!(presentation().source(1, 7, 1).is_none());
        assert_eq!(
            retire_swapchain(
                device,
                7,
                || DEVICE_LOST,
                |sem| {
                    assert!(!try_begin_lease());
                    retired.push(sem);
                }
            ),
            DEVICE_LOST
        );
        assert_eq!(retired, [81, 82]);
        forget_win32_surface(55, 33);
        assert!(try_begin_lease());
        end_lease();
    }
}
