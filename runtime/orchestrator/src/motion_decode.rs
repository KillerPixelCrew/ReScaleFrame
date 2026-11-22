// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::SavedBindings;
use crate::graphics_d3d11::{
    GROUP_EDGE, Handle, SourceInfo, SourceViews, compute_shader, compute_texture, constant_buffer,
    context_matches, copy_params, describe_source, dispatch_compute, prepare_compute,
    rsf_call_native_log, rsf_d3d11_state_restore, rsf_d3d11_state_save, shader_code, source_view,
    target_view,
};
use crate::sr_session::has_capacity;
use crate::sys::{RSF_MOTION_DECODE_ABI_VERSION as ABI, rsf_motion_decode_setup as Setup};
use rsf_game_sys::rsf_motion_decode_params as Params;
use std::ffi::{CString, c_void};
use std::ptr::{self, null_mut};
use std::sync::{Mutex, TryLockError};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE, ID3D11Buffer, ID3D11ComputeShader,
    ID3D11Device, ID3D11DeviceContext,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R16G16_FLOAT};
use windows::core::Interface;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const SHADER_FAILED: i32 = -3;
const RESOURCE_FAILED: i32 = -4;
const SOURCE_MISMATCH: i32 = -5;
const PARAMS_V1: usize = std::mem::offset_of!(Params, validity_enabled);

type Resources = [Handle; 4];

#[repr(C)]
struct Constants {
    scale: [f32; 2],
    bias: [f32; 2],
    output_scale: [f32; 2],
    invalid: f32,
    zero_means_unwritten: u32,
    size: [u32; 2],
    validity_enabled: u32,
    validity_channel: u32,
    validity_threshold: f32,
    padding: [u32; 3],
}

const SHADER: &[u8] = include_bytes!("../../graphics/shaders/motion_decode.hlsl");

fn say(setup: &Setup, message: &str) {
    if setup.log.is_some()
        && let Ok(message) = CString::new(message)
    {
        unsafe { rsf_call_native_log(setup.log, setup.log_user, message.as_ptr()) };
    }
}

unsafe fn make_resources(device: Handle, setup: &Setup, bytes: u32, out: &mut Resources) -> i32 {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.expect("live device");
    let shader = match shader_code(SHADER, c"motion_decode", c"main", c"cs_5_0", 0) {
        Ok(code) => compute_shader(device, &code),
        Err(message) => {
            if !message.is_empty() {
                say(setup, &message);
            }
            None
        }
    };
    let Some(shader) = shader else {
        say(setup, "motion decode shader could not be created");
        return SHADER_FAILED;
    };
    let format = if setup.output_format == 0 {
        DXGI_FORMAT_R16G16_FLOAT
    } else {
        DXGI_FORMAT(setup.output_format as i32)
    };
    let Some(texture) = compute_texture(device, setup.width, setup.height, format) else {
        say(setup, "decoded motion target could not be created");
        return RESOURCE_FAILED;
    };
    let Some(view) = target_view(device, &texture, 0) else {
        say(setup, "unordered access view could not be created");
        return RESOURCE_FAILED;
    };
    let Some(buffer) = constant_buffer(device, bytes, true) else {
        say(setup, "constant buffer could not be created");
        return RESOURCE_FAILED;
    };
    *out = [
        shader.into_raw(),
        texture.into_raw(),
        view.into_raw(),
        buffer.into_raw(),
    ];
    OK
}

unsafe fn dispatch(
    context: Handle,
    resources: &Resources,
    source: Handle,
    constants: *const c_void,
    bytes: usize,
    x: u32,
    y: u32,
) -> i32 {
    let raw = context;
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.expect("live context");
    let shader =
        unsafe { ID3D11ComputeShader::from_raw_borrowed(&resources[0]) }.expect("live shader");
    let buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&resources[3]) }.expect("live buffer");
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        if context
            .Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
            .is_err()
        {
            return 0;
        }
        ptr::copy_nonoverlapping(constants.cast::<u8>(), mapped.pData.cast(), bytes);
        context.Unmap(buffer, 0);
    }
    let Some(_saved) =
        (unsafe { SavedBindings::new(raw, rsf_d3d11_state_save, rsf_d3d11_state_restore) })
    else {
        return 0;
    };
    prepare_compute(context);
    dispatch_compute(
        context,
        shader,
        &[source],
        &[resources[2]],
        Some(&[u32::MAX]),
        [x, y],
        false,
        Some(resources[3]),
    );
    1
}

const _: () = {
    assert!(size_of::<Setup>() == 40);
    assert!(size_of::<Constants>() == 64 && PARAMS_V1 == 36);
};

pub(super) struct Pass {
    sources: Mutex<SourceViews<2>>,
    resources: [ComRef; 4],
    device: ComRef,
    setup: Setup,
}

