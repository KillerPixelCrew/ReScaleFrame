// SPDX-License-Identifier: MIT
//! Update the CPU staging copy of the position constant before the engine uploads it.

use std::ffi::c_void;
use std::ptr::from_ref;

type Vector = [f32; 4];
const VECTOR_BYTES: usize = size_of::<Vector>();

#[repr(C)]
struct Layout {
    destination: u32,
    load: u32,
    buffer_size: u32,
    buffer_data: u32,
    buffer_changed: u32,
    buffer_slots: u32,
    stage_bits: [u32; 6],
    stage_shifts: [u32; 6],
}

#[repr(C)]
struct Platform {
    user: *mut c_void,
    read: unsafe extern "C" fn(*mut c_void, usize, *mut c_void, usize) -> bool,
    write: unsafe extern "C" fn(*mut c_void, usize, *const c_void, usize) -> bool,
    buffer: unsafe extern "C" fn(*mut c_void, usize, u32) -> usize,
}

const _: () = {
    assert!(size_of::<Layout>() == 72);
    assert!(size_of::<Platform>() == 32);
};

impl Platform {
    fn read<const N: usize>(&self, base: usize, offset: usize) -> Option<[u8; N]> {
        let address = base.checked_add(offset)?;
        if base == 0 || address.checked_add(N - 1).is_none() {
            return None;
        }
        let mut bytes = [0; N];
        unsafe { (self.read)(self.user, address, bytes.as_mut_ptr().cast(), N) }.then_some(bytes)
    }
}

fn corrected(mut value: Vector, x: f32, y: f32) -> Option<Vector> {
    if !x.is_finite() || !y.is_finite() || value.iter().any(|v| !v.is_finite()) {
        return None;
    }
    value[0] += x * value[2];
    value[1] += y * value[3];
    (value[0].is_finite() && value[1].is_finite()).then_some(value)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_correct_position_vector(value: &mut Vector, x: f32, y: f32) -> bool {
    let Some(next) = corrected(*value, x, y) else {
        return false;
    };
    *value = next;
    true
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
    fn write(&self, platform: &Platform, value: &Vector, dirty: u8) -> bool {
        unsafe {
            (platform.write)(
                platform.user,
                self.data,
                from_ref(value).cast(),
                VECTOR_BYTES,
            ) && (platform.write)(platform.user, self.dirty, from_ref(&dirty).cast(), 1)
        }
    }

    fn restore(&self, platform: &Platform) -> bool {
        // A failed write can still have changed part of the destination. Verify
        // both values while the engine's staging slot is still in scope.
        for _ in 0..2 {
            unsafe {
                (platform.write)(
                    platform.user,
                    self.data,
                    from_ref(&self.stock).cast(),
                    VECTOR_BYTES,
                );
                (platform.write)(
                    platform.user,
                    self.dirty,
                    from_ref(&self.was_dirty).cast(),
                    1,
                );
            }
            if platform.read::<VECTOR_BYTES>(self.data, 0)
                == Some(std::array::from_fn(|i| {
                    self.stock[i / 4].to_le_bytes()[i % 4]
                }))
                && platform.read::<1>(self.dirty, 0) == Some([self.was_dirty])
            {
                return true;
            }
        }
        false
    }
}

fn prepare_slot(
    layout: &Layout,
    platform: &Platform,
    constant: usize,
    stage: u32,
    x: f32,
    y: f32,
) -> Option<Slot> {
    let [a, b, c, d] = platform.read(constant, layout.load as usize + stage as usize * 4)?;
    let offset = usize::from(u16::from_le_bytes([a, b]));
    if u16::from_le_bytes([c, d]) != 1 || !offset.is_multiple_of(size_of::<f32>()) {
        return None;
    }
    let buffer = unsafe { (platform.buffer)(platform.user, constant, stage) };
    let size = u32::from_le_bytes(platform.read(buffer, layout.buffer_size as usize)?) as usize;
    let base = usize::from_le_bytes(platform.read(buffer, layout.buffer_data as usize)?);
    if offset + VECTOR_BYTES > size {
        return None;
    }
    let bytes = platform.read::<VECTOR_BYTES>(base, offset)?;
    let stock = std::array::from_fn(|i| f32::from_le_bytes(bytes.as_chunks::<4>().0[i]));
    let [was_dirty] = platform.read(buffer, layout.buffer_changed as usize)?;
    Some(Slot {
        data: base.checked_add(offset)?,
        dirty: buffer.checked_add(layout.buffer_changed as usize)?,
        stock,
        corrected: corrected(stock, x, y)?,
        was_dirty,
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_correct_position_constant(
    layout: &Layout,
    platform: &Platform,
    constant: usize,
    x: f32,
    y: f32,
) -> u32 {
    let Some(destination) = platform
        .read(constant, layout.destination as usize)
        .map(u32::from_le_bytes)
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
        let Some(slot) = prepare_slot(layout, platform, constant, stage as u32, x, y) else {
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
        if !slot.write(platform, &slot.corrected, 1) {
            let mut restored = true;
            for restore in &slots[..=i] {
                restored &= restore.restore(platform);
            }
            return if restored { 0 } else { 2 };
        }
    }
    u32::from(count != 0)
}
