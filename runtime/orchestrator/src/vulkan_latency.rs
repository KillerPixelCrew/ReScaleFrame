// SPDX-License-Identifier: MIT
use crate::latency_lease::{
    pacing_owner, rsf_native_latency_claim, rsf_native_latency_has_claim,
    rsf_native_latency_release_claim,
};
use crate::native_cpu::{rsf_native_cpu_dispatch, rsf_native_cpu_release_session};
use crate::sr_session::has_capacity;
use crate::sys::*;
use crate::sys::{RSF_FGVK_LATENCY_ABI_VERSION, rsf_fgvk, rsf_fgvk_latency_status};
use crate::sys::{RSF_PACING_NONE, RSF_PACING_REFLEX, RSF_PACING_XELL};
use crate::vulkan_antilag::*;
use crate::vulkan_latency_frames::{streamline::*, *};
use crate::vulkan_pcl::{
    self as pcl, rsf_core_latencyvk_pcl_bind as bind_pcl, rsf_core_latencyvk_pcl_reset,
    rsf_core_latencyvk_pcl_status,
};
use crate::vulkan_presentation::*;
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INVALID_ARGUMENT, LOAD_FAILED, NOT_READY, NOT_SUPPORTED, OK,
};
// The Rust definitions, not the header declarations crate::sys also provides.
use crate::vulkan_presentation::{
    rsf_core_fgvk_presentation_attach, rsf_core_fgvk_presentation_shutdown_begin,
    rsf_core_fgvk_presentation_shutdown_end, rsf_core_fgvk_presentation_unbind,
};
use crate::vulkan_timing::{timing_begin, timing_end};
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::Mutex;

use crate::sys::{rsf_slvk_callback, rsf_vk_function, rsf_vk_wsi_call};

/// The Streamline frame-token calls of the latency owner.
pub struct TokenCalls {
    pub begin: Option<unsafe extern "C" fn(u32, *mut usize) -> i32>,
    pub current: Option<unsafe extern "C" fn() -> usize>,
    pub matches: Option<unsafe extern "C" fn(usize, u32) -> i32>,
    pub mark: Option<unsafe extern "C" fn(usize, u32) -> i32>,
}

/// The Streamline PCL and Reflex-state calls.
pub struct PclCalls {
    pub bind: Option<unsafe extern "C" fn() -> i32>,
    pub message: Option<unsafe extern "C" fn() -> u32>,
    pub bind_reflex: Option<unsafe extern "C" fn() -> i32>,
    pub reflex: Option<unsafe extern "C" fn(*mut u32, *mut u32) -> i32>,
}

/// The Streamline Vulkan interposer calls (rsf-streamline-vk's bridge exports).
pub struct Streamline {
    pub initialize: Option<unsafe extern "C" fn(*const u16, *const c_char, u32, u32, u32) -> i32>,
    pub shutdown: Option<unsafe extern "C" fn() -> i32>,
    pub module: Option<unsafe extern "C" fn() -> *mut c_void>,
    pub invoke:
        Option<unsafe extern "C" fn(rsf_slvk_callback, *mut c_void, usize, *mut c_void) -> i32>,
    pub set_interval: Option<unsafe extern "C" fn(i32, u32) -> i32>,
    pub sleep: Option<unsafe extern "C" fn() -> i32>,
    pub tokens: TokenCalls,
    pub pcl: PclCalls,
}

/// What the latency owner calls: Streamline when the build has it, and the FG provider's latency
/// entries. PLATFORM is the production table; the latency fixtures build their own.
pub struct Platform {
    pub streamline: *const Streamline,
    pub backend: Option<unsafe extern "C" fn(*const rsf_fgvk, *mut u32) -> i32>,
    pub options: Option<unsafe extern "C" fn(*mut rsf_fgvk, u32, u32) -> i32>,
    pub sleep: Option<unsafe extern "C" fn(*mut rsf_fgvk, u64) -> i32>,
    pub marker: Option<unsafe extern "C" fn(*mut rsf_fgvk, u64, u32) -> i32>,
    pub status: Option<unsafe extern "C" fn(*mut rsf_fgvk, *mut rsf_fgvk_latency_status) -> i32>,
}

// SAFETY: the tables hold only function addresses and a pointer to a static table.
unsafe impl Sync for Platform {}
const AMD: u32 = 4;
const XE_SS_FG: u32 = 5;
const CLIENT: u32 = 2;

#[derive(Clone, Copy, Default)]
struct Stats {
    mode: u32,
    sleep_calls: u64,
    marker_calls: u64,
    refused_calls: u64,
    last_result: i32,
}

