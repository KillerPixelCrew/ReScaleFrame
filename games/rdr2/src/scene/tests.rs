// SPDX-License-Identifier: MIT
use super::*;

fn state() -> Box<State> {
    Box::new(State {
        swapchain: 0,
        markers: [Markers { frame: 0, mask: 0 }; 128],
        scenes: [Scene::EMPTY; 64],
    })
}

fn pass(frame: u64, source: u64) -> rsf_game_render_pass {
    let mut pass = Scene::EMPTY.pass;
    pass.native_frame = frame;
    pass.source_frame_id = source;
    pass.camera_valid = 1;
    pass.color_input = 0x10 as *mut c_void;
    pass.depth = 0x20 as *mut c_void;
    pass
}

#[test]
fn source_requires_simulation_input_and_submit_markers() {
    let frame = 300;
    let mut markers = Markers { frame, mask: 0 };
    for kind in [0, 1, 2] {
        markers.mask |= 1 << kind;
        assert_eq!(source_of(&markers, frame), 0);
    }
    markers.mask |= 1 << 6;
    assert_eq!(source_of(&markers, frame), frame);
    // A ring slot reused by another frame carries no identity.
    assert_eq!(source_of(&markers, frame + 128), 0);
}

#[test]
fn recorded_scene_keeps_identity_without_resources() {
    let mut state = state();
    record_upscale(&mut state, 0x1234, &pass(70, 70));
    let scene = &state.scenes[70 % 64];
    assert_eq!(scene.commands, 0x1234);
    assert_eq!(scene.pass.native_frame, 70);
    assert_eq!(scene.pass.source_frame_id, 70);
    assert!(scene.pass.color_input.is_null() && scene.pass.depth.is_null());
    assert_eq!((scene.surface, scene.ordinal), (0, 0));
}

#[test]
fn a_second_upscale_view_withdraws_the_scene_identity() {
    let mut state = state();
    record_upscale(&mut state, 0x1234, &pass(70, 70));
    record_upscale(&mut state, 0x5678, &pass(70, 70));
    let scene = &state.scenes[70 % 64];
    assert_eq!(scene.commands, 0);
    assert_eq!(scene.pass.source_frame_id, 0);
    assert_eq!(scene.pass.camera_valid, 0);
    // The next frame in the same slot replaces it.
    record_upscale(&mut state, 0x9abc, &pass(134, 134));
    let scene = &state.scenes[134 % 64];
    assert_eq!((scene.commands, scene.pass.source_frame_id), (0x9abc, 134));
}

#[test]
fn ring_slots_wrap_by_frame() {
    let mut items = [0u64; 64];
    *slot(&mut items, 65) = 9;
    assert_eq!(items[1], 9);
}
