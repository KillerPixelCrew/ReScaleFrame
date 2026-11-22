// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_output::texture_desc;
use crate::sys::{rsf_d3d11_output_child_device, rsf_d3d11_output_query_texture};
use std::sync::OnceLock;
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
use windows_sys::core::GUID;

unsafe extern "C" {
    fn rsf_frame_tap_foreign_callbacks() -> *const rsf_frame_tap_platform;
}

pub(super) fn platform() -> &'static Platform {
    admitted_platform().expect("frame tap install admits the platform before patching")
}

pub(super) fn admitted_platform() -> Option<&'static Platform> {
    static PLATFORM: OnceLock<Option<Platform>> = OnceLock::new();
    PLATFORM.get_or_init(check_platform).as_ref()
}

fn check_platform() -> Option<Platform> {
    // The C++ boundary contains exceptions from foreign C ABI callbacks.
    let foreign = unsafe { rsf_frame_tap_foreign_callbacks().as_ref() }?;
    Some(Platform {
        patch: patch_slot,
        context1,
        texture_behind,
        view_resource,
        texture_facts,
        target_view_format: target_format,
        buffer_width,
        shader_views,
        set_shader_views,
        set_targets,
        set_viewports,
        set_scissors,
        viewport,
        set_blend,
        save_output,
        patch_blend,
        stage_constants,
        bind_constants,
        seed_geometry,
        draw,
        samplers,
        set_samplers: set_shader_views,
        clone_sampler,
        log: foreign.log?,
        pass: foreign.pass?,
        report: foreign.report?,
        geometry: foreign.geometry?,
        compute: foreign.compute?,
        constants: foreign.constants?,
        gate: foreign.gate?,
        verdict: foreign.verdict?,
        override_constants: foreign.override_constants?,
    })
}

// SDK 10.0.26100.0 d3d11.h and d3d11_1.h C vtables. Each use names the exact
// native argument types (with opaque pointers for COM interfaces and input records).
macro_rules! method {
    ($object:expr, $index:expr, $ty:ty) => {
        std::mem::transmute::<Handle, $ty>(slot($object, $index))
    };
}

