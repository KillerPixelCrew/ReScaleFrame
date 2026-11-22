// SPDX-License-Identifier: MIT
mod d3d11;
use rsf_orchestrator::sys;

use d3d11::{Checks, Com, Handle, TextureDesc, create_device, create_texture, slot, stage};
use crate::sys::*;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};

const OK: i32 = 0;
const R8G8B8A8_UNORM: u32 = 28;
const MAP_READ: u32 = 1;
const MAP_WRITE_DISCARD: u32 = 4;
const TRIANGLE_LIST: u32 = 4;
const THREE_CONTROL_POINT_PATCH_LIST: u32 = 35;

unsafe extern "C" {
    fn rsf_frame_tap_install(context: Handle, options: *const rsf_frame_tap_options) -> i32;
    fn rsf_frame_tap_uninstall() -> i32;
    fn rsf_frame_tap_set_constant_override(
        callback: rsf_frame_tap_constant_override_fn,
        user: Handle,
    ) -> i32;
    fn rsf_frame_tap_set_constant_override_format(format: u32);
    fn rsf_frame_tap_set_constant_watch(
        bytes: u32,
        callback: rsf_frame_tap_constants_fn,
        user: Handle,
    ) -> i32;
    fn rsf_frame_tap_set_constant_watch_writable(writable: u32);
    fn rsf_frame_tap_set_constant_observer(
        bytes: u32,
        callback: rsf_frame_tap_constant_observer_fn,
        user: Handle,
    ) -> i32;

}

unsafe extern "system" {
    fn D3DCompile(
        src: *const c_void,
        bytes: usize,
        name: *const c_char,
        defines: *const c_void,
        include: Handle,
        entry: *const c_char,
        target: *const c_char,
        flags1: u32,
        flags2: u32,
        code: *mut Handle,
        errors: *mut Handle,
    ) -> i32;
}

// Exact SDK vtable slots for the interface named at each call site.
macro_rules! method {
    ($object:expr, $index:expr, $ty:ty) => {
        std::mem::transmute::<Handle, $ty>(slot($object, $index))
    };
}

const VS_TEXT: &str = r#"cbuffer V:register(b13){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};O main(uint id:SV_VertexID){O o;o.p=float4(id==2?3:-1,id==1?3:-1,0,1);o.r=value.x;return o;}cbuffer P:register(b7){float4 value;} float4 main(float4 p:SV_Position,float r:TEXCOORD0):SV_Target{return float4(r,value.x,0,1);}"#;
const PS_TEXT: &str = r#"cbuffer P:register(b7){float4 value;} float4 main(float4 p:SV_Position,float r:TEXCOORD0):SV_Target{return float4(r,value.x,0,1);}"#;
const HS_TEXT: &str = r#"cbuffer H:register(b5){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};struct P{float e[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};P patch(InputPatch<O,3> v){P p;p.e[0]=p.e[1]=p.e[2]=p.inside=1;return p;}[domain("tri")][partitioning("integer")][outputtopology("triangle_cw")][outputcontrolpoints(3)][patchconstantfunc("patch")]O main(InputPatch<O,3> v,uint id:SV_OutputControlPointID){O o=v[id];o.r+=value.x;return o;}cbuffer D:register(b4){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};struct P{float e[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};[domain("tri")] O main(P p,float3 b:SV_DomainLocation,const OutputPatch<O,3> v){O o;o.p=v[0].p*b.x+v[1].p*b.y+v[2].p*b.z;o.r=v[0].r*b.x+v[1].r*b.y+v[2].r*b.z+value.x;return o;}cbuffer G:register(b6){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};[maxvertexcount(3)] void main(triangle O v[3],inout TriangleStream<O> s){for(uint i=0;i<3;i++){O o=v[i];o.r+=value.x;s.Append(o);}s.RestartStrip();}"#;
const DS_TEXT: &str = r#"cbuffer D:register(b4){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};struct P{float e[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};[domain("tri")] O main(P p,float3 b:SV_DomainLocation,const OutputPatch<O,3> v){O o;o.p=v[0].p*b.x+v[1].p*b.y+v[2].p*b.z;o.r=v[0].r*b.x+v[1].r*b.y+v[2].r*b.z+value.x;return o;}cbuffer G:register(b6){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};[maxvertexcount(3)] void main(triangle O v[3],inout TriangleStream<O> s){for(uint i=0;i<3;i++){O o=v[i];o.r+=value.x;s.Append(o);}s.RestartStrip();}"#;
const GS_TEXT: &str = r#"cbuffer G:register(b6){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};[maxvertexcount(3)] void main(triangle O v[3],inout TriangleStream<O> s){for(uint i=0;i<3;i++){O o=v[i];o.r+=value.x;s.Append(o);}s.RestartStrip();}"#;

