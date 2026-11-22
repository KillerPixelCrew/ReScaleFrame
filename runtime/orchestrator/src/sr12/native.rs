// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_output_abi::rsf_d3d11_output_srv_format;
use crate::d3d12_host::sr::*;
use crate::d3d12_host::{borrow, rsf_core_d3d12_create_uav_texture, rsf_core_d3d12_transition};
use crate::frame_capture12::native::{rsf_fg12_platform_map, rsf_fg12_platform_unmap};
use crate::frame_capture12::{self as capture, Owner as Capture};
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};
use windows::core::Interface;

const _: () = {
    use crate::dxgi_format::{
        D24_UNORM_S8_UINT, D32_FLOAT, D32_FLOAT_S8X24_UINT, R24_UNORM_X8_TYPELESS, R24G8_TYPELESS,
        R32_TYPELESS, R32G8X24_TYPELESS, RGBA16_FLOAT, RGBA32_FLOAT,
    };
    assert!(RGBA32_FLOAT == DXGI_FORMAT_R32G32B32A32_FLOAT.0 as u32);
    assert!(RGBA16_FLOAT == DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32);
    assert!(RG32_FLOAT == DXGI_FORMAT_R32G32_FLOAT.0 as u32);
    assert!(R32G8X24_TYPELESS == DXGI_FORMAT_R32G8X24_TYPELESS.0 as u32);
    assert!(D32_FLOAT_S8X24_UINT == DXGI_FORMAT_D32_FLOAT_S8X24_UINT.0 as u32);
    assert!(R32_FLOAT_X8X24_TYPELESS == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS.0 as u32);
    assert!(R11G11B10_FLOAT == DXGI_FORMAT_R11G11B10_FLOAT.0 as u32);
    assert!(RG16_FLOAT == DXGI_FORMAT_R16G16_FLOAT.0 as u32);
    assert!(R32_TYPELESS == DXGI_FORMAT_R32_TYPELESS.0 as u32);
    assert!(D32_FLOAT == DXGI_FORMAT_D32_FLOAT.0 as u32);
    assert!(R32_FLOAT == DXGI_FORMAT_R32_FLOAT.0 as u32);
    assert!(R24G8_TYPELESS == DXGI_FORMAT_R24G8_TYPELESS.0 as u32);
    assert!(D24_UNORM_S8_UINT == DXGI_FORMAT_D24_UNORM_S8_UINT.0 as u32);
    assert!(R24_UNORM_X8_TYPELESS == DXGI_FORMAT_R24_UNORM_X8_TYPELESS.0 as u32);
    assert!(R16_FLOAT == DXGI_FORMAT_R16_FLOAT.0 as u32);
    assert!(RENDER_TARGET == D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32);
    assert!(UNORDERED_ACCESS == D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32);
    assert!(SHADER_READ == D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32);
    assert!(COPY_DEST == D3D12_RESOURCE_STATE_COPY_DEST.0 as u32);
    assert!(COPY_SOURCE == D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32);
    assert!(TEXTURE2D == D3D12_RESOURCE_DIMENSION_TEXTURE2D.0 as u32);
    assert!(EXPOSURE_UPLOAD_BYTES == D3D12_TEXTURE_DATA_PITCH_ALIGNMENT as u64);
    assert!(size_of::<PlanResult>() == 24);
};

