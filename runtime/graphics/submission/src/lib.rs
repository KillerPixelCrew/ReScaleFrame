// SPDX-License-Identifier: MIT
//! Runtime submission sequencing for an adapter-owned command ring and its fence timelines.

use std::sync::{Mutex, MutexGuard, PoisonError};

const MAX_SLOTS: usize = 32;

#[derive(Debug, PartialEq, Eq)]
pub struct SubmissionRing {
    slots: usize,
    next: usize,
    complete: [u64; MAX_SLOTS],
    submitted: u64,
    shared: u64,
    copy_complete: u64,
    copy_unknown: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FenceStatus {
    Removed,
    Pending,
    Complete,
}

pub fn fence_status(completed: u64, required: u64) -> FenceStatus {
    if completed == u64::MAX {
        FenceStatus::Removed
    } else if completed < required {
        FenceStatus::Pending
    } else {
        FenceStatus::Complete
    }
}

fn advance_value(value: u64, count: u64) -> Option<u64> {
    value.checked_add(count).filter(|value| *value != u64::MAX)
}

impl SubmissionRing {
    pub const fn new(slots: usize) -> Self {
        assert!(slots > 0 && slots <= MAX_SLOTS);
        Self {
            slots,
            next: 0,
            complete: [0; MAX_SLOTS],
            submitted: 0,
            shared: 0,
            copy_complete: 0,
            copy_unknown: false,
        }
    }

    pub fn slot(&self) -> (usize, u64) {
        (self.next, self.complete[self.next])
    }

    pub fn shared_pair(&mut self) -> Option<(u64, u64)> {
        self.shared = advance_value(self.shared, 2)?;
        Some((self.shared - 1, self.shared))
    }

    pub fn shared_tail(&mut self) -> Option<u64> {
        self.shared = advance_value(self.shared, 1)?;
        Some(self.shared)
    }

    pub fn next_completion(&self) -> Option<u64> {
        advance_value(self.submitted, 1)
    }

    pub fn completion(&mut self) -> Option<u64> {
        self.submitted = self.next_completion()?;
        Some(self.submitted)
    }

    pub fn submit(&mut self, slot: usize) -> Option<u64> {
        if slot >= self.slots {
            return None;
        }
        let complete = self.completion()?;
        self.complete[slot] = complete;
        Some(complete)
    }

    pub fn advance(&mut self, slot: usize) {
        if slot < self.slots {
            self.next = (slot + 1) % self.slots;
        }
    }

    pub fn submitted(&self) -> u64 {
        self.submitted
    }

    pub fn status(&self, completed: u64) -> FenceStatus {
        fence_status(completed, self.submitted)
    }

    pub fn drained(&self, completed: u64) -> bool {
        self.status(completed) == FenceStatus::Complete
    }

    pub fn record_copy(&mut self) {
        self.copy_unknown = true;
    }

    pub fn copy_submitted(&mut self, value: u64) {
        if value > self.copy_complete && value != u64::MAX {
            self.copy_complete = value;
            self.copy_unknown = false;
        }
    }

    pub fn copy_drained(&self, completed: u64) -> bool {
        !self.copy_unknown && fence_status(completed, self.copy_complete) == FenceStatus::Complete
    }
}

/// The adapter owns one handle per bridge and keeps it alive through callback retirement. The
/// handle serializes observations with submission updates.
pub struct SubmissionHandle(Mutex<SubmissionRing>);

impl SubmissionHandle {
    pub const fn new(slots: usize) -> Self {
        Self(Mutex::new(SubmissionRing::new(slots)))
    }

    pub fn lock(&self) -> MutexGuard<'_, SubmissionRing> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn reset(&self) {
        let mut ring = self.lock();
        *ring = SubmissionRing::new(ring.slots);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_rotates_after_signalled_submissions() {
        let mut ring = SubmissionRing::new(3);
        assert_eq!(ring.slot(), (0, 0));
        assert_eq!(ring.shared_pair(), Some((1, 2)));
        assert_eq!(ring.submit(0), Some(1));
        ring.advance(0);
        assert_eq!(ring.slot(), (1, 0));
        assert_eq!(ring.shared_pair(), Some((3, 4)));
        assert_eq!(ring.submit(1), Some(2));
        ring.advance(1);
        assert_eq!(ring.submit(2), Some(3));
        ring.advance(2);
        // Slot 0 is reused after its first completion.
        assert_eq!(ring.slot(), (0, 1));
        assert_eq!(ring.submit(3), None);
        ring.advance(3);
        assert_eq!(ring.slot(), (0, 1));
    }

    #[test]
    fn an_unsignalled_slot_stays_current() {
        let mut ring = SubmissionRing::new(3);
        assert_eq!(ring.submit(0), Some(1));
        // The signal failed: the bridge stops without advancing.
        assert_eq!(ring.slot(), (0, 1));
        assert!(!ring.drained(0));
    }

    #[test]
    fn window_submissions_share_the_completion_counter() {
        let mut ring = SubmissionRing::new(3);
        assert!(ring.drained(0));
        assert_eq!(ring.completion(), Some(1));
        assert_eq!(ring.submit(0), Some(2));
        assert!(!ring.drained(1));
        assert!(ring.drained(2));
        assert_eq!(ring.status(u64::MAX), FenceStatus::Removed);
    }

    #[test]
    fn an_unproved_copy_prevents_retirement_even_after_queue_completion() {
        let mut ring = SubmissionRing::new(3);
        let value = ring.shared_tail().unwrap();
        ring.record_copy();
        assert!(ring.drained(0));
        assert!(!ring.copy_drained(value));
        ring.copy_submitted(value);
        assert!(!ring.copy_drained(0));
        assert!(ring.copy_drained(value));
        assert!(!ring.copy_drained(u64::MAX));
    }

    #[test]
    fn separate_bridges_do_not_share_completion_values() {
        let first = SubmissionHandle::new(3);
        let second = SubmissionHandle::new(3);
        assert_eq!(first.lock().submit(0), Some(1));
        assert!(!first.lock().drained(0));
        assert!(second.lock().drained(0));
        first.reset();
        assert!(first.lock().drained(0));
    }
}
