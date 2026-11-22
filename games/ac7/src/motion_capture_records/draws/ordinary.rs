// SPDX-License-Identifier: MIT
//! Ordinary draw rows pair the frame tap's shadow state with live post-draw observations.

use super::{
    BoundConstant, BoundView, DrawTarget, QueuedDraw, Resource, commit, write_constants,
    write_queued, write_resource, write_targets, write_views,
};
use crate::motion_capture_records::{
    NativeRootScope,
    artifacts::{Bytes, write_float},
};
use crate::motion_capture_session::CaptureInterval;
use std::fmt::{self, Write as _};
use std::mem::{offset_of, size_of};

#[repr(C)]
struct DrawInput {
    resource: u64,
    slot: u32,
    width: u32,
    height: u32,
    format: u32,
}

#[repr(C)]
struct DrawConstants {
    entries: [BoundConstant; 14],
    count: u32,
}

// Keep this private snapshot in sync with MotionDraw in motion_capture_records.h.
#[repr(C)]
struct Draw {
    root: NativeRootScope,
    queued: QueuedDraw,
    qpc: u64,
    target: u64,
    layout: u64,
    thread: u32,
    draw_index: u32,
    format: u32,
    width: u32,
    height: u32,
    target_count: u32,
    depth_bound: u32,
    indexed: u32,
    elements: u32,
    shader_hashes: [u32; 5],
    topology: u32,
    stride: u32,
    viewport_x: f32,
    viewport_y: f32,
    viewport_width: u32,
    viewport_height: u32,
    input_count: u32,
    ps_resource_count: u32,
    vs_resource_count: u32,
    render_target_count: u32,
    inputs: [DrawInput; 16],
    ps_resources: [BoundView; 128],
    vs_resources: [BoundView; 128],
    render_targets: [DrawTarget; 8],
    depth: Resource,
    constants: [DrawConstants; 5],
    context: u64,
}

const _: () = {
    assert!(size_of::<DrawInput>() == 24 && offset_of!(DrawInput, slot) == 8);
    assert!(size_of::<DrawConstants>() == 456 && offset_of!(DrawConstants, count) == 448);
    assert!(size_of::<Draw>() == 14288 && offset_of!(Draw, qpc) == 72);
    assert!(offset_of!(Draw, context) == 14280);
    assert!(offset_of!(Draw, shader_hashes) == 132 && offset_of!(Draw, viewport_x) == 160);
    assert!(offset_of!(Draw, inputs) == 192 && offset_of!(Draw, ps_resources) == 576);
    assert!(offset_of!(Draw, vs_resources) == 5696 && offset_of!(Draw, render_targets) == 10816);
    assert!(offset_of!(Draw, depth) == 11968 && offset_of!(Draw, constants) == 12000);
};

fn write_inputs(out: &mut Bytes, inputs: &[DrawInput]) -> fmt::Result {
    for (index, input) in inputs.iter().enumerate() {
        if index != 0 {
            out.write_char(',')?;
        }
        write!(
            out,
            "{{\"slot\":{},\"resource\":{},\"width\":{},\"height\":{},\"format\":{}}}",
            input.slot, input.resource, input.width, input.height, input.format,
        )?;
    }
    Ok(())
}

fn write_draw(out: &mut Bytes, interval: u32, d: &Draw) -> fmt::Result {
    let inputs = d.inputs.get(..d.input_count as usize).ok_or(fmt::Error)?;
    let ps = d
        .ps_resources
        .get(..d.ps_resource_count as usize)
        .ok_or(fmt::Error)?;
    let vs = d
        .vs_resources
        .get(..d.vs_resource_count as usize)
        .ok_or(fmt::Error)?;
    let targets = d
        .render_targets
        .get(..d.render_target_count as usize)
        .ok_or(fmt::Error)?;
    let [vs_hash, ps_hash, gs_hash, hs_hash, ds_hash] = d.shader_hashes;
    write!(
        out,
        concat!(
            "{{\"kind\":\"draw\",\"qpc\":{},\"interval\":{},\"thread\":{},\"native_scope\":{},",
            "\"draw_index\":{},\"context\":{},\"native_view\":{},\"target\":{},\"format\":{},\"extent\":[{},{}],",
            "\"targets\":{},\"depth_bound\":{},\"indexed\":{},\"elements\":{},",
            "\"vs_hash\":{},\"ps_hash\":{},\"gs_hash\":{},\"hs_hash\":{},\"ds_hash\":{},",
            "\"layout\":{},\"topology\":{},\"stride\":{},\"viewport\":["
        ),
        d.qpc,
        interval,
        d.thread,
        d.root.id,
        d.draw_index,
        d.context,
        d.root.view,
        d.target,
        d.format,
        d.width,
        d.height,
        d.target_count,
        d.depth_bound,
        d.indexed,
        d.elements,
        vs_hash,
        ps_hash,
        gs_hash,
        hs_hash,
        ds_hash,
        d.layout,
        d.topology,
        d.stride,
    )?;
    write_float(out, d.viewport_x)?;
    out.write_char(',')?;
    write_float(out, d.viewport_y)?;
    write!(
        out,
        ",{},{}],\"inputs\":[",
        d.viewport_width, d.viewport_height
    )?;
    write_inputs(out, inputs)?;
    out.write_str("],\"ps_resources\":[")?;
    write_views(out, ps)?;
    out.write_str("],\"vs_resources\":[")?;
    write_views(out, vs)?;
    out.write_str("],\"render_targets\":[")?;
    write_targets(out, targets)?;
    out.write_str("],\"depth\":")?;
    if d.depth.resource == 0 {
        out.write_str("null")?;
    } else {
        write_resource(out, &d.depth)?;
    }
    for (key, stage) in ["vs_cb", "ps_cb", "gs_cb", "hs_cb", "ds_cb"]
        .into_iter()
        .zip(&d.constants)
    {
        let entries = stage
            .entries
            .get(..stage.count as usize)
            .ok_or(fmt::Error)?;
        write!(out, ",\"{key}\":[")?;
        write_constants(out, entries)?;
        out.write_char(']')?;
    }
    write_queued(out, &d.queued)?;
    out.write_char('}')
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_record_draw(capture: CaptureInterval, draw: &Draw) -> bool {
    let mut row = Bytes::default();
    write_draw(&mut row, capture.interval, draw).is_ok()
        && commit(capture, row) != super::Commit::Failed
}
