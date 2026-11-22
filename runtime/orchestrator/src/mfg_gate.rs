// SPDX-License-Identifier: MIT
// NGX provider count/index gate and protected executable publication, adapted
// from MFG-Unlocker 2b52823e ngx_mfg_gate.h and protected_pointer.h
// (Copyright 2026 Michael Robles, MIT; notice in third_party/mfg_unlocker).
use super::Handle;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Mutex, PoisonError};
use windows_sys::Win32::System::{
    Diagnostics::Debug::{
        FlushInstructionCache, IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC,
        IMAGE_SCN_MEM_EXECUTE, IMAGE_SECTION_HEADER, RtlLookupFunctionEntry,
    },
    Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_WRITECOPY,
        PAGE_GUARD, PAGE_NOCACHE, PAGE_WRITECOMBINE, VirtualProtect, VirtualQuery,
    },
    SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE},
    Threading::GetCurrentProcess,
};

// This is the provider's count/index validator, not the shared NGX runtime's
// feature-support query. All count, index and profile-limit checks remain.
const PATTERN: [u8; 13] = [
    0x84, 0xd2, 0x0f, 0x84, 0x03, 0x01, 0x00, 0x00, 0xbe, 0x05, 0x00, 0x00, 0x00,
];
const ORIGINAL: [u8; 2] = [0x0f, 0x84];
// Jump over the remaining four bytes of the old near-branch displacement.
const REPLACEMENT: [u8; 2] = [0xeb, 0x04];
const COUNT_ONE: [u8; 4] = [0x41, 0x83, 0xf8, 0x01];
const PROTECTION_MASK: u32 = 0xff | PAGE_GUARD | PAGE_NOCACHE | PAGE_WRITECOMBINE;

