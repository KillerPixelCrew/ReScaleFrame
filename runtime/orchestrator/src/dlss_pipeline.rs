// SPDX-License-Identifier: MIT
// The D3D11 SR pipeline owner behind rescaleframe/dlss_pipeline.h.
//
// This owns the provider session (Streamline load or shared host, device, support and planning),
// backend and quality switching, the per-frame sequence (decode, dense resolve, assembly, colour
// transport, evaluation, FG capture, decode back, colour correction, sharpening and the request
// dump), reset and give-up policy, the output and layer leases, status and teardown. Every SDK,
// D3D11, shader-pass and capture call is one leaf in dlss_pipeline_platform.h; the legacy FSR/XeSS
// adapter is the existing Rust owner in sr_legacy.rs.
//
// Locking: the frame-thread state is taken with try_lock, so a reentrant or concurrent call to a
// frame-thread entry refuses instead of racing. Cross-thread fields sit behind a second lock that
// is never held across a leaf call or a log line. The sink has its own lock, so vendor lines
// forwarded from inside a leaf never wait on either.
use crate::com_ref::ComRef;
use crate::d3d11_output_abi::{
    RSF_D3D11_BIND_RENDER_TARGET, RSF_D3D11_BIND_SHADER_RESOURCE, RSF_D3D11_FORMAT_RG16_FLOAT,
    RSF_D3D11_FORMAT_RG32_FLOAT, RSF_D3D11_FORMAT_RGBA16_FLOAT, RSF_D3D11_USAGE_DEFAULT,
    rsf_d3d11_texture_desc as TextureDesc,
};
use crate::dlss_pipeline_abi::*;
use crate::frame_assembly::rsf_assemble_dlss_frame;
use crate::native_sr_abi::{
    RSF_DLSS_PIPELINE_ABI_VERSION as ABI, rsf_dlss_pipeline_frame as Frame,
    rsf_motion_decode_params as DecodeParams,
};
use crate::render_abi::{
    RSF_FRAME_ASSEMBLY_ABI_VERSION, RSF_SHARPENING_ABI_VERSION, rsf_dlss_frame as DlssFrame,
    rsf_frame_resources, rsf_motion_resolve_params, rsf_pipeline_camera_frame as Camera,
    rsf_sharpening_options as Sharpening,
};
use crate::sr_legacy::{
    Adapter, rsf_sr_legacy_destroy, rsf_sr_legacy_evaluate, rsf_sr_legacy_fg_inputs,
    rsf_sr_legacy_select, rsf_sr_legacy_warm_up,
};
use std::ffi::{CStr, CString, c_char};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::Instant;

type P = rsf_dlssp_platform;
type Setup = rsf_dlss_pipeline_setup;
type Status = rsf_dlss_pipeline_status;
type Layer = rsf_dlss_pipeline_layer;

pub(super) const OK: i32 = 0;
pub(super) const INVALID: i32 = -1;
pub(super) const ABI_MISMATCH: i32 = -2;
pub(super) const ALREADY_RUNNING: i32 = -3;
pub(super) const NOT_RUNNING: i32 = -4;
pub(super) const STREAMLINE_FAILED: i32 = -5;
pub(super) const NOT_SUPPORTED: i32 = -6;
pub(super) const RESOURCE_FAILED: i32 = -7;
pub(super) const MOTION_DECODE_FAILED: i32 = -8;
pub(super) const FRAME_REFUSED: i32 = -9;
pub(super) const EVALUATE_FAILED: i32 = -10;
// rsf_dlss_result: the adapter, driver or OS cannot run DLSS.
const DLSS_NOT_SUPPORTED: i32 = -7;
// sr_session.h backend identifiers and dlss.h quality.
const SR_NONE: u32 = crate::NONE;
const SR_DLSS: u32 = crate::DLSS;
const SR_XESS: u32 = crate::XESS;
const QUALITY_NATIVE: u32 = 0;
// D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION and D3D11_BIND_UNORDERED_ACCESS.
const MAX_EXTENT: u32 = 16384;
const BIND_UNORDERED_ACCESS: u32 = 128;
const LAYER_VIEWPORT: u32 = 1;
// texture_dump.h views.
const VIEW_RAW: u32 = 0;
const VIEW_VELOCITY: u32 = 1;
const VIEW_DECODED_MOTION: u32 = 2;
const VIEW_HDR: u32 = 3;
const VIEW_DEPTH: u32 = 4;
const VIEW_SCALAR: u32 = 5;
const VIEW_DENSE_MOTION: u32 = 7;
const DUMP_OK: i32 = 0;
// rsf::say formats into a 512-byte buffer.
const LINE_LIMIT: usize = 511;

/* How many evaluates may fail in a row before the pipeline stops asking. Generous enough that a
resource rebuild or a mission load is never mistaken for a wall, small enough that a wall costs a
fraction of a second rather than the session.

A failing evaluate is not free. NGX tries to create its feature on each one, and when that cannot
succeed the attempts cost time and memory that are never returned: the game slows down frame by
frame until it stops responding. A single success resets the count, because an evaluate that fails
while resources are being rebuilt is ordinary. */
pub(super) const FAILURE_LIMIT: u32 = 120;

/// The last failure that reached the log and the render size it named. A frame that fails usually
/// fails the same way on the next one, and the sink appends and closes per line.
#[derive(Clone, Copy, Default, PartialEq)]
struct Reported {
    result: i32,
    extent: [u32; 2],
}

impl Reported {
    fn worth_saying(&mut self, result: i32, extent: [u32; 2]) -> bool {
        let next = Self { result, extent };
        if *self == next {
            return false;
        }
        *self = next;
        true
    }
}

#[derive(Clone, Copy, Default)]
struct Counters {
    evaluated: u64,
    refused: u64,
    last_result: i32,
}

/// The render sizes a frame may carry, and the one planned for.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Range {
    width: u32,
    height: u32,
    min: [u32; 2],
    max: [u32; 2],
}

impl Range {
    /// A build of the SDK that reports no range accepts the optimal size only, rather than
    /// being read as "any size".
    fn of(plan: rsf_dlssp_range) -> Self {
        let or = |value: u32, fallback: u32| if value != 0 { value } else { fallback };
        Self {
            width: plan.width,
            height: plan.height,
            min: [
                or(plan.min_width, plan.width),
                or(plan.min_height, plan.height),
            ],
            max: [
                or(plan.max_width, plan.width),
                or(plan.max_height, plan.height),
            ],
        }
    }

    /// Taken as reported, without the zero defaulting above.
    fn reported(plan: rsf_dlssp_range) -> Self {
        Self {
            width: plan.width,
            height: plan.height,
            min: [plan.min_width, plan.min_height],
            max: [plan.max_width, plan.max_height],
        }
    }

    /// The alternate backends accept any render size up to the output.
    fn alternate(output: [u32; 2]) -> Self {
        Self {
            width: 0,
            height: 0,
            min: [1, 1],
            max: output,
        }
    }
}

/// Fields read or written from other threads.
struct Shared {
    running: bool,
    supported: bool,
    output: [u32; 2],
    range: Range,
    preset: u32,
    sharpening: Option<Sharpening>,
    quality: u32,
    scene: Counters,
    layer: Counters,
    layer_extent: [u32; 2],
    backend: u32,
    requested_backend: u32,
    last_switch_result: i32,
    dump: Option<Vec<u8>>,
    reset_pending: bool,
    given_up: bool,
    failures: u32,
}

impl Shared {
    const fn new() -> Self {
        Self {
            running: false,
            supported: false,
            output: [0; 2],
            range: Range {
                width: 0,
                height: 0,
                min: [0; 2],
                max: [0; 2],
            },
            preset: 0,
            sharpening: None,
            quality: QUALITY_NATIVE,
            scene: Counters {
                evaluated: 0,
                refused: 0,
                last_result: OK,
            },
            layer: Counters {
                evaluated: 0,
                refused: 0,
                last_result: OK,
            },
            layer_extent: [0; 2],
            backend: SR_DLSS,
            requested_backend: SR_DLSS,
            last_switch_result: 0,
            dump: None,
            reset_pending: false,
            given_up: false,
            failures: 0,
        }
    }

    // Starting again, a preset, a quality, a backend or an output change is the way back from
    // having given up, and the next frame starts without history.
    fn restart_history(&mut self) {
        self.reset_pending = true;
        self.given_up = false;
        self.failures = 0;
    }
}

/// Owned by the thread that drives frames.
struct Owned {
    device: Option<ComRef>,
    output: Option<ComRef>,
    streamline_loaded: bool,
    // A cold viewport has no DLSS resources to release.
    dlss_evaluation_attempted: bool,
    // The decode pass is built for one render size. A size that failed to build is not retried
    // per frame: an HLSL compile and two allocations fail for reasons the next frame cannot fix.
    decode: Raw,
    decode_extent: [u32; 2],
    decode_failed: [u32; 2],
    dense: Raw,
    dense_extent: [u32; 2],
    hints_reported: bool,
    clouds_reported: bool,
    fidelity: Raw,
    fidelity_failed: bool,
    transport: Raw,
    transport_failed: bool,
    transport_reported: bool,
    motion: DecodeParams,
    reported: Reported,
    alternate: *mut Adapter,
    // Whether the frames offer an exposure texture, so that an alternate context is created with
    // the policy the first evaluate will ask for.
    exposure_manual: bool,
    last_evaluated: [u32; 3],
    directories: Vec<CString>,
    units_to_meters: f32,
    // setup.standard_depth: the convention the alternate backends' contexts are opened with.
    standard_depth: u32,
    sharpen: Raw,
    sharpen_reported: bool,
    // The layer feature: viewport 1, one to one, with its own history.
    layer_output: Option<ComRef>,
    layer_motion: Option<ComRef>,
    layer_extent: [u32; 2],
    layer_rebuilt: bool,
    layer_failures: u32,
    layer_given_up: bool,
    layer_reported: Reported,
}

