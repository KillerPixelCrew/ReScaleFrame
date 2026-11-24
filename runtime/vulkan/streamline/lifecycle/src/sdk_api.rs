// SPDX-License-Identifier: MIT
//! Typed Streamline 2.14.1 entry points for the Rust call leaves.
//!
//! The interposer exports its API with C linkage, and the references in the
//! C++ declarations are pointers in that ABI. The records are the C mirrors in
//! sdk_records.h, sdk_fg_options.h and sdk_sr_options.h, which
//! sdk_records_checks.h checks against the pinned C++ headers. The entry
//! types use the C-unwind ABI and each call runs inside `guarded`, whose C
//! leaf turns a foreign C++ exception into eErrorExceptionHandler.
//!
//! [`Entries`] is the one table of the interposer's core entry points; the
//! Runtime's D3D owners use it and these types from this crate.

use crate::sys::{
    rsf_slvk_sdk_adapter, rsf_slvk_sdk_constants, rsf_slvk_sdk_feature_version,
    rsf_slvk_sdk_header, rsf_slvk_sdk_preferences, rsf_slvk_sdk_requirements, rsf_slvk_sdk_tag,
    rsf_slvk_sdk_viewport,
};
use std::ffi::{CStr, c_char, c_void};
use std::mem::{size_of, transmute_copy};
use std::ptr;

/// `sl::kSDKVersion` of the pinned headers, passed to `slInit`.
pub const SDK_VERSION: u64 = 0x0002_000e_0001_fedc;
pub const OK: i32 = 0;
/// `sl::Result::eErrorExceptionHandler`.
pub const EXCEPTION_HANDLER: i32 = 24;
/// `sl::Result::eWarnOutOfVRAM`.
pub const OUT_OF_VRAM: i32 = 39;
pub const FEATURE_DLSS: u32 = 0;
pub const FEATURE_REFLEX: u32 = 3;
pub const FEATURE_PCL: u32 = 4;
pub const FEATURE_DLSS_G: u32 = 1000;
pub const FEATURE_COMMON: u32 = u32::MAX;
/// `sl::ResourceLifecycle::eValidUntilEvaluate`.
pub const VALID_UNTIL_EVALUATE: u32 = 2;

/// A borrowed `sl::FrameToken`.
pub type Token = *const c_void;
pub type Commands = *mut c_void;

pub type IsFeatureSupported = unsafe extern "C-unwind" fn(u32, *const rsf_slvk_sdk_adapter) -> i32;
pub type GetFeatureFunction =
    unsafe extern "C-unwind" fn(u32, *const c_char, *mut *mut c_void) -> i32;
pub type SetConstants<C> =
    unsafe extern "C-unwind" fn(*const C, Token, *const rsf_slvk_sdk_viewport) -> i32;
pub type SetTagForFrame<T> = unsafe extern "C-unwind" fn(
    Token,
    *const rsf_slvk_sdk_viewport,
    *const T,
    u32,
    Commands,
) -> i32;
pub type EvaluateFeature = unsafe extern "C-unwind" fn(
    u32,
    Token,
    *const *const rsf_slvk_sdk_header,
    u32,
    Commands,
) -> i32;
pub type FreeResources = unsafe extern "C-unwind" fn(u32, *const rsf_slvk_sdk_viewport) -> i32;
/// `slDLSSSetOptions` and `slDLSSGSetOptions`.
pub type SetOptions<O> = unsafe extern "C-unwind" fn(*const rsf_slvk_sdk_viewport, *const O) -> i32;
pub type GetOptimalSettings<O, S> = unsafe extern "C-unwind" fn(*const O, *mut S) -> i32;
pub type GetFgState<S, O> =
    unsafe extern "C-unwind" fn(*const rsf_slvk_sdk_viewport, *mut S, *const O) -> i32;
pub type Init<P> = unsafe extern "C-unwind" fn(*const P, u64) -> i32;
pub type Shutdown = unsafe extern "C-unwind" fn() -> i32;
pub type GetNewFrameToken = unsafe extern "C-unwind" fn(*mut Token, *const u32) -> i32;
/// `slReflexSetOptions`, `slPCLSetOptions`, `slReflexGetState` and `slPCLGetState`.
pub type Apply<R> = unsafe extern "C-unwind" fn(*const R) -> i32;
pub type Read<R> = unsafe extern "C-unwind" fn(*mut R) -> i32;
pub type ReflexSleep = unsafe extern "C-unwind" fn(Token) -> i32;
pub type PclSetMarker = unsafe extern "C-unwind" fn(u32, Token) -> i32;
/// `PFunOnAPIErrorCallback`, which receives `const APIError&`.
pub type ApiErrorCallback = unsafe extern "C" fn(*const i32);
pub type SetD3DDevice = unsafe extern "C-unwind" fn(*mut c_void) -> i32;
pub type UpgradeInterface = unsafe extern "C-unwind" fn(*mut *mut c_void) -> i32;
pub type GetNativeInterface = unsafe extern "C-unwind" fn(*mut c_void, *mut *mut c_void) -> i32;
pub type GetFeatureRequirements =
    unsafe extern "C-unwind" fn(u32, *mut rsf_slvk_sdk_requirements) -> i32;
