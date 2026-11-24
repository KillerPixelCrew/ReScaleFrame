// SPDX-License-Identifier: MIT
// Generic D3D11 callback host: session admission, callback leases, presentation
// installation and binding, SR lifecycle, reduced-frame recovery and status.
// Every D3D11, DXGI, SDK, overlay and capture call goes through `native`
// (d3d11_host_platform.rs; fakes under test). Native calls cross individually
// guarded boundaries.
use crate::com_ref;
use crate::d3d11_host_abi::*;
#[cfg(not(test))]
use crate::d3d11_host_platform as native;
use crate::d3d11_host_reduced::{MAX_RESERVED, Products, Raw, Ticket, zeroed_pass};
use crate::native_cpu::{SourcePacket, decode_source_event};
use crate::sync::lock;
use crate::sys::RSF_SR_DLSS;
use crate::sys::rsf_d3d11_present_setup;
use crate::sys::{
    RSF_FG_OFF, RSF_RENDER_FRONTEND_ABI_VERSION, rsf_native_fg_options, rsf_render_settings,
};
use crate::sys::{
    RSF_OVERLAY_API_VERSION, RSF_OVERLAY_SERVICE_OPTION_TOGGLE, RSF_OVERLAY_SR_INPUT_COLOR,
    RSF_OVERLAY_SR_INPUT_DEPTH, RSF_OVERLAY_SR_INPUT_JITTER, RSF_OVERLAY_SR_INPUT_MOTION,
    rsf_overlay_service_option, rsf_overlay_service_setup, rsf_overlay_sr_report,
};
use crate::sys::{rsf_native_present_record, rsf_observer_present_event};
use crate::wide::copy_nul as wide_copy;
use rsf_game_sys::ffi_util::{checked, has_capacity};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_FG, RSF_GAME_ABI_VERSION,
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_EVENT_VERSION_SOURCE,
    RSF_GAME_CPU_SOURCE_ABORT, RSF_GAME_CPU_SOURCE_RESERVE, RSF_GAME_FRAME_ABI_VERSION,
    RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_FRAME,
    RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR,
    RSF_GAME_RENDER_SUBMISSION, RSF_GAME_RENDER_WINDOW, rsf_camera_frame, rsf_frame_record,
    rsf_game_cpu_event, rsf_game_host_services, rsf_game_render_config, rsf_game_render_feedback,
};
use std::ffi::{CStr, CString, c_char, c_int};
use std::mem::offset_of;
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};
#[cfg(test)]
use tests::native;
use windows::core::{IUnknown, Interface};

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;
const NATIVE_REFUSED: i32 = -5;
const DLSS_OK: i32 = 0;
const SR_BACKEND_CHOICES: u32 = 0x3e;
const FG_BACKEND_DLSS: u32 = 1;
const FG_BACKEND_FSR3: u32 = 3;
const FG_BACKEND_FSR4: u32 = 4;
const FG_BACKEND_XESS: u32 = 5;
const FG_BACKEND_AUTO: u32 = 7;
const REFLEX_OFF: u32 = 0;
const REFLEX_ON: u32 = 1;
const REFLEX_BOOST: u32 = 2;
const MAX_GENERATED_FRAMES: u32 = 15;
const UI_MODE_BACKBUFFER_HUDLESS: u32 = 4;
const SCREEN_GAMEPLAY: u32 = 8;
const DXGI_PRESENT_TEST: u32 = 1;
const SCENE_PRESENT_NONE: u32 = 0;
const AUX_DEPTH_STENCIL: u32 = 0;
const AUX_COLOUR: u32 = 1;
const AUX_RESTORE_CONTENTS: u32 = 1;

use crate::sampler_bias::{rsf_sampler_bias_apply, rsf_sampler_bias_create};

fn current_thread() -> u32 {
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}

fn set_reason(report: &mut rsf_overlay_sr_report, text: &str) {
    let bytes = &text.as_bytes()[..text.len().min(report.reason.len() - 1)];
    for (target, &byte) in report.reason.iter_mut().zip(bytes) {
        *target = byte as c_char;
    }
    report.reason[bytes.len()] = 0;
}

fn fresh_report() -> rsf_overlay_sr_report {
    rsf_overlay_sr_report {
        struct_size: size_of::<rsf_overlay_sr_report>() as u32,
        ..Default::default()
    }
}

fn zeroed_record() -> rsf_frame_record {
    // SAFETY: the frame record contains only integers and floats.
    unsafe { std::mem::zeroed() }
}

fn zeroed_settings() -> rsf_render_settings {
    // SAFETY: the render settings contain only integers and floats.
    unsafe { std::mem::zeroed() }
}

fn rect_extent([left, top, right, bottom]: [i32; 4]) -> Option<[u32; 2]> {
    (left >= 0 && top >= 0 && right > left && bottom > top)
        .then(|| [(right - left) as u32, (bottom - top) as u32])
}

fn ptr_of(object: &Option<IUnknown>) -> Raw {
    object.as_ref().map_or(null_mut(), Interface::as_raw)
}

struct Text {
    streamline: CString,
    fidelityfx: CString,
    xess: CString,
    captures: CString,
    identity: CString,
    fingerprint: CString,
    version: CString,
    project: CString,
}

struct Shared {
    desired: rsf_render_settings,
    planned: rsf_game_render_config,
    report: rsf_overlay_sr_report,
}

#[derive(Default)]
struct CaptureBudget {
    failures: u8,
    sequence_remaining: u8,
    last_source: Option<(u64, u64)>,
}

impl CaptureBudget {
    fn claim_failure(&mut self, enabled: bool, pending: bool, result: i32) -> bool {
        if !enabled || pending || result == OK || self.failures >= 3 {
            return false;
        }
        self.failures += 1;
        true
    }

    fn request_sequence(&mut self) {
        self.sequence_remaining = 4;
        self.last_source = None;
    }

    fn claim_source(&mut self, session: u64, source: u64, ready: bool, pending: bool) -> bool {
        let identity = Some((session, source));
        if self.sequence_remaining == 0
            || !ready
            || pending
            || session == 0
            || source == 0
            || self.last_source == identity
        {
            return false;
        }
        self.last_source = identity;
        self.sequence_remaining -= 1;
        true
    }
}

/// State owned by the admitted graphics callback. Every field is reached only
/// under the callback lease; a contended lease refuses rather than waits.
struct Graphics {
    auxiliaries: Option<Box<dyn Products>>,
    reduced: Option<Ticket>,
    next_reduced_token: u64,
    sampler_bias: Raw,
    chain: Option<IUnknown>,
    creation_outer: Option<IUnknown>,
    device: Option<IUnknown>,
    capture_record: rsf_frame_record,
    capture_view: u64,
    capture_native_frame: u64,
    capture_pending: bool,
    capture_budget: CaptureBudget,
    execution_thread: u32,
    backend: u32,
    quality: u32,
    preset: u32,
    output_width: u32,
    output_height: u32,
    configured: bool,
    scene_bound: bool,
    observer: bool,
    attempted: bool,
    running: bool,
    reset: bool,
    applied_generation: u32,
    evaluated: u64,
    refused: u64,
    logged: u64,
    last_result: i32,
    present_result: i32,
    dlss_started: bool,
    present_pairs: u64,
    ownership_reports: u32,
    capture_refusal_terms: u32,
}

impl Graphics {
    fn new() -> Self {
        Self {
            auxiliaries: None,
            reduced: None,
            next_reduced_token: 1,
            sampler_bias: null_mut(),
            chain: None,
            creation_outer: None,
            device: None,
            capture_record: zeroed_record(),
            capture_view: 0,
            capture_native_frame: 0,
            capture_pending: false,
            capture_budget: CaptureBudget::default(),
            execution_thread: 0,
            backend: 0,
            quality: 0,
            preset: 0,
            output_width: 0,
            output_height: 0,
            configured: false,
            scene_bound: false,
            observer: false,
            attempted: false,
            running: false,
            reset: true,
            applied_generation: 0,
            evaluated: 0,
            refused: 0,
            logged: 0,
            last_result: NOT_READY,
            present_result: DLSS_OK,
            dlss_started: false,
            present_pairs: 0,
            ownership_reports: 0,
            capture_refusal_terms: 0,
        }
    }

    fn chain_ptr(&self) -> Raw {
        ptr_of(&self.chain)
    }

    fn device_ptr(&self) -> Raw {
        ptr_of(&self.device)
    }

    fn auxiliaries_pending(&self) -> bool {
        self.auxiliaries
            .as_ref()
            .is_some_and(|products| products.pending())
    }

    fn reduced_pending(&self) -> bool {
        self.reduced.as_ref().is_some_and(|ticket| ticket.pending)
    }
}

// SAFETY: COM references and the sampler cache are used only by the graphics
// owner that holds the callback lease.
unsafe impl Send for Graphics {}

pub(crate) struct Host {
    upstream: rsf_game_host_services,
    setup: rsf_d3d11_host_setup,
    ini: Vec<u16>,
    section: Vec<u16>,
    text: Text,
    bridge_installed: AtomicBool,
    bridge_bound: AtomicBool,
    active: AtomicBool,
    transport: AtomicBool,
    fidelity: AtomicBool,
    capture: AtomicBool,
    capture_failures: AtomicBool,
    capture_sequence: AtomicBool,
    requested_generation: AtomicU32,
    cpu_reports: AtomicU32,
    cpu_sample_reports: AtomicU32,
    cpu_refusal_stages: AtomicU32,
    cpu_stage_reports: [AtomicU32; RSF_GAME_CPU_SOURCE_ABORT as usize + 1],
    shared: Mutex<Shared>,
    graphics: Mutex<Graphics>,
}

// SAFETY: the copied setup and upstream services hold process-lived adapter
// callbacks and user pointers; mutable state is behind atomics and mutexes.
unsafe impl Send for Host {}
unsafe impl Sync for Host {}

static CREATION: Mutex<()> = Mutex::new(());
static OWNER: AtomicPtr<Host> = AtomicPtr::new(null_mut());

fn owner(user: Raw) -> Option<&'static Host> {
    let host = OWNER.load(Ordering::Acquire);
    (!host.is_null() && host.cast() == user).then(|| unsafe { &*host })
}

#[repr(u32)]
enum Advanced {
    CaptureNext,
    Transport,
    Correction,
    CaptureFailures,
    CaptureSequence,
}

#[derive(Clone, Copy)]
struct AdvancedOption {
    label: &'static CStr,
    flag: fn(&Host) -> &AtomicBool,
    key: Option<&'static CStr>,
    bump_generation: bool,
}

impl AdvancedOption {
    fn value(self, host: &Host) -> i32 {
        (self.key.is_some() && (self.flag)(host).load(Ordering::SeqCst)) as i32
    }
}

const ADVANCED: [AdvancedOption; 5] = {
    let next = AdvancedOption {
        label: c"Capture next SR inputs",
        flag: |h| &h.capture,
        key: None,
        bump_generation: false,
    };
    let mut options = [next; 5];
    options[Advanced::CaptureNext as usize] = next;
    options[Advanced::Transport as usize] = AdvancedOption {
        label: c"DLSS colour transport",
        flag: |h| &h.transport,
        key: Some(c"ColourTransport"),
        bump_generation: true,
    };
    options[Advanced::Correction as usize] = AdvancedOption {
        label: c"SR colour correction",
        flag: |h| &h.fidelity,
        key: Some(c"ColourCorrection"),
        bump_generation: true,
    };
    options[Advanced::CaptureFailures as usize] = AdvancedOption {
        label: c"Capture SR failures (max 3 per session)",
        flag: |h| &h.capture_failures,
        key: Some(c"CaptureSRFailures"),
        bump_generation: false,
    };
    options[Advanced::CaptureSequence as usize] = AdvancedOption {
        label: c"Capture 4 ready SR frames",
        flag: |h| &h.capture_sequence,
        key: None,
        bump_generation: false,
    };
    options
};

