// SPDX-License-Identifier: MIT
use crate::sys::rsf_fg_retirement;
use std::cell::Cell;
use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::ptr::{from_ref, null_mut};
use windows::Win32::{
    Foundation::{E_NOINTERFACE, S_OK},
    Graphics::{
        Direct3D::D3D_FEATURE_LEVEL_11_0,
        Direct3D12::*,
        Dxgi::{Common::*, *},
    },
};
use windows::core::{GUID, HRESULT, IUnknown_Vtbl, Interface};

type Raw = *mut c_void;

const OK: i32 = 0;
const INVALID: i32 = -1;
const NOT_READY: i32 = -8;
const DEVICE_FAILED: i32 = -9;
const STALE: i32 = -10;

unsafe extern "C" {
    fn rsf_fg_leases_create(capacity: u32, lease: *mut Raw) -> i32;
    fn rsf_fg_leases_acquire(
        lease: Raw,
        source: u64,
        generation: u32,
        resources: *const Raw,
        count: u32,
    ) -> i32;
    fn rsf_fg_leases_submit(lease: Raw, source: u64, fence: Raw, value: u64) -> i32;
    fn rsf_fg_leases_seal(lease: Raw, source: u64, retirement: *const rsf_fg_retirement) -> i32;
    fn rsf_fg_leases_collect(lease: Raw, pending: *mut u32) -> i32;
    fn rsf_fg_leases_cancel(lease: Raw, source: u64) -> i32;
    fn rsf_fg_leases_destroy(lease: Raw) -> i32;
    fn rsf_fg_leases_abandon_device_lost(lease: Raw) -> i32;
}


fn retirement(fence: Raw, value: u64) -> rsf_fg_retirement {
    rsf_fg_retirement {
        struct_size: size_of::<rsf_fg_retirement>() as u32,
        fence,
        value,
    }
}

fn warp_device() -> Option<ID3D12Device> {
    let mut device = None;
    unsafe {
        let factory = CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0)).ok()?;
        let adapter = factory.EnumWarpAdapter::<IDXGIAdapter>().ok()?;
        D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device).ok()?;
    }
    device
}

fn make_resource(device: &ID3D12Device) -> ID3D12Resource {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: 16,
        Height: 16,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    };
    let mut resource = None;
    let result = unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )
    };
    check!(result.is_ok());
    resource.expect("successful resource creation")
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_fg_leases() -> i32 {
    let Some(device) = warp_device() else {
        return 77;
    };
    let resource = make_resource(&device);
    let gpu = unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) };
    let vendor = unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) };
    check!(gpu.is_ok() && vendor.is_ok());
    let (gpu, vendor) = (gpu.unwrap(), vendor.unwrap());
    let mut lease = null_mut();
    let mut resources = [resource.as_raw()];
    let mut pending = 0;
    unsafe {
        check!(rsf_fg_leases_create(1, &mut lease) == OK);
        check!(rsf_fg_leases_acquire(lease, 1, 1, resources.as_ptr(), 1) == OK);
        drop(resource);
        {
            let retained = ID3D12Resource::from_raw_borrowed(&resources[0]).unwrap();
            check!(retained.GetDesc().Width == 16);
        }
        check!(rsf_fg_leases_acquire(lease, 2, 1, resources.as_ptr(), 1) == NOT_READY);
        check!(rsf_fg_leases_destroy(lease) == NOT_READY);
        check!(rsf_fg_leases_submit(lease, 1, gpu.as_raw(), 1) == OK);
        check!(rsf_fg_leases_cancel(lease, 1) == NOT_READY);
        check!(rsf_fg_leases_collect(lease, &mut pending) == OK && pending == 1);
        check!(rsf_fg_leases_seal(lease, 1, &retirement(vendor.as_raw(), 2)) == OK);
        // CPU signaling isolates the two completion gates.
        check!(gpu.Signal(1).is_ok());
        check!(rsf_fg_leases_collect(lease, &mut pending) == OK && pending == 1);
        {
            let retained = ID3D12Resource::from_raw_borrowed(&resources[0]).unwrap();
            check!(retained.GetDesc().Width == 16);
        }
        check!(rsf_fg_leases_destroy(lease) == NOT_READY);
        check!(vendor.Signal(2).is_ok());
        check!(rsf_fg_leases_collect(lease, &mut pending) == OK && pending == 0);
        check!(rsf_fg_leases_acquire(lease, 1, 1, resources.as_ptr(), 1) == STALE);
        let resource = make_resource(&device);
        resources[0] = resource.as_raw();
        check!(rsf_fg_leases_acquire(lease, 2, 2, resources.as_ptr(), 1) == OK);
        check!(rsf_fg_leases_cancel(lease, 2) == OK);
        check!(rsf_fg_leases_destroy(lease) == OK);
    }
    println!("WARP COM retention and independent synthetic fence completion gates passed");
    0
}

#[repr(C)]
struct Mock {
    vtable: *const Raw,
    refs: Cell<u32>,
    device: Cell<Raw>,
    completed: Cell<u64>,
}

impl Mock {
    fn new(vtable: &[Raw], device: Raw, completed: u64) -> Self {
        Self {
            vtable: vtable.as_ptr(),
            refs: Cell::new(1),
            device: Cell::new(device),
            completed: Cell::new(completed),
        }
    }

    fn raw(&self) -> Raw {
        from_ref(self).cast_mut().cast()
    }
}

unsafe extern "system" fn add_ref(raw: Raw) -> u32 {
    let refs = unsafe { &(*raw.cast::<Mock>()).refs };
    check!(refs.get() != 0);
    let count = refs.get() + 1;
    refs.set(count);
    count
}

