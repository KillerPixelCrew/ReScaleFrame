// SPDX-License-Identifier: MIT
//! Sample-local admission for AC7 geometry diagnostics and their native buffer reads.

use super::{LayoutSnapshot, observations};
use crate::motion_capture_records::rsf_ac7_capture_geometry_scope;
use crate::motion_capture_session::CaptureInterval;
use rsf_game_sys::{RSF_GAME_RENDER_TRANSLUCENCY, RSF_GAME_RENDER_TRANSLUCENT_MESH};
use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::{offset_of, size_of};

// DXGI resource formats used in the original AC7 captures.
const FLOAT4: u32 = 2;
const FLOAT3: u32 = 6;
const R16G16_UNORM: u32 = 35;
const READ_LIMITS: [u32; 5] = [8, 12, 12, 8, 12];

#[derive(Default)]
pub(super) struct GeometryReads {
    sample: Option<CaptureInterval>,
    reads: [u32; READ_LIMITS.len()],
    particle_candidates: [HashSet<(u32, usize)>; 3],
}

#[repr(u32)]
pub(crate) enum GeometryAdmission {
    Skipped,
    Admitted,
    Failed,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct GeometryPlan {
    pub(crate) layout: LayoutSnapshot,
    pub(crate) vs_hash: u32,
    pub(crate) bucket: u32,
    pub(crate) ia_slots: u32,
    pub(crate) vs_slot_count: u32,
    pub(crate) vs_slots: [u32; 3],
}

const _: () = {
    assert!(size_of::<GeometryPlan>() == 356 && offset_of!(GeometryPlan, vs_hash) == 328);
    assert!(offset_of!(GeometryPlan, ia_slots) == 336 && offset_of!(GeometryPlan, vs_slots) == 344);
};

fn classify(layout: &LayoutSnapshot, native_velocity: bool) -> (usize, u32) {
    let mut slots = 0;
    let mut float4 = [false; 16];
    let mut float3_at_one = false;
    for element in &layout.elements[..layout.copied as usize] {
        if element.input_slot < 32 {
            slots |= 1 << element.input_slot;
        }
        match (element.semantic_index, element.format) {
            (index @ 0..=15, FLOAT4) => float4[index as usize] = true,
            (1, FLOAT3) => float3_at_one = true,
            _ => {}
        }
    }
    let bucket = if native_velocity {
        4
    } else if float4[8] && float4[9] && float4[10] {
        2
    } else if float4[0] && float3_at_one && float4[2] && float4[3] && float4[4] {
        3
    } else if float4[0] && float4[1] {
        1
    } else {
        0
    };
    (bucket, slots)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_ac7_capture_plan_geometry(
    capture: CaptureInterval,
    format: u32,
    depth_bound: bool,
    layout: *mut c_void,
    vertex_shader: *mut c_void,
    plan: &mut GeometryPlan,
) -> GeometryAdmission {
    let native_velocity = format == R16G16_UNORM && depth_bound;
    if !capture.active()
        || (!native_velocity
            && !matches!(
                rsf_ac7_capture_geometry_scope().role,
                RSF_GAME_RENDER_TRANSLUCENCY | RSF_GAME_RENDER_TRANSLUCENT_MESH
            ))
    {
        return GeometryAdmission::Skipped;
    }

    let mut observations = observations();
    if CaptureInterval::load() != capture {
        return GeometryAdmission::Skipped;
    }
    if observations.geometry.sample != Some(capture) {
        observations.geometry = GeometryReads {
            sample: Some(capture),
            ..Default::default()
        };
    }
    let identity = layout.addr();
    let Some(&layout) = observations.layouts.get(&identity) else {
        return GeometryAdmission::Skipped;
    };
    let (bucket, ia_slots) = classify(&layout, native_velocity);
    let vs_hash = observations.shader_facts(vertex_shader.addr(), 0).hash;
    let geometry = &mut observations.geometry;
    // Present can advance during the layout and shader lookup.
    if CaptureInterval::load() != capture || geometry.reads[bucket] >= READ_LIMITS[bucket] {
        return GeometryAdmission::Skipped;
    }
    if (1..=3).contains(&bucket) {
        let seen = &mut geometry.particle_candidates[bucket - 1];
        let key = (vs_hash, identity);
        if seen.contains(&key) {
            return GeometryAdmission::Skipped;
        }
        if seen.try_reserve(1).is_err() {
            return GeometryAdmission::Failed;
        }
        seen.insert(key);
    }
    let (vs_slot_count, vs_slots) = match bucket {
        4 => (3, [0, 1, 127]),
        2 => (1, [127, 0, 0]),
        _ => (0, [0; 3]),
    };
    *plan = GeometryPlan {
        layout,
        vs_hash,
        bucket: bucket as u32,
        ia_slots,
        vs_slot_count,
        vs_slots,
    };
    GeometryAdmission::Admitted
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_ac7_capture_geometry_read(
    capture: CaptureInterval,
    bucket: u32,
    reserve: bool,
) -> bool {
    let mut observations = observations();
    let geometry = &mut observations.geometry;
    if geometry.sample != Some(capture) || CaptureInterval::load() != capture {
        return false;
    }
    let bucket = bucket as usize;
    let Some(reads) = geometry.reads.get_mut(bucket) else {
        return false;
    };
    if *reads >= READ_LIMITS[bucket] {
        return false;
    }
    *reads += u32::from(reserve);
    true
}
