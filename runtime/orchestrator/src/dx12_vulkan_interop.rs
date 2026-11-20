// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::dx12_vulkan_interop_abi::*;
use crate::sr_session::has_capacity;
use crate::vulkan_sr_abi::{RSF_SRVK_ABI_VERSION, rsf_srvk_setup};
use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

const OUT_OF_HOST_MEMORY: i32 = -1;
const INITIALIZATION_FAILED: i32 = -3;
const INCOMPATIBLE_DRIVER: i32 = -9;
const TOO_MANY_OBJECTS: i32 = -10;
const FORMAT_NOT_SUPPORTED: i32 = -11;
const INVALID_EXTERNAL_HANDLE: i32 = -1_000_072_003;
const BGRA8_UNORM: u32 = 44;
const RG16_FLOAT: u32 = 83;
const R32_FLOAT: u32 = 100;
const IMAGE_SLOTS: usize = 24;

type Handle = *mut c_void;
type Callbacks = rsf_dx12vk_interop_callbacks;
type Platform = rsf_dx12vk_platform;
type Output = rsf_dx12vk_shared_image;
const INVALID: i32 = RSF_DX12VK_INTEROP_INVALID_ARGUMENT;
const ABI_MISMATCH: i32 = RSF_DX12VK_INTEROP_ABI_MISMATCH;

fn checked(result: i32) -> Result<(), i32> {
    if result == 0 { Ok(()) } else { Err(result) }
}

// Win32 imports retain the payload, so the NT handle always remains ours.
unsafe fn owned_handle(raw: Handle) -> Option<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return None;
    }
    Some(unsafe { OwnedHandle::from_raw_handle(raw) })
}

#[derive(Default)]
struct Image {
    image: u64,
    memory: u64,
    dx12: Handle,
}

struct Owner {
    platform: &'static Platform,
    native: Handle,
    callbacks: Callbacks,
    timeline: u64,
    images: [Option<Image>; IMAGE_SLOTS],
}

impl Owner {
    fn release(&self, image: &Image) {
        unsafe {
            if image.image != 0 {
                (self.platform.destroy_image.unwrap())(self.native, image.image);
            }
            if image.memory != 0 {
                (self.platform.free_memory.unwrap())(self.native, image.memory);
            }
            if !image.dx12.is_null() {
                (self.callbacks.release_image.unwrap())(image.dx12);
            }
        }
    }

    fn import_timeline(&mut self) -> Result<(), i32> {
        let platform = self.platform;
        if unsafe { (platform.timeline_importable.unwrap())(self.native) } == 0 {
            return Err(INVALID_EXTERNAL_HANDLE);
        }
        checked(unsafe { (platform.create_timeline.unwrap())(self.native, &mut self.timeline) })?;
        if self.timeline == 0 {
            return Err(INITIALIZATION_FAILED);
        }
        let mut raw = ptr::null_mut();
        let result =
            unsafe { (self.callbacks.create_fence.unwrap())(self.callbacks.user, &mut raw) };
        let handle = unsafe { owned_handle(raw) };
        checked(result)?;
        let handle = handle.ok_or(INVALID_EXTERNAL_HANDLE)?;
        checked(unsafe {
            (platform.import_timeline.unwrap())(self.native, self.timeline, handle.as_raw_handle())
        })
    }

    fn import_image(&self, format: u32, width: u32, height: u32) -> Result<Image, i32> {
        let platform = self.platform;
        let (mut importable, mut max_width, mut max_height) = (0, 0, 0);
        checked(unsafe {
            (platform.image_properties.unwrap())(
                self.native,
                format,
                &mut importable,
                &mut max_width,
                &mut max_height,
            )
        })?;
        if importable == 0 || width > max_width || height > max_height {
            return Err(FORMAT_NOT_SUPPORTED);
        }

        let mut pending = PendingImage {
            owner: self,
            image: Image::default(),
        };
        let mut raw = ptr::null_mut();
        let result = unsafe {
            (self.callbacks.create_image.unwrap())(
                self.callbacks.user,
                width,
                height,
                format,
                &mut raw,
                &mut pending.image.dx12,
            )
        };
        let handle = unsafe { owned_handle(raw) };
        checked(result)?;
        let handle = handle.ok_or(INVALID_EXTERNAL_HANDLE)?;
        if pending.image.dx12.is_null() {
            return Err(INVALID_EXTERNAL_HANDLE);
        }
        checked(unsafe {
            (platform.create_image.unwrap())(
                self.native,
                format,
                width,
                height,
                &mut pending.image.image,
            )
        })?;
        if pending.image.image == 0 {
            return Err(INITIALIZATION_FAILED);
        }
        let (mut bytes, mut image_types, mut handle_types) = (0, 0, 0);
        unsafe {
            (platform.image_requirements.unwrap())(
                self.native,
                pending.image.image,
                &mut bytes,
                &mut image_types,
            )
        };
        checked(unsafe {
            (platform.handle_memory_types.unwrap())(
                self.native,
                handle.as_raw_handle(),
                &mut handle_types,
            )
        })?;
        let memory_types = image_types
            & handle_types
            & unsafe { (platform.device_local_types.unwrap())(self.native) };
        if memory_types == 0 {
            return Err(INCOMPATIBLE_DRIVER);
        }
        checked(unsafe {
            (platform.allocate_memory.unwrap())(
                self.native,
                pending.image.image,
                handle.as_raw_handle(),
                bytes,
                memory_types.trailing_zeros(),
                &mut pending.image.memory,
            )
        })?;
        if pending.image.memory == 0 {
            return Err(INITIALIZATION_FAILED);
        }
        checked(unsafe {
            (platform.bind_image.unwrap())(self.native, pending.image.image, pending.image.memory)
        })?;
        Ok(std::mem::take(&mut pending.image))
    }
}

