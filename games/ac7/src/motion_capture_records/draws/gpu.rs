// SPDX-License-Identifier: MIT
//! Typed COM access for synchronous AC7 capture observations.

use super::{BoundConstant, BoundView, Dispatch, QueuedDraw, Resource};
use crate::capture_platform_abi::rsf_ac7_copy_memory;
use crate::motion_capture_graphics_abi::*;
use crate::motion_capture_observations::buffers::*;
use crate::motion_capture_observations::colour::{
    ColourSnapshot, rsf_ac7_capture_colour_snapshot, rsf_ac7_capture_colour_snapshot_available,
};
use crate::motion_capture_observations::rsf_ac7_capture_shader_facts;
use crate::motion_capture_records::{
    rsf_ac7_capture_queued_scope, rsf_ac7_capture_root_scope, store_blob,
};
use crate::motion_capture_session::{
    CaptureInterval, rsf_ac7_capture_fail, rsf_ac7_capture_sample,
};
use std::ffi::{c_char, c_void};
use std::fmt;
use std::ops::Deref;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_ac7_capture_current_command_address() -> *const c_void;
}

pub(super) fn tick() -> u64 {
    let mut ticks = 0;
    let _ = unsafe { QueryPerformanceCounter(&mut ticks) };
    ticks as u64
}

pub(super) fn queued_draw() -> QueuedDraw {
    let mut draw = QueuedDraw::default();
    let address = unsafe { rsf_ac7_capture_current_command_address() };
    let observed = read_u64(address, &mut draw.command)
        && draw.command != 0
        && read_u64(
            (draw.command as usize).wrapping_add(8) as *const c_void,
            &mut draw.execute,
        );
    draw.scope = rsf_ac7_capture_queued_scope(draw.command, draw.execute, observed);
    draw
}

pub(super) fn thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

pub(super) fn facts(resource: &ID3D11Resource) -> Resource {
    let dimension = unsafe { resource.GetType() };
    let raw = resource.as_raw();
    let mut facts = Resource {
        resource: resource.as_raw().addr() as u64,
        dimension: dimension.0 as u32,
        ..Default::default()
    };
    match dimension {
        D3D11_RESOURCE_DIMENSION_TEXTURE2D => {
            // GetType identified the concrete D3D11 resource. Borrow the same interface pointer
            // to retain the resource identity supplied by GetResource.
            let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { texture.GetDesc(&mut desc) };
            (facts.width, facts.height, facts.format) =
                (desc.Width, desc.Height, desc.Format.0 as u32);
        }
        D3D11_RESOURCE_DIMENSION_TEXTURE3D => {
            let texture = unsafe { ID3D11Texture3D::from_raw_borrowed(&raw) }.unwrap();
            let mut desc = D3D11_TEXTURE3D_DESC::default();
            unsafe { texture.GetDesc(&mut desc) };
            (facts.width, facts.height, facts.depth, facts.format) =
                (desc.Width, desc.Height, desc.Depth, desc.Format.0 as u32);
        }
        _ => {}
    }
    facts
}

pub(super) fn visit_views<T: Deref<Target = ID3D11View>>(
    views: impl IntoIterator<Item = Option<T>>,
    mut visit: impl FnMut(BoundView),
) {
    for (slot, view) in views.into_iter().enumerate() {
        if let Some(view) = view
            && let Ok(resource) = unsafe { view.GetResource() }
        {
            visit(BoundView {
                resource: facts(&resource),
                slot: slot as u32,
            });
        }
    }
}

pub(super) fn snapshot(
    capture: CaptureInterval,
    context: &ID3D11DeviceContext,
    resource: &ID3D11Resource,
    role: *const c_char,
    lighting: bool,
) -> Result<ColourSnapshot, fmt::Error> {
    let mut snapshot = ColourSnapshot::default();
    if role.is_null() || !rsf_ac7_capture_colour_snapshot_available(capture, lighting) {
        return Ok(snapshot);
    }
    let Ok(texture) = resource.cast::<ID3D11Texture2D>() else {
        return Ok(snapshot);
    };
    let mut input = (context, &texture);
    // The receipt writer invokes the dump synchronously while the context and texture are live.
    if unsafe {
        rsf_ac7_capture_colour_snapshot(
            capture,
            role,
            lighting,
            std::ptr::from_mut(&mut input).cast(),
            dump_colour,
            &mut snapshot,
        )
    } {
        Ok(snapshot)
    } else {
        Err(fmt::Error)
    }
}

unsafe extern "C" fn dump_colour(user: *mut c_void, path: *const c_char) -> i32 {
    let (context, texture) = unsafe { *user.cast::<(&ID3D11DeviceContext, &ID3D11Texture2D)>() };
    let options = rsf_texture_dump_options {
        struct_size: size_of::<rsf_texture_dump_options>() as u32,
        abi_version: RSF_TEXTURE_DUMP_ABI_VERSION,
        output_prefix_utf8: path,
        ..Default::default()
    };
    let Ok(device) = (unsafe { context.GetDevice() }) else {
        return -1;
    };
    unsafe {
        rsf_dump_texture_bytes(
            device.as_raw(),
            context.as_raw(),
            texture.as_raw(),
            &options,
        )
    }
}

unsafe extern "C" fn copy_upload(user: *mut c_void, bytes: *const u8, size: usize) -> bool {
    let out = unsafe { &mut *user.cast::<Vec<u8>>() };
    if out.try_reserve_exact(size).is_err() {
        return false;
    }
    let data = unsafe { std::slice::from_raw_parts(bytes, size) };
    out.extend_from_slice(data);
    true
}

