// SPDX-License-Identifier: MIT
//! SR and final-scene pass records: identity, keys, flags, screen class, driver route, camera
//! scalars and the last recorded SR pass that the final scene repeats.
//!
//! The C++ side keeps every Godot read, the camera matrix math, the previous camera and
//! projection, motion resolve, native texture handles, the Vulkan image descriptions and the
//! driver callback's engine calls. Everything here runs on Godot's rendering server thread.

use core::cell::UnsafeCell;
use core::mem::{align_of, offset_of, size_of};

use crate::host;
use crate::packet_abi::rsf_game_render_pass;
use crate::primary::{self, Admission, DRIVER_VULKAN, History};

use crate::frame_abi::{RSF_GAME_CAMERA_ABI_VERSION, rsf_camera_frame};
use crate::frame_event_abi::{
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR,
};

/// The non-pointer pass fields, private to the module.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Record {
    session_id: u64,
    source_frame_id: u64,
    /// The view, history and viewport key: the primary render target.
    key: u64,
    pub(crate) role: u32,
    flags: u32,
    resource_generation: u32,
    screen: u32,
    /// `primary.h` driver of this route.
    pub(crate) driver: u32,
    /// The SR depth input is the motion pass's R32 copy rather than Godot's depth texture.
    normalized_depth: u32,
    motion_to_uv: [f32; 2],
    motion_camera_included: u32,
    camera_valid: u32,
    previous_clip_to_clip: [f32; 16],
    camera: rsf_camera_frame,
    reset_generation: u64,
}

impl Record {
    pub(crate) fn public_pass(&self) -> rsf_game_render_pass {
        rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            role: self.role,
            session_id: self.session_id,
            source_frame_id: self.source_frame_id,
            native_frame: self.source_frame_id,
            view_key: self.key,
            history_key: self.key,
            viewport_key: self.key,
            flags: self.flags,
            resource_generation: self.resource_generation,
            screen: self.screen,
            motion_to_uv: self.motion_to_uv,
            motion_camera_included: self.motion_camera_included,
            camera_valid: self.camera_valid,
            camera: self.camera,
            previous_clip_to_clip: self.previous_clip_to_clip,
            ..Default::default()
        }
    }
}

/// Engine facts read by the SR hook after admission and valid textures.
#[repr(C)]
pub struct SrFacts {
    history: History,
    source_frame_id: u64,
    render_target: u64,
    output_width: i32,
    output_height: i32,
    near_plane: f32,
    far_plane: f32,
    vertical_fov_radians: f32,
    /// Godot's TAA jitter in its own units, as `taa_jitter` and `prev_taa_jitter`.
    jitter: [f32; 2],
    previous_jitter: [f32; 2],
    screen: u32,
    driver: u32,
    _reserved: u32,
}

const _: () = {
    assert!(size_of::<Record>() == 520 && align_of::<Record>() == 8);
    assert!(offset_of!(Record, role) == 24);
    assert!(offset_of!(Record, driver) == 40);
    assert!(offset_of!(Record, motion_to_uv) == 48);
    assert!(offset_of!(Record, previous_clip_to_clip) == 64);
    assert!(offset_of!(Record, camera) == 128);
    assert!(offset_of!(Record, reset_generation) == 512);
    assert!(size_of::<SrFacts>() == 104 && align_of::<SrFacts>() == 8);
    assert!(offset_of!(SrFacts, source_frame_id) == 40);
    assert!(offset_of!(SrFacts, output_width) == 56);
    assert!(offset_of!(SrFacts, near_plane) == 64);
    assert!(offset_of!(SrFacts, jitter) == 76);
    assert!(offset_of!(SrFacts, previous_jitter) == 84);
    assert!(offset_of!(SrFacts, screen) == 92);
    assert!(offset_of!(SrFacts, driver) == 96);
};

const ZERO_CAMERA: rsf_camera_frame = rsf_camera_frame {
    struct_size: 0,
    abi_version: 0,
    view_to_clip: [0.0; 16],
    clip_to_view: [0.0; 16],
    view_to_world: [0.0; 16],
    world_to_view: [0.0; 16],
    clip_to_previous_clip: [0.0; 16],
    jitter_pixels: [0.0; 2],
    previous_jitter_pixels: [0.0; 2],
    render_width: 0,
    render_height: 0,
    output_width: 0,
    output_height: 0,
    near_plane: 0.0,
    far_plane: 0.0,
    vertical_fov_radians: 0.0,
    depth_inverted: 0,
    motion_jittered: 0,
    frame_time_seconds: 0.0,
};

