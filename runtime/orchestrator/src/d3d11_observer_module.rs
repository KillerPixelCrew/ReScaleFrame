// SPDX-License-Identifier: MIT
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read as _;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::ptr;

const FILE_SHARE_READ: u32 = 1;
const MAX_FILE: u64 = 1 << 28;
const OK: u32 = 0;
const OPEN_FAILED: u32 = 1;
const READ_FAILED: u32 = 2;
const INVALID_HEADERS: u32 = 3;

// Windows SDK IMAGE_FILE_HEADER and IMAGE_SECTION_HEADER sizes.
const COFF_SIZE: usize = 20;
const SECTION_SIZE: usize = 40;

fn le<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..)?.get(..N)?.try_into().ok()
}

struct ModuleFile {
    bytes: Vec<u8>,
    sections: std::ops::Range<usize>,
    preferred_base: u64,
}

impl ModuleFile {
    fn parse(bytes: Vec<u8>) -> Option<Self> {
        if le::<2>(&bytes, 0)? != *b"MZ" {
            return None;
        }
        let nt = u32::from_le_bytes(le(&bytes, 0x3c)?) as usize;
        if le::<4>(&bytes, nt)? != *b"PE\0\0" {
            return None;
        }
        let coff = bytes.get(nt.checked_add(4)?..)?.get(..COFF_SIZE)?;
        // The observer resolves AMD64 function pointers from PE32+ images.
        if u16::from_le_bytes(le(coff, 0)?) != 0x8664 {
            return None;
        }
        let count = usize::from(u16::from_le_bytes(le(coff, 2)?));
        let optional_size = usize::from(u16::from_le_bytes(le(coff, 16)?));
        let optional_start = nt.checked_add(4 + COFF_SIZE)?;
        let sections_start = optional_start.checked_add(optional_size)?;
        let optional = bytes.get(optional_start..sections_start)?;
        if u16::from_le_bytes(le(optional, 0)?) != 0x20b {
            return None;
        }
        let preferred_base = u64::from_le_bytes(le(optional, 24)?);
        let sections_end = sections_start.checked_add(count.checked_mul(SECTION_SIZE)?)?;
        bytes.get(sections_start..sections_end)?;
        Some(Self {
            bytes,
            sections: sections_start..sections_end,
            preferred_base,
        })
    }

    fn span(&self, rva: usize, count: usize) -> Option<&[u8]> {
        for section in self.bytes[self.sections.clone()].chunks_exact(SECTION_SIZE) {
            let virtual_size = u32::from_le_bytes(le(section, 8)?) as usize;
            let virtual_address = u32::from_le_bytes(le(section, 12)?) as usize;
            let raw_size = u32::from_le_bytes(le(section, 16)?) as usize;
            let raw_offset = u32::from_le_bytes(le(section, 20)?) as usize;
            let Some(delta) = rva.checked_sub(virtual_address) else {
                continue;
            };
            if delta >= virtual_size.max(raw_size) {
                continue;
            }
            // A virtual tail does not have on-disk bytes. Do not read into the next section.
            if delta.checked_add(count)? > raw_size {
                return None;
            }
            let start = raw_offset.checked_add(delta)?;
            return self.bytes.get(start..)?.get(..count);
        }
        None
    }
}

fn open(path: &[u16]) -> Result<ModuleFile, u32> {
    let path = OsString::from_wide(path);
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .map_err(|_| OPEN_FAILED)?;
    let length = file.metadata().map_err(|_| READ_FAILED)?.len();
    if !(1..MAX_FILE).contains(&length) {
        return Err(READ_FAILED);
    }
    let length = length as usize;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| READ_FAILED)?;
    bytes.resize(length, 0);
    file.read_exact(&mut bytes).map_err(|_| READ_FAILED)?;
    ModuleFile::parse(bytes).ok_or(INVALID_HEADERS)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_module_open(
    path: *const u16,
    length: u32,
    out: *mut *mut ModuleFile,
) -> u32 {
    if out.is_null() {
        return READ_FAILED;
    }
    unsafe { out.write(ptr::null_mut()) };
    if path.is_null() || length == 0 {
        return OPEN_FAILED;
    }
    let path = unsafe { std::slice::from_raw_parts(path, length as usize) };
    let result = match open(path) {
        Ok(file) => crate::allocate_handle(file),
        Err(error) => return error,
    };
    if result.is_null() {
        return READ_FAILED;
    }
    unsafe { out.write(result) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_module_preferred_base(file: &ModuleFile) -> u64 {
    file.preferred_base
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_module_span(
    file: &ModuleFile,
    rva: usize,
    count: usize,
) -> *const u8 {
    file.span(rva, count).map_or(ptr::null(), <[u8]>::as_ptr)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_module_close(file: *mut ModuleFile) {
    if !file.is_null() {
        drop(unsafe { Box::from_raw(file) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> Vec<u8> {
        let mut bytes = vec![0; 0x210];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
        bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes[0xb0..0xb8].copy_from_slice(&0x1_8000_0000u64.to_le_bytes());
        bytes[0x190..0x194].copy_from_slice(&16u32.to_le_bytes());
        bytes[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[0x198..0x19c].copy_from_slice(&8u32.to_le_bytes());
        bytes[0x19c..0x1a0].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[0x200..].fill(0x5a);
        bytes
    }

    #[test]
    fn truncated_section_table_is_refused() {
        let mut bytes = file();
        bytes.truncate(0x188 + SECTION_SIZE - 1);
        assert!(ModuleFile::parse(bytes).is_none());
    }

    #[test]
    fn virtual_tail_cannot_read_more_bytes_from_disk() {
        let image = ModuleFile::parse(file()).unwrap();
        assert_eq!(image.preferred_base, 0x1_8000_0000);
        assert_eq!(image.span(0x1004, 4), Some(&[0x5a; 4][..]));
        assert!(image.span(0x1004, 5).is_none());
        assert!(image.span(0x1008, 1).is_none());
    }
}
