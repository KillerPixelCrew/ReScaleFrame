// SPDX-License-Identifier: MIT
use super::image::dump_image;
use super::measure::{record, rsf_dump_poll_code, rsf_measure_module_code};
use super::sections::{Stream, collect_modules, rsf_dump_now_ms};
use super::sidecar::write_sidecar;
use crate::sys::*;
use std::ffi::{OsString, c_char, c_void};
use std::mem::offset_of;
use std::os::windows::ffi::OsStringExt;
use std::ptr::{self, null_mut};
use windows_sys::Win32::{
    Globalization::{CP_UTF8, MultiByteToWideChar},
    System::{
        Diagnostics::Debug::{
            IMAGE_DIRECTORY_ENTRY_EXCEPTION, IMAGE_DIRECTORY_ENTRY_IMPORT, IMAGE_NT_HEADERS64,
            IMAGE_NT_OPTIONAL_HDR64_MAGIC,
        },
        LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
        SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE},
    },
};

const OK: rsf_dump_result = 0;
const INVALID_ARGUMENT: rsf_dump_result = -1;
const ABI_MISMATCH: rsf_dump_result = -2;
const NOT_A_PE: rsf_dump_result = -3;
const WRITE_FAILED: rsf_dump_result = -4;
const STILL_ENCRYPTED: rsf_dump_result = -5;
const OPTIONS_V1: usize = offset_of!(rsf_dump_options, name_list_path_utf8);
const REPORT_V1: usize = offset_of!(rsf_dump_report, rtti_classes);
const PATH: usize = RSF_DUMP_MODULE_PATH_UNITS as usize;
const OUTPUT_PATH: usize = PATH * 2;

unsafe fn load_options(
    options: *const rsf_dump_options,
) -> Result<rsf_dump_options, rsf_dump_result> {
    if options.is_null() {
        return Err(INVALID_ARGUMENT);
    }
    let length = unsafe { (&raw const (*options).struct_size).read() } as usize;
    if length < OPTIONS_V1 {
        return Err(INVALID_ARGUMENT);
    }
    if unsafe { (&raw const (*options).output_directory_utf8).read() }.is_null() {
        return Err(INVALID_ARGUMENT);
    }
    if unsafe { (&raw const (*options).abi_version).read() } != RSF_MODULE_DUMP_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    let mut result = rsf_dump_options::default();
    // A short legacy record supplies no partially covered pointer or scalar.
    let copy_length = [
        OPTIONS_V1,
        offset_of!(rsf_dump_options, rtti_budget_ms),
        offset_of!(rsf_dump_options, reserved),
        size_of::<rsf_dump_options>(),
    ]
    .into_iter()
    .filter(|&end| end <= length)
    .max()
    .unwrap_or(OPTIONS_V1);
    unsafe {
        ptr::copy_nonoverlapping(options.cast::<u8>(), (&raw mut result).cast(), copy_length)
    };
    Ok(result)
}

unsafe fn copy_report(out: *mut rsf_dump_report, mut report: rsf_dump_report, length: usize) {
    if out.is_null() {
        return;
    }
    report.struct_size = length as u32;
    let copy_length = length.min(size_of::<rsf_dump_report>());
    let copy_length = REPORT_V1 + (copy_length - REPORT_V1) / size_of::<u32>() * size_of::<u32>();
    unsafe { ptr::copy_nonoverlapping((&raw const report).cast::<u8>(), out.cast(), copy_length) };
}

struct ImageHeaders {
    nt_offset: u32,
    sections_offset: u32,
    headers: IMAGE_NT_HEADERS64,
}

fn read_headers(base: usize) -> Option<ImageHeaders> {
    let dos: IMAGE_DOS_HEADER = unsafe { record(base, 0) }?;
    let nt_offset = u32::try_from(dos.e_lfanew).ok()?;
    if dos.e_magic != IMAGE_DOS_SIGNATURE {
        return None;
    }
    let headers: IMAGE_NT_HEADERS64 = unsafe { record(base, nt_offset as usize) }?;
    if headers.Signature != IMAGE_NT_SIGNATURE
        || headers.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC
    {
        return None;
    }
    let sections_offset = nt_offset
        .checked_add(offset_of!(IMAGE_NT_HEADERS64, OptionalHeader) as u32)?
        .checked_add(u32::from(headers.FileHeader.SizeOfOptionalHeader))?;
    Some(ImageHeaders {
        nt_offset,
        sections_offset,
        headers,
    })
}

fn dump_base(module: *const c_void) -> usize {
    if module.is_null() {
        (unsafe { GetModuleHandleW(ptr::null()) }) as usize
    } else {
        module as usize
    }
}

