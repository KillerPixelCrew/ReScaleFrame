// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::owned_win32_handle;
use crate::sr_session::has_capacity;
use crate::sys::*;
use crate::sys::{RSF_SRVK_ABI_VERSION, rsf_srvk_setup};
use ash::vk;
use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::ptr;

const OUT_OF_HOST_MEMORY: i32 = -1;
const INITIALIZATION_FAILED: i32 = -3;
const INCOMPATIBLE_DRIVER: i32 = -9;
const TOO_MANY_OBJECTS: i32 = -10;
const FORMAT_NOT_SUPPORTED: i32 = vk::Result::ERROR_FORMAT_NOT_SUPPORTED.as_raw();
const INVALID_EXTERNAL_HANDLE: i32 = vk::Result::ERROR_INVALID_EXTERNAL_HANDLE.as_raw();
pub(super) const BGRA8_UNORM: u32 = vk::Format::B8G8R8A8_UNORM.as_raw() as u32;
pub(super) const RG16_FLOAT: u32 = vk::Format::R16G16_SFLOAT.as_raw() as u32;
pub(super) const R32_FLOAT: u32 = vk::Format::R32_SFLOAT.as_raw() as u32;
const IMAGE_SLOTS: usize = 24;

type Handle = *mut c_void;
type Callbacks = rsf_dx12vk_interop_callbacks;
/// The Vulkan leaves of the importer: `dx12_vulkan_platform::PLATFORM` in production, fakes in
/// the FSR4 SR state fixture.
#[derive(Clone, Copy)]
pub(crate) struct Platform {
    pub(crate) adapter_luid: unsafe extern "C" fn(*const rsf_srvk_setup, *mut u64) -> i32,
    pub(crate) open: unsafe extern "C" fn(*const rsf_srvk_setup, *mut *mut c_void) -> i32,
    pub(crate) close: unsafe extern "C" fn(*mut c_void),
    pub(crate) timeline_importable: unsafe extern "C" fn(*mut c_void) -> u32,
    pub(crate) create_timeline: unsafe extern "C" fn(*mut c_void, *mut u64) -> i32,
    pub(crate) import_timeline: unsafe extern "C" fn(*mut c_void, u64, *mut c_void) -> i32,
    pub(crate) destroy_timeline: unsafe extern "C" fn(*mut c_void, u64),
    pub(crate) image_properties: unsafe extern "C" fn(
        *mut c_void,
        format: u32,
        extra_usage: u32,
        importable: *mut u32,
        max_width: *mut u32,
        max_height: *mut u32,
    ) -> i32,
    pub(crate) create_image: unsafe extern "C" fn(
        *mut c_void,
        format: u32,
        width: u32,
        height: u32,
        extra_usage: u32,
        *mut u64,
    ) -> i32,
    pub(crate) image_requirements:
        unsafe extern "C" fn(*mut c_void, image: u64, bytes: *mut u64, types: *mut u32),
    pub(crate) handle_memory_types:
        unsafe extern "C" fn(*mut c_void, handle: *mut c_void, *mut u32) -> i32,
    pub(crate) device_local_types: unsafe extern "C" fn(*mut c_void) -> u32,
    pub(crate) allocate_memory: unsafe extern "C" fn(
        *mut c_void,
        image: u64,
        handle: *mut c_void,
        bytes: u64,
        type_: u32,
        *mut u64,
    ) -> i32,
    pub(crate) bind_image: unsafe extern "C" fn(*mut c_void, image: u64, memory: u64) -> i32,
    pub(crate) destroy_image: unsafe extern "C" fn(*mut c_void, u64),
    pub(crate) free_memory: unsafe extern "C" fn(*mut c_void, u64),
}

pub(super) type FenceFactory = unsafe extern "C" fn(Handle, *mut Handle) -> i32;
pub(super) type ImageRelease = unsafe extern "C" fn(Handle);
#[derive(Clone, Copy)]
struct LegacyImageFactory {
    user: Handle,
    create_image: unsafe extern "C" fn(
        user: *mut c_void,
        width: u32,
        height: u32,
        format: u32,
        native_handle: *mut *mut c_void,
        resource: *mut *mut c_void,
    ) -> i32,
}

type Output = rsf_dx12vk_shared_image;
const INVALID: i32 = RSF_DX12VK_INTEROP_INVALID_ARGUMENT;
const ABI_MISMATCH: i32 = RSF_DX12VK_INTEROP_ABI_MISMATCH;

