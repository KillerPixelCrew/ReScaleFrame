// SPDX-License-Identifier: MIT
use super::*;
use crate::sr_stage::jitter_sample;
use crate::sr_swap::jitter_projection;
use std::ptr::{copy_nonoverlapping, from_mut};

const LAYOUT: Layout = Layout {
    destination: 0x10,
    load: 0x14,
    buffer_size: 0x58,
    buffer_data: 0x60,
    buffer_changed: 0x68,
    buffer_slots: 14,
    stage_bits: [1, 2, 8, 16, 32, 64],
    stage_shifts: [16, 12, 8, 20, 24, 28],
};

const CONSTANT: usize = 0x20;
const STOCK: Vector = [0.8, -0.5, 0.001, -0.002];
const CORRECTED: Vector = [0.8 + 0.25 * 0.001, -0.5 - 0.125 * -0.002, 0.001, -0.002];

fn buffer_address(stage: usize) -> usize {
    0x80 + 0x80 * stage
}
fn data_address(stage: usize) -> usize {
    0x400 + 0x80 * stage
}
fn data_offset(stage: usize) -> usize {
    16 + stage * 16
}

struct Engine {
    memory: [u8; 0x700],
    selected: [usize; 6],
    gets: usize,
    writes: usize,
    fail_write: Option<usize>,
    fail_restoration: bool,
}

impl Engine {
    fn new() -> Self {
        let mut engine = Self {
            memory: [0; 0x700],
            selected: std::array::from_fn(buffer_address),
            gets: 0,
            writes: 0,
            fail_write: None,
            fail_restoration: false,
        };
        engine.put(CONSTANT + 0x10, 0x7bu32.to_le_bytes());
        for stage in 0..6 {
            engine.put(
                CONSTANT + 0x14 + stage * 4,
                (data_offset(stage) as u16).to_le_bytes(),
            );
            engine.put(CONSTANT + 0x16 + stage * 4, 1u16.to_le_bytes());
            engine.put(buffer_address(stage) + 0x58, 128u32.to_le_bytes());
            engine.put(
                buffer_address(stage) + 0x60,
                data_address(stage).to_le_bytes(),
            );
            for (index, sample) in STOCK.iter().enumerate() {
                engine.put(
                    data_address(stage) + data_offset(stage) + 4 * index,
                    sample.to_le_bytes(),
                );
            }
        }
        engine
    }

    fn put<const N: usize>(&mut self, offset: usize, value: [u8; N]) {
        self.memory[offset..offset + N].copy_from_slice(&value);
    }

    fn value(&self, stage: usize) -> Vector {
        let offset = data_address(stage) + data_offset(stage);
        std::array::from_fn(|i| {
            let start = offset + i * 4;
            f32::from_le_bytes(self.memory[start..start + 4].try_into().unwrap())
        })
    }

    fn correct(&mut self) -> u32 {
        let platform = Platform {
            user: from_mut(self).cast(),
            read,
            write,
            buffer,
        };
        rsf_anomaly_sr_correct_position_constant(&LAYOUT, &platform, CONSTANT, 0.25, -0.125)
    }
}

unsafe extern "C" fn read(
    user: *mut c_void,
    address: usize,
    out: *mut c_void,
    size: usize,
) -> bool {
    let engine = unsafe { &*user.cast::<Engine>() };
    let Some(bytes) = engine.memory.get(address..address.saturating_add(size)) else {
        return false;
    };
    unsafe { copy_nonoverlapping(bytes.as_ptr(), out.cast(), size) };
    true
}

unsafe extern "C" fn write(
    user: *mut c_void,
    address: usize,
    input: *const c_void,
    size: usize,
) -> bool {
    let engine = unsafe { &mut *user.cast::<Engine>() };
    let write = engine.writes;
    engine.writes += 1;
    if engine
        .fail_write
        .is_some_and(|failure| failure == write || (engine.fail_restoration && write > failure))
    {
        return false;
    }
    let Some(bytes) = engine.memory.get_mut(address..address.saturating_add(size)) else {
        return false;
    };
    unsafe { copy_nonoverlapping(input.cast(), bytes.as_mut_ptr(), size) };
    true
}

unsafe extern "C" fn buffer(user: *mut c_void, constant: usize, stage: u32) -> usize {
    let engine = unsafe { &mut *user.cast::<Engine>() };
    assert_eq!(constant, CONSTANT);
    engine.gets += 1;
    engine.selected[stage as usize]
}

