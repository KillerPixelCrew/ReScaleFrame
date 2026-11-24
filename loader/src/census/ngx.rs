// SPDX-License-Identifier: MIT
use super::hooks::rsf_census_hook_original;
use super::json::{Json, emit_decimal, rsf_census_json_i64, rsf_census_json_wide};
use super::memory::{plausible, read};
use super::platform;
use super::rsf_census_call_state;
use super::session::{rsf_census_call_begin, rsf_census_call_finish};
use crate::sys::RSF_CENSUS_NGX_ENTRIES;
use std::ffi::{CStr, c_void};
use std::mem::{MaybeUninit, transmute};
use std::ptr::null_mut;
use std::sync::{
    OnceLock,
    atomic::{
        AtomicPtr,
        Ordering::{Acquire, Release},
    },
};
use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;

type Ptr = *mut c_void;
type Args = [u64; 8];
type Original = unsafe extern "system" fn(u64, u64, u64, u64, u64, u64, u64, u64) -> u64;
const ENTRIES: usize = RSF_CENSUS_NGX_ENTRIES as usize;
const GROUP: usize = 14;
const D3D12: usize = 0;
const D3D11: usize = 1;
const SUCCESS: u32 = 1;

struct Services {
    entries: [u16; ENTRIES],
}
static SERVICES: OnceLock<Services> = OnceLock::new();

fn services() -> &'static Services {
    SERVICES
        .get()
        .expect("NGX entries require an initialized platform")
}

type Created = unsafe extern "C" fn(Ptr, *const c_void, u32);
type Evaluate = unsafe extern "C" fn(Ptr, Ptr, *const c_void, u32, u64);
type Released = unsafe extern "C" fn(Ptr);
static CREATED: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static EVALUATE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static RELEASED: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

#[unsafe(no_mangle)]
extern "C" fn rsf_census_observe_ngx_d3d12(
    created: Option<Created>,
    evaluate: Option<Evaluate>,
    released: Option<Released>,
) {
    CREATED.store(
        created.map_or(null_mut(), |callback| callback as Ptr),
        Release,
    );
    EVALUATE.store(
        evaluate.map_or(null_mut(), |callback| callback as Ptr),
        Release,
    );
    RELEASED.store(
        released.map_or(null_mut(), |callback| callback as Ptr),
        Release,
    );
}

fn observe_evaluate(args: &Args, phase: u32, result: u64) {
    let callback = EVALUATE.load(Acquire);
    if !callback.is_null() {
        let callback = unsafe { transmute::<Ptr, Evaluate>(callback) };
        unsafe { callback(args[0] as Ptr, args[1] as Ptr, args[2] as _, phase, result) };
    }
}

fn observe_result(operation: usize, args: &Args, result: u64) {
    match operation {
        10 | 11 if result as u32 == SUCCESS => {
            let callback = CREATED.load(Acquire);
            let first = operation - 10;
            if !callback.is_null()
                && let Some(handle) = read::<8>(args[first + 3], 0)
                && u64::from_le_bytes(handle) != 0
            {
                let callback = unsafe { transmute::<Ptr, Created>(callback) };
                unsafe {
                    callback(
                        u64::from_le_bytes(handle) as Ptr,
                        args[first + 2] as _,
                        args[first + 1] as u32,
                    )
                };
            }
        }
        12 => observe_evaluate(args, 1, result),
        13 if result as u32 == SUCCESS => {
            let callback = RELEASED.load(Acquire);
            if !callback.is_null() {
                let callback = unsafe { transmute::<Ptr, Released>(callback) };
                unsafe { callback(args[0] as Ptr) };
            }
        }
        _ => {}
    }
}

fn parameter<T: Default>(params: u64, slot: usize, name: &CStr) -> Option<T> {
    let mut out = T::default();
    let mut code = 0;
    let returned = unsafe {
        platform().parameter.unwrap()(
            params as _,
            slot,
            name.as_ptr(),
            (&raw mut out).cast(),
            &mut code,
        )
    };
    (returned && code as u32 == SUCCESS).then_some(out)
}