pub(super) fn checked(result: i32) -> Result<(), i32> {
    if result == 0 { Ok(()) } else { Err(result) }
}

#[cfg(test)]
mod tests;

#[derive(Default)]
struct Image {
    image: u64,
    memory: u64,
    dx12: Handle,
}

pub(super) struct Owner {
    platform: Platform,
    native: Handle,
    release_image: ImageRelease,
    legacy: Option<LegacyImageFactory>,
    timeline: u64,
    images: [Option<Image>; IMAGE_SLOTS],
}

impl Owner {
    pub(super) fn device_luid(&self, setup: &rsf_srvk_setup) -> Result<u64, i32> {
        Self::adapter_luid(&self.platform, setup)
    }

    pub(super) fn adapter_luid(platform: &Platform, setup: &rsf_srvk_setup) -> Result<u64, i32> {
        let mut luid = 0;
        checked(unsafe { (platform.adapter_luid)(setup, &mut luid) })?;
        Ok(luid)
    }

    pub(super) fn open(
        platform: Platform,
        setup: &rsf_srvk_setup,
        release_image: ImageRelease,
    ) -> Result<Self, i32> {
        let mut native = ptr::null_mut();
        checked(unsafe { (platform.open)(setup, &mut native) })?;
        if native.is_null() {
            return Err(INITIALIZATION_FAILED);
        }
        Ok(Self {
            platform,
            native,
            release_image,
            legacy: None,
            timeline: 0,
            images: std::array::from_fn(|_| None),
        })
    }

    pub(super) fn timeline(&self) -> u64 {
        self.timeline
    }

    pub(super) fn add_image(
        &mut self,
        format: u32,
        extent: [u32; 2],
        extra_usage: u32,
        create: impl FnOnce(*mut Handle, *mut Handle) -> i32,
    ) -> Result<Output, i32> {
        let index = self
            .images
            .iter()
            .position(Option::is_none)
            .ok_or(TOO_MANY_OBJECTS)?;
        let mut image = Image::default();
        let result = self.import_image(format, extent, extra_usage, &mut image, create);
        if let Err(error) = result
            && self.legacy.is_some()
        {
            // The legacy caller can retry individual imports on a live owner.
            self.release(&image);
            return Err(error);
        }
        let output = Output {
            image: image.image,
            dx12_resource: image.dx12,
            format,
            width: extent[0],
            height: extent[1],
        };
        // A construction transaction owns partial imports through checked cleanup.
        self.images[index] = Some(image);
        result.map(|()| output)
    }

    fn release(&self, image: &Image) {
        unsafe {
            if image.image != 0 {
                (self.platform.destroy_image)(self.native, image.image);
            }
            if image.memory != 0 {
                (self.platform.free_memory)(self.native, image.memory);
            }
            if !image.dx12.is_null() {
                (self.release_image)(image.dx12);
            }
        }
    }

    fn release_images(&mut self, images: [u64; 2]) -> Result<(), i32> {
        // Check both identities before touching either slot. Zero represents
        // a guide that has yet to be allocated after a failed prepare.
        if images[0] != 0 && images[0] == images[1] {
            return Err(INVALID);
        }
        let mut slots = [None; 2];
        for (slot, handle) in slots.iter_mut().zip(images) {
            if handle != 0 {
                *slot = Some(
                    self.images
                        .iter()
                        .position(|image| image.as_ref().is_some_and(|image| image.image == handle))
                        .ok_or(INVALID)?,
                );
            }
        }
        for slot in slots.into_iter().flatten() {
            let image = self.images[slot].take().unwrap();
            self.release(&image);
        }
        Ok(())
    }

    pub(super) fn import_timeline(
        &mut self,
        user: Handle,
        create: FenceFactory,
    ) -> Result<(), i32> {
        let platform = &self.platform;
        if unsafe { (platform.timeline_importable)(self.native) } == 0 {
            return Err(INVALID_EXTERNAL_HANDLE);
        }
        checked(unsafe { (platform.create_timeline)(self.native, &mut self.timeline) })?;
        if self.timeline == 0 {
            return Err(INITIALIZATION_FAILED);
        }
        let mut raw = ptr::null_mut();
        let result = unsafe { create(user, &mut raw) };
        let handle = unsafe { owned_win32_handle(raw) };
        checked(result)?;
        let handle = handle.ok_or(INVALID_EXTERNAL_HANDLE)?;
        checked(unsafe {
            (platform.import_timeline)(self.native, self.timeline, handle.as_raw_handle())
        })
    }

