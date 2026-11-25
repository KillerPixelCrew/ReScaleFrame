// SPDX-License-Identifier: MIT
//! The Unity package's session services: the host services and managed bridge API of each
//! session, the package log, the input window's registration and retirement, and the routing of
//! the bridge's CPU events, the input window's messages and rsfu_input into the Runtime.
//!
//! Everything reported here is a fact from a callback that happened: a CPU event the bridge
//! received, a message the input window dequeued or a managed rsfu_input call, stamped when it
//! arrives and keyed to the CPU frame in progress. What the Runtime makes of it (pacing, markers,
//! latency) is its own decision.
//!
//! The log lock is held across the log leaves and the input-window lock across the window leaves;
//! neither leaf calls back. The message and CPU paths take neither lock, so a message the window
//! dispatches while it is being hooked cannot wait on the hook.
use super::{PACKAGE, Package, raw};
use crate::bridge_state::{
    build_cpu_event, rsf_unity_frames_active, rsf_unity_frames_callback_enter,
    rsf_unity_frames_callback_leave,
};
use crate::generation::RSF_DLSS_ENGINE_UNITY;
use crate::sys::{
    RSF_CALLBACK_HOST_ABI_VERSION_V3, RSF_SR_CHOICES_ALL, rsf_callback_host_setup,
    rsf_callback_host_setup_v3, rsf_unity_native_api,
};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{
    RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_FRAME_END, RSF_GAME_CPU_INPUT_EVENT,
    RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_INPUT_UNFRAMED, RSF_GAME_CPU_WINDOW_MESSAGE,
    RSF_GAME_INPUT_CONTROLLER, RSF_GAME_INPUT_KEYBOARD, RSF_GAME_INPUT_MOUSE, backend_result,
    rsf_game_cpu_event, rsf_game_host_services,
};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};

/// The kinds rsfu_input accepts.
const INPUT_KINDS: u32 = RSF_GAME_INPUT_KEYBOARD | RSF_GAME_INPUT_MOUSE | RSF_GAME_INPUT_CONTROLLER;

const BRIDGE_REFUSED: &str = "Unity graphics bridge or player presentation surface could not open.";
const HOST_REFUSED: &str = "The Runtime D3D12 host refused to attach; see its log.";

struct InputCallback;

impl InputCallback {
    fn enter() -> Option<Self> {
        rsf_unity_frames_callback_enter()
            .then_some(Self)
            .filter(|_| rsf_unity_frames_active())
    }
}

impl Drop for InputCallback {
    fn drop(&mut self) {
        rsf_unity_frames_callback_leave();
    }
}

/// A file or window that only platform leaves interpret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Handle(pub(super) *mut c_void);
// SAFETY: a Win32 handle or procedure address, usable from any thread and never dereferenced
// here.
unsafe impl Send for Handle {}

impl Handle {
    pub(super) const NONE: Self = Self(null_mut());

    fn is_none(self) -> bool {
        self.0.is_null()
    }
}

/// The bridge's managed API, handed to rsfu_initialize's caller.
pub(super) struct Api(pub(super) rsf_unity_native_api);
// SAFETY: the bridge's process-lifetime functions and render event, never dereferenced here.
unsafe impl Send for Api {}

const fn empty_api() -> rsf_unity_native_api {
    // SAFETY: integers, nullable function pointers and a nullable pointer, for which zero is
    // valid.
    unsafe { std::mem::zeroed() }
}

pub(super) struct Services {
    /// Unity.exe hosts the package: generation installs per session and input comes from the
    /// preview's parent window.
    pub(super) editor: AtomicBool,
    /// The open log file, NONE while there is none; held across every write, open and close.
    pub(super) log: Mutex<Handle>,
    /// The hooked input window, NONE until one is found; it stays hooked across sessions.
    pub(super) window: Mutex<Handle>,
    /// The CPU frame between its begin and end events, 0 outside one.
    pub(super) input_frame: AtomicU64,
    /// The bridge's API as its last preparation wrote it.
    pub(super) api: Mutex<Api>,
    /// That API's session id, read by the input paths without the lock.
    pub(super) session: AtomicU64,
    /// The open log's path, which the Runtime host appends to as well; empty while none is open.
    pub(super) log_path: Mutex<Vec<u16>>,
}

