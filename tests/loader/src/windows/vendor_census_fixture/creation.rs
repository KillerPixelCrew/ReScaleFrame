// SPDX-License-Identifier: MIT
use super::{ConstPtr, Ptr};
use std::ffi::CStr;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;

static EARLY: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
const NAMES: [&CStr; 4] = [
    c"CreateDXGIFactory",
    c"CreateDXGIFactory1",
    c"CreateDXGIFactory2",
    c"D3D12CreateDevice",
];

fn raw() -> [Ptr; 4] {
    [
        fixture_CreateDXGIFactory as Ptr,
        fixture_CreateDXGIFactory1 as Ptr,
        fixture_CreateDXGIFactory2 as Ptr,
        fixture_D3D12CreateDevice as Ptr,
    ]
}

#[unsafe(no_mangle)]
extern "C" fn fixture_CreateDXGIFactory(_: ConstPtr, _: *mut Ptr) -> i32 {
    0x51
}
#[unsafe(no_mangle)]
extern "C" fn fixture_CreateDXGIFactory1(_: ConstPtr, _: *mut Ptr) -> i32 {
    0x52
}
#[unsafe(no_mangle)]
extern "C" fn fixture_CreateDXGIFactory2(_: u32, _: ConstPtr, _: *mut Ptr) -> i32 {
    0x53
}
#[unsafe(no_mangle)]
extern "C" fn fixture_D3D12CreateDevice(_: Ptr, _: i32, _: ConstPtr, _: *mut Ptr) -> i32 {
    0x54
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_census_creation_attach(module: HMODULE) {
    for (address, name) in EARLY.iter().zip(NAMES) {
        let entry = unsafe { GetProcAddress(module, name.as_ptr().cast()) };
        address.store(entry.map_or(0, |entry| entry as usize), Ordering::Release);
    }
}

#[unsafe(no_mangle)]
extern "C" fn fixture_creation_identity() -> u32 {
    let mut matched = 0;
    for (index, (entry, expected)) in EARLY.iter().zip(raw()).enumerate() {
        if entry.load(Ordering::Acquire) == expected as usize {
            matched |= 1 << index;
        }
    }
    matched
}

#[unsafe(no_mangle)]
extern "C" fn fixture_creation_raw(index: u32) -> Ptr {
    raw().get(index as usize).copied().unwrap_or(null_mut())
}
