// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_output::texture_desc;
use crate::d3d11_output_platform::{rsf_d3d11_output_child_device, rsf_d3d11_output_query_texture};
use windows::core::IUnknown;
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
use windows_sys::core::GUID;

// SDK 10.0.26100.0 d3d11.h and d3d11_1.h C vtables. Each use names the exact
// native argument types (with opaque pointers for COM interfaces and input records).
macro_rules! method {
    ($object:expr, $index:expr, $ty:ty) => {
        std::mem::transmute::<Handle, $ty>(slot($object, $index))
    };
}

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
    i32::from(unsafe { context1_ref(context) }.is_some())
}

pub(super) unsafe fn context1_ref(context: Handle) -> Option<IUnknown> {
    // IID_ID3D11DeviceContext1, d3d11_1.h.
    const IID: GUID = GUID {
        data1: 0xbb2c6faa,
        data2: 0xb5fb,
        data3: 0x4082,
        data4: [0x8e, 0x6b, 0x38, 0x8b, 0x8c, 0xfa, 0x90, 0xe1],
    };
    type Query = unsafe extern "system" fn(Handle, *const GUID, *mut Handle) -> i32;
    let query = unsafe { method!(context, 0, Query) };
    let mut result = null_mut();
    if unsafe { query(context, &IID, &mut result) } < 0 {
        return None;
    }
    unsafe { com_ref::owned(result) }
}

unsafe fn view_resource(view: Handle) -> Handle {
    type Get = unsafe extern "system" fn(Handle, *mut Handle);
    let get = unsafe { method!(view, 7, Get) }; // ID3D11View::GetResource
    let mut resource = null_mut();
    unsafe { get(view, &mut resource) };
    resource
}

pub(super) unsafe fn texture_behind(view: Handle) -> Handle {
    let Some(resource) = (unsafe { com_ref::owned(view_resource(view)) }) else {
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
    // D3D11_RENDER_TARGET_VIEW_DESC, including the largest union member.
    type Desc = [u32; 5];
    type Get = unsafe extern "system" fn(Handle, *mut Desc);
    let get = unsafe { method!(view, 8, Get) };
    let mut desc = [0; 5];
    unsafe { get(view, &mut desc) };
    desc[0]
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

type GetRange = unsafe extern "system" fn(Handle, u32, u32, *mut Handle);
type SetRange = unsafe extern "system" fn(Handle, u32, u32, *const Handle);
type GetViewport = unsafe extern "system" fn(Handle, *mut u32, *mut rsf_frame_tap_viewport);

pub(super) unsafe fn shader_views(context: Handle, views: *mut Handle) {
    let get = unsafe { method!(context, 73, GetRange) };
    unsafe { get(context, 0, VIEWS as u32, views) };
}

pub(super) unsafe fn samplers(context: Handle, samplers: *mut Handle) {
    let get = unsafe { method!(context, 75, GetRange) };
    unsafe { get(context, 0, SAMPLERS as u32, samplers) };
}

pub(super) unsafe fn set_shader_views(
    original: Handle,
    context: Handle,
    start: u32,
    count: u32,
    views: *const Handle,
) {
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

pub(super) unsafe fn viewport(context: Handle, result: *mut rsf_frame_tap_viewport) -> u32 {
    let get = unsafe { method!(context, 95, GetViewport) };
    let mut count = 1;
    unsafe { get(context, &mut count, result) };
    count
}

#[repr(C)]
#[derive(Default)]
struct SamplerDesc {
    filter: u32,
    address_u: u32,
    address_v: u32,
    address_w: u32,
    bias: f32,
    anisotropy: u32,
    comparison: u32,
    border: [f32; 4],
    min_lod: f32,
    max_lod: f32,
}

const _: () = assert!(size_of::<SamplerDesc>() == 52);

pub(super) unsafe fn clone_sampler(context: Handle, sampler: Handle, bias: f32) -> Handle {
    type Get = unsafe extern "system" fn(Handle, *mut SamplerDesc);
    type Create = unsafe extern "system" fn(Handle, *const SamplerDesc, *mut Handle) -> i32;
    let mut desc = SamplerDesc::default();
    unsafe { method!(sampler, 7, Get)(sampler, &mut desc) };
    if desc.filter & 1 == 0
        || desc.filter & 0x180 != 0
        || desc.max_lod.partial_cmp(&desc.min_lod) != Some(std::cmp::Ordering::Greater)
    {
        return null_mut();
    }
    desc.bias = (desc.bias + bias).clamp(-16.0, 15.99);
    let Some(device) = (unsafe { com_ref::owned(rsf_d3d11_output_child_device(context)) }) else {
        return null_mut();
    };
    let device = device.as_raw();
    let mut sampler = null_mut();
    unsafe { method!(device, 23, Create)(device, &desc, &mut sampler) };
    sampler
}

// The context retains the live bindings; release the temporary getter references.
unsafe fn release_range(buffer: *const Handle, count: usize) {
    for &resource in unsafe { slice::from_raw_parts(buffer, count) } {
        drop(unsafe { com_ref::owned(resource) });
    }
}

unsafe fn get_constants(context: Handle, slot: usize, buffers: *mut Handle) {
    let get = unsafe { method!(context, slot, GetRange) };
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
    unsafe {
        get_constants(context, 81, geometry);
        get_constants(context, 100, hull);
        get_constants(context, 104, domain);
    }
}

pub(super) unsafe fn seed_geometry(
    context: Handle,
    geometry: *mut rsf_frame_tap_geometry,
    pixel: *mut Handle,
) {
    type GetVertex = unsafe extern "system" fn(Handle, u32, u32, *mut Handle, *mut u32, *mut u32);
    type GetIndex = unsafe extern "system" fn(Handle, *mut Handle, *mut u32, *mut u32);
    type GetLayout = unsafe extern "system" fn(Handle, *mut Handle);
    type GetTopology = unsafe extern "system" fn(Handle, *mut u32);
    type GetShader = unsafe extern "system" fn(Handle, *mut Handle, *mut Handle, *mut u32);
    let geometry = unsafe { &mut *geometry };
    unsafe {
        method!(context, 79, GetVertex)(
            context,
            0,
            VERTEX_BUFFERS as u32,
            geometry.vertex_buffers.as_mut_ptr(),
            geometry.strides.as_mut_ptr(),
            geometry.offsets.as_mut_ptr(),
        );
        release_range(geometry.vertex_buffers.as_ptr(), VERTEX_BUFFERS);
        method!(context, 80, GetIndex)(
            context,
            &mut geometry.index_buffer,
            &mut geometry.index_format,
            &mut geometry.index_offset,
        );
        drop(com_ref::owned(geometry.index_buffer));
        method!(context, 78, GetLayout)(context, &mut geometry.input_layout);
        drop(com_ref::owned(geometry.input_layout));
        method!(context, 83, GetTopology)(context, &mut geometry.topology);
        method!(context, 76, GetShader)(
            context,
            &mut geometry.vertex_shader,
            null_mut(),
            null_mut(),
        );
        drop(com_ref::owned(geometry.vertex_shader));
        get_constants(context, 72, geometry.vertex_constants.as_mut_ptr());
        get_constants(context, 77, pixel);
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