static STATS: Mutex<Stats> = Mutex::new(Stats {
    mode: 0,
    sleep_calls: 0,
    marker_calls: 0,
    refused_calls: 0,
    last_result: 0,
});

// SDK leaf calls report their raw results. This lock is independent of the
// SDK session guard and never spans a foreign call.
pub(super) fn rsf_core_latencyvk_sdk_result(result: i32, marker: u32) {
    let mut stats = STATS.lock().unwrap();
    stats.last_result = result;
    stats.marker_calls += u64::from(result == OK && marker != 0);
    stats.refused_calls += u64::from(result != OK);
}

struct Host {
    initialized: bool,
    closing: bool,
    owner: u32,
    // The last provider to acquire the process lease. A refused selection may
    // clear the current owner while leaving that provider's lease outstanding.
    claimed_owner: u32,
    requested: u32,
    limit: u32,
    facts_session: u64,
}

static HOST: Mutex<Host> = Mutex::new(Host {
    initialized: false,
    closing: false,
    owner: RSF_PACING_NONE,
    claimed_owner: RSF_PACING_NONE,
    requested: 0,
    limit: 0,
    facts_session: 0,
});

impl PresentationLease {
    fn xell(&self, platform: &Platform) -> bool {
        let mut backend = 0;
        !self.context.is_null()
            && unsafe { (platform.backend.unwrap())(self.context, &mut backend) } == OK
            && backend == XE_SS_FG
    }

    fn mark(&self, platform: &Platform, owner: u32, id: u64, kind: u32) {
        if owner == RSF_PACING_XELL && !self.context.is_null() {
            self.record_latency_result(unsafe {
                (platform.marker.unwrap())(self.context, id, kind)
            });
        }
    }
}

struct Shutdown {
    finish: extern "C" fn(u32),
    commit: bool,
}

impl Shutdown {
    fn enter(begin: extern "C" fn() -> i32, finish: extern "C" fn(u32)) -> Option<Self> {
        if begin() != OK {
            return None;
        }
        Some(Self {
            finish,
            commit: false,
        })
    }
}

impl Drop for Shutdown {
    fn drop(&mut self) {
        (self.finish)(u32::from(self.commit));
    }
}

impl Host {
    fn sdk<'a>(&self, platform: &'a Platform) -> Option<&'a Streamline> {
        self.initialized
            .then_some(unsafe { platform.streamline.as_ref() })
            .flatten()
    }

    fn choose(&mut self, platform: &Platform, lease: &PresentationLease) -> u32 {
        let mut owner = RSF_PACING_NONE;
        if !self.closing && !lease.closing {
            let reflex = self.sdk(platform).is_some_and(|sdk| {
                bind_pcl(0, &sdk.pcl) != 0 && rsf_core_latencyvk_pcl_status().available != 0
            });
            let amd = rsf_core_amd_vk_status().available;
            let required = if lease.xell(platform) {
                RSF_PACING_XELL
            } else {
                RSF_PACING_NONE
            };
            owner = pacing_owner(required, reflex && amd == 0, amd != 0, AMD);
            if owner != RSF_PACING_NONE {
                if rsf_native_latency_claim(CLIENT) != 0 {
                    self.claimed_owner = owner;
                } else {
                    owner = RSF_PACING_NONE;
                }
            }
        }
        self.owner = owner;
        owner
    }

    fn mark(&self, platform: &Platform, session: u64, frame: u64, kind: u32) {
        if let Some(sdk) = self.sdk(platform)
            && rsf_core_latencyvk_pcl_status().message != 0
        {
            rsf_core_latencyvk_token_mark(session, frame, kind, &sdk.tokens);
        }
    }
}

// Called after AMD calls drain, with no AMD lock held and new AMD admission closed.
pub(crate) fn amd_device_retired() {
    let mut host = HOST.lock().unwrap();
    if host.owner == AMD {
        host.owner = RSF_PACING_NONE;
    }
    if host.claimed_owner == AMD && rsf_core_latencyvk_token_pacing() == 0 {
        rsf_native_latency_release_claim(CLIENT);
        host.claimed_owner = RSF_PACING_NONE;
    }
}

