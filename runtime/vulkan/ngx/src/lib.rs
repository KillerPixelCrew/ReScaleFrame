// SPDX-License-Identifier: MIT
//! NGX Vulkan's native caller module with its original flat C ABI.
mod loader;
mod parameters;

use std::ffi::{CStr, c_char, c_void};
use std::ptr::{self, NonNull};
use std::sync::OnceLock;

type Handle = *mut c_void;
type ResultCode = u32;
const SUCCESS: ResultCode = 1;
const RUNTIME_MISSING: ResultCode = 0xBAD1_0001;
const ENTRY_MISSING: ResultCode = 0xBAD1_0002;
const INVALID: ResultCode = 0xBAD1_0003;
const LOAD_ERROR_CAPACITY: usize = 512;

macro_rules! vendor_entries {
    ($($field:ident: $symbol:literal ($($arg:ty),*);)*) => {
        struct Entries {
            $($field: Option<unsafe extern "C" fn($($arg),*) -> ResultCode>),*
        }

        impl Entries {
            fn resolve(module: NonNull<c_void>) -> Self {
                Self {
                    $($field: NonNull::new(loader::symbol(module, $symbol)).map(|symbol| {
                        // The x64 vendor entry's signature is mapped below.
                        unsafe { std::mem::transmute::<Handle, unsafe extern "C" fn($($arg),*) -> ResultCode>(symbol.as_ptr()) }
                    })),*
                }
            }
        }
    };
}

// The init and shutdown shapes follow the driver entry points recorded in
// docs/research/rust-vulkanstory-ngx-loader.md. The SDK header exposes wrappers.
vendor_entries! {
    init_project: c"NVSDK_NGX_VULKAN_Init_ProjectID"(
        *const c_char, i32, *const c_char, *const c_void, Handle, Handle, Handle, i32, *const c_void
    );
    shutdown: c"NVSDK_NGX_VULKAN_Shutdown1"(Handle, *mut i32);
    capability_parameters: c"NVSDK_NGX_VULKAN_GetCapabilityParameters"(*mut Handle);
    allocate_parameters: c"NVSDK_NGX_VULKAN_AllocateParameters"(*mut Handle);
    destroy_parameters: c"NVSDK_NGX_VULKAN_DestroyParameters"(Handle);
    feature_requirements: c"NVSDK_NGX_VULKAN_GetFeatureRequirements"(Handle, Handle, *const c_void, Handle);
    instance_extensions: c"NVSDK_NGX_VULKAN_GetFeatureInstanceExtensionRequirements"(*const c_void, *mut u32, *mut Handle);
    device_extensions: c"NVSDK_NGX_VULKAN_GetFeatureDeviceExtensionRequirements"(Handle, Handle, *const c_void, *mut u32, *mut Handle);
    create_feature: c"NVSDK_NGX_VULKAN_CreateFeature"(Handle, i32, Handle, *mut Handle);
    create_feature1: c"NVSDK_NGX_VULKAN_CreateFeature1"(Handle, Handle, i32, Handle, *mut Handle);
    evaluate_feature: c"NVSDK_NGX_VULKAN_EvaluateFeature"(Handle, Handle, Handle, Handle);
    release_feature: c"NVSDK_NGX_VULKAN_ReleaseFeature"(Handle);
}

struct LoadState {
    runtime: Result<Entries, [u8; LOAD_ERROR_CAPACITY]>,
}

static LOAD_STATE: OnceLock<LoadState> = OnceLock::new();

fn load() -> &'static LoadState {
    LOAD_STATE.get_or_init(|| LoadState {
        // The acquired module reference is intentionally retained for the process.
        runtime: loader::open().map(Entries::resolve).map_err(|message| {
            let mut error = [0; LOAD_ERROR_CAPACITY];
            let count = message.len().min(error.len() - 1);
            error[..count].copy_from_slice(&message[..count]);
            error
        }),
    })
}

macro_rules! require {
    ($field:ident) => {{
        let Ok(entries) = &load().runtime else {
            return RUNTIME_MISSING;
        };
        let Some(call) = entries.$field else {
            return ENTRY_MISSING;
        };
        call
    }};
}

/// Keep post-call work in this native module even when the optimizer can inline
/// the helper. The driver resolves its caller's module from the return address.
#[inline(always)]
fn caller_result(result: ResultCode) -> ResultCode {
    unsafe { ptr::read_volatile(&result) }
}

#[unsafe(export_name = "VulkanStoryNgx_Version")]
extern "C" fn version() -> u32 {
    1
}

#[unsafe(export_name = "VulkanStoryNgx_RuntimeName")]
extern "C" fn runtime_name() -> *const c_char {
    loader::NAME.as_ptr()
}

