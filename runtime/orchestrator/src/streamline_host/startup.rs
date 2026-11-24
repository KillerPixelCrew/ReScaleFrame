// SPDX-License-Identifier: MIT
// Host admission and SDK startup: interposer load, slInit, the upgraded device, factory and
// queue, and the feature functions of the requested profile.
use super::*;
use crate::reflex_audit::rsf_reflex_audit_start;
use crate::sdk_module::wide_path;
use crate::streamline_sdk::{
    FEATURE_DLSS as DLSS, FEATURE_DLSS_G as DLSS_G, FEATURE_PCL as PCL, FEATURE_REFLEX as REFLEX,
    LOG_LEVEL_DEFAULT as LOG_DEFAULT, PREFERENCE_FRAME_TAGGING, PREFERENCE_MANUAL_HOOKING,
    RENDER_API_D3D12 as D3D12,
};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12CreateDevice,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, IDXGIAdapter};

const NVIDIA_VENDOR: u32 = 0x10de;

// `PreferenceFlags::eDisableDebugText` and `LogLevel::eVerbose`.
const DISABLE_DEBUG_TEXT: u64 = 1 << 1;
const LOG_VERBOSE: u32 = 2;

/// Whether the interposer exports every core entry the host calls.
fn has_required(entries: &Entries) -> bool {
    entries.init.is_some()
        && entries.shutdown.is_some()
        && entries.upgrade_interface.is_some()
        && entries.get_native_interface.is_some()
        && entries.set_d3d_device.is_some()
        && entries.is_feature_supported.is_some()
        && entries.get_feature_function.is_some()
        && entries.get_feature_version.is_some()
        && entries.set_feature_loaded.is_some()
        && entries.get_new_frame_token.is_some()
        && entries.set_constants.is_some()
        && entries.set_tag_for_frame.is_some()
}

pub(super) struct FeatureList {
    ids: [u32; 4],
    len: usize,
}

impl FeatureList {
    fn new(profile: u32, load_dlss_sr: bool) -> Self {
        // PCL has no sleeper and requests NGX only if shared DLSS SR needs it.
        let mut ids = [0; 4];
        let mut len = 0;
        for (id, enabled) in [
            (DLSS, profile != PROFILE_PCL || load_dlss_sr),
            (PCL, true),
            (REFLEX, profile != PROFILE_PCL),
            (DLSS_G, profile == PROFILE_DLSS_FG),
        ] {
            if enabled {
                ids[len] = id;
                len += 1;
            }
        }
        Self { ids, len }
    }

    fn contains(&self, id: u32) -> bool {
        self.ids[..self.len].contains(&id)
    }
}

unsafe fn admit(setup: &Setup) -> Result<(), i32> {
    if setup.engine_type >= sdk::ENGINE_COUNT {
        return Err(INVALID);
    }
    let candidate = unsafe { IUnknown::from_raw_borrowed(&setup.dxgi_adapter) };
    let adapter = candidate.and_then(|candidate| candidate.cast::<IDXGIAdapter>().ok());
    let Some(description) = adapter.and_then(|adapter| unsafe { adapter.GetDesc() }.ok()) else {
        return Err(INVALID);
    };
    if description.VendorId != NVIDIA_VENDOR {
        let vendor = description.VendorId;
        if let (Some(log), Ok(text)) = (
            setup.log,
            CString::new(format!(
                "Streamline host refused: adapter vendor 0x{vendor:04x} is not NVIDIA"
            )),
        ) {
            unsafe { log(setup.log_user, text.as_ptr()) };
        }
        return Err(NOT_SUPPORTED);
    }
    // An existing interposer owns the SDK and its device registration.
    if sdk::interposer_loaded() {
        return Err(NEEDS_RESTART);
    }
    Ok(())
}