unsafe extern "C" {
    fn rsf_dlss_native12_create(
        device: Handle,
        setup: *const rsf_dlss_setup,
        out: *mut Handle,
    ) -> i32;
    fn rsf_dlss_native12_create_shared(
        device: Handle,
        setup: *const rsf_dlss_setup,
        host: Handle,
        out: *mut Handle,
    ) -> i32;
    fn rsf_dlss_native12_create_registered(
        device: Handle,
        setup: *const rsf_dlss_setup,
        registration: Handle,
        out: *mut Handle,
    ) -> i32;
    fn rsf_dlss_native12_plan(
        context: Handle,
        width: u32,
        height: u32,
        quality: u32,
        render_width: *mut u32,
        render_height: *mut u32,
    ) -> i32;
    fn rsf_dlss_native12_set_sdr(context: Handle, sdr: u32);
    fn rsf_dlss_native12_evaluate(context: Handle, list: Handle, frame: *const rsf_sr_frame)
    -> i32;
    fn rsf_dlss_native12_evaluate_borrowed(
        context: Handle,
        list: Handle,
        frame: *const rsf_sr_frame,
        latency_token: Handle,
    ) -> i32;
    fn rsf_dlss_native12_present(
        context: Handle,
        chain: Handle,
        sync_interval: u32,
        flags: u32,
        begin: u32,
    ) -> i32;
    fn rsf_dlss_native12_destroy(context: Handle);
    fn rsf_sr_session_create(setup: *const rsf_sr_session_setup, out: *mut Handle) -> i32;
    fn rsf_sr_session_select(session: Handle, backend: u32, quality: u32, version: u64) -> i32;
    fn rsf_sr_session_get_status(session: Handle, status: *mut rsf_sr_session_status) -> i32;
    fn rsf_sr_session_evaluate(session: Handle, list: Handle, frame: *const rsf_sr_frame) -> i32;
    fn rsf_sr_session_destroy(session: Handle);
    fn rsf_sharpen12_host_create() -> Handle;
    fn rsf_sharpen12_host_destroy(owner: Handle);
    fn rsf_sharpen12_host_run(
        owner: Handle,
        list: Handle,
        input: Handle,
        output: Handle,
        depth: Handle,
        exposure: Handle,
        options: *const rsf_sharpening_options,
        width: u32,
        height: u32,
        near_plane: f32,
        far_plane: f32,
        slot: u32,
    ) -> i32;
    fn rsf_input_dump_begin_record(record: *const rsf_frame_record, backend: u32) -> i32;
    fn rsf_input_dump_pass_metadata(pass: *const Pass);
    fn rsf_input_dump_sr_frame(frame: *const rsf_sr_frame, backend: u32, sdr: u32);
    fn rsf_input_dump_result(
        record: *const rsf_frame_record,
        stage: *const c_char,
        backend: u32,
        result: i32,
    );
}

pub(super) static PLATFORM: Platform = Platform {
    struct_size: size_of::<Platform>() as u32,
    dlss_create: rsf_dlss_native12_create,
    dlss_create_shared: rsf_dlss_native12_create_shared,
    dlss_create_registered: rsf_dlss_native12_create_registered,
    dlss_plan: rsf_dlss_native12_plan,
    dlss_set_sdr: rsf_dlss_native12_set_sdr,
    dlss_evaluate: rsf_dlss_native12_evaluate,
    dlss_evaluate_borrowed: rsf_dlss_native12_evaluate_borrowed,
    dlss_present: rsf_dlss_native12_present,
    dlss_destroy: rsf_dlss_native12_destroy,
    session_create: rsf_sr_session_create,
    session_select: rsf_sr_session_select,
    session_get_status: rsf_sr_session_get_status,
    session_evaluate: rsf_sr_session_evaluate,
    session_destroy: rsf_sr_session_destroy,
    create_pipeline: rsf_core_sr12_create_pipeline,
    create_heap: rsf_core_sr12_create_heap,
    create_texture: rsf_core_d3d12_create_uav_texture,
    create_exposure_texture: rsf_core_sr12_create_exposure_texture,
    create_exposure_upload: rsf_core_sr12_create_exposure_upload,
    write_exposure_upload,
    create_readback: rsf_core_sr12_create_readback,
    describe,
    owned_by: rsf_core_sr12_owned_by,
    write_srv,
    write_uav,
    dispatch,
    transition: rsf_core_d3d12_transition,
    copy_region,
    copy_unit_exposure,
    copy_motion_tile,
    map: rsf_fg12_platform_map,
    unmap: rsf_fg12_platform_unmap,
    depth_domains: crate::d3d12_host::pass_depth_domains,
    capture_create,
    capture_destroy,
    capture_enabled,
    capture_configure,
    capture_flush,
    capture_record,
    capture_event,
    capture_wants,
    capture_force_reset,
    sharpen_create: rsf_sharpen12_host_create,
    sharpen_destroy: rsf_sharpen12_host_destroy,
    sharpen_run: rsf_sharpen12_host_run,
    dump_begin_record: rsf_input_dump_begin_record,
    dump_pass_metadata: rsf_input_dump_pass_metadata,
    dump_sr_frame: rsf_input_dump_sr_frame,
    dump_result: rsf_input_dump_result,
    fg_bind,
};

