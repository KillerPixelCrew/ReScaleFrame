// SPDX-License-Identifier: MIT
//! Shared first-party Vulkan readback owner behind `rsf::FrameCaptureVk`.
//!
//! Rust owns frame selection, capture records, completion proof, pending
//! allocation lifetime, refusal and abandon handling, and publishing. The C++
//! façade supplies narrow leaves for Vulkan buffer, memory, event and command
//! calls, the shared capture naming, and the existing input dump C ABI.
use crate::vulkan_sr_abi::rsf_srvk_image as Image;
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

type Handle = *mut c_void;

const UNSUPPORTED_FORMAT: i32 = -3;
const STAGING_FAILED: i32 = -4;
const MAP_FAILED: i32 = -5;
const VULKAN_API: u32 = 3;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FRAMES: u64 = 16;

/// Non-dispatchable handles created for one readback. Zero means absent.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Allocation {
    pub buffer: u64,
    pub memory: u64,
    pub event: u64,
}

#[repr(C)]
pub struct Pixels {
    pub struct_size: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub row_pitch: u32,
    pub data_size: u64,
    pub data: *const c_void,
}

#[repr(C)]
pub struct Platform {
    /// Creates the buffer, host-coherent memory and optional event. Partial
    /// handles stay in `out` on failure; the owner releases them later.
    pub allocate: unsafe extern "C" fn(Handle, *const Image, u64, u32, *mut Allocation) -> i32,
    /// Records the image-to-buffer copy, its barriers and the optional event.
    pub encode: unsafe extern "C" fn(Handle, Handle, *const Image, *const Allocation),
    pub event_set: unsafe extern "C" fn(Handle, u64) -> i32,
    pub map: unsafe extern "C" fn(Handle, u64, u64, *mut *mut c_void) -> i32,
    pub unmap: unsafe extern "C" fn(Handle, u64),
    pub release: unsafe extern "C" fn(Handle, *const Allocation),
    /// Shared capture naming: writes a NUL-terminated name, its view and scale.
    pub name: unsafe extern "C" fn(*const c_char, *mut c_char, u32, *mut u32, *mut f32) -> i32,
    pub wants: unsafe extern "C" fn(*const Record) -> i32,
    pub wants_frame: unsafe extern "C" fn(u64) -> i32,
    pub reserve: unsafe extern "C" fn(*const Record, *const c_char, *const c_char, u32, f32) -> u64,
    pub failed: unsafe extern "C" fn(u64, *const c_char, i32),
    pub pixels: unsafe extern "C" fn(u64, *const c_char, *const Pixels, u32, u32) -> i32,
    pub gpu_ready: unsafe extern "C" fn(u64, *const c_char, u32, Handle) -> i32,
}

const _: () = {
    assert!(size_of::<Platform>() == 13 * size_of::<Handle>());
    assert!(size_of::<Allocation>() == 24);
    assert!(size_of::<Pixels>() == 40);
};

// VkFormat values with their packed pixel size and DXGI storage encoding.
fn layout(format: u32) -> Option<(u32, u32)> {
    Some(match format {
        97 => (8, 10),  // R16G16B16A16_SFLOAT
        103 => (8, 16), // R32G32_SFLOAT
        9 => (1, 61),   // R8_UNORM
        83 => (4, 34),  // R16G16_SFLOAT
        37 => (4, 28),  // R8G8B8A8_UNORM
        43 => (4, 29),  // R8G8B8A8_SRGB
        44 => (4, 87),  // B8G8R8A8_UNORM
        50 => (4, 91),  // B8G8R8A8_SRGB
        64 => (4, 24),  // A2B10G10R10_UNORM_PACK32
        100 => (4, 41), // R32_SFLOAT
        126 => (4, 40), // D32_SFLOAT
        _ => return None,
    })
}

struct Readback {
    commands: usize,
    allocation: Allocation,
    bytes: u64,
    format: u32,
    width: u32,
    height: u32,
    identity: Record,
    role: String,
    reset: u32,
    recorded: bool,
    legacy: bool,
    ticket: u64,
    name: CString,
    group: u64,
    group_complete: bool,
    discarded: bool,
}

// The bounded legacy capture window shared with the D3D12 owner.
#[derive(Default)]
pub(crate) struct Legacy {
    pub(crate) directory: Option<PathBuf>,
    pub(crate) first: u64,
    pub(crate) count: u64,
}

impl Legacy {
    fn from_environment() -> Self {
        Self::from_variable("RSF_VULKAN_FRAME_CAPTURE_DIR")
    }

