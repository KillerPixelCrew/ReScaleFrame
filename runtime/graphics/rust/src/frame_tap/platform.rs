// SPDX-License-Identifier: MIT
use super::*;
use crate::com_ref::vtable;
use crate::d3d11_output::texture_desc;
use crate::d3d11_output_platform::{rsf_d3d11_output_child_device, rsf_d3d11_output_query_texture};
use std::ptr;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_RENDER_TARGET_VIEW_DESC, D3D11_SAMPLER_DESC, D3D11_VIEWPORT, ID3D11Device,
    ID3D11DeviceContext, ID3D11DeviceContext1, ID3D11RenderTargetView, ID3D11SamplerState,
    ID3D11View,
};
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};

pub(super) unsafe fn patch_slot(vtable: *mut Handle, index: u32, replacement: Handle) -> i32 {
    let entry = unsafe { vtable.add(index as usize) };
    let mut protection = 0;
    if unsafe {
        VirtualProtect(
            entry.cast(),
            size_of::<Handle>(),
            PAGE_EXECUTE_READWRITE,
            &mut protection,
        )
    } == 0
    {
        return 0;
    }
    let mut previous = 0;
    unsafe {
        entry.write_volatile(replacement);
        VirtualProtect(entry.cast(), size_of::<Handle>(), protection, &mut previous);
    }
    1
}

pub(super) unsafe fn context1(context: Handle) -> i32 {
    let context = unsafe { IUnknown::from_raw_borrowed(&context) };
    i32::from(context.is_some_and(|context| context.cast::<ID3D11DeviceContext1>().is_ok()))
}

pub(super) unsafe fn texture_behind(view: Handle) -> Handle {
    let mut resource = null_mut();
    unsafe { (vtable::<ID3D11View>(&view).GetResource)(view, &mut resource) };
    let Some(resource) = (unsafe { com_ref::owned(resource) }) else {
        return null_mut();
    };
    unsafe { rsf_d3d11_output_query_texture(resource.as_raw()) }
}

pub(super) unsafe fn texture_facts(texture: Handle, result: *mut rsf_texture_facts) {
    let desc = unsafe { texture_desc(texture) };
    unsafe {
        result.write(rsf_texture_facts {
            struct_size: size_of::<rsf_texture_facts>() as u32,
            width: desc.width,
            height: desc.height,
            format: desc.format,
            bind_flags: desc.bind_flags,
            mip_levels: desc.mips,
            array_size: desc.layers,
            sample_count: desc.samples,
        });
    }
}

pub(super) unsafe fn target_format(view: Handle) -> u32 {
    let mut desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
    unsafe { (vtable::<ID3D11RenderTargetView>(&view).GetDesc)(view, &mut desc) };
    desc.Format.0 as u32
}

pub(super) unsafe fn buffer_width(resource: Handle, bind_flags: *mut u32) -> u32 {
    let Some((width, flags, _)) =
        (unsafe { crate::constant_buffer_read::resource_buffer_info(resource) })
    else {
        return 0;
    };
    unsafe { bind_flags.write(flags) };
    width
}

pub(super) unsafe fn shader_views(context: Handle, views: *mut Handle) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    unsafe { (api.PSGetShaderResources)(context, 0, VIEWS as u32, views) };
}

pub(super) unsafe fn samplers(context: Handle, samplers: *mut Handle) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    unsafe { (api.PSGetSamplers)(context, 0, SAMPLERS as u32, samplers) };
}

pub(super) unsafe fn set_shader_views(
    original: Handle,
    context: Handle,
    start: u32,
    count: u32,
    views: *const Handle,
) {
    type SetRange = unsafe extern "system" fn(Handle, u32, u32, *const Handle);
    let set: SetRange = unsafe { std::mem::transmute(original) };
    unsafe { set(context, start, count, views) };
}

pub(super) unsafe fn set_targets(
    original: Handle,
    context: Handle,
    count: u32,
    views: *const Handle,
    depth: Handle,
    uavs: *const rsf_frame_tap_uavs,
) {
    if let Some(uavs) = unsafe { uavs.as_ref() } {
        type Set = unsafe extern "system" fn(
            Handle,
            u32,
            *const Handle,
            Handle,
            u32,
            u32,
            *const Handle,
            *const u32,
        );
        let set: Set = unsafe { std::mem::transmute(original) };
        unsafe {
            set(
                context,
                count,
                views,
                depth,
                uavs.start,
                uavs.count,
                uavs.views,
                uavs.initial_counts,
            )
        };
    } else {
        type Set = unsafe extern "system" fn(Handle, u32, *const Handle, Handle);
        let set: Set = unsafe { std::mem::transmute(original) };
        unsafe { set(context, count, views, depth) };
    }
}

pub(super) unsafe fn viewport(context: Handle, result: *mut D3D11_VIEWPORT) -> u32 {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    let mut count = 1;
    unsafe { (api.RSGetViewports)(context, &mut count, result) };
    count
}

