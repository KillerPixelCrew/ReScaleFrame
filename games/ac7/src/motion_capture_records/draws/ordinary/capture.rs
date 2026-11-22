// SPDX-License-Identifier: MIT
use super::{Draw, DrawInput, rsf_ac7_capture_record_draw};
use crate::motion_capture_graphics_abi::rsf_frame_tap_target_draw;
use crate::motion_capture_observations::{
    colour::{
        LightingDraw, rsf_ac7_capture_is_cloud_composite, rsf_ac7_capture_take_lighting_after,
    },
    rsf_ac7_capture_shader_facts,
};
use crate::motion_capture_records::{
    draws::{BoundView, DrawTarget, QueuedDraw, gpu::*, rsf_ac7_capture_can_record_draw},
    rsf_ac7_capture_root_scope,
};
use crate::motion_capture_session::{
    CaptureInterval, rsf_ac7_capture_fail, rsf_ac7_capture_sample,
};
use std::ffi::{c_char, c_void};
use std::fmt;
use std::ptr::null;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;

pub(in crate::motion_capture_records::draws) unsafe extern "C" fn observe(
    _: *mut c_void,
    draw: *const rsf_frame_tap_target_draw,
) {
    let capture = rsf_ac7_capture_sample();
    let Some(draw) = (unsafe { draw.as_ref() }) else {
        return;
    };
    if capture.active() && unsafe { observe_draw(capture, draw) }.is_err() {
        rsf_ac7_capture_fail(capture);
    }
}