fn initialize(
    platform: &Platform,
    directory: *const c_char,
    project: *const c_char,
    features: u32,
) -> i32 {
    if directory.is_null()
        || project.is_null()
        || features & !(RSF_SLVK_DLSS_SR | RSF_SLVK_DLSS_FG) != 0
    {
        return INVALID_ARGUMENT;
    }
    let mut host = HOST.lock().unwrap();
    if host.initialized || host.closing {
        return NOT_READY;
    }
    let Some(sdk) = (unsafe { platform.streamline.as_ref() }) else {
        return NOT_SUPPORTED;
    };
    let Ok(directory) = unsafe { CStr::from_ptr(directory) }.to_str() else {
        return INVALID_ARGUMENT;
    };
    let mut wide = Vec::new();
    if wide.try_reserve_exact(directory.len() + 1).is_err() {
        return FEATURE_FAILED;
    }
    wide.extend(directory.encode_utf16());
    wide.push(0);
    // The bridge enters the Rust lifecycle directly. Its foreign SDK calls
    // already contain exceptions at the native call site.
    let result = unsafe {
        (sdk.initialize.unwrap())(
            wide.as_ptr(),
            project,
            1,
            u32::from(features & RSF_SLVK_DLSS_FG != 0),
            u32::from(features & RSF_SLVK_DLSS_SR != 0),
        )
    };
    if result != OK {
        rsf_core_latencyvk_sdk_result(result, 0);
        return LOAD_FAILED;
    }
    rsf_core_latencyvk_pcl_reset();
    rsf_core_latencyvk_frames_reset();
    *STATS.lock().unwrap() = Stats::default();
    host.requested = 0;
    host.limit = 0;
    host.initialized = true;
    OK
}

/// # Safety
///
/// `setup` is null or a size-prefixed record of its stated size.
pub unsafe fn rsf_core_latencyvk_initialize(
    platform: &Platform,
    setup: *const rsf_latencyvk_setup,
) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != RSF_LATENCYVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    initialize(platform, setup.runtime_directory_utf8, setup.project_id, 0)
}

/// # Safety
///
/// `setup` is null or a size-prefixed record of its stated size.
pub unsafe fn rsf_core_slvk_initialize(platform: &Platform, setup: *const rsf_slvk_setup) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != RSF_SLVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    initialize(
        platform,
        setup.runtime_directory_utf8,
        setup.project_id,
        setup.features,
    )
}

/// # Safety
///
/// `call` is null or a size-prefixed record of its stated size.
pub unsafe fn rsf_core_slvk_invoke(platform: &Platform, call: *const rsf_slvk_call) -> i32 {
    if !unsafe { has_capacity(call) } {
        return INVALID_ARGUMENT;
    }
    let request = unsafe { &*call };
    if request.abi_version != RSF_SLVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Some(callback) = request.callback else {
        return INVALID_ARGUMENT;
    };
    if (request.session_id == 0) != (request.frame_id == 0) {
        return INVALID_ARGUMENT;
    }
    let host = HOST.lock().unwrap();
    let Some(sdk) = host.sdk(platform) else {
        return NOT_READY;
    };
    let token = if request.frame_id == 0 {
        0
    } else {
        let token =
            rsf_core_latencyvk_token_lookup(request.session_id, request.frame_id, &sdk.tokens);
        if token == 0 {
            return NOT_READY;
        }
        token
    };
    let module = unsafe { (sdk.module.unwrap())() };
    if module.is_null() {
        return NOT_READY;
    }
    unsafe { (sdk.invoke.unwrap())(Some(callback), module, token, request.user) }
}

fn options(platform: &Platform, settings: Option<(u32, u32)>) -> i32 {
    let lease = PresentationLease::borrow();
    let (owner, requested, limit) = {
        let mut host = HOST.lock().unwrap();
        if host.closing {
            return NOT_READY;
        }
        if let Some((requested, limit)) = settings {
            host.requested = requested;
            host.limit = limit;
        }
        (host.choose(platform, &lease), host.requested, host.limit)
    };
    if owner == RSF_PACING_XELL {
        lease.record_latency_result(unsafe { (platform.options.unwrap())(lease.context, 1, limit) })
    } else if owner == RSF_PACING_NONE && (requested != 0 || limit != 0) {
        NOT_SUPPORTED
    } else {
        OK
    }
}

pub fn rsf_core_latencyvk_options(platform: &Platform, requested: u32, limit: u32) -> i32 {
    if requested > 2 {
        return INVALID_ARGUMENT;
    }
    options(platform, Some((requested, limit)))
}

pub fn rsf_core_latencyvk_presentation(platform: &Platform, context: *mut rsf_fgvk) -> i32 {
    if context.is_null() {
        return rsf_core_fgvk_presentation_unbind();
    }
    let result = rsf_core_fgvk_presentation_attach(context);
    if result != OK {
        return result;
    }
    options(platform, None)
}

