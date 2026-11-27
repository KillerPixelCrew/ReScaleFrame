// SPDX-License-Identifier: MIT
use super::entropy::{entropy, sampled};
use super::memory::{MemoryCache, read, read_guarded, readable_extent};
use super::names::escape_unit;
use super::sections::{Stream, rsf_dump_now_ms};
use crate::sys::{rsf_dump_image_info, rsf_dump_image_stats, rsf_module_entry, rsf_section_digest};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write as _};
use std::mem::offset_of;
use std::os::windows::ffi::OsStringExt;
use std::slice;
use windows_sys::Win32::System::Diagnostics::Debug::{
    IMAGE_DIRECTORY_ENTRY_SECURITY, IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE, IMAGE_NT_HEADERS64,
    IMAGE_OPTIONAL_HEADER64, IMAGE_SCN_CNT_INITIALIZED_DATA, IMAGE_SCN_CNT_UNINITIALIZED_DATA,
    IMAGE_SCN_MEM_EXECUTE, IMAGE_SECTION_HEADER,
};

const IMPORT_DESCRIPTOR_BYTES: usize = 20;
const IMPORT_ORDINAL: u64 = 1 << 63;
const IMPORT_NAME_BYTES: usize = 260;
const ENTROPY_WINDOW: usize = 65_536;
const ENTROPY_SAMPLES: usize = 16;
const SECTION_BYTES: usize = size_of::<IMAGE_SECTION_HEADER>();

struct LiveImage {
    base: usize,
    size: usize,
}

impl LiveImage {
    fn address(&self, rva: u32, length: usize) -> Option<usize> {
        let rva = rva as usize;
        if rva.checked_add(length)? > self.size {
            return None;
        }
        let address = self.base.checked_add(rva)?;
        address.checked_add(length)?;
        Some(address)
    }

    fn bytes<const N: usize>(&self, rva: u32) -> Option<[u8; N]> {
        let address = self.address(rva, N)?;
        let mut output = [0; N];
        let (ok, copied) = unsafe { read(address, output.as_mut_ptr(), N) };
        (ok && copied == N).then_some(output)
    }

    fn walk_imports(&self, directory: u32, mut visit: impl FnMut(u32, u32, u64) -> bool) {
        if directory == 0 {
            return;
        }
        let mut descriptor = directory;
        while let Some(data) = self.bytes::<IMPORT_DESCRIPTOR_BYTES>(descriptor) {
            let library = word32(&data, 12);
            if library == 0 {
                break;
            }
            let first = word32(&data, 16);
            let original = word32(&data, 0);
            let mut name = if original == 0 { first } else { original };
            if name != 0 && first != 0 {
                let mut slot = first;
                while let Some(thunk) = self.bytes::<8>(name).map(u64::from_le_bytes) {
                    if thunk == 0 || self.address(slot, 8).is_none() {
                        break;
                    }
                    if !visit(library, slot, thunk) {
                        return;
                    }
                    let (Some(next_name), Some(next_slot)) =
                        (name.checked_add(8), slot.checked_add(8))
                    else {
                        break;
                    };
                    name = next_name;
                    slot = next_slot;
                }
            }
            let Some(next) = descriptor.checked_add(IMPORT_DESCRIPTOR_BYTES as u32) else {
                break;
            };
            descriptor = next;
        }
    }

    fn import_text(&self, out: &mut Stream, rva: u32) {
        let length = self
            .size
            .saturating_sub(rva as usize)
            .min(IMPORT_NAME_BYTES);
        if length == 0 {
            return;
        }
        let Some(address) = self.address(rva, length) else {
            return;
        };
        let mut buffer = [0u8; IMPORT_NAME_BYTES];
        let mut cache = MemoryCache::default();
        let length = readable_extent(&mut cache, address, length);
        if length != 0 {
            let (_, copied) = unsafe { read(address, buffer.as_mut_ptr(), length) };
            for &byte in buffer[..copied].iter().take_while(|&&byte| byte != 0) {
                escape_unit(out, byte.into());
            }
        }
    }

    fn slots(&self, directory: u32) -> Vec<u32> {
        let mut slots = Vec::new();
        let mut complete = true;
        self.walk_imports(directory, |_, slot, _| {
            if slots.try_reserve(1).is_err() {
                complete = false;
                return false;
            }
            slots.push(slot);
            true
        });
        if !complete {
            return Vec::new();
        }
        slots.sort_unstable();
        slots
    }
}

fn word32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn word16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn set32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn aligned(size: u32, alignment: u32) -> Option<u32> {
    if alignment == 0 {
        Some(size)
    } else {
        Some(size.checked_add(alignment - 1)? & !(alignment - 1))
    }
}

fn count_references(section: &[u8], rva: u32, slots: &[u32]) -> u32 {
    if slots.is_empty() {
        return 0;
    }
    section
        .windows(6)
        .enumerate()
        .filter(|&(offset, bytes)| {
            if bytes[0] != 0xff || !matches!(bytes[1], 0x15 | 0x25) {
                return false;
            }
            let target = rva
                .wrapping_add(offset as u32)
                .wrapping_add(6)
                .wrapping_add(word32(bytes, 2));
            slots.binary_search(&target).is_ok()
        })
        .count() as u32
}

