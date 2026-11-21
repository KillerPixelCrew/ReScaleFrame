// SPDX-License-Identifier: MIT
mod planning;
mod tags;

use crate::com_ref::ComRef;
use crate::diagnostic_output::Output;
use crate::dlss_streamline_abi::{rsf_dlss_plan as Plan, rsf_dlss_support as Support};
use crate::gl_pipeline_abi::{rsf_dlss_log_fn as Log, rsf_dlss_setup as Setup};
use crate::render_abi::{RSF_DLSS_ABI_VERSION, rsf_dlss_frame as Frame};
use crate::sdk_module::{SdkModule, wide_path};
use crate::sr_session::has_capacity;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{NonNull, addr_of, null, null_mut};
use std::sync::Mutex;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::core::w;

type Handle = *mut c_void;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const LOAD_FAILED: i32 = -4;
const MISSING_ENTRY: i32 = -5;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const OPTIMAL: u32 = 1;
const OPTIONS: u32 = 2;
const USE_MANUAL_HOOKING: u64 = 1 << 2;
const USE_FRAME_BASED_RESOURCE_TAGGING: u64 = 1 << 7;
const ENGINE_CUSTOM: u32 = 0;
const ENGINE_UNREAL: u32 = 1;
const ENGINE_UNITY: u32 = 2;
const RENDER_D3D11: u32 = 0;
const LOG_DEFAULT: u32 = 1;
const FEATURE_DLSS: u32 = 0;

const SHARED_ENTRY_COUNT: usize = 7;
const ENTRY_NAMES: [&CStr; 12] = [
    c"slIsFeatureSupported",
    c"slGetFeatureFunction",
    c"slGetFeatureRequirements",
    c"slSetConstants",
    c"slSetTagForFrame",
    c"slEvaluateFeature",
    c"slFreeResources",
    c"slInit",
    c"slShutdown",
    c"slSetD3DDevice",
    c"slGetNewFrameToken",
    c"slAllocateResources",
];

type Entries = [Handle; ENTRY_NAMES.len()];

unsafe fn resolve_entries(
    module: Handle,
    owned: bool,
    addresses: &mut Entries,
    missing: unsafe extern "C" fn(*const c_char),
) -> i32 {
    // The shared host already owns initialization, the device, tokens and allocation.
    let count = if owned {
        ENTRY_NAMES.len()
    } else {
        SHARED_ENTRY_COUNT
    };
    for (address, name) in addresses.iter_mut().zip(&ENTRY_NAMES[..count]) {
        let Some(entry) = (unsafe { GetProcAddress(module, name.as_ptr().cast()) }) else {
            unsafe { missing(name.as_ptr()) };
            return MISSING_ENTRY;
        };
        *address = entry as *const () as Handle;
    }
    OK
}

unsafe fn resolve_features(
    native: Handle,
    strict: bool,
    lookup: unsafe extern "C" fn(Handle, *const c_char, &mut Handle) -> i32,
    bind: unsafe extern "C" fn(Handle, Handle, Handle),
    features: &mut u32,
) -> i32 {
    let mut optimal = null_mut();
    let mut options = null_mut();
    for (name, address) in [
        (c"slDLSSGetOptimalSettings", &mut optimal),
        (c"slDLSSSetOptions", &mut options),
    ] {
        let status = unsafe { lookup(native, name.as_ptr(), address) };
        // Only the owned-device probe uses addresses returned alongside a vendor error.
        if status == FEATURE_FAILED || (strict && status != OK) {
            return status;
        }
    }

    unsafe { bind(native, optimal, options) };
    let optimal_bit = if optimal.is_null() { 0 } else { OPTIMAL };
    let options_bit = if options.is_null() { 0 } else { OPTIONS };
    let resolved = optimal_bit | options_bit;
    *features = resolved;
    if resolved == (OPTIMAL | OPTIONS) {
        OK
    } else {
        MISSING_ENTRY
    }
}