pub type GetFeatureVersion =
    unsafe extern "C-unwind" fn(u32, *mut rsf_slvk_sdk_feature_version) -> i32;
pub type SetFeatureLoaded = unsafe extern "C-unwind" fn(u32, bool) -> i32;
pub type AllocateResources =
    unsafe extern "C-unwind" fn(Commands, u32, *const rsf_slvk_sdk_viewport) -> i32;
/// `slHookPresent`: `HRESULT(IDXGISwapChain*, UINT, UINT, bool& skip)`.
pub type HookPresent = unsafe extern "C-unwind" fn(*mut c_void, u32, u32, *mut bool) -> i32;
/// `slHookAfterPresent`: `HRESULT(UINT)`.
pub type HookAfterPresent = unsafe extern "C-unwind" fn(u32) -> i32;

/// The interposer's core entry points, resolved once by name. A missing export is None; each
/// caller checks the entries it requires.
#[derive(Clone, Copy, Default)]
pub struct Entries {
    pub init: Option<Init<rsf_slvk_sdk_preferences>>,
    pub shutdown: Option<Shutdown>,
    pub set_d3d_device: Option<SetD3DDevice>,
    pub upgrade_interface: Option<UpgradeInterface>,
    pub get_native_interface: Option<GetNativeInterface>,
    pub is_feature_supported: Option<IsFeatureSupported>,
    pub get_feature_function: Option<GetFeatureFunction>,
    pub get_feature_requirements: Option<GetFeatureRequirements>,
    pub get_feature_version: Option<GetFeatureVersion>,
    pub set_feature_loaded: Option<SetFeatureLoaded>,
    pub get_new_frame_token: Option<GetNewFrameToken>,
    pub set_constants: Option<SetConstants<rsf_slvk_sdk_constants>>,
    pub set_tag_for_frame: Option<SetTagForFrame<rsf_slvk_sdk_tag>>,
    pub evaluate_feature: Option<EvaluateFeature>,
    pub free_resources: Option<FreeResources>,
    pub allocate_resources: Option<AllocateResources>,
}

impl Entries {
    /// Resolves every entry through `symbol`, which returns the export's address or null.
    ///
    /// # Safety
    /// Each address `symbol` returns must be the named export of a Streamline 2.14.1 interposer.
    pub unsafe fn resolve_with(mut symbol: impl FnMut(&CStr) -> *mut c_void) -> Self {
        unsafe {
            Self {
                init: function(symbol(c"slInit")),
                shutdown: function(symbol(c"slShutdown")),
                set_d3d_device: function(symbol(c"slSetD3DDevice")),
                upgrade_interface: function(symbol(c"slUpgradeInterface")),
                get_native_interface: function(symbol(c"slGetNativeInterface")),
                is_feature_supported: function(symbol(c"slIsFeatureSupported")),
                get_feature_function: function(symbol(c"slGetFeatureFunction")),
                get_feature_requirements: function(symbol(c"slGetFeatureRequirements")),
                get_feature_version: function(symbol(c"slGetFeatureVersion")),
                set_feature_loaded: function(symbol(c"slSetFeatureLoaded")),
                get_new_frame_token: function(symbol(c"slGetNewFrameToken")),
                set_constants: function(symbol(c"slSetConstants")),
                set_tag_for_frame: function(symbol(c"slSetTagForFrame")),
                evaluate_feature: function(symbol(c"slEvaluateFeature")),
                free_resources: function(symbol(c"slFreeResources")),
                allocate_resources: function(symbol(c"slAllocateResources")),
            }
        }
    }

    /// Resolves every entry from the loaded interposer `module`.
    ///
    /// # Safety
    /// `module` is a loaded Streamline 2.14.1 interposer or null.
    #[cfg(windows)]
    pub unsafe fn resolve(module: *mut c_void) -> Self {
        if module.is_null() {
            return Self::default();
        }
        unsafe { Self::resolve_with(|name| proc_address(module, name.as_ptr())) }
    }
}

pub const fn header(
    type_lo: u32,
    type_mid: u16,
    type_hi: u16,
    type_tail: [u8; 8],
    version: usize,
) -> rsf_slvk_sdk_header {
    rsf_slvk_sdk_header {
        next: ptr::null_mut(),
        type_lo,
        type_mid,
        type_hi,
        type_tail,
        version,
    }
}

