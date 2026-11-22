// SPDX-License-Identifier: MIT
//! Optional probe for the flat capture directory. Synthetic cases live in the AC7 crate.
#[path = "../games/ac7/src/view_abi.rs"]
mod view_abi;

use std::ffi::{c_char, c_int, c_void};
use view_abi::{
    RSF_AC7_VIEW_ABI_VERSION, RSF_AC7_VIEW_BUFFER_BYTES, rsf_ac7_view, rsf_ac7_view_result,
};

unsafe extern "C" {
    fn rsf_ac7_view_capture_read(directory: *const c_char, index: c_int, data: *mut u8) -> c_int;
    fn rsf_ac7_view_read(
        data: *const c_void,
        size: u32,
        abi: u32,
        view: *mut rsf_ac7_view,
    ) -> rsf_ac7_view_result;
}

fn check_inverse(view: &rsf_ac7_view) -> bool {
    let mut passed = true;
    for row in 0..4 {
        for column in 0..4 {
            let sum: f64 = (0..4)
                .map(|k| {
                    f64::from(view.clip_to_prev_clip[row * 4 + k])
                        * f64::from(view.prev_clip_to_clip[k * 4 + column])
                })
                .sum();
            let expected = if row == column { 1.0 } else { 0.0 };
            if (sum - expected).abs() > 1e-4 {
                eprintln!("prev_clip_to_clip is not the inverse at [{row}][{column}]: {sum:.6}");
                passed = false;
            }
        }
    }
    passed
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_view_probe(directory: *const c_char) -> c_int {
    let mut found = 0;
    let mut accepted = 0;
    let mut full_targets = 0;
    let mut passed = true;
    for index in 0..100 {
        let mut data = [0; RSF_AC7_VIEW_BUFFER_BYTES as usize];
        if unsafe { rsf_ac7_view_capture_read(directory, index, data.as_mut_ptr()) } == 0 {
            continue;
        }
        found += 1;
        let mut view = rsf_ac7_view {
            struct_size: size_of::<rsf_ac7_view>() as u32,
            ..Default::default()
        };
        let result = unsafe {
            rsf_ac7_view_read(
                data.as_ptr().cast(),
                RSF_AC7_VIEW_BUFFER_BYTES,
                RSF_AC7_VIEW_ABI_VERSION,
                &mut view,
            )
        };
        if result != 0 {
            println!("capture{index:02}: refused, result {result}");
            continue;
        }
        accepted += 1;
        full_targets += view.fills_target;
        let [x, y] = view.jitter_pixels;
        let jitter = if view.has_jitter != 0 {
            if !(x.abs() <= 0.5 && y.abs() <= 0.5) {
                eprintln!("A jitter read from the game must stay inside half a pixel.");
                passed = false;
            }
            format!("jitter ({x:+.3}, {y:+.3}) px")
        } else {
            "no jitter".into()
        };
        let coverage = if view.fills_target != 0 {
            "full target"
        } else {
            "partial target"
        };
        println!(
            "capture{index:02}: {}x{} in {}x{}, fov {:.1} deg, aspect {:.4}, near {:.2}, {coverage}, {jitter}",
            view.view_width,
            view.view_height,
            view.buffer_width,
            view.buffer_height,
            f64::from(view.vertical_fov) * 57.29578,
            view.aspect_ratio,
            view.near_plane,
        );
        passed &= check_inverse(&view);
    }
    println!(
        "\n{found} buffers read, {accepted} accepted as perspective views, {full_targets} of those full-target views"
    );
    if found == 0 { 2 } else { c_int::from(!passed) }
}
