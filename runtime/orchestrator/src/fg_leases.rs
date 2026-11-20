// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::render_abi::{rsf_backend_result, rsf_fg_retirement};
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::slice;

const MAX_LEASES: usize = 16;
const MAX_RESOURCES: usize = 5;

const OK: rsf_backend_result = 0;
const INVALID: rsf_backend_result = -1;
const INIT_FAILED: rsf_backend_result = -6;
const NOT_READY: rsf_backend_result = -8;
const FEATURE_FAILED: rsf_backend_result = -9;
const STALE_RESOURCES: rsf_backend_result = -10;

unsafe extern "C" {
    fn rsf_fg_lease_com_get_device(child: *mut c_void, device: *mut *mut c_void) -> i32;
    fn rsf_d3d12_fence_completed(fence: *mut c_void) -> u64;
}

unsafe fn device(child: *mut c_void) -> Option<ComRef> {
    let mut device = ptr::null_mut();
    let result = unsafe { rsf_fg_lease_com_get_device(child, &mut device) };
    let device = NonNull::new(device).map(ComRef);
    if result < 0 { None } else { device }
}

unsafe fn same_device(first: *mut c_void, second: *mut c_void) -> bool {
    let Some(first) = (unsafe { device(first) }) else {
        return false;
    };
    unsafe { device(second) }.is_some_and(|second| first.0 == second.0)
}

#[derive(Clone)]
pub(super) struct FencePoint {
    pub(super) fence: ComRef,
    pub(super) value: u64,
}

// D3D12 fences are free-threaded and all calls on the retained interface are safe across threads.
unsafe impl Send for FencePoint {}

#[derive(PartialEq, Eq)]
pub(super) enum FenceState {
    Pending,
    Complete,
    DeviceRemoved,
}

impl FencePoint {
    pub(super) fn state(&self) -> FenceState {
        let completed = unsafe { rsf_d3d12_fence_completed(self.fence.0.as_ptr()) };
        if completed == u64::MAX {
            FenceState::DeviceRemoved
        } else if completed >= self.value {
            FenceState::Complete
        } else {
            FenceState::Pending
        }
    }
}

enum VendorRetirement {
    Complete,
    Fence(FencePoint),
}

struct Lease {
    id: u64,
    _generation: u32,
    first: ComRef,
    _additional: [Option<ComRef>; MAX_RESOURCES - 1],
    queue: Option<FencePoint>,
    vendor: Option<VendorRetirement>,
}

struct Leases {
    slots: [Option<Lease>; MAX_LEASES],
    capacity: usize,
    last_acquire: u64,
}

impl Leases {
    fn slot_for(&mut self, id: u64) -> &mut Option<Lease> {
        &mut self.slots[(id % self.capacity as u64) as usize]
    }

    fn find(&mut self, id: u64) -> Option<&mut Lease> {
        self.slot_for(id).as_mut().filter(|lease| lease.id == id)
    }

