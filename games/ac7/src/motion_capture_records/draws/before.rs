// SPDX-License-Identifier: MIT
//! Before-draw rows over the live bindings and dump receipts collected by native code.

use super::{
    BoundConstant, Commit, DrawTarget, QueuedDraw, commit, write_constants, write_queued,
    write_resource, write_snapshot, write_targets,
};
use crate::motion_capture_observations::colour::{
    BeforeDrawPlan, LightingDraw, record_lighting_before,
};
use crate::motion_capture_records::artifacts::{Bytes, write_float_with_precision};
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
    context: u64,
}

const _: () = {
    assert!(size_of::<Sampler>() == 32 && offset_of!(Sampler, min_lod) == 24);
    assert!(size_of::<BeforeDraw>() == 1504 && offset_of!(BeforeDraw, colour) == 88);
    assert!(offset_of!(BeforeDraw, context) == 1496);
    assert!(offset_of!(BeforeDraw, inputs) == 376 && offset_of!(BeforeDraw, samplers) == 1240);
    assert!(offset_of!(BeforeDraw, constants) == 1432);
};

fn write_sampled_view(out: &mut Bytes, target: &DrawTarget) -> fmt::Result {
    out.write_str("{\"view\":")?;
    write_resource(out, &target.view.resource)?;
    write_snapshot(out, &target.snapshot)?;
    out.write_char('}')
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
        write_float_with_precision(out, sampler.min_lod, 6)?;
        out.write_char(',')?;
        write_float_with_precision(out, sampler.max_lod, 6)?;
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
    write!(
        out,
        ",\"target\":{},\"draw_index\":{},\"context\":{}",
        draw.colour.view.resource.resource, draw.draw_index, draw.context
    )?;
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
    if write_before(&mut row, capture.interval, plan, draw).is_err() {
        return false;
    }
    match commit(capture, row) {
        Commit::Failed => false,
        Commit::Skipped => true,
        Commit::Recorded => {
            if plan.lighting != 0
                && draw.colour.snapshot.name[0] != 0
                && draw.colour.snapshot.result == 0
            {
                record_lighting_before(
                    capture,
                    LightingDraw {
                        context: draw.context,
                        target: draw.colour.view.resource.resource,
                        index: draw.draw_index,
                        hash: plan.ps_hash,
                    },
                );
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_lod_is_json_with_six_significant_digits() {
        let mut output = Bytes::default();
        write_samplers(
            &mut output,
            &[
                Sampler {
                    slot: 0,
                    filter: 0,
                    address: [1, 1, 1],
                    comparison: 0,
                    min_lod: f32::INFINITY,
                    max_lod: f32::from_bits(0xffc0_0000),
                },
                Sampler {
                    slot: 1,
                    filter: 0,
                    address: [1, 1, 1],
                    comparison: 0,
                    min_lod: f32::NEG_INFINITY,
                    max_lod: 0.12345678,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            str::from_utf8(&output.0).unwrap(),
            concat!(
                "{\"slot\":0,\"filter\":0,\"address\":[1,1,1],\"comparison\":0,\"lod\":[null,null]},",
                "{\"slot\":1,\"filter\":0,\"address\":[1,1,1],\"comparison\":0,\"lod\":[null,0.123457]}"
            )
        );
    }
}
