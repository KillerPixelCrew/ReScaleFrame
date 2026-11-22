// SPDX-License-Identifier: MIT
use super::Fixture;
use crate::camera::{CameraInputs, CameraResult, decode};
use rsf_game_sys::rsf_camera_frame;
use std::ffi::OsString;
use std::io::{self, Write};

fn print_matrix(out: &mut impl Write, name: &str, values: &[f32; 16]) -> io::Result<()> {
    write!(out, "\"{name}\":[{}", values[0])?;
    for value in &values[1..] {
        write!(out, ",{value}")?;
    }
    write!(out, "]")
}

fn capture(bytes: &[u8], arguments: &[OsString]) -> Option<()> {
    let argument = |index: usize| arguments[index].to_str();
    let integer = |index| argument(index)?.parse::<u32>().ok();
    let real = |index| argument(index)?.parse::<f32>().ok();
    let input = CameraInputs {
        render_width: integer(0)?,
        render_height: integer(1)?,
        output_width: integer(2)?,
        output_height: integer(3)?,
        jitter: [real(4)?, real(5)?],
        previous_jitter: [real(6)?, real(7)?],
        reset: argument(8)?.parse::<i32>().ok()? != 0,
        ..CameraInputs::default()
    };
    let mut result = CameraResult::default();
    if !decode(bytes, &input, &mut result) {
        return None;
    }
    let camera = result.camera;
    let mut out = io::stdout().lock();
    write!(
        out,
        "{{\"jitter_matches\":{},\"near\":{},\"far\":{},\"fov\":{},",
        result.projection_jitter_matches,
        camera.near_plane,
        camera.far_plane,
        camera.vertical_fov_radians
    )
    .ok()?;
    print_matrix(&mut out, "projection", &camera.view_to_clip).ok()?;
    write!(out, ",").ok()?;
    print_matrix(&mut out, "inverse_projection", &camera.clip_to_view).ok()?;
    write!(out, ",").ok()?;
    print_matrix(&mut out, "clip_to_previous", &camera.clip_to_previous_clip).ok()?;
    writeln!(out, "}}").ok()
}

fn check_fixture(bytes: &[u8]) -> bool {
    let Some(fixture) = Fixture::parse(bytes) else {
        return false;
    };
    let mut result = CameraResult::default();
    decode(fixture.view, &fixture.input, &mut result)
        && result.camera.struct_size as usize == size_of::<rsf_camera_frame>()
        && result.projection_jitter_matches == fixture.jitter_matches
        && (result.camera.vertical_fov_radians - fixture.fov).abs() < 1e-5
}

fn run() -> Option<bool> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next()?;
    let arguments: Vec<_> = args.collect();
    match arguments.len() {
        0 => Some(check_fixture(&std::fs::read(path).ok()?)),
        9 => capture(&std::fs::read(path).ok()?, &arguments).map(|()| true),
        _ => None,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_camera_fixture() -> i32 {
    i32::from(run() != Some(true))
}
