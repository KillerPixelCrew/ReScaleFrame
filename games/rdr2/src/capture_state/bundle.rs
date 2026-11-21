// SPDX-License-Identifier: MIT
//! Bundle publication uses the engine facts lent under the native capture mutex.
use super::Progress;
use super::artifacts::output_directory;
use super::recordings::Draw;
use std::ffi::{CStr, c_char};
use std::fmt::Write as _;
use std::{fs, slice, str};

#[repr(C)]
struct Temporal {
    sequence: u32,
    index: u32,
    frame: u32,
    thread: u32,
    feature_flags: u32,
    camera_valid: u32,
    tick: u64,
    qpc: u64,
    list: u64,
    generation: u64,
    view: u64,
    draw_context: u64,
    job: u64,
    result: u64,
}

#[repr(C)]
struct Scalar {
    key: *const c_char,
    value: f64,
}

#[repr(C)]
struct TemporalView {
    facts: *const Temporal,
    scalars: *const Scalar,
    scalar_count: usize,
    refusal_count: usize,
}

#[repr(C)]
struct Presentation {
    format: u32,
    width: u32,
    height: u32,
    color_space: u32,
    color_space_set: u32,
    metadata_type: u32,
    metadata: *const u8,
    metadata_size: usize,
}

#[repr(C)]
struct Bundle {
    directory: *const u16,
    directory_len: usize,
    base: u64,
    progress: Progress,
    presentation: Presentation,
    draws: *const Draw,
    draw_count: usize,
    temporal_count: usize,
    pid: u32,
}

const _: () = {
    assert!(size_of::<Temporal>() == 88);
    assert!(size_of::<Scalar>() == 16);
    assert!(size_of::<TemporalView>() == 32);
    assert!(size_of::<Presentation>() == 40);
    assert!(size_of::<Bundle>() == 128 && std::mem::offset_of!(Bundle, draws) == 96);
};

type ReadTemporal = unsafe extern "C" fn(usize, *mut TemporalView);
type ReadRefusal = unsafe extern "C" fn(usize, usize) -> *const c_char;
type FormatScalar = unsafe extern "C" fn(f64, *mut c_char, usize) -> i32;

/// A C++ vector may give a null data pointer when empty. The caller lends any nonempty span.
unsafe fn borrowed<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(ptr, len) }
    }
}

fn separator(out: &mut String, index: usize) {
    if index != 0 {
        out.push(',');
    }
}

fn quoted(out: &mut String, value: &CStr) {
    out.push('"');
    for ch in value.to_string_lossy().chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\0'..='\u{1f}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(ch));
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
}

fn write_draw(out: &mut String, draw: &Draw) {
    let _ = write!(
        out,
        concat!(
            "{{\"draw\":{},\"frame_counter_observation\":{},\"vertices\":{},\"instances\":{},",
            "\"command_list\":\"0x{:x}\",\"generation\":{},\"caller\":\"0x{:x}\",\"pso\":\"0x{:x}\",",
            "{},\"stack\":["
        ),
        draw.ordinal,
        draw.frame,
        draw.vertices,
        draw.instances,
        draw.list,
        draw.generation,
        draw.caller,
        draw.pso,
        draw.pipeline,
    );
    for (index, address) in draw.stack.iter().take(draw.stack_size as usize).enumerate() {
        separator(out, index);
        let _ = write!(out, "\"0x{:x}\"", address.addr());
    }
    out.push_str("]}");
}

