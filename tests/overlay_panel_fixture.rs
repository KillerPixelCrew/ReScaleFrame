// SPDX-License-Identifier: MIT
// Deterministic triangle and one atlas upload for the native overlay host fixtures.

#[allow(dead_code)]
#[path = "../ui/overlay/src/abi.rs"]
mod abi;

use abi::{
    RSF_OVERLAY_ABI_VERSION, RSF_OVERLAY_OK, RsfOverlayDrawCall, RsfOverlayDrawData,
    RsfOverlayInput, RsfOverlayIntent, RsfOverlayResult, RsfOverlayStats, RsfOverlayTextureUpdate,
    RsfOverlayVertex,
};
use std::ptr::null_mut;

struct Panel {
    uploaded: bool,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_create(abi: u32) -> *mut Panel {
    if abi != RSF_OVERLAY_ABI_VERSION {
        return null_mut();
    }
    Box::into_raw(Box::new(Panel { uploaded: false }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_destroy(panel: *mut Panel) {
    if !panel.is_null() {
        // SAFETY: the host destroys each handle from create once.
        drop(unsafe { Box::from_raw(panel) });
    }
}

const GREEN: u32 = 0xff00ff00;
static VERTICES: [RsfOverlayVertex; 3] = [
    RsfOverlayVertex {
        x: 8.0,
        y: 8.0,
        u: 0.0,
        v: 0.0,
        color: GREEN,
    },
    RsfOverlayVertex {
        x: 72.0,
        y: 8.0,
        u: 1.0,
        v: 0.0,
        color: GREEN,
    },
    RsfOverlayVertex {
        x: 8.0,
        y: 72.0,
        u: 0.0,
        v: 1.0,
        color: GREEN,
    },
];
static INDICES: [u32; 3] = [0, 1, 2];
static DRAW: RsfOverlayDrawCall = RsfOverlayDrawCall {
    index_offset: 0,
    index_count: 3,
    vertex_offset: 0,
    clip_x: 0,
    clip_y: 0,
    clip_width: 80,
    clip_height: 80,
    texture_id: 0,
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_frame(
    _: *mut Panel,
    _: *const RsfOverlayInput,
    _: *const RsfOverlayStats,
    data: *mut RsfOverlayDrawData,
    _: *mut RsfOverlayIntent,
) -> RsfOverlayResult {
    // SAFETY: both fixture hosts supply room for the full draw record.
    unsafe {
        data.write(RsfOverlayDrawData {
            struct_size: size_of::<RsfOverlayDrawData>() as u32,
            vertices: VERTICES.as_ptr(),
            vertex_count: VERTICES.len() as u32,
            indices: INDICES.as_ptr(),
            index_count: INDICES.len() as u32,
            calls: &DRAW,
            call_count: 1,
        });
    }
    RSF_OVERLAY_OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_texture_updates(
    panel: *mut Panel,
    updates: *mut RsfOverlayTextureUpdate,
    capacity: u32,
) -> u32 {
    // SAFETY: calls for a valid fixture panel are serialized by its host.
    let panel = unsafe { &mut *panel };
    if panel.uploaded || capacity == 0 {
        return 0;
    }
    static WHITE: [u8; 4] = [255; 4];
    // SAFETY: the host supplies an update buffer with at least one slot.
    unsafe {
        updates.write(RsfOverlayTextureUpdate {
            id: 0,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            pixels: WHITE.as_ptr(),
            is_whole_texture: 1,
        });
    }
    panel.uploaded = true;
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_textures_to_free(_: *mut Panel, _: *mut u64, _: u32) -> u32 {
    0
}