unsafe extern "C" fn write_exposure_upload(resource: Handle, value: f32) -> i32 {
    let upload: &ID3D12Resource = unsafe { borrow(&resource) };
    let mut mapped: Handle = ptr::null_mut();
    let empty = D3D12_RANGE { Begin: 0, End: 0 };
    if unsafe { upload.Map(0, Some(&empty), Some(&mut mapped)) }.is_err() {
        return 0;
    }
    let mut written = empty;
    if !mapped.is_null() {
        unsafe { mapped.cast::<f32>().write(value) };
        written.End = size_of::<f32>();
    }
    unsafe { upload.Unmap(0, Some(&written)) };
    i32::from(!mapped.is_null())
}

unsafe extern "C" fn describe(resource: Handle, out: *mut TextureDesc) {
    let desc = unsafe { borrow::<ID3D12Resource>(&resource).GetDesc() };
    let format = desc.Format.0 as u32;
    unsafe {
        out.write(TextureDesc {
            width: desc.Width,
            height: desc.Height,
            dimension: desc.Dimension.0 as u32,
            array_size: u32::from(desc.DepthOrArraySize),
            samples: desc.SampleDesc.Count,
            format,
            srv_format: rsf_d3d11_output_srv_format(format),
        });
    }
}

fn descriptor(
    device: &ID3D12Device,
    heap: &ID3D12DescriptorHeap,
    index: u32,
) -> D3D12_CPU_DESCRIPTOR_HANDLE {
    unsafe {
        let mut cpu = heap.GetCPUDescriptorHandleForHeapStart();
        cpu.ptr += index as usize
            * device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
                as usize;
        cpu
    }
}

unsafe extern "C" fn write_srv(
    device: Handle,
    heap: Handle,
    index: u32,
    resource: Handle,
    format: u32,
) {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource) };
    let cpu = descriptor(device, unsafe { borrow(&heap) }, index);
    let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT(format as i32),
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_SRV {
                MipLevels: 1,
                ..Default::default()
            },
        },
    };
    unsafe { device.CreateShaderResourceView(resource, Some(&view), cpu) };
}

unsafe extern "C" fn write_uav(
    device: Handle,
    heap: Handle,
    index: u32,
    resource: Handle,
    format: u32,
) {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource) };
    let cpu = descriptor(device, unsafe { borrow(&heap) }, index);
    let view = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: DXGI_FORMAT(format as i32),
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    unsafe {
        device.CreateUnorderedAccessView(resource, None::<&ID3D12Resource>, Some(&view), cpu)
    };
}

unsafe extern "C" fn dispatch(
    list: Handle,
    heap: Handle,
    root: Handle,
    pipeline: Handle,
    constants: *const u32,
    groups_x: u32,
    groups_y: u32,
) {
    let commands: &ID3D12GraphicsCommandList = unsafe { borrow(&list) };
    let descriptors: &ID3D12DescriptorHeap = unsafe { borrow(&heap) };
    unsafe {
        (commands.vtable().SetDescriptorHeaps)(commands.as_raw(), 1, &heap);
        commands.SetComputeRootSignature(borrow::<ID3D12RootSignature>(&root));
        commands.SetComputeRootDescriptorTable(0, descriptors.GetGPUDescriptorHandleForHeapStart());
        commands.SetPipelineState(borrow::<ID3D12PipelineState>(&pipeline));
        commands.SetComputeRoot32BitConstants(1, 6, constants.cast(), 0);
        commands.Dispatch(groups_x, groups_y, 1);
    }
}

fn texture_copy(
    resource: &Handle,
    footprint: Option<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>,
) -> D3D12_TEXTURE_COPY_LOCATION {
    // The recording owner retains the resource. This descriptor borrows its COM reference.
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(resource) };
    let (kind, location) = match footprint {
        Some(footprint) => (
            D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: footprint,
            },
        ),
        None => (
            D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        ),
    };
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: ManuallyDrop::new(
            resource.map(|resource| unsafe { ID3D12Resource::from_raw(resource.as_raw()) }),
        ),
        Type: kind,
        Anonymous: location,
    }
}

fn footprint(
    offset: u64,
    format: DXGI_FORMAT,
    width: u32,
    height: u32,
) -> D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
        Offset: offset,
        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
            Format: format,
            Width: width,
            Height: height,
            Depth: 1,
            RowPitch: D3D12_TEXTURE_DATA_PITCH_ALIGNMENT,
        },
    }
}

