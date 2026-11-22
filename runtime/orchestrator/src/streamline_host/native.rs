// SPDX-License-Identifier: MIT
//! The typed SDK, COM and Win32 leaves of the D3D12 Streamline host, formerly
//! runtime/backends/dlss/src/dlss_generation.cpp, and the public entry points of
//! rescaleframe/streamline_host.h. The owner above calls every leaf under its own locks.
use super::generation::{
    Observed, Request, rsf_core_slhost_session_abort, rsf_core_slhost_session_after,
    rsf_core_slhost_session_begin, rsf_core_slhost_session_configure,
    rsf_core_slhost_session_create, rsf_core_slhost_session_destroy,
    rsf_core_slhost_session_marker, rsf_core_slhost_session_prepare,
    rsf_core_slhost_session_retirement, rsf_core_slhost_session_status,
};
use super::*;
use crate::mfg_state::{self, Owner as Mfg};
use crate::reflex_audit::{rsf_reflex_audit_start, rsf_reflex_audit_stop, rsf_reflex_driver_sleep};
use crate::streamline_generation_records as records;
use crate::streamline_log::{
    rsf_core_streamline_log_clear, rsf_core_streamline_log_emit, rsf_core_streamline_log_set,
};
use crate::streamline_sdk::{self as sdk, Entries};
use crate::sys::{
    rsf_fg_options as Options, rsf_generation_provider as Provider,
    rsf_generation_setup as GenerationSetup,
};
use rsf_streamline_vk::sdk_api;
use std::mem::transmute;
use std::ptr::null;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_FENCE_FLAG_NONE,
    D3D12CreateDevice, ID3D12CommandQueue, ID3D12Device, ID3D12Fence,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_SWAP_CHAIN_DESC1, IDXGIAdapter,
    IDXGIFactory2, IDXGIOutput, IDXGISwapChain1, IDXGISwapChain4,
};
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::CreateEventW;

const LOG_SLOT: u32 = 1; // RSF_STREAMLINE_LOG_D3D12_HOST
const DRAIN_TIMEOUT_MS: u32 = 10_000;
const PCL_OPTIONS: u32 = 128;

unsafe extern "C" {
    // The shared FG12 admission leaves in d3d12_generation.
    fn rsf_core_fg12_setup_header(
        setup: *const GenerationSetup,
        out: *mut Handle,
        chain: *mut Handle,
    ) -> i32;
    fn rsf_core_fg12_frame(frame: *const Frame, device: Handle) -> i32;
    fn rsf_core_fg12_command(command: Handle, device: Handle) -> i32;
    fn rsf_core_fg12_chain_description(desc: &ChainDesc, out: *mut DXGI_SWAP_CHAIN_DESC1);
}

/// A required core entry. Startup binds the table only with every one of them resolved.
fn core<T>(entry: Option<T>) -> T {
    entry.expect("required Streamline entry")
}

/// The typed SDK, COM and Win32 state of one host.
struct Native {
    initialized: bool,
    log: Log,
    log_user: Handle,
    device: Option<ID3D12Device>,
    native_device: Option<ID3D12Device>,
    queue: Option<ID3D12CommandQueue>,
    native_queue: Option<ID3D12CommandQueue>,
    factory: Option<IDXGIFactory2>,
    // Borrowed from the owner until shutdown, as is the plugin path array.
    directory: *const u16,
    paths: [*const u16; 1],
    core: Option<Entries>,
    get_state: Option<sdk_api::GetFgState<sdk::DlssgState, sdk::FgOptions>>,
    set_options: Option<sdk_api::SetOptions<sdk::FgOptions>>,
    reflex_options: Option<sdk_api::Apply<sdk::ReflexOptions>>,
    reflex_state: Option<sdk_api::Read<sdk::ReflexState>>,
    sleep: Option<sdk_api::ReflexSleep>,
    pcl_marker: Option<sdk_api::PclSetMarker>,
    pcl_options: Option<sdk_api::Apply<sdk::PclOptions>>,
    pcl_state: Option<sdk_api::Read<sdk::PclState>>,
    mfg: *mut Mfg,
}

impl Native {
    fn core(&self) -> &Entries {
        self.core.as_ref().expect("bound Streamline core")
    }

    fn resolved(&self) -> u32 {
        [
            (self.get_state.is_some(), GET_STATE),
            (self.set_options.is_some(), SET_OPTIONS),
            (self.reflex_options.is_some(), REFLEX_OPTIONS),
            (self.reflex_state.is_some(), REFLEX_STATE),
            (self.sleep.is_some(), SLEEP),
            (self.pcl_marker.is_some(), PCL_MARKER),
            (self.pcl_state.is_some(), PCL_STATE),
        ]
        .into_iter()
        .filter_map(|(present, entry)| present.then_some(entry))
        .fold(0, |entries, entry| entries | entry)
    }

    /// One feature function, or None when the SDK refuses or returns null.
    unsafe fn feature(&self, id: u32, name: *const c_char) -> Option<Handle> {
        let mut pointer = null_mut();
        let function = core(self.core().get_feature_function);
        let found = unsafe { function(id, name, &mut pointer) } == sdk::OK;
        (found && !pointer.is_null()).then_some(pointer)
    }

