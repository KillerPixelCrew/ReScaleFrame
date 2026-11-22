// SPDX-License-Identifier: MIT
// Guarded RTX 40 temporal program publication, adapted from MFG-Unlocker
// 2b52823e midpoint_fix.cpp (Copyright 2026 Michael Robles, MIT; notice in
// third_party/mfg_unlocker). The six payload profiles, exact source and output
// SHA-256 checks, CUDA LUID/SM89 admission and retained descriptor publication
// are unchanged.
use super::gate::protection_matches;
use super::{Handle, provider, retain_module};
use crate::sync::lock;
use std::ffi::c_void;
use std::mem::forget;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows::Win32::Graphics::Direct3D12::ID3D12Device;
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::{
    Security::Cryptography::{
        BCRYPT_ALG_HANDLE, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider, BCryptHash,
        BCryptOpenAlgorithmProvider,
    },
    System::{
        Diagnostics::Debug::{
            IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC, IMAGE_SCN_MEM_EXECUTE,
            IMAGE_SCN_MEM_READ, IMAGE_SCN_MEM_WRITE, IMAGE_SECTION_HEADER, ReadProcessMemory,
            WriteProcessMemory,
        },
        LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        Memory::{
            MEM_COMMIT, MEM_IMAGE, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION,
            PAGE_EXECUTE_READ, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY, VirtualAlloc,
            VirtualFree, VirtualProtect, VirtualQuery,
        },
        SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE},
        Threading::GetCurrentProcess,
    },
};

const KERNEL_COUNT: usize = 25;
const DESCRIPTOR_BYTES: usize = 48;
const NAME_BYTES: usize = 96;
const MAXIMUM_FATBIN_BYTES: u64 = 64 * 1024 * 1024;
const OUTPUT_CAPACITY: usize = 384 * 1024;
const SCRATCH_CAPACITY: usize = 256 * 1024;
const FATBIN_MAGIC: u32 = 0xba55_ed50;
const JOIN_LABEL: &[u8] = b"$L__BB0_3:";
const MIDPOINT_BITS: &[u8] = b"0f3F000000";
const MUL_PREFIX: &[u8] = b"mul.ftz.f32 ";
const TEMPORAL_MULTIPLY_COUNT: usize = 104;
const TEMPORAL_DIRECTION_COUNT: usize = TEMPORAL_MULTIPLY_COUNT / 2;

struct Profile {
    temporal_slot: usize,
    named_kernels: bool,
    source_fatbin_bytes: u64,
    source_fatbin_sha256: &'static str,
    source_ptx_sha256: &'static str,
    output_fatbin_sha256: &'static str,
    register_declaration: Option<(&'static [u8], &'static [u8])>,
    temporal_input: &'static [u8],
    curr2prev_scale: &'static [u8],
    prev2curr_scale: &'static [u8],
    sm120_entry_bytes: usize,
    sm120_payload_bytes: u64,
    sm120_compressed_bytes: u32,
    sm120_raw_bytes: u64,
    sm89_payload_bytes: u64,
    sm89_compressed_bytes: u32,
    sm89_raw_bytes: u64,
    kernel_names: &'static [&'static str],
}

const V3109_KERNELS: [&str; KERNEL_COUNT] = [
    "BlendCandidatesFused",
    "Copy4Channel",
    "DL1Net_Input",
    "DL1Net_Output",
    "DetectMenus",
    "DetectMenusHudless",
    "Distortion",
    "DownsampleDistortion",
    "DownsampleRGBAndEstimateUIFused",
    "EstimateIntermMvecsScatter",
    "EstimatePrev2CurrScatter",
    "InitMvecQualityMask",
    "InputMvecProcessing",
    "MenuDetectionOutput",
    "OutputPull",
    "OutputPullMiddle",
    "OutputPush",
    "OutputPushFine",
    "Prev2CurrUnpackPull",
    "Prev2CurrPullMiddle",
    "Prev2CurrPush",
    "Prev2CurrPushFine",
    "Scatter3d",
    "WarpBlendingWeights",
    "ZeroBuffer",
];

