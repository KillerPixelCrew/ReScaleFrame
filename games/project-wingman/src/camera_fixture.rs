// SPDX-License-Identifier: MIT
use super::{read_camera, read_jitter};
use crate::windows::rsf_wingman_copy as copy;
use rsf_game_sys::rsf_game_render_pass;

fn store<T: Copy>(bytes: &mut [u8], offset: usize, value: T) {
    let dest = &mut bytes[offset..offset + size_of::<T>()];
    unsafe { dest.as_mut_ptr().cast::<T>().write_unaligned(value) };
}

fn sample(view: &[u8], pass: &mut rsf_game_render_pass) -> bool {
    let view = view.as_ptr() as usize;
    read_camera(view, pass, copy, read_jitter(view, copy))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_camera_fixture() -> bool {
    let mut view = [0_u8; 0x5700];
    let mut family = [0_u8; 0xd0];
    store(&mut view, 0, family.as_ptr() as usize);
    let frame = 37_u32;
    store(&mut family, 0x5c, frame);
    store(&mut family, 0x54, 1.0_f32 / 60.0);
    let identity: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let projection: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 10.0, 0.0,
    ];
    for offset in [0x2c0, 0x5010] {
        store(&mut view, offset, projection);
    }
    for offset in [0x340, 0x380, 0x480, 0x51d0] {
        store(&mut view, offset, identity);
    }
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.render_rect = [0, 0, 100, 80];
    pass.output_rect = [0, 0, 200, 160];
    let close = |a: f32, b: f32| (a - b).abs() < 0.0001;
    if !sample(&view, &mut pass)
        || pass.native_frame != u64::from(frame)
        || !close(pass.camera.near_plane, 10.0)
        || !close(pass.camera.vertical_fov_radians, 2.0 * 0.5_f32.atan())
        || !pass
            .camera
            .clip_to_previous_clip
            .iter()
            .zip(identity)
            .all(|(&a, b)| close(a, b))
    {
        return false;
    }

    // Reprojection starts in translated coordinates. At this near plane, one unit is 0.1 NDC.
    store(&mut view, 0x600, [-1.0_f32, 0.0, 0.0]);
    if !sample(&view, &mut pass) || !close(pass.camera.clip_to_previous_clip[8], 0.1) {
        return false;
    }
    pass.render_rect[0] = 101;
    if sample(&view, &mut pass) || pass.camera_valid != 0 {
        return false;
    }
    pass.render_rect[0] = 0;
    store(&mut family, 0x5c, u32::MAX);
    if sample(&view, &mut pass) || pass.camera_valid != 0 {
        return false;
    }
    store(&mut family, 0x5c, frame);
    store(&mut view, 0x4fa4, [0.125_f32, f32::NAN]);
    if sample(&view, &mut pass) || pass.camera_valid != 0 {
        return false;
    }
    store(&mut view, 0x4fa4, [0.125_f32, -0.25]);
    if !sample(&view, &mut pass) || !close(pass.camera.jitter_pixels[1], -0.25) {
        return false;
    }
    store(&mut family, 0x54, 0.0_f32);
    if sample(&view, &mut pass) || pass.camera_valid != 0 {
        return false;
    }
    store(&mut view, 0, 1_usize);
    !sample(&view, &mut pass) && pass.camera_valid == 0
}

#[test]
fn decodes_guarded_camera_record() {
    assert!(rsf_wingman_camera_fixture());
}