#[test]
fn jittered_rays_round_trip_through_the_corrected_position_vector() {
    for [width, height] in [[853, 481], [1286, 724], [1920, 1080]] {
        let w = width as f32;
        let h = height as f32;
        for fov in [0.55f32, 1.1, 1.7] {
            let vertical = -(fov * 0.5).tan();
            let horizontal = -vertical * w / h;
            let projection = [
                1.0 / horizontal,
                0.0,
                0.0,
                0.0,
                0.0,
                -1.0 / vertical,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0001,
                1.0,
                0.0,
                0.0,
                -0.1,
                0.0,
            ];
            for phase in 0..18 {
                let sample = jitter_sample(phase, 18);
                let jittered =
                    jitter_projection(&projection, sample.x, sample.y, width, height).unwrap();
                let stock = [
                    horizontal,
                    vertical,
                    2.0 * horizontal / w,
                    2.0 * vertical / h,
                ];
                let adjusted = corrected(stock, sample.x, sample.y).unwrap();
                assert_eq!(adjusted[2..], stock[2..]);
                for z in [0.5, 5.0, 40.0, 200.0] {
                    let x = z * 0.25;
                    let y = z * -0.15;
                    let px = ((x * jittered[0] + z * jittered[8]) / z + 1.0) * w * 0.5;
                    let py = (1.0 - (y * jittered[5] + z * jittered[9]) / z) * h * 0.5;
                    assert!(
                        (z * (px * adjusted[2] - adjusted[0]) - x).abs() <= 2e-6 * (1.0 + x.abs())
                    );
                    assert!(
                        (z * (py * adjusted[3] - adjusted[1]) - y).abs() <= 2e-6 * (1.0 + y.abs())
                    );
                    let stock_error = z * (px * stock[2] - stock[0]) - x;
                    assert!(
                        (stock_error - z * sample.x * stock[2]).abs() <= 2e-6 * (1.0 + x.abs())
                    );
                }
            }
        }
    }
}

#[test]
fn updates_all_six_stages_or_one_selected_stage() {
    let mut engine = Engine::new();
    assert_eq!(engine.correct(), 1);
    assert_eq!((engine.gets, engine.writes), (6, 12));
    for stage in 0..6 {
        assert_eq!(engine.value(stage), CORRECTED);
        assert_eq!(engine.memory[buffer_address(stage) + 0x68], 1);
        let mut single = Engine::new();
        single.put(CONSTANT + 0x10, LAYOUT.stage_bits[stage].to_le_bytes());
        assert_eq!(single.correct(), 1);
        assert_eq!((single.gets, single.writes), (1, 2));
        for other in 0..6 {
            assert_eq!(
                single.value(other),
                if other == stage { CORRECTED } else { STOCK }
            );
        }
    }
}

#[test]
fn identical_aliases_share_a_write_and_overlaps_refuse() {
    for (index, expected, writes) in [(16u16, 1, 10), (20, 0, 0)] {
        let mut engine = Engine::new();
        engine.selected[1] = engine.selected[0];
        engine.put(CONSTANT + 0x18, index.to_le_bytes());
        assert_eq!(engine.correct(), expected);
        assert_eq!(engine.writes, writes);
        assert_eq!(
            engine.value(0),
            if expected == 1 { CORRECTED } else { STOCK }
        );
    }
}

#[test]
fn verifies_rollback_after_each_write_and_surfaces_incomplete_rollback() {
    for failure in 0..12 {
        let mut engine = Engine::new();
        let original = engine.memory;
        engine.fail_write = Some(failure);
        assert_eq!(engine.correct(), 0);
        assert_eq!(engine.memory, original);
    }
    let mut incomplete = Engine::new();
    incomplete.fail_write = Some(1);
    incomplete.fail_restoration = true;
    assert_eq!(incomplete.correct(), 2);
    assert_ne!(incomplete.value(0), STOCK);
}

#[test]
fn malformed_destinations_are_refused_before_writes() {
    let faults: [fn(&mut Engine); 5] = [
        |engine| engine.put(CONSTANT + 0x16 + 5 * 4, 3u16.to_le_bytes()),
        |engine| engine.put(buffer_address(5) + 0x58, 16u32.to_le_bytes()),
        |engine| engine.put(CONSTANT + 0x10, (0x7bu32 | (15 << 16)).to_le_bytes()),
        |engine| engine.put(CONSTANT + 0x10, 4u32.to_le_bytes()),
        |engine| engine.put(data_address(5) + data_offset(5), f32::NAN.to_le_bytes()),
    ];
    for fault in faults {
        let mut engine = Engine::new();
        fault(&mut engine);
        assert_eq!(engine.correct(), 0);
        assert_eq!(engine.writes, 0);
    }
}
