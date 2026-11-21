// SPDX-License-Identifier: MIT
use crate::engine_guards::Read;
use std::ptr;

#[repr(C)]
struct VertexConstantTable {
    module: usize,
    table_rva: u32,
    slots: u32,
    size_offset: u32,
    data_offset: u32,
}

const _: () = {
    assert!(size_of::<VertexConstantTable>() == 24);
    assert!(std::mem::offset_of!(VertexConstantTable, table_rva) == 8);
};

fn read_bytes<const N: usize>(read: Read, address: usize) -> Option<[u8; N]> {
    address.checked_add(N)?;
    let mut bytes = [0; N];
    // The native leaf guards the read and reports whether the complete span was copied.
    unsafe {
        read(
            bytes.as_mut_ptr().cast(),
            ptr::with_exposed_provenance(address),
            N,
        )
    }
    .then_some(bytes)
}

impl VertexConstantTable {
    fn data(
        &self,
        slot: u32,
        expected_size: u32,
        offset: u32,
        bytes: u32,
        read: Read,
    ) -> Option<usize> {
        if self.module == 0
            || slot >= self.slots
            || expected_size == 0
            || offset
                .checked_add(bytes)
                .is_none_or(|end| end > expected_size)
        {
            return None;
        }
        let entry = self
            .module
            .checked_add(self.table_rva as usize)?
            .checked_add((slot as usize).checked_mul(size_of::<usize>())?)?;
        let object = usize::from_ne_bytes(read_bytes(read, entry)?);
        if object == 0 {
            return None;
        }
        let size = u32::from_ne_bytes(read_bytes(
            read,
            object.checked_add(self.size_offset as usize)?,
        )?);
        if size != expected_size {
            return None;
        }
        let data = usize::from_ne_bytes(read_bytes(
            read,
            object.checked_add(self.data_offset as usize)?,
        )?);
        if data == 0 {
            return None;
        }
        let address = data.checked_add(offset as usize)?;
        address.checked_add(bytes as usize)?;
        Some(address)
    }
}

/// Resolves the staged data address. The caller supplies the immutable build facts and performs
/// the guarded copy while the observed buffer still belongs to the current draw.
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_vertex_constant_data(
    table: &VertexConstantTable,
    slot: u32,
    expected_size: u32,
    offset: u32,
    bytes: u32,
    read: Read,
) -> usize {
    table
        .data(slot, expected_size, offset, bytes, read)
        .unwrap_or(0)
}