    /// The directory variable plus `RSF_FRAME_CAPTURE_FIRST` and
    /// `RSF_FRAME_CAPTURE_COUNT`, read with the C++ owners' limits.
    pub(crate) fn from_variable(variable: &str) -> Self {
        let mut legacy = Self {
            directory: None,
            first: 30,
            count: 4,
        };
        let Some(directory) = std::env::var_os(variable)
            .filter(|path| !path.is_empty() && path.encode_wide_len() < 32768)
        else {
            return legacy;
        };
        if let Some(value) = std::env::var_os("RSF_FRAME_CAPTURE_FIRST").filter(|v| !v.is_empty()) {
            legacy.first = parse_count(&value);
        }
        if let Some(value) = std::env::var_os("RSF_FRAME_CAPTURE_COUNT").filter(|v| !v.is_empty()) {
            legacy.count = parse_count(&value).min(MAX_FRAMES);
        }
        let directory = PathBuf::from(directory);
        if fs::create_dir_all(&directory).is_ok() {
            legacy.directory = Some(directory);
        }
        legacy
    }

    pub(crate) fn enabled(&self) -> bool {
        self.directory.is_some() && self.count != 0
    }

    pub(crate) fn wants(&self, frame: u64) -> bool {
        self.enabled() && frame >= self.first && frame - self.first < self.count
    }

    fn failure(&self, role: &str, frame: u64) {
        if let Some(directory) = &self.directory
            && let Ok(mut log) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("capture-errors.log"))
        {
            let _ = writeln!(log, "{frame} {role}");
        }
    }
}

trait WideLength {
    fn encode_wide_len(&self) -> usize;
}

impl WideLength for std::ffi::OsStr {
    fn encode_wide_len(&self) -> usize {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            self.encode_wide().count()
        }
        #[cfg(not(windows))]
        {
            self.len()
        }
    }
}

/// `wcstoull(value, nullptr, 10)` over a 32-character environment buffer.
pub(crate) fn parse_count(value: &std::ffi::OsStr) -> u64 {
    if value.encode_wide_len() >= 32 {
        return 0;
    }
    let text = value.to_string_lossy();
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut parsed = 0u64;
    let mut overflow = false;
    for digit in digits.bytes().take_while(u8::is_ascii_digit) {
        match parsed
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(digit - b'0')))
        {
            Some(next) => parsed = next,
            None => overflow = true,
        }
    }
    if overflow {
        u64::MAX
    } else if negative {
        parsed.wrapping_neg()
    } else {
        parsed
    }
}

struct State {
    legacy: Legacy,
    provider: Provider,
    device: usize,
    pending: Vec<Readback>,
}

pub struct Owner {
    command_completion: bool,
    native: usize,
    ops: &'static Platform,
    state: Mutex<State>,
}

enum Step {
    Keep,
    Retire,
}

