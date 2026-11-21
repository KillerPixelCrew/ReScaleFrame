// SPDX-License-Identifier: MIT
//! F9 checkpoint and completion artifacts over the native capture's copied observations.

use super::{EngineRecord, NativeRecordCounts, ROOT_NAMES, RootRecord, observe_velocity, records};
use crate::motion_capture_observations::{Observations, observations};
use std::ffi::{CStr, c_char, c_void};
use std::fmt::{self, Write as _};
use std::io::Write as _;
use std::mem::{offset_of, size_of, take};
use std::slice;

#[derive(Default)]
pub(super) struct Bytes(pub(super) Vec<u8>);

impl Bytes {
    pub(super) fn extend(&mut self, bytes: &[u8]) -> fmt::Result {
        self.0.try_reserve(bytes.len()).map_err(|_| fmt::Error)?;
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

impl fmt::Write for Bytes {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.extend(text.as_bytes())
    }
}

fn write_roots(out: &mut Bytes, roots: &[RootRecord]) -> fmt::Result {
    for r in roots {
        let kind = ROOT_NAMES.get(r.kind as usize).copied().unwrap_or_default();
        // The native producer bounds and sanitizes this field to printable JSON-safe ASCII.
        let name = r.output_name.map(|c| c as u8);
        let name = name.split(|&c| c == 0).next().unwrap_or_default();
        let name = str::from_utf8(name).map_err(|_| fmt::Error)?;
        writeln!(
            out,
            concat!(
                "{{\"kind\":\"{}\",\"id\":{},\"parent\":{},\"qpc\":{},\"thread\":{},\"interval\":{},",
                "\"owner\":{},\"command_list\":{},\"view\":{},\"view_blob\":{},\"context\":{},",
                "\"input_node\":{},\"output_node\":{},\"velocity_node\":{},\"vtable\":{},\"node_blob\":{},",
                "\"family\":{},\"native_frame\":{},\"process_fn\":{},\"desc_fn\":{},\"output_id\":{},",
                "\"output_name\":\"{}\",\"output_valid\":{},\"output_extent\":[{},{}],\"output_format\":{},",
                "\"pooled_target\":{},\"queue_begin\":{},\"queue_end\":{},\"queue_uid\":{},\"commands\":{},",
                "\"velocity_reference\":{},\"velocity_rhi\":{},\"execution_scope\":{},\"native_role\":{},",
                "\"color_input\":{},\"color_output\":{},\"depth\":{},\"motion\":{},\"fields_valid\":{},",
                "\"draw_size\":[{},{}],\"widget\":{},\"target\":{},\"downsample\":{},\"blur_x\":{},",
                "\"blur_y\":{},\"glow\":{}}}"
            ),
            kind,
            r.id,
            r.parent,
            r.qpc,
            r.thread,
            r.interval,
            r.owner,
            r.command_list,
            r.view,
            r.view_blob,
            r.context,
            r.input_node,
            r.output_node,
            r.velocity_node,
            r.vtable,
            r.node_blob,
            r.family,
            r.native_frame,
            r.process_fn,
            r.desc_fn,
            r.output_id,
            name,
            r.output_valid,
            r.width,
            r.height,
            r.format,
            r.pooled_target,
            r.queue_begin,
            r.queue_end,
            r.queue_uid,
            r.commands,
            r.velocity_reference,
            r.velocity_rhi,
            r.execution_scope,
            r.native_role,
            r.color_input,
            r.color_output,
            r.depth,
            r.motion,
            r.fields_valid,
            r.draw_size[0],
            r.draw_size[1],
            r.widget,
            r.target,
            r.downsample,
            r.blur_x,
            r.blur_y,
            r.glow,
        )?;
    }
    Ok(())
}

fn trim_fraction(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

fn write_float(out: &mut Bytes, value: f32) -> fmt::Result {
    if !value.is_finite() {
        return out.write_str("null");
    }
    if value == 0.0 {
        return out.write_str(if value.is_sign_negative() { "-0" } else { "0" });
    }
    // Same default-float conversion as frame_capture_vk::general_precision at precision 9.
    // The stack buffer fits either representation for all finite f32 values.
    let value = f64::from(value);
    let mut buffer = [0; 24];
    let mut cursor = buffer.as_mut_slice();
    write!(cursor, "{value:.8e}").map_err(|_| fmt::Error)?;
    let remaining = cursor.len();
    let scientific = str::from_utf8(&buffer[..buffer.len() - remaining]).map_err(|_| fmt::Error)?;
    let (mantissa, exponent) = scientific.split_once('e').ok_or(fmt::Error)?;
    let exponent: i32 = exponent.parse().map_err(|_| fmt::Error)?;
    if !(-4..9).contains(&exponent) {
        return write!(out, "{}e{exponent:+03}", trim_fraction(mantissa));
    }
    let mut cursor = buffer.as_mut_slice();
    write!(
        cursor,
        "{value:.precision$}",
        precision = (8 - exponent) as usize
    )
    .map_err(|_| fmt::Error)?;
    let remaining = cursor.len();
    let fixed = str::from_utf8(&buffer[..buffer.len() - remaining]).map_err(|_| fmt::Error)?;
    out.write_str(trim_fraction(fixed))
}

fn write_floats(out: &mut Bytes, values: &[f32]) -> fmt::Result {
    out.write_char('[')?;
    for (index, &value) in values.iter().enumerate() {
        if index != 0 {
            out.write_char(',')?;
        }
        write_float(out, value)?;
    }
    out.write_char(']')
}

fn write_engine(out: &mut Bytes, engine: &[EngineRecord]) -> fmt::Result {
    for e in engine {
        let f = &e.facts;
        let reason = observe_velocity(Some(f)).to_str().map_err(|_| fmt::Error)?;
        write!(
            out,
            concat!(
                "{{\"qpc\":{},\"interval\":{},\"thread\":{},\"primitive\":{},\"proxy\":{},\"view\":{},",
                "\"component\":{},\"index\":{},\"accepted\":{},\"reason\":\"{}\",\"mobility\":{},",
                "\"relevance\":{},\"fields_valid\":{},\"visible\":{},\"camera_cut\":{},",
                "\"always_velocity\":{},\"has_velocity_called\":{},\"has_velocity\":{},\"selection_metrics\":"
            ),
            e.qpc,
            e.interval,
            e.thread,
            e.primitive,
            e.proxy,
            e.view,
            e.component,
            e.index,
            f.accepted,
            reason,
            f.mobility,
            f.relevance,
            f.fields_valid,
            f.visible,
            f.camera_cut,
            f.always_velocity,
            f.has_velocity_called,
            f.has_velocity,
        )?;
        write_floats(
            out,
            &[f.radius, f.distance_squared, f.lod_factor, f.minimum_size],
        )?;
        out.write_str(",\"camera\":")?;
        write_floats(out, &e.camera)?;
        out.write_str(",\"origin\":")?;
        write_floats(out, &e.origin)?;
        out.write_str(",\"current\":")?;
        write_floats(out, &e.current)?;
        write!(
            out,
            ",\"history_checked\":{},\"history_found\":{},\"previous\":",
            f.history_checked, f.history_found
        )?;
        write_floats(out, &e.previous)?;
        out.write_str("}\n")?;
    }
    Ok(())
}

struct Snapshot {
    engine: Vec<EngineRecord>,
    roots: Vec<RootRecord>,
    draws: Vec<Vec<u8>>,
    counts: NativeRecordCounts,
}

impl Snapshot {
    fn finish() -> Self {
        // The scheduler disarmed the session and excludes reset until finish returns.
        // Taking the record lock also waits for any in-flight draw commit.
        let mut records = records();
        Self {
            counts: NativeRecordCounts {
                engine: records.engine.len(),
                roots: records.roots.len(),
                commands: records.commands.len(),
                engine_dropped: records.engine_dropped,
                roots_dropped: records.roots_dropped,
                commands_dropped: records.commands_dropped,
                draw_dropped: records.draw_dropped,
            },
            engine: take(&mut records.engine),
            roots: take(&mut records.roots),
            draws: take(&mut records.draws),
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // Return the capacities even if formatting failed. The native guard is still held.
        let mut records = records();
        records.engine = take(&mut self.engine);
        records.roots = take(&mut self.roots);
        records.draws = take(&mut self.draws);
    }
}

// Private ABI mirrored in motion_capture_records.h. Native lists lend bytes for the full call.
#[repr(C)]
struct Artifact {
    data: *const u8,
    size: usize,
}

#[repr(C)]
struct ArtifactList {
    user: *const c_void,
    count: usize,
    at: unsafe extern "C" fn(*const c_void, usize) -> Artifact,
}

impl ArtifactList {
    fn entries(&self) -> impl Iterator<Item = &[u8]> {
        (0..self.count).map(move |index| -> &[u8] {
            // SAFETY: Native supplies its live list, callback and exact size. At finish the
            // session guard prevents mutation, and the checkpoint list is a local copy.
            let entry = unsafe { (self.at)(self.user, index) };
            if entry.size == 0 {
                &[]
            } else {
                // SAFETY: The returned data is valid and unchanged for the list borrow.
                unsafe { slice::from_raw_parts(entry.data, entry.size) }
            }
        })
    }
}

#[repr(C)]
struct Stats {
    qpc_frequency: i64,
    engine_hooks: u32,
    renderer_root_hooks: u32,
    blob_dropped: u32,
    colour_readbacks: u32,
    colour_readback_failures: u32,
    capture_failures: u32,
}

#[repr(C)]
struct FinalArtifacts {
    blobs: ArtifactList,
    user: *const c_void,
    stats: unsafe extern "C" fn(*const c_void) -> Stats,
}

const _: () = {
    assert!(size_of::<Artifact>() == 16 && size_of::<ArtifactList>() == 24);
    assert!(size_of::<Stats>() == 32 && offset_of!(Stats, capture_failures) == 28);
    assert!(size_of::<FinalArtifacts>() == 40 && offset_of!(FinalArtifacts, stats) == 32);
};

fn write_summary(
    out: &mut Bytes,
    counts: &NativeRecordCounts,
    stats: &Stats,
    draws: usize,
    observations: &Observations,
    files_written: bool,
) -> fmt::Result {
    let (readbacks, readback_failures) = observations.constant_readback_counts();
    writeln!(
        out,
        concat!(
            "{{\"schema\":1,\"qpc_frequency\":{},\"engine_hooks\":{},\"renderer_root_hooks\":{},",
            "\"root_records\":{},\"root_dropped\":{},\"command_associations\":{},\"command_associations_dropped\":{},",
            "\"intervals\":{},\"engine_records\":{},\"draw_records\":{},\"engine_dropped\":{},\"draw_dropped\":{},",
            "\"blob_dropped\":{},\"shader_creation_dropped\":{},\"readbacks\":{},\"readback_failures\":{},",
            "\"colour_readbacks\":{},\"colour_readback_failures\":{},\"capture_failures\":{},\"files_written\":{},",
            "\"timing\":\"present intervals and QPC; CPU selection may precede GPU execution\",",
            "\"draw_state\":\"shadow draw facts are game-requested; live resources and targets are post-draw bindings; temporary constant-buffer overrides restored\",",
            "\"gpu_context\":\"installed immediate context; deferred command lists not decomposed\",",
            "\"native_scope\":\"thread-local engine call scope only; zero outside it. CPU scopes are not GPU frame identity. view_blob uses cb_<id>.bin and AC7 stride 0x27c0\",",
            "\"queued_scope\":\"candidate graph-to-RHI command match by pointer and execute thunk, bounded to sampled native calls; validates neither GPU completion nor Present identity\",",
            "\"selection_metric_order\":[\"bounds_radius\",\"distance_squared\",\"lod_factor\",\"minimum_size\"]}}"
        ),
        stats.qpc_frequency,
        stats.engine_hooks,
        stats.renderer_root_hooks,
        counts.roots,
        counts.roots_dropped,
        counts.commands,
        counts.commands_dropped,
        crate::motion_capture_session::INTERVALS,
        counts.engine,
        draws,
        counts.engine_dropped,
        counts.draw_dropped,
        stats.blob_dropped,
        observations.shaders_dropped,
        readbacks,
        readback_failures,
        stats.colour_readbacks,
        stats.colour_readback_failures,
        stats.capture_failures,
        files_written,
    )
}

type WriteFile = unsafe extern "C" fn(*const c_char, *const u8, usize) -> bool;

#[repr(u32)]
enum Publication {
    Failed,
    Partial,
    Complete,
}

impl From<Result<bool, fmt::Error>> for Publication {
    fn from(result: Result<bool, fmt::Error>) -> Self {
        match result {
            Ok(true) => Self::Complete,
            Ok(false) => Self::Partial,
            Err(_) => Self::Failed,
        }
    }
}

struct Publisher<'a> {
    prefix: &'a CStr,
    write: WriteFile,
    path: Bytes,
    ok: bool,
}

impl<'a> Publisher<'a> {
    fn new(prefix: &'a CStr, write: WriteFile) -> Self {
        Self {
            prefix,
            write,
            path: Bytes::default(),
            ok: true,
        }
    }

    fn file(&mut self, suffix: fmt::Arguments<'_>, data: &[u8]) -> fmt::Result {
        self.path.0.clear();
        self.path.extend(self.prefix.to_bytes())?;
        self.path.write_fmt(suffix)?;
        self.path.write_char('\0')?;
        // SAFETY: The native writer borrows this terminated narrow path and payload only
        // for the call. An I/O failure still allows attempts at the remaining files.
        self.ok &= unsafe { (self.write)(self.path.0.as_ptr().cast(), data.as_ptr(), data.len()) };
        Ok(())
    }

    fn blobs(&mut self, blobs: &ArtifactList) -> fmt::Result {
        for (index, bytes) in blobs.entries().enumerate() {
            self.file(format_args!("\\cb_{}.bin", index + 1), bytes)?;
        }
        Ok(())
    }

    fn checkpoint(
        mut self,
        roots: &[RootRecord],
        blobs: &ArtifactList,
    ) -> Result<bool, fmt::Error> {
        let mut text = Bytes::default();
        write_roots(&mut text, roots)?;
        self.file(format_args!("\\native.partial.jsonl"), &text.0)?;
        self.blobs(blobs)?;
        Ok(self.ok)
    }

    fn finish(mut self, sources: &FinalArtifacts) -> Result<bool, fmt::Error> {
        // Freeze creation observations through publication so the shader ordinals and drop
        // counter describe the same cache. Disassembly happens before taking this lock.
        let observations = observations();
        let snapshot = Snapshot::finish();
        let mut text = Bytes::default();
        write_engine(&mut text, &snapshot.engine)?;
        self.file(format_args!("\\engine.jsonl"), &text.0)?;
        text.0.clear();
        write_roots(&mut text, &snapshot.roots)?;
        self.file(format_args!("\\native.jsonl"), &text.0)?;
        drop(text);
        let mut text = Bytes::default();
        for draw in &snapshot.draws {
            text.extend(draw)?;
            text.write_char('\n')?;
        }
        self.file(format_args!("\\draws.jsonl"), &text.0)?;
        self.blobs(&sources.blobs)?;
        for (index, shader) in observations.shaders.iter().enumerate() {
            self.file(
                format_args!(
                    "\\shader_{}_{}_{}.dxbc",
                    shader.stage,
                    index + 1,
                    shader.hash()
                ),
                &shader.data,
            )?;
        }
        // SAFETY: The native callback samples completion counters while the guard and
        // capture are still live, after the other file attempts as in the original writer.
        let stats = unsafe { (sources.stats)(sources.user) };
        text.0.clear();
        write_summary(
            &mut text,
            &snapshot.counts,
            &stats,
            snapshot.draws.len(),
            &observations,
            self.ok,
        )?;
        self.file(format_args!("\\session.json"), &text.0)?;
        Ok(self.ok)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_publish_checkpoint(
    prefix: *const c_char,
    write: WriteFile,
    roots: *const RootRecord,
    count: usize,
    blobs: &ArtifactList,
) -> Publication {
    let roots = if count == 0 {
        &[]
    } else {
        // SAFETY: Native lends its completed checkpoint copy for this call.
        unsafe { slice::from_raw_parts(roots, count) }
    };
    // SAFETY: Native lends the checkpoint's terminated narrow prefix for this call.
    let prefix = unsafe { CStr::from_ptr(prefix) };
    Publisher::new(prefix, write)
        .checkpoint(roots, blobs)
        .into()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_publish_finish(
    prefix: *const c_char,
    write: WriteFile,
    sources: &FinalArtifacts,
) -> Publication {
    // SAFETY: Native lends the guarded session's terminated narrow prefix for this call.
    let prefix = unsafe { CStr::from_ptr(prefix) };
    Publisher::new(prefix, write).finish(sources).into()
}