fn number(params: u64, name: &CStr, prefer_float: bool) -> Option<f64> {
    let float = || parameter::<f32>(params, 14, name).map(f64::from);
    (if prefer_float { float() } else { None })
        .or_else(|| parameter::<u32>(params, 12, name).map(f64::from))
        .or_else(|| parameter::<i32>(params, 11, name).map(f64::from))
        .or_else(|| if prefer_float { None } else { float() })
        .or_else(|| parameter::<u64>(params, 15, name).map(|value| value as f64))
        .or_else(|| parameter::<f64>(params, 13, name))
}

trait NgxJson {
    fn scalar(&mut self, params: u64, key: &CStr, prefer_float: bool);
    fn string(&mut self, key: &CStr, address: u64);
    fn path(&mut self, address: u64);
}

impl NgxJson for Json<'_> {
    fn scalar(&mut self, params: u64, key: &CStr, prefer_float: bool) {
        if let Some(value) = number(params, key, prefer_float) {
            unsafe { emit_decimal(self.0, key.as_ptr(), value, platform().decimal.unwrap()) };
        }
    }
    fn string(&mut self, key: &CStr, address: u64) {
        let mut text = [0; 128];
        if unsafe {
            platform().read_string.unwrap()(address as _, text.as_mut_ptr().cast(), text.len())
        } && let Ok(value) = CStr::from_bytes_until_nul(&text)
        {
            self.text(Some(key), value);
        }
    }
    fn path(&mut self, address: u64) {
        let mut text = [0u16; 260];
        if unsafe { platform().read_wstring.unwrap()(address as _, text.as_mut_ptr(), text.len()) }
            && text[0] != 0
        {
            unsafe { rsf_census_json_wide(self.0, c"data_path".as_ptr(), text.as_ptr()) };
        }
    }
}

fn feature_name(feature: u32) -> &'static CStr {
    match feature {
        1 => c"SuperSampling",
        2 => c"InPainting",
        3 => c"ImageSuperResolution",
        4 => c"SlowMotion",
        5 => c"VideoSuperResolution",
        9 => c"ImageSignalProcessing",
        10 => c"DeepResolve",
        11 => c"FrameGeneration",
        12 => c"DeepDVC",
        13 => c"RayReconstruction",
        _ => c"other",
    }
}

fn quality_name(quality: i32) -> &'static CStr {
    match quality {
        0 => c"MaxPerf",
        1 => c"Balanced",
        2 => c"MaxQuality",
        3 => c"UltraPerformance",
        4 => c"UltraQuality",
        5 => c"DLAA",
        _ => c"other",
    }
}

fn creation_flags(json: &mut Json<'_>, flags: u32) {
    const NAMES: [&CStr; 9] = [
        c"IsHDR",
        c"MVLowRes",
        c"MVJittered",
        c"DepthInverted",
        c"Reserved_0",
        c"DoSharpening",
        c"AutoExposure",
        c"AlphaUpscaling",
        c"Reserved_8",
    ];
    json.hex(Some(c"create_flags"), flags.into());
    json.open(Some(c"create_flag_names"), b'[');
    for (index, name) in NAMES.into_iter().enumerate() {
        if flags & (1 << index) != 0 {
            json.text(None, name);
        }
    }
    json.close(b']');
}

fn create(json: &mut Json<'_>, args: &[u64]) {
    let feature = args[1] as u32;
    let params = args[2];
    json.hex(Some(c"command_list_or_context"), args[0]);
    json.number(c"feature_id", feature);
    json.text(Some(c"feature"), feature_name(feature));
    json.hex(Some(c"parameters"), params);
    if !plausible(params) {
        return;
    }
    json.open(Some(c"create"), b'{');
    for key in [c"Width", c"Height", c"OutWidth", c"OutHeight"] {
        json.scalar(params, key, false);
    }
    if let Some(quality) = number(params, c"PerfQualityValue", false) {
        unsafe { rsf_census_json_i64(json.0, c"PerfQualityValue".as_ptr(), quality as i64) };
        json.text(Some(c"quality"), quality_name(quality as i32));
    }
    if let Some(flags) = number(params, c"DLSS.Feature.Create.Flags", false) {
        creation_flags(json, flags as u32);
    }
    for key in [
        c"DLSS.Enable.Output.Subrects",
        c"CreationNodeMask",
        c"VisibilityNodeMask",
    ] {
        json.scalar(params, key, false);
    }
    json.close(b'}');
}

