// SPDX-License-Identifier: MIT
//! The GameSDK lifecycle and its process-lived Runtime session.
//!
//! Prepare checks the exact build, the cold creation bytes and DPI startup before any hook,
//! creates the Runtime's scoped D3D11 host once per process and then prepares creation, render
//! and CPU hooks. A later prepare must present the original session. The host setup carries this
//! adapter's facts and declared producers; the Runtime owns every vendor and pacing decision.
use crate::creation::{self, rsf_fallout4_accept_window};
use crate::platform_abi::{
    RSF_D3D11_HOST_ABI_VERSION, RSF_D3D11_HOST_CAP_FG_INPUTS, RSF_D3D11_HOST_CAP_INPUT_TIMELINE,
    RSF_FALLOUT4_CPU_BUSY, RSF_FALLOUT4_PREPARE_CPU, RSF_FALLOUT4_PREPARE_RENDER,
    RSF_FALLOUT4_QUIESCE_CPU, RSF_FALLOUT4_QUIESCE_RENDER, RSF_FALLOUT4_RENDER_BUSY,
    RSF_FALLOUT4_START_CPU, RSF_FALLOUT4_START_RENDER, RSF_FALLOUT4_STOP_CPU,
    RSF_FALLOUT4_STOP_RENDER, RSF_FALLOUT4_VERIFY_RENDER, rsf_d3d11_host_setup,
    rsf_fallout4_host_active_fn, rsf_fallout4_host_create_fn, rsf_motion_decode_params,
};
use crate::{
    EXECUTABLE_SHA256, Ops, PENDING, RSF_ERROR_ABI_MISMATCH, RSF_ERROR_BUSY,
    RSF_ERROR_INVALID_ARGUMENT, RSF_ERROR_NATIVE_REFUSED, RSF_ERROR_NOT_READY, RSF_OK, Threw, dpi,
    ops, pin,
};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_game_control_args, rsf_game_host_services, rsf_game_prepare_args,
    rsf_game_renderer_status, rsf_game_start_args, rsf_result,
};
use std::ffi::{CStr, c_void};
use std::sync::{Mutex, MutexGuard, PoisonError};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleW, GetProcAddress,
};
use windows_sys::core::{s, w};

const ENGINE_VERSION: &CStr = c"1.11.240.0";
const PROJECT_ID: &CStr = c"8b91a3c1-3ce2-4c66-a35c-42d2074da9fe";
// 117/8192 exactly: the f32 the native literal 0.01428222656f produced.
const UNITS_TO_METERS: f32 = 0.014_282_226_562_5;
// Reduction requires the independently negotiated recovery export. Older runtimes retain the
// standard Native control and native raster.
const QUALITY_WITH_REDUCTION: u32 = 0x1f;
const QUALITY_NATIVE: u32 = 1;
// Adapter-scoped factory interception below external wrappers.
const PRESENTATION_BRIDGE_SCOPED: u32 = 3;
// Native colour, transport Off, unless a saved ColourTransport value exists.
const COLOUR_TRANSPORT_NATIVE: u32 = 1;

type Export = unsafe extern "system" fn() -> isize;
type Create =
    unsafe extern "C" fn(*const rsf_d3d11_host_setup, *mut rsf_game_host_services) -> rsf_result;
type Activate = unsafe extern "C" fn(*mut c_void, u32) -> rsf_result;

struct Session {
    prepared: bool,
    active: bool,
    // The prepare host that created the process-lived Runtime host, and the services it returned.
    upstream: rsf_game_host_services,
    services: rsf_game_host_services,
    activate: Option<Activate>,
    reason: &'static CStr,
}
// SAFETY: the services are process-lived Runtime and host state, used under the session lock.
unsafe impl Send for Session {}

static SESSION: Mutex<Session> = Mutex::new(Session {
    prepared: false,
    active: false,
    // SAFETY: the services table is plain data; all-zero is its empty state.
    upstream: unsafe { std::mem::zeroed() },
    services: unsafe { std::mem::zeroed() },
    activate: None,
    reason: PENDING,
});

fn session() -> MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The GameSDK size and ABI check on a caller structure that leads with struct_size and
/// abi_version.
///
/// # Safety
/// A nonnull `value` points to at least its two leading fields.
unsafe fn validate<T>(value: *const T) -> rsf_result {
    if value.is_null() {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    let header = value.cast::<u32>();
    if unsafe { header.read() } < size_of::<T>() as u32 {
        RSF_ERROR_INVALID_ARGUMENT
    } else if unsafe { header.add(1).read() } != RSF_GAME_ABI_VERSION {
        RSF_ERROR_ABI_MISMATCH
    } else {
        RSF_OK
    }
}

fn runtime_export<F: Copy>(runtime: *mut c_void, name: windows_sys::core::PCSTR) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<Export>()) };
    // SAFETY: each caller names the export's C signature.
    unsafe { GetProcAddress(runtime, name) }
        .map(|export| unsafe { std::mem::transmute_copy::<Export, F>(&export) })
}

