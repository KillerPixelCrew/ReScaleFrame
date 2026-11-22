// SPDX-License-Identifier: MIT
use crate::sr_pass::{PassInputs, ResetFacts, ResetReason, build_pass, reset_reason};
use crate::sr_stage::plan_for_render;
use crate::sys::{
    RSF_ANOMALY_SR_ADAPTER_BACKENDS, RSF_ANOMALY_SR_ADAPTER_QUALITIES,
    RSF_ANOMALY_SR_ENGINE_VERSION, RSF_ANOMALY_SR_MOTION_SENTINEL,
    RSF_ANOMALY_SR_MOTION_VALIDITY_CHANNEL, RSF_ANOMALY_SR_MOTION_VALIDITY_THRESHOLD,
    RSF_ANOMALY_SR_PROJECT_ID, RSF_ANOMALY_SR_VIEW_SPACE_TO_METERS, rsf_anomaly_sr_driver_config,
    rsf_anomaly_sr_settings,
};
use rsf_game_sys::*;
use rsf_game_sys::{
    RSF_D3D11_SR_ADMISSION_ABI_VERSION, RSF_SR_AUTO as AUTO, rsf_d3d11_sr_admission as Admission,
};
use rsf_game_sys::{rsf_camera_frame, rsf_game_log_fn, rsf_game_render_pass};
use std::cell::UnsafeCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

#[cfg(all(windows, feature = "sr-fixtures"))]
mod runtime_fixture;
#[cfg(windows)]
pub(crate) mod stage_bridge;

#[cfg(all(
    windows,
    any(feature = "sr-fixtures", all(feature = "fg-fixture", not(test)))
))]
pub(crate) mod fg_fixture;

type Raw = *mut c_void;
const OK: i32 = 0;
const STREAMLINE_FAILED: i32 = -5;
const NOT_SUPPORTED: i32 = -6;
pub(crate) const DLSS: u32 = 1;
pub(crate) const FSR3: u32 = 3;
pub(crate) const FSR4: u32 = 4;
pub(crate) const XESS: u32 = 5;
// RSF_DLSS_ENGINE_CUSTOM, checked in the native facade.
const CUSTOM_ENGINE: u32 = 0;

fn backend_name(backend: u32) -> &'static str {
    match backend {
        DLSS => "DLSS",
        2 => "FSR2",
        FSR3 => "FSR3",
        FSR4 => "FSR4",
        XESS => "XeSS",
        AUTO => "Auto",
        _ => "none",
    }
}

fn result_name(result: i32) -> &'static str {
    match result {
        OK => "ok",
        -1 => "invalid argument",
        -2 => "ABI mismatch",
        -3 => "already running",
        -4 => "not running",
        STREAMLINE_FAILED => "Streamline failed",
        NOT_SUPPORTED => "not supported",
        -7 => "resource failed",
        -8 => "motion decode failed",
        -9 => "frame refused",
        -10 => "evaluate failed",
        _ => "other",
    }
}

fn start_refusal(result: i32) -> String {
    match result {
        NOT_SUPPORTED => "SR off: the selected SR provider is unsupported on this GPU".into(),
        STREAMLINE_FAILED => {
            "SR off (bilinear): Streamline failed to start; check SRStreamlineDir in the log".into()
        }
        _ => format!(
            "SR off (bilinear): the SR pipeline did not start ({})",
            result_name(result)
        ),
    }
}

fn copy_text(out: &mut [c_char], text: &str) {
    if out.is_empty() {
        return;
    }
    let mut count = text.len().min(out.len() - 1);
    while !text.is_char_boundary(count) {
        count -= 1;
    }
    for (out, byte) in out[..count].iter_mut().zip(text.bytes()) {
        *out = byte as c_char;
    }
    out[count] = 0;
}

type LegacyStart = unsafe extern "C" fn(Raw, *const rsf_dlss_pipeline_setup) -> i32;
type Select = unsafe extern "C" fn(u32, *mut u32, *mut u32) -> i32;

#[derive(Clone, Copy)]
struct RuntimeApi {
    start: LegacyStart,
    evaluate: unsafe extern "C" fn(Raw, *const rsf_game_render_pass) -> i32,
    stop: unsafe extern "C" fn() -> i32,
    status: unsafe extern "C" fn(*mut rsf_dlss_pipeline_status) -> i32,
    select_backend: Select,
    set_quality: Select,
    set_preset: unsafe extern "C" fn(u32) -> i32,
    resize_output: unsafe extern "C" fn(u32, u32, *mut u32, *mut u32) -> i32,
    #[cfg_attr(not(windows), allow(dead_code))]
    output_texture: unsafe extern "C" fn() -> Raw,
    request_dump: unsafe extern "C" fn(*const c_char) -> i32,
}

impl RuntimeApi {
    fn admit(table: rsf_d3d11_sr_api) -> Option<Self> {
        // Required by the Runtime ABI even though this adapter has no caller.
        table.reset?;
        table.set_sharpening?;
        Some(Self {
            start: table.start?,
            evaluate: table.evaluate?,
            stop: table.stop?,
            status: table.status?,
            select_backend: table.select_backend?,
            set_quality: table.set_quality?,
            set_preset: table.set_preset?,
            resize_output: table.resize_output?,
            output_texture: table.output_texture?,
            request_dump: table.request_dump?,
        })
    }
}

fn validate_api(table: rsf_d3d11_sr_api) -> Result<RuntimeApi, &'static CStr> {
    if table.struct_size < size_of::<rsf_d3d11_sr_api>() as u32
        || table.abi_version != RSF_D3D11_SR_API_VERSION
    {
        return Err(c"the Runtime's SR API table is short or of another version");
    }
    if table.pipeline_abi_version != RSF_DLSS_PIPELINE_ABI_VERSION {
        return Err(c"the Runtime's SR pipeline structures are of another version");
    }
    RuntimeApi::admit(table).ok_or(c"the Runtime's SR API table has a missing entry")
}