// DLSS-G 310.9.16 is the 310.9 table with BlendCandidatesFGLite (Preset C)
// inserted at slot 1. The midpoint kernel moves to slot 10 with the same payload.
const V310916_KERNELS: [&str; KERNEL_COUNT + 1] = [
    "BlendCandidatesFused",
    "BlendCandidatesFGLite",
    "Copy4Channel",
    "DL1Net_Input",
    "DL1Net_Output",
    "DetectMenus",
    "DetectMenusHudless",
    "Distortion",
    "DownsampleDistortion",
    "DownsampleRGBAndEstimateUIFused",
    "EstimateIntermMvecsScatter",
    "EstimatePrev2CurrScatter",
    "InitMvecQualityMask",
    "InputMvecProcessing",
    "MenuDetectionOutput",
    "OutputPull",
    "OutputPullMiddle",
    "OutputPush",
    "OutputPushFine",
    "Prev2CurrUnpackPull",
    "Prev2CurrPullMiddle",
    "Prev2CurrPush",
    "Prev2CurrPushFine",
    "Scatter3d",
    "WarpBlendingWeights",
    "ZeroBuffer",
];

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut index = 0;
    while index < a.len() {
        if a[index] != b[index] {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = {
    assert!(same(V310916_KERNELS[0], V3109_KERNELS[0]));
    assert!(same(V310916_KERNELS[1], "BlendCandidatesFGLite"));
    let mut slot = 2;
    while slot < V310916_KERNELS.len() {
        assert!(same(V310916_KERNELS[slot], V3109_KERNELS[slot - 1]));
        slot += 1;
    }
    assert!(same(
        V3109_KERNELS[V3109.temporal_slot],
        "EstimateIntermMvecsScatter"
    ));
    assert!(same(
        V310916_KERNELS[V310916.temporal_slot],
        "EstimateIntermMvecsScatter"
    ));
    assert!(same(V3109_KERNELS[14], "OutputPull") && same(V310916_KERNELS[15], "OutputPull"));
    assert!(EARLY.source_fatbin_bytes as usize <= OUTPUT_CAPACITY && 264_968 <= OUTPUT_CAPACITY);
};

const EARLY: Profile = Profile {
    temporal_slot: 12,
    named_kernels: false,
    source_fatbin_bytes: 197_560,
    source_fatbin_sha256: "25E24B204A58BB5E2757199B1C856B5FDE7D26B9967FFEF82FD794BDE4BCBC67",
    source_ptx_sha256: "946A4CCE7B2E43BD0F6228EBCC99F85B9EF03F64D4A5F6D19097FA502BF912CB",
    output_fatbin_sha256: "9E72F801D8B73F47AA2904BC6A7EFAE203AF7139D168AEBD94724BFB102DC5D9",
    register_declaration: Some((b".reg .f32 %f<4010>;", b".reg .f32 %f<4013>;")),
    temporal_input: b"ld.param.f32 %f4010, [main_kernel_param_5];\r\n\
mov.f32 %f4011, 0f3F800000;\r\n\
sub.ftz.f32 %f4012, %f4011, %f4010;\r\n",
    curr2prev_scale: b"%f4012",
    prev2curr_scale: b"%f4010",
    sm120_entry_bytes: 54_200,
    sm120_payload_bytes: 54_096,
    sm120_compressed_bytes: 54_094,
    sm120_raw_bytes: 188_378,
    sm89_payload_bytes: 60_456,
    sm89_compressed_bytes: 60_450,
    sm89_raw_bytes: 210_951,
    kernel_names: &V3109_KERNELS,
};

const fn legacy(temporal_slot: usize) -> Profile {
    Profile {
        temporal_slot,
        named_kernels: false,
        source_fatbin_bytes: 98_408,
        source_fatbin_sha256: "5A8E0284AAB8AC14FC82B0504BBEEF25D2FCE1D13A1C11D8BDB3F91FEE8145FC",
        source_ptx_sha256: "46C05996A2EF199BBE39378681734ED5EE757655DE70410D9900D85BA91222F1",
        output_fatbin_sha256: "19FB3CD5500BFD1FC88F96C36B381E096D6AB1DFC7B1DB02A04D40A00849104B",
        register_declaration: None,
        temporal_input: b"ld.param.f32 %f134, [main_kernel_param_0+32];\r\n\
mov.f32 %f135, 0f3F800000;\r\n\
sub.ftz.f32 %f136, %f135, %f134;\r\n",
        curr2prev_scale: b"%f136",
        prev2curr_scale: b"%f134",
        sm120_entry_bytes: 28_128,
        sm120_payload_bytes: 28_024,
        sm120_compressed_bytes: 28_017,
        sm120_raw_bytes: 90_490,
        sm89_payload_bytes: 30_384,
        sm89_compressed_bytes: 30_379,
        sm89_raw_bytes: 99_362,
        kernel_names: &V3109_KERNELS,
    }
}

const fn named(temporal_slot: usize, kernel_names: &'static [&'static str]) -> Profile {
    Profile {
        temporal_slot,
        named_kernels: true,
        source_fatbin_bytes: 98_704,
        source_fatbin_sha256: "FBA7599CC9CDC1EED947AD5ADB94436052E8C17AD7FD268C2590AB36A0C731E9",
        source_ptx_sha256: "D1EB7A60C57915238AA228511DA16386F95AAAA5A0D1C1B7903F1DA8FB9519B4",
        output_fatbin_sha256: "579ACF3317ED39B45AC0ABBC70F4DC812A53AE0F8B11E528B7B5542681E8BEF4",
        register_declaration: None,
        temporal_input: b"ld.param.f32 %f134, [Kernel_EstimateIntermMvecsScatter_param_0+32];\r\n\
mov.f32 %f135, 0f3F800000;\r\n\
sub.ftz.f32 %f136, %f135, %f134;\r\n",
        curr2prev_scale: b"%f136",
        prev2curr_scale: b"%f134",
        sm120_entry_bytes: 28_144,
        sm120_payload_bytes: 28_040,
        sm120_compressed_bytes: 28_040,
        sm120_raw_bytes: 90_732,
        sm89_payload_bytes: 30_408,
        sm89_compressed_bytes: 30_402,
        sm89_raw_bytes: 99_626,
        kernel_names,
    }
}

const LEGACY_SLOT11: Profile = legacy(11);
const LEGACY_SLOT10: Profile = legacy(10);
const LEGACY_SLOT9: Profile = legacy(9);
const V3109: Profile = named(9, &V3109_KERNELS);
const V310916: Profile = named(10, &V310916_KERNELS);

// The provider version receipt in mfg_provider.rs selects the profile.
fn profile_for(path: *const u16) -> Option<&'static Profile> {
    match unsafe { provider::provider_profile(path) } {
        1 => Some(&EARLY),
        2 => Some(&LEGACY_SLOT11),
        3 => Some(&LEGACY_SLOT10),
        4 => Some(&LEGACY_SLOT9),
        5 => Some(&V3109),
        6 => Some(&V310916),
        _ => None,
    }
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Failure {
    None = 0,
    AdapterUnavailable = 1,
    AdapterNotAda = 2,
    ProviderVersion = 3,
    ProviderLayout = 4,
    SourceIdentity = 5,
    Decompression = 6,
    TemporalLayout = 7,
    OutputIdentity = 8,
    Allocation = 9,
    Publication = 10,
    RestartRequired = 11,
    ProviderNotReady = 12,
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap_or_default())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap_or_default())
}

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
}

