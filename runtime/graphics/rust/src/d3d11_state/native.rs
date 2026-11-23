// SPDX-License-Identifier: MIT
use super::*;
use crate::com_ref::vtable;
use crate::d3d11_output_platform::rsf_d3d11_output_child_device;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::{
    Direct3D::{D3D_FEATURE_LEVEL_11_1, D3D_PRIMITIVE_TOPOLOGY},
    Direct3D11::*,
    Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_UNKNOWN},
};
use windows::core::{BOOL, IUnknown, Interface};

const _: () = {
    assert!(RESOURCES == D3D11_COMMONSHADER_INPUT_RESOURCE_SLOT_COUNT as usize);
    assert!(SAMPLERS == D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT as usize);
    assert!(CONSTANTS == D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT as usize);
    assert!(INTERFACES == D3D11_SHADER_MAX_INTERFACES as usize);
    assert!(VERTEX_BUFFERS == D3D11_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT as usize);
    assert!(UAVS == D3D11_1_UAV_SLOT_COUNT as usize);
    assert!(STREAM_TARGETS == D3D11_SO_BUFFER_SLOT_COUNT as usize);
    assert!(VIEWPORTS == D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE as usize);
    assert!(TARGETS == D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT as usize);
    assert!(size_of::<Viewport>() == size_of::<D3D11_VIEWPORT>());
    assert!(size_of::<Rect>() == size_of::<RECT>());
    assert!(align_of::<Viewport>() == align_of::<D3D11_VIEWPORT>());
    assert!(align_of::<Rect>() == align_of::<RECT>());
};

const KEEP_COUNTS: [u32; UAVS] = [u32::MAX; UAVS];

fn raw(binding: &Option<IUnknown>) -> Handle {
    binding.as_ref().map_or(null_mut(), Interface::as_raw)
}

fn out(binding: &mut Option<IUnknown>) -> *mut Handle {
    ptr::from_mut(binding).cast()
}

type GetShader = unsafe extern "system" fn(Handle, *mut Handle, *mut Handle, *mut u32);
type SetShader = unsafe extern "system" fn(Handle, Handle, *const Handle, u32);
type GetSlots = unsafe extern "system" fn(Handle, u32, u32, *mut Handle);
type SetSlots = unsafe extern "system" fn(Handle, u32, u32, *const Handle);
type GetRanges = unsafe extern "system" fn(Handle, u32, u32, *mut Handle, *mut u32, *mut u32);
type SetRanges = unsafe extern "system" fn(Handle, u32, u32, *const Handle, *const u32, *const u32);

struct StageOps {
    get_shader: GetShader,
    set_shader: SetShader,
    get_resources: GetSlots,
    set_resources: SetSlots,
    get_samplers: GetSlots,
    set_samplers: SetSlots,
    get_constants: GetSlots,
    set_constants: SetSlots,
}

macro_rules! stage {
    ($api:ident, $get_shader:ident, $set_shader:ident, $get_resources:ident, $set_resources:ident,
     $get_samplers:ident, $set_samplers:ident, $get_constants:ident, $set_constants:ident) => {
        StageOps {
            get_shader: $api.$get_shader,
            set_shader: $api.$set_shader,
            get_resources: $api.$get_resources,
            set_resources: $api.$set_resources,
            get_samplers: $api.$get_samplers,
            set_samplers: $api.$set_samplers,
            get_constants: $api.$get_constants,
            set_constants: $api.$set_constants,
        }
    };
}

fn stage_ops(api: &ID3D11DeviceContext_Vtbl) -> [StageOps; 6] {
    [
        stage!(
            api,
            VSGetShader,
            VSSetShader,
            VSGetShaderResources,
            VSSetShaderResources,
            VSGetSamplers,
            VSSetSamplers,
            VSGetConstantBuffers,
            VSSetConstantBuffers
        ),
        stage!(
            api,
            HSGetShader,
            HSSetShader,
            HSGetShaderResources,
            HSSetShaderResources,
            HSGetSamplers,
            HSSetSamplers,
            HSGetConstantBuffers,
            HSSetConstantBuffers
        ),
        stage!(
            api,
            DSGetShader,
            DSSetShader,
            DSGetShaderResources,
            DSSetShaderResources,
            DSGetSamplers,
            DSSetSamplers,
            DSGetConstantBuffers,
            DSSetConstantBuffers
        ),
        stage!(
            api,
            GSGetShader,
            GSSetShader,
            GSGetShaderResources,
            GSSetShaderResources,
            GSGetSamplers,
            GSSetSamplers,
            GSGetConstantBuffers,
            GSSetConstantBuffers
        ),
        stage!(
            api,
            PSGetShader,
            PSSetShader,
            PSGetShaderResources,
            PSSetShaderResources,
            PSGetSamplers,
            PSSetSamplers,
            PSGetConstantBuffers,
            PSSetConstantBuffers
        ),
        stage!(
            api,
            CSGetShader,
            CSSetShader,
            CSGetShaderResources,
            CSSetShaderResources,
            CSGetSamplers,
            CSSetSamplers,
            CSGetConstantBuffers,
            CSSetConstantBuffers
        ),
    ]
}

