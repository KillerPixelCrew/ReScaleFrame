// SPDX-License-Identifier: MIT
use super::d3d::rsf_census_d3d_bind;
use super::fsr::rsf_census_fsr_bind;
use super::misc::rsf_census_misc_bind;
use super::ngx::rsf_census_ngx_bind;
use super::vulkan::rsf_census_vk_bind;
use crate::sys::rsf_census_platform;
use std::ffi::c_void;
use std::ptr::null_mut;

type Wrapper = *mut c_void;
type Bind<const N: usize> = unsafe fn(&[u16; N], &mut [Wrapper; N]);

unsafe fn register<const N: usize>(
    table: &mut [Wrapper],
    entries: *const u16,
    bind: Bind<N>,
) -> bool {
    let Some(entries) = (unsafe { entries.cast::<[u16; N]>().as_ref() }) else {
        return false;
    };
    if entries
        .iter()
        .any(|&entry| usize::from(entry) >= table.len())
    {
        return false;
    }
    let mut wrappers = [null_mut(); N];
    unsafe { bind(entries, &mut wrappers) };
    for (&entry, wrapper) in entries.iter().zip(wrappers) {
        if !wrapper.is_null() {
            table[usize::from(entry)] = wrapper;
        }
    }
    true
}

/// Binds every wrapper family to its function ids in the census table.
pub(super) unsafe fn register_vendors(
    platform: &rsf_census_platform,
    table: &mut [Wrapper],
) -> bool {
    unsafe {
        register(table, platform.d3d_entries, rsf_census_d3d_bind)
            && register(table, platform.vk_entries, rsf_census_vk_bind)
            && register(table, platform.ngx_entries, rsf_census_ngx_bind)
            && register(table, platform.fsr_entries, rsf_census_fsr_bind)
            && register(table, platform.misc_entries, rsf_census_misc_bind)
    }
}
