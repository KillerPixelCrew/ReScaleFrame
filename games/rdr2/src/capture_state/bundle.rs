// SPDX-License-Identifier: MIT
//! Bundle publication consumes the capture's snapshots under the native capture mutex.
use super::Capture;
use super::artifacts::{output_directory, write_camera};
use super::metadata::Temporal;
use super::recordings::Draw;
use std::ffi::c_char;
use std::fmt::Write as _;
use std::{fs, slice, str};

#[repr(C)]
struct Bundle {
    directory: *const u16,
    directory_len: usize,
    base: u64,
    pid: u32,
}

const _: () = assert!(size_of::<Bundle>() == 32 && std::mem::offset_of!(Bundle, pid) == 24);

type FormatScalar = unsafe extern "C" fn(f64, *mut c_char, usize) -> i32;

fn separator(out: &mut String, index: usize) {
    if index != 0 {
        out.push(',');
    }
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

fn write_temporal(out: &mut String, input: &Temporal, format_scalar: FormatScalar) -> Option<()> {
    let facts = input.facts;
    let _ = write!(
        out,
        concat!(
            "{{\"index\":{},\"frame_counter_observation\":{},\"thread\":{},\"feature_flags\":{},",
            "\"tick\":{},\"qpc\":{},\"command_list\":\"0x{:x}\",\"generation\":{},\"view\":\"0x{:x}\",",
            "\"draw_context\":\"0x{:x}\",\"job\":\"0x{:x}\",\"camera_valid\":{},\"evaluation_result\":{},\"scalars\":{{"
        ),
        input.id.index,
        facts.frame,
        facts.thread,
        input.feature_flags,
        facts.tick,
        facts.qpc,
        input.list,
        input.generation,
        facts.view,
        facts.draw_context,
        facts.job,
        input.camera.is_some(),
        input.result.unwrap_or_default(),
    );
    for (index, (key, value)) in input.scalars().enumerate() {
        separator(out, index);
        let _ = write!(out, "\"{key}\":");
        // Preserve the existing %.12g representation of the finite NGX scalars.
        let mut number = [0u8; 32];
        let count = unsafe { format_scalar(value, number.as_mut_ptr().cast(), number.len()) };
        let count = usize::try_from(count).ok()?;
        out.push_str(str::from_utf8(number.get(..count)?).ok()?);
    }
    out.push_str("},\"refusals\":[");
    for (index, (prefix, suffix)) in input.refusals().enumerate() {
        separator(out, index);
        let _ = write!(out, "\"{prefix}{suffix}\"");
    }
    out.push_str("]}");
    Some(())
}

/// The native caller lends the directory and scalar formatter while holding the capture mutex.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_write_bundle(
    owner: &Capture,
    bundle: &Bundle,
    format_scalar: FormatScalar,
    draw_count: &mut usize,
    temporal_count: &mut usize,
) -> bool {
    let directory =
        output_directory(unsafe { slice::from_raw_parts(bundle.directory, bundle.directory_len) });
    let metadata = unsafe { &*owner.metadata.get() };
    let draws = &unsafe { &*owner.recordings.get() }.draws;
    let ledger = unsafe { &mut *owner.ledger.get() };
    *draw_count = draws.len();
    *temporal_count = metadata.temporal.len();
    for input in &metadata.temporal {
        if let Some(camera) = &input.camera
            && !write_camera(&directory, bundle.pid, input.id, camera)
        {
            ledger.write_failed = true;
        }
    }

    let mut out = String::new();
    let progress = ledger.progress();
    let presentation = &metadata.presentation;
    let descriptor = presentation.descriptor;
    let color_space = presentation.details.color_space;
    let hdr = presentation.details.hdr.as_ref();
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
        descriptor.format,
        descriptor.width,
        descriptor.height,
        color_space.unwrap_or_default(),
        color_space.is_some(),
        hdr.map_or(0, |hdr| hdr.kind),
    );
    if let Some(hdr) = hdr {
        for byte in hdr.bytes() {
            let _ = write!(out, "{byte:02x}");
        }
    }
    let _ = write!(
        out,
        "\"}},\"requested_input_frames\":{},\"temporal\":[",
        progress.requested_input_frames,
    );
    for (index, input) in metadata.temporal.iter().enumerate() {
        separator(&mut out, index);
        if write_temporal(&mut out, input, format_scalar).is_none() {
            return false;
        }
    }
    out.push_str("],\"draws\":[");
    for (index, draw) in draws.iter().enumerate() {
        separator(&mut out, index);
        write_draw(&mut out, draw);
    }
    out.push_str("]}\n");

    let mut path = directory.join(format!("bundle-{}-{:03}", bundle.pid, progress.sequence));
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
