// SPDX-License-Identifier: MIT
use super::{Platform, Read, profile, read_bytes, rsf_anomaly_geometry_binding};
use std::{ffi::c_void, ptr};

/// CBackend::m_aVertexConstants slots, the reflected size and the staged data
/// pointer of the engine's dx10ConstantBuffer (documented in geometry_profile.rs).
pub(crate) const SLOTS: u32 = 14;
pub(crate) const SIZE_OFFSET: u32 = 0x58;
pub(crate) const DATA_OFFSET: u32 = 0x60;

struct VertexConstantTable {
    module: usize,
    table_rva: u32,
}

impl VertexConstantTable {
    fn bound() -> Option<Self> {
        let engine = rsf_anomaly_geometry_binding();
        (engine.module != 0).then_some(())?;
        Some(Self {
            module: engine.module,
            table_rva: profile::geometry(engine)?.vertex_constant_buffers,
        })
    }

    fn data(
        &self,
        slot: u32,
        expected_size: u32,
        offset: u32,
        bytes: u32,
        read: Read,
    ) -> Option<usize> {
        if self.module == 0
            || slot >= SLOTS
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
        let size = u32::from_ne_bytes(read_bytes(read, object.checked_add(SIZE_OFFSET as usize)?)?);
        if size != expected_size {
            return None;
        }
        let data =
            usize::from_ne_bytes(read_bytes(read, object.checked_add(DATA_OFFSET as usize)?)?);
        if data == 0 {
            return None;
        }
        let address = data.checked_add(offset as usize)?;
        address.checked_add(bytes as usize)?;
        Some(address)
    }
}

/// Copies `bytes` of the staged vertex constants of `slot` into `out`.
///
/// # Safety
/// `out` names `bytes` writable bytes. All engine memory access, including the
/// final transfer, goes through the guarded native read.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_read_vertex(
    platform: &Platform,
    slot: u32,
    expected_size: u32,
    offset: u32,
    out: *mut c_void,
    bytes: u32,
) -> bool {
    VertexConstantTable::bound()
        .and_then(|table| table.data(slot, expected_size, offset, bytes, platform.read.unwrap()))
        .is_some_and(|source| unsafe {
            (platform.read.unwrap())(out, ptr::with_exposed_provenance(source), bytes as usize)
        })
}

/// Copies `bytes` from `input` into the staged vertex constants of `slot`.
///
/// # Safety
/// `input` names `bytes` readable bytes. The write goes through the guarded
/// native copy, whose arguments are (destination, source, size).
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_write_vertex(
    platform: &Platform,
    slot: u32,
    expected_size: u32,
    offset: u32,
    input: *const c_void,
    bytes: u32,
) -> bool {
    VertexConstantTable::bound()
        .and_then(|table| table.data(slot, expected_size, offset, bytes, platform.read.unwrap()))
        .is_some_and(|target| unsafe {
            (platform.read.unwrap())(
                ptr::with_exposed_provenance_mut(target),
                input,
                bytes as usize,
            )
        })
}