fn query_api(
    query: rsf_get_d3d11_sr_api_fn,
) -> Result<(RuntimeApi, rsf_d3d11_sr_api), &'static CStr> {
    let Some(query) = query else {
        return Err(c"the Runtime does not export rsf_get_d3d11_sr_api");
    };
    let mut table = rsf_d3d11_sr_api {
        struct_size: size_of::<rsf_d3d11_sr_api>() as u32,
        abi_version: RSF_D3D11_SR_API_VERSION,
        ..Default::default()
    };
    // The resolver supplies the carrier-loaded Runtime's versioned query.
    match unsafe { query(&mut table) } {
        OK => validate_api(table).map(|api| (api, table)),
        -2 => Err(c"the Runtime's SR API version does not match"),
        _ => Err(c"the Runtime refused the SR API query"),
    }
}

type Settings = rsf_anomaly_sr_settings;

const DEFAULT_SETTINGS: Settings = Settings {
    enabled: true,
    backend: AUTO,
    quality: 1,
    preset: 0,
};

impl Settings {
    fn admitted(self, current: Self, backends: u32, qualities: u32) -> Self {
        let supports = |bits: u32, id| bits.checked_shr(id).unwrap_or(0) & 1 != 0;
        Self {
            backend: if supports(backends, self.backend) {
                self.backend
            } else {
                current.backend
            },
            quality: if supports(qualities, self.quality) {
                self.quality
            } else {
                current.quality
            },
            ..self
        }
    }
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum Refusal {
    NotRunning,
    Disabled,
    Prepare,
    Pass,
    Pipeline,
    Reinsert,
}

#[derive(Clone, Copy, Default)]
struct Counters {
    evaluated: [u64; 8],
    refused: [u64; 6],
    pass_errors: [u64; 6],
    results: [u64; 12],
    resets: [u64; 6],
    starts: u64,
    start_failures: u64,
    stops: u64,
    selects: u64,
    select_failures: u64,
    qualities: u64,
    quality_failures: u64,
    presets: u64,
    resizes: u64,
    resize_failures: u64,
    bilinear: u64,
    bilinear_failed: u64,
    motion_missing: u64,
    prepared: u64,
    select_announced: u64,
    longest_select_ms: u64,
}

#[derive(Clone, Copy)]
struct Status {
    bound: bool,
    running: bool,
    pipeline: rsf_dlss_pipeline_status,
    applied: Settings,
    requested: Settings,
    start_result: i32,
    select_result: i32,
    last_evaluated: bool,
    last_backend: u32,
    last_render_width: u32,
    last_render_height: u32,
    last_output_width: u32,
    last_output_height: u32,
    jitter: [f32; 2],
    motion_present: bool,
    exposure_present: bool,
    pending_backend: u32,
    reason: [c_char; 96],
}

impl Default for Status {
    fn default() -> Self {
        Self {
            bound: false,
            running: false,
            pipeline: fresh_pipeline_status(),
            applied: DEFAULT_SETTINGS,
            requested: DEFAULT_SETTINGS,
            start_result: OK,
            select_result: OK,
            last_evaluated: false,
            last_backend: 0,
            last_render_width: 0,
            last_render_height: 0,
            last_output_width: 0,
            last_output_height: 0,
            jitter: [0.0; 2],
            motion_present: false,
            exposure_present: false,
            pending_backend: 0,
            reason: [0; 96],
        }
    }
}

impl Status {
    fn reason(&mut self, text: &str) {
        copy_text(&mut self.reason, text);
    }

    fn reason_text(&self) -> &str {
        // copy_text always leaves a NUL inside this array.
        unsafe { CStr::from_ptr(self.reason.as_ptr()) }
            .to_str()
            .expect("SR status reason is UTF-8")
    }
}

#[derive(Default)]
struct Published {
    stage: u32,
    backends: u32,
    qualities: u32,
    status: Status,
    counters: Counters,
    last_quality: u32,
    reported: Counters,
}

struct Setup {
    template: rsf_dlss_pipeline_setup,
    streamline: CString,
    fidelityfx: CString,
    xess: CString,
}

pub(crate) const MOTION_VALIDITY_THRESHOLD: f32 = RSF_ANOMALY_SR_MOTION_VALIDITY_THRESHOLD as f32;

pub(crate) fn motion_decode_params() -> rsf_motion_decode_params {
    rsf_motion_decode_params {
        struct_size: size_of::<rsf_motion_decode_params>() as u32,
        scale_x: 1.0,
        scale_y: 1.0,
        output_scale_x: 1.0,
        output_scale_y: 1.0,
        invalid_value: RSF_ANOMALY_SR_MOTION_SENTINEL as f32,
        validity_enabled: 1,
        validity_channel: RSF_ANOMALY_SR_MOTION_VALIDITY_CHANNEL,
        validity_threshold: MOTION_VALIDITY_THRESHOLD,
        ..Default::default()
    }
}

#[cfg(feature = "motion-binder-fixture")]
pub(crate) extern "C" fn rsf_anomaly_sr_motion_decode_params(out: &mut rsf_motion_decode_params) {
    *out = motion_decode_params();
}

impl Setup {
    unsafe fn new(options: &rsf_anomaly_sr_driver_config, log: rsf_game_log_fn, user: Raw) -> Self {
        let directory = |path: *const c_char| {
            if path.is_null() {
                c"".to_owned()
            } else {
                // The stage and fixture facade lend the directory strings for configure.
                unsafe { CStr::from_ptr(path) }.to_owned()
            }
        };
        Self {
            template: rsf_dlss_pipeline_setup {
                struct_size: size_of::<rsf_dlss_pipeline_setup>() as u32,
                abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
                require_signature: u32::from(options.require_signature),
                motion: motion_decode_params(),
                log,
                log_user: user,
                engine: CUSTOM_ENGINE,
                engine_version_utf8: RSF_ANOMALY_SR_ENGINE_VERSION.as_ptr().cast(),
                project_id_utf8: RSF_ANOMALY_SR_PROJECT_ID.as_ptr().cast(),
                fsr2_directory_utf8: c"".as_ptr(),
                view_space_to_meters: RSF_ANOMALY_SR_VIEW_SPACE_TO_METERS as f32,
                standard_depth: 1,
                ..Default::default()
            },
            streamline: directory(options.streamline),
            fidelityfx: directory(options.fidelityfx),
            xess: directory(options.xess),
        }
    }

