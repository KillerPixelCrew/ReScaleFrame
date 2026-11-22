// SPDX-License-Identifier: MIT
use super::{Read, read_bytes};
use std::{ffi::c_void, ptr};

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

// The native caller lends the draw's input or output span. All engine memory
// access, including the final transfer, still goes through the guarded leaf.
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_read_vertex(
    table: &VertexConstantTable,
    slot: u32,
    expected_size: u32,
    offset: u32,
    out: *mut c_void,
    bytes: u32,
    copy: Read,
) -> bool {
    table
        .data(slot, expected_size, offset, bytes, copy)
        .is_some_and(|source| unsafe {
            copy(out, ptr::with_exposed_provenance(source), bytes as usize)
        })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_write_vertex(
    table: &VertexConstantTable,
    slot: u32,
    expected_size: u32,
    offset: u32,
    input: *const c_void,
    bytes: u32,
    copy: Read,
) -> bool {
    table
        .data(slot, expected_size, offset, bytes, copy)
        .is_some_and(|target| unsafe {
            copy(
                ptr::with_exposed_provenance_mut(target),
                input,
                bytes as usize,
            )
        })
}
