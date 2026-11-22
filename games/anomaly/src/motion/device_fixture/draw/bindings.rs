// SPDX-License-Identifier: MIT
use super::Raw;
use crate::ffi::borrow_span;
use crate::sys::rsf_anomaly_motion_fixture_override as Override;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
use windows::Win32::Graphics::Direct3D::{
    D3D_SIT_CBUFFER, D3D_SIT_SAMPLER, D3D_SIT_TEXTURE, D3D_SVC_MATRIX_COLUMNS, D3D_SVC_MATRIX_ROWS,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32G32B32A32_FLOAT;
use windows::core::Interface;

#[derive(Default)]
pub(in crate::motion::device_fixture) struct Bindings {
    buffers: Vec<ID3D11Buffer>,
    views: Vec<ID3D11ShaderResourceView>,
    samplers: Vec<ID3D11SamplerState>,
}

#[unsafe(no_mangle)]
pub(in crate::motion::device_fixture) extern "C" fn rsf_anomaly_motion_fixture_bindings_new()
-> *mut Bindings {
    Box::into_raw(Box::new(Bindings::default()))
}

#[unsafe(no_mangle)]
pub(in crate::motion::device_fixture) unsafe extern "C" fn rsf_anomaly_motion_fixture_bindings_drop(
    handle: *mut Bindings,
) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

struct Stage<'a> {
    device: &'a ID3D11Device,
    context: &'a ID3D11DeviceContext,
    index: u8,
    tessellated: bool,
    core: bool,
}

impl Stage<'_> {
    fn buffer(&self, values: &[f32]) -> Option<ID3D11Buffer> {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(std::mem::size_of_val(values)).ok()?,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: values.as_ptr().cast(),
            ..Default::default()
        };
        let mut buffer = None;
        unsafe {
            self.device
                .CreateBuffer(&desc, Some(&data), Some(&mut buffer))
        }
        .ok()?;
        buffer
    }

    fn texture(&self, name: &[u8]) -> Result<ID3D11ShaderResourceView, &'static CStr> {
        let mut ramp = [0.; 64];
        let solid = [0.5, 0.4, 0.6, 1.].repeat(4);
        let (width, texels) = if self.tessellated && name == b"s_tbumpX" {
            for (i, texel) in ramp.chunks_exact_mut(4).enumerate() {
                texel.copy_from_slice(&[0.5, 0.5, 1., 0.15 + 0.05 * i as f32]);
            }
            (4, ramp.as_slice())
        } else {
            (2, solid.as_slice())
        };
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: width,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: texels.as_ptr().cast(),
            SysMemPitch: width * 16,
            ..Default::default()
        };
        let mut texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, Some(&init), Some(&mut texture))
        }
        .map_err(|_| c"texture failed")?;
        let texture = texture.ok_or(c"texture failed")?;
        let mut view = None;
        unsafe {
            self.device
                .CreateShaderResourceView(&texture, None, Some(&mut view))
        }
        .map_err(|_| c"srv failed")?;
        view.ok_or(c"srv failed")
    }

    fn sampler(&self) -> Option<ID3D11SamplerState> {
        let address = if self.core {
            D3D11_TEXTURE_ADDRESS_CLAMP
        } else {
            D3D11_TEXTURE_ADDRESS_WRAP
        };
        let desc = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: address,
            AddressV: address,
            AddressW: address,
            MaxLOD: D3D11_FLOAT32_MAX,
            ..Default::default()
        };
        let mut sampler = None;
        unsafe { self.device.CreateSamplerState(&desc, Some(&mut sampler)) }.ok()?;
        sampler
    }
}

fn constants(
    buffer: &ID3D11ShaderReflectionConstantBuffer,
    overrides: &[Override],
    tessellated: bool,
) -> Option<Vec<f32>> {
    let mut desc = D3D11_SHADER_BUFFER_DESC::default();
    unsafe { buffer.GetDesc(&mut desc) }.ok()?;
    let mut values = vec![0.1; (desc.Size as usize).div_ceil(16) * 4];
    for index in 0..desc.Variables {
        let variable = unsafe { buffer.GetVariableByIndex(index) }?;
        let mut var = D3D11_SHADER_VARIABLE_DESC::default();
        unsafe { variable.GetDesc(&mut var) }.ok()?;
        let mut kind = D3D11_SHADER_TYPE_DESC::default();
        let type_desc = unsafe { variable.GetType() }?;
        unsafe { type_desc.GetDesc(&mut kind) }.ok()?;
        if var.Name.is_null() {
            return None;
        }
        let name = unsafe { CStr::from_ptr(var.Name.0.cast()) }.to_bytes();
        let start = (var.StartOffset / 4) as usize;
        let end = start.checked_add((var.Size / 4) as usize)?;
        let words = values.get_mut(start..end)?;
        if kind.Class == D3D_SVC_MATRIX_COLUMNS || kind.Class == D3D_SVC_MATRIX_ROWS {
            for (k, word) in words.iter_mut().enumerate() {
                *word = if k % 5 == 0 { 1. } else { 0. };
            }
        }
        if !tessellated && matches!(name, b"def_aref" | b"alpha_ref") {
            *words.first_mut()? = 0.5;
        }
        if let Some(value) = overrides.iter().find(|value| {
            !value.name.is_null() && unsafe { CStr::from_ptr(value.name) }.to_bytes() == name
        }) {
            let slice = unsafe { borrow_span(value.values, value.count) };
            let count = words.len().min(slice.len());
            words[..count].copy_from_slice(&slice[..count]);
        }
    }
    Some(values)
}