pub fn rsf_core_latencyvk_input(
    platform: &Platform,
    session: u64,
    frame: u64,
    kind: u32,
    message: u32,
) -> i32 {
    if platform.streamline.is_null() {
        return NOT_SUPPORTED;
    }
    let host = HOST.lock().unwrap();
    let Some(sdk) = host.sdk(platform) else {
        return NOT_READY;
    };
    let id = rsf_core_latencyvk_pcl_status().message;
    if id == 0 {
        return NOT_READY;
    }
    rsf_core_latencyvk_token_input(session, frame, kind, message, id, &sdk.tokens)
}

unsafe extern "C" fn fact(user: *mut c_void, event: *const rsf_game_cpu_event) {
    if !unsafe { has_capacity(event) } {
        return;
    }
    let event = unsafe { &*event };
    let (id, session, stage) = (event.source_frame_id, event.session_id, event.stage);
    if id == 0 || session == 0 {
        return;
    }
    if !crate::native_cpu::observed_boundary(event) {
        return;
    }
    let platform = unsafe { &*user.cast::<Platform>() };
    if stage == RSF_GAME_CPU_INPUT_EVENT {
        rsf_core_latencyvk_input(platform, session, id, event.input_kind, event.message_id);
        return;
    }
    let lease = PresentationLease::borrow();
    let keyed = crate::native_cpu::input_keyed(event);
    let pacing = crate::native_cpu::pacing_boundary(event);
    let (owner, requested, limit) = {
        let mut host = HOST.lock().unwrap();
        if host.closing {
            return;
        }
        if keyed
            && matches!(stage, RSF_GAME_CPU_FRAME_BEGIN | RSF_GAME_CPU_INPUT_SAMPLE)
            && let Some(sdk) = host.sdk(platform)
            && bind_pcl(1, &sdk.pcl) == 0
        {
            return;
        }
        if stage == RSF_GAME_CPU_FRAME_BEGIN {
            host.facts_session = session;
            rsf_core_amd_vk_begin(session, id);
        }
        let owner = if pacing {
            host.choose(platform, &lease)
        } else {
            host.owner
        };
        (owner, host.requested, host.limit)
    };
    // No host mutex spans any provider's pacing call.
    if pacing && rsf_native_latency_has_claim(CLIENT) != 0 {
        if owner == AMD {
            rsf_core_amd_vk_options(requested, limit);
            rsf_core_amd_vk_frame(session, id, 0);
        } else {
            rsf_core_amd_vk_options(0, 0);
        }
        if owner == RSF_PACING_XELL
            && !lease.context.is_null()
            && lease.record_latency_result(unsafe {
                (platform.options.unwrap())(lease.context, 1, limit)
            }) == OK
        {
            lease.record_latency_result(unsafe { (platform.sleep.unwrap())(lease.context, id) });
        }
    }
    let marker = match stage {
        RSF_GAME_CPU_SIMULATION_BEGIN => Some(0),
        RSF_GAME_CPU_SIMULATION_END => Some(1),
        RSF_GAME_CPU_INPUT_SAMPLE => Some(6),
        _ => None,
    };
    if let Some(kind) = marker {
        lease.mark(platform, owner, id, kind);
    }

    let skipped = stage == RSF_GAME_CPU_FRAME_END
        && (event.flags & RSF_GAME_CPU_RENDER_SKIPPED != 0
            || (event.render_expected == 0 && event.flags & RSF_GAME_CPU_DERIVED_BOUNDARY == 0));
    if skipped {
        rsf_core_amd_vk_cancel(session, id);
    }
    let host = HOST.lock().unwrap();
    if host.closing {
        return;
    }
    if stage == RSF_GAME_CPU_FRAME_BEGIN || skipped {
        rsf_core_latencyvk_frame_boundary(session, id, u32::from(skipped));
    }
    let Some(sdk) = host.sdk(platform).filter(|sdk| bind_pcl(0, &sdk.pcl) != 0) else {
        return;
    };
    if stage == RSF_GAME_CPU_FRAME_BEGIN {
        rsf_core_latencyvk_token_begin(session, id, &sdk.tokens);
    } else if pacing
        && owner != RSF_PACING_NONE
        && host.owner == owner
        && rsf_native_latency_has_claim(CLIENT) != 0
        && rsf_core_latencyvk_token_pace_begin(session, id, &sdk.tokens) != 0
    {
        drop(host);
        let reflex = owner == RSF_PACING_REFLEX;
        let started = timing_begin();
        let options_started = timing_begin();
        let options = unsafe {
            (sdk.set_interval.unwrap())(
                if reflex { requested as i32 } else { 0 },
                if reflex { limit } else { 0 },
            )
        };
        timing_end("reflex_options", id, options_started);
        let mut slept = OK;
        if options == OK && reflex {
            let sleep_started = timing_begin();
            slept = unsafe { (sdk.sleep.unwrap())() };
            timing_end("reflex_sleep", id, sleep_started);
        }
        timing_end("cpu_pacing_total", id, started);
        let _host = HOST.lock().unwrap();
        rsf_core_latencyvk_token_pace_end();
        rsf_core_latencyvk_sdk_result(options, 0);
        if options == OK {
            rsf_core_latencyvk_sdk_result(slept, 0);
            if slept == OK {
                let mut stats = STATS.lock().unwrap();
                stats.mode = if reflex { requested } else { 0 };
                stats.sleep_calls += u64::from(reflex);
            }
        }
    } else if let Some(kind @ (0 | 1)) = marker {
        host.mark(platform, session, id, kind);
    }
}