unsafe extern "C" fn immediate(context: Handle) -> u32 {
    u32::from(
        unsafe { (vtable::<ID3D11DeviceContext>(&context).GetType)(context) }
            == D3D11_DEVICE_CONTEXT_IMMEDIATE,
    )
}

unsafe extern "C" fn device(context: Handle) -> Handle {
    unsafe { rsf_d3d11_output_child_device(context) }
}

unsafe extern "C" fn features(device: Handle, features: &mut Features) {
    let api = unsafe { vtable::<ID3D11Device>(&device) };
    let mut options = D3D11_FEATURE_DATA_D3D11_OPTIONS::default();
    let level = unsafe { (api.GetFeatureLevel)(device) };
    features.uav_count = if level.0 >= D3D_FEATURE_LEVEL_11_1.0 {
        D3D11_1_UAV_SLOT_COUNT
    } else {
        D3D11_PS_CS_UAV_REGISTER_COUNT
    };
    let status = unsafe {
        (api.CheckFeatureSupport)(
            device,
            D3D11_FEATURE_D3D11_OPTIONS,
            ptr::from_mut(&mut options).cast(),
            size_of_val(&options) as u32,
        )
    };
    features.constant_ranges =
        u32::from(status.is_ok() && options.ConstantBufferOffsetting.as_bool());
}

unsafe extern "C" fn context1(context: Handle) -> Handle {
    let mut extended = null_mut();
    let status = unsafe {
        (vtable::<IUnknown>(&context).QueryInterface)(
            context,
            &ID3D11DeviceContext1::IID,
            &mut extended,
        )
    };
    unsafe { com_ref::from_result(status, extended) }.map_or(null_mut(), Interface::into_raw)
}

unsafe fn get_ranges(context1: Handle, index: usize) -> GetRanges {
    let api = unsafe { vtable::<ID3D11DeviceContext1>(&context1) };
    [
        api.VSGetConstantBuffers1,
        api.HSGetConstantBuffers1,
        api.DSGetConstantBuffers1,
        api.GSGetConstantBuffers1,
        api.PSGetConstantBuffers1,
        api.CSGetConstantBuffers1,
    ][index]
}

unsafe fn set_ranges(context1: Handle, index: usize) -> SetRanges {
    let api = unsafe { vtable::<ID3D11DeviceContext1>(&context1) };
    [
        api.VSSetConstantBuffers1,
        api.HSSetConstantBuffers1,
        api.DSSetConstantBuffers1,
        api.GSSetConstantBuffers1,
        api.PSSetConstantBuffers1,
        api.CSSetConstantBuffers1,
    ][index]
}

