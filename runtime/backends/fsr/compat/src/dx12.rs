// SPDX-License-Identifier: MIT
use super::*;
use windows::{
    Win32::Graphics::{Direct3D12::*, Dxgi::*},
    core::Interface,
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4_compat_adapter_vendor(adapter: *mut c_void) -> u32 {
    let Some(adapter) = (unsafe { IDXGIAdapter1::from_raw_borrowed(&adapter) }) else {
        return 0;
    };
    unsafe { adapter.GetDesc1() }.map_or(0, |desc| desc.VendorId)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4_compat_device_supports_int8(device: *mut c_void) -> bool {
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return false;
    };
    let mut shader_model = D3D12_FEATURE_DATA_SHADER_MODEL {
        HighestShaderModel: D3D_SHADER_MODEL_6_6,
    };
    let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS1::default();
    unsafe {
        device
            .CheckFeatureSupport(
                D3D12_FEATURE_SHADER_MODEL,
                (&raw mut shader_model).cast(),
                size_of_val(&shader_model) as u32,
            )
            .is_ok()
            && shader_model.HighestShaderModel.0 >= D3D_SHADER_MODEL_6_6.0
            && device
                .CheckFeatureSupport(
                    D3D12_FEATURE_D3D12_OPTIONS1,
                    (&raw mut options).cast(),
                    size_of_val(&options) as u32,
                )
                .is_ok()
            && options.WaveOps.as_bool()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4_compat_retain_device(device: *mut c_void) {
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }.unwrap();
    let _ = device.clone().into_raw();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4_compat_release_device(device: *mut c_void) {
    drop(unsafe { ID3D12Device::from_raw(device) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4_compat_acquire_device(
    platform: *const rsf_fsr4_compat_platform,
    module: *mut c_void,
    device: *mut c_void,
    log: rsf_fsr4_compat_log_fn,
    user: *mut c_void,
) -> *const rsf_fsr4_compat_lease {
    if module.is_null() {
        return ptr::null();
    }
    let Some(device_ref) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return ptr::null();
    };
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory4>() }) else {
        return ptr::null();
    };
    let adapter =
        unsafe { factory.EnumAdapterByLuid::<IDXGIAdapter1>(device_ref.GetAdapterLuid()) };
    drop(factory);
    match adapter {
        Ok(adapter) => unsafe {
            rsf_fsr4_compat_acquire_logged(platform, module, adapter.as_raw(), device, log, user)
        },
        Err(_) => ptr::null(),
    }
}
