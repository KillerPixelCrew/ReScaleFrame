// SPDX-License-Identifier: MIT
//! Opt-in XeSS-FG diagnostic capture owner for both bridge builds.
//!
//! Rust owns the configuration, frame source identity, callback counter,
//! admission budgets, duplicate detection, the pending readbacks and their
//! publication after the bridge has drained its DX12 queue. The native leaves
//! provide the loaded module digest, the typed SDK callback table, and the
//! DX12 readback, copy and map calls.
//!
//! The shared `rsf::FrameCapture12` owner does not fit this path: it records
//! into its own command lists and completes on its own fences, while these
//! copies are recorded into SDK-owned lists inside the diagnostic callbacks
//! and complete with the bridge queue. It also depends on orchestrator crate
//! modules that the standalone `rustc` build of the session cannot link.

use super::sdk::{XEFG_SWAPCHAIN_RES_BACKBUFFER, xefg_swapchain_d3d12_resource_data_t};
use super::session_records_abi::rsf_xess_capture_desc as Desc;
use super::{Handle, Platform};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

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
const PROVIDER_SHA256: &str = "ec5e0c65e075570c6ede72618bb666d0be0c2e10b2ea9762c0fe8cb8e375ab27";
const _: () = assert!(PROVIDER_SHA256.len() == 64);
// RsfXessBridgeCaptureSource result when the source cannot be bound.
const SOURCE_FAILED: i32 = -7012;

use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record as Record,
};

