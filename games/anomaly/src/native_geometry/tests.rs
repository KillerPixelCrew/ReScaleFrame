// SPDX-License-Identifier: MIT
use super::*;

#[derive(Default)]
struct PaletteRead {
    batches: Vec<(u32, u32)>,
    fail_at: Option<u32>,
}

const INSTANCES: usize = 0x1000;
thread_local! {
    static READER: std::cell::RefCell<PaletteRead> = Default::default();
}

unsafe extern "C" fn read_bones(
    start: usize,
    stride: usize,
    count: u32,
    out: *mut NativeMatrix,
) -> bool {
    assert_eq!(stride, 168);
    let offset = start - INSTANCES - 64;
    assert_eq!(offset % stride, 0);
    let first = (offset / stride) as u32;
    let failed = READER.with_borrow_mut(|reader| {
        reader.batches.push((first, count));
        reader.fail_at == Some(first)
    });
    if failed {
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
    READER.set(Default::default());
    let mut output = vec![[0.; 16]; MAX_CAPTURE_BONES as usize];
    assert!(write_palette(read_bones, INSTANCES, &mut output));
    assert_eq!(
        READER.with_borrow(|reader| reader.batches.clone()),
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
    READER.set(PaletteRead {
        fail_at: Some(BONE_BATCH as u32),
        ..Default::default()
    });
    let mut output = [[0.; 16]; BONE_BATCH * 2 + 1];
    assert!(!write_palette(read_bones, INSTANCES, &mut output));
    READER.with_borrow(|reader| assert_eq!(reader.batches, [(0, 64), (64, 64)]));
}
