// SPDX-License-Identifier: MIT
//! Captured View layout used by the native graph fixture's synthetic 2x2 input.
const VIEW_BYTES: usize = 4096;

/// Writes the fixture directly into the caller's float-aligned storage.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_graph_fixture_view(output: *mut f32, size: usize) -> bool {
    if output.is_null() || size != VIEW_BYTES {
        return false;
    }
    let output = unsafe { std::slice::from_raw_parts_mut(output, VIEW_BYTES / size_of::<f32>()) };
    output.fill(0.0);
    let mut put = |offset: usize, values: &[f32]| {
        let first = offset / size_of::<f32>();
        output[first..first + values.len()].copy_from_slice(values);
    };
    // Rotated camera basis in Unreal's order, where Z is forward.
    put(
        0x0c0,
        &[
            0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    );
    // Reversed-Z infinite projection with jitter, followed by its full inverse.
    put(
        0x180,
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.125, -0.25, 0.0, 1.0, 0.0, 0.0, 0.1, 0.0,
        ],
    );
    put(
        0x1c0,
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0, -0.125, 0.25, 1.0, 0.0,
        ],
    );
    put(0x300, &[1.0, 0.0, 0.0]);
    put(0x310, &[0.0, 0.0, 1.0]);
    put(0x320, &[0.0, 1.0, 0.0]);
    let position = [260_968.45, 107_399.9, 21_228.2];
    put(0x370, &position);
    put(0x3a0, &position.map(|value| -value));
    put(
        0x6e0,
        &[
            1.0, -0.00009, 0.0, 0.0, 0.00003, 1.0, 0.0, 0.0, -0.02808, -0.20304, 1.0, 17.49332,
            -0.00001, 0.00003, 0.0, 1.0,
        ],
    );
    put(0x720, &[0.125, -0.25]);
    for offset in [0x7f0, 0x800] {
        put(offset, &[2.0, 2.0, 0.5, 0.5]);
    }
    true
}
