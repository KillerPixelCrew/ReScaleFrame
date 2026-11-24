// SPDX-License-Identifier: MIT
//! The translucent velocity pass's per-draw stream policy: scope and draw admission, the
//! pipeline route, the input vertex and stream-output record bounds, the indirect packing and
//! argument range, capture and zero-source sizing, the hash build's record count and dispatch,
//! and the packed b13 draw parameters. The D3D11 replay applies these decisions to
//! the borrowed context, including the argument copy, GPU validation, draws and hash build.

use crate::sys::rsf_frame_tap_geometry;
use crate::velocity_history::{MIN_CAPACITY, RECORD_LIMIT};
use rsf_game_sys::{
    RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_TRANSLUCENT_MESH, rsf_game_render_pass,
};
use std::mem::offset_of;

// Bound shader stages besides vertex and pixel.
pub(crate) const STAGE_GEOMETRY: u32 = 1;
pub(crate) const STAGE_HULL: u32 = 2;
pub(crate) const STAGE_DOMAIN: u32 = 4;

/// Whether a draw's pipeline is captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Admitted,
    Skipped,
    /// A partial tessellation pipeline, or tessellation with a native GS.
    Mismatched,
}

// D3D11_PRIMITIVE_TOPOLOGY values. An N-point patch list is N plus PATCH_LIST_BASE.
const TRIANGLE_LIST: u32 = 4;
const TRIANGLE_STRIP: u32 = 5;
const PATCH_LIST_BASE: u32 = 32;

// rsf_frame_tap_geometry kinds: Draw, DrawIndexed, DrawInstanced, DrawIndexedInstanced,
// DrawAuto, DrawIndexedInstancedIndirect and DrawInstancedIndirect.
const FIRST_INSTANCED: u32 = 2;
const DRAW_AUTO: u32 = 4;
const INDEXED_INDIRECT: u32 = 5;
const LAST_KIND: u32 = 6;
const INDEXED_ARGUMENTS: u32 = 20;
const ARGUMENTS: u32 = 16;

// Indirect validation packings: records per input vertex, per strip triangle, per patch, or
// the full output bound for native GS emission.
const PACK_VERTICES: u32 = 1;
const PACK_STRIP: u32 = 2;
const PACK_PATCHES: u32 = 3;
const PACK_EMITTED: u32 = 4;

// The hash build's 64-thread groups and per-dimension dispatch limit.
const HASH_GROUP: u32 = 64;
const DISPATCH_LIMIT: u32 = 65535;
// UE4.27 clips translucent, additive and modulate opacity, while AlphaComposite has no opacity
// discard in GetMaterialClippingVelocity. Its native opacity is saturated.
const ALPHA_COMPOSITE: u32 = 5;
const OPACITY_STEP: f32 = 1.0 / 255.0;

/// The indirect validation's cbuffer (Capacity, OutputCapacity, Packing, Points): the input
/// vertex bound, the stream-output record bound, the packing and the patch control points.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct IndirectBounds {
    pub(crate) vertices: u32,
    pub(crate) records: u32,
    pub(crate) packing: u32,
    pub(crate) points: u32,
}

/// `capacity` is what a capture or zero-source buffer holding fewer than `bounds.records` grows
/// to. `built` is the hash build's record count, an upper bound for indirect draws.
/// `zero_source`: native GS emission or DrawAuto leaves no readable count, so the capture is
/// zeroed before the draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StreamPlan {
    pub(crate) bounds: IndirectBounds,
    pub(crate) capacity: u32,
    pub(crate) built: u32,
    pub(crate) zero_source: bool,
}

/// The geometry, tessellation and pixel b13 cbuffer of velocity_geometry.rs,
/// velocity_tessellation.rs and velocity_pixel.rs: Rect, Jitter, Cutoff, HasHistory,
/// HistoryMask, Padding and ClipToPrevious.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DrawParameters {
    rect: [f32; 4],
    jitter: [f32; 4],
    cutoff: f32,
    history: u32,
    mask: u32,
    padding: u32,
    matrix: [f32; 16],
}