    /// Borrowed native alias, valid through the supplied proxy object's lifetime.
    unsafe fn alias(&self, proxy: Handle) -> Handle {
        let mut native = null_mut();
        let native_interface = core(self.core().get_native_interface);
        if unsafe { native_interface(proxy, &mut native) } != sdk::OK || native.is_null() {
            return null_mut();
        }
        drop(unsafe { IUnknown::from_raw(native) });
        native
    }

    /// slUpgradeInterface on an owned reference. A refusal releases it.
    unsafe fn upgrade<T: Interface>(&self, object: &mut Option<T>) -> u32 {
        let Some(original) = object.take() else {
            return 0;
        };
        let raw = original.into_raw();
        let mut upgraded = raw;
        if unsafe { core(self.core().upgrade_interface)(&mut upgraded) } != sdk::OK {
            drop(unsafe { T::from_raw(raw) });
            return 0;
        }
        *object = Some(unsafe { T::from_raw(upgraded) });
        1
    }

    /// The native interface behind a proxy, retaining the reference the SDK returns.
    unsafe fn retain_native<T: Interface>(&self, proxy: &Option<T>) -> Option<T> {
        let mut native = null_mut();
        let proxy = proxy.as_ref().map_or(null_mut(), Interface::as_raw);
        let native_interface = core(self.core().get_native_interface);
        if unsafe { native_interface(proxy, &mut native) } != sdk::OK || native.is_null() {
            return None;
        }
        Some(unsafe { T::from_raw(native) })
    }

    fn native_device_raw(&self) -> Handle {
        self.native_device
            .as_ref()
            .map_or(null_mut(), Interface::as_raw)
    }

    fn report_driver_sleep(&self) {
        unsafe { rsf_reflex_driver_sleep(self.native_device_raw(), self.log, self.log_user) };
    }
}

unsafe fn native<'a>(native: Handle) -> &'a mut Native {
    unsafe { &mut *native.cast::<Native>() }
}

unsafe extern "C" fn emit(log: Log, user: Handle, text: *const c_char) {
    if let Some(log) = log {
        unsafe { log(user, text) };
    }
}

unsafe extern "C" fn message(kind: u32, text: *const c_char) {
    let important = u32::from(kind == sdk::LOG_ERROR || kind == sdk::LOG_WARN);
    unsafe { rsf_core_streamline_log_emit(LOG_SLOT, important, text, emit) };
}

fn say(log: Log, user: Handle, text: &str) {
    if let (Some(log), Ok(text)) = (log, CString::new(text)) {
        unsafe { log(user, text.as_ptr()) };
    }
}

// One driver ring snapshot. The owner's RING lock serializes its capture and write.
struct Ring(Box<sdk::ReflexState>);
unsafe impl Send for Ring {}
static RING_STATE: Mutex<Option<Ring>> = Mutex::new(None);

// Startup leaves.

unsafe extern "C" fn interposer_loaded() -> u32 {
    sdk::interposer_loaded().into()
}

unsafe extern "C" fn adapter_vendor(
    candidate: Handle,
    vendor: &mut u32,
    retained: &mut Handle,
) -> u32 {
    let Some(candidate) = (unsafe { IUnknown::from_raw_borrowed(&candidate) }) else {
        return 0;
    };
    let Ok(adapter) = candidate.cast::<IDXGIAdapter>() else {
        return 0;
    };
    let Ok(description) = (unsafe { adapter.GetDesc() }) else {
        return 0;
    };
    *vendor = description.VendorId;
    *retained = adapter.into_raw();
    1
}

unsafe extern "C" fn report_vendor(log: Log, user: Handle, vendor: u32) {
    say(
        log,
        user,
        &format!("Streamline host refused: adapter vendor 0x{vendor:04x} is not NVIDIA"),
    );
}

unsafe extern "C" fn load(path: *const u16, setup: &Setup) -> Handle {
    let log = crate::input_dump::Log::new(setup.log, setup.log_user);
    unsafe { sdk::load_interposer(path, setup.require_signature != 0, &log) }
}

unsafe extern "C" fn allocate(log: Log, user: Handle) -> Handle {
    let native = Native {
        initialized: false,
        log,
        log_user: user,
        device: None,
        native_device: None,
        queue: None,
        native_queue: None,
        factory: None,
        directory: null(),
        paths: [null()],
        core: None,
        get_state: None,
        set_options: None,
        reflex_options: None,
        reflex_state: None,
        sleep: None,
        pcl_marker: None,
        pcl_options: None,
        pcl_state: None,
        mfg: mfg_state::rsf_core_mfg_create(),
    };
    crate::allocate_handle(native).cast()
}

unsafe extern "C" fn bind_required(handle: Handle, entries: &Entries) {
    // Startup checked that every required entry was resolved.
    unsafe { native(handle).core = Some(*entries) };
}

unsafe extern "C" fn init(handle: Handle, request: &InitPreferences) -> u32 {
    let host = unsafe { native(handle) };
    host.directory = request.directory;
    host.paths[0] = host.directory;
    let show_console = request.show_console != 0;
    rsf_core_streamline_log_set(
        LOG_SLOT,
        handle as usize,
        host.log,
        host.log_user,
        u32::from(show_console),
    );
    let preferences = sdk::Preferences {
        base: sdk::PREFERENCES,
        show_console: u8::from(show_console),
        log_level: request.log_level,
        plugin_paths: host.paths.as_mut_ptr(),
        num_plugin_paths: 1,
        log: Some(message),
        flags: request.flags,
        features: request.features,
        num_features: request.feature_count,
        engine: request.engine_type,
        engine_version: request.engine_version,
        project: request.project_id,
        render_api: request.render_api,
        ..Default::default()
    };
    host.initialized = unsafe { core(host.core().init)(&preferences, sdk::SDK_VERSION) } == sdk::OK;
    host.initialized.into()
}

