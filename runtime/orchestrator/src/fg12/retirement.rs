// SPDX-License-Identifier: MIT
use crate::fg_leases::{FencePoint, FenceState};

pub(super) enum VendorUse {
    None,
    Preparing(u64),
    Unfenced(u64),
    Joined(FencePoint),
}

impl VendorUse {
    pub(super) fn needs_join(&self, generation: u64, exact_present: bool) -> bool {
        match self {
            Self::Preparing(old) => *old == generation && exact_present,
            Self::Unfenced(old) => *old == generation,
            _ => false,
        }
    }

    pub(super) fn reusable(&self, generation: u64) -> bool {
        match self {
            Self::None => false,
            // The bridge advances the generation after a drained switch or retirement.
            Self::Preparing(old) | Self::Unfenced(old) => *old != generation,
            Self::Joined(point) => point.state() == FenceState::Complete,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VendorUse;
    use crate::com_ref;
    use crate::fg_leases::FencePoint;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[repr(C)]
    struct Vtable {
        query_interface: unsafe extern "system" fn(),
        add_ref: extern "system" fn(*mut c_void) -> u32,
        release: extern "system" fn(*mut c_void) -> u32,
        device_child_methods: [unsafe extern "system" fn(); 5],
        completed: unsafe extern "system" fn(*mut FakeFence) -> u64,
    }

    #[repr(C)]
    struct FakeFence {
        vtable: &'static Vtable,
        completion: AtomicU64,
    }

    extern "system" fn unused() {}
    extern "system" fn keep(_: *mut c_void) -> u32 {
        1
    }
    unsafe extern "system" fn completed(fence: *mut FakeFence) -> u64 {
        unsafe { (*fence).completion.load(Ordering::Relaxed) }
    }

    static FENCE: Vtable = Vtable {
        query_interface: unused,
        add_ref: keep,
        release: keep,
        device_child_methods: [unused; 5],
        completed,
    };

    #[test]
    fn unfenced_provider_reads_wait_for_a_same_generation_join_or_drained_switch() {
        assert!(!VendorUse::None.needs_join(7, true));
        assert!(!VendorUse::None.reusable(7));
        let recording = VendorUse::Preparing(7);
        assert!(recording.needs_join(7, true));
        assert!(!recording.needs_join(7, false));
        assert!(!recording.needs_join(8, true));
        assert!(!recording.reusable(7));
        assert!(recording.reusable(8));

        let unknown = VendorUse::Unfenced(7);
        assert!(unknown.needs_join(7, false)); // An ineligible Present can still join.
        assert!(!unknown.needs_join(8, true));
        assert!(!unknown.reusable(7));
        assert!(unknown.reusable(8));
    }

    #[test]
    fn joined_reads_require_real_fence_completion() {
        let mut native = FakeFence {
            vtable: &FENCE,
            completion: AtomicU64::new(2),
        };
        let joined = VendorUse::Joined(FencePoint {
            fence: unsafe { com_ref::retain(ptr::from_mut(&mut native).cast()) },
            value: 3,
        });
        assert!(!joined.needs_join(7, true));
        assert!(!joined.reusable(7));
        assert!(!joined.reusable(8));
        native.completion.store(3, Ordering::Relaxed);
        assert!(joined.reusable(8));
        native.completion.store(u64::MAX, Ordering::Relaxed);
        assert!(!joined.reusable(8));
    }
}