// SAFETY: the D3D11 objects and pass handles are used only by the frame-driving owner, under the
// owner lock, and released by it.
unsafe impl Send for Owned {}

impl Owned {
    const fn new() -> Self {
        Self {
            device: None,
            output: None,
            streamline_loaded: false,
            dlss_evaluation_attempted: false,
            decode: null_mut(),
            decode_extent: [0; 2],
            decode_failed: [0; 2],
            dense: null_mut(),
            dense_extent: [0; 2],
            hints_reported: false,
            clouds_reported: false,
            fidelity: null_mut(),
            fidelity_failed: false,
            transport: null_mut(),
            transport_failed: false,
            transport_reported: false,
            motion: unsafe { std::mem::zeroed() },
            reported: Reported {
                result: OK,
                extent: [0; 2],
            },
            alternate: null_mut(),
            exposure_manual: false,
            last_evaluated: [0; 3],
            directories: Vec::new(),
            units_to_meters: 1.0,
            standard_depth: 0,
            sharpen: null_mut(),
            sharpen_reported: false,
            layer_output: None,
            layer_motion: None,
            layer_extent: [0; 2],
            layer_rebuilt: false,
            layer_failures: 0,
            layer_given_up: false,
            layer_reported: Reported {
                result: OK,
                extent: [0; 2],
            },
        }
    }

    fn device(&self) -> Raw {
        self.device.as_ref().map_or(null_mut(), ComRef::as_ptr)
    }

    fn output(&self) -> Raw {
        self.output.as_ref().map_or(null_mut(), ComRef::as_ptr)
    }

    fn directory_pointers(&self) -> [*const c_char; 4] {
        std::array::from_fn(|i| self.directories.get(i).map_or(c"".as_ptr(), |d| d.as_ptr()))
    }
}

#[derive(Clone, Copy)]
struct Sink {
    log: rsf_dlss_pipeline_log_fn,
    user: usize,
}

/* Input conditioning this pipeline does for a backend before it evaluates. The alternate backends
condition their own inputs in the legacy adapter, which resolves dense motion with the unwritten
sentinel and converts units, so only DLSS declares anything here. Colour transport and colour
correction are remedies for DLSS's own behaviour: its auto-exposing presets band in dark linear
HDR, and its output is pulled toward the current jittered frame. */
#[derive(Clone, Copy)]
struct Needs {
    dense_motion: bool,
    colour_transport: bool,
    colour_correction: bool,
}

fn needs_of(backend: u32) -> Needs {
    let dlss = backend == SR_DLSS;
    Needs {
        dense_motion: dlss,
        colour_transport: dlss,
        colour_correction: dlss,
    }
}

/* Whether the decode writes the sentinel for some pixels: a stored zero under the original rule, or
an invalid validity channel. Either way those pixels need camera motion reconstructed, and a
validity-marked decode leaves zero for valid stationary pixels, which must not be read as
unwritten. */
fn marks_unwritten(motion: &DecodeParams) -> bool {
    motion.zero_means_unwritten != 0 || motion.validity_enabled != 0
}

// Canonical input is current-minus-previous UV. DLSS consumes displacement from the current pixel
// toward its previous location. Dense resolve already emits that displacement; legacy unspecified
// encodings retain their scale. The same rule as motion_convention.h.
fn dlss_motion_scale(scale: f32, canonical_uv: bool, resolved: bool) -> f32 {
    if canonical_uv && !resolved {
        -scale
    } else {
        scale
    }
}

// printf's %.9g for the diagnostic frame record.
fn g9(value: f64) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }
        .into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let trim = |text: String| {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text
        }
    };
    let scientific = format!("{value:.8e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-4..9).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{sign}{:02}",
            trim(mantissa.to_owned()),
            exponent.unsigned_abs()
        )
    } else {
        trim(format!("{value:.*}", (8 - exponent) as usize))
    }
}

fn texture_desc(width: u32, height: u32, format: u32, bind_flags: u32) -> TextureDesc {
    TextureDesc {
        width,
        height,
        mips: 1,
        layers: 1,
        format,
        samples: 1,
        usage: RSF_D3D11_USAGE_DEFAULT,
        bind_flags,
        ..Default::default()
    }
}

unsafe fn has_capacity<T>(value: *const T) -> bool {
    !value.is_null() && unsafe { value.cast::<u32>().read() } as usize >= size_of::<T>()
}

unsafe fn copy_string(text: *const c_char) -> CString {
    if text.is_null() {
        CString::default()
    } else {
        unsafe { CStr::from_ptr(text) }.to_owned()
    }
}

pub(super) struct Pipeline {
    owned: Mutex<Owned>,
    shared: Mutex<Shared>,
    sink: Mutex<Sink>,
    // Published for the output getters, which carry no lock and may be asked from inside a leaf.
    output: AtomicPtr<std::ffi::c_void>,
    layer_output: AtomicPtr<std::ffi::c_void>,
    colour_correction: AtomicBool,
    colour_transport: AtomicBool,
}

// One pipeline per process, because the entry points carry no handle: there is one game, one
// device and one Streamline.
static PROCESS: Pipeline = Pipeline::new();

unsafe extern "C" fn forward_dlss(_: Raw, message: *const c_char) {
    unsafe { PROCESS.forward(b"dlss", message) };
}
unsafe extern "C" fn forward_decode(_: Raw, message: *const c_char) {
    unsafe { PROCESS.forward(b"motion decode", message) };
}
unsafe extern "C" fn forward_dump(_: Raw, message: *const c_char) {
    unsafe { PROCESS.forward(b"dump", message) };
}

impl Pipeline {
    pub(super) const fn new() -> Self {
        Self {
            owned: Mutex::new(Owned::new()),
            shared: Mutex::new(Shared::new()),
            sink: Mutex::new(Sink { log: None, user: 0 }),
            output: AtomicPtr::new(null_mut()),
            layer_output: AtomicPtr::new(null_mut()),
            colour_correction: AtomicBool::new(false),
            colour_transport: AtomicBool::new(true),
        }
    }

