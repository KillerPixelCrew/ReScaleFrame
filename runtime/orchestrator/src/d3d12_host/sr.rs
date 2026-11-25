// SPDX-License-Identifier: MIT
use super::*;
use crate::sys::rsf_backend_log_fn;
use rsf_shader_compiler::{Compiler, Request, blob_bytes, compile};
use std::ffi::{CStr, CString};
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};

fn create_root(device: &ID3D12Device) -> Option<ID3D12RootSignature> {
    let ranges = [
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 4,
            ..Default::default()
        },
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
            NumDescriptors: 4,
            OffsetInDescriptorsFromTableStart: 4,
            ..Default::default()
        },
    ];
    let parameters = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 2,
                    pDescriptorRanges: ranges.as_ptr(),
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Constants: D3D12_ROOT_CONSTANTS {
                    ShaderRegister: 0,
                    RegisterSpace: 0,
                    Num32BitValues: 6,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
    ];
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: 2,
        pParameters: parameters.as_ptr(),
        ..Default::default()
    };
    let mut blob = None;
    let mut errors = None;
    unsafe {
        D3D12SerializeRootSignature(
            &desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut blob,
            Some(&mut errors),
        )
    }
    .ok()?;
    let code = blob_bytes(blob.as_ref()?);
    receive(|iid, out| unsafe {
        (device.vtable().CreateRootSignature)(
            device.as_raw(),
            0,
            code.as_ptr().cast(),
            code.len(),
            iid,
            out,
        )
    })
}

// The sr12 compute kernels, each compiled from its source with the System32 compiler.
const NORMALIZATION: (&[u8], &CStr) = (
    include_bytes!("normalize.hlsl"),
    c"RSF temporal input normalization",
);
const SPATIAL: (&[u8], &CStr) = (
    include_bytes!("../sr12/easu.hlsl"),
    c"RSF spatial upscaling (FSR1 EASU)",
);

fn compute_shader(
    (source, name): (&[u8], &CStr),
    log: rsf_backend_log_fn,
    user: Raw,
) -> Option<Vec<u8>> {
    let say = |message: String| {
        if let (Some(log), Ok(message)) = (log, CString::new(message)) {
            unsafe { log(user, message.as_ptr()) };
        }
    };
    let Some(module) = Compiler::system("d3dcompiler_47.dll") else {
        say(format!(
            "shader {}: d3dcompiler_47 is not available",
            name.to_string_lossy()
        ));
        return None;
    };
    let request = Request {
        source,
        name,
        entry: c"main",
        target: c"cs_5_0",
        flags: D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
    };
    let result = unsafe { compile(module.entry()?, request, &[], null_mut()) };
    if result.status >= 0 && result.code.is_some() {
        return result.code;
    }
    let status = if result.status < 0 {
        result.status
    } else {
        windows_sys::Win32::Foundation::E_FAIL
    };
    let errors = result.errors.as_deref().unwrap_or_default();
    let errors = errors.split(|byte| *byte == 0).next().unwrap_or_default();
    let mut message = format!(
        "shader {} (main): compile failed 0x{:08x} ",
        name.to_string_lossy(),
        status as u32
    )
    .into_bytes();
    message.extend_from_slice(&errors[..errors.len().min(511 - message.len())]);
    say(String::from_utf8_lossy(&message).into_owned());
    None
}

fn compute_pipeline(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    code: &[u8],
) -> Option<ID3D12PipelineState> {
    let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
        pRootSignature: ManuallyDrop::new(Some(unsafe {
            ID3D12RootSignature::from_raw(root.as_raw())
        })),
        CS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: code.as_ptr().cast(),
            BytecodeLength: code.len(),
        },
        ..Default::default()
    };
    receive::<ID3D12PipelineState>(|iid, out| unsafe {
        (device.vtable().CreateComputePipelineState)(device.as_raw(), &desc, iid, out)
    })
}

// The normalization root signature and pipeline, as owned references.
pub(crate) unsafe fn rsf_core_sr12_create_pipeline(
    device: Raw,
    log: rsf_backend_log_fn,
    user: Raw,
) -> Option<(Raw, Raw)> {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    let root = create_root(device)?;
    let code = compute_shader(NORMALIZATION, log, user)?;
    let pipeline = compute_pipeline(device, &root, &code)?;
    Some((root.into_raw(), pipeline.into_raw()))
}

// The spatial upscaling pipeline on the normalization root signature, as an owned reference.
pub(crate) unsafe fn rsf_core_sr12_create_spatial_pipeline(
    device: Raw,
    root: Raw,
    log: rsf_backend_log_fn,
    user: Raw,
) -> Raw {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    compute_shader(SPATIAL, log, user)
        .and_then(|code| compute_pipeline(device, unsafe { borrow(&root) }, &code))
        .map_or(null_mut(), Interface::into_raw)
}

pub(crate) unsafe extern "C" fn rsf_core_sr12_create_heap(device: Raw) -> Raw {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    let desc = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        NumDescriptors: 8,
        Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
        NodeMask: 0,
    };
    receive::<ID3D12DescriptorHeap>(|iid, out| unsafe {
        (device.vtable().CreateDescriptorHeap)(device.as_raw(), &desc, iid, out)
    })
    .map_or(null_mut(), Interface::into_raw)
}

pub(crate) unsafe extern "C" fn rsf_core_sr12_create_exposure_texture(device: Raw) -> Raw {
    let desc = texture_desc(1, 1, DXGI_FORMAT_R32_FLOAT);
    committed_resource(
        unsafe { borrow(&device) },
        D3D12_HEAP_TYPE_DEFAULT,
        &desc,
        D3D12_RESOURCE_STATE_COPY_DEST,
    )
}

fn buffer(
    device: &ID3D12Device,
    bytes: u64,
    heap: D3D12_HEAP_TYPE,
    state: D3D12_RESOURCE_STATES,
) -> Raw {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    committed_resource(device, heap, &desc, state)
}

pub(crate) unsafe extern "C" fn rsf_core_sr12_create_exposure_upload(
    device: Raw,
    bytes: u64,
) -> Raw {
    buffer(
        unsafe { borrow(&device) },
        bytes,
        D3D12_HEAP_TYPE_UPLOAD,
        D3D12_RESOURCE_STATE_GENERIC_READ,
    )
}

pub(crate) unsafe extern "C" fn rsf_core_sr12_create_readback(device: Raw, bytes: u64) -> Raw {
    buffer(
        unsafe { borrow(&device) },
        bytes,
        D3D12_HEAP_TYPE_READBACK,
        D3D12_RESOURCE_STATE_COPY_DEST,
    )
}

pub(crate) unsafe extern "C" fn rsf_core_sr12_owned_by(child: Raw, device: Raw) -> i32 {
    i32::from(owned_by(unsafe { borrow(&child) }, device))
}