#[unsafe(export_name = "VulkanStoryNgx_LastLoadError")]
extern "C" fn last_load_error() -> *const c_char {
    LOAD_STATE
        .get()
        .and_then(|state| state.runtime.as_ref().err())
        .map_or(c"".as_ptr(), |error| error.as_ptr().cast())
}

#[unsafe(export_name = "VulkanStoryNgx_LoadRuntime")]
extern "C" fn load_runtime() -> ResultCode {
    if load().runtime.is_ok() {
        SUCCESS
    } else {
        RUNTIME_MISSING
    }
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
    let call = require!(init_project);
    if project.is_null() || device.is_null() {
        return INVALID;
    }
    caller_result(unsafe {
        call(
            project, engine, version, data_path, instance, physical, device, sdk, info,
        )
    })
}

#[unsafe(export_name = "VulkanStoryNgx_VulkanShutdown")]
extern "C" fn shutdown(device: Handle) -> ResultCode {
    let call = require!(shutdown);
    // Linux 615.71.09 writes through the driver's second argument.
    let mut remaining = 0;
    caller_result(unsafe { call(device, &mut remaining) })
}

macro_rules! out_pointer_entry {
    ($export:literal, $name:ident) => {
        #[unsafe(export_name = $export)]
        unsafe extern "C" fn $name(output: *mut Handle) -> ResultCode {
            let call = require!($name);
            if output.is_null() {
                return INVALID;
            }
            unsafe { output.write(ptr::null_mut()) };
            caller_result(unsafe { call(output) })
        }
    };
}

out_pointer_entry!(
    "VulkanStoryNgx_GetCapabilityParameters",
    capability_parameters
);
out_pointer_entry!("VulkanStoryNgx_AllocateParameters", allocate_parameters);

macro_rules! handle_entry {
    ($export:literal, $name:ident) => {
        #[unsafe(export_name = $export)]
        extern "C" fn $name(handle: Handle) -> ResultCode {
            let call = require!($name);
            if handle.is_null() {
                return INVALID;
            }
            caller_result(unsafe { call(handle) })
        }
    };
}

handle_entry!("VulkanStoryNgx_DestroyParameters", destroy_parameters);
handle_entry!("VulkanStoryNgx_ReleaseFeature", release_feature);

#[unsafe(export_name = "VulkanStoryNgx_GetFeatureRequirements")]
extern "C" fn feature_requirements(
    instance: Handle,
    physical: Handle,
    discovery: *const c_void,
    output: Handle,
) -> ResultCode {
    let call = require!(feature_requirements);
    if discovery.is_null() || output.is_null() {
        return INVALID;
    }
    caller_result(unsafe { call(instance, physical, discovery, output) })
}

macro_rules! extension_entry {
    ($export:literal, $name:ident $(, $arg:ident)*) => {
        #[unsafe(export_name = $export)]
        unsafe extern "C" fn $name($($arg: Handle,)* discovery: *const c_void, count: *mut u32, output: *mut Handle) -> ResultCode {
            let call = require!($name);
            if discovery.is_null() || count.is_null() || output.is_null() {
                return INVALID;
            }
            unsafe {
                count.write(0);
                output.write(ptr::null_mut());
            }
            caller_result(unsafe { call($($arg,)* discovery, count, output) })
        }
    };
}

extension_entry!(
    "VulkanStoryNgx_GetFeatureInstanceExtensionRequirements",
    instance_extensions
);
extension_entry!(
    "VulkanStoryNgx_GetFeatureDeviceExtensionRequirements",
    device_extensions,
    instance,
    physical
);

macro_rules! create_entry {
    ($export:literal, $name:ident $(, $arg:ident)*) => {
        #[unsafe(export_name = $export)]
        unsafe extern "C" fn $name($($arg: Handle,)* commands: Handle, feature: i32, parameters: Handle, output: *mut Handle) -> ResultCode {
            let call = require!($name);
            if parameters.is_null() || output.is_null() {
                return INVALID;
            }
            unsafe { output.write(ptr::null_mut()) };
            caller_result(unsafe { call($($arg,)* commands, feature, parameters, output) })
        }
    };
}

create_entry!("VulkanStoryNgx_CreateFeature", create_feature);
create_entry!("VulkanStoryNgx_CreateFeature1", create_feature1, device);

#[unsafe(export_name = "VulkanStoryNgx_EvaluateFeature")]
extern "C" fn evaluate_feature(
    commands: Handle,
    feature: Handle,
    parameters: Handle,
    progress: Handle,
) -> ResultCode {
    let call = require!(evaluate_feature);
    if feature.is_null() || parameters.is_null() {
        return INVALID;
    }
    caller_result(unsafe { call(commands, feature, parameters, progress) })
}
