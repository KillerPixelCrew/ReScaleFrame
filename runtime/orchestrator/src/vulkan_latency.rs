// SPDX-License-Identifier: MIT
use crate::latency_lease::{
    rsf_core_latency_pacing_owner, rsf_native_latency_claim, rsf_native_latency_has_claim,
};
use crate::native_cpu::rsf_native_cpu_release_session;
use crate::render_abi::{RSF_PACING_NONE, RSF_PACING_REFLEX, RSF_PACING_XELL};
use crate::sr_session::has_capacity;
use crate::vulkan_antilag::*;
use crate::vulkan_fg_abi::{RSF_FGVK_LATENCY_ABI_VERSION, rsf_fgvk, rsf_fgvk_latency_status};
use crate::vulkan_latency_abi::*;
use crate::vulkan_latency_frames::{streamline::*, *};
use crate::vulkan_pcl::{
    self as pcl, Sdk as PclSdk, rsf_core_latencyvk_pcl_bind as bind_pcl,
    rsf_core_latencyvk_pcl_reset, rsf_core_latencyvk_pcl_status,
};
use crate::vulkan_presentation::*;
use rsf_game_sys::*;
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::Mutex;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const LOAD_FAILED: i32 = -4;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const AMD: u32 = 4;
const XE_SS_FG: u32 = 5;
const CLIENT: u32 = 2;

#[repr(C)]
struct Streamline {
    initialize: unsafe extern "C" fn(*const c_char, *const c_char, u32, *mut i32) -> i32,
    shutdown: unsafe extern "C" fn() -> i32,
    module: unsafe extern "C" fn() -> *mut c_void,
    invoke: unsafe extern "C" fn(*const rsf_slvk_call, *mut c_void, usize) -> i32,
    pace: unsafe extern "C" fn(u64, u32, u32, u32, *mut i32),
    tokens: SdkTokens,
    pcl: PclSdk,
}

#[repr(C)]
struct Platform {
    streamline: *const Streamline,
    dispatch:
        unsafe extern "C" fn(*const rsf_game_cpu_event, rsf_game_cpu_event_fn, *mut c_void) -> i32,
    backend: unsafe extern "C" fn(*const rsf_fgvk, *mut u32) -> i32,
    options: unsafe extern "C" fn(*mut rsf_fgvk, u32, u32) -> i32,
    sleep: unsafe extern "C" fn(*mut rsf_fgvk, u64) -> i32,
    marker: unsafe extern "C" fn(*mut rsf_fgvk, u64, u32) -> i32,
    status: unsafe extern "C" fn(*mut rsf_fgvk, *mut rsf_fgvk_latency_status) -> i32,
}

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
#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_sdk_result(result: i32, marker: u32) {
    let mut stats = STATS.lock().unwrap();
    stats.last_result = result;
    stats.marker_calls += u64::from(result == OK && marker != 0);
    stats.refused_calls += u64::from(result != OK);
}

struct Host {
    initialized: bool,
    closing: bool,
    owner: u32,
    requested: u32,
    limit: u32,
    facts_session: u64,
}

static HOST: Mutex<Host> = Mutex::new(Host {
    initialized: false,
    closing: false,
    owner: RSF_PACING_NONE,
    requested: 0,
    limit: 0,
    facts_session: 0,
});

struct Lease {
    context: *mut rsf_fgvk,
    closing: bool,
}

impl Lease {
    fn borrow() -> Self {
        let mut closing = 0;
        let context = unsafe { rsf_core_fgvk_presentation_borrow(&mut closing) };
        Self {
            context,
            closing: closing != 0,
        }
    }

    fn xell(&self, platform: &Platform) -> bool {
        let mut backend = 0;
        !self.context.is_null()
            && unsafe { (platform.backend)(self.context, &mut backend) } == OK
            && backend == XE_SS_FG
    }

