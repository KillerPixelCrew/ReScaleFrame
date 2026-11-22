// SPDX-License-Identifier: MIT
// SR stage driver (docs/research/anomaly-sr-seam.md sections 9 to 12, docs/research/rust-anomaly-sr-stage.md).
// Rust owns the lifecycle, source identity and view admission, the H1 preparation and S-B seam ordering, the
// published jitter and main-view camera, the frame dump, burst bookkeeping and the burst readback staging transaction
// (staging.rs), the configuration (the [Anomaly] keys,
// the INI path, the vendor directories and their availability, the live panel options), GPU query retirement and the
// status text.
// sr_stage.cpp keeps guarded engine reads, trampolines, the SwapController and its D3D11 host, typed INI and file
// calls, and the TemporalDriver facade over the existing Rust temporal driver
// (docs/research/rust-anomaly-sr-configuration.md).
mod math;

pub(crate) use self::math::plan_for_render;
use self::math::{
    BurstGate, DEFAULT_MIP_OFFSET, DEFAULT_SCALE, JitterSequence, clamp_mip_offset, clamp_scale,
    jitter_to_uv, make_plan, mip_delta, next_jitter,
};
use crate::camera;
use crate::frame_dump::DumpApi;
use crate::lease::Lease;
use crate::native_geometry_abi::SCOPE;
use crate::sr_d3d11::{
    DrawViews, Extent, PREPARE, Platform as D3d11Platform, REINSERT, ROUND_TRIP, TemporalTargets,
    UPSCALE,
};
use crate::sr_stage_abi::*;
use crate::sr_swap_abi::{
    rsf_anomaly_swap_fields as TargetFields, rsf_anomaly_swap_upscale as UpscaleInputs,
};
use rsf_game_sys::{RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record};
use std::ffi::{CStr, CString, c_char, c_void};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

const PHASE_RENDER_SUBMIT: u32 = 2;
const SCREEN_UNKNOWN: u32 = 0;
const E_FAIL: i32 = 0x8000_4005_u32 as i32;
const FORMAT_RGBA16F: u32 = 10;
pub(super) const DX10_MSAA: u32 = 1 << 14;
const FIRST_PERSON_DEPTH_BITS: u32 = 0.02f32.to_bits();
const VIEW_RAW: u32 = 0;
const VIEW_HDR: u32 = 3;
const VIEW_DEPTH: u32 = 4;
const VIEW_DENSE_MOTION: u32 = 7;
const BACKEND_DLSS: u32 = 1;
const BACKEND_FSR3: u32 = 3;
const BACKEND_FSR4: u32 = 4;
const BACKEND_XESS: u32 = 5;
const BACKENDS: u32 = RSF_ANOMALY_SR_ADAPTER_BACKENDS;
const QUALITIES: u32 = RSF_ANOMALY_SR_ADAPTER_QUALITIES;
const MAX_PATH: usize = 260;
// The vendor runtime directories, in Paths::vendors order.
const STREAMLINE: usize = 0;
const FIDELITYFX: usize = 1;
const XESS: usize = 2;
// Refusal (sr_temporal.h).
const REFUSAL_PREPARE: u8 = 2;
const REFUSAL_REINSERT: u8 = 5;
// Skip and Restore (sr_swap.h).
const SKIP_OFF: usize = 0;
const SKIP_SCOPE: usize = 1;
const SKIP_MSAA: usize = 2;
const SKIP_LAYOUT: usize = 3;
const SKIP_TWINS: usize = 4;
const SKIP_DEPTH: usize = 5;
const RESTORE_SEAM: usize = 0;
const RESTORE_COMBINE_END: usize = 1;
const RESTORE_STALE_PREPARE: usize = 2;
const RESTORE_SHUTDOWN: usize = 3;
const RESTORE_ROLLBACK: usize = 4;
const RESTORE_FRAME_END: usize = 5;

type Platform = rsf_anomaly_sr_stage_platform;
type Config = rsf_anomaly_sr_stage_config;
type JitterState = rsf_anomaly_sr_jitter_state;

#[derive(Default)]
pub(crate) struct OverlayChoices {
    pub backends: u32,
    pub qualities: u32,
    pub preset: bool,
    pub defaults: rsf_anomaly_sr_settings,
}

// A missing native leaf answers with a refusal: null, zero or false.
trait Refused {
    fn refused() -> Self;
}
macro_rules! refused {
    ($($t:ty => $v:expr),* $(,)?) => { $(impl Refused for $t { fn refused() -> Self { $v } })* };
}
refused!(() => (), bool => false, u32 => 0, i32 => 0, u64 => 0, f32 => 0.0,
    *mut c_void => null_mut(), *const c_void => std::ptr::null(), *const c_char => std::ptr::null(),
    rsf_anomaly_sr_jitter_sample => rsf_anomaly_sr_jitter_sample::default());

macro_rules! leaf {
    ($p:expr, $name:ident($($arg:expr),* $(,)?)) => {
        match $p.$name {
            // The native leaves are noexcept functions with exactly these C signatures.
            Some(call) => unsafe { call($($arg),*) },
            None => Refused::refused(),
        }
    };
}

// H1 and the stock seam both belong to the current CRender::Target.
fn admitted_target(platform: &Platform, this: *mut c_void) -> *mut c_void {
    let target = leaf!(platform, render_target());
    if target == this { target } else { null_mut() }
}

// The swap controller lends these views through the synchronous seam call.
struct Seam {
    twin_surface: *mut c_void,
    engine_surface: *mut c_void,
    views: DrawViews,
}

impl Seam {
    unsafe fn upscale(input: &UpscaleInputs) -> Option<Self> {
        let colour = unsafe { input.generic0_twin.as_ref() }?;
        let position = unsafe { input.position_twin.as_ref() }?;
        let distort = unsafe { input.generic1_twin.as_ref() }?;
        let colour_target = unsafe { input.generic0.as_ref() }?;
        let position_target = unsafe { input.position.as_ref() }?;
        let distort_target = unsafe { input.generic1.as_ref() }?;
        Some(Self {
            twin_surface: colour.surface,
            engine_surface: colour_target.surface,
            views: DrawViews {
                output: Extent {
                    width: input.output_width,
                    height: input.output_height,
                },
                source: Extent {
                    width: input.render_width,
                    height: input.render_height,
                },
                targets: [colour_target.rtv, position_target.rtv, distort_target.rtv],
                depth: input.engine_dsv,
                inputs: [
                    colour.srv,
                    position.srv,
                    distort.srv,
                    input.depth_srv,
                    null_mut(),
                ],
                ..DrawViews::default()
            },
        })
    }

    fn round_trip(twin: &TargetFields, engine: &TargetFields) -> Self {
        Self {
            twin_surface: twin.surface,
            engine_surface: engine.surface,
            views: DrawViews {
                output: Extent {
                    width: engine.width,
                    height: engine.height,
                },
                source: Extent {
                    width: twin.width,
                    height: twin.height,
                },
                targets: [engine.rtv, null_mut(), null_mut()],
                inputs: [twin.srv, null_mut(), null_mut(), null_mut(), null_mut()],
                ..DrawViews::default()
            },
        }
    }
}

mod burst_file;
mod gpu_timer;
mod staging;

fn default_jitter() -> JitterState {
    JitterState {
        phase_count: 8,
        ..Default::default()
    }
}

fn default_config() -> Config {
    Config {
        scale: DEFAULT_SCALE,
        scale_defaulted: true,
        jitter: true,
        mip_bias: true,
        mip_bias_offset: DEFAULT_MIP_OFFSET,
        capture_start: 600,
        capture_count: 3,
        capture_interval: 300,
        first_person_zero: true,
        cut_distance: 4.0,
        cut_degrees: 45.0,
        settings: rsf_anomaly_sr_settings {
            enabled: true,
            backend: BACKEND_DLSS,
            quality: 1,
            preset: 0,
        },
        ..Default::default()
    }
}

fn world_window(stage: u32) -> bool {
    (2..=5).contains(&stage)
}
fn temporal(stage: u32) -> bool {
    (3..=5).contains(&stage)
}
fn sr_inputs(stage: u32) -> bool {
    stage == 4 || stage == 5
}

// ---- INI parsing ----------------------------------------------------------------------------------------------
fn is_space(c: u16) -> bool {
    matches!(c, 0x20 | 0x09..=0x0d)
}

// The prefix of `text` that the CRT's wcstod converts, and its value. Zero length: no conversion.
fn wcstod(text: &[u16]) -> (f64, usize) {
    let at = |i: usize| text.get(i).copied().unwrap_or(0);
    let lower = |i: usize| {
        let c = at(i);
        if (b'A' as u16..=b'Z' as u16).contains(&c) {
            c + 32
        } else {
            c
        }
    };
    let word = |i: usize, w: &str| w.bytes().enumerate().all(|(k, b)| lower(i + k) == b as u16);
    let digit = |c: u16| (b'0' as u16..=b'9' as u16).contains(&c);
    let hex = |c: u16| -> Option<u32> { char::from_u32(c as u32).and_then(|c| c.to_digit(16)) };
    let mut i = 0;
    while is_space(at(i)) {
        i += 1;
    }
    let start = i;
    let negative = at(i) == b'-' as u16;
    if negative || at(i) == b'+' as u16 {
        i += 1;
    }
    let sign = if negative { -1.0 } else { 1.0 };
    if word(i, "infinity") {
        return (sign * f64::INFINITY, i + 8);
    }
    if word(i, "inf") {
        return (sign * f64::INFINITY, i + 3);
    }
    if word(i, "nan") {
        let mut end = i + 3;
        if at(end) == b'(' as u16 {
            let mut k = end + 1;
            while char::from_u32(at(k) as u32)
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                k += 1;
            }
            if at(k) == b')' as u16 {
                end = k + 1;
            }
        }
        return (f64::NAN, end);
    }
    if at(i) == b'0' as u16 && lower(i + 1) == b'x' as u16 {
        let mut k = i + 2;
        let (mut mantissa, mut exponent, mut digits) = (0.0f64, 0i64, 0);
        while let Some(d) = hex(at(k)) {
            mantissa = mantissa * 16.0 + f64::from(d);
            digits += 1;
            k += 1;
        }
        if at(k) == b'.' as u16 {
            k += 1;
            while let Some(d) = hex(at(k)) {
                mantissa = mantissa * 16.0 + f64::from(d);
                exponent -= 4;
                digits += 1;
                k += 1;
            }
        }
        if digits > 0 {
            if lower(k) == b'p' as u16 {
                let mut e = k + 1;
                let negative_exponent = at(e) == b'-' as u16;
                if negative_exponent || at(e) == b'+' as u16 {
                    e += 1;
                }
                if digit(at(e)) {
                    let mut value = 0i64;
                    while digit(at(e)) {
                        value = (value * 10 + i64::from(at(e) - b'0' as u16)).min(100_000);
                        e += 1;
                    }
                    exponent += if negative_exponent { -value } else { value };
                    k = e;
                }
            }
            let exponent = exponent.clamp(-100_000, 100_000) as i32;
            return (sign * mantissa * 2f64.powi(exponent), k);
        }
        // "0x" without a hex digit converts the "0".
        return (0.0 * sign, i + 1);
    }
    let mut k = i;
    let mut digits = 0;
    while digit(at(k)) {
        k += 1;
        digits += 1;
    }
    if at(k) == b'.' as u16 {
        k += 1;
        while digit(at(k)) {
            k += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return (0.0, 0);
    }
    if lower(k) == b'e' as u16 {
        let mut e = k + 1;
        if at(e) == b'-' as u16 || at(e) == b'+' as u16 {
            e += 1;
        }
        if digit(at(e)) {
            while digit(at(e)) {
                e += 1;
            }
            k = e;
        }
    }
    let literal: String = text[start..k].iter().map(|&c| c as u8 as char).collect();
    (literal.parse().unwrap_or(0.0), k)
}