pub(super) unsafe fn clone_sampler(context: Handle, sampler: Handle, bias: f32) -> Handle {
    let mut desc = D3D11_SAMPLER_DESC::default();
    unsafe { (vtable::<ID3D11SamplerState>(&sampler).GetDesc)(sampler, &mut desc) };
    if desc.Filter.0 & 1 == 0
        || desc.Filter.0 & 0x180 != 0
        || desc.MaxLOD.partial_cmp(&desc.MinLOD) != Some(std::cmp::Ordering::Greater)
    {
        return null_mut();
    }
    desc.MipLODBias = (desc.MipLODBias + bias).clamp(-16.0, 15.99);
    let Some(device) = (unsafe { com_ref::owned(rsf_d3d11_output_child_device(context)) }) else {
        return null_mut();
    };
    let device = device.as_raw();
    let mut sampler = null_mut();
    let _ = unsafe {
        (vtable::<ID3D11Device>(&device).CreateSamplerState)(device, &desc, &mut sampler)
    };
    sampler
}

// The context retains the live bindings; release the temporary getter references.
unsafe fn release_range(buffer: *const Handle, count: usize) {
    for &resource in unsafe { slice::from_raw_parts(buffer, count) } {
        drop(unsafe { com_ref::owned(resource) });
    }
}

type GetConstants = unsafe extern "system" fn(Handle, u32, u32, *mut Handle);

unsafe fn get_constants(get: GetConstants, context: Handle, buffers: *mut Handle) {
    unsafe {
        get(context, 0, STAGE_CONSTANTS as u32, buffers);
        release_range(buffers, STAGE_CONSTANTS);
    }
}

pub(super) unsafe fn stage_constants(
    context: Handle,
    geometry: *mut Handle,
    hull: *mut Handle,
    domain: *mut Handle,
) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    unsafe {
        get_constants(api.GSGetConstantBuffers, context, geometry);
        get_constants(api.HSGetConstantBuffers, context, hull);
        get_constants(api.DSGetConstantBuffers, context, domain);
    }
}

pub(super) unsafe fn seed_geometry(
    context: Handle,
    geometry: *mut rsf_frame_tap_geometry,
    pixel: *mut Handle,
) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    let geometry = unsafe { &mut *geometry };
    unsafe {
        (api.IAGetVertexBuffers)(
            context,
            0,
            VERTEX_BUFFERS as u32,
            geometry.vertex_buffers.as_mut_ptr(),
            geometry.strides.as_mut_ptr(),
            geometry.offsets.as_mut_ptr(),
        );
        release_range(geometry.vertex_buffers.as_ptr(), VERTEX_BUFFERS);
        (api.IAGetIndexBuffer)(
            context,
            &mut geometry.index_buffer,
            ptr::from_mut(&mut geometry.index_format).cast(),
            &mut geometry.index_offset,
        );
        drop(com_ref::owned(geometry.index_buffer));
        (api.IAGetInputLayout)(context, &mut geometry.input_layout);
        drop(com_ref::owned(geometry.input_layout));
        (api.IAGetPrimitiveTopology)(context, ptr::from_mut(&mut geometry.topology).cast());
        (api.VSGetShader)(context, &mut geometry.vertex_shader, null_mut(), null_mut());
        drop(com_ref::owned(geometry.vertex_shader));
        get_constants(
            api.VSGetConstantBuffers,
            context,
            geometry.vertex_constants.as_mut_ptr(),
        );
        get_constants(api.PSGetConstantBuffers, context, pixel);
    }
}

// PSGetShader, OMGetBlendState and OMGetDepthStencilState. Like the IA/VS queries above, these
// only seed handles the context currently owns.
pub(super) unsafe fn pipeline_objects(
    context: Handle,
    pixel_shader: &mut Handle,
    blend: &mut Handle,
    depth_stencil: &mut Handle,
) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    unsafe {
        (api.PSGetShader)(context, pixel_shader, null_mut(), null_mut());
        (api.OMGetBlendState)(context, blend, [0.0; 4].as_mut_ptr(), &mut 0);
        (api.OMGetDepthStencilState)(context, depth_stencil, &mut 0);
        for object in [*pixel_shader, *blend, *depth_stencil] {
            drop(com_ref::owned(object));
        }
    }
}

pub(super) unsafe fn draw(original: Handle, context: Handle, call: *const rsf_frame_tap_draw_call) {
    let call = unsafe { &*call };
    match call.kind {
        0 => {
            let draw: unsafe extern "system" fn(Handle, u32, u32) =
                unsafe { std::mem::transmute(original) };
            unsafe { draw(context, call.count, call.start) };
        }
        1 => {
            let draw: unsafe extern "system" fn(Handle, u32, u32, i32) =
                unsafe { std::mem::transmute(original) };
            unsafe { draw(context, call.count, call.start, call.base_vertex) };
        }
        2 => {
            let draw: unsafe extern "system" fn(Handle, u32, u32, u32, u32) =
                unsafe { std::mem::transmute(original) };
            unsafe {
                draw(
                    context,
                    call.count,
                    call.instances,
                    call.start,
                    call.first_instance,
                )
            };
        }
        3 => {
            let draw: unsafe extern "system" fn(Handle, u32, u32, u32, i32, u32) =
                unsafe { std::mem::transmute(original) };
            unsafe {
                draw(
                    context,
                    call.count,
                    call.instances,
                    call.start,
                    call.base_vertex,
                    call.first_instance,
                )
            };
        }
        _ => {}
    }
}