    fn mark(&self, platform: &Platform, owner: u32, id: u64, kind: u32) {
        if owner == RSF_PACING_XELL && !self.context.is_null() {
            unsafe { (platform.marker)(self.context, id, kind) };
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.context.is_null() {
            rsf_core_fgvk_presentation_return();
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

    fn choose(&mut self, platform: &Platform, lease: &Lease) -> u32 {
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
            owner =
                rsf_core_latency_pacing_owner(required, u32::from(reflex && amd == 0), amd, AMD);
            if owner != RSF_PACING_NONE && rsf_native_latency_claim(CLIENT) == 0 {
                owner = RSF_PACING_NONE;
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
        return INVALID;
    }
    let mut host = HOST.lock().unwrap();
    if host.initialized || host.closing {
        return NOT_READY;
    }
    let Some(sdk) = (unsafe { platform.streamline.as_ref() }) else {
        return NOT_SUPPORTED;
    };
    let mut result = 0;
    let bridge = unsafe { (sdk.initialize)(directory, project, features, &mut result) };
    if bridge != OK {
        return bridge;
    }
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_latencyvk_initialize(
    platform: &Platform,
    setup: *const rsf_latencyvk_setup,
) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != RSF_LATENCYVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    initialize(platform, setup.runtime_directory_utf8, setup.project_id, 0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slvk_initialize(
    platform: &Platform,
    setup: *const rsf_slvk_setup,
) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID;
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slvk_invoke(platform: &Platform, call: *const rsf_slvk_call) -> i32 {
    if !unsafe { has_capacity(call) } {
        return INVALID;
    }
    let request = unsafe { &*call };
    if request.abi_version != RSF_SLVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if request.callback.is_none() || (request.session_id == 0) != (request.frame_id == 0) {
        return INVALID;
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
    let module = unsafe { (sdk.module)() };
    if module.is_null() {
        return NOT_READY;
    }
    unsafe { (sdk.invoke)(call, module, token) }
}

fn options(platform: &Platform, settings: Option<(u32, u32)>) -> i32 {
    let lease = Lease::borrow();
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
        unsafe { (platform.options)(lease.context, 1, limit) }
    } else if owner == RSF_PACING_NONE && (requested != 0 || limit != 0) {
        NOT_SUPPORTED
    } else {
        OK
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_options(platform: &Platform, requested: u32, limit: u32) -> i32 {
    if requested > 2 {
        return INVALID;
    }
    options(platform, Some((requested, limit)))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_presentation(platform: &Platform, context: *mut rsf_fgvk) -> i32 {
    if context.is_null() {
        return rsf_core_fgvk_presentation_unbind();
    }
    let result = rsf_core_fgvk_presentation_attach(context);
    if result != OK {
        return result;
    }
    options(platform, None)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_input(
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
    let substituted =
        event.flags & (RSF_GAME_CPU_INPUT_SUBSTITUTE | RSF_GAME_CPU_DERIVED_BOUNDARY) != 0;
    if substituted
        && matches!(
            stage,
            RSF_GAME_CPU_INPUT_SAMPLE | RSF_GAME_CPU_SIMULATION_BEGIN | RSF_GAME_CPU_SIMULATION_END
        )
    {
        return;
    }
    let platform = unsafe { &*user.cast::<Platform>() };
    if stage == RSF_GAME_CPU_INPUT_EVENT {
        rsf_core_latencyvk_input(platform, session, id, event.input_kind, event.message_id);
        return;
    }
    let lease = Lease::borrow();
    let keyed = event.event_version == RSF_GAME_CPU_EVENT_VERSION
        && event.flags & RSF_GAME_CPU_INPUT_KEYED != 0;
    let pacing = if keyed {
        stage == RSF_GAME_CPU_INPUT_SAMPLE
    } else {
        stage == RSF_GAME_CPU_PACING
    };
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
            && unsafe { (platform.options)(lease.context, 1, limit) } == OK
        {
            unsafe { (platform.sleep)(lease.context, id) };
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
        && rsf_core_latencyvk_token_pace_begin(session, id, &sdk.tokens) != 0
    {
        drop(host);
        let reflex = owner == RSF_PACING_REFLEX;
        let mut results = [0; 2];
        unsafe {
            (sdk.pace)(
                id,
                if reflex { requested } else { 0 },
                if reflex { limit } else { 0 },
                u32::from(reflex),
                results.as_mut_ptr(),
            );
        }
        let _host = HOST.lock().unwrap();
        rsf_core_latencyvk_token_pace_end();
        rsf_core_latencyvk_sdk_result(results[0], 0);
        if results[0] == OK {
            rsf_core_latencyvk_sdk_result(results[1], 0);
            if results[1] == OK {
                let mut stats = STATS.lock().unwrap();
                stats.mode = if reflex { requested } else { 0 };
                stats.sleep_calls += u64::from(reflex);
            }
        }
    } else if let Some(kind @ (0 | 1)) = marker {
        host.mark(platform, session, id, kind);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_latencyvk_cpu(platform: &Platform, event: *const rsf_game_cpu_event) {
    if !unsafe { has_capacity(event) } {
        return;
    }
    let user = ptr::from_ref(platform).cast_mut().cast();
    if unsafe { (*event).event_version } == RSF_GAME_CPU_EVENT_VERSION {
        if unsafe { (platform.dispatch)(event, Some(fact), user) } != 0 {
            pcl::pump(unsafe { &*event }, |ping| unsafe {
                (platform.dispatch)(ping, Some(fact), user);
            });
        }
    } else {
        unsafe { fact(user, event) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_submit_end(platform: &Platform, frame: u64) {
    let lease = Lease::borrow();
    let owner = HOST.lock().unwrap().owner;
    lease.mark(platform, owner, frame, 3);
    if owner == AMD {
        rsf_core_amd_vk_frame(0, frame, 1);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_render(platform: &Platform, frame: u64) {
    let lease = Lease::borrow();
    let owner = HOST.lock().unwrap().owner;
    lease.mark(platform, owner, frame, 2);
    let host = HOST.lock().unwrap();
    rsf_core_latencyvk_legacy_render(frame);
    host.mark(platform, 0, frame, 2);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_present(platform: &Platform, begin: u32) {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_latencyvk_submit(
    platform: &Platform,
    pass: *const rsf_game_render_pass,
    begin: u32,
) -> i32 {
    let lease = Lease::borrow();
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_latencyvk_get_status(
    platform: &Platform,
    output: *mut rsf_latencyvk_status,
) -> i32 {
    if !unsafe { has_capacity(output) } {
        return INVALID;
    }
    if unsafe { (*output).abi_version } != RSF_LATENCYVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let lease = Lease::borrow();
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
    if result.owner == RSF_PACING_XELL && !lease.context.is_null() {
        let mut status = rsf_fgvk_latency_status {
            struct_size: size_of::<rsf_fgvk_latency_status>() as u32,
            abi_version: RSF_FGVK_LATENCY_ABI_VERSION,
            mode: 0,
            available: 0,
            sleep_calls: 0,
            marker_calls: 0,
            result: 0,
        };
        if unsafe { (platform.status)(lease.context, &mut status) } == OK {
            result.mode = status.mode;
            result.low_latency_available = status.available;
            result.sleep_calls = status.sleep_calls;
            result.marker_calls += status.marker_calls;
        }
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

#[unsafe(no_mangle)]
extern "C" fn rsf_core_latencyvk_shutdown(platform: &Platform) -> i32 {
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
            let result = unsafe { (sdk.shutdown)() };
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
    HOST.lock().unwrap().closing = false;
    OK
}