struct PendingImage<'a> {
    owner: &'a Owner,
    image: Image,
}

impl Drop for PendingImage<'_> {
    fn drop(&mut self) {
        self.owner.release(&self.image);
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        for image in self.images.iter().flatten() {
            self.release(image);
        }
        unsafe {
            if self.timeline != 0 {
                (self.platform.destroy_timeline.unwrap())(self.native, self.timeline);
            }
            (self.platform.close.unwrap())(self.native);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dx12vk_interop_create(
    platform: &'static Platform,
    setup: *const rsf_srvk_setup,
    callbacks: *const Callbacks,
    timeline_enabled: u32,
    out: *mut *mut Owner,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    if timeline_enabled != 1 || !unsafe { has_capacity(setup) && has_capacity(callbacks) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    let callbacks = unsafe { &*callbacks };
    if callbacks.adapter_luid == 0
        || callbacks.create_image.is_none()
        || callbacks.release_image.is_none()
        || callbacks.create_fence.is_none()
        || setup.instance.is_null()
        || setup.physical_device.is_null()
        || setup.device.is_null()
        || setup.instance_proc.is_none()
        || setup.device_proc.is_none()
    {
        return INVALID;
    }
    if setup.abi_version != RSF_SRVK_ABI_VERSION
        || callbacks.abi_version != RSF_DX12VK_INTEROP_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    let mut luid = 0;
    let result = unsafe { (platform.adapter_luid.unwrap())(setup, &mut luid) };
    if result != 0 {
        return result;
    }
    if luid != callbacks.adapter_luid {
        return INVALID_EXTERNAL_HANDLE;
    }
    let mut native = ptr::null_mut();
    let result = unsafe { (platform.open.unwrap())(setup, &mut native) };
    if result != 0 {
        return result;
    }
    if native.is_null() {
        return INITIALIZATION_FAILED;
    }
    let raw = allocate_handle(Owner {
        platform,
        native,
        callbacks: *callbacks,
        timeline: 0,
        images: std::array::from_fn(|_| None),
    });
    if raw.is_null() {
        return OUT_OF_HOST_MEMORY;
    }
    let mut owner = unsafe { Box::from_raw(raw) };
    if let Err(error) = owner.import_timeline() {
        return error;
    }
    unsafe { out.write(Box::into_raw(owner)) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dx12vk_interop_timeline(owner: *const Owner) -> u64 {
    unsafe { owner.as_ref() }.map_or(0, |owner| owner.timeline)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dx12vk_interop_image(
    owner: *mut Owner,
    format: u32,
    width: u32,
    height: u32,
    out: *mut Output,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe {
        out.write(Output {
            image: 0,
            dx12_resource: ptr::null_mut(),
            format: 0,
            width: 0,
            height: 0,
        })
    };
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    if width == 0 || height == 0 || !matches!(format, BGRA8_UNORM | R32_FLOAT | RG16_FLOAT) {
        return INVALID;
    }
    let Some(index) = owner.images.iter().position(Option::is_none) else {
        return TOO_MANY_OBJECTS;
    };
    let image = match owner.import_image(format, width, height) {
        Ok(image) => image,
        Err(error) => return error,
    };
    unsafe {
        out.write(Output {
            image: image.image,
            dx12_resource: image.dx12,
            format,
            width,
            height,
        })
    };
    owner.images[index] = Some(image);
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dx12vk_interop_destroy(owner: *mut Owner, queues_drained: u32) -> i32 {
    if owner.is_null() {
        return 0;
    }
    if queues_drained != 1 {
        return INVALID;
    }
    unsafe { drop(Box::from_raw(owner)) };
    0
}