unsafe extern "C" fn patch_slot(vtable: *mut Handle, index: u32, replacement: Handle) -> i32 {
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

unsafe extern "C" fn context1(context: Handle) -> i32 {
    i32::from(unsafe { context1_ref(context) }.is_some())
}

pub(super) unsafe fn context1_ref(context: Handle) -> Option<ComRef> {
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
    unsafe { ComRef::from_owned(result) }
}

unsafe extern "C" fn view_resource(view: Handle) -> Handle {
    type Get = unsafe extern "system" fn(Handle, *mut Handle);
    let get = unsafe { method!(view, 7, Get) }; // ID3D11View::GetResource
    let mut resource = null_mut();
    unsafe { get(view, &mut resource) };
    resource
}

unsafe extern "C" fn texture_behind(view: Handle) -> Handle {
    let Some(resource) = (unsafe { ComRef::from_owned(view_resource(view)) }) else {
        return null_mut();
    };
    unsafe { rsf_d3d11_output_query_texture(resource.as_ptr()) }
}

unsafe extern "C" fn texture_facts(texture: Handle, result: *mut rsf_texture_facts) {
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

unsafe extern "C" fn target_format(view: Handle) -> u32 {
    // D3D11_RENDER_TARGET_VIEW_DESC, including the largest union member.
    type Desc = [u32; 5];
    type Get = unsafe extern "system" fn(Handle, *mut Desc);
    let get = unsafe { method!(view, 8, Get) };
    let mut desc = [0; 5];
    unsafe { get(view, &mut desc) };
    desc[0]
}

unsafe extern "C" fn buffer_width(resource: Handle, bind_flags: *mut u32) -> u32 {
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
type GetScissor = unsafe extern "system" fn(Handle, *mut u32, *mut rsf_frame_tap_rect);
type GetBlend = unsafe extern "system" fn(Handle, *mut Handle, *mut f32, *mut u32);
type SetBlend = unsafe extern "system" fn(Handle, Handle, *const f32, u32);

unsafe extern "C" fn shader_views(context: Handle, views: *mut Handle) {
    let get = unsafe { method!(context, 73, GetRange) };
    unsafe { get(context, 0, VIEWS as u32, views) };
}

unsafe extern "C" fn samplers(context: Handle, samplers: *mut Handle) {
    let get = unsafe { method!(context, 75, GetRange) };
    unsafe { get(context, 0, SAMPLERS as u32, samplers) };
}

unsafe extern "C" fn set_shader_views(
    original: Handle,
    context: Handle,
    start: u32,
    count: u32,
    views: *const Handle,
) {
    let set: SetRange = unsafe { std::mem::transmute(original) };
    unsafe { set(context, start, count, views) };
}

unsafe extern "C" fn set_targets(
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

unsafe extern "C" fn set_viewports(
    original: Handle,
    context: Handle,
    count: u32,
    views: *const rsf_frame_tap_viewport,
) {
    type Set = unsafe extern "system" fn(Handle, u32, *const rsf_frame_tap_viewport);
    let set: Set = if original.is_null() {
        unsafe { method!(context, 44, Set) }
    } else {
        unsafe {
            std::mem::transmute::<
                *mut std::ffi::c_void,
                unsafe extern "system" fn(
                    *mut std::ffi::c_void,
                    u32,
                    *const crate::sys::rsf_frame_tap_viewport,
                ),
            >(original)
        }
    };
    unsafe { set(context, count, views) };
}

unsafe extern "C" fn set_scissors(
    original: Handle,
    context: Handle,
    count: u32,
    rects: *const rsf_frame_tap_rect,
) {
    type Set = unsafe extern "system" fn(Handle, u32, *const rsf_frame_tap_rect);
    let set: Set = if original.is_null() {
        unsafe { method!(context, 45, Set) }
    } else {
        unsafe {
            std::mem::transmute::<
                *mut std::ffi::c_void,
                unsafe extern "system" fn(
                    *mut std::ffi::c_void,
                    u32,
                    *const crate::sys::rsf_frame_tap_rect,
                ),
            >(original)
        }
    };
    unsafe { set(context, count, rects) };
}

unsafe extern "C" fn viewport(context: Handle, result: *mut rsf_frame_tap_viewport) -> u32 {
    let get = unsafe { method!(context, 95, GetViewport) };
    let mut count = 1;
    unsafe { get(context, &mut count, result) };
    count
}

unsafe extern "C" fn set_blend(context: Handle, blend: Handle, factor: *const f32, mask: u32) {
    let set = unsafe { method!(context, 35, SetBlend) };
    unsafe { set(context, blend, factor, mask) };
}

unsafe extern "C" fn save_output(context: Handle, result: *mut rsf_frame_tap_output) {
    type GetTargets = unsafe extern "system" fn(Handle, u32, *mut Handle, *mut Handle);
    let result = unsafe { &mut *result };
    unsafe {
        method!(context, 89, GetTargets)(
            context,
            TARGETS,
            result.targets.as_mut_ptr(),
            &mut result.depth_view,
        );
        method!(context, 91, GetBlend)(
            context,
            &mut result.blend,
            result.blend_factor.as_mut_ptr(),
            &mut result.blend_mask,
        );
        result.viewport_count = VIEWPORTS as u32;
        method!(context, 95, GetViewport)(
            context,
            &mut result.viewport_count,
            result.viewports.as_mut_ptr(),
        );
        result.scissor_count = VIEWPORTS as u32;
        method!(context, 96, GetScissor)(
            context,
            &mut result.scissor_count,
            result.scissors.as_mut_ptr(),
        );
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TargetBlend {
    enable: i32,
    source: u32,
    destination: u32,
    operation: u32,
    alpha_source: u32,
    alpha_destination: u32,
    alpha_operation: u32,
    write_mask: u8,
}

#[repr(C)]
#[derive(Default)]
struct BlendDesc {
    alpha_to_coverage: i32,
    independent: i32,
    targets: [TargetBlend; TARGETS as usize],
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

const _: () = {
    assert!(size_of::<TargetBlend>() == 32);
    assert!(size_of::<BlendDesc>() == 264);
    assert!(size_of::<SamplerDesc>() == 52);
};

unsafe extern "C" fn patch_blend(
    context: Handle,
    original: Handle,
    log: *mut rsf_frame_tap_blend_log,
) -> Handle {
    type Get = unsafe extern "system" fn(Handle, *mut BlendDesc);
    type Create = unsafe extern "system" fn(Handle, *const BlendDesc, *mut Handle) -> i32;
    let mut desc = BlendDesc::default();
    let before = if original.is_null() {
        desc.targets[0].write_mask = 0x0f; // D3D11_COLOR_WRITE_ENABLE_ALL
        TargetBlend::default()
    } else {
        unsafe { method!(original, 7, Get)(original, &mut desc) };
        desc.targets[0]
    };
    let count = if desc.independent == 0 {
        1
    } else {
        desc.targets.len()
    };
    for target in &mut desc.targets[..count] {
        target.write_mask |= 0x08; // D3D11_COLOR_WRITE_ENABLE_ALPHA
        if target.enable != 0 {
            // Preserve colour; alpha uses the over operator on coverage.
            target.alpha_source = 2; // D3D11_BLEND_ONE
            target.alpha_destination = 6; // D3D11_BLEND_INV_SRC_ALPHA
            target.alpha_operation = 1; // D3D11_BLEND_OP_ADD
        }
    }
    let Some(device) = (unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(context)) })
    else {
        return null_mut();
    };
    let device = device.as_ptr();
    let mut patched = null_mut();
    if unsafe { method!(device, 20, Create)(device, &desc, &mut patched) } < 0 || patched.is_null()
    {
        return null_mut();
    }
    unsafe {
        log.write(rsf_frame_tap_blend_log {
            enable: before.enable as u32,
            source: before.source,
            destination: before.destination,
            operation: before.operation,
            alpha_source: before.alpha_source,
            alpha_destination: before.alpha_destination,
            alpha_operation: before.alpha_operation,
            write_mask: u32::from(before.write_mask),
            patched_write_mask: u32::from(desc.targets[0].write_mask),
        });
    }
    patched
}

unsafe extern "C" fn clone_sampler(context: Handle, sampler: Handle, bias: f32) -> Handle {
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
    let Some(device) = (unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(context)) })
    else {
        return null_mut();
    };
    let device = device.as_ptr();
    let mut sampler = null_mut();
    unsafe { method!(device, 23, Create)(device, &desc, &mut sampler) };
    sampler
}

// The context retains the live bindings; release the temporary getter references.
unsafe fn release_range(buffer: *const Handle, count: usize) {
    for &resource in unsafe { slice::from_raw_parts(buffer, count) } {
        drop(unsafe { ComRef::from_owned(resource) });
    }
}

unsafe fn get_constants(context: Handle, slot: usize, buffers: *mut Handle) {
    let get = unsafe { method!(context, slot, GetRange) };
    unsafe {
        get(context, 0, STAGE_CONSTANTS as u32, buffers);
        release_range(buffers, STAGE_CONSTANTS);
    }
}

unsafe extern "C" fn stage_constants(
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

unsafe extern "C" fn bind_constants(
    original: Handle,
    context: Handle,
    stage: u32,
    index: u32,
    buffer: Handle,
) {
    let set: SetRange = match stage {
        0 | 1 => unsafe {
            std::mem::transmute::<
                *mut std::ffi::c_void,
                unsafe extern "system" fn(
                    *mut std::ffi::c_void,
                    u32,
                    u32,
                    *const *mut std::ffi::c_void,
                ),
            >(original)
        },
        2 => unsafe { method!(context, 22, SetRange) },
        3 => unsafe { method!(context, 62, SetRange) },
        4 => unsafe { method!(context, 66, SetRange) },
        _ => return,
    };
    unsafe { set(context, index, 1, &buffer) };
}

unsafe extern "C" fn seed_geometry(
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
        drop(ComRef::from_owned(geometry.index_buffer));
        method!(context, 78, GetLayout)(context, &mut geometry.input_layout);
        drop(ComRef::from_owned(geometry.input_layout));
        method!(context, 83, GetTopology)(context, &mut geometry.topology);
        method!(context, 76, GetShader)(
            context,
            &mut geometry.vertex_shader,
            null_mut(),
            null_mut(),
        );
        drop(ComRef::from_owned(geometry.vertex_shader));
        get_constants(context, 72, geometry.vertex_constants.as_mut_ptr());
        get_constants(context, 77, pixel);
    }
}

unsafe extern "C" fn draw(original: Handle, context: Handle, call: *const rsf_frame_tap_draw_call) {
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