const ZERO: Record = Record {
    session_id: 0,
    source_frame_id: 0,
    key: 0,
    role: 0,
    flags: 0,
    resource_generation: 0,
    screen: 0,
    driver: 0,
    normalized_depth: 0,
    motion_to_uv: [0.0; 2],
    motion_camera_included: 0,
    camera_valid: 0,
    previous_clip_to_clip: [0.0; 16],
    camera: ZERO_CAMERA,
    reset_generation: 0,
};

/// The last recorded SR pass, as `last_pass` was.
struct Last(UnsafeCell<Record>);

// SAFETY: only the rendering server thread reads or writes it.
unsafe impl Sync for Last {}

static LAST: Last = Last(UnsafeCell::new(ZERO));

/// Rendering server thread: retargeting retires the previous target's receipt.
pub(crate) fn invalidate() {
    // SAFETY: rendering server thread only, see `Last`.
    unsafe { *LAST.0.get() = ZERO };
}

/// Rendering server thread: decides the SR pass identity, reset, screen class, driver route and
/// camera scalars. The C++ then fills the five camera matrices and `previous_clip_to_clip`, with
/// the identity reprojection when `flags` carries the reset.
///
/// # Safety
/// `facts` must point to a valid `rsf_godot_sr_facts` and `out` to writable, aligned storage
/// for one `rsf_godot_pass_record`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_sr_plan(facts: *const SrFacts, out: *mut Record) {
    let facts = unsafe { &*facts };
    let identity = primary::decide(&facts.history);
    let render_width = facts.history.internal_width as u32;
    let render_height = facts.history.internal_height as u32;
    // Godot's jitter in pixels of the render extent: the same single-precision products, in
    // the same order, as the C++ expressions they replace.
    let pixels = |jitter: [f32; 2]| {
        [
            jitter[0] * render_width as f32 * 0.5,
            jitter[1] * render_height as f32 * 0.5,
        ]
    };
    let record = Record {
        session_id: host::rsf_godot_host_session(),
        source_frame_id: facts.source_frame_id,
        key: facts.render_target,
        role: RSF_GAME_RENDER_SR,
        flags: RSF_GAME_RENDER_PRIMARY
            | if identity.reset {
                RSF_GAME_RENDER_RESET
            } else {
                0
            },
        resource_generation: identity.resource_generation,
        screen: facts.screen,
        driver: facts.driver,
        normalized_depth: (facts.driver == DRIVER_VULKAN) as u32,
        motion_to_uv: [-1.0, -1.0],
        motion_camera_included: 1,
        camera_valid: 1,
        previous_clip_to_clip: [0.0; 16],
        reset_generation: identity.reset_generation,
        camera: rsf_camera_frame {
            struct_size: size_of::<rsf_camera_frame>() as u32,
            abi_version: RSF_GAME_CAMERA_ABI_VERSION,
            jitter_pixels: pixels(facts.jitter),
            previous_jitter_pixels: pixels(facts.previous_jitter),
            render_width,
            render_height,
            output_width: facts.output_width as u32,
            output_height: facts.output_height as u32,
            near_plane: facts.near_plane,
            far_plane: facts.far_plane,
            vertical_fov_radians: facts.vertical_fov_radians,
            depth_inverted: 1,
            frame_time_seconds: facts.history.frame_seconds as f32,
            ..ZERO_CAMERA
        },
    };
    unsafe { out.write(record) };
}

/// Rendering server thread: the planned SR pass passed its resolve checks and is recorded. The
/// history state and frame evaluation update first, then the record becomes the last pass.
///
/// # Safety
/// `facts` must be the facts `record` was planned from, and both must point to valid records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_sr_record(facts: *const SrFacts, record: *const Record) {
    let (facts, record) = unsafe { (&*facts, &*record) };
    primary::recorded(
        &facts.history,
        record.resource_generation,
        record.reset_generation,
    );
    // SAFETY: rendering server thread only, see `Last`.
    unsafe { *LAST.0.get() = *record };
}

/// Rendering server thread: admits the final-scene pass and plans it as the last recorded SR
/// pass with the final-scene role. Returns 0 when not admitted.
///
/// # Safety
/// `facts` must point to a valid `rsf_godot_render_admission` and `out` to writable, aligned
/// storage for one `rsf_godot_pass_record`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_final_scene_plan(
    facts: *const Admission,
    out: *mut Record,
) -> u32 {
    let facts = unsafe { &*facts };
    // SAFETY: rendering server thread only, see `Last`.
    let mut record = unsafe { *LAST.0.get() };
    if !primary::final_scene_admitted(
        facts,
        record.source_frame_id,
        record.key,
        record.resource_generation,
    ) {
        return 0;
    }
    record.role = RSF_GAME_RENDER_FINAL_SCENE;
    record.driver = facts.driver;
    unsafe { out.write(record) };
    1
}
