// SPDX-License-Identifier: MIT
//! DXBC containers and signatures for AC7's velocity shader transforms.
//! The native callers keep their vectors and their typed compiler and shader operations.
use crate::dxbc_chunks::{chunk_at, word};
use crate::shader_hash::{dxbc_checksum, dxbc_digest_bytes};
use std::ffi::{c_char, c_void};
use std::slice;

const HEADER: usize = 32;
const MAX_READ: usize = 8 * 1024 * 1024;
const SIGNATURE_HEADER: usize = 8;
const MAX_SEMANTICS: usize = 64;

#[repr(C)]
struct ChunkView {
    tag: [u8; 4],
    bytes: *const u8,
    size: usize,
}

#[repr(C)]
struct SemanticView {
    name: *const c_char,
    name_size: usize,
    index: u32,
    system: u32,
    ty: u32,
    reg: u32,
    mask: u8,
    used: u8,
}

const _: () = {
    assert!(size_of::<ChunkView>() == 24);
    assert!(size_of::<SemanticView>() == 40);
};

type AppendChunk = unsafe extern "C" fn(*mut c_void, *const ChunkView) -> bool;
type AppendSemantic = unsafe extern "C" fn(*mut c_void, *const SemanticView) -> bool;
pub(crate) type Resize = unsafe extern "C" fn(*mut c_void, usize) -> *mut u8;

/// The caller lends a readable span for the call. An empty vector may lend null.
pub(crate) unsafe fn borrowed<'a, T>(ptr: *const T, len: usize) -> Option<&'a [T]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() || len > isize::MAX as usize / size_of::<T>() {
        return None;
    }
    Some(unsafe { slice::from_raw_parts(ptr, len) })
}

fn store(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// The caller lends a container. Each callback must copy the chunk before returning.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_read_container(
    bytes: *const c_void,
    size: usize,
    user: *mut c_void,
    append: AppendChunk,
) -> bool {
    if !(HEADER..=MAX_READ).contains(&size) {
        return false;
    }
    let Some(bytes) = (unsafe { borrowed(bytes.cast::<u8>(), size) }) else {
        return false;
    };
    if &bytes[..4] != b"DXBC" {
        return false;
    }
    let (Some(total), Some(count)) = (word(bytes, 24), word(bytes, 28)) else {
        return false;
    };
    let (total, count) = (total as usize, count as usize);
    if total > size || total < HEADER || count > 32 || count * 4 > total - HEADER {
        return false;
    }
    let bytes = &bytes[..total];
    let table_end = HEADER + count * 4;
    for index in 0..count {
        let Some(chunk) = word(bytes, HEADER + index * 4)
            .and_then(|offset| chunk_at(bytes, table_end, offset as usize))
        else {
            return false;
        };
        let view = ChunkView {
            tag: *chunk.tag,
            bytes: chunk.data.as_ptr(),
            size: chunk.data.len(),
        };
        if !unsafe { append(user, &view) } {
            return false;
        }
    }
    true
}

/// The caller lends each chunk and a callback returning a nonoverlapping writable output.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_write_container(
    chunks: *const ChunkView,
    count: usize,
    user: *mut c_void,
    resize: Resize,
) -> bool {
    let Some(chunks) = (unsafe { borrowed(chunks, count) }) else {
        return false;
    };
    let Some(table_end) = count
        .checked_mul(4)
        .and_then(|size| HEADER.checked_add(size))
    else {
        return false;
    };
    let Some(size) = chunks.iter().try_fold(table_end, |size, chunk| {
        if chunk.size != 0 && chunk.bytes.is_null() {
            return None;
        }
        size.checked_add(8)?.checked_add(chunk.size)
    }) else {
        return false;
    };
    let Ok(total) = u32::try_from(size) else {
        return false;
    };
    let bytes = unsafe { resize(user, size) };
    if bytes.is_null() {
        return false;
    }
    let output = unsafe { slice::from_raw_parts_mut(bytes, size) };
    output[..table_end].fill(0);
    output[..4].copy_from_slice(b"DXBC");
    store(output, 20, 1);
    store(output, 24, total);
    store(output, 28, count as u32);

    let mut offset = table_end;
    for (index, chunk) in chunks.iter().enumerate() {
        let Some(data) = (unsafe { borrowed(chunk.bytes, chunk.size) }) else {
            return false;
        };
        store(output, HEADER + index * 4, offset as u32);
        output[offset..offset + 4].copy_from_slice(&chunk.tag);
        store(output, offset + 4, chunk.size as u32);
        offset += 8;
        output[offset..offset + data.len()].copy_from_slice(data);
        offset += data.len();
    }
    let Some(checksum) = dxbc_checksum(&output[20..]) else {
        return false;
    };
    output[4..20].copy_from_slice(&dxbc_digest_bytes(checksum));
    true
}