fn module_path(base: usize) -> [u16; PATH] {
    let mut output = [0; PATH];
    let length = unsafe { GetModuleFileNameW(base as _, output.as_mut_ptr(), PATH as u32) };
    if length == 0 || length >= PATH as u32 {
        output.fill(0);
        for (unit, byte) in output.iter_mut().zip(b"<unknown>") {
            *unit = u16::from(*byte);
        }
    }
    output
}

fn convert<const N: usize>(text: *const c_char) -> Option<[u16; N]> {
    let mut output = [0; N];
    let len =
        unsafe { MultiByteToWideChar(CP_UTF8, 0, text.cast(), -1, output.as_mut_ptr(), N as i32) };
    (len > 0).then_some(output)
}

fn unterminated(units: &[u16]) -> &[u16] {
    &units[..units
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(units.len())]
}

fn build_path(dir: &[u16], name: &[u16], suffix: &str) -> Option<[u16; OUTPUT_PATH]> {
    let len = dir
        .len()
        .checked_add(1)?
        .checked_add(name.len())?
        .checked_add(suffix.len())?;
    if len >= OUTPUT_PATH {
        return None;
    }
    let mut result = [0; OUTPUT_PATH];
    for (slot, unit) in result.iter_mut().zip(
        dir.iter()
            .copied()
            .chain([u16::from(b'\\')])
            .chain(name.iter().copied())
            .chain(suffix.encode_utf16()),
    ) {
        *slot = unit;
    }
    Some(result)
}

fn paths(
    options: &rsf_dump_options,
    module: &[u16],
) -> Option<([u16; OUTPUT_PATH], [u16; OUTPUT_PATH])> {
    let directory = convert::<PATH>(options.output_directory_utf8)?;
    let directory = unterminated(&directory);
    let explicit;
    let name = if options.output_name_utf8.is_null() {
        let original = unterminated(module);
        let begin = original
            .iter()
            .rposition(|&unit| unit == u16::from(b'\\'))
            .map_or(0, |i| i + 1);
        &original[begin..]
    } else {
        explicit = convert::<PATH>(options.output_name_utf8)?;
        unterminated(&explicit)
    };
    Some((
        build_path(directory, name, ".dump")?,
        build_path(directory, name, ".dump.json")?,
    ))
}

