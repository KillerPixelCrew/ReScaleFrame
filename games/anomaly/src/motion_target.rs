// SPDX-License-Identifier: MIT
use crate::motion_target_abi::{
    RSF_ANOMALY_MOTION_BIND_ALREADY_BOUND as ALREADY_BOUND, RSF_ANOMALY_MOTION_BIND_BOUND as BOUND,
    RSF_ANOMALY_MOTION_BIND_CONFLICT as CONFLICT, RSF_ANOMALY_MOTION_BIND_FAULT as FAULT,
    RSF_ANOMALY_MOTION_BIND_NONE as NONE, RSF_ANOMALY_MOTION_BIND_UNBOUND as UNBOUND,
    rsf_anomaly_motion_bind_layout as Layout, rsf_anomaly_motion_bind_platform as NativePlatform,
    rsf_anomaly_motion_bind_state as State,
};
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;

type Target = *mut c_void;

#[repr(C)]
pub(crate) struct Platform {
    read: unsafe extern "C" fn(Target, *const c_void, Target, usize) -> u32,
    write: unsafe extern "C" fn(Target, Target, *const c_void, usize) -> u32,
    set_targets: unsafe extern "C" fn(Target, *const Target, Target),
    clear_targets: unsafe extern "C" fn(Target),
}

const _: () = {
    assert!(size_of::<Platform>() == size_of::<NativePlatform>());
    assert!(align_of::<Platform>() == align_of::<NativePlatform>());
    assert!(std::mem::offset_of!(Platform, read) == std::mem::offset_of!(NativePlatform, read));
    assert!(std::mem::offset_of!(Platform, write) == std::mem::offset_of!(NativePlatform, write));
    assert!(
        std::mem::offset_of!(Platform, set_targets)
            == std::mem::offset_of!(NativePlatform, set_targets)
    );
    assert!(
        std::mem::offset_of!(Platform, clear_targets)
            == std::mem::offset_of!(NativePlatform, clear_targets)
    );
};

fn cache_available(state: &State, backend: *mut u8, host: Target) -> bool {
    state.cache_may_hold_motion == 0 || (state.backend == backend.cast() && state.host == host)
}

struct Cache<'a> {
    platform: &'a Platform,
    host: *mut c_void,
    slot_two: *mut u8,
    changed: *mut u8,
}

enum WriteResult {
    Applied,
    Refused,
    UnknownSlot,
}

impl<'a> Cache<'a> {
    fn new(platform: &'a Platform, host: *mut c_void, layout: &Layout, backend: *mut u8) -> Self {
        Self {
            platform,
            host,
            slot_two: backend.wrapping_add(layout.rt_offset as usize + 2 * size_of::<Target>()),
            changed: backend.wrapping_add(layout.changed_offset as usize),
        }
    }

    fn read<T: Copy>(&self, address: *const u8) -> Option<T> {
        // Cache reads use only raw pointers and bytes, which admit any bit pattern.
        let mut value = MaybeUninit::<T>::uninit();
        let accepted = unsafe {
            (self.platform.read)(
                self.host,
                address.cast(),
                value.as_mut_ptr().cast(),
                size_of::<T>(),
            )
        };
        (accepted != 0).then(|| unsafe { value.assume_init() })
    }

    fn write<T>(&self, address: *mut u8, value: T) -> bool {
        unsafe {
            (self.platform.write)(
                self.host,
                address.cast(),
                ptr::from_ref(&value).cast(),
                size_of::<T>(),
            ) != 0
        }
    }