/// The native callbacks lend facts and strings from this bundle's capture, held under its mutex.
unsafe fn write_temporal(
    out: &mut String,
    index: usize,
    read_temporal: ReadTemporal,
    read_refusal: ReadRefusal,
    format_scalar: FormatScalar,
) -> Option<()> {
    let mut view = std::mem::MaybeUninit::uninit();
    unsafe { read_temporal(index, view.as_mut_ptr()) };
    let view = unsafe { view.assume_init() };
    let facts = unsafe { &*view.facts };
    let _ = write!(
        out,
        concat!(
            "{{\"index\":{},\"frame_counter_observation\":{},\"thread\":{},\"feature_flags\":{},",
            "\"tick\":{},\"qpc\":{},\"command_list\":\"0x{:x}\",\"generation\":{},\"view\":\"0x{:x}\",",
            "\"draw_context\":\"0x{:x}\",\"job\":\"0x{:x}\",\"camera_valid\":{},\"evaluation_result\":{},\"scalars\":{{"
        ),
        facts.index,
        facts.frame,
        facts.thread,
        facts.feature_flags,
        facts.tick,
        facts.qpc,
        facts.list,
        facts.generation,
        facts.view,
        facts.draw_context,
        facts.job,
        facts.camera_valid != 0,
        facts.result,
    );
    let scalars = unsafe { borrowed(view.scalars, view.scalar_count) };
    for (index, scalar) in scalars.iter().enumerate() {
        separator(out, index);
        quoted(out, unsafe { CStr::from_ptr(scalar.key) });
        out.push(':');
        // Preserve the existing %.12g representation of the finite NGX scalars.
        let mut number = [0u8; 32];
        let count =
            unsafe { format_scalar(scalar.value, number.as_mut_ptr().cast(), number.len()) };
        let count = usize::try_from(count).ok()?;
        out.push_str(str::from_utf8(number.get(..count)?).ok()?);
    }
    out.push_str("},\"refusals\":[");
    for item in 0..view.refusal_count {
        separator(out, item);
        quoted(out, unsafe { CStr::from_ptr(read_refusal(index, item)) });
    }
    out.push_str("]}");
    Some(())
}

/// The native caller lends all spans and typed callbacks while it holds the capture mutex.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_write_bundle(
    bundle: &Bundle,
    read_temporal: ReadTemporal,
    read_refusal: ReadRefusal,
    format_scalar: FormatScalar,
) -> bool {
    let mut out = String::new();
    let progress = &bundle.progress;
    let presentation = &bundle.presentation;
    let _ = write!(
        out,
        concat!(
            "{{\"version\":1,\"sequence\":{},\"module_base\":\"0x{:x}\",\"draw_count\":{},",
            "\"draw_overflow\":{},\"capture_timeout\":{},\"write_failed\":{},\"readbacks_written\":{},",
            "\"presentation\":{{\"format\":{},\"width\":{},\"height\":{},\"color_space\":{},\"color_space_set\":{},",
            "\"hdr_metadata_type\":{},\"hdr_metadata_hex\":\""
        ),
        progress.sequence,
        bundle.base,
        progress.draws,
        progress.draw_overflow != 0,
        progress.timed_out != 0,
        progress.write_failed != 0,
        progress.written,
        presentation.format,
        presentation.width,
        presentation.height,
        presentation.color_space,
        presentation.color_space_set != 0,
        presentation.metadata_type,
    );
    for byte in unsafe { borrowed(presentation.metadata, presentation.metadata_size) } {
        let _ = write!(out, "{byte:02x}");
    }
    let _ = write!(
        out,
        "\"}},\"requested_input_frames\":{},\"temporal\":[",
        progress.requested_input_frames,
    );
    for index in 0..bundle.temporal_count {
        separator(&mut out, index);
        if unsafe { write_temporal(&mut out, index, read_temporal, read_refusal, format_scalar) }
            .is_none()
        {
            return false;
        }
    }
    out.push_str("],\"draws\":[");
    let draws = unsafe { borrowed(bundle.draws, bundle.draw_count) };
    for (index, draw) in draws.iter().enumerate() {
        separator(&mut out, index);
        write_draw(&mut out, draw);
    }
    out.push_str("]}\n");

    let directory = unsafe { borrowed(bundle.directory, bundle.directory_len) };
    let mut path =
        output_directory(directory).join(format!("bundle-{}-{:03}", bundle.pid, progress.sequence));
    path.set_extension("json");
    let temporary = path.with_extension("json.tmp");
    // Readers treat the manifest's appearance as the publication point.
    if fs::write(&temporary, out)
        .and_then(|()| fs::rename(&temporary, &path))
        .is_ok()
    {
        return true;
    }
    let _ = fs::remove_file(temporary);
    false
}