/// # Safety
///
/// `event` is null or a size-prefixed CPU event of its stated size.
pub unsafe fn rsf_core_latencyvk_cpu(platform: &Platform, event: *const rsf_game_cpu_event) {
    if !unsafe { has_capacity(event) } {
        return;
    }
    let user = ptr::from_ref(platform).cast_mut().cast();
    let version = unsafe { (*event).event_version };
    if matches!(
        version,
        RSF_GAME_CPU_EVENT_VERSION
            | RSF_GAME_CPU_EVENT_VERSION_SOURCE
            | RSF_GAME_CPU_EVENT_VERSION_RECEIPT
    ) {
        if unsafe { rsf_native_cpu_dispatch(event, Some(fact), user) } != 0
            && version == RSF_GAME_CPU_EVENT_VERSION
        {
            pcl::pump(unsafe { &*event }, |ping| unsafe {
                rsf_native_cpu_dispatch(ping, Some(fact), user);
            });
        }
    } else if version == 0 {
        unsafe { fact(user, event) };
    }
}

pub fn rsf_core_latencyvk_submit_end(platform: &Platform, frame: u64) {
    let lease = PresentationLease::borrow();
    let owner = HOST.lock().unwrap().owner;
    lease.mark(platform, owner, frame, 3);
    if owner == AMD {
        rsf_core_amd_vk_frame(0, frame, 1);
    }
}

pub fn rsf_core_latencyvk_render(platform: &Platform, frame: u64) {
    let lease = PresentationLease::borrow();
    let owner = HOST.lock().unwrap().owner;
    lease.mark(platform, owner, frame, 2);
    let host = HOST.lock().unwrap();
    rsf_core_latencyvk_legacy_render(frame);
    host.mark(platform, 0, frame, 2);
}

pub fn rsf_core_latencyvk_present(platform: &Platform, begin: u32) {
    let (owner, frame) = {
        let host = HOST.lock().unwrap();
        let frame = rsf_core_latencyvk_legacy_present(begin);
        host.mark(platform, 0, frame, if begin != 0 { 4 } else { 5 });
        (host.owner, frame)
    };
    if begin != 0 && frame != 0 && owner == AMD {
        rsf_core_amd_vk_frame(0, frame, 1);
    }
}

/// # Safety
///
/// `pass` is null or a valid render pass record for this call.
pub unsafe fn rsf_core_latencyvk_submit(
    platform: &Platform,
    pass: *const rsf_game_render_pass,
    begin: u32,
) -> i32 {
    let lease = PresentationLease::borrow();
    let owner;
    let kind;
    let frame;
    let session;
    {
        let host = HOST.lock().unwrap();
        let admitted = unsafe { rsf_core_latencyvk_execution(pass, begin) };
        if admitted <= 0 {
            return admitted;
        }
        frame = unsafe { (*pass).source_frame_id };
        session = unsafe { (*pass).session_id };
        kind = admitted as u32;
        host.mark(platform, session, frame, kind);
        owner = host.owner;
    }
    lease.mark(platform, owner, frame, kind);
    if kind == 4 && owner == AMD {
        rsf_core_amd_vk_frame(session, frame, 1);
    }
    OK
}