fn scrub_optional(bytes: &mut [u8], base: usize, alignment: u32, header_size: u32) {
    type Header = IMAGE_OPTIONAL_HEADER64;
    set32(bytes, offset_of!(Header, FileAlignment), alignment);
    set32(bytes, offset_of!(Header, SizeOfHeaders), header_size);
    let image_base = offset_of!(Header, ImageBase);
    bytes[image_base..image_base + 8].copy_from_slice(&(base as u64).to_le_bytes());
    let flags = offset_of!(Header, DllCharacteristics);
    let dll_flags = word16(bytes, flags) & !IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE;
    bytes[flags..flags + 2].copy_from_slice(&dll_flags.to_le_bytes());
    set32(bytes, offset_of!(Header, CheckSum), 0);
    let security =
        offset_of!(Header, DataDirectory) + usize::from(IMAGE_DIRECTORY_ENTRY_SECURITY) * 8;
    if let Some(directory) = bytes.get_mut(security..security + 8) {
        directory.fill(0);
    }
}

fn scrub_section(bytes: &mut [u8], alignment: u32) -> Option<(u32, u32, u32)> {
    type Section = IMAGE_SECTION_HEADER;
    let rva = word32(bytes, offset_of!(Section, VirtualAddress));
    let virtual_size = word32(bytes, offset_of!(Section, Misc));
    let size = if virtual_size == 0 {
        word32(bytes, offset_of!(Section, SizeOfRawData))
    } else {
        virtual_size
    };
    let flags = word32(bytes, offset_of!(Section, Characteristics));
    set32(bytes, offset_of!(Section, PointerToRawData), rva);
    set32(
        bytes,
        offset_of!(Section, SizeOfRawData),
        aligned(size, alignment)?,
    );
    bytes[offset_of!(Section, PointerToRelocations)..offset_of!(Section, Characteristics)].fill(0);
    if flags & IMAGE_SCN_CNT_UNINITIALIZED_DATA != 0 {
        let initialized =
            flags & !IMAGE_SCN_CNT_UNINITIALIZED_DATA | IMAGE_SCN_CNT_INITIALIZED_DATA;
        set32(bytes, offset_of!(Section, Characteristics), initialized);
    }
    Some((rva, size, flags))
}

fn resize(buffer: &mut Vec<u8>, size: usize) -> Option<()> {
    if size > buffer.len() {
        buffer.try_reserve_exact(size - buffer.len()).ok()?;
        buffer.resize(size, 0);
    }
    Some(())
}

unsafe fn wide_path(path: *const u16) -> Option<OsString> {
    if path.is_null() {
        return None;
    }
    let length = (0..520).find(|&index| unsafe { path.add(index).read() } == 0)?;
    Some(OsString::from_wide(unsafe {
        slice::from_raw_parts(path, length)
    }))
}

pub(super) unsafe fn dump_image(
    info: &rsf_dump_image_info,
    digests: *mut rsf_section_digest,
) -> Option<rsf_dump_image_stats> {
    let image = LiveImage {
        base: info.base as usize,
        size: info.image_size as usize,
    };
    let header_size = aligned(info.header_size, info.alignment)?;
    let header_source = image.address(0, header_size as usize)?;
    let section_start = info.sections_offset as usize;
    let section_end =
        section_start.checked_add((info.sections as usize).checked_mul(SECTION_BYTES)?)?;
    let optional =
        (info.nt_offset as usize).checked_add(offset_of!(IMAGE_NT_HEADERS64, OptionalHeader))?;
    let optional_fields =
        optional.checked_add(offset_of!(IMAGE_OPTIONAL_HEADER64, DataDirectory))?;
    if optional_fields > section_start || section_end > header_size as usize {
        return None;
    }
    let mut header = Vec::new();
    resize(&mut header, header_size as usize)?;
    let (ok, copied) = unsafe { read(header_source, header.as_mut_ptr(), section_end) };
    if !ok
        || copied != section_end
        || header.get(..2) != Some(b"MZ")
        || header
            .get(info.nt_offset as usize..optional)
            .and_then(|nt| nt.get(..4))
            != Some(b"PE\0\0")
    {
        return None;
    }
    let padding = &mut header[section_end..];
    if !padding.is_empty() {
        read_guarded(header_source + section_end, padding);
    }
    scrub_optional(
        &mut header[optional..section_start],
        image.base,
        info.alignment,
        header_size,
    );
    let mut sections = Vec::new();
    sections.try_reserve_exact(info.sections as usize).ok()?;
    for row in header[section_start..section_end].chunks_exact_mut(SECTION_BYTES) {
        let (rva, size, flags) = scrub_section(row, info.alignment)?;
        if size != 0 {
            image.address(rva, size as usize)?;
        }
        sections.push((rva, size, flags));
    }
    let slots = image.slots(info.import_directory);
    let mut file = File::create(unsafe { wide_path(info.path) }?).ok()?;
    file.write_all(&header).ok()?;
    let mut stats = rsf_dump_image_stats {
        bytes_written: header_size.into(),
        hash_ms: 0.0,
        sections_written: 0,
        iat_references: 0,
    };
    let mut buffer = Vec::new();
    for (index, (rva, size, flags)) in sections.into_iter().enumerate() {
        if size == 0 {
            continue;
        }
        let len = size as usize;
        let source = image.address(rva, len)?;
        resize(&mut buffer, len)?;
        let buffer = &mut buffer[..len];
        let readable = read_guarded(source, buffer);
        if !digests.is_null() {
            let started = rsf_dump_now_ms();
            let digest = rsf_section_digest {
                valid: 1,
                dumped_size: size.into(),
                readable_bytes: readable as u64,
                entropy: sampled(len, ENTROPY_WINDOW, ENTROPY_SAMPLES, |offset, length| {
                    entropy(&buffer[offset..offset + length])
                }),
                sha256: sha256_text(buffer),
            };
            unsafe { digests.add(index).write(digest) };
            stats.hash_ms += rsf_dump_now_ms() - started;
        }
        if flags & IMAGE_SCN_MEM_EXECUTE != 0 {
            stats.iat_references = stats
                .iat_references
                .wrapping_add(count_references(buffer, rva, &slots));
        }
        file.seek(SeekFrom::Start(rva.into())).ok()?;
        file.write_all(buffer).ok()?;
        stats.bytes_written += u64::from(size);
        stats.sections_written += 1;
    }
    file.flush().ok()?;
    Some(stats)
}

