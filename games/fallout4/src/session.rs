// SPDX-License-Identifier: MIT
//! The GameSDK lifecycle and its process-lived Runtime session.
//!
//! Prepare discovers every native site, checks the cold creation bytes and DPI startup before any hook,
//! creates the Runtime's scoped D3D11 host once per process and then prepares creation, render
//! and CPU hooks. A later prepare must present the original session. The host setup carries this
//! adapter's facts and declared producers; the Runtime owns every vendor and pacing decision.
use crate::creation::{self, rsf_fallout4_accept_window};
use crate::sys::{
    RSF_D3D11_HOST_ABI_VERSION, RSF_D3D11_HOST_CAP_FG_INPUTS, RSF_D3D11_HOST_CAP_INPUT_TIMELINE,
    rsf_d3d11_host_setup, rsf_motion_decode_params,
};
use crate::{
    PENDING, RSF_ERROR_BUSY, RSF_ERROR_INVALID_ARGUMENT, RSF_ERROR_NATIVE_REFUSED,
    RSF_ERROR_NOT_READY, RSF_OK, Threw, cpu_events, discovery, dpi, pin, render_hooks,
};
use rsf_game_sys::call_header::validate;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::same_upstream_identity;
use rsf_game_sys::win32::{module_path, typed_export};
use rsf_game_sys::{
    rsf_game_control_args, rsf_game_host_services, rsf_game_prepare_args, rsf_game_renderer_status,
    rsf_game_start_args, rsf_result,
};
use std::ffi::{CStr, c_void};
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::core::{s, w};

const ENGINE_VERSION: &CStr = c"1.11.240.0";
const PROJECT_ID: &CStr = c"8b91a3c1-3ce2-4c66-a35c-42d2074da9fe";
// 117/8192 exactly: the f32 the native literal 0.01428222656f produced.
const UNITS_TO_METERS: f32 = 0.014_282_227;
// Reduction requires the independently negotiated recovery export. Older runtimes retain the
// standard Native control and native raster.
const QUALITY_WITH_REDUCTION: u32 = 0x1f;
const QUALITY_NATIVE: u32 = 1;
// Adapter-scoped factory interception below external wrappers.
const PRESENTATION_BRIDGE_SCOPED: u32 = 3;
// Native colour, transport Off, unless a saved ColourTransport value exists.
const COLOUR_TRANSPORT_NATIVE: u32 = 1;

pub(crate) type Create =
    unsafe extern "C" fn(*const rsf_d3d11_host_setup, *mut rsf_game_host_services) -> rsf_result;
pub(crate) type Activate = unsafe extern "C" fn(*mut c_void, u32) -> rsf_result;

#[derive(Clone, Copy)]
struct BoundHost {
    // The prepare host that created the process-lived Runtime host, and the services it returned.
    upstream: rsf_game_host_services,
    services: rsf_game_host_services,
    activate: Activate,
}

struct Session {
    prepared: bool,
    active: bool,
    host: Option<BoundHost>,
    reason: &'static CStr,
}
// SAFETY: the services are process-lived Runtime and host state, used under the session lock.
unsafe impl Send for Session {}

static SESSION: Mutex<Session> = Mutex::new(Session {
    prepared: false,
    active: false,
    host: None,
    reason: PENDING,
});

fn session() -> MutexGuard<'static, Session> {
    lock_or_recover(&SESSION)
}

use rsf_game_sys::ffi_util::wide_path as wide;
impl Session {
    fn busy(&self) -> bool {
        creation::busy() || render_hooks::busy() || cpu_events::busy()
    }

    fn refuse(
        &mut self,
        host: &rsf_game_host_services,
        message: &'static CStr,
    ) -> Result<rsf_result, Threw> {
        self.reason = message;
        crate::log_host(host, message)?;
        Ok(RSF_ERROR_NATIVE_REFUSED)
    }

    fn suspend(&mut self) -> Result<(), Threw> {
        creation::quiesce();
        render_hooks::quiesce();
        cpu_events::quiesce();
        if let Some(host) = self.host
            && !host.services.user.is_null()
        {
            crate::host_set_active(host.activate, host.services.user, 0)?;
        }
        self.active = false;
        Ok(())
    }

    fn release_hooks(&self) -> bool {
        let rendering = render_hooks::stop();
        let created = creation::stop();
        // Keep the CPU owner until its native Swap has closed every render lease.
        if !rendering || !created || self.busy() {
            return false;
        }
        cpu_events::stop()
    }

