// SPDX-License-Identifier: MIT
use crate::d3d11_output_abi::rsf_d3d11_output_srv_format;
use crate::d3d11_sharpening::{Frame, rsf_sharpen11_active, rsf_sharpen11_pack};
use crate::render_abi::rsf_sharpening_options as Options;
use crate::sharpening_policy::{FILTERS, effective_mode};
use rsf_shader_compiler::{Compiler, Request, blob_bytes, compile};
use std::ffi::{CStr, c_char, c_void};
use std::mem::ManuallyDrop;
use std::ptr;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCOMPILE_ENABLE_STRICTNESS;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

const SLOT_COUNT: usize = 3;
const GROUP_EDGE: u32 = 16;
const CONSTANT_WORDS: usize = 32;

unsafe extern "C" {
    fn rsf_sharpening_hlsl(mode: u32) -> *const c_char;
    #[cfg_attr(test, allow(dead_code))]
    fn rsf_sharpening_requested(options: &Options) -> u32;
}

#[derive(Default)]
struct Slot {
    scratch: Option<ID3D12Resource>,
    heap: Option<ID3D12DescriptorHeap>,
}

#[derive(Default)]
pub(super) struct Sharpen12 {
    slots: [Slot; SLOT_COUNT],
    pipelines: [Option<ID3D12PipelineState>; FILTERS.len()],
    root: Option<ID3D12RootSignature>,
    device: Option<ID3D12Device>,
    dispatches: u64,
}

fn device_of(child: &ID3D12DeviceChild) -> Option<ID3D12Device> {
    let mut device = None;
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}

fn shader_readable(desc: &D3D12_RESOURCE_DESC) -> bool {
    desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && desc.DepthOrArraySize == 1
        && desc.MipLevels == 1
        && desc.SampleDesc.Count == 1
        && (1..=u64::from(u32::MAX)).contains(&desc.Width)
        && desc.Height != 0
        && !desc
            .Flags
            .contains(D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE)
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
            NumDescriptors: 1,
            OffsetInDescriptorsFromTableStart: 4,
            ..Default::default()
        },
    ];
    let parameters = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: ranges.len() as u32,
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
                    Num32BitValues: CONSTANT_WORDS as u32,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
    ];
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: parameters.len() as u32,
        pParameters: parameters.as_ptr(),
        ..Default::default()
    };
    let mut blob = None;
    unsafe { D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None) }
        .ok()?;
    unsafe { device.CreateRootSignature(0, blob_bytes(blob.as_ref()?)) }.ok()
}

fn create_pipeline(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    mode: u32,
) -> Option<ID3D12PipelineState> {
    // FILTERS supplies the three modes for which the native payload returns a static string.
    let source = unsafe { rsf_sharpening_hlsl(mode) };
    if source.is_null() {
        return None;
    }
    let source = unsafe { CStr::from_ptr(source) };
    let compiler = Compiler::system("d3dcompiler_47.dll")?;
    let request = Request {
        source: source.to_bytes(),
        name: c"Sharpening12",
        entry: c"CSMain",
        target: c"cs_5_0",
        flags: D3DCOMPILE_ENABLE_STRICTNESS,
    };
    let result = unsafe { compile(compiler.entry()?, request, &[], ptr::null_mut()) };
    if result.status < 0 {
        return None;
    }
    let code = result.code?;
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
    unsafe { device.CreateComputePipelineState(&desc) }.ok()
}

fn create_heap(device: &ID3D12Device) -> Option<ID3D12DescriptorHeap> {
    let desc = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        NumDescriptors: 5,
        Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
        NodeMask: 0,
    };
    unsafe { device.CreateDescriptorHeap(&desc) }.ok()
}

fn create_scratch(device: &ID3D12Device, output: &D3D12_RESOURCE_DESC) -> Option<ID3D12Resource> {
    let properties = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: output.Width,
        Height: output.Height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: output.Format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let mut scratch = None;
    unsafe {
        device.CreateCommittedResource(
            &properties,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            None,
            &mut scratch,
        )
    }
    .ok()?;
    scratch
}

