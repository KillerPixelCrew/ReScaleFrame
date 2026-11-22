// SPDX-License-Identifier: MIT
use super::*;

use super::platform::{admitted_platform, platform};

// Call an exact STDMETHODCALLTYPE original. The occupied slots were verified against
// the Windows SDK ID3D11DeviceContext and ID3D11DeviceContext1 C vtables.
macro_rules! forward {
    ($hook:expr; $($arg:expr => $ty:ty),+ $(,)?) => {{
        let original = rsf_core_frame_tap_enter(platform(), $hook);
        if !original.is_null() {
            let call: unsafe extern "system" fn($($ty),+) = unsafe { std::mem::transmute(original) };
            unsafe { call($($arg),+) };
            true
        } else {
            false
        }
    }};
}

unsafe extern "system" fn vertex_buffers(
    context: Handle,
    start: u32,
    count: u32,
    buffers: *const Handle,
    strides: *const u32,
    offsets: *const u32,
) {
    forward!(RSF_FRAME_TAP_HOOK_VERTEX_BUFFERS; context => Handle, start => u32, count => u32,
        buffers => *const Handle, strides => *const u32, offsets => *const u32);
    unsafe { rsf_core_frame_tap_vertex_buffers(context, start, count, buffers, strides, offsets) };
}

unsafe extern "system" fn index_buffer(context: Handle, buffer: Handle, format: u32, offset: u32) {
    forward!(RSF_FRAME_TAP_HOOK_INDEX_BUFFER; context => Handle, buffer => Handle, format => u32, offset => u32);
    rsf_core_frame_tap_index_buffer(context, buffer, format, offset);
}

unsafe extern "system" fn layout(context: Handle, value: Handle) {
    forward!(RSF_FRAME_TAP_HOOK_LAYOUT; context => Handle, value => Handle);
    rsf_core_frame_tap_object(context, OBJECT_LAYOUT, value);
}

unsafe extern "system" fn topology(context: Handle, value: u32) {
    forward!(RSF_FRAME_TAP_HOOK_TOPOLOGY; context => Handle, value => u32);
    rsf_core_frame_tap_topology(context, value);
}

unsafe extern "system" fn shader<const HOOK: i32, const OBJECT: u32>(
    context: Handle,
    value: Handle,
    classes: *const Handle,
    count: u32,
) {
    forward!(HOOK; context => Handle, value => Handle, classes => *const Handle, count => u32);
    rsf_core_frame_tap_object(context, OBJECT, value);
}

unsafe extern "system" fn blend(context: Handle, value: Handle, factor: *const f32, mask: u32) {
    forward!(RSF_FRAME_TAP_HOOK_BLEND; context => Handle, value => Handle, factor => *const f32, mask => u32);
    rsf_core_frame_tap_object(context, OBJECT_BLEND, value);
}

unsafe extern "system" fn depth_stencil(context: Handle, value: Handle, reference: u32) {
    forward!(RSF_FRAME_TAP_HOOK_DEPTH_STENCIL; context => Handle, value => Handle, reference => u32);
    rsf_core_frame_tap_object(context, OBJECT_DEPTH_STENCIL, value);
}

unsafe extern "system" fn constants<const HOOK: i32>(
    context: Handle,
    start: u32,
    count: u32,
    buffers: *const Handle,
) {
    forward!(HOOK; context => Handle, start => u32, count => u32, buffers => *const Handle);
    if HOOK == RSF_FRAME_TAP_HOOK_VERTEX_CONSTANTS {
        unsafe { rsf_core_frame_tap_vertex_constants(context, start, count, buffers) };
    } else {
        unsafe { rsf_core_frame_tap_pixel_constants(context, start, count, buffers) };
    }
}

unsafe extern "system" fn views(context: Handle, start: u32, count: u32, views: *const Handle) {
    unsafe { rsf_core_frame_tap_views(platform(), context, start, count, views) };
}

unsafe extern "system" fn targets(
    context: Handle,
    count: u32,
    views: *const Handle,
    depth: Handle,
) {
    unsafe { rsf_core_frame_tap_targets(platform(), context, count, views, depth, null()) };
}