unsafe extern "C" fn borrow_device(handle: Handle, borrowed: Handle) {
    unsafe { native(handle).device = ID3D12Device::from_raw_borrowed(&borrowed).cloned() };
}

unsafe extern "C" fn create_device(handle: Handle, adapter: Handle) -> u32 {
    let adapter = unsafe { IUnknown::from_raw_borrowed(&adapter) };
    let mut device: Option<ID3D12Device> = None;
    if unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.is_err()
        || device.is_none()
    {
        return 0;
    }
    unsafe { native(handle).device = device };
    1
}

unsafe fn prime(host: &Native, device: Handle, require_signature: u32) {
    let verify: mfg_state::VerifySignature =
        (require_signature != 0).then_some(rsf_streamline_vk::admission::nvidia_signed);
    if let Some(mfg) = unsafe { host.mfg.as_ref() } {
        unsafe {
            mfg_state::rsf_core_mfg_prime(
                mfg,
                device,
                host.directory,
                verify,
                host.log,
                host.log_user,
            )
        };
    }
}

unsafe fn prepare(host: &Native) {
    if let Some(mfg) = unsafe { host.mfg.as_ref() } {
        let get_state = host.get_state.map_or(null_mut(), |entry| entry as Handle);
        unsafe {
            mfg_state::rsf_core_mfg_prepare(
                mfg,
                host.native_device_raw(),
                get_state,
                host.log,
                host.log_user,
            )
        };
    }
}

unsafe extern "C" fn prime_mfg(handle: Handle, require_signature: u32) {
    let host = unsafe { native(handle) };
    let device = host.device.as_ref().map_or(null_mut(), Interface::as_raw);
    unsafe { prime(host, device, require_signature) };
}

unsafe extern "C" fn upgrade_device(handle: Handle) -> u32 {
    let host = unsafe { native(handle) };
    let mut device = host.device.take();
    let upgraded = unsafe { host.upgrade(&mut device) };
    host.device = device;
    upgraded
}

unsafe extern "C" fn native_device(handle: Handle) -> u32 {
    let host = unsafe { native(handle) };
    host.native_device = unsafe { host.retain_native(&host.device) };
    host.native_device.is_some().into()
}

unsafe extern "C" fn start_audit(handle: Handle) {
    let host = unsafe { native(handle) };
    unsafe { rsf_reflex_audit_start(host.log, host.log_user) };
}

unsafe extern "C" fn register_device(handle: Handle) -> u32 {
    let host = unsafe { native(handle) };
    let device = host.device.as_ref().map_or(null_mut(), Interface::as_raw);
    u32::from(unsafe { core(host.core().set_d3d_device)(device) } == sdk::OK)
}

unsafe extern "C" fn create_factory(handle: Handle) -> u32 {
    let factory = unsafe { CreateDXGIFactory2::<IDXGIFactory2>(DXGI_CREATE_FACTORY_FLAGS(0)) };
    let Ok(factory) = factory else {
        return 0;
    };
    unsafe { native(handle).factory = Some(factory) };
    1
}

unsafe extern "C" fn upgrade_factory(handle: Handle) -> u32 {
    let host = unsafe { native(handle) };
    let mut factory = host.factory.take();
    let upgraded = unsafe { host.upgrade(&mut factory) };
    host.factory = factory;
    upgraded
}

unsafe extern "C" fn queue_type(queue: Handle) -> u32 {
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.expect("live queue");
    unsafe { queue.GetDesc() }.Type.0 as u32
}

unsafe extern "C" fn queue_matches_device(
    handle: Handle,
    queue: Handle,
    retained: &mut Handle,
) -> u32 {
    let host = unsafe { native(handle) };
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.expect("live queue");
    let mut queue_device: Option<ID3D12Device> = None;
    let found = unsafe { queue.GetDevice(&mut queue_device) }.is_ok();
    // The same interface pointer, as the native comparison required.
    let matches = found
        && queue_device
            .as_ref()
            .is_some_and(|device| device.as_raw() == host.native_device_raw());
    *retained = queue_device.map_or(null_mut(), Interface::into_raw);
    matches.into()
}

unsafe extern "C" fn create_queue(handle: Handle) -> u32 {
    let host = unsafe { native(handle) };
    let description = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    let Some(device) = host.device.as_ref() else {
        return 0;
    };
    let Ok(queue) = (unsafe { device.CreateCommandQueue::<ID3D12CommandQueue>(&description) })
    else {
        return 0;
    };
    host.queue = Some(queue);
    1
}

unsafe extern "C" fn native_queue(handle: Handle) -> u32 {
    let host = unsafe { native(handle) };
    host.native_queue = unsafe { host.retain_native(&host.queue) };
    host.native_queue.is_some().into()
}