impl Services {
    pub(super) const fn new() -> Self {
        Self {
            editor: AtomicBool::new(false),
            log: Mutex::new(Handle::NONE),
            window: Mutex::new(Handle::NONE),
            input_frame: AtomicU64::new(0),
            api: Mutex::new(Api(empty_api())),
            session: AtomicU64::new(0),
            log_path: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn session(&self) -> u64 {
        self.session.load(SeqCst)
    }

    pub(super) fn api(&self) -> rsf_unity_native_api {
        lock_or_recover(&self.api).0
    }

    pub(super) fn retired(&self) {
        *lock_or_recover(&self.api) = Api(empty_api());
        self.session.store(0, SeqCst);
        self.input_frame.store(0, SeqCst);
    }

    /// A received-input fact for this session, stamped `now`.
    fn received(
        &self,
        source: u64,
        now: (u64, u64),
        kind: u32,
        message: u32,
        flags: u32,
    ) -> rsf_game_cpu_event {
        build_cpu_event(
            self.session(),
            RSF_GAME_CPU_INPUT_EVENT,
            source,
            now,
            kind,
            message,
            flags,
        )
    }
}

impl Package {
    /// Plugin load, after the identity: the [UnitySR] Log path when one is set, and the player's
    /// generation install. The Editor installs from each session's open instead.
    pub(super) fn loaded(&self, editor: bool, log: &[u16]) {
        self.services.editor.store(editor, SeqCst);
        if !log.is_empty() {
            let mut file = lock_or_recover(&self.services.log);
            if file.is_none() {
                *file = Handle(raw::log_open(log));
                self.opened_log(*file, log);
            }
        }
        if !editor {
            self.install();
        }
    }

    /// `%LOCALAPPDATA%\ReScaleFrame\Unity\<process id>.log` unless a log is open, its directories
    /// created first. A refusal is retried by the next session.
    fn open_process_log(&self) {
        let mut file = lock_or_recover(&self.services.log);
        if !file.is_none() {
            return;
        }
        let Some(mut path) = raw::local_app_data() else {
            return;
        };
        for directory in ["\\ReScaleFrame", "\\Unity"] {
            path.extend(directory.encode_utf16());
            raw::create_directory(&path);
        }
        path.extend(format!("\\{}.log", std::process::id()).encode_utf16());
        *file = Handle(raw::log_open(&path));
        self.opened_log(*file, &path);
    }

    fn opened_log(&self, file: Handle, path: &[u16]) {
        if !file.is_none() {
            *lock_or_recover(&self.services.log_path) = path.to_vec();
        }
    }

    /// One line to the debugger and the open log, serialized with its open and close.
    pub(super) fn log(&self, text: &str) {
        if let Ok(text) = CString::new(text) {
            self.log_text(&text);
        }
    }

    fn log_text(&self, text: &CStr) {
        let file = lock_or_recover(&self.services.log);
        raw::log_write(file.0, text);
    }

    /// A new session's services, in order: the process log, the Editor's install or the
    /// player's input window, the player's Vulkan surface, then the bridge prepared with this
    /// session's host services.
    pub(super) fn open(&self) -> bool {
        self.open_process_log();
        let editor = self.services.editor.load(SeqCst);
        if editor {
            self.install();
        } else {
            self.hook(raw::player_window);
        }
        if !raw::bridge_open(editor) {
            self.log(BRIDGE_REFUSED);
            return false;
        }
        let Some(host) = self.attach() else {
            self.log(HOST_REFUSED);
            return false;
        };
        // The bridge's passes and CPU events come here first; its plan is the host's own.
        let services = rsf_game_host_services {
            log: Some(backend_log),
            render_pass: Some(super::routing::render_callback),
            cpu_event: Some(host_cpu_event),
            ..host
        };
        // The bridge writes into the stored API.
        let mut api = self.services.api();
        let prepared = unsafe { raw::bridge_prepare(&services, &mut api) } != 0;
        lock_or_recover(&self.services.api).0 = api;
        self.services.session.store(api.session_id, SeqCst);
        if prepared {
            *lock_or_recover(&self.host) = Some(super::Host(host));
        } else {
            let _ = unsafe { raw::host_stop() };
        }
        prepared
    }

    /// Attaches the Runtime host for this session: the package identity's SDK directories and
    /// engine identity, its preferences beside the SDK, the package log, the developer's
    /// defaults as start values, and URP's FSR1 as the engine's spatial scaler.
    fn attach(&self) -> Option<rsf_game_host_services> {
        let identity = self.presentation.identity();
        let mut log = lock_or_recover(&self.services.log_path).clone();
        if log.is_empty() {
            // Without a package log the host's lines are discarded.
            log = "NUL".encode_utf16().collect();
        }
        log.push(0);
        let mut preferences: Vec<u16> = identity.root.to_string_lossy().encode_utf16().collect();
        preferences.extend("\\preferences.ini\0".encode_utf16());
        let mut host = rsf_game_host_services::default();
        let defaults = super::DEFAULT_SETTINGS;
        let setup = rsf_callback_host_setup_v3 {
            base: rsf_callback_host_setup {
                struct_size: size_of::<rsf_callback_host_setup_v3>() as u32,
                abi_version: RSF_CALLBACK_HOST_ABI_VERSION_V3,
                plugin_path: null(),
                log_path: log.as_ptr(),
                preferences_path: preferences.as_ptr(),
                probe: null(),
                backend: defaults.sr_backend,
                quality: defaults.quality,
                dlss_preset: defaults.dlss_preset,
                sr_backend_choices: RSF_SR_CHOICES_ALL,
                engine_type: RSF_DLSS_ENGINE_UNITY,
                engine_version_utf8: identity.version.as_ptr(),
                project_id_utf8: identity.project.as_ptr(),
                streamline_directory_utf8: identity.streamline.as_ptr(),
                fsr2_directory_utf8: identity.fsr.as_ptr(),
                fsr3_directory_utf8: identity.fsr.as_ptr(),
                fsr4_directory_utf8: identity.fsr.as_ptr(),
                xess_directory_utf8: identity.xess.as_ptr(),
                set_engine_spatial: Some(raw::set_engine_spatial),
            },
            services: &mut host,
        };
        (unsafe { raw::host_start(&setup.base) } == 0).then_some(host)
    }
    /// Hooks the window `find` names while none is hooked. A window that is not found yet is
    /// looked for again by the next call.
    fn hook(&self, find: fn() -> *mut c_void) {
        let mut window = lock_or_recover(&self.services.window);
        if !window.is_none() {
            return;
        }
        let found = find();
        if !found.is_null() && raw::subclass(found) {
            *window = Handle(found);
        }
    }

    /// rsfu_preview_region's Editor input: the preview's parent once it exists.
    pub(super) fn preview_input(&self) {
        if self.services.editor.load(SeqCst) {
            self.hook(raw::preview_parent);
        }
    }

    pub(super) fn stop_window_input(&self) {
        let mut window = lock_or_recover(&self.services.window);
        if !window.is_none() {
            raw::restore(window.0);
        }
        *window = Handle::NONE;
    }

    /// A WNDPROC that the platform could not detach keeps its own forwarding receipt.
    pub(super) fn unload(&self) {
        self.stop_window_input();
        let mut file = lock_or_recover(&self.services.log);
        if !file.is_none() {
            raw::log_close(file.0);
            *file = Handle::NONE;
        }
    }

    /// The bridge's CPU events reach the Runtime unchanged; a frame's begin and end bracket the
    /// frame received input is keyed to.
    pub(super) fn cpu_event(&self, event: &rsf_game_cpu_event) {
        if event.stage == RSF_GAME_CPU_FRAME_BEGIN {
            self.services
                .input_frame
                .store(event.source_frame_id, SeqCst);
        }
        unsafe { raw::cpu(event) };
        if event.stage == RSF_GAME_CPU_FRAME_END {
            self.services.input_frame.store(0, SeqCst);
        }
    }

    /// Every message the input window dequeues, classified by its range and WM_INPUT by its raw
    /// device. A keyed one outside a CPU frame is unframed.
    pub(super) fn window_message(&self, message: u32, lparam: isize) {
        let kind = raw::input_kind(message, lparam);
        let now = raw::timestamp();
        let source = self.services.input_frame.load(SeqCst);
        let mut flags = RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE;
        if source == 0 && kind != 0 {
            flags |= RSF_GAME_CPU_INPUT_UNFRAMED;
        }
        unsafe { raw::cpu(&self.services.received(source, now, kind, message, flags)) };
    }

    /// rsfu_input after its argument checks: managed received input, keyed to `frame` while it
    /// is the CPU frame in progress and unframed otherwise.
    pub(super) fn input(&self, frame: u64, kinds: u32) -> i32 {
        let now = raw::timestamp();
        let source = if self.services.input_frame.load(SeqCst) == frame {
            frame
        } else {
            0
        };
        let mut flags = RSF_GAME_CPU_INPUT_KEYED;
        if source == 0 {
            flags |= RSF_GAME_CPU_INPUT_UNFRAMED;
        }
        unsafe { raw::cpu(&self.services.received(source, now, kinds, 0, flags)) };
        backend_result::OK
    }
}

/// rsf_game_host_services::log.
pub(super) unsafe extern "C" fn backend_log(_: *mut c_void, text: *const c_char) {
    unsafe { rsf_unity_package_log(text) }
}

/// rsf_game_host_services::cpu_event.
unsafe extern "C" fn host_cpu_event(_: *mut c_void, event: *const rsf_game_cpu_event) {
    if !event.is_null() {
        PACKAGE.cpu_event(unsafe { &*event });
    }
}

/// The package log in every form package.c lends it.
///
/// # Safety
/// `text` is null or NUL-terminated.
pub(crate) unsafe fn rsf_unity_package_log(text: *const c_char) {
    if !text.is_null() {
        PACKAGE.log_text(unsafe { CStr::from_ptr(text) });
    }
}

#[cfg(windows)]
pub(super) fn report_window_message(message: u32, lparam: isize) {
    if let Some(_callback) = InputCallback::enter() {
        PACKAGE.window_message(message, lparam);
    }
}

/// rsfu_input's checks come first, as they did; nothing is reported for no kinds.
#[unsafe(no_mangle)]
extern "C" fn rsfu_input(frame: u64, kinds: u32) -> c_int {
    if frame == 0 || kinds & !INPUT_KINDS != 0 {
        return backend_result::INVALID_ARGUMENT;
    }
    if kinds == 0 {
        return backend_result::OK;
    }
    match InputCallback::enter() {
        Some(_callback) => PACKAGE.input(frame, kinds),
        None => backend_result::OK,
    }
}

pub(super) fn rsf_unity_package_preview_input() {
    PACKAGE.preview_input();
}

pub(crate) fn editor() -> bool {
    PACKAGE.services.editor.load(SeqCst)
}

#[cfg(windows)]
pub(super) fn unload() {
    PACKAGE.unload();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsfu_input_checks_its_arguments_first() {
        assert_eq!(rsfu_input(0, 1), backend_result::INVALID_ARGUMENT);
        assert_eq!(rsfu_input(1, 8), backend_result::INVALID_ARGUMENT);
        assert_eq!(rsfu_input(1, 0), backend_result::OK);
    }
}