    // The Runtime's scoped D3D11 host for this process, with this adapter's facts.
    fn create_host(
        runtime: *mut c_void,
        create: Create,
        activate: Activate,
        host: &rsf_game_host_services,
    ) -> Result<BoundHost, rsf_result> {
        let path = unsafe { module_path(runtime) }.ok_or(RSF_ERROR_NOT_READY)?;
        let path = path.units();
        let separator = path
            .iter()
            .rposition(|&unit| unit == u16::from(b'\\'))
            .ok_or(RSF_ERROR_NOT_READY)?;
        let directory = wide(&path[..separator], "");
        let ini = wide(&path[..separator], "\\ReScaleFrame.ini");
        let reduction = unsafe { GetProcAddress(runtime, s!("rsf_d3d11_host_reduced_frame")) };
        // SAFETY: the setup is plain data; all-zero is its value-initialized state.
        let mut setup: rsf_d3d11_host_setup = unsafe { std::mem::zeroed() };
        setup.struct_size = size_of::<rsf_d3d11_host_setup>() as u32;
        setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
        setup.configuration_path = ini.as_ptr();
        setup.configuration_section = w!("Rendering");
        setup.runtime_directory = directory.as_ptr();
        setup.identity_utf8 = c"fallout4".as_ptr();
        setup.executable_sha256 = discovery::executable_sha256().as_ptr();
        setup.engine_version_utf8 = ENGINE_VERSION.as_ptr();
        setup.project_id_utf8 = PROJECT_ID.as_ptr();
        setup.units_to_meters = UNITS_TO_METERS;
        setup.motion.struct_size = size_of::<rsf_motion_decode_params>() as u32;
        setup.motion.scale_x = 1.0;
        setup.motion.scale_y = 1.0;
        setup.motion.output_scale_x = 1.0;
        setup.motion.output_scale_y = 1.0;
        setup.colour_transport_default = COLOUR_TRANSPORT_NATIVE;
        setup.quality_choices = if reduction.is_some() {
            QUALITY_WITH_REDUCTION
        } else {
            QUALITY_NATIVE
        };
        setup.presentation_bridge = PRESENTATION_BRIDGE_SCOPED;
        setup.accept_window = Some(rsf_fallout4_accept_window);
        // Implemented producer declarations for cold admission tests, not acceptance.
        setup.capabilities = RSF_D3D11_HOST_CAP_FG_INPUTS | RSF_D3D11_HOST_CAP_INPUT_TIMELINE;
        let mut bound = *host;
        let result =
            crate::host_create(create, &setup, &mut bound).map_err(|_| RSF_ERROR_NOT_READY)?;
        if result != RSF_OK {
            return Err(result);
        }
        Ok(BoundHost {
            upstream: *host,
            services: bound,
            activate,
        })
    }

    fn prepare(&mut self, host: &rsf_game_host_services) -> Result<rsf_result, Threw> {
        if self.prepared || self.busy() {
            return Ok(RSF_ERROR_BUSY);
        }
        if let Err(refusal) = discovery::admit() {
            return self.refuse(host, refusal);
        }
        // Evidence only: the hash names the build in the log, it admits nothing.
        let evidence = crate::message(format!(
            "Fallout4 native sites discovered in executable SHA-256 {}.",
            discovery::executable_sha256().to_string_lossy()
        ));
        crate::log_host(host, &evidence)?;
        if !creation::preflight() {
            return self.refuse(
                host,
                c"Fallout4 cold creation refused: expected native bytes or cold owner unavailable.",
            );
        }
        if let Some(refusal) = dpi::prepare(host)? {
            self.reason = refusal;
            crate::log_host(host, refusal)?;
            return Ok(RSF_ERROR_NATIVE_REFUSED);
        }
        if !render_hooks::verify_with_inputs() {
            return self.refuse(
                host,
                c"Fallout4 render preparation refused: native call, TAA, resource, camera or depth-domain guard mismatch.",
            );
        }
        if !crate::initialize_hooks() {
            return self.refuse(
                host,
                c"Fallout4 cold creation refused: native hook initialization failed.",
            );
        }
        let adapter = prepare as unsafe extern "C" fn(_) -> _;
        if !pin(adapter as *const c_void) {
            return self.refuse(host, c"Fallout4 native hook lifetime pin refused.");
        }
        let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
        // SAFETY: the process-lived Runtime exports these signatures in d3d11_host.h.
        let create = unsafe { typed_export::<Create>(runtime, c"rsf_d3d11_host_create") };
        let activate = unsafe { typed_export::<Activate>(runtime, c"rsf_d3d11_host_set_active") };
        let (Some(create), Some(activate)) = (create, activate) else {
            return Ok(RSF_ERROR_NOT_READY);
        };
        if !pin(runtime.cast_const()) {
            return Ok(RSF_ERROR_NOT_READY);
        }
        let bound = if let Some(bound) = self.host
            && !bound.services.user.is_null()
        {
            if !same_upstream_identity(&bound.upstream, host) {
                return self.refuse(
                    host,
                    c"Fallout4 process-lived creation host requires the original session.",
                );
            }
            bound
        } else {
            match Self::create_host(runtime, create, activate, host) {
                Ok(bound) => bound,
                Err(result) => return Ok(result),
            }
        };
        self.host = Some(bound);
        let services = bound.services;
        if !creation::prepare(&services) {
            return self.refuse(
                host,
                c"Fallout4 cold creation refused: native bytes, owner, Steam forwarding or scoped services unavailable.",
            );
        }
        if !render_hooks::prepare(&services) {
            return self.refuse(
                host,
                c"Fallout4 render preparation refused: native guards, synchronous services or hook creation unavailable.",
            );
        }
        if !cpu_events::prepare(&services) {
            return self.refuse(
                host,
                c"Fallout4 CPU preparation refused: guarded Swap/input callbacks or source services unavailable.",
            );
        }
        self.prepared = true;
        self.reason = PENDING;
        Ok(RSF_OK)
    }