fn shader(device: Handle, source: &str, profile: &CStr, create_slot: usize) -> Option<Com> {
    let mut code = Com(null_mut());
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            null(),
            null(),
            null_mut(),
            c"main".as_ptr(),
            profile.as_ptr(),
            0,
            0,
            &mut code.0,
            null_mut(),
        )
    };
    if result < 0 || code.0.is_null() {
        eprintln!("Test shader {profile:?} must compile.");
        return None;
    }
    type Data = unsafe extern "system" fn(Handle) -> *const c_void;
    type Size = unsafe extern "system" fn(Handle) -> usize;
    type Create =
        unsafe extern "system" fn(Handle, *const c_void, usize, Handle, *mut Handle) -> i32;
    let mut shader = Com(null_mut());
    let result = unsafe {
        let data = method!(code.0, 3, Data)(code.0);
        let size = method!(code.0, 4, Size)(code.0);
        method!(device, create_slot, Create)(device, data, size, null_mut(), &mut shader.0)
    };
    if result < 0 || shader.0.is_null() {
        eprintln!("Test shader {profile:?} must be created.");
        return None;
    }
    Some(shader)
}

#[repr(C)]
#[derive(Default)]
struct BufferDesc {
    bytes: u32,
    usage: u32,
    bind_flags: u32,
    access: u32,
    misc: u32,
    stride: u32,
}

#[repr(C)]
struct Initial {
    data: *const c_void,
    row: u32,
    slice: u32,
}

fn buffer(device: Handle, desc: &BufferDesc, contents: *const c_void) -> Com {
    let initial = Initial {
        data: contents,
        row: 0,
        slice: 0,
    };
    let initial = if contents.is_null() { null() } else { &initial };
    type Create =
        unsafe extern "system" fn(Handle, *const BufferDesc, *const Initial, *mut Handle) -> i32;
    let mut result = Com(null_mut());
    unsafe { method!(device, 3, Create)(device, desc, initial, &mut result.0) };
    result
}

fn constant(device: Handle, value: f32) -> Com {
    let contents = [value, 0.0, 0.0, 0.0];
    buffer(
        device,
        &BufferDesc {
            bytes: 16,
            bind_flags: 4,
            ..BufferDesc::default()
        },
        contents.as_ptr().cast(),
    )
}

fn target_view(device: Handle, texture: Handle) -> Com {
    type Create = unsafe extern "system" fn(Handle, Handle, *const c_void, *mut Handle) -> i32;
    let mut view = Com(null_mut());
    if !texture.is_null() {
        unsafe { method!(device, 9, Create)(device, texture, null(), &mut view.0) };
    }
    view
}

#[repr(C)]
#[derive(Default)]
struct Mapped {
    data: *mut c_void,
    row_pitch: u32,
    depth_pitch: u32,
}

struct Context(Handle);

impl Context {
    fn map(&self, resource: Handle, kind: u32) -> Option<Mapped> {
        type Map = unsafe extern "system" fn(Handle, Handle, u32, u32, u32, *mut Mapped) -> i32;
        let mut mapped = Mapped::default();
        (unsafe { method!(self.0, 14, Map)(self.0, resource, 0, kind, 0, &mut mapped) } >= 0)
            .then_some(mapped)
    }