fn sha256_equals(bytes: &[u8], expected: &str) -> bool {
    let Ok(count) = u32::try_from(bytes.len()) else {
        return false;
    };
    if bytes.is_empty() || expected.len() != 64 {
        return false;
    }
    let mut algorithm: BCRYPT_ALG_HANDLE = null_mut();
    let mut digest = [0u8; 32];
    let mut status = unsafe {
        BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, std::ptr::null(), 0)
    };
    if status >= 0 {
        status = unsafe {
            BCryptHash(
                algorithm,
                std::ptr::null(),
                0,
                bytes.as_ptr(),
                count,
                digest.as_mut_ptr(),
                digest.len() as u32,
            )
        };
    }
    if !algorithm.is_null() {
        unsafe { BCryptCloseAlgorithmProvider(algorithm, 0) };
    }
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    status >= 0
        && digest
            .iter()
            .flat_map(|byte| {
                [
                    DIGITS[usize::from(byte >> 4)],
                    DIGITS[usize::from(byte & 15)],
                ]
            })
            .eq(expected.bytes())
}

// Fault-tolerant copy from process memory. A failed read refuses the caller's check.
fn read_guarded(address: usize, out: &mut [u8]) -> bool {
    let mut copied = 0;
    address != 0
        && !out.is_empty()
        && unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                address as *const c_void,
                out.as_mut_ptr().cast(),
                out.len(),
                &mut copied,
            )
        } != 0
        && copied == out.len()
}

fn read_usize(address: usize) -> Option<usize> {
    let mut value = [0u8; 8];
    read_guarded(address, &mut value).then(|| usize::from_le_bytes(value))
}

fn read_record<T: Default>(address: usize) -> Option<T> {
    let mut record = T::default();
    let bytes =
        unsafe { std::slice::from_raw_parts_mut((&raw mut record).cast::<u8>(), size_of::<T>()) };
    read_guarded(address, bytes).then_some(record)
}

// Copies of the read-only sections serve the table scan, so each probe does
// not need its own guarded read. Other image memory is still read live.
struct Image {
    base: usize,
    end: usize,
    sections: Vec<(usize, Vec<u8>)>,
}

impl Image {
    fn read(&self, address: usize, out: &mut [u8]) -> bool {
        for (begin, bytes) in &self.sections {
            let Some(start) = address.checked_sub(*begin) else {
                continue;
            };
            if let Some(source) = start
                .checked_add(out.len())
                .and_then(|end| bytes.get(start..end))
            {
                out.copy_from_slice(source);
                return true;
            }
        }
        read_guarded(address, out)
    }

    fn pointer(&self, address: usize) -> Option<usize> {
        let mut value = [0u8; 8];
        self.read(address, &mut value)
            .then(|| usize::from_le_bytes(value))
    }

    fn name(&self, address: usize) -> Option<[u8; NAME_BYTES]> {
        let mut name = [0u8; NAME_BYTES];
        self.read(address, &mut name[..NAME_BYTES - 1])
            .then_some(name)
    }
}

fn c_name(bytes: &[u8]) -> &[u8] {
    bytes.split(|&byte| byte == 0).next().unwrap_or_default()
}

fn kernel_identity(profile: &Profile, slot: usize, descriptor: &[u8], entry: &[u8]) -> bool {
    let Some(expected) = profile.kernel_names.get(slot) else {
        return false;
    };
    let (descriptor, entry) = (c_name(descriptor), c_name(entry));
    if !profile.named_kernels {
        return descriptor == b"dlfg_kernel" && entry == b"main_kernel";
    }
    descriptor == expected.as_bytes()
        && entry
            .strip_prefix(b"Kernel_")
            .is_some_and(|name| name == expected.as_bytes())
}

fn descriptor_matches(image: &Image, descriptor: usize, slot: usize, profile: &Profile) -> bool {
    let (base, end) = (image.base, image.end);
    if end - base < DESCRIPTOR_BYTES || descriptor < base || descriptor > end - DESCRIPTOR_BYTES {
        return false;
    }
    let mut bytes = [0u8; DESCRIPTOR_BYTES];
    if !image.read(descriptor, &mut bytes) {
        return false;
    }
    let name = u64_at(&bytes, 0) as usize;
    let fatbin = u64_at(&bytes, 8) as usize;
    let supplied = u32_at(&bytes, 0x10);
    let entry = u64_at(&bytes, 0x18) as usize;
    let in_range = |address: usize, size: usize| address >= base && address <= end - size;
    if name == 0
        || entry == 0
        || fatbin == 0
        || end - base < NAME_BYTES
        || !in_range(name, NAME_BYTES)
        || !in_range(entry, NAME_BYTES)
        || !in_range(fatbin, 16)
    {
        return false;
    }
    let (Some(descriptor_name), Some(entry_name)) = (image.name(name), image.name(entry)) else {
        return false;
    };
    if !kernel_identity(profile, slot, &descriptor_name, &entry_name) {
        return false;
    }
    let mut header = [0u8; 16];
    if !image.read(fatbin, &mut header) || u32_at(&header, 0) != FATBIN_MAGIC {
        return false;
    }
    let header_size = u64::from(u16_at(&header, 6));
    let Some(total) = header_size.checked_add(u64_at(&header, 8)) else {
        return false;
    };
    if header_size < 16
        || total > MAXIMUM_FATBIN_BYTES
        || total > (end - fatbin) as u64
        || (supplied != 0 && u64::from(supplied) != total)
    {
        return false;
    }
    if slot != profile.temporal_slot {
        return true;
    }
    let mut payload = vec![0u8; total as usize];
    total == profile.source_fatbin_bytes
        && image.read(fatbin, &mut payload)
        && sha256_equals(&payload, profile.source_fatbin_sha256)
}

