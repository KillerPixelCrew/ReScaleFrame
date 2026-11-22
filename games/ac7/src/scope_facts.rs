// SPDX-License-Identifier: MIT
//! Exact-build view facts read from the matched AC7 renderer and view layouts.
use crate::platform_abi::rsf_ac7_render_scope;
use crate::scope_abi::rsf_ac7_scope_read;
use std::ffi::c_void;

const RENDERER_VIEWS: u64 = 0xb8;
const RENDERER_VIEW_COUNT: u64 = 0xc0;
const VIEW_FAMILY: u64 = 0;
const FAMILY_FRAME: u64 = 0x68;
const VIEW_RENDER_RECT: u64 = 0x70;
const VIEW_OUTPUT_RECT: u64 = 0x80;
const VIEW_UNCONSTRAINED_RECT: u64 = 0x90;
const VIEW_CAPTURE_FLAGS: u64 = 0xc42;

fn field<T: Default>(read: rsf_ac7_scope_read, base: u64, offset: u64) -> Option<T> {
    let read = read?;
    let address = base.checked_add(offset)?;
    if base == 0 {
        return None;
    }
    let mut value = T::default();
    unsafe {
        read(
            (&raw mut value).cast(),
            address as *const c_void,
            size_of::<T>(),
        )
    }
    .then_some(value)
}

fn single_view(read: rsf_ac7_scope_read, renderer: u64) -> Option<u64> {
    let view = field(read, renderer, RENDERER_VIEWS)?;
    (field::<i32>(read, renderer, RENDERER_VIEW_COUNT)? == 1).then_some(view)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_scope_primary_view(
    read: rsf_ac7_scope_read,
    view: u64,
    width: u32,
    height: u32,
) -> bool {
    field::<[i32; 4]>(read, view, VIEW_UNCONSTRAINED_RECT)
        == Some([0, 0, width as i32, height as i32])
        && field::<[u8; 3]>(read, view, VIEW_CAPTURE_FLAGS) == Some([0; 3])
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_scope_primary_renderer(
    read: rsf_ac7_scope_read,
    renderer: u64,
    width: u32,
    height: u32,
) -> bool {
    width != 0
        && height != 0
        && single_view(read, renderer)
            .is_some_and(|view| rsf_ac7_scope_primary_view(read, view, width, height))
}

fn fill(
    read: rsf_ac7_scope_read,
    renderer: u64,
    width: u32,
    height: u32,
    scope: &mut rsf_ac7_render_scope,
) -> Option<()> {
    let view = single_view(read, renderer)?;
    let family = field(read, view, VIEW_FAMILY)?;
    let frame: u32 = field(read, family, FAMILY_FRAME)?;
    let render: [i32; 4] = field(read, view, VIEW_RENDER_RECT)?;
    let output: [i32; 4] = field(read, view, VIEW_OUTPUT_RECT)?;
    if !rsf_ac7_scope_primary_view(read, view, width, height) {
        return None;
    }

    scope.view_key = view;
    scope.family_key = family;
    scope.native_frame = u64::from(frame);
    scope.render_rect = render;
    scope.output_rect = output;
    // UE4.18 lacks the temporal-upscale material mip policy.
    let render_width = i64::from(render[2]) - i64::from(render[0]);
    let output_width = i64::from(output[2]) - i64::from(output[0]);
    if render_width > 0 && render_width <= output_width {
        scope.material_texture_mip_bias =
            ((render_width as f32 / output_width as f32).log2() - 0.3).max(-2.0);
    }
    Some(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_scope_primary_facts(
    read: rsf_ac7_scope_read,
    renderer: u64,
    width: u32,
    height: u32,
    scope: Option<&mut rsf_ac7_render_scope>,
) -> bool {
    scope.is_some_and(|scope| fill(read, renderer, width, height, scope).is_some())
}
