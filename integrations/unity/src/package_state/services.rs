// SPDX-License-Identifier: MIT
//! The Unity package's session services behind package.c: the host services and managed bridge
//! API of each session, the package log, the input window's registration and retirement, and the
//! routing of the bridge's CPU events, the input window's messages and rsfu_input into the
//! Runtime. package.c keeps the Win32, Unity and Runtime calls as leaves: the file, window,
//! raw-input and clock calls, the Vulkan surface, the render callback and plan, the bridge's
//! preparation and rsf_fg12_cpu.
//!
//! Everything reported here is a fact from a callback that happened: a CPU event the bridge
//! received, a message the input window dequeued or a managed rsfu_input call, stamped when it
//! arrives and keyed to the CPU frame in progress. What the Runtime makes of it (pacing, markers,
//! latency) is its own decision.
//!
//! The log lock is held across the log leaves and the input-window lock across the window leaves;
//! neither leaf calls back. The message and CPU paths take neither lock, so a message the window
//! dispatches while it is being hooked cannot wait on the hook.
use super::{Leaves, PACKAGE, Package, loaded, raw};
use crate::bridge_state::{
    rsf_unity_frames_active, rsf_unity_frames_callback_enter, rsf_unity_frames_callback_leave,
};
use crate::runtime_config::{PATH_CAPACITY, terminated};
use crate::sys::rsf_unity_native_api;
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_FRAME_BEGIN,
    RSF_GAME_CPU_FRAME_END, RSF_GAME_CPU_INPUT_EVENT, RSF_GAME_CPU_INPUT_KEYED,
    RSF_GAME_CPU_INPUT_UNFRAMED, RSF_GAME_CPU_WINDOW_MESSAGE, RSF_GAME_INPUT_CONTROLLER,
    RSF_GAME_INPUT_KEYBOARD, RSF_GAME_INPUT_MOUSE, backend_result, rsf_game_cpu_event,
    rsf_game_host_services,
};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The kinds rsfu_input accepts.
const INPUT_KINDS: u32 = RSF_GAME_INPUT_KEYBOARD | RSF_GAME_INPUT_MOUSE | RSF_GAME_INPUT_CONTROLLER;

const BRIDGE_REFUSED: &str = "Unity graphics bridge or player presentation surface could not open.";

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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
        }
    }

    pub(super) fn session(&self) -> u64 {
        self.session.load(SeqCst)
    }

    pub(super) fn api(&self) -> rsf_unity_native_api {
        lock(&self.api).0
    }

    pub(super) fn retired(&self) {
        *lock(&self.api) = Api(empty_api());
        self.session.store(0, SeqCst);
        self.input_frame.store(0, SeqCst);
    }

    /// A received-input fact for this session, stamped `now`.
    fn received(
        &self,
        source: u64,
        (now, frequency): (u64, u64),
        kind: u32,
        message: u32,
        flags: u32,
    ) -> rsf_game_cpu_event {
        rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_event>() as u32,
            stage: RSF_GAME_CPU_INPUT_EVENT,
            session_id: self.session(),
            source_frame_id: source,
            timestamp_qpc: now,
            qpc_frequency: frequency,
            render_expected: 0,
            input_kind: kind,
            message_id: message,
            event_version: RSF_GAME_CPU_EVENT_VERSION,
            flags,
        }
    }
}

// SAFETY for every call below: package.c's leaves, borrowing their pointer arguments
// for the call only. Paths are terminated here.
impl Leaves {
    fn bridge_open(&self, editor: bool) -> bool {
        raw::bridge_open(u32::from(editor)) != 0
    }
    fn session_seed(&self) -> u64 {
        raw::session_seed()
    }
    fn bridge_prepare(
        &self,
        services: &rsf_game_host_services,
        api: &mut rsf_unity_native_api,
    ) -> bool {
        unsafe { raw::bridge_prepare(services, api) != 0 }
    }
    /// %LOCALAPPDATA%; None when it is unset, empty or longer than a path buffer.
    fn local_app_data(&self) -> Option<Vec<u16>> {
        let mut value = vec![0u16; PATH_CAPACITY];
        let length = unsafe { raw::local_app_data(value.as_mut_ptr(), PATH_CAPACITY as u32) };
        let length = length as usize;
        (length != 0 && length < PATH_CAPACITY).then(|| {
            value.truncate(length);
            value
        })
    }
    fn create_directory(&self, path: &[u16]) {
        let path = terminated(path.to_vec());
        unsafe { raw::create_directory(path.as_ptr()) }
    }
    fn log_open(&self, path: &[u16]) -> Handle {
        let path = terminated(path.to_vec());
        Handle(unsafe { raw::log_open(path.as_ptr()) })
    }
    fn log_write(&self, file: Handle, text: &CStr) {
        unsafe { raw::log_write(file.0, text.as_ptr()) }
    }
    fn log_close(&self, file: Handle) {
        unsafe { raw::log_close(file.0) }
    }
    fn player_window(&self) -> Handle {
        Handle(raw::player_window())
    }
    fn preview_parent(&self) -> Handle {
        Handle(raw::preview_parent())
    }
    fn subclass(&self, window: Handle) -> bool {
        raw::subclass(window.0) != 0
    }
    fn restore(&self, window: Handle) {
        raw::restore(window.0)
    }
    fn timestamp(&self) -> (u64, u64) {
        let (mut now, mut frequency) = (0u64, 0u64);
        unsafe { raw::timestamp(&mut now, &mut frequency) };
        (now, frequency)
    }
    fn cpu(&self, event: &rsf_game_cpu_event) {
        unsafe { raw::cpu(event) }
    }
}