fn scratch_key(desc: &D3D12_RESOURCE_DESC) -> (u64, u32, DXGI_FORMAT) {
    (desc.Width, desc.Height, desc.Format)
}

pub(super) fn transition(
    resource: &ID3D12Resource,
    from: D3D12_RESOURCE_STATES,
    to: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // The barrier borrows the caller's live reference.
                pResource: ManuallyDrop::new(Some(unsafe {
                    ID3D12Resource::from_raw(resource.as_raw())
                })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: from,
                StateAfter: to,
            }),
        },
    }
}

fn write_descriptors(
    device: &ID3D12Device,
    heap: &ID3D12DescriptorHeap,
    scratch: &ID3D12Resource,
    resources: [Option<&ID3D12Resource>; 4],
    descriptions: &[D3D12_RESOURCE_DESC; 4],
) {
    let mut cpu = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
    let stride =
        unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) }
            as usize;
    let format = descriptions[0].Format;
    let null_formats = [
        format,
        DXGI_FORMAT_R16G16_FLOAT,
        DXGI_FORMAT_R32_FLOAT,
        DXGI_FORMAT_R32_FLOAT,
    ];
    for (index, resource) in resources.into_iter().enumerate() {
        let format = if resource.is_some() {
            DXGI_FORMAT(
                unsafe { rsf_d3d11_output_srv_format(descriptions[index].Format.0 as u32) } as i32,
            )
        } else {
            null_formats[index]
        };
        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: format,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        unsafe { device.CreateShaderResourceView(resource, Some(&desc), cpu) };
        cpu.ptr += stride;
    }
    let desc = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    unsafe { device.CreateUnorderedAccessView(scratch, None::<&ID3D12Resource>, Some(&desc), cpu) };
}

impl Sharpen12 {
    pub(super) fn dispatches(&self) -> u64 {
        self.dispatches
    }

    pub(super) fn run(
        &mut self,
        commands: *mut c_void,
        textures: [*mut c_void; 4],
        options: &Options,
        frame: &Frame,
        slot: u32,
    ) -> bool {
        (unsafe { rsf_sharpen11_active(options) }) == 0
            || self
                .record(commands, textures, options, frame, slot)
                .is_some()
    }

