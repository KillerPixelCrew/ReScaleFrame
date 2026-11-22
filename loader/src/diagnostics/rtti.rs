// SPDX-License-Identifier: MIT
// MSVC x64 CompleteObjectLocator and class hierarchy recovery from a live PE image.
use super::image::{Section, sidecar_section};
use super::measure::record;
use super::memory::{MemoryCache, Region, read, read_exact, readable_extent};
use super::names::escape_unit;
use super::sections::{Stream, rsf_dump_now_ms};
use crate::sys::rsf_dump_image_info;
use std::ffi::c_char;
use std::fmt::Write as _;
use std::mem::transmute;
use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};
use windows_sys::{
    Win32::System::{
        Diagnostics::Debug::{
            IMAGE_DATA_DIRECTORY, IMAGE_DIRECTORY_ENTRY_BASERELOC, IMAGE_DIRECTORY_ENTRY_EXCEPTION,
            IMAGE_DIRECTORY_ENTRY_RESOURCE, IMAGE_NT_HEADERS64, IMAGE_SCN_MEM_DISCARDABLE,
            IMAGE_SCN_MEM_EXECUTE, IMAGE_SCN_MEM_READ,
        },
        LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
    },
    core::w,
};

const DEFAULT_BUDGET_MS: u32 = 20_000;
const CLASS_LIMIT: u32 = 400_000;
const BASE_LIMIT: u32 = 512;
const NAME_BYTES: usize = 2048;
const CACHE_SLOTS: usize = 4096;
const PRETTY_BYTES: usize = 512;
const CHUNK_BYTES: usize = 64 * 1024;
const EXCLUDED_FLAGS: u32 = IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_DISCARDABLE;

type Undecorate = unsafe extern "system" fn(*const c_char, *mut c_char, u32, u32) -> u32;
static UNDECORATOR: OnceLock<Option<Undecorate>> = OnceLock::new();
static DBGHELP: Mutex<()> = Mutex::new(());

fn trim_nul(value: &[u8]) -> &[u8] {
    &value[..value
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(value.len())]
}

fn text(out: &mut Stream, value: &[u8]) {
    for &byte in trim_nul(value) {
        escape_unit(out, byte.into());
    }
}

