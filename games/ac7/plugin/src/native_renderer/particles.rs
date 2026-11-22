// SPDX-License-Identifier: MIT
//! Beam, ribbon and anim-trail curve tags in the CPU vertices of the matched particle fillers.
//!
//! The matched beam fillers own these CPU vertices until their asynchronous upload completes.
//! Only unused factory fields are tagged; position, old position, width, rotation, colour and UV
//! stay native. The tag is self-describing and normal float values avoid denormal transport loss.
use super::{Guard, hook, installed, key, original, read};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::write_unaligned;
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
    PAGE_GUARD, PAGE_NOACCESS, PAGE_READWRITE, PAGE_WRITECOPY, VirtualQuery,
};

fn normal_tag(word: u32) -> u32 {
    0x2000_0000 | (word & 0x1fff_ffff) | ((word & 0x2000_0000) << 2)
}

fn write_curve_tag(
    vertex: *mut u8,
    first: u32,
    second: u32,
    kind: u32,
    sheet: u32,
    sample: u32,
    intervals: u32,
) {
    let a = normal_tag(first | ((sheet & 3) << 26) | (kind << 28));
    let b = normal_tag(second | ((sheet >> 2) << 26));
    let c = normal_tag(sample | (intervals << 12));
    unsafe {
        write_unaligned(vertex.add(12).cast::<u32>(), a);
        write_unaligned(vertex.add(28).cast::<u32>(), b);
        write_unaligned(vertex.add(44).cast::<u32>(), c);
    }
}

