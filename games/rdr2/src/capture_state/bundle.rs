// SPDX-License-Identifier: MIT
//! The poller publishes detached capture metadata without holding the graphics mutex.
#[cfg(not(test))]
use super::artifacts::{output_directory, pipeline_fields, write_camera};
use super::metadata::{Presentation, Temporal};
use super::recordings::Draw;
use super::shots::settle_tail;
use super::{Capture, Progress, can_publish};
#[cfg(not(test))]
use std::fmt::Write as _;
#[cfg(not(test))]
use std::{fs, slice};

#[cfg(not(test))]
pub(super) struct Bundle {
    pub(super) directory: *const u16,
    pub(super) directory_len: usize,
    pub(super) base: u64,
    pub(super) pid: u32,
}

#[cfg(not(test))]
pub(super) struct Published {
    pub(super) sequence: u32,
    pub(super) saved: bool,
    pub(super) inputs: usize,
    pub(super) draws: usize,
    pub(super) written: u32,
}

#[cfg_attr(test, allow(dead_code))]
pub(super) struct Snapshot {
    progress: Progress,
    presentation: Presentation,
    temporal: Vec<Temporal>,
    draws: Vec<Draw>,
    unobserved_hook_roles: u32,
}

#[cfg(not(test))]
fn separator(out: &mut String, index: usize) {
    if index != 0 {
        out.push(',');
    }
}

#[cfg(not(test))]
fn write_draw(out: &mut String, draw: &Draw) {
    let _ = write!(
        out,
        concat!(
            "{{\"draw\":{},\"frame_counter_observation\":{},\"vertices\":{},\"instances\":{},",
            "\"command_list\":\"0x{:x}\",\"generation\":{},\"caller\":\"0x{:x}\",\"pso\":\"0x{:x}\",",
            "{},\"stack\":["
        ),
        draw.ordinal,
        draw.frame,
        draw.vertices,
        draw.instances,
        draw.list,
        draw.generation,
        draw.caller,
        draw.pso,
        pipeline_fields(&draw.pipeline, draw.pipeline_known),
    );
    for (index, address) in draw.stack.iter().take(draw.stack_size as usize).enumerate() {
        separator(out, index);
        let _ = write!(out, "\"0x{:x}\"", address.addr());
    }
    out.push_str("]}");
}

#[cfg(not(test))]
fn write_temporal(out: &mut String, input: &Temporal) {
    let facts = input.facts;
    let _ = write!(
        out,
        concat!(
            "{{\"index\":{},\"frame_counter_observation\":{},\"thread\":{},\"feature_flags\":{},",
            "\"tick\":{},\"qpc\":{},\"command_list\":\"0x{:x}\",\"generation\":{},\"view\":\"0x{:x}\",",
            "\"draw_context\":\"0x{:x}\",\"job\":\"0x{:x}\",\"camera_valid\":{},\"evaluation_result\":{},\"scalars\":{{"
        ),
        input.id.index,
        facts.frame,
        facts.thread,
        input.feature_flags,
        facts.tick,
        facts.qpc,
        input.list,
        input.generation,
        facts.view,
        facts.draw_context,
        facts.job,
        input.camera.is_some(),
        input.result.unwrap_or_default(),
    );
    for (index, (key, value)) in input.scalars().enumerate() {
        separator(out, index);
        let _ = write!(out, "\"{key}\":{value}");
    }
    out.push_str("},\"refusals\":[");
    for (index, (prefix, suffix)) in input.refusals().enumerate() {
        separator(out, index);
        let _ = write!(out, "\"{prefix}{suffix}\"");
    }
    out.push_str("]}");
}

/// Detach after accounting for ready images. The dirty ledger keeps the next arm out
/// until the poller finishes publishing this sequence.
pub(super) unsafe fn take_bundle(owner: &Capture) -> bool {
    let publish = unsafe { can_publish(owner) };
    unsafe { settle_tail(owner, publish) };
    if !publish {
        return false;
    }
    let metadata = unsafe { &mut *owner.metadata.get() };
    let recordings = unsafe { &mut *owner.recordings.get() };
    unsafe { &mut *owner.ready.get() }.bundle = Some(Snapshot {
        progress: unsafe { &*owner.ledger.get() }.progress(),
        presentation: std::mem::take(&mut metadata.presentation),
        temporal: std::mem::take(&mut metadata.temporal),
        draws: std::mem::take(&mut recordings.draws),
        unobserved_hook_roles: super::hooks::unobserved_roles(),
    });
    true
}