    fn collect(&mut self) -> (rsf_backend_result, u32) {
        let mut result = OK;
        let mut pending = 0;
        for slot in &mut self.slots[..self.capacity] {
            let Some(lease) = slot else { continue };
            if let (Some(queue), Some(vendor)) = (&lease.queue, &lease.vendor) {
                // Read both fences even when one is pending. Either can report device removal.
                let queue = queue.state();
                let vendor = match vendor {
                    VendorRetirement::Complete => FenceState::Complete,
                    VendorRetirement::Fence(point) => point.state(),
                };
                if queue == FenceState::Complete && vendor == FenceState::Complete {
                    *slot = None;
                    continue;
                }
                if queue == FenceState::DeviceRemoved || vendor == FenceState::DeviceRemoved {
                    result = FEATURE_FAILED;
                }
            }
            pending += 1;
        }
        (result, pending)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_leases_create(
    capacity: u32,
    out: *mut *mut Leases,
) -> rsf_backend_result {
    if out.is_null() || capacity == 0 || capacity as usize > MAX_LEASES {
        return INVALID;
    }
    let handle = crate::allocate_handle(Leases {
        slots: std::array::from_fn(|_| None),
        capacity: capacity as usize,
        last_acquire: 0,
    });
    unsafe { out.write(handle) };
    if handle.is_null() { INIT_FAILED } else { OK }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_leases_acquire(
    leases: *mut Leases,
    id: u64,
    generation: u32,
    resources: *const *mut c_void,
    count: u32,
) -> rsf_backend_result {
    let Some(leases) = (unsafe { leases.as_mut() }) else {
        return INVALID;
    };
    if id == 0
        || generation == 0
        || resources.is_null()
        || count == 0
        || count as usize > MAX_RESOURCES
    {
        return INVALID;
    }
    if id <= leases.last_acquire {
        return STALE_RESOURCES;
    }
    let slot = leases.slot_for(id);
    if slot.is_some() {
        return NOT_READY;
    }
    let resources = unsafe { slice::from_raw_parts(resources, count as usize) };
    if resources.iter().any(|resource| resource.is_null()) {
        return INVALID;
    }
    let (&first, additional) = resources.split_first().unwrap();
    if additional
        .iter()
        .any(|&resource| !unsafe { same_device(first, resource) })
    {
        return INVALID;
    }
    *slot = Some(Lease {
        id,
        _generation: generation,
        first: unsafe { ComRef::retain(first) },
        _additional: std::array::from_fn(|index| {
            additional
                .get(index)
                .map(|&resource| unsafe { ComRef::retain(resource) })
        }),
        queue: None,
        vendor: None,
    });
    leases.last_acquire = id;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_leases_submit(
    leases: *mut Leases,
    id: u64,
    fence: *mut c_void,
    value: u64,
) -> rsf_backend_result {
    let Some(lease) = (unsafe { leases.as_mut() }).and_then(|leases| leases.find(id)) else {
        return INVALID;
    };
    if fence.is_null() || value == 0 || value == u64::MAX {
        return INVALID;
    }
    if lease.queue.is_some() {
        return NOT_READY;
    }
    if !unsafe { same_device(lease.first.0.as_ptr(), fence) } {
        return INVALID;
    }
    lease.queue = Some(FencePoint {
        fence: unsafe { ComRef::retain(fence) },
        value,
    });
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_leases_seal(
    leases: *mut Leases,
    id: u64,
    retirement: *const rsf_fg_retirement,
) -> rsf_backend_result {
    let Some(lease) = (unsafe { leases.as_mut() }).and_then(|leases| leases.find(id)) else {
        return INVALID;
    };
    // A short descriptor may contain only struct_size.
    if retirement.is_null()
        || (unsafe { retirement.cast::<u32>().read() } as usize) < size_of::<rsf_fg_retirement>()
    {
        return INVALID;
    }
    let rsf_fg_retirement { fence, value, .. } = unsafe { retirement.read() };
    if (fence.is_null() && value != 0) || value == u64::MAX {
        return INVALID;
    }
    if lease.queue.is_none() || lease.vendor.is_some() {
        return NOT_READY;
    }
    lease.vendor = Some(if fence.is_null() {
        VendorRetirement::Complete
    } else {
        if !unsafe { same_device(lease.first.0.as_ptr(), fence) } {
            return INVALID;
        }
        VendorRetirement::Fence(FencePoint {
            fence: unsafe { ComRef::retain(fence) },
            value,
        })
    });
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_leases_collect(
    leases: *mut Leases,
    pending: *mut u32,
) -> rsf_backend_result {
    let Some(leases) = (unsafe { leases.as_mut() }) else {
        return INVALID;
    };
    if pending.is_null() {
        return INVALID;
    }
    let (result, count) = leases.collect();
    unsafe { pending.write(count) };
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_leases_cancel(leases: *mut Leases, id: u64) -> rsf_backend_result {
    let Some(leases) = (unsafe { leases.as_mut() }) else {
        return INVALID;
    };
    let slot = leases.slot_for(id);
    let Some(lease) = slot.as_ref().filter(|lease| lease.id == id) else {
        return INVALID;
    };
    if lease.queue.is_some() {
        return NOT_READY;
    }
    *slot = None;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_leases_destroy(leases: *mut Leases) -> rsf_backend_result {
    let Some(owner) = (unsafe { leases.as_mut() }) else {
        return INVALID;
    };
    let (result, pending) = owner.collect();
    if result != OK {
        return result;
    }
    if pending != 0 {
        return NOT_READY;
    }
    drop(unsafe { Box::from_raw(leases) });
    OK
}
