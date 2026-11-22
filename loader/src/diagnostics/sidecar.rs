// SPDX-License-Identifier: MIT
use super::image::{Section, rsf_dump_imports_impl, sidecar_section};
use super::names::{escape_unit, rsf_dump_names_impl};
use super::rtti::write_rtti;
use super::sections::{
    Stream, Write, rsf_dump_functions_impl, rsf_dump_now_ms, rsf_dump_threads_impl,
};
use super::sections_abi::{rsf_dump_sidecar_info, rsf_dump_sidecar_stats};
use std::ffi::{CStr, c_void};
use std::fmt::Write as _;
use std::slice;

fn fixed(out: &mut Stream, number: f64) {
    if number < 0.0 {
        out.emit("-");
    }
    let scaled = (number.abs() * 10000.0 + 0.5) as u64;
    let _ = write!(out, "{}.{:04}", scaled / 10000, scaled % 10000);
}

fn wide(out: &mut Stream, value: &[u16]) {
    for &unit in value.iter().take_while(|&&unit| unit != 0) {
        escape_unit(out, unit);
    }
}

unsafe fn path(out: &mut Stream, source: *const u16) {
    let length = (0..260)
        .find(|&i| unsafe { source.add(i).read() } == 0)
        .unwrap_or(260);
    wide(out, unsafe { slice::from_raw_parts(source, length) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dump_sidecar_impl(
    context: *mut c_void,
    write: Write,
    info: &rsf_dump_sidecar_info,
    stats: *mut rsf_dump_sidecar_stats,
) {
    let image = unsafe { &*info.image };
    let mut out = Stream::new(context, write);
    out.emit("{\n  \"tool\": \"rescaleframe-module-dump\",\n");
    let _ = write!(
        out,
        "  \"abi_version\": {},\n  \"schema_version\": {},\n",
        info.abi_version, info.schema_version
    );
    out.emit("  \"module\": {\"path\": \"");
    unsafe { path(&mut out, info.module_path) };
    let _ = write!(
        out,
        "\", \"load_base\": \"{:#x}\", \"preferred_base\": \"{:#x}\", \
        \"size_of_image\": {}, \"entry_point_rva\": \"{:#x}\"}},\n",
        image.base as usize, info.preferred_base, image.image_size, info.entry_rva
    );
    out.emit("  \"code_entropy\": ");
    fixed(&mut out, info.code_entropy);
    let _ = write!(out, ",\n  \"iat_references\": {},\n", info.iat_references);

    out.emit("  \"sections\": [");
    let mut comma = "";
    for index in 0..image.sections {
        let Some(Section {
            name,
            rva,
            virtual_size: size,
            flags,
            ..
        }) = sidecar_section(image, index)
        else {
            break;
        };
        let _ = write!(out, "{comma}\n    {{\"name\": \"");
        comma = ",";
        for byte in name.into_iter().take_while(|&byte| byte != 0) {
            escape_unit(&mut out, byte.into());
        }
        let _ = write!(
            out,
            "\", \"rva\": \"{rva:#x}\", \"virtual_size\": {size}, \"characteristics\": \"{flags:#x}\""
        );
        if !info.digests.is_null() {
            let digest = unsafe { &*info.digests.add(index as usize) };
            if digest.valid != 0 {
                let _ = write!(
                    out,
                    ", \"dumped_size\": {}, \"readable_bytes\": {}, \"entropy\": ",
                    digest.dumped_size, digest.readable_bytes
                );
                fixed(&mut out, digest.entropy);
                let text = unsafe { CStr::from_ptr(digest.sha256.as_ptr()) }
                    .to_str()
                    .unwrap_or("");
                let _ = write!(out, ", \"sha256\": \"{text}\"");
            }
        }
        out.emit("}");
    }
    out.emit("\n  ],\n  \"modules\": [");
    if !info.modules.is_null() && info.module_count != 0 {
        let modules = unsafe { slice::from_raw_parts(info.modules, info.module_count) };
        for (index, module) in modules.iter().enumerate() {
            let comma = if index == 0 { "" } else { "," };
            let _ = write!(out, "{comma}\n    {{\"path\": \"");
            wide(&mut out, &module.path);
            let _ = write!(
                out,
                "\", \"base\": \"{:#x}\", \"size\": {}}}",
                module.base as usize, module.size
            );
        }
    }
    out.emit("\n  ],\n  \"imports\": [");
    let imports_described = if info.import_bytes == 0 {
        0
    } else {
        unsafe {
            rsf_dump_imports_impl(
                context,
                write,
                image.base,
                image.image_size,
                image.import_directory,
                info.modules,
                info.module_count,
            )
        }
    };
    out.emit("\n  ],\n");
    let structured = rsf_dump_now_ms();
    let threads_listed =
        unsafe { rsf_dump_threads_impl(context, write, info.modules, info.module_count) };
    let rtti_classes = write_rtti(&mut out, image, info.rtti_budget_ms);
    let function_starts = rsf_dump_functions_impl(
        context,
        write,
        image.base,
        image.image_size,
        info.exception_rva,
        info.exception_bytes,
    );
    unsafe { rsf_dump_names_impl(context, info.name_list, write) };
    let now = rsf_dump_now_ms();
    let _ = write!(
        out,
        "  \"timing_ms\": {{\"section_hash_and_entropy\": {}, \"structured_sections\": {}, \"total\": {}}},\n",
        (info.hash_ms + 0.5) as u32,
        (now - structured + 0.5) as u32,
        (now - info.started_ms + 0.5) as u32
    );
    out.emit("  \"limits\": \"Section raw offsets equal virtual addresses, so the dump maps one to one onto runtime addresses. It is not runnable: the entry point is unchanged and no import table was rebuilt. Import values are the addresses resolved in this process at dump time. Threads, RTTI and function starts describe the process at dump time.\"\n}\n");
    unsafe {
        stats.write(rsf_dump_sidecar_stats {
            imports_described,
            threads_listed,
            rtti_classes,
            function_starts,
        })
    };
}