pub(super) unsafe fn visit_constants(
    capture: CaptureInterval,
    context: &ID3D11DeviceContext,
    buffers: &[*mut c_void],
    mask: u32,
    lighting: bool,
    mut visit: impl FnMut(BoundConstant),
) -> fmt::Result {
    for (slot, raw) in buffers.iter().enumerate() {
        if mask & (1 << slot) == 0 {
            continue;
        }
        let Some(buffer) = (unsafe { ID3D11Buffer::from_raw_borrowed(raw) }) else {
            continue;
        };
        let mut bytes = Vec::<u8>::new();
        let mut record = ConstantRecord::default();
        let source = unsafe {
            rsf_ac7_capture_constant_source(
                capture,
                *raw,
                lighting,
                &mut record,
                std::ptr::from_mut(&mut bytes).cast(),
                copy_upload,
            )
        };
        let mut readback = false;
        match source {
            ConstantSource::Stopped => break,
            ConstantSource::Failed => return Err(fmt::Error),
            ConstantSource::TryRead => {
                let mut desc = D3D11_BUFFER_DESC::default();
                unsafe { buffer.GetDesc(&mut desc) };
                match rsf_ac7_capture_reserve_constant_read(capture, desc.ByteWidth, lighting) {
                    ConstantRead::Stopped => break,
                    ConstantRead::Skipped => {}
                    ConstantRead::Reserved => {
                        readback = true;
                        bytes
                            .try_reserve_exact(desc.ByteWidth as usize)
                            .map_err(|_| fmt::Error)?;
                        bytes.resize(desc.ByteWidth as usize, 0);
                        let device = unsafe { context.GetDevice() };
                        let copied = device.is_ok_and(|device| unsafe {
                            rsf_read_constant_buffer(
                                device.as_raw(),
                                context.as_raw(),
                                *raw,
                                bytes.as_mut_ptr().cast(),
                                desc.ByteWidth,
                            ) == 0
                        });
                        if !copied {
                            bytes.clear();
                            rsf_ac7_capture_constant_read_failed(capture);
                        }
                    }
                }
            }
            ConstantSource::Missing | ConstantSource::Uploaded => {}
        }
        visit(BoundConstant {
            buffer: record.buffer,
            upload_serial: record.upload_serial,
            slot: slot as u32,
            blob: store_blob(capture, &bytes).ok_or(fmt::Error)?,
            readback: u32::from(readback),
        });
    }
    Ok(())
}

pub(super) fn identity<T: Interface>(object: &Option<T>) -> *mut c_void {
    object.as_ref().map_or(null_mut(), Interface::as_raw)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_set_sample(enabled: bool) {
    unsafe {
        if enabled {
            rsf_frame_tap_set_research_phase_callbacks(
                Some(super::before::observe),
                Some(super::ordinary::observe),
                Some(dispatch),
                null_mut(),
            );
        } else {
            rsf_frame_tap_set_research_callbacks(None, None, null_mut());
        }
    }
}

unsafe extern "C" fn dispatch(
    _: *mut c_void,
    context: *mut c_void,
    x: u32,
    y: u32,
    z: u32,
    indirect: *mut c_void,
    offset: u32,
) {
    let capture = rsf_ac7_capture_sample();
    if !capture.active() || !super::rsf_ac7_capture_can_record_draw(capture) {
        return;
    }
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return;
    };
    let mut shader = None;
    unsafe { context.CSGetShader(&mut shader, None, None) };
    let shader = rsf_ac7_capture_shader_facts(identity(&shader), 5);
    let mut row = Dispatch {
        root: rsf_ac7_capture_root_scope(),
        queued: QueuedDraw::default(),
        qpc: tick(),
        indirect: indirect.addr() as u64,
        thread: thread_id(),
        groups: [x, y, z],
        offset,
        cs_hash: shader.hash,
        input_count: 0,
        output_count: 0,
        constant_count: 0,
        inputs: [BoundView::default(); 128],
        outputs: [BoundView::default(); 8],
        constants: [BoundConstant::default(); 14],
    };
    let mut inputs = [const { None }; 128];
    let mut outputs = [const { None }; 8];
    let mut constants = [const { None }; 14];
    unsafe {
        context.CSGetShaderResources(0, Some(&mut inputs));
        context.CSGetUnorderedAccessViews(0, Some(&mut outputs));
        context.CSGetConstantBuffers(0, Some(&mut constants));
    }
    visit_views(inputs, |view| {
        row.inputs[row.input_count as usize] = view;
        row.input_count += 1;
    });
    visit_views(outputs, |view| {
        row.outputs[row.output_count as usize] = view;
        row.output_count += 1;
    });
    let raw = std::array::from_fn::<_, 14, _>(|slot| identity(&constants[slot]));
    let result = unsafe {
        visit_constants(
            capture,
            context,
            &raw,
            shader.constant_mask,
            false,
            |item| {
                row.constants[row.constant_count as usize] = item;
                row.constant_count += 1;
            },
        )
    };
    row.queued = queued_draw();
    if result.is_err() || !super::rsf_ac7_capture_record_dispatch(capture, &row) {
        rsf_ac7_capture_fail(capture);
    }
}

fn read_u64(source: *const c_void, value: &mut u64) -> bool {
    !source.is_null() && unsafe { rsf_ac7_copy_memory(std::ptr::from_mut(value).cast(), source, 8) }
}