unsafe extern "C" fn copy_region(
    list: Handle,
    target: Handle,
    source: Handle,
    width: u32,
    height: u32,
) {
    let commands: &ID3D12GraphicsCommandList = unsafe { borrow(&list) };
    let from = texture_copy(&source, None);
    let to = texture_copy(&target, None);
    let rectangle = D3D12_BOX {
        left: 0,
        top: 0,
        front: 0,
        right: width,
        bottom: height,
        back: 1,
    };
    unsafe { commands.CopyTextureRegion(&to, 0, 0, 0, &from, Some(&rectangle)) };
}

unsafe extern "C" fn copy_unit_exposure(list: Handle, target: Handle, upload: Handle) {
    let commands: &ID3D12GraphicsCommandList = unsafe { borrow(&list) };
    let from = texture_copy(&upload, Some(footprint(0, DXGI_FORMAT_R32_FLOAT, 1, 1)));
    let to = texture_copy(&target, None);
    unsafe { commands.CopyTextureRegion(&to, 0, 0, 0, &from, None) };
}

unsafe extern "C" fn copy_motion_tile(
    list: Handle,
    buffer: Handle,
    offset: u64,
    source: Handle,
    x: u32,
    y: u32,
) {
    let commands: &ID3D12GraphicsCommandList = unsafe { borrow(&list) };
    let from = texture_copy(&source, None);
    let to = texture_copy(
        &buffer,
        Some(footprint(offset, DXGI_FORMAT_R16G16_FLOAT, TILE, TILE)),
    );
    let rectangle = D3D12_BOX {
        left: x,
        top: y,
        front: 0,
        right: x + TILE,
        bottom: y + TILE,
        back: 1,
    };
    unsafe { commands.CopyTextureRegion(&to, 0, 0, 0, &from, Some(&rectangle)) };
}

extern "C" fn fg_bind() {
    crate::fg12::bind_native_platform();
}

extern "C" fn capture_create() -> Handle {
    capture::rsf_core_capture12_create(1).cast()
}

unsafe extern "C" fn capture_destroy(owner: Handle) {
    unsafe { capture::rsf_core_capture12_destroy(owner.cast::<Capture>()) };
}

unsafe extern "C" fn capture_enabled(owner: Handle) -> i32 {
    unsafe { capture::rsf_core_capture12_enabled(owner.cast::<Capture>()) }
}

unsafe extern "C" fn capture_configure(
    owner: Handle,
    directory: *const c_char,
    first: u64,
    frames: u32,
) -> i32 {
    unsafe {
        capture::rsf_core_capture12_configure(owner.cast::<Capture>(), directory, first, frames)
    }
}

unsafe extern "C" fn capture_flush(owner: Handle, slot: u32) -> i32 {
    unsafe { capture::rsf_core_capture12_flush(owner.cast::<Capture>(), slot, 0) }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn capture_record(
    owner: Handle,
    list: Handle,
    source: Handle,
    state: u32,
    record: *const rsf_frame_record,
    role: *const c_char,
    slot: u32,
    ordinal: u32,
    reset: u32,
    preview: u32,
) -> i32 {
    unsafe {
        capture::rsf_core_capture12_record(
            owner.cast::<Capture>(),
            list,
            source,
            state,
            record,
            role,
            slot,
            ordinal,
            reset,
            preview,
            0.0,
        )
    }
}

unsafe extern "C" fn capture_event(
    owner: Handle,
    record: *const rsf_frame_record,
    name: *const c_char,
    sequence: u64,
    reset: u32,
    result: i32,
) -> i32 {
    unsafe {
        capture::rsf_core_capture12_event(
            owner.cast::<Capture>(),
            record,
            name,
            sequence,
            reset,
            result,
        )
    }
}

unsafe extern "C" fn capture_wants(owner: Handle, frame: u64) -> i32 {
    unsafe { capture::rsf_core_capture12_wants(owner.cast::<Capture>(), frame) }
}

unsafe extern "C" fn capture_force_reset(owner: Handle, frame: u64) -> i32 {
    unsafe { capture::rsf_core_capture12_force_reset(owner.cast::<Capture>(), frame) }
}
