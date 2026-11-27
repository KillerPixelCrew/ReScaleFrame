// SPDX-License-Identifier: MIT
//! Native owner and reset mask shared by resolution and post-processing.

use crate::platform::Platform;
use std::ffi::c_void;
use windows_sys::Win32::System::{
    Diagnostics::Debug::WriteProcessMemory, Threading::GetCurrentProcess,
};

const OWNER: usize = 0x1f0;
const CLEAR_MASK: usize = 0x30;
pub(crate) const ENABLED: usize = 0x18;

pub(crate) fn owner(platform: &Platform, manager: usize) -> Option<usize> {
    if manager == 0 {
        return None;
    }
    platform.read_usize(manager.checked_add(OWNER)?)
}

pub(crate) fn clear_main_history(platform: &Platform, taa: usize) -> Option<()> {
    let address = taa.checked_add(CLEAR_MASK)?;
    // Bit zero requests reset of main history targets 81 and 82.
    write(
        platform,
        address,
        &(platform.read_u32(address)? | 1).to_le_bytes(),
    )
    .then_some(())
}

pub(crate) fn write(platform: &Platform, address: usize, bytes: &[u8]) -> bool {
    let error = platform.last_error();
    let mut copied = 0;
    let written = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            bytes.as_ptr().cast(),
            bytes.len(),
            &mut copied,
        )
    };
    platform.set_last_error(error);
    written != 0 && copied == bytes.len()
}
