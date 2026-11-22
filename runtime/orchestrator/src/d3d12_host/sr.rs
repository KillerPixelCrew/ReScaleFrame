// SPDX-License-Identifier: MIT
use super::*;
use crate::sys::rsf_backend_log_fn;
use rsf_shader_compiler::{Compiler, Request, blob_bytes, compile};
use std::ffi::CString;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::{
    Direct3D::Fxc::{D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3},
    Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory4},
};

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_sr12_adapter_identity(
    device: Raw,
    vendor: *mut u32,
    software: *mut u32,
) -> u32 {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    let desc = unsafe {
        CreateDXGIFactory1::<IDXGIFactory4>()
            .and_then(|factory| factory.EnumAdapterByLuid::<IDXGIAdapter1>(device.GetAdapterLuid()))
            .and_then(|adapter| adapter.GetDesc1())
    };
    let Ok(desc) = desc else {
        return 0;
    };
    unsafe {
        vendor.write(desc.VendorId);
        software.write(u32::from(
            desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0,
        ));
    }
    1
}

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

fn normalization_shader(log: rsf_backend_log_fn, user: Raw) -> Option<Vec<u8>> {
    let Some(module) = Compiler::system("d3dcompiler_47.dll") else {
        if let Some(log) = log {
            unsafe {
                log(
                    user,
                    c"shader RSF temporal input normalization: d3dcompiler_47 is not available"
                        .as_ptr(),
                )
            };
        }
        return None;
    };
    let request = Request {
        source: include_bytes!("normalize.hlsl"),
        name: c"RSF temporal input normalization",
        entry: c"main",
        target: c"cs_5_0",
        flags: D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
    };
    let result = unsafe { compile(module.entry()?, request, &[], null_mut()) };
    if result.status >= 0 && result.code.is_some() {
        return result.code;
    }
    if let Some(log) = log {
        let status = if result.status < 0 {
            result.status
        } else {
            windows_sys::Win32::Foundation::E_FAIL
        };
        let errors = result.errors.as_deref().unwrap_or_default();
        let errors = errors.split(|byte| *byte == 0).next().unwrap_or_default();
        let mut message = format!(
            "shader RSF temporal input normalization (main): compile failed 0x{:08x} ",
            status as u32
        )
        .into_bytes();
        message.extend_from_slice(&errors[..errors.len().min(511 - message.len())]);
        if let Ok(message) = CString::new(message) {
            unsafe { log(user, message.as_ptr()) };
        }
    }
    None
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_sr12_create_pipeline(
    device: Raw,
    log: rsf_backend_log_fn,
    user: Raw,
    root_out: *mut Raw,
    pipeline_out: *mut Raw,
) -> i32 {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    let Some(root) = create_root(device) else {
        return 0;
    };
    let Some(code) = normalization_shader(log, user) else {
        return 0;
    };
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
    let Some(pipeline) = receive::<ID3D12PipelineState>(|iid, out| unsafe {
        (device.vtable().CreateComputePipelineState)(device.as_raw(), &desc, iid, out)
    }) else {
        return 0;
    };
    unsafe {
        root_out.write(root.into_raw());
        pipeline_out.write(pipeline.into_raw());
    }
    1
}

#[unsafe(no_mangle)]
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

#[unsafe(no_mangle)]
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

#[unsafe(no_mangle)]
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

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_sr12_create_readback(device: Raw, bytes: u64) -> Raw {
    buffer(
        unsafe { borrow(&device) },
        bytes,
        D3D12_HEAP_TYPE_READBACK,
        D3D12_RESOURCE_STATE_COPY_DEST,
    )
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_sr12_owned_by(child: Raw, device: Raw) -> i32 {
    i32::from(owned_by(unsafe { borrow(&child) }, device))
}
