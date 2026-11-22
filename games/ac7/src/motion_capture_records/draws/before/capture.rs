// SPDX-License-Identifier: MIT
use super::{BeforeDraw, Sampler, rsf_ac7_capture_record_before_draw};
use crate::motion_capture_graphics_abi::rsf_frame_tap_target_draw;
use crate::motion_capture_observations::colour::{
    BeforeDrawPlan, rsf_ac7_capture_plan_before_draw,
};
use crate::motion_capture_records::draws::{
    BoundConstant, BoundView, DrawTarget, QueuedDraw, gpu::*,
};
use crate::motion_capture_session::{
    CaptureInterval, rsf_ac7_capture_fail, rsf_ac7_capture_sample,
};
use std::ffi::{c_char, c_void};
use std::fmt;
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
    if capture.active() && unsafe { observe_before(capture, draw) }.is_err() {
        rsf_ac7_capture_fail(capture);
    }
}

unsafe fn observe_before(
    capture: CaptureInterval,
    draw: &rsf_frame_tap_target_draw,
) -> fmt::Result {
    let mut plan = BeforeDrawPlan::default();
    if !rsf_ac7_capture_plan_before_draw(
        capture,
        draw.target_width,
        draw.target_height,
        draw.pixel_shader,
        &mut plan,
    ) {
        return Ok(());
    }
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&draw.context) }) else {
        return Ok(());
    };
    // The tap lends the slot-0 texture for the callback, with the original texture identity.
    let Some(resource) = (unsafe { ID3D11Resource::from_raw_borrowed(&draw.render_target) }) else {
        return Ok(());
    };
    let lighting = plan.lighting != 0;
    let sample =
        |resource: &ID3D11Resource, role: *const c_char, slot| -> Result<DrawTarget, fmt::Error> {
            Ok(DrawTarget {
                view: BoundView {
                    resource: facts(resource),
                    slot,
                },
                snapshot: snapshot(capture, context, resource, role, lighting)?,
            })
        };
    let mut row = BeforeDraw {
        queued: QueuedDraw::default(),
        qpc: tick(),
        thread: thread_id(),
        draw_index: draw.draw_index,
        input_count: 0,
        sampler_count: 0,
        constant_count: 0,
        colour: sample(resource, plan.colour_role, 0)?,
        loss: DrawTarget::default(),
        inputs: std::array::from_fn(|_| Default::default()),
        samplers: std::array::from_fn(|_| Default::default()),
        constants: [BoundConstant::default(); 2],
        context: draw.context.addr() as u64,
    };
    if !plan.loss_role.is_null() {
        let mut targets = [None, None];
        unsafe { context.OMGetRenderTargets(Some(&mut targets), None) };
        if let Some(target) = &targets[1]
            && let Ok(resource) = unsafe { target.GetResource() }
        {
            row.loss = sample(&resource, plan.loss_role, 1)?;
        }
    }
    let mut inputs = [const { None }; 6];
    unsafe { context.PSGetShaderResources(0, Some(&mut inputs[..plan.input_slots as usize])) };
    for (slot, input) in inputs.into_iter().enumerate() {
        if let Some(input) = input
            && let Ok(resource) = unsafe { input.GetResource() }
        {
            row.inputs[row.input_count as usize] =
                sample(&resource, plan.input_roles[slot], slot as u32)?;
            row.input_count += 1;
        }
    }
    if lighting {
        let mut samplers = [const { None }; 6];
        unsafe { context.PSGetSamplers(0, Some(&mut samplers[..plan.sampler_slots as usize])) };
        for (slot, sampler) in samplers.into_iter().enumerate() {
            let Some(sampler) = sampler else { continue };
            let mut desc = D3D11_SAMPLER_DESC::default();
            unsafe { sampler.GetDesc(&mut desc) };
            row.samplers[row.sampler_count as usize] = Sampler {
                slot: slot as u32,
                filter: desc.Filter.0 as u32,
                address: [
                    desc.AddressU.0 as u32,
                    desc.AddressV.0 as u32,
                    desc.AddressW.0 as u32,
                ],
                comparison: desc.ComparisonFunc.0 as u32,
                min_lod: desc.MinLOD,
                max_lod: desc.MaxLOD,
            };
            row.sampler_count += 1;
        }
        // The before draw samples the two live shader inputs with the reserved lighting quota.
        let mut buffers = [None, None];
        unsafe {
            context.PSGetConstantBuffers(0, Some(&mut buffers[..plan.constant_slots as usize]))
        };
        let raw = buffers.each_ref().map(identity);
        unsafe {
            visit_constants(
                capture,
                context,
                &raw,
                (1 << plan.constant_slots) - 1,
                lighting,
                |item| {
                    row.constants[row.constant_count as usize] = item;
                    row.constant_count += 1;
                },
            )
        }?;
        row.queued = queued_draw();
    }
    if rsf_ac7_capture_record_before_draw(capture, &plan, &row) {
        Ok(())
    } else {
        Err(fmt::Error)
    }
}
