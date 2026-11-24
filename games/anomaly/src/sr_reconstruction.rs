// SPDX-License-Identifier: MIT
//! Update the CPU staging copy of the position constant before the engine uploads it.

// The guarded engine reads and writes and the constant-buffer lookup: sr_stage/native.rs's
// position leaves, the fake engine under test.
#[cfg(not(test))]
use crate::sr_stage::native::position as engine;
use std::ptr::from_ref;
#[cfg(test)]
use tests::engine;

type Vector = [f32; 4];
const VECTOR_BYTES: usize = size_of::<Vector>();

pub(crate) struct Layout {
    pub(crate) destination: u32,
    pub(crate) load: u32,
    pub(crate) buffer_size: u32,
    pub(crate) buffer_data: u32,
    pub(crate) buffer_changed: u32,
    pub(crate) buffer_slots: u32,
    pub(crate) stage_bits: [u32; 6],
    pub(crate) stage_shifts: [u32; 6],
}

fn read<const N: usize>(base: usize, offset: usize) -> Option<[u8; N]> {
    let address = base.checked_add(offset)?;
    if base == 0 || address.checked_add(N - 1).is_none() {
        return None;
    }
    let mut bytes = [0; N];
    unsafe { engine::read(address, bytes.as_mut_ptr().cast(), N) }.then_some(bytes)
}

fn corrected(mut value: Vector, x: f32, y: f32) -> Option<Vector> {
    if !x.is_finite() || !y.is_finite() || value.iter().any(|v| !v.is_finite()) {
        return None;
    }
    value[0] += x * value[2];
    value[1] += y * value[3];
    (value[0].is_finite() && value[1].is_finite()).then_some(value)
}

#[derive(Clone, Copy, Default)]
struct Slot {
    data: usize,
    dirty: usize,
    stock: Vector,
    corrected: Vector,
    was_dirty: u8,
}

impl Slot {
    fn write(&self, value: &Vector, dirty: u8) -> bool {
        unsafe {
            engine::write(self.data, from_ref(value).cast(), VECTOR_BYTES)
                && engine::write(self.dirty, from_ref(&dirty).cast(), 1)
        }
    }

    fn restore(&self) -> bool {
        // A failed write can still have changed part of the destination. Verify
        // both values while the engine's staging slot is still in scope.
        for _ in 0..2 {
            unsafe {
                engine::write(self.data, from_ref(&self.stock).cast(), VECTOR_BYTES);
                engine::write(self.dirty, from_ref(&self.was_dirty).cast(), 1);
            }
            if read::<VECTOR_BYTES>(self.data, 0)
                == Some(std::array::from_fn(|i| {
                    self.stock[i / 4].to_le_bytes()[i % 4]
                }))
                && read::<1>(self.dirty, 0) == Some([self.was_dirty])
            {
                return true;
            }
        }
        false
    }
}

fn prepare_slot(layout: &Layout, constant: usize, stage: u32, x: f32, y: f32) -> Option<Slot> {
    let [a, b, c, d] = read(constant, layout.load as usize + stage as usize * 4)?;
    let offset = usize::from(u16::from_le_bytes([a, b]));
    if u16::from_le_bytes([c, d]) != 1 || !offset.is_multiple_of(size_of::<f32>()) {
        return None;
    }
    let buffer = unsafe { engine::buffer(constant, stage) };
    let size = u32::from_le_bytes(read(buffer, layout.buffer_size as usize)?) as usize;
    let base = usize::from_le_bytes(read(buffer, layout.buffer_data as usize)?);
    if offset + VECTOR_BYTES > size {
        return None;
    }
    let bytes = read::<VECTOR_BYTES>(base, offset)?;
    let stock = std::array::from_fn(|i| f32::from_le_bytes(bytes.as_chunks::<4>().0[i]));
    let [was_dirty] = read(buffer, layout.buffer_changed as usize)?;
    Some(Slot {
        data: base.checked_add(offset)?,
        dirty: buffer.checked_add(layout.buffer_changed as usize)?,
        stock,
        corrected: corrected(stock, x, y)?,
        was_dirty,
    })
}

pub(crate) fn rsf_anomaly_sr_correct_position_constant(
    layout: &Layout,
    constant: usize,
    x: f32,
    y: f32,
) -> u32 {
    let Some(destination) = read(constant, layout.destination as usize).map(u32::from_le_bytes)
    else {
        return 0;
    };
    if destination & 0x7b == 0 || destination & 0x84 != 0 {
        return 0;
    }
    let mut slots = [Slot::default(); 6];
    let mut count = 0;
    for (stage, (&bit, &shift)) in layout
        .stage_bits
        .iter()
        .zip(&layout.stage_shifts)
        .enumerate()
    {
        if destination & bit == 0 {
            continue;
        }
        if ((destination >> shift) & 15) >= layout.buffer_slots {
            return 0;
        }
        let Some(slot) = prepare_slot(layout, constant, stage as u32, x, y) else {
            return 0;
        };
        // Multiple stages may use the same vector. Overlapping distinct vectors would overwrite one another.
        if let Some(other) = slots[..count]
            .iter()
            .find(|other| other.data.abs_diff(slot.data) < VECTOR_BYTES)
        {
            if other.data != slot.data || other.dirty != slot.dirty {
                return 0;
            }
            continue;
        }
        slots[count] = slot;
        count += 1;
    }
    for (i, slot) in slots[..count].iter().enumerate() {
        if !slot.write(&slot.corrected, 1) {
            let mut restored = true;
            for restore in &slots[..=i] {
                restored &= restore.restore();
            }
            return if restored { 0 } else { 2 };
        }
    }
    u32::from(count != 0)
}

#[cfg(test)]
mod tests;