fn vulkan_resource(json: &mut Json<'_>, key: &CStr, resource: u64) {
    if resource == 0 {
        json.raw(key, c"null");
        return;
    }
    json.open(Some(key), b'{');
    json.hex(Some(c"ptr"), resource);
    if let Some(raw) = read::<64>(resource, 0) {
        // The census marks these offsets as assumed until independently verified.
        json.hex(
            Some(c"image_view"),
            u64::from_le_bytes(raw[..8].try_into().unwrap()),
        );
        json.hex(
            Some(c"image"),
            u64::from_le_bytes(raw[8..16].try_into().unwrap()),
        );
        for (field, offset) in [
            (c"vk_format", 36),
            (c"width", 40),
            (c"height", 44),
            (c"resource_type", 48),
        ] {
            json.number(
                field,
                u32::from_le_bytes(raw[offset..offset + 4].try_into().unwrap()),
            );
        }
        json.raw(c"read_write", if raw[52] != 0 { c"true" } else { c"false" });
        json.text(
            Some(c"layout"),
            c"assumed: view@0 image@8 format@36 width@40 height@44 type@48 rw@52",
        );
    } else {
        json.raw(c"desc_unreadable", c"true");
    }
    json.close(b'}');
}

fn resource(json: &mut Json<'_>, api: usize, params: u64, name: &CStr) {
    let slot = match api {
        D3D12 => Some(9),
        D3D11 => Some(10),
        _ => None,
    };
    let resource = slot
        .and_then(|slot| parameter::<u64>(params, slot, name))
        .or_else(|| parameter::<u64>(params, 8, name));
    if let Some(resource) = resource {
        if api == D3D12 || api == D3D11 {
            unsafe {
                super::decode::rsf_census_json_resource(
                    json.0,
                    name.as_ptr(),
                    resource as _,
                    api == D3D12,
                    platform(),
                );
            }
        } else {
            vulkan_resource(json, name, resource);
        }
    }
}

fn evaluate(json: &mut Json<'_>, api: usize, args: &Args) {
    let params = args[2];
    json.hex(Some(c"command_list_or_context"), args[0]);
    json.hex(Some(c"handle"), args[1]);
    json.hex(Some(c"parameters"), params);
    json.hex(Some(c"progress_callback"), args[3]);
    if !plausible(params) {
        return;
    }
    json.open(Some(c"resources"), b'{');
    for key in [
        c"Color",
        c"Output",
        c"Depth",
        c"MotionVectors",
        c"TransparencyMask",
        c"ExposureTexture",
        c"DLSS.Input.Bias.Current.Color.Mask",
    ] {
        resource(json, api, params, key);
    }
    json.close(b'}');
    json.open(Some(c"scalars"), b'{');
    for (key, float) in [
        (c"Jitter.Offset.X", true),
        (c"Jitter.Offset.Y", true),
        (c"MV.Scale.X", true),
        (c"MV.Scale.Y", true),
        (c"Reset", false),
        (c"Sharpness", true),
        (c"DLSS.Render.Subrect.Dimensions.Width", false),
        (c"DLSS.Render.Subrect.Dimensions.Height", false),
        (c"DLSS.Pre.Exposure", true),
        (c"DLSS.Exposure.Scale", true),
        (c"FrameTimeDeltaInMsec", true),
        (c"DLSS.Input.Color.Subrect.Base.X", false),
        (c"DLSS.Input.Color.Subrect.Base.Y", false),
        (c"DLSS.Input.Depth.Subrect.Base.X", false),
        (c"DLSS.Input.Depth.Subrect.Base.Y", false),
        (c"DLSS.Input.MV.Subrect.Base.X", false),
        (c"DLSS.Input.MV.Subrect.Base.Y", false),
        (c"DLSS.Input.Translucency.Subrect.Base.X", false),
        (c"DLSS.Input.Translucency.Subrect.Base.Y", false),
        (c"DLSS.Output.Subrect.Base.X", false),
        (c"DLSS.Output.Subrect.Base.Y", false),
        (c"DLSS.Input.Bias.Current.Color.Subrect.Base.X", false),
        (c"DLSS.Input.Bias.Current.Color.Subrect.Base.Y", false),
    ] {
        json.scalar(params, key, float);
    }
    json.close(b'}');
}

