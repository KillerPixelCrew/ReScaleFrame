// SPDX-License-Identifier: MIT
//! VulkanStory package aliases for the standalone NGX ABI.
use super::{Handle, ResultCode, c_char, c_void};

#[unsafe(export_name = "VulkanStoryNgx_Version")]
extern "C" fn version() -> u32 {
    super::version()
}

#[unsafe(export_name = "VulkanStoryNgx_RuntimeName")]
extern "C" fn runtime_name() -> *const c_char {
    super::runtime_name()
}

#[unsafe(export_name = "VulkanStoryNgx_LastLoadError")]
extern "C" fn last_load_error() -> *const c_char {
    super::last_load_error()
}

#[unsafe(export_name = "VulkanStoryNgx_LoadRuntime")]
extern "C" fn load_runtime() -> ResultCode {
    super::load_runtime()
}

#[unsafe(export_name = "VulkanStoryNgx_VulkanInitProjectId")]
extern "C" fn init_project(
    project: *const c_char,
    engine: i32,
    version: *const c_char,
    data_path: *const c_void,
    instance: Handle,
    physical: Handle,
    device: Handle,
    sdk: i32,
    info: *const c_void,
) -> ResultCode {
    super::init_project(
        project, engine, version, data_path, instance, physical, device, sdk, info,
    )
}

#[unsafe(export_name = "VulkanStoryNgx_VulkanShutdown")]
extern "C" fn shutdown(device: Handle) -> ResultCode {
    super::shutdown(device)
}

#[unsafe(export_name = "VulkanStoryNgx_GetFeatureRequirements")]
extern "C" fn feature_requirements(
    instance: Handle,
    physical: Handle,
    discovery: *const c_void,
    output: Handle,
) -> ResultCode {
    super::feature_requirements(instance, physical, discovery, output)
}

#[unsafe(export_name = "VulkanStoryNgx_EvaluateFeature")]
extern "C" fn evaluate_feature(
    commands: Handle,
    feature: Handle,
    parameters: Handle,
    progress: Handle,
) -> ResultCode {
    super::evaluate_feature(commands, feature, parameters, progress)
}

#[unsafe(export_name = "VulkanStoryNgx_DlssGetOptimalSettings")]
unsafe extern "C" fn optimal_settings(
    parameters: Handle,
    width: u32,
    height: u32,
    quality: i32,
    optimal_width: *mut u32,
    optimal_height: *mut u32,
    min_width: *mut u32,
    min_height: *mut u32,
    max_width: *mut u32,
    max_height: *mut u32,
    sharpness: *mut f32,
) -> ResultCode {
    unsafe {
        super::parameters::optimal_settings(
            parameters,
            width,
            height,
            quality,
            optimal_width,
            optimal_height,
            min_width,
            min_height,
            max_width,
            max_height,
            sharpness,
        )
    }
}

#[unsafe(export_name = "VulkanStoryNgx_GetCapabilityParameters")]
unsafe extern "C" fn capability_parameters(output: *mut Handle) -> ResultCode {
    unsafe { super::capability_parameters(output) }
}

#[unsafe(export_name = "VulkanStoryNgx_AllocateParameters")]
unsafe extern "C" fn allocate_parameters(output: *mut Handle) -> ResultCode {
    unsafe { super::allocate_parameters(output) }
}

#[unsafe(export_name = "VulkanStoryNgx_DestroyParameters")]
extern "C" fn destroy_parameters(handle: Handle) -> ResultCode {
    super::destroy_parameters(handle)
}

#[unsafe(export_name = "VulkanStoryNgx_ReleaseFeature")]
extern "C" fn release_feature(handle: Handle) -> ResultCode {
    super::release_feature(handle)
}

#[unsafe(export_name = "VulkanStoryNgx_GetFeatureInstanceExtensionRequirements")]
unsafe extern "C" fn instance_extensions(
    discovery: *const c_void,
    count: *mut u32,
    output: *mut Handle,
) -> ResultCode {
    unsafe { super::instance_extensions(discovery, count, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_GetFeatureDeviceExtensionRequirements")]
unsafe extern "C" fn device_extensions(
    instance: Handle,
    physical: Handle,
    discovery: *const c_void,
    count: *mut u32,
    output: *mut Handle,
) -> ResultCode {
    unsafe { super::device_extensions(instance, physical, discovery, count, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_CreateFeature")]
unsafe extern "C" fn create_feature(
    commands: Handle,
    feature: i32,
    parameters: Handle,
    output: *mut Handle,
) -> ResultCode {
    unsafe { super::create_feature(commands, feature, parameters, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_CreateFeature1")]
unsafe extern "C" fn create_feature1(
    device: Handle,
    commands: Handle,
    feature: i32,
    parameters: Handle,
    output: *mut Handle,
) -> ResultCode {
    unsafe { super::create_feature1(device, commands, feature, parameters, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterSetULongLong")]
unsafe extern "C" fn set_ull(parameters: Handle, name: *const c_char, value: u64) -> ResultCode {
    unsafe { super::parameters::set_ull(parameters, name, value) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterGetULongLong")]
unsafe extern "C" fn get_ull(
    parameters: Handle,
    name: *const c_char,
    output: *mut u64,
) -> ResultCode {
    unsafe { super::parameters::get_ull(parameters, name, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterSetFloat")]
unsafe extern "C" fn set_float(parameters: Handle, name: *const c_char, value: f32) -> ResultCode {
    unsafe { super::parameters::set_float(parameters, name, value) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterGetFloat")]
unsafe extern "C" fn get_float(
    parameters: Handle,
    name: *const c_char,
    output: *mut f32,
) -> ResultCode {
    unsafe { super::parameters::get_float(parameters, name, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterSetDouble")]
unsafe extern "C" fn set_double(parameters: Handle, name: *const c_char, value: f64) -> ResultCode {
    unsafe { super::parameters::set_double(parameters, name, value) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterGetDouble")]
unsafe extern "C" fn get_double(
    parameters: Handle,
    name: *const c_char,
    output: *mut f64,
) -> ResultCode {
    unsafe { super::parameters::get_double(parameters, name, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterSetUInt")]
unsafe extern "C" fn set_uint(parameters: Handle, name: *const c_char, value: u32) -> ResultCode {
    unsafe { super::parameters::set_uint(parameters, name, value) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterGetUInt")]
unsafe extern "C" fn get_uint(
    parameters: Handle,
    name: *const c_char,
    output: *mut u32,
) -> ResultCode {
    unsafe { super::parameters::get_uint(parameters, name, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterSetInt")]
unsafe extern "C" fn set_int(parameters: Handle, name: *const c_char, value: i32) -> ResultCode {
    unsafe { super::parameters::set_int(parameters, name, value) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterGetInt")]
unsafe extern "C" fn get_int(
    parameters: Handle,
    name: *const c_char,
    output: *mut i32,
) -> ResultCode {
    unsafe { super::parameters::get_int(parameters, name, output) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterSetVoidPointer")]
unsafe extern "C" fn set_pointer(
    parameters: Handle,
    name: *const c_char,
    value: Handle,
) -> ResultCode {
    unsafe { super::parameters::set_pointer(parameters, name, value) }
}

#[unsafe(export_name = "VulkanStoryNgx_ParameterGetVoidPointer")]
unsafe extern "C" fn get_pointer(
    parameters: Handle,
    name: *const c_char,
    output: *mut Handle,
) -> ResultCode {
    unsafe { super::parameters::get_pointer(parameters, name, output) }
}
