// SPDX-License-Identifier: MIT
#[path = "../games/ac7/src/scene_color_abi.rs"]
mod scene_color_abi;

use scene_color_abi::rsf_frame_tap_target_draw;
use std::ffi::{c_int, c_void};

unsafe extern "C" {
    fn rsf_ac7_translucency_scale(render_percent: u32, target_percent: u32) -> f32;
    fn rsf_ac7_separate_translucency_draw(draw: *const rsf_frame_tap_target_draw) -> c_int;
    fn rsf_preferences_read(path: *const u16, enabled: *mut u32, quality: *mut u32);
    fn rsf_preferences_write(path: *const u16, enabled: u32, quality: u32) -> c_int;
}

unsafe extern "system" {
    fn GetTempPathW(capacity: u32, path: *mut u16) -> u32;
    fn GetTempFileNameW(path: *const u16, prefix: *const u16, unique: u32, file: *mut u16) -> u32;
    fn WritePrivateProfileStringW(
        section: *const u16,
        key: *const u16,
        value: *const u16,
        path: *const u16,
    ) -> c_int;
    fn DeleteFileW(path: *const u16) -> c_int;
}

fn check_geometry() {
    for percent in [100, 50, 67, 58, 34, 100, 50] {
        let scale = unsafe { rsf_ac7_translucency_scale(percent, 50) };
        let native_scale = unsafe { rsf_ac7_translucency_scale(percent, 100) };
        for output in [3840.0, 2160.0] {
            let scene = output * percent as f32 / 100.0;
            assert!((scene * scale - output * 0.5).abs() < 0.001);
            assert!((scene * native_scale - output).abs() < 0.001);
        }
    }
    assert_eq!(unsafe { rsf_ac7_translucency_scale(0, 50) }, 0.5);
    assert_eq!(unsafe { rsf_ac7_translucency_scale(50, 0) }, 1.0);
    let mut draw = rsf_frame_tap_target_draw {
        render_target: std::ptr::dangling_mut::<c_void>(),
        depth_bound: 1,
        target_count: 1,
        target_samples: 1,
        target_format: 10, // DXGI_FORMAT_R16G16B16A16_FLOAT
        ..Default::default()
    };
    assert_ne!(unsafe { rsf_ac7_separate_translucency_draw(&draw) }, 0);
    draw.depth_bound = 0;
    assert_eq!(unsafe { rsf_ac7_separate_translucency_draw(&draw) }, 0);
    draw.depth_bound = 1;
    draw.target_count = 3;
    assert_eq!(unsafe { rsf_ac7_separate_translucency_draw(&draw) }, 0);
}

const MAX_PATH: usize = 260;
struct TemporaryFile([u16; MAX_PATH]);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        unsafe { DeleteFileW(self.0.as_ptr()) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn check_preferences() {
    let mut directory = [0; MAX_PATH];
    let mut file = [0; MAX_PATH];
    let length = unsafe { GetTempPathW(MAX_PATH as u32, directory.as_mut_ptr()) } as usize;
    assert!((1..MAX_PATH).contains(&length));
    assert_ne!(
        unsafe {
            GetTempFileNameW(
                directory.as_ptr(),
                wide("rsf").as_ptr(),
                0,
                file.as_mut_ptr(),
            )
        },
        0
    );
    let file = TemporaryFile(file);
    let path = file.0.as_ptr();
    let (mut enabled, mut quality) = (1, 3);
    unsafe { rsf_preferences_read(path, &mut enabled, &mut quality) };
    assert_eq!((enabled, quality), (1, 3));
    for preset in 0..=4 {
        assert_ne!(
            unsafe { rsf_preferences_write(path, preset % 2, preset) },
            0
        );
        (enabled, quality) = (9, 9);
        unsafe { rsf_preferences_read(path, &mut enabled, &mut quality) };
        assert_eq!((enabled, quality), (preset % 2, preset));
    }
    assert_ne!(
        unsafe {
            WritePrivateProfileStringW(
                wide("Rendering").as_ptr(),
                wide("Quality").as_ptr(),
                wide("99").as_ptr(),
                path,
            )
        },
        0
    );
    quality = 3;
    unsafe { rsf_preferences_read(path, &mut enabled, &mut quality) };
    assert_eq!(quality, 3);
    assert_eq!(unsafe { rsf_preferences_write(path, 1, 5) }, 0);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_session_fixture() -> c_int {
    check_geometry();
    check_preferences();
    0
}
