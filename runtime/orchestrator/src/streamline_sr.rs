// SPDX-License-Identifier: MIT
use crate::render_abi::{rsf_dlss_frame as Frame11, rsf_sr_frame as Frame12};
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::c_void;
use std::ptr::null_mut;

#[cfg(not(test))]
mod native;

fn luid_bits(luid: windows::Win32::Foundation::LUID) -> u64 {
    u64::from(luid.LowPart) | (u64::from(luid.HighPart as u32) << 32)
}

pub(super) unsafe fn d3d12_luid(device: Handle) -> u64 {
    use windows::Win32::Graphics::Direct3D12::ID3D12Device;
    luid_bits(unsafe { crate::d3d11_d3d12::borrow::<ID3D12Device>(&device).GetAdapterLuid() })
}

type Handle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Options {
    pub(super) mode: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) hdr: u32,
    pub(super) auto_exposure: u32,
    pub(super) alpha: u32,
    pub(super) preset: u32,
}

pub(super) fn preset_valid(preset: u32) -> bool {
    preset <= 13 && (crate::dlss_pipeline_abi::RSF_DLSS_PRESET_MASK & (1 << preset)) != 0
}

pub(super) fn dlss_mode(quality: u32) -> u32 {
    // DLAA, MaxQuality, Balanced, MaxPerformance, UltraPerformance, Off.
    [6, 3, 2, 1, 4].get(quality as usize).copied().unwrap_or(0)
}

const MAX_TAGS: usize = 9;
const ONLY_VALID_NOW: u32 = 0;
const NON_PIXEL_SHADER_RESOURCE: u32 = 0x40;
const UNORDERED_ACCESS: u32 = 0x08;

// Streamline buffer types checked against the native SDK headers.
const COLOR_IN: u32 = 3;
const COLOR_OUT: u32 = 4;
const DEPTH: u32 = 0;
const MOTION: u32 = 1;
const EXPOSURE: u32 = 13;
const TRANSPARENCY_HINT: u32 = 27;
const BIAS_CURRENT_COLOR: u32 = 29;
const COLOR_BEFORE_TRANSPARENCY: u32 = 40;
const TRANSPARENCY_LAYER: u32 = 51;

#[repr(C)]
#[derive(Clone, Copy)]
struct Tag {
    texture: Handle,
    kind: u32,
    state: u32,
    size: [u32; 2],
    has_extent: u32,
}

impl Tag {
    const EMPTY: Self = Self::new(null_mut(), 0, 0, [0, 0]);

    const fn new(texture: Handle, kind: u32, state: u32, size: [u32; 2]) -> Self {
        Self {
            texture,
            kind,
            state,
            size,
            has_extent: 1,
        }
    }
}

#[repr(C)]
pub(super) struct TagBatch {
    count: u32,
    lifecycle: u32,
    entries: [Tag; MAX_TAGS],
}

const _: () = {
    assert!(size_of::<Tag>() == 32);
    assert!(size_of::<TagBatch>() == 296);
    assert!(std::mem::offset_of!(TagBatch, entries) == 8);
};