/// # Safety
///
/// `output` is null or a writable size-prefixed status record.
pub unsafe fn rsf_core_latencyvk_get_status(
    platform: &Platform,
    output: *mut rsf_latencyvk_status,
) -> i32 {
    if !unsafe { has_capacity(output) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*output).abi_version } != RSF_LATENCYVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let lease = PresentationLease::borrow();
    let amd = rsf_core_amd_vk_status();
    let (mut result, ready) = {
        let host = HOST.lock().unwrap();
        let stats = *STATS.lock().unwrap();
        let pcl = rsf_core_latencyvk_pcl_status();
        let ready = host.initialized || lease.xell(platform) || amd.available != 0;
        (
            rsf_latencyvk_status {
                struct_size: size_of::<rsf_latencyvk_status>() as u32,
                abi_version: RSF_LATENCYVK_ABI_VERSION,
                low_latency_available: if host.owner != RSF_PACING_NONE {
                    pcl.available
                } else {
                    0
                },
                report_available: pcl.reports,
                pcl_message: pcl.message,
                mode: if host.owner != RSF_PACING_NONE {
                    stats.mode
                } else {
                    0
                },
                sleep_calls: stats.sleep_calls,
                marker_calls: stats.marker_calls,
                refused_calls: stats.refused_calls,
                last_result: stats.last_result,
                owner: host.owner,
            },
            ready,
        )
    };
    if result.owner == RSF_PACING_XELL {
        let mut status = rsf_fgvk_latency_status {
            struct_size: size_of::<rsf_fgvk_latency_status>() as u32,
            abi_version: RSF_FGVK_LATENCY_ABI_VERSION,
            mode: 0,
            available: 0,
            sleep_calls: 0,
            marker_calls: 0,
            result: 0,
        };
        let query = if lease.context.is_null() {
            NOT_READY
        } else {
            unsafe { (platform.status.unwrap())(lease.context, &mut status) }
        };
        let (last, refused) = lease.latency_outcome();
        result.mode = 0;
        result.low_latency_available = 0;
        result.sleep_calls = 0;
        result.marker_calls = 0;
        if query == OK {
            result.mode = status.mode;
            result.low_latency_available = status.available;
            result.sleep_calls = status.sleep_calls;
            result.marker_calls = status.marker_calls;
        }
        result.refused_calls = refused;
        result.last_result = if query == OK {
            last.filter(|result| *result != OK).unwrap_or(status.result)
        } else {
            query
        };
    }
    if result.owner == AMD {
        result.mode = amd.enabled;
        result.low_latency_available = amd.available;
        result.sleep_calls = amd.input_updates;
        result.marker_calls += amd.input_updates + amd.present_updates;
    }
    unsafe { output.write(result) };
    if ready { OK } else { NOT_READY }
}

pub fn rsf_core_latencyvk_shutdown(platform: &Platform) -> i32 {
    let Some(mut presentation) = Shutdown::enter(
        rsf_core_fgvk_presentation_shutdown_begin,
        rsf_core_fgvk_presentation_shutdown_end,
    ) else {
        return NOT_READY;
    };
    let Some(mut amd) =
        Shutdown::enter(rsf_core_amd_vk_shutdown_begin, rsf_core_amd_vk_shutdown_end)
    else {
        return NOT_READY;
    };
    {
        let mut host = HOST.lock().unwrap();
        if host.closing || rsf_core_latencyvk_token_pacing() != 0 {
            return NOT_READY;
        }
        if let Some(sdk) = host.sdk(platform) {
            let result = unsafe { (sdk.shutdown.unwrap())() };
            rsf_core_latencyvk_sdk_result(result, 0);
            if result != OK {
                return FEATURE_FAILED;
            }
        }
        host.closing = true;
        host.initialized = false;
        host.owner = RSF_PACING_NONE;
        rsf_native_cpu_release_session(std::mem::take(&mut host.facts_session));
        rsf_core_latencyvk_pcl_reset();
        rsf_core_latencyvk_frames_reset();
    }
    amd.commit = true;
    drop(amd);
    presentation.commit = true;
    drop(presentation);
    let mut host = HOST.lock().unwrap();
    rsf_native_latency_release_claim(CLIENT);
    host.claimed_owner = RSF_PACING_NONE;
    host.closing = false;
    OK
}

// ---- Production table and the public rsf_latencyvk_*/rsf_slvk_* entry points ----

/// rsf-streamline-vk's bridge exports (the VulkanStoryStreamline.dll ABI, sl.def), linked from the
/// same Runtime archive. The streamline-vk feature is CMake's verified corrected Streamline runtime.
#[cfg(feature = "streamline-vk")]
mod streamline_bridge {
    use super::*;
    use crate::vulkan_fg_wsi::{SynchronizedPresent, WsiScope};