pub(super) unsafe fn create_host(setup: &Setup, graphics: [Handle; 2]) -> Result<Box<Host>, i32> {
    if setup.dxgi_adapter.is_null()
        || setup.runtime_directory_utf8.is_null()
        || setup.profile > PROFILE_PCL
    {
        return Err(INVALID);
    }
    // Admission and SDK startup may log into the caller before publishing a host.
    let _owner = OWNER.try_lock().map_err(|_| NOT_READY)?;
    unsafe { admit(setup) }?;
    let mut engine_version = [0; 128];
    let mut project_id = [0; 256];
    if !unsafe { copy_text(&mut engine_version, setup.engine_version_utf8) }
        || !unsafe { copy_text(&mut project_id, setup.project_id_utf8) }
    {
        return Err(INVALID);
    }
    let Some(directory) = (unsafe { wide_path(setup.runtime_directory_utf8) }).filter(|path| {
        path.len() <= 1024 && rsf_game_sys::windows_path::absolute_path(path.iter().copied())
    }) else {
        return Err(INVALID);
    };
    let path: Vec<u16> = directory[..directory.len() - 1]
        .iter()
        .copied()
        .chain("\\sl.interposer.dll".encode_utf16())
        .chain([0])
        .collect();
    let log = crate::input_dump::Log::new(setup.log, setup.log_user);
    let module = unsafe { sdk::load_interposer(path.as_ptr(), setup.require_signature != 0, &log) };
    let module =
        SdkModule::adopt(module, crate::sdk_module::rsf_core_vk_sdk_unload).ok_or(LOAD_FAILED)?;
    let raw = crate::allocate_handle(Host {
        module,
        directory,
        paths: [null()],
        engine_version,
        project_id,
        profile: setup.profile,
        engine_type: setup.engine_type,
        features: FeatureList::new(setup.profile, setup.load_dlss_sr != 0),
        signature_verified: setup.require_signature != 0,
        development_runtime: setup.development_runtime != 0,
        log: setup.log,
        log_user: setup.log_user,
        initialized: false,
        core: Entries::default(),
        device: None,
        native_device: None,
        queue: None,
        native_queue: None,
        factory: None,
        generation: Mutex::new(Generation::default()),
        reflex_options: None,
        reflex_state: None,
        sleep: None,
        pcl_marker: None,
        pcl_options: None,
        pcl_state: None,
        mfg: mfg_state::rsf_core_mfg_create(),
        borrowers: AtomicU32::new(0),
        generation_loaded: AtomicBool::new(true),
        frames: Mutex::new(Frames::new(0)),
        common_present: Mutex::new(None),
        reflex: Mutex::new(()),
    });
    if raw.is_null() {
        return Err(INIT_FAILED);
    }
    let mut host = unsafe { Box::from_raw(raw) };
    let mut refused_queue_device = None;
    if let Err(result) = unsafe {
        initialize(
            &mut host,
            graphics,
            setup.dxgi_adapter,
            &mut refused_queue_device,
        )
    } {
        // The queue's device reference outlives the host's shutdown, as in the C++.
        drop(host);
        drop(refused_queue_device);
        return Err(result);
    }
    unsafe { resolve_features(&mut host) };
    Ok(host)
}

unsafe fn initialize(
    host: &mut Host,
    [device, queue]: [Handle; 2],
    adapter: Handle,
    refused_queue_device: &mut Option<IUnknown>,
) -> Result<(), i32> {
    let module = &host.module;
    let entries = unsafe {
        Entries::resolve_with(|name| {
            module
                .symbol(name)
                .map_or(null_mut(), |address| address as *const () as Handle)
        })
    };
    if !has_required(&entries) {
        return Err(MISSING_ENTRY);
    }
    host.core = entries;

    let mut flags = PREFERENCE_MANUAL_HOOKING | PREFERENCE_FRAME_TAGGING;
    let log_level = if host.development_runtime {
        LOG_VERBOSE
    } else {
        flags |= DISABLE_DEBUG_TEXT;
        LOG_DEFAULT
    };
    host.paths[0] = host.directory.as_ptr();
    streamline_log::set(
        LOG_SLOT,
        std::ptr::from_ref(&*host) as usize,
        host.log,
        host.log_user,
        host.development_runtime,
    );
    let preferences = sdk::Preferences {
        base: sdk::PREFERENCES,
        show_console: u8::from(host.development_runtime),
        log_level,
        plugin_paths: host.paths.as_mut_ptr(),
        num_plugin_paths: 1,
        log: Some(message),
        flags,
        features: host.features.ids.as_ptr(),
        num_features: host.features.len as u32,
        engine: host.engine_type,
        engine_version: host.engine_version.as_ptr(),
        project: host.project_id.as_ptr(),
        render_api: D3D12,
        ..Default::default()
    };
    host.initialized = unsafe { core(host.core.init)(&preferences, sdk::SDK_VERSION) } == sdk::OK;
    if !host.initialized {
        return Err(INIT_FAILED);
    }

    if !device.is_null() && !queue.is_null() {
        host.device = unsafe { ID3D12Device::from_raw_borrowed(&device) }.cloned();
    } else {
        let adapter = unsafe { IUnknown::from_raw_borrowed(&adapter) };
        let mut created: Option<ID3D12Device> = None;
        if unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut created) }.is_err()
            || created.is_none()
        {
            return Err(INIT_FAILED);
        }
        host.device = created;
    }
    if host.features.contains(DLSS_G) {
        unsafe { prime(host, Host::raw(&host.device)) };
    }
    let mut device = host.device.take();
    let upgraded = unsafe { upgrade(host, &mut device) };
    host.device = device;
    if !upgraded {
        return Err(INIT_FAILED);
    }
    host.native_device = unsafe { retain_native(host, &host.device) };
    if host.native_device.is_none() {
        return Err(INIT_FAILED);
    }
    unsafe { rsf_reflex_audit_start(host.log, host.log_user) };
    if unsafe { core(host.core.set_d3d_device)(Host::raw(&host.device)) } != sdk::OK {
        return Err(INIT_FAILED);
    }
    let Ok(factory) =
        (unsafe { CreateDXGIFactory2::<IDXGIFactory2>(DXGI_CREATE_FACTORY_FLAGS(0)) })
    else {
        return Err(INIT_FAILED);
    };
    let mut factory = Some(factory);
    let upgraded = unsafe { upgrade(host, &mut factory) };
    host.factory = factory;
    if !upgraded {
        return Err(INIT_FAILED);
    }
    if !queue.is_null() {
        let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.expect("live queue");
        if unsafe { queue.GetDesc() }.Type != D3D12_COMMAND_LIST_TYPE_DIRECT {
            return Err(WRONG_API);
        }
        let mut queue_device: Option<ID3D12Device> = None;
        let found = unsafe { queue.GetDevice(&mut queue_device) }.is_ok();
        // The same interface pointer, as the native comparison required.
        let matches = found
            && queue_device
                .as_ref()
                .is_some_and(|device| device.as_raw() == host.native_device_raw());
        if !matches {
            // The queue's GetDevice reference outlives the host's shutdown on refusal.
            *refused_queue_device = queue_device.map(|device| device.into());
            return Err(WRONG_API);
        }
    }
    // The proxy queue unwraps Streamline command lists and registers the DLSS-G hooks.
    // The presentation bridge retains the borrowed engine queue and joins the two queues.
    let description = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    let created = host.device.as_ref().and_then(|device| {
        unsafe { device.CreateCommandQueue::<ID3D12CommandQueue>(&description) }.ok()
    });
    host.queue = created;
    if host.queue.is_none() {
        return Err(INIT_FAILED);
    }
    host.native_queue = unsafe { retain_native(host, &host.queue) };
    if host.native_queue.is_none() {
        return Err(INIT_FAILED);
    }
    Ok(())
}