unsafe extern "C" fn read(context: Handle, state: &mut Snapshot) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    unsafe {
        (api.IAGetInputLayout)(context, out(&mut state.layout));
        (api.IAGetPrimitiveTopology)(context, ptr::from_mut(&mut state.topology).cast());
        (api.IAGetIndexBuffer)(
            context,
            out(&mut state.index_buffer),
            ptr::from_mut(&mut state.index_format).cast(),
            &mut state.index_offset,
        );
        (api.IAGetVertexBuffers)(
            context,
            0,
            VERTEX_BUFFERS as u32,
            state.vertex_buffers.as_mut_ptr().cast(),
            state.vertex_strides.as_mut_ptr(),
            state.vertex_offsets.as_mut_ptr(),
        );

        let extended = raw(&state.context1);
        let ops = stage_ops(api);
        for ((stage, shader), op) in state.stages.iter_mut().zip(&mut state.shaders).zip(&ops) {
            stage.class_count = INTERFACES as u32;
            (op.get_shader)(
                context,
                out(shader),
                stage.classes.as_mut_ptr().cast(),
                &mut stage.class_count,
            );
        }
        for (index, (stage, op)) in state.stages.iter_mut().zip(ops).enumerate() {
            (op.get_resources)(
                context,
                0,
                RESOURCES as u32,
                stage.resources.as_mut_ptr().cast(),
            );
            (op.get_samplers)(
                context,
                0,
                SAMPLERS as u32,
                stage.samplers.as_mut_ptr().cast(),
            );
            if state.constant_ranges != 0 {
                get_ranges(extended, index)(
                    extended,
                    0,
                    CONSTANTS as u32,
                    stage.constants.as_mut_ptr().cast(),
                    stage.first_constants.as_mut_ptr(),
                    stage.constant_counts.as_mut_ptr(),
                );
            } else {
                (op.get_constants)(
                    context,
                    0,
                    CONSTANTS as u32,
                    stage.constants.as_mut_ptr().cast(),
                );
            }
        }
        (api.CSGetUnorderedAccessViews)(
            context,
            0,
            state.uav_count,
            state.compute_uavs.as_mut_ptr().cast(),
        );
        (api.SOGetTargets)(
            context,
            STREAM_TARGETS as u32,
            state.stream_targets.as_mut_ptr().cast(),
        );
        (api.GetPredication)(
            context,
            out(&mut state.predicate),
            ptr::from_mut(&mut state.predicate_value).cast(),
        );

        (api.RSGetState)(context, out(&mut state.raster));
        state.viewport_count = VIEWPORTS as u32;
        (api.RSGetViewports)(
            context,
            &mut state.viewport_count,
            state.viewports.as_mut_ptr().cast(),
        );
        state.scissor_count = VIEWPORTS as u32;
        (api.RSGetScissorRects)(
            context,
            &mut state.scissor_count,
            state.scissors.as_mut_ptr().cast(),
        );

        (api.OMGetRenderTargetsAndUnorderedAccessViews)(
            context,
            TARGETS as u32,
            state.targets.as_mut_ptr().cast(),
            out(&mut state.depth_view),
            0,
            state.uav_count,
            state.pixel_uavs.as_mut_ptr().cast(),
        );
        (api.OMGetBlendState)(
            context,
            out(&mut state.blend),
            state.blend_factor.as_mut_ptr(),
            &mut state.blend_mask,
        );
        (api.OMGetDepthStencilState)(
            context,
            out(&mut state.depth_state),
            &mut state.stencil_reference,
        );
    }
}

unsafe extern "C" fn clear(context: Handle, uav_count: u32) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    let no_resources = [null_mut(); RESOURCES];
    let no_uavs = [null_mut(); UAVS];
    let no_vertices = [null_mut(); VERTEX_BUFFERS];
    let no_strides = [0; VERTEX_BUFFERS];
    unsafe {
        for op in stage_ops(api) {
            (op.set_resources)(context, 0, RESOURCES as u32, no_resources.as_ptr());
        }
        (api.CSSetUnorderedAccessViews)(
            context,
            0,
            uav_count,
            no_uavs.as_ptr(),
            KEEP_COUNTS.as_ptr(),
        );
        (api.OMSetRenderTargetsAndUnorderedAccessViews)(
            context,
            0,
            ptr::null(),
            null_mut(),
            0,
            uav_count,
            no_uavs.as_ptr(),
            KEEP_COUNTS.as_ptr(),
        );
        (api.SOSetTargets)(context, 0, ptr::null(), ptr::null());
        (api.IASetVertexBuffers)(
            context,
            0,
            VERTEX_BUFFERS as u32,
            no_vertices.as_ptr(),
            no_strides.as_ptr(),
            no_strides.as_ptr(),
        );
        (api.IASetIndexBuffer)(context, null_mut(), DXGI_FORMAT_UNKNOWN, 0);
    }
}