unsafe extern "system" fn targets_uavs(
    context: Handle,
    count: u32,
    views: *const Handle,
    depth: Handle,
    start: u32,
    uav_count: u32,
    uavs: *const Handle,
    initial_counts: *const u32,
) {
    let unordered = rsf_frame_tap_uavs {
        start,
        count: uav_count,
        views: uavs,
        initial_counts,
    };
    unsafe { rsf_core_frame_tap_targets(platform(), context, count, views, depth, &unordered) };
}

unsafe extern "system" fn samplers(
    context: Handle,
    start: u32,
    count: u32,
    samplers: *const Handle,
) {
    let platform = platform();
    let original = rsf_core_frame_tap_enter(platform, RSF_FRAME_TAP_HOOK_SAMPLERS);
    if original.is_null() {
        return;
    }
    let call: unsafe extern "system" fn(Handle, u32, u32, *const Handle) =
        unsafe { std::mem::transmute(original) };
    let mut bound = [null_mut(); SAMPLERS];
    let mut admitted = 0;
    let views = unsafe {
        rsf_core_frame_tap_samplers(
            platform,
            context,
            start,
            count,
            samplers,
            bound.as_mut_ptr(),
            &mut admitted,
        )
    };
    unsafe { call(context, start, count, views) };
    rsf_core_frame_tap_leave(platform, admitted);
}

unsafe extern "system" fn draw(context: Handle, count: u32, start: u32) {
    rsf_core_frame_tap_draw(
        platform(),
        context,
        RSF_FRAME_TAP_HOOK_DRAW,
        &rsf_frame_tap_draw_call {
            kind: 0,
            count,
            start,
            instances: 1,
            first_instance: 0,
            base_vertex: 0,
        },
    );
}

unsafe extern "system" fn indexed(context: Handle, count: u32, start: u32, base_vertex: i32) {
    rsf_core_frame_tap_draw(
        platform(),
        context,
        RSF_FRAME_TAP_HOOK_DRAW_INDEXED,
        &rsf_frame_tap_draw_call {
            kind: 1,
            count,
            start,
            instances: 1,
            first_instance: 0,
            base_vertex,
        },
    );
}

unsafe extern "system" fn instanced(
    context: Handle,
    count: u32,
    instances: u32,
    start: u32,
    first_instance: u32,
) {
    rsf_core_frame_tap_draw(
        platform(),
        context,
        RSF_FRAME_TAP_HOOK_DRAW_INSTANCED,
        &rsf_frame_tap_draw_call {
            kind: 2,
            count,
            start,
            instances,
            first_instance,
            base_vertex: 0,
        },
    );
}

unsafe extern "system" fn indexed_instanced(
    context: Handle,
    count: u32,
    instances: u32,
    start: u32,
    base_vertex: i32,
    first_instance: u32,
) {
    rsf_core_frame_tap_draw(
        platform(),
        context,
        RSF_FRAME_TAP_HOOK_DRAW_INDEXED_INSTANCED,
        &rsf_frame_tap_draw_call {
            kind: 3,
            count,
            start,
            instances,
            first_instance,
            base_vertex,
        },
    );
}

unsafe extern "system" fn draw_auto(context: Handle) {
    forward!(RSF_FRAME_TAP_HOOK_DRAW_AUTO; context => Handle);
    rsf_core_frame_tap_indirect(platform(), context, 4, null_mut(), 0);
}

unsafe extern "system" fn indirect<const HOOK: i32, const KIND: u32>(
    context: Handle,
    args: Handle,
    offset: u32,
) {
    forward!(HOOK; context => Handle, args => Handle, offset => u32);
    rsf_core_frame_tap_indirect(platform(), context, KIND, args, offset);
}

unsafe extern "system" fn clear_state(context: Handle) {
    forward!(RSF_FRAME_TAP_HOOK_CLEAR_STATE; context => Handle);
    rsf_core_frame_tap_invalidate(context, 0);
}

unsafe extern "system" fn execute(context: Handle, list: Handle, restore: i32) {
    forward!(RSF_FRAME_TAP_HOOK_EXECUTE; context => Handle, list => Handle, restore => i32);
    rsf_core_frame_tap_invalidate(context, u32::from(restore != 0));
}

unsafe extern "system" fn map(
    context: Handle,
    resource: Handle,
    subresource: u32,
    kind: u32,
    flags: u32,
    mapped: *mut MappedResource,
) -> i32 {
    unsafe {
        rsf_core_frame_tap_map(
            platform(),
            context,
            resource,
            subresource,
            kind,
            flags,
            mapped,
        )
    }
}

