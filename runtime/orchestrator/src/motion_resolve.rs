// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::SavedBindings;
use crate::graphics_d3d11::{
    GROUP_EDGE, Handle, MakeView, SourceInfo, SourceViews, compute_shader, compute_texture,
    constant_buffer, context_matches, copy_params, describe_source, dispatch_compute,
    prepare_compute, rsf_d3d11_state_restore, rsf_d3d11_state_save, shader_code, source_view,
    target_view,
};
use crate::render_abi::rsf_motion_resolve_params as Params;
use std::ffi::c_void;
use std::ptr::{self, null_mut};
use std::sync::{Mutex, TryLockError};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R32_FLOAT};
use windows::core::Interface;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

const PARAMS_V1: usize = std::mem::offset_of!(Params, jitter_pixels);
const R32_FLOAT: u32 = 41;

type Resources = [Handle; 6];

#[repr(C)]
struct Constants {
    matrix: [f32; 16],
    scale: [f32; 2],
    sentinel: f32,
    has_sentinel: u32,
    size: [u32; 2],
    has_layer: u32,
    padding: u32,
    jitter: [f32; 2],
    padding2: [f32; 2],
}

#[repr(C)]
struct Platform {
    make: unsafe extern "C" fn(Handle, u32, u32, u32, &mut Resources) -> i32,
    context: unsafe extern "C" fn(Handle, Handle) -> i32,
    describe: unsafe extern "C" fn(Handle, Handle, &mut SourceInfo) -> i32,
    source_view: MakeView,
    dispatch:
        unsafe extern "C" fn(Handle, &Resources, &[Handle; 3], *const c_void, u32, u32) -> i32,
}

const SHADER: &[u8] = include_bytes!("../../graphics/shaders/motion_resolve.hlsl");

unsafe extern "C" fn make_resources(
    device: Handle,
    width: u32,
    height: u32,
    bytes: u32,
    out: &mut Resources,
) -> i32 {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.expect("live device");
    let make = || {
        let code = shader_code(SHADER, c"motion_resolve", c"main", c"cs_5_0", 0).ok()?;
        let shader = compute_shader(device, &code)?;
        let constants = constant_buffer(device, bytes, false)?;
        // R16G16_FLOAT crosses into D3D12. Upstream decoding retains R32G32_FLOAT precision.
        let motion = compute_texture(device, width, height, DXGI_FORMAT_R16G16_FLOAT)?;
        let motion_view = target_view(device, &motion, 0)?;
        let depth = compute_texture(device, width, height, DXGI_FORMAT_R32_FLOAT)?;
        let depth_view = target_view(device, &depth, 0)?;
        Some([
            shader.into_raw(),
            constants.into_raw(),
            motion.into_raw(),
            depth.into_raw(),
            motion_view.into_raw(),
            depth_view.into_raw(),
        ])
    };
    let Some(resources) = make() else {
        return 0;
    };
    *out = resources;
    1
}

unsafe extern "C" fn dispatch(
    context: Handle,
    resources: &Resources,
    sources: &[Handle; 3],
    constants: *const c_void,
    x: u32,
    y: u32,
) -> i32 {
    let raw = context;
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.expect("live context");
    let shader =
        unsafe { ID3D11ComputeShader::from_raw_borrowed(&resources[0]) }.expect("live shader");
    let buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&resources[1]) }.expect("live buffer");
    unsafe { context.UpdateSubresource(buffer, 0, None, constants, 0, 0) };
    let Some(_saved) =
        (unsafe { SavedBindings::new(raw, rsf_d3d11_state_save, rsf_d3d11_state_restore) })
    else {
        return 0;
    };
    prepare_compute(context);
    dispatch_compute(
        context,
        shader,
        sources,
        &[resources[4], resources[5]],
        None,
        [x, y],
        false,
        Some(resources[1]),
    );
    1
}

static PLATFORM: Platform = Platform {
    make: make_resources,
    context: context_matches,
    describe: describe_source,
    source_view,
    dispatch,
};

const _: () = {
    assert!(size_of::<Constants>() == 112 && PARAMS_V1 == 96);
    assert!(size_of::<Platform>() == 5 * size_of::<Handle>());
};

struct Pass {
    sources: Mutex<SourceViews<8>>,
    resources: [ComRef; 6],
    device: ComRef,
    width: u32,
    height: u32,
    platform: &'static Platform,
}

impl Pass {
    fn source(&self, texture: Handle) -> Option<SourceInfo> {
        let mut info = SourceInfo::default();
        if unsafe { (self.platform.describe)(self.device.as_ptr(), texture, &mut info) } == 0
            || !info.single_surface()
            || info.width < self.width
            || info.height < self.height
        {
            return None;
        }
        Some(info)
    }
}

