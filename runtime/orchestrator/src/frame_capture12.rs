// SPDX-License-Identifier: MIT
//! Shared first-party D3D12 readback owner behind `rsf::FrameCapture12`.
//!
//! Rust owns configuration, admission, pending identity, slot and fence
//! completion, retirement, core ticket refusal and publishing, and the
//! readback buffer, source and fence leases. The C++ façade supplies typed
//! D3D12/Win32 leaves and the public symbol forwards.
use crate::capture_names::CaptureName;
use crate::com_ref::ComRef;
use crate::frame_capture_vk::{Legacy, Pixels, general, general_precision};
use crate::sr_session::has_capacity;
use rsf_game_sys::{RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record as Record};
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

type Handle = *mut c_void;

const ALL_SLOTS: u32 = u32::MAX;
const DEFAULT_VIEW: u32 = u32::MAX;
const NO_RESET: u32 = u32::MAX;
const STAGING_FAILED: i32 = -4;
const MAP_FAILED: i32 = -5;
const D3D12_API: u32 = 2;
const MAX_FRAMES: u32 = 16;
// `submitted` signals a fresh fence to this value on the recording queue.
const SUBMITTED: u64 = 1;

// rsf_backend_result from rescaleframe/backend.h.
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;

/// Copyable footprint of subresource 0 and the readback buffer size.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Layout {
    pub bytes: u64,
    pub offset: u64,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub row_pitch: u32,
}

#[repr(C)]
pub struct Platform {
    /// Refuses all but a single-sample, single-slice 2D texture, then creates
    /// its readback buffer in COPY_DEST. Returns the buffer with one reference.
    pub allocate: unsafe extern "C" fn(Handle, *mut Layout) -> Handle,
    /// Records the source transitions and the footprint copy.
    pub copy: unsafe extern "C" fn(Handle, Handle, u32, Handle, *const Layout),
    pub map: unsafe extern "C" fn(Handle, u64) -> *const u8,
    pub unmap: unsafe extern "C" fn(Handle),
    /// A new fence signalled to 1 on the queue, with one reference.
    pub signal: unsafe extern "C" fn(Handle) -> Handle,
    pub completed: unsafe extern "C" fn(Handle) -> u64,
    /// Signals the queue and waits at most 15 seconds. Returns the fence
    /// value on a completed wait, or zero if the wait was refused.
    pub drain: unsafe extern "C" fn(Handle) -> u64,
    pub wants: unsafe extern "C" fn(*const Record) -> i32,
    pub reserve: unsafe extern "C" fn(*const Record, *const c_char, *const c_char, u32, f32) -> u64,
    pub failed: unsafe extern "C" fn(u64, *const c_char, i32),
    pub pixels: unsafe extern "C" fn(u64, *const c_char, *const Pixels, u32, u32) -> i32,
}

const _: () = {
    assert!(size_of::<Platform>() == 11 * size_of::<Handle>());
    assert!(size_of::<Layout>() == 32);
};

struct Readback {
    buffer: ComRef,
    // The source stays alive until its copy is retired.
    _source: ComRef,
    layout: Layout,
    identity: Record,
    role: String,
    slot: u32,
    ordinal: u32,
    reset: u32,
    legacy: bool,
    ticket: u64,
    name: CString,
    completion: Option<ComRef>,
}

impl Readback {
    fn submitted_as(&self, slot: u32, identity: Option<&Record>) -> bool {
        self.completion.is_none()
            && (slot == ALL_SLOTS || self.slot == slot)
            && identity.is_none_or(|identity| {
                let r = &self.identity;
                (r.session_id, r.frame_id, r.view_id, r.resource_generation)
                    == (
                        identity.session_id,
                        identity.frame_id,
                        identity.view_id,
                        identity.resource_generation,
                    )
            })
    }
}

struct State {
    legacy: Legacy,
    reset_sr: bool,
    pending: Vec<Readback>,
}

