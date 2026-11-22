// SPDX-License-Identifier: MIT
use super::{Geometry, GeometryStream, GeometryVsBuffer, record_geometry};
use crate::motion_capture_observations::geometry::{
    geometry_read, plan_geometry,
};
use crate::motion_capture_records::{
    draws::gpu::{queued_draw, tick},
    geometry_scope, store_blob,
};
use crate::motion_capture_session::{
    CaptureInterval, rsf_ac7_capture_fail, rsf_ac7_capture_sample,
};
use crate::sys::rsf_frame_tap_geometry;
use std::fmt;
use windows::Win32::Graphics::Direct3D::D3D11_SRV_DIMENSION_BUFFEREX;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{BOOL, Interface};

const MAX_SAMPLE_BYTES: u32 = 16384;

struct Predication<'a> {
    context: &'a ID3D11DeviceContext,
    predicate: Option<ID3D11Predicate>,
    value: BOOL,
}

impl<'a> Predication<'a> {
    fn disable(context: &'a ID3D11DeviceContext) -> Self {
        let mut predicate = None;
        let mut value = BOOL::default();
        unsafe {
            context.GetPredication(Some(&mut predicate), Some(&mut value));
            context.SetPredication(None::<&ID3D11Predicate>, false);
        }
        Self {
            context,
            predicate,
            value,
        }
    }
}

impl Drop for Predication<'_> {
    fn drop(&mut self) {
        unsafe {
            self.context
                .SetPredication(self.predicate.as_ref(), self.value.as_bool())
        };
    }
}

struct Mapped<'a> {
    context: &'a ID3D11DeviceContext,
    buffer: &'a ID3D11Buffer,
}

impl Drop for Mapped<'_> {
    fn drop(&mut self) {
        unsafe { self.context.Unmap(self.buffer, 0) };
    }
}

fn buffer_desc(buffer: &ID3D11Buffer) -> D3D11_BUFFER_DESC {
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { buffer.GetDesc(&mut desc) };
    desc
}

fn read_buffer(
    capture: CaptureInterval,
    bucket: u32,
    context: &ID3D11DeviceContext,
    device: &ID3D11Device,
    buffer: &ID3D11Buffer,
    offset: u32,
    bytes: u32,
) -> Result<(u32, u32), fmt::Error> {
    if bytes == 0 || !geometry_read(capture, bucket, true) {
        return Ok((0, 0));
    }
    let staging = D3D11_BUFFER_DESC {
        ByteWidth: bytes,
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..Default::default()
    };
    let mut readable = None;
    let _ = unsafe { device.CreateBuffer(&staging, None, Some(&mut readable)) };
    let Some(readable) = readable else {
        return Ok((0, 0));
    };
    let area = D3D11_BOX {
        left: offset,
        right: offset + bytes,
        bottom: 1,
        back: 1,
        ..Default::default()
    };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context.CopySubresourceRegion(&readable, 0, 0, 0, 0, buffer, 0, Some(&area));
        if context
            .Map(&readable, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .is_err()
        {
            return Ok((0, 0));
        }
    }
    let _unmap = Mapped {
        context,
        buffer: &readable,
    };
    // The successful immediate-context map lends this D3D buffer range until Unmap.
    let data = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), bytes as usize) };
    Ok((bytes, store_blob(capture, data).ok_or(fmt::Error)?))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_motion_capture_geometry(draw: *const rsf_frame_tap_geometry) {
    // SAFETY: The frame tap lends the draw record and its bound objects to this callback.
    let Some(draw) = (unsafe { draw.as_ref() }) else {
        return;
    };
    let capture = rsf_ac7_capture_sample();
    if capture.active() && unsafe { capture_geometry(capture, draw) }.is_err() {
        rsf_ac7_capture_fail(capture);
    }
}