    fn unmap(&self, resource: Handle) {
        type Unmap = unsafe extern "system" fn(Handle, Handle, u32);
        unsafe { method!(self.0, 15, Unmap)(self.0, resource, 0) };
    }

    fn set_constant(&self, method_slot: usize, index: u32, buffer: Handle) {
        type Set = unsafe extern "system" fn(Handle, u32, u32, *const Handle);
        unsafe { method!(self.0, method_slot, Set)(self.0, index, 1, &buffer) };
    }

    fn constant(&self, method_slot: usize, index: u32) -> Com {
        type Get = unsafe extern "system" fn(Handle, u32, u32, *mut Handle);
        let mut buffer = Com(null_mut());
        unsafe { method!(self.0, method_slot, Get)(self.0, index, 1, &mut buffer.0) };
        buffer
    }

    fn set_shader(&self, method_slot: usize, shader: Handle) {
        type Set = unsafe extern "system" fn(Handle, Handle, *const Handle, u32);
        unsafe { method!(self.0, method_slot, Set)(self.0, shader, null(), 0) };
    }

    fn topology(&self, value: u32) {
        type Set = unsafe extern "system" fn(Handle, u32);
        unsafe { method!(self.0, 24, Set)(self.0, value) };
    }

    fn bind_draw(&self, target: Handle, vs: Handle, ps: Handle) {
        type Targets = unsafe extern "system" fn(Handle, u32, *const Handle, Handle);
        type Viewports = unsafe extern "system" fn(Handle, u32, *const [f32; 6]);
        unsafe { method!(self.0, 33, Targets)(self.0, 1, &target, null_mut()) };
        self.set_shader(11, vs);
        self.set_shader(9, ps);
        self.topology(TRIANGLE_LIST);
        let viewport = [0.0, 0.0, 16.0, 16.0, 0.0, 1.0];
        unsafe { method!(self.0, 44, Viewports)(self.0, 1, &viewport) };
    }

    fn draw(&self) {
        type Draw = unsafe extern "system" fn(Handle, u32, u32);
        unsafe { method!(self.0, 13, Draw)(self.0, 3, 0) };
    }

    fn draw_instanced(&self) {
        type Draw = unsafe extern "system" fn(Handle, u32, u32, u32, u32);
        unsafe { method!(self.0, 21, Draw)(self.0, 3, 1, 0, 0) };
    }

    fn copy(&self, target: Handle, source: Handle) {
        type Copy = unsafe extern "system" fn(Handle, Handle, Handle);
        unsafe { method!(self.0, 47, Copy)(self.0, target, source) };
    }

    fn update(&self, target: Handle, data: &[f32; 4]) {
        type Update =
            unsafe extern "system" fn(Handle, Handle, u32, *const c_void, *const c_void, u32, u32);
        unsafe {
            method!(self.0, 48, Update)(self.0, target, 0, null(), data.as_ptr().cast(), 16, 0)
        };
    }

    fn clear(&self) {
        type Clear = unsafe extern "system" fn(Handle);
        unsafe { method!(self.0, 110, Clear)(self.0) };
    }

    fn pixels(&self, checks: &mut Checks, staging: Handle, target: Handle, red: u8, green: u8) {
        self.copy(staging, target);
        let Some(mapped) = self.map(staging, MAP_READ) else {
            checks.check(false, "Readback must map.");
            return;
        };
        if !mapped.data.is_null() {
            let pixel = unsafe {
                mapped
                    .data
                    .cast::<u8>()
                    .add(mapped.row_pitch as usize * 8 + 8 * 4)
            };
            let close =
                unsafe { (*pixel).abs_diff(red) <= 1 && (*pixel.add(1)).abs_diff(green) <= 1 };
            checks.check(
                close,
                "VS and PS must both read their replacements during the draw.",
            );
        } else {
            checks.check(false, "Readback must expose pixel data.");
        }
        self.unmap(staging);
    }

