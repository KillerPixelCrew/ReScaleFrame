// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;

pub(super) type Handle = *mut c_void;
pub(super) type MakeView = unsafe extern "C" fn(Handle, Handle, u32) -> Handle;
pub(super) const GROUP_EDGE: u32 = 8;
const CACHE_IDLE_MS: u64 = 2000;

#[repr(C)]
#[derive(Default)]
pub(super) struct SourceInfo {
    pub width: u32,
    pub height: u32,
    pub slices: u32,
    pub samples: u32,
    pub format: u32,
    pub view_format: u32,
}

const _: () = assert!(size_of::<SourceInfo>() == 24);

impl SourceInfo {
    pub(super) fn single_surface(&self) -> bool {
        self.slices == 1 && self.samples == 1
    }
}

struct Entry {
    view: ComRef,
    texture: ComRef,
    format: u32,
    used: u64,
}

pub(super) struct SourceViews<const N: usize> {
    entries: [Option<Entry>; N],
}

impl<const N: usize> SourceViews<N> {
    pub(super) const fn new() -> Self {
        Self {
            entries: [const { None }; N],
        }
    }

    // The device and texture must be live native interfaces for the call.
    pub(super) unsafe fn get(
        &mut self,
        device: Handle,
        texture: Handle,
        format: u32,
        now: u64,
        make: MakeView,
    ) -> Option<&ComRef> {
        for entry in &mut self.entries {
            if entry
                .as_ref()
                .is_some_and(|entry| now.wrapping_sub(entry.used) > CACHE_IDLE_MS)
            {
                *entry = None;
            }
        }
        if let Some(index) = self.entries.iter().position(|entry| {
            entry
                .as_ref()
                .is_some_and(|entry| entry.texture.as_ptr() == texture && entry.format == format)
        }) {
            let entry = self.entries[index].as_mut()?;
            entry.used = now;
            return Some(&entry.view);
        }
        let index = self.entries.iter().position(Option::is_none).or_else(|| {
            self.entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| Some((index, entry.as_ref()?.used)))
                .min_by_key(|&(_, used)| used)
                .map(|(index, _)| index)
        })?;
        let view = unsafe { ComRef::from_owned(make(device, texture, format)) }?;
        self.entries[index] = Some(Entry {
            view,
            texture: unsafe { ComRef::retain(texture) },
            format,
            used: now,
        });
        self.entries[index].as_ref().map(|entry| &entry.view)
    }
}

// The ABI starts with struct_size. The caller guarantees that the indicated
// prefix is readable and that zeroed extension fields are valid for T.
pub(super) unsafe fn copy_params<T>(input: *const T, prefix: usize) -> Option<T> {
    if input.is_null() || prefix > size_of::<T>() {
        return None;
    }
    let size = unsafe { input.cast::<u32>().read() } as usize;
    if size < prefix {
        return None;
    }
    let bytes = if size >= size_of::<T>() {
        size_of::<T>()
    } else {
        prefix
    };
    let mut result = MaybeUninit::<T>::zeroed();
    unsafe {
        ptr::copy_nonoverlapping(input.cast::<u8>(), result.as_mut_ptr().cast::<u8>(), bytes);
        Some(result.assume_init())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{fake, raw};

    unsafe extern "C" fn make_view(_: Handle, texture: Handle, _: u32) -> Handle {
        unsafe { ComRef::retain(texture) }.into_raw()
    }

    #[test]
    fn a_selected_view_outlives_its_cache_slot() {
        let (device, first, second) = (fake(), fake(), fake());
        let mut cache = SourceViews::<1>::new();
        let held = unsafe { cache.get(raw(&device), raw(&first), 0, 1, make_view) }
            .cloned()
            .unwrap();
        assert!(unsafe { cache.get(raw(&device), raw(&second), 0, 1, make_view) }.is_some());
        assert_eq!(held.as_ptr(), raw(&first));
        assert_eq!(first.refs.get(), 2);
        drop(held);
        assert_eq!(first.refs.get(), 1);
    }
}