    unsafe extern "C" {
        fn rsf_vk_sl_initialize_features(
            path: *const u16,
            application: *const c_char,
            features: u32,
            flags: u32,
            reflex: u32,
        ) -> i32;
        fn rsf_vk_sl_module() -> *mut c_void;
        fn rsf_vk_sl_shutdown_checked() -> i32;
        fn rsf_vk_sl_get_instance_proc_addr(
            instance: *mut c_void,
            name: *const c_char,
        ) -> rsf_vk_function;
        fn rsf_vk_sl_bind_reflex() -> i32;
        fn rsf_vk_sl_bind_pcl() -> i32;
        fn rsf_vk_sl_pcl_window_message() -> u32;
        fn rsf_vk_sl_get_reflex_state(available: *mut u32, reports: *mut u32) -> i32;
        fn rsf_vk_sl_set_reflex_interval(mode: i32, interval: u32) -> i32;
        fn rsf_vk_sl_begin_frame_checked(index: u32, token: *mut usize) -> i32;
        fn rsf_vk_sl_current_frame_token() -> usize;
        fn rsf_vk_sl_token_matches(token: usize, index: u32) -> i32;
        fn rsf_vk_sl_reflex_sleep() -> i32;
        fn rsf_vk_sl_marker_for_token(token: usize, marker: u32) -> i32;
        fn rsf_vk_sl_queue_present_synchronized(
            token: usize,
            queue: *mut c_void,
            info: *const c_void,
        ) -> i32;
        fn rsf_vk_sl_vulkan_wsi_call(callback: rsf_vk_wsi_call, user: *mut c_void) -> i32;
        fn rsf_vk_sl_without_dlss_g(callback: rsf_vk_wsi_call, user: *mut c_void) -> i32;
    }

    /// A borrowed callback can enter foreign C++ code; a C++ exception becomes FEATURE_FAILED.
    unsafe extern "C" fn invoke_checked(
        callback: rsf_slvk_callback,
        module: *mut c_void,
        token: usize,
        user: *mut c_void,
    ) -> i32 {
        let Some(callback) = callback else {
            return FEATURE_FAILED;
        };
        // The same callback, called with the C-unwind ABI so the guard can contain the exception.
        let callback = unsafe {
            std::mem::transmute::<
                unsafe extern "C" fn(*mut c_void, usize, *mut c_void) -> i32,
                unsafe extern "C-unwind" fn(*mut c_void, usize, *mut c_void) -> i32,
            >(callback)
        };
        rsf_streamline_vk::sdk_api::guarded_or(FEATURE_FAILED, || unsafe {
            callback(module, token, user)
        })
    }

    pub(super) static CALLS: Streamline = Streamline {
        initialize: Some(rsf_vk_sl_initialize_features),
        shutdown: Some(rsf_vk_sl_shutdown_checked),
        module: Some(rsf_vk_sl_module),
        invoke: Some(invoke_checked),
        set_interval: Some(rsf_vk_sl_set_reflex_interval),
        sleep: Some(rsf_vk_sl_reflex_sleep),
        tokens: TokenCalls {
            begin: Some(rsf_vk_sl_begin_frame_checked),
            current: Some(rsf_vk_sl_current_frame_token),
            matches: Some(rsf_vk_sl_token_matches),
            mark: Some(rsf_vk_sl_marker_for_token),
        },
        pcl: PclCalls {
            bind: Some(rsf_vk_sl_bind_pcl),
            message: Some(rsf_vk_sl_pcl_window_message),
            bind_reflex: Some(rsf_vk_sl_bind_reflex),
            reflex: Some(rsf_vk_sl_get_reflex_state),
        },
    };
    pub(super) const RESOLVE: rsf_vk_get_proc = Some(rsf_vk_sl_get_instance_proc_addr);
    pub(super) const PRESENT: SynchronizedPresent = Some(rsf_vk_sl_queue_present_synchronized);
    pub(super) const WSI: WsiScope = Some(rsf_vk_sl_vulkan_wsi_call);
    pub(super) const WITHOUT_DLSS_G: WsiScope = Some(rsf_vk_sl_without_dlss_g);
}

#[cfg(not(feature = "streamline-vk"))]
mod streamline_bridge {
    use super::*;
    use crate::vulkan_fg_wsi::{SynchronizedPresent, WsiScope};
    pub(super) const RESOLVE: rsf_vk_get_proc = None;
    pub(super) const PRESENT: SynchronizedPresent = None;
    pub(super) const WSI: WsiScope = None;
    pub(super) const WITHOUT_DLSS_G: WsiScope = None;
}