#[cfg(test)]
#[test]
fn shared_features_refuse_a_vendor_error_even_with_an_address() {
    use std::cell::Cell;
    use std::ptr::without_provenance_mut;

    thread_local! {
        static BOUND: Cell<bool> = const { Cell::new(false) };
    }

    unsafe extern "C" fn lookup(_: Handle, name: *const c_char, address: &mut Handle) -> i32 {
        if unsafe { CStr::from_ptr(name) } == c"slDLSSGetOptimalSettings" {
            *address = without_provenance_mut(1);
            MISSING_ENTRY
        } else {
            *address = without_provenance_mut(2);
            OK
        }
    }
    unsafe extern "C" fn bind(_: Handle, optimal: Handle, options: Handle) {
        assert_eq!((optimal.addr(), options.addr()), (1, 2));
        BOUND.set(true);
    }
    let mut features = 0;
    let result = unsafe { resolve_features(null_mut(), true, lookup, bind, &mut features) };
    assert_eq!((result, features, BOUND.get()), (MISSING_ENTRY, 0, false));
    let result = unsafe { resolve_features(null_mut(), false, lookup, bind, &mut features) };
    assert_eq!(
        (result, features, BOUND.get()),
        (OK, OPTIMAL | OPTIONS, true)
    );
}

#[repr(C)]
struct InitPreferences {
    plugins: *const u16,
    logs: *const u16,
    engine_version: *const c_char,
    project_id: *const c_char,
    flags: u64,
    engine: u32,
    application_id: u32,
    render_api: u32,
    log_level: u32,
    show_console: u32,
    feature: u32,
}

#[repr(C)]
struct Startup {
    allocate: unsafe extern "C" fn() -> Handle,
    dispose: unsafe extern "C" fn(Handle),
    set_log: unsafe extern "C" fn(Log, Handle),
    missing_entry: unsafe extern "C" fn(*const c_char),
    bind_entries: unsafe extern "C" fn(Handle, &Entries),
    init: unsafe extern "C" fn(Handle, &InitPreferences) -> i32,
    host_module: unsafe extern "C" fn(Handle) -> Handle,
    host_device: unsafe extern "C" fn(Handle) -> Handle,
    register_device: unsafe extern "C" fn(Handle, Handle) -> i32,
    lookup_feature: unsafe extern "C" fn(Handle, *const c_char, &mut Handle) -> i32,
    bind_features: unsafe extern "C" fn(Handle, Handle, Handle),
    report: unsafe extern "C" fn(*const c_char),
}

impl Startup {
    fn report_text(&self, line: String) {
        if let Ok(line) = CString::new(line) {
            unsafe { (self.report)(line.as_ptr()) };
        }
    }

    fn open(&self, module: Handle, owned: bool) -> Result<NonNull<c_void>, i32> {
        let native = NonNull::new(unsafe { (self.allocate)() }).ok_or(INIT_FAILED)?;
        let mut entries = [null_mut(); ENTRY_NAMES.len()];
        let result = unsafe { resolve_entries(module, owned, &mut entries, self.missing_entry) };
        if result != OK {
            unsafe { (self.dispose)(native.as_ptr()) };
            return Err(result);
        }
        unsafe { (self.bind_entries)(native.as_ptr(), &entries) };
        Ok(native)
    }

    fn start_owned(
        &self,
        module: Handle,
        setup: &Setup,
        plugins: *const u16,
        logs: *const u16,
    ) -> Result<NonNull<c_void>, i32> {
        unsafe { (self.set_log)(setup.log, setup.log_user) };
        let native = match self.open(module, true) {
            Ok(native) => native,
            Err(result) => {
                unsafe { (self.set_log)(None, null_mut()) };
                return Err(result);
            }
        };
        let preferences = InitPreferences {
            plugins,
            logs,
            engine_version: setup.engine_version_utf8,
            project_id: setup.project_id_utf8,
            flags: USE_MANUAL_HOOKING | USE_FRAME_BASED_RESOURCE_TAGGING,
            engine: match setup.engine {
                ENGINE_UNREAL => ENGINE_UNREAL,
                ENGINE_UNITY => ENGINE_UNITY,
                _ => ENGINE_CUSTOM,
            },
            application_id: setup.application_id,
            render_api: RENDER_D3D11,
            log_level: LOG_DEFAULT,
            show_console: 0,
            feature: FEATURE_DLSS,
        };
        let result = unsafe { (self.init)(native.as_ptr(), &preferences) };
        if result != OK {
            unsafe {
                (self.dispose)(native.as_ptr());
                (self.set_log)(None, null_mut());
            }
            return Err(result);
        }
        unsafe { (self.report)(c"streamline loaded and initialised".as_ptr()) };
        Ok(native)
    }

