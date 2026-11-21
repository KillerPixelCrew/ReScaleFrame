// SPDX-License-Identifier: MIT
//! Submission sequencing of the Unity package's D3D11 bridge (integrations/unity/native/bridge.cpp).
//! That bridge copies Unity's D3D11 textures into shared surfaces and records the render pass on
//! its own D3D12 queue. This module owns the command ring's slot rotation and per-slot completion
//! values, the bridge's completion counter and the shared D3D11/D3D12 fence values. bridge.cpp
//! keeps the devices, fences, waits, copies and command lists, and reads the completion fence.
//! Packets, policy, activity and callbacks are the shared bridge state (bridge_state.rs).
//!
//! The package's Vulkan bridge (vulkan_bridge.cpp, vulkan_state.rs) uses the same ring for its
//! command slots and the values on its shared D3D12/Vulkan fence: `completion` for the Vulkan
//! upload signal, `submit` for the D3D12 signal, `drained` against that fence. It reuses a slot
//! after the slot's Vulkan fence and releases the ring with its objects. One Unity renderer per
//! process means the two bridges never use the ring at once.
//!
//! The C++ kept these counters as unguarded globals that the lifecycle thread read in drained
//! while the render thread advanced them; one lock now orders both.
use crate::bridge_abi::RSF_UNITY_FRAMES_SLOTS;
use std::sync::{Mutex, MutexGuard, PoisonError};

const SLOTS: usize = RSF_UNITY_FRAMES_SLOTS as usize;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Interop {
    /// The ring slot the next reconstruction records into.
    next: usize,
    /// Completion value of each slot's last submission; 0 for none.
    complete: [u64; SLOTS],
    /// The last completion value handed out, to slots and to slotless window submissions.
    submitted: u64,
    /// The last shared D3D11/D3D12 fence value handed out.
    shared: u64,
}

static INTEROP: Mutex<Interop> = Mutex::new(Interop::new());

fn lock() -> MutexGuard<'static, Interop> {
    INTEROP.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Interop {
    pub(crate) const fn new() -> Self {
        Self {
            next: 0,
            complete: [0; SLOTS],
            submitted: 0,
            shared: 0,
        }
    }

    /// The slot the next reconstruction records into and the completion it waits for first.
    pub(crate) fn slot(&self) -> (usize, u64) {
        (self.next, self.complete[self.next])
    }

    /// Two consecutive shared fence values: the D3D11 upload, then the D3D12 hand-back.
    fn shared_pair(&mut self) -> (u64, u64) {
        self.shared += 2;
        (self.shared - 1, self.shared)
    }

    fn completion(&mut self) -> u64 {
        self.submitted += 1;
        self.submitted
    }

    /// Assigns the slot its submission's completion value; the slot is reused only after it.
    pub(crate) fn submit(&mut self, slot: usize) -> Option<u64> {
        if slot >= SLOTS {
            return None;
        }
        let complete = self.completion();
        self.complete[slot] = complete;
        Some(complete)
    }

    /// Moves the ring past a slot whose completion was signalled.
    pub(crate) fn advance(&mut self, slot: usize) {
        if slot < SLOTS {
            self.next = (slot + 1) % SLOTS;
        }
    }

    pub(crate) fn submitted(&self) -> u64 {
        self.submitted
    }

    /// No submission, or the completion fence reached the last one. A fence value of 0 stands
    /// for a missing fence.
    fn drained(&self, completed: u64) -> bool {
        self.submitted == 0 || completed >= self.submitted
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_interop_slot(complete: *mut u64) -> u32 {
    let (slot, value) = lock().slot();
    if !complete.is_null() {
        unsafe { complete.write(value) };
    }
    slot as u32
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_interop_submitted() -> u64 {
    lock().submitted()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_interop_shared_pair(first: *mut u64, second: *mut u64) -> u32 {
    if first.is_null() || second.is_null() {
        return 0;
    }
    let (upload, download) = lock().shared_pair();
    unsafe {
        first.write(upload);
        second.write(download);
    }
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_interop_completion() -> u64 {
    lock().completion()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_interop_submit(slot: u32) -> u64 {
    lock().submit(slot as usize).unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_interop_advance(slot: u32) {
    lock().advance(slot as usize);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_interop_drained(completed: u64) -> u32 {
    u32::from(lock().drained(completed))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_interop_release() {
    *lock() = Interop::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_rotates_after_signalled_submissions() {
        let mut ring = Interop::new();
        assert_eq!(ring.slot(), (0, 0));
        assert_eq!(ring.shared_pair(), (1, 2));
        assert_eq!(ring.submit(0), Some(1));
        ring.advance(0);
        assert_eq!(ring.slot(), (1, 0));
        assert_eq!(ring.shared_pair(), (3, 4));
        assert_eq!(ring.submit(1), Some(2));
        ring.advance(1);
        assert_eq!(ring.submit(2), Some(3));
        ring.advance(2);
        // Slot 0 is reused after its first completion.
        assert_eq!(ring.slot(), (0, 1));
        assert_eq!(ring.submit(SLOTS), None);
        ring.advance(SLOTS);
        assert_eq!(ring.slot(), (0, 1));
    }

    #[test]
    fn an_unsignalled_slot_stays_current() {
        let mut ring = Interop::new();
        assert_eq!(ring.submit(0), Some(1));
        // The signal failed: the bridge stops without advancing.
        assert_eq!(ring.slot(), (0, 1));
        assert!(!ring.drained(0));
    }

    #[test]
    fn window_submissions_share_the_completion_counter() {
        let mut ring = Interop::new();
        assert!(ring.drained(0));
        assert_eq!(ring.completion(), 1);
        assert_eq!(ring.submit(0), Some(2));
        assert!(!ring.drained(1));
        assert!(ring.drained(2));
        assert!(ring.drained(u64::MAX));
    }

    // The only test that touches the process-wide ring.
    #[test]
    fn release_forgets_the_ring() {
        rsf_unity_interop_release();
        let (mut first, mut second) = (0u64, 0u64);
        assert_eq!(
            unsafe { rsf_unity_interop_shared_pair(&mut first, &mut second) },
            1
        );
        assert_eq!((first, second), (1, 2));
        assert_eq!(rsf_unity_interop_submit(0), 1);
        rsf_unity_interop_advance(0);
        assert_eq!(rsf_unity_interop_submitted(), 1);
        assert_eq!(rsf_unity_interop_drained(0), 0);
        rsf_unity_interop_release();
        assert_eq!(*lock(), Interop::new());
        assert_eq!(rsf_unity_interop_drained(0), 1);
        let mut complete = 7u64;
        assert_eq!(unsafe { rsf_unity_interop_slot(&mut complete) }, 0);
        assert_eq!(complete, 0);
    }
}
