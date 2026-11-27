// SPDX-License-Identifier: MIT
//! DXBC containers and signatures for AC7's velocity shader transforms.
use crate::dxbc_chunks::{Chunk, chunk_table, word};
use crate::shader_hash::{dxbc_checksum, dxbc_digest_bytes};
use std::slice;

const HEADER: usize = 32;
const MAX_CONTAINER_SIZE: usize = 8 * 1024 * 1024;
const SIGNATURE_HEADER: usize = 8;
const MAX_SEMANTICS: usize = 64;

#[derive(Clone)]
pub(crate) struct Semantic<'a> {
    pub(crate) name: &'a [u8],
    pub(crate) index: u32,
    pub(crate) system: u32,
    pub(crate) ty: u32,
    pub(crate) reg: u32,
    pub(crate) mask: u8,
    pub(crate) used: u8,
}

pub(crate) fn valid_container_size(size: usize) -> bool {
    (HEADER..=MAX_CONTAINER_SIZE).contains(&size)
}

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

pub(crate) fn resize_bytes(bytes: &mut Vec<u8>, size: usize) -> Option<&mut [u8]> {
    bytes
        .try_reserve_exact(size.saturating_sub(bytes.len()))
        .ok()?;
    bytes.resize(size, 0);
    Some(bytes.as_mut_slice())
}

pub(crate) fn store(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn read_container<'a>(
    bytes: &'a [u8],
    mut visit: impl FnMut(Chunk<'a>) -> Option<()>,
) -> Option<()> {
    if !valid_container_size(bytes.len()) || &bytes[..4] != b"DXBC" {
        return None;
    }
    let total = word(bytes, 24)? as usize;
    let count = word(bytes, 28)? as usize;
    let size = bytes.len();
    if total > size || total < HEADER || count > 32 {
        return None;
    }
    let bytes = &bytes[..total];
    for entry in chunk_table(bytes, count)? {
        visit(entry?.1)?;
    }
    Some(())
}

pub(crate) fn chunks(bytes: &[u8]) -> Option<Vec<Chunk<'_>>> {
    let mut chunks = Vec::new();
    read_container(bytes, |chunk| {
        chunks.try_reserve(1).ok()?;
        chunks.push(chunk);
        Some(())
    })?;
    Some(chunks)
}

pub(crate) fn chunk_index(chunks: &[Chunk<'_>], tag: &[u8; 4]) -> Option<usize> {
    chunks.iter().position(|chunk| chunk.tag == tag)
}

pub(crate) fn shader_index(chunks: &[Chunk<'_>]) -> Option<usize> {
    chunk_index(chunks, b"SHEX").or_else(|| chunk_index(chunks, b"SHDR"))
}

pub(crate) fn write_container(chunks: &[Chunk<'_>], destination: &mut Vec<u8>) -> Option<()> {
    let count = chunks.len();
    let table_end = HEADER.checked_add(count.checked_mul(4)?)?;
    let size = chunks.iter().try_fold(table_end, |size, chunk| {
        size.checked_add(8)?.checked_add(chunk.data.len())
    })?;
    let total = u32::try_from(size).ok()?;
    let output = resize_bytes(destination, size)?;
    output[..table_end].fill(0);
    output[..4].copy_from_slice(b"DXBC");
    store(output, 20, 1);
    store(output, 24, total);
    store(output, 28, count as u32);

    let mut offset = table_end;
    for (index, chunk) in chunks.iter().enumerate() {
        let data = chunk.data;
        store(output, HEADER + index * 4, offset as u32);
        output[offset..offset + 4].copy_from_slice(chunk.tag);
        store(output, offset + 4, data.len() as u32);
        offset += 8;
        output[offset..offset + data.len()].copy_from_slice(data);
        offset += data.len();
    }
    let checksum = dxbc_checksum(&output[20..])?;
    output[4..20].copy_from_slice(&dxbc_digest_bytes(checksum));
    Some(())
}

fn signature_layout(streams: bool) -> (usize, usize) {
    let fields_offset = if streams { 4 } else { 0 };
    (24 + fields_offset, fields_offset)
}

pub(crate) fn read_signature<'a>(
    bytes: &'a [u8],
    streams: bool,
    mut visit: impl FnMut(Semantic<'a>) -> Option<()>,
) -> Option<()> {
    let count = word(bytes, 0)? as usize;
    if count > MAX_SEMANTICS {
        return None;
    }
    let (stride, fields_offset) = signature_layout(streams);
    let table_end = SIGNATURE_HEADER + count * stride;
    if table_end > bytes.len() {
        return None;
    }
    for i in 0..count {
        let record = SIGNATURE_HEADER + i * stride;
        if streams && word(bytes, record) != Some(0) {
            return None;
        }
        let offset = record + fields_offset;
        let fields = &bytes[offset..offset + 24];
        let value = |at| word(fields, at).unwrap_or(0);
        let name = value(0) as usize;
        let text = bytes.get(name..)?;
        let end = text.iter().position(|&byte| byte == 0)?;
        visit(Semantic {
            name: &text[..end],
            index: value(4),
            system: value(8),
            ty: value(12),
            reg: value(16),
            mask: fields[20],
            used: fields[21],
        })?;
    }
    Some(())
}

pub(crate) fn write_signature(
    semantics: &[Semantic<'_>],
    streams: bool,
    destination: &mut Vec<u8>,
) -> Option<()> {
    let count = semantics.len();
    let (stride, fields_offset) = signature_layout(streams);
    let table_end = SIGNATURE_HEADER.checked_add(count.checked_mul(stride)?)?;
    let size = semantics
        .iter()
        .try_fold(table_end, |size, semantic| {
            size.checked_add(semantic.name.len())?.checked_add(1)
        })
        .and_then(|size| size.checked_add(3))
        .map(|size| size & !3)?;
    u32::try_from(size).ok()?;
    let mut order = Vec::new();
    order.try_reserve_exact(count).ok()?;
    order.extend(0..count);
    order.sort_unstable_by_key(|&i| (semantics[i].reg, semantics[i].mask, i));
    let output = resize_bytes(destination, size)?;
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
        let name = semantic.name;
        output[cursor..cursor + name.len()].copy_from_slice(name);
        cursor += name.len() + 1;
    }
    Some(())
}