fn load_undecorator() -> Option<Undecorate> {
    let library =
        unsafe { LoadLibraryExW(w!("dbghelp.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if library.is_null() {
        return None;
    }
    let symbol = unsafe { GetProcAddress(library, c"UnDecorateSymbolName".as_ptr().cast()) }?;
    // The DLL reference is held for the process lifetime, as is the cached address.
    Some(unsafe { transmute::<unsafe extern "system" fn() -> isize, Undecorate>(symbol) })
}

fn undecorate(decorated: &[u8], output: &mut [u8; PRETTY_BYTES]) -> bool {
    let Ok(_guard) = DBGHELP.try_lock() else {
        return false;
    };
    let Some(undecorate) = *UNDECORATOR.get_or_init(load_undecorator) else {
        return false;
    };
    let mut symbol = [0u8; NAME_BYTES + 16];
    let mut text = [0u8; PRETTY_BYTES + 64];
    let name_end = decorated.len() + 4;
    symbol[..5].copy_from_slice(b"??_R0");
    symbol[5..name_end].copy_from_slice(&decorated[1..]);
    symbol[name_end..name_end + 2].copy_from_slice(b"@8");
    let copied = unsafe {
        undecorate(
            symbol.as_ptr().cast(),
            text.as_mut_ptr().cast(),
            text.len() as u32,
            0,
        )
    };
    if copied == 0 {
        return false;
    }
    let name = trim_nul(&text);
    let name = name
        .strip_prefix(b"class ")
        .or_else(|| name.strip_prefix(b"struct "))
        .unwrap_or(name);
    const END: &[u8] = b" `RTTI Type Descriptor'";
    let end = name
        .windows(END.len())
        .position(|window| window == END)
        .unwrap_or(name.len());
    let count = end.min(output.len() - 1);
    output[..count].copy_from_slice(&name[..count]);
    true
}

fn pretty_name(decorated: &[u8]) -> [u8; PRETTY_BYTES] {
    let mut output = [0u8; PRETTY_BYTES];
    let class = decorated.starts_with(b".?AV") || decorated.starts_with(b".?AU");
    if class && !decorated[4..].contains(&b'?') {
        let body = &decorated[4..];
        let end = body
            .windows(2)
            .position(|window| window == b"@@")
            .unwrap_or(body.len());
        let mut used = 0;
        if end != 0 {
            for part in body[..end].rsplit(|&byte| byte == b'@') {
                if used + part.len() + 3 >= output.len() {
                    break;
                }
                if used != 0 {
                    output[used..used + 2].copy_from_slice(b"::");
                    used += 2;
                }
                output[used..used + part.len()].copy_from_slice(part);
                used += part.len();
            }
        }
        if used != 0 {
            return output;
        }
    }
    if class && undecorate(decorated, &mut output) {
        return output;
    }
    let count = decorated.len().min(output.len() - 1);
    output[..count].copy_from_slice(&decorated[..count]);
    output
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let offset = index * 4;
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[derive(Clone, Copy)]
struct CachedName {
    rva: u32,
    pretty: [u8; PRETTY_BYTES],
}

#[derive(Clone, Copy)]
enum Stop {
    Time,
    Classes,
}

struct Scan {
    base: usize,
    size: u32,
    cache: MemoryCache,
    sections: Vec<Section>,
    excluded: [IMAGE_DATA_DIRECTORY; 3],
    names: Vec<CachedName>,
    classes: u32,
    deadline: f64,
    stop: Option<Stop>,
}

impl Scan {
    fn address(&self, rva: u32, size: usize) -> Option<usize> {
        if rva == 0 || rva >= self.size || size > (self.size - rva) as usize {
            return None;
        }
        let address = self.base.checked_add(rva as usize)?;
        address.checked_add(size)?;
        Some(address)
    }

    fn exact<const N: usize>(&mut self, rva: u32) -> Option<[u8; N]> {
        let address = self.address(rva, N)?;
        read_exact(&mut self.cache, address)
    }

    fn decorated(&mut self, rva: u32, output: &mut [u8; NAME_BYTES]) -> Option<usize> {
        let descriptor = self.address(rva, 24)?;
        let wanted = (NAME_BYTES - 1).min((self.size - rva - 16) as usize);
        let available = readable_extent(&mut self.cache, descriptor + 16, wanted);
        if available < 4 {
            return None;
        }
        let (_, copied) = unsafe { read(descriptor + 16, output.as_mut_ptr(), available) };
        output[copied] = 0;
        let length = trim_nul(&output[..copied]).len();
        if (copied == wanted && length == copied) || length < 4 || !output.starts_with(b".?A") {
            return None;
        }
        Some(length)
    }

    fn cached_name(&mut self, rva: u32) -> Option<[u8; PRETTY_BYTES]> {
        let index = (rva as usize >> 3) % CACHE_SLOTS;
        if self.names[index].rva != rva {
            let mut bytes = [0; NAME_BYTES];
            let length = self.decorated(rva, &mut bytes)?;
            self.names[index] = CachedName {
                rva,
                pretty: pretty_name(&bytes[..length]),
            };
        }
        Some(self.names[index].pretty)
    }

    fn target_in_data(&self, rva: u32) -> bool {
        self.sections
            .iter()
            .find(|section| {
                rva.checked_sub(section.rva)
                    .is_some_and(|delta| delta < section.mapped_size())
            })
            .is_some_and(|section| section.flags & EXCLUDED_FLAGS == 0)
    }

    fn class(&mut self, out: &mut Stream, slot: u32, locator: u32) {
        let Some(col) = self.exact::<24>(locator) else {
            return;
        };
        let type_rva = word(&col, 3);
        let hierarchy_rva = word(&col, 4);
        if word(&col, 0) != 1
            || word(&col, 5) != locator
            || self.address(type_rva, 24).is_none()
            || self.address(hierarchy_rva, 16).is_none()
        {
            return;
        }
        let mut decorated = [0; NAME_BYTES];
        let Some(length) = self.decorated(type_rva, &mut decorated) else {
            return;
        };
        let pretty = pretty_name(&decorated[..length]);
        let comma = if self.classes == 0 { "" } else { "," };
        let vtable = slot + 8;
        let offset = word(&col, 1) as i32;
        let cd_offset = word(&col, 2);
        let _ = write!(
            out,
            "{comma}\n    {{\"vtable_rva\": \"{vtable:#x}\", \"locator_rva\": \"{locator:#x}\", \"name\": \""
        );
        text(out, &pretty);
        out.emit("\", \"decorated\": \"");
        text(out, &decorated[..length]);
        let _ = write!(
            out,
            "\", \"offset\": {offset}, \"cd_offset\": {cd_offset}, \"bases\": ["
        );
        let bases = self.bases(out, hierarchy_rva, type_rva);
        let _ = write!(out, "], \"declared_bases\": {bases}}}");
        self.classes += 1;
    }

    fn bases(&mut self, out: &mut Stream, hierarchy: u32, type_rva: u32) -> u32 {
        let Some(hierarchy) = self.exact::<16>(hierarchy).filter(|row| word(row, 0) == 0) else {
            return 0;
        };
        let declared = word(&hierarchy, 2);
        let array = word(&hierarchy, 3);
        let mut comma = "";
        for index in 0..declared.min(BASE_LIMIT) {
            let Some(address) = array.checked_add(index * 4) else {
                break;
            };
            let Some(base) = self.exact::<4>(address).map(u32::from_le_bytes) else {
                break;
            };
            let Some(item) = self.exact::<28>(base) else {
                continue;
            };
            let ty = word(&item, 0);
            if index == 0 && ty == type_rva {
                continue;
            }
            let Some(name) = self.cached_name(ty) else {
                continue;
            };
            let _ = write!(out, "{comma}{{\"name\": \"");
            text(out, &name);
            let _ = write!(
                out,
                "\", \"contained\": {}, \"mdisp\": {}, \"pdisp\": {}, \"vdisp\": {}, \"attributes\": \"{:#x}\"}}",
                word(&item, 1),
                word(&item, 2) as i32,
                word(&item, 3) as i32,
                word(&item, 4) as i32,
                word(&item, 5)
            );
            comma = ", ";
        }
        declared
    }

    fn scan_section(&mut self, out: &mut Stream, rva: u32, size: u32, buffer: &mut [u8]) {
        let Some(mut cursor) = self.base.checked_add(rva as usize) else {
            return;
        };
        let Some(end) = cursor.checked_add(size as usize) else {
            return;
        };
        while cursor < end && self.stop.is_none() {
            let Some(region) = Region::containing(cursor) else {
                break;
            };
            let limit = region.end.min(end);
            if region.readable {
                self.scan_region(out, cursor, limit, buffer);
            }
            cursor = limit;
        }
    }

    fn scan_region(&mut self, out: &mut Stream, mut cursor: usize, end: usize, buffer: &mut [u8]) {
        let mut checks = 0;
        while end - cursor >= 8 {
            if rsf_dump_now_ms() > self.deadline {
                self.stop = Some(Stop::Time);
                return;
            }
            let count = (end - cursor).min(buffer.len()) & !7;
            let (ok, copied) = unsafe { read(cursor, buffer.as_mut_ptr(), count) };
            for (index, bytes) in buffer[..copied].chunks_exact(8).enumerate() {
                let value = u64::from_le_bytes(bytes.try_into().unwrap());
                let relative = value.wrapping_sub(self.base as u64);
                if relative >= u64::from(self.size)
                    || relative & 3 != 0
                    || !self.target_in_data(relative as u32)
                {
                    continue;
                }
                self.class(
                    out,
                    (cursor + index * 8 - self.base) as u32,
                    relative as u32,
                );
                if self.classes >= CLASS_LIMIT {
                    self.stop = Some(Stop::Classes);
                    return;
                }
                checks += 1;
                if checks == 2048 {
                    checks = 0;
                    if rsf_dump_now_ms() > self.deadline {
                        self.stop = Some(Stop::Time);
                        return;
                    }
                }
            }
            if !ok || copied != count {
                break;
            }
            cursor += count;
        }
    }

    fn run(&mut self, out: &mut Stream, buffer: &mut [u8]) {
        for index in 0..self.sections.len() {
            let section = &self.sections[index];
            let rva = section.rva;
            let size = section.mapped_size();
            if size < 8
                || section.flags & EXCLUDED_FLAGS != 0
                || section.flags & IMAGE_SCN_MEM_READ == 0
                || self.address(rva, size as usize).is_none()
            {
                continue;
            }
            if self.excluded.iter().any(|range| {
                range.VirtualAddress != 0
                    && range.Size != 0
                    && u64::from(rva) < u64::from(range.VirtualAddress) + u64::from(range.Size)
                    && u64::from(range.VirtualAddress) < u64::from(rva) + u64::from(size)
            }) {
                continue;
            }
            self.scan_section(out, rva, size, buffer);
            if self.stop.is_some() {
                break;
            }
        }
    }
}

fn prepare(image: &rsf_dump_image_info, deadline: f64) -> Result<Scan, &'static str> {
    let mut sections = Vec::new();
    sections
        .try_reserve_exact(image.sections as usize)
        .map_err(|_| "allocation failed")?;
    sections.extend((0..image.sections).filter_map(|index| sidecar_section(image, index)));
    let mut names = Vec::new();
    names
        .try_reserve_exact(CACHE_SLOTS)
        .map_err(|_| "allocation failed")?;
    names.resize(
        CACHE_SLOTS,
        CachedName {
            rva: 0,
            pretty: [0; PRETTY_BYTES],
        },
    );
    let base = image.base as usize;
    let headers = unsafe { record::<IMAGE_NT_HEADERS64>(base, image.nt_offset as usize) }
        .ok_or("the module headers could not be read for RTTI recovery")?;
    let excluded = [
        IMAGE_DIRECTORY_ENTRY_EXCEPTION,
        IMAGE_DIRECTORY_ENTRY_RESOURCE,
        IMAGE_DIRECTORY_ENTRY_BASERELOC,
    ]
    .map(|index| headers.OptionalHeader.DataDirectory[usize::from(index)]);
    Ok(Scan {
        base,
        size: image.image_size,
        cache: MemoryCache::default(),
        sections,
        excluded,
        names,
        classes: 0,
        deadline,
        stop: None,
    })
}

pub(super) fn write_rtti(out: &mut Stream, image: &rsf_dump_image_info, budget_ms: u32) -> u32 {
    let started = rsf_dump_now_ms();
    let budget = if budget_ms == 0 {
        DEFAULT_BUDGET_MS
    } else {
        budget_ms
    };
    out.emit("  \"rtti\": {\"classes\": [");
    let scan = prepare(image, started + f64::from(budget)).and_then(|mut scan| {
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(CHUNK_BYTES)
            .map_err(|_| "allocation failed")?;
        buffer.resize(CHUNK_BYTES, 0);
        scan.run(out, &mut buffer);
        Ok(scan)
    });
    let (classes, stop) = scan
        .as_ref()
        .map_or((0, None), |scan| (scan.classes, scan.stop));
    let partial = stop.is_some();
    let reason = match stop {
        Some(Stop::Time) => {
            Some("the time budget expired before the scan finished; the list is partial")
        }
        Some(Stop::Classes) => Some("the class bound was reached; the list is partial"),
        None if scan.is_err() => scan.err(),
        None if classes == 0 => Some(
            "no RTTICompleteObjectLocator referenced from a vtable was found; the module has no MSVC x64 RTTI, it was stripped, or the image is not decrypted",
        ),
        None => None,
    };
    let _ = write!(out, "\n  ], \"count\": {classes}, \"partial\": {partial}, ");
    if let Some(reason) = reason {
        let _ = write!(out, "\"reason\": \"{reason}\", ");
    }
    let elapsed = (rsf_dump_now_ms() - started + 0.5) as u32;
    let _ = writeln!(out, "\"budget_ms\": {budget}, \"elapsed_ms\": {elapsed}}},");
    classes
}