fn table_matches(image: &Image, table: usize, profile: &Profile) -> bool {
    let table_bytes = profile.kernel_names.len() * size_of::<usize>();
    if table < image.base || image.end - image.base < table_bytes || table > image.end - table_bytes
    {
        return false;
    }
    (0..profile.kernel_names.len()).all(|slot| {
        image
            .pointer(table + slot * size_of::<usize>())
            .is_some_and(|descriptor| descriptor_matches(image, descriptor, slot, profile))
    })
}

fn image_headers(base: usize) -> Option<(IMAGE_DOS_HEADER, IMAGE_NT_HEADERS64)> {
    let dos: IMAGE_DOS_HEADER = read_record(base)?;
    if dos.e_magic != IMAGE_DOS_SIGNATURE || dos.e_lfanew <= 0 {
        return None;
    }
    let nt: IMAGE_NT_HEADERS64 = read_record(base.checked_add(dos.e_lfanew as usize)?)?;
    (nt.Signature == IMAGE_NT_SIGNATURE && nt.OptionalHeader.Magic == IMAGE_NT_OPTIONAL_HDR64_MAGIC)
        .then_some((dos, nt))
}

fn image_size(base: usize) -> Option<usize> {
    let (_, nt) = image_headers(base)?;
    let size = nt.OptionalHeader.SizeOfImage as usize;
    (size >= size_of::<IMAGE_NT_HEADERS64>()).then_some(size)
}