/// Lowercase hex SHA-256, NUL terminated for the C record; empty when the hash cannot be taken.
fn sha256_text(bytes: &[u8]) -> [std::ffi::c_char; 65] {
    let mut text = [0; 65];
    for (slot, digit) in text
        .iter_mut()
        .zip(rsf_windows::sha256_hex(bytes).unwrap_or([0; 64]))
    {
        *slot = digit as std::ffi::c_char;
    }
    text
}

pub(super) unsafe fn write_imports(
    output: &mut Stream,
    base: *const u8,
    image_size: u32,
    directory: u32,
    modules: *const rsf_module_entry,
    module_count: usize,
) -> u32 {
    let image = LiveImage {
        base: base as usize,
        size: image_size as usize,
    };
    let modules = if modules.is_null() || module_count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(modules, module_count) }
    };
    let mut count = 0u32;
    image.walk_imports(directory, |library, slot, thunk| {
        let Some(value) = image.bytes::<8>(slot).map(u64::from_le_bytes) else {
            return true;
        };
        let comma = if count == 0 { "" } else { "," };
        let _ = write!(output, "{comma}\n    {{\"dll\": \"");
        image.import_text(output, library);
        output.emit("\", ");
        if thunk & IMPORT_ORDINAL != 0 {
            let _ = write!(output, "\"ordinal\": {}", thunk & 0xffff);
        } else {
            output.emit("\"name\": \"");
            if let Some(name) = (thunk as u32).checked_add(2) {
                image.import_text(output, name);
            }
            output.emit("\"");
        }
        let _ = write!(
            output,
            ", \"slot_rva\": \"{slot:#x}\", \"value\": \"{value:#x}\""
        );
        if let Some(owner) = modules.iter().find(|module| {
            value
                .checked_sub(module.base as u64)
                .is_some_and(|rva| rva < module.size as u64)
        }) {
            output.emit(", \"resolves_into\": \"");
            for &unit in owner.path.iter().take_while(|&&unit| unit != 0) {
                escape_unit(output, unit);
            }
            output.emit("\"");
        }
        output.emit("}");
        count = count.wrapping_add(1);
        true
    });
    count
}

pub(super) struct Section {
    pub(super) name: [u8; 8],
    pub(super) rva: u32,
    pub(super) virtual_size: u32,
    pub(super) raw_size: u32,
    pub(super) flags: u32,
}

impl Section {
    pub(super) fn mapped_size(&self) -> u32 {
        if self.virtual_size == 0 {
            self.raw_size
        } else {
            self.virtual_size
        }
    }
}

pub(super) fn sidecar_section(info: &rsf_dump_image_info, index: u32) -> Option<Section> {
    let image = LiveImage {
        base: info.base as usize,
        size: info.image_size as usize,
    };
    let offset = index
        .checked_mul(SECTION_BYTES as u32)?
        .checked_add(info.sections_offset)?;
    let row = image.bytes::<SECTION_BYTES>(offset)?;
    Some(Section {
        name: row[..8].try_into().unwrap(),
        rva: word32(&row, offset_of!(IMAGE_SECTION_HEADER, VirtualAddress)),
        virtual_size: word32(&row, offset_of!(IMAGE_SECTION_HEADER, Misc)),
        raw_size: word32(&row, offset_of!(IMAGE_SECTION_HEADER, SizeOfRawData)),
        flags: word32(&row, offset_of!(IMAGE_SECTION_HEADER, Characteristics)),
    })
}