unsafe extern "C" fn write(context: Handle, state: &Snapshot, targets: u32) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    let uav_start = targets as usize;
    unsafe {
        // Hidden UAV counters and stream-output cursors are preserved.
        (api.OMSetRenderTargetsAndUnorderedAccessViews)(
            context,
            targets,
            state.targets.as_ptr().cast(),
            raw(&state.depth_view),
            targets,
            state.uav_count - targets,
            state.pixel_uavs[uav_start..].as_ptr().cast(),
            KEEP_COUNTS[uav_start..].as_ptr(),
        );
        (api.OMSetBlendState)(
            context,
            raw(&state.blend),
            state.blend_factor.as_ptr(),
            state.blend_mask,
        );
        (api.OMSetDepthStencilState)(context, raw(&state.depth_state), state.stencil_reference);
        (api.CSSetUnorderedAccessViews)(
            context,
            0,
            state.uav_count,
            state.compute_uavs.as_ptr().cast(),
            KEEP_COUNTS.as_ptr(),
        );
        (api.SOSetTargets)(
            context,
            STREAM_TARGETS as u32,
            state.stream_targets.as_ptr().cast(),
            [u32::MAX; STREAM_TARGETS].as_ptr(),
        );

        let extended = raw(&state.context1);
        let ops = stage_ops(api);
        for (index, (stage, op)) in state.stages.iter().zip(&ops).enumerate() {
            (op.set_resources)(
                context,
                0,
                RESOURCES as u32,
                stage.resources.as_ptr().cast(),
            );
            (op.set_samplers)(context, 0, SAMPLERS as u32, stage.samplers.as_ptr().cast());
            (op.set_constants)(
                context,
                0,
                CONSTANTS as u32,
                stage.constants.as_ptr().cast(),
            );
            if state.constant_ranges != 0 {
                set_ranges(extended, index)(
                    extended,
                    0,
                    CONSTANTS as u32,
                    stage.constants.as_ptr().cast(),
                    stage.first_constants.as_ptr(),
                    stage.constant_counts.as_ptr(),
                );
            }
        }
        for ((shader, stage), op) in state.shaders.iter().zip(&state.stages).zip(ops) {
            (op.set_shader)(
                context,
                raw(shader),
                stage.classes.as_ptr().cast(),
                stage.class_count,
            );
        }

        (api.IASetInputLayout)(context, raw(&state.layout));
        (api.IASetPrimitiveTopology)(context, D3D_PRIMITIVE_TOPOLOGY(state.topology as i32));
        (api.IASetIndexBuffer)(
            context,
            raw(&state.index_buffer),
            DXGI_FORMAT(state.index_format as i32),
            state.index_offset,
        );
        (api.IASetVertexBuffers)(
            context,
            0,
            VERTEX_BUFFERS as u32,
            state.vertex_buffers.as_ptr().cast(),
            state.vertex_strides.as_ptr(),
            state.vertex_offsets.as_ptr(),
        );
        (api.RSSetState)(context, raw(&state.raster));
        (api.RSSetViewports)(
            context,
            state.viewport_count,
            state.viewports.as_ptr().cast(),
        );
        (api.RSSetScissorRects)(context, state.scissor_count, state.scissors.as_ptr().cast());
        (api.SetPredication)(context, raw(&state.predicate), BOOL(state.predicate_value));
    }
}

const PLATFORM: Platform = Platform {
    immediate,
    device,
    features,
    context1,
    read,
    clear,
    write,
};

/// # Safety
///
/// `context` is null or a live `ID3D11DeviceContext`; `storage` is null or a writable state record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_d3d11_state_save(context: Handle, storage: *mut State) -> u32 {
    unsafe { rsf_core_d3d11_state_save(context, storage, 0, &PLATFORM) }
}

/// # Safety
///
/// `context` is null or a live `ID3D11DeviceContext`; `storage` is null or a writable state record.
pub unsafe extern "C" fn rsf_d3d11_state_save_independent(
    context: Handle,
    storage: *mut State,
) -> u32 {
    unsafe { rsf_core_d3d11_state_save(context, storage, 1, &PLATFORM) }
}

/// # Safety
///
/// `context` is null or a live `ID3D11DeviceContext`; `storage` is null or a record a save on
/// this context filled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_d3d11_state_restore(context: Handle, storage: *mut State) {
    unsafe { rsf_core_d3d11_state_restore(context, storage, &PLATFORM) }
}

/// # Safety
///
/// Each handle is null or live; both resources belong to the context's device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_d3d11_copy_resource(
    context: Handle,
    destination: Handle,
    source: Handle,
) {
    if !context.is_null() && !destination.is_null() && !source.is_null() {
        unsafe {
            (vtable::<ID3D11DeviceContext>(&context).CopyResource)(context, destination, source)
        };
    }
}

/// # Safety
///
/// `device` is null or a live `ID3D11Device` and `texture` null or a live texture of it; the
/// caller owns the returned view.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_d3d11_create_shader_view(device: Handle, texture: Handle) -> Handle {
    if device.is_null() || texture.is_null() {
        return null_mut();
    }
    let mut view = null_mut();
    let status = unsafe {
        (vtable::<ID3D11Device>(&device).CreateShaderResourceView)(
            device,
            texture,
            ptr::null(),
            &mut view,
        )
    };
    unsafe { com_ref::from_result(status, view) }.map_or(null_mut(), Interface::into_raw)
}