/// `sl::Resource` and `sl::ResourceTag` headers.
pub const RESOURCE: rsf_slvk_sdk_header = header(
    0x3a9d_70cf,
    0x2418,
    0x4b72,
    [0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61],
    1,
);
pub const TAG: rsf_slvk_sdk_header = header(
    0x4c6a_5aad,
    0xb445,
    0x496c,
    [0x87, 0xff, 0x1a, 0xf3, 0x84, 0x5b, 0xe6, 0x53],
    1,
);
pub const PREFERENCES: rsf_slvk_sdk_header = header(
    0x1ca1_0965,
    0xbf8e,
    0x432b,
    [0x8d, 0xa1, 0x67, 0x16, 0xd8, 0x79, 0xfb, 0x14],
    1,
);
/// `sl::DLSSOptions` v3 and `sl::DLSSOptimalSettings` headers.
pub const DLSS_OPTIONS: rsf_slvk_sdk_header = header(
    0x6ac8_26e4,
    0x4c61,
    0x4101,
    [0xa9, 0x2d, 0x63, 0x8d, 0x42, 0x10, 0x57, 0xb8],
    3,
);
pub const DLSS_OPTIMAL: rsf_slvk_sdk_header = header(
    0xef1d_0957,
    0xfd58,
    0x4df7,
    [0xb5, 0x04, 0x8b, 0x69, 0xd8, 0xaa, 0x6b, 0x76],
    1,
);
/// `sl::ReflexState` v2 and `sl::PCLState` headers.
pub const REFLEX_STATE: rsf_slvk_sdk_header = header(
    0xf0bb_5985,
    0xdaf9,
    0x4728,
    [0xb2, 0xfd, 0xae, 0x80, 0xa2, 0xbd, 0x79, 0x89],
    2,
);
pub const PCL_STATE: rsf_slvk_sdk_header = header(
    0xcfa3_2f9b,
    0x023c,
    0x420e,
    [0x90, 0x56, 0x68, 0x32, 0xb7, 0x4f, 0x89, 0xb5],
    1,
);

pub const fn viewport(value: u32) -> rsf_slvk_sdk_viewport {
    rsf_slvk_sdk_viewport {
        base: header(
            0x171b_6435,
            0x9b3c,
            0x4fc8,
            [0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4],
            1,
        ),
        value,
    }
}

/// Selects the adapter by its Vulkan physical device only.
pub const fn adapter(physical_device: *mut c_void) -> rsf_slvk_sdk_adapter {
    rsf_slvk_sdk_adapter {
        base: header(
            0x0677_315f,
            0xa746,
            0x4492,
            [0x9f, 0x42, 0xcb, 0x61, 0x42, 0xc9, 0xc3, 0xd4],
            1,
        ),
        luid: ptr::null_mut(),
        luid_size: 0,
        physical_device,
    }
}

/// Reads an entry point. Null resolves to None.
///
/// # Safety
/// `T` must be the function pointer type of the named entry.
pub unsafe fn function<T: Copy>(address: *mut c_void) -> Option<T> {
    const { assert!(size_of::<T>() == size_of::<*mut c_void>()) };
    (!address.is_null()).then(|| unsafe { transmute_copy(&address) })
}

/// The address of the export `name` of `module`, or null.
///
/// # Safety
/// `module` is a loaded module and `name` a NUL-terminated string.
#[cfg(windows)]
pub(crate) unsafe fn proc_address(module: *mut c_void, name: *const c_char) -> *mut c_void {
    let address =
        unsafe { windows_sys::Win32::System::LibraryLoader::GetProcAddress(module, name.cast()) };
    address.map_or(ptr::null_mut(), |address| address as *mut c_void)
}

/// Resolves an export from the module without taking a module reference.
///
/// # Safety
/// `module` is a loaded module and `T` the function pointer type of `name`.
#[cfg(windows)]
pub unsafe fn export<T: Copy>(module: *mut c_void, name: &CStr) -> Option<T> {
    if module.is_null() {
        return None;
    }
    unsafe { function(proc_address(module, name.as_ptr())) }
}

/// Resolves one feature function only when Streamline reports success and a
/// nonnull address.
///
/// # Safety
/// `T` must be the function pointer type of the named feature function.
pub unsafe fn feature<T: Copy>(get: GetFeatureFunction, id: u32, name: &CStr) -> Option<T> {
    let mut address = ptr::null_mut();
    let result = guarded(|| unsafe { get(id, name.as_ptr(), &mut address) });
    if result != OK {
        return None;
    }
    unsafe { function(address) }
}

type GuardedCall = unsafe extern "C-unwind" fn(*mut c_void) -> i32;

unsafe extern "C" {
    // sdk_guard.c, compiled by this crate's build script.
    fn rsf_slvk_guarded(call: GuardedCall, user: *mut c_void, thrown: i32) -> i32;
}

/// Runs one SDK call. A foreign C++ exception from the SDK returns
/// eErrorExceptionHandler.
pub fn guarded<F: FnMut() -> i32>(call: F) -> i32 {
    guarded_or(EXCEPTION_HANDLER, call)
}

/// Runs call, which may enter foreign C++ code through C-unwind calls; a foreign C++ exception
/// (MSVC SEH code 0xE06D7363) returns `thrown`. Other faults propagate.
pub fn guarded_or<F: FnMut() -> i32>(thrown: i32, mut call: F) -> i32 {
    unsafe extern "C-unwind" fn thunk<F: FnMut() -> i32>(user: *mut c_void) -> i32 {
        unsafe { (*user.cast::<F>())() }
    }
    unsafe { rsf_slvk_guarded(thunk::<F>, (&raw mut call).cast(), thrown) }
}
