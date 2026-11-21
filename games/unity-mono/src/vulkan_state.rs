// SPDX-License-Identifier: MIT
//! State of the Unity package's Vulkan bridge (integrations/unity/native/vulkan_bridge.cpp). That
//! bridge copies Unity's Vulkan images into same-adapter D3D12 resources, records the render pass
//! on its own D3D12 queue and replaces Unity's present with its own DXGI swap chain.
//!
//! This module owns the bridge's admission and lifecycle: the shared device's readiness and its
//! single creator, the permanent fault, the render events queued to Unity and their packets, the
//! present interceptions and surface updates in progress, the player window frame the present
//! renders under, whether the bridge's DXGI swap chain exists, and the checked close and release.
//! The command slot rotation and the shared D3D12 fence values are the package bridge's submission
//! ring (`bridge_interop.rs`), whose contract matches: one slot per recording, a completion value
//! per signal, the ring advanced after a signalled submission, drained against the shared fence.
//!
//! The presentation state outlives a release, because Unity owns the swapchains and the window
//! stays: Unity's intercepted swapchains (their images and the present-ready semaphores the bridge
//! created for them), the presentation window's identity, and the presentation lease. The lease is
//! the one exclusive hold the C++ took as a mutex: present interceptions, swapchain retirement and
//! the swap chain's creation on the window thread run one at a time, a present keeps it through
//! the forwarded vkQueuePresentKHR, and retirement keeps it through vkDeviceWaitIdle and the
//! semaphore destruction, which need host access to the device's queues.
//!
//! vulkan_bridge.cpp keeps the Unity interfaces, Vulkan, D3D12 and DXGI objects and calls, images,
//! command buffers, slot fences, waits, copies, barriers, the swapchain and semaphore creation and
//! destruction, the window and its swap chain. The orchestrator's Vulkan frame-generation
//! swapchain owners present their own images through their own ops tables and this crate does
//! not link them, so the registry here is the bridge's own.
//!
//! The C++ kept this as unguarded globals. Its release counted only queued render events, so it
//! could destroy the slots, images and swap chain a present interception was recording into on the
//! render thread, and it cleared the window frame identity while that thread copied it.
use crate::bridge_abi::{RSF_UNITY_FRAMES_PACKETS, rsf_unity_packet};
use std::ffi::c_void;
use std::ptr;
use std::slice;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

const EVENTS: usize = RSF_UNITY_FRAMES_PACKETS as usize;
// Event kinds and device claims, as vulkan_state.h defines them.
const EVENT_HUDLESS: u32 = 2;
const DEVICE_BUSY: u32 = 0;
const DEVICE_CLAIMED: u32 = 1;
const DEVICE_READY: u32 = 2;

// SAFETY: all-zero is a valid packet: integers, floats and null pointers.
const EMPTY_PACKET: rsf_unity_packet = unsafe { std::mem::zeroed() };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Device {
    Absent,
    Creating,
    Ready,
}

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
    packet: rsf_unity_packet,
    kind: u32,
    reconfigure: bool,
    stage: Stage,
}

const FREE: Event = Event {
    packet: EMPTY_PACKET,
    kind: 0,
    reconfigure: false,
    stage: Stage::Free,
};

/// One started event: its slot, packet, kind and whether it applies a policy change.
type Started = (usize, rsf_unity_packet, u32, bool);

struct State {
    device: Device,
    /// A failed step stopped the bridge until release.
    faulted: bool,
    /// Release is checking the GPU; nothing new starts.
    closed: bool,
    events: [Event; EVENTS],
    /// Present interceptions and surface updates in progress.
    entries: u32,
    /// The last player window frame, whose identity the present interception renders under.
    window: Option<rsf_unity_packet>,
    /// The bridge's DXGI swap chain exists. It goes with the other objects at release.
    surface: bool,
}

// The packets hold Unity resource addresses as identity. This module never dereferences them.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State::new());

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