// Returns the address of the unique registration-table entry for the temporal kernel.
fn find_descriptor_entry(base: usize, size: usize, profile: &Profile) -> Option<usize> {
    let table_bytes = profile.kernel_names.len() * size_of::<usize>();
    if size < table_bytes {
        return None;
    }
    let end = base.checked_add(size)?;
    let (dos, nt) = image_headers(base)?;
    let section_offset = dos.e_lfanew as usize
        + std::mem::offset_of!(IMAGE_NT_HEADERS64, OptionalHeader)
        + usize::from(nt.FileHeader.SizeOfOptionalHeader);
    if section_offset > size
        || usize::from(nt.FileHeader.NumberOfSections)
            > (size - section_offset) / size_of::<IMAGE_SECTION_HEADER>()
    {
        return None;
    }
    let mut ranges = Vec::new();
    for index in 0..usize::from(nt.FileHeader.NumberOfSections) {
        let section: IMAGE_SECTION_HEADER =
            read_record(base + section_offset + index * size_of::<IMAGE_SECTION_HEADER>())?;
        let rva = section.VirtualAddress as usize;
        if section.Characteristics & IMAGE_SCN_MEM_READ == 0
            || section.Characteristics & (IMAGE_SCN_MEM_WRITE | IMAGE_SCN_MEM_EXECUTE) != 0
            || rva >= size
        {
            continue;
        }
        let requested = unsafe { section.Misc.VirtualSize }.max(section.SizeOfRawData) as usize;
        let length = requested.min(size - rva);
        if length >= table_bytes {
            ranges.push((base + rva, length));
        }
    }
    let mut image = Image {
        base,
        end,
        sections: Vec::new(),
    };
    for &(begin, length) in &ranges {
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(length).is_err() {
            continue;
        }
        bytes.extend(std::iter::repeat_n(0, length));
        if read_guarded(begin, &mut bytes) {
            image.sections.push((begin, bytes));
        }
    }
    let slot = profile.temporal_slot * size_of::<usize>();
    let mut found = None;
    for (begin, length) in ranges {
        let mut offset = 0;
        while offset + table_bytes <= length {
            let table = begin + offset;
            offset += align_of::<usize>();
            if !image.pointer(table + slot).is_some_and(|descriptor| {
                descriptor_matches(&image, descriptor, profile.temporal_slot, profile)
            }) || !table_matches(&image, table, profile)
            {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(table + slot);
        }
    }
    found
}

fn decompress(input: &[u8], output: &mut [u8]) -> bool {
    fn extend(input: &[u8], offset: &mut usize, value: &mut usize) -> bool {
        loop {
            let Some(&extension) = input.get(*offset) else {
                return false;
            };
            *offset += 1;
            let Some(sum) = value.checked_add(usize::from(extension)) else {
                return false;
            };
            *value = sum;
            if extension != 0xff {
                return true;
            }
        }
    }
    if input.is_empty() || output.is_empty() {
        return false;
    }
    let (mut read, mut written) = (0, 0);
    while read < input.len() {
        let token = input[read];
        read += 1;
        let mut literal = usize::from(token >> 4);
        if literal == 15 && !extend(input, &mut read, &mut literal) {
            return false;
        }
        if literal > input.len() - read || literal > output.len() - written {
            return false;
        }
        output[written..written + literal].copy_from_slice(&input[read..read + literal]);
        read += literal;
        written += literal;
        if read == input.len() {
            break;
        }
        if input.len() - read < 2 {
            return false;
        }
        let back = usize::from(input[read]) | usize::from(input[read + 1]) << 8;
        read += 2;
        if back == 0 || back > written {
            return false;
        }
        let mut matched = 4 + usize::from(token & 15);
        if token & 15 == 15 && !extend(input, &mut read, &mut matched) {
            return false;
        }
        if matched > output.len() - written {
            return false;
        }
        for index in written..written + matched {
            output[index] = output[index - back];
        }
        written += matched;
    }
    read == input.len() && written == output.len()
}

fn find_unique(bytes: &[u8], marker: &[u8]) -> Option<usize> {
    if marker.is_empty() || marker.len() > bytes.len() {
        return None;
    }
    let mut found = None;
    for (offset, window) in bytes.windows(marker.len()).enumerate() {
        if window == marker {
            if found.is_some() {
                return None;
            }
            found = Some(offset);
        }
    }
    found
}

// Rewrites the cloned fatbin in place: the SM89 PTX gains the temporal input
// and its 104 midpoint multiplies take the two directional scales.
fn build_temporal_fatbin(
    fatbin: &mut [u8],
    scratch: &mut [u8],
    profile: &Profile,
) -> Result<u32, Failure> {
    const OUTER_HEADER: usize = 16;
    const SM89_HEADER: usize = 104;
    const COMPRESSED_FLAGS: u64 = 0x2041;
    const UNCOMPRESSED_FLAGS: u64 = 0x41;
    let layout = Err(Failure::TemporalLayout);
    if fatbin.len() < OUTPUT_CAPACITY
        || u32_at(fatbin, 0) != FATBIN_MAGIC
        || usize::from(u16_at(fatbin, 6)) != OUTER_HEADER
        || u64_at(fatbin, 8).wrapping_add(OUTER_HEADER as u64) != profile.source_fatbin_bytes
    {
        return layout;
    }
    let sm120 = OUTER_HEADER;
    let sm89 = sm120 + profile.sm120_entry_bytes;
    if u16_at(fatbin, sm120) != 1
        || u32_at(fatbin, sm120 + 4) != 104
        || u64_at(fatbin, sm120 + 8) != profile.sm120_payload_bytes
        || u32_at(fatbin, sm120 + 16) != profile.sm120_compressed_bytes
        || u32_at(fatbin, sm120 + 28) != 120
        || u64_at(fatbin, sm120 + 40) != COMPRESSED_FLAGS
        || u64_at(fatbin, sm120 + 56) != profile.sm120_raw_bytes
        || u16_at(fatbin, sm89) != 1
        || u32_at(fatbin, sm89 + 4) != SM89_HEADER as u32
        || u64_at(fatbin, sm89 + 8) != profile.sm89_payload_bytes
        || u32_at(fatbin, sm89 + 16) != profile.sm89_compressed_bytes
        || u32_at(fatbin, sm89 + 28) != 89
        || u64_at(fatbin, sm89 + 40) != COMPRESSED_FLAGS
        || u64_at(fatbin, sm89 + 56) != profile.sm89_raw_bytes
    {
        return layout;
    }
    let raw = profile.sm89_raw_bytes as usize;
    let compressed =
        sm89 + SM89_HEADER..sm89 + SM89_HEADER + profile.sm89_compressed_bytes as usize;
    let (Some(input), Some(ptx)) = (fatbin.get(compressed), scratch.get_mut(..raw)) else {
        return layout;
    };
    if !decompress(input, ptx) {
        return Err(Failure::Decompression);
    }
    if !sha256_equals(ptx, profile.source_ptx_sha256) {
        return Err(Failure::SourceIdentity);
    }
    if let Some((source, replacement)) = profile.register_declaration {
        if source.is_empty() || source.len() != replacement.len() {
            return layout;
        }
        let Some(declaration) = find_unique(ptx, source) else {
            return layout;
        };
        put(ptx, declaration, replacement);
    }
    let Some(label) = find_unique(ptx, JOIN_LABEL) else {
        return layout;
    };
    let Some(newline) = ptx[label + JOIN_LABEL.len()..]
        .iter()
        .position(|&byte| byte == b'\n')
    else {
        return layout;
    };
    let insertion = label + JOIN_LABEL.len() + newline + 1;
    let mut markers = [0usize; TEMPORAL_MULTIPLY_COUNT];
    let mut count = 0;
    for offset in 0..raw.saturating_sub(MIDPOINT_BITS.len()) {
        if &ptx[offset..offset + MIDPOINT_BITS.len()] != MIDPOINT_BITS
            || ptx[offset + MIDPOINT_BITS.len()] != b';'
        {
            continue;
        }
        let line = ptx[..offset]
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(0, |newline| newline + 1);
        if offset - line < MUL_PREFIX.len()
            || !ptx[line..].starts_with(MUL_PREFIX)
            || count >= markers.len()
        {
            continue;
        }
        markers[count] = offset;
        count += 1;
    }
    if count != markers.len() || markers[0] <= insertion {
        return layout;
    }
    let prefix = sm89 + SM89_HEADER;
    let mut written = 0;
    let mut append = |fatbin: &mut [u8], source: &[u8]| {
        if prefix > OUTPUT_CAPACITY || source.len() > OUTPUT_CAPACITY - prefix - written {
            return false;
        }
        put(fatbin, prefix + written, source);
        written += source.len();
        true
    };
    if !append(fatbin, &ptx[..insertion]) || !append(fatbin, profile.temporal_input) {
        return layout;
    }
    let mut source = insertion;
    for (index, &marker) in markers.iter().enumerate() {
        let scale = if index < TEMPORAL_DIRECTION_COUNT {
            profile.curr2prev_scale
        } else {
            profile.prev2curr_scale
        };
        if marker < source || !append(fatbin, &ptx[source..marker]) || !append(fatbin, scale) {
            return layout;
        }
        source = marker + MIDPOINT_BITS.len();
    }
    if !append(fatbin, &ptx[source..]) {
        return layout;
    }
    let padded = (written + 7) & !7;
    let total = prefix + padded;
    let Ok(output) = u32::try_from(total) else {
        return layout;
    };
    if total > OUTPUT_CAPACITY {
        return layout;
    }
    fatbin[prefix + written..total].fill(0);
    put(fatbin, sm89 + 8, &(padded as u64).to_le_bytes());
    put(fatbin, sm89 + 16, &0u32.to_le_bytes());
    put(fatbin, sm89 + 40, &UNCOMPRESSED_FLAGS.to_le_bytes());
    put(fatbin, sm89 + 56, &0u64.to_le_bytes());
    put(fatbin, 8, &((total - OUTER_HEADER) as u64).to_le_bytes());
    Ok(output)
}

fn read_only_image_address(module: usize, address: usize) -> bool {
    let mut memory = MEMORY_BASIC_INFORMATION::default();
    if module == 0
        || address == 0
        || unsafe {
            VirtualQuery(
                address as *const c_void,
                &mut memory,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        } != size_of::<MEMORY_BASIC_INFORMATION>()
        || memory.State != MEM_COMMIT
        || memory.Type != MEM_IMAGE
        || memory.AllocationBase as usize != module
    {
        return false;
    }
    matches!(memory.Protect & 0xff, PAGE_READONLY | PAGE_EXECUTE_READ)
}

fn restore_protection(address: usize, protection: u32) -> bool {
    let set = || {
        let mut ignored = 0;
        unsafe { VirtualProtect(address as *const c_void, 8, protection, &mut ignored) };
        protection_matches(address, protection, 8)
    };
    set() || set()
}

fn set_writable(address: usize) -> bool {
    let mut ignored = 0;
    unsafe { VirtualProtect(address as *const c_void, 8, PAGE_READWRITE, &mut ignored) };
    let mut memory = MEMORY_BASIC_INFORMATION::default();
    let queried = unsafe {
        VirtualQuery(
            address as *const c_void,
            &mut memory,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    queried == size_of::<MEMORY_BASIC_INFORMATION>()
        && matches!(memory.Protect & 0xff, PAGE_READWRITE | PAGE_WRITECOPY)
}

struct Published {
    success: bool,
    visible: bool,
}

// Swaps one aligned pointer in a read-only image page and restores its protection.
fn publish_pointer(address: usize, expected: usize, replacement: usize) -> Published {
    let refused = Published {
        success: false,
        visible: false,
    };
    if address == 0 || expected == 0 || replacement == 0 || address & 7 != 0 {
        return refused;
    }
    let mut memory = MEMORY_BASIC_INFORMATION::default();
    if unsafe {
        VirtualQuery(
            address as *const c_void,
            &mut memory,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    } != size_of::<MEMORY_BASIC_INFORMATION>()
        || memory.State != MEM_COMMIT
    {
        return refused;
    }
    let old = memory.Protect;
    if !set_writable(address) || read_usize(address) != Some(expected) {
        restore_protection(address, old);
        return refused;
    }
    // Interlocked writes do not fault a shared WRITECOPY image page into a
    // private page on every Windows build. A same-value ordinary write does;
    // the subsequent pointer change is still a single compare-exchange.
    let mut written = 0;
    if unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            (&raw const expected).cast(),
            size_of::<usize>(),
            &mut written,
        )
    } == 0
        || written != size_of::<usize>()
    {
        restore_protection(address, old);
        return refused;
    }
    let slot = unsafe { AtomicPtr::<c_void>::from_ptr(address as *mut *mut c_void) };
    let swap = |from: usize, to: usize| {
        slot.compare_exchange(
            from as *mut c_void,
            to as *mut c_void,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
    };
    if let Err(observed) = swap(expected, replacement) {
        restore_protection(address, old);
        return Published {
            success: false,
            visible: observed as usize == replacement,
        };
    }
    if restore_protection(address, old) && read_usize(address) == Some(replacement) {
        return Published {
            success: true,
            visible: true,
        };
    }
    // The replacement was visible. Roll it back only after writable access is
    // positively re-established; its allocation must be retained either way.
    if set_writable(address) {
        let _ = swap(replacement, expected);
    }
    restore_protection(address, old);
    Published {
        success: false,
        visible: true,
    }
}

struct Publication {
    module: usize,
    luid: u64,
    entry: usize,
    replacement: usize,
}

static VERIFIED: AtomicBool = AtomicBool::new(false);
static FAILURE: AtomicU32 = AtomicU32::new(Failure::AdapterUnavailable as u32);
static LUID: AtomicU64 = AtomicU64::new(0);
// The published clone, its allocation and the provider reference are
// process-lifetime: a reader may retain the descriptor at any time.
static PUBLICATION: Mutex<Option<Publication>> = Mutex::new(None);

fn publication() -> MutexGuard<'static, Option<Publication>> {
    lock(&PUBLICATION)
}

fn set_failure(failure: Failure) {
    FAILURE.store(failure as u32, Ordering::Release);
}

pub(super) fn failure_code() -> u32 {
    FAILURE.load(Ordering::Acquire)
}

// Returns the compute capability of the unique CUDA device with this LUID,
// and whether the complete enumeration identified it as SM 8.9.
fn verify_ada(luid: u64) -> (bool, i32, i32) {
    type Init = unsafe extern "system" fn(u32) -> i32;
    type Count = unsafe extern "system" fn(*mut i32) -> i32;
    type Device = unsafe extern "system" fn(*mut i32, i32) -> i32;
    type Capability = unsafe extern "system" fn(*mut i32, *mut i32, i32) -> i32;
    type Luid = unsafe extern "system" fn(*mut u8, *mut u32, i32) -> i32;
    let module = unsafe {
        LoadLibraryExW(
            windows::core::w!("nvcuda.dll").as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    let Some(cuda) = crate::sdk_module::SdkModule::adopt_system(module) else {
        return (false, 0, 0);
    };
    let symbol = |name| cuda.symbol(name).map(|address| address as *const c_void);
    let (Some(init), Some(count), Some(device), Some(capability), Some(get_luid)) = (
        symbol(c"cuInit"),
        symbol(c"cuDeviceGetCount"),
        symbol(c"cuDeviceGet"),
        symbol(c"cuDeviceComputeCapability"),
        symbol(c"cuDeviceGetLuid"),
    ) else {
        return (false, 0, 0);
    };
    let (init, count, device, capability, get_luid) = unsafe {
        (
            std::mem::transmute::<*const c_void, Init>(init),
            std::mem::transmute::<*const c_void, Count>(count),
            std::mem::transmute::<*const c_void, Device>(device),
            std::mem::transmute::<*const c_void, Capability>(capability),
            std::mem::transmute::<*const c_void, Luid>(get_luid),
        )
    };
    let mut devices = 0;
    let mut complete = unsafe { init(0) == 0 && count(&mut devices) == 0 } && devices > 0;
    let (mut matches, mut major, mut minor) = (0u32, 0, 0);
    for ordinal in 0..devices {
        if !complete {
            break;
        }
        let (mut handle, mut candidate_major, mut candidate_minor) = (0, 0, 0);
        let mut raw = [0u8; 8];
        let mut node_mask = 0;
        if unsafe {
            device(&mut handle, ordinal) != 0
                || capability(&mut candidate_major, &mut candidate_minor, handle) != 0
                || get_luid(raw.as_mut_ptr(), &mut node_mask, handle) != 0
        } {
            complete = false;
            break;
        }
        // LUID is { DWORD LowPart; LONG HighPart; }, packed low word first.
        if u64::from_le_bytes(raw) == luid {
            matches += 1;
            major = candidate_major;
            minor = candidate_minor;
        }
    }
    (
        complete && matches == 1 && major == 8 && minor == 9,
        major,
        minor,
    )
}

fn reject_adapter() -> bool {
    let _publication = publication();
    VERIFIED.store(false, Ordering::Release);
    set_failure(Failure::AdapterUnavailable);
    false
}

// The device is borrowed for this call.
pub(super) unsafe fn observe_d3d12_device(device: Handle) -> bool {
    let Some(unknown) = (unsafe { IUnknown::from_raw_borrowed(&device) }) else {
        return reject_adapter();
    };
    let Ok(d3d12) = unknown.cast::<ID3D12Device>() else {
        return reject_adapter();
    };
    let luid = unsafe { d3d12.GetAdapterLuid() };
    drop(d3d12);
    let packed = u64::from(luid.LowPart) | u64::from(luid.HighPart as u32) << 32;
    let (verified, major, minor) = verify_ada(packed);
    let publication = publication();
    if publication
        .as_ref()
        .is_some_and(|published| published.luid != packed)
    {
        VERIFIED.store(false, Ordering::Release);
        set_failure(Failure::RestartRequired);
        return false;
    }
    LUID.store(packed, Ordering::Release);
    VERIFIED.store(verified, Ordering::Release);
    if !verified {
        set_failure(if major == 0 && minor == 0 {
            Failure::AdapterUnavailable
        } else {
            Failure::AdapterNotAda
        });
    }
    verified
}

// The caller retains the provider and lends its NUL-terminated module path.
pub(super) fn patch_provider(module: Handle, supplied_path: *const u16) -> bool {
    if module.is_null() || !VERIFIED.load(Ordering::Acquire) {
        return false;
    }
    let mut publication = publication();
    let luid = LUID.load(Ordering::Acquire);
    if !VERIFIED.load(Ordering::Acquire) || luid == 0 {
        return false;
    }
    if let Some(published) = publication.as_ref() {
        let current = published.module == module as usize
            && published.luid == luid
            && read_usize(published.entry) == Some(published.replacement)
            && read_only_image_address(module as usize, published.entry);
        if !current {
            set_failure(Failure::RestartRequired);
        }
        return current;
    }
    let Some(pinned) = retain_module(module) else {
        set_failure(Failure::ProviderLayout);
        return false;
    };
    let mut loaded = [0u16; 32768];
    let path = if provider::module_path(pinned.as_ptr(), &mut loaded).is_some() {
        loaded.as_ptr()
    } else {
        supplied_path
    };
    // Dropping the pinned reference releases a refused attempt.
    let fail = |failure: Failure| {
        set_failure(failure);
        false
    };
    if unsafe { provider::supported_provider(pinned.as_ptr(), path) } == 0 {
        return fail(Failure::ProviderVersion);
    }
    let Some(profile) = profile_for(path) else {
        return fail(Failure::ProviderVersion);
    };
    let base = module as usize;
    let Some((size, end)) = image_size(base).and_then(|size| Some((size, base.checked_add(size)?)))
    else {
        return fail(Failure::ProviderLayout);
    };
    let Some(entry) = find_descriptor_entry(base, size, profile)
        .filter(|&entry| read_only_image_address(base, entry))
    else {
        return fail(Failure::ProviderLayout);
    };
    let mut descriptor = [0u8; DESCRIPTOR_BYTES];
    let Some(original) = read_usize(entry).filter(|&original| {
        original >= base
            && original <= end - DESCRIPTOR_BYTES
            && read_guarded(original, &mut descriptor)
    }) else {
        return fail(Failure::ProviderLayout);
    };
    let original_fatbin = u64_at(&descriptor, 8) as usize;
    let supplied = u32_at(&descriptor, 0x10);
    if supplied == 0 {
        return fail(Failure::ProviderNotReady);
    }
    let source_bytes = profile.source_fatbin_bytes as usize;
    if source_bytes > size
        || u64::from(supplied) != profile.source_fatbin_bytes
        || original_fatbin < base
        || original_fatbin > end - source_bytes
    {
        return fail(Failure::SourceIdentity);
    }
    let allocation_size = DESCRIPTOR_BYTES + OUTPUT_CAPACITY + SCRATCH_CAPACITY;
    let allocation = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            allocation_size,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    }
    .cast::<u8>();
    if allocation.is_null() {
        return fail(Failure::Allocation);
    }
    let free = |failure: Failure| {
        unsafe { VirtualFree(allocation.cast(), 0, MEM_RELEASE) };
        fail(failure)
    };
    let memory = unsafe { std::slice::from_raw_parts_mut(allocation, allocation_size) };
    let (cloned_descriptor, rest) = memory.split_at_mut(DESCRIPTOR_BYTES);
    let (cloned_fatbin, scratch) = rest.split_at_mut(OUTPUT_CAPACITY);
    cloned_descriptor.copy_from_slice(&descriptor);
    if !read_guarded(original_fatbin, &mut cloned_fatbin[..source_bytes])
        || !sha256_equals(&cloned_fatbin[..source_bytes], profile.source_fatbin_sha256)
    {
        return free(Failure::SourceIdentity);
    }
    let output = match build_temporal_fatbin(cloned_fatbin, scratch, profile) {
        Ok(output) => output,
        Err(failure) => return free(failure),
    };
    if !sha256_equals(
        &cloned_fatbin[..output as usize],
        profile.output_fatbin_sha256,
    ) {
        return free(Failure::OutputIdentity);
    }
    let cloned_fatbin_address = cloned_fatbin.as_ptr() as usize;
    let replacement = cloned_descriptor.as_ptr() as usize;
    put(cloned_descriptor, 8, &cloned_fatbin_address.to_le_bytes());
    put(cloned_descriptor, 0x10, &output.to_le_bytes());
    let mut previous = 0;
    if unsafe {
        VirtualProtect(
            allocation.cast(),
            allocation_size,
            PAGE_READONLY,
            &mut previous,
        )
    } == 0
    {
        return free(Failure::Publication);
    }
    if !VERIFIED.load(Ordering::Acquire) || LUID.load(Ordering::Acquire) != luid {
        return free(Failure::RestartRequired);
    }
    let published = publish_pointer(entry, original, replacement);
    if !published.success {
        if published.visible {
            // A concurrent reader may retain the briefly visible clone.
            forget(pinned);
        } else {
            unsafe { VirtualFree(allocation.cast(), 0, MEM_RELEASE) };
        }
        return fail(Failure::Publication);
    }
    forget(pinned);
    *publication = Some(Publication {
        module: base,
        luid,
        entry,
        replacement,
    });
    set_failure(Failure::None);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompress_expands_literals_and_overlapping_matches() {
        // Literal "ab", then a 6-byte match at distance 2.
        let input = [0x22, b'a', b'b', 0x02, 0x00];
        let mut output = [0u8; 8];
        assert!(decompress(&input, &mut output));
        assert_eq!(&output, b"abababab");
        let mut short = [0u8; 7];
        assert!(!decompress(&input, &mut short));
        assert!(!decompress(&[0x10, b'a', 0x00, 0x00], &mut [0u8; 5]));
    }

    #[test]
    fn unique_marker_refuses_repeats() {
        assert_eq!(find_unique(b"xx$L__BB0_3:yy", JOIN_LABEL), Some(2));
        assert_eq!(find_unique(b"$L__BB0_3:$L__BB0_3:", JOIN_LABEL), None);
    }

    #[test]
    fn sha256_matches_uppercase_digest() {
        assert!(sha256_equals(
            b"abc",
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        ));
        assert!(!sha256_equals(
            b"abd",
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        ));
    }

    // Set RSF_DLSSG_PROVIDER to an eligible nvngx_dlssg.dll. The image is mapped
    // without running its code, and nothing is published.
    #[test]
    #[ignore = "needs a local DLSS-G provider"]
    fn provider_transform_reproduces_pinned_output() {
        use windows_sys::Win32::System::LibraryLoader::DONT_RESOLVE_DLL_REFERENCES;
        let path: Vec<u16> = std::env::var("RSF_DLSSG_PROVIDER")
            .expect("RSF_DLSSG_PROVIDER")
            .encode_utf16()
            .chain([0])
            .collect();
        let module = crate::sdk_module::SdkModule::adopt_system(unsafe {
            LoadLibraryExW(path.as_ptr(), null_mut(), DONT_RESOLVE_DLL_REFERENCES)
        })
        .expect("map provider");
        let base = module.as_ptr() as usize;
        assert_ne!(
            unsafe { provider::supported_provider(module.as_ptr(), path.as_ptr()) },
            0
        );
        let profile = profile_for(path.as_ptr()).expect("pinned profile");
        let size = image_size(base).expect("image size");
        let entry = find_descriptor_entry(base, size, profile).expect("unique table");
        assert!(read_only_image_address(base, entry));
        let original = read_usize(entry).expect("descriptor");
        let mut descriptor = [0u8; DESCRIPTOR_BYTES];
        assert!(read_guarded(original, &mut descriptor));
        // The provider records the size during its own initialization, which
        // this mapping skips. Publication refuses that state as not ready.
        let supplied = u64::from(u32_at(&descriptor, 0x10));
        assert!(supplied == 0 || supplied == profile.source_fatbin_bytes);
        let source = profile.source_fatbin_bytes as usize;
        let mut fatbin = vec![0u8; OUTPUT_CAPACITY];
        let mut scratch = vec![0u8; SCRATCH_CAPACITY];
        assert!(read_guarded(
            u64_at(&descriptor, 8) as usize,
            &mut fatbin[..source]
        ));
        assert!(sha256_equals(
            &fatbin[..source],
            profile.source_fatbin_sha256
        ));
        let output = build_temporal_fatbin(&mut fatbin, &mut scratch, profile).expect("transform");
        assert!(sha256_equals(
            &fatbin[..output as usize],
            profile.output_fatbin_sha256
        ));
    }

    #[test]
    fn kernel_identity_follows_profile_naming() {
        assert!(kernel_identity(
            &LEGACY_SLOT9,
            9,
            b"dlfg_kernel\0",
            b"main_kernel\0x"
        ));
        assert!(kernel_identity(
            &V310916,
            10,
            b"EstimateIntermMvecsScatter\0",
            b"Kernel_EstimateIntermMvecsScatter\0"
        ));
        assert!(!kernel_identity(
            &V3109,
            10,
            b"EstimateIntermMvecsScatter\0",
            b"Kernel_EstimateIntermMvecsScatter\0"
        ));
        assert!(!kernel_identity(
            &V3109,
            25,
            b"ZeroBuffer\0",
            b"Kernel_ZeroBuffer\0"
        ));
    }
}
