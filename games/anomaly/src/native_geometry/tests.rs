// SPDX-License-Identifier: MIT
use super::*;

#[derive(Default)]
struct PaletteRead {
    batches: Vec<(u32, u32)>,
    fail_at: Option<u32>,
}

unsafe extern "C" fn read_bones(
    reader: usize,
    first: u32,
    count: u32,
    out: *mut NativeMatrix,
) -> bool {
    // The test lends an exclusive reader and write-only scratch span for this call.
    let reader = unsafe { &mut *ptr::with_exposed_provenance_mut::<PaletteRead>(reader) };
    reader.batches.push((first, count));
    if reader.fail_at == Some(first) {
        return false;
    }
    for index in 0..count {
        let mut rows = ENGINE_IDENTITY;
        rows[12] = (first + index) as f32;
        unsafe { out.add(index as usize).write(rows) };
    }
    true
}

#[test]
fn max_palette_uses_sixteen_batches_in_engine_order() {
    let mut reader = PaletteRead::default();
    let mut output = vec![[0.; 16]; MAX_CAPTURE_BONES as usize];
    let instances = ptr::from_mut(&mut reader).expose_provenance();
    assert!(write_palette(read_bones, instances, &mut output));
    assert_eq!(
        reader.batches,
        (0..MAX_CAPTURE_BONES)
            .step_by(BONE_BATCH)
            .map(|first| (first, BONE_BATCH as u32))
            .collect::<Vec<_>>()
    );
    for (index, matrix) in output.iter().enumerate() {
        assert_eq!(matrix[3], index as f64);
    }
}

#[test]
fn unreadable_later_batch_stops_the_palette() {
    let mut reader = PaletteRead {
        fail_at: Some(BONE_BATCH as u32),
        ..Default::default()
    };
    let mut output = [[0.; 16]; BONE_BATCH * 2 + 1];
    let instances = ptr::from_mut(&mut reader).expose_provenance();
    assert!(!write_palette(read_bones, instances, &mut output));
    assert_eq!(reader.batches, [(0, 64), (64, 64)]);
}