impl Owner {
    fn new(
        command_completion: bool,
        native: Handle,
        ops: &'static Platform,
        legacy: Legacy,
    ) -> Self {
        Self {
            command_completion,
            native: native as usize,
            ops,
            state: Mutex::new(State {
                legacy,
                provider: Provider::default(),
                device: 0,
                pending: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn native(&self) -> Handle {
        self.native as Handle
    }

    fn fail(&self, value: &Readback, result: i32) {
        if value.ticket != 0 {
            unsafe { (self.ops.failed)(value.ticket, value.name.as_ptr(), result) };
        }
    }

    fn release(&self, value: &Readback) {
        let a = value.allocation;
        if a.buffer != 0 || a.memory != 0 || a.event != 0 {
            unsafe { (self.ops.release)(self.native(), &a) };
        }
    }

    // Shared-submission owners poll the shared completion proof on every query.
    fn wants(&self, state: &mut State, frame: u64) -> bool {
        if !self.command_completion {
            let _ = self.flush(state, true, 0);
        }
        state.legacy.wants(frame) || unsafe { (self.ops.wants_frame)(frame) } != 0
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        commands: usize,
        image: &Image,
        identity: &Record,
        role: &CStr,
        reset: u32,
        core_source: bool,
        ticket: &mut u64,
        group: u64,
    ) -> bool {
        *ticket = 0;
        let mut state = self.lock();
        let frame = identity.frame_id;
        if !self.wants(&mut state, frame) {
            return true;
        }
        if state.device == 0
            || commands == 0
            || image.device as usize != state.device
            || image.width == 0
            || image.height == 0
        {
            return false;
        }
        let role_text = role.to_string_lossy().into_owned();
        if image.copy_source == 0 {
            state.legacy.failure(&role_text, frame);
            return false;
        }
        let Some((pixel_bytes, _)) = layout(image.format) else {
            state.legacy.failure(&role_text, frame);
            return false;
        };
        let mut owned = Readback {
            commands,
            allocation: Allocation::default(),
            bytes: 0,
            format: image.format,
            width: image.width,
            height: image.height,
            identity: *identity,
            role: role_text,
            reset,
            recorded: false,
            legacy: state.legacy.wants(frame),
            ticket: 0,
            name: CString::default(),
            group,
            group_complete: false,
            discarded: false,
        };
        if core_source && unsafe { (self.ops.wants)(identity) } != 0 {
            let mut name = [0 as c_char; 128];
            let (mut view, mut scale) = (0u32, 1.0f32);
            if unsafe {
                (self.ops.name)(
                    role.as_ptr(),
                    name.as_mut_ptr(),
                    name.len() as u32,
                    &mut view,
                    &mut scale,
                )
            } == 0
            {
                state.legacy.failure(&owned.role, frame);
                return false;
            }
            owned.name = unsafe { CStr::from_ptr(name.as_ptr()) }.to_owned();
            owned.ticket = unsafe {
                (self.ops.reserve)(identity, owned.name.as_ptr(), role.as_ptr(), view, scale)
            };
            *ticket = owned.ticket;
        }
        if !owned.legacy && owned.ticket == 0 {
            return true;
        }
        owned.bytes = u64::from(image.width) * u64::from(image.height) * u64::from(pixel_bytes);
        if owned.bytes == 0 || owned.bytes > MAX_BYTES {
            self.fail(&owned, UNSUPPORTED_FORMAT);
            return false;
        }
        // Retain even a partial allocation. A failed recording has no GPU copy;
        // a recorded copy keeps its allocation until proven retirement or drain.
        state.pending.push(owned);
        let value = state.pending.last_mut().expect("just pushed");
        let code = unsafe {
            (self.ops.allocate)(
                self.native(),
                image,
                value.bytes,
                u32::from(self.command_completion),
                &mut value.allocation,
            )
        };
        if code != 0 {
            let role = value.role.clone();
            state.legacy.failure(&role, frame);
            return false;
        }
        unsafe { (self.ops.encode)(self.native(), commands as Handle, image, &value.allocation) };
        value.recorded = true;
        true
    }

    fn complete_group(&self, group: u64) {
        if group == 0 {
            return;
        }
        for value in &mut self.lock().pending {
            if value.group == group && !value.discarded {
                value.group_complete = true;
            }
        }
    }

    fn discard_group(&self, group: u64) {
        if group == 0 {
            return;
        }
        for value in &mut self.lock().pending {
            if value.group != group || value.discarded {
                continue;
            }
            self.fail(value, STAGING_FAILED);
            value.ticket = 0;
            value.group_complete = false;
            value.discarded = true;
        }
    }

    // Only a retired recording whose caller proved completion may match `completed`.
    // A full flush requires all recorded work to have drained.
    fn flush(&self, state: &mut State, ready_only: bool, completed: usize) -> bool {
        let mut okay = true;
        let mut index = 0;
        while index < state.pending.len() {
            let step = self.flush_one(state, index, ready_only, completed, &mut okay);
            match step {
                Step::Keep => index += 1,
                Step::Retire => {
                    let value = state.pending.remove(index);
                    self.release(&value);
                }
            }
        }
        if !okay {
            state.legacy.failure("readback-flush", 0);
        }
        okay
    }

    fn flush_one(
        &self,
        state: &mut State,
        index: usize,
        ready_only: bool,
        completed: usize,
        okay: &mut bool,
    ) -> Step {
        let State {
            legacy,
            provider,
            device,
            pending,
        } = state;
        let device = *device;
        let value = &mut pending[index];
        if completed != 0 && value.commands != completed {
            return Step::Keep;
        }
        if ready_only && self.command_completion {
            return Step::Keep;
        }
        if value.group != 0 && !value.group_complete {
            if ready_only {
                return Step::Keep;
            }
            if !value.discarded {
                *okay = false;
                self.fail(value, MAP_FAILED);
            }
            return Step::Retire;
        }
        if ready_only && value.legacy && value.ticket == 0 {
            return Step::Keep;
        }
        if !value.recorded {
            *okay = false;
            self.fail(value, STAGING_FAILED);
            return Step::Retire;
        }
        // A drain makes freeing safe, but cannot prove an abandoned recording ran.
        if self.command_completion
            && unsafe { (self.ops.event_set)(self.native(), value.allocation.event) } == 0
        {
            *okay = false;
            self.fail(value, MAP_FAILED);
            return Step::Retire;
        }
        if value.ticket != 0
            && !self.command_completion
            && !value.group_complete
            && unsafe {
                (self.ops.gpu_ready)(
                    value.ticket,
                    value.name.as_ptr(),
                    VULKAN_API,
                    device as Handle,
                )
            } == 0
        {
            if ready_only {
                return Step::Keep;
            }
            *okay = false;
            self.fail(value, MAP_FAILED);
            value.ticket = 0;
            if !value.legacy {
                return Step::Retire;
            }
        }
        let mut data: *mut c_void = std::ptr::null_mut();
        if unsafe {
            (self.ops.map)(
                self.native(),
                value.allocation.memory,
                value.bytes,
                &mut data,
            )
        } != 0
            || data.is_null()
        {
            *okay = false;
            self.fail(value, MAP_FAILED);
            return Step::Retire;
        }
        if value.ticket != 0 {
            let (pixel_bytes, encoding) = layout(value.format).unwrap_or((4, 0));
            let pixels = Pixels {
                struct_size: size_of::<Pixels>() as u32,
                width: value.width,
                height: value.height,
                format: encoding,
                row_pitch: value.width * pixel_bytes,
                data_size: value.bytes,
                data,
            };
            *okay &= unsafe {
                (self.ops.pixels)(
                    value.ticket,
                    value.name.as_ptr(),
                    &pixels,
                    VULKAN_API,
                    value.format,
                )
            } != 0;
            value.ticket = 0;
        }
        if ready_only && value.legacy {
            unsafe { (self.ops.unmap)(self.native(), value.allocation.memory) };
            return Step::Keep;
        }
        if value.legacy {
            let bytes =
                unsafe { std::slice::from_raw_parts(data.cast::<u8>(), value.bytes as usize) };
            *okay &= publish_legacy(
                legacy.directory.as_deref(),
                value,
                bytes,
                completed != 0,
                provider,
            );
        }
        unsafe { (self.ops.unmap)(self.native(), value.allocation.memory) };
        Step::Retire
    }
}

#[derive(Default)]
struct Provider {
    generation: bool,
    backend: u32,
    version: u64,
}

/// C++ iostream `std::setprecision(9)` default float formatting (`%.9g`).
pub(crate) fn general(value: f64) -> String {
    general_precision(value, 9)
}

/// C++ iostream default float formatting at a stream precision (`%.Ng`).
pub(crate) fn general_precision(value: f64, precision: i32) -> String {
    let precision = precision.max(1);
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let scientific = format!("{:.*e}", (precision - 1) as usize, value);
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent");
    let exponent: i32 = exponent.parse().expect("exponent digits");
    let trim = |text: &str| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text.to_owned()
        }
    };
    if !(-4..precision).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim(mantissa), exponent.abs())
    } else {
        trim(&format!(
            "{:.*}",
            (precision - 1 - exponent) as usize,
            value
        ))
    }
}