pub struct Owner {
    ops: &'static Platform,
    state: Mutex<State>,
}

fn from_environment() -> (Legacy, bool) {
    let legacy = Legacy::from_variable("RSF_FRAME_CAPTURE_DIR");
    let reset = legacy.directory.is_some()
        && std::env::var_os("RSF_FRAME_CAPTURE_RESET_SR").is_some_and(|value| value == "1");
    (legacy, reset)
}

impl Owner {
    pub(super) fn from_environment(ops: &'static Platform) -> Self {
        let (legacy, reset) = from_environment();
        Self::new(ops, legacy, reset)
    }

    pub(super) fn enabled(&self) -> bool {
        self.lock().legacy.enabled()
    }

    pub(super) fn wants(&self, frame: u64) -> bool {
        self.lock().legacy.wants(frame)
    }

    fn new(ops: &'static Platform, legacy: Legacy, reset_sr: bool) -> Self {
        Self {
            ops,
            state: Mutex::new(State {
                legacy,
                reset_sr,
                pending: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn fail(&self, value: &Readback, result: i32) {
        if value.ticket != 0 {
            unsafe { (self.ops.failed)(value.ticket, value.name.as_ptr(), result) };
        }
    }

    fn configure(&self, path: &CStr, first: u64, frames: u32) -> bool {
        if path.is_empty() || frames == 0 || frames > MAX_FRAMES {
            return false;
        }
        let mut state = self.lock();
        if !state.pending.is_empty() {
            return false;
        }
        let Ok(text) = path.to_str() else {
            return false;
        };
        let selected = PathBuf::from(text);
        if !selected.is_absolute() || fs::create_dir_all(&selected).is_err() {
            return false;
        }
        state.legacy = Legacy {
            directory: Some(selected),
            first,
            count: frames.into(),
        };
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn record(
        &self,
        list: Handle,
        source: Handle,
        resource_state: u32,
        identity: &Record,
        role: &CStr,
        slot: u32,
        ordinal: u32,
        reset: u32,
        preview: u32,
        preview_scale: f32,
    ) -> bool {
        let mut state = self.lock();
        let _ = self.flush(&mut state, ALL_SLOTS, true);
        let legacy = state.legacy.wants(identity.frame_id);
        let core = unsafe { (self.ops.wants)(identity) } != 0;
        if (!legacy && !core) || list.is_null() || source.is_null() {
            return false;
        }
        let mut layout = Layout::default();
        let Some(buffer) =
            (unsafe { ComRef::from_owned((self.ops.allocate)(source, &mut layout)) })
        else {
            return false;
        };
        let mut readback = Readback {
            buffer,
            _source: unsafe { ComRef::retain(source) },
            layout,
            identity: *identity,
            role: role.to_string_lossy().into_owned(),
            slot,
            ordinal,
            reset,
            legacy,
            ticket: 0,
            name: CString::default(),
            completion: None,
        };
        if core {
            let Some(CaptureName { name, view, scale }) =
                CaptureName::for_role(role, ordinal, u32::MAX as usize)
            else {
                return false;
            };
            let view = if preview == DEFAULT_VIEW {
                view
            } else {
                preview
            };
            let scale = if preview_scale != 0.0 {
                preview_scale
            } else {
                scale
            };
            readback.ticket =
                unsafe { (self.ops.reserve)(identity, name.as_ptr(), role.as_ptr(), view, scale) };
            readback.name = name;
        }
        if !legacy && readback.ticket == 0 {
            return false;
        }
        unsafe {
            (self.ops.copy)(
                list,
                source,
                resource_state,
                readback.buffer.as_ptr(),
                &readback.layout,
            )
        };
        state.pending.push(readback);
        true
    }

    // A slot flush trusts its caller's proof that the slot's submission
    // completed. A ready-only poll publishes only copies whose fence passed.
    fn flush(&self, state: &mut State, slot: u32, ready_only: bool) -> bool {
        self.flush_matching(state, ready_only, |value| {
            slot == ALL_SLOTS || value.slot == slot
        })
    }

    fn flush_matching(
        &self,
        state: &mut State,
        ready_only: bool,
        matches: impl Fn(&Readback) -> bool,
    ) -> bool {
        let mut okay = true;
        let mut index = 0;
        while index < state.pending.len() {
            let value = &state.pending[index];
            if !matches(value) {
                index += 1;
                continue;
            }
            if ready_only {
                let Some(fence) = &value.completion else {
                    index += 1;
                    continue;
                };
                let completed = unsafe { (self.ops.completed)(fence.as_ptr()) };
                if completed == u64::MAX {
                    // A removed device never completes the copy.
                    self.fail(value, MAP_FAILED);
                    okay = false;
                    state.pending.remove(index);
                    continue;
                }
                if completed < SUBMITTED {
                    index += 1;
                    continue;
                }
            }
            let data = unsafe { (self.ops.map)(value.buffer.as_ptr(), value.layout.bytes) };
            if data.is_null() {
                // Retained with its ticket; a later flush retries.
                okay = false;
                index += 1;
                continue;
            }
            okay &= self.publish(&state.legacy, value, data);
            state.pending.remove(index);
        }
        okay
    }

    fn publish(&self, legacy: &Legacy, value: &Readback, data: *const u8) -> bool {
        let mut okay = true;
        let layout = &value.layout;
        if value.ticket != 0 {
            let pixels = Pixels {
                struct_size: size_of::<Pixels>() as u32,
                width: layout.width,
                height: layout.height,
                format: layout.format,
                row_pitch: layout.row_pitch,
                data_size: layout.bytes.saturating_sub(layout.offset),
                data: data.wrapping_add(layout.offset as usize).cast(),
            };
            okay &= unsafe {
                (self.ops.pixels)(
                    value.ticket,
                    value.name.as_ptr(),
                    &pixels,
                    D3D12_API,
                    layout.format,
                )
            } != 0;
        }
        if !value.legacy {
            unsafe { (self.ops.unmap)(value.buffer.as_ptr()) };
            return okay;
        }
        let stem = format!(
            "{}-{}-{}-{}",
            value.identity.session_id, value.identity.frame_id, value.role, value.ordinal
        );
        let directory = legacy.directory.as_deref().unwrap_or(Path::new(""));
        let bytes = unsafe { std::slice::from_raw_parts(data, layout.bytes as usize) };
        let raw = fs::write(directory.join(format!("{stem}.raw")), bytes).is_ok();
        unsafe { (self.ops.unmap)(value.buffer.as_ptr()) };
        let json = fs::write(directory.join(format!("{stem}.json")), legacy_json(value)).is_ok();
        okay && raw && json
    }

    fn submitted(&self, queue: Handle, slot: u32, identity: Option<&Record>) -> bool {
        let mut state = self.lock();
        let matching = |value: &Readback| value.submitted_as(slot, identity);
        if queue.is_null() || !state.pending.iter().any(matching) {
            return true;
        }
        let Some(fence) = (unsafe { ComRef::from_owned((self.ops.signal)(queue)) }) else {
            return false;
        };
        for value in state.pending.iter_mut().filter(|value| matching(value)) {
            value.completion = Some(fence.clone());
        }
        true
    }

    // Unsubmitted recordings of a discarded slot never ran.
    fn abandon(&self, slot: u32) {
        let mut state = self.lock();
        let abandoned = state
            .pending
            .extract_if(.., |value| value.slot == slot && value.completion.is_none());
        for value in abandoned {
            self.fail(&value, MAP_FAILED);
        }
    }

    // Snapshot the readbacks before asking the SDK to finish its submitted
    // Presents. Later recordings are outside that proof. Retained buffers
    // keep their identities unique if another call removes an entry.
    pub(super) fn flush_after_queue(
        &self,
        queue: Handle,
        wait_submissions: impl FnOnce() -> bool,
    ) -> bool {
        self.flush_queue_batch(queue, wait_submissions)
            .unwrap_or(false)
    }

    // The provider already ended SDK recording. Once GPU retirement is
    // proven, a file or map failure is safe to handle on Drop.
    pub(super) fn retire_after_wait(&self, queue: Handle) -> bool {
        self.flush_queue_batch(queue, || true).is_some()
    }

    fn flush_queue_batch(
        &self,
        queue: Handle,
        wait_submissions: impl FnOnce() -> bool,
    ) -> Option<bool> {
        if queue.is_null() {
            return Some(true);
        }
        let batch: Vec<_> = self
            .lock()
            .pending
            .iter()
            .map(|value| value.buffer.clone())
            .collect();
        if batch.is_empty() {
            return Some(true);
        }
        if !wait_submissions() {
            return None;
        }
        let completed = unsafe { (self.ops.drain)(queue) };
        if completed < SUBMITTED || completed == u64::MAX {
            return None;
        }
        Some(self.flush_matching(&mut self.lock(), false, |value| {
            batch
                .iter()
                .any(|buffer| buffer.as_ptr() == value.buffer.as_ptr())
        }))
    }

    pub(super) fn event(
        &self,
        identity: &Record,
        name: &CStr,
        sequence: u64,
        reset: bool,
        result: i32,
    ) -> bool {
        let directory = {
            let state = self.lock();
            if !state.legacy.wants(identity.frame_id) {
                return true;
            }
            state.legacy.directory.clone().unwrap_or_default()
        };
        let metadata = directory.join("metadata");
        if fs::create_dir_all(&metadata).is_err() {
            return false;
        }
        let file = metadata.join(format!(
            "{}-{}-{}.json",
            identity.session_id,
            identity.frame_id,
            name.to_string_lossy()
        ));
        fs::write(file, event_json(identity, sequence, reset, result)).is_ok()
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        // Owners drain referencing submissions before destruction. A ticket
        // left here can no longer publish, so refuse it instead of leaving
        // the input dump to time out.
        let pending = std::mem::take(&mut self.lock().pending);
        for value in &pending {
            self.fail(value, STAGING_FAILED);
        }
    }
}

fn event_json(identity: &Record, sequence: u64, reset: bool, result: i32) -> String {
    let (ticks, frequency, thread) = unsafe {
        use windows_sys::Win32::System::Performance::{
            QueryPerformanceCounter, QueryPerformanceFrequency,
        };
        let (mut ticks, mut frequency) = (0i64, 0i64);
        QueryPerformanceCounter(&mut ticks);
        QueryPerformanceFrequency(&mut frequency);
        (
            ticks,
            frequency,
            windows_sys::Win32::System::Threading::GetCurrentThreadId(),
        )
    };
    format!(
        "{{\"frame\":{},\"session\":{},\"view\":{},\"generation\":{},\"flags\":{},\"sdk_sequence\":{sequence},\"frame_time_ms\":{},\"reset\":{},\"result\":{result},\"cpu_ticks\":{ticks},\"cpu_tick_frequency\":{frequency},\"thread\":{thread}}}\n",
        identity.frame_id,
        identity.session_id,
        identity.view_id,
        identity.resource_generation,
        identity.flags,
        general_precision(f64::from(identity.frame_time_ms), 6),
        u8::from(reset),
    )
}

fn legacy_json(value: &Readback) -> String {
    let identity = &value.identity;
    let camera = &identity.camera;
    let layout = &value.layout;
    let f = |v: f32| general(f64::from(v));
    let mut json = String::new();
    let _ = write!(
        json,
        "{{\n\"role\":\"{}\",\n\"frame\":{},\n\"session\":{},\n\"view\":{},\n\"generation\":{},\n\"flags\":{},\n\"frame_time_ms\":{},\n\"width\":{},\n\"height\":{},\n\"row_pitch\":{},\n\"offset\":{},\n\"dxgi_format\":{},\n\"bytes\":{},\n\"render_size\":[{},{}],\n\"output_size\":[{},{}],\n\"jitter_pixels\":[{},{}],\n\"matrix_convention\":\"row_vectors\"",
        value.role,
        identity.frame_id,
        identity.session_id,
        identity.view_id,
        identity.resource_generation,
        identity.flags,
        f(identity.frame_time_ms),
        layout.width,
        layout.height,
        layout.row_pitch,
        layout.offset,
        layout.format,
        layout.bytes,
        camera.render_width,
        camera.render_height,
        camera.output_width,
        camera.output_height,
        f(camera.jitter_pixels[0]),
        f(camera.jitter_pixels[1]),
    );
    for (key, values) in [
        ("view_to_clip", &camera.view_to_clip),
        ("clip_to_view", &camera.clip_to_view),
        ("view_to_world", &camera.view_to_world),
        ("world_to_view", &camera.world_to_view),
        ("clip_to_previous_clip", &camera.clip_to_previous_clip),
    ] {
        let joined: Vec<String> = values.iter().map(|&v| f(v)).collect();
        let _ = write!(json, ",\n\"{key}\":[{}]", joined.join(","));
    }
    if value.reset != NO_RESET {
        let _ = write!(json, ",\n\"sdk_dispatch_reset\":{}", value.reset);
    }
    json.push_str("\n}\n");
    json
}

unsafe fn owner<'a>(owner: *const Owner) -> Option<&'a Owner> {
    unsafe { owner.as_ref() }
}

unsafe fn text<'a>(value: *const c_char) -> Option<&'a CStr> {
    (!value.is_null()).then(|| unsafe { CStr::from_ptr(value) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_create(
    environment: u32,
    ops: &'static Platform,
) -> *mut Owner {
    let owner = if environment != 0 {
        Owner::from_environment(ops)
    } else {
        Owner::new(ops, Legacy::default(), false)
    };
    crate::allocate_handle(owner)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_destroy(owner: *mut Owner) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_enabled(owner: *const Owner) -> i32 {
    i32::from(unsafe { self::owner(owner) }.is_some_and(|owner| owner.enabled()))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_configure(
    owner: *const Owner,
    path: *const c_char,
    first: u64,
    frames: u32,
) -> i32 {
    match (unsafe { self::owner(owner) }, unsafe { text(path) }) {
        (Some(owner), Some(path)) => i32::from(owner.configure(path, first, frames)),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_wants(owner: *const Owner, frame: u64) -> i32 {
    i32::from(unsafe { self::owner(owner) }.is_some_and(|owner| owner.wants(frame)))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_force_reset(owner: *const Owner, frame: u64) -> i32 {
    i32::from(unsafe { self::owner(owner) }.is_some_and(|owner| {
        let state = owner.lock();
        state.reset_sr && state.legacy.wants(frame)
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_event(
    owner: *const Owner,
    identity: *const Record,
    name: *const c_char,
    sequence: u64,
    reset: u32,
    result: i32,
) -> i32 {
    match (
        unsafe { self::owner(owner) },
        unsafe { identity.as_ref() },
        unsafe { text(name) },
    ) {
        (Some(owner), Some(identity), Some(name)) => {
            i32::from(owner.event(identity, name, sequence, reset != 0, result))
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn rsf_core_capture12_record(
    owner: *const Owner,
    list: Handle,
    source: Handle,
    state: u32,
    identity: *const Record,
    role: *const c_char,
    slot: u32,
    ordinal: u32,
    reset: u32,
    preview: u32,
    preview_scale: f32,
) -> i32 {
    match (
        unsafe { self::owner(owner) },
        unsafe { identity.as_ref() },
        unsafe { text(role) },
    ) {
        (Some(owner), Some(identity), Some(role)) => i32::from(owner.record(
            list,
            source,
            state,
            identity,
            role,
            slot,
            ordinal,
            reset,
            preview,
            preview_scale,
        )),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_flush(
    owner: *const Owner,
    slot: u32,
    ready_only: u32,
) -> i32 {
    unsafe { self::owner(owner) }.map_or(0, |owner| {
        let mut state = owner.lock();
        i32::from(owner.flush(&mut state, slot, ready_only != 0))
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_submitted(
    owner: *const Owner,
    queue: Handle,
    slot: u32,
    identity: *const Record,
) -> i32 {
    unsafe { self::owner(owner) }.map_or(0, |owner| {
        i32::from(owner.submitted(queue, slot, unsafe { identity.as_ref() }))
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_abandon(owner: *const Owner, slot: u32) {
    if let Some(owner) = unsafe { self::owner(owner) } {
        owner.abandon(slot);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture12_flush_after_queue(
    owner: *const Owner,
    queue: Handle,
    wait_submissions: unsafe extern "C" fn(Handle) -> i32,
    user: Handle,
) -> i32 {
    unsafe { self::owner(owner) }.map_or(0, |owner| {
        i32::from(owner.flush_after_queue(queue, || unsafe { wait_submissions(user) } != 0))
    })
}

// Public capture readers use the same owner as the embedded backends without
// the environment-driven capture window.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump12_create(out: *mut *mut Owner, ops: &'static Platform) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    let owner = unsafe { rsf_core_capture12_create(0, ops) };
    unsafe { out.write(owner) };
    if owner.is_null() { INIT_FAILED } else { OK }
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn rsf_core_dump12_record(
    owner: *const Owner,
    list: Handle,
    source: Handle,
    identity: *const Record,
    role: *const c_char,
    resource_state: u32,
    slot: u32,
    view: u32,
    scale: f32,
) -> i32 {
    let Some(owner) = (unsafe { self::owner(owner) }) else {
        return INVALID;
    };
    if list.is_null() || source.is_null() || role.is_null() || !unsafe { has_capacity(identity) } {
        return INVALID;
    }
    let identity = unsafe { &*identity };
    if identity.abi_version != RSF_GAME_FRAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if unsafe { (owner.ops.wants)(identity) } == 0 {
        return OK;
    }
    let role = unsafe { CStr::from_ptr(role) };
    let scale = if scale == 0.0 { 1.0 } else { scale };
    if owner.record(
        list,
        source,
        resource_state,
        identity,
        role,
        slot,
        0,
        NO_RESET,
        view,
        scale,
    ) {
        OK
    } else {
        FEATURE_FAILED
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump12_submitted(
    owner: *const Owner,
    queue: Handle,
    slot: u32,
) -> i32 {
    let Some(owner) = (unsafe { self::owner(owner) }) else {
        return INVALID;
    };
    if queue.is_null() {
        INVALID
    } else if owner.submitted(queue, slot, None) {
        OK
    } else {
        FEATURE_FAILED
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump12_poll(owner: *const Owner) -> i32 {
    let Some(owner) = (unsafe { self::owner(owner) }) else {
        return INVALID;
    };
    if owner.flush(&mut owner.lock(), ALL_SLOTS, true) {
        OK
    } else {
        FEATURE_FAILED
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump12_abandon(owner: *const Owner, slot: u32) {
    unsafe { rsf_core_capture12_abandon(owner, slot) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump12_destroy(owner: *mut Owner) -> i32 {
    let Some(capture) = (unsafe { self::owner(owner) }) else {
        return OK;
    };
    {
        let mut state = capture.lock();
        if !capture.flush(&mut state, ALL_SLOTS, true) {
            return FEATURE_FAILED;
        }
        if !state.pending.is_empty() {
            return NOT_READY;
        }
    }
    unsafe { rsf_core_capture12_destroy(owner) };
    OK
}

#[cfg(test)]
mod tests;