fn same_session(upstream: &rsf_game_host_services, host: &rsf_game_host_services) -> bool {
    upstream.session_id == host.session_id
        && upstream.user == host.user
        && upstream.log.map(|log| log as *const ()) == host.log.map(|log| log as *const ())
        && upstream.cpu_event.map(|event| event as *const ())
            == host.cpu_event.map(|event| event as *const ())
}

fn wide(units: &[u16], suffix: &str) -> Vec<u16> {
    let mut path = units.to_vec();
    path.extend(suffix.encode_utf16());
    path.push(0);
    path
}

impl Session {
    fn busy(&self, ops: &Ops) -> Result<bool, Threw> {
        Ok(creation::busy()
            || ops.subsystem(RSF_FALLOUT4_RENDER_BUSY, None)?
            || ops.subsystem(RSF_FALLOUT4_CPU_BUSY, None)?)
    }

    fn refuse(&mut self, ops: &Ops, message: &'static CStr) -> Result<rsf_result, Threw> {
        self.reason = message;
        ops.log(self.services.log, self.services.user, message)?;
        Ok(RSF_ERROR_NATIVE_REFUSED)
    }

    fn suspend(&mut self, ops: &Ops) -> Result<(), Threw> {
        creation::quiesce();
        ops.subsystem(RSF_FALLOUT4_QUIESCE_RENDER, None)?;
        ops.subsystem(RSF_FALLOUT4_QUIESCE_CPU, None)?;
        if let Some(activate) = self.activate
            && !self.services.user.is_null()
        {
            ops.host_set_active(Some(activate), self.services.user, 0)?;
        }
        self.active = false;
        Ok(())
    }

    fn release_hooks(&mut self, ops: &Ops) -> Result<bool, Threw> {
        let rendering = ops.subsystem(RSF_FALLOUT4_STOP_RENDER, None)?;
        let created = creation::stop(ops);
        // Keep the CPU owner until its native Swap has closed every render lease.
        if !rendering || !created || self.busy(ops)? {
            return Ok(false);
        }
        ops.subsystem(RSF_FALLOUT4_STOP_CPU, None)
    }

    // The Runtime's scoped D3D11 host for this process, with this adapter's facts.
    fn create_host(
        &mut self,
        ops: &Ops,
        runtime: *mut c_void,
        create: Create,
        host: &rsf_game_host_services,
    ) -> Result<rsf_result, Threw> {
        let mut path = [0u16; 32768];
        let length = unsafe { GetModuleFileNameW(runtime, path.as_mut_ptr(), 32768) } as usize;
        if length == 0 || length >= path.len() {
            return Ok(RSF_ERROR_NOT_READY);
        }
        let Some(separator) = path[..length]
            .iter()
            .rposition(|&unit| unit == u16::from(b'\\'))
        else {
            return Ok(RSF_ERROR_NOT_READY);
        };
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
        setup.executable_sha256 = EXECUTABLE_SHA256.as_ptr();
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
        let create: rsf_fallout4_host_create_fn = Some(create);
        let result = ops.host_create(create, &setup, &mut bound)?;
        if result != RSF_OK {
            return Ok(result);
        }
        self.services = bound;
        self.upstream = *host;
        Ok(RSF_OK)
    }

