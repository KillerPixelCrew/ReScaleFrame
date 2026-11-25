// SPDX-License-Identifier: MIT
//! View facts read at the documented AC7 renderer and view layout offsets (build 9855922).
use crate::sys::{rsf_ac7_copy_memory, rsf_ac7_render_scope};
use std::ffi::c_void;

const RENDERER_VIEWS: u64 = 0xb8;
const RENDERER_VIEW_COUNT: u64 = 0xc0;
const VIEW_FAMILY: u64 = 0;
const FAMILY_FRAME: u64 = 0x68;
const VIEW_RENDER_RECT: u64 = 0x70;
const VIEW_OUTPUT_RECT: u64 = 0x80;
const VIEW_UNCONSTRAINED_RECT: u64 = 0x90;
const VIEW_CAPTURE_FLAGS: u64 = 0xc42;

/// The `T` at `base + offset` under the guarded read.
fn field<T: Default>(base: u64, offset: u64) -> Option<T> {
    let address = base.checked_add(offset)?;
    if base == 0 {
        return None;
    }
    let mut value = T::default();
    unsafe {
        rsf_ac7_copy_memory(
            (&raw mut value).cast(),
            address as *const c_void,
            size_of::<T>(),
        )
    }
    .then_some(value)
}

fn single_view(renderer: u64) -> Option<u64> {
    let view = field(renderer, RENDERER_VIEWS)?;
    (field::<i32>(renderer, RENDERER_VIEW_COUNT)? == 1).then_some(view)
}

/// A view of the whole `width` x `height` output that no scene capture renders.
pub(crate) fn primary_view(view: u64, width: u32, height: u32) -> bool {
    field::<[i32; 4]>(view, VIEW_UNCONSTRAINED_RECT) == Some([0, 0, width as i32, height as i32])
        && field::<[u8; 3]>(view, VIEW_CAPTURE_FLAGS) == Some([0; 3])
}

/// A renderer of a single primary view.
pub(crate) fn primary_renderer(renderer: u64, width: u32, height: u32) -> bool {
    width != 0
        && height != 0
        && single_view(renderer).is_some_and(|view| primary_view(view, width, height))
}

/// Fills the native view, family, frame, rectangles and mip bias of an initialized scope from a
/// primary renderer. `mip_bias` is the plan's vendor texture LOD bias.
pub(crate) fn primary_facts(
    renderer: u64,
    width: u32,
    height: u32,
    mip_bias: f32,
    scope: &mut rsf_ac7_render_scope,
) -> Option<()> {
    let view = single_view(renderer)?;
    let family = field(view, VIEW_FAMILY)?;
    let frame: u32 = field(family, FAMILY_FRAME)?;
    let render: [i32; 4] = field(view, VIEW_RENDER_RECT)?;
    let output: [i32; 4] = field(view, VIEW_OUTPUT_RECT)?;
    if !primary_view(view, width, height) {
        return None;
    }

    scope.view_key = view;
    scope.family_key = family;
    scope.native_frame = u64::from(frame);
    scope.render_rect = render;
    scope.output_rect = output;
    // UE4.18 lacks the temporal-upscale material mip policy; the plan's vendor bias stands in.
    let render_width = i64::from(render[2]) - i64::from(render[0]);
    let output_width = i64::from(output[2]) - i64::from(output[0]);
    if render_width > 0 && render_width <= output_width {
        scope.material_texture_mip_bias = mip_bias;
    }
    Some(())
}
