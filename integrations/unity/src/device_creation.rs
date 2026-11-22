// SPDX-License-Identifier: MIT
//! Single-creator publication for Unity's interop device owners.

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Claim<View> {
    Ready(View),
    Create,
    Busy,
    Stopped,
}

pub(crate) struct Creation<Device> {
    pub(crate) creating: bool,
    pub(crate) device: Option<Device>,
    failed: bool,
}

impl<Device> Creation<Device> {
    pub(crate) const fn new() -> Self {
        Self {
            creating: false,
            device: None,
            failed: false,
        }
    }

    pub(crate) fn claim<View>(
        &mut self,
        admitted: bool,
        view: impl FnOnce(&Device) -> View,
    ) -> Claim<View> {
        if !admitted || self.creating {
            return Claim::Busy;
        }
        if let Some(device) = &self.device {
            return Claim::Ready(view(device));
        }
        if self.failed {
            return Claim::Stopped;
        }
        self.creating = true;
        Claim::Create
    }

    /// Call only after destruction of partially prepared resources outside the state lock.
    pub(crate) fn rolled_back(&mut self, latch_failure: bool) {
        debug_assert!(self.creating && self.device.is_none());
        self.creating = false;
        self.failed = latch_failure;
    }

    /// Returns any displaced owner for destruction outside the state lock.
    pub(crate) fn publish(&mut self, device: Device) -> Option<Device> {
        debug_assert!(self.creating && self.device.is_none());
        self.creating = false;
        self.device.replace(device)
    }
}
