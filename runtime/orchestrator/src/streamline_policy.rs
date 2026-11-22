// SPDX-License-Identifier: MIT

pub(crate) fn engine_type(engine: u32) -> u32 {
    match engine {
        1 | 2 => engine, // Unreal and Unity are recognized by Streamline.
        _ => 0,          // Custom
    }
}

pub(crate) fn camera_forward(
    view_to_world: &[f32; 16],
    view_to_clip: &[f32; 16],
    orthographic_sign: f32,
) -> [f32; 3] {
    let w = view_to_clip[11];
    let sign = if w < 0.0 {
        -1.0
    } else if w > 0.0 {
        1.0
    } else {
        orthographic_sign
    };
    [8, 9, 10].map(|i| sign * view_to_world[i])
}

#[test]
fn perspective_and_orthographic_forward() {
    let mut world = [0.0; 16];
    world[8..11].copy_from_slice(&[1.0, 2.0, 3.0]);
    for (w, fallback, expected) in [
        (-1.0, 1.0, -1.0),
        (1.0, -1.0, 1.0),
        (0.0, -1.0, -1.0),
        (0.0, 1.0, 1.0),
    ] {
        let mut projection = [0.0; 16];
        projection[11] = w;
        assert_eq!(
            camera_forward(&world, &projection, fallback),
            [expected, 2.0 * expected, 3.0 * expected]
        );
    }
}

// Windows-style absolute paths accepted by the fixed-search Streamline loader.
pub(super) unsafe extern "C" fn rsf_streamline_absolute_path(path: *const u16) -> u32 {
    if path.is_null() || unsafe { path.read() } == 0 {
        return 0;
    }
    let first = unsafe { path.read() };
    let second = unsafe { path.add(1).read() };
    u32::from(
        (first == u16::from(b'\\') && second == first)
            || (second == u16::from(b':') && matches!(unsafe { path.add(2).read() }, 47 | 92)),
    )
}

#[test]
fn streamline_loader_requires_an_absolute_windows_path() {
    for (text, absolute) in [
        (r"C:\sdk\sl.interposer.dll", 1),
        ("C:/sdk/sl.interposer.dll", 1),
        (r"\\server\sdk\sl.interposer.dll", 1),
        (r"C:sl.interposer.dll", 0),
        (r"\sl.interposer.dll", 0),
        ("sl.interposer.dll", 0),
        ("C:", 0),
        ("", 0),
    ] {
        let path: Vec<_> = text.encode_utf16().chain([0]).collect();
        assert_eq!(
            unsafe { rsf_streamline_absolute_path(path.as_ptr()) },
            absolute
        );
    }
    assert_eq!(unsafe { rsf_streamline_absolute_path(std::ptr::null()) }, 0);
}

#[test]
fn engine_metadata_only_accepts_known_values() {
    assert_eq!([0, 1, 2, 3, u32::MAX].map(engine_type), [0, 1, 2, 0, 0]);
}