impl State {
    const fn new() -> Self {
        Self {
            device: Device::Absent,
            faulted: false,
            closed: false,
            events: [FREE; EVENTS],
            entries: 0,
            window: None,
            surface: false,
        }
    }

    fn open(&self) -> bool {
        !self.faulted && !self.closed
    }

    fn set_window(&mut self, packet: &rsf_unity_packet) -> bool {
        if !self.open() {
            return false;
        }
        self.window = Some(*packet);
        true
    }

    /// The window frame, once one with a frame identity arrived, until a fault or release.
    fn window(&self) -> Option<rsf_unity_packet> {
        if self.faulted {
            return None;
        }
        self.window.filter(|packet| packet.frame_id != 0)
    }

    fn admit_event(
        &mut self,
        packet: &rsf_unity_packet,
        kind: u32,
        reconfigure: bool,
    ) -> Option<usize> {
        if !self.open() || kind > EVENT_HUDLESS {
            return None;
        }
        let slot = self
            .events
            .iter()
            .position(|event| event.stage == Stage::Free)?;
        self.events[slot] = Event {
            packet: *packet,
            kind,
            reconfigure,
            stage: Stage::Queued,
        };
        Some(slot)
    }

    fn event_address(&self, slot: usize) -> *mut c_void {
        ptr::from_ref(&self.events[slot].packet).cast_mut().cast()
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
        Some((slot, event.packet, event.kind, event.reconfigure))
    }

    fn finish_event(&mut self, slot: usize) {
        if let Some(event) = self.events.get_mut(slot) {
            *event = FREE;
        }
    }

    fn enter(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.entries += 1;
        true
    }

    fn leave(&mut self) {
        self.entries = self.entries.saturating_sub(1);
    }

    fn claim_device(&mut self) -> u32 {
        if self.closed {
            return DEVICE_BUSY;
        }
        match self.device {
            Device::Ready => DEVICE_READY,
            Device::Creating => DEVICE_BUSY,
            Device::Absent => {
                self.device = Device::Creating;
                DEVICE_CLAIMED
            }
        }
    }

    fn device_created(&mut self, created: bool) {
        if self.device == Device::Creating {
            self.device = if created {
                Device::Ready
            } else {
                Device::Absent
            };
        }
    }

    /// Stops the bridge; true for the first fault.
    fn fault(&mut self) -> bool {
        let first = !self.faulted;
        self.faulted = true;
        first
    }

    fn idle(&self) -> bool {
        self.entries == 0
            && self.device != Device::Creating
            && self.events.iter().all(|event| event.stage == Stage::Free)
    }

    fn close(&mut self) -> bool {
        if self.closed || !self.idle() {
            return false;
        }
        self.closed = true;
        true
    }

    fn reopen(&mut self) {
        self.closed = false;
    }