struct Ini<'a> {
    platform: &'a Platform,
    ini: *const c_void,
}

impl Ini<'_> {
    fn int(&self, key: &CStr, fallback: i32) -> i32 {
        leaf!(self.platform, ini_int(self.ini, key.as_ptr(), fallback))
    }
    fn text(&self, key: &CStr, fallback: &CStr) -> Vec<u16> {
        self.text_of::<64>(key, fallback)
    }
    // At most N - 1 UTF-16 units, as GetPrivateProfileStringW returns them for an N-unit buffer.
    fn text_of<const N: usize>(&self, key: &CStr, fallback: &CStr) -> Vec<u16> {
        let mut out = [0u16; N];
        let length = leaf!(
            self.platform,
            ini_text(
                self.ini,
                key.as_ptr(),
                fallback.as_ptr(),
                out.as_mut_ptr(),
                N as u32
            )
        );
        let length = (length as usize).min(N - 1);
        let end = out[..length].iter().position(|&c| c == 0).unwrap_or(length);
        out[..end].to_vec()
    }
    // SRStreamlineDir, SRFidelityFXDir and SRXeSSDir: an explicit value, else the package layout beside the INI
    // (ReScaleFrame\streamline, fidelityfx, xess), as a plain directory. `path` is the INI path without its NUL.
    fn vendor_directories(&self, path: &[u16]) -> [PathBuf; 3] {
        let beside = path_of(path);
        let base = beside
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join("ReScaleFrame");
        [
            (c"SRStreamlineDir", "streamline"),
            (c"SRFidelityFXDir", "fidelityfx"),
            (c"SRXeSSDir", "xess"),
        ]
        .map(|(key, fallback)| {
            let value = self.text_of::<MAX_PATH>(key, c"");
            let value = if value.is_empty() {
                units(&base.join(fallback))
            } else {
                value
            };
            path_of(&plain_directory(&value))
        })
    }
    // SRScale: clamped; `defaulted` when missing or not a number (sr_stage.h).
    fn scale(&self) -> (f32, bool) {
        let text = self.text(c"SRScale", c"");
        let (value, end) = wcstod(&text);
        if text.is_empty() || end == 0 {
            return (DEFAULT_SCALE, true);
        }
        if text[end..].iter().any(|&c| c != 0x20 && c != 0x09) {
            return (DEFAULT_SCALE, true); // trailing garbage: "0.67x"
        }
        let scale = clamp_scale(value as f32);
        (scale, value.is_nan() || value <= 0.0 || !value.is_finite())
    }
    // SRMipBias: a decimal offset clamped to [-2, 1], or "off".
    fn mip_bias(&self) -> (f32, bool) {
        let text = self.text(c"SRMipBias", c"");
        let blank = |c: &u16| *c == 0x20 || *c == 0x09;
        let begin = text.iter().position(|c| !blank(c)).unwrap_or(text.len());
        let rest = &text[begin..];
        if rest.len() >= 3
            && rest[..3]
                .iter()
                .map(|&c| c | 0x20)
                .eq("off".bytes().map(u16::from))
            && rest[3..].iter().all(blank)
        {
            return (DEFAULT_MIP_OFFSET, false);
        }
        let (value, end) = wcstod(rest);
        if rest.is_empty()
            || end == 0
            || rest[end..].iter().any(|c| !blank(c))
            || !value.is_finite()
        {
            return (DEFAULT_MIP_OFFSET, true);
        }
        (clamp_mip_offset(value as f32), true)
    }
    fn float(&self, key: &CStr, fallback: f32, low: f32, high: f32) -> f32 {
        let text = self.text(key, c"");
        let (value, end) = wcstod(&text);
        if text.is_empty() || end == 0 || !value.is_finite() {
            return fallback;
        }
        if value < f64::from(low) {
            low
        } else if value > f64::from(high) {
            high
        } else {
            value as f32
        }
    }
    fn options(&self) -> Config {
        let mut out = default_config();
        let stage = self.int(c"SRStage", 0);
        out.stage = if (1..=5).contains(&stage) {
            stage as u32
        } else {
            0
        };
        (out.scale, out.scale_defaulted) = self.scale();
        out.jitter = self.int(c"SRJitter", 1) != 0;
        (out.mip_bias_offset, out.mip_bias) = self.mip_bias();
        out.capture = self.int(c"SRStageCapture", 0) != 0;
        out.capture_start = self.int(c"SRStageCaptureStart", 600) as u32;
        out.capture_count =
            self.int(c"SRStageCaptureCount", if out.stage == 3 { 8 } else { 3 }) as u32;
        out.capture_interval = self.int(c"SRStageCaptureInterval", 300) as u32;
        // A Stage 3 burst can hold a whole 18- or 32-phase cycle.
        out.capture_count = out.capture_count.min(if out.stage == 3 { 32 } else { 16 });
        out.capture_interval = out.capture_interval.max(1);
        let exposure = self.text(c"SRExposure", c"constant");
        out.exposure_auto = exposure.len() == 4
            && exposure
                .iter()
                .map(|&c| c | 0x20)
                .eq("auto".bytes().map(u16::from));
        out.colour_transport = self.int(c"SRColourTransport", 0) != 0;
        out.first_person_zero = self.int(c"SRFirstPersonZero", 1) != 0;
        out.cut_distance = self.float(c"SRCutDistance", 4.0, 0.1, 1000.0);
        out.cut_degrees = self.float(c"SRCutAngle", 45.0, 1.0, 180.0);
        out.require_signature = self.int(c"SRRequireSignature", 0) != 0;
        // The shared frontend keys the overlay persists in the same section; a value the adapter cannot apply keeps
        // the default (sanitize, sr_settings.h).
        let backend = self.int(c"SRBackend", BACKEND_DLSS as i32) as u32;
        let quality = self.int(c"SRQuality", 1) as u32;
        out.settings = rsf_anomaly_sr_settings {
            enabled: self.int(c"SR", 1) != 0,
            backend: if backend < 32 && (BACKENDS >> backend) & 1 != 0 {
                backend
            } else {
                BACKEND_DLSS
            },
            quality: if quality < 32 && (QUALITIES >> quality) & 1 != 0 {
                quality
            } else {
                1
            },
            preset: self.int(c"DLSSPreset", 0) as u32,
        };
        out
    }
}

// ---- process state ------------------------------------------------------------------------------------------
struct GpuLease(Lease);
// D3D11 resources and views are free-threaded. The stage serializes access.
unsafe impl Send for GpuLease {}

#[derive(Clone, Copy)]
struct Raw(*mut c_void);
// Opaque engine identities and log context, used only on the threads the native caller allows.
unsafe impl Send for Raw {}

struct MotionView {
    device: Raw,
    texture: Raw,
    view: GpuLease,
}

#[derive(Default)]
struct BurstFrame {
    world: Option<GpuLease>,
    presented: Option<GpuLease>,
    engine_frame: u32,
    jitter: JitterState,
    mip_bias: f32,
    fp_jittered: u64,
}

struct Burst {
    enabled: bool,
    armed: bool,
    finished: bool,
    written: bool,
    wanted: u32,
    wait: u32,
    start: u64,
    fp_at_prepare: u64,
    directory: PathBuf,
    frames: Vec<BurstFrame>,
    result: String,
}

impl Default for Burst {
    fn default() -> Self {
        Self {
            enabled: false,
            armed: false,
            finished: false,
            written: false,
            wanted: 0,
            wait: 0,
            start: 0,
            fp_at_prepare: 0,
            directory: PathBuf::new(),
            frames: Vec::new(),
            result: "not started".into(),
        }
    }
}

#[derive(Clone, Copy)]
struct Capture {
    active: bool,
    record: rsf_frame_record,
    remaining: u32,
    next: u64,
    taken: u64,
    images: u64,
    failed: u64,
    seam_taken: bool,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            active: false,
            // A plain C record; every field is valid as zero.
            record: unsafe { std::mem::zeroed() },
            remaining: 0,
            next: 0,
            taken: 0,
            images: 0,
            failed: 0,
            seam_taken: false,
        }
    }
}

#[derive(Default)]
struct Reported {
    counters: rsf_anomaly_sr_swap_counters,
    frames: u64,
    without_world: u64,
    world_ms: [f64; 2],
    seam_ms: f64,
    world_samples: [u64; 2],
    seam_samples: u64,
}

// The configuration beyond its scalar keys: the INI that SRScale is re-read from after a device reset
// (NUL-terminated, empty when configured without one) and the vendor runtime directories.
#[derive(Clone, Default)]
struct Paths {
    ini: Vec<u16>,
    vendors: [PathBuf; 3],
}

struct State {
    config: Config,
    paths: Paths,
    log: rsf_anomaly_sr_log,
    log_user: Raw,
    build: Raw,
    session: u64,
    world_frames: u64,
    frames: u64,
    frames_without_world: u64,
    prepared_this_frame: bool,
    native_depth_frame: Option<u32>,
    current_scale: f32,
    scale_generation: u64,
    last_plan: rsf_anomaly_sr_plan,
    main_state: JitterState,
    published: JitterState,
    no_world_mark: u64,
    jitter_enabled: bool,
    sequence: JitterSequence,
    frame_camera: rsf_anomaly_sr_camera,
    previous_camera: rsf_anomaly_sr_camera,
    frame_cut: bool,
    frame_seconds: f32,
    last_main_qpc: i64,
    dump: Option<DumpApi>,
    capture: Capture,
    burst: Burst,
    burst_gate: BurstGate,
    timer: gpu_timer::Timer,
    motion_view: Option<MotionView>,
    temporal_targets: Option<TemporalTargets>,
    twin_drops: u64,
    reported: Reported,
}

impl Default for State {
    fn default() -> Self {
        Self {
            config: default_config(),
            paths: Paths::default(),
            log: None,
            log_user: Raw(null_mut()),
            build: Raw(null_mut()),
            session: 0,
            world_frames: 0,
            frames: 0,
            frames_without_world: 0,
            prepared_this_frame: false,
            native_depth_frame: None,
            current_scale: DEFAULT_SCALE,
            scale_generation: u64::MAX,
            last_plan: rsf_anomaly_sr_plan::default(),
            main_state: default_jitter(),
            published: default_jitter(),
            no_world_mark: u64::MAX,
            jitter_enabled: false,
            sequence: JitterSequence::new(),
            frame_camera: Default::default(),
            previous_camera: Default::default(),
            frame_cut: false,
            frame_seconds: 0.0,
            last_main_qpc: 0,
            dump: None,
            capture: Capture::default(),
            burst: Burst::default(),
            burst_gate: BurstGate::new(),
            timer: gpu_timer::Timer::default(),
            motion_view: None,
            temporal_targets: None,
            twin_drops: 0,
            reported: Reported::default(),
        }
    }
}

// The options the panel changes while the game runs. Read on the render thread without the lock.
struct Live {
    colour_transport: AtomicBool,
    exposure_auto: AtomicBool,
    first_person_zero: AtomicBool,
    jitter: AtomicBool,
    mip_bias: AtomicBool,
    mip_offset: AtomicU32,
}

pub(crate) struct Stage {
    platform: AtomicPtr<Platform>,
    state: Mutex<Option<Box<State>>>,
    active: AtomicBool,
    // The configured stage and SRStageCapture, read without the stage lock by the overlay's report and request paths,
    // the observer's install decision and the motion stage's capture binding.
    stage: AtomicU32,
    capture: AtomicBool,
    uv: [AtomicU32; 4],
    live: Live,
}

// The locked state a seam call lends to the controller's seam pass (Host::upscale and Host::round_trip).
struct Scope<'a> {
    stage: &'a Stage,
    state: *mut State,
}

fn emit(log: rsf_anomaly_sr_log, user: Raw, text: &str) {
    if let (Some(log), Ok(text)) = (log, CString::new(text)) {
        // The configured host log accepts any NUL-terminated message for the duration of the call.
        unsafe { log(user.0, text.as_ptr()) };
    }
}