    fn owned(&self) -> Option<MutexGuard<'_, Owned>> {
        match self.owned.try_lock() {
            Ok(owned) => Some(owned),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_sink(&self, sink: Sink) {
        *self.sink.lock().unwrap_or_else(PoisonError::into_inner) = sink;
    }

    // One line through the caller's sink, cut where rsf::say cut it.
    fn emit(&self, text: &[u8]) {
        let sink = *self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(log) = sink.log else {
            return;
        };
        let end = text
            .iter()
            .take(LINE_LIMIT)
            .position(|&byte| byte == 0)
            .unwrap_or(text.len().min(LINE_LIMIT));
        let mut line = text[..end].to_vec();
        line.push(0);
        unsafe { log(sink.user as Raw, line.as_ptr().cast()) };
    }

    fn say(&self, text: impl AsRef<[u8]>) {
        self.emit(text.as_ref());
    }

    // Streamline, the decode pass and the dump each take their own sink. All are forwarded to ours
    // with a prefix, so one log carries the whole sequence in the order it happened.
    unsafe fn forward(&self, prefix: &[u8], message: *const c_char) {
        let mut line = prefix.to_vec();
        line.extend_from_slice(b": ");
        if !message.is_null() {
            line.extend_from_slice(unsafe { CStr::from_ptr(message) }.to_bytes());
        }
        self.emit(&line);
    }

    fn set_output(&self, owned: &mut Owned, output: Option<ComRef>) -> Option<ComRef> {
        let raw = output.as_ref().map_or(null_mut(), ComRef::as_ptr);
        let previous = std::mem::replace(&mut owned.output, output);
        self.output.store(raw, Ordering::Release);
        previous
    }

    fn release_layer(&self, owned: &mut Owned) {
        self.layer_output.store(null_mut(), Ordering::Release);
        owned.layer_output = None;
        owned.layer_motion = None;
        owned.layer_extent = [0; 2];
        self.shared().layer_extent = [0; 2];
    }

    // Record the outcome of a frame. Every frame that did not reach a successful evaluate counts as
    // refused, whether this code refused it or the backend did, so that the two counters sum to the
    // well formed frames offered while running. A call that carried no frame at all, or the wrong
    // ABI, is counted as neither.
    fn count(&self, layer: bool, result: i32) -> i32 {
        let mut shared = self.shared();
        let counters = if layer {
            &mut shared.layer
        } else {
            &mut shared.scene
        };
        if result == OK {
            counters.evaluated += 1;
        } else {
            counters.refused += 1;
        }
        counters.last_result = result;
        result
    }

    fn finish(&self, owned: &mut Owned, result: i32) -> i32 {
        if result == OK {
            // A failure that follows a good frame is news again.
            owned.reported = Reported::default();
        }
        self.count(false, result)
    }

    /* A float colour target at output size with every bind, cleared to black once. A fresh texture
    holds whatever was in that memory and a reconstruction does not write every pixel of it: a frame
    whose render size differs from the one the feature was built for left a corner untouched, and
    the previous tenant of that memory showed through as blocks. R16G16B16A16_FLOAT is the format
    of the full resolution colour targets the input came from, wide enough for pre-tonemap values.
    DLSS writes it, whatever reinserts it reads it, and a compute pass may yet need to touch it. */
    unsafe fn create_cleared_target(
        &self,
        p: &P,
        device: Raw,
        width: u32,
        height: u32,
    ) -> Option<ComRef> {
        let desc = texture_desc(
            width,
            height,
            RSF_D3D11_FORMAT_RGBA16_FLOAT,
            RSF_D3D11_BIND_RENDER_TARGET | RSF_D3D11_BIND_SHADER_RESOURCE | BIND_UNORDERED_ACCESS,
        );
        let target = unsafe { ComRef::from_owned((p.create_texture)(device, &desc)) }?;
        unsafe { (p.clear_black)(device, target.as_ptr()) };
        Some(target)
    }

    unsafe fn create_alternate(&self, p: &P, owned: &Owned, width: u32, height: u32) -> (i32, Raw) {
        let sink = *self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let directories = owned.directory_pointers();
        let mut adapter = null_mut();
        let result = unsafe {
            (p.alternate_create)(
                owned.device(),
                width,
                height,
                directories.as_ptr(),
                owned.units_to_meters,
                owned.standard_depth,
                u32::from(!owned.exposure_manual),
                sink.log,
                sink.user as Raw,
                &mut adapter,
            )
        };
        (result, adapter)
    }

    // Undo whatever is held, in the reverse of the order it was acquired. Safe at any point in a
    // partially completed start, which is what its failure paths rely on.
    unsafe fn tear_down(&self, p: &P, owned: &mut Owned) {
        unsafe {
            (p.native_sr_release)();
            if !owned.fidelity.is_null() {
                (p.fidelity_destroy)(owned.fidelity);
            }
            if !owned.transport.is_null() {
                (p.transport_destroy)(owned.transport);
            }
            owned.fidelity = null_mut();
            owned.transport = null_mut();
            owned.transport_failed = false;
            owned.transport_reported = false;
            owned.fidelity_failed = false;
            if !owned.dense.is_null() {
                (p.resolve_destroy)(owned.dense);
            }
            owned.dense = null_mut();
            owned.hints_reported = false;
            owned.clouds_reported = false;
            owned.dense_extent = [0; 2];
            if !owned.decode.is_null() {
                (p.decode_destroy)(owned.decode);
            }
            owned.decode = null_mut();
            owned.decode_extent = [0; 2];
            owned.decode_failed = [0; 2];
        }
        drop(self.set_output(owned, None));
        self.release_layer(owned);
        unsafe { rsf_sr_legacy_destroy(owned.alternate) };
        owned.alternate = null_mut();
        if owned.streamline_loaded {
            self.say("shutting streamline down");
            // Frees the DLSS feature's own resources first. Streamline's shutdown does this too,
            // but the order matters when the caller's device is about to go.
            unsafe {
                (p.release_resources)();
                (p.release_viewport)(LAYER_VIEWPORT);
                (p.shutdown)();
            }
            owned.streamline_loaded = false;
        }
        owned.device = None;
        owned.dlss_evaluation_attempted = false;
        // Dropped after the last line above, because the DLSS backend keeps forward_dlss as its
        // sink for the life of the process and a later vendor line must not reach a `log_user`
        // the caller stopped owning when it stopped the pipeline.
        self.set_sink(Sink { log: None, user: 0 });
        owned.reported = Reported::default();
        owned.last_evaluated = [0; 3];
        owned.layer_failures = 0;
        owned.layer_given_up = false;
        owned.layer_reported = Reported::default();
        let mut shared = self.shared();
        shared.running = false;
        shared.backend = SR_DLSS;
        shared.requested_backend = SR_DLSS;
        shared.last_switch_result = 0;
        shared.supported = false;
        shared.dump = None;
        shared.layer = Counters::default();
    }

    pub(super) unsafe fn start(&self, p: &P, device: Raw, setup: *const Setup) -> i32 {
        if device.is_null() || !unsafe { has_capacity(setup) } {
            return INVALID;
        }
        let setup = unsafe { &*setup };
        if setup.streamline_directory_utf8.is_null()
            || unsafe { *setup.streamline_directory_utf8 } == 0
            || setup.output_width == 0
            || setup.output_height == 0
            || (setup.motion.struct_size as usize) < size_of::<DecodeParams>()
        {
            return INVALID;
        }
        if setup.abi_version != ABI {
            return ABI_MISMATCH;
        }
        let Some(mut guard) = self.owned() else {
            return ALREADY_RUNNING;
        };
        let owned = &mut *guard;
        if owned.streamline_loaded || owned.device.is_some() {
            return ALREADY_RUNNING;
        }
        // A zero decode scale multiplies every stored vector to nothing: a perfectly stable image
        // rather than a visible failure. There is no default worth guessing for an encoding.
        if setup.motion.scale_x == 0.0 || setup.motion.scale_y == 0.0 {
            return INVALID;
        }

        self.set_sink(Sink {
            log: setup.log,
            user: setup.log_user as usize,
        });
        owned.motion = setup.motion;
        owned.directories = [
            setup.fsr2_directory_utf8,
            setup.fsr3_directory_utf8,
            setup.fsr4_directory_utf8,
            setup.xess_directory_utf8,
        ]
        .into_iter()
        .map(|text| unsafe { copy_string(text) })
        .collect();
        owned.units_to_meters = setup.view_space_to_meters;
        owned.standard_depth = u32::from(setup.standard_depth != 0);
        owned.motion.struct_size = size_of::<DecodeParams>() as u32;
        // Per axis. A zero output scale on one axis decodes that axis to no motion at all, and no
        // one means it: a flip is -1.
        if owned.motion.output_scale_x == 0.0 {
            owned.motion.output_scale_x = 1.0;
        }
        if owned.motion.output_scale_y == 0.0 {
            owned.motion.output_scale_y = 1.0;
        }

        let mut plugins = unsafe { CStr::from_ptr(setup.streamline_directory_utf8) }
            .to_bytes()
            .to_vec();
        while matches!(plugins.last(), Some(b'\\' | b'/')) {
            plugins.pop();
        }
        let interposer = [plugins.as_slice(), b"\\sl.interposer.dll"].concat();
        self.say([b"loading streamline from ".as_slice(), &plugins].concat());
        let plugins = CString::new(plugins).unwrap_or_default();
        let interposer = CString::new(interposer).unwrap_or_default();
        let host = unsafe { (p.present_host)() };
        let loaded = if host.is_null() {
            unsafe {
                (p.load)(
                    setup,
                    interposer.as_ptr(),
                    plugins.as_ptr(),
                    Some(forward_dlss),
                )
            }
        } else {
            unsafe { (p.share_host)(host, setup.log, setup.log_user) }
        };
        if loaded != 0 {
            self.say(format!("streamline did not load (result {loaded})"));
            unsafe { self.tear_down(p, owned) };
            return STREAMLINE_FAILED;
        }
        owned.streamline_loaded = true;

        self.say("handing the game's device to streamline");
        owned.device = Some(unsafe { ComRef::retain(device) });
        let device_set = unsafe { (p.set_device)(device) };
        if device_set != 0 {
            self.say(format!(
                "streamline refused the device (result {device_set})"
            ));
            unsafe { self.tear_down(p, owned) };
            return STREAMLINE_FAILED;
        }

        self.say("asking whether DLSS runs on this adapter");
        let mut support = rsf_dlssp_support::default();
        let queried = unsafe { (p.query_support)(&mut support) };
        if queried != 0 || support.supported == 0 {
            // Which requirement failed is the difference between "update the driver" and "this
            // machine cannot do it".
            let flag = |set: u32, text: &'static str| if set != 0 { text } else { "" };
            self.say(format!(
                "DLSS is not available here (result {queried}), driver {}.{} against a required {}.{}{}{}{}",
                support.detected_major,
                support.detected_minor,
                support.required_major,
                support.required_minor,
                flag(support.driver_out_of_date, ", driver out of date"),
                flag(support.os_out_of_date, ", OS out of date"),
                flag(support.no_supported_adapter, ", no supported adapter"),
            ));
            unsafe { self.tear_down(p, owned) };
            return if queried == DLSS_NOT_SUPPORTED {
                NOT_SUPPORTED
            } else {
                STREAMLINE_FAILED
            };
        }

        // The render size comes from DLSS rather than from a ratio computed here: DLSS decides
        // what a quality level means, and a size that does not match is a rejected evaluate.
        let [width, height] = [setup.output_width, setup.output_height];
        self.say(format!(
            "asking DLSS what quality level {} renders at for {width}x{height}",
            setup.quality
        ));
        let mut plan = rsf_dlssp_range::default();
        let planned = unsafe { (p.plan)(width, height, setup.quality, &mut plan) };
        if planned != 0 || plan.width == 0 || plan.height == 0 {
            self.say(format!("DLSS gave no render size (result {planned})"));
            unsafe { self.tear_down(p, owned) };
            return STREAMLINE_FAILED;
        }

        self.say(format!("creating the {width}x{height} output target"));
        let Some(output) = (unsafe { self.create_cleared_target(p, device, width, height) }) else {
            self.say("the output target could not be created");
            unsafe { self.tear_down(p, owned) };
            return RESOURCE_FAILED;
        };
        drop(self.set_output(owned, Some(output)));
        {
            let mut shared = self.shared();
            shared.supported = true;
            shared.output = [width, height];
            shared.quality = setup.quality;
            shared.range = Range::of(plan);
            shared.scene = Counters::default();
            shared.failures = 0;
            shared.given_up = false;
            shared.running = true;
        }
        self.say(format!(
            "pipeline ready: DLSS renders {}x{} for {width}x{height}",
            plan.width, plan.height
        ));
        OK
    }

    pub(super) unsafe fn on_frame(&self, p: &P, context: Raw, frame: *const Frame) -> i32 {
        if context.is_null() || !unsafe { has_capacity(frame) } {
            return INVALID;
        }
        let frame = unsafe { &*frame };
        if frame.abi_version != ABI {
            return ABI_MISMATCH;
        }
        let Some(mut guard) = self.owned() else {
            return NOT_RUNNING;
        };
        let owned = &mut *guard;
        unsafe { self.evaluate_scene(p, owned, context, frame) }
    }

    unsafe fn evaluate_scene(&self, p: &P, o: &mut Owned, context: Raw, frame: &Frame) -> i32 {
        if o.device.is_none() || o.output.is_none() || !o.streamline_loaded {
            return NOT_RUNNING;
        }
        // Given up on, and said so once already. Returning before the decode and the assembly
        // means a run that cannot upscale costs the game nothing instead of more every frame.
        if self.shared().given_up {
            return self.finish(o, EVALUATE_FAILED);
        }
        if frame.scene_color.is_null()
            || frame.depth.is_null()
            || frame.game_motion.is_null()
            || frame.camera.is_null()
            || frame.render_width == 0
            || frame.render_height == 0
        {
            return self.finish(o, INVALID);
        }
        // Checked before the copy below, which reads a whole structure out of the caller's memory.
        if !unsafe { has_capacity(frame.camera) } {
            return self.finish(o, INVALID);
        }
        // Not left to assembly, which would report a camera from a mismatched build as a refused
        // frame, read as "no jitter". Uncounted, like the frame ABI.
        if unsafe { (*frame.camera).abi_version } != RSF_FRAME_ASSEMBLY_ABI_VERSION {
            return ABI_MISMATCH;
        }
        o.exposure_manual = !frame.exposure.is_null();
        let render = [frame.render_width, frame.render_height];
        let [rw, rh] = render;

        // The sequence is announced once; failures and size changes still speak every time.
        let (first, range, [ow, oh], quality) = {
            let shared = self.shared();
            (
                shared.scene.evaluated == 0 && shared.scene.refused == 0,
                shared.range,
                shared.output,
                shared.quality,
            )
        };
        if rw < range.min[0] || rw > range.max[0] || rh < range.min[1] || rh > range.max[1] {
            if o.reported.worth_saying(INVALID, render) {
                self.say(format!(
                    "frame renders at {rw}x{rh}, outside the {}x{} to {}x{} DLSS accepts at this quality",
                    range.min[0], range.min[1], range.max[0], range.max[1]
                ));
            }
            return self.finish(o, INVALID);
        }

        // A decode pass is built for one size, and so is the feature behind it. A render scale
        // change therefore costs both.
        if !o.decode.is_null() && o.decode_extent != render {
            self.say(format!(
                "render size moved from {}x{} to {rw}x{rh}, rebuilding",
                o.decode_extent[0], o.decode_extent[1]
            ));
            unsafe { (p.decode_destroy)(o.decode) };
            o.decode = null_mut();
            o.decode_extent = [0; 2];
            if o.dlss_evaluation_attempted && unsafe { (p.release_resources)() } == 0 {
                o.dlss_evaluation_attempted = false;
            }
        }
        // The first frame a freshly built feature sees has no usable history; reusing it smears.
        // The caller cannot know this: it does not see the rebuild.
        let rebuilt = o.decode.is_null();
        if o.decode.is_null() {
            if o.decode_failed == render {
                return self.finish(o, RESOURCE_FAILED);
            }
            self.say(format!("building the motion decode pass for {rw}x{rh}"));
            // Decoded NDC motion spans [-2,2]; the dense resolve consumes R32G32 on D3D11 and only
            // its R16G16_FLOAT output is shared with D3D12.
            let mut decode = null_mut();
            let built = unsafe {
                (p.decode_create)(
                    o.device(),
                    rw,
                    rh,
                    RSF_D3D11_FORMAT_RG32_FLOAT,
                    Some(forward_decode),
                    &mut decode,
                )
            };
            if built != 0 || decode.is_null() {
                if o.reported.worth_saying(RESOURCE_FAILED, render) {
                    self.say(format!(
                        "the motion decode pass could not be built (result {built})"
                    ));
                }
                o.decode_failed = render;
                return self.finish(o, RESOURCE_FAILED);
            }
            o.decode = decode;
            o.decode_extent = render;
            o.decode_failed = [0; 2];
        }
        if first {
            self.say("first frame: decoding motion, assembling, evaluating");
        }

        // Mandatory: no backend can subtract the bias of Unreal-style storage.
        let decoded = unsafe { (p.decode_run)(o.decode, context, frame.game_motion, &o.motion) };
        if decoded != 0 {
            if o.reported.worth_saying(MOTION_DECODE_FAILED, render) {
                self.say(format!("the motion decode did not run (result {decoded})"));
            }
            return self.finish(o, MOTION_DECODE_FAILED);
        }

        // Only the fields this pipeline is the one to know. Matrices, jitter and the camera basis
        // are used exactly as the caller read them from the game.
        let mut camera: Camera = unsafe { *frame.camera };
        camera.struct_size = size_of::<Camera>() as u32;
        camera.motion_decoded = 1;
        camera.has_motion_sentinel = u32::from(marks_unwritten(&o.motion));
        camera.motion_sentinel = o.motion.invalid_value;
        // A caller that gives a scale gives canonical current-minus-previous UV.
        let motion_in_uv = camera.motion_scale != [0.0; 2];
        if !motion_in_uv {
            camera.motion_scale = [1.0; 2];
        }
        // Or'd in, never cleared: the caller's own reasons for a reset stand.
        let reset_pending = self.shared().reset_pending;
        if rebuilt || reset_pending {
            camera.reset = 1;
        }

        let resources = rsf_frame_resources {
            struct_size: size_of::<rsf_frame_resources>() as u32,
            color_in: frame.scene_color,
            color_out: o.output(),
            depth: frame.depth,
            motion: unsafe { (p.decode_texture)(o.decode) },
            exposure: frame.exposure,
            render_width: rw,
            render_height: rh,
            output_width: ow,
            output_height: oh,
            quality,
        };
        let mut dlss: DlssFrame = unsafe { std::mem::zeroed() };
        dlss.struct_size = size_of::<DlssFrame>() as u32;
        let assembled = unsafe { rsf_assemble_dlss_frame(&camera, &resources, &mut dlss) };
        dlss.preset = self.shared().preset;
        if assembled != 0 {
            // A frame refused for want of jitter repeats for as long as the game runs.
            if o.reported.worth_saying(FRAME_REFUSED, render) {
                self.say(format!(
                    "the frame was refused before DLSS saw it (result {assembled})"
                ));
            }
            return self.finish(o, FRAME_REFUSED);
        }
        dlss.color_before_transparency = frame.color_before_transparency;
        dlss.transparency_layer = frame.transparency_layer;
        dlss.reactive_mask = frame.reactive_mask;
        dlss.transparency_hint = frame.transparency_mask;
        dlss.bias_current_color = frame.bias_mask;
        dlss.motion_depth_layer = frame.motion_depth_layer;
        if !frame.reactive_mask.is_null() && !o.hints_reported {
            o.hints_reported = true;
            self.say(format!(
                "translucency hints: opaque-only colour{}, reactive, coverage and bias masks at {rw}x{rh}",
                if frame.transparency_layer.is_null() {
                    ""
                } else {
                    ", separate layer"
                }
            ));
        }
        if !frame.motion_depth_layer.is_null() && !o.clouds_reported {
            o.clouds_reported = true;
            self.say(
                "cloud motion: unwritten pixels reproject at the nearer of cloud and scene depth",
            );
        }

        let backend = self.shared().backend;
        let needs = needs_of(backend);
        let mut resolved = false;
        // Shared D3D12 evaluation also needs copy-compatible R16G16 motion and R32 depth, even when
        // the producer already includes camera motion in every sample.
        let reconstruct = camera.camera_motion_included == 0 && camera.has_motion_sentinel != 0;
        if needs.dense_motion && (reconstruct || !unsafe { (p.present_host)() }.is_null()) {
            let Some(rebuilt_resolve) = (unsafe { self.fit_resolve(p, o, render) }) else {
                return self.finish(o, RESOURCE_FAILED);
            };
            if rebuilt_resolve {
                self.say(format!(
                    "DLSS input normalization: R16G16 pixel motion and R32 depth, preserving valid zero vectors at {rw}x{rh}"
                ));
            }
            // Canonical inputs are current-minus-previous UV; legacy decode is NDC with Y up.
            // Resolve emits previous-minus-current pixels, without adding camera motion to written
            // samples.
            let params = rsf_motion_resolve_params {
                struct_size: size_of::<rsf_motion_resolve_params>() as u32,
                clip_to_previous: dlss.clip_to_prev_clip,
                decoded_to_pixels: [
                    -(rw as f32) * dlss.motion_scale_x * if motion_in_uv { 1.0 } else { 0.5 },
                    -(rh as f32) * dlss.motion_scale_y * if motion_in_uv { 1.0 } else { -0.5 },
                ],
                sentinel: camera.motion_sentinel,
                has_sentinel: camera.has_motion_sentinel,
                depth_layer: frame.motion_depth_layer,
                jitter_pixels: camera.jitter_pixels,
            };
            if unsafe { (p.resolve_run)(o.dense, context, dlss.motion, dlss.depth, &params) } == 0 {
                return self.finish(o, RESOURCE_FAILED);
            }
            dlss.motion = unsafe { (p.resolve_motion)(o.dense) };
            dlss.depth = unsafe { (p.resolve_depth)(o.dense) };
            dlss.camera_motion_included = 1;
            dlss.motion_scale_x = 1.0 / rw as f32;
            dlss.motion_scale_y = 1.0 / rh as f32;
            resolved = true;
        }
        if backend == SR_DLSS {
            dlss.motion_scale_x = dlss_motion_scale(dlss.motion_scale_x, motion_in_uv, resolved);
            dlss.motion_scale_y = dlss_motion_scale(dlss.motion_scale_y, motion_in_uv, resolved);
        }
        let correct_colour =
            needs.colour_correction && self.colour_correction.load(Ordering::Acquire);
        if correct_colour && o.fidelity.is_null() {
            if !o.fidelity_failed {
                o.fidelity = unsafe { (p.fidelity_create)(o.device()) };
                if o.fidelity.is_null() {
                    o.fidelity_failed = true;
                    self.say("DLSS colour correction could not create its GPU pass; retaining the native graph");
                }
            }
            if o.fidelity.is_null() {
                return self.finish(o, RESOURCE_FAILED);
            }
            self.say("DLSS colour correction: current linear scene colour, depth-guarded bounded RGB residual, before native tonemapping");
        }
        // DLSS receives an invertible display-range encoding with HDR input off, on every preset,
        // and its output is decoded back to linear before the engine grades it: its auto-exposing
        // presets band in dark linear HDR, and a driver override can replace any preset choice.
        let transported = needs.colour_transport && self.colour_transport.load(Ordering::Acquire);
        if transported && o.transport.is_null() && !o.transport_failed {
            o.transport = unsafe { (p.transport_create)(o.device()) };
            if o.transport.is_null() {
                o.transport_failed = true;
                self.say("DLSS colour transport could not create its GPU passes; DLSS receives linear HDR");
            }
        }
        if transported && !o.transport.is_null() {
            let mut encoded = null_mut();
            if unsafe {
                (p.transport_encode)(
                    o.transport,
                    context,
                    frame.scene_color,
                    frame.exposure,
                    rw,
                    rh,
                    &mut encoded,
                )
            } == 0
            {
                return self.finish(o, RESOURCE_FAILED);
            }
            dlss.color_in = encoded;
            dlss.color_encoded = 1;
            dlss.exposure = null_mut();
            if !o.transport_reported {
                o.transport_reported = true;
                self.say("DLSS colour transport: exposed display-range encoding in, HDR input off, decoded to linear with highlight recovery");
            }
        }
        unsafe { self.dump_inputs(p, o, context, frame) };

        let sharpen = self
            .shared()
            .sharpening
            .unwrap_or_else(|| unsafe { defaults(p) });
        let evaluated = if backend == SR_DLSS {
            o.dlss_evaluation_attempted = true;
            unsafe { (p.fg_evaluate)(context, &dlss) }
        } else {
            // The legacy adapter takes canonical UV. The decode's own output, with no scale from
            // the caller, is current-minus-previous NDC with y up.
            let mut alternate = dlss;
            if !motion_in_uv {
                alternate.motion_scale_x *= 0.5;
                alternate.motion_scale_y *= -0.5;
            }
            unsafe {
                rsf_sr_legacy_evaluate(
                    o.alternate,
                    context,
                    &alternate,
                    u32::from(marks_unwritten(&o.motion)),
                    (p.vendor_sharpness)(&sharpen, backend),
                )
            }
        };
        // Every backend offers its frame to FG capture, which keeps one copy per frame. The
        // alternate backends hand over the adapter's dense inputs.
        if evaluated == 0 {
            let mut generation = dlss;
            if backend != SR_DLSS
                && unsafe {
                    rsf_sr_legacy_fg_inputs(
                        o.alternate,
                        &mut generation.depth,
                        &mut generation.motion,
                    )
                } != 0
            {
                generation.camera_motion_included = 1;
            }
            unsafe { (p.fg_capture)(context, &generation) };
        }
        if evaluated != 0 {
            let failures = {
                let mut shared = self.shared();
                shared.failures += 1;
                if shared.failures >= FAILURE_LIMIT {
                    shared.given_up = true;
                }
                shared.failures
            };
            if failures >= FAILURE_LIMIT {
                self.say(format!(
                    "SR failed to evaluate {failures} frames in a row (last result {evaluated}), so it will not be \
asked again. Each attempt costs time and memory that is not returned, and a game \
slowing to a stop hides the error rather than showing it. Restart the backend to \
try again. If the log above says NGX create feature failed, a capture layer such \
as RenderDoc is the usual reason: it wraps the device and NGX refuses it"
                ));
            } else if o.reported.worth_saying(EVALUATE_FAILED, render) {
                self.say(format!(
                    "SR did not evaluate this frame (result {evaluated})"
                ));
            }
            return self.finish(o, EVALUATE_FAILED);
        }
        let output = o.output();
        unsafe {
            (p.input_dump)(
                context,
                c"sr_backend_output".as_ptr(),
                c"SR backend output before colour-transport decode and optional fidelity correction"
                    .as_ptr(),
                output,
                if dlss.color_encoded != 0 {
                    VIEW_RAW
                } else {
                    VIEW_HDR
                },
                1.0,
            )
        };
        if dlss.color_encoded != 0
            && unsafe {
                (p.transport_decode)(
                    o.transport,
                    context,
                    output,
                    frame.exposure,
                    ow,
                    oh,
                    frame.scene_color,
                    rw,
                    rh,
                    camera.jitter_pixels.as_ptr(),
                )
            } == 0
        {
            self.shared().reset_pending = true;
            return self.finish(o, RESOURCE_FAILED);
        }
        if first {
            // Not "DLSS is working": the inputs were accepted and the output written. Whether the
            // image is correct is answered by looking at it, which the dump is for.
            self.say(
                "SR evaluated a frame. This says the inputs were accepted, not that the image is right.",
            );
        }

        // Taken here rather than in request_dump: the requesting thread is a key poller, and an
        // immediate context used from two threads at once has taken this process down before.
        let prefix = self.shared().dump.take();
        if correct_colour {
            if let Some(prefix) = &prefix {
                let raw = CString::new([prefix.as_slice(), b"_output_uncorrected"].concat())
                    .unwrap_or_default();
                unsafe {
                    (p.dump_bytes)(
                        o.device(),
                        context,
                        output,
                        raw.as_ptr(),
                        VIEW_RAW,
                        0.0,
                        Some(forward_dump),
                    )
                };
            }
            if unsafe {
                (p.fidelity_run)(
                    o.fidelity,
                    context,
                    frame.scene_color,
                    output,
                    dlss.depth,
                    dlss.depth_inverted,
                    camera.jitter_pixels.as_ptr(),
                )
            } == 0
            {
                self.shared().reset_pending = true;
                if o.reported.worth_saying(RESOURCE_FAILED, render) {
                    self.say(
                        "DLSS colour correction refused this frame; retaining the native graph",
                    );
                }
                return self.finish(o, RESOURCE_FAILED);
            }
        }
        // Post-SR, after decoding and fidelity correction. Failure keeps the vendor's valid output.
        unsafe { self.sharpen(p, o, context, frame, &dlss, &sharpen, backend) };
        if let Some(prefix) = prefix {
            unsafe { self.write_dump(p, o, context, frame, &dlss, backend, &prefix, [ow, oh]) };
        }
        unsafe {
            (p.input_dump)(
                context,
                c"sr_output".as_ptr(),
                c"Reconstructed linear HDR image reinserted before native tonemapping".as_ptr(),
                output,
                VIEW_HDR,
                1.0,
            )
        };

        // One good frame means the run of failures was a rebuild rather than a wall.
        if o.last_evaluated != [backend, rw, rh] {
            self.say(format!(
                "SR submitted successfully: backend {backend}, input {rw}x{rh}, output {ow}x{oh}; reconstruction output ready in GPU command order"
            ));
            o.last_evaluated = [backend, rw, rh];
        }
        {
            let mut shared = self.shared();
            shared.reset_pending = false;
            shared.failures = 0;
        }
        self.finish(o, OK)
    }

    // Keep a resolve pass built for the render size. A failed rebuild keeps the previous pass.
    unsafe fn fit_resolve(&self, p: &P, o: &mut Owned, render: [u32; 2]) -> Option<bool> {
        if !o.dense.is_null() && o.dense_extent == render {
            return Some(false);
        }
        let replacement = unsafe { (p.resolve_create)(o.device(), render[0], render[1]) };
        if replacement.is_null() {
            return None;
        }
        if !o.dense.is_null() {
            unsafe { (p.resolve_destroy)(o.dense) };
        }
        o.dense = replacement;
        o.dense_extent = render;
        Some(true)
    }

    unsafe fn dump_inputs(&self, p: &P, o: &Owned, context: Raw, frame: &Frame) {
        let decoded = unsafe { (p.decode_texture)(o.decode) };
        let motion_view = if o.motion.zero_means_unwritten != 0 {
            VIEW_VELOCITY
        } else {
            VIEW_DENSE_MOTION
        };
        let inputs: [(&CStr, &CStr, Raw, u32, f32); 11] = [
            (
                c"sr_source_jittered",
                c"Jittered pre-tonemap linear HDR source at the active render rectangle",
                frame.scene_color,
                VIEW_HDR,
                1.0,
            ),
            (
                c"sr_game_motion",
                c"Native motion in adapter-declared storage and units",
                frame.game_motion,
                motion_view,
                8.0,
            ),
            (
                c"sr_motion_decoded",
                c"Decoded sparse game motion before dense camera reconstruction",
                decoded,
                VIEW_DECODED_MOTION,
                8.0,
            ),
            (
                c"sr_scene_depth",
                c"Original scene device depth before cloud/motion depth preparation",
                frame.depth,
                VIEW_DEPTH,
                1.0,
            ),
            (
                c"sr_engine_exposure",
                c"Engine exposure guide before colour transport/backend policy",
                frame.exposure,
                VIEW_SCALAR,
                1.0,
            ),
            (
                c"sr_opaque_color",
                c"Opaque-only colour before translucency; auxiliary, not a DLSS SR input",
                frame.color_before_transparency,
                VIEW_HDR,
                1.0,
            ),
            (
                c"sr_transparency_layer",
                c"Separate translucency colour; auxiliary, not a DLSS SR input",
                frame.transparency_layer,
                VIEW_HDR,
                1.0,
            ),
            (
                c"sr_reactive_mask",
                c"FSR/XeSS reactive-mask input",
                frame.reactive_mask,
                VIEW_SCALAR,
                1.0,
            ),
            (
                c"sr_transparency_mask",
                c"Translucency coverage hint; consumption depends on backend",
                frame.transparency_mask,
                VIEW_SCALAR,
                1.0,
            ),
            (
                c"sr_bias_mask",
                c"Bias-current-colour hint; DLSS use is preset dependent",
                frame.bias_mask,
                VIEW_SCALAR,
                1.0,
            ),
            (
                c"sr_cloud_depth",
                c"Cloud/translucent depth used by the dense-motion resolve",
                frame.motion_depth_layer,
                VIEW_DEPTH,
                1.0,
            ),
        ];
        for (name, description, texture, view, scale) in inputs {
            unsafe {
                (p.input_dump)(
                    context,
                    name.as_ptr(),
                    description.as_ptr(),
                    texture,
                    view,
                    scale,
                )
            };
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn sharpen(
        &self,
        p: &P,
        o: &mut Owned,
        context: Raw,
        frame: &Frame,
        dlss: &DlssFrame,
        options: &Sharpening,
        backend: u32,
    ) {
        let (mut depth, mut motion) = (dlss.depth, dlss.motion);
        if backend != SR_DLSS {
            unsafe { rsf_sr_legacy_fg_inputs(o.alternate, &mut depth, &mut motion) };
        }
        let mapping = rsf_dlssp_sharpen_frame {
            depth_inverted: dlss.depth_inverted,
            motion_x: if backend == SR_DLSS {
                frame.render_width as f32 * dlss.motion_scale_x
            } else {
                1.0
            },
            motion_y: if backend == SR_DLSS {
                frame.render_height as f32 * dlss.motion_scale_y
            } else {
                1.0
            },
            near_plane: dlss.near_plane,
            far_plane: dlss.far_plane,
        };
        let sharpened = unsafe {
            (p.sharpen)(
                &mut o.sharpen,
                context,
                o.output(),
                motion,
                depth,
                frame.exposure,
                options,
                &mapping,
            )
        };
        if sharpened == 0 && !o.sharpen_reported {
            o.sharpen_reported = true;
            self.say("Sharpening pass unavailable; retaining the unsharpened SR output");
        }
    }

    // Both sides of the comparison, from the same frame: the inputs the backend was actually handed
    // and the exact assembled constants, never a later view-buffer read. A failed dump is
    // diagnostic only and does not change the frame's outcome.
    #[allow(clippy::too_many_arguments)]
    unsafe fn write_dump(
        &self,
        p: &P,
        o: &Owned,
        context: Raw,
        frame: &Frame,
        dlss: &DlssFrame,
        backend: u32,
        prefix: &[u8],
        output_extent: [u32; 2],
    ) {
        let decoded = if o.decode.is_null() {
            null_mut()
        } else {
            unsafe { (p.decode_texture)(o.decode) }
        };
        let targets: [(&str, &[u8], Raw, u32); 7] = [
            (
                "the scene colour it was given",
                b"_input",
                frame.scene_color,
                VIEW_RAW,
            ),
            ("the upscaled result", b"_output", o.output(), VIEW_RAW),
            (
                "the game velocity it was given",
                b"_motion",
                frame.game_motion,
                VIEW_VELOCITY,
            ),
            (
                "the decoded motion it submitted",
                b"_motion_decoded",
                decoded,
                VIEW_DECODED_MOTION,
            ),
            (
                "the complete motion it submitted",
                b"_motion_submitted",
                dlss.motion,
                VIEW_DECODED_MOTION,
            ),
            ("the depth it submitted", b"_depth", frame.depth, VIEW_RAW),
            (
                "the engine exposure guide",
                b"_exposure",
                frame.exposure,
                VIEW_RAW,
            ),
        ];
        for (what, suffix, texture, view) in targets {
            if texture.is_null() {
                continue;
            }
            let path = [prefix, suffix].concat();
            self.say([format!("writing {what} to ").as_bytes(), &path].concat());
            let path = CString::new(path).unwrap_or_default();
            let raw = unsafe {
                (p.dump_bytes)(
                    o.device(),
                    context,
                    texture,
                    path.as_ptr(),
                    view,
                    1.0,
                    Some(forward_dump),
                )
            };
            if raw != DUMP_OK {
                self.say(format!("raw readback of {what} failed (result {raw})"));
            }
            let written = if texture == frame.depth {
                raw
            } else {
                unsafe {
                    (p.dump_image)(
                        o.device(),
                        context,
                        texture,
                        path.as_ptr(),
                        view,
                        1.0,
                        Some(forward_dump),
                    )
                }
            };
            if written != DUMP_OK {
                self.say(format!("writing {what} failed (result {written})"));
            }
        }
        let record = frame_record(frame, dlss, backend, output_extent);
        let path = String::from_utf8_lossy(&[prefix, b"_frame.json"].concat()).into_owned();
        let _ = std::fs::write(path, record);
    }

    pub(super) fn output_texture(&self) -> Raw {
        self.output.load(Ordering::Acquire)
    }

    pub(super) fn layer_texture(&self) -> Raw {
        self.layer_output.load(Ordering::Acquire)
    }

    // Safe from any thread. Nothing is logged: the sink belongs to the frame-driving thread.
    pub(super) unsafe fn request_dump(&self, prefix: *const c_char) -> i32 {
        if prefix.is_null() || unsafe { *prefix } == 0 {
            return INVALID;
        }
        let prefix = unsafe { CStr::from_ptr(prefix) }.to_bytes().to_vec();
        let mut shared = self.shared();
        if !shared.running {
            return NOT_RUNNING;
        }
        shared.dump = Some(prefix);
        OK
    }

    pub(super) unsafe fn status(&self, status: *mut Status) -> i32 {
        if !unsafe { has_capacity(status) } {
            return INVALID;
        }
        let shared = self.shared();
        let status = unsafe { &mut *status };
        status.running = u32::from(shared.running);
        status.dlss_supported = u32::from(shared.supported);
        status.render_width = shared.range.width;
        status.render_height = shared.range.height;
        [status.output_width, status.output_height] = shared.output;
        status.frames_evaluated = shared.scene.evaluated;
        status.frames_refused = shared.scene.refused;
        status.last_result = shared.scene.last_result;
        status.layer_frames_evaluated = shared.layer.evaluated;
        status.layer_frames_refused = shared.layer.refused;
        [status.layer_width, status.layer_height] = shared.layer_extent;
        status.layer_last_result = shared.layer.last_result;
        status.backend = shared.backend;
        status.requested_backend = shared.requested_backend;
        status.last_switch_result = shared.last_switch_result;
        OK
    }

    pub(super) unsafe fn prepare_layer(
        &self,
        p: &P,
        width: u32,
        height: u32,
        out: *mut Raw,
    ) -> i32 {
        if out.is_null() || width == 0 || height == 0 {
            return INVALID;
        }
        unsafe { out.write(null_mut()) };
        let Some(mut guard) = self.owned() else {
            return NOT_RUNNING;
        };
        let o = &mut *guard;
        if o.device.is_none() || !o.streamline_loaded {
            return NOT_RUNNING;
        }
        if let Some(layer) = o
            .layer_output
            .as_ref()
            .filter(|_| o.layer_extent == [width, height])
        {
            unsafe { out.write(layer.as_ptr()) };
            return OK;
        }
        if o.layer_output.is_some() {
            self.say(format!(
                "layer size moved from {}x{} to {width}x{height}, rebuilding the layer feature",
                o.layer_extent[0], o.layer_extent[1]
            ));
            unsafe { (p.release_viewport)(LAYER_VIEWPORT) };
        }
        self.release_layer(o);

        self.say(format!(
            "creating the {width}x{height} layer output and its zero motion"
        ));
        let Some(output) = (unsafe { self.create_cleared_target(p, o.device(), width, height) })
        else {
            self.say("the layer output could not be created");
            self.release_layer(o);
            return RESOURCE_FAILED;
        };
        // Zero motion for the whole layer. Its materials carry no velocity of their own, and the
        // camera's share is derived from the depth by Streamline as for the scene's static geometry.
        let desc = texture_desc(
            width,
            height,
            RSF_D3D11_FORMAT_RG16_FLOAT,
            RSF_D3D11_BIND_SHADER_RESOURCE,
        );
        let Some(motion) =
            (unsafe { ComRef::from_owned((p.create_zeroed)(o.device(), &desc, width * 4)) })
        else {
            drop(output);
            self.say("the layer's zero motion could not be created");
            self.release_layer(o);
            return RESOURCE_FAILED;
        };
        let raw = output.as_ptr();
        o.layer_output = Some(output);
        o.layer_motion = Some(motion);
        o.layer_extent = [width, height];
        self.layer_output.store(raw, Ordering::Release);
        self.shared().layer_extent = [width, height];
        o.layer_rebuilt = true;
        o.layer_failures = 0;
        o.layer_given_up = false;
        unsafe { out.write(raw) };
        OK
    }

    pub(super) unsafe fn on_layer(&self, p: &P, context: Raw, layer: *const Layer) -> i32 {
        if context.is_null() || !unsafe { has_capacity(layer) } {
            return INVALID;
        }
        let layer = unsafe { &*layer };
        if layer.abi_version != ABI {
            return ABI_MISMATCH;
        }
        let Some(mut guard) = self.owned() else {
            return NOT_RUNNING;
        };
        let o = &mut *guard;
        let backend = self.shared().backend;
        let (Some(output), Some(motion)) = (o.layer_output.as_ref(), o.layer_motion.as_ref())
        else {
            return NOT_RUNNING;
        };
        if backend != SR_DLSS || o.device.is_none() || !o.streamline_loaded {
            return NOT_RUNNING;
        }
        let (output, motion) = (output.as_ptr(), motion.as_ptr());
        if o.layer_given_up {
            return self.count(true, EVALUATE_FAILED);
        }
        if layer.color.is_null()
            || layer.depth.is_null()
            || layer.camera.is_null()
            || [layer.width, layer.height] != o.layer_extent
            || !unsafe { has_capacity(layer.camera) }
        {
            return self.count(true, INVALID);
        }
        if unsafe { (*layer.camera).abi_version } != RSF_FRAME_ASSEMBLY_ABI_VERSION {
            return ABI_MISMATCH;
        }

        let mut camera: Camera = unsafe { *layer.camera };
        camera.struct_size = size_of::<Camera>() as u32;
        // Zeros are already the decoded convention: no sentinel, nothing to scale.
        camera.motion_decoded = 1;
        camera.has_motion_sentinel = 0;
        camera.motion_scale = [1.0; 2];
        if o.layer_rebuilt {
            camera.reset = 1;
            o.layer_rebuilt = false;
        }
        let resources = rsf_frame_resources {
            struct_size: size_of::<rsf_frame_resources>() as u32,
            color_in: layer.color,
            color_out: output,
            depth: layer.depth,
            motion,
            exposure: null_mut(),
            render_width: layer.width,
            render_height: layer.height,
            output_width: layer.width,
            output_height: layer.height,
            quality: QUALITY_NATIVE,
        };
        let mut dlss: DlssFrame = unsafe { std::mem::zeroed() };
        dlss.struct_size = size_of::<DlssFrame>() as u32;
        let assembled = unsafe { rsf_assemble_dlss_frame(&camera, &resources, &mut dlss) };
        dlss.preset = self.shared().preset;
        if assembled != 0 {
            if o.layer_reported.worth_saying(FRAME_REFUSED, [0; 2]) {
                self.say(format!(
                    "the layer frame was refused before DLSS saw it (result {assembled})"
                ));
            }
            return self.count(true, FRAME_REFUSED);
        }
        dlss.viewport = LAYER_VIEWPORT;
        dlss.alpha = 1;
        {
            let shared = self.shared();
            dlss.frame_index = (shared.scene.evaluated + shared.scene.refused) as u32;
        }

        let evaluated = unsafe { (p.evaluate_layer)(context, &dlss) };
        if evaluated != 0 {
            o.layer_failures += 1;
            if o.layer_failures >= FAILURE_LIMIT {
                o.layer_given_up = true;
                self.say(format!(
                    "DLSS failed to integrate the layer {} frames in a row (last result {evaluated}), so it will not be asked again until the layer is prepared anew",
                    o.layer_failures
                ));
            } else if o.layer_reported.worth_saying(EVALUATE_FAILED, [0; 2]) {
                self.say(format!(
                    "DLSS did not integrate the layer this frame (result {evaluated})"
                ));
            }
            return self.count(true, EVALUATE_FAILED);
        }
        if self.shared().layer.evaluated == 0 {
            self.say(format!(
                "DLSS integrated the layer at {}x{}, one to one, alpha carried",
                layer.width, layer.height
            ));
        }
        o.layer_failures = 0;
        o.layer_reported = Reported::default();
        self.count(true, OK)
    }

    pub(super) fn set_colour_correction(&self, enabled: u32) {
        self.colour_correction
            .store(enabled != 0, Ordering::Release);
    }

    pub(super) fn set_colour_transport(&self, enabled: u32) {
        self.colour_transport.store(enabled != 0, Ordering::Release);
    }

    pub(super) unsafe fn set_sharpening(&self, p: &P, options: *const Sharpening) -> i32 {
        if !unsafe { has_capacity(options) } {
            return INVALID;
        }
        let options = unsafe { *options };
        if options.abi_version != RSF_SHARPENING_ABI_VERSION {
            return ABI_MISMATCH;
        }
        if unsafe { (p.sharpening_valid)(&options) } == 0 {
            return INVALID;
        }
        self.shared().sharpening = Some(options);
        OK
    }

    pub(super) unsafe fn set_preset(&self, p: &P, preset: u32) -> i32 {
        if unsafe { (p.preset_valid)(preset) } == 0 {
            return INVALID;
        }
        let changed = {
            let mut shared = self.shared();
            let changed = shared.preset != preset;
            if changed {
                shared.preset = preset;
                shared.restart_history();
            }
            changed
        };
        if changed {
            self.say(format!(
                "DLSS model preset requested: {preset} (driver overrides may apply)"
            ));
        }
        OK
    }

    pub(super) unsafe fn set_quality(
        &self,
        p: &P,
        quality: u32,
        width_out: *mut u32,
        height_out: *mut u32,
    ) -> i32 {
        let write = |width: u32, height: u32| unsafe {
            if !width_out.is_null() {
                width_out.write(width);
            }
            if !height_out.is_null() {
                height_out.write(height);
            }
        };
        let backend = self.shared().backend;
        if backend != SR_DLSS {
            // Only this path touches the frame-thread adapter.
            let Some(guard) = self.owned() else {
                return NOT_RUNNING;
            };
            let (mut width, mut height) = (0, 0);
            if unsafe {
                rsf_sr_legacy_select(guard.alternate, backend, quality, &mut width, &mut height)
            } != 0
            {
                return NOT_SUPPORTED;
            }
            let mut shared = self.shared();
            shared.quality = quality;
            shared.range.width = width;
            shared.range.height = height;
            shared.reset_pending = true;
            drop(shared);
            write(width, height);
            return OK;
        }
        let output = {
            let shared = self.shared();
            if !shared.running {
                return NOT_RUNNING;
            }
            shared.output
        };
        let mut plan = rsf_dlssp_range::default();
        let planned = unsafe { (p.plan)(output[0], output[1], quality, &mut plan) };
        if planned != 0 || plan.width == 0 || plan.height == 0 {
            self.say(format!(
                "quality {quality} refused: DLSS gave no render size (result {planned})"
            ));
            return STREAMLINE_FAILED;
        }
        {
            let mut shared = self.shared();
            shared.quality = quality;
            shared.restart_history();
            shared.range = Range::of(plan);
        }
        self.say(format!(
            "quality {quality}: DLSS renders {}x{} for {}x{}",
            plan.width, plan.height, output[0], output[1]
        ));
        write(plan.width, plan.height);
        OK
    }

    // Change the output extent on the graphics execution owner. The replacement texture and the
    // active backend are prepared before anything is committed; a refusal keeps the current set.
    pub(super) unsafe fn resize_output(
        &self,
        p: &P,
        width: u32,
        height: u32,
        width_out: *mut u32,
        height_out: *mut u32,
    ) -> i32 {
        if width == 0 || height == 0 || width > MAX_EXTENT || height > MAX_EXTENT {
            return INVALID;
        }
        let Some(mut guard) = self.owned() else {
            return NOT_RUNNING;
        };
        let o = &mut *guard;
        if o.device.is_none() || o.output.is_none() {
            return NOT_RUNNING;
        }
        let write = |range: &Range| unsafe {
            if !width_out.is_null() {
                width_out.write(range.width);
            }
            if !height_out.is_null() {
                height_out.write(range.height);
            }
        };
        let (output, current, backend, quality) = {
            let shared = self.shared();
            (shared.output, shared.range, shared.backend, shared.quality)
        };
        if output == [width, height] {
            write(&current);
            return OK;
        }
        let mut range = Range::alternate([width, height]);
        let mut next_alternate: *mut Adapter = null_mut();
        if backend == SR_DLSS {
            let mut plan = rsf_dlssp_range::default();
            let result = unsafe { (p.plan)(width, height, quality, &mut plan) };
            if result != 0 || plan.width == 0 || plan.height == 0 {
                return STREAMLINE_FAILED;
            }
            range = Range::of(plan);
        } else {
            let (mut result, created) = unsafe { self.create_alternate(p, o, width, height) };
            next_alternate = created.cast();
            if result == 0 {
                result = unsafe {
                    rsf_sr_legacy_select(
                        next_alternate,
                        backend,
                        quality,
                        &mut range.width,
                        &mut range.height,
                    )
                };
            }
            if result != 0 || range.width == 0 || range.height == 0 {
                unsafe { rsf_sr_legacy_destroy(next_alternate) };
                return NOT_SUPPORTED;
            }
        }
        let mut desc = TextureDesc::default();
        unsafe { (p.texture_desc)(o.output(), &mut desc) };
        desc.width = width;
        desc.height = height;
        let Some(replacement) =
            (unsafe { ComRef::from_owned((p.create_texture)(o.device(), &desc)) })
        else {
            unsafe { rsf_sr_legacy_destroy(next_alternate) };
            return RESOURCE_FAILED;
        };
        if backend == SR_DLSS && o.dlss_evaluation_attempted {
            if unsafe { (p.release_resources)() } != 0 {
                return STREAMLINE_FAILED;
            }
            o.dlss_evaluation_attempted = false;
        }
        unsafe { (p.clear_black)(o.device(), replacement.as_ptr()) };
        let previous_output = self.set_output(o, Some(replacement));
        let previous_alternate = std::mem::replace(&mut o.alternate, next_alternate);
        if !o.decode.is_null() {
            unsafe { (p.decode_destroy)(o.decode) };
        }
        o.decode = null_mut();
        o.decode_extent = [0; 2];
        o.decode_failed = [0; 2];
        {
            let mut shared = self.shared();
            shared.restart_history();
            shared.output = [width, height];
            shared.range = range;
        }
        drop(previous_output);
        unsafe { rsf_sr_legacy_destroy(previous_alternate) };
        write(&range);
        self.say(format!(
            "output resized to {width}x{height}; backend {backend} plans {}x{}",
            range.width, range.height
        ));
        OK
    }

    // Compatibility selector for the existing frame pipeline, run at the render-thread command
    // boundary. Refusals preserve the active backend.
    pub(super) unsafe fn select_backend(
        &self,
        p: &P,
        backend: u32,
        width_out: *mut u32,
        height_out: *mut u32,
    ) -> i32 {
        let Some(mut guard) = self.owned() else {
            return NOT_RUNNING;
        };
        let o = &mut *guard;
        if o.device.is_none() || o.output.is_none() {
            return NOT_RUNNING;
        }
        if !(SR_DLSS..=SR_XESS).contains(&backend) {
            return INVALID;
        }
        let (output, quality) = {
            let mut shared = self.shared();
            shared.requested_backend = backend;
            (shared.output, shared.quality)
        };
        let mut range = Range::alternate(output);
        let mut result;
        if backend == SR_DLSS {
            let mut plan = rsf_dlssp_range::default();
            result = unsafe { (p.plan)(output[0], output[1], quality, &mut plan) };
            range = Range::reported(plan);
            if result == 0 && !o.alternate.is_null() {
                result = unsafe {
                    rsf_sr_legacy_select(o.alternate, SR_NONE, quality, null_mut(), null_mut())
                };
            }
            // The DLSS feature is not released: it was idle, and the next frame starts it without
            // history. Releasing it rebuilt the feature inside the first frame back, about 50 ms.
        } else {
            result = 0;
            if o.alternate.is_null() {
                let (created, adapter) =
                    unsafe { self.create_alternate(p, o, output[0], output[1]) };
                result = created;
                o.alternate = adapter.cast();
            }
            if result == 0 {
                // A vendor builds its pipelines at its first dispatch (FSR4: about 25 s on a cold
                // driver cache). Done here, at the boundary, with a line before and after.
                self.say(format!(
                    "preparing backend {backend}: the vendor builds its pipelines on first use; the game pauses until it is done"
                ));
                let started = Instant::now();
                result = unsafe {
                    rsf_sr_legacy_select(
                        o.alternate,
                        backend,
                        quality,
                        &mut range.width,
                        &mut range.height,
                    )
                };
                if result == 0
                    && unsafe {
                        rsf_sr_legacy_warm_up(
                            o.alternate,
                            range.width,
                            range.height,
                            output[0],
                            output[1],
                        )
                    } != 0
                {
                    self.say(format!(
                        "backend {backend} warm-up dispatch failed; its first frame may build the pipelines instead"
                    ));
                }
                self.say(format!(
                    "backend {backend} prepared in {:.0} ms",
                    started.elapsed().as_secs_f64() * 1000.0
                ));
            }
        }
        let active = {
            let mut shared = self.shared();
            shared.last_switch_result = result;
            if result == 0 {
                shared.backend = backend;
                shared.range = range;
            }
            shared.backend
        };
        if result != 0 {
            self.say(format!(
                "backend {backend} refused ({result}); keeping {active}"
            ));
            return NOT_SUPPORTED;
        }
        self.shared().restart_history();
        unsafe {
            if !width_out.is_null() {
                width_out.write(range.width);
            }
            if !height_out.is_null() {
                height_out.write(range.height);
            }
        }
        self.say(format!(
            "selected backend {backend}: {}x{}",
            range.width, range.height
        ));
        OK
    }

    pub(super) unsafe fn stop(&self, p: &P) -> i32 {
        let Some(mut guard) = self.owned() else {
            return NOT_RUNNING;
        };
        let o = &mut *guard;
        if !o.sharpen.is_null() {
            unsafe { (p.sharpen_destroy)(o.sharpen) };
        }
        o.sharpen = null_mut();
        o.sharpen_reported = false;
        if o.device.is_none() && !o.streamline_loaded && o.output.is_none() {
            return NOT_RUNNING;
        }
        self.say("stopping: releasing pipeline resources");
        unsafe { self.tear_down(p, o) };
        OK
    }
}

unsafe fn defaults(p: &P) -> Sharpening {
    let mut options: Sharpening = unsafe { std::mem::zeroed() };
    unsafe { (p.sharpening_defaults)(&mut options) };
    options
}

fn frame_record(frame: &Frame, dlss: &DlssFrame, backend: u32, output: [u32; 2]) -> String {
    let f = |value: f32| g9(f64::from(value));
    let clip = dlss
        .clip_to_prev_clip
        .iter()
        .map(|&value| f(value))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"render\":[{},{}],\"output\":[{},{}],\"backend\":{backend},\
\"jitter\":[{},{}],\"motion_scale\":[{},{}],\
\"camera_motion_included\":{},\"reset\":{},\"clip_to_previous\":[{clip}]}}\n",
        frame.render_width,
        frame.render_height,
        output[0],
        output[1],
        f(dlss.jitter_x),
        f(dlss.jitter_y),
        f(dlss.motion_scale_x),
        f(dlss.motion_scale_y),
        dlss.camera_motion_included,
        dlss.reset,
    )
}

// C entry points. dlss_pipeline.cpp keeps every public export as a one-line forward that passes
// its static leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_start(p: &P, device: Raw, setup: *const Setup) -> i32 {
    unsafe { PROCESS.start(p, device, setup) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_on_frame(
    p: &P,
    context: Raw,
    frame: *const Frame,
) -> i32 {
    unsafe { PROCESS.on_frame(p, context, frame) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_pipeline_output_texture() -> Raw {
    PROCESS.output_texture()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_request_dump(prefix: *const c_char) -> i32 {
    unsafe { PROCESS.request_dump(prefix) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_get_status(status: *mut Status) -> i32 {
    unsafe { PROCESS.status(status) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_prepare_layer(
    p: &P,
    width: u32,
    height: u32,
    output: *mut Raw,
) -> i32 {
    unsafe { PROCESS.prepare_layer(p, width, height, output) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_on_layer(
    p: &P,
    context: Raw,
    layer: *const Layer,
) -> i32 {
    unsafe { PROCESS.on_layer(p, context, layer) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_pipeline_layer_output() -> Raw {
    PROCESS.layer_texture()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_pipeline_set_colour_correction(enabled: u32) {
    PROCESS.set_colour_correction(enabled);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_pipeline_set_colour_transport(enabled: u32) {
    PROCESS.set_colour_transport(enabled);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_set_sharpening(
    p: &P,
    options: *const Sharpening,
) -> i32 {
    unsafe { PROCESS.set_sharpening(p, options) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_set_preset(p: &P, preset: u32) -> i32 {
    unsafe { PROCESS.set_preset(p, preset) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_set_quality(
    p: &P,
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.set_quality(p, quality, width, height) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_resize_output(
    p: &P,
    output_width: u32,
    output_height: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.resize_output(p, output_width, output_height, width, height) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_select_backend(
    p: &P,
    backend: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.select_backend(p, backend, width, height) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_pipeline_stop(p: &P) -> i32 {
    unsafe { PROCESS.stop(p) }
}

#[cfg(test)]
#[path = "dlss_pipeline_tests.rs"]
mod tests;