fn bind(
    keep: &mut Bindings,
    stage: &Stage<'_>,
    reflection: &ID3D11ShaderReflection,
    overrides: &[Override],
) -> Result<(), &'static CStr> {
    let mut desc = D3D11_SHADER_DESC::default();
    unsafe { reflection.GetDesc(&mut desc) }.map_err(|_| c"reflect failed")?;
    let sdk = stage.context.vtable();
    let context = stage.context.as_raw();
    for i in 0..desc.BoundResources {
        let mut binding = D3D11_SHADER_INPUT_BIND_DESC::default();
        unsafe { reflection.GetResourceBindingDesc(i, &mut binding) }
            .map_err(|_| c"reflect failed")?;
        if binding.Name.is_null() {
            return Err(c"reflect failed");
        }
        let name = unsafe { CStr::from_ptr(binding.Name.0.cast()) }.to_bytes();
        let slot = binding.BindPoint;
        let resources = !stage.core || stage.index == b'p';
        if binding.Type == D3D_SIT_CBUFFER && !name.starts_with(b"RSF") {
            let layout = unsafe { reflection.GetConstantBufferByName(binding.Name) }
                .ok_or(c"reflect failed")?;
            let values =
                constants(&layout, overrides, stage.tessellated).ok_or(c"reflect failed")?;
            let buffer = stage.buffer(&values).ok_or(c"engine cb failed")?;
            let set = match stage.index {
                b'v' => sdk.VSSetConstantBuffers,
                b'h' => sdk.HSSetConstantBuffers,
                b'd' => sdk.DSSetConstantBuffers,
                _ => sdk.PSSetConstantBuffers,
            };
            unsafe { set(context, slot, 1, &buffer.as_raw()) };
            keep.buffers.push(buffer);
        } else if resources && binding.Type == D3D_SIT_TEXTURE {
            let view = stage.texture(name)?;
            let set = match stage.index {
                b'v' => sdk.VSSetShaderResources,
                b'h' => sdk.HSSetShaderResources,
                b'd' => sdk.DSSetShaderResources,
                _ => sdk.PSSetShaderResources,
            };
            unsafe { set(context, slot, 1, &view.as_raw()) };
            keep.views.push(view);
        } else if resources && binding.Type == D3D_SIT_SAMPLER {
            let sampler = stage.sampler().ok_or(c"sampler failed")?;
            let set = match stage.index {
                b'v' => sdk.VSSetSamplers,
                b'h' => sdk.HSSetSamplers,
                b'd' => sdk.DSSetSamplers,
                _ => sdk.PSSetSamplers,
            };
            unsafe { set(context, slot, 1, &sampler.as_raw()) };
            keep.samplers.push(sampler);
        }
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub(in crate::motion::device_fixture) unsafe extern "C" fn rsf_anomaly_motion_fixture_bind_stage(
    keep: &mut Bindings,
    device: Raw,
    context: Raw,
    index: u8,
    tessellated: bool,
    code: *const c_void,
    code_size: usize,
    overrides: *const Override,
    override_count: usize,
) -> *const c_char {
    let (Some(device), Some(context)) = (unsafe {
        (
            ID3D11Device::from_raw_borrowed(&device),
            ID3D11DeviceContext::from_raw_borrowed(&context),
        )
    }) else {
        return c"missing device or context".as_ptr();
    };
    let mut raw = null_mut();
    let status = unsafe { D3DReflect(code, code_size, &ID3D11ShaderReflection::IID, &mut raw) };
    let reflection = (!raw.is_null()).then(|| unsafe { ID3D11ShaderReflection::from_raw(raw) });
    let Some(reflection) = reflection.filter(|_| status.is_ok()) else {
        return c"reflect failed".as_ptr();
    };
    let stage = Stage {
        device,
        context,
        index,
        tessellated,
        core: false,
    };
    let overrides = unsafe { borrow_span(overrides, override_count) };
    bind(keep, &stage, &reflection, overrides)
        .err()
        .map_or(null(), CStr::as_ptr)
}

#[unsafe(no_mangle)]
pub(in crate::motion::device_fixture) unsafe extern "C" fn rsf_anomaly_motion_fixture_bind_core(
    keep: &mut Bindings,
    device: Raw,
    context: Raw,
    reflection: Raw,
    vertex: bool,
) -> *const c_char {
    let (Some(device), Some(context), Some(reflection)) = (unsafe {
        (
            ID3D11Device::from_raw_borrowed(&device),
            ID3D11DeviceContext::from_raw_borrowed(&context),
            ID3D11ShaderReflection::from_raw_borrowed(&reflection),
        )
    }) else {
        return c"missing core fixture bindings".as_ptr();
    };
    let stage = Stage {
        device,
        context,
        index: if vertex { b'v' } else { b'p' },
        tessellated: false,
        core: true,
    };
    bind(keep, &stage, reflection, &[])
        .err()
        .map_or(null(), CStr::as_ptr)
}

fn staged_constants(
    stage: &Stage<'_>,
    slot: u32,
) -> Result<(Vec<f32>, ID3D11Buffer), &'static CStr> {
    let mut current = [None];
    unsafe { stage.context.VSGetConstantBuffers(slot, Some(&mut current)) };
    let [current] = current;
    let current = current.ok_or(c"engine constant buffer bound")?;
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { current.GetDesc(&mut desc) };
    let mut values = vec![0.0f32; desc.ByteWidth as usize / 4];
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    unsafe { stage.device.CreateBuffer(&desc, None, Some(&mut staging)) }
        .map_err(|_| c"staging failed")?;
    let staging = staging.ok_or(c"staging failed")?;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        stage.context.CopyResource(&staging, &current);
        stage
            .context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|_| c"map cb failed")?;
        if !mapped.pData.is_null() {
            std::ptr::copy_nonoverlapping(
                mapped.pData.cast::<f32>(),
                values.as_mut_ptr(),
                values.len(),
            );
        }
        stage.context.Unmap(&staging, 0);
    }
    if mapped.pData.is_null() {
        return Err(c"map cb failed");
    }
    Ok((values, staging))
}

