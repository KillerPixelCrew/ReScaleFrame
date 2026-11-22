// SPDX-License-Identifier: MIT
//! Typed Streamline calls behind the native D3D12 DLSS SR registration. The registration leases
//! the interposer module and keeps the borrowed plugin directory alive; the SDK exports are plain C
//! entry points taking record pointers.
use super::*;
use crate::streamline_sr::native::{
    rsf_dlss_sr_load_sdk, rsf_dlss_sr_shared_module, rsf_dlss_sr_validate_d3d12, verify_signature,
};
use crate::streamline_sr::sdk::{
    self as records, ConstantsFn, EvaluateFn, FEATURE_COMMON, FEATURE_DLSS, FeatureFn, FreeFn,
    HookAfterPresentFn, HookPresentFn, InitFn, OptimalFn, OptionsFn, PREFERENCES, RESULT_OK,
    SDK_VERSION, SetDeviceFn, ShutdownFn, SupportedFn, TagRecords, TagsFn, TokenFn,
};
use crate::vulkan_streamline_camera::{rsf_slvk_sdk_header, rsf_slvk_sdk_preferences};
use std::mem::transmute_copy;

/// `LogLevel::eDefault`.
const LOG_DEFAULT: u32 = 1;

unsafe extern "C" {
    fn rsf_dlss_sdk_compiled() -> u32;
    fn rsf_streamline_host_token(host: Handle, frame_id: u64) -> Handle;
}

struct Native {
    // The SDK borrows this array and the registration owns the string through shutdown.
    plugin_paths: [*const u16; 1],
    feature_id: u32,
    init: Option<InitFn>,
    set_device: Option<SetDeviceFn>,
    support: Option<SupportedFn>,
    feature: Option<FeatureFn>,
    shutdown: Option<ShutdownFn>,
    token: Option<TokenFn>,
    constants: Option<ConstantsFn>,
    tags: Option<TagsFn>,
    evaluate: Option<EvaluateFn>,
    free_resources: Option<FreeFn>,
    optimal: Option<OptimalFn>,
    options: Option<OptionsFn>,
    present: Option<HookPresentFn>,
    after_present: Option<HookAfterPresentFn>,
}

unsafe fn native<'a>(handle: Handle) -> &'a mut Native {
    unsafe { &mut *handle.cast::<Native>() }
}

unsafe fn typed<F: Copy>(address: Handle) -> Option<F> {
    (!address.is_null()).then(|| unsafe { transmute_copy::<Handle, F>(&address) })
}

unsafe extern "C" fn verify(path: *const u16) -> bool {
    unsafe { verify_signature(path) }
}

unsafe extern "C" fn allocate(directory: *const u16) -> Handle {
    Box::into_raw(Box::new(Native {
        plugin_paths: [directory],
        feature_id: 0,
        init: None,
        set_device: None,
        support: None,
        feature: None,
        shutdown: None,
        token: None,
        constants: None,
        tags: None,
        evaluate: None,
        free_resources: None,
        optimal: None,
        options: None,
        present: None,
        after_present: None,
    }))
    .cast()
}

unsafe extern "C" fn bind_core(handle: Handle, entries: &CoreEntries) {
    let native = unsafe { native(handle) };
    // The order is CORE_NAMES.
    let [
        init,
        set_device,
        support,
        feature,
        shutdown,
        token,
        constants,
        tags,
        evaluate,
        free_resources,
    ] = *entries;
    unsafe {
        native.init = typed(init);
        native.set_device = typed(set_device);
        native.support = typed(support);
        native.feature = typed(feature);
        native.shutdown = typed(shutdown);
        native.token = typed(token);
        native.constants = typed(constants);
        native.tags = typed(tags);
        native.evaluate = typed(evaluate);
        native.free_resources = typed(free_resources);
    }
}

unsafe extern "C" fn init(handle: Handle, input: &Preferences) -> i32 {
    let native = unsafe { native(handle) };
    native.feature_id = input.feature;
    let preferences = rsf_slvk_sdk_preferences {
        base: PREFERENCES,
        log_level: LOG_DEFAULT,
        plugin_paths: native.plugin_paths.as_mut_ptr(),
        num_plugin_paths: 1,
        flags: input.flags,
        features: &raw const native.feature_id,
        num_features: 1,
        application_id: input.application_id,
        engine: input.engine,
        engine_version: input.engine_version,
        project: input.project_id,
        render_api: input.render_api,
        ..Default::default()
    };
    let Some(init) = native.init else {
        return INIT_FAILED;
    };
    if unsafe { init(&preferences, SDK_VERSION) } == RESULT_OK {
        OK
    } else {
        INIT_FAILED
    }
}

unsafe extern "C" fn set_device(handle: Handle, device: Handle) -> i32 {
    let Some(set_device) = unsafe { native(handle) }.set_device else {
        return INIT_FAILED;
    };
    if unsafe { set_device(device) } == RESULT_OK {
        OK
    } else {
        INIT_FAILED
    }
}

unsafe extern "C" fn support(handle: Handle, luid: u64, result: &mut u32) -> i32 {
    let Some(support) = unsafe { native(handle) }.support else {
        return INIT_FAILED;
    };
    let mut luid = luid;
    let adapter = records::adapter(&mut luid);
    *result = unsafe { support(FEATURE_DLSS, &adapter) } as u32;
    OK
}

unsafe extern "C" fn lookup_feature(
    handle: Handle,
    group: u32,
    name: *const c_char,
    out: &mut Handle,
) -> i32 {
    let Some(feature) = unsafe { native(handle) }.feature else {
        return INIT_FAILED;
    };
    let feature_id = if group == 0 {
        FEATURE_DLSS
    } else {
        FEATURE_COMMON
    };
    if unsafe { feature(feature_id, name, out) } == RESULT_OK {
        OK
    } else {
        MISSING_ENTRY
    }
}

