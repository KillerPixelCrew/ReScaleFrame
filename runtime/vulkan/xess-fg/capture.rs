// SPDX-License-Identifier: MIT
//! Opt-in XeSS-FG diagnostic capture owner for both bridge builds.
//!
//! Rust owns the configuration, frame source identity, callback counter,
//! admission budgets, duplicate detection, the pending readbacks and their
//! publication after the bridge has drained its DX12 queue. The native leaves
//! keep the fixed fingerprint admission, the typed SDK callback table, the
//! DX12 readback, copy and map calls, and the on-disk file formats.
//!
//! The shared `rsf::FrameCapture12` owner does not fit this path: it records
//! into its own command lists and completes on its own fences, while these
//! copies are recorded into SDK-owned lists inside the diagnostic callbacks
//! and complete with the bridge queue. It also depends on orchestrator crate
//! modules that the standalone `rustc` build of the session cannot link.

use super::{Handle, Platform};
use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

const VALUE_CAPACITY: usize = 32;
const DIRECTORY_CAPACITY: usize = 32_768;
const DEFAULT_FIRST: u64 = 30;
const DEFAULT_COUNT: u64 = 4;
const MAX_COUNT: u64 = 16;
const MAX_SOURCES: usize = 32;
const MAX_PENDING: usize = 64;
const BYTE_BUDGET: u64 = 256 * 1024 * 1024;
// Output callback ordinals and the observed output count stay below this.
const OUTPUT_SLOTS: u32 = 15;
// D3D12_RESOURCE_DIMENSION_TEXTURE2D, DXGI_FORMAT_B8G8R8A8_UNORM and
// D3D12_RESOURCE_STATE_COMMON.
const TEXTURE2D: u32 = 3;
const BGRA8: u32 = 87;
const STATE_COMMON: u32 = 0;
// Results of the native publication leaf.
const PUBLISHED: i32 = 0;
const MAP_FAILED: i32 = -1;
const WRITE_FAILED: i32 = -2;
// RsfXessBridgeCaptureSource result when the source cannot be bound.
const SOURCE_FAILED: i32 = -7012;

/// The rsf_frame_record ABI 2 prefix Rust reads. The remainder, from
/// `input_qpc` through `source_qpc_frequency`, is published unchanged.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Record {
    _header: [u32; 2],
    frame_id: u64,
    _session_id: u64,
    _identity: [u32; 7],
    output_width: u32,
    output_height: u32,
    _tail: [MaybeUninit<u8>; 420],
}

/// The D3D12_RESOURCE_DESC fields the callback contract and receipt use.
#[repr(C)]
#[derive(Default)]
pub(super) struct Desc {
    width: u64,
    height: u32,
    dimension: u32,
    format: u32,
    depth_or_array_size: u32,
    mip_levels: u32,
    sample_count: u32,
}

/// One xess-diagnostic-callbacks.jsonl receipt.
#[repr(C)]
pub(super) struct Event {
    frame: u64,
    token: u64,
    resource: u64,
    commands: u64,
    width: u64,
    kind: u32,
    ordinal: u32,
    generated: u32,
    height: u32,
    format: u32,
    state: u32,
}

/// D3D12_PLACED_SUBRESOURCE_FOOTPRINT.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Footprint {
    offset: u64,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    row_pitch: u32,
}

/// A pending readback. It holds one reference to its readback buffer and only
/// the identities of the borrowed source resource and SDK command list.
#[repr(C)]
pub(super) struct Image {
    readback: Handle,
    footprint: Footprint,
    bytes: u64,
    token: u64,
    source_resource: u64,
    command_list: u64,
    ordinal: u32,
    kind: u32,
    generated_count: u32,
    actual_count: u32,
    generated: u32,
    _reserved: u32,
    source: Record,
}

const _: () = {
    assert!(size_of::<Record>() == 480 && align_of::<Record>() == 8);
    assert!(std::mem::offset_of!(Record, frame_id) == 8);
    assert!(std::mem::offset_of!(Record, output_width) == 52);
    assert!(std::mem::offset_of!(Record, output_height) == 56);
    assert!(std::mem::offset_of!(Record, _tail) == 60);
    assert!(size_of::<Desc>() == 32);
    assert!(size_of::<Event>() == 64);
    assert!(std::mem::offset_of!(Event, kind) == 40);
    assert!(std::mem::offset_of!(Event, state) == 60);
    assert!(size_of::<Footprint>() == 32);
    assert!(std::mem::offset_of!(Footprint, row_pitch) == 24);
    assert!(size_of::<Image>() == 576 && align_of::<Image>() == 8);
    assert!(std::mem::offset_of!(Image, footprint) == 8);
    assert!(std::mem::offset_of!(Image, bytes) == 40);
    assert!(std::mem::offset_of!(Image, ordinal) == 72);
    assert!(std::mem::offset_of!(Image, generated) == 88);
    assert!(std::mem::offset_of!(Image, source) == 96);
};