unsafe extern "system" fn unmap(context: Handle, resource: Handle, subresource: u32) {
    unsafe { rsf_core_frame_tap_unmap(platform(), context, resource, subresource) };
}

unsafe extern "system" fn dispatch(context: Handle, x: u32, y: u32, z: u32) {
    forward!(RSF_FRAME_TAP_HOOK_DISPATCH; context => Handle, x => u32, y => u32, z => u32);
    rsf_core_frame_tap_leave(platform(), 0);
    rsf_core_frame_tap_dispatch(platform(), context, x, y, z, null_mut(), 0);
}

unsafe extern "system" fn dispatch_indirect(context: Handle, args: Handle, offset: u32) {
    forward!(RSF_FRAME_TAP_HOOK_DISPATCH_INDIRECT; context => Handle, args => Handle, offset => u32);
    rsf_core_frame_tap_leave(platform(), 0);
    rsf_core_frame_tap_dispatch(platform(), context, 0, 0, 0, args, offset);
}

// These calls only track the runtime's work-submission vtable rewrite. Viewports and scissors
// were hooked for the removed substitution plans; patching them is harmless and keeps the hook ids.
macro_rules! pass_through {
    ($name:ident, $hook:ident $(, $arg:ident: $ty:ty)* $(,)?) => {
        unsafe extern "system" fn $name(context: Handle $(, $arg: $ty)*) {
            forward!($hook; context => Handle $(, $arg => $ty)*);
            rsf_core_frame_tap_leave(platform(), 0);
        }
    };
}

pass_through!(flush, RSF_FRAME_TAP_HOOK_FLUSH);
pass_through!(viewports, RSF_FRAME_TAP_HOOK_VIEWPORTS, count: u32, values: *const c_void);
pass_through!(scissors, RSF_FRAME_TAP_HOOK_SCISSORS, count: u32, values: *const c_void);
pass_through!(clear_target, RSF_FRAME_TAP_HOOK_CLEAR_TARGET, view: Handle, color: *const f32);
pass_through!(copy, RSF_FRAME_TAP_HOOK_COPY, destination: Handle, source: Handle);
pass_through!(copy_region, RSF_FRAME_TAP_HOOK_COPY_REGION, destination: Handle, subresource: u32,
    x: u32, y: u32, z: u32, source: Handle, source_subresource: u32, region: *const c_void);
pass_through!(copy_structure_count, RSF_FRAME_TAP_HOOK_COPY_STRUCTURE_COUNT, destination: Handle,
    offset: u32, source: Handle);
pass_through!(clear_uav_uint, RSF_FRAME_TAP_HOOK_CLEAR_UAV_UINT, view: Handle, values: *const u32);
pass_through!(clear_uav_float, RSF_FRAME_TAP_HOOK_CLEAR_UAV_FLOAT, view: Handle, values: *const f32);
pass_through!(clear_depth, RSF_FRAME_TAP_HOOK_CLEAR_DEPTH, view: Handle, flags: u32, depth: f32, stencil: u8);
pass_through!(generate_mips, RSF_FRAME_TAP_HOOK_GENERATE_MIPS, view: Handle);
pass_through!(resolve, RSF_FRAME_TAP_HOOK_RESOLVE, destination: Handle, subresource: u32,
    source: Handle, source_subresource: u32, format: u32);
pass_through!(copy_region1, RSF_FRAME_TAP_HOOK_COPY_REGION1, destination: Handle, subresource: u32,
    x: u32, y: u32, z: u32, source: Handle, source_subresource: u32, region: *const c_void, flags: u32);
pass_through!(discard_resource, RSF_FRAME_TAP_HOOK_DISCARD_RESOURCE, destination: Handle);

// A whole-buffer constant upload is offered to the observer before it is forwarded.
unsafe extern "system" fn update(
    context: Handle,
    destination: Handle,
    subresource: u32,
    region: *const c_void,
    data: *const c_void,
    row: u32,
    depth: u32,
) {
    let whole = region.is_null();
    unsafe { rsf_core_frame_tap_update(platform(), context, destination, subresource, whole, data, row) };
    forward!(RSF_FRAME_TAP_HOOK_UPDATE; context => Handle, destination => Handle, subresource => u32,
        region => *const c_void, data => *const c_void, row => u32, depth => u32);
    rsf_core_frame_tap_leave(platform(), 0);
}

