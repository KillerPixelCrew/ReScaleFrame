// SPDX-License-Identifier: MIT
//! Shared draw-stream admission and serialization over copied D3D11 facts.

mod geometry;
mod ordinary;

use super::{NativeRootScope, QueuedScope, Records, artifacts::Bytes, records};
use crate::motion_capture_session::{CaptureInterval, rsf_ac7_capture_current};
use std::ffi::c_char;
use std::fmt::{self, Write as _};
use std::mem::{offset_of, size_of};
use std::slice;

const MAX_DRAWS: usize = 12000;
const TEXTURE_2D: u32 = 3;
const TEXTURE_3D: u32 = 4;

// Private ABI mirrored in motion_capture_records.h. IDs are numeric and never dereferenced.
#[repr(C)]
struct QueuedDraw {
    command: u64,
    execute: u64,
    scope: QueuedScope,
}

#[repr(C)]
struct Resource {
    resource: u64,
    dimension: u32,
    width: u32,
    height: u32,
    depth: u32,
    format: u32,
}

#[repr(C)]
struct BoundView {
    resource: Resource,
    slot: u32,
}

#[repr(C)]
struct BoundConstant {
    buffer: u64,
    upload_serial: u64,
    slot: u32,
    blob: u32,
    readback: u32,
}

#[repr(C)]
struct Dispatch {
    root: NativeRootScope,
    queued: QueuedDraw,
    qpc: u64,
    indirect: u64,
    thread: u32,
    groups: [u32; 3],
    offset: u32,
    cs_hash: u32,
    input_count: u32,
    output_count: u32,
    constant_count: u32,
    inputs: [BoundView; 128],
    outputs: [BoundView; 8],
    constants: [BoundConstant; 14],
}

const _: () = {
    assert!(size_of::<QueuedDraw>() == 56 && size_of::<Resource>() == 32);
    assert!(size_of::<BoundView>() == 40 && offset_of!(BoundView, slot) == 32);
    assert!(size_of::<BoundConstant>() == 32 && offset_of!(BoundConstant, readback) == 24);
    assert!(size_of::<Dispatch>() == 6016 && offset_of!(Dispatch, qpc) == 72);
    assert!(offset_of!(Dispatch, inputs) == 128 && offset_of!(Dispatch, outputs) == 5248);
    assert!(offset_of!(Dispatch, constants) == 5568);
};

fn draw_available(records: &mut Records, capture: CaptureInterval) -> bool {
    // A row can finish after its sample interval but before that same session retires.
    if !rsf_ac7_capture_current(capture.session) {
        return false;
    }
    if records.draws.len() >= MAX_DRAWS {
        records.draw_dropped = records.draw_dropped.wrapping_add(1);
        return false;
    }
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_can_record_draw(capture: CaptureInterval) -> bool {
    draw_available(&mut records(), capture)
}

fn commit(
    capture: CaptureInterval,
    row: impl FnOnce() -> Result<Bytes, fmt::Error>,
) -> fmt::Result {
    let mut records = records();
    if draw_available(&mut records, capture) {
        let row = row()?;
        records.draws.try_reserve(1).map_err(|_| fmt::Error)?;
        records.draws.push(row.0);
    }
    Ok(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_record_draw_text(
    capture: CaptureInterval,
    text: *const c_char,
    size: usize,
) -> bool {
    commit(capture, || {
        let mut row = Bytes::default();
        if size != 0 {
            // SAFETY: Native lends the completed row for the duration of this call.
            row.extend(unsafe { slice::from_raw_parts(text.cast(), size) })?;
        }
        Ok(row)
    })
    .is_ok()
}

fn write_resource(out: &mut Bytes, r: &Resource) -> fmt::Result {
    write!(
        out,
        "{{\"resource\":{},\"dimension\":{}",
        r.resource, r.dimension
    )?;
    if matches!(r.dimension, TEXTURE_2D | TEXTURE_3D) {
        write!(out, ",\"width\":{},\"height\":{}", r.width, r.height)?;
        if r.dimension == TEXTURE_3D {
            write!(out, ",\"depth\":{}", r.depth)?;
        }
        write!(out, ",\"format\":{}", r.format)?;
    }
    out.write_char('}')
}

fn write_views(out: &mut Bytes, views: &[BoundView]) -> fmt::Result {
    for (index, view) in views.iter().enumerate() {
        if index != 0 {
            out.write_char(',')?;
        }
        write!(out, "{{\"slot\":{},\"view\":", view.slot)?;
        write_resource(out, &view.resource)?;
        out.write_char('}')?;
    }
    Ok(())
}

fn write_constants(out: &mut Bytes, constants: &[BoundConstant]) -> fmt::Result {
    for (index, cb) in constants.iter().enumerate() {
        if index != 0 {
            out.write_char(',')?;
        }
        write!(
            out,
            "{{\"slot\":{},\"buffer\":{},\"blob\":{},\"upload_serial\":{},\"readback\":{}}}",
            cb.slot, cb.buffer, cb.blob, cb.upload_serial, cb.readback
        )?;
    }
    Ok(())
}

fn write_queued(out: &mut Bytes, queued: &QueuedDraw) -> fmt::Result {
    let scope = &queued.scope;
    write!(
        out,
        concat!(
            ",\"rhi_command\":{},\"rhi_execute\":{},\"queued_native_scope\":{},",
            "\"queued_native_view\":{},\"queued_native_family\":{},\"queued_native_frame\":{},",
            "\"queued_list\":{},\"queued_uid\":{}"
        ),
        queued.command,
        queued.execute,
        scope.scope,
        scope.view,
        scope.family,
        scope.native_frame,
        scope.command_list,
        scope.queue_uid,
    )
}

fn write_dispatch(out: &mut Bytes, interval: u32, d: &Dispatch) -> fmt::Result {
    let inputs = d.inputs.get(..d.input_count as usize).ok_or(fmt::Error)?;
    let outputs = d.outputs.get(..d.output_count as usize).ok_or(fmt::Error)?;
    let constants = d
        .constants
        .get(..d.constant_count as usize)
        .ok_or(fmt::Error)?;
    write!(
        out,
        concat!(
            "{{\"kind\":\"dispatch\",\"qpc\":{},\"interval\":{},\"thread\":{},",
            "\"native_scope\":{},\"native_view\":{},\"groups\":[{},{},{}],",
            "\"indirect\":{},\"offset\":{},\"cs_hash\":{},\"inputs\":["
        ),
        d.qpc,
        interval,
        d.thread,
        d.root.id,
        d.root.view,
        d.groups[0],
        d.groups[1],
        d.groups[2],
        d.indirect,
        d.offset,
        d.cs_hash,
    )?;
    write_views(out, inputs)?;
    out.write_str("],\"outputs\":[")?;
    write_views(out, outputs)?;
    out.write_str("],\"cs_cb\":[")?;
    write_constants(out, constants)?;
    out.write_char(']')?;
    write_queued(out, &d.queued)?;
    out.write_char('}')
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_record_dispatch(
    capture: CaptureInterval,
    dispatch: &Dispatch,
) -> bool {
    let mut row = Bytes::default();
    write_dispatch(&mut row, capture.interval, dispatch).is_ok()
        && commit(capture, || Ok(row)).is_ok()
}