    fn replace(
        &self,
        original: Target,
        target: Target,
        changed_before: u8,
        changed_after: u8,
    ) -> WriteResult {
        // The engine must accept the dirty-byte update before its cache can
        // contain a borrowed RTV. A refused byte write never reaches the slot.
        let change_flag = changed_before != changed_after;
        if change_flag && !self.write(self.changed, changed_after) {
            return WriteResult::Refused;
        }
        if self.write(self.slot_two, target) {
            return WriteResult::Applied;
        }
        let restored = self.write(self.slot_two, original);
        if change_flag {
            self.write(self.changed, changed_before);
        }
        if restored {
            WriteResult::Refused
        } else {
            WriteResult::UnknownSlot
        }
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_motion_bind_draw(
    state: &mut State,
    platform: &Platform,
    host: *mut c_void,
    layout: &Layout,
    backend: *mut u8,
    motion_wanted: u32,
    position_rtv: *const c_void,
    motion_rtv: Target,
) -> i32 {
    state.bound = 0;
    if !cache_available(state, backend, host) {
        return FAULT;
    }
    if backend.is_null() || motion_rtv.is_null() {
        return if state.cache_may_hold_motion != 0 {
            FAULT
        } else {
            NONE
        };
    }
    let cache = Cache::new(platform, host, layout, backend);
    let Some(mut targets) =
        cache.read::<[Target; 4]>(backend.wrapping_add(layout.rt_offset as usize))
    else {
        return FAULT;
    };
    let original = targets[2];
    state.backend = backend.cast();
    state.host = host;
    state.cache_may_hold_motion = u32::from(original == motion_rtv);
    let Some(depth) = cache.read::<Target>(backend.wrapping_add(layout.zb_offset as usize)) else {
        return FAULT;
    };
    let gbuffer =
        motion_wanted != 0 && !position_rtv.is_null() && ptr::eq(position_rtv, targets[0]);
    let target = if gbuffer {
        if original == motion_rtv {
            state.bound = 1;
            return ALREADY_BOUND;
        }
        if !original.is_null() {
            return CONFLICT;
        }
        motion_rtv
    } else if original == motion_rtv {
        ptr::null_mut()
    } else {
        return NONE;
    };
    let Some(changed) = cache.read::<u8>(cache.changed) else {
        return FAULT;
    };
    match cache.replace(original, target, changed, 0) {
        WriteResult::Applied => {}
        WriteResult::Refused => return FAULT,
        WriteResult::UnknownSlot => {
            state.cache_may_hold_motion = 1;
            return FAULT;
        }
    }
    targets[2] = target;
    unsafe { (platform.set_targets)(host, targets.as_ptr(), depth) };
    state.bound = u32::from(gbuffer);
    state.cache_may_hold_motion = state.bound;
    if gbuffer { BOUND } else { UNBOUND }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_motion_bind_close(
    state: &mut State,
    platform: &Platform,
    host: *mut c_void,
    layout: &Layout,
    backend: *mut u8,
    motion_rtv: Target,
) -> i32 {
    state.bound = 0;
    if state.cache_may_hold_motion != 0 && state.backend != backend.cast() {
        return FAULT;
    }
    if backend.is_null() || motion_rtv.is_null() {
        return if state.cache_may_hold_motion != 0 {
            FAULT
        } else {
            NONE
        };
    }
    let cache = Cache::new(platform, host, layout, backend);
    let Some(slot) = cache.read::<Target>(cache.slot_two) else {
        return FAULT;
    };
    if slot != motion_rtv {
        state.cache_may_hold_motion = 0;
        return NONE;
    }
    // A new immediate context can prove the original slot is empty, but cannot
    // clear an OM binding on the original context if its RTV is still cached.
    if state.cache_may_hold_motion != 0 && state.host != host {
        return FAULT;
    }
    state.backend = backend.cast();
    state.host = host;
    state.cache_may_hold_motion = 1;
    let Some(changed) = cache.read::<u8>(cache.changed) else {
        return FAULT;
    };
    if !matches!(
        cache.replace(slot, ptr::null_mut(), changed, 1),
        WriteResult::Applied
    ) {
        return FAULT;
    }
    state.cache_may_hold_motion = 0;
    if changed == 0 {
        unsafe { (platform.clear_targets)(host) };
    }
    UNBOUND
}
