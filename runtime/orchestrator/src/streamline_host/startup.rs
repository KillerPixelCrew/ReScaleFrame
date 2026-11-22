// SPDX-License-Identifier: MIT
use super::*;
use crate::com_ref;
use windows::core::IUnknown;

const NVIDIA_VENDOR: u32 = 0x10de;
const DIRECT_QUEUE: u32 = 0;

// Streamline feature IDs and preference values, asserted against the native headers.
const DLSS: u32 = 0;
const REFLEX: u32 = 3;
const PCL: u32 = 4;
const DLSS_G: u32 = 1000;
const DISABLE_DEBUG_TEXT: u64 = 1 << 1;
const USE_MANUAL_HOOKING: u64 = 1 << 2;
const USE_FRAME_BASED_RESOURCE_TAGGING: u64 = 1 << 7;
const D3D12: u32 = 1;
const LOG_DEFAULT: u32 = 1;
const LOG_VERBOSE: u32 = 2;
const PCL_OPTIONS: u32 = 128;

pub(super) const ENTRY_NAMES: [&CStr; 12] = [
    c"slInit",
    c"slShutdown",
    c"slUpgradeInterface",
    c"slGetNativeInterface",
    c"slSetD3DDevice",
    c"slIsFeatureSupported",
    c"slGetFeatureFunction",
    c"slGetFeatureVersion",
    c"slSetFeatureLoaded",
    c"slGetNewFrameToken",
    c"slSetConstants",
    c"slSetTagForFrame",
];

pub(super) struct FeatureList {
    ids: [u32; 4],
    len: usize,
}