    fn for_output(&self, width: u32, height: u32, quality: u32) -> rsf_dlss_pipeline_setup {
        rsf_dlss_pipeline_setup {
            output_width: width,
            output_height: height,
            quality,
            streamline_directory_utf8: self.streamline.as_ptr(),
            fsr3_directory_utf8: self.fidelityfx.as_ptr(),
            fsr4_directory_utf8: self.fidelityfx.as_ptr(),
            xess_directory_utf8: self.xess.as_ptr(),
            ..self.template
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct StartRequest {
    device: Raw,
    output: [u32; 2],
    settings: Settings,
}

pub(crate) type Admit = unsafe extern "C" fn(*mut Admission) -> i32;

#[cfg(windows)]
pub(crate) unsafe fn admission_export(runtime: Raw) -> Option<Admit> {
    unsafe { crate::runtime_module::export(runtime, c"rsf_d3d11_sr_admit".to_bytes_with_nul()) }
}

pub(crate) fn admit(
    service: Admit,
    directories: [&CStr; 3],
    device: Raw,
    requested_backend: u32,
) -> Option<Admission> {
    let mut request = Admission {
        struct_size: size_of::<Admission>() as u32,
        abi_version: RSF_D3D11_SR_ADMISSION_ABI_VERSION,
        backend_choices: RSF_ANOMALY_SR_ADAPTER_BACKENDS,
        requested_backend,
        streamline_directory_utf8: directories[0].as_ptr(),
        fidelityfx_directory_utf8: directories[1].as_ptr(),
        xess_directory_utf8: directories[2].as_ptr(),
        device,
        ..Default::default()
    };
    // All strings and the engine's device are borrowed for this synchronous call.
    (unsafe { service(&mut request) } == OK).then_some(request)
}

type StartBackend = unsafe extern "C" fn(Raw, *const rsf_dlss_pipeline_setup, u32) -> i32;

#[derive(Clone, Copy, Default)]
enum Startup {
    #[default]
    Legacy,
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    Runtime { start: StartBackend, admit: Admit },
}

unsafe fn start_selected(
    startup: Startup,
    legacy_start: LegacyStart,
    device: Raw,
    setup: &rsf_dlss_pipeline_setup,
    requested: u32,
) -> (i32, u32) {
    match startup {
        Startup::Runtime { start, .. } => (unsafe { start(device, setup, requested) }, requested),
        // The synthetic Runtime fixture also drives the service table's DLSS-first route.
        Startup::Legacy => (unsafe { legacy_start(device, setup) }, DLSS),
    }
}

#[derive(Default)]
struct RenderState {
    setup: Option<Setup>,
    colour_transport: bool,
    api: Option<RuntimeApi>,
    startup: Startup,
    backend_request: Option<(u32, Option<u32>)>,
    device: Raw, // identity only: the running Runtime owns its device reference
    failed_start: Option<StartRequest>,
    applied: Settings,
    refused_request: Option<Settings>,
    resets: ResetFacts,
    pending_backend: Option<u32>,
    prepared_backends: u32,
    session: u64,
}

// Native Stage serializes render operations with its render guard and quiesces before
// teardown. Overlay readers and request writers only use the mutex-protected snapshot.
// The mutex is released before calling the Runtime or a log sink, which can publish UI.
struct Driver {
    render: UnsafeCell<RenderState>,
    published: Mutex<Published>,
}

impl Driver {
    fn published(&self) -> MutexGuard<'_, Published> {
        self.published
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn refresh(&self, render: &RenderState) {
        let pipeline = render
            .pipeline_status()
            .unwrap_or_else(fresh_pipeline_status);
        let mut published = self.published();
        let status = &mut published.status;
        status.pipeline = pipeline;
        status.running = !render.device.is_null();
        status.applied = render.applied;
        status.pending_backend = render.pending_backend.unwrap_or(0);
    }

    fn bind(
        &self,
        render: &mut RenderState,
        result: Result<(RuntimeApi, Startup), &'static CStr>,
        session: u64,
    ) -> bool {
        if !render.device.is_null() {
            return false; // the bound table still owns a live device
        }
        render.api = result.ok().map(|(api, _)| api);
        render.startup = result.map_or(Startup::Legacy, |(_, startup)| startup);
        render.backend_request = None;
        render.session = session;
        render.failed_start = None;
        let mut published = self.published();
        published.status.bound = result.is_ok();
        published.status.reason(match result {
            Ok(_) => "",
            Err(reason) => reason.to_str().expect("static ABI reason"),
        });
        result.is_ok()
    }

    fn resolve_backend(render: &RenderState, device: Raw, requested: u32) -> Option<u32> {
        let Startup::Runtime { admit: service, .. } = render.startup else {
            // The native fixture binds a legacy table without a Runtime module.
            return Some(if requested == AUTO { DLSS } else { requested });
        };
        let setup = render.setup.as_ref()?;
        let result = admit(
            service,
            [&setup.streamline, &setup.fidelityfx, &setup.xess],
            device,
            requested,
        )?;
        let backend = result.selected_backend;
        let offered = RSF_ANOMALY_SR_ADAPTER_BACKENDS & result.offered_backends;
        (offered.checked_shr(backend).unwrap_or(0) & 1 != 0).then_some(backend)
    }

    fn start(&self, render: &mut RenderState, api: RuntimeApi, request: StartRequest) {
        if render.failed_start == Some(request) {
            return;
        }
        let Some(config) = &render.setup else { return };
        let [ow, oh] = request.output;
        let mut wanted = request.settings;
        let selected = Self::resolve_backend(render, request.device, wanted.backend);
        render.backend_request = Some((wanted.backend, selected));
        let Some(selected) = selected else {
            render.failed_start = Some(request);
            {
                let mut published = self.published();
                published.counters.start_failures += 1;
                published.status.start_result = NOT_SUPPORTED;
                published
                    .status
                    .reason("SR off: the Runtime admitted no provider on this device");
            }
            render.say("Anomaly SR: the Runtime admitted no provider on the engine device; the seam stays bilinear.");
            return;
        };
        wanted.backend = selected;
        let setup = config.for_output(ow, oh, wanted.quality);
        let streamline = config.streamline.to_string_lossy();
        let fidelityfx = config.fidelityfx.to_string_lossy();
        let xess = config.xess.to_string_lossy();
        render.say(&format!(
            "Anomaly SR stage 5: starting the shared SR pipeline at {ow}x{oh} (backend {}, quality {}, preset {}; Streamline {streamline}).",
            backend_name(wanted.backend), wanted.quality, wanted.preset
        ));
        // The device is borrowed from the engine, and the setup strings span the call.
        let (result, backend) = unsafe {
            start_selected(
                render.startup,
                api.start,
                request.device,
                &setup,
                wanted.backend,
            )
        };
        let mut published = self.published();
        published.status.start_result = result;
        if result != OK {
            published.counters.start_failures += 1;
            published.status.reason(&start_refusal(result));
            drop(published);
            render.failed_start = Some(request);
            render.say(&format!(
                "Anomaly SR stage 5: the SR pipeline did not start ({result}, {}); the seam stays bilinear.",
                result_name(result)
            ));
            render.say(&format!(
                "Anomaly SR stage 5: SR is OFF (bilinear upscale) until the device, output or settings change. Vendor directories: Streamline '{streamline}', FidelityFX '{fidelityfx}', XeSS '{xess}' (SRStreamlineDir, SRFidelityFXDir, SRXeSSDir)."
            ));
            return;
        }
        published.counters.starts += 1;
        drop(published);
        render.device = request.device;
        render.failed_start = None;
        render.resets.first = true;
        render.prepared_backends |= 1 << backend;
        render.applied = Settings {
            backend,
            preset: 0,
            ..wanted
        };
        render.refused_request = None;
        self.apply(render, api, wanted);
    }

    fn apply(&self, render: &mut RenderState, api: RuntimeApi, wanted: Settings) {
        // The Runtime builds a newly selected backend using the quality already in force.
        if wanted.quality != render.applied.quality {
            let mut size = [0, 0];
            let result = unsafe { (api.set_quality)(wanted.quality, &mut size[0], &mut size[1]) };
            let mut published = self.published();
            published.counters.qualities += 1;
            if result == OK {
                render.applied.quality = wanted.quality;
                render.resets.settings = true;
            } else {
                published.counters.quality_failures += 1;
                drop(published);
                render.say(&format!(
                    "Anomaly SR stage 5: quality {} refused ({result}, {}); keeping {}.",
                    wanted.quality,
                    result_name(result),
                    render.applied.quality
                ));
            }
        }
        if render
            .pending_backend
            .is_some_and(|backend| backend != wanted.backend)
        {
            render.pending_backend = None;
        }
        if wanted.backend != render.applied.backend {
            let bit = 1 << wanted.backend;
            let name = backend_name(wanted.backend);
            if render.prepared_backends & bit == 0 && render.pending_backend.is_none() {
                // Let the current Present show the pending backend before blocking at the next boundary.
                render.pending_backend = Some(wanted.backend);
                self.published().counters.select_announced += 1;
                render.say(&format!(
                    "Anomaly SR stage 5: preparing {name}. The first use makes the vendor build its pipelines and the game pauses until it is done (tens of seconds for FSR4 on a cold driver cache); the select runs at the next boundary."
                ));
                render.refused_request = None;
                return;
            }
            render.pending_backend = None;
            let begun = Instant::now();
            let result = unsafe { (api.select_backend)(wanted.backend, null_mut(), null_mut()) };
            let took = begun.elapsed().as_millis() as u64;
            let mut published = self.published();
            published.counters.selects += 1;
            published.counters.longest_select_ms = published.counters.longest_select_ms.max(took);
            published.status.select_result = result;
            if result == OK {
                render.applied.backend = wanted.backend;
                render.resets.settings = true;
                render.prepared_backends |= bit;
                drop(published);
                render.say(&format!(
                    "Anomaly SR stage 5: {name} selected, the boundary took {took} ms."
                ));
            } else {
                published.counters.select_failures += 1;
                drop(published);
                render.say(&format!(
                    "Anomaly SR stage 5: backend {name} refused ({result}, {}) after {took} ms; keeping {}.",
                    result_name(result), backend_name(render.applied.backend)
                ));
            }
        }
        if wanted.preset != render.applied.preset
            && unsafe { (api.set_preset)(wanted.preset) } == OK
        {
            self.published().counters.presets += 1;
            render.applied.preset = wanted.preset;
            render.resets.settings = true;
        }
        if wanted.enabled != render.applied.enabled {
            render.applied.enabled = wanted.enabled;
            render.resets.settings = true;
        }
        render.refused_request = (wanted != render.applied).then_some(wanted);
    }

    fn stop(&self, render: &mut RenderState) {
        if render.device.is_null() {
            return;
        }
        if let Some(api) = render.api {
            unsafe { (api.stop)() };
        }
        render.device = null_mut();
        render.pending_backend = None;
        render.backend_request = None;
        {
            let mut published = self.published();
            published.counters.stops += 1;
            published.status.running = false;
            published.status.last_evaluated = false;
            published.status.pending_backend = 0;
            published.status.reason("SR pipeline stopped");
        }
        self.refresh(render);
    }

    fn boundary(&self, render: &mut RenderState, device: Raw, output: [u32; 2]) {
        let Some(api) = render.api else { return };
        let (stage, wanted) = {
            let published = self.published();
            (published.stage, published.status.requested)
        };
        if stage != 5 {
            return;
        }
        if !render.device.is_null() && device != render.device {
            render.say("Anomaly SR stage 5: the engine device changed; restarting the SR pipeline on the new device.");
            self.stop(render);
            render.resets.device = true;
        }
        let [ow, oh] = output;
        if render.device.is_null() {
            let request = StartRequest {
                device,
                output,
                settings: wanted,
            };
            if render.failed_start != Some(request) {
                render.failed_start = None;
            }
            if !device.is_null() && ow != 0 && oh != 0 && wanted.enabled {
                self.start(render, api, request);
            }
        } else {
            let mut wanted = wanted;
            let previous = render.backend_request;
            if previous.is_none_or(|(request, _)| request != wanted.backend) {
                let selected = Self::resolve_backend(render, device, wanted.backend);
                render.backend_request = Some((wanted.backend, selected));
                if selected.is_none() {
                    let mut published = self.published();
                    published.counters.select_failures += 1;
                    published.status.select_result = NOT_SUPPORTED;
                }
            }
            // A refused setting leaves the running provider available for subsequent frames.
            wanted.backend = render
                .backend_request
                .and_then(|(_, selected)| selected)
                .unwrap_or(render.applied.backend);
            if render.refused_request != Some(wanted) {
                render.refused_request = None;
            }
            if render.pending_backend.is_some()
                || (wanted != render.applied && render.refused_request != Some(wanted))
            {
                self.apply(render, api, wanted);
            }
        }
        if !render.device.is_null()
            && ow != 0
            && oh != 0
            && let Some(status) = render.pipeline_status()
            && [status.output_width, status.output_height] != output
        {
            let result = unsafe { (api.resize_output)(ow, oh, null_mut(), null_mut()) };
            let mut published = self.published();
            published.counters.resizes += 1;
            published.counters.resize_failures += u64::from(result != OK);
        }
        self.refresh(render);
    }
}

fn fresh_pipeline_status() -> rsf_dlss_pipeline_status {
    rsf_dlss_pipeline_status {
        struct_size: size_of::<rsf_dlss_pipeline_status>() as u32,
        ..Default::default()
    }
}

impl RenderState {
    fn say(&self, text: &str) {
        if let Some(config) = &self.setup {
            // SAFETY: the setup template carries the host's callback with its user pointer.
            unsafe {
                rsf_game_sys::log_guarded(config.template.log, config.template.log_user, text)
            };
        }
    }

    fn pipeline_status(&self) -> Option<rsf_dlss_pipeline_status> {
        let api = self.api?;
        let mut status = fresh_pipeline_status();
        let result = unsafe { (api.status)(&mut status) };
        (result == OK).then_some(status)
    }
}

#[cfg(any(feature = "sr-fixtures", all(feature = "fg-fixture", not(test))))]
extern "C" fn rsf_anomaly_sr_create() -> *mut Driver {
    Box::into_raw(Box::new(new_driver()))
}

fn new_driver() -> Driver {
    Driver {
        render: UnsafeCell::new(RenderState {
            resets: ResetFacts {
                first: true,
                ..ResetFacts::default()
            },
            applied: DEFAULT_SETTINGS,
            ..RenderState::default()
        }),
        published: Mutex::default(),
    }
}

unsafe extern "C" fn rsf_anomaly_sr_configure(
    driver: &Driver,
    options: &rsf_anomaly_sr_driver_config,
    log: rsf_game_log_fn,
    user: Raw,
) {
    // The facade serializes configuration with the render side of the handle.
    let render = unsafe { &mut *driver.render.get() };
    render.setup = Some(unsafe { Setup::new(options, log, user) });
    render.colour_transport = options.colour_transport;

    let mut published = driver.published();
    published.stage = options.stage;
    published.backends = RSF_ANOMALY_SR_ADAPTER_BACKENDS | (1 << AUTO);
    published.qualities = RSF_ANOMALY_SR_ADAPTER_QUALITIES;
    published.status.requested =
        options
            .initial
            .admitted(DEFAULT_SETTINGS, published.backends, published.qualities);
}

#[cfg(windows)]
extern "C" fn rsf_anomaly_sr_session_id() -> u64 {
    let mut counter = 0;
    // Same session source as the native facade. Force a nonzero ID even if QPC fails.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut counter) };
    counter as u64 | 1
}

#[cfg(windows)]
unsafe extern "C" fn rsf_anomaly_sr_bind_runtime(driver: &Driver, runtime: Raw) -> bool {
    use crate::runtime_module::export;

    // The carrier keeps the borrowed Runtime module mapped for the adapter session.
    let query: rsf_get_d3d11_sr_api_fn =
        unsafe { export(runtime, c"rsf_get_d3d11_sr_api".to_bytes_with_nul()) };
    let set_transport = unsafe {
        export::<unsafe extern "C" fn(u32)>(
            runtime,
            c"rsf_dlss_pipeline_set_colour_transport".to_bytes_with_nul(),
        )
    };
    let start_backend = unsafe {
        export::<StartBackend>(
            runtime,
            c"rsf_dlss_pipeline_start_backend".to_bytes_with_nul(),
        )
    };
    let admission = unsafe { admission_export(runtime) };
    let result = query_api(query).and_then(|(api, _)| {
        let (Some(start), Some(admit)) = (start_backend, admission) else {
            return Err(c"Runtime SR admission or selected-provider startup export missing");
        };
        Ok((api, Startup::Runtime { start, admit }))
    });
    // Configuration and binding run on the serialized render side.
    let render = unsafe { &mut *driver.render.get() };
    let bound = driver.bind(render, result, rsf_anomaly_sr_session_id());
    if bound && let Some(set_transport) = set_transport {
        unsafe { set_transport(u32::from(render.colour_transport)) };
        render.say(if render.colour_transport {
            "Anomaly SR: DLSS colour as display-range encoding, HDR off (SRColourTransport=1)."
        } else {
            "Anomaly SR: DLSS colour as exposed linear, HDR on (SRColourTransport=0)."
        });
    }
    let stage = driver.published().stage;
    match result {
        Ok(_) if bound => render.say(&format!(
            "Anomaly SR stage {stage}: Runtime SR API bound (API {}, pipeline ABI {}).",
            RSF_D3D11_SR_API_VERSION, RSF_DLSS_PIPELINE_ABI_VERSION
        )),
        Err(reason) => render.say(&format!(
            "Anomaly SR stage {stage}: {}; SR stays on the bilinear seam pass.",
            reason.to_string_lossy()
        )),
        _ => {}
    }
    bound
}

extern "C" fn rsf_anomaly_sr_request(driver: &Driver, wanted: &Settings) {
    let mut published = driver.published();
    published.status.requested = wanted.admitted(
        published.status.requested,
        published.backends,
        published.qualities,
    );
}

extern "C" fn rsf_anomaly_sr_enabled(driver: &Driver) -> bool {
    driver.published().status.requested.enabled
}

#[cfg(feature = "sr-fixtures")]
extern "C" fn rsf_anomaly_sr_running(driver: &Driver) -> bool {
    driver.published().status.running
}

#[cfg(windows)]
unsafe extern "C" fn rsf_anomaly_sr_output_format(driver: &Driver) -> u32 {
    // This borrowed texture is used under the Stage render guard before another table call.
    let render = unsafe { &*driver.render.get() };
    match render.api {
        Some(api) if !render.device.is_null() => unsafe {
            let texture = (api.output_texture)();
            crate::sr_d3d11::texture_format(texture)
        },
        _ => 0,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_plan(
    driver: &Driver,
    ow: u32,
    oh: u32,
    rw: *mut u32,
    rh: *mut u32,
) -> bool {
    let status = driver.published().status;
    let pipeline = status.pipeline;
    if !status.running
        || pipeline.running == 0
        || [pipeline.output_width, pipeline.output_height] != [ow, oh]
        || plan_for_render(ow, oh, pipeline.render_width, pipeline.render_height).is_none()
    {
        return false;
    }
    unsafe {
        rw.write(pipeline.render_width);
        rh.write(pipeline.render_height);
    }
    true
}

unsafe extern "C" fn rsf_anomaly_sr_boundary(driver: &Driver, device: Raw, ow: u32, oh: u32) {
    driver.boundary(unsafe { &mut *driver.render.get() }, device, [ow, oh]);
}

const REFUSALS: [Refusal; 6] = [
    Refusal::NotRunning,
    Refusal::Disabled,
    Refusal::Prepare,
    Refusal::Pass,
    Refusal::Pipeline,
    Refusal::Reinsert,
];

impl Published {
    fn count(&mut self, reason: Refusal) {
        self.counters.refused[reason as usize] += 1;
        match reason {
            Refusal::Pass | Refusal::Pipeline => {}
            _ => {
                self.status.last_evaluated = false;
                match reason {
                    Refusal::Reinsert => self.status.reason("SR output pass failed"),
                    Refusal::Prepare => self.status.reason("SR input preparation failed"),
                    Refusal::Disabled => self.status.reason("SR disabled by setting"),
                    Refusal::NotRunning if self.status.reason[0] == 0 => {
                        self.status.reason("SR pipeline not running")
                    }
                    _ => {}
                }
            }
        }
    }
}

unsafe extern "C" fn rsf_anomaly_sr_evaluate(
    driver: &Driver,
    context: Raw,
    inputs: &mut PassInputs,
    facts: &ResetFacts,
    captured_camera: *mut rsf_camera_frame,
    captured_backend: *mut u32,
) -> bool {
    let render = unsafe { &mut *driver.render.get() };
    let Some(api) = render.api.filter(|_| !render.device.is_null()) else {
        driver.published().count(Refusal::NotRunning);
        return false;
    };
    let facts = ResetFacts {
        first: facts.first || render.resets.first,
        device: facts.device || render.resets.device,
        settings: facts.settings || render.resets.settings,
        ..*facts
    };
    let reset = reset_reason(&facts);
    inputs.session = render.session;
    inputs.reset = reset != ResetReason::None;
    let pass = match build_pass(inputs) {
        Ok(pass) => pass,
        Err(error) => {
            let mut published = driver.published();
            published.counters.pass_errors[error as usize] += 1;
            published.count(Refusal::Pass);
            published.status.last_evaluated = false;
            published
                .status
                .reason(&format!("SR input refused: {}", error.name()));
            return false;
        }
    };
    // Runtime consumes this pass synchronously while the native caller still owns its leases.
    let result = unsafe { (api.evaluate)(context, &pass) };
    let pipeline = render.pipeline_status();
    let mut published = driver.published();
    let Published {
        status,
        counters,
        last_quality,
        ..
    } = &mut *published;
    if let Some(pipeline) = pipeline {
        status.pipeline = pipeline;
    }
    status.jitter = pass.camera.jitter_pixels;
    status.motion_present = !pass.motion.is_null() && !inputs.frame.motion_missing;
    status.exposure_present = !pass.exposure.is_null();
    if result != OK {
        let index = if (-10..=-1).contains(&result) {
            (-result) as usize
        } else {
            11
        };
        counters.results[index] += 1;
        counters.refused[Refusal::Pipeline as usize] += 1;
        status.last_evaluated = false;
        status.reason(&format!(
            "SR pipeline refused the frame ({result}, {})",
            result_name(result)
        ));
        return false;
    }
    if reset != ResetReason::None {
        counters.resets[reset as usize] += 1;
    }
    render.resets.first = false;
    render.resets.device = false;
    render.resets.settings = false;
    let backend = pipeline.map_or(render.applied.backend, |status| status.backend);
    let index = if (backend as usize) < counters.evaluated.len() {
        backend as usize
    } else {
        0
    };
    counters.evaluated[index] += 1;
    status.last_evaluated = true;
    status.last_backend = backend;
    *last_quality = render.applied.quality;
    status.last_render_width = pass.camera.render_width;
    status.last_render_height = pass.camera.render_height;
    status.last_output_width = pass.camera.output_width;
    status.last_output_height = pass.camera.output_height;
    status.reason("");
    if !captured_camera.is_null() && !captured_backend.is_null() {
        unsafe {
            captured_camera.write(pass.camera);
            captured_backend.write(backend);
        }
    }
    true
}

unsafe extern "C" fn rsf_anomaly_sr_request_dump(driver: &Driver, prefix: *const c_char) -> bool {
    let render = unsafe { &*driver.render.get() };
    let Some(api) = render.api.filter(|_| !render.device.is_null()) else {
        return false;
    };
    unsafe { prefix.read() != 0 && (api.request_dump)(prefix) == OK }
}

extern "C" fn rsf_anomaly_sr_fallback(driver: &Driver, reason: u8, drawn: bool) {
    let mut published = driver.published();
    if let Some(reason) = REFUSALS.get(reason as usize) {
        published.count(*reason);
    }
    if drawn {
        published.counters.bilinear += 1;
        if reason == Refusal::Reinsert as u8 {
            published
                .status
                .reason("SR output pass failed; bilinear recovery drawn");
        }
    } else {
        published.counters.bilinear_failed += 1;
        published.status.last_evaluated = false;
        published.status.reason(match reason {
            2 => "SR input preparation and bilinear recovery failed",
            5 => "SR output pass and bilinear recovery failed",
            7 => "SR evaluation and bilinear recovery failed",
            8 => "Bilinear recovery after camera restore failed",
            _ => "Bilinear pass failed",
        });
    }
}

extern "C" fn rsf_anomaly_sr_count_prepared(driver: &Driver, missing: bool) {
    let mut published = driver.published();
    published.counters.prepared += 1;
    published.counters.motion_missing += u64::from(missing);
}

unsafe extern "C" fn rsf_anomaly_sr_stop(driver: &Driver) {
    driver.stop(unsafe { &mut *driver.render.get() });
}

extern "C" fn rsf_anomaly_sr_overlay_report(driver: &Driver, out: &mut rsf_overlay_sr_report) {
    let published = driver.published();
    let status = &published.status;
    let active = status.last_evaluated;
    let mut report = rsf_overlay_sr_report {
        struct_size: size_of::<rsf_overlay_sr_report>() as u32,
        render_width: status.last_render_width,
        render_height: status.last_render_height,
        output_width: status.last_output_width,
        output_height: status.last_output_height,
        frames_evaluated: published.counters.evaluated.iter().sum::<u64>() as u32,
        frames_refused: published.counters.refused.iter().sum::<u64>() as u32,
        last_result: status.pipeline.last_result,
        ..Default::default()
    };
    if active {
        report.effective_backend = status.last_backend;
        report.effective_quality = published.last_quality;
        report.inputs =
            RSF_OVERLAY_SR_INPUT_COLOR | RSF_OVERLAY_SR_INPUT_DEPTH | RSF_OVERLAY_SR_INPUT_JITTER;
        if status.motion_present {
            report.inputs |= RSF_OVERLAY_SR_INPUT_MOTION;
        }
        if status.exposure_present {
            report.inputs |= RSF_OVERLAY_SR_INPUT_EXPOSURE;
        }
        report.jitter_pixels = status.jitter;
    }
    let detail;
    let reason = if status.pending_backend != 0 {
        detail = format!(
            "Preparing {}: first use loads its model; the game pauses until it is ready",
            backend_name(status.pending_backend)
        );
        &detail
    } else if !active {
        if status.reason[0] != 0 {
            status.reason_text()
        } else if status.requested.enabled {
            "SR not evaluated yet"
        } else {
            "SR disabled by setting"
        }
    } else if status.requested.backend != status.applied.backend {
        detail = format!(
            "{} refused on this GPU",
            backend_name(status.requested.backend)
        );
        &detail
    } else if status.requested.quality != status.applied.quality {
        "Requested quality refused"
    } else {
        ""
    };
    copy_text(&mut report.reason, reason);
    *out = report;
}

extern "C" fn rsf_anomaly_sr_report(
    driver: &Driver,
    exposure_auto: bool,
    first_person_zero: bool,
    log: rsf_game_log_fn,
    user: Raw,
) {
    let (stage, status, now, old) = {
        let mut published = driver.published();
        let now = published.counters;
        let old = std::mem::replace(&mut published.reported, now);
        (published.stage, published.status, now, old)
    };
    let delta = |current: u64, previous: u64| current.wrapping_sub(previous);
    let evaluated = |backend: u32| {
        delta(
            now.evaluated[backend as usize],
            old.evaluated[backend as usize],
        )
    };
    let refused =
        |reason: Refusal| delta(now.refused[reason as usize], old.refused[reason as usize]);
    let pass_errors = |index| delta(now.pass_errors[index], old.pass_errors[index]);
    let results = |index| delta(now.results[index], old.results[index]);
    let resets = |index| delta(now.resets[index], old.resets[index]);
    // SAFETY: the driver passes the host's callback with its user pointer.
    let say = |text: &str| unsafe { rsf_game_sys::log_guarded(log, user, text) };
    say(&format!(
        "Anomaly SR stage {stage} evaluated dlss={} fsr3={} fsr4={} xess={} | refused not_running={} disabled={} prepare={} \
         pass={} (sizes={} camera={} projection={} jitter={} resources={}) pipeline={} (invalid={} abi={} not_running={} \
         streamline={} not_supported={} resource={} decode={} frame={} evaluate={} other={}) reinsert={} | resets first={} \
         device={} settings={} sequence={} cut={} | prepared={} bilinear={} bilinear_failed={} motion_missing={}",
        evaluated(DLSS),
        evaluated(FSR3),
        evaluated(FSR4),
        evaluated(XESS),
        refused(Refusal::NotRunning),
        refused(Refusal::Disabled),
        refused(Refusal::Prepare),
        refused(Refusal::Pass),
        pass_errors(1),
        pass_errors(2),
        pass_errors(3),
        pass_errors(4),
        pass_errors(5),
        refused(Refusal::Pipeline),
        results(1),
        results(2),
        results(4),
        results(5),
        results(6),
        results(7),
        results(8),
        results(9),
        results(10),
        results(11) + results(3),
        refused(Refusal::Reinsert),
        resets(1),
        resets(5),
        resets(4),
        resets(2),
        resets(3),
        delta(now.prepared, old.prepared),
        delta(now.bilinear, old.bilinear),
        delta(now.bilinear_failed, old.bilinear_failed),
        delta(now.motion_missing, old.motion_missing)
    ));
    if stage == 5 {
        let pipeline = status.pipeline;
        say(&format!(
            "Anomaly SR stage 5 pipeline bound={} running={} backend={} requested={} switch={} quality={}/{} preset={} \
             render={}x{} output={}x{} frames_evaluated={} frames_refused={} last={} ({}) | starts={} start_failed={} \
             (last {}) stops={} selects={}/{} qualities={}/{} presets={} resizes={}/{} | announced={} longest_select_ms={} \
             pending={} | SR={} exposure={} first_person_zero={} | {}",
            u8::from(status.bound),
            pipeline.running,
            backend_name(pipeline.backend),
            backend_name(status.requested.backend),
            pipeline.last_switch_result,
            status.applied.quality,
            status.requested.quality,
            status.applied.preset,
            pipeline.render_width,
            pipeline.render_height,
            pipeline.output_width,
            pipeline.output_height,
            pipeline.frames_evaluated,
            pipeline.frames_refused,
            pipeline.last_result,
            result_name(pipeline.last_result),
            now.starts,
            now.start_failures,
            status.start_result,
            now.stops,
            now.selects - now.select_failures,
            now.selects,
            now.qualities - now.quality_failures,
            now.qualities,
            now.presets,
            now.resizes - now.resize_failures,
            now.resizes,
            now.select_announced,
            now.longest_select_ms,
            backend_name(status.pending_backend),
            u8::from(status.requested.enabled),
            if exposure_auto {
                "auto"
            } else {
                "constant(1.0)"
            },
            u8::from(first_person_zero),
            if status.reason[0] == 0 {
                "ok"
            } else {
                status.reason_text()
            }
        ));
    }
}

#[cfg(test)]
pub(super) extern "C" fn report_fixture(
    exposure_auto: bool,
    first_person_zero: bool,
    log: rsf_game_log_fn,
    user: Raw,
) {
    let driver = Driver {
        render: UnsafeCell::new(RenderState::default()),
        published: Mutex::new(Published {
            stage: 5,
            ..Published::default()
        }),
    };
    rsf_anomaly_sr_report(&driver, exposure_auto, first_person_zero, log, user);
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    unsafe extern "C" fn legacy(_: Raw, _: *const rsf_dlss_pipeline_setup) -> i32 {
        NOT_SUPPORTED
    }

    unsafe extern "C" fn no_provider(_: *mut Admission) -> i32 {
        NOT_SUPPORTED
    }

    fn fixture_api() -> RuntimeApi {
        unsafe extern "C" fn evaluate(_: Raw, _: *const rsf_game_render_pass) -> i32 {
            NOT_SUPPORTED
        }
        unsafe extern "C" fn stop() -> i32 {
            OK
        }
        unsafe extern "C" fn status(_: *mut rsf_dlss_pipeline_status) -> i32 {
            NOT_SUPPORTED
        }
        unsafe extern "C" fn select(_: u32, _: *mut u32, _: *mut u32) -> i32 {
            NOT_SUPPORTED
        }
        unsafe extern "C" fn preset(_: u32) -> i32 {
            NOT_SUPPORTED
        }
        unsafe extern "C" fn resize(_: u32, _: u32, _: *mut u32, _: *mut u32) -> i32 {
            NOT_SUPPORTED
        }
        unsafe extern "C" fn texture() -> Raw {
            null_mut()
        }
        unsafe extern "C" fn dump(_: *const c_char) -> i32 {
            NOT_SUPPORTED
        }
        RuntimeApi {
            start: legacy,
            evaluate,
            stop,
            status,
            select_backend: select,
            set_quality: select,
            set_preset: preset,
            resize_output: resize,
            output_texture: texture,
            request_dump: dump,
        }
    }

    #[test]
    fn provider_start_opens_requested_alternate_without_legacy_start() {
        unsafe extern "C" fn start_backend(
            _: Raw,
            _: *const rsf_dlss_pipeline_setup,
            backend: u32,
        ) -> i32 {
            if backend == XESS { OK } else { NOT_SUPPORTED }
        }
        let setup = rsf_dlss_pipeline_setup::default();
        let startup = Startup::Runtime {
            start: start_backend,
            admit: no_provider,
        };
        let receipt = unsafe { start_selected(startup, legacy, null_mut(), &setup, XESS) };
        assert_eq!(receipt, (OK, XESS));
    }

    #[test]
    fn auto_starts_the_concrete_provider_admitted_for_the_engine_device() {
        unsafe extern "C" fn admission(input: *mut Admission) -> i32 {
            let input = unsafe { &mut *input };
            assert_eq!(input.backend_choices, RSF_ANOMALY_SR_ADAPTER_BACKENDS);
            assert_eq!(input.requested_backend, AUTO);
            assert_eq!(input.device.addr(), 0x1200);
            assert_eq!(
                unsafe { CStr::from_ptr(input.xess_directory_utf8) },
                c"vendors/xess"
            );
            input.offered_backends = (1 << XESS) | (1 << AUTO);
            input.selected_backend = XESS;
            OK
        }
        unsafe extern "C" fn start(_: Raw, _: *const rsf_dlss_pipeline_setup, backend: u32) -> i32 {
            if backend == XESS { OK } else { NOT_SUPPORTED }
        }

        let driver = Driver {
            render: UnsafeCell::new(RenderState::default()),
            published: Mutex::new(Published::default()),
        };
        let options = rsf_anomaly_sr_driver_config {
            stage: 5,
            initial: DEFAULT_SETTINGS,
            xess: c"vendors/xess".as_ptr(),
            ..Default::default()
        };
        unsafe { rsf_anomaly_sr_configure(&driver, &options, None, null_mut()) };
        let render = unsafe { &mut *driver.render.get() };
        let api = fixture_api();
        assert!(driver.bind(
            render,
            Ok((
                api,
                Startup::Runtime {
                    start,
                    admit: admission
                }
            )),
            1
        ));
        let device = std::ptr::without_provenance_mut(0x1200);
        driver.start(
            render,
            api,
            StartRequest {
                device,
                output: [1920, 1080],
                settings: DEFAULT_SETTINGS,
            },
        );
        assert_eq!(render.device, device);
        assert_eq!(render.applied.backend, XESS);
        assert_eq!(render.backend_request, Some((AUTO, Some(XESS))));
        assert_eq!(driver.published().status.requested.backend, AUTO);
    }

    #[test]
    fn setting_admission_keeps_the_current_unsupported_fields() {
        let current = Settings {
            enabled: true,
            backend: XESS,
            quality: 2,
            preset: 0,
        };
        let admit = |wanted: Settings| {
            wanted.admitted(
                current,
                RSF_ANOMALY_SR_ADAPTER_BACKENDS | (1 << AUTO),
                RSF_ANOMALY_SR_ADAPTER_QUALITIES,
            )
        };
        for backend in [DLSS, FSR3, FSR4, XESS, AUTO] {
            assert_eq!(admit(Settings { backend, ..current }).backend, backend);
        }
        for backend in [0, 2, 6, 40] {
            assert_eq!(admit(Settings { backend, ..current }).backend, XESS);
        }
        for quality in 0..5 {
            assert_eq!(admit(Settings { quality, ..current }).quality, quality);
        }
        for quality in [5, 6] {
            assert_eq!(admit(Settings { quality, ..current }).quality, 2);
        }
        let actual = admit(Settings {
            enabled: false,
            backend: 2,
            quality: 5,
            preset: 3,
        });
        assert!(!actual.enabled);
        assert_eq!(
            (actual.backend, actual.quality, actual.preset),
            (XESS, 2, 3)
        );
    }
}