#[derive(Clone, Copy)]
struct Source {
    record: Record,
    generated_count: u32,
    actual_count: u32,
}

#[derive(Default)]
struct State {
    // Sorted by the low 32 bits of the frame ID, which the SDK token carries.
    // At most MAX_SOURCES entries; the smallest key is evicted first.
    sources: Vec<(u32, Source)>,
    // At most MAX_PENDING images, reserved before any GPU readback exists.
    pending: Vec<Image>,
    retained_bytes: u64,
    registered: bool,
    // Set when checked retirement starts. No receipt or readback follows.
    closed: bool,
}

impl State {
    fn find(&self, token: u64) -> Result<usize, usize> {
        let key = token as u32;
        self.sources.binary_search_by_key(&key, |&(entry, _)| entry)
    }
}

struct Config {
    // NUL-terminated UTF-16. Only the terminator when capture is disabled.
    directory: Vec<u16>,
    first: u64,
    count: u64,
    probe: bool,
}

// GetEnvironmentVariableW into a 32-unit buffer: a value that does not fit
// reports its required size and leaves the buffer unchanged, and an empty
// value reports zero.
fn read_variable(name: &str, buffer: &mut [u16; VALUE_CAPACITY]) -> bool {
    let Some(value) = std::env::var_os(name) else {
        return false;
    };
    let wide: Vec<u16> = value.encode_wide().collect();
    if wide.len() >= VALUE_CAPACITY {
        return true;
    }
    buffer[..wide.len()].copy_from_slice(&wide);
    buffer[wide.len()] = 0;
    !wide.is_empty()
}

fn terminated(buffer: &[u16]) -> &[u16] {
    let end = buffer.iter().position(|&unit| unit == 0);
    &buffer[..end.unwrap_or(buffer.len())]
}

// wcstoull with base 10: leading white space, an optional sign that negates
// modulo 2^64, and saturation on overflow.
fn parse_unsigned(buffer: &[u16]) -> u64 {
    let mut units = terminated(buffer)
        .iter()
        .copied()
        .skip_while(|&unit| unit == 0x20 || (0x09..=0x0d).contains(&unit))
        .peekable();
    let negative = match units.peek() {
        Some(0x2d) => {
            units.next();
            true
        }
        Some(0x2b) => {
            units.next();
            false
        }
        _ => false,
    };
    let mut value = 0u64;
    let mut overflow = false;
    for unit in units.take_while(|unit| (0x30..=0x39).contains(unit)) {
        let next = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(unit - 0x30)));
        match next {
            Some(next) => value = next,
            None => overflow = true,
        }
    }
    if overflow {
        u64::MAX
    } else if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

impl Config {
    fn from_environment() -> Self {
        let mut config = Self {
            directory: vec![0],
            first: DEFAULT_FIRST,
            count: DEFAULT_COUNT,
            probe: false,
        };
        let Some(path) = std::env::var_os("RSF_VULKAN_FRAME_CAPTURE_DIR") else {
            return config;
        };
        let mut directory: Vec<u16> = path.encode_wide().collect();
        if directory.is_empty() || directory.len() >= DIRECTORY_CAPACITY {
            return config;
        }
        let mut value = [0; VALUE_CAPACITY];
        if read_variable("RSF_FRAME_CAPTURE_FIRST", &mut value) {
            config.first = parse_unsigned(&value);
        }
        if read_variable("RSF_FRAME_CAPTURE_COUNT", &mut value) {
            config.count = parse_unsigned(&value).min(MAX_COUNT);
        }
        config.probe = read_variable("RSF_XESS_DIAGNOSTIC_PROBE", &mut value)
            && terminated(&value) == [u16::from(b'1')];
        // A directory that cannot be created leaves capture disabled.
        if std::fs::create_dir_all(&path).is_ok() {
            directory.push(0);
            config.directory = directory;
        }
        config
    }
}

pub(super) struct Capture {
    ops: &'static Platform,
    config: Config,
    state: Mutex<State>,
    callbacks: AtomicU64,
}

impl Capture {
    pub(super) fn from_environment(ops: &'static Platform) -> Self {
        Self {
            ops,
            config: Config::from_environment(),
            state: Mutex::new(State::default()),
            callbacks: AtomicU64::new(0),
        }
    }