/// One xess-diagnostic-callbacks.jsonl receipt.
struct Event {
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
struct Image {
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
    source: Record,
}

const _: () = {
    assert!(size_of::<Record>() == 480 && align_of::<Record>() == 8);
    assert!(std::mem::offset_of!(Record, frame_id) == 8);
    assert!(std::mem::offset_of!(Record, output_width) == 52);
    assert!(std::mem::offset_of!(Record, output_height) == 56);
    assert!(size_of::<Desc>() == 32);
    assert!(size_of::<Footprint>() == 32);
    assert!(std::mem::offset_of!(Footprint, row_pitch) == 24);
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
    directory: Option<PathBuf>,
    first: u64,
    count: u64,
    probe: bool,
}

use rsf_game_sys::diagnostic_preferences;

fn read_number(name: &str) -> Option<u64> {
    diagnostic_preferences::value(name)?
        .to_str()?
        .trim()
        .parse()
        .ok()
}

impl Config {
    fn from_environment() -> Self {
        let mut config = Self {
            directory: None,
            first: DEFAULT_FIRST,
            count: DEFAULT_COUNT,
            probe: false,
        };
        let Some(path) = diagnostic_preferences::value("RSF_VULKAN_FRAME_CAPTURE_DIR") else {
            return config;
        };
        if path.is_empty() {
            return config;
        }
        if let Some(first) = read_number("RSF_FRAME_CAPTURE_FIRST") {
            config.first = first;
        }
        if let Some(count) = read_number("RSF_FRAME_CAPTURE_COUNT") {
            config.count = count.min(MAX_COUNT);
        }
        config.probe = diagnostic_preferences::value("RSF_XESS_DIAGNOSTIC_PROBE")
            .is_some_and(|value| value == "1");
        // A directory that cannot be created leaves capture disabled.
        if std::fs::create_dir_all(&path).is_ok() {
            config.directory = Some(path.into());
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
        self.config.directory.is_some() && self.config.count != 0
    }

    fn wants(&self, frame: u64) -> bool {
        let config = &self.config;
        config.count != 0 && frame >= config.first && frame - config.first < config.count
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn failure(&self, reason: &str) {
        if let Some(directory) = &self.config.directory {
            let _ = append(directory.join("xess-capture-errors.log"))
                .and_then(|mut file| writeln!(file, "{reason}"));
        }
    }

    /// Admits the exact SDK binary and registers the callbacks once the
    /// presenter exists. Disabled capture never inspects the SDK module.
    pub(super) fn initialize(&self, native: Handle, module: Handle) -> bool {
        if !self.enabled() {
            return true;
        }
        let ops = self.ops;
        let mut digest = [0; 32];
        let hex = b"0123456789abcdef";
        let expected = PROVIDER_SHA256.as_bytes().chunks_exact(2);
        if unsafe { (ops.capture_digest)(module, digest.as_mut_ptr()) } == 0
            || !digest
                .iter()
                .zip(expected)
                .all(|(&byte, pair)| pair == [hex[(byte >> 4) as usize], hex[(byte & 15) as usize]])
        {
            self.failure("diagnostic-runtime-fingerprint");
            return false;
        }
        let setter = unsafe {
            (ops.lookup_export)(module, c"xefgSwapChainD3D12SetDiagnosticCallbacks".as_ptr())
        };
        if setter.is_null() {
            self.failure("diagnostic-export");
            return false;
        }
        unsafe { (ops.bind_diagnostics)(native, setter) };
        if unsafe { (ops.register_diagnostics)(native) } < 0 {
            self.failure("diagnostic-registration");
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
                    self.failure("source-binding");
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
        if let Some(directory) = &self.config.directory
            && event.write(directory).is_err()
        {
            self.failure("callback-write");
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
            self.failure("callback-resource-contract");
            return;
        }
        let duplicate = state.pending.iter().any(|image| {
            image.source.frame_id == record.frame_id
                && (image.generated != 0) == generated
                && image.ordinal == ordinal
        });
        if duplicate {
            self.failure("duplicate-callback-resource");
            return;
        }
        if state.pending.len() >= MAX_PENDING {
            self.failure("capture-count-budget");
            return;
        }
        // Reserve the bounded slot first, so the push below cannot allocate
        // after a readback exists or a copy is recorded.
        let spare = MAX_PENDING - state.pending.len();
        if state.pending.try_reserve_exact(spare).is_err() {
            self.failure("callback-exception");
            return;
        }
        let mut footprint = Footprint::default();
        let mut bytes = 0;
        let mut device = std::ptr::null_mut();
        if unsafe { (ops.capture_footprint)(resource, &mut footprint, &mut bytes, &mut device) }
            != 0
        {
            self.failure("resource-device");
            return;
        }
        if bytes > BYTE_BUDGET.saturating_sub(state.retained_bytes) {
            unsafe { (ops.capture_release)(device) };
            self.failure("capture-byte-budget");
            return;
        }
        let readback = unsafe { (ops.capture_readback)(device, bytes) };
        unsafe { (ops.capture_release)(device) };
        if readback.is_null() {
            self.failure("readback-allocation");
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
        if let Some(directory) = &self.config.directory {
            for image in &state.pending {
                if let Err(reason) = image.publish(ops, directory) {
                    okay = false;
                    self.failure(reason);
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
                self.failure("diagnostic-unregister");
                return false;
            }
            self.lock().registered = false;
        }
        okay
    }
}

fn append(path: PathBuf) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

impl Event {
    fn write(&self, directory: &Path) -> io::Result<()> {
        let line = format!(
            concat!(
                "{{\"frame\":{},\"token\":{},\"kind\":{},\"ordinal\":{},",
                "\"generated\":{},\"resource\":{},\"commands\":{},",
                "\"width\":{},\"height\":{},\"format\":{},",
                "\"state\":{},\"fraction_established\":false}}\n",
            ),
            self.frame,
            self.token,
            self.kind,
            self.ordinal,
            self.generated != 0,
            self.resource,
            self.commands,
            self.width,
            self.height,
            self.format,
            self.state,
        );
        append(directory.join("xess-diagnostic-callbacks.jsonl"))?.write_all(line.as_bytes())
    }
}

impl Image {
    fn publish(&self, ops: &Platform, directory: &Path) -> Result<(), &'static str> {
        let record = &self.source;
        let generated = self.generated != 0;
        let (role, boundary) = if generated {
            (
                "fg-xess-generated-before-present",
                "sdk-interpolation-output-callback",
            )
        } else {
            (
                "fg-xess-sdk-backbuffer-input",
                "sdk-input-backbuffer-callback",
            )
        };
        let stem = format!(
            "{}-{}-{role}-{}",
            record.session_id, record.frame_id, self.ordinal
        );

        let mut pixels = ptr::null();
        if unsafe { (ops.capture_map)(self.readback, self.bytes, &mut pixels) } != 0 {
            return Err("readback-map");
        }
        let written = unsafe { self.write_pixels(&directory.join(format!("{stem}.raw")), pixels) };
        unsafe { (ops.capture_unmap)(self.readback) };
        let receipt = self.write_receipt(&directory.join(format!("{stem}.json")), role, boundary);
        written.and(receipt).map_err(|_| "readback-write")
    }

    // The mapping remains live for this call. D3D12 writes the active texels
    // of each row. Emit zeros for footprint padding without reading it.
    unsafe fn write_pixels(&self, path: &Path, pixels: *const u8) -> io::Result<()> {
        let layout = &self.footprint;
        let start = layout.offset as usize;
        let row_bytes = layout.width as usize * 4;
        let pitch = layout.row_pitch as usize;
        let rows = layout.height as usize;
        let size = self.bytes as usize;
        let end = rows
            .checked_sub(1)
            .and_then(|last| last.checked_mul(pitch))
            .and_then(|offset| start.checked_add(offset))
            .and_then(|offset| offset.checked_add(row_bytes));
        if pixels.is_null()
            || layout.format != BGRA8
            || layout.depth != 1
            || row_bytes > pitch
            || end.is_none_or(|end| end > size)
        {
            return Err(io::Error::other("invalid mapped footprint"));
        }
        let mut raw = BufWriter::new(File::create(path)?);
        let mut cursor = 0;
        for row in 0..rows {
            let offset = start + row * pitch;
            write_padding(&mut raw, offset - cursor)?;
            raw.write_all(unsafe { std::slice::from_raw_parts(pixels.add(offset), row_bytes) })?;
            cursor = offset + row_bytes;
        }
        write_padding(&mut raw, size - cursor)?;
        raw.flush()
    }

    fn write_receipt(&self, path: &Path, role: &str, boundary: &str) -> io::Result<()> {
        let record = &self.source;
        let camera = &record.camera;
        let layout = &self.footprint;
        // The metadata fields read by capture tooling retain their previous
        // names and units. The matrices are sixteen row-major numbers.
        let receipt = format!(
            concat!(
                "{{\n\"role\":\"{}\",\n\"frame\":{},\n\"session\":{},\n\"view\":{},",
                "\n\"generation\":{},\n\"flags\":{},\n\"frame_time_ms\":{},",
                "\n\"width\":{},\n\"height\":{},\n\"row_pitch\":{},\n\"offset\":{},",
                "\n\"dxgi_format\":{},\n\"bytes\":{},\n\"fg_backend\":5,\n\"sdk_token\":{},",
                "\n\"sdk_output_kind\":{},\n\"sdk_array_ordinal\":{},\n\"fraction_established\":false,",
                "\n\"requested_generated_count\":{},\n\"observed_output_count\":{},\n\"generated\":{},",
                "\n\"capture_boundary\":\"{}\",\n\"provider_sha256\":\"{}\",",
                "\n\"source_resource\":{},\n\"command_list\":{},",
                "\n\"completion\":\"bridge-owned-dx12-queue-drained\",\n\"render_size\":[{},{}],",
                "\n\"output_size\":[{},{}],\n\"jitter_pixels\":{:?},\n\"matrix_convention\":\"row_vectors\",",
                "\n\"view_to_clip\":{:?},\n\"clip_to_view\":{:?},\n\"view_to_world\":{:?},",
                "\n\"world_to_view\":{:?},\n\"clip_to_previous_clip\":{:?}\n}}\n",
            ),
            role,
            record.frame_id,
            record.session_id,
            record.view_id,
            record.resource_generation,
            record.flags,
            record.frame_time_ms,
            layout.width,
            layout.height,
            layout.row_pitch,
            layout.offset,
            layout.format,
            self.bytes,
            self.token,
            self.kind,
            self.ordinal,
            self.generated_count,
            self.actual_count,
            self.generated != 0,
            boundary,
            PROVIDER_SHA256,
            self.source_resource,
            self.command_list,
            camera.render_width,
            camera.render_height,
            camera.output_width,
            camera.output_height,
            camera.jitter_pixels,
            camera.view_to_clip,
            camera.clip_to_view,
            camera.view_to_world,
            camera.world_to_view,
            camera.clip_to_previous_clip,
        );
        std::fs::write(path, receipt)
    }
}

fn write_padding(out: &mut impl Write, mut bytes: usize) -> io::Result<()> {
    const ZERO: [u8; 4096] = [0; 4096];
    while bytes != 0 {
        let count = bytes.min(ZERO.len());
        out.write_all(&ZERO[..count])?;
        bytes -= count;
    }
    Ok(())
}

impl Drop for Capture {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        for image in state.pending.drain(..) {
            unsafe { (self.ops.capture_release)(image.readback) };
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_capture_source(
    capture: *const Capture,
    record: *const Record,
    generated_count: u32,
) -> i32 {
    use super::bridge_abi::{RSF_XESS_BRIDGE_ABI_MISMATCH, RSF_XESS_BRIDGE_INVALID_ARGUMENT};

    if record.is_null() || (unsafe { record.cast::<u32>().read() } as usize) < size_of::<Record>() {
        return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    }
    let record = unsafe { &*record };
    if (record.camera.struct_size as usize) < size_of_val(&record.camera)
        || record.frame_id == 0
        || record.frame_id > u64::from(u32::MAX)
        || record.session_id == 0
    {
        return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    }
    if record.abi_version != RSF_GAME_FRAME_ABI_VERSION
        || record.camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION
    {
        return RSF_XESS_BRIDGE_ABI_MISMATCH;
    }
    let Some(capture) = (unsafe { capture.as_ref() }) else {
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
    data: *const xefg_swapchain_d3d12_resource_data_t,
    commands: Handle,
) {
    let Some(capture) = (unsafe { capture.as_ref() }) else {
        return;
    };
    let Some(data) = (unsafe { data.as_ref() }) else {
        return;
    };
    if capture.config.probe || kind == XEFG_SWAPCHAIN_RES_BACKBUFFER as u32 {
        capture.observe(
            token,
            kind,
            data.pResource,
            commands,
            0,
            false,
            data.incomingState,
        );
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
    if kind == 0
        && let Some(capture) = unsafe { capture.as_ref() }
    {
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