static PLATFORM: Platform = Platform {
    #[cfg(feature = "streamline-vk")]
    streamline: &streamline_bridge::CALLS,
    #[cfg(not(feature = "streamline-vk"))]
    streamline: ptr::null(),
    backend: Some(crate::vulkan_fg::rsf_fgvk_backend),
    options: Some(crate::vulkan_fg::rsf_fgvk_latency_options),
    sleep: Some(crate::vulkan_fg::rsf_fgvk_latency_sleep),
    marker: Some(crate::vulkan_fg::rsf_fgvk_latency_marker),
    status: Some(crate::vulkan_fg::rsf_fgvk_latency_get_status),
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_latencyvk_initialize(setup: *const rsf_latencyvk_setup) -> i32 {
    unsafe { rsf_core_latencyvk_initialize(&PLATFORM, setup) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_initialize(setup: *const rsf_slvk_setup) -> i32 {
    unsafe { rsf_core_slvk_initialize(&PLATFORM, setup) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_slvk_invoke(call: *const rsf_slvk_call) -> i32 {
    unsafe { rsf_core_slvk_invoke(&PLATFORM, call) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_options(mode: u32, limit: u32) -> i32 {
    rsf_core_latencyvk_options(&PLATFORM, mode, limit)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_presentation(context: *mut c_void) -> i32 {
    rsf_core_latencyvk_presentation(&PLATFORM, context.cast())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_submit_end(frame: u64) {
    rsf_core_latencyvk_submit_end(&PLATFORM, frame);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_ping(window: *mut c_void) -> i32 {
    pcl::rsf_core_latencyvk_pcl_ping(window)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_latencyvk_cpu(event: *const rsf_game_cpu_event) {
    unsafe { rsf_core_latencyvk_cpu(&PLATFORM, event) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_input(frame: u64, kind: u32, message: u32) -> i32 {
    rsf_core_latencyvk_input(&PLATFORM, 0, frame, kind, message)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_render(frame: u64) {
    rsf_core_latencyvk_render(&PLATFORM, frame);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_present(begin: u32) {
    rsf_core_latencyvk_present(&PLATFORM, begin);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_latencyvk_execution(pass: *const rsf_game_render_pass, begin: u32) -> i32 {
    unsafe { rsf_core_latencyvk_submit(&PLATFORM, pass, begin) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_latencyvk_get_status(status: *mut rsf_latencyvk_status) -> i32 {
    unsafe { rsf_core_latencyvk_get_status(&PLATFORM, status) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_shutdown() -> i32 {
    rsf_core_latencyvk_shutdown(&PLATFORM)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_slvk_without_dlssg(
    callback: rsf_vk_wsi_call,
    user: *mut c_void,
) -> i32 {
    unsafe {
        crate::vulkan_fg_wsi::rsf_core_slvk_without_dlssg(
            streamline_bridge::WITHOUT_DLSS_G,
            callback,
            user,
        )
    }
}

/// The resolver stays usable after slShutdown for Vulkan object destruction.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_latencyvk_instance_proc(
    instance: *mut c_void,
    name: *const c_char,
) -> rsf_vk_function {
    unsafe {
        crate::vulkan_fg_wsi::rsf_core_latencyvk_instance_proc(
            streamline_bridge::RESOLVE,
            instance,
            name,
        )
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_latencyvk_queue_present(
    callback: rsf_vk_function,
    queue: *mut c_void,
    info: *const c_void,
) -> i32 {
    unsafe {
        crate::vulkan_fg_wsi::rsf_core_latencyvk_queue_present(
            streamline_bridge::PRESENT,
            callback,
            queue,
            info,
        )
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_latencyvk_wsi_call(
    callback: rsf_vk_wsi_call,
    user: *mut c_void,
) -> i32 {
    unsafe {
        crate::vulkan_fg_wsi::rsf_core_latencyvk_wsi_call(streamline_bridge::WSI, callback, user)
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_cpu_service() -> *const rsf_cpu_session_service {
    static SERVICE: rsf_cpu_session_service = rsf_cpu_session_service {
        struct_size: size_of::<rsf_cpu_session_service>() as u32,
        version: RSF_CPU_SESSION_SERVICE_VERSION,
        report: Some(rsf_latencyvk_cpu),
        ping: Some(rsf_latencyvk_ping),
        execution: Some(rsf_latencyvk_execution),
    };
    &SERVICE
}

// The AMD Anti-Lag 2 device entries; the antilag-vk feature is CMake's Vulkan header detection.
const ANTILAG: u32 = cfg!(feature = "antilag-vk") as u32;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_latencyvk_require_device(
    query: *const rsf_latencyvk_device_query,
    out: *mut rsf_latencyvk_device_requirements,
) -> i32 {
    unsafe { rsf_core_amd_vk_require(ANTILAG, query, out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_latencyvk_device_created(
    facts: *const rsf_latencyvk_created_device,
) -> i32 {
    unsafe { rsf_core_amd_vk_device_created(ANTILAG, facts) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_latencyvk_device_destroyed(device: *mut c_void) {
    rsf_core_amd_vk_device_destroyed(device);
}