fn dump_directory() -> Option<PathBuf> {
    let limit = |v: &std::ffi::OsString| !v.is_empty() && v.len() < 32768;
    if let Some(custom) = std::env::var_os("RSF_DUMP_DIR").filter(limit) {
        return Some(custom.into());
    }
    let local = std::env::var_os("LOCALAPPDATA").filter(limit)?;
    Some(PathBuf::from(local).join("ReScaleFrame").join("Anomaly"))
}

// A path's UTF-16 units, without a terminator.
fn units(path: &Path) -> Vec<u16> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().collect()
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy().encode_utf16().collect()
    }
}

fn wide(path: &Path) -> Vec<u16> {
    let mut out = units(path);
    out.push(0);
    out
}

// The path UTF-16 units name. Windows keeps unpaired surrogates.
fn path_of(units: &[u16]) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(units).into()
    }
    #[cfg(not(windows))]
    {
        String::from_utf16_lossy(units).into()
    }
}

// A vendor runtime directory as Streamline, FidelityFX and XeSS can use it. The INI path the carrier hands over can
// be a Win32 long-path name ("\\?\D:\...", "\\?\UNC\server\share\..."). Streamline appends "/sl.common.dll" to its
// plugin directory, and a "\\?\" name disables the slash conversion, so every plugin failed to load and Stage 5
// stayed on the bilinear seam (wave 7 run, c1 log). Strips the prefix, uses backslashes only and drops trailing
// separators.
pub(crate) fn plain_directory(path: &[u16]) -> Vec<u16> {
    let starts = |prefix: &str| {
        let prefix: Vec<u16> = prefix.encode_utf16().collect();
        path.starts_with(&prefix)
    };
    let backslash = u16::from(b'\\');
    let mut out = if starts(r"\\?\UNC\") {
        [backslash, backslash]
            .into_iter()
            .chain(path[8..].iter().copied())
            .collect()
    } else if starts(r"\\?\") || starts(r"\??\") {
        path[4..].to_vec()
    } else {
        path.to_vec()
    };
    for unit in &mut out {
        if *unit == u16::from(b'/') {
            *unit = backslash;
        }
    }
    while out.len() > 3 && out.last() == Some(&backslash) {
        out.pop();
    }
    out
}

fn selected(config: &Config) -> bool {
    config.stage == 1 || world_window(config.stage) || config.capture
}