#[cfg(not(test))]
pub(super) unsafe fn publish_bundle(owner: &Capture, bundle: &Bundle) -> Published {
    let snapshot = unsafe { &mut *owner.ready.get() }
        .bundle
        .take()
        .expect("detached bundle");
    Published {
        sequence: snapshot.progress.sequence,
        saved: unsafe { write_bundle(&snapshot, bundle) },
        inputs: snapshot.temporal.len(),
        draws: snapshot.draws.len(),
        written: snapshot.progress.written,
    }
}

#[cfg(not(test))]
unsafe fn write_bundle(snapshot: &Snapshot, bundle: &Bundle) -> bool {
    let directory =
        output_directory(unsafe { slice::from_raw_parts(bundle.directory, bundle.directory_len) });
    let mut write_failed = snapshot.progress.write_failed != 0;
    for input in &snapshot.temporal {
        if let Some(camera) = &input.camera
            && !write_camera(&directory, bundle.pid, input.id, camera)
        {
            write_failed = true;
        }
    }

    let mut out = String::new();
    let progress = &snapshot.progress;
    let presentation = &snapshot.presentation;
    let descriptor = presentation.descriptor;
    let color_space = presentation.details.color_space;
    let hdr = presentation.details.hdr.as_ref();
    let _ = write!(
        out,
        concat!(
            "{{\"version\":1,\"sequence\":{},\"module_base\":\"0x{:x}\",\"draw_count\":{},",
            "\"draw_overflow\":{},\"capture_timeout\":{},\"write_failed\":{},\"readbacks_written\":{},",
            "\"readbacks_unavailable\":{},\"unobserved_hook_roles\":{},",
            "\"presentation\":{{\"format\":{},\"width\":{},\"height\":{},\"color_space\":{},\"color_space_set\":{},",
            "\"hdr_metadata_type\":{},\"hdr_metadata_hex\":\""
        ),
        progress.sequence,
        bundle.base,
        progress.draws,
        progress.draw_overflow != 0,
        progress.timed_out != 0,
        write_failed,
        progress.written,
        progress.readbacks_unavailable,
        snapshot.unobserved_hook_roles,
        descriptor.format,
        descriptor.width,
        descriptor.height,
        color_space.unwrap_or_default(),
        color_space.is_some(),
        hdr.map_or(0, |hdr| hdr.kind),
    );
    if let Some(hdr) = hdr {
        for byte in hdr.bytes() {
            let _ = write!(out, "{byte:02x}");
        }
    }
    let _ = write!(
        out,
        "\"}},\"requested_input_frames\":{},\"temporal\":[",
        progress.requested_input_frames,
    );
    for (index, input) in snapshot.temporal.iter().enumerate() {
        separator(&mut out, index);
        write_temporal(&mut out, input);
    }
    out.push_str("],\"draws\":[");
    for (index, draw) in snapshot.draws.iter().enumerate() {
        separator(&mut out, index);
        write_draw(&mut out, draw);
    }
    out.push_str("]}\n");

    let mut path = directory.join(format!("bundle-{}-{:03}", bundle.pid, progress.sequence));
    path.set_extension("json");
    let temporary = path.with_extension("json.tmp");
    // Readers treat the manifest's appearance as the publication point.
    if fs::write(&temporary, out)
        .and_then(|()| fs::rename(&temporary, &path))
        .is_ok()
    {
        return true;
    }
    let _ = fs::remove_file(temporary);
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_state::{arm, can_arm, request, timeout};
    use std::ptr::null_mut;

    #[test]
    fn detaching_metadata_holds_admission_until_the_matching_sequence_finishes() {
        let owner = Capture::new();
        request(&owner);
        unsafe {
            arm(&owner, 0, null_mut(), &Default::default());
            (*owner.recordings.get()).draws.push(Draw::default());
            assert!(timeout(&owner, 5001));
            request(&owner);
            assert!(take_bundle(&owner));
            assert!((*owner.recordings.get()).draws.is_empty());
            let snapshot = (*owner.ready.get()).bundle.take().unwrap();
            assert_eq!(snapshot.draws.len(), 1);
            assert!(!can_arm(&owner));
            super::super::mark_published(&owner, snapshot.progress.sequence + 1);
            assert!(!can_arm(&owner));
            super::super::mark_published(&owner, snapshot.progress.sequence);
            assert!(can_arm(&owner));
        }
    }
}