    fn enabled(&self) -> bool {
        self.config.directory.len() > 1 && self.config.count != 0
    }

    fn wants(&self, frame: u64) -> bool {
        let config = &self.config;
        config.count != 0 && frame >= config.first && frame - config.first < config.count
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn directory(&self) -> *const u16 {
        self.config.directory.as_ptr()
    }

    fn failure(&self, reason: &CStr) {
        unsafe { (self.ops.capture_failure)(self.directory(), reason.as_ptr()) };
    }

    /// Admits the exact SDK binary and registers the callbacks once the
    /// presenter exists. Disabled capture never inspects the SDK module.
    pub(super) fn initialize(&self, native: Handle, module: Handle) -> bool {
        if !self.enabled() {
            return true;
        }
        let ops = self.ops;
        if unsafe { (ops.capture_fingerprint)(module) } == 0 {
            self.failure(c"diagnostic-runtime-fingerprint");
            return false;
        }
        if unsafe { (ops.find_diagnostics)(native, module) } != 0 {
            self.failure(c"diagnostic-export");
            return false;
        }
        if unsafe { (ops.register_diagnostics)(native) } < 0 {
            self.failure(c"diagnostic-registration");
            return false;
        }
        self.lock().registered = true;
        true
    }

    fn source(&self, record: &Record, generated_count: u32) -> bool {
        if !self.enabled() {
            return true;
        }
        let source = Source {
            record: *record,
            generated_count,
            actual_count: 0,
        };
        let mut state = self.lock();
        // The SDK token carries only the low 32 bits of the frame ID.
        match state.find(record.frame_id) {
            Ok(index) => state.sources[index].1 = source,
            Err(index) => {
                // Room for the bound plus the entry evicted after insertion.
                let spare = MAX_SOURCES + 1 - state.sources.len();
                if state.sources.try_reserve_exact(spare).is_err() {
                    drop(state);
                    self.failure(c"source-binding");
                    return false;
                }
                state
                    .sources
                    .insert(index, (record.frame_id as u32, source));
                if state.sources.len() > MAX_SOURCES {
                    state.sources.remove(0);
                }
            }
        }
        true
    }

    fn before_outputs(&self, token: u64, count: u32) {
        let mut state = self.lock();
        if let Ok(index) = state.find(token) {
            state.sources[index].1.actual_count = if count <= OUTPUT_SLOTS { count } else { 0 };
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn observe(
        &self,
        token: u64,
        kind: u32,
        resource: Handle,
        commands: Handle,
        ordinal: u32,
        generated: bool,
        resource_state: u32,
    ) {
        let ops = self.ops;
        let mut state = self.lock();
        let Ok(index) = state.find(token) else {
            return;
        };
        let source = state.sources[index].1;
        if resource.is_null() || commands.is_null() {
            return;
        }
        if generated {
            self.callbacks.fetch_add(1, Ordering::SeqCst);
        }
        if state.closed {
            return;
        }
        let record = &source.record;
        if !self.wants(record.frame_id) {
            return;
        }
        let mut desc = Desc::default();
        unsafe { (ops.capture_describe)(resource, &mut desc) };
        // Preserve the exact borrowed resource and command-list identities in
        // the receipt. No callback pointer or opaque ordinal is a temporal
        // fraction.
        let event = Event {
            frame: record.frame_id,
            token,
            resource: resource as u64,
            commands: commands as u64,
            width: desc.width,
            kind,
            ordinal,
            generated: u32::from(generated),
            height: desc.height,
            format: desc.format,
            state: resource_state,
        };
        if unsafe { (ops.capture_event)(self.directory(), &event) } != 0 {
            self.failure(c"callback-exception");
            return;
        }
        if self.config.probe {
            return;
        }
        if desc.dimension != TEXTURE2D
            || desc.sample_count != 1
            || desc.depth_or_array_size != 1
            || desc.mip_levels != 1
            || desc.format != BGRA8
            || desc.width != u64::from(record.output_width)
            || desc.height != record.output_height
            || ordinal >= OUTPUT_SLOTS
        {
            self.failure(c"callback-resource-contract");
            return;
        }
        let duplicate = state.pending.iter().any(|image| {
            image.source.frame_id == record.frame_id
                && (image.generated != 0) == generated
                && image.ordinal == ordinal
        });
        if duplicate {
            self.failure(c"duplicate-callback-resource");
            return;
        }
        if state.pending.len() >= MAX_PENDING {
            self.failure(c"capture-count-budget");
            return;
        }
        // Reserve the bounded slot first, so the push below cannot allocate
        // after a readback exists or a copy is recorded.
        let spare = MAX_PENDING - state.pending.len();
        if state.pending.try_reserve_exact(spare).is_err() {
            self.failure(c"callback-exception");
            return;
        }
        let mut footprint = Footprint::default();
        let mut bytes = 0;
        let mut device = std::ptr::null_mut();
        if unsafe { (ops.capture_footprint)(resource, &mut footprint, &mut bytes, &mut device) }
            != 0
        {
            self.failure(c"resource-device");
            return;
        }
        if bytes > BYTE_BUDGET.saturating_sub(state.retained_bytes) {
            unsafe { (ops.capture_release)(device) };
            self.failure(c"capture-byte-budget");
            return;
        }
        let readback = unsafe { (ops.capture_readback)(device, bytes) };
        unsafe { (ops.capture_release)(device) };
        if readback.is_null() {
            self.failure(c"readback-allocation");
            return;
        }
        // Own the allocation before recording any GPU reference.
        state.retained_bytes += bytes;
        state.pending.push(Image {
            readback,
            footprint,
            bytes,
            token,
            source_resource: resource as u64,
            command_list: commands as u64,
            ordinal,
            kind,
            generated_count: source.generated_count,
            actual_count: source.actual_count,
            generated: u32::from(generated),
            _reserved: 0,
            source: source.record,
        });
        unsafe { (ops.capture_copy)(commands, resource, readback, &footprint, resource_state) };
    }

    pub(super) fn observed(&self) -> u64 {
        self.callbacks.load(Ordering::SeqCst)
    }

    /// Publishes every pending readback and unregisters the callbacks. The
    /// bridge disables and drains SDK work and completes its exact DX12 fence
    /// before this call, so no readback is mapped or released earlier.
    pub(super) fn flush(&self, native: Handle) -> bool {
        let ops = self.ops;
        let mut state = self.lock();
        // Retirement has started. A late callback still counts but is no
        // longer admitted, including while the setter runs unlocked below.
        state.closed = true;
        let mut okay = true;
        for image in &state.pending {
            match unsafe { (ops.capture_publish)(self.directory(), image) } {
                PUBLISHED => {}
                MAP_FAILED => {
                    okay = false;
                    self.failure(c"readback-map");
                }
                WRITE_FAILED => okay = false,
                _ => {
                    // The batch stays owned for checked retirement.
                    self.failure(c"readback-flush-exception");
                    return false;
                }
            }
        }
        for image in state.pending.drain(..) {
            unsafe { (ops.capture_release)(image.readback) };
        }
        state.retained_bytes = 0;
        let registered = state.registered;
        // The SDK setter can wait for a callback in flight, which takes this
        // lock. On refusal the registration and the owner stay in place.
        drop(state);
        if registered {
            if unsafe { (ops.unregister_diagnostics)(native) } < 0 {
                self.failure(c"diagnostic-unregister");
                return false;
            }
            self.lock().registered = false;
        }
        okay
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        for image in state.pending.drain(..) {
            unsafe { (self.ops.capture_release)(image.readback) };
        }
    }
}

// The native bridge validates the public record before binding it.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_capture_source(
    capture: *const Capture,
    record: *const Record,
    generated_count: u32,
) -> i32 {
    let (Some(capture), Some(record)) = (unsafe { capture.as_ref() }, unsafe { record.as_ref() })
    else {
        return SOURCE_FAILED;
    };
    if capture.source(record, generated_count) {
        0
    } else {
        SOURCE_FAILED
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_capture_input(
    capture: *const Capture,
    token: u64,
    kind: u32,
    backbuffer: u32,
    resource: Handle,
    commands: Handle,
    resource_state: u32,
) {
    let Some(capture) = (unsafe { capture.as_ref() }) else {
        return;
    };
    if capture.config.probe || backbuffer != 0 {
        capture.observe(token, kind, resource, commands, 0, false, resource_state);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_capture_output(
    capture: *const Capture,
    token: u64,
    kind: u32,
    resource: Handle,
    commands: Handle,
    ordinal: u32,
) {
    if let Some(capture) = unsafe { capture.as_ref() } {
        capture.observe(token, kind, resource, commands, ordinal, true, STATE_COMMON);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_capture_before_outputs(
    capture: *const Capture,
    token: u64,
    count: u32,
) {
    if let Some(capture) = unsafe { capture.as_ref() } {
        capture.before_outputs(token, count);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_capture_callbacks(capture: *const Capture) -> u64 {
    unsafe { capture.as_ref() }.map_or(0, Capture::observed)
}