impl TagBatch {
    pub(super) fn d3d11(frame: &Frame11, shared: bool) -> Self {
        let render = [frame.render_width, frame.render_height];
        let output = [frame.output_width, frame.output_height];
        let (read, write) = if shared {
            (NON_PIXEL_SHADER_RESOURCE, UNORDERED_ACCESS)
        } else {
            (0, 0)
        };
        let mut batch = Self {
            count: 4,
            // The engine may reuse these scene resources later in the same frame.
            lifecycle: ONLY_VALID_NOW,
            entries: [Tag::EMPTY; MAX_TAGS],
        };
        batch.entries[..4].copy_from_slice(&[
            Tag::new(frame.color_in, COLOR_IN, read, render),
            Tag::new(frame.color_out, COLOR_OUT, write, output),
            Tag::new(frame.depth, DEPTH, read, render),
            Tag::new(frame.motion, MOTION, read, render),
        ]);
        for tag in [
            Tag::new(frame.exposure, EXPOSURE, read, [1, 1]),
            Tag::new(frame.transparency_hint, TRANSPARENCY_HINT, read, render),
            Tag::new(frame.bias_current_color, BIAS_CURRENT_COLOR, read, render),
            Tag::new(
                frame.color_before_transparency,
                COLOR_BEFORE_TRANSPARENCY,
                read,
                render,
            ),
            Tag::new(frame.transparency_layer, TRANSPARENCY_LAYER, read, render),
        ] {
            if !tag.texture.is_null() {
                batch.entries[batch.count as usize] = tag;
                batch.count += 1;
            }
        }
        batch
    }
    pub(super) fn d3d12(frame: &Frame12, record: &Record) -> Self {
        let render = [record.render_width, record.render_height];
        let output = [record.output_width, record.output_height];
        let read = NON_PIXEL_SHADER_RESOURCE;
        let mut exposure = Tag::new(frame.exposure.resource, EXPOSURE, read, [0, 0]);
        // Native D3D12 uses the full exposure resource with no explicit extent.
        exposure.has_extent = 0;
        let mut entries = [Tag::EMPTY; MAX_TAGS];
        entries[..5].copy_from_slice(&[
            Tag::new(frame.color.resource, COLOR_IN, read, render),
            Tag::new(frame.depth.resource, DEPTH, read, render),
            Tag::new(frame.motion.resource, MOTION, read, render),
            Tag::new(frame.output.resource, COLOR_OUT, UNORDERED_ACCESS, output),
            exposure,
        ]);
        Self {
            count: 4 + u32::from(!frame.exposure.resource.is_null()),
            lifecycle: ONLY_VALID_NOW,
            entries,
        }
    }
}

#[cfg(test)]
#[test]
fn native_d3d12_keeps_output_after_motion_and_uses_the_full_exposure_resource() {
    let mut frame: Frame12 = unsafe { std::mem::zeroed() };
    let mut record: Record = unsafe { std::mem::zeroed() };
    record.render_width = 640;
    record.render_height = 360;
    record.output_width = 1280;
    record.output_height = 720;
    assert_eq!(TagBatch::d3d12(&frame, &record).count, 4);
    frame.exposure.resource = std::ptr::without_provenance_mut(1);
    let tags = TagBatch::d3d12(&frame, &record);
    assert_eq!(tags.count, 5);
    assert_eq!(
        tags.entries[..5]
            .iter()
            .map(|tag| tag.kind)
            .collect::<Vec<_>>(),
        [3, 0, 1, 4, 13]
    );
    assert_eq!(
        (tags.entries[3].size, tags.entries[3].state),
        ([1280, 720], 8)
    );
    assert_eq!(
        (tags.entries[4].has_extent, tags.entries[4].state),
        (0, 0x40)
    );
}

#[cfg(test)]
#[test]
fn optional_tags_keep_order_extents_and_shared_states() {
    fn texture(id: usize) -> Handle {
        std::ptr::without_provenance_mut(id)
    }

    // Only the tag inputs are used. The fake handles are never dereferenced.
    let mut frame: Frame11 = unsafe { std::mem::zeroed() };
    frame.color_in = texture(1);
    frame.color_out = texture(2);
    frame.depth = texture(3);
    frame.motion = texture(4);
    frame.bias_current_color = texture(5);
    frame.transparency_layer = texture(6);
    frame.render_width = 640;
    frame.render_height = 360;
    frame.output_width = 1280;
    frame.output_height = 720;

    let owned = TagBatch::d3d11(&frame, false);
    assert_eq!((owned.count, owned.lifecycle), (6, 0));
    assert!(owned.entries.iter().all(|tag| tag.state == 0));
    assert_eq!(owned.entries[4].kind, 29);
    assert_eq!(owned.entries[5].kind, 51);

    frame.exposure = texture(7);
    frame.transparency_hint = texture(8);
    frame.color_before_transparency = texture(9);
    let shared = TagBatch::d3d11(&frame, true);
    assert_eq!((shared.count, shared.lifecycle), (9, 0));
    assert_eq!(
        shared.entries.map(|tag| tag.kind),
        [3, 4, 0, 1, 13, 27, 29, 40, 51]
    );
    assert_eq!(shared.entries[1].state, 8);
    assert_eq!(shared.entries[1].size, [1280, 720]);
    assert_eq!(shared.entries[4].size, [1, 1]);
    for index in [0, 2, 3, 5, 6, 7, 8] {
        assert_eq!(shared.entries[index].state, 0x40);
        assert_eq!(shared.entries[index].size, [640, 360]);
    }
    assert_eq!(shared.entries[4].state, 0x40);
}