/// The hash build's cbuffer (Count, Mask, Mode, WorkWidth). A zero work width reads the count
/// and width from the validated indirect arguments.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct HashParameters {
    count: u32,
    mask: u32,
    mode: u32,
    work_width: u32,
}

/// `groups` is the direct dispatch; an indirect build dispatches from the arguments instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct HashDispatch {
    pub(crate) parameters: HashParameters,
    pub(crate) groups: [u32; 2],
}

const _: () = {
    assert!(size_of::<IndirectBounds>() == 16);
    assert!(size_of::<DrawParameters>() == 112 && offset_of!(DrawParameters, cutoff) == 32);
    assert!(offset_of!(DrawParameters, matrix) == 48);
    assert!(size_of::<HashParameters>() == 16);
};

/// A primary translucent mesh scope with a camera, and a single-sample draw with depth of a
/// known kind that has vertices when it is direct.
pub(crate) fn draw_admitted(scope: &rsf_game_render_pass, draw: &rsf_frame_tap_geometry) -> bool {
    scope.role == RSF_GAME_RENDER_TRANSLUCENT_MESH
        && scope.camera_valid != 0
        && scope.flags & RSF_GAME_RENDER_PRIMARY != 0
        && !draw.context.is_null()
        && !draw.depth_view.is_null()
        && draw.samples == 1
        && draw.kind <= LAST_KIND
        && (draw.kind >= DRAW_AUTO || draw.count != 0)
}

/// Tessellation needs both stages and no native GS. Without it only triangle lists and strips
/// are captured; the patch topology is checked once the variant knows its control points.
pub(crate) fn route(topology: u32, stages: u32) -> Route {
    let geometry = stages & STAGE_GEOMETRY != 0;
    let hull = stages & STAGE_HULL != 0;
    let domain = stages & STAGE_DOMAIN != 0;
    if hull != domain || (hull && geometry) {
        Route::Mismatched
    } else if !hull && topology != TRIANGLE_LIST && topology != TRIANGLE_STRIP {
        Route::Skipped
    } else {
        Route::Admitted
    }
}

/// The record bounds of an admitted route. Indirect and DrawAuto inputs are bounded by the
/// scope's geometry capacity, read as triangles of control points for patches.
pub(crate) fn plan(
    draw: &rsf_frame_tap_geometry,
    stages: u32,
    geometry_capacity: u32,
    maximum_vertices: u32,
    points: u32,
) -> Option<StreamPlan> {
    let geometry = stages & STAGE_GEOMETRY != 0;
    let hull = stages & STAGE_HULL != 0;
    if hull && (points == 0 || draw.topology != PATCH_LIST_BASE.wrapping_add(points)) {
        return None;
    }
    let strip = draw.topology == TRIANGLE_STRIP;
    let instances = if draw.kind >= FIRST_INSTANCED {
        draw.instances
    } else {
        1
    };
    let vertices = if draw.kind >= DRAW_AUTO {
        u64::from(geometry_capacity) * u64::from(if hull { points } else { 3 }) / 3
    } else {
        u64::from(draw.count) * u64::from(instances)
    };
    // A strip can expand to three records per emitted vertex; the input bound already counts
    // three vertices per primitive.
    let expansion = maximum_vertices.checked_mul(if strip { 3 } else { 1 })?;
    if vertices == 0 || expansion == 0 || vertices > u64::from(RECORD_LIMIT / expansion) {
        return None;
    }
    let records = u32::try_from(vertices * u64::from(expansion)).ok()?;
    let vertices = u32::try_from(vertices).ok()?;
    let packing = if geometry {
        PACK_EMITTED
    } else if hull {
        PACK_PATCHES
    } else if strip {
        PACK_STRIP
    } else {
        PACK_VERTICES
    };
    let mut capacity = MIN_CAPACITY;
    while capacity < records {
        capacity *= 2;
    }
    // Fixed vertex and patch producers of a direct draw build from their exact record count.
    let built = if !geometry && draw.kind < DRAW_AUTO {
        if hull {
            (draw.count / points)
                .wrapping_mul(3)
                .wrapping_mul(instances)
        } else if strip {
            (draw.count.max(2) - 2)
                .wrapping_mul(3)
                .wrapping_mul(instances)
        } else {
            vertices
        }
    } else {
        records
    };
    Some(StreamPlan {
        bounds: IndirectBounds {
            vertices,
            records,
            packing,
            points,
        },
        capacity,
        built,
        zero_source: geometry || draw.kind == DRAW_AUTO,
    })
}