    fn import_image(
        &self,
        format: u32,
        [width, height]: [u32; 2],
        extra_usage: u32,
        image: &mut Image,
        create: impl FnOnce(*mut Handle, *mut Handle) -> i32,
    ) -> Result<(), i32> {
        let platform = &self.platform;
        let (mut importable, mut max_width, mut max_height) = (0, 0, 0);
        checked(unsafe {
            (platform.image_properties)(
                self.native,
                format,
                extra_usage,
                &mut importable,
                &mut max_width,
                &mut max_height,
            )
        })?;
        if importable == 0 || width > max_width || height > max_height {
            return Err(FORMAT_NOT_SUPPORTED);
        }

        let mut raw = ptr::null_mut();
        let result = create(&mut raw, &mut image.dx12);
        let handle = unsafe { owned_win32_handle(raw) };
        checked(result)?;
        let handle = handle.ok_or(INVALID_EXTERNAL_HANDLE)?;
        if image.dx12.is_null() {
            return Err(INVALID_EXTERNAL_HANDLE);
        }
        checked(unsafe {
            (platform.create_image)(
                self.native,
                format,
                width,
                height,
                extra_usage,
                &mut image.image,
            )
        })?;
        if image.image == 0 {
            return Err(INITIALIZATION_FAILED);
        }
        let (mut bytes, mut image_types, mut handle_types) = (0, 0, 0);
        unsafe {
            (platform.image_requirements)(self.native, image.image, &mut bytes, &mut image_types)
        };
        checked(unsafe {
            (platform.handle_memory_types)(self.native, handle.as_raw_handle(), &mut handle_types)
        })?;
        let memory_types =
            image_types & handle_types & unsafe { (platform.device_local_types)(self.native) };
        if memory_types == 0 {
            return Err(INCOMPATIBLE_DRIVER);
        }
        checked(unsafe {
            (platform.allocate_memory)(
                self.native,
                image.image,
                handle.as_raw_handle(),
                bytes,
                memory_types.trailing_zeros(),
                &mut image.memory,
            )
        })?;
        if image.memory == 0 {
            return Err(INITIALIZATION_FAILED);
        }
        checked(unsafe { (platform.bind_image)(self.native, image.image, image.memory) })?;
        Ok(())
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        for image in self.images.iter().flatten() {
            self.release(image);
        }
        unsafe {
            if self.timeline != 0 {
                (self.platform.destroy_timeline)(self.native, self.timeline);
            }
            (self.platform.close)(self.native);
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dx12vk_interop_create(
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
    let (Some(create_image), Some(release_image), Some(create_fence)) = (
        callbacks.create_image,
        callbacks.release_image,
        callbacks.create_fence,
    ) else {
        return INVALID;
    };
    if callbacks.adapter_luid == 0
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
    let platform = crate::dx12_vulkan_platform::PLATFORM;
    let luid = match Owner::adapter_luid(&platform, setup) {
        Ok(luid) => luid,
        Err(error) => return error,
    };
    if luid != callbacks.adapter_luid {
        return INVALID_EXTERNAL_HANDLE;
    }
    let mut owner = match Owner::open(platform, setup, release_image) {
        Ok(owner) => owner,
        Err(error) => return error,
    };
    owner.legacy = Some(LegacyImageFactory {
        user: callbacks.user,
        create_image,
    });
    let raw = allocate_handle(owner);
    if raw.is_null() {
        return OUT_OF_HOST_MEMORY;
    }
    let mut owner = unsafe { Box::from_raw(raw) };
    if let Err(error) = owner.import_timeline(callbacks.user, create_fence) {
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
    let Some(callbacks) = owner.legacy else {
        return INVALID;
    };
    match owner.add_image(format, [width, height], 0, |handle, resource| unsafe {
        (callbacks.create_image)(callbacks.user, width, height, format, handle, resource)
    }) {
        Ok(image) => {
            unsafe { out.write(image) };
            0
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dx12vk_interop_release_guides(
    owner: *mut Owner,
    depth: u64,
    motion: u64,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    owner.release_images([depth, motion]).err().unwrap_or(0)
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