fn constants(params: Params, width: u32, height: u32) -> Option<Constants> {
    let values = [
        params.scale_x,
        params.scale_y,
        params.bias_x,
        params.bias_y,
        params.output_scale_x,
        params.output_scale_y,
        params.invalid_value,
    ];
    if !values.into_iter().all(f32::is_finite)
        || (params.validity_enabled != 0
            && (params.validity_channel > 3 || !params.validity_threshold.is_finite()))
    {
        return None;
    }
    Some(Constants {
        scale: [params.scale_x, params.scale_y],
        bias: [params.bias_x, params.bias_y],
        output_scale: [params.output_scale_x, params.output_scale_y],
        invalid: params.invalid_value,
        zero_means_unwritten: params.zero_means_unwritten,
        size: [width, height],
        validity_enabled: u32::from(params.validity_enabled != 0),
        validity_channel: params.validity_channel,
        validity_threshold: params.validity_threshold,
        padding: [0; 3],
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_motion_decode_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Pass,
) -> i32 {
    if device.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.width == 0 || setup.height == 0 {
        return INVALID;
    }
    if setup.abi_version != ABI {
        return ABI_MISMATCH;
    }
    unsafe { out.write(null_mut()) };
    let mut raw = [null_mut(); 4];
    let status = unsafe { make_resources(device, setup, size_of::<Constants>() as u32, &mut raw) };
    let resources = unsafe { ComRef::adopt_all(raw) };
    if status != OK {
        return status;
    }
    let Some(resources) = resources else {
        return RESOURCE_FAILED;
    };
    let owner = crate::allocate_handle(Pass {
        sources: Mutex::new(SourceViews::new()),
        resources,
        device: unsafe { ComRef::retain(device) },
        setup: *setup,
    });
    let Some(pass) = (unsafe { owner.as_ref() }) else {
        return RESOURCE_FAILED;
    };
    say(
        &pass.setup,
        &format!("motion decode ready for {}x{}", setup.width, setup.height),
    );
    unsafe { out.write(owner) };
    OK
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_motion_decode_run(
    pass: *const Pass,
    context: Handle,
    texture: Handle,
    params: *const Params,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return INVALID;
    };
    if context.is_null() || texture.is_null() {
        return INVALID;
    }
    let Some(params) = (unsafe { copy_params(params, PARAMS_V1) }) else {
        return INVALID;
    };
    let device = pass.device.as_ptr();
    if unsafe { context_matches(device, context) } == 0 {
        return SOURCE_MISMATCH;
    }
    let (width, height) = (pass.setup.width, pass.setup.height);
    let Some(constants) = constants(params, width, height) else {
        return INVALID;
    };
    let mut extent = SourceInfo::default();
    if unsafe { describe_source(device, texture, &mut extent) } == 0 {
        say(&pass.setup, "source belongs to another device");
        return SOURCE_MISMATCH;
    }
    if extent.width != width || extent.height != height || !extent.single_surface() {
        say(
            &pass.setup,
            &format!(
                "source is {}x{} but this pass was built for {width}x{height}",
                extent.width, extent.height
            ),
        );
        return SOURCE_MISMATCH;
    }
    let mut sources = match pass.sources.try_lock() {
        Ok(sources) => sources,
        Err(TryLockError::Poisoned(error)) => error.into_inner(),
        Err(TryLockError::WouldBlock) => return RESOURCE_FAILED,
    };
    let now = unsafe { GetTickCount64() };
    let Some(view) = (unsafe { sources.get(device, texture, 0, now, source_view) }) else {
        drop(sources);
        say(&pass.setup, "source view could not be created");
        return RESOURCE_FAILED;
    };
    let resources = std::array::from_fn(|i| pass.resources[i].as_ptr());
    if unsafe {
        dispatch(
            context,
            &resources,
            view.as_ptr(),
            ptr::from_ref(&constants).cast(),
            size_of::<Constants>(),
            width.div_ceil(GROUP_EDGE),
            height.div_ceil(GROUP_EDGE),
        )
    } != 0
    {
        OK
    } else {
        RESOURCE_FAILED
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_motion_decode_texture(pass: *const Pass) -> Handle {
    unsafe { pass.as_ref() }.map_or(null_mut(), |pass| pass.resources[1].as_ptr())
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_motion_decode_destroy(pass: *mut Pass) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_prefix_does_not_admit_validity_fields() {
        let input = Params {
            struct_size: (PARAMS_V1 + size_of::<u32>()) as u32,
            zero_means_unwritten: 1,
            validity_enabled: 1,
            validity_channel: 99,
            validity_threshold: f32::NAN,
            ..Params::default()
        };
        let copied = unsafe { copy_params(&input, PARAMS_V1) }.unwrap();
        let values = constants(copied, 16, 8).unwrap();
        assert_eq!(values.zero_means_unwritten, 1);
        assert_eq!(values.validity_enabled, 0);

        let full = Params {
            struct_size: size_of::<Params>() as u32,
            ..input
        };
        assert!(constants(unsafe { copy_params(&full, PARAMS_V1) }.unwrap(), 16, 8).is_none());
    }
}