fn legacy_json(value: &Readback, drained_command: bool, provider: &Provider) -> String {
    let camera = &value.identity.camera;
    let identity = &value.identity;
    let f = |v: f32| general(f64::from(v));
    let mut json = String::new();
    let _ = write!(
        json,
        "{{\n\"role\":\"{}\",\n\"frame\":{},\n\"session\":{},\n\"view\":{},\n\"generation\":{},\n\"flags\":{},\n\"frame_time_ms\":{},\n\"width\":{},\n\"height\":{},\n\"row_pitch\":{},\n\"offset\":0,\n\"vk_format\":{},\n\"bytes\":{},\n\"sdk_dispatch_reset\":{},\n\"completion\":\"{}\",\n\"{}\":{},\n\"provider_version_id\":{},\n\"render_size\":[{},{}],\n\"output_size\":[{},{}],\n\"jitter_pixels\":[{},{}],\n\"matrix_convention\":\"row_vectors\"",
        value.role,
        identity.frame_id,
        identity.session_id,
        identity.view_id,
        identity.resource_generation,
        identity.flags,
        f(identity.frame_time_ms),
        value.width,
        value.height,
        value.bytes / u64::from(value.height),
        value.format,
        value.bytes,
        value.reset,
        if drained_command {
            "owner-drained-command-retirement"
        } else {
            "owner-drained-context-retirement"
        },
        if provider.generation {
            "fg_backend"
        } else {
            "sr_backend"
        },
        provider.backend,
        provider.version,
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
    json.push_str("\n}\n");
    json
}

fn publish_legacy(
    directory: Option<&std::path::Path>,
    value: &Readback,
    bytes: &[u8],
    drained_command: bool,
    provider: &Provider,
) -> bool {
    let Some(directory) = directory else {
        return false;
    };
    let stem = format!(
        "{}-{}-{}-0",
        value.identity.session_id, value.identity.frame_id, value.role
    );
    let raw = fs::write(directory.join(format!("{stem}.raw")), bytes).is_ok();
    let json = fs::write(
        directory.join(format!("{stem}.json")),
        legacy_json(value, drained_command, provider),
    )
    .is_ok();
    raw && json
}

impl Drop for Owner {
    fn drop(&mut self) {
        // Owners drain referencing submissions before destruction.
        let pending = std::mem::take(&mut self.lock().pending);
        for value in &pending {
            self.fail(value, STAGING_FAILED);
            self.release(value);
        }
    }
}

unsafe fn owner<'a>(owner: *const Owner) -> Option<&'a Owner> {
    unsafe { owner.as_ref() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_create(
    command_completion: u32,
    native: Handle,
    ops: &'static Platform,
) -> *mut Owner {
    crate::allocate_handle(Owner::new(
        command_completion != 0,
        native,
        ops,
        Legacy::from_environment(),
    ))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_destroy(owner: *mut Owner) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_enabled(owner: *const Owner) -> i32 {
    i32::from(unsafe { self::owner(owner) }.is_some_and(|owner| owner.lock().legacy.enabled()))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_bind(owner: *const Owner, device: Handle) {
    if let Some(owner) = unsafe { self::owner(owner) } {
        owner.lock().device = device as usize;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_pending(owner: *const Owner) -> u64 {
    unsafe { self::owner(owner) }.map_or(0, |owner| owner.lock().pending.len() as u64)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_provider(
    owner: *const Owner,
    version: u64,
    backend: u32,
    generation: u32,
) {
    if let Some(owner) = unsafe { self::owner(owner) } {
        owner.lock().provider = Provider {
            generation: generation != 0,
            backend,
            version,
        };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_wants(owner: *const Owner, frame: u64) -> i32 {
    unsafe { self::owner(owner) }.map_or(0, |owner| {
        let mut state = owner.lock();
        i32::from(owner.wants(&mut state, frame))
    })
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn rsf_core_capture_vk_record(
    owner: *const Owner,
    commands: Handle,
    image: *const Image,
    identity: *const Record,
    role: *const c_char,
    reset: u32,
    core_source: u32,
    ticket: *mut u64,
    group: u64,
) -> i32 {
    let mut issued = 0;
    let recorded = match (
        unsafe { self::owner(owner) },
        unsafe { image.as_ref() },
        unsafe { identity.as_ref() },
    ) {
        (Some(owner), Some(image), Some(identity)) if !role.is_null() => owner.record(
            commands as usize,
            image,
            identity,
            unsafe { CStr::from_ptr(role) },
            reset,
            core_source != 0,
            &mut issued,
            group,
        ),
        _ => false,
    };
    if !ticket.is_null() {
        unsafe { ticket.write(issued) };
    }
    i32::from(recorded)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_complete_group(owner: *const Owner, group: u64) {
    if let Some(owner) = unsafe { self::owner(owner) } {
        owner.complete_group(group);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_discard_group(owner: *const Owner, group: u64) {
    if let Some(owner) = unsafe { self::owner(owner) } {
        owner.discard_group(group);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_retire(owner: *const Owner, commands: Handle) {
    if let Some(owner) = unsafe { self::owner(owner) }
        && owner.command_completion
        && !commands.is_null()
    {
        let mut state = owner.lock();
        let _ = owner.flush(&mut state, false, commands as usize);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_capture_vk_flush(
    owner: *const Owner,
    ready_only: u32,
    commands: Handle,
) -> i32 {
    unsafe { self::owner(owner) }.map_or(0, |owner| {
        let mut state = owner.lock();
        i32::from(owner.flush(&mut state, ready_only != 0, commands as usize))
    })
}

#[cfg(test)]
mod tests;