// Feature functions resolve only on success; a refusal keeps any earlier entry.
unsafe extern "C" fn resolve_feature(
    handle: Handle,
    id: u32,
    name: *const c_char,
    entry: u32,
) -> u32 {
    let host = unsafe { native(handle) };
    let Some(pointer) = (unsafe { host.feature(id, name) }) else {
        return 0;
    };
    unsafe {
        match entry {
            GET_STATE => host.get_state = sdk_api::function(pointer),
            SET_OPTIONS => host.set_options = sdk_api::function(pointer),
            REFLEX_OPTIONS => host.reflex_options = sdk_api::function(pointer),
            REFLEX_STATE => host.reflex_state = sdk_api::function(pointer),
            SLEEP => host.sleep = sdk_api::function(pointer),
            PCL_MARKER => host.pcl_marker = sdk_api::function(pointer),
            PCL_STATE => host.pcl_state = sdk_api::function(pointer),
            PCL_OPTIONS => host.pcl_options = sdk_api::function(pointer),
            _ => return 0,
        }
    }
    1
}

unsafe extern "C" fn prepare_mfg(handle: Handle) {
    unsafe { prepare(native(handle)) };
}

unsafe extern "C" fn pcl_defaults(handle: Handle) {
    let host = unsafe { native(handle) };
    let mut options = sdk::PclOptions::default();
    // eUsePingMessage with no marker thread.
    sdk::pcl_options(0, &mut options);
    if let Some(set) = host.pcl_options {
        unsafe { set(&options) };
    }
}

// Frame, Reflex and PCL leaves.

unsafe extern "C" fn dispose(handle: Handle) {
    rsf_reflex_audit_stop();
    let mut host = unsafe { Box::from_raw(handle.cast::<Native>()) };
    host.queue = None;
    host.native_queue = None;
    host.factory = None;
    host.device = None;
    host.native_device = None;
    if host.initialized {
        unsafe { core(host.core().shutdown)() };
    }
    unsafe { mfg_state::rsf_core_mfg_release(host.mfg) };
    rsf_core_streamline_log_clear(LOG_SLOT, handle as usize);
    unsafe { mfg_state::rsf_core_mfg_destroy(host.mfg) };
}

unsafe extern "C" fn token(handle: Handle, index: u32, out: &mut Handle) -> i32 {
    let host = unsafe { native(handle) };
    let mut token = null();
    let new_token = core(host.core().get_new_frame_token);
    if unsafe { new_token(&mut token, &index) } != sdk::OK || token.is_null() {
        return FEATURE_FAILED;
    }
    *out = token.cast_mut();
    OK
}

unsafe extern "C" fn sleep(handle: Handle, token: Handle) -> i32 {
    let host = unsafe { native(handle) };
    match host.sleep {
        Some(sleep) if unsafe { sleep(token) } == sdk::OK => OK,
        _ => FEATURE_FAILED,
    }
}

unsafe extern "C" fn marker(handle: Handle, token: Handle, marker: u32) -> i32 {
    let host = unsafe { native(handle) };
    match host.pcl_marker {
        Some(set) if unsafe { set(marker, token) } == sdk::OK => OK,
        _ => FEATURE_FAILED,
    }
}

unsafe extern "C" fn common(
    handle: Handle,
    token: Handle,
    viewport: u32,
    constants: *const c_void,
) -> i32 {
    let host = unsafe { native(handle) };
    let viewport = sdk::viewport(viewport);
    let set = unsafe { core(host.core().set_constants)(constants.cast(), token, &viewport) };
    if set == sdk::OK { OK } else { FEATURE_FAILED }
}

unsafe extern "C" fn reflex_state(
    handle: Handle,
    report: u32,
    inputs: &[u64; 4],
    out: &mut Reflex,
) -> i32 {
    let host = unsafe { native(handle) };
    let mut state = sdk::reflex_state();
    match host.reflex_state {
        Some(get) if unsafe { get(&mut *state) } == sdk::OK => {}
        _ => return FEATURE_FAILED,
    }
    out.low_latency_available = u32::from(state.low_latency_available != 0);
    out.latency_report_available = u32::from(state.latency_report_available != 0);
    if report == 0 || host.log.is_none() {
        return OK;
    }
    records::reflex_summary(
        &state,
        inputs,
        out.latency_report_available,
        host.log,
        host.log_user,
    );
    host.report_driver_sleep();
    OK
}

unsafe extern "C" fn reflex_options(handle: Handle, mode: u32, limit: u32) -> i32 {
    let host = unsafe { native(handle) };
    let options = records::pack_reflex(mode, limit);
    match host.reflex_options {
        Some(set) if unsafe { set(&options) } == sdk::OK => {}
        _ => return FEATURE_FAILED,
    }
    if host.log.is_some() {
        records::report_reflex_options(
            mode,
            limit,
            options.optimize_markers.into(),
            host.log,
            host.log_user,
        );
        host.report_driver_sleep();
    }
    OK
}

unsafe extern "C" fn pcl_message(handle: Handle, out: &mut u32) -> i32 {
    let host = unsafe { native(handle) };
    let mut state = sdk::pcl_state();
    match host.pcl_state {
        Some(get) if unsafe { get(&mut state) } == sdk::OK => {
            *out = state.window_message;
            OK
        }
        _ => FEATURE_FAILED,
    }
}

