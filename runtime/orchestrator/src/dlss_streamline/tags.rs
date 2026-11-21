// SPDX-License-Identifier: MIT
use super::{Frame, Handle};
use std::ptr::null_mut;

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
}

impl Tag {
    const EMPTY: Self = Self::new(null_mut(), 0, 0, [0, 0]);

    const fn new(texture: Handle, kind: u32, state: u32, size: [u32; 2]) -> Self {
        Self {
            texture,
            kind,
            state,
            size,
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
    assert!(size_of::<Tag>() == 24);
    assert!(size_of::<TagBatch>() == 224);
    assert!(std::mem::offset_of!(TagBatch, entries) == 8);
};

impl TagBatch {
    pub(super) fn new(frame: &Frame, shared: bool) -> Self {
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
}

#[cfg(test)]
#[test]
fn optional_tags_keep_order_extents_and_shared_states() {
    fn texture(id: usize) -> Handle {
        std::ptr::without_provenance_mut(id)
    }

    // Only the tag inputs are used. The fake handles are never dereferenced.
    let mut frame: Frame = unsafe { std::mem::zeroed() };
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

    let owned = TagBatch::new(&frame, false);
    assert_eq!((owned.count, owned.lifecycle), (6, 0));
    assert!(owned.entries.iter().all(|tag| tag.state == 0));
    assert_eq!(owned.entries[4].kind, 29);
    assert_eq!(owned.entries[5].kind, 51);

    frame.exposure = texture(7);
    frame.transparency_hint = texture(8);
    frame.color_before_transparency = texture(9);
    let shared = TagBatch::new(&frame, true);
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