fn zeroed_vec<T>(length: usize, zero: impl Fn() -> T) -> Vec<T> {
    let mut output = Vec::new();
    if output.try_reserve_exact(length).is_ok() {
        output.resize_with(length, zero);
    }
    output
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dump_module(
    module: *const c_void,
    options: *const rsf_dump_options,
    out: *mut rsf_dump_report,
) -> rsf_dump_result {
    let options = match unsafe { load_options(options) } {
        Ok(options) => options,
        Err(error) => return error,
    };
    let report_size = if out.is_null() {
        0
    } else {
        let size = unsafe { (&raw const (*out).struct_size).read() } as usize;
        if size < REPORT_V1 {
            return INVALID_ARGUMENT;
        }
        size
    };
    let started_ms = rsf_dump_now_ms();
    let base = dump_base(module);
    let Some(image) = read_headers(base) else {
        return NOT_A_PE;
    };
    let header = image.headers.OptionalHeader;
    let mut report = rsf_dump_report {
        load_base: base as u64,
        preferred_base: header.ImageBase,
        ..Default::default()
    };
    if unsafe { rsf_measure_module_code(base as _, &mut report.code_entropy) } != OK {
        return NOT_A_PE;
    }
    if options.require_decrypted != 0 && report.code_entropy >= options.entropy_threshold {
        unsafe { copy_report(out, report, report_size) };
        return STILL_ENCRYPTED;
    }
    let module_path = module_path(base);
    let Some((dump_path, sidecar_path)) = paths(&options, &module_path) else {
        return INVALID_ARGUMENT;
    };
    let imports = header.DataDirectory[usize::from(IMAGE_DIRECTORY_ENTRY_IMPORT)];
    let exception = header.DataDirectory[usize::from(IMAGE_DIRECTORY_ENTRY_EXCEPTION)];
    let info = rsf_dump_image_info {
        base: base as _,
        path: dump_path.as_ptr(),
        image_size: header.SizeOfImage,
        header_size: header.SizeOfHeaders,
        alignment: header.SectionAlignment,
        nt_offset: image.nt_offset,
        sections_offset: image.sections_offset,
        sections: u32::from(image.headers.FileHeader.NumberOfSections),
        import_directory: imports.VirtualAddress,
    };
    let mut digest_storage = zeroed_vec(info.sections as usize, || unsafe {
        std::mem::zeroed::<rsf_section_digest>()
    });
    let digests = if digest_storage.is_empty() {
        null_mut()
    } else {
        digest_storage.as_mut_ptr()
    };
    let Some(image_stats) = (unsafe { dump_image(&info, digests) }) else {
        return WRITE_FAILED;
    };
    report.sections_written = image_stats.sections_written;
    report.iat_references = image_stats.iat_references;
    report.bytes_written = image_stats.bytes_written;

    let mut modules = zeroed_vec(RSF_MAX_MODULES as usize, || unsafe {
        std::mem::zeroed::<rsf_module_entry>()
    });
    let module_count = collect_modules(&mut modules);
    report.modules_listed = module_count as u32;
    let sidecar = rsf_dump_sidecar_info {
        image: &info,
        module_path: module_path.as_ptr(),
        modules: modules.as_ptr(),
        module_count,
        digests,
        name_list: options.name_list_path_utf8,
        preferred_base: report.preferred_base,
        code_entropy: report.code_entropy,
        hash_ms: image_stats.hash_ms,
        started_ms,
        abi_version: RSF_MODULE_DUMP_ABI_VERSION,
        schema_version: RSF_MODULE_DUMP_SCHEMA_VERSION,
        entry_rva: header.AddressOfEntryPoint,
        iat_references: report.iat_references,
        import_bytes: imports.Size,
        exception_rva: exception.VirtualAddress,
        exception_bytes: exception.Size,
        rtti_budget_ms: options.rtti_budget_ms,
    };
    let status =
        if let Ok(mut stream) = Stream::open(&OsString::from_wide(unterminated(&sidecar_path))) {
            let stats = unsafe { write_sidecar(&mut stream, &sidecar) };
            report.imports_described = stats.imports_described;
            report.threads_listed = stats.threads_listed;
            report.rtti_classes = stats.rtti_classes;
            report.function_starts = stats.function_starts;
            if stream.finish() { OK } else { WRITE_FAILED }
        } else {
            WRITE_FAILED
        };
    unsafe { copy_report(out, report, report_size) };
    status
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dump_when_decrypted(
    options: *const rsf_dump_options,
    poll_interval_ms: u32,
    timeout_ms: u32,
    stable_samples: u32,
    report: *mut rsf_dump_report,
) -> rsf_dump_result {
    if options.is_null()
        || (unsafe { (&raw const (*options).struct_size).read() } as usize) < OPTIONS_V1
    {
        return INVALID_ARGUMENT;
    }
    let threshold = unsafe { (&raw const (*options).entropy_threshold).read() };
    let mut entropy = 0.0;
    let result = rsf_dump_poll_code(
        threshold,
        poll_interval_ms,
        timeout_ms,
        stable_samples,
        &mut entropy,
    );
    if result == OK {
        return unsafe { rsf_dump_module(ptr::null(), options, report) };
    }
    if result == STILL_ENCRYPTED
        && !report.is_null()
        && (unsafe { (&raw const (*report).struct_size).read() }) as usize
            >= size_of::<rsf_dump_report>()
    {
        unsafe { (&raw mut (*report).code_entropy).write(entropy) };
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_write_module_list_impl(
    path_utf8: *const c_char,
    label: *const c_char,
    native: &rsf_dump_list_stream,
) -> rsf_dump_result {
    if path_utf8.is_null() {
        return INVALID_ARGUMENT;
    }
    let Some(path) = convert::<OUTPUT_PATH>(path_utf8) else {
        return INVALID_ARGUMENT;
    };
    let stream = unsafe { native.open_append.unwrap()(path.as_ptr()) };
    if stream.is_null() {
        return WRITE_FAILED;
    }
    let mut modules = zeroed_vec(RSF_MAX_MODULES as usize, || unsafe {
        std::mem::zeroed::<rsf_module_entry>()
    });
    if modules.is_empty() {
        unsafe { native.close.unwrap()(stream) };
        return WRITE_FAILED;
    }
    let count = collect_modules(&mut modules);
    let label = if label.is_null() {
        c"sample".as_ptr()
    } else {
        label
    };
    unsafe { native.write_title.unwrap()(stream, label, count) };
    for module in &modules[..count] {
        let path = unterminated(&module.path);
        let begin = path
            .iter()
            .rposition(|&unit| unit == u16::from(b'\\'))
            .map_or(0, |i| i + 1);
        unsafe { native.write_leaf.unwrap()(stream, module.path[begin..].as_ptr()) };
    }
    unsafe { native.close.unwrap()(stream) }
}