unsafe extern "C" fn driver_sleep(handle: Handle) {
    unsafe { native(handle) }.report_driver_sleep();
}

// The driver's own record of its last 64 frames: marker times as it received them and its
// queue and GPU spans. Written when a sleep stalls, and a few times while it does not, to show
// what the driver saw before it chose its interval.
unsafe extern "C" fn ring_capture(handle: Handle) -> u32 {
    let host = unsafe { native(handle) };
    let mut ring = RING_STATE.lock().unwrap();
    let state = &mut ring.insert(Ring(sdk::reflex_state())).0;
    match host.reflex_state {
        Some(get) => u32::from(
            unsafe { get(&mut **state) } == sdk::OK && state.latency_report_available != 0,
        ),
        None => 0,
    }
}

unsafe extern "C" fn ring_write(handle: Handle, reason: *const c_char, source: u64) {
    let host = unsafe { native(handle) };
    if host.log.is_none() {
        return;
    }
    let ring = RING_STATE.lock().unwrap();
    let Some(Ring(state)) = ring.as_ref() else {
        return;
    };
    let reason = unsafe { CStr::from_ptr(reason) };
    records::reflex_ring(state, reason, source, host.log, host.log_user);
}

// FEATURE_FAILED means the plugin state did not change. Any other result reports the
// feature functions now resolved.
unsafe extern "C" fn generation_load(
    handle: Handle,
    enabled: u32,
    require_signature: u32,
    entries: &mut u32,
) -> i32 {
    let host = unsafe { native(handle) };
    if enabled != 0 {
        unsafe { prime(host, host.native_device_raw(), require_signature) };
    }
    let loaded = core(host.core().set_feature_loaded);
    if unsafe { loaded(sdk::FEATURE_DLSS_G, enabled != 0) } != sdk::OK {
        return FEATURE_FAILED;
    }
    host.get_state = None;
    host.set_options = None;
    // Set options is looked up only after get state resolved, as before.
    let found = enabled == 0
        || unsafe {
            host.get_state = host
                .feature(sdk::FEATURE_DLSS_G, c"slDLSSGGetState".as_ptr())
                .and_then(|entry| sdk_api::function(entry));
            host.get_state.is_some() && {
                host.set_options = host
                    .feature(sdk::FEATURE_DLSS_G, c"slDLSSGSetOptions".as_ptr())
                    .and_then(|entry| sdk_api::function(entry));
                host.set_options.is_some()
            }
        };
    *entries = host.resolved();
    if !found {
        return MISSING_ENTRY;
    }
    if enabled != 0 {
        unsafe { prepare(host) };
    }
    OK
}

unsafe extern "C" fn same_graphics(handle: Handle, device: Handle, queue: Handle) -> u32 {
    let host = unsafe { native(handle) };
    let ours_device = host.device.as_ref().map_or(null_mut(), Interface::as_raw);
    let ours_queue = host.queue.as_ref().map_or(null_mut(), Interface::as_raw);
    u32::from(device == ours_device && queue == ours_queue)
}

unsafe extern "C" fn support(handle: Handle, log: Log, user: Handle) -> i32 {
    let host = unsafe { native(handle) };
    let Some(device) = host.native_device.as_ref() else {
        return NOT_SUPPORTED;
    };
    let mut luid = crate::streamline_sr::luid_bits(unsafe { device.GetAdapterLuid() });
    let info = sdk::adapter(&mut luid);
    let supported = core(host.core().is_feature_supported);
    let fg = unsafe { supported(sdk::FEATURE_DLSS_G, &info) };
    let reflex = unsafe { supported(sdk::FEATURE_REFLEX, &info) };
    if fg == sdk::OK && reflex == sdk::OK {
        return OK;
    }
    records::report_support(fg as u32, reflex as u32, log, user);
    NOT_SUPPORTED
}

unsafe extern "C" fn version(handle: Handle, status: &mut Status) {
    let host = unsafe { native(handle) };
    let mut version = sdk::feature_version();
    let get = core(host.core().get_feature_version);
    let found = unsafe { get(sdk::FEATURE_DLSS_G, &mut version) } == sdk::OK
        && sdk::version_present(&version.ngx);
    records::format_version(found.then_some(&version), status);
}

/// Recognizes an already upgraded factory-created interface, else upgrades the owned one.
unsafe fn upgrade_chain(host: &Native, chain: &mut Handle) -> i32 {
    let native = unsafe { host.alias(*chain) };
    if native.is_null() {
        return FEATURE_FAILED;
    }
    let upgraded = native != *chain;
    if upgraded || unsafe { core(host.core().upgrade_interface)(chain) } == sdk::OK {
        OK
    } else {
        FEATURE_FAILED
    }
}