fn pre_decode(json: &mut Json<'_>, api: usize, operation: usize, args: &Args) {
    // The operation order comes from RSF_NGX_FUNCS in the native export catalog.
    match operation {
        0 | 1 => {
            json.hex(Some(c"app_id_or_arg0"), args[0]);
            json.path(args[1]);
            json.args(&args[..6]);
        }
        2 | 3 => {
            json.string(c"project_id", args[0]);
            json.number(c"engine_type", args[1] as u32);
            json.string(c"engine_version", args[2]);
            json.path(args[3]);
            json.args(&args[..7]);
        }
        5 | 13 => json.hex(Some(c"handle"), args[0]),
        9 => json.hex(Some(c"parameters"), args[0]),
        10 | 11 => create(json, &args[operation - 10..]),
        12 => evaluate(json, api, args),
        _ => {}
    }
}

fn post_decode(json: &mut Json<'_>, operation: usize, args: &Args) {
    match operation {
        6..=8 => json.output(c"parameters", args[0]),
        10 | 11 => json.output(c"handle", args[operation - 7]),
        _ => {}
    }
}

// The existing NGX probe forwards the four integer registers and four stack words
// for version-dependent signatures. Its existing return-register convention is retained.
#[inline(never)]
extern "system" fn wrapper<const ENTRY: usize>(
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
    a7: u64,
) -> u64 {
    let mut caller = null_mut();
    unsafe { RtlCaptureStackBackTrace(1, 1, &mut caller, null_mut()) };
    let entry = services().entries[ENTRY];
    let original = rsf_census_hook_original(entry);
    let mut state = MaybeUninit::<rsf_census_call_state>::uninit();
    let call = state.as_mut_ptr();
    unsafe { rsf_census_call_begin(call, entry, caller) };
    let api = ENTRY / GROUP;
    let operation = ENTRY % GROUP;
    let args = [a0, a1, a2, a3, a4, a5, a6, a7];
    if unsafe { (*call).full } {
        pre_decode(
            &mut Json(unsafe { &mut (*call).json }),
            api,
            operation,
            &args,
        );
    }
    if api == D3D12 && operation == 12 {
        observe_evaluate(&args, 0, 0);
    }
    let result = if original.is_null() {
        0
    } else {
        let forward = unsafe { transmute::<Ptr, Original>(original) };
        unsafe { forward(a0, a1, a2, a3, a4, a5, a6, a7) }
    };
    if unsafe { (*call).full } {
        post_decode(&mut Json(unsafe { &mut (*call).json }), operation, &args);
    }
    if api == D3D12 {
        observe_result(operation, &args, result);
    }
    unsafe { rsf_census_call_finish(call, result) };
    result
}

macro_rules! group {
    ($base:literal) => {
        [
            wrapper::<$base> as Ptr,
            wrapper::<{ $base + 1 }> as Ptr,
            wrapper::<{ $base + 2 }> as Ptr,
            wrapper::<{ $base + 3 }> as Ptr,
            wrapper::<{ $base + 4 }> as Ptr,
            wrapper::<{ $base + 5 }> as Ptr,
            wrapper::<{ $base + 6 }> as Ptr,
            wrapper::<{ $base + 7 }> as Ptr,
            wrapper::<{ $base + 8 }> as Ptr,
            wrapper::<{ $base + 9 }> as Ptr,
            wrapper::<{ $base + 10 }> as Ptr,
            wrapper::<{ $base + 11 }> as Ptr,
            wrapper::<{ $base + 12 }> as Ptr,
            wrapper::<{ $base + 13 }> as Ptr,
        ]
    };
}

pub(super) unsafe fn rsf_census_ngx_bind(entries: &[u16; ENTRIES], output: &mut [Ptr; ENTRIES]) {
    let _ = SERVICES.set(Services { entries: *entries });
    for (slots, functions) in
        output
            .chunks_exact_mut(GROUP)
            .zip([group!(0), group!(14), group!(28)])
    {
        slots.copy_from_slice(&functions);
    }
}