impl FeatureList {
    pub(super) fn new(profile: u32, load_dlss_sr: bool) -> Self {
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

impl Startup {
    pub(super) unsafe fn admit(&self, setup: &Setup) -> Result<(), i32> {
        if setup.engine_type >= self.engine_count {
            return Err(INVALID);
        }
        let mut vendor = 0;
        let mut adapter = null_mut();
        if unsafe { (self.adapter_vendor)(setup.dxgi_adapter, &mut vendor, &mut adapter) } == 0 {
            return Err(INVALID);
        }
        let adapter = unsafe { com_ref::owned(adapter) };
        if vendor != NVIDIA_VENDOR {
            unsafe { (self.report_vendor)(setup.log, setup.log_user, vendor) };
            return Err(NOT_SUPPORTED);
        }
        drop(adapter);
        // An existing interposer owns the SDK and its device registration.
        if unsafe { (self.interposer_loaded)() } != 0 {
            return Err(NEEDS_RESTART);
        }
        Ok(())
    }

    unsafe fn initialize(
        &self,
        native: Handle,
        host: &Host,
        adapter: Handle,
        [device, queue]: [Handle; 2],
        refused_queue_device: &mut Option<IUnknown>,
        mut symbol: impl FnMut(&CStr) -> FARPROC,
    ) -> Result<(), i32> {
        let mut entries = [null_mut(); ENTRY_NAMES.len()];
        for (entry, name) in entries.iter_mut().zip(ENTRY_NAMES) {
            let address = symbol(name).ok_or(MISSING_ENTRY)?;
            *entry = address as *const () as Handle;
        }
        unsafe { (self.bind_required)(native, &entries) };

        let mut flags = USE_MANUAL_HOOKING | USE_FRAME_BASED_RESOURCE_TAGGING;
        let log_level = if host.development_runtime {
            LOG_VERBOSE
        } else {
            flags |= DISABLE_DEBUG_TEXT;
            LOG_DEFAULT
        };
        let preferences = InitPreferences {
            directory: host.directory.as_ptr(),
            engine_version: host.engine_version.as_ptr(),
            project_id: host.project_id.as_ptr(),
            features: host.features.ids.as_ptr(),
            flags,
            feature_count: host.features.len as u32,
            engine_type: host.engine_type,
            render_api: D3D12,
            show_console: host.development_runtime.into(),
            log_level,
        };
        if unsafe { (self.init)(native, &preferences) } == 0 {
            return Err(INIT_FAILED);
        }

        if !device.is_null() && !queue.is_null() {
            unsafe { (self.borrow_device)(native, device) };
        } else if unsafe { (self.create_device)(native, adapter) } == 0 {
            return Err(INIT_FAILED);
        }
        if host.features.contains(DLSS_G) {
            unsafe { (self.prime_mfg)(native, host.signature_verified.into()) };
        }
        if unsafe { (self.upgrade_device)(native) } == 0
            || unsafe { (self.native_device)(native) } == 0
        {
            return Err(INIT_FAILED);
        }
        unsafe { (self.start_audit)(native) };
        if unsafe { (self.register_device)(native) } == 0
            || unsafe { (self.create_factory)(native) } == 0
            || unsafe { (self.upgrade_factory)(native) } == 0
        {
            return Err(INIT_FAILED);
        }
        if !queue.is_null() {
            if unsafe { (self.queue_type)(queue) } != DIRECT_QUEUE {
                return Err(WRONG_API);
            }
            let mut queue_device = null_mut();
            let matched = unsafe { (self.queue_matches_device)(native, queue, &mut queue_device) };
            let queue_device = unsafe { com_ref::owned(queue_device) };
            if matched == 0 {
                // The queue's GetDevice reference outlives native disposal on refusal.
                *refused_queue_device = queue_device;
                return Err(WRONG_API);
            }
        }
        // The proxy queue unwraps Streamline command lists and registers the DLSS-G hooks.
        // The presentation bridge retains the borrowed engine queue and joins the two queues.
        if unsafe { (self.create_queue)(native) } == 0
            || unsafe { (self.native_queue)(native) } == 0
        {
            return Err(INIT_FAILED);
        }
        Ok(())
    }

    unsafe fn resolve(&self, native: Handle, feature: u32, functions: &[(u32, &CStr)]) -> u32 {
        let mut entries = 0;
        for &(entry, name) in functions {
            if unsafe { (self.resolve_feature)(native, feature, name.as_ptr(), entry) } != 0 {
                entries |= entry;
            }
        }
        entries
    }
}

pub(super) struct Opened {
    pub(super) native: Handle,
    pub(super) entries: u32,
    pub(super) pcl_message: u32,
}

impl Platform {
    pub(super) unsafe fn open(
        &self,
        host: &Host,
        adapter: Handle,
        graphics: [Handle; 2],
        module: &SdkModule,
        mut symbol: impl FnMut(&SdkModule, &CStr) -> FARPROC,
    ) -> Result<Opened, i32> {
        let startup = self.startup;
        let native = unsafe { (startup.allocate)(host.log, host.log_user) };
        if native.is_null() {
            return Err(INIT_FAILED);
        }
        let mut refused_queue_device = None;
        if let Err(result) = unsafe {
            startup.initialize(
                native,
                host,
                adapter,
                graphics,
                &mut refused_queue_device,
                |name| symbol(module, name),
            )
        } {
            // Shutdown and release the native state while the caller still holds the module.
            unsafe { (self.dispose)(native) };
            drop(refused_queue_device);
            return Err(result);
        }

        let mut entries = 0;
        if host.features.contains(DLSS_G) {
            entries |= unsafe {
                startup.resolve(
                    native,
                    DLSS_G,
                    &[
                        (GET_STATE, c"slDLSSGGetState"),
                        (SET_OPTIONS, c"slDLSSGSetOptions"),
                    ],
                )
            };
            unsafe { (startup.prepare_mfg)(native) };
        }
        if host.features.contains(REFLEX) {
            entries |= unsafe {
                startup.resolve(
                    native,
                    REFLEX,
                    &[
                        (REFLEX_OPTIONS, c"slReflexSetOptions"),
                        (REFLEX_STATE, c"slReflexGetState"),
                        (SLEEP, c"slReflexSleep"),
                    ],
                )
            };
        }
        let pcl_entries = unsafe {
            startup.resolve(
                native,
                PCL,
                &[
                    (PCL_MARKER, c"slPCLSetMarker"),
                    (PCL_STATE, c"slPCLGetState"),
                    (PCL_OPTIONS, c"slPCLSetOptions"),
                ],
            )
        };
        let mut pcl_message = 0;
        if pcl_entries & PCL_OPTIONS != 0 {
            unsafe { (startup.pcl_defaults)(native) };
            if pcl_entries & PCL_STATE != 0 {
                unsafe { (self.pcl_message)(native, &mut pcl_message) };
            }
        }
        Ok(Opened {
            native,
            entries: entries | (pcl_entries & !PCL_OPTIONS),
            pcl_message,
        })
    }
}