// Returns one owned, upgraded IDXGISwapChain4 reference.
unsafe extern "C" fn chain(handle: Handle, desc: &ChainDesc, out: &mut Handle) -> i32 {
    let host = unsafe { native(handle) };
    let mut description = DXGI_SWAP_CHAIN_DESC1::default();
    unsafe { rsf_core_fg12_chain_description(desc, &mut description) };
    let (Some(factory), Some(queue)) = (host.factory.as_ref(), host.queue.as_ref()) else {
        return INIT_FAILED;
    };
    let created = unsafe {
        factory.CreateSwapChainForHwnd(
            queue,
            HWND(desc.hwnd),
            &description,
            None,
            None::<&IDXGIOutput>,
        )
    };
    let Ok(created) = created else {
        return INIT_FAILED;
    };
    let mut raw = created.into_raw();
    if unsafe { upgrade_chain(host, &mut raw) } != OK {
        drop(unsafe { IUnknown::from_raw(raw) });
        return INIT_FAILED;
    }
    let upgraded = unsafe { IDXGISwapChain1::from_raw(raw) };
    let Ok(chain4) = upgraded.cast::<IDXGISwapChain4>() else {
        return INIT_FAILED;
    };
    *out = chain4.into_raw();
    OK
}

unsafe extern "C" fn options(handle: Handle, request: &Request) -> i32 {
    let host = unsafe { native(handle) };
    let options = records::pack_options(
        request.mode,
        request.frames,
        request.ui_recomposition,
        request.dynamic_target_fps,
    );
    let viewport = sdk::viewport(0);
    match host.set_options {
        Some(set) if unsafe { set(&viewport, &options) } == sdk::OK => OK,
        _ => FEATURE_FAILED,
    }
}

// sl::DLSSGOptions{}: mode eOff with the SDK member defaults.
fn disabled_options() -> sdk::FgOptions {
    let mut options = records::pack_options(0, 1, 0, 0.0);
    options.flags = 0;
    options
}

unsafe extern "C" fn off(handle: Handle) {
    let host = unsafe { native(handle) };
    if let Some(set) = host.set_options {
        let viewport = sdk::viewport(0);
        unsafe { set(&viewport, &disabled_options()) };
    }
}

unsafe extern "C" fn observe(handle: Handle, out: &mut Observed) -> i32 {
    let host = unsafe { native(handle) };
    let mut state = sdk::dlssg_state();
    let viewport = sdk::viewport(0);
    match host.get_state {
        Some(get) if unsafe { get(&viewport, &mut state, null()) } == sdk::OK => {}
        _ => return FEATURE_FAILED,
    }
    out.max_generated_frames = state.max_generated;
    out.dynamic_supported = u32::from(state.dynamic_mfg == sdk::TRUE);
    out.vsync_supported = u32::from(state.vsync == sdk::TRUE);
    out.min_dimension = state.min_dimension;
    out.status = state.status as i32;
    unsafe { mfg_state::rsf_core_mfg_status(host.mfg, &mut out.mfg_status, &mut out.mfg_maximum) };
    out.presented = state.actual_presents.into();
    out.fence = state.inputs_fence;
    out.fence_value = state.inputs_fence_value;
    OK
}

unsafe extern "C" fn frame(handle: Handle, frame: *const Frame) -> i32 {
    unsafe { rsf_core_fg12_frame(frame, native(handle).native_device_raw()) }
}

unsafe extern "C" fn command(handle: Handle, command: Handle) -> i32 {
    let host = unsafe { native(handle) };
    let native_command = unsafe { host.alias(command) };
    if native_command.is_null() {
        return INVALID;
    }
    unsafe { rsf_core_fg12_command(native_command, host.native_device_raw()) }
}

// Builds the frame's common constants. Sets them only when SR has not already done so.
unsafe extern "C" fn constants(
    handle: Handle,
    token: Handle,
    frame: &Frame,
    reset: u32,
    apply: u32,
) -> i32 {
    let host = unsafe { native(handle) };
    let mut constants = unsafe { records::pack_fg_camera(frame, reset) };
    constants.previous_to_clip = sdk::full_invert(&constants.clip_to_previous);
    if !constants
        .previous_to_clip
        .iter()
        .all(|value| value.is_finite())
    {
        return INVALID;
    }
    let viewport = sdk::viewport(0);
    if apply != 0
        && unsafe { core(host.core().set_constants)(&constants, token, &viewport) } != sdk::OK
    {
        return FEATURE_FAILED;
    }
    OK
}

unsafe extern "C" fn tags(handle: Handle, token: Handle, command: Handle, frame: &Frame) -> i32 {
    let host = unsafe { native(handle) };
    let (planned, count) = records::plan_tags(frame);
    let mut resources: [sdk::Resource; 5] =
        std::array::from_fn(|i| sdk::texture(planned[i].resource, planned[i].state));
    let tags: [sdk::ResourceTag; 5] = std::array::from_fn(|i| {
        sdk::tag(&raw mut resources[i], planned[i].kind, planned[i].extent)
    });
    let viewport = sdk::viewport(0);
    let set_tags = core(host.core().set_tag_for_frame);
    let set = unsafe { set_tags(token, &viewport, tags.as_ptr(), count, command) };
    if set == sdk::OK { OK } else { FEATURE_FAILED }
}

static STARTUP: Startup = Startup {
    engine_count: sdk::ENGINE_COUNT,
    interposer_loaded,
    adapter_vendor,
    report_vendor,
    load,
    unload: crate::sdk_module::rsf_core_vk_sdk_unload,
    allocate,
    bind_required,
    init,
    borrow_device,
    create_device,
    prime_mfg,
    upgrade_device,
    native_device,
    start_audit,
    register_device,
    create_factory,
    upgrade_factory,
    queue_type,
    queue_matches_device,
    create_queue,
    native_queue,
    resolve_feature,
    prepare_mfg,
    pcl_defaults,
};