fn patch_core(
    keep: &mut Bindings,
    stage: &Stage<'_>,
    reflection: &ID3D11ShaderReflection,
    overrides: &[Override],
) -> Result<(), &'static CStr> {
    let mut desc = D3D11_SHADER_DESC::default();
    unsafe { reflection.GetDesc(&mut desc) }.map_err(|_| c"reflect failed")?;
    for i in 0..desc.BoundResources {
        let mut binding = D3D11_SHADER_INPUT_BIND_DESC::default();
        unsafe { reflection.GetResourceBindingDesc(i, &mut binding) }
            .map_err(|_| c"reflect failed")?;
        if binding.Type != D3D_SIT_CBUFFER {
            continue;
        }
        if binding.Name.is_null() {
            return Err(c"reflect failed");
        }
        if unsafe { CStr::from_ptr(binding.Name.0.cast()) }
            .to_bytes()
            .starts_with(b"RSF")
        {
            continue;
        }
        let cb =
            unsafe { reflection.GetConstantBufferByName(binding.Name) }.ok_or(c"reflect failed")?;
        let mut desc = D3D11_SHADER_BUFFER_DESC::default();
        unsafe { cb.GetDesc(&mut desc) }.map_err(|_| c"reflect failed")?;
        let (mut values, _staging) = staged_constants(stage, binding.BindPoint)?;
        for index in 0..desc.Variables {
            let variable = unsafe { cb.GetVariableByIndex(index) }.ok_or(c"reflect failed")?;
            let mut var = D3D11_SHADER_VARIABLE_DESC::default();
            unsafe { variable.GetDesc(&mut var) }.map_err(|_| c"reflect failed")?;
            if var.Name.is_null() {
                return Err(c"reflect failed");
            }
            let name = unsafe { CStr::from_ptr(var.Name.0.cast()) };
            let Some(value) = overrides
                .iter()
                .find(|row| !row.name.is_null() && unsafe { CStr::from_ptr(row.name) } == name)
            else {
                continue;
            };
            let offset = var.StartOffset as usize / 4;
            if value.count > var.Size as usize / 4 {
                return Err(c"override exceeds reflected variable");
            }
            let words = values
                .get_mut(offset..offset + value.count)
                .ok_or(c"override exceeds constant buffer")?;
            words.copy_from_slice(unsafe { borrow_span(value.values, value.count) });
        }
        let buffer = stage.buffer(&values).ok_or(c"patched cb failed")?;
        unsafe {
            (stage.context.vtable().VSSetConstantBuffers)(
                stage.context.as_raw(),
                binding.BindPoint,
                1,
                &buffer.as_raw(),
            );
        }
        keep.buffers.push(buffer);
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub(in crate::motion::device_fixture) unsafe extern "C" fn rsf_anomaly_motion_fixture_patch_core(
    keep: &mut Bindings,
    device: Raw,
    context: Raw,
    reflection: Raw,
    overrides: *const Override,
    override_count: usize,
) -> *const c_char {
    let (Some(device), Some(context), Some(reflection)) = (unsafe {
        (
            ID3D11Device::from_raw_borrowed(&device),
            ID3D11DeviceContext::from_raw_borrowed(&context),
            ID3D11ShaderReflection::from_raw_borrowed(&reflection),
        )
    }) else {
        return c"missing core fixture bindings".as_ptr();
    };
    let stage = Stage {
        device,
        context,
        index: b'v',
        tessellated: false,
        core: true,
    };
    patch_core(keep, &stage, reflection, unsafe {
        borrow_span(overrides, override_count)
    })
    .err()
    .map_or(null(), CStr::as_ptr)
}