fn signature_layout(streams: bool) -> (usize, usize) {
    let fields_offset = if streams { 4 } else { 0 };
    (24 + fields_offset, fields_offset)
}

/// The caller lends a signature chunk. Each callback copies the semantic name and fields.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_read_signature(
    bytes: *const u8,
    size: usize,
    streams: bool,
    user: *mut c_void,
    append: AppendSemantic,
) -> bool {
    let Some(bytes) = (unsafe { borrowed(bytes, size) }) else {
        return false;
    };
    let Some(count) = word(bytes, 0) else {
        return false;
    };
    let count = count as usize;
    if count > MAX_SEMANTICS {
        return false;
    }
    let (stride, fields_offset) = signature_layout(streams);
    let table_end = SIGNATURE_HEADER + count * stride;
    if table_end > size {
        return false;
    }
    for i in 0..count {
        let record = SIGNATURE_HEADER + i * stride;
        if streams && word(bytes, record) != Some(0) {
            return false;
        }
        let offset = record + fields_offset;
        let fields = &bytes[offset..offset + 24];
        let value = |at| word(fields, at).unwrap_or(0);
        let name = value(0) as usize;
        let Some(text) = bytes.get(name..) else {
            return false;
        };
        let Some(end) = text.iter().position(|&byte| byte == 0) else {
            return false;
        };
        let view = SemanticView {
            name: text.as_ptr().cast(),
            name_size: end,
            index: value(4),
            system: value(8),
            ty: value(12),
            reg: value(16),
            mask: fields[20],
            used: fields[21],
        };
        if !unsafe { append(user, &view) } {
            return false;
        }
    }
    true
}

/// The caller lends the semantic records and a nonoverlapping native output allocation.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_write_signature(
    semantics: *const SemanticView,
    count: usize,
    streams: bool,
    user: *mut c_void,
    resize: Resize,
) -> bool {
    let Some(semantics) = (unsafe { borrowed(semantics, count) }) else {
        return false;
    };
    let (stride, fields_offset) = signature_layout(streams);
    let Some(table_end) = count
        .checked_mul(stride)
        .and_then(|size| SIGNATURE_HEADER.checked_add(size))
    else {
        return false;
    };
    let Some(size) = semantics
        .iter()
        .try_fold(table_end, |size, semantic| {
            if semantic.name_size != 0 && semantic.name.is_null() {
                return None;
            }
            size.checked_add(semantic.name_size)?.checked_add(1)
        })
        .and_then(|size| size.checked_add(3))
        .map(|size| size & !3)
    else {
        return false;
    };
    if u32::try_from(size).is_err() {
        return false;
    }
    let mut order = Vec::new();
    if order.try_reserve_exact(count).is_err() {
        return false;
    }
    order.extend(0..count);
    order.sort_unstable_by_key(|&i| (semantics[i].reg, semantics[i].mask, i));
    let bytes = unsafe { resize(user, size) };
    if bytes.is_null() {
        return false;
    }
    let output = unsafe { slice::from_raw_parts_mut(bytes, size) };
    output.fill(0);
    store(output, 0, count as u32);
    store(output, 4, SIGNATURE_HEADER as u32);
    let mut cursor = table_end;
    for (i, index) in order.into_iter().enumerate() {
        let semantic = &semantics[index];
        let offset = SIGNATURE_HEADER + i * stride + fields_offset;
        store(output, offset, cursor as u32);
        store(output, offset + 4, semantic.index);
        store(output, offset + 8, semantic.system);
        store(output, offset + 12, semantic.ty);
        store(output, offset + 16, semantic.reg);
        output[offset + 20] = semantic.mask;
        output[offset + 21] = semantic.used;
        let Some(name) = (unsafe { borrowed(semantic.name.cast::<u8>(), semantic.name_size) })
        else {
            return false;
        };
        output[cursor..cursor + name.len()].copy_from_slice(name);
        cursor += name.len() + 1;
    }
    true
}