static PLATFORM: Platform = Platform {
    startup: &STARTUP,
    dispose,
    token,
    sleep,
    marker,
    common,
    reflex_state,
    reflex_options,
    pcl_message,
    driver_sleep,
    ring_capture,
    ring_write,
    generation_load,
    same_graphics,
    support,
    version,
    chain,
    options,
    off,
    observe,
    frame,
    command,
    constants,
    tags,
};

// The DLSS-G generation provider.

unsafe extern "C" fn create(
    setup: *const GenerationSetup,
    out: *mut Handle,
    chain: *mut Handle,
) -> i32 {
    let header = unsafe { rsf_core_fg12_setup_header(setup, out, chain) };
    if header != OK {
        return header;
    }
    unsafe { rsf_core_slhost_session_create(setup, out, chain, &PLATFORM) }
}

unsafe extern "C" fn configure(session: Handle, options: *const Options) -> i32 {
    unsafe { rsf_core_slhost_session_configure(session, options, &PLATFORM) }
}

unsafe extern "C" fn begin_frame(session: Handle, id: u64) -> i32 {
    unsafe { rsf_core_slhost_session_begin(session, id, &PLATFORM) }
}

unsafe extern "C" fn session_marker(session: Handle, marker: u32, id: u64, controller: u32) -> i32 {
    unsafe { rsf_core_slhost_session_marker(session, marker, id, controller, &PLATFORM) }
}

unsafe extern "C" fn prepare_frame(session: Handle, command: Handle, frame: *const Frame) -> i32 {
    unsafe { rsf_core_slhost_session_prepare(session, command, frame, &PLATFORM) }
}

unsafe extern "C" fn after_present(session: Handle) -> i32 {
    unsafe { rsf_core_slhost_session_after(session, &PLATFORM) }
}

unsafe extern "C" fn destroy_session(session: Handle) {
    unsafe { rsf_core_slhost_session_destroy(session, &PLATFORM) };
}

static PROVIDER: Provider = Provider {
    struct_size: size_of::<Provider>() as u32,
    create: Some(create),
    configure: Some(configure),
    begin_frame: Some(begin_frame),
    marker: Some(session_marker),
    prepare: Some(prepare_frame),
    after_present: Some(after_present),
    status: Some(rsf_core_slhost_session_status),
    retirement: Some(rsf_core_slhost_session_retirement),
    destroy: Some(destroy_session),
    abort_frame: Some(rsf_core_slhost_session_abort),
    destroy_checked: None,
};

#[unsafe(no_mangle)]
extern "C" fn rsf_generation_dlss() -> *const Provider {
    &PROVIDER
}

/// Shared D3D11 DLSS SR sets this frame's common constants.
pub(crate) unsafe fn common_set(
    host: *mut Host,
    id: u64,
    viewport: u32,
    constants: &sdk::Constants,
) -> bool {
    let constants = std::ptr::from_ref(constants).cast();
    unsafe { rsf_core_slhost_common(host, id, viewport, constants, &PLATFORM) != 0 }
}

/// What an SR route borrows from a live host: its interposer, entry table and native device.
pub(crate) struct Shared {
    pub(crate) module: Handle,
    pub(crate) entries: Entries,
    pub(crate) native_device: Handle,
}

/// The session a live host lends to DLSS SR. None for a null or unopened host.
pub(crate) unsafe fn shared(host: *mut Host) -> Option<Shared> {
    let native = unsafe { host_native(host) }?;
    Some(Shared {
        module: unsafe { rsf_core_slhost_module(host) },
        entries: *native.core.as_ref()?,
        native_device: raw(&native.native_device),
    })
}

