// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use std::borrow::Borrow;
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

const SLOT_COUNT: usize = 16;
const CACHE_CAPACITY: usize = 32;

type Slots<T> = [Option<T>; SLOT_COUNT];
type Entries<T> = [Option<Entry<T>>; CACHE_CAPACITY];

unsafe extern "C" {
    fn rsf_sampler_bias_com_is_immediate(context: *mut c_void) -> i32;
    fn rsf_sampler_bias_com_device(child: *mut c_void) -> *mut c_void;
    fn rsf_sampler_bias_com_read_slots(context: *mut c_void, out: *mut *mut c_void);
    fn rsf_sampler_bias_com_write_slot(context: *mut c_void, slot: u32, sampler: *mut c_void);
    fn rsf_sampler_bias_com_read_bias(sampler: *mut c_void) -> f32;
    fn rsf_sampler_bias_com_clone(
        device: *mut c_void,
        original: *mut c_void,
        bias: f32,
    ) -> *mut c_void;
    fn rsf_sampler_bias_com_device_ready(device: *mut c_void) -> i32;
}

struct Entry<T> {
    original: T,
    replacement: T,
    bias: f32,
}

enum Binding<T, K> {
    Keep,
    Restore(Option<K>),
    Replace(T),
}

struct Update<T, K> {
    entries: Entries<T>,
    next: usize,
    bindings: [Binding<T, K>; SLOT_COUNT],
    applied_mask: u32,
}

struct Cache<T> {
    entries: Entries<T>,
    // Bound replacements keep a separate reference across cache eviction.
    owned: Slots<T>,
    next: usize,
}

impl<T: Clone> Cache<T> {
    fn new() -> Self {
        Self {
            entries: [const { None }; CACHE_CAPACITY],
            owned: [const { None }; SLOT_COUNT],
            next: 0,
        }
    }

    fn prepare<K: Copy + Eq>(
        &self,
        current: &Slots<T>,
        baselines: &Slots<K>,
        slot_mask: u32,
        bias: f32,
        create: impl Fn(K) -> Option<(T, T)>,
    ) -> Option<Update<T, K>>
    where
        T: Borrow<K>,
    {
        let mut update = Update {
            entries: [const { None }; CACHE_CAPACITY],
            next: self.next,
            bindings: [const { Binding::Keep }; SLOT_COUNT],
            applied_mask: 0,
        };
        for (slot, (&baseline, current)) in baselines.iter().zip(current).enumerate() {
            let current = current.as_ref().map(|sampler| *sampler.borrow());
            let owned = self.owned[slot].as_ref().map(|sampler| *sampler.borrow());
            if current != baseline && (owned.is_none() || current != owned) {
                continue;
            }

            let bit = 1 << slot;
            let Some(original) = baseline.filter(|_| slot_mask & bit != 0 && bias != 0.0) else {
                update.bindings[slot] = Binding::Restore(baseline);
                continue;
            };

            // Staged entries shadow their eviction targets and can be reused within this call.
            let cached = update
                .entries
                .iter()
                .zip(&self.entries)
                .filter_map(|(staged, cached)| staged.as_ref().or(cached.as_ref()))
                .find(|entry| *entry.original.borrow() == original && entry.bias == bias);
            let replacement = if let Some(entry) = cached {
                entry.replacement.clone()
            } else {
                let (original, replacement) = create(original)?;
                let selected = replacement.clone();
                update.entries[update.next] = Some(Entry {
                    original,
                    replacement,
                    bias,
                });
                update.next = (update.next + 1) % CACHE_CAPACITY;
                selected
            };
            update.bindings[slot] = Binding::Replace(replacement);
            update.applied_mask |= bit;
        }
        Some(update)
    }

    fn commit<K>(&mut self, update: Update<T, K>) {
        for (cached, staged) in self.entries.iter_mut().zip(update.entries) {
            if staged.is_some() {
                *cached = staged;
            }
        }
        self.next = update.next;
        self.owned = update.bindings.map(|binding| match binding {
            Binding::Replace(replacement) => Some(replacement),
            _ => None,
        });
    }
}

struct Owner {
    context: *mut c_void,
    thread: u32,
    device: ComRef,
}

