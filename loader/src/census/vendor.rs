// SPDX-License-Identifier: MIT
use super::fsr::rsf_census_fsr_bind;
use super::misc::rsf_census_misc_bind;
use super::ngx::rsf_census_ngx_bind;
use super::vulkan::rsf_census_vk_bind;
use crate::sys::rsf_census_vendor_platform;
use std::ffi::c_void;
use std::ptr::null_mut;

type Wrapper = *mut c_void;
type Bind<P, const N: usize> = unsafe fn(&P, &[u16; N], &mut [Wrapper; N]);

unsafe fn register<P, const N: usize>(
    table: &mut [Wrapper],
    entries: *const u16,
    platform: *const P,
    bind: Bind<P, N>,
) -> bool {
    let (Some(entries), Some(platform)) =
        (unsafe { entries.cast::<[u16; N]>().as_ref() }, unsafe {
            platform.as_ref()
        })
    else {
        return false;
    };
    if entries
        .iter()
        .any(|&entry| usize::from(entry) >= table.len())
    {
        return false;
    }
    let mut wrappers = [null_mut(); N];
    unsafe { bind(platform, entries, &mut wrappers) };
    for (&entry, wrapper) in entries.iter().zip(wrappers) {
        table[usize::from(entry)] = wrapper;
    }
    true
}

pub(super) unsafe fn register_vendors(
    platform: &rsf_census_vendor_platform,
    table: &mut [Wrapper],
) -> bool {
    unsafe {
        register(table, platform.vk_entries, platform.vk, rsf_census_vk_bind)
            && register(
                table,
                platform.ngx_entries,
                platform.ngx,
                rsf_census_ngx_bind,
            )
            && register(
                table,
                platform.fsr_entries,
                platform.fsr,
                rsf_census_fsr_bind,
            )
            && register(
                table,
                platform.misc_entries,
                platform.misc,
                rsf_census_misc_bind,
            )
    }
}