// Public host entry points (rescaleframe/streamline_host.h).

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_create(setup: *const Setup, out: *mut *mut Host) -> i32 {
    unsafe { rsf_core_slhost_create(setup, null_mut(), null_mut(), out, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_adopt(
    setup: *const Setup,
    device: Handle,
    queue: Handle,
    out: *mut *mut Host,
) -> i32 {
    if device.is_null() || queue.is_null() {
        return INVALID;
    }
    unsafe { rsf_core_slhost_create(setup, device, queue, out, &PLATFORM) }
}

/// The native state of a live host.
unsafe fn host_native<'a>(host: *mut Host) -> Option<&'a mut Native> {
    let native = unsafe { rsf_core_slhost_native(host) };
    unsafe { native.cast::<Native>().as_mut() }
}

#[repr(C)]
struct Graphics {
    struct_size: u32,
    device: Handle,
    queue: Handle,
    factory: Handle,
    native_device: Handle,
    native_queue: Handle,
}

const _: () = assert!(size_of::<Graphics>() == 48);

fn raw<T: Interface>(object: &Option<T>) -> Handle {
    object.as_ref().map_or(null_mut(), Interface::as_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_graphics(host: *mut Host, out: *mut Graphics) -> i32 {
    if host.is_null() || !unsafe { has_capacity(out) } {
        return INVALID;
    }
    let Some(native) = (unsafe { host_native(host) }) else {
        return INVALID;
    };
    let out = unsafe { &mut *out };
    out.device = raw(&native.device);
    out.queue = raw(&native.queue);
    out.factory = raw(&native.factory);
    out.native_device = raw(&native.native_device);
    out.native_queue = raw(&native.native_queue);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_module(host: *mut Host) -> Handle {
    unsafe { rsf_core_slhost_module(host) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_drain(host: *mut Host) -> i32 {
    unsafe { drain(host) }
}

/// Waits for the host's proxy queue to finish its submitted work.
pub(crate) unsafe fn drain(host: *mut Host) -> i32 {
    let Some(native) = (unsafe { host_native(host) }) else {
        return INVALID;
    };
    let (Some(queue), Some(device)) = (native.queue.as_ref(), native.device.as_ref()) else {
        return INVALID;
    };
    let Ok(fence) = (unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }) else {
        return FEATURE_FAILED;
    };
    let event = unsafe { CreateEventW(null(), 0, 0, null()) };
    if event.is_null() {
        return FEATURE_FAILED;
    }
    let ready = unsafe { queue.Signal(&fence, 1) }.is_ok()
        && unsafe {
            crate::d3d12_host::rsf_core_d3d12_wait_native_fence(
                fence.as_raw(),
                1,
                event,
                DRAIN_TIMEOUT_MS,
            )
        } >= 0;
    unsafe { CloseHandle(event) };
    if ready { OK } else { FEATURE_FAILED }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_token(host: *mut Host, id: u64) -> Handle {
    unsafe { rsf_core_slhost_token(host, id) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_destroy(host: *mut Host) -> i32 {
    unsafe { rsf_core_slhost_destroy(host, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_begin(host: *mut Host, id: u64) -> i32 {
    unsafe { rsf_core_slhost_begin(host, id, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_acquire(host: *mut Host, id: u64) -> i32 {
    unsafe { rsf_core_slhost_acquire(host, id, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_sleep(host: *mut Host, id: u64) -> i32 {
    unsafe { rsf_core_slhost_sleep(host, id, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_marker(
    host: *mut Host,
    id: u64,
    marker: u32,
    controller: u32,
) -> i32 {
    unsafe { rsf_core_slhost_marker(host, id, marker, controller, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_abort(host: *mut Host, id: u64) -> i32 {
    unsafe { rsf_core_slhost_abort(host, id) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_reset_sources(host: *mut Host) -> i32 {
    unsafe { rsf_core_slhost_reset_sources(host) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_retire_through(host: *mut Host, id: u64) -> i32 {
    unsafe { rsf_core_slhost_retire_through(host, id) }
}

unsafe extern "C" fn resolve_common(handle: Handle, entries: &mut [Handle; 2]) -> u32 {
    let host = unsafe { native(handle) };
    let function = core(host.core().get_feature_function);
    let [before, after] = entries;
    u32::from(
        unsafe { function(sdk::FEATURE_COMMON, c"slHookPresent".as_ptr(), before) } == sdk::OK
            && unsafe { function(sdk::FEATURE_COMMON, c"slHookAfterPresent".as_ptr(), after) }
                == sdk::OK,
    )
}

unsafe extern "C" fn common_before(entry: Handle, chain: Handle, interval: u32, flags: u32) -> u32 {
    let hook = unsafe { transmute::<Handle, sdk::HookPresent>(entry) };
    let mut skip = false;
    let result = unsafe { hook(chain, interval, flags, &mut skip) };
    u32::from(result >= 0 && !skip)
}

unsafe extern "C" fn common_after(entry: Handle, flags: u32) -> u32 {
    let hook = unsafe { transmute::<Handle, sdk::HookAfterPresent>(entry) };
    u32::from(unsafe { hook(flags) } >= 0)
}

static COMMON_PRESENT: CommonPresent = CommonPresent {
    resolve: resolve_common,
    before: common_before,
    after: common_after,
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_present_common(
    host: *mut Host,
    chain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    unsafe { rsf_core_slhost_present_common(host, chain, interval, flags, begin, &COMMON_PRESENT) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_presented(host: *mut Host, id: u64) -> i32 {
    unsafe { rsf_core_slhost_presented(host, id) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_input(
    host: *mut Host,
    id: u64,
    kinds: u32,
    message: u32,
) -> i32 {
    unsafe { rsf_core_slhost_input(host, id, kinds, message, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_reflex(host: *mut Host, mode: u32, limit: u32) -> i32 {
    unsafe { rsf_core_slhost_reflex(host, mode, limit, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_latency_status(
    host: *mut Host,
    out: *mut LatencyStatus,
) -> i32 {
    unsafe { rsf_core_slhost_latency_status(host, out, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_upgrade_chain(host: *mut Host, chain: *mut Handle) -> i32 {
    let Some(chain) = (unsafe { chain.as_mut() }).filter(|chain| !chain.is_null()) else {
        return INVALID;
    };
    let Some(native) = (unsafe { host_native(host) }) else {
        return INVALID;
    };
    unsafe { upgrade_chain(native, chain) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_generation_load(host: *mut Host, enabled: u32) -> i32 {
    unsafe { rsf_core_slhost_generation_load(host, enabled, &PLATFORM) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_native(host: *mut Host, proxy: Handle) -> Handle {
    if proxy.is_null() {
        return null_mut();
    }
    let Some(native) = (unsafe { host_native(host) }) else {
        return null_mut();
    };
    unsafe { native.alias(proxy) }
}