    fn prepare(&mut self, ops: &Ops, host: &rsf_game_host_services) -> Result<rsf_result, Threw> {
        if self.prepared || self.busy(ops)? {
            return Ok(RSF_ERROR_BUSY);
        }
        if !creation::verify_executable(ops)? {
            return self.refuse(
                ops,
                c"Fallout4 cold creation refused: executable identity mismatch.",
            );
        }
        if !creation::preflight() {
            return self.refuse(
                ops,
                c"Fallout4 cold creation refused: expected native bytes or cold owner unavailable.",
            );
        }
        if let Some(refusal) = dpi::prepare(ops, host)? {
            self.reason = refusal;
            ops.log_host(host, refusal)?;
            return Ok(RSF_ERROR_NATIVE_REFUSED);
        }
        if !ops.subsystem(RSF_FALLOUT4_VERIFY_RENDER, None)? {
            return self.refuse(
                ops,
                c"Fallout4 render preparation refused: native call, TAA, resource, camera or depth-domain guard mismatch.",
            );
        }
        if !ops.initialize_hooks() {
            return self.refuse(
                ops,
                c"Fallout4 cold creation refused: native hook initialization failed.",
            );
        }
        let adapter = prepare as unsafe extern "C" fn(_) -> _;
        if !pin(adapter as *const c_void) {
            return self.refuse(ops, c"Fallout4 native hook lifetime pin refused.");
        }
        let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
        let create = (!runtime.is_null())
            .then(|| runtime_export::<Create>(runtime, s!("rsf_d3d11_host_create")))
            .flatten();
        self.activate = (!runtime.is_null())
            .then(|| runtime_export::<Activate>(runtime, s!("rsf_d3d11_host_set_active")))
            .flatten();
        let (Some(create), Some(_)) = (create, self.activate) else {
            return Ok(RSF_ERROR_NOT_READY);
        };
        if !pin(runtime.cast_const()) {
            return Ok(RSF_ERROR_NOT_READY);
        }
        if !self.services.user.is_null() {
            if !same_session(&self.upstream, host) {
                return self.refuse(
                    ops,
                    c"Fallout4 process-lived creation host requires the original session.",
                );
            }
        } else {
            let result = self.create_host(ops, runtime, create, host)?;
            if result != RSF_OK {
                return Ok(result);
            }
        }
        let services = self.services;
        if !creation::prepare(ops, &services) {
            return self.refuse(
                ops,
                c"Fallout4 cold creation refused: native bytes, owner, Steam forwarding or scoped services unavailable.",
            );
        }
        if !ops.subsystem(RSF_FALLOUT4_PREPARE_RENDER, Some(&services))? {
            return self.refuse(
                ops,
                c"Fallout4 render preparation refused: native guards, synchronous services or hook creation unavailable.",
            );
        }
        if !ops.subsystem(RSF_FALLOUT4_PREPARE_CPU, Some(&services))? {
            return self.refuse(
                ops,
                c"Fallout4 CPU preparation refused: guarded Swap/input callbacks or source services unavailable.",
            );
        }
        self.prepared = true;
        self.reason = PENDING;
        Ok(RSF_OK)
    }

    fn start(&mut self, ops: &Ops) -> Result<rsf_result, Threw> {
        if !self.prepared {
            return Ok(RSF_ERROR_NOT_READY);
        }
        if self.active {
            return Ok(RSF_OK);
        }
        let failed = if !ops.subsystem(RSF_FALLOUT4_START_CPU, None)? {
            Some(c"Fallout4 start refused: CPU source/input guards or hooks.")
        } else if !ops.subsystem(RSF_FALLOUT4_START_RENDER, None)? {
            Some(c"Fallout4 start refused: render/TAA guards or hooks.")
        } else if !creation::start(ops) {
            Some(c"Fallout4 start refused: cold owner, creation call/thunk or hook.")
        } else {
            None
        };
        if let Some(failed) = failed {
            self.suspend(ops)?;
            let _ = self.release_hooks(ops)?;
            self.prepared = false;
            return self.refuse(ops, failed);
        }
        let activate: rsf_fallout4_host_active_fn = self.activate;
        let result = ops.host_set_active(activate, self.services.user, 1)?;
        if result != RSF_OK {
            self.suspend(ops)?;
            let _ = self.release_hooks(ops)?;
            self.prepared = false;
            return Ok(result);
        }
        self.active = true;
        Ok(RSF_OK)
    }

    fn quiesce(&mut self, ops: &Ops) -> Result<rsf_result, Threw> {
        self.suspend(ops)?;
        Ok(if self.busy(ops)? {
            RSF_ERROR_BUSY
        } else {
            RSF_OK
        })
    }

    fn stop(&mut self, ops: &Ops) -> Result<rsf_result, Threw> {
        self.suspend(ops)?;
        if !self.release_hooks(ops)? {
            return Ok(RSF_ERROR_BUSY);
        }
        // Steam's immutable relays and MinHook trampolines remain process-lived.
        self.prepared = false;
        Ok(RSF_OK)
    }
}

fn run(entry: impl FnOnce(&mut Session, &Ops) -> Result<rsf_result, Threw>) -> rsf_result {
    let Some(ops) = ops() else {
        return RSF_ERROR_NOT_READY;
    };
    let mut session = session();
    entry(&mut *session, ops).unwrap_or(RSF_ERROR_NOT_READY)
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
    run(|session, ops| session.prepare(ops, &host))
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