unsafe fn capture_geometry(capture: CaptureInterval, draw: &rsf_frame_tap_geometry) -> fmt::Result {
    let Some(plan) = plan_geometry(
        capture,
        draw.format,
        !draw.depth_view.is_null(),
        draw.input_layout,
        draw.vertex_shader,
    )?
    else {
        return Ok(());
    };
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&draw.context) }) else {
        return Ok(());
    };
    if unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return Ok(());
    }
    let Ok(device) = (unsafe { context.GetDevice() }) else {
        return Ok(());
    };
    let mut row = Geometry {
        scope: geometry_scope(),
        qpc: tick(),
        layout: draw.input_layout.addr() as u64,
        kind: draw.kind,
        count: draw.count,
        start: draw.start,
        base_vertex: draw.base_vertex,
        instances: draw.instances,
        start_instance: draw.start_instance,
        ..Default::default()
    };
    let can_read = || geometry_read(capture, plan.bucket, false);
    let _predication = Predication::disable(context);
    for (slot, raw) in draw.vertex_buffers.iter().enumerate() {
        if plan.ia_slots & (1 << slot) == 0 {
            continue;
        }
        if !can_read() {
            break;
        }
        let Some(buffer) = (unsafe { ID3D11Buffer::from_raw_borrowed(raw) }) else {
            continue;
        };
        let desc = buffer_desc(buffer);
        let offset = draw.offsets[slot];
        if offset >= desc.ByteWidth {
            continue;
        }
        let bytes = (desc.ByteWidth - offset).min(MAX_SAMPLE_BYTES);
        let (_, blob) = read_buffer(
            capture,
            plan.bucket,
            context,
            &device,
            buffer,
            offset,
            bytes,
        )?;
        row.streams[row.stream_count as usize] = GeometryStream {
            buffer: raw.addr() as u64,
            slot: slot as u32,
            stride: draw.strides[slot],
            offset,
            source_bytes: desc.ByteWidth,
            sample_bytes: bytes,
            blob,
        };
        row.stream_count += 1;
    }
    for &slot in &plan.vs_slots[..plan.vs_slot_count as usize] {
        if !can_read() {
            continue;
        }
        let mut views = [None];
        unsafe { context.VSGetShaderResources(slot, Some(&mut views)) };
        let [Some(view)] = views else { continue };
        let Ok(buffer) =
            unsafe { view.GetResource() }.and_then(|resource| resource.cast::<ID3D11Buffer>())
        else {
            continue;
        };
        let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        unsafe { view.GetDesc(&mut desc) };
        let width = buffer_desc(&buffer);
        let stride = match desc.Format {
            DXGI_FORMAT_R32G32B32A32_FLOAT => 16,
            DXGI_FORMAT_R32G32B32_FLOAT => 12,
            DXGI_FORMAT_R32G32_FLOAT => 8,
            DXGI_FORMAT_R32_FLOAT | DXGI_FORMAT_R32_TYPELESS => 4,
            _ => width.StructureByteStride,
        };
        let (element, elements) = unsafe {
            if desc.ViewDimension == D3D11_SRV_DIMENSION_BUFFEREX {
                let range = desc.Anonymous.BufferEx;
                (range.FirstElement, range.NumElements)
            } else {
                let range = desc.Anonymous.Buffer;
                (range.Anonymous1.FirstElement, range.Anonymous2.NumElements)
            }
        };
        let Some(offset) = element
            .checked_mul(stride)
            .filter(|&offset| offset <= width.ByteWidth)
        else {
            continue;
        };
        let Some(bytes) = elements.checked_mul(stride).filter(|_| stride != 0) else {
            continue;
        };
        let bytes = bytes.min(MAX_SAMPLE_BYTES).min(width.ByteWidth - offset);
        let (sample_bytes, blob) = read_buffer(
            capture,
            plan.bucket,
            context,
            &device,
            &buffer,
            offset,
            bytes,
        )?;
        row.vs_buffers[row.vs_buffer_count as usize] = GeometryVsBuffer {
            resource: buffer.as_raw().addr() as u64,
            slot,
            format: desc.Format.0 as u32,
            offset,
            source_bytes: width.ByteWidth,
            sample_bytes,
            blob,
        };
        row.vs_buffer_count += 1;
    }
    row.queued = queued_draw();
    if record_geometry(capture, &plan, &row) {
        Ok(())
    } else {
        Err(fmt::Error)
    }
}
