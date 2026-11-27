// SPDX-License-Identifier: MIT
// DLSS-G sessions borrowed from a Streamline host: admission against the host, option and
// status handoff, prepare ordering on the frame token and vendor retirement.
use super::*;
use crate::com_ref;
use crate::d3d12_generation::admission::{
    rsf_core_fg12_command, rsf_core_fg12_frame, rsf_core_fg12_setup_header,
};
use crate::d3d12_generation::{PrepareOrder, read_options};
use crate::sys::{
    RSF_FG_AUTO, RSF_FG_DYNAMIC, RSF_FG_FIXED, RSF_FG_OFF, RSF_FG_STAT_ACTIVITY,
    RSF_FG_STAT_TOTAL_PRESENTED, rsf_fg_options as Options, rsf_fg_retirement as Retirement,
    rsf_generation_provider as Provider, rsf_generation_setup as GenerationSetup,
};
use rsf_game_sys::RSF_FRAME_SOURCE_RENDERED;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dxgi::{IDXGIOutput, IDXGISwapChain1, IDXGISwapChain4};

const UI_NONE: u32 = 1;
const HUDLESS: u32 = 4;
const HUDLESS_UI: u32 = 8;
const STAT_LATENCY_AVAILABLE: u32 = 4;
const MFG_READY: u32 = 1;
const MFG_REFUSED: u32 = 2;
#[derive(Clone, Copy)]
pub(super) struct Snapshot {
    status: Status,
    options: Options,
    retirement: Retirement,
    order: PrepareOrder,
}

// The retirement fence is a borrowed, free-threaded interface that is only copied here.
unsafe impl Send for Snapshot {}

// CPU copies only. No method calls an SDK, waits for the GPU, or holds the lock across a
// vendor operation. The revision makes abort win over late publication.
pub(super) struct Copied(Mutex<Snapshot>);