/// The fill's committed, writable vertex range: `count` vertices of 80 bytes, or None.
fn vertex_range(fill: *const c_void) -> Option<(*mut u8, i32)> {
    let (mut vertices, mut count, mut stride) = (0u64, 0i32, 0i32);
    let fill = key(fill);
    if !read(fill, 0x88, &mut count)
        || !read(fill, 0x8c, &mut stride)
        || !read(fill, 0x90, &mut vertices)
        || stride != 80
        || count <= 0
        || count > 1024 * 1024
        || vertices == 0
    {
        return None;
    }
    let mut region = MEMORY_BASIC_INFORMATION::default();
    let bytes = u64::from(count as u32) * 80;
    let base = vertices as usize as *const c_void;
    let writable =
        PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    if unsafe {
        VirtualQuery(
            Some(base),
            &mut region,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    } == 0
        || region.State != MEM_COMMIT
        || region.Protect.0 & (PAGE_NOACCESS.0 | PAGE_GUARD.0) != 0
        || region.Protect.0 & writable.0 == 0
        || vertices < region.BaseAddress as usize as u64
        || bytes
            > (region.BaseAddress as usize as u64 + region.RegionSize as u64).wrapping_sub(vertices)
    {
        return None;
    }
    let data = vertices as usize as *mut u8;
    // A rejected producer must not leave a tag from an earlier pooled buffer allocation.
    for index in 0..count as usize {
        unsafe { write_unaligned(data.add(index * 80 + 44).cast::<f32>(), 0.0) };
    }
    Some((data, count))
}

fn push<T>(list: &mut Vec<T>, value: T) -> bool {
    if list.try_reserve(1).is_err() {
        return false;
    }
    list.push(value);
    true
}

pub(super) fn tag_beam_vertices(source: *const c_void, fill: *const c_void) -> bool {
    let Some((data, count)) = vertex_range(fill) else {
        return false;
    };
    let (mut particles, mut active, mut particle_stride, mut payload_offset, mut sheets) =
        (0u64, 0i32, 0i32, 0i32, 0i32);
    let source = key(source);
    if !read(source, 0x44, &mut active)
        || !read(source, 0x48, &mut particle_stride)
        || !read(source, 0x60, &mut particles)
        || !read(source, 0x12c, &mut payload_offset)
        || !read(source, 0x154, &mut sheets)
        || !(0..=65536).contains(&active)
        || !(96..=1024 * 1024).contains(&particle_stride)
        || particles == 0
        || payload_offset < 96
        || payload_offset > particle_stride - 92
        || !(1..=32).contains(&sheets)
    {
        return false;
    }
    struct Beam {
        counter: u32,
        pairs: u32,
        offset: u32,
    }
    let mut beams = Vec::new();
    let mut total = 0u64;
    for index in 0..active as u32 {
        let particle = particles.wrapping_add(u64::from(index) * u64::from(particle_stride as u32));
        let (mut triangles, mut flags) = (0i32, 0u32);
        if !read(particle, payload_offset as usize + 0x58, &mut triangles)
            || !read(particle, 0x5c, &mut flags)
            || triangles < 0
        {
            return false;
        }
        if triangles == 0 {
            continue;
        }
        if triangles % 2 != 0 || triangles > 8190 {
            return false;
        }
        let pairs = (triangles / 2) as u32 + 1;
        if !push(
            &mut beams,
            Beam {
                counter: flags & 0x03ff_ffff,
                pairs,
                offset: total as u32,
            },
        ) {
            return false;
        }
        total += u64::from(pairs) * 2 * u64::from(sheets as u32);
        if total > u64::from(count as u32) {
            return false;
        }
    }
    if total != u64::from(count as u32) {
        return false;
    }
    for beam in &beams {
        for sheet in 0..sheets as u32 {
            for sample in 0..beam.pairs {
                for side in 0..2 {
                    let vertex = (beam.offset + (sheet * beam.pairs + sample) * 2 + side) as usize;
                    write_curve_tag(
                        data.wrapping_add(vertex * 80),
                        beam.counter,
                        beam.counter,
                        0,
                        sheet,
                        sample,
                        beam.pairs - 1,
                    );
                }
            }
        }
    }
    true
}

pub(super) fn tag_trail_vertices(source: *const c_void, fill: *const c_void, kind: u32) -> bool {
    let Some((data, count)) = vertex_range(fill) else {
        return false;
    };
    let (mut particles, mut indices) = (0u64, 0u64);
    let (mut active, mut particle_stride, mut payload_offset) = (0i32, 0i32, 0i32);
    let source = key(source);
    if !read(source, 0x64, &mut active)
        || !read(source, 0x68, &mut particle_stride)
        || !read(source, 0x80, &mut particles)
        || !read(source, 0x88, &mut indices)
        || !read(source, 0x138, &mut payload_offset)
        || !(0..=16384).contains(&active)
        || !(96..=1024 * 1024).contains(&particle_stride)
        || particles == 0
        || indices == 0
        || payload_offset < 96
        || payload_offset > particle_stride - 32
    {
        return false;
    }
    struct Point {
        first: u32,
        second: u32,
        sample: u32,
        intervals: u32,
    }
    let stride = u64::from(particle_stride as u32);
    let mut points = Vec::new();
    let mut visited: Vec<u16> = Vec::new();
    for position in 0..active as usize {
        let (mut index, mut flags) = (0u16, 0u32);
        if !read(indices, position * 2, &mut index)
            || index >= 16384
            || !read(
                particles.wrapping_add(u64::from(index) * stride),
                payload_offset as usize,
                &mut flags,
            )
        {
            return false;
        }
        let kind_bits = flags & 0xf000_0000;
        if (kind_bits != 0x4000_0000 && kind_bits != 0x1000_0000) || flags & 0x3fff == 0x3fff {
            continue;
        }
        visited.clear();
        let mut previous = 0u32;
        let mut first = true;
        while index != 0x3fff {
            if visited.len() >= active as usize || visited.contains(&index) {
                return false;
            }
            if !push(&mut visited, index) {
                return false;
            }
            let particle = particles.wrapping_add(u64::from(index) * stride);
            let (mut counter, mut subdivisions) = (0u32, 0i32);
            if !read(particle, 0x5c, &mut counter)
                || !read(particle, payload_offset as usize, &mut flags)
                || !read(particle, payload_offset as usize + 0x1c, &mut subdivisions)
            {
                return false;
            }
            counter &= 0x03ff_ffff;
            let steps = if first { 1 } else { subdivisions.max(1) as u32 };
            if steps > 4095 || points.len() + steps as usize > count as usize / 2 {
                return false;
            }
            for sample in (1..=steps).rev() {
                let point = Point {
                    first: counter,
                    second: if first { counter } else { previous },
                    sample: sample - 1,
                    intervals: steps,
                };
                if !push(&mut points, point) {
                    return false;
                }
            }
            first = false;
            previous = counter;
            index = (flags & 0x3fff) as u16;
        }
    }
    if points.len() * 2 != count as usize {
        return false;
    }
    for (index, point) in points.iter().enumerate() {
        for side in 0..2 {
            write_curve_tag(
                data.wrapping_add((index * 2 + side) * 80),
                point.first,
                point.second,
                kind,
                0,
                point.sample,
                point.intervals,
            );
        }
    }
    true
}

pub(super) unsafe extern "C" fn hooked_beam_fill<const HOOK: u32>(
    source: *mut c_void,
    fill: *mut c_void,
) -> u64 {
    let _lifetime = Guard::outer();
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 = unsafe { original(HOOK) };
    let result = unsafe { forward(source, fill) };
    if let Some(owner) = installed()
        && owner.active()
        && !owner.quiescing()
    {
        if HOOK == hook::RIBBON_FILL || HOOK == hook::ANIM_TRAIL_FILL {
            tag_trail_vertices(source, fill, if HOOK == hook::RIBBON_FILL { 1 } else { 2 });
        } else {
            tag_beam_vertices(source, fill);
        }
    }
    result
}