struct SamplerBias {
    cache: Cache<ComRef>,
    owner: Option<Owner>,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sampler_bias_create() -> *mut SamplerBias {
    crate::allocate_handle(SamplerBias {
        cache: Cache::new(),
        owner: None,
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_sampler_bias_destroy(handle: *mut SamplerBias) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

// The facade provides exclusive access, live COM pointers and SLOT_COUNT borrowed baselines.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_sampler_bias_apply(
    handle: *mut SamplerBias,
    context: *mut c_void,
    baselines: *const *mut c_void,
    slot_mask: u32,
    bias: f32,
    applied_mask: *mut u32,
) -> i32 {
    let Some(applied_mask) = (unsafe { applied_mask.as_mut() }) else {
        return 0;
    };
    *applied_mask = 0;
    if context.is_null() || slot_mask >> SLOT_COUNT != 0 || !(-16.0..=16.0).contains(&bias) {
        return 0;
    }
    let Some(state) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some(baselines) = (unsafe { baselines.cast::<[*mut c_void; SLOT_COUNT]>().as_ref() })
    else {
        return 0;
    };
    let thread = unsafe { GetCurrentThreadId() };
    if state
        .owner
        .as_ref()
        .is_some_and(|owner| owner.context != context || owner.thread != thread)
        || unsafe { rsf_sampler_bias_com_is_immediate(context) } == 0
    {
        return 0;
    }
    let Some(device) = NonNull::new(unsafe { rsf_sampler_bias_com_device(context) }).map(ComRef)
    else {
        return 0;
    };
    if state
        .owner
        .as_ref()
        .is_some_and(|owner| owner.device.0 != device.0)
    {
        return 0;
    }

    let baselines = baselines.map(NonNull::new);
    for sampler in baselines.iter().flatten() {
        let owner = NonNull::new(unsafe { rsf_sampler_bias_com_device(sampler.as_ptr()) });
        if owner.map(ComRef).is_none_or(|owner| owner.0 != device.0) {
            return 0;
        }
    }

    let mut current = [ptr::null_mut(); SLOT_COUNT];
    unsafe { rsf_sampler_bias_com_read_slots(context, current.as_mut_ptr()) };
    let current = current.map(|sampler| NonNull::new(sampler).map(ComRef));
    let device_ptr = device.0.as_ptr();
    let Some(update) = state
        .cache
        .prepare(&current, &baselines, slot_mask, bias, |original| {
            let original_ptr = original.as_ptr();
            let adjusted = unsafe { rsf_sampler_bias_com_read_bias(original_ptr) } + bias;
            if !(-16.0..=15.99).contains(&adjusted) {
                return None;
            }
            let replacement = NonNull::new(unsafe {
                rsf_sampler_bias_com_clone(device_ptr, original_ptr, adjusted)
            })?;
            Some((unsafe { ComRef::retain(original_ptr) }, ComRef(replacement)))
        })
    else {
        return 0;
    };
    if unsafe { rsf_sampler_bias_com_device_ready(device_ptr) } == 0 {
        return 0;
    }

    for (slot, (binding, current)) in update.bindings.iter().zip(&current).enumerate() {
        let selected = match binding {
            Binding::Keep => continue,
            Binding::Restore(baseline) => *baseline,
            Binding::Replace(replacement) => Some(replacement.0),
        };
        if selected != current.as_ref().map(|sampler| sampler.0) {
            let selected = selected.map_or(ptr::null_mut(), NonNull::as_ptr);
            unsafe { rsf_sampler_bias_com_write_slot(context, slot as u32, selected) };
        }
    }
    *applied_mask = update.applied_mask;
    state.cache.commit(update);
    state.owner.get_or_insert(Owner {
        context,
        thread,
        device,
    });
    // PSSetSamplers is void. If the device was lost during commit, report it and
    // retain the ownership record for the bindings this call attempted.
    unsafe { rsf_sampler_bias_com_device_ready(device_ptr) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_clone_failure_preserves_installed_ownership_and_cache() {
        let mut cache = Cache::<u32>::new();
        let mut baselines = [None; SLOT_COUNT];
        baselines[..2].copy_from_slice(&[Some(1), Some(2)]);
        let initial = cache
            .prepare(&baselines, &baselines, 1, -1.0, |original| {
                Some((original, 3))
            })
            .unwrap();
        cache.commit(initial);
        let mut current = baselines;
        current[0] = Some(3);

        let failed = cache.prepare(&current, &baselines, 3, -2.0, |original| {
            (original == 1).then_some((original, 4))
        });
        assert!(failed.is_none());
        assert_eq!(cache.owned[0], Some(3));
        assert_eq!(cache.next, 1);
        assert!(cache.entries[1].is_none());

        let drain = cache
            .prepare(&current, &baselines, 0, 0.0, |_| unreachable!())
            .unwrap();
        assert!(matches!(drain.bindings[0], Binding::Restore(Some(1))));
        assert_eq!(drain.applied_mask, 0);
    }
}
