// SPDX-License-Identifier: MIT
//! Geometry rows pair admitted layout metadata with bounded native readback results.

use super::{QueuedDraw, commit, write_queued};
use crate::motion_capture_observations::geometry::GeometryPlan;
use crate::motion_capture_records::{GeometryScope, artifacts::Bytes};
use crate::motion_capture_session::CaptureInterval;
use std::fmt::{self, Write as _};
use std::mem::{offset_of, size_of};

// Private ABI mirrored in motion_capture_records.h. Buffer identities are opaque numbers.
#[repr(C)]
struct GeometryStream {
    buffer: u64,
    slot: u32,
    stride: u32,
    offset: u32,
    source_bytes: u32,
    sample_bytes: u32,
    blob: u32,
}

#[repr(C)]
struct GeometryVsBuffer {
    resource: u64,
    slot: u32,
    format: u32,
    offset: u32,
    source_bytes: u32,
    sample_bytes: u32,
    blob: u32,
}

#[repr(C)]
struct Geometry {
    queued: QueuedDraw,
    scope: GeometryScope,
    qpc: u64,
    layout: u64,
    kind: u32,
    count: u32,
    start: u32,
    base_vertex: i32,
    instances: u32,
    start_instance: u32,
    stream_count: u32,
    vs_buffer_count: u32,
    streams: [GeometryStream; 32],
    vs_buffers: [GeometryVsBuffer; 3],
}

const _: () = {
    assert!(size_of::<GeometryStream>() == 32 && offset_of!(GeometryStream, blob) == 28);
    assert!(size_of::<GeometryVsBuffer>() == 32 && offset_of!(GeometryVsBuffer, blob) == 28);
    assert!(size_of::<Geometry>() == 1256 && offset_of!(Geometry, qpc) == 88);
    assert!(offset_of!(Geometry, base_vertex) == 116 && offset_of!(Geometry, streams) == 136);
    assert!(offset_of!(Geometry, vs_buffers) == 1160);
};

fn write_geometry(
    out: &mut Bytes,
    interval: u32,
    plan: &GeometryPlan,
    d: &Geometry,
) -> fmt::Result {
    let scope = &d.scope;
    let layout = &plan.layout;
    let elements = layout
        .elements
        .get(..layout.copied as usize)
        .ok_or(fmt::Error)?;
    let streams = d.streams.get(..d.stream_count as usize).ok_or(fmt::Error)?;
    let vs_buffers = d
        .vs_buffers
        .get(..d.vs_buffer_count as usize)
        .ok_or(fmt::Error)?;
    write!(
        out,
        concat!(
            "{{\"kind\":\"geometry\",\"interval\":{},\"qpc\":{},\"layout\":{},",
            "\"vs_hash\":{},\"geometry_bucket\":{},\"native_role\":{},\"native_frame\":{},",
            "\"primitive\":{},\"material\":{},\"geometry_capacity\":{},",
            "\"draw_kind\":{},\"count\":{},\"start\":{},\"base_vertex\":{},",
            "\"instances\":{},\"start_instance\":{},\"layout_count\":{},\"elements\":["
        ),
        interval,
        d.qpc,
        d.layout,
        plan.vs_hash,
        plan.bucket,
        scope.role,
        scope.native_frame,
        scope.primitive,
        scope.material,
        scope.geometry_capacity,
        d.kind,
        d.count,
        d.start,
        d.base_vertex,
        d.instances,
        d.start_instance,
        layout.count,
    )?;
    for (index, element) in elements.iter().enumerate() {
        if index != 0 {
            out.write_char(',')?;
        }
        write!(
            out,
            "{{\"attribute\":{},\"format\":{},\"slot\":{},\"offset\":{},\"instance\":{}}}",
            element.semantic_index,
            element.format,
            element.input_slot,
            element.byte_offset,
            element.per_instance,
        )?;
    }
    out.write_str("],\"streams\":[")?;
    for (index, stream) in streams.iter().enumerate() {
        if index != 0 {
            out.write_char(',')?;
        }
        write!(
            out,
            concat!(
                "{{\"slot\":{},\"buffer\":{},\"stride\":{},\"offset\":{},",
                "\"source_bytes\":{},\"sample_bytes\":{},\"blob\":{}}}"
            ),
            stream.slot,
            stream.buffer,
            stream.stride,
            stream.offset,
            stream.source_bytes,
            stream.sample_bytes,
            stream.blob,
        )?;
    }
    out.write_str("],\"vs_buffers\":[")?;
    for (index, view) in vs_buffers.iter().enumerate() {
        if index != 0 {
            out.write_char(',')?;
        }
        write!(
            out,
            concat!(
                "{{\"slot\":{},\"resource\":{},\"format\":{},\"offset\":{},",
                "\"source_bytes\":{},\"sample_bytes\":{},\"blob\":{}}}"
            ),
            view.slot,
            view.resource,
            view.format,
            view.offset,
            view.source_bytes,
            view.sample_bytes,
            view.blob,
        )?;
    }
    out.write_char(']')?;
    write_queued(out, &d.queued)?;
    out.write_char('}')
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_record_geometry(
    capture: CaptureInterval,
    plan: &GeometryPlan,
    draw: &Geometry,
) -> bool {
    let mut row = Bytes::default();
    write_geometry(&mut row, capture.interval, plan, draw).is_ok()
        && commit(capture, row) != super::Commit::Failed
}
