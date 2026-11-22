// SPDX-License-Identifier: MIT
//! Derives capture facts from the descriptors accepted by the native PSO calls.
use super::recordings::{Pipeline, hash_shader};
use std::mem::{align_of, offset_of, size_of};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::DXGI_SAMPLE_DESC};

fn blend_facts(blend: D3D12_RENDER_TARGET_BLEND_DESC) -> Pipeline {
    Pipeline {
        blend: blend.BlendEnable.0,
        src: blend.SrcBlend.0 as u32,
        dst: blend.DestBlend.0 as u32,
        op: blend.BlendOp.0 as u32,
        src_alpha: blend.SrcBlendAlpha.0 as u32,
        dst_alpha: blend.DestBlendAlpha.0 as u32,
        op_alpha: blend.BlendOpAlpha.0 as u32,
        write_mask: u32::from(blend.RenderTargetWriteMask),
        ..Pipeline::default()
    }
}

unsafe fn shader_facts(
    ps: D3D12_SHADER_BYTECODE,
    vs: D3D12_SHADER_BYTECODE,
    facts: &mut Pipeline,
) -> bool {
    unsafe {
        hash_shader(ps.pShaderBytecode.cast(), ps.BytecodeLength, &mut facts.ps)
            && hash_shader(vs.pShaderBytecode.cast(), vs.BytecodeLength, &mut facts.vs)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_graphics_facts(
    desc: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    out: &mut Pipeline,
) -> bool {
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return false;
    };
    *out = blend_facts(desc.BlendState.RenderTarget[0]);
    unsafe { shader_facts(desc.PS, desc.VS, out) }
}

// Each tagged payload has natural alignment and the stream advances in pointer-aligned steps.
#[repr(C)]
struct Tagged<T> {
    tag: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE,
    value: T,
}

const fn stride<T>() -> usize {
    size_of::<Tagged<T>>().next_multiple_of(align_of::<usize>())
}

/// Reads only the payload. Padding in a caller's native stream need not be initialized.
unsafe fn value<T: Copy>(item: *const u8, remaining: usize) -> Option<T> {
    if remaining < stride::<T>() {
        return None;
    }
    Some(unsafe {
        item.add(offset_of!(Tagged<T>, value))
            .cast::<T>()
            .read_unaligned()
    })
}

fn other_stride(tag: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE) -> Option<usize> {
    Some(match tag {
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS => stride::<D3D12_SHADER_BYTECODE>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE => stride::<usize>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT => stride::<D3D12_STREAM_OUTPUT_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER => stride::<D3D12_RASTERIZER_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL => stride::<D3D12_DEPTH_STENCIL_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1 => stride::<D3D12_DEPTH_STENCIL_DESC1>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT => stride::<D3D12_INPUT_LAYOUT_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS => {
            stride::<D3D12_RT_FORMAT_ARRAY>()
        }
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC => stride::<DXGI_SAMPLE_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO => stride::<D3D12_CACHED_PIPELINE_STATE>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING => {
            stride::<D3D12_VIEW_INSTANCING_DESC>()
        }
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS => stride::<u32>(),
        _ => return None,
    })
}

unsafe fn stream(desc: &D3D12_PIPELINE_STATE_STREAM_DESC) -> Option<Pipeline> {
    let data = desc.pPipelineStateSubobjectStream.cast::<u8>();
    if data.is_null() || desc.SizeInBytes > isize::MAX as usize {
        return None;
    }
    let mut vs = D3D12_SHADER_BYTECODE::default();
    let mut ps = D3D12_SHADER_BYTECODE::default();
    let mut facts = Pipeline::default();
    let mut offset = 0;
    while offset < desc.SizeInBytes {
        let remaining = desc.SizeInBytes - offset;
        if remaining < size_of::<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE>() {
            return None;
        }
        let item = unsafe { data.add(offset) };
        let tag = unsafe {
            item.cast::<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE>()
                .read_unaligned()
        };
        let size = match tag {
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS => {
                let shader = unsafe { value::<D3D12_SHADER_BYTECODE>(item, remaining) }?;
                match tag {
                    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS => vs = shader,
                    _ => ps = shader,
                }
                stride::<D3D12_SHADER_BYTECODE>()
            }
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND => {
                let blend = unsafe { value::<D3D12_BLEND_DESC>(item, remaining) }?;
                facts = blend_facts(blend.RenderTarget[0]);
                stride::<D3D12_BLEND_DESC>()
            }
            _ => other_stride(tag)?,
        };
        if size > remaining {
            return None;
        }
        offset += size;
    }
    unsafe { shader_facts(ps, vs, &mut facts) }.then_some(facts)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_stream_facts(
    desc: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    out: &mut Pipeline,
) -> bool {
    let Some(facts) = (unsafe { desc.as_ref() }).and_then(|desc| unsafe { stream(desc) }) else {
        return false;
    };
    *out = facts;
    true
}
