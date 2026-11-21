// SPDX-License-Identifier: MIT
//! Before-draw rows over the live bindings and dump receipts collected by native code.

use super::{
    BoundConstant, DrawTarget, QueuedDraw, commit, write_constants, write_queued, write_resource,
    write_snapshot, write_targets,
};
use crate::motion_capture_observations::colour::BeforeDrawPlan;
use crate::motion_capture_records::artifacts::{Bytes, write_finite_float};
use crate::motion_capture_session::CaptureInterval;
use std::fmt::{self, Write as _};
use std::mem::{offset_of, size_of};

#[repr(C)]
struct Sampler {
    slot: u32,
    filter: u32,
    address: [u32; 3],
    comparison: u32,
    min_lod: f32,
    max_lod: f32,
}

#[repr(C)]
struct BeforeDraw {
    queued: QueuedDraw,
    qpc: u64,
    thread: u32,
    draw_index: u32,
    input_count: u32,
    sampler_count: u32,
    constant_count: u32,
    colour: DrawTarget,
    loss: DrawTarget,
    inputs: [DrawTarget; 6],
    samplers: [Sampler; 6],
    constants: [BoundConstant; 2],
}

const _: () = {
    assert!(size_of::<Sampler>() == 32 && offset_of!(Sampler, min_lod) == 24);
    assert!(size_of::<BeforeDraw>() == 1496 && offset_of!(BeforeDraw, colour) == 88);
    assert!(offset_of!(BeforeDraw, inputs) == 376 && offset_of!(BeforeDraw, samplers) == 1240);
    assert!(offset_of!(BeforeDraw, constants) == 1432);
};

fn write_sampled_view(out: &mut Bytes, target: &DrawTarget) -> fmt::Result {
    out.write_str("{\"view\":")?;
    write_resource(out, &target.view.resource)?;
    write_snapshot(out, &target.snapshot)?;
    out.write_char('}')
}

fn write_lod(out: &mut Bytes, value: f32) -> fmt::Result {
    if value.is_finite() {
        return write_finite_float(out, value, 6);
    }
    // The old Windows stream wrote these tokens after promoting the LOD to double.
    if value.is_sign_negative() {
        out.write_char('-')?;
    }
    out.write_str(match value.to_bits() {
        0xffc0_0000 => "nan(ind)",
        _ if value.is_nan() => "nan",
        _ => "inf",
    })
}

fn write_samplers(out: &mut Bytes, samplers: &[Sampler]) -> fmt::Result {
    for (index, sampler) in samplers.iter().enumerate() {
        if index != 0 {
            out.write_char(',')?;
        }
        let [u, v, w] = sampler.address;
        write!(
            out,
            "{{\"slot\":{},\"filter\":{},\"address\":[{u},{v},{w}],\"comparison\":{},\"lod\":[",
            sampler.slot, sampler.filter, sampler.comparison
        )?;
        write_lod(out, sampler.min_lod)?;
        out.write_char(',')?;
        write_lod(out, sampler.max_lod)?;
        out.write_str("]}")?;
    }
    Ok(())
}

fn write_before(
    out: &mut Bytes,
    interval: u32,
    plan: &BeforeDrawPlan,
    draw: &BeforeDraw,
) -> fmt::Result {
    let lighting = plan.lighting != 0;
    let kind = if lighting {
        "contact_light_before"
    } else {
        "cloud_composite_before"
    };
    write!(
        out,
        "{{\"kind\":\"{kind}\",\"qpc\":{},\"interval\":{interval},\"thread\":{}",
        draw.qpc, draw.thread
    )?;
    if !lighting {
        write!(
            out,
            ",\"target\":{},\"draw_index\":{}",
            draw.colour.view.resource.resource, draw.draw_index
        )?;
    }
    write!(out, ",\"ps_hash\":{},\"colour\":", plan.ps_hash)?;
    write_sampled_view(out, &draw.colour)?;
    if !lighting {
        out.write_str(",\"loss\":")?;
        if draw.loss.view.resource.resource == 0 {
            out.write_str("null")?;
        } else {
            write_sampled_view(out, &draw.loss)?;
        }
    }
    let inputs = draw
        .inputs
        .get(..draw.input_count as usize)
        .ok_or(fmt::Error)?;
    out.write_str(",\"inputs\":[")?;
    write_targets(out, inputs)?;
    out.write_char(']')?;
    if lighting {
        let samplers = draw
            .samplers
            .get(..draw.sampler_count as usize)
            .ok_or(fmt::Error)?;
        let constants = draw
            .constants
            .get(..draw.constant_count as usize)
            .ok_or(fmt::Error)?;
        out.write_str(",\"samplers\":[")?;
        write_samplers(out, samplers)?;
        out.write_str("],\"ps_cb\":[")?;
        write_constants(out, constants)?;
        out.write_char(']')?;
        write_queued(out, &draw.queued)?;
    }
    out.write_char('}')
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_record_before_draw(
    capture: CaptureInterval,
    plan: &BeforeDrawPlan,
    draw: &BeforeDraw,
) -> bool {
    let mut row = Bytes::default();
    write_before(&mut row, capture.interval, plan, draw).is_ok()
        && commit(capture, || Ok(row)).is_ok()
}