impl Package {
    /// Plugin load, after the identity: the [UnitySR] Log path when one is set, and the player's
    /// generation install. The Editor installs from each session's open instead.
    pub(super) fn loaded(&self, leaves: &Leaves, editor: bool, log: &[u16]) {
        self.services.editor.store(editor, SeqCst);
        if !log.is_empty() {
            let mut file = lock(&self.services.log);
            if file.is_none() {
                *file = leaves.log_open(log);
            }
        }
        if !editor {
            self.install(leaves);
        }
    }

    /// `%LOCALAPPDATA%\ReScaleFrame\Unity\<process id>.log` unless a log is open, its directories
    /// created first. A refusal is retried by the next session.
    fn open_process_log(&self, leaves: &Leaves) {
        let mut file = lock(&self.services.log);
        if !file.is_none() {
            return;
        }
        let Some(mut path) = leaves.local_app_data() else {
            return;
        };
        for directory in ["\\ReScaleFrame", "\\Unity"] {
            path.extend(directory.encode_utf16());
            leaves.create_directory(&path);
        }
        path.extend(format!("\\{}.log", std::process::id()).encode_utf16());
        *file = leaves.log_open(&path);
    }

    /// One line to the debugger and the open log, serialized with its open and close.
    pub(super) fn log(&self, leaves: &Leaves, text: &str) {
        if let Ok(text) = CString::new(text) {
            self.log_text(leaves, &text);
        }
    }

    fn log_text(&self, leaves: &Leaves, text: &CStr) {
        let file = lock(&self.services.log);
        leaves.log_write(*file, text);
    }

    /// A new session's services, in order: the process log, the Editor's install or the
    /// player's input window, the player's Vulkan surface, then the bridge prepared with this
    /// session's host services.
    pub(super) fn open(&self, leaves: &Leaves) -> bool {
        self.open_process_log(leaves);
        let editor = self.services.editor.load(SeqCst);
        if editor {
            self.install(leaves);
        } else {
            self.hook(leaves, Leaves::player_window);
        }
        if !leaves.bridge_open(editor) {
            self.log(leaves, BRIDGE_REFUSED);
            return false;
        }
        let services = rsf_game_host_services {
            struct_size: size_of::<rsf_game_host_services>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            session_id: leaves.session_seed().wrapping_add(1),
            log: Some(backend_log),
            render_pass: Some(super::routing::render_callback),
            user: null_mut(),
            render_config: Some(super::render_config),
            cpu_event: Some(host_cpu_event),
            scope_state: None,
            process_upscale: None,
            submit: None,
            present: None,
            process_latency: None,
            render_feedback: None,
        };
        // The bridge writes into the stored API.
        let mut api = self.services.api();
        let prepared = leaves.bridge_prepare(&services, &mut api);
        lock(&self.services.api).0 = api;
        self.services.session.store(api.session_id, SeqCst);
        prepared
    }

    /// Hooks the window `find` names while none is hooked. A window that is not found yet is
    /// looked for again by the next call.
    fn hook(&self, leaves: &Leaves, find: fn(&Leaves) -> Handle) {
        let mut window = lock(&self.services.window);
        if !window.is_none() {
            return;
        }
        let found = find(leaves);
        if found.is_none() {
            return;
        }
        if leaves.subclass(found) {
            *window = found;
        }
    }

    /// rsfu_preview_region's Editor input: the preview's parent once it exists.
    pub(super) fn preview_input(&self, leaves: &Leaves) {
        if self.services.editor.load(SeqCst) {
            self.hook(leaves, Leaves::preview_parent);
        }
    }

    pub(super) fn stop_window_input(&self, leaves: &Leaves) {
        let mut window = lock(&self.services.window);
        if !window.is_none() {
            leaves.restore(*window);
        }
        *window = Handle::NONE;
    }

