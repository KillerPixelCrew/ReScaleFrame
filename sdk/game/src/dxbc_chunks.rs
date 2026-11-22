// SPDX-License-Identifier: MIT
//! Borrowing DXBC chunk bounds shared by the Runtime patch service and native shader adapters.

pub fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub struct Chunk<'a> {
    pub tag: &'a [u8; 4],
    pub data: &'a [u8],
}

/// Walk the offset table of a caller-validated, declared DXBC container slice.
pub fn chunk_table(
    bytes: &[u8],
    count: usize,
) -> Option<impl Iterator<Item = Option<(usize, Chunk<'_>)>>> {
    let table_end = 32usize.checked_add(count.checked_mul(4)?)?;
    let table = bytes.get(32..table_end)?;
    Some(table.chunks_exact(4).map(move |entry| {
        let offset = word(entry, 0)? as usize;
        Some((offset, chunk_at(bytes, table_end, offset)?))
    }))
}

fn chunk_at(bytes: &[u8], table_end: usize, offset: usize) -> Option<Chunk<'_>> {
    if offset < table_end {
        return None;
    }
    let tag = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    let size = word(bytes, offset + 4)? as usize;
    let start = offset.checked_add(8)?;
    let end = start.checked_add(size)?;
    Some(Chunk {
        tag,
        data: bytes.get(start..end)?,
    })
}