    fn release(&mut self) {
        *self = Self::new();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_set_window(packet: *const rsf_unity_packet) -> u32 {
    let Some(packet) = (unsafe { packet.as_ref() }) else {
        return 0;
    };
    u32::from(lock().set_window(packet))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_window(packet: *mut rsf_unity_packet) -> u32 {
    if packet.is_null() {
        return 0;
    }
    let Some(window) = lock().window() else {
        return 0;
    };
    unsafe { packet.write(window) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_admit_event(
    packet: *const rsf_unity_packet,
    kind: u32,
    reconfigure: u32,
    slot: *mut u32,
) -> *mut c_void {
    let Some(packet) = (unsafe { packet.as_ref() }) else {
        return ptr::null_mut();
    };
    if slot.is_null() {
        return ptr::null_mut();
    }
    let mut state = lock();
    let Some(index) = state.admit_event(packet, kind, reconfigure != 0) else {
        return ptr::null_mut();
    };
    unsafe { slot.write(index as u32) };
    state.event_address(index)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_start_event(
    address: *const c_void,
    packet: *mut rsf_unity_packet,
    kind: *mut u32,
    reconfigure: *mut u32,
    slot: *mut u32,
) -> u32 {
    if packet.is_null() || kind.is_null() || reconfigure.is_null() || slot.is_null() {
        return 0;
    }
    let Some((index, stored, event_kind, applies)) = lock().start_event(address) else {
        return 0;
    };
    unsafe {
        packet.write(stored);
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

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_claim_device() -> u32 {
    lock().claim_device()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_device_created(created: u32) {
    lock().device_created(created != 0);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_ready() -> u32 {
    u32::from(lock().device == Device::Ready)
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
unsafe extern "C" fn rsf_unity_vulkan_idle(ready: *mut u32) -> u32 {
    let state = lock();
    if !ready.is_null() {
        unsafe { ready.write(u32::from(state.device == Device::Ready)) };
    }
    u32::from(state.idle())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_close() -> u32 {
    u32::from(lock().close())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_reopen() {
    lock().reopen();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_release() {
    lock().release();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_surface_created() {
    lock().surface = true;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_surface_ready() -> u32 {
    u32::from(lock().surface)
}

/// One intercepted Unity swapchain: its extent, its images and the present-ready semaphore the
/// bridge created for each image. Handles are Vulkan's, kept as identity only.
struct Swapchain {
    chain: u64,
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
    fn register(&mut self, swapchain: Swapchain) -> bool {
        if swapchain.chain == 0
            || swapchain.images.is_empty()
            || swapchain.images.len() != swapchain.present_ready.len()
            || self
                .swapchains
                .iter()
                .any(|known| known.chain == swapchain.chain)
            || self.swapchains.try_reserve(1).is_err()
        {
            return false;
        }
        self.swapchains.push(swapchain);
        true
    }

    fn source(&self, chain: u64, index: u32) -> Option<PresentSource> {
        let swapchain = self.swapchains.iter().find(|known| known.chain == chain)?;
        let index = index as usize;
        Some(PresentSource {
            image: *swapchain.images.get(index)?,
            present_ready: swapchain.present_ready[index],
            width: swapchain.width,
            height: swapchain.height,
        })
    }

    /// Forgets the swapchain and returns its present-ready semaphores for destruction.
    fn retire(&mut self, chain: u64) -> Option<Vec<u64>> {
        let position = self
            .swapchains
            .iter()
            .position(|known| known.chain == chain)?;
        Some(self.swapchains.swap_remove(position).present_ready)
    }
}

/// Destroys a retired swapchain's present-ready semaphores, with the presentation lease held.
type Retire = unsafe extern "C" fn(context: *mut c_void, present_ready: *const u64, count: u32);

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_set_surface_window(window: *mut c_void) -> u32 {
    u32::from(presentation().set_window(window))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_surface_window() -> *mut c_void {
    presentation().window
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_swapchain_register(
    chain: u64,
    width: u32,
    height: u32,
    images: *const u64,
    present_ready: *const u64,
    count: u32,
) -> u32 {
    if images.is_null() || present_ready.is_null() || count == 0 {
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
    let swapchain = Swapchain {
        chain,
        width,
        height,
        images: image_copy,
        present_ready: semaphore_copy,
    };
    u32::from(unleased().register(swapchain))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_swapchain_retire(
    chain: u64,
    retire: Option<Retire>,
    context: *mut c_void,
) -> u32 {
    let Some(retire) = retire else {
        return 0;
    };
    let mut presentation = unleased();
    let Some(present_ready) = presentation.retire(chain) else {
        return 0;
    };
    presentation.leased = true;
    drop(presentation);
    // SAFETY: vulkan_bridge.cpp's noexcept leaf, borrowing the handles for the call.
    unsafe { retire(context, present_ready.as_ptr(), present_ready.len() as u32) };
    end_lease();
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vulkan_present_begin(
    chain: u64,
    index: u32,
    source: *mut PresentSource,
) -> u32 {
    let mut presentation = unleased();
    presentation.leased = true;
    let Some(found) = presentation.source(chain, index) else {
        return 0;
    };
    if source.is_null() {
        return 0;
    }
    unsafe { source.write(found) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_surface_begin() -> u32 {
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

    fn packet(frame_id: u64) -> rsf_unity_packet {
        rsf_unity_packet {
            frame_id,
            ..EMPTY_PACKET
        }
    }

    #[test]
    fn events_stay_outstanding_until_their_callback_finishes() {
        let mut state = State::new();
        let slot = state.admit_event(&packet(4), EVENT_HUDLESS, false).unwrap();
        let address = state.event_address(slot);
        assert!(!state.idle());
        let (started, stored, kind, reconfigure) = state.start_event(address).unwrap();
        assert_eq!(
            (started, stored.frame_id, kind, reconfigure),
            (slot, 4, EVENT_HUDLESS, false)
        );
        // A running event is not started twice and still blocks retirement.
        assert!(state.start_event(address).is_none());
        assert!(!state.idle());
        state.finish_event(slot);
        assert!(state.idle());
        assert!(state.start_event(address).is_none());
        assert!(
            state
                .admit_event(&packet(5), EVENT_HUDLESS + 1, false)
                .is_none()
        );
    }

    #[test]
    fn a_full_table_refuses_events() {
        let mut state = State::new();
        for frame in 0..EVENTS {
            assert_eq!(
                state.admit_event(&packet(frame as u64 + 1), EVENT_SR, true),
                Some(frame)
            );
        }
        assert!(state.admit_event(&packet(99), EVENT_SR, false).is_none());
        state.finish_event(EVENTS);
        state.finish_event(3);
        assert_eq!(state.admit_event(&packet(99), EVENT_SR, false), Some(3));
    }

    #[test]
    fn a_fault_discards_queued_events_and_the_window_frame() {
        let mut state = State::new();
        assert!(state.set_window(&packet(8)));
        let slot = state.admit_event(&packet(9), EVENT_SR, false).unwrap();
        assert!(state.fault());
        assert!(!state.fault());
        assert!(state.window().is_none());
        assert!(!state.set_window(&packet(10)));
        assert!(state.admit_event(&packet(11), EVENT_SR, false).is_none());
        assert!(state.start_event(state.event_address(slot)).is_none());
        assert!(state.idle());
        state.release();
        assert!(!state.faulted);
        assert!(state.window().is_none());
    }

    #[test]
    fn the_window_frame_needs_a_frame_identity() {
        let mut state = State::new();
        assert!(state.window().is_none());
        assert!(state.set_window(&packet(0)));
        assert!(state.window().is_none());
        assert!(state.set_window(&packet(12)));
        assert_eq!(state.window().unwrap().frame_id, 12);
    }

    #[test]
    fn one_thread_creates_the_device() {
        let mut state = State::new();
        assert_eq!(state.claim_device(), DEVICE_CLAIMED);
        assert_eq!(state.claim_device(), DEVICE_BUSY);
        assert!(!state.idle());
        state.device_created(false);
        assert!(state.idle());
        assert_eq!(state.claim_device(), DEVICE_CLAIMED);
        state.device_created(true);
        assert_eq!(state.claim_device(), DEVICE_READY);
        // A report without a claim changes nothing.
        state.device_created(false);
        assert_eq!(state.device, Device::Ready);
    }

    #[test]
    fn close_waits_for_entries_and_blocks_new_work() {
        let mut state = State::new();
        assert!(state.enter());
        assert!(!state.close());
        state.leave();
        assert!(state.close());
        assert!(!state.close());
        assert!(!state.enter());
        assert_eq!(state.claim_device(), DEVICE_BUSY);
        assert!(state.admit_event(&packet(1), EVENT_SR, false).is_none());
        assert!(!state.set_window(&packet(1)));
        state.reopen();
        assert!(state.enter());
        state.leave();
        assert!(state.close());
        state.release();
        assert!(!state.closed);
        assert_eq!(state.claim_device(), DEVICE_CLAIMED);
    }

    // The only test that touches the process-wide state.
    #[test]
    fn exports_check_their_pointers() {
        rsf_unity_vulkan_release();
        let frame = packet(21);
        let mut slot = u32::MAX;
        let address = unsafe { rsf_unity_vulkan_admit_event(&frame, EVENT_SR, 1, &mut slot) };
        assert!(!address.is_null());
        assert_eq!(slot, 0);
        assert!(
            unsafe { rsf_unity_vulkan_admit_event(ptr::null(), EVENT_SR, 0, &mut slot) }.is_null()
        );
        let (mut out, mut kind, mut reconfigure, mut index) = (EMPTY_PACKET, 9, 9, 9);
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
            (out.frame_id, kind, reconfigure, index),
            (21, EVENT_SR, 1, 0)
        );
        let mut ready = 7;
        assert_eq!(unsafe { rsf_unity_vulkan_idle(&mut ready) }, 0);
        assert_eq!(ready, 0);
        rsf_unity_vulkan_finish_event(index);
        assert_eq!(unsafe { rsf_unity_vulkan_idle(ptr::null_mut()) }, 1);
        assert_eq!(unsafe { rsf_unity_vulkan_window(ptr::null_mut()) }, 0);
        assert_eq!(unsafe { rsf_unity_vulkan_set_window(ptr::null()) }, 0);
        rsf_unity_vulkan_release();
    }

    #[test]
    fn release_forgets_the_surface() {
        let mut state = State::new();
        state.surface = true;
        state.release();
        assert!(!state.surface);
    }

    fn swapchain(chain: u64, count: u64) -> Swapchain {
        Swapchain {
            chain,
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
            presentation.source(3, 1),
            Some(PresentSource {
                image: 302,
                present_ready: 3002,
                width: 640,
                height: 360,
            })
        );
        assert!(presentation.source(3, 2).is_none());
        assert!(presentation.source(4, 0).is_none());
        assert_eq!(presentation.retire(3), Some(vec![3001, 3002]));
        assert!(presentation.retire(3).is_none());
        assert!(presentation.source(3, 0).is_none());
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
        let leased = rsf_unity_vulkan_surface_begin() == 0;
        *RETIRED.lock().unwrap() = (handles, leased);
    }

    // The only test that touches the process-wide presentation.
    #[test]
    fn the_presentation_lease_is_exclusive() {
        let (images, ready) = ([71, 72], [81, 82]);
        assert_eq!(
            unsafe {
                rsf_unity_vulkan_swapchain_register(7, 4, 2, images.as_ptr(), ready.as_ptr(), 2)
            },
            1
        );
        assert_eq!(
            unsafe { rsf_unity_vulkan_swapchain_register(8, 4, 2, ptr::null(), ready.as_ptr(), 2) },
            0
        );
        let mut source = PresentSource::default();
        assert_eq!(
            unsafe { rsf_unity_vulkan_present_begin(7, 1, &mut source) },
            1
        );
        assert_eq!((source.image, source.present_ready), (72, 82));
        assert_eq!(rsf_unity_vulkan_surface_begin(), 0);
        rsf_unity_vulkan_presentation_end();
        // A miss still holds the lease: the C++ forwards the present under it.
        assert_eq!(
            unsafe { rsf_unity_vulkan_present_begin(7, 2, &mut source) },
            0
        );
        assert_eq!(rsf_unity_vulkan_surface_begin(), 0);
        rsf_unity_vulkan_presentation_end();
        assert_eq!(rsf_unity_vulkan_surface_begin(), 1);
        rsf_unity_vulkan_presentation_end();
        assert_eq!(
            unsafe { rsf_unity_vulkan_swapchain_retire(7, None, ptr::null_mut()) },
            0
        );
        assert_eq!(
            unsafe { rsf_unity_vulkan_swapchain_retire(7, Some(retire), ptr::null_mut()) },
            1
        );
        assert_eq!(*RETIRED.lock().unwrap(), (vec![81, 82], true));
        assert_eq!(
            unsafe { rsf_unity_vulkan_swapchain_retire(7, Some(retire), ptr::null_mut()) },
            0
        );
        assert_eq!(rsf_unity_vulkan_surface_begin(), 1);
        rsf_unity_vulkan_presentation_end();
    }
}