unsafe extern "system" fn release(raw: Raw) -> u32 {
    let refs = unsafe { &(*raw.cast::<Mock>()).refs };
    check!(refs.get() != 0);
    let count = refs.get() - 1;
    refs.set(count);
    count
}

unsafe extern "system" fn get_device(raw: Raw, iid: *const GUID, out: *mut Raw) -> HRESULT {
    check!(unsafe { *iid } == ID3D12Device::IID);
    let device = unsafe { (*raw.cast::<Mock>()).device.get() };
    unsafe { out.write(device) };
    if device.is_null() {
        return E_NOINTERFACE;
    }
    unsafe { add_ref(device) };
    S_OK
}

unsafe extern "system" fn completed_value(raw: Raw) -> u64 {
    unsafe { (*raw.cast::<Mock>()).completed.get() }
}

unsafe extern "system" fn unexpected() {
    eprintln!("Unexpected COM call in the lease fixture");
    std::process::abort();
}

fn methods<const N: usize>() -> [Raw; N] {
    let mut vtable = [unexpected as Raw; N];
    vtable[offset_of!(IUnknown_Vtbl, AddRef) / size_of::<Raw>()] = add_ref as Raw;
    vtable[offset_of!(IUnknown_Vtbl, Release) / size_of::<Raw>()] = release as Raw;
    vtable
}

fn check_fence_failure(queue_removed: bool) {
    const GET_DEVICE: usize = offset_of!(ID3D12DeviceChild_Vtbl, GetDevice) / size_of::<Raw>();
    const COMPLETED: usize = offset_of!(ID3D12Fence_Vtbl, GetCompletedValue) / size_of::<Raw>();
    let device_methods = methods::<{ size_of::<ID3D12Device_Vtbl>() / size_of::<Raw>() }>();
    let mut resource_methods = methods::<{ size_of::<ID3D12Resource_Vtbl>() / size_of::<Raw>() }>();
    resource_methods[GET_DEVICE] = get_device as Raw;
    let mut fence_methods = methods::<{ size_of::<ID3D12Fence_Vtbl>() / size_of::<Raw>() }>();
    fence_methods[GET_DEVICE] = get_device as Raw;
    fence_methods[COMPLETED] = completed_value as Raw;
    let device = Mock::new(&device_methods, null_mut(), 0);
    let foreign = Mock::new(&device_methods, null_mut(), 0);
    let resource = Mock::new(&resource_methods, device.raw(), 0);
    let other = Mock::new(&resource_methods, foreign.raw(), 0);
    let queue = Mock::new(&fence_methods, device.raw(), 0);
    let vendor = Mock::new(&fence_methods, device.raw(), 0);
    let foreign_fence = Mock::new(&fence_methods, foreign.raw(), 0);
    let completed = Mock::new(&fence_methods, device.raw(), 1);

    let mut lease = null_mut();
    let mut resources = [resource.raw(), other.raw()];
    let mut pending = 0;
    unsafe {
        check!(rsf_fg_leases_create(2, &mut lease) == OK);
        check!(rsf_fg_leases_acquire(lease, 2, 1, resources.as_ptr(), 2) == INVALID);
        check!(
            resource.refs.get() == 1
                && other.refs.get() == 1
                && device.refs.get() == 1
                && foreign.refs.get() == 1
        );
        other.device.set(null_mut());
        check!(rsf_fg_leases_acquire(lease, 2, 1, resources.as_ptr(), 2) == INVALID);
        check!(resource.refs.get() == 1 && other.refs.get() == 1 && device.refs.get() == 1);
        resources[1] = resource.raw();
        check!(rsf_fg_leases_acquire(lease, 2, 1, resources.as_ptr(), 2) == OK);
        check!(rsf_fg_leases_submit(lease, 2, foreign_fence.raw(), 1) == INVALID);
        check!(rsf_fg_leases_seal(lease, 2, &retirement(foreign_fence.raw(), 1)) == NOT_READY);
        check!(rsf_fg_leases_submit(lease, 2, queue.raw(), 1) == OK);
        check!(rsf_fg_leases_seal(lease, 2, &retirement(foreign_fence.raw(), 1)) == INVALID);
        check!(foreign_fence.refs.get() == 1 && device.refs.get() == 1 && foreign.refs.get() == 1);
        check!(rsf_fg_leases_seal(lease, 2, &retirement(vendor.raw(), 1)) == OK);
        check!(rsf_fg_leases_acquire(lease, 3, 1, resources.as_ptr(), 1) == OK);
        check!(rsf_fg_leases_submit(lease, 3, completed.raw(), 1) == OK);
        check!(rsf_fg_leases_seal(lease, 3, &retirement(null_mut(), 0)) == OK);

        let removed = if queue_removed { &queue } else { &vendor };
        removed.completed.set(u64::MAX);
        check!(rsf_fg_leases_collect(lease, &mut pending) == DEVICE_FAILED && pending == 1);
        check!(completed.refs.get() == 1);
        check!(rsf_fg_leases_destroy(lease) == DEVICE_FAILED);
        check!(resource.refs.get() == 3 && queue.refs.get() == 2 && vendor.refs.get() == 2);
        check!(rsf_fg_leases_acquire(lease, 4, 1, resources.as_ptr(), 1) == NOT_READY);
        check!(rsf_fg_leases_cancel(lease, 2) == NOT_READY);
        check!(rsf_fg_leases_abandon_device_lost(lease) == OK);
        check!(removed.completed.get() == u64::MAX);
        check!(
            resource.refs.get() == 1
                && queue.refs.get() == 1
                && vendor.refs.get() == 1
                && device.refs.get() == 1
        );
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_fg_leases_faults() -> i32 {
    check_fence_failure(true);
    check_fence_failure(false);
    0
}