unsafe fn lookup<T: Copy>(host: &Host, id: u32, name: &CStr) -> Option<T> {
    unsafe {
        host.feature(id, name)
            .and_then(|entry| sdk_api::function(entry))
    }
}

unsafe fn resolve_features(host: &mut Host) {
    if host.features.contains(DLSS_G) {
        let generation = unsafe {
            Generation {
                get_state: lookup(host, DLSS_G, c"slDLSSGGetState"),
                set_options: lookup(host, DLSS_G, c"slDLSSGSetOptions"),
            }
        };
        *host.generation.get_mut().unwrap() = generation;
    }
    if host.features.contains(REFLEX) {
        unsafe {
            host.reflex_options = lookup(host, REFLEX, c"slReflexSetOptions");
            host.reflex_state = lookup(host, REFLEX, c"slReflexGetState");
            host.sleep = lookup(host, REFLEX, c"slReflexSleep");
        }
    }
    unsafe {
        host.pcl_marker = lookup(host, PCL, c"slPCLSetMarker");
        host.pcl_state = lookup(host, PCL, c"slPCLGetState");
        host.pcl_options = lookup(host, PCL, c"slPCLSetOptions");
    }
    if host.features.contains(DLSS_G) {
        unsafe { prepare_mfg(host) };
    }
    let mut pcl_message = 0;
    if let Some(set) = host.pcl_options {
        // eUsePingMessage with no marker thread.
        unsafe { set(&sdk::pcl_options(0)) };
        pcl_message = host.pcl_message().unwrap_or(0);
    }
    *host.frames.get_mut().unwrap() = Frames::new(pcl_message);
}

unsafe extern "C" fn message(kind: u32, text: *const c_char) {
    let important = kind == sdk::LOG_ERROR || kind == sdk::LOG_WARN;
    unsafe { streamline_log::emit(LOG_SLOT, important, text) };
}

/// slUpgradeInterface on an owned reference. A refusal releases it.
unsafe fn upgrade<T: Interface>(host: &Host, object: &mut Option<T>) -> bool {
    let Some(original) = object.take() else {
        return false;
    };
    let raw = original.into_raw();
    let mut upgraded = raw;
    if unsafe { core(host.core.upgrade_interface)(&mut upgraded) } != sdk::OK {
        drop(unsafe { T::from_raw(raw) });
        return false;
    }
    *object = Some(unsafe { T::from_raw(upgraded) });
    true
}

/// The native interface behind a proxy, retaining the reference the SDK returns.
unsafe fn retain_native<T: Interface>(host: &Host, proxy: &Option<T>) -> Option<T> {
    let mut native = null_mut();
    let native_interface = core(host.core.get_native_interface);
    if unsafe { native_interface(Host::raw(proxy), &mut native) } != sdk::OK || native.is_null() {
        return None;
    }
    Some(unsafe { T::from_raw(native) })
}

pub(super) unsafe fn prime(host: &Host, device: Handle) {
    let verify: mfg_state::VerifySignature = host
        .signature_verified
        .then_some(rsf_streamline_vk::admission::nvidia_signed);
    if let Some(mfg) = unsafe { host.mfg.as_ref() } {
        unsafe {
            mfg_state::rsf_core_mfg_prime(
                mfg,
                device,
                host.directory.as_ptr(),
                verify,
                host.log,
                host.log_user,
            )
        };
    }
}

pub(super) unsafe fn prepare_mfg(host: &Host) {
    if let Some(mfg) = unsafe { host.mfg.as_ref() } {
        let get_state = host
            .generation()
            .get_state
            .map_or(null_mut(), |entry| entry as Handle);
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