fn query(address: usize, size: usize) -> Option<u32> {
    let mut memory = MEMORY_BASIC_INFORMATION::default();
    if address == 0
        || size == 0
        || unsafe {
            VirtualQuery(
                address as *const c_void,
                &mut memory,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        } != size_of::<MEMORY_BASIC_INFORMATION>()
        || memory.State != MEM_COMMIT
    {
        return None;
    }
    let begin = memory.BaseAddress as usize;
    (memory.RegionSize >= size && address >= begin && address - begin <= memory.RegionSize - size)
        .then_some(memory.Protect)
}

pub(super) fn protection_matches(address: usize, expected: u32, size: usize) -> bool {
    query(address, size)
        .is_some_and(|protection| protection & PROTECTION_MASK == expected & PROTECTION_MASK)
}

fn set_and_verify(address: usize, size: usize, protection: u32) -> bool {
    let mut ignored = 0;
    unsafe { VirtualProtect(address as *const c_void, size, protection, &mut ignored) };
    protection_matches(address, protection, size)
}

// A failed VirtualProtect is normally non-mutating, but one retry keeps a
// transient failure from leaving the temporary writable page behind.
fn restore(address: usize, size: usize, protection: u32) -> bool {
    set_and_verify(address, size, protection) || set_and_verify(address, size, protection)
}

fn flush(address: usize, size: usize) -> bool {
    unsafe { FlushInstructionCache(GetCurrentProcess(), address as *const c_void, size) != 0 }
}

// The caller verified that the two bytes lie in committed image memory.
fn bytes(address: usize) -> [u8; 2] {
    unsafe { (address as *const [u8; 2]).read_volatile() }
}

// Publishes two aligned executable bytes from PAGE_EXECUTE_READ. Only a
// certified exchange by this call with restored protection counts as success;
// a foreign or already-visible value never authorizes publication.
fn replace_bytes(address: usize, expected: [u8; 2], replacement: [u8; 2]) -> bool {
    const FINAL: u32 = PAGE_EXECUTE_READ;
    const WRITABLE: u32 = PAGE_EXECUTE_WRITECOPY;
    if address == 0 || address & 1 != 0 || query(address, 2).is_none() {
        return false;
    }
    if !protection_matches(address, FINAL, 2) {
        restore(address, 2, FINAL);
        return false;
    }
    if bytes(address) == replacement {
        return true;
    }
    if bytes(address) != expected {
        return false;
    }
    if !set_and_verify(address, 2, WRITABLE) {
        restore(address, 2, FINAL);
        if bytes(address) == replacement {
            flush(address, 2);
        }
        return false;
    }
    // Revalidate after making the page writable so another writer is never overwritten.
    if bytes(address) != expected {
        if bytes(address) == replacement {
            flush(address, 2);
        }
        restore(address, 2, FINAL);
        return false;
    }
    let slot = unsafe { AtomicU16::from_ptr(address as *mut u16) };
    let swap = |from: [u8; 2], to: [u8; 2]| {
        slot.compare_exchange(
            u16::from_ne_bytes(from),
            u16::from_ne_bytes(to),
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
    };
    if let Err(observed) = swap(expected, replacement) {
        // A competing writer published these bytes. Flush for containment but never claim them.
        if observed.to_ne_bytes() == replacement {
            flush(address, 2);
        }
        restore(address, 2, FINAL);
        return false;
    }
    let flushed = flush(address, 2);
    if flushed && restore(address, 2, FINAL) && bytes(address) == replacement {
        return true;
    }
    // The replacement was visible but final protection was not certified.
    // Re-establish writable access before any rollback.
    if !set_and_verify(address, 2, WRITABLE) {
        return restore(address, 2, FINAL) && flushed && bytes(address) == replacement;
    }
    if bytes(address) == replacement && (swap(replacement, expected).is_err() || !flush(address, 2))
    {
        restore(address, 2, FINAL);
        return false;
    }
    // A rollback, successful or not, is never a publication.
    restore(address, 2, FINAL);
    false
}

// Returns the unique validated candidate. The provider is retained for the
// scan, and ambiguity never selects the first match.
fn find(module: Handle) -> Option<usize> {
    if module.is_null() {
        return None;
    }
    let base = module as usize;
    let dos = unsafe { &*(base as *const IMAGE_DOS_HEADER) };
    if dos.e_magic != IMAGE_DOS_SIGNATURE || dos.e_lfanew <= 0 || dos.e_lfanew > 0x10_0000 {
        return None;
    }
    let nt_offset = dos.e_lfanew as usize;
    let nt = unsafe { &*((base + nt_offset) as *const IMAGE_NT_HEADERS64) };
    if nt.Signature != IMAGE_NT_SIGNATURE
        || nt.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC
        || nt.FileHeader.NumberOfSections > 96
    {
        return None;
    }
    let image = nt.OptionalHeader.SizeOfImage as usize;
    let valid = |rva: usize, size: usize| rva < image && size <= image - rva;
    let table = base
        + nt_offset
        + std::mem::offset_of!(IMAGE_NT_HEADERS64, OptionalHeader)
        + usize::from(nt.FileHeader.SizeOfOptionalHeader);
    let mut found = None;
    for index in 0..usize::from(nt.FileHeader.NumberOfSections) {
        let section = unsafe {
            &*((table + index * size_of::<IMAGE_SECTION_HEADER>()) as *const IMAGE_SECTION_HEADER)
        };
        let rva = section.VirtualAddress as usize;
        if section.Characteristics & IMAGE_SCN_MEM_EXECUTE == 0 || rva >= image {
            continue;
        }
        let size = (image - rva)
            .min(unsafe { section.Misc.VirtualSize }.max(section.SizeOfRawData) as usize);
        if size < PATTERN.len() {
            continue;
        }
        let code = unsafe { std::slice::from_raw_parts((base + rva) as *const u8, size) };
        for (offset, window) in code.windows(PATTERN.len()).enumerate() {
            let tail = [window[2], window[3]];
            if window[..2] != PATTERN[..2]
                || (tail != ORIGINAL && tail != REPLACEMENT)
                || window[4..] != PATTERN[4..]
            {
                continue;
            }
            if found.is_some() {
                return None;
            }
            let address = base + rva + offset;
            let match_rva = address - base;
            let displacement = i32::from_le_bytes([window[4], window[5], window[6], window[7]]);
            let target = (match_rva + 8).wrapping_add_signed(displacement as isize);
            if (address + 2) & 1 != 0
                || !valid(target, COUNT_ONE.len())
                || unsafe { *((base + target) as *const [u8; 4]) } != COUNT_ONE
            {
                return None;
            }
            let mut image_base = 0;
            let function =
                unsafe { RtlLookupFunctionEntry(address as u64, &mut image_base, null_mut()) };
            let function = unsafe { function.as_ref() }?;
            let begin = function.BeginAddress as usize;
            let end = function.EndAddress as usize;
            if image_base != base as u64
                || begin > match_rva
                || end <= target + COUNT_ONE.len()
                || !valid(
                    begin,
                    function.EndAddress.wrapping_sub(function.BeginAddress) as usize,
                )
                || !protection_matches(address, PAGE_EXECUTE_READ, PATTERN.len())
                || !protection_matches(base + target, PAGE_EXECUTE_READ, COUNT_ONE.len())
            {
                return None;
            }
            found = Some(address);
        }
    }
    found
}

#[derive(Clone, Copy)]
struct Publication {
    module: usize,
    address: usize,
    ready: bool,
}

static PUBLICATIONS: Mutex<[Option<Publication>; 64]> = Mutex::new([None; 64]);

// Retention establishes module lifetime before scanning or publishing. No
// filename, sibling version, cached base address or UI maximum admits it.
pub(super) fn patch(module: Handle) -> bool {
    if !super::provider::retained_provider(module) {
        return false;
    }
    let mut publications = PUBLICATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(site) = find(module) else {
        return false;
    };
    let tail = site + 2;
    if let Some(item) = publications
        .iter()
        .flatten()
        .find(|item| item.module == module as usize)
    {
        return item.ready && item.address == site && bytes(tail) == REPLACEMENT;
    }
    // A foreign edit must not be adopted as this module's successful patch.
    if bytes(tail) != ORIGINAL {
        return false;
    }
    let Some(slot) = publications.iter_mut().find(|item| item.is_none()) else {
        return false;
    };
    // An ambiguous publication stays permanently closed for this module.
    let ready = replace_bytes(tail, ORIGINAL, REPLACEMENT);
    *slot = Some(Publication {
        module: module as usize,
        address: site,
        ready,
    });
    ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::LibraryLoader::{DONT_RESOLVE_DLL_REFERENCES, LoadLibraryExW};

    // Set RSF_DLSSG_PROVIDER to an eligible nvngx_dlssg.dll. The image is mapped
    // without running its code, and nothing is published.
    #[test]
    #[ignore = "needs a local DLSS-G provider"]
    fn provider_has_one_unpatched_gate() {
        let path: Vec<u16> = std::env::var("RSF_DLSSG_PROVIDER")
            .expect("RSF_DLSSG_PROVIDER")
            .encode_utf16()
            .chain([0])
            .collect();
        let module = crate::sdk_module::SdkModule::adopt_system(unsafe {
            LoadLibraryExW(path.as_ptr(), null_mut(), DONT_RESOLVE_DLL_REFERENCES)
        })
        .expect("map provider");
        let site = find(module.as_ptr()).expect("unique gate");
        assert_eq!(bytes(site + 2), ORIGINAL);
    }
}