    fn start(&mut self) -> Result<rsf_result, Threw> {
        if !self.prepared {
            return Ok(RSF_ERROR_NOT_READY);
        }
        if self.active {
            return Ok(RSF_OK);
        }
        let Some(host) = self.host else {
            return Ok(RSF_ERROR_NOT_READY);
        };
        let failed = if !cpu_events::start() {
            Some(c"Fallout4 start refused: CPU source/input guards or hooks.")
        } else if !render_hooks::start() {
            Some(c"Fallout4 start refused: render/TAA guards or hooks.")
        } else if !creation::start() {
            Some(c"Fallout4 start refused: cold owner, creation call/thunk or hook.")
        } else {
            None
        };
        if let Some(failed) = failed {
            self.suspend()?;
            let _ = self.release_hooks();
            self.prepared = false;
            return self.refuse(&host.upstream, failed);
        }
        let result = crate::host_set_active(host.activate, host.services.user, 1)?;
        if result != RSF_OK {
            self.suspend()?;
            let _ = self.release_hooks();
            self.prepared = false;
            return Ok(result);
        }
        self.active = true;
        Ok(RSF_OK)
    }

    fn quiesce(&mut self) -> Result<rsf_result, Threw> {
        self.suspend()?;
        Ok(if self.busy() { RSF_ERROR_BUSY } else { RSF_OK })
    }

    fn stop(&mut self) -> Result<rsf_result, Threw> {
        self.suspend()?;
        if !self.release_hooks() {
            return Ok(RSF_ERROR_BUSY);
        }
        // Steam's immutable relays and MinHook trampolines remain process-lived.
        self.prepared = false;
        Ok(RSF_OK)
    }
}

fn run(entry: impl FnOnce(&mut Session) -> Result<rsf_result, Threw>) -> rsf_result {
    entry(&mut session()).unwrap_or(RSF_ERROR_NOT_READY)
}

pub(crate) unsafe extern "C" fn prepare(args: *const rsf_game_prepare_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    let host = unsafe { (*args).host };
    let valid = unsafe { validate(host) };
    if valid != RSF_OK {
        return valid;
    }
    let host = unsafe { *host };
    if host.session_id == 0 {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    run(|session| session.prepare(&host))
}

pub(crate) unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    run(Session::start)
}

pub(crate) unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    run(Session::quiesce)
}

pub(crate) unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != RSF_OK {
        return valid;
    }
    run(Session::stop)
}

pub(crate) unsafe extern "C" fn status(output: *mut rsf_game_renderer_status) -> rsf_result {
    let valid = unsafe { validate(output.cast_const()) };
    if valid != RSF_OK {
        return valid;
    }
    let session = session();
    let output = unsafe { &mut *output };
    output.prepared = u32::from(session.prepared);
    output.active = u32::from(session.active);
    output.rendering_ready = 0;
    output.reason = session.reason.as_ptr();
    RSF_OK
}