    fn first_float(&self, staging: Handle, target: Handle) -> f32 {
        self.copy(staging, target);
        let Some(mapped) = self.map(staging, MAP_READ) else {
            return -1.0;
        };
        let value = if mapped.data.is_null() {
            -1.0
        } else {
            unsafe { mapped.data.cast::<f32>().read_unaligned() }
        };
        self.unmap(staging);
        value
    }
}

#[derive(Default)]
struct Watched {
    contents: *const c_void,
    bytes: u32,
    calls: u32,
    write: bool,
}

unsafe extern "C" fn observe(user: Handle, _: Handle, contents: *const c_void, bytes: u32) {
    let seen = unsafe { &mut *user.cast::<Watched>() };
    seen.contents = contents;
    seen.bytes = bytes;
    seen.calls += 1;
}

unsafe extern "C" fn watch(user: Handle, buffer: Handle, contents: Handle, bytes: u32) {
    unsafe { observe(user, buffer, contents, bytes) };
    if unsafe { (*user.cast::<Watched>()).write } {
        unsafe { contents.cast::<f32>().write(9.0) };
    }
}

struct Bindings {
    vs: Handle,
    ps: Handle,
    replacement_vs: Handle,
    replacement_ps: Handle,
    extra: Handle,
    replacement_extra: Handle,
    calls: u32,
}

unsafe extern "C" fn replace(
    user: Handle,
    draw: *const rsf_frame_tap_target_draw,
    slots: *mut u32,
    buffers: *mut Handle,
) -> i32 {
    let state = unsafe { &mut *user.cast::<Bindings>() };
    let draw = unsafe { &*draw };
    state.calls += 1;
    if draw.vertex_constants[13] != state.vs || draw.pixel_constants[7] != state.ps {
        return 0;
    }
    // Include a duplicate vertex slot. It must not corrupt the restore list.
    let replacements = [
        (13, state.replacement_vs),
        (14 + 7, state.replacement_ps),
        (13, state.replacement_ps),
    ];
    for (index, (slot, buffer)) in replacements.into_iter().enumerate() {
        unsafe {
            slots.add(index).write(slot);
            buffers.add(index).write(buffer);
        }
    }
    if state.extra.is_null()
        || draw.geometry_constants[6] != state.extra
        || draw.hull_constants[5] != state.extra
        || draw.domain_constants[4] != state.extra
    {
        return 3;
    }
    for (index, slot) in [28 + 6, 42 + 5, 56 + 4].into_iter().enumerate() {
        unsafe {
            slots.add(3 + index).write(slot);
            buffers.add(3 + index).write(state.replacement_extra);
        }
    }
    6
}

fn uploads(checks: &mut Checks, device: Handle, context: &Context) {
    let uploaded = constant(device, 0.0);
    let readback = buffer(
        device,
        &BufferDesc {
            bytes: 16,
            usage: 3,
            access: 0x20000,
            ..BufferDesc::default()
        },
        null(),
    );
    if uploaded.0.is_null() || readback.0.is_null() {
        checks.check(false, "The upload fixture must be created.");
        return;
    }

    let mut seen = Watched {
        write: true,
        ..Watched::default()
    };
    let user = (&raw mut seen).cast();
    let upload = [1.0, 0.0, 0.0, 0.0];
    let data = upload.as_ptr().cast();
    unsafe { rsf_frame_tap_set_constant_watch(16, Some(watch), user) };
    context.update(uploaded.0, &upload);
    checks.check(
        seen.calls == 1 && seen.bytes == 16 && seen.contents != data,
        "A writing watch must be handed a copy of the upload.",
    );
    checks.check(
        upload[0] == 1.0,
        "The game's memory must not be written by the watch.",
    );
    checks.check(
        context.first_float(readback.0, uploaded.0) == 9.0,
        "What the watch writes must be what the buffer receives.",
    );

    unsafe { rsf_frame_tap_set_constant_watch_writable(0) };
    seen.write = false;
    context.update(uploaded.0, &upload);
    checks.check(
        seen.calls == 2 && seen.contents == data,
        "A watch that only reads must be handed the game's memory.",
    );
    checks.check(
        context.first_float(readback.0, uploaded.0) == 1.0,
        "An unwritten upload must arrive as the game made it.",
    );

    unsafe {
        rsf_frame_tap_set_constant_watch_writable(1);
        rsf_frame_tap_set_constant_watch(32, Some(watch), user);
    }
    context.update(uploaded.0, &upload);
    checks.check(
        seen.calls == 2,
        "A buffer of another width must be skipped.",
    );

    unsafe {
        rsf_frame_tap_set_constant_watch_writable(0);
        rsf_frame_tap_set_constant_watch(16, Some(watch), user);
    }
    seen.write = true;
    context.update(uploaded.0, &upload);
    checks.check(
        seen.calls == 3 && seen.contents != data && upload[0] == 1.0,
        "Re-arming the mutable callback must restore writable-copy mode.",
    );
    unsafe { rsf_frame_tap_set_constant_observer(16, Some(observe), user) };
    context.update(uploaded.0, &upload);
    checks.check(
        seen.calls == 4 && seen.contents == data,
        "An observer must see the original upload on its first call.",
    );
    unsafe { rsf_frame_tap_set_constant_observer(0, None, null_mut()) };
}