    /// A WNDPROC that the platform could not detach keeps its own forwarding receipt.
    pub(super) fn unload(&self, leaves: &Leaves) {
        self.stop_window_input(leaves);
        let mut file = lock(&self.services.log);
        if !file.is_none() {
            leaves.log_close(*file);
            *file = Handle::NONE;
        }
    }

    /// The bridge's CPU events reach the Runtime unchanged; a frame's begin and end bracket the
    /// frame received input is keyed to.
    pub(super) fn cpu_event(&self, leaves: &Leaves, event: &rsf_game_cpu_event) {
        if event.stage == RSF_GAME_CPU_FRAME_BEGIN {
            self.services
                .input_frame
                .store(event.source_frame_id, SeqCst);
        }
        leaves.cpu(event);
        if event.stage == RSF_GAME_CPU_FRAME_END {
            self.services.input_frame.store(0, SeqCst);
        }
    }

    /// Every message the input window dequeues, classified by its range and WM_INPUT by its raw
    /// device. A keyed one outside a CPU frame is unframed.
    pub(super) fn window_message(&self, leaves: &Leaves, message: u32, lparam: isize) {
        let kind = raw::input_kind(message, lparam);
        let now = leaves.timestamp();
        let source = self.services.input_frame.load(SeqCst);
        let mut flags = RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE;
        if source == 0 && kind != 0 {
            flags |= RSF_GAME_CPU_INPUT_UNFRAMED;
        }
        leaves.cpu(&self.services.received(source, now, kind, message, flags));
    }

    /// rsfu_input after its argument checks: managed received input, keyed to `frame` while it
    /// is the CPU frame in progress and unframed otherwise.
    pub(super) fn input(&self, leaves: &Leaves, frame: u64, kinds: u32) -> i32 {
        let now = leaves.timestamp();
        let source = if self.services.input_frame.load(SeqCst) == frame {
            frame
        } else {
            0
        };
        let mut flags = RSF_GAME_CPU_INPUT_KEYED;
        if source == 0 {
            flags |= RSF_GAME_CPU_INPUT_UNFRAMED;
        }
        leaves.cpu(&self.services.received(source, now, kinds, 0, flags));
        backend_result::OK
    }
}

/// rsf_game_host_services::log.
pub(super) unsafe extern "C" fn backend_log(_: *mut c_void, text: *const c_char) {
    unsafe { rsf_unity_package_log(text) }
}

/// rsf_game_host_services::cpu_event.
unsafe extern "C" fn host_cpu_event(_: *mut c_void, event: *const rsf_game_cpu_event) {
    if event.is_null() {
        return;
    }
    if let Some(leaves) = loaded() {
        PACKAGE.cpu_event(leaves, unsafe { &*event });
    }
}

/// The package log in every form package.c lends it.
///
/// # Safety
/// `text` is null or NUL-terminated.
pub(crate) unsafe extern "C" fn rsf_unity_package_log(text: *const c_char) {
    if text.is_null() {
        return;
    }
    if let Some(leaves) = loaded() {
        PACKAGE.log_text(leaves, unsafe { CStr::from_ptr(text) });
    }
}

#[cfg(windows)]
pub(super) fn report_window_message(message: u32, lparam: isize) {
    if let Some(leaves) = loaded()
        && let Some(_callback) = InputCallback::enter()
    {
        PACKAGE.window_message(leaves, message, lparam);
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
    match loaded() {
        Some(leaves) => {
            let Some(_callback) = InputCallback::enter() else {
                return backend_result::OK;
            };
            PACKAGE.input(leaves, frame, kinds)
        }
        None => backend_result::FEATURE_FAILED,
    }
}

pub(super) extern "C" fn rsf_unity_package_preview_input() {
    if let Some(leaves) = loaded() {
        PACKAGE.preview_input(leaves);
    }
}

pub(crate) fn editor() -> bool {
    PACKAGE.services.editor.load(SeqCst)
}

#[cfg(windows)]
pub(super) fn unload() {
    if let Some(leaves) = loaded() {
        PACKAGE.unload(leaves);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsfu_input_checks_its_arguments_first() {
        unsafe {
            assert_eq!(rsfu_input(0, 1), backend_result::INVALID_ARGUMENT);
            assert_eq!(rsfu_input(1, 8), backend_result::INVALID_ARGUMENT);
            assert_eq!(rsfu_input(1, 0), backend_result::OK);
            assert_eq!(rsfu_input(1, 7), backend_result::FEATURE_FAILED);
            // A null text or table is ignored.
            rsf_unity_package_log(std::ptr::null());
            rsf_unity_package_log(c"x".as_ptr());
            host_cpu_event(null_mut(), std::ptr::null());
        }
    }
}