unsafe extern "C" fn bind_features(handle: Handle, entries: &FeatureEntries, owns_present: u32) {
    let native = unsafe { native(handle) };
    let [optimal, options, before, after] = *entries;
    unsafe {
        native.optimal = typed(optimal);
        native.options = typed(options);
        if owns_present != 0 {
            native.present = typed(before);
            native.after_present = typed(after);
        }
    }
}

unsafe extern "C" fn shutdown(handle: Handle) -> i32 {
    let Some(shutdown) = unsafe { native(handle) }.shutdown else {
        return FEATURE_FAILED;
    };
    if unsafe { shutdown() } == RESULT_OK {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe extern "C" fn close(handle: Handle) {
    drop(unsafe { Box::from_raw(handle.cast::<Native>()) });
}

unsafe extern "C" fn present(
    handle: Handle,
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    let native = unsafe { native(handle) };
    let mut skip = false;
    let result = if begin != 0 {
        let Some(before) = native.present else {
            return FEATURE_FAILED;
        };
        unsafe { before(swapchain, interval, flags, &raw mut skip) }
    } else {
        let Some(after) = native.after_present else {
            return FEATURE_FAILED;
        };
        unsafe { after(flags) }
    };
    if result >= 0 && !skip {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe extern "C" fn plan(handle: Handle, dimensions: &Dimensions, out: &mut Optimal) -> i32 {
    let Some(optimal) = unsafe { native(handle) }.optimal else {
        return FEATURE_FAILED;
    };
    let options = records::plan_options(dimensions);
    let mut settings = records::dlss_optimal();
    if unsafe { optimal(&options, &mut settings) } != RESULT_OK {
        return FEATURE_FAILED;
    }
    *out = Optimal {
        width: settings.width,
        height: settings.height,
        width_min: settings.width_min,
        height_min: settings.height_min,
        width_max: settings.width_max,
        height_max: settings.height_max,
    };
    OK
}

unsafe extern "C" fn token(handle: Handle, index: u32, out: &mut Handle) -> i32 {
    let Some(token) = unsafe { native(handle) }.token else {
        return FEATURE_FAILED;
    };
    let mut frame = null_mut();
    if unsafe { token(&mut frame, &index) } != RESULT_OK {
        return FEATURE_FAILED;
    }
    *out = frame;
    OK
}

// The determinant and inverse follow DirectXMath so the SR singularity gate keeps its numerics.
unsafe extern "C" fn inverse(matrix: &[f32; 16], out: &mut [f32; 16]) -> f32 {
    let (inverted, determinant) = super::xm::inverse(matrix);
    *out = inverted;
    determinant
}

unsafe extern "C" fn constants(
    handle: Handle,
    token: Handle,
    viewport: u32,
    input: &Constants,
) -> i32 {
    let Some(constants) = unsafe { native(handle) }.constants else {
        return FEATURE_FAILED;
    };
    // The record's motionVectorsJittered stays eFalse: the evaluation path supplies unjittered vectors.
    let record = records::constants(input);
    let viewport = records::viewport(viewport);
    if unsafe { constants(&record, token, &viewport) } == RESULT_OK {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe extern "C" fn options(handle: Handle, viewport: u32, input: &OptionPayload) -> i32 {
    let Some(options) = unsafe { native(handle) }.options else {
        return FEATURE_FAILED;
    };
    let record = records::options(input);
    let viewport = records::viewport(viewport);
    if unsafe { options(&viewport, &record) } == RESULT_OK {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe extern "C" fn dispatch(
    handle: Handle,
    token: Handle,
    viewport: u32,
    list: Handle,
    batch: &TagBatch,
) -> i32 {
    let native = unsafe { native(handle) };
    let (Some(tags), Some(evaluate)) = (native.tags, native.evaluate) else {
        return FEATURE_FAILED;
    };
    let viewport = records::viewport(viewport);
    let mut tag_records = TagRecords::new();
    tag_records.fill(batch);
    let (entries, count) = tag_records.tags();
    if unsafe { tags(token, &viewport, entries, count, list) } != RESULT_OK {
        return FEATURE_FAILED;
    }
    let inputs: [*const rsf_slvk_sdk_header; 1] = [&raw const viewport.base];
    if unsafe { evaluate(FEATURE_DLSS, token, inputs.as_ptr(), 1, list) } == RESULT_OK {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe extern "C" fn free_viewport(handle: Handle, viewport: u32) {
    if let Some(free) = unsafe { native(handle) }.free_resources {
        let viewport = records::viewport(viewport);
        // Teardown has no way to report a failed release.
        let _ = unsafe { free(FEATURE_DLSS, &viewport) };
    }
}

static PLATFORM: Platform = Platform {
    load: rsf_dlss_sr_load_sdk,
    verify,
    shared_module: rsf_dlss_sr_shared_module,
    allocate,
    bind_core,
    init,
    set_device,
    support,
    lookup_feature,
    bind_features,
    shutdown,
    close,
    present,
    plan,
    validate: rsf_dlss_sr_validate_d3d12,
    token,
    shared_token: rsf_streamline_host_token,
    inverse,
    constants,
    options,
    dispatch,
    free_viewport,
};

/// The typed SDK calls, or nothing in a build whose Streamline headers were unavailable to verify
/// the record layouts.
pub(super) fn platform() -> Option<&'static Platform> {
    (unsafe { rsf_dlss_sdk_compiled() } != 0).then_some(&PLATFORM)
}