fn mapping_retirement(
    checks: &mut Checks,
    device: Handle,
    context: &Context,
    options: &rsf_frame_tap_options,
) {
    let dynamic = buffer(
        device,
        &BufferDesc {
            bytes: 16,
            usage: 2,
            bind_flags: 4,
            access: 0x10000,
            ..BufferDesc::default()
        },
        null(),
    );
    if dynamic.0.is_null() {
        checks.check(
            false,
            "The lifetime fixture requires a dynamic constant buffer.",
        );
        return;
    }

    let mut seen = Watched::default();
    let user = (&raw mut seen).cast();
    unsafe { rsf_frame_tap_set_constant_observer(16, Some(observe), user) };
    let first = context.map(dynamic.0, MAP_WRITE_DISCARD).is_some();
    checks.check(first, "The tap must see the first Map.");
    checks.check(
        unsafe { rsf_frame_tap_uninstall() } == OK,
        "The first installation must retire with the Map still pending.",
    );
    if first {
        context.unmap(dynamic.0);
    }
    let second = context.map(dynamic.0, MAP_WRITE_DISCARD).is_some();
    checks.check(
        second,
        "The second Map must succeed outside a tap installation.",
    );
    checks.check(
        unsafe { rsf_frame_tap_install(context.0, options) } == OK,
        "The tap must accept a new installation.",
    );
    unsafe { rsf_frame_tap_set_constant_observer(16, Some(observe), user) };
    if second {
        context.unmap(dynamic.0);
    }
    checks.check(
        seen.calls == 0,
        "An Unmap cannot deliver a Map from an earlier installation.",
    );

    let current = context.map(dynamic.0, MAP_WRITE_DISCARD).is_some();
    checks.check(
        current,
        "The current installation must have a Map to observe.",
    );
    if current {
        context.unmap(dynamic.0);
    }
    checks.check(
        seen.calls == 1 && seen.bytes == 16,
        "The current Map must be delivered once.",
    );
    unsafe { rsf_frame_tap_set_constant_observer(0, None, null_mut()) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_main() -> i32 {
    let Some((device, context_owner)) = create_device(5) else {
        return 77;
    }; // WARP
    let context = Context(context_owner.0);
    stage("creating shaders");
    let Some(vs) = shader(device.0, VS_TEXT, c"vs_5_0", 12) else {
        return 1;
    };
    let Some(ps) = shader(device.0, PS_TEXT, c"ps_5_0", 15) else {
        return 1;
    };
    let Some(hs) = shader(device.0, HS_TEXT, c"hs_5_0", 16) else {
        return 1;
    };
    let Some(ds) = shader(device.0, DS_TEXT, c"ds_5_0", 17) else {
        return 1;
    };
    let Some(gs) = shader(device.0, GS_TEXT, c"gs_5_0", 13) else {
        return 1;
    };

    let originals = [
        constant(device.0, 0.1),
        constant(device.0, 0.2),
        constant(device.0, 0.01),
    ];
    let replacements = [
        constant(device.0, 0.7),
        constant(device.0, 0.8),
        constant(device.0, 0.05),
    ];
    let mut desc = TextureDesc {
        width: 16,
        height: 16,
        mip_levels: 1,
        array_size: 1,
        samples: 1,
        format: R8G8B8A8_UNORM,
        bind_flags: 0x20,
        ..TextureDesc::default()
    };
    let target = unsafe { create_texture(device.0, &desc) };
    let rtv = target_view(device.0, target.0);
    desc.bind_flags = 0;
    desc.usage = 3; // D3D11_USAGE_STAGING
    desc.cpu_access_flags = 0x20000; // D3D11_CPU_ACCESS_READ
    let staging = unsafe { create_texture(device.0, &desc) };
    if originals
        .iter()
        .chain(&replacements)
        .chain([&target, &rtv, &staging])
        .any(|resource| resource.0.is_null())
    {
        eprintln!("Constants, target, view and readback must be created.");
        return 1;
    }

    stage("binding inherited constants before installing the tap");
    let mut bindings = Bindings {
        vs: originals[0].0,
        ps: originals[1].0,
        replacement_vs: replacements[0].0,
        replacement_ps: replacements[1].0,
        extra: null_mut(),
        replacement_extra: null_mut(),
        calls: 0,
    };
    context.set_constant(7, 13, bindings.vs);
    context.set_constant(16, 7, bindings.ps);
    let mut options: rsf_frame_tap_options = unsafe { std::mem::zeroed() };
    options.struct_size = size_of::<rsf_frame_tap_options>() as u32;
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    if unsafe { rsf_frame_tap_install(context.0, &options) } != OK {
        eprintln!("The tap must install.");
        return 1;
    }
    let mut checks = Checks(true);
    unsafe {
        rsf_frame_tap_set_constant_override(Some(replace), (&raw mut bindings).cast());
        rsf_frame_tap_set_constant_override_format(R8G8B8A8_UNORM);
    }
    context.bind_draw(rtv.0, vs.0, ps.0);
    context.draw();
    context.pixels(&mut checks, staging.0, target.0, 179, 204);
    checks.check(
        context.constant(72, 13).0 == bindings.vs && context.constant(77, 7).0 == bindings.ps,
        "Both original stage bindings must be restored.",
    );
    context.draw_instanced();
    context.pixels(&mut checks, staging.0, target.0, 179, 204);

    stage("drawing tessellated geometry");
    bindings.extra = originals[2].0;
    bindings.replacement_extra = replacements[2].0;
    for (method_slot, shader) in [(60, hs.0), (64, ds.0), (23, gs.0)] {
        context.set_shader(method_slot, shader);
    }
    for (method_slot, index) in [(62, 5), (66, 4), (22, 6)] {
        context.set_constant(method_slot, index, bindings.extra);
    }
    context.topology(THREE_CONTROL_POINT_PATCH_LIST);
    context.draw();
    context.pixels(&mut checks, staging.0, target.0, 217, 204);
    for (method_slot, index) in [(81, 6), (100, 5), (104, 4)] {
        checks.check(
            context.constant(method_slot, index).0 == bindings.extra,
            "Later-stage original constants must be restored after the draw.",
        );
    }

    stage("uploading constant buffers");
    uploads(&mut checks, device.0, &context);
    context.clear();
    context.bind_draw(rtv.0, vs.0, ps.0);
    context.draw();
    context.pixels(&mut checks, staging.0, target.0, 0, 0);
    checks.check(
        bindings.calls == 4,
        "The first, instanced, tessellated and post-ClearState draws must each be offered.",
    );
    unsafe { rsf_frame_tap_set_constant_override(None, null_mut()) };
    mapping_retirement(&mut checks, device.0, &context, &options);
    checks.check(
        unsafe { rsf_frame_tap_uninstall() } == OK,
        "The tap must uninstall.",
    );
    context.clear();

    checks.status("constant_override")
}