/// The end of the argument range copied from a 4-byte-aligned offset, or None when the buffer
/// does not hold it.
pub(crate) fn indirect_end(kind: u32, offset: u32, byte_width: u32) -> Option<u32> {
    let bytes = if kind == INDEXED_INDIRECT {
        INDEXED_ARGUMENTS
    } else {
        ARGUMENTS
    };
    if offset > byte_width || bytes > byte_width - offset || !offset.is_multiple_of(4) {
        return None;
    }
    Some(offset + bytes)
}

/// `rect` is the single viewport's left, top, width and height. `previous_records` is the
/// previous history buffer's capacity, zero when it does not exist.
fn parameters(
    scope: &rsf_game_render_pass,
    rect: [f32; 4],
    usable: bool,
    previous_records: u32,
) -> DrawParameters {
    let camera = &scope.camera;
    let [width, height] = [rect[2], rect[3]];
    DrawParameters {
        rect,
        jitter: [
            camera.jitter_pixels[0] * 2.0 / width,
            camera.jitter_pixels[1] * -2.0 / height,
            camera.previous_jitter_pixels[0] * 2.0 / width,
            camera.previous_jitter_pixels[1] * -2.0 / height,
        ],
        cutoff: if scope.material_blend == ALPHA_COMPOSITE {
            -1.0
        } else {
            scope.material_opacity_clip + OPACITY_STEP
        },
        history: u32::from(usable && previous_records != 0),
        mask: previous_records.saturating_sub(1),
        padding: 0,
        matrix: camera.clip_to_previous_clip,
    }
}

/// None when there is nothing to build.
pub(crate) fn hash_dispatch(
    records: u32,
    destination_records: u32,
    mode: u32,
    dynamic: bool,
) -> Option<HashDispatch> {
    if records == 0 {
        return None;
    }
    let groups = records.div_ceil(HASH_GROUP);
    let width = groups.min(DISPATCH_LIMIT);
    Some(HashDispatch {
        parameters: HashParameters {
            count: records,
            mask: destination_records.wrapping_sub(1),
            mode,
            work_width: if dynamic { 0 } else { width },
        },
        groups: [width, groups.div_ceil(width)],
    })
}

/// The parameters for exactly one viewport of positive extent. A NaN extent passes, as the
/// D3D11 viewport check did.
pub(crate) fn draw_parameters(
    scope: &rsf_game_render_pass,
    viewports: u32,
    rect: [f32; 4],
    usable: bool,
    previous_records: u32,
) -> Option<DrawParameters> {
    if viewports != 1 || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return None;
    }
    Some(parameters(scope, rect, usable, previous_records))
}

#[cfg(windows)]
pub(crate) const INDIRECT_SOURCE: &str = r#"
RWByteAddressBuffer Arguments:register(u0);
cbuffer Parameters:register(b0) {uint Capacity;uint OutputCapacity;uint Packing;uint Points;};
[numthreads(1,1,1)]void main(uint3 id:SV_DispatchThreadID) {
    uint vertices=Arguments.Load(0),instances=Arguments.Load(4);
    if(vertices==0 || instances>Capacity/vertices) {Arguments.Store(4,0);instances=0;}
    uint records=0;
    if(instances!=0) {
        records=Packing==4 ? OutputCapacity : Packing==3 ? (vertices/max(Points,1))*3*instances :
            Packing==2 ? (max(vertices,2)-2)*3*instances : vertices*instances;
        if(records>OutputCapacity) {Arguments.Store(4,0);records=0;}
    }
    uint groups=(records+63)/64;
    Arguments.Store(32,records);Arguments.Store3(36,uint3(min(groups,65535),(groups+65534)/65535,1));
}"#;

#[cfg(test)]
mod tests;
