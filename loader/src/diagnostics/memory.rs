// SPDX-License-Identifier: MIT
use super::memory_abi::rsf_mem_cache;
use std::ffi::{c_int, c_void};
use std::ptr;
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory,
    Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
        PAGE_WRITECOPY, VirtualQuery,
    },
    Threading::GetCurrentProcess,
};

pub(super) struct Region {
    start: usize,
    pub(super) end: usize,
    pub(super) readable: bool,
}

impl Region {
    pub(super) fn containing(address: usize) -> Option<Self> {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let size = size_of::<MEMORY_BASIC_INFORMATION>();
        if unsafe { VirtualQuery(address as *const c_void, &mut info, size) } != size {
            return None;
        }
        let start = info.BaseAddress as usize;
        let end = start.checked_add(info.RegionSize)?;
        if !(start..end).contains(&address) {
            return None;
        }
        let readable = PAGE_READONLY
            | PAGE_READWRITE
            | PAGE_WRITECOPY
            | PAGE_EXECUTE_READ
            | PAGE_EXECUTE_READWRITE
            | PAGE_EXECUTE_WRITECOPY;
        Some(Self {
            start,
            end,
            readable: info.State == MEM_COMMIT
                && info.Protect & readable != 0
                && info.Protect & (PAGE_GUARD | PAGE_NOACCESS) == 0,
        })
    }
}

pub(super) fn readable_extent(cache: &mut rsf_mem_cache, start: usize, size: usize) -> usize {
    let mut offset = 0;
    while offset < size {
        let Some(address) = start.checked_add(offset) else {
            break;
        };
        if !(cache.low as usize..cache.high as usize).contains(&address) {
            let Some(region) = Region::containing(address).filter(|region| region.readable) else {
                *cache = rsf_mem_cache::default();
                break;
            };
            cache.low = region.start as *const u8;
            cache.high = region.end as *const u8;
        }
        offset += (cache.high as usize - address).min(size - offset);
    }
    offset
}

pub(super) unsafe fn read(source: usize, destination: *mut u8, size: usize) -> (bool, usize) {
    let mut copied = 0;
    let result = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            source as *const c_void,
            destination.cast(),
            size,
            &mut copied,
        )
    };
    (result != 0, copied.min(size))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_dump_readable_extent(
    cache: &mut rsf_mem_cache,
    address: *const c_void,
    wanted: usize,
) -> usize {
    readable_extent(cache, address as usize, wanted)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dump_read_exact(
    cache: &mut rsf_mem_cache,
    address: *const c_void,
    destination: *mut c_void,
    size: usize,
) -> c_int {
    if size == 0 {
        return 1;
    }
    if readable_extent(cache, address as usize, size) != size {
        return 0;
    }
    // A region can disappear after VirtualQuery. Commit an exact result only once copied.
    let mut small = [0u8; 256];
    let mut large = Vec::new();
    let scratch = if size <= small.len() {
        &mut small[..size]
    } else {
        if large.try_reserve_exact(size).is_err() {
            return 0;
        }
        large.resize(size, 0);
        &mut large
    };
    let (ok, copied) = unsafe { read(address as usize, scratch.as_mut_ptr(), size) };
    if !ok || copied != size {
        *cache = rsf_mem_cache::default();
        return 0;
    }
    unsafe { ptr::copy_nonoverlapping(scratch.as_ptr(), destination.cast(), size) };
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dump_read_guarded(
    source: *const u8,
    destination: *mut u8,
    size: usize,
) -> usize {
    if size == 0 {
        return 0;
    }
    unsafe { destination.write_bytes(0, size) };
    let mut copied = 0;
    let mut offset = 0;
    while offset < size {
        let Some(address) = (source as usize).checked_add(offset) else {
            break;
        };
        let Some(region) = Region::containing(address) else {
            break;
        };
        let available = (region.end - address).min(size - offset);
        if region.readable {
            let output = unsafe { destination.add(offset) };
            let (_, read_bytes) = unsafe { read(address, output, available) };
            unsafe {
                output
                    .add(read_bytes)
                    .write_bytes(0, available - read_bytes)
            };
            copied += read_bytes;
        }
        offset += available;
    }
    copied
}