fn yes(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
fn on(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

impl Stage {
    pub(crate) const fn new() -> Self {
        Self {
            platform: AtomicPtr::new(null_mut()),
            state: Mutex::new(None),
            active: AtomicBool::new(false),
            stage: AtomicU32::new(0),
            capture: AtomicBool::new(false),
            uv: [
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
            ],
            live: Live {
                colour_transport: AtomicBool::new(false),
                exposure_auto: AtomicBool::new(false),
                first_person_zero: AtomicBool::new(true),
                jitter: AtomicBool::new(true),
                mip_bias: AtomicBool::new(true),
                mip_offset: AtomicU32::new(0),
            },
        }
    }

    fn platform(&self) -> &Platform {
        static NONE: Platform = unsafe { std::mem::zeroed() };
        let table = self.platform.load(Ordering::Acquire);
        // The registered table is a static native table that lives for the process.
        if table.is_null() {
            &NONE
        } else {
            unsafe { &*table }
        }
    }

    pub(crate) fn register(&self, table: *const Platform) {
        self.platform.store(table.cast_mut(), Ordering::Release);
    }

    fn lock(&self) -> MutexGuard<'_, Option<Box<State>>> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn active_lock(&self) -> Option<MutexGuard<'_, Option<Box<State>>>> {
        if !self.active() {
            return None;
        }
        let guard = self.lock();
        self.active().then_some(guard)
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut guard = self.lock();
        f(guard.get_or_insert_with(Default::default))
    }

    fn say(&self, state: &State, text: &str) {
        emit(state.log, state.log_user, text);
    }

    fn live_mip_offset(&self) -> f32 {
        f32::from_bits(self.live.mip_offset.load(Ordering::Relaxed))
    }

    fn controller(&self) -> rsf_anomaly_sr_controller {
        let mut out = rsf_anomaly_sr_controller::default();
        leaf!(self.platform(), controller(&mut out));
        out
    }

    fn counters(&self) -> rsf_anomaly_sr_swap_counters {
        let mut out = rsf_anomaly_sr_swap_counters::default();
        leaf!(self.platform(), swap_counters(&mut out));
        out
    }

    fn publish(&self, state: &mut State, value: JitterState) {
        state.published = value;
        let uv = jitter_to_uv(&value);
        for (slot, value) in
            self.uv
                .iter()
                .zip([uv.current_u, uv.current_v, uv.previous_u, uv.previous_v])
        {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    // ---- options and configuration ------------------------------------------------------------------------
    pub(crate) fn read_options(&self, ini: *const c_void) -> Config {
        Ini {
            platform: self.platform(),
            ini,
        }
        .options()
    }
    pub(crate) fn read_scale(&self, ini: *const c_void) -> (f32, bool) {
        Ini {
            platform: self.platform(),
            ini,
        }
        .scale()
    }
    pub(crate) fn read_mip_bias(&self, ini: *const c_void) -> (f32, bool) {
        Ini {
            platform: self.platform(),
            ini,
        }
        .mip_bias()
    }

    // The cold entry: reads the [Anomaly] SR keys from the INI at `path` (UTF-16, no terminator) and, when `apply`
    // and a stage or the capture is selected, keeps them with the INI path and the vendor directories. Returns
    // whether one is selected.
    pub(crate) fn configure_ini(
        &self,
        path: &[u16],
        apply: bool,
        log: rsf_anomaly_sr_log,
        user: *mut c_void,
    ) -> bool {
        let ini: Vec<u16> = path.iter().copied().chain([0]).collect();
        let reader = Ini {
            platform: self.platform(),
            ini: ini.as_ptr().cast(),
        };
        let config = reader.options();
        if !(apply && selected(&config)) {
            return selected(&config);
        }
        let vendors = reader.vendor_directories(path);
        let paths = Paths { ini, vendors };
        if self.configure(&config, paths, log, user) {
            #[cfg(windows)]
            register_live_options();
        }
        true
    }

    // Before Observe starts. Refused while the hooks are installed.
    fn configure(
        &self,
        config: &Config,
        paths: Paths,
        log: rsf_anomaly_sr_log,
        user: *mut c_void,
    ) -> bool {
        let [streamline, fidelityfx, xess] = paths
            .vendors
            .clone()
            .map(|path| CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default());
        let accepted = self.with(|state| {
            if self.active.load(Ordering::Acquire) {
                return false;
            }
            state.config = *config;
            state.paths = paths;
            self.stage.store(config.stage, Ordering::Release);
            self.capture.store(config.capture, Ordering::Release);
            state.log = log;
            state.log_user = Raw(user);
            let live = &self.live;
            live.colour_transport
                .store(config.colour_transport, Ordering::Relaxed);
            live.exposure_auto
                .store(config.exposure_auto, Ordering::Relaxed);
            live.first_person_zero
                .store(config.first_person_zero, Ordering::Relaxed);
            live.jitter.store(config.jitter, Ordering::Relaxed);
            live.mip_bias.store(config.mip_bias, Ordering::Relaxed);
            live.mip_offset
                .store(config.mip_bias_offset.to_bits(), Ordering::Relaxed);
            true
        });
        if !accepted {
            return false;
        }
        // Stages 4 and 5 hand the TemporalDriver its options; the directories are borrowed for the call.
        let options = rsf_anomaly_sr_driver_config {
            stage: config.stage,
            exposure_auto: config.exposure_auto,
            colour_transport: config.colour_transport,
            first_person_zero: config.first_person_zero,
            require_signature: config.require_signature,
            initial: config.settings,
            streamline: streamline.as_ptr(),
            fidelityfx: fidelityfx.as_ptr(),
            xess: xess.as_ptr(),
        };
        let driver = if sr_inputs(config.stage) {
            &raw const options
        } else {
            std::ptr::null()
        };
        leaf!(self.platform(), configured(log, user, driver));
        true
    }

    // Whether the configured stage or capture needs the SR hooks (the observer installs them only then).
    pub(crate) fn selected(&self) -> bool {
        let stage = self.stage.load(Ordering::Acquire);
        stage == 1 || world_window(stage) || self.capture()
    }

    // SRStageCapture: this run's frame dump service belongs to the SR stage.
    pub(crate) fn capture(&self) -> bool {
        self.capture.load(Ordering::Acquire)
    }

    pub(crate) fn option(&self, id: u32) -> i32 {
        let live = &self.live;
        let flag = |value: &AtomicBool| i32::from(value.load(Ordering::Relaxed));
        match id {
            RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT => 1 - flag(&live.colour_transport),
            RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO => flag(&live.exposure_auto),
            RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO => flag(&live.first_person_zero),
            RSF_ANOMALY_SR_OPTION_JITTER => flag(&live.jitter),
            RSF_ANOMALY_SR_OPTION_MIP_BIAS => flag(&live.mip_bias),
            RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET => (self.live_mip_offset() * 10.0).round() as i32,
            _ => 0,
        }
    }

    // Applies a panel value and saves it through the adapter option path.
    pub(crate) fn set_option(&self, id: u32, value: i32) {
        let p = self.platform();
        let live = &self.live;
        let persist_int = |key: &CStr, v: i32| leaf!(p, persist_int(key.as_ptr(), v));
        let persist_text = |key: &CStr, v: &str| {
            if let Ok(text) = CString::new(v) {
                leaf!(p, persist_text(key.as_ptr(), text.as_ptr()));
            }
        };
        let offset_text = || format!("{:.1}", f64::from(self.live_mip_offset()));
        match id {
            RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT => {
                let transport = value == 0;
                live.colour_transport.store(transport, Ordering::Relaxed);
                leaf!(p, apply_colour_transport(transport));
                persist_int(c"SRColourTransport", i32::from(transport));
            }
            RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO => {
                live.exposure_auto.store(value != 0, Ordering::Relaxed);
                persist_text(c"SRExposure", if value != 0 { "auto" } else { "constant" });
            }
            RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO => {
                live.first_person_zero.store(value != 0, Ordering::Relaxed);
                persist_int(c"SRFirstPersonZero", i32::from(value != 0));
            }
            RSF_ANOMALY_SR_OPTION_JITTER => {
                live.jitter.store(value != 0, Ordering::Relaxed);
                persist_int(c"SRJitter", i32::from(value != 0));
            }
            RSF_ANOMALY_SR_OPTION_MIP_BIAS => {
                live.mip_bias.store(value != 0, Ordering::Relaxed);
                persist_text(
                    c"SRMipBias",
                    &if value != 0 {
                        offset_text()
                    } else {
                        "off".into()
                    },
                );
            }
            RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET => {
                live.mip_offset
                    .store((value as f32 / 10.0).to_bits(), Ordering::Relaxed);
                if live.mip_bias.load(Ordering::Relaxed) {
                    persist_text(c"SRMipBias", &offset_text());
                }
            }
            _ => {}
        }
    }

    pub(crate) fn set_scale(&self, value: f32) {
        let scale = clamp_scale(value);
        self.with(|state| state.current_scale = scale);
    }

    pub(crate) fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn owns_depth(&self, native_frame: u32) -> bool {
        let stage = self.stage.load(Ordering::Acquire);
        world_window(stage)
            && (stage != 5
                || !self.active()
                || self.with(|state| state.native_depth_frame != Some(native_frame)))
    }

    pub(crate) fn jitter(&self) -> JitterState {
        self.with(|state| state.published)
    }

    pub(crate) fn jitter_uv(&self) -> rsf_anomaly_sr_jitter_uv {
        let [u, v, pu, pv] =
            [0, 1, 2, 3].map(|i| f32::from_bits(self.uv[i].load(Ordering::Relaxed)));
        rsf_anomaly_sr_jitter_uv {
            current_u: u,
            current_v: v,
            previous_u: pu,
            previous_v: pv,
        }
    }

    // ---- install and quiesce ------------------------------------------------------------------------------------
    pub(crate) fn install(
        &self,
        module: *mut c_void,
        image_size: u32,
        expected_size: u32,
        build: *const c_void,
    ) -> bool {
        let p = self.platform();
        let mut guard = self.lock();
        let state = guard.get_or_insert_with(Default::default);
        let stage = state.config.stage;
        if !selected(&state.config) {
            return true;
        }
        if self.active.load(Ordering::Acquire) {
            return true;
        }
        if module.is_null() || image_size != expected_size {
            self.say(
                state,
                "Anomaly SR stage refused: image size differs from the verified build.",
            );
            return false;
        }
        // Trampolines are process-lifetime: a re-start must be the build the first install verified.
        let created = leaf!(p, hooks_created());
        if created && state.build.0.cast_const() != build {
            self.say(
                state,
                "Anomaly SR stage refused: a different build than the installed hooks.",
            );
            return false;
        }
        // A re-start after quiesce finds our own detours in place of the guarded bytes; the first install verified them.
        if !created {
            let failed = leaf!(
                p,
                verify_guards(
                    module,
                    image_size,
                    build,
                    world_window(stage),
                    temporal(stage)
                )
            );
            if !failed.is_null() {
                // The leaf returns an immutable site name from the profile tables.
                let name = unsafe { CStr::from_ptr(failed) }.to_string_lossy();
                self.say(
                    state,
                    &format!("Anomaly SR stage refused, game left stock: guard mismatch at {name}"),
                );
                return false;
            }
        }
        state.build = Raw(build.cast_mut());
        leaf!(p, set_engine(module, build, temporal(stage)));
        let (mut counter, mut frequency) = (0i64, 0i64);
        leaf!(p, now(&mut counter, &mut frequency));
        state.session = counter as u64 | 1;
        let swap =
            (stage == 1 || world_window(stage)) && (stage != 5 || leaf!(p, driver_enabled()));
        leaf!(p, set_swap_enabled(swap));
        state.native_depth_frame = None;
        state.current_scale = clamp_scale(state.config.scale);
        state.scale_generation = u64::MAX;
        // Stage 3: the main projection is jittered here, the first-person ones only through the shared rmNear slot.
        // Without that slot (geometry hooks refused) the weapon would shimmer against the world, so jitter stays off.
        state.sequence = JitterSequence::new();
        state.main_state = default_jitter();
        self.publish(state, default_jitter());
        state.no_world_mark = u64::MAX;
        state.jitter_enabled = temporal(stage) && leaf!(p, geometry_active());
        if temporal(stage) && !state.jitter_enabled {
            self.say(state, "Anomaly SR stage 3: jitter off, the rmNear slot is unavailable (geometry hooks refused); mip bias only.");
        }
        if stage == 5 && !state.jitter_enabled {
            self.say(state, "Anomaly SR stage 5: no jitter (SRJitter=0 or the rmNear slot is unavailable); every SR pass is refused and the seam stays bilinear.");
        }
        // Stage 5 borrows the carrier-loaded Runtime's SR pipeline; the pipeline starts at the first frame boundary.
        if stage == 5 {
            leaf!(p, driver_bind());
        }
        state.frame_camera = Default::default();
        state.previous_camera = Default::default();
        state.frame_cut = false;
        state.last_main_qpc = 0;
        if !leaf!(p, install_hooks()) {
            self.say(
                state,
                "Anomaly SR stage refused: guarded detour installation failed; game left stock.",
            );
            return false;
        }
        if !leaf!(p, install_position_slot()) {
            leaf!(p, disable_hooks());
            self.say(state, "Anomaly SR stage refused: the position binder's vtable slot does not hold the profiled setup; game left stock.");
            return false;
        }
        if stage == 1 || world_window(stage) {
            leaf!(p, set_rm_near(true));
        }
        self.active.store(true, Ordering::Release);
        drop(guard);
        let mut guard = self.lock();
        let state = guard.get_or_insert_with(Default::default);
        if state.config.capture {
            self.bind_dump(state);
        }
        if state.config.capture && stage == 3 {
            self.bind_burst(state);
        }
        let text = self.install_text(state);
        self.say(state, &text);
        true
    }

    fn install_text(&self, state: &State) -> String {
        let c = &state.config;
        let geometry = leaf!(self.platform(), geometry_active());
        let defaulted = if c.scale_defaulted { " (default)" } else { "" };
        let scale = state.current_scale;
        let detail = if sr_inputs(c.stage) {
            format!(
                "{}; SRScale={scale:.3}{defaulted}{}; jitter={}, mip bias {} (log2(render/output) {:+.3}); SR inputs: linear colour, R24G8 depth twin, motion target (validity in Z, first-person zero {}); exposure {}; cut at {:.2} m or {:.1} deg",
                if c.stage == 5 {
                    "temporal SR through the shared RSF pipeline at the seam (bilinear on any refusal)"
                } else {
                    "SR inputs prepared at the seam, bilinear output"
                },
                if c.stage == 5 {
                    " (fallback; the selected backend's quality plans the render size)"
                } else {
                    ""
                },
                on(state.jitter_enabled),
                on(c.mip_bias),
                f64::from(c.mip_bias_offset),
                on(c.first_person_zero),
                if c.exposure_auto {
                    "auto (backend)"
                } else {
                    "constant 1.0"
                },
                f64::from(c.cut_distance),
                f64::from(c.cut_degrees),
            )
        } else if c.stage == 3 {
            format!(
                "SRScale={scale:.3}{defaulted}, bilinear upscale at the seam; jitter={} (Halton 2,3, ceil(8*(output/render)^2) phases), mip bias {} (log2(render/output) {:+.3}), burst={} consecutive frames from world frame {}",
                on(state.jitter_enabled),
                on(c.mip_bias),
                f64::from(c.mip_bias_offset),
                if c.capture { c.capture_count } else { 0 },
                c.capture_start,
            )
        } else if c.stage == 2 {
            format!("SRScale={scale:.3}{defaulted}, bilinear upscale at the seam")
        } else {
            "scale 1.0".into()
        };
        format!(
            "Anomaly SR stage hooks installed: SRStage={} SRStageCapture={} (start={} count={} interval={}) rmNear slot={}; {detail}{}.",
            c.stage,
            i32::from(c.capture),
            c.capture_start,
            c.capture_count,
            c.capture_interval,
            if c.stage == 0 {
                "unused"
            } else if geometry {
                "shared"
            } else {
                "unavailable (geometry hooks refused)"
            },
            if c.stage == 5 {
                ""
            } else {
                ", no SR provider is called"
            },
        )
    }

    fn bind_dump(&self, state: &mut State) {
        let Some(table) = DumpApi::bind() else {
            self.say(
                state,
                "Anomaly SR stage capture refused: Runtime frame dump exports missing.",
            );
            return;
        };
        let Some(directory) = dump_directory() else {
            self.say(
                state,
                "Anomaly SR stage capture refused: LOCALAPPDATA unavailable.",
            );
            return;
        };
        let Ok(directory) = CString::new(directory.to_string_lossy().as_bytes()) else {
            return;
        };
        let sha = leaf!(self.platform(), build_sha256());
        // The Runtime copies the strings; the log context outlives the session.
        unsafe {
            (table.configure)(
                directory.as_ptr(),
                c"Anomaly".as_ptr(),
                sha,
                state.log,
                state.log_user.0,
            )
        };
        state.dump = Some(table);
        // Stage 3 replaces the scheduled single-frame captures with the consecutive burst; F10 still takes one.
        state.capture.remaining = if state.config.stage == 3 {
            0
        } else {
            state.config.capture_count
        };
        state.capture.next = u64::from(state.config.capture_start);
    }

    fn bind_burst(&self, state: &mut State) {
        let Some(directory) = dump_directory() else {
            self.say(
                state,
                "Anomaly SR stage 3 burst refused: LOCALAPPDATA unavailable.",
            );
            return;
        };
        state.burst_gate = BurstGate::new();
        state.burst = Burst {
            directory: directory.join(format!("sr-jitter-{:016x}", state.session)),
            wanted: state.config.capture_count,
            start: u64::from(state.config.capture_start),
            frames: Vec::with_capacity(state.config.capture_count as usize),
            enabled: state.config.capture_count > 0,
            ..Burst::default()
        };
    }

    pub(crate) fn quiesce(&self) -> bool {
        let p = self.platform();
        if !leaf!(p, hooks_created()) {
            return true;
        }
        let disabled = leaf!(p, remove_hooks());
        let mut guard = self.lock();
        self.active.store(false, Ordering::Release);
        let state = guard.get_or_insert_with(Default::default);
        if !disabled {
            // A partially disabled hook can still enter. Let it run stock and
            // keep its resources until the native removal can be retried.
            leaf!(p, close_stale());
            self.publish(state, default_jitter());
            state.main_state = default_jitter();
            return false;
        }
        if !leaf!(p, shutdown()) {
            self.publish(state, default_jitter());
            state.main_state = default_jitter();
            return false;
        }
        state.motion_view = None;
        state.temporal_targets = None;
        state.native_depth_frame = None;
        state.timer = gpu_timer::Timer::default();
        self.publish(state, default_jitter());
        state.main_state = default_jitter();
        for frame in &mut state.burst.frames {
            frame.world = None;
            frame.presented = None;
        }
        state.burst.armed = false;
        if let Some(dump) = state.dump {
            self.capture_abandon(state);
            unsafe { (dump.cancel)(state.session) };
        }
        // Teardown: the pipeline releases its device reference and every vendor context before the engine's device goes.
        leaf!(p, driver_stop());
        leaf!(p, host_reset());
        true
    }

    // ---- H1: CRenderTarget::phase_scene_prepare ---------------------------------------------------------------
    fn gather(&self, state: &mut State, this: *mut c_void) -> rsf_anomaly_sr_frame_facts {
        let p = self.platform();
        let stage = state.config.stage;
        let target = admitted_target(p, this);
        let mut reads = rsf_anomaly_sr_frame_reads::default();
        leaf!(
            p,
            frame_facts(world_window(stage), temporal(stage), &mut reads)
        );
        let mut facts = reads.facts;
        facts.render_target = target;
        // An unreadable view or options word cannot admit a reduced world frame.
        facts.scope = !reads.view_read || reads.render_view == u32::from(SCOPE);
        facts.msaa = !reads.options_read || reads.options_word3 & DX10_MSAA != 0;
        facts.stage = 1;
        if !world_window(stage) {
            state.last_plan = make_plan(facts.device_width, facts.device_height, 1.0).unwrap_or(
                rsf_anomaly_sr_plan {
                    scale: 1.0,
                    ..Default::default()
                },
            );
            return facts;
        }
        // A device reset (vid_restart, resolution change) re-reads SRScale, so a new value applies without a restart.
        if facts.generation != state.scale_generation {
            if state.scale_generation != u64::MAX && !state.paths.ini.is_empty() {
                let (scale, _) = Ini {
                    platform: p,
                    ini: state.paths.ini.as_ptr().cast(),
                }
                .scale();
                if scale != state.current_scale {
                    let text = format!(
                        "Anomaly SR stage {stage}: SRScale {:.3} -> {:.3} after a device reset.",
                        f64::from(state.current_scale),
                        f64::from(scale)
                    );
                    self.say(state, &text);
                    state.current_scale = scale;
                }
            }
            state.scale_generation = facts.generation;
        }
        if let Some(plan) = make_plan(facts.device_width, facts.device_height, state.current_scale)
        {
            facts.render_width = plan.render_width;
            facts.render_height = plan.render_height;
            state.last_plan = plan;
        }
        // Stage 5: the render size the selected backend plans for this output at the selected quality. SRScale stays
        // the fallback while no backend plans (not started, refused, or planning for another output size).
        let (mut width, mut height) = (0, 0);
        if stage == 5
            && leaf!(
                p,
                driver_planned(
                    facts.device_width,
                    facts.device_height,
                    &mut width,
                    &mut height
                )
            )
            && let Some(plan) =
                plan_for_render(facts.device_width, facts.device_height, width, height)
        {
            facts.render_width = plan.render_width;
            facts.render_height = plan.render_height;
            state.last_plan = plan;
        }
        facts.stage = 2;
        if temporal(stage) {
            facts.mip_bias = self.live.mip_bias.load(Ordering::Relaxed);
            facts.mip_bias_delta = mip_delta(
                facts.render_width,
                facts.device_width,
                self.live_mip_offset(),
            );
        }
        facts
    }

    // Stages 4 and 5, at a main-view H1 before the jitter is applied.
    fn read_main_camera(&self, state: &mut State) {
        let p = self.platform();
        state.previous_camera = state.frame_camera;
        let mut camera = rsf_anomaly_sr_camera::default();
        camera.valid = leaf!(p, read_camera(&mut camera))
            && camera
                .view
                .iter()
                .chain(camera.projection.iter())
                .all(|v| v.is_finite());
        state.frame_camera = camera;
        let c = &state.config;
        state.frame_cut = camera.valid
            && state.previous_camera.valid
            && camera::camera_cut(
                camera::pose(&state.previous_camera.view),
                camera::pose(&camera.view),
                c.cut_distance,
                c.cut_degrees,
            );
        let (mut now, mut frequency) = (0i64, 0i64);
        leaf!(p, now(&mut now, &mut frequency));
        let mut seconds = if state.last_main_qpc != 0 && frequency != 0 {
            ((now - state.last_main_qpc) as f64 / frequency as f64) as f32
        } else {
            0.0
        };
        if seconds.is_nan() || seconds <= 0.0 || seconds > 1.0 {
            seconds = 0.0;
        }
        state.frame_seconds = seconds;
        state.last_main_qpc = now;
    }

    fn choose_jitter(
        &self,
        state: &mut State,
        facts: &mut rsf_anomaly_sr_frame_facts,
    ) -> (rsf_anomaly_sr_jitter_sample, bool) {
        if !temporal(state.config.stage)
            || facts.scope
            || !state.jitter_enabled
            || !self.live.jitter.load(Ordering::Relaxed)
        {
            return (
                rsf_anomaly_sr_jitter_sample {
                    phase_count: 8,
                    ..Default::default()
                },
                false,
            );
        }
        let continuous = state.no_world_mark == state.frames_without_world; // no menu/loading frame since the last main H1
        state.no_world_mark = state.frames_without_world;
        let sample = state.sequence.next(
            [
                facts.render_width,
                facts.render_height,
                facts.device_width,
                facts.device_height,
            ],
            facts.generation,
            continuous,
        );
        facts.jitter = true;
        facts.jitter_x = sample.x;
        facts.jitter_y = sample.y;
        (sample, state.sequence.reset)
    }

    fn publish_jitter(
        &self,
        state: &mut State,
        facts: &rsf_anomaly_sr_frame_facts,
        sample: &rsf_anomaly_sr_jitter_sample,
        reset: bool,
    ) {
        if !temporal(state.config.stage) {
            return;
        }
        let c = self.controller();
        let applied = c.window_open && c.jitter_applied;
        let (width, height) = if applied {
            (c.render_width, c.render_height)
        } else {
            (facts.render_width, facts.render_height)
        };
        let next = next_jitter(
            &state.main_state,
            facts.scope,
            applied,
            sample,
            width,
            height,
            reset,
        );
        if !facts.scope {
            state.main_state = next;
        }
        self.publish(state, next);
    }

    fn loading(&self) -> bool {
        let p = self.platform();
        let (mut registered, mut precache) = (0, 0);
        // Both precache and the post-load key wait draw the loading screen over the world.
        !leaf!(p, loading_registered(&mut registered))
            || !leaf!(p, precache_frame(&mut precache))
            || registered != 0
            || precache != 0
    }

    fn capture_abandon(&self, state: &mut State) {
        if !state.capture.active {
            return;
        }
        if let Some(dump) = state.dump {
            // A Runtime export resolved at bind time; the record outlives the call.
            unsafe { (dump.complete)(&state.capture.record, E_FAIL) };
        }
        state.capture.active = false;
        state.capture.failed += 1;
    }

    // At H1, on a main-view frame: arm the next scheduled capture, or join an F10 request.
    fn capture_arm(&self, state: &mut State, facts: &rsf_anomaly_sr_frame_facts) {
        let Some(dump) = state.dump else { return };
        self.capture_abandon(state);
        state.capture.seam_taken = false;
        unsafe { (dump.poll)() };
        let scheduled = state.capture.remaining != 0
            && state.world_frames >= state.capture.next
            && !self.loading();
        if scheduled {
            unsafe { (dump.request)() };
        }
        if unsafe { (dump.requested)() } == 0 {
            return;
        }
        let c = self.controller();
        let stage2 = c.window_open && c.stage == 2; // this frame renders the world at render size
        let engine_frame = leaf!(self.platform(), engine_frame());
        let record = &mut state.capture.record;
        // A plain C record; every field is valid as zero.
        *record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<rsf_frame_record>() as u32;
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = u64::from(engine_frame) + 1; // never RSF_FRAME_ID_NONE
        record.session_id = state.session;
        record.view_id = 0;
        record.resource_generation = c.twin_generation as u32;
        record.phase = PHASE_RENDER_SUBMIT;
        record.screen = SCREEN_UNKNOWN;
        record.output_width = facts.device_width;
        record.output_height = facts.device_height;
        record.render_width = if stage2 {
            facts.render_width
        } else {
            facts.device_width
        };
        record.render_height = if stage2 {
            facts.render_height
        } else {
            facts.device_height
        };
        if unsafe { (dump.begin_record)(&state.capture.record, 0) } == 0 {
            return;
        }
        state.capture.active = true;
        state.capture.taken += 1;
        if scheduled {
            state.capture.remaining -= 1;
            state.capture.next = state.world_frames + u64::from(state.config.capture_interval);
        }
    }

    // Stage 3 burst: arm a consecutive capture frame at H1 (after publish_jitter).
    fn burst_arm(&self, state: &mut State, facts: &rsf_anomaly_sr_frame_facts) {
        let burst = &state.burst;
        if !burst.enabled || burst.finished || facts.scope {
            return;
        }
        let loading = self.loading();
        let verdict = state.burst_gate.frame(
            loading,
            state.frames_without_world,
            !burst.frames.is_empty(),
        );
        if verdict.discard {
            state.burst.frames.clear();
            state.burst.armed = false;
        }
        let c = self.controller();
        if !verdict.capture || !c.window_open || state.world_frames < state.burst.start {
            return;
        }
        if state.burst.frames.len() >= state.burst.wanted as usize {
            return;
        }
        let frame = BurstFrame {
            engine_frame: leaf!(self.platform(), engine_frame()),
            jitter: state.published,
            mip_bias: if c.mip_bias_applied {
                c.window_mip_bias
            } else {
                c.saved_mip_bias
            },
            ..Default::default()
        };
        state.burst.frames.push(frame);
        state.burst.fp_at_prepare = self.counters().fp_jittered;
        state.burst.armed = true;
    }

    pub(crate) fn prepare(&self, this: *mut c_void) {
        let p = self.platform();
        let Some(mut guard) = self.active_lock() else {
            return;
        };
        let state = guard.get_or_insert_with(Default::default);
        // A window still open here (never expected) is closed first, so Device.dwWidth reads the output size again.
        leaf!(p, close_stale());
        state
            .timer
            .begin(p, leaf!(p, device()), leaf!(p, context()));
        let mut facts = self.gather(state, this);
        if sr_inputs(state.config.stage) && !facts.scope {
            self.read_main_camera(state);
        }
        let (sample, reset) = self.choose_jitter(state, &mut facts);
        state.prepared_this_frame = true;
        let swapped = leaf!(p, prepare(&facts, state.log, state.log_user.0));
        if sr_inputs(state.config.stage) {
            let twin_drops = self.counters().twin_drops;
            if state.twin_drops != twin_drops {
                state.motion_view = None;
                state.temporal_targets = None;
                state.twin_drops = twin_drops;
            } else if state
                .motion_view
                .as_ref()
                .is_some_and(|cached| cached.device.0 != facts.device)
            {
                state.motion_view = None;
            }
        }
        if state.config.stage == 5 && (swapped || !facts.scope) {
            // Expose the stock depth only for the main frame that actually ran with SR off.
            state.native_depth_frame = if !swapped && !facts.scope && !leaf!(p, driver_enabled()) {
                let frame = leaf!(p, engine_frame());
                (frame != 0).then_some(frame)
            } else {
                None
            };
        }
        self.publish_jitter(state, &facts, &sample, reset);
        if !facts.scope {
            state.world_frames += 1;
            if state.config.capture {
                self.capture_arm(state, &facts);
            }
            self.burst_arm(state, &facts);
        }
    }

    // H1 epilogue. Stage 2: the viewport now describes the render-size targets.
    pub(crate) fn prepared(&self) {
        let Some(_guard) = self.active_lock() else {
            return;
        };
        leaf!(self.platform(), prepared());
    }

    // ---- the S-B seam and the other exits -------------------------------------------------------------------
    fn take(
        &self,
        state: &mut State,
        name: &CStr,
        description: &CStr,
        texture: *mut c_void,
        view: u32,
        scale: f32,
    ) {
        let Some(dump) = state.dump else {
            return;
        };
        let context = leaf!(self.platform(), context());
        if !state.capture.active || context.is_null() || texture.is_null() {
            return;
        }
        // A Runtime export resolved at bind time; every pointer is valid for the call.
        if unsafe {
            (dump.texture_record)(
                context,
                &state.capture.record,
                name.as_ptr(),
                description.as_ptr(),
                texture,
                view,
                scale,
            )
        } != 0
        {
            state.capture.images += 1;
        } else {
            state.capture.failed += 1;
        }
    }

    // Lends the locked state to the controller's seam pass for the duration of the call.
    fn controller_seam(&self, state: &mut State) -> bool {
        let mut scope = Scope { stage: self, state };
        leaf!(self.platform(), seam((&mut scope as *mut Scope).cast()))
    }

    pub(crate) fn seam(&self, this: *mut c_void) {
        let p = self.platform();
        let Some(mut guard) = self.active_lock() else {
            return;
        };
        let state = guard.get_or_insert_with(Default::default);
        let device = leaf!(p, device());
        let context = leaf!(p, context());
        if self.controller().window_open {
            state.timer.seam(p, device, context, true);
            self.controller_seam(state);
            state
                .timer
                .finish(p, leaf!(p, device()), leaf!(p, context()));
            return;
        }
        state.timer.seam(p, device, context, false);
        state.timer.finish(p, device, context);
        self.controller_seam(state); // counted as a pass-through
        // Stock capture: the engine generic0 as the post chain receives it, once per frame at the first seam call.
        if state.capture.active && !state.capture.seam_taken {
            let target = admitted_target(p, this);
            let surface = if target.is_null() {
                null_mut()
            } else {
                leaf!(p, seam_surface(target))
            };
            if !surface.is_null() {
                let stock = self.controller().refused || state.config.stage == 0;
                self.take(
                    state,
                    c"anomaly_seam_generic0",
                    if stock {
                        c"Stock (no swap this frame): engine UNORM8 generic0 at the S-B seam; input to every post pass"
                    } else {
                        c"SRStage frame without a window (skipped): stock engine UNORM8 generic0 at the S-B seam"
                    },
                    surface,
                    VIEW_RAW,
                    1.0,
                );
            }
            state.capture.seam_taken = true;
        }
    }

    // Stage 2 and later, after the restore at the seam (Host::upscale).
    fn upscale(&self, state: &mut State, seam: &Seam) -> bool {
        let p = self.platform();
        let stage = state.config.stage;
        if state.capture.active {
            self.take(
                state,
                c"anomaly_seam_fp16_input",
                c"SRStage=2/3: render-size FP16 generic0 twin at the S-B seam, engine low tonemap space, before the upscale",
                seam.twin_surface,
                VIEW_RAW,
                1.0,
            );
        }
        if state.burst.armed && state.burst.frames.last().is_some_and(|f| f.world.is_none()) {
            let context = leaf!(p, context());
            let copy = staging::copy(p, context, seam.twin_surface);
            if let Some(frame) = state.burst.frames.last_mut() {
                frame.world = copy;
            }
        }
        let ok = if sr_inputs(stage) {
            self.temporal_seam(state, seam)
        } else {
            leaf!(p, draw_pass(UPSCALE, &seam.views))
        };
        if state.capture.active {
            self.take(
                state,
                c"anomaly_seam_generic0",
                if stage == 5 {
                    c"SRStage=5: engine UNORM8 generic0 after the SR reinsertion (or the bilinear fallback) at output size; input to every post pass"
                } else {
                    c"SRStage=2/3/4: engine UNORM8 generic0 after the bilinear upscale to output size; input to every post pass"
                },
                seam.engine_surface,
                VIEW_RAW,
                1.0,
            );
            state.capture.seam_taken = true;
        }
        ok
    }

    // Stage 1, after the restore at the seam (Host::round_trip).
    fn round_trip(&self, state: &mut State, seam: &Seam) -> bool {
        if state.capture.active {
            self.take(
                state,
                c"anomaly_seam_fp16_input",
                c"SRStage=1: FP16 generic0 twin at the S-B seam, engine low tonemap space, before the round trip",
                seam.twin_surface,
                VIEW_RAW,
                1.0,
            );
        }
        let ok = leaf!(self.platform(), draw_pass(ROUND_TRIP, &seam.views));
        if state.capture.active {
            self.take(
                state,
                c"anomaly_seam_generic0",
                c"SRStage=1: engine UNORM8 generic0 after the f^-1/f round trip; input to every post pass",
                seam.engine_surface,
                VIEW_RAW,
                1.0,
            );
            state.capture.seam_taken = true;
        }
        ok
    }

    fn own(&self, handle: *mut c_void) -> Option<GpuLease> {
        let release = self.platform().release?;
        Lease::new(handle, release).map(GpuLease)
    }

    fn motion_view(
        &self,
        state: &mut State,
        device: *mut c_void,
        texture: *mut c_void,
    ) -> *mut c_void {
        if let Some(cached) = &state.motion_view
            && cached.device.0 == device
            && cached.texture.0 == texture
        {
            return cached.view.0.as_ptr();
        }
        state.motion_view = None;
        let p = self.platform();
        let Some(release) = p.release else {
            return null_mut();
        };
        let handle = leaf!(p, create_motion_view(device, texture));
        if let Some(view) = Lease::new(handle, release) {
            state.motion_view = Some(MotionView {
                device: Raw(device),
                texture: Raw(texture),
                view: GpuLease(view),
            });
        }
        handle
    }

    // Stages 4 and 5 at the S-B seam, after the engine fields are restored (section 12). Every refusal draws the Stage 2
    // bilinear pass instead, so the engine always receives a complete world image.
    fn temporal_seam(&self, state: &mut State, seam: &Seam) -> bool {
        let p = self.platform();
        let bilinear = || {
            leaf!(p, driver_count_bilinear());
            leaf!(p, draw_pass(UPSCALE, &seam.views))
        };
        let refuse = |reason: u8| {
            leaf!(p, driver_count(reason));
            bilinear()
        };
        let device = leaf!(p, device());
        if device.is_null() || leaf!(p, context()).is_null() || !leaf!(p, temporal_ready()) {
            return refuse(REFUSAL_PREPARE);
        }
        let stage = state.config.stage;
        let (rw, rh, ow, oh) = (
            seam.views.source.width,
            seam.views.source.height,
            seam.views.output.width,
            seam.views.output.height,
        );
        let evaluate = stage == 5 && leaf!(p, driver_enabled());
        let pipeline_format = if evaluate {
            leaf!(p, driver_output_format())
        } else {
            0
        };
        let format = if pipeline_format != 0 {
            pipeline_format
        } else {
            FORMAT_RGBA16F
        };
        let render = Extent {
            width: rw,
            height: rh,
        };
        let output = Extent {
            width: ow,
            height: oh,
        };
        if state
            .temporal_targets
            .as_ref()
            .is_none_or(|targets| !targets.matches(device, render, output, format))
        {
            // The old targets are only valid for their old shape. Free them before allocating replacements.
            state.temporal_targets = None;
            let platform = leaf!(p, temporal_platform()).cast::<D3d11Platform>();
            // This leaf returns the process-lifetime table from sr_d3d11.cpp.
            state.temporal_targets = unsafe { platform.as_ref() }.and_then(|platform| {
                TemporalTargets::new(platform, device, render, output, format).ok()
            });
            let text = format!(
                "Anomaly SR stage {stage}: SR input targets {} at render {rw}x{rh}, output {ow}x{oh} (format {format}).",
                if state.temporal_targets.is_some() {
                    "created"
                } else {
                    "refused"
                }
            );
            self.say(state, &text);
        }
        let Some(targets) = &state.temporal_targets else {
            return refuse(REFUSAL_PREPARE);
        };
        let textures = targets.view();
        let depth = leaf!(p, depth_texture());
        // The motion target the main view's G-buffer window wrote this frame, at render size.
        let mut target = rsf_anomaly_sr_motion_target::default();
        leaf!(p, motion_target(&mut target));
        let engine_frame = leaf!(p, engine_frame());
        let current = !target.texture.is_null()
            && target.written
            && target.main
            && target.frame == engine_frame
            && target.width == rw
            && target.height == rh;
        let motion_view = if current {
            self.motion_view(state, device, target.texture)
        } else {
            null_mut()
        };
        let motion_present = !motion_view.is_null();
        let motion_input = if motion_present {
            motion_view
        } else {
            textures.empty_motion_srv
        };
        let first_person_zero = self.live.first_person_zero.load(Ordering::Relaxed);
        let prepare = DrawViews {
            output: seam.views.source,
            targets: [textures.colour_rtv, textures.motion_rtv, null_mut()],
            inputs: [
                seam.views.inputs[0],
                seam.views.inputs[3],
                motion_input,
                null_mut(),
                null_mut(),
            ],
            first_person_depth: FIRST_PERSON_DEPTH_BITS,
            first_person_zero: u32::from(first_person_zero),
            ..DrawViews::default()
        };
        if !leaf!(p, draw_pass(PREPARE, &prepare)) {
            return refuse(REFUSAL_PREPARE);
        }
        leaf!(p, driver_count_prepared(!motion_present));
        self.take(
            state,
            c"anomaly_sr_color",
            c"SRStage=4/5 SR colour: render-size f^-1 of the FP16 generic0 twin, exposure-scaled linear",
            textures.colour,
            VIEW_HDR,
            1.0,
        );
        self.take(
            state,
            c"anomaly_sr_depth",
            c"SRStage=4/5 SR depth: render-size R24G8 depth twin, standard device depth (first person in [0, 0.02])",
            depth,
            VIEW_DEPTH,
            1.0,
        );
        self.take(
            state,
            c"anomaly_sr_motion",
            c"SRStage=4/5 SR motion: current-minus-previous UV (Y down), validity in Z, first-person rule applied (UV x20)",
            textures.motion,
            VIEW_DENSE_MOTION,
            20.0,
        );
        if motion_present {
            self.take(
                state,
                c"anomaly_motion_target",
                c"Motion RTV2 as the G-buffer wrote it (raw RGBA16F)",
                target.texture,
                VIEW_RAW,
                1.0,
            );
        }
        if !evaluate {
            return bilinear();
        }
        let pass = rsf_anomaly_sr_pass {
            native_frame: u64::from(engine_frame),
            generation: self.controller().twin_generation as u32,
            render_width: rw,
            render_height: rh,
            output_width: ow,
            output_height: oh,
            current: state.frame_camera,
            previous: state.previous_camera,
            jitter: state.published,
            frame_seconds: state.frame_seconds,
            motion_missing: !motion_present,
            exposure_auto: self.live.exposure_auto.load(Ordering::Relaxed),
            reset_sequence: state.published.reset,
            cut: state.frame_cut,
        };
        // A captured frame also asks the pipeline for its own guides beside the core dump record.
        if state.capture.active
            && let Some(directory) = dump_directory()
        {
            let directory = directory.join(format!("sr-pipeline-{:016x}", state.session));
            let _ = std::fs::create_dir(&directory);
            let prefix = format!(
                "{}\\frame_{}",
                directory.to_string_lossy(),
                state.capture.record.frame_id
            );
            if let Ok(text) = CString::new(prefix.as_bytes())
                && leaf!(p, driver_request_dump(text.as_ptr()))
            {
                self.say(
                    state,
                    &format!("Anomaly SR stage 5 capture: pipeline guides requested at {prefix}"),
                );
            }
        }
        let record = &mut state.capture.record;
        let result = state.dump.and_then(|dump| dump.result).filter(|_| {
            state.capture.active
                && engine_frame != 0
                && record.frame_id == pass.native_frame + 1
                && record.resource_generation == pass.generation
                && [
                    record.render_width,
                    record.render_height,
                    record.output_width,
                    record.output_height,
                ] == [rw, rh, ow, oh]
        });
        let mut backend = 0;
        let (camera_out, backend_out) = if result.is_some() {
            (
                std::ptr::addr_of_mut!(record.camera),
                std::ptr::addr_of_mut!(backend),
            )
        } else {
            (null_mut(), null_mut())
        };
        let mut inputs = rsf_anomaly_sr_pass_inputs {
            frame: pass,
            color: textures.colour,
            color_output: textures.output,
            depth,
            motion: textures.motion,
            exposure: if pass.exposure_auto {
                null_mut()
            } else {
                textures.exposure
            },
            ..Default::default()
        };
        if !leaf!(p, driver_evaluate(&mut inputs, camera_out, backend_out)) {
            return bilinear();
        }
        if let Some(result) = result {
            unsafe { result(record, c"sr".as_ptr(), backend, 0) };
        }
        self.take(
            state,
            c"anomaly_sr_output",
            c"SRStage=5 reconstruction at output size, linear (before the stock tonemap curve)",
            textures.output,
            VIEW_HDR,
            1.0,
        );
        let mut reinsert = seam.views;
        reinsert.inputs[4] = textures.output_srv;
        if !leaf!(p, draw_pass(REINSERT, &reinsert)) {
            return refuse(REFUSAL_REINSERT);
        }
        true
    }

    pub(crate) fn combine_end(&self) {
        let p = self.platform();
        let Some(mut guard) = self.active_lock() else {
            return;
        };
        let state = guard.get_or_insert_with(Default::default);
        let was_refused = self.controller().refused;
        leaf!(p, combine_end());
        let c = self.controller();
        if !was_refused && c.refused {
            let reason = if c.refusal.is_null() {
                "".into()
            } else {
                // The controller's refusal reasons are static strings.
                unsafe { CStr::from_ptr(c.refusal) }.to_string_lossy()
            };
            self.say(
                state,
                &format!("Anomaly SR stage refused further swaps: {reason}"),
            );
        }
    }

    pub(crate) fn sun_begin(&self) -> bool {
        let Some(_guard) = self.active_lock() else {
            return false;
        };
        leaf!(self.platform(), sun_begin())
    }
    pub(crate) fn sun_end(&self, suspended: bool) {
        let Some(_guard) = self.active_lock() else {
            return;
        };
        leaf!(self.platform(), sun_end(suspended));
    }
    pub(crate) fn position_setup(&self, constant: *mut c_void) {
        let Some(_guard) = self.active_lock() else {
            return;
        };
        leaf!(self.platform(), position_setup(constant));
    }
    pub(crate) fn rm_near(&self) {
        let Some(_guard) = self.active_lock() else {
            return;
        };
        leaf!(self.platform(), rm_near());
    }

    // ---- IRenderDeviceRender::End, before Present -------------------------------------------------------------
    pub(crate) fn frame_end(&self) {
        let p = self.platform();
        let Some(mut guard) = self.active_lock() else {
            return;
        };
        let state = guard.get_or_insert_with(Default::default);
        leaf!(p, frame_end());
        state.native_depth_frame = None;
        if state.config.stage == 5 {
            // The drained boundary: this frame's SR work is submitted and Present has not run. Start the pipeline,
            // follow a device or output change and apply the shared settings; the next H1 plans its render size.
            let (mut width, mut height) = (0, 0);
            leaf!(p, device_size(&mut width, &mut height));
            leaf!(p, driver_boundary(width, height));
            // SR=0: the world window stays closed and the frame renders stock at output size.
            if !self.controller().refused {
                let enabled = leaf!(p, driver_enabled());
                leaf!(p, set_swap_enabled(enabled));
            }
        }
        state.frames += 1;
        if !state.prepared_this_frame {
            state.frames_without_world += 1; // menu, loading, first frame after reset
        }
        state.prepared_this_frame = false;
        if state.burst.armed {
            // The burst frame armed at this frame's H1: the presented image, then the next frame (or the drain).
            state.burst.armed = false;
            if self.loading() {
                // The loading screen appeared during this frame and may be the presented image: start over.
                state.burst.frames.clear();
                state.burst_gate.interrupted();
            } else {
                let fp = self
                    .counters()
                    .fp_jittered
                    .wrapping_sub(state.burst.fp_at_prepare);
                let back = self.own(leaf!(p, back_buffer()));
                let context = leaf!(p, context());
                let presented = back
                    .as_ref()
                    .and_then(|back| staging::copy(p, context, back.0.as_ptr()));
                drop(back);
                if let Some(frame) = state.burst.frames.last_mut() {
                    frame.fp_jittered = fp;
                    frame.presented = presented;
                }
                if state.burst.frames.len() >= state.burst.wanted as usize {
                    state.burst.finished = true;
                    state.burst.wait = 3;
                }
            }
        } else if state.burst.finished && !state.burst.written {
            let wait = state.burst.wait;
            state.burst.wait = wait.wrapping_sub(1);
            if wait == 0 {
                self.burst_write(state);
            }
        }
        if !state.capture.active {
            return;
        }
        // The presented frame: the back buffer immediately before End's Present, with UI.
        if let Some(back) = self.own(leaf!(p, back_buffer())) {
            self.take(
                state,
                c"final_color",
                c"Back buffer before Present (world, post, HUD and UI)",
                back.0.as_ptr(),
                VIEW_RAW,
                1.0,
            );
        }
        if let Some(dump) = state.dump {
            unsafe { (dump.complete)(&state.capture.record, 0) };
        }
        state.capture.active = false;
    }

    // After the last frame and a short GPU drain: map every copy once, write world_NN.bin / final_NN.bin and burst.json.
    fn burst_write(&self, state: &mut State) {
        let p = self.platform();
        state.burst.written = true;
        let context = leaf!(p, context());
        if context.is_null() {
            state.burst.result = "no device context".into();
            state.burst.frames.clear();
            return;
        }
        if let Err(error) = std::fs::create_dir(&state.burst.directory) {
            state.burst.result = format!(
                "could not create {} ({error})",
                state.burst.directory.to_string_lossy()
            );
            state.burst.frames.clear();
            self.say(
                state,
                &format!("Anomaly SR stage 3 jitter burst: {}", state.burst.result),
            );
            return;
        }
        let mut json = String::from(
            "{\n  \"schema\": \"rsf-anomaly-sr-jitter-burst-1\",\n  \"convention\": \"jitter in render pixels, +X right, +Y down; the rasterized world moves by (+x, +y). world_NN.bin: render-size FP16 RGBA generic0 twin at the S-B seam (engine low tonemap space), rows packed. final_NN.bin: back buffer before Present, rows packed.\",\n",
        );
        json += &format!(
            "  \"session\": \"{:016x}\", \"scale\": {:.4}, \"mip_bias_offset\": {:.3}, \"mip_bias\": {},\n  \"frames\": [\n",
            state.session,
            f64::from(state.current_scale),
            f64::from(self.live_mip_offset()),
            yes(self.live.mip_bias.load(Ordering::Relaxed))
        );
        let count = state.burst.frames.len();
        let mut written = 0u32;
        for (i, frame) in state.burst.frames.iter_mut().enumerate() {
            let write = |name: String, texture: &Option<GpuLease>| {
                burst_file::staging(p, context, texture, &state.burst.directory.join(name))
            };
            let (world_ok, ww, wh, wf) = write(format!("world_{i:02}.bin"), &frame.world);
            let (final_ok, fw, fh, ff) = write(format!("final_{i:02}.bin"), &frame.presented);
            written += u32::from(world_ok);
            let j = &frame.jitter;
            json += &format!(
                "    {{\"index\": {i}, \"engine_frame\": {}, \"jitter_frame\": {}, \"sequence_index\": {}, \"phase_count\": {}, \"applied\": {}, \"reset\": {}, \"jitter_px\": [{:.6}, {:.6}], \"previous_jitter_px\": [{:.6}, {:.6}], \"render\": [{}, {}], \"mip_bias\": {:.4}, \"fp_jittered\": {}, \"loading_screen\": false, \"world\": {}, \"world_size\": [{ww}, {wh}], \"world_dxgi\": {wf}, \"final\": {}, \"final_size\": [{fw}, {fh}], \"final_dxgi\": {ff}}}{}\n",
                frame.engine_frame,
                j.frame,
                j.sequence_index,
                j.phase_count,
                yes(j.applied),
                yes(j.reset),
                f64::from(j.current_x),
                f64::from(j.current_y),
                f64::from(j.previous_x),
                f64::from(j.previous_y),
                j.render_width,
                j.render_height,
                f64::from(frame.mip_bias),
                frame.fp_jittered,
                yes(world_ok),
                yes(final_ok),
                if i + 1 < count { "," } else { "" }
            );
            frame.world = None;
            frame.presented = None;
        }
        json += "  ]\n}\n";
        let summary = state.burst.directory.join("burst.json");
        state.burst.result =
            match burst_file::write_new(&summary, |file| file.write_all(json.as_bytes())) {
                Ok(()) => format!(
                    "{written} of {count} world frames written to {}",
                    state.burst.directory.to_string_lossy()
                ),
                Err(error) => format!("could not write {} ({error})", summary.to_string_lossy()),
            };
        let text = format!("Anomaly SR stage 3 jitter burst: {}", state.burst.result);
        self.say(state, &text);
    }

    // ---- status -----------------------------------------------------------------------------------------------
    pub(crate) fn report(&self, sink: &mut dyn FnMut(&str)) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        let p = self.platform();
        let mut lines = Vec::with_capacity(5);
        let mut guard = self.lock();
        let state = guard.get_or_insert_with(Default::default);
        let c = self.controller();
        let now = self.counters();
        let stage = state.config.stage;
        let jitter = state.main_state;
        let sequence_resets = state.sequence.resets;
        let mip_applied = c.mip_bias_applied || now.mip_applied != 0;
        let loading = self.loading();
        let (held, restarts) = (state.burst_gate.held, state.burst_gate.restarts);
        let burst = &state.burst;
        let burst_state = if !burst.enabled {
            "off".into()
        } else if burst.written {
            burst.result.clone()
        } else {
            format!(
                "{}/{} held={held} restarts={restarts}{}",
                burst.frames.len(),
                burst.wanted,
                if loading { " loading-screen" } else { "" }
            )
        };
        let (mut twin_bytes, mut twin_refs) = (0, 0);
        leaf!(p, host_stats(&mut twin_bytes, &mut twin_refs));
        let timer = state
            .timer
            .sample(p, leaf!(p, device()), leaf!(p, context()));
        let r = &state.reported;
        let average = |sum: f64, before: f64, count: u64, before_count: u64| {
            if count > before_count {
                (sum - before) / (count - before_count) as f64
            } else {
                0.0
            }
        };
        let stock_ms = average(
            timer.world_ms[0],
            r.world_ms[0],
            timer.samples[0],
            r.world_samples[0],
        );
        let swapped_ms = average(
            timer.world_ms[1],
            r.world_ms[1],
            timer.samples[1],
            r.world_samples[1],
        );
        let seam_ms = average(timer.seam_ms, r.seam_ms, timer.seam_samples, r.seam_samples);
        let was = &r.counters;
        let skip = |i: usize| now.skipped[i].wrapping_sub(was.skipped[i]);
        let rest = |i: usize| now.restores[i].wrapping_sub(was.restores[i]);
        macro_rules! d {
            ($field:ident) => {
                now.$field.wrapping_sub(was.$field)
            };
        }
        lines.push(format!(
            "Anomaly SR stage window frames={} no_world={} prepare={} eligible={} swapped={} skipped off={} scope={} msaa={} layout={} twins={} | seams={} passthrough={} round_trips={} rt_failed={} | restores seam={} combine_end={} stale_prepare={} frame_end={} shutdown={} rollback={} | rm_near_in_window={} foreign={} write_faults={} | depth={} alloc_failed={} viewports={} ssao_viewport={} | gpu_world_ms swapped={swapped_ms:.3} (n={}) stock={stock_ms:.3} (n={}) gpu_seam_ms={seam_ms:.3}",
            state.frames.wrapping_sub(r.frames),
            state.frames_without_world.wrapping_sub(r.without_world),
            d!(prepares),
            d!(eligible),
            d!(swapped),
            skip(SKIP_OFF),
            skip(SKIP_SCOPE),
            skip(SKIP_MSAA),
            skip(SKIP_LAYOUT),
            skip(SKIP_TWINS),
            d!(seams),
            d!(seams_without_window),
            d!(round_trips),
            d!(round_trip_failures),
            rest(RESTORE_SEAM),
            rest(RESTORE_COMBINE_END),
            rest(RESTORE_STALE_PREPARE),
            rest(RESTORE_FRAME_END),
            rest(RESTORE_SHUTDOWN),
            rest(RESTORE_ROLLBACK),
            d!(rm_near_in_window),
            d!(foreign_changes),
            d!(write_faults),
            skip(SKIP_DEPTH),
            d!(allocation_failures),
            d!(viewports),
            d!(ssao_viewport_swaps),
            timer.samples[1].wrapping_sub(r.world_samples[1]),
            timer.samples[0].wrapping_sub(r.world_samples[0]),
        ));
        let restores: u64 = now.restores.iter().sum();
        let plan = state.last_plan;
        let window = world_window(stage);
        let refusal = if c.refusal.is_null() {
            "no".into()
        } else {
            // The controller's refusal reasons are static strings.
            unsafe { CStr::from_ptr(c.refusal) }.to_string_lossy()
        };
        lines.push(format!(
            "Anomaly SR stage totals stage={stage} swapped={} restores={restores} round_trips={} twin_generation={} twin_drops={} twin_refs_at_drop={twin_refs} captures={} images={} capture_failed={} render={}x{} output={}x{} scale={:.3} twin_mib={:.1} refused={refusal}",
            now.swapped,
            now.round_trips,
            now.twin_generations,
            now.twin_drops,
            state.capture.taken,
            state.capture.images,
            state.capture.failed,
            if window { plan.render_width } else { plan.output_width },
            if window { plan.render_height } else { plan.output_height },
            plan.output_width,
            plan.output_height,
            if window { f64::from(state.current_scale) } else { 1.0 },
            twin_bytes as f64 / (1024.0 * 1024.0),
        ));
        if temporal(stage) {
            lines.push(format!(
                "Anomaly SR stage 3 window jittered={} jitter_invalid={} fp_jittered={} fp_already={} fp_foreign={} projection_restores={} | mip applied={} restored={} unchanged={} | sequence frame={} index={}/{} resets={sequence_resets} last=({:+.4},{:+.4}) previous=({:+.4},{:+.4}) applied={} | mip window={:.3} user={:.3} {} | burst {burst_state}",
                d!(jittered),
                d!(jitter_invalid),
                d!(fp_jittered),
                d!(fp_already),
                d!(fp_foreign),
                d!(projection_restores),
                d!(mip_applied),
                d!(mip_restored),
                d!(mip_unchanged),
                jitter.frame,
                jitter.sequence_index,
                jitter.phase_count,
                f64::from(jitter.current_x),
                f64::from(jitter.current_y),
                f64::from(jitter.previous_x),
                f64::from(jitter.previous_y),
                i32::from(jitter.applied),
                f64::from(c.window_mip_bias),
                f64::from(c.saved_mip_bias),
                if mip_applied { "" } else { "(never applied)" },
            ));
        }
        lines.push(format!(
            "Anomaly SR sun cascades calls={} unjittered={} restored={} refused={}",
            d!(sun_calls),
            d!(sun_unjittered),
            d!(sun_restored),
            d!(sun_refused)
        ));
        lines.push(format!(
            "Anomaly SR position reconstruction corrected={} stock={} refused={}",
            d!(position_corrected),
            d!(position_stock),
            d!(position_refused)
        ));
        let frames = (state.frames, state.frames_without_world);
        state.reported = Reported {
            counters: now,
            frames: frames.0,
            without_world: frames.1,
            world_ms: timer.world_ms,
            seam_ms: timer.seam_ms,
            world_samples: timer.samples,
            seam_samples: timer.seam_samples,
        };
        // The lines are emitted, and the driver reports, with the stage lock released.
        drop(guard);
        for line in &lines {
            sink(line);
        }
        if sr_inputs(stage) {
            let mut target: &mut dyn FnMut(&str) = sink;
            leaf!(
                p,
                driver_report(
                    Some(forward_line),
                    (&mut target as *mut &mut dyn FnMut(&str)).cast()
                )
            );
        }
    }

    // ---- the shared overlay -------------------------------------------------------------------------------------
    pub(crate) fn choices(&self) -> OverlayChoices {
        let p = self.platform();
        let (config, vendors) = self.with(|state| (state.config, state.paths.vendors.clone()));
        let mut out = OverlayChoices::default();
        if config.stage != 5 {
            return out;
        }
        let has = |directory: usize, name: &str| {
            let path = wide(&vendors[directory].join(name));
            leaf!(p, file_exists(path.as_ptr()))
        };
        // Offered when the runtime the backend loads is where the pipeline will look. Whether this GPU runs it is
        // only known once the pipeline starts; the report says so when it does not.
        if has(STREAMLINE, "sl.interposer.dll") && has(STREAMLINE, "nvngx_dlss.dll") {
            out.backends |= 1 << BACKEND_DLSS;
        }
        if has(FIDELITYFX, "amd_fidelityfx_upscaler_dx12.dll")
            && has(FIDELITYFX, "amd_fidelityfx_loader_dx12.dll")
        {
            out.backends |= (1 << BACKEND_FSR3) | (1 << BACKEND_FSR4);
        }
        if has(XESS, "libxess.dll") {
            out.backends |= 1 << BACKEND_XESS;
        }
        if out.backends == 0 {
            return out;
        }
        let offered = |backend: u32| out.backends.checked_shr(backend).unwrap_or(0) & 1 != 0;
        out.qualities = QUALITIES;
        out.preset = offered(BACKEND_DLSS);
        out.defaults = config.settings;
        if !offered(out.defaults.backend)
            && let Some(backend) = [BACKEND_DLSS, BACKEND_XESS, BACKEND_FSR3, BACKEND_FSR4]
                .into_iter()
                .find(|&b| offered(b))
        {
            out.defaults.backend = backend;
        }
        out
    }

    pub(crate) fn overlay_report(&self, report: *mut c_void) -> bool {
        if report.is_null()
            || self.stage.load(Ordering::Acquire) != 5
            || !self.active.load(Ordering::Acquire)
        {
            return false;
        }
        leaf!(self.platform(), driver_overlay_report(report));
        true
    }

    pub(crate) fn request(&self, settings: &rsf_anomaly_sr_settings) {
        if self.stage.load(Ordering::Acquire) == 5 {
            leaf!(self.platform(), driver_request(settings));
        }
    }
}

unsafe extern "C" fn forward_line(user: *mut c_void, text: *const c_char) {
    if user.is_null() || text.is_null() {
        return;
    }
    // `user` is the report's sink for the duration of the driver report.
    let sink = unsafe { &mut *user.cast::<&mut dyn FnMut(&str)>() };
    sink(&unsafe { CStr::from_ptr(text) }.to_string_lossy());
}

pub(crate) static STAGE: Stage = Stage::new();

// ---- the private C ABI the native leaves call --------------------------------------------------------------------
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_register(table: *const Platform) {
    STAGE.register(table);
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_read_options(ini: *const c_void, out: *mut Config) {
    if !out.is_null() {
        let value = if ini.is_null() {
            default_config()
        } else {
            STAGE.read_options(ini)
        };
        unsafe { out.write(value) };
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_read_scale(
    ini: *const c_void,
    defaulted: *mut bool,
) -> f32 {
    let (value, was_defaulted) = if ini.is_null() {
        (DEFAULT_SCALE, true)
    } else {
        STAGE.read_scale(ini)
    };
    if !defaulted.is_null() {
        unsafe { defaulted.write(was_defaulted) };
    }
    value
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_read_mip_bias(
    ini: *const c_void,
    enabled: *mut bool,
) -> f32 {
    let (value, on) = if ini.is_null() {
        (DEFAULT_MIP_OFFSET, true)
    } else {
        STAGE.read_mip_bias(ini)
    };
    if !enabled.is_null() {
        unsafe { enabled.write(on) };
    }
    value
}
// `ini` is null (nothing selected) or a NUL-terminated UTF-16 path that stays readable for the call: the carrier's
// configuration path at cold start, or a fixture's INI.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_configure_ini(
    ini: *const u16,
    apply: bool,
    log: rsf_anomaly_sr_log,
    user: *mut c_void,
) -> bool {
    if ini.is_null() {
        return false;
    }
    let mut length = 0;
    while unsafe { ini.add(length).read() } != 0 {
        length += 1;
    }
    let path = unsafe { std::slice::from_raw_parts(ini, length) };
    STAGE.configure_ini(path, apply, log, user)
}

// The six live options in the shared overlay (AGENTS.md Rule 2), in RSF_ANOMALY_SR_OPTION_* order. The panel calls
// get and set on the Present-owner thread, which is the engine's render thread.
#[cfg(windows)]
fn register_live_options() {
    for option in &live_options() {
        // The registry copies the entry; labels and functions are static.
        unsafe { crate::overlay::rsf_anomaly_overlay_option_add(option) };
    }
}

#[cfg(windows)]
fn live_options() -> [crate::overlay_abi::rsf_anomaly_overlay_option; 6] {
    use crate::overlay_abi::rsf_anomaly_overlay_option;
    extern "C" fn get<const ID: u32>() -> i32 {
        STAGE.option(ID)
    }
    extern "C" fn set<const ID: u32>(value: i32) {
        STAGE.set_option(ID, value);
    }
    type Get = unsafe extern "C" fn() -> i32;
    type Set = unsafe extern "C" fn(i32);
    let toggle = |label: &'static CStr, get: Get, set: Set| rsf_anomaly_overlay_option {
        label: label.as_ptr(),
        toggle: 1,
        restart: 0,
        minimum: 0,
        maximum: 1,
        step: 1,
        divisor: 1,
        get: Some(get),
        set: Some(set),
    };
    [
        toggle(
            c"DLSS input: linear HDR colour",
            get::<RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT>,
            set::<RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT>,
        ),
        toggle(
            c"DLSS auto exposure",
            get::<RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO>,
            set::<RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO>,
        ),
        toggle(
            c"First-person pixels: zero motion",
            get::<RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO>,
            set::<RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO>,
        ),
        toggle(
            c"Subpixel jitter (SR needs it; off = no upscaling)",
            get::<RSF_ANOMALY_SR_OPTION_JITTER>,
            set::<RSF_ANOMALY_SR_OPTION_JITTER>,
        ),
        toggle(
            c"Texture mip bias",
            get::<RSF_ANOMALY_SR_OPTION_MIP_BIAS>,
            set::<RSF_ANOMALY_SR_OPTION_MIP_BIAS>,
        ),
        rsf_anomaly_overlay_option {
            toggle: 0,
            minimum: -20,
            maximum: 0,
            divisor: 10,
            ..toggle(
                c"Mip bias offset",
                get::<RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET>,
                set::<RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET>,
            )
        },
    ]
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_set_scale(value: f32) {
    STAGE.set_scale(value);
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_install(
    module: *mut c_void,
    image_size: u32,
    expected_size: u32,
    build: *const c_void,
) -> bool {
    STAGE.install(module, image_size, expected_size, build)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_quiesce() -> bool {
    STAGE.quiesce()
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_active() -> bool {
    STAGE.active()
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_owns_depth(native_frame: u32) -> bool {
    STAGE.owns_depth(native_frame)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_prepare(this: *mut c_void) {
    STAGE.prepare(this);
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_prepared() {
    STAGE.prepared();
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_seam(this: *mut c_void) {
    STAGE.seam(this);
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_combine_end() {
    STAGE.combine_end();
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_sun_begin() -> bool {
    STAGE.sun_begin()
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_sun_end(suspended: bool) {
    STAGE.sun_end(suspended);
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_position_setup(constant: *mut c_void) {
    STAGE.position_setup(constant);
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_rm_near() {
    STAGE.rm_near();
}
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_frame_end() {
    STAGE.frame_end();
}
// The controller's seam pass: `cookie` is the scope the enclosing seam call lent to the controller.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_upscale(
    cookie: *mut c_void,
    input: *const UpscaleInputs,
) -> bool {
    let (Some(scope), Some(input)) = (unsafe { cookie.cast::<Scope>().as_ref() }, unsafe {
        input.as_ref()
    }) else {
        return false;
    };
    let Some(seam) = (unsafe { Seam::upscale(input) }) else {
        return false;
    };
    scope.stage.upscale(unsafe { &mut *scope.state }, &seam)
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_round_trip(
    cookie: *mut c_void,
    twin: *const TargetFields,
    engine: *const TargetFields,
) -> bool {
    let (Some(scope), Some(twin), Some(engine)) = (
        unsafe { cookie.cast::<Scope>().as_ref() },
        unsafe { twin.as_ref() },
        unsafe { engine.as_ref() },
    ) else {
        return false;
    };
    scope.stage.round_trip(
        unsafe { &mut *scope.state },
        &Seam::round_trip(twin, engine),
    )
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_report(emit: rsf_anomaly_sr_log, user: *mut c_void) {
    let Some(emit) = emit else { return };
    STAGE.report(&mut |text| {
        if let Ok(text) = CString::new(text) {
            unsafe { emit(user, text.as_ptr()) };
        }
    });
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_jitter(out: *mut JitterState) {
    if !out.is_null() {
        unsafe { out.write(STAGE.jitter()) };
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_jitter_uv(out: *mut rsf_anomaly_sr_jitter_uv) {
    if !out.is_null() {
        unsafe { out.write(STAGE.jitter_uv()) };
    }
}
#[cfg(test)]
mod tests;