    fn record(
        &mut self,
        commands: *mut c_void,
        textures: [*mut c_void; 4],
        options: &Options,
        frame: &Frame,
        slot: u32,
    ) -> Option<()> {
        let slot_index = slot as usize;
        if slot_index >= SLOT_COUNT {
            return None;
        }
        // The facade lends typed, live resources for the duration of recording.
        let commands = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&commands) }?;
        let resources = textures
            .each_ref()
            .map(|raw| unsafe { ID3D12Resource::from_raw_borrowed(raw) });
        let [Some(output), _, depth, exposure] = resources else {
            return None;
        };
        let device = device_of(commands)?;
        let mut descriptions = [D3D12_RESOURCE_DESC::default(); 4];
        for (resource, desc) in resources.into_iter().zip(&mut descriptions) {
            if let Some(resource) = resource {
                if device_of(resource)? != device {
                    return None;
                }
                *desc = unsafe { resource.GetDesc() };
                if !shader_readable(desc) {
                    return None;
                }
            }
        }
        let [target, motion_desc, depth_desc, exposure_desc] = descriptions;
        if !matches!(
            target.Format,
            DXGI_FORMAT_R16G16B16A16_FLOAT | DXGI_FORMAT_R32G32B32A32_FLOAT
        ) || (exposure.is_some() && (exposure_desc.Width != 1 || exposure_desc.Height != 1))
        {
            return None;
        }
        let mode = effective_mode(options.mode, depth.is_some())?;
        let selected = FILTERS.iter().position(|&filter| filter == mode)?;
        if *self.device.get_or_insert_with(|| device.clone()) != device {
            return None;
        }
        if self.root.is_none() {
            self.root = Some(create_root(&device)?);
        }
        let root = self.root.as_ref()?;
        for (pipeline, filter) in self.pipelines.iter_mut().zip(FILTERS) {
            if pipeline.is_none() {
                *pipeline = Some(create_pipeline(&device, root, filter)?);
            }
        }
        let pipeline = self.pipelines[selected].as_ref()?;
        let slot = &mut self.slots[slot_index];
        if slot.heap.is_none() {
            slot.heap = Some(create_heap(&device)?);
        }
        let heap = slot.heap.as_ref()?;
        if slot.scratch.as_ref().is_none_or(|scratch| {
            scratch_key(&unsafe { scratch.GetDesc() }) != scratch_key(&target)
        }) {
            // Allocate first so a refused resize leaves the retired slot available.
            slot.scratch = Some(create_scratch(&device, &target)?);
        }
        let scratch = slot.scratch.as_ref()?;

        let extent = [
            target.Width as u32,
            target.Height,
            motion_desc.Width as u32,
            motion_desc.Height,
            depth_desc.Width as u32,
            depth_desc.Height,
        ];
        let mut constants = [0; CONSTANT_WORDS];
        unsafe {
            rsf_sharpen11_pack(
                options,
                frame,
                extent.as_ptr(),
                u32::from(exposure.is_some()),
                mode,
                constants.as_mut_ptr(),
            );
        }
        write_descriptors(&device, heap, scratch, resources, &descriptions);
        let uav = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
        let read = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        let copy_src = D3D12_RESOURCE_STATE_COPY_SOURCE;
        let copy_dst = D3D12_RESOURCE_STATE_COPY_DEST;
        unsafe {
            commands.ResourceBarrier(&[transition(output, uav, read)]);
            commands.SetDescriptorHeaps(std::slice::from_ref(&slot.heap));
            commands.SetComputeRootSignature(root);
            commands.SetComputeRootDescriptorTable(0, heap.GetGPUDescriptorHandleForHeapStart());
            commands.SetComputeRoot32BitConstants(
                1,
                CONSTANT_WORDS as u32,
                constants.as_ptr().cast(),
                0,
            );
            commands.SetPipelineState(pipeline);
            commands.Dispatch(
                extent[0].div_ceil(GROUP_EDGE),
                extent[1].div_ceil(GROUP_EDGE),
                1,
            );
            commands.ResourceBarrier(&[
                transition(scratch, uav, copy_src),
                transition(output, read, copy_dst),
            ]);
            commands.CopyResource(output, scratch);
            commands.ResourceBarrier(&[
                transition(output, copy_dst, uav),
                transition(scratch, copy_src, uav),
            ]);
        }
        self.dispatches = self.dispatches.wrapping_add(1);
        Some(())
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(super) extern "C" fn create_host() -> *mut c_void {
    crate::allocate_handle(Sharpen12::default()).cast()
}

#[cfg_attr(test, allow(dead_code))]
pub(super) unsafe extern "C" fn destroy_host(owner: *mut c_void) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner.cast::<Sharpen12>()) });
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(super) unsafe extern "C" fn run_host(
    owner: *mut c_void,
    commands: *mut c_void,
    output: *mut c_void,
    motion: *mut c_void,
    depth: *mut c_void,
    exposure: *mut c_void,
    options: *const Options,
    hdr: u32,
    depth_inverted: u32,
    near_plane: f32,
    far_plane: f32,
    slot: u32,
) -> i32 {
    let Some(options) = (unsafe { options.as_ref() }) else {
        return 0;
    };
    if unsafe { rsf_sharpening_requested(options) } == 0 {
        return 1;
    }
    let Some(owner) = (unsafe { owner.cast::<Sharpen12>().as_mut() }) else {
        return 0;
    };
    let frame = Frame {
        depth_linear: 0,
        depth_inverted,
        hdr: u32::from(hdr != 0),
        near_plane,
        far_plane,
        exposure: 1.0,
        motion_x: 1.0,
        motion_y: 1.0,
    };
    i32::from(owner.run(
        commands,
        [output, motion, depth, exposure],
        options,
        &frame,
        slot,
    ))
}
