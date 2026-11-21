// SPDX-License-Identifier: MIT
//! State of the Unity package's Vulkan bridge (integrations/unity/native/vulkan_bridge.cpp). That
//! bridge copies Unity's Vulkan images into same-adapter D3D12 resources, records the render pass
//! on its own D3D12 queue and replaces Unity's present with its own DXGI swap chain.
//!
//! This module owns the bridge's admission and lifecycle: the shared device's readiness and its
//! single creator, the permanent fault, the render events queued to Unity and their packets, the
//! present interceptions and surface updates in progress, the player window frame the present
//! renders under, and the checked close and release. The command slot rotation and the shared
//! D3D12 fence values are the package bridge's submission ring (`bridge_interop.rs`), whose
//! contract matches: one slot per recording, a completion value per signal, the ring advanced
//! after a signalled submission, drained against the shared fence. vulkan_bridge.cpp keeps the
//! Unity interfaces, Vulkan, D3D12 and DXGI objects and calls, images, command buffers, slot
//! fences, waits, copies, barriers, the swapchain interception and the window.
//!
//! The C++ kept this as unguarded globals. Its release counted only queued render events, so it
//! could destroy the slots, images and swap chain a present interception was recording into on the
//! render thread, and it cleared the window frame identity while that thread copied it.
use crate::bridge_abi::{RSF_UNITY_FRAMES_PACKETS, rsf_unity_packet};
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

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
}