    fn borrow_host(
        &self,
        host: Handle,
        log: Log,
        user: Handle,
    ) -> Result<(NonNull<c_void>, ComRef), i32> {
        let module = unsafe { (self.host_module)(host) };
        if module.is_null() {
            return Err(NOT_READY);
        }
        let device = unsafe { (self.host_device)(host) };
        if device.is_null() {
            return Err(NOT_READY);
        }
        let native = self.open(module, false)?;
        let mut features = 0;
        let result = unsafe {
            resolve_features(
                native.as_ptr(),
                true,
                self.lookup_feature,
                self.bind_features,
                &mut features,
            )
        };
        if result != OK {
            unsafe { (self.dispose)(native.as_ptr()) };
            return Err(if result == FEATURE_FAILED {
                INIT_FAILED
            } else {
                result
            });
        }
        unsafe {
            (self.set_log)(log, user);
            (self.report)(c"DLSS SR shares the D3D12 FG host and CPU frame token".as_ptr());
        }
        Ok((native, unsafe { ComRef::retain(device) }))
    }

    fn set_device(&self, native: Handle, device: Handle, features: &mut u32) -> i32 {
        let result = unsafe { (self.register_device)(native, device) };
        if result != OK {
            return result;
        }
        let result = unsafe {
            resolve_features(
                native,
                false,
                self.lookup_feature,
                self.bind_features,
                features,
            )
        };
        if result == MISSING_ENTRY {
            unsafe { (self.report)(c"DLSS feature functions are not available".as_ptr()) };
        }
        result
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Options {
    mode: u32,
    width: u32,
    height: u32,
    hdr: u32,
    auto_exposure: u32,
    alpha: u32,
    preset: u32,
}

#[repr(C)]
struct Platform {
    load: unsafe extern "C" fn(*const u16, &Setup) -> Handle,
    startup: &'static Startup,
    planning: &'static planning::Platform,
    present: unsafe extern "C" fn(Handle, Handle, u32, u32, u32) -> i32,
    token: unsafe extern "C" fn(Handle, Handle, u64, u32) -> Handle,
    constants: unsafe extern "C" fn(Handle, Handle, Handle, u64, u32, &Frame) -> i32,
    options: unsafe extern "C" fn(Handle, u32, &Options) -> i32,
    tags: unsafe extern "C" fn(Handle, Handle, Handle, u32, &tags::TagBatch) -> i32,
    evaluate: unsafe extern "C" fn(Handle, Handle, Handle, u32) -> i32,
    drain: unsafe extern "C" fn(Handle) -> u32,
    release_viewport: unsafe extern "C" fn(Handle, u32) -> i32,
    shutdown: unsafe extern "C" fn(Handle) -> i32,
    destroy: unsafe extern "C" fn(Handle),
}

const _: () = {
    assert!(size_of::<Options>() == 28);
    assert!(size_of::<InitPreferences>() == 64);
    assert!(std::mem::offset_of!(InitPreferences, flags) == 32);
    assert!(size_of::<Startup>() == 12 * size_of::<Handle>());
    assert!(size_of::<Platform>() == 13 * size_of::<Handle>());
};

enum Route {
    Owned(Option<ComRef>),
    Shared {
        host: NonNull<c_void>,
        device: ComRef,
    },
}

impl Route {
    fn host(&self) -> Handle {
        match self {
            Self::Owned(_) => null_mut(),
            Self::Shared { host, .. } => host.as_ptr(),
        }
    }

    fn device(&self) -> Option<&ComRef> {
        match self {
            Self::Owned(device) => device.as_ref(),
            Self::Shared { device, .. } => Some(device),
        }
    }
}

struct Session {
    native: NonNull<c_void>,
    route: Route,
    features: u32,
    applied: [Option<Options>; 2],
    _module: Option<SdkModule>,
    _directories: [Option<Vec<u16>>; 2],
}

// Native calls and access to the borrowed graphics host are serialized here. The caller keeps the
// shared host alive until SR shutdown, and each COM device is retained until its route is retired.
unsafe impl Send for Session {}
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn viewport_slot(viewport: u32) -> u32 {
    if viewport < 2 { viewport } else { 0 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_load(
    setup: *const Setup,
    platform: &Platform,
) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.interposer_path_utf8.is_null() {
        return INVALID;
    }
    if setup.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    if state.is_some() {
        return OK;
    }
    if !unsafe { GetModuleHandleW(w!("sl.interposer.dll")) }.is_null() {
        if let Some(log) = setup.log
            && let Some(_output) = Output::try_enter()
        {
            unsafe {
                log(setup.log_user, c"D3D11 DLSS cannot initialize over an existing Streamline owner; use the shared D3D12 host".as_ptr());
            }
        }
        return INIT_FAILED;
    }
    let Some(path) = (unsafe { wide_path(setup.interposer_path_utf8) }) else {
        return INVALID;
    };
    let Some(module) = SdkModule::adopt_system(unsafe { (platform.load)(path.as_ptr(), setup) })
    else {
        return LOAD_FAILED;
    };
    let directories = unsafe {
        [
            wide_path(setup.plugin_directory_utf8),
            wide_path(setup.log_directory_utf8),
        ]
    };
    let [plugins, logs] = directories
        .each_ref()
        .map(|directory| directory.as_deref().map_or(null(), <[u16]>::as_ptr));
    let native = match platform
        .startup
        .start_owned(module.as_ptr(), setup, plugins, logs)
    {
        Ok(native) => native,
        Err(result) => return result,
    };
    *state = Some(Session {
        native,
        route: Route::Owned(None),
        features: 0,
        applied: [None; 2],
        _module: Some(module),
        _directories: directories,
    });
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_share_host(
    host: Handle,
    log: Log,
    user: Handle,
    platform: &Platform,
) -> i32 {
    let Some(host) = NonNull::new(host) else {
        return NOT_READY;
    };
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    if state.is_some() {
        return NOT_READY;
    }
    let (native, device) = match platform.startup.borrow_host(host.as_ptr(), log, user) {
        Ok(route) => route,
        Err(result) => return result,
    };
    *state = Some(Session {
        native,
        route: Route::Shared { host, device },
        features: OPTIMAL | OPTIONS,
        applied: [None; 2],
        _module: None,
        _directories: [None, None],
    });
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_set_device(
    device: Handle,
    platform: &Platform,
) -> i32 {
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    if state
        .as_ref()
        .is_some_and(|session| !session.route.host().is_null())
    {
        return OK;
    }
    if device.is_null() {
        return INVALID;
    }
    let Some(session) = state.as_mut() else {
        return NOT_READY;
    };
    let mut capabilities = 0;
    let result = platform
        .startup
        .set_device(session.native.as_ptr(), device, &mut capabilities);
    if matches!(result, OK | MISSING_ENTRY) {
        session.route = Route::Owned(Some(unsafe { ComRef::retain(device) }));
        session.features = capabilities;
        session.applied = [None; 2];
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_support(
    support: *mut Support,
    platform: &Platform,
) -> i32 {
    if !unsafe { has_capacity(support) } {
        return INVALID;
    }
    let Ok(state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_ref() else {
        return NOT_READY;
    };
    let Some(device) = session.route.device() else {
        return NOT_READY;
    };
    let shared = !session.route.host().is_null();
    unsafe {
        platform.planning.support(
            session.native.as_ptr(),
            device.as_ptr(),
            shared,
            support,
            platform.startup,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_plan(plan: *mut Plan, platform: &Platform) -> i32 {
    if !unsafe { has_capacity(plan) } {
        return INVALID;
    }
    // The caller only needs to initialize the inputs.
    let (width, height) = unsafe {
        (
            addr_of!((*plan).output_width).read(),
            addr_of!((*plan).output_height).read(),
        )
    };
    if width == 0 || height == 0 {
        return INVALID;
    }
    let Ok(state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state
        .as_ref()
        .filter(|session| session.features & OPTIMAL != 0)
    else {
        return NOT_READY;
    };
    unsafe {
        platform
            .planning
            .plan(session.native.as_ptr(), plan, platform.startup)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_present(
    chain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
    platform: &Platform,
) -> i32 {
    if chain.is_null() || begin > 1 {
        return INVALID;
    }
    let Ok(state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_ref() else {
        return NOT_READY;
    };
    if !session.route.host().is_null() {
        return OK;
    }
    if session.route.device().is_none() {
        return NOT_READY;
    }
    unsafe { (platform.present)(session.native.as_ptr(), chain, interval, flags, begin) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_evaluate(
    context: Handle,
    frame: *const Frame,
    source: u64,
    platform: &Platform,
) -> i32 {
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let host = state
        .as_ref()
        .map_or(null_mut(), |session| session.route.host());
    let shared = !host.is_null();
    if shared != (source != 0) {
        return NOT_READY;
    }
    if context.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if [frame.color_in, frame.color_out, frame.depth, frame.motion]
        .into_iter()
        .any(|input| input.is_null())
    {
        return INVALID;
    }
    let Some(session) = state
        .as_mut()
        .filter(|session| session.features & OPTIONS != 0)
    else {
        return NOT_READY;
    };
    let native = session.native.as_ptr();
    let token = unsafe { (platform.token)(native, host, source, frame.frame_index) };
    if token.is_null() {
        return if shared { NOT_READY } else { FEATURE_FAILED };
    }
    let slot = viewport_slot(frame.viewport);
    let constants = unsafe { (platform.constants)(native, host, token, source, slot, frame) };
    if constants != OK {
        return constants;
    }

    let options = Options {
        mode: planning::dlss_mode(frame.quality),
        width: frame.output_width,
        height: frame.output_height,
        hdr: (frame.color_encoded == 0).into(),
        auto_exposure: (frame.exposure.is_null() && frame.color_encoded == 0).into(),
        alpha: (frame.alpha != 0).into(),
        preset: frame.preset,
    };
    let applied = &mut session.applied[slot as usize];
    if *applied != Some(options) {
        let status = unsafe { (platform.options)(native, slot, &options) };
        if status != OK {
            if status != INVALID {
                *applied = None;
            }
            return status;
        }
        *applied = Some(options);
    }
    let tags = tags::TagBatch::new(frame, shared);
    let tagged = unsafe { (platform.tags)(native, context, token, slot, &tags) };
    if tagged != OK {
        return tagged;
    }
    unsafe { (platform.evaluate)(native, context, token, slot) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_release(viewport: u32, platform: &Platform) -> i32 {
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_mut() else {
        return NOT_READY;
    };
    let host = session.route.host();
    if !host.is_null() && unsafe { (platform.drain)(host) } == 0 {
        return FEATURE_FAILED;
    }
    let slot = viewport_slot(viewport);
    session.applied[slot as usize] = None;
    unsafe { (platform.release_viewport)(session.native.as_ptr(), slot) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_shutdown(platform: &Platform) -> i32 {
    if Output::active_here() {
        return NOT_READY;
    }
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_ref() else {
        return NOT_READY;
    };
    let native = session.native.as_ptr();
    if matches!(&session.route, Route::Owned(_)) {
        let stopped = unsafe { (platform.shutdown)(native) };
        if stopped != OK {
            return stopped;
        }
    }
    // Keep the COM device, module and borrowed directories through native cleanup.
    let retired = state.take();
    unsafe { (platform.destroy)(native) };
    drop(retired);
    OK
}