unsafe fn observe_draw(capture: CaptureInterval, draw: &rsf_frame_tap_target_draw) -> fmt::Result {
    let ps = rsf_ac7_capture_shader_facts(draw.pixel_shader, 1);
    let identity_of_draw = LightingDraw {
        context: draw.context.addr() as u64,
        target: draw.render_target.addr() as u64,
        index: draw.draw_index,
        hash: ps.hash,
    };
    let matched_light = rsf_ac7_capture_take_lighting_after(capture, &identity_of_draw);
    if !rsf_ac7_capture_can_record_draw(capture) {
        return Ok(());
    }
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&draw.context) }) else {
        return Ok(());
    };
    let mut gs = None;
    let mut hs = None;
    let mut ds = None;
    unsafe {
        context.GSGetShader(&mut gs, None, None);
        context.HSGetShader(&mut hs, None, None);
        context.DSGetShader(&mut ds, None, None);
    }
    let shaders = [
        draw.vertex_shader,
        draw.pixel_shader,
        identity(&gs),
        identity(&hs),
        identity(&ds),
    ];
    let stages = std::array::from_fn::<_, 5, _>(|index| {
        if index == 1 {
            ps
        } else {
            rsf_ac7_capture_shader_facts(shaders[index], index as u32)
        }
    });
    let mut row = Draw {
        root: rsf_ac7_capture_root_scope(),
        queued: QueuedDraw::default(),
        qpc: tick(),
        target: identity_of_draw.target,
        layout: draw.input_layout.addr() as u64,
        thread: thread_id(),
        draw_index: draw.draw_index,
        format: draw.target_format,
        width: draw.target_width,
        height: draw.target_height,
        target_count: draw.target_count,
        depth_bound: draw.depth_bound,
        indexed: draw.indexed,
        elements: draw.element_count,
        shader_hashes: stages.each_ref().map(|s| s.hash),
        topology: draw.topology,
        stride: draw.vertex_stride,
        viewport_x: draw.viewport_x,
        viewport_y: draw.viewport_y,
        viewport_width: draw.viewport_width,
        viewport_height: draw.viewport_height,
        input_count: draw.input_count,
        ps_resource_count: 0,
        vs_resource_count: 0,
        render_target_count: 0,
        inputs: std::array::from_fn(|_| Default::default()),
        ps_resources: [BoundView::default(); 128],
        vs_resources: [BoundView::default(); 128],
        render_targets: std::array::from_fn(|_| Default::default()),
        depth: Default::default(),
        constants: std::array::from_fn(|_| Default::default()),
        context: identity_of_draw.context,
    };
    if draw.input_count as usize > row.inputs.len() {
        return Err(fmt::Error);
    }
    let inputs =
        unsafe { crate::velocity_container::borrowed(draw.inputs, draw.input_count as usize) }
            .ok_or(fmt::Error)?;
    for (out, input) in row.inputs.iter_mut().zip(inputs) {
        *out = DrawInput {
            resource: input.texture.addr() as u64,
            slot: input.slot,
            width: input.width,
            height: input.height,
            format: input.format,
        };
    }
    let mut ps_views = [const { None }; 128];
    let mut vs_views = [const { None }; 128];
    let mut targets = [const { None }; 8];
    let mut depth = None;
    unsafe {
        context.PSGetShaderResources(0, Some(&mut ps_views));
        context.VSGetShaderResources(0, Some(&mut vs_views));
        context.OMGetRenderTargets(Some(&mut targets), Some(&mut depth));
    }
    visit_views(ps_views, |view| {
        row.ps_resources[row.ps_resource_count as usize] = view;
        row.ps_resource_count += 1;
    });
    visit_views(vs_views, |view| {
        row.vs_resources[row.vs_resource_count as usize] = view;
        row.vs_resource_count += 1;
    });
    let large = draw.target_width >= 400 && draw.target_height >= 200;
    for (slot, target) in targets.into_iter().enumerate() {
        let Some(target) = target else { continue };
        let Ok(resource) = (unsafe { target.GetResource() }) else {
            continue;
        };
        let view = BoundView {
            resource: facts(&resource),
            slot: slot as u32,
        };
        let light_after =
            slot == 0 && matched_light && view.resource.resource == identity_of_draw.target;
        let role = match slot {
            0 if light_after => c"contact_light_after".as_ptr(),
            0 if large => colour_stage(ps.hash),
            1 if large && rsf_ac7_capture_is_cloud_composite(ps.hash) => {
                c"cloud_loss_after".as_ptr()
            }
            _ => null(),
        };
        row.render_targets[row.render_target_count as usize] = DrawTarget {
            view,
            snapshot: snapshot(capture, context, &resource, role, light_after)?,
        };
        row.render_target_count += 1;
    }
    if let Some(depth) = depth
        && let Ok(resource) = unsafe { depth.GetResource() }
    {
        row.depth = facts(&resource);
    }
    let buffers = [
        &draw.vertex_constants,
        &draw.pixel_constants,
        &draw.geometry_constants,
        &draw.hull_constants,
        &draw.domain_constants,
    ];
    for (index, buffers) in buffers.into_iter().enumerate() {
        if shaders[index].is_null() {
            continue;
        }
        let constants = &mut row.constants[index];
        unsafe {
            visit_constants(
                capture,
                context,
                buffers,
                stages[index].constant_mask,
                false,
                |item| {
                    constants.entries[constants.count as usize] = item;
                    constants.count += 1;
                },
            )
        }?;
    }
    row.queued = queued_draw();
    if rsf_ac7_capture_record_draw(capture, &row) {
        Ok(())
    } else {
        Err(fmt::Error)
    }
}

fn colour_stage(hash: u32) -> *const c_char {
    // Diagnostic producer labels from the original AC7 DXBC and RenoDX AC7 reference.
    match hash {
        0x6bcbd787 | 0x5fb31417 => c"tonemap".as_ptr(),
        0xa5c9147b => c"scene_effect".as_ptr(),
        0xb2f7719d => c"hud".as_ptr(),
        0x2136e125 | 0xa7441be5 => c"composite".as_ptr(),
        hash if rsf_ac7_capture_is_cloud_composite(hash) => c"cloud_composite_after".as_ptr(),
        _ => null(),
    }
}
