// SPDX-License-Identifier: MIT
use std::ffi::c_void;
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

#[derive(Default)]
pub(super) struct MemoryCache {
    low: usize,
    high: usize,
}

pub(super) fn readable_extent(cache: &mut MemoryCache, start: usize, size: usize) -> usize {
    let mut offset = 0;
    while offset < size {
        let Some(address) = start.checked_add(offset) else {
            break;
        };
        if !(cache.low..cache.high).contains(&address) {
            let Some(region) = Region::containing(address).filter(|region| region.readable) else {
                *cache = MemoryCache::default();
                break;
            };
            cache.low = region.start;
            cache.high = region.end;
        }
        offset += (cache.high - address).min(size - offset);
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

pub(super) fn read_exact<const N: usize>(
    cache: &mut MemoryCache,
    address: usize,
) -> Option<[u8; N]> {
    if readable_extent(cache, address, N) != N {
        return None;
    }
    let mut bytes = [0; N];
    let (ok, copied) = unsafe { read(address, bytes.as_mut_ptr(), N) };
    if !ok || copied != N {
        *cache = MemoryCache::default();
        return None;
    }
    Some(bytes)
}

pub(super) fn read_guarded(source: usize, destination: &mut [u8]) -> usize {
    destination.fill(0);
    let size = destination.len();
    let mut copied = 0;
    let mut offset = 0;
    while offset < size {
        let Some(address) = source.checked_add(offset) else {
            break;
        };
        let Some(region) = Region::containing(address) else {
            break;
        };
        let available = (region.end - address).min(size - offset);
        if region.readable {
            let chunk = &mut destination[offset..offset + available];
            let (_, read_bytes) = unsafe { read(address, chunk.as_mut_ptr(), available) };
            chunk[read_bytes..].fill(0);
            copied += read_bytes;
        }
        offset += available;
    }
    copied
}