fn constants(params: &Params, width: u32, height: u32) -> Option<Constants> {
    if !params
        .clip_to_previous
        .into_iter()
        .chain(params.decoded_to_pixels)
        .chain([params.sentinel])
        .chain(params.jitter_pixels)
        .all(f32::is_finite)
    {
        return None;
    }
    Some(Constants {
        matrix: params.clip_to_previous,
        scale: params.decoded_to_pixels,
        sentinel: params.sentinel,
        has_sentinel: params.has_sentinel,
        size: [width, height],
        has_layer: 0,
        padding: 0,
        jitter: params.jitter_pixels,
        padding2: [0.0; 2],
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_motion_resolve_create(
    device: Handle,
    width: u32,
    height: u32,
    out: *mut *mut Pass,
) -> i32 {
    if device.is_null() || out.is_null() || width == 0 || height == 0 {
        return 0;
    }
    unsafe { out.write(null_mut()) };
    let platform = &PLATFORM;
    let mut raw = [null_mut(); 6];
    let status = unsafe {
        (platform.make)(
            device,
            width,
            height,
            size_of::<Constants>() as u32,
            &mut raw,
        )
    };
    let resources = unsafe { ComRef::adopt_all(raw) };
    if status == 0 {
        return 0;
    }
    let Some(resources) = resources else {
        return 0;
    };
    let pass = crate::allocate_handle(Pass {
        sources: Mutex::new(SourceViews::new()),
        resources,
        device: unsafe { ComRef::retain(device) },
        width,
        height,
        platform,
    });
    unsafe { out.write(pass) };
    i32::from(!pass.is_null())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_motion_resolve_run(
    pass: *const Pass,
    context: Handle,
    motion: Handle,
    depth: Handle,
    params: *const Params,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return 0;
    };
    if context.is_null() || motion.is_null() || depth.is_null() {
        return 0;
    }
    let Some(params) = (unsafe { copy_params(params, PARAMS_V1) }) else {
        return 0;
    };
    let Some(mut constants) = constants(&params, pass.width, pass.height) else {
        return 0;
    };
    let platform = pass.platform;
    let device = pass.device.as_ptr();
    if unsafe { (platform.context)(device, context) } == 0 {
        return 0;
    }
    let (Some(motion_info), Some(depth_info)) = (pass.source(motion), pass.source(depth)) else {
        return 0;
    };
    let layer = params.depth_layer;
    if !layer.is_null()
        && !pass
            .source(layer)
            .is_some_and(|info| info.format == R32_FLOAT)
    {
        return 0;
    }
    let mut sources = match pass.sources.try_lock() {
        Ok(sources) => sources,
        Err(TryLockError::Poisoned(error)) => error.into_inner(),
        Err(TryLockError::WouldBlock) => return 0,
    };
    let now = unsafe { GetTickCount64() };
    // A later lookup may replace a selected cache slot. Own the view
    // through dispatch even when it is no longer in the cache.
    let Some(motion_view) = (unsafe {
        sources.get(
            device,
            motion,
            motion_info.view_format,
            now,
            platform.source_view,
        )
    })
    .cloned() else {
        return 0;
    };
    let Some(depth_view) = (unsafe {
        sources.get(
            device,
            depth,
            depth_info.view_format,
            now,
            platform.source_view,
        )
    })
    .cloned() else {
        return 0;
    };
    let layer_view = if layer.is_null() {
        None
    } else {
        let Some(view) =
            (unsafe { sources.get(device, layer, R32_FLOAT, now, platform.source_view) }).cloned()
        else {
            return 0;
        };
        Some(view)
    };
    constants.has_layer = layer_view.is_some().into();
    let bindings = [
        motion_view.as_ptr(),
        depth_view.as_ptr(),
        layer_view.as_ref().map_or(null_mut(), ComRef::as_ptr),
    ];
    let resources = std::array::from_fn(|i| pass.resources[i].as_ptr());
    unsafe {
        (platform.dispatch)(
            context,
            &resources,
            &bindings,
            ptr::from_ref(&constants).cast(),
            pass.width.div_ceil(GROUP_EDGE),
            pass.height.div_ceil(GROUP_EDGE),
        )
    }
}

unsafe fn resolve_texture(pass: *const Pass, output: usize) -> Handle {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return null_mut();
    };
    pass.resources[output].as_ptr()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_motion_resolve_motion(pass: *const Pass) -> Handle {
    unsafe { resolve_texture(pass, 2) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_motion_resolve_depth(pass: *const Pass) -> Handle {
    unsafe { resolve_texture(pass, 3) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_motion_resolve_destroy(pass: *mut Pass) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_resolve_parameters_keep_zero_jitter() {
        let input = Params {
            struct_size: PARAMS_V1 as u32,
            clip_to_previous: [0.0; 16],
            decoded_to_pixels: [1.0; 2],
            sentinel: -1000.0,
            has_sentinel: 1,
            depth_layer: null_mut(),
            jitter_pixels: [f32::NAN; 2],
        };
        let legacy = unsafe { copy_params(&input, PARAMS_V1) }.unwrap();
        assert_eq!(constants(&legacy, 8, 8).unwrap().jitter, [0.0; 2]);
        let full = Params {
            struct_size: size_of::<Params>() as u32,
            ..input
        };
        assert!(constants(&unsafe { copy_params(&full, PARAMS_V1) }.unwrap(), 8, 8).is_none());
    }
}
