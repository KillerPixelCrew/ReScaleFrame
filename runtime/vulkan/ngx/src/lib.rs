use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::OnceLock;

const LOAD_ERROR_CAPACITY: usize = 512;

// Matches struct vulkanstory_ngx_entries in vulkanstory_ngx.c.
#[repr(C)]
struct NgxEntries {
    init_project_id: *mut c_void,
    shutdown1: *mut c_void,
    get_capability_params: *mut c_void,
    allocate_params: *mut c_void,
    destroy_params: *mut c_void,
    feature_requirements: *mut c_void,
    instance_extensions: *mut c_void,
    device_extensions: *mut c_void,
    create_feature: *mut c_void,
    create_feature1: *mut c_void,
    evaluate_feature: *mut c_void,
    release_feature: *mut c_void,
}

struct LoadedRuntime {
    _module: *mut c_void,
    entries: NgxEntries,
}

// SAFETY: The module and its symbol addresses are process-retained. Publishing
// the immutable addresses across threads does not access vendor state.
unsafe impl Send for LoadedRuntime {}
// SAFETY: Shared access only reads those immutable addresses.
unsafe impl Sync for LoadedRuntime {}

struct LoadState {
    runtime: Option<LoadedRuntime>,
    error: [c_char; LOAD_ERROR_CAPACITY],
}

static LOAD_STATE: OnceLock<LoadState> = OnceLock::new();

unsafe extern "C" {
    fn rsf_vulkanstory_ngx_open(error: *mut c_char, capacity: usize) -> *mut c_void;
    fn rsf_vulkanstory_ngx_symbol(module: *mut c_void, name: *const c_char) -> *mut c_void;
}

impl LoadState {
    fn load() -> Self {
        let mut error = [0; LOAD_ERROR_CAPACITY];
        // SAFETY: The native loader writes within this buffer and terminates any diagnostic.
        let module = unsafe { rsf_vulkanstory_ngx_open(error.as_mut_ptr(), error.len()) };
        let runtime = if module.is_null() {
            None
        } else {
            let symbol = |name: &CStr| {
                // SAFETY: The loader returned a live module and the name is NUL-terminated.
                unsafe { rsf_vulkanstory_ngx_symbol(module, name.as_ptr()) }
            };
            Some(LoadedRuntime {
                _module: module,
                entries: NgxEntries {
                    init_project_id: symbol(c"NVSDK_NGX_VULKAN_Init_ProjectID"),
                    shutdown1: symbol(c"NVSDK_NGX_VULKAN_Shutdown1"),
                    get_capability_params: symbol(c"NVSDK_NGX_VULKAN_GetCapabilityParameters"),
                    allocate_params: symbol(c"NVSDK_NGX_VULKAN_AllocateParameters"),
                    destroy_params: symbol(c"NVSDK_NGX_VULKAN_DestroyParameters"),
                    feature_requirements: symbol(c"NVSDK_NGX_VULKAN_GetFeatureRequirements"),
                    instance_extensions: symbol(
                        c"NVSDK_NGX_VULKAN_GetFeatureInstanceExtensionRequirements",
                    ),
                    device_extensions: symbol(
                        c"NVSDK_NGX_VULKAN_GetFeatureDeviceExtensionRequirements",
                    ),
                    create_feature: symbol(c"NVSDK_NGX_VULKAN_CreateFeature"),
                    create_feature1: symbol(c"NVSDK_NGX_VULKAN_CreateFeature1"),
                    evaluate_feature: symbol(c"NVSDK_NGX_VULKAN_EvaluateFeature"),
                    release_feature: symbol(c"NVSDK_NGX_VULKAN_ReleaseFeature"),
                },
            })
        };
        Self { runtime, error }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vulkanstory_ngx_get_entries() -> *const NgxEntries {
    LOAD_STATE
        .get_or_init(LoadState::load)
        .runtime
        .as_ref()
        .map_or(ptr::null(), |runtime| &runtime.entries)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vulkanstory_ngx_load_error() -> *const c_char {
    LOAD_STATE
        .get()
        .map_or(c"".as_ptr(), |state| state.error.as_ptr())
}
