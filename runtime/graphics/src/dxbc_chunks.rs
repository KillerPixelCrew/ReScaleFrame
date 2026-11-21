// SPDX-License-Identifier: MIT
//! Borrowing DXBC chunk bounds shared by the Runtime patch service and native shader adapters.

pub(crate) fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(crate) struct Chunk<'a> {
    pub(crate) tag: &'a [u8; 4],
    pub(crate) data: &'a [u8],
}

pub(crate) fn chunk_at(bytes: &[u8], table_end: usize, offset: usize) -> Option<Chunk<'_>> {
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