impl Copied {
    pub(super) fn new() -> Self {
        Self(Mutex::new(Snapshot {
            status: unsafe { zeroed() },
            options: unsafe { zeroed() },
            retirement: Retirement {
                struct_size: size_of::<Retirement>() as u32,
                fence: null_mut(),
                value: 0,
            },
            order: PrepareOrder::default(),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, Snapshot> {
        self.0.lock().unwrap()
    }

    pub(super) fn read(&self) -> Snapshot {
        *self.lock()
    }

    pub(super) fn initialize(&self, status: Status) {
        self.lock().status = status;
    }

    pub(super) fn configured(
        &self,
        options: &Options,
        reflex: u32,
        generated: u32,
        generation_changed: bool,
    ) {
        let mut value = self.lock();
        value.options = *options;
        value.status.effective_reflex = reflex;
        if !generation_changed {
            // Latency-only changes preserve current history and invalidation.
            return;
        }
        value.order.invalidate();
        value.status.configured_mode = options.mode;
        value.status.effective_mode = options.mode;
        value.status.effective_generated_frames = generated;
        value.status.active = 0;
    }

    pub(super) fn stage_prepare(&self, revision: u64, id: u64, mode: u32, generated: u32) -> bool {
        let mut value = self.lock();
        if !value.order.stage(revision, id) {
            return false;
        }
        value.status.effective_mode = mode;
        value.status.effective_generated_frames = generated;
        value.status.active = 0;
        true
    }

    pub(super) fn complete_prepare(&self, revision: u64, id: u64, enabled: bool) -> bool {
        self.lock().order.complete(revision, id, enabled)
    }

    pub(super) fn abort(&self) {
        let mut value = self.lock();
        value.order.invalidate();
        value.status.active = 0;
    }

    pub(super) fn paced(&self) {
        let mut value = self.lock();
        if value.status.effective_reflex != RSF_REFLEX_OFF {
            value.status.latency_paced_frames = value.status.latency_paced_frames.wrapping_add(1);
        }
    }

    pub(super) fn polled(
        &self,
        revision: u64,
        observed: &Status,
        presented: u64,
        sdk_active: bool,
        retirement: Retirement,
    ) -> Status {
        let mut value = self.lock();
        let active = value.order.active(revision, sdk_active);
        let state = &mut value.status;
        // Publish only observed fields, preserving newer configuration and cumulative counters.
        state.max_generated_frames = observed.max_generated_frames;
        state.mfg_unlock_status = observed.mfg_unlock_status;
        state.dynamic_supported = observed.dynamic_supported;
        state.vsync_supported = observed.vsync_supported;
        state.min_dimension = observed.min_dimension;
        state.vendor_status = observed.vendor_status;
        state.low_latency_available = observed.low_latency_available;
        state.valid_statistics = observed.valid_statistics;
        state.total_presented = state.total_presented.wrapping_add(presented);
        state.active = active.into();
        let state = *state;
        value.retirement = retirement;
        state
    }
}

struct Session {
    host: NonNull<Host>,
    chain: Option<IUnknown>,
    copied: Copied,
    ui_mode: u32,
}

impl Session {
    // The caller keeps the host alive until every session from it is destroyed.
    fn host(&self) -> &Host {
        unsafe { self.host.as_ref() }
    }

    // slDLSSGSetOptions with the fields this provider sets.
    fn set_options(&self, mode: u32, frames: u32, dynamic_target_fps: f32) -> i32 {
        let options = records::pack_options(
            mode,
            frames,
            (self.ui_mode == HUDLESS_UI).into(),
            dynamic_target_fps,
        );
        let viewport = sdk::viewport(0);
        match self.host().generation().set_options {
            Some(set) if unsafe { set(&viewport, &options) } == sdk::OK => OK,
            _ => FEATURE_FAILED,
        }
    }
}

fn release(mut session: Box<Session>) {
    let host = unsafe { session.host.as_ref() };
    if let Some(chain) = session.chain.take() {
        if session.copied.read().status.effective_mode != RSF_FG_OFF
            && let Some(set) = host.generation().set_options
        {
            // sl::DLSSGOptions{}: mode eOff with the SDK member defaults.
            let mut options = records::pack_options(0, 1, 0, 0.0);
            options.flags = 0;
            unsafe { set(&sdk::viewport(0), &options) };
        }
        drop(chain);
    }
    host.borrowers.fetch_sub(1, Relaxed);
}

fn poll(session: &Session) -> i32 {
    let host = session.host();
    let before = session.copied.read();
    let mut sdk_state = sdk::dlssg_state();
    let viewport = sdk::viewport(0);
    match host.generation().get_state {
        Some(get) if unsafe { get(&viewport, &mut sdk_state, null()) } == sdk::OK => {}
        _ => return FEATURE_FAILED,
    }
    let (mut mfg_status, mut mfg_maximum) = (0, 0);
    unsafe { mfg_state::rsf_core_mfg_status(host.mfg, &mut mfg_status, &mut mfg_maximum) };
    let mut state = before.status;
    state.max_generated_frames = sdk_state.max_generated;
    state.mfg_unlock_status = mfg_status;
    // Both the initialized SDK limit and the verified compiled allocation must agree.
    if state.mfg_unlock_status == MFG_READY {
        state.max_generated_frames = state.max_generated_frames.min(mfg_maximum);
    } else if state.mfg_unlock_status == MFG_REFUSED {
        state.max_generated_frames = state.max_generated_frames.min(1);
    }
    state.dynamic_supported = u32::from(sdk_state.dynamic_mfg == sdk::TRUE);
    if state.mfg_unlock_status == MFG_REFUSED {
        state.dynamic_supported = 0;
    }
    state.vsync_supported = u32::from(sdk_state.vsync == sdk::TRUE);
    state.min_dimension = sdk_state.min_dimension;
    state.vendor_status = sdk_state.status as i32;
    state.valid_statistics = RSF_FG_STAT_TOTAL_PRESENTED | RSF_FG_STAT_ACTIVITY;
    let mut frames = host.frames();
    let now = Instant::now();
    let report = host.log.is_some()
        && frames
            .last_report
            .is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_secs(1));
    if let Some((low_latency, report_available)) = host.read_reflex(report, &frames.input_counts) {
        state.low_latency_available = low_latency;
        if report_available != 0 {
            state.valid_statistics |= STAT_LATENCY_AVAILABLE;
        }
        if report {
            frames.last_report = Some(now);
        }
    }
    let retirement = Retirement {
        struct_size: size_of::<Retirement>() as u32,
        fence: sdk_state.inputs_fence,
        value: sdk_state.inputs_fence_value,
    };
    let presented = u64::from(sdk_state.actual_presents);
    // DLSSGStatus::eOk is zero.
    let sdk_active = sdk_state.status == 0 && presented > 1;
    frames.cached_capability = session.copied.polled(
        before.order.revision,
        &state,
        presented,
        sdk_active,
        retirement,
    );
    OK
}

unsafe fn same_text(requested: *const c_char, owned: &[c_char]) -> bool {
    requested.is_null() || unsafe { CStr::from_ptr(requested) == CStr::from_ptr(owned.as_ptr()) }
}

// slIsFeatureSupported for DLSS-G and Reflex on the native device's adapter.
fn supported(host: &Host, log: Log, user: Handle) -> i32 {
    let Some(device) = host.native_device.as_ref() else {
        return NOT_SUPPORTED;
    };
    let mut luid = crate::streamline_sr::luid_bits(unsafe { device.GetAdapterLuid() });
    let info = sdk::adapter(&mut luid);
    let supported = core(host.core.is_feature_supported);
    let fg = unsafe { supported(sdk::FEATURE_DLSS_G, &info) };
    let reflex = unsafe { supported(sdk::FEATURE_REFLEX, &info) };
    if fg == sdk::OK && reflex == sdk::OK {
        return OK;
    }
    records::report_support(fg as u32, reflex as u32, log, user);
    NOT_SUPPORTED
}

// One owned, upgraded IDXGISwapChain4 reference on the host's factory and queue.
unsafe fn create_chain(host: &Host, desc: &crate::sys::rsf_fg_swapchain_desc) -> Option<IUnknown> {
    let description = crate::d3d12_generation::chain_description(desc);
    let (factory, queue) = (host.factory.as_ref()?, host.queue.as_ref()?);
    let created = unsafe {
        factory.CreateSwapChainForHwnd(
            queue,
            HWND(desc.hwnd),
            &description,
            None,
            None::<&IDXGIOutput>,
        )
    };
    let mut raw = created.ok()?.into_raw();
    if unsafe { host.upgrade_chain(&mut raw) } != OK {
        drop(unsafe { IUnknown::from_raw(raw) });
        return None;
    }
    let upgraded = unsafe { IDXGISwapChain1::from_raw(raw) };
    let chain = upgraded.cast::<IDXGISwapChain4>().ok()?;
    unsafe { com_ref::owned(chain.into_raw()) }
}

unsafe extern "C" fn create(
    setup: *const GenerationSetup,
    out: *mut Handle,
    chain: *mut Handle,
) -> i32 {
    let header = unsafe { rsf_core_fg12_setup_header(setup, out, chain) };
    if header != OK {
        return header;
    }
    let setup = unsafe { &*setup };
    let ui_mode = setup.chain.ui_mode;
    if ![UI_NONE, HUDLESS, HUDLESS_UI].contains(&ui_mode) {
        return NOT_SUPPORTED;
    }
    let Some(host_pointer) = NonNull::new(setup.streamline_host.cast::<Host>()) else {
        return NEEDS_RESTART;
    };
    let host = unsafe { host_pointer.as_ref() };
    // Optional assertions against the host. They never initialize another SDK.
    if (setup.engine_type != 0 && setup.engine_type != host.engine_type)
        || !unsafe { same_text(setup.engine_version_utf8, &host.engine_version) }
        || !unsafe { same_text(setup.project_id_utf8, &host.project_id) }
    {
        return INVALID;
    }
    if setup.require_signature != 0 && !host.signature_verified {
        return LOAD_FAILED;
    }
    if setup.development_runtime != 0 && !host.development_runtime {
        return NEEDS_RESTART;
    }
    if setup.chain.d3d12_device != Host::raw(&host.device)
        || setup.chain.d3d12_queue != Host::raw(&host.queue)
    {
        return WRONG_API;
    }
    // Active DLSS-G needs Reflex frame tracking and PCL markers on the same token.
    let generation = host.generation();
    if generation.get_state.is_none()
        || generation.set_options.is_none()
        || host.sleep.is_none()
        || host.pcl_marker.is_none()
        || host.reflex_options.is_none()
        || host.reflex_state.is_none()
    {
        return NOT_SUPPORTED;
    }
    if setup.chain.width == 0 || setup.chain.height == 0 || setup.chain.buffer_count < 2 {
        return INVALID;
    }
    let supported = supported(host, setup.chain.log, setup.chain.log_user);
    if supported != OK {
        return supported;
    }
    let raw = crate::allocate_handle(Session {
        host: host_pointer,
        chain: None,
        copied: Copied::new(),
        ui_mode,
    });
    if raw.is_null() {
        return INIT_FAILED;
    }
    let mut session = unsafe { Box::from_raw(raw) };
    let borrowers = host.borrowers.fetch_add(1, Relaxed) + 1;
    let mut initial: Status = unsafe { zeroed() };
    initial.struct_size = size_of::<Status>() as u32;
    initial.supported = 1;
    initial.auto_supported = 1;
    initial.pacing_owner = RSF_PACING_REFLEX;
    let mut version = sdk::feature_version();
    let get_version = core(host.core.get_feature_version);
    let found = unsafe { get_version(sdk::FEATURE_DLSS_G, &mut version) } == sdk::OK
        && sdk::version_present(&version.ngx);
    records::format_version(found.then_some(&version), &mut initial);
    if setup.version_id != 0 && setup.version_id != initial.version_id {
        release(session);
        return NOT_SUPPORTED;
    }
    if borrowers > 1 {
        // A later borrower starts from the first one's published capabilities.
        initial = host.frames().cached_capability;
        initial.active = 0;
        initial.total_presented = 0;
        initial.valid_statistics = 0;
    }
    session.copied.initialize(initial);
    if borrowers <= 1 {
        let result = poll(&session);
        if result != OK {
            release(session);
            return result;
        }
    }
    if !setup.chain.hwnd.is_null() {
        // One host has one presentation chain.
        if borrowers > 1 {
            release(session);
            return NOT_READY;
        }
        let Some(created) = (unsafe { create_chain(host, &setup.chain) }) else {
            release(session);
            return INIT_FAILED;
        };
        session.chain = Some(created);
    }
    unsafe {
        chain.write(session.chain.as_ref().map_or(null_mut(), Interface::as_raw));
        out.write(Box::into_raw(session).cast());
    }
    OK
}

unsafe fn session<'a>(pointer: Handle) -> Option<&'a Session> {
    unsafe { pointer.cast::<Session>().as_ref() }
}

unsafe extern "C" fn configure(pointer: Handle, options: *const Options) -> i32 {
    let Some(session) = (unsafe { session(pointer) }) else {
        return INVALID;
    };
    let options = match unsafe { read_options(options) } {
        Ok(options) => options,
        Err(result) => return result,
    };
    let before = session.copied.read();
    if session.chain.is_none() {
        return NOT_READY;
    }
    let same_generation = before.options.struct_size != 0
        && before.options.mode == options.mode
        && before.options.generated_frames == options.generated_frames
        && before.options.dynamic_target_fps == options.dynamic_target_fps;
    // Active DLSS-G needs Reflex's runtime frame tracking. Preserve the requested
    // preference, while reporting the required effective mode separately.
    let mode = if options.mode != RSF_FG_OFF && options.reflex_mode == RSF_REFLEX_OFF {
        REFLEX_ON
    } else {
        options.reflex_mode
    };
    let host = session.host();
    let reflex_matches = {
        let frames = host.frames();
        frames.reflex_mode == mode && frames.frame_limit_us == options.frame_limit_us
    };
    if same_generation
        && reflex_matches
        && before.options.reflex_mode == options.reflex_mode
        && before.options.frame_limit_us == options.frame_limit_us
    {
        return OK;
    }
    if options.mode == RSF_FG_DYNAMIC && before.status.dynamic_supported == 0 {
        return NOT_SUPPORTED;
    }
    if options.mode == RSF_FG_AUTO && before.status.mfg_unlock_status == MFG_REFUSED {
        return NOT_SUPPORTED;
    }
    if options.mode == RSF_FG_FIXED && options.generated_frames == 0 {
        return INVALID;
    }
    if options.mode != RSF_FG_OFF && before.status.low_latency_available == 0 {
        return NOT_SUPPORTED;
    }
    let latency = reflex(host, mode, options.frame_limit_us);
    if latency != OK {
        return latency;
    }
    // Record the accepted latency change even if generation refuses the request.
    // The next call compares against the live host before taking the equality fast path.
    session.copied.lock().status.effective_reflex = mode;
    // A Reflex mode or limit change leaves generation and its history alone.
    if same_generation {
        session.copied.configured(&options, mode, 0, false);
        return OK;
    }
    let generated = options
        .generated_frames
        .min(before.status.max_generated_frames);
    if session.set_options(options.mode, generated, options.dynamic_target_fps) != OK {
        return FEATURE_FAILED;
    }
    let fixed = if options.mode == RSF_FG_FIXED {
        generated
    } else {
        0
    };
    session.copied.configured(&options, mode, fixed, true);
    OK
}

unsafe extern "C" fn begin_frame(pointer: Handle, id: u64) -> i32 {
    let Some(session) = (unsafe { session(pointer) }) else {
        return INVALID;
    };
    if id == 0 {
        return INVALID;
    }
    let result = begin(session.host(), id);
    if result == OK {
        session.copied.paced();
    }
    result
}

unsafe extern "C" fn marker(pointer: Handle, marker: u32, id: u64, controller: u32) -> i32 {
    unsafe { session(pointer) }.map_or(INVALID, |session| {
        marker_for(session.host(), id, marker, controller)
    })
}

/// The constants of one generated frame. Sets them only when SR has not already done so.
unsafe fn set_constants(
    host: &Host,
    token: Handle,
    frame: &Frame,
    reset: bool,
    apply: bool,
) -> i32 {
    let mut constants = unsafe { records::pack_fg_camera(frame, reset.into()) };
    constants.previous_to_clip = sdk::full_invert(&constants.clip_to_previous);
    if !constants
        .previous_to_clip
        .iter()
        .all(|value| value.is_finite())
    {
        return INVALID;
    }
    let viewport = sdk::viewport(0);
    if apply && unsafe { core(host.core.set_constants)(&constants, token, &viewport) } != sdk::OK {
        return FEATURE_FAILED;
    }
    OK
}

unsafe fn set_tags(host: &Host, token: Handle, command: Handle, frame: &Frame) -> i32 {
    let (planned, count) = records::plan_tags(frame);
    let mut resources: [sdk::Resource; 5] =
        std::array::from_fn(|i| sdk::texture(planned[i].resource, planned[i].state));
    let tags: [sdk::ResourceTag; 5] = std::array::from_fn(|i| {
        sdk::tag(&raw mut resources[i], planned[i].kind, planned[i].extent)
    });
    let viewport = sdk::viewport(0);
    let set_tags = core(host.core.set_tag_for_frame);
    let set = unsafe { set_tags(token, &viewport, tags.as_ptr(), count, command) };
    if set == sdk::OK { OK } else { FEATURE_FAILED }
}

unsafe extern "C" fn prepare(pointer: Handle, command: Handle, frame: *const Frame) -> i32 {
    let Some(session) = (unsafe { session(pointer) }) else {
        return INVALID;
    };
    let host = session.host();
    let checked = unsafe { rsf_core_fg12_frame(frame, host.native_device_raw()) };
    if checked != OK {
        return checked;
    }
    let frame = unsafe { &*frame };
    let record = unsafe { &*frame.record };
    let before = session.copied.read();
    if session.chain.is_none() || record.frame_id <= before.order.last_prepare {
        return NOT_READY;
    }
    if record.source_provenance == RSF_FRAME_SOURCE_RENDERED {
        return NOT_SUPPORTED;
    }
    let enabled = frame.interpolate != 0 && before.options.mode != RSF_FG_OFF;
    if enabled {
        if command.is_null() {
            return INVALID;
        }
        let native_command = unsafe { host.alias(command) };
        if native_command.is_null() {
            return INVALID;
        }
        let result = unsafe { rsf_core_fg12_command(native_command, host.native_device_raw()) };
        if result != OK {
            return result;
        }
    }
    if enabled
        && ((session.ui_mode != UI_NONE && frame.hudless.resource.is_null())
            || (session.ui_mode == HUDLESS_UI && frame.ui.resource.is_null()))
    {
        return INVALID;
    }
    let mode = if enabled {
        before.options.mode
    } else {
        RSF_FG_OFF
    };
    let generated = if before.options.mode == RSF_FG_FIXED {
        before
            .options
            .generated_frames
            .min(before.status.max_generated_frames)
    } else {
        1
    };
    if before.status.effective_mode != mode
        && session.set_options(mode, generated, before.options.dynamic_target_fps) != OK
    {
        return FEATURE_FAILED;
    }
    let staged_frames = if enabled && before.options.mode == RSF_FG_FIXED {
        generated
    } else {
        0
    };
    if !session
        .copied
        .stage_prepare(before.order.revision, record.frame_id, mode, staged_frames)
    {
        return NOT_READY;
    }
    if !enabled {
        return if session
            .copied
            .complete_prepare(before.order.revision, record.frame_id, false)
        {
            OK
        } else {
            NOT_READY
        };
    }
    let mut frames = host.frames();
    let Some(token) = frames.token_mut(record.frame_id) else {
        return NOT_READY;
    };
    let handle = token.handle.as_ptr();
    let reset = before.order.reset_for(record);
    // The SR side may already have set this frame's shared constants.
    let apply = !token.common[0];
    let result = unsafe { set_constants(host, handle, frame, reset, apply) };
    if result != OK {
        return result;
    }
    if apply {
        token.common[0] = true;
    }
    if unsafe { set_tags(host, handle, command, frame) } != OK {
        return FEATURE_FAILED;
    }
    drop(frames);
    if session
        .copied
        .complete_prepare(before.order.revision, record.frame_id, true)
    {
        OK
    } else {
        NOT_READY
    }
}

unsafe extern "C" fn after_present(pointer: Handle) -> i32 {
    unsafe { session(pointer) }.map_or(INVALID, poll)
}

unsafe extern "C" fn status(pointer: Handle, out: *mut Status) -> i32 {
    let Some(session) = (unsafe { session(pointer) }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(out) } {
        return INVALID;
    }
    unsafe { out.write(session.copied.read().status) };
    OK
}

unsafe extern "C" fn retirement(pointer: Handle, out: *mut Retirement) -> i32 {
    let Some(session) = (unsafe { session(pointer) }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(out) } {
        return INVALID;
    }
    let copy = session.copied.read().retirement;
    unsafe {
        (*out).fence = copy.fence;
        (*out).value = copy.value;
    }
    OK
}

unsafe extern "C" fn destroy(pointer: Handle) {
    if !pointer.is_null() {
        release(unsafe { Box::from_raw(pointer.cast::<Session>()) });
    }
}

unsafe extern "C" fn abort_frame(pointer: Handle, id: u64) -> i32 {
    let Some(session) = (unsafe { session(pointer) }) else {
        return INVALID;
    };
    if id == 0 {
        return INVALID;
    }
    let result = abort(session.host(), id);
    if result != OK {
        return result;
    }
    session.copied.abort();
    OK
}

static PROVIDER: Provider = Provider {
    struct_size: size_of::<Provider>() as u32,
    create: Some(create),
    configure: Some(configure),
    begin_frame: Some(begin_frame),
    marker: Some(marker),
    prepare: Some(prepare),
    after_present: Some(after_present),
    status: Some(status),
    retirement: Some(retirement),
    destroy: Some(destroy),
    abort_frame: Some(abort_frame),
    destroy_checked: None,
};

/// The DLSS-G generation provider.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_generation_dlss() -> *const Provider {
    &PROVIDER
}

#[cfg(test)]
mod tests;