fn sr_inputs(pass: &Pass) -> (u32, [f32; 2]) {
    let mut bits = 0;
    for (texture, bit) in [
        (pass.color_input, RSF_OVERLAY_SR_INPUT_COLOR),
        (pass.depth, RSF_OVERLAY_SR_INPUT_DEPTH),
        (pass.motion, RSF_OVERLAY_SR_INPUT_MOTION),
    ] {
        if !texture.is_null() {
            bits |= bit;
        }
    }
    let camera = &pass.camera;
    if pass.camera_valid != 0
        && camera.struct_size >= size_of::<rsf_camera_frame>() as u32
        && camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
        && camera.jitter_pixels.iter().all(|value| value.is_finite())
    {
        return (bits | RSF_OVERLAY_SR_INPUT_JITTER, camera.jitter_pixels);
    }
    (bits, [0.0; 2])
}

fn depth_infinite(setup: &rsf_d3d11_host_setup) -> u32 {
    if setup.struct_size as usize == offset_of!(rsf_d3d11_host_setup, depth_far_plane) {
        setup.depth_inverted
    } else {
        (setup.depth_far_plane == RSF_D3D11_HOST_DEPTH_INFINITE) as u32
    }
}

fn validate_setup(setup: &rsf_d3d11_host_setup, services: &rsf_game_host_services) -> bool {
    let motion = &setup.motion;
    let caps = setup.capabilities;
    let known = RSF_D3D11_HOST_CAP_FG_INPUTS
        | RSF_D3D11_HOST_CAP_INPUT_TIMELINE
        | RSF_D3D11_HOST_CAP_RENDERED_SOURCE;
    !(setup.configuration_path.is_null()
        || setup.configuration_section.is_null()
        || setup.runtime_directory.is_null()
        || setup.identity_utf8.is_null()
        || setup.executable_sha256.is_null()
        || setup.engine_version_utf8.is_null()
        || setup.project_id_utf8.is_null()
        || services.session_id == 0
        || !crate::sr_session::valid_unit_scale(setup.units_to_meters)
        || setup.quality_choices == 0
        || setup.quality_choices & !0x1f != 0
        || motion.struct_size < size_of_val(motion) as u32
        || motion.scale_x == 0.0
        || motion.scale_y == 0.0
        || !motion.scale_x.is_finite()
        || !motion.scale_y.is_finite()
        || setup.colour_transport_default > 1
        || setup.presentation_bridge > 3
        || (setup.presentation_bridge != 0 && setup.accept_window.is_none())
        || caps & !known != 0
        || (caps != 0 && setup.presentation_bridge == 0)
        || (caps & RSF_D3D11_HOST_CAP_RENDERED_SOURCE != 0
            && caps & RSF_D3D11_HOST_CAP_INPUT_TIMELINE != 0)
        || services.render_pass.is_some()
        || services.render_config.is_some()
        || services.process_upscale.is_some()
        || services.present.is_some()
        || services.scope_state.is_some()
        || services.submit.is_some()
        || services.process_latency.is_some()
        || services.render_feedback.is_some())
}

impl Host {
    fn new(setup: &rsf_d3d11_host_setup, upstream: rsf_game_host_services) -> Option<Self> {
        let copy = |text: *const c_char| unsafe { CStr::from_ptr(text) }.to_owned();
        let runtime = unsafe { wide_copy(setup.runtime_directory) };
        let root = String::from_utf16_lossy(&runtime[..runtime.len() - 1]);
        let path = |leaf: &str| CString::new(format!("{root}/{leaf}")).ok();
        let text = Text {
            streamline: path("streamline")?,
            fidelityfx: path("fidelityfx")?,
            xess: path("xess")?,
            captures: path("captures")?,
            identity: copy(setup.identity_utf8),
            fingerprint: copy(setup.executable_sha256),
            version: copy(setup.engine_version_utf8),
            project: copy(setup.project_id_utf8),
        };
        let mut retained = *setup;
        // Numeric metadata and process-lived policy callbacks survive; strings are copied.
        retained.configuration_path = ptr::null();
        retained.configuration_section = ptr::null();
        retained.runtime_directory = ptr::null();
        retained.identity_utf8 = ptr::null();
        retained.executable_sha256 = ptr::null();
        retained.engine_version_utf8 = ptr::null();
        retained.project_id_utf8 = ptr::null();
        Some(Self {
            upstream,
            setup: retained,
            ini: unsafe { wide_copy(setup.configuration_path) },
            section: unsafe { wide_copy(setup.configuration_section) },
            text,
            bridge_installed: AtomicBool::new(false),
            bridge_bound: AtomicBool::new(false),
            active: AtomicBool::new(false),
            transport: AtomicBool::new(true),
            fidelity: AtomicBool::new(false),
            capture: AtomicBool::new(false),
            capture_failures: AtomicBool::new(false),
            capture_sequence: AtomicBool::new(false),
            requested_generation: AtomicU32::new(1),
            cpu_reports: AtomicU32::new(0),
            cpu_sample_reports: AtomicU32::new(0),
            cpu_refusal_stages: AtomicU32::new(0),
            cpu_stage_reports: Default::default(),
            shared: Mutex::new(Shared {
                desired: rsf_render_settings {
                    struct_size: size_of::<rsf_render_settings>() as u32,
                    abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
                    ..zeroed_settings()
                },
                planned: rsf_game_render_config {
                    struct_size: size_of::<rsf_game_render_config>() as u32,
                    enabled: 0,
                    output_width: 0,
                    output_height: 0,
                    render_width: 0,
                    render_height: 0,
                },
                report: fresh_report(),
            }),
            graphics: Mutex::new(Graphics::new()),
        })
    }

    fn user(&self) -> Raw {
        ptr::from_ref(self).cast_mut().cast()
    }

    fn bridge(&self) -> bool {
        self.bridge_installed.load(Ordering::Acquire)
    }

    fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    // The callback lease. A contended lease is refused, never waited on, so a
    // reentrant or concurrent native callback cannot deadlock the owner.
    fn try_graphics(&self) -> Option<MutexGuard<'_, Graphics>> {
        crate::sync::try_lock(&self.graphics)
    }

    fn lock_graphics(&self) -> MutexGuard<'_, Graphics> {
        lock(&self.graphics)
    }

    fn log(&self, text: &str) {
        if let (Some(log), Ok(text)) = (self.upstream.log, CString::new(text)) {
            unsafe { log(self.upstream.user, text.as_ptr()) };
        }
    }

    fn same(&self, left: Raw, right: Raw) -> bool {
        unsafe { native::same_object(left, right) != 0 }
    }

    fn child_device(&self, child: Raw) -> Option<IUnknown> {
        unsafe { com_ref::owned(native::child_device(child)) }
    }

    fn on_device(&self, child: Raw, device: Raw) -> bool {
        self.same(ptr_of(&self.child_device(child)), device)
    }

    fn is_immediate(&self, context: Raw) -> bool {
        unsafe { native::is_immediate(context) != 0 }
    }

    fn dump_wants(&self, record: &rsf_frame_record) -> bool {
        unsafe { native::dump_wants(record) != 0 }
    }

    fn dump_texture(
        &self,
        context: Raw,
        record: &rsf_frame_record,
        name: *const c_char,
        description: &CStr,
        texture: Raw,
        view: u32,
    ) {
        unsafe {
            native::dump_texture(
                context,
                record,
                name,
                description.as_ptr(),
                texture,
                view,
                1.0,
            )
        };
    }

    fn dump_result(&self, record: &rsf_frame_record, stage: &CStr, backend: u32, result: i32) {
        unsafe { native::dump_result(record, stage.as_ptr(), backend, result) };
    }

    fn ini_read(&self, key: &CStr, fallback: bool) -> bool {
        let (ini, section) = (self.ini.as_ptr(), self.section.as_ptr());
        unsafe { native::ini_read(ini, section, key.as_ptr(), u32::from(fallback)) != 0 }
    }

    fn ini_write(&self, key: &CStr, value: bool) -> bool {
        let (ini, section) = (self.ini.as_ptr(), self.section.as_ptr());
        unsafe { native::ini_write(ini, section, key.as_ptr(), u32::from(value)) == OK }
    }

    fn install_bridge(&self) {
        let mode = self.setup.presentation_bridge;
        if mode == 0 {
            return;
        }
        let rendered = self.setup.capabilities & RSF_D3D11_HOST_CAP_RENDERED_SOURCE != 0;
        let bridge = rsf_d3d11_present_setup {
            struct_size: size_of::<rsf_d3d11_present_setup>() as u32,
            runtime_directory_utf8: self.text.streamline.as_ptr(),
            development_runtime: 0,
            log: Some(host_log),
            user: self.user(),
            accept_window: Some(host_accept_window),
            before_present: None,
            present_event: Some(host_present),
            latency_event: Some(native::fg_latency_event),
            retire: Some(native::fg_retire),
            prepare: Some(native::fg_prepare),
            debug_timing: 0,
            backend: 0,
            max_generated_frames: MAX_GENERATED_FRAMES,
            depth_inverted: self.setup.depth_inverted,
            depth_infinite: depth_infinite(&self.setup),
            units_to_meters: self.setup.units_to_meters,
            runtime_switching: 1,
            ui_mode: UI_MODE_BACKBUFFER_HUDLESS,
            fsr3_directory_utf8: self.text.fidelityfx.as_ptr(),
            fsr4_directory_utf8: self.text.fidelityfx.as_ptr(),
            xess_directory_utf8: self.text.xess.as_ptr(),
            streamline_directory_utf8: self.text.streamline.as_ptr(),
            engine_type: self.setup.engine_type,
            engine_version_utf8: self.text.version.as_ptr(),
            project_id_utf8: self.text.project.as_ptr(),
            allowed_backend_choices: if rendered {
                (1 << FG_BACKEND_FSR3) | (1 << FG_BACKEND_FSR4)
            } else {
                0
            },
        };
        let install = match mode {
            2 => native::present_prepare,
            3 => native::present_install_scoped,
            _ => native::present_install,
        };
        let installed = unsafe {
            // The source mode is published even when the bridge refuses installation.
            native::fg_rendered_source_set(u32::from(rendered));
            install(&bridge) != 0
        };
        if installed {
            // Runtime switching starts with plain presentation and FG Off.
            let off = rsf_native_fg_options {
                struct_size: size_of::<rsf_native_fg_options>() as u32,
                mode: RSF_FG_OFF,
                generated_frames: 1,
                reflex_mode: REFLEX_OFF,
                frame_limit_us: 0,
                debug: 0,
            };
            unsafe {
                native::fg_set_log(bridge.log, bridge.user);
                native::fg_options_set(&off);
            }
        }
        self.bridge_installed.store(installed, Ordering::Release);
        self.log(match (installed, mode) {
            (false, _) => {
                "D3D11 callback host presentation bridge refused; native presentation retained."
            }
            (true, 3) => {
                "D3D11 callback host installed scoped creation hooks; outer canonical ownership awaits native return."
            }
            (true, 2) => {
                "D3D11 callback host prepared outer creation root; facade ownership awaits engine binding."
            }
            _ => {
                "D3D11 callback host installed cold presentation bridge; facade ownership awaits engine binding."
            }
        });
    }

    fn initialize(&self, g: &mut Graphics, context: Raw, swapchain: Raw) -> bool {
        if g.configured {
            return true;
        }
        let bridge = self.bridge();
        if self.setup.presentation_bridge != 0
            && (!bridge || unsafe { native::present_is_owner(swapchain) } == 0)
        {
            self.log(
                "D3D11 host refused presentation binding: engine chain is not the final application facade.",
            );
            return false;
        }
        g.device = self.child_device(context);
        let mut vendor = 0;
        if g.device.is_none() || unsafe { native::device_vendor(g.device_ptr(), &mut vendor) } == 0
        {
            return false;
        }
        let selected =
            crate::rsf_gpu_select_sr_backend(vendor, (vendor == 0x1414) as u32, SR_BACKEND_CHOICES);
        if selected == 0 {
            return false;
        }
        let caps = self.setup.capabilities;
        let timeline = bridge && caps & RSF_D3D11_HOST_CAP_INPUT_TIMELINE != 0;
        let rendered = bridge && caps & RSF_D3D11_HOST_CAP_RENDERED_SOURCE != 0;
        let generation = (timeline || rendered) && caps & RSF_D3D11_HOST_CAP_FG_INPUTS != 0;
        let mut defaults = rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: 1,
            sr_backend: selected,
            fg_backend: FG_BACKEND_AUTO,
            generated_frames: 1,
            latency: REFLEX_OFF,
            status_overlay: 1,
            ..zeroed_settings()
        };
        let mut fg_choices = if generation {
            [
                FG_BACKEND_AUTO,
                FG_BACKEND_DLSS,
                FG_BACKEND_FSR3,
                FG_BACKEND_FSR4,
                FG_BACKEND_XESS,
            ]
            .iter()
            .fold(0, |mask, backend| mask | 1 << backend)
        } else {
            0
        };
        if rendered {
            fg_choices = if generation {
                (1 << FG_BACKEND_FSR3) | (1 << FG_BACKEND_FSR4)
            } else {
                0
            };
            defaults.fg_backend = FG_BACKEND_FSR3;
        }
        if rendered {
            self.log(
                "D3D11 rendered-source FG: input timing unavailable; latency Off, no pre-input pacing.",
            );
        } else if !timeline {
            self.log(
                "D3D11 host refuses FG/latency controls: native input timeline capability unavailable.",
            );
        } else if !generation {
            self.log(
                "D3D11 host refuses FG controls: native FG input lease capability unavailable.",
            );
        }
        for (index, option) in ADVANCED.iter().enumerate() {
            if let Some(key) = option.key {
                let fallback = index == Advanced::Transport as usize
                    && self.setup.colour_transport_default == 0;
                (option.flag)(self).store(self.ini_read(key, fallback), Ordering::SeqCst);
            }
        }
        let toggle = |label: &'static CStr, value: bool| rsf_overlay_service_option {
            label_utf8: label.as_ptr(),
            kind: RSF_OVERLAY_SERVICE_OPTION_TOGGLE,
            value: value as i32,
            minimum: 0,
            maximum: 1,
            step: 1,
            divisor: 1,
            restart: 0,
        };
        let advanced = ADVANCED.map(|option| toggle(option.label, option.value(self) != 0));
        let panel = rsf_overlay_service_setup {
            struct_size: size_of::<rsf_overlay_service_setup>() as u32,
            abi_version: RSF_OVERLAY_API_VERSION,
            identity_utf8: self.text.identity.as_ptr(),
            configuration_path: self.ini.as_ptr(),
            configuration_section: self.section.as_ptr(),
            defaults,
            sr_backend_choices: SR_BACKEND_CHOICES,
            sr_quality_choices: self.setup.quality_choices,
            fg_backend_choices: fg_choices,
            latency_choices: if timeline {
                (1 << REFLEX_OFF) | (1 << REFLEX_ON) | (1 << REFLEX_BOOST)
            } else {
                1 << REFLEX_OFF
            },
            max_generated_frames: if generation { MAX_GENERATED_FRAMES } else { 1 },
            dlss_preset_available: 1,
            log: Some(host_log),
            sr_report: Some(host_sr_report),
            settings_changed: Some(host_changed),
            user: self.user(),
            option_count: advanced.len() as u32,
            options: advanced.as_ptr(),
            options_read: Some(host_options),
            option_changed: Some(host_action),
            ..Default::default()
        };
        let mut applied = zeroed_settings();
        if unsafe { native::overlay_configure(&panel, &mut applied) } != OK {
            return false;
        }
        self.shared().desired = applied;
        g.execution_thread = current_thread();
        g.chain = Some(unsafe { com_ref::retain(swapchain) });
        if unsafe { native::present_is_owner(swapchain) } != 0 {
            g.observer = true;
            self.log(
                "D3D11 callback host uses bridge application Present events; native DXGI interception is unnecessary.",
            );
        } else {
            g.observer = unsafe {
                native::observer_install(Some(host_log), self.user(), Some(host_present))
            } != 0;
        }
        if !g.observer {
            self.log("D3D11 callback host: Present observation refused; SR activation suspended.");
        }
        let text = &self.text;
        unsafe {
            native::dump_configure(
                text.captures.as_ptr(),
                text.identity.as_ptr(),
                text.fingerprint.as_ptr(),
                Some(host_log),
                self.user(),
            )
        };
        set_reason(
            &mut self.shared().report,
            "Waiting for verified scene inputs",
        );
        let window = unsafe { native::chain_window(swapchain) };
        if !window.is_null() {
            native::latency_window_bind(self.upstream.session_id, window);
        }
        g.configured = true;
        if g.observer && self.setup.presentation_bridge != 0 {
            self.bridge_bound.store(true, Ordering::Release);
        }
        self.log(
            "D3D11 callback host configured; FG/latency controls follow declared capabilities and native eligibility.",
        );
        g.observer
    }

    fn arm_failure_capture(&self, budget: &mut CaptureBudget, result: i32) -> bool {
        let enabled = self.capture_failures.load(Ordering::Acquire);
        let pending = unsafe { native::dump_requested() } != 0;
        if !budget.claim_failure(enabled, pending, result) {
            return false;
        }
        unsafe { native::dump_request() };
        self.log(
            "Automatic SR failure capture: failure-time resources, not a pre-evaluation snapshot.",
        );
        true
    }

    fn capture_inputs(
        &self,
        backend: u32,
        context: Raw,
        pass: &Pass,
        record: &mut rsf_frame_record,
        original_colour: Raw,
    ) {
        record.struct_size = size_of::<rsf_frame_record>() as u32;
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = if pass.source_frame_id != 0 {
            pass.source_frame_id
        } else {
            pass.native_frame
        };
        record.session_id = pass.session_id;
        record.view_id = pass.view_key as u32;
        record.flags = RSF_FRAME_FLAG_NO_FG
            | if pass.source_frame_id != 0 {
                0
            } else {
                RSF_FRAME_FLAG_AMBIGUOUS_ID
            };
        record.render_width = pass.camera.render_width;
        record.render_height = pass.camera.render_height;
        record.output_width = pass.camera.output_width;
        record.output_height = pass.camera.output_height;
        record.camera = pass.camera;
        record.frame_time_ms = pass.camera.frame_time_seconds * 1000.0;
        if self.capture.swap(false, Ordering::AcqRel) {
            unsafe { native::dump_request() };
        }
        unsafe { native::dump_poll() };
        if unsafe { native::dump_begin(record, backend) } == 0 {
            return;
        }
        unsafe { native::dump_inputs(context, pass, record, original_colour) };
    }

    fn prepare_pipeline(
        &self,
        g: &mut Graphics,
        pass: &Pass,
        settings: &rsf_render_settings,
    ) -> bool {
        let (width, height) = (pass.camera.output_width, pass.camera.output_height);
        if settings.sr_enabled != 0 {
            if !g.running && !g.attempted {
                g.attempted = true;
                let (setup, text) = (&self.setup, &self.text);
                let start = rsf_dlss_pipeline_setup {
                    struct_size: size_of::<rsf_dlss_pipeline_setup>() as u32,
                    abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
                    streamline_directory_utf8: text.streamline.as_ptr(),
                    streamline_log_directory_utf8: ptr::null(),
                    require_signature: 1,
                    output_width: width,
                    output_height: height,
                    quality: settings.sr_quality,
                    motion: setup.motion,
                    log: Some(host_log),
                    log_user: self.user(),
                    engine: setup.engine_type,
                    engine_version_utf8: text.version.as_ptr(),
                    project_id_utf8: text.project.as_ptr(),
                    fsr2_directory_utf8: text.fidelityfx.as_ptr(),
                    fsr3_directory_utf8: text.fidelityfx.as_ptr(),
                    fsr4_directory_utf8: text.fidelityfx.as_ptr(),
                    xess_directory_utf8: text.xess.as_ptr(),
                    view_space_to_meters: setup.units_to_meters,
                    standard_depth: u32::from(setup.depth_inverted == 0),
                };
                g.last_result =
                    unsafe { native::pipeline_start(g.device_ptr(), &start, settings.sr_backend) };
                g.running = g.last_result == OK;
                g.backend = settings.sr_backend;
                g.dlss_started = g.running && g.backend == RSF_SR_DLSS;
                g.present_result = DLSS_OK;
                g.quality = settings.sr_quality;
                g.output_width = width;
                g.output_height = height;
            }
            if g.running && g.present_result == DLSS_OK {
                if g.output_width != width || g.output_height != height {
                    g.last_result =
                        unsafe { native::pipeline_resize(width, height, null_mut(), null_mut()) };
                    if g.last_result == OK {
                        g.output_width = width;
                        g.output_height = height;
                        g.reset = true;
                    }
                }
                if g.backend != settings.sr_backend {
                    g.last_result = unsafe {
                        native::pipeline_select(settings.sr_backend, null_mut(), null_mut())
                    };
                    if g.last_result == OK {
                        g.backend = settings.sr_backend;
                        g.dlss_started |= g.backend == RSF_SR_DLSS;
                        g.reset = true;
                    }
                }
                if g.quality != settings.sr_quality {
                    g.last_result = unsafe {
                        native::pipeline_quality(settings.sr_quality, null_mut(), null_mut())
                    };
                    if g.last_result == OK {
                        g.quality = settings.sr_quality;
                        g.reset = true;
                    }
                }
                if g.preset != settings.dlss_preset {
                    g.last_result = unsafe { native::pipeline_preset(settings.dlss_preset) };
                    if g.last_result == OK {
                        g.preset = settings.dlss_preset;
                        g.reset = true;
                    }
                }
                native::pipeline_sharpen(settings);
            }
        }
        // SAFETY: the status record contains only integers.
        let mut status = rsf_dlss_pipeline_status {
            struct_size: size_of::<rsf_dlss_pipeline_status>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let ready = g.running
            && unsafe { native::pipeline_status(&mut status) } == OK
            && status.running != 0
            && status.output_width == width
            && status.output_height == height
            && g.quality == settings.sr_quality
            && g.backend == settings.sr_backend
            && g.present_result == DLSS_OK;
        self.shared().planned = rsf_game_render_config {
            struct_size: size_of::<rsf_game_render_config>() as u32,
            enabled: (ready && settings.sr_enabled != 0) as u32,
            output_width: width,
            output_height: height,
            render_width: if ready { status.render_width } else { width },
            render_height: if ready { status.render_height } else { height },
        };
        ready
    }

    fn process(&self, context: Raw, pass: &Pass, auxiliary: Option<&[Surface]>) -> i32 {
        if pass.session_id != self.upstream.session_id {
            return INVALID;
        }
        let Some(mut guard) = self.try_graphics() else {
            return BUSY;
        };
        let g = &mut *guard;
        if !self.active() {
            return NOT_READY;
        }
        let reconfigure = pass.flags & RSF_GAME_RENDER_RECONFIGURE != 0;
        let recovery = g.reduced_pending();
        if let Some(ticket) = g.reduced.as_ref().filter(|ticket| ticket.pending)
            && (!ticket.sealed || !ticket.matches(pass, true) || reconfigure || ticket.attempted)
        {
            return BUSY;
        }
        if g.auxiliaries_pending() {
            return BUSY;
        }
        if let Some(ticket) = g.reduced.as_ref().filter(|ticket| ticket.pending)
            && (!ticket.temporal_surfaces(auxiliary)
                || ticket.context_ptr() != context
                || ticket.generation != self.requested_generation.load(Ordering::Acquire))
        {
            return NOT_READY;
        }
        // Bind ownership from a verified scene lease, never from an early loading callback.
        if reconfigure && !g.scene_bound {
            return NOT_READY;
        }
        if !self.initialize(g, context, pass.swapchain)
            || !g.observer
            || g.execution_thread != current_thread()
        {
            return NOT_READY;
        }
        let device = g.device_ptr();
        if !self.on_device(context, device)
            || pass.swapchain != g.chain_ptr()
            || !self.is_immediate(context)
        {
            return NOT_READY;
        }
        // Reject foreign temporal resources before auxiliary preparation or capture.
        let borrowed = [
            pass.color_input,
            pass.color_output,
            pass.color_output_readable,
            pass.depth,
            pass.motion,
            pass.exposure,
            pass.temporal_mask,
        ];
        if borrowed
            .into_iter()
            .any(|texture| !texture.is_null() && !self.on_device(texture, device))
        {
            return INVALID;
        }
        g.scene_bound = true;
        let settings = self.shared().desired;
        let generation = self.requested_generation.load(Ordering::SeqCst);
        let unchanged_configuration = generation == g.applied_generation
            && g.running
            && settings.sr_enabled != 0
            && g.backend == settings.sr_backend
            && g.quality == settings.sr_quality
            && g.preset == settings.dlss_preset
            && g.output_width == pass.camera.output_width
            && g.output_height == pass.camera.output_height;
        if !recovery && generation != g.applied_generation {
            g.attempted = false;
            g.reset = true;
            g.applied_generation = generation;
        }
        if !recovery {
            native::pipeline_colour_transport(self.transport.load(Ordering::Acquire) as u32);
            native::pipeline_colour_correction(self.fidelity.load(Ordering::Acquire) as u32);
        }
        let pipeline_ready = if recovery {
            g.running
        } else {
            self.prepare_pipeline(g, pass, &settings)
        };
        let configure_only = reconfigure;
        // Planning returns no output. An unchanged successful plan also has no new
        // evaluation result to publish, including when the last evaluation failed.
        if configure_only && pipeline_ready && unchanged_configuration {
            return NOT_READY;
        }
        let mut record = zeroed_record();
        // A sequence waits for ready gameplay rather than consuming size plans.
        if self.capture_sequence.swap(false, Ordering::AcqRel) {
            g.capture_budget.request_sequence();
        }
        let sequence_frame = !configure_only && {
            let ready = unchanged_configuration
                && pipeline_ready
                && pass.camera_valid != 0
                && pass.screen == SCREEN_GAMEPLAY;
            let pending =
                self.capture.load(Ordering::Acquire) || unsafe { native::dump_requested() } != 0;
            g.capture_budget
                .claim_source(pass.session_id, pass.source_frame_id, ready, pending)
        };
        if sequence_frame {
            unsafe { native::dump_request() };
        }
        if !configure_only {
            self.capture_inputs(g.backend, context, pass, &mut record, null_mut());
        }
        if sequence_frame && self.dump_wants(&record) {
            self.dump_result(&record, c"sequence_capture", g.backend, OK);
        }
        if let Some(surfaces) = auxiliary
            && self.dump_wants(&record)
        {
            for surface in surfaces
                .iter()
                .filter(|surface| !surface.source_name.is_null())
            {
                self.dump_texture(
                    context,
                    &record,
                    surface.source_name,
                    c"Original auxiliary before reconstruction",
                    surface.texture,
                    DUMP_RAW,
                );
            }
        }
        let mut result = NOT_READY;
        let mut observed = None;
        if settings.sr_enabled != 0 {
            if g.running && g.present_result == DLSS_OK && !configure_only {
                let mut evaluated = *pass;
                if g.reset {
                    evaluated.flags |= RSF_GAME_RENDER_RESET;
                }
                if auxiliary.is_some() && g.auxiliaries.is_none() {
                    g.auxiliaries = native::auxiliary_products();
                }
                if recovery {
                    let ticket = g.reduced.as_mut().expect("pending recovery ticket");
                    let prepared =
                        auxiliary.is_none_or(|surfaces| ticket.products.temporal_prepare(surfaces));
                    ticket.attempted = true;
                    ticket.flags |= RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED;
                    let mut ordered = 0;
                    result = if prepared {
                        observed = Some(sr_inputs(&evaluated));
                        unsafe { native::sr_evaluate(context, &evaluated, &mut ordered) }
                    } else {
                        NOT_READY
                    };
                    ticket.ordered = !prepared || ordered != 0;
                    if result == OK && !ticket.products.commit(true) {
                        result = NOT_READY;
                        // A partial native commit remains an owned cleanup obligation.
                        ticket.ordered = false;
                    }
                    ticket.temporal_result = result;
                    ticket.temporal = result == OK;
                } else {
                    let prepared = auxiliary.is_none_or(|surfaces| {
                        g.auxiliaries.as_mut().is_some_and(|products| {
                            products.prepare(
                                context,
                                &evaluated,
                                surfaces,
                                Some(host_log),
                                self.user(),
                            )
                        })
                    });
                    result = if prepared {
                        observed = Some(sr_inputs(&evaluated));
                        unsafe { native::sr_evaluate(context, &evaluated, null_mut()) }
                    } else {
                        NOT_READY
                    };
                    if auxiliary.is_some() {
                        if result == OK
                            && !g
                                .auxiliaries
                                .as_mut()
                                .is_some_and(|products| products.commit(false))
                        {
                            result = NOT_READY;
                        }
                        if result != OK
                            && !g
                                .auxiliaries
                                .as_mut()
                                .is_none_or(|products| products.restore())
                        {
                            result = BUSY;
                        }
                    }
                }
                if result == OK {
                    g.reset = false;
                    g.evaluated += 1;
                } else {
                    g.reset = true;
                    g.refused += 1;
                }
            } else {
                result = if configure_only {
                    NOT_READY
                } else if g.running {
                    g.present_result
                } else {
                    g.last_result
                };
                // An unchanged, successful plan does not evaluate or invalidate temporal history.
                // Genuine reconfiguration keeps its pending reset; failed preparation still resets.
                if !configure_only || !pipeline_ready {
                    g.reset = true;
                }
            }
        } else {
            g.reset = true;
        }
        if !configure_only
            && settings.sr_enabled != 0
            && self.arm_failure_capture(&mut g.capture_budget, result)
        {
            self.capture_inputs(g.backend, context, pass, &mut record, null_mut());
            self.dump_result(&record, c"automatic_sr_failure", g.backend, result);
        }
        if self.dump_wants(&record) {
            if result == OK {
                self.dump_texture(
                    context,
                    &record,
                    c"sr_reinserted".as_ptr(),
                    c"Reconstructed linear colour before native HDR",
                    pass.color_output,
                    DUMP_HDR,
                );
                for surface in auxiliary
                    .unwrap_or_default()
                    .iter()
                    .filter(|surface| !surface.name.is_null())
                {
                    self.dump_texture(
                        context,
                        &record,
                        surface.name,
                        c"Reconstructed auxiliary before native post-processing",
                        surface.texture,
                        if surface.kind == AUX_DEPTH_STENCIL {
                            DUMP_DEPTH
                        } else {
                            DUMP_RAW
                        },
                    );
                }
            }
            self.dump_result(&record, c"sr", g.backend, result);
            unsafe { native::dump_pass(context, pass, 0) };
            g.capture_record = record;
            g.capture_view = pass.view_key;
            g.capture_native_frame = pass.native_frame;
            g.capture_pending = true;
        }
        {
            let mut shared = self.shared();
            let report = &mut shared.report;
            let prior_inputs = (report.inputs, report.jitter_pixels);
            *report = fresh_report();
            report.effective_backend = if result == OK { g.backend } else { 0 };
            report.effective_quality = g.quality;
            report.render_width = pass.camera.render_width;
            report.render_height = pass.camera.render_height;
            report.output_width = pass.camera.output_width;
            report.output_height = pass.camera.output_height;
            report.frames_evaluated = g.evaluated as u32;
            report.frames_refused = g.refused as u32;
            report.last_result = result;
            report.quality_locked = (self.setup.quality_choices == 1) as u32;
            (report.inputs, report.jitter_pixels) = if configure_only {
                prior_inputs
            } else {
                observed.unwrap_or_default()
            };
            if configure_only {
                set_reason(
                    report,
                    "Renderer preparation only; native temporal processing retained",
                );
            } else if result != OK {
                set_reason(
                    report,
                    &format!("SR result {result}; native temporal processing retained"),
                );
            }
        }
        if result != g.last_result || g.logged < 3 {
            g.logged += 1;
            self.log(&format!(
                "D3D11 host SR result={result} evaluated={} refused={} native_frame={}",
                g.evaluated, g.refused, pass.native_frame
            ));
        }
        g.last_result = result;
        result
    }

    fn incomplete(
        &self,
        ticket: &mut Ticket,
        disposition: u32,
        request: &mut rsf_d3d11_reduced_frame,
    ) -> i32 {
        ticket.disposition = disposition;
        ticket.flags |= RSF_D3D11_REDUCED_CLEANUP_REQUIRED;
        let defined = ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED != 0;
        if !defined {
            ticket.flags |= RSF_D3D11_REDUCED_REQUIRED_RECOVERY_FAILED;
        }
        {
            let mut shared = self.shared();
            if !defined {
                shared.report.effective_backend = 0;
            }
            set_reason(
                &mut shared.report,
                &format!(
                    "Reduced-frame cleanup pending; output_defined={}",
                    defined as u32
                ),
            );
        }
        ticket.output(request);
        BUSY
    }

    fn reduced_reserve(
        &self,
        g: &mut Graphics,
        context: Raw,
        pass: &Pass,
        request: &mut rsf_d3d11_reduced_frame,
    ) -> i32 {
        let count = request.surface_count as usize;
        if !self.active()
            || !g.scene_bound
            || !g.running
            || pass.role != RSF_GAME_RENDER_SR
            || pass.source_frame_id == 0
            || pass.native_frame == 0
            || pass.view_key == 0
            || pass.swapchain != g.chain_ptr()
            || pass.color_input.is_null()
            || pass.color_output.is_null()
            || pass.flags & RSF_GAME_RENDER_RECONFIGURE != 0
            || count > MAX_RESERVED
            || (count != 0 && request.surfaces.is_null())
            || g.next_reduced_token == 0
        {
            return NOT_READY;
        }
        let plan = self.shared().planned;
        let generation = self.requested_generation.load(Ordering::Acquire);
        if plan.enabled == 0
            || generation != g.applied_generation
            || rect_extent(pass.render_rect) != Some([plan.render_width, plan.render_height])
            || rect_extent(pass.output_rect) != Some([plan.output_width, plan.output_height])
        {
            return NOT_READY;
        }
        let device = self.child_device(context);
        let immediate = unsafe { com_ref::owned(native::immediate_context(g.device_ptr())) };
        if context != ptr_of(&immediate)
            || !self.same(ptr_of(&device), g.device_ptr())
            || !self.is_immediate(context)
            || unsafe { native::device_ok(ptr_of(&device)) } == 0
        {
            return NOT_READY;
        }
        let private_output = native::pipeline_output();
        let private =
            |texture: Raw| !private_output.is_null() && self.same(texture, private_output);
        if private(pass.color_input)
            || private(pass.color_output)
            || private(pass.color_output_readable)
        {
            return INVALID;
        }
        let inputs = if count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(request.surfaces, count) }
        };
        // Build the new reservation aside. A refused RESERVE leaves the
        // previous ticket's idempotent acknowledgement intact.
        let mut surfaces = [Surface::default(); MAX_RESERVED + 2];
        for (target, input) in surfaces.iter_mut().zip(inputs) {
            if input.struct_size < size_of::<Surface>() as u32
                || input.kind > AUX_COLOUR
                || input.flags & !AUX_RESTORE_CONTENTS != 0
                || input.texture.is_null()
                || input.texture == pass.color_input
                || input.texture == pass.color_output
                || input.texture == pass.color_output_readable
                || private(input.texture)
            {
                return INVALID;
            }
            *target = *input;
            target.name = ptr::null();
            target.source_name = ptr::null();
        }
        let mut total = count;
        let mut append_main = |destination: Raw| {
            let surface = &mut surfaces[total];
            *surface = Surface {
                struct_size: size_of::<Surface>() as u32,
                kind: AUX_COLOUR,
                texture: destination,
                source_rect: pass.render_rect.map(|edge| edge as f32),
                output_rect: pass.output_rect,
                ..Default::default()
            };
            total += 1;
        };
        append_main(pass.color_output);
        if !pass.color_output_readable.is_null() && pass.color_output_readable != pass.color_output
        {
            append_main(pass.color_output_readable);
        }
        if g.reduced.is_none() {
            g.reduced = native::auxiliary_products().map(Ticket::new);
        }
        let Some(ticket) = g.reduced.as_mut() else {
            return NOT_READY;
        };
        if !ticket.products.reserve(
            context,
            pass,
            &surfaces[..total],
            Some(host_log),
            self.user(),
            true,
        ) {
            return NOT_READY;
        }
        let mut owner = *pass;
        owner.depth = null_mut();
        owner.motion = null_mut();
        owner.exposure = null_mut();
        owner.temporal_mask = null_mut();
        owner.ui_input = null_mut();
        owner.scene_surface = null_mut();
        owner.sampled_texture = null_mut();
        owner.camera_valid = 0;
        owner.depth_domains = zeroed_pass().depth_domains;
        ticket.owner = owner;
        ticket.surfaces = surfaces;
        ticket.count = count as u32;
        ticket.context = Some(unsafe { com_ref::retain(context) });
        ticket.chain = g.chain.clone();
        ticket.generation = generation;
        ticket.leases = std::iter::once(pass.color_input)
            .chain(surfaces[..total].iter().map(|surface| surface.texture))
            .map(|texture| unsafe { com_ref::retain(texture) })
            .collect();
        ticket.token = g.next_reduced_token;
        g.next_reduced_token += 1;
        ticket.pending = true;
        ticket.sealed = false;
        ticket.attempted = false;
        ticket.temporal = false;
        ticket.ordered = true;
        ticket.flags = 0;
        ticket.temporal_result = NOT_READY;
        ticket.disposition = RSF_D3D11_REDUCED_RESERVED;
        ticket.output(request);
        OK
    }

    fn reduced_step(
        &self,
        g: &mut Graphics,
        ticket: &mut Ticket,
        context: Raw,
        pass: &Pass,
        request: &mut rsf_d3d11_reduced_frame,
    ) -> i32 {
        let operation = request.operation;
        if !ticket.pending {
            ticket.output(request);
            return if ticket.acknowledges(operation) {
                OK
            } else {
                NOT_READY
            };
        }
        if unsafe { native::device_ok(g.device_ptr()) } == 0 {
            return self.incomplete(ticket, RSF_D3D11_REDUCED_UNSAFE, request);
        }
        if operation == RSF_D3D11_REDUCED_CANCEL {
            if ticket.sealed || !ticket.products.cancel() {
                return NOT_READY;
            }
            ticket.disposition = RSF_D3D11_REDUCED_CANCELLED;
            ticket.flags = RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED;
            ticket.output(request);
            ticket.release();
            return OK;
        }
        if operation == RSF_D3D11_REDUCED_SEAL {
            let count = ticket.count as usize;
            if ticket.sealed
                || request.surface_count != ticket.count
                || (count != 0 && request.surfaces.is_null())
            {
                return NOT_READY;
            }
            let mut refined = ticket.surfaces;
            if count != 0 {
                refined[..count].copy_from_slice(unsafe {
                    std::slice::from_raw_parts(request.surfaces, count)
                });
            }
            let mut total = count + 1;
            if !pass.color_output_readable.is_null()
                && pass.color_output_readable != pass.color_output
            {
                total += 1;
            }
            for surface in &mut refined[count..total] {
                surface.source_rect = pass.render_rect.map(|edge| edge as f32);
            }
            if !ticket.products.refine(&refined[..total]) {
                return INVALID;
            }
            ticket.owner.render_rect = pass.render_rect;
            for (target, surface) in ticket.surfaces.iter_mut().zip(&refined[..total]) {
                *target = *surface;
                target.name = ptr::null();
                target.source_name = ptr::null();
            }
            // Failure after snapshot entry cannot be cancelled as an unused reservation.
            ticket.sealed = true;
            if !ticket.products.seal() {
                ticket.ordered = false;
                return self.incomplete(ticket, RSF_D3D11_REDUCED_UNSAFE, request);
            }
            ticket.disposition = RSF_D3D11_REDUCED_SEALED;
            ticket.output(request);
            return OK;
        }
        if !ticket.sealed {
            return NOT_READY;
        }
        if operation == RSF_D3D11_REDUCED_FINISH {
            return self.reduced_finish(g, ticket, context, pass, request);
        }
        if ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED == 0 {
            return BUSY;
        }
        if !ticket.products.restore() {
            return self.incomplete(ticket, RSF_D3D11_REDUCED_CLEANUP_PENDING, request);
        }
        ticket.flags &= !RSF_D3D11_REDUCED_CLEANUP_REQUIRED;
        ticket.flags |= RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED;
        ticket.output(request);
        ticket.release();
        OK
    }

    fn reduced_finish(
        &self,
        g: &mut Graphics,
        ticket: &mut Ticket,
        context: Raw,
        pass: &Pass,
        request: &mut rsf_d3d11_reduced_frame,
    ) -> i32 {
        if ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED != 0 {
            ticket.output(request);
            return OK;
        }
        if !ticket.ordered || g.auxiliaries_pending() {
            return self.incomplete(ticket, RSF_D3D11_REDUCED_UNSAFE, request);
        }
        if !ticket.temporal
            && (!ticket.products.spatial_prepare() || !ticket.products.commit(false))
        {
            ticket.ordered = false;
            return self.incomplete(ticket, RSF_D3D11_REDUCED_CLEANUP_PENDING, request);
        }
        ticket.flags |= RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED
            | RSF_D3D11_REDUCED_CLEANUP_REQUIRED
            | if ticket.temporal {
                RSF_D3D11_REDUCED_TEMPORAL_OUTPUT
            } else {
                RSF_D3D11_REDUCED_SPATIAL_OUTPUT
            };
        ticket.disposition = if ticket.temporal {
            RSF_D3D11_REDUCED_TEMPORAL
        } else {
            RSF_D3D11_REDUCED_SPATIAL
        };
        if !ticket.temporal {
            g.reset = true;
            let [render_width, render_height] = rect_size(pass.render_rect);
            let [output_width, output_height] = rect_size(pass.output_rect);
            {
                let mut shared = self.shared();
                let report = &mut shared.report;
                report.effective_backend = 0;
                report.last_result = ticket.temporal_result;
                report.inputs = RSF_OVERLAY_SR_INPUT_COLOR;
                report.render_width = render_width;
                report.render_height = render_height;
                report.output_width = output_width;
                report.output_height = output_height;
                set_reason(
                    report,
                    &format!(
                        "Spatial reduced-frame recovery; temporal result {} attempted={}",
                        ticket.temporal_result, ticket.attempted as u32
                    ),
                );
            }
            self.arm_failure_capture(&mut g.capture_budget, ticket.temporal_result);
            let mut record = zeroed_record();
            if g.capture_pending
                && g.capture_native_frame == pass.native_frame
                && g.capture_view == pass.view_key
                && g.capture_record.frame_id == pass.source_frame_id
            {
                record = g.capture_record;
            } else {
                let mut raster = ticket.owner;
                let [width, height] = rect_size(raster.render_rect);
                raster.camera.render_width = width;
                raster.camera.render_height = height;
                let [width, height] = rect_size(raster.output_rect);
                raster.camera.output_width = width;
                raster.camera.output_height = height;
                self.capture_inputs(
                    g.backend,
                    context,
                    &raster,
                    &mut record,
                    ticket.products.original_colour(),
                );
            }
            if self.dump_wants(&record) {
                self.dump_texture(
                    context,
                    &record,
                    c"spatial_recovered".as_ptr(),
                    c"Numeric-nearest recovery, not temporal SR acceptance",
                    pass.color_output,
                    DUMP_HDR,
                );
                self.dump_result(&record, c"sr", g.backend, ticket.temporal_result);
                self.dump_result(&record, c"spatial_recovery", 0, OK);
                g.capture_record = record;
                g.capture_native_frame = pass.native_frame;
                g.capture_view = pass.view_key;
                g.capture_pending = true;
            }
        }
        ticket.output(request);
        OK
    }

    fn reduced_frame(&self, context: Raw, request: &mut rsf_d3d11_reduced_frame) -> i32 {
        let Some(mut guard) = self.try_graphics() else {
            return BUSY;
        };
        let g = &mut *guard;
        // Session/thread/token guards precede all borrowed graphics dereferences.
        if !g.configured
            || g.execution_thread != current_thread()
            || context.is_null()
            || !unsafe { has_capacity(request.owner) }
            || unsafe { (*request.owner).session_id } != self.upstream.session_id
        {
            return NOT_READY;
        }
        request.disposition = RSF_D3D11_REDUCED_NONE;
        request.result_flags = 0;
        request.temporal_result = NOT_READY;
        let pass = unsafe { *request.owner };
        if request.operation == RSF_D3D11_REDUCED_RESERVE {
            if request.token != 0 || g.reduced_pending() || g.auxiliaries_pending() {
                return BUSY;
            }
            return self.reduced_reserve(g, context, &pass, request);
        }
        let Some(mut ticket) = g.reduced.take() else {
            return NOT_READY;
        };
        let result = if request.token == 0
            || request.token != ticket.token
            || context != ticket.context_ptr()
            || !ticket.matches(&pass, request.operation != RSF_D3D11_REDUCED_SEAL)
        {
            NOT_READY
        } else {
            self.reduced_step(g, &mut ticket, context, &pass, request)
        };
        g.reduced = Some(ticket);
        result
    }

    fn passes(&self, context: Raw, pass: &Pass, begin: u32) {
        let Some(guard) = self.try_graphics() else {
            return;
        };
        let g = &*guard;
        if !g.configured || g.execution_thread != current_thread() || (begin != 0 && !self.active())
        {
            return;
        }
        let bridge = self.bridge();
        // Native ownership facts remain useful before a generation provider is enabled.
        match pass.role {
            RSF_GAME_RENDER_FRAME if bridge => unsafe { native::fg_frame(pass, begin) },
            RSF_GAME_RENDER_SUBMISSION if bridge => unsafe { native::fg_submission(pass, begin) },
            RSF_GAME_RENDER_FG_INPUTS => {
                let requested = self.shared().desired.fg_enabled != 0;
                if requested
                    && begin != 0
                    && bridge
                    && self.setup.capabilities & RSF_D3D11_HOST_CAP_FG_INPUTS != 0
                    && !context.is_null()
                    && pass.swapchain == g.chain_ptr()
                    && self.on_device(context, g.device_ptr())
                    && self.is_immediate(context)
                {
                    unsafe { native::fg_capture(context, pass, &self.setup.motion) };
                }
            }
            RSF_GAME_RENDER_FINAL_SCENE => {
                if context.is_null()
                    || pass.scene_surface.is_null()
                    || !self.is_immediate(context)
                    || !self.on_device(context, g.device_ptr())
                    || !self.on_device(pass.scene_surface, g.device_ptr())
                {
                    return;
                }
                let accepted = unsafe { native::scene_pass(pass, begin) } != 0;
                if accepted && begin == 0 && bridge {
                    unsafe { native::fg_final(context, pass) };
                }
            }
            RSF_GAME_RENDER_WINDOW if pass.swapchain == g.chain_ptr() => unsafe {
                native::window_scope(pass, begin);
            },
            _ => {}
        }
    }

    fn present(&self, event: &rsf_observer_present_event) {
        let Some(mut guard) = self.try_graphics() else {
            return;
        };
        let g = &mut *guard;
        let event_thread = current_thread();
        if !self.active()
            || event.flags & DXGI_PRESENT_TEST != 0
            || event_thread != g.execution_thread
            || (self.bridge() && unsafe { native::present_is_owner(event.swapchain) } == 0)
            || !self.same(event.swapchain, g.chain_ptr())
        {
            return;
        }
        unsafe { native::window_present(event) };
        if event.completed != 0 && g.ownership_reports < 4 {
            let mut observed = rsf_native_present_record {
                struct_size: size_of::<rsf_native_present_record>() as u32,
                ..Default::default()
            };
            if unsafe { native::window_last_present(&mut observed) } != 0 && observed.matched != 0 {
                g.ownership_reports += 1;
                self.log(&format!(
                    "D3D11 native ownership source={} submission={} native_frame={} scene={} direct={} accepted={}; generation still requires complete inputs and policy",
                    observed.source_frame_id,
                    observed.scene_submission_id,
                    observed.scene_native_frame,
                    observed.scene_matched,
                    observed.scene_direct_present,
                    observed.accepted
                ));
            }
        }
        if g.running && g.dlss_started {
            let result = unsafe {
                native::dlss_native_present(
                    event.swapchain,
                    event.sync_interval,
                    event.flags,
                    (event.completed == 0) as u32,
                )
            };
            if event.completed != 0 {
                g.present_pairs += 1;
            }
            if result != g.present_result || (event.completed != 0 && g.present_pairs <= 2) {
                self.log(&format!(
                    "D3D11 host native Present bookkeeping result={result} completed={} pairs={}",
                    event.completed, g.present_pairs
                ));
            }
            g.present_result = result;
        }
        if g.capture_pending
            && self.dump_wants(&g.capture_record)
            && g.capture_record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID == 0
        {
            let mut window = zeroed_pass();
            window.struct_size = size_of::<Pass>() as u32;
            let (mut reached, mut passed, mut scene_path) = (0u32, 0u32, u32::MAX);
            let record = g.capture_record;
            let mut term = |bit: u32, ok: bool| {
                reached |= bit;
                if ok {
                    passed |= bit;
                }
                ok
            };
            // Preserve the original short circuit: no WINDOW or scene query is added.
            let matched = term(
                1,
                unsafe { native::window_match(event.swapchain, &mut window) } != 0,
            ) && term(2, window.session_id == record.session_id)
                && term(4, window.source_frame_id == record.frame_id)
                && term(8, window.native_frame == g.capture_native_frame)
                && term(16, window.view_key == g.capture_view)
                && term(32, {
                    scene_path = unsafe { native::scene_present_path(&window) };
                    scene_path != SCENE_PRESENT_NONE
                });
            if !matched {
                let failed = reached & !passed;
                // At most one receipt for each of the six first failing terms.
                if g.capture_refusal_terms & failed == 0 {
                    g.capture_refusal_terms |= failed;
                    self.log(&format!(
                        "D3D11 capture Present refused reached=0x{reached:x} passed=0x{passed:x} failed=0x{failed:x} capture_session={} source={} native={} view={} window_session={} source={} native={} view={} completed={} thread={event_thread} scene_path={scene_path}",
                        record.session_id,
                        record.frame_id,
                        g.capture_native_frame,
                        g.capture_view,
                        window.session_id,
                        window.source_frame_id,
                        window.native_frame,
                        window.view_key,
                        event.completed
                    ));
                }
                unsafe { native::dump_complete(&g.capture_record, NOT_READY) };
                g.capture_pending = false;
            }
        }
        if g.capture_pending && self.dump_wants(&g.capture_record) {
            if event.completed == 0 {
                let context = unsafe { com_ref::owned(native::immediate_context(g.device_ptr())) };
                let buffer = unsafe { com_ref::owned(native::chain_buffer(g.chain_ptr())) };
                if let Some(buffer) = &buffer {
                    self.dump_texture(
                        ptr_of(&context),
                        &g.capture_record,
                        c"final_color".as_ptr(),
                        c"Actual native Present colour before shared overlay",
                        buffer.as_raw(),
                        DUMP_HDR,
                    );
                }
            } else {
                unsafe { native::dump_complete(&g.capture_record, event.result) };
                g.capture_pending = false;
            }
        }
        if event.completed != 0 {
            return;
        }
        native::overlay_poll_toggle();
        unsafe { native::overlay_present(event.swapchain) };
    }

    fn cpu(&self, event: *const rsf_game_cpu_event) {
        // Installation/preparation alone does not prove the engine retained the facade.
        if self.setup.presentation_bridge != 0 && !self.bridge_bound.load(Ordering::Acquire) {
            return;
        }
        if !self.bridge() {
            match self.upstream.cpu_event {
                Some(cpu) => unsafe { cpu(self.upstream.user, event) },
                None => unsafe { native::cpu_event(event) },
            }
            return;
        }
        let accepted = unsafe { native::cpu_dispatch_fg(event) };
        // Keep startup receipts, plus bounded late stage samples and the first refusal
        // of each stage. Read no extension fields until its declared size/version allows it.
        // This host owns normalization; the same fact is not dispatched upstream twice.
        let Some(fact) = (unsafe { checked(event) }) else {
            return;
        };
        let source_event =
            (RSF_GAME_CPU_SOURCE_RESERVE..=RSF_GAME_CPU_SOURCE_ABORT).contains(&fact.stage);
        let qualified = fact.stage <= RSF_GAME_CPU_SOURCE_ABORT
            && if source_event {
                fact.event_version == RSF_GAME_CPU_EVENT_VERSION_SOURCE
            } else {
                fact.event_version <= RSF_GAME_CPU_EVENT_VERSION
            };
        if !qualified {
            return;
        }
        let initial = self.cpu_reports.fetch_add(1, Ordering::SeqCst) < 24;
        let stage_bit = 1u32 << fact.stage;
        let first_refusal = accepted == 0
            && self
                .cpu_refusal_stages
                .fetch_or(stage_bit, Ordering::SeqCst)
                & stage_bit
                == 0;
        let first_stage =
            self.cpu_stage_reports[fact.stage as usize].fetch_add(1, Ordering::SeqCst) < 4;
        // Reserve eleven of the 64 late rows for independent first-stage refusals.
        let sampled = !initial
            && !first_refusal
            && (first_stage || (fact.source_frame_id != 0 && fact.source_frame_id % 128 == 0))
            && self.cpu_sample_reports.fetch_add(1, Ordering::SeqCst) < 53;
        if !(initial || first_refusal || sampled) {
            return;
        }
        let decoded = source_event
            .then(|| unsafe { decode_source_event(event, fact) })
            .flatten();
        let packet = match &decoded {
            Some(SourcePacket::Legacy(_)) => "legacy",
            Some(SourcePacket::Full(_)) => "full",
            None if source_event => "invalid",
            None => "base",
        };
        let (native_generation, poll_begin, abort_reason) =
            decoded.as_ref().map_or((0, 0, 0), |packet| {
                let source = packet.event();
                (
                    source.native_generation,
                    source.poll_begin_qpc,
                    source.reason,
                )
            });
        self.log(&format!(
            "D3D11 CPU fact session={} source={} stage={} accepted={accepted} flags=0x{:x} version={} size={} native_generation={native_generation} poll_begin={poll_begin} abort_reason={abort_reason} packet={packet}",
            fact.session_id,
            fact.source_frame_id,
            fact.stage,
            fact.flags,
            fact.event_version,
            fact.struct_size
        ));
    }

    fn action(&self, index: u32, value: i32) -> bool {
        let Some(option) = ADVANCED.get(index as usize) else {
            return false;
        };
        if !(0..=1).contains(&value) {
            return false;
        }
        let enabled = value != 0;
        if let Some(key) = option.key {
            if !self.ini_write(key, enabled) {
                return false;
            }
        } else if !enabled {
            return false;
        }
        (option.flag)(self).store(enabled, Ordering::Release);
        if option.bump_generation {
            self.requested_generation.fetch_add(1, Ordering::SeqCst);
        }
        true
    }

    fn bind_presentation(&self, swapchain: Raw) -> i32 {
        if !self.active() {
            return NOT_READY;
        }
        let Some(mut guard) = self.try_graphics() else {
            return BUSY;
        };
        let g = &mut *guard;
        if g.configured {
            if !g.observer || g.chain_ptr() != swapchain {
                return NOT_READY;
            }
            let thread = current_thread();
            if g.execution_thread != thread {
                // Startup presentation can transfer from a loading worker to the main
                // renderer. Serialize UI work and admit that transfer before SR owns it.
                if g.running || g.scene_bound {
                    return NOT_READY;
                }
                self.log(&format!(
                    "D3D11 host presentation owner transferred {} -> {thread} on the same native chain before scene admission.",
                    g.execution_thread
                ));
                g.execution_thread = thread;
            }
            return OK;
        }
        let Some(device) = (unsafe { com_ref::owned(native::chain_device(swapchain)) }) else {
            return INVALID;
        };
        if unsafe { native::chain_window(swapchain) }.is_null() {
            return INVALID;
        }
        let context = unsafe { com_ref::owned(native::immediate_context(device.as_raw())) };
        if !self.initialize(g, ptr_of(&context), swapchain) {
            return NOT_READY;
        }
        self.log(
            "D3D11 host presentation/window bound; overlay available before scene admission, SR SDK not started.",
        );
        OK
    }

    // Outer D3D11 creation adoption. The caller's slot keeps the native chain
    // until an owned facade is proven; then the slot takes the facade's
    // reference and the caller's native reference is released.
    fn wrap_native_chain(&self, request: &rsf_d3d11_native_chain) -> i32 {
        let context = request.immediate_context;
        if !self.is_immediate(context) {
            return INVALID;
        }
        // Exact COM identity: the declared device first, then the context's device.
        let context_device = self.child_device(context);
        if !self.same(request.device, ptr_of(&context_device)) {
            return INVALID;
        }
        // SAFETY: the export admitted the request and checked both slot levels.
        let native = unsafe { *request.swapchain };
        let mut facade = null_mut();
        let result = unsafe { native::wrap_native(request.device, native, &mut facade) };
        // A set out pointer owns one facade reference even beside a failed
        // HRESULT; filtering drops (releases) it so a refusal never leaks it.
        let Some(facade) = (unsafe { com_ref::owned(facade) }).filter(|_| result >= 0) else {
            self.log(&format!(
                "D3D11 outer creation facade refused HRESULT=0x{:08x}; native outputs preserved",
                result as u32
            ));
            return NOT_READY;
        };
        if unsafe { native::present_is_owner(facade.as_raw()) } == 0 {
            drop(facade);
            self.log("D3D11 outer creation refused non-owner facade; native outputs preserved.");
            return NOT_READY;
        }
        let native_identity = unsafe { com_ref::owned(native::com_identity(native)) };
        let facade_identity = unsafe { com_ref::owned(native::com_identity(facade.as_raw())) };
        let native_unknown = ptr_of(&native_identity);
        self.log(&format!(
            "D3D11 outer creation identity native={} unknown={} Present={} facade={} unknown={} Present={} owner={}",
            msvc_pointer(native),
            msvc_pointer(native_unknown),
            msvc_pointer(unsafe { native::chain_present(native) }),
            msvc_pointer(facade.as_raw()),
            msvc_pointer(ptr_of(&facade_identity)),
            msvc_pointer(unsafe { native::chain_present(facade.as_raw()) }),
            unsafe { native::present_is_owner(facade.as_raw()) },
        ));
        // SAFETY: the slot is writable caller storage; it now owns the facade reference.
        unsafe { *request.swapchain = facade.into_raw() };
        // Drop our diagnostic identity reference before observing the caller's
        // release. A remaining count belongs to another owner, not this temporary.
        drop(native_identity);
        let remaining = unsafe { native::com_release(native) };
        self.log(&format!(
            "D3D11 outer native chain caller reference released identity={} reported_remaining={remaining}; no external references consumed",
            msvc_pointer(native_unknown)
        ));
        self.log(
            "D3D11 outer native creation returned final application facade; native creation HRESULT preserved by adapter.",
        );
        drop(facade_identity);
        OK
    }
}