unsafe extern "system" fn update1(
    context: Handle,
    destination: Handle,
    subresource: u32,
    region: *const c_void,
    data: *const c_void,
    row: u32,
    depth: u32,
    flags: u32,
) {
    let whole = region.is_null();
    unsafe { rsf_core_frame_tap_update(platform(), context, destination, subresource, whole, data, row) };
    forward!(RSF_FRAME_TAP_HOOK_UPDATE1; context => Handle, destination => Handle, subresource => u32,
        region => *const c_void, data => *const c_void, row => u32, depth => u32, flags => u32);
    rsf_core_frame_tap_leave(platform(), 0);
}
const fn hook(slot: u32, context1: u32, function: *const ()) -> rsf_frame_tap_hook {
    rsf_frame_tap_hook {
        slot,
        context1,
        replacement: function.cast_mut().cast(),
    }
}

// Indexed by RSF_FRAME_TAP_HOOK_* in installation and rollback order.
const DETOURS: [rsf_frame_tap_hook; HOOKS] = [
    hook(18, 0, vertex_buffers as _),
    hook(19, 0, index_buffer as _),
    hook(17, 0, layout as _),
    hook(24, 0, topology as _),
    hook(
        11,
        0,
        shader::<RSF_FRAME_TAP_HOOK_VERTEX_SHADER, OBJECT_VERTEX_SHADER> as _,
    ),
    hook(7, 0, constants::<RSF_FRAME_TAP_HOOK_VERTEX_CONSTANTS> as _),
    hook(20, 0, indexed_instanced as _),
    hook(21, 0, instanced as _),
    hook(110, 0, clear_state as _),
    hook(58, 0, execute as _),
    hook(
        39,
        0,
        indirect::<RSF_FRAME_TAP_HOOK_DRAW_INDEXED_INSTANCED_INDIRECT, 5> as _,
    ),
    hook(
        40,
        0,
        indirect::<RSF_FRAME_TAP_HOOK_DRAW_INSTANCED_INDIRECT, 6> as _,
    ),
    hook(38, 0, draw_auto as _),
    hook(
        9,
        0,
        shader::<RSF_FRAME_TAP_HOOK_PIXEL_SHADER, OBJECT_PIXEL_SHADER> as _,
    ),
    hook(35, 0, blend as _),
    hook(36, 0, depth_stencil as _),
    hook(8, 0, views as _),
    hook(16, 0, constants::<RSF_FRAME_TAP_HOOK_PIXEL_CONSTANTS> as _),
    hook(10, 0, samplers as _),
    hook(12, 0, indexed as _),
    hook(13, 0, draw as _),
    hook(33, 0, targets as _),
    hook(34, 0, targets_uavs as _),
    hook(44, 0, viewports as _),
    hook(45, 0, scissors as _),
    hook(50, 0, clear_target as _),
    hook(14, 0, map as _),
    hook(15, 0, unmap as _),
    hook(111, 0, flush as _),
    hook(41, 0, dispatch as _),
    hook(42, 0, dispatch_indirect as _),
    hook(46, 0, copy_region as _),
    hook(47, 0, copy as _),
    hook(48, 0, update as _),
    hook(49, 0, copy_structure_count as _),
    hook(51, 0, clear_uav_uint as _),
    hook(52, 0, clear_uav_float as _),
    hook(53, 0, clear_depth as _),
    hook(54, 0, generate_mips as _),
    hook(57, 0, resolve as _),
    hook(115, 1, copy_region1 as _),
    hook(116, 1, update1 as _),
    hook(117, 1, discard_resource as _),
];

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_tap_install(
    context: Handle,
    options: *const rsf_frame_tap_options,
) -> i32 {
    let Some(platform) = admitted_platform() else {
        return INVALID;
    };
    unsafe { rsf_core_frame_tap_install(platform, context, options, DETOURS.as_ptr()) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_sampler_bias(bias: f32) -> i32 {
    let Some(platform) = admitted_platform() else {
        return INVALID;
    };
    rsf_core_frame_tap_set_sampler_bias(platform, bias)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_refresh() -> i32 {
    let Some(platform) = admitted_platform() else {
        return INVALID;
    };
    rsf_core_frame_tap_refresh(platform)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_uninstall() -> i32 {
    let Some(platform) = admitted_platform() else {
        return INVALID;
    };
    rsf_core_frame_tap_uninstall(platform)
}

