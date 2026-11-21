// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use std::cmp::Reverse;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, MutexGuard, PoisonError};

type Handle = *mut c_void;
type Identity = *const c_void;
const CAPACITY: usize = 32;

#[repr(C)]
struct Platform {
    owns_context: unsafe extern "C" fn(Handle, Handle) -> u32,
    make_buffer: unsafe extern "C" fn(Handle, u32) -> Handle,
    upload: unsafe extern "C" fn(Handle, Handle, *const c_void, u32) -> u32,
    matches_size: unsafe extern "C" fn(Identity, u32) -> u32,
}

const _: () = assert!(size_of::<Platform>() == 4 * size_of::<Handle>());

#[derive(Default)]
struct Slot {
    // Never retain the original. It is an address in the caller's buffer pool.
    original: Identity,
    buffer: Option<ComRef>,
    written: u64,
}

#[derive(Default)]
struct Cache {
    slots: [Slot; CAPACITY],
    clock: u64,
    upload: Option<Identity>,
}

impl Cache {
    fn index(&self, original: Identity) -> Option<usize> {
        self.slots.iter().position(|slot| slot.original == original)
    }

    fn slot_for_write(&self, original: Identity) -> usize {
        self.index(original).unwrap_or_else(|| {
            self.slots
                .iter()
                .enumerate()
                .min_by_key(|(_, slot)| {
                    (
                        !slot.original.is_null(),
                        Reverse(self.clock.wrapping_sub(slot.written)),
                    )
                })
                .map_or(0, |(index, _)| index)
        })
    }
}

struct Twins {
    cache: Mutex<Cache>,
    device: ComRef,
    bytes: u32,
    platform: &'static Platform,
}

impl Twins {
    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(
        &self,
        context: Handle,
        original: Identity,
        contents: *const c_void,
    ) -> Option<Handle> {
        if context.is_null()
            || original.is_null()
            || contents.is_null()
            || unsafe { (self.platform.owns_context)(self.device.as_ptr(), context) } == 0
        {
            return None;
        }

        let (index, buffer) = {
            let mut cache = self.cache();
            if cache.upload.is_some() {
                return None;
            }
            let index = cache.slot_for_write(original);
            let slot = &mut cache.slots[index];
            slot.original = null();
            slot.written = 0;
            let buffer = slot.buffer.take();
            cache.upload = Some(original);
            (index, buffer)
        };

        // Map and Unmap can enter the frame tap. Hide this slot until the upload publishes.
        let buffer = buffer.or_else(|| unsafe {
            ComRef::from_owned((self.platform.make_buffer)(
                self.device.as_ptr(),
                self.bytes,
            ))
        });
        let uploaded = buffer.as_ref().is_some_and(|buffer| unsafe {
            (self.platform.upload)(context, buffer.as_ptr(), contents, self.bytes) != 0
        });

        let mut cache = self.cache();
        let publish = cache.upload.take() == Some(original) && uploaded;
        cache.slots[index].buffer = buffer;
        if !publish {
            return None;
        }
        let written = cache.clock.wrapping_add(1);
        cache.clock = written;
        let slot = &mut cache.slots[index];
        slot.original = original;
        slot.written = written;
        slot.buffer.as_ref().map(ComRef::as_ptr)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_constant_twins_create(
    device: Handle,
    bytes: u32,
    platform: &'static Platform,
) -> *mut Twins {
    if device.is_null() || bytes == 0 || !bytes.is_multiple_of(16) {
        return null_mut();
    }
    crate::allocate_handle(Twins {
        cache: Mutex::default(),
        device: unsafe { ComRef::retain(device) },
        bytes,
        platform,
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_constant_twins_destroy(twins: *mut Twins) {
    if !twins.is_null() {
        drop(unsafe { Box::from_raw(twins) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_constant_twins_write(
    twins: *const Twins,
    context: Handle,
    original: Identity,
    contents: *const c_void,
) -> Handle {
    unsafe { twins.as_ref() }
        .and_then(|twins| twins.write(context, original, contents))
        .unwrap_or(null_mut())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_constant_twins_find(
    twins: *const Twins,
    original: Identity,
) -> Handle {
    let Some(twins) = (unsafe { twins.as_ref() }).filter(|_| !original.is_null()) else {
        return null_mut();
    };
    let cache = twins.cache();
    let Some(index) = cache.index(original) else {
        return null_mut();
    };
    // A reused address can now name a differently sized buffer.
    if unsafe { (twins.platform.matches_size)(original, twins.bytes) } == 0 {
        return null_mut();
    }
    cache.slots[index]
        .buffer
        .as_ref()
        .map_or(null_mut(), ComRef::as_ptr)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_constant_twins_forget(twins: *const Twins, original: Identity) {
    let Some(twins) = (unsafe { twins.as_ref() }).filter(|_| !original.is_null()) else {
        return;
    };
    let mut cache = twins.cache();
    if cache.upload == Some(original) {
        // Keep the slot checked out, but a completed upload must no longer publish it.
        cache.upload = Some(null());
    }
    if let Some(index) = cache.index(original) {
        cache.slots[index].original = null();
        cache.slots[index].written = 0;
    }
}