// The native-chain receipts print pointers as MSVC's %p does.
fn msvc_pointer(value: Raw) -> String {
    format!("{:016X}", value as usize)
}

fn rect_size([left, top, right, bottom]: [i32; 4]) -> [u32; 2] {
    [
        right.wrapping_sub(left) as u32,
        bottom.wrapping_sub(top) as u32,
    ]
}

unsafe fn host<'a>(user: Raw) -> &'a Host {
    unsafe { &*user.cast::<Host>() }
}

unsafe extern "C" fn host_log(user: Raw, text: *const c_char) {
    let host = unsafe { host(user) };
    if let Some(log) = host.upstream.log {
        unsafe { log(host.upstream.user, text) };
    }
}

unsafe extern "C" fn host_changed(user: Raw, settings: *const rsf_render_settings) {
    let host = unsafe { host(user) };
    if let Some(settings) = unsafe { settings.as_ref() } {
        // Publish the generation before a reader can copy the new settings.
        let mut shared = host.shared();
        shared.desired = *settings;
        host.requested_generation.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe extern "C" fn host_sr_report(user: Raw, output: *mut rsf_overlay_sr_report) -> c_int {
    let host = unsafe { host(user) };
    if !unsafe { has_capacity(output) } {
        return 0;
    }
    unsafe { *output = host.shared().report };
    1
}

unsafe extern "C" fn host_cpu(user: Raw, event: *const rsf_game_cpu_event) {
    unsafe { host(user) }.cpu(event);
}

unsafe extern "C" fn host_accept_window(user: Raw, window: Raw) -> c_int {
    let host = unsafe { host(user) };
    host.setup
        .accept_window
        .is_some_and(|accept| unsafe { accept(host.setup.presentation_user, window) != 0 })
        as c_int
}

unsafe extern "C" fn host_passes(user: Raw, context: Raw, pass: *const Pass, begin: u32) {
    let host = unsafe { host(user) };
    let Some(pass) = (unsafe { checked(pass) }) else {
        return;
    };
    if begin > 1 || pass.session_id != host.upstream.session_id {
        return;
    }
    host.passes(context, pass, begin);
}

unsafe extern "C" fn host_config(user: Raw, output: *mut rsf_game_render_config) -> c_int {
    let host = unsafe { host(user) };
    if !unsafe { has_capacity(output) } {
        return 0;
    }
    let output = unsafe { &mut *output };
    let shared = host.shared();
    *output = shared.planned;
    output.enabled =
        (output.enabled != 0 && shared.desired.sr_enabled != 0 && host.active()) as u32;
    1
}

unsafe extern "C" fn host_feedback(user: Raw, event: *const rsf_game_render_feedback) {
    let host = unsafe { host(user) };
    let Some(event) = (unsafe { checked(event) }) else {
        return;
    };
    if event.session_id != host.upstream.session_id {
        return;
    }
    if event.reason != 0 {
        let mut shared = host.shared();
        shared.report.effective_backend = 0;
        set_reason(
            &mut shared.report,
            &format!("Native input admission refused (reason {})", event.reason),
        );
    }
}

unsafe extern "C" fn host_options(user: Raw, values: *mut i32, count: u32) {
    let host = unsafe { host(user) };
    if values.is_null() {
        return;
    }
    let values = unsafe { std::slice::from_raw_parts_mut(values, count as usize) };
    for (target, value) in values
        .iter_mut()
        .zip(ADVANCED.map(|option| option.value(host)))
    {
        *target = value;
    }
}

unsafe extern "C" fn host_action(user: Raw, index: u32, value: i32) -> c_int {
    unsafe { host(user) }.action(index, value) as c_int
}

unsafe extern "C" fn host_present(user: Raw, event: *const rsf_observer_present_event) {
    if let Some(event) = unsafe { event.as_ref() } {
        unsafe { host(user) }.present(event);
    }
}

unsafe extern "C" fn host_process(user: Raw, context: Raw, pass: *const Pass) -> i32 {
    let Some(pass) = (unsafe { checked(pass) }) else {
        return INVALID;
    };
    if context.is_null()
        || pass.role != RSF_GAME_RENDER_SR
        || pass.swapchain.is_null()
        || (pass.camera_valid == 0 && pass.flags & RSF_GAME_RENDER_RECONFIGURE == 0)
        || pass.camera.output_width == 0
        || pass.camera.output_height == 0
    {
        return INVALID;
    }
    unsafe { host(user) }.process(context, pass, None)
}

// Size, then version, are checked before any other field is read.
unsafe fn admitted<T>(object: *const T, version: u32) -> Result<&'static T, i32> {
    if !unsafe { has_capacity(object) } {
        return Err(INVALID);
    }
    let abi = unsafe { object.cast::<u32>().add(1).read() };
    if abi != version {
        return Err(ABI_MISMATCH);
    }
    Ok(unsafe { &*object })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_create(
    setup: *const rsf_d3d11_host_setup,
    services: *mut rsf_game_host_services,
) -> i32 {
    const LEGACY_SETUP_SIZE: usize = offset_of!(rsf_d3d11_host_setup, depth_far_plane);
    if setup.is_null() || !unsafe { has_capacity(services) } {
        return INVALID;
    }
    // The legacy prefix or the full record; nothing in between.
    let size = unsafe { setup.cast::<u32>().read() } as usize;
    if size != LEGACY_SETUP_SIZE && size < size_of::<rsf_d3d11_host_setup>() {
        return INVALID;
    }
    let mut decoded: rsf_d3d11_host_setup = unsafe { std::mem::zeroed() };
    let prefix = if size >= size_of::<rsf_d3d11_host_setup>() {
        size_of::<rsf_d3d11_host_setup>()
    } else {
        LEGACY_SETUP_SIZE
    };
    unsafe {
        ptr::copy_nonoverlapping(
            setup.cast::<u8>(),
            ptr::from_mut(&mut decoded).cast(),
            prefix,
        )
    };
    let (setup, upstream) = (&decoded, unsafe { *services });
    if setup.abi_version != RSF_D3D11_HOST_ABI_VERSION
        || upstream.abi_version != RSF_GAME_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    if setup.depth_far_plane > RSF_D3D11_HOST_DEPTH_INFINITE || !validate_setup(setup, &upstream) {
        return INVALID;
    }
    // Bridge installation can call back into the adapter before publishing the owner.
    let Ok(_creation) = CREATION.try_lock() else {
        return BUSY;
    };
    if !OWNER.load(Ordering::Acquire).is_null() {
        return BUSY;
    }
    let Some(host) = Host::new(setup, upstream) else {
        return NOT_READY;
    };
    let host = crate::allocate_handle(host);
    if host.is_null() {
        return NOT_READY;
    }
    // The host is process-lived: adapter callbacks may hold its user pointer.
    let host_ref = unsafe { &*host };
    host_ref.install_bridge();
    unsafe {
        *services = rsf_game_host_services {
            struct_size: size_of::<rsf_game_host_services>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            session_id: upstream.session_id,
            log: Some(host_log),
            render_pass: Some(host_passes),
            user: host.cast(),
            render_config: Some(host_config),
            cpu_event: Some(host_cpu),
            scope_state: None,
            process_upscale: Some(host_process),
            submit: None,
            present: None,
            process_latency: None,
            render_feedback: Some(host_feedback),
        }
    };
    OWNER.store(host, Ordering::Release);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11_host_request_capture(_: Raw) -> u32 {
    let host = OWNER.load(Ordering::Acquire);
    if host.is_null() {
        return NOT_READY as u32;
    }
    unsafe { &*host }.capture.store(true, Ordering::Release);
    OK as u32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_capture_control(
    request: *const rsf_d3d11_capture_control,
) -> i32 {
    let request = match unsafe { admitted(request, RSF_D3D11_CAPTURE_CONTROL_VERSION) } {
        Ok(request) => request,
        Err(code) => return code,
    };
    if request.value > 1
        || (request.operation != RSF_D3D11_CAPTURE_SEQUENCE
            && request.operation != RSF_D3D11_CAPTURE_FAILURES)
        || (request.operation == RSF_D3D11_CAPTURE_SEQUENCE && request.value == 0)
    {
        return INVALID;
    }
    let host = OWNER.load(Ordering::Acquire);
    if host.is_null() {
        return NOT_READY;
    }
    let index = if request.operation == RSF_D3D11_CAPTURE_SEQUENCE {
        Advanced::CaptureSequence as u32
    } else {
        Advanced::CaptureFailures as u32
    };
    if unsafe { &*host }.action(index, request.value as i32) {
        OK
    } else {
        NOT_READY
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11_host_set_active(user: Raw, active: u32) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    if active > 1 {
        return INVALID;
    }
    if host.active.swap(active != 0, Ordering::AcqRel) != (active != 0) {
        host.requested_generation.fetch_add(1, Ordering::SeqCst);
    }
    OK
}

unsafe fn dispatch_chain(host: &Host, request: *mut rsf_d3d11_chain_dispatch) -> i32 {
    if !unsafe { has_capacity(request) } {
        return INVALID;
    }
    let request = unsafe { &mut *request };
    if request.receiver.is_null() {
        return INVALID;
    }
    if request.abi_version != RSF_D3D11_HOST_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if !(RSF_D3D11_DISPATCH_RELEASE..=RSF_D3D11_DISPATCH_RESIZE_BUFFERS1).contains(&request.method)
    {
        return INVALID;
    }
    request.handled = 0;
    if host.bridge() {
        unsafe { native::chain_dispatch(request) };
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_dispatch_chain(
    user: Raw,
    request: *mut rsf_d3d11_chain_dispatch,
) -> i32 {
    match owner(user) {
        Some(host) => unsafe { dispatch_chain(host, request) },
        None => INVALID,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_dispatch_present(
    user: Raw,
    request: *mut rsf_d3d11_present_dispatch,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    if !unsafe { has_capacity(request) } {
        return INVALID;
    }
    let request = unsafe { &mut *request };
    if request.receiver.is_null() {
        return INVALID;
    }
    if request.abi_version != RSF_D3D11_HOST_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let mut call = rsf_d3d11_chain_dispatch {
        struct_size: size_of::<rsf_d3d11_chain_dispatch>() as u32,
        abi_version: request.abi_version,
        method: RSF_D3D11_DISPATCH_PRESENT,
        handled: 0,
        receiver: request.receiver,
        args: unsafe { std::mem::zeroed() },
        result: rsf_d3d11_chain_dispatch__bindgen_ty_2 { hresult: 0 },
    };
    call.args.present = rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_1 {
        sync_interval: request.sync_interval,
        flags: request.flags,
    };
    let status = unsafe { dispatch_chain(host, &mut call) };
    if status != OK {
        return status;
    }
    request.handled = call.handled;
    if call.handled != 0 {
        request.native_result = unsafe { call.result.hresult };
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_creation_begin(
    user: Raw,
    scope: *mut rsf_d3d11_creation_scope,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    if let Err(code) = unsafe { admitted(scope, RSF_D3D11_HOST_ABI_VERSION) } {
        return code;
    }
    let scope = unsafe { &mut *scope };
    if scope.token != 0 || scope.window.is_null() {
        return INVALID;
    }
    let Some(g) = host.try_graphics() else {
        return BUSY;
    };
    if host.setup.presentation_bridge != 3
        || !host.bridge()
        || g.configured
        || g.creation_outer.is_some()
        || unsafe { host_accept_window(host.user(), scope.window) } == 0
    {
        return NOT_READY;
    }
    if unsafe { native::creation_begin(scope.window, &mut scope.token) } >= 0 {
        OK
    } else {
        NOT_READY
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_creation_end(
    user: Raw,
    scope: *const rsf_d3d11_creation_scope,
    result: *const rsf_d3d11_native_chain,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    let scope = match unsafe { admitted(scope, RSF_D3D11_HOST_ABI_VERSION) } {
        Ok(scope) => scope,
        Err(code) => return code,
    };
    let result_error = match unsafe { result.as_ref() } {
        None => OK,
        Some(_) => match unsafe { admitted(result, RSF_D3D11_HOST_ABI_VERSION) } {
            Ok(_) => OK,
            Err(code) => code,
        },
    };
    if host.setup.presentation_bridge != 3 || !host.bridge() {
        return NOT_READY;
    }
    // End must always close the owning transaction, even if another callback is busy.
    let mut g = host.lock_graphics();
    if result_error != OK {
        unsafe {
            native::creation_end(
                scope.window,
                scope.token,
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        return result_error;
    }
    let result = unsafe { result.as_ref() };
    let outer = result
        .filter(|result| !result.swapchain.is_null())
        .map_or(null_mut(), |result| unsafe { *result.swapchain });
    let admitted = unsafe {
        native::creation_end(
            scope.window,
            scope.token,
            result.map_or(null_mut(), |result| result.device),
            result.map_or(null_mut(), |result| result.immediate_context),
            outer,
        )
    };
    if admitted < 0 {
        return NATIVE_REFUSED;
    }
    // Keep the explicitly verified alias alive; never replace it.
    g.creation_outer = (!outer.is_null()).then(|| unsafe { com_ref::retain(outer) });
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_wrap_native_swapchain(
    user: Raw,
    request: *const rsf_d3d11_native_chain,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    let request = match unsafe { admitted(request, RSF_D3D11_HOST_ABI_VERSION) } {
        Ok(request) => request,
        Err(code) => return code,
    };
    if request.device.is_null()
        || request.immediate_context.is_null()
        || request.swapchain.is_null()
        || unsafe { *request.swapchain }.is_null()
    {
        return INVALID;
    }
    if host.setup.presentation_bridge != 2 || !host.bridge() {
        return NOT_READY;
    }
    let Some(g) = host.try_graphics() else {
        return NOT_READY;
    };
    if g.configured || g.scene_bound {
        return NOT_READY;
    }
    // The lease `g` stays held across the adoption.
    host.wrap_native_chain(request)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_bind_presentation(
    user: Raw,
    binding: *const rsf_d3d11_presentation_binding,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    let Some(binding) =
        (unsafe { checked(binding) }).filter(|binding| !binding.swapchain.is_null())
    else {
        return INVALID;
    };
    if binding.abi_version != RSF_D3D11_HOST_ABI_VERSION {
        return ABI_MISMATCH;
    }
    host.bind_presentation(binding.swapchain)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_reduced_frame(
    user: Raw,
    context: Raw,
    request: *mut rsf_d3d11_reduced_frame,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    if let Err(code) = unsafe { admitted(request, RSF_D3D11_REDUCED_FRAME_ABI_VERSION) } {
        return code;
    }
    let request = unsafe { &mut *request };
    if request.operation > RSF_D3D11_REDUCED_CANCEL || request.reserved != 0 {
        return INVALID;
    }
    host.reduced_frame(context, request)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_upscale_auxiliaries(
    user: Raw,
    context: Raw,
    request: *const rsf_d3d11_auxiliary_pass,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    if context.is_null() {
        return INVALID;
    }
    let Some(request) = (unsafe { checked(request) }) else {
        return INVALID;
    };
    if request.abi_version != RSF_D3D11_HOST_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Some(pass) = (unsafe { checked(request.pass) }) else {
        return INVALID;
    };
    let count = request.surface_count as usize;
    if pass.role != RSF_GAME_RENDER_SR
        || pass.camera_valid == 0
        || pass.swapchain.is_null()
        || pass.flags & RSF_GAME_RENDER_RECONFIGURE != 0
        || count == 0
        || count > MAX_RESERVED
        || request.surfaces.is_null()
        || pass.session_id != host.upstream.session_id
    {
        return INVALID;
    }
    let surfaces = unsafe { std::slice::from_raw_parts(request.surfaces, count) };
    if surfaces
        .iter()
        .any(|surface| surface.struct_size < size_of::<Surface>() as u32)
    {
        return INVALID;
    }
    host.process(context, pass, Some(surfaces))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_restore_auxiliaries(
    user: Raw,
    scope: *const rsf_d3d11_auxiliary_scope,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    let scope = match unsafe { admitted(scope, RSF_D3D11_HOST_ABI_VERSION) } {
        Ok(scope) => scope,
        Err(code) => return code,
    };
    let Some(mut g) = host.try_graphics() else {
        return BUSY;
    };
    if !g.configured
        || g.execution_thread != current_thread()
        || scope.session_id != host.upstream.session_id
    {
        return NOT_READY;
    }
    if g.reduced_pending() {
        return BUSY;
    }
    // An unsuccessful upscale may already have rolled back its preparation.
    // Acknowledge that empty lease only on the verified session/graphics owner.
    let Some(products) = g.auxiliaries.as_mut().filter(|products| products.pending()) else {
        return OK;
    };
    if !products.matches([scope.session_id, scope.native_frame, scope.view_key]) {
        return NOT_READY;
    }
    if products.restore() { OK } else { NOT_READY }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_bind_sampler_bias(
    user: Raw,
    context: Raw,
    request: *mut rsf_d3d11_sampler_bias,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    if let Err(code) = unsafe { admitted(request, RSF_D3D11_HOST_ABI_VERSION) } {
        return code;
    }
    let request = unsafe { &mut *request };
    let Some(mut g) = host.try_graphics() else {
        return BUSY;
    };
    if !g.scene_bound
        || g.execution_thread != current_thread()
        || request.session_id != host.upstream.session_id
        || (!host.active() && request.slot_mask != 0)
    {
        return NOT_READY;
    }
    if g.sampler_bias.is_null() {
        g.sampler_bias = rsf_sampler_bias_create().cast();
    }
    let applied = unsafe {
        rsf_sampler_bias_apply(
            g.sampler_bias.cast(),
            context,
            request.samplers.as_ptr(),
            request.slot_mask,
            request.bias,
            &mut request.applied_mask,
        )
    };
    if applied != 0 { OK } else { NOT_READY }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_capture_texture(
    user: Raw,
    context: Raw,
    request: *const rsf_d3d11_host_capture,
) -> i32 {
    let Some(host) = owner(user) else {
        return INVALID;
    };
    if context.is_null() {
        return INVALID;
    }
    let request = match unsafe { admitted(request, RSF_D3D11_HOST_ABI_VERSION) } {
        Ok(request) => request,
        Err(code) => return code,
    };
    if request.texture.is_null()
        || request.name.is_null()
        || request.description.is_null()
        || request.view > DUMP_DENSE_MOTION
        || !request.scale.is_finite()
        || request.scale < 0.0
    {
        return INVALID;
    }
    let Some(g) = host.try_graphics() else {
        return BUSY;
    };
    // Do not touch borrowed graphics pointers before verifying this exact capture lease.
    if !host.active()
        || !g.configured
        || g.execution_thread != current_thread()
        || !g.capture_pending
        || request.session_id != g.capture_record.session_id
        || request.native_frame != g.capture_native_frame
        || request.view_key != g.capture_view
        || !host.dump_wants(&g.capture_record)
    {
        return NOT_READY;
    }
    if !host.is_immediate(context) {
        return INVALID;
    }
    if !host.on_device(context, g.device_ptr()) {
        return INVALID;
    }
    let recorded = unsafe {
        native::dump_texture(
            context,
            &g.capture_record,
            request.name,
            request.description,
            request.texture,
            request.view,
            request.scale,
        )
    };
    if recorded != 0 { OK } else { NOT_READY }
}

#[cfg(test)]
#[path = "d3d11_host_tests.rs"]
mod tests;
