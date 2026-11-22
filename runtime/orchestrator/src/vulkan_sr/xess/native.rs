// SPDX-License-Identifier: MIT
//! Production XeSS Vulkan SR table. It binds the resolved SDK exports to their
//! C signatures and owns the per-context graphics, capture and sharpening state.
//! The table stays a C record so fixtures can substitute the loader and lookup.
use super::graphics::{
    Graphics, rsf_core_xessvk_graphics_barrier, rsf_core_xessvk_graphics_bind,
    rsf_core_xessvk_graphics_copy, rsf_core_xessvk_graphics_destroy,
    rsf_core_xessvk_graphics_images,
};
use super::sdk_abi::{
    xess_2d_t, xess_quality_settings_t, xess_version_t, xess_vk_execute_params_t as XessExecute,
    xess_vk_init_params_t as XessInit,
};
use super::{Entries, Frame, Handle, Image, Names, Setup, XessOps};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_create, rsf_core_capture_vk_destroy,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_provider, rsf_core_capture_vk_record,
    rsf_core_capture_vk_retire,
};
use crate::sdk_module::{
    Loader, rsf_core_vk_sdk_load, rsf_core_vk_sdk_pin, rsf_core_vk_sdk_unload,
};
use crate::sys::rsf_vkfg_image_copy as ImageCopy;
use crate::vulkan_fg_copy::Barrier;
use crate::vulkan_fg_resources::device::rsf_core_vkfg_platform;
use crate::vulkan_sharpening::Host as Sharpen;
use crate::vulkan_sharpening::{
    rsf_sharpenvk_host_clear, rsf_sharpenvk_host_destroy, rsf_sharpenvk_host_retire,
    rsf_sharpenvk_host_run,
};
use rsf_game_sys::rsf_frame_record;
use std::ffi::c_char;
use std::ptr;

// RSF_CAPTURE_VK_COMMAND_RETIREMENT from frame_capture_vk.h.
const COMMAND_RETIREMENT: u32 = 1;
const XESS_SUCCESS: i32 = 0;

type Context = Handle;

// Signatures from xess.h and xess_vk.h (XeSS 2.x C API, Windows x64 C convention).
#[repr(C)]
struct Sdk {
    instance: unsafe extern "C" fn(*mut u32, Names, *mut u32) -> i32,
    device: unsafe extern "C" fn(Handle, Handle, *mut u32, Names) -> i32,
    features: unsafe extern "C" fn(Handle, Handle, *mut Handle) -> i32,
    create: unsafe extern "C" fn(Handle, Handle, Handle, *mut Context) -> i32,
    initialize: unsafe extern "C" fn(Context, *const XessInit) -> i32,
    execute: unsafe extern "C" fn(Context, Handle, *const XessExecute) -> i32,
    destroy: unsafe extern "C" fn(Context) -> i32,
    resolution: unsafe extern "C" fn(
        Context,
        *const xess_2d_t,
        xess_quality_settings_t,
        *mut xess_2d_t,
        *mut xess_2d_t,
        *mut xess_2d_t,
    ) -> i32,
    velocity: unsafe extern "C" fn(Context, f32, f32) -> i32,
    version: unsafe extern "C" fn(*mut xess_version_t) -> i32,
}

const _: () = assert!(size_of::<Sdk>() == size_of::<Entries>());

// The session allocates the table before resolving exports and only uses it
// after bind_entries has stored all ten non-null addresses in REQUIRED_EXPORTS order.
type Table = Option<Sdk>;

struct Native {
    sdk: *const Sdk,
    context: Context,
    gpu: *mut Graphics,
    // The SR owner retires its readbacks with the command buffer that recorded them.
    capture: *mut Capture,
    sharpen: Sharpen,
}

impl Drop for Native {
    fn drop(&mut self) {
        // Same order as the retired native owner: sharpening, capture, then graphics.
        unsafe {
            rsf_sharpenvk_host_destroy(&mut self.sharpen);
            rsf_core_capture_vk_destroy(self.capture);
            rsf_core_xessvk_graphics_destroy(self.gpu);
        }
    }
}

unsafe fn table<'a>(sdk: Handle) -> &'a Sdk {
    unsafe { (*sdk.cast::<Table>()).as_ref() }.expect("XeSS SDK entries are bound before use")
}

unsafe fn native<'a>(native: Handle) -> &'a mut Native {
    unsafe { &mut *native.cast::<Native>() }
}

unsafe fn sdk<'a>(owner: &Native) -> &'a Sdk {
    unsafe { &*owner.sdk }
}

unsafe extern "C" fn allocate_table(out: &mut Handle) -> i32 {
    *out = Box::into_raw(Box::new(Table::None)).cast();
    0
}

unsafe extern "C" fn drop_table(sdk: Handle) {
    if !sdk.is_null() {
        drop(unsafe { Box::from_raw(sdk.cast::<Table>()) });
    }
}

unsafe extern "C" fn bind_entries(sdk: Handle, entries: &Entries) {
    // The session verified every entry is non-null and resolved by exact export name.
    let bound = unsafe { std::mem::transmute_copy::<Entries, Sdk>(entries) };
    unsafe { *sdk.cast::<Table>() = Some(bound) };
}

unsafe extern "C" fn instance_requirements(
    sdk: Handle,
    count: *mut u32,
    names: Names,
    minimum: *mut u32,
) -> i32 {
    unsafe { (table(sdk).instance)(count, names, minimum) }
}

unsafe extern "C" fn device_extensions(
    sdk: Handle,
    instance: Handle,
    physical: Handle,
    count: *mut u32,
    names: Names,
) -> i32 {
    unsafe { (table(sdk).device)(instance, physical, count, names) }
}

unsafe extern "C" fn device_features(
    sdk: Handle,
    instance: Handle,
    physical: Handle,
    features: *mut Handle,
) -> i32 {
    unsafe { (table(sdk).features)(instance, physical, features) }
}

unsafe extern "C" fn version(sdk: Handle, out: &mut xess_version_t) -> i32 {
    unsafe { (table(sdk).version)(out) }
}

unsafe extern "C" fn open(sdk: Handle, out: &mut Handle) -> i32 {
    let owner = Native {
        sdk: unsafe { table(sdk) },
        context: ptr::null_mut(),
        gpu: ptr::null_mut(),
        capture: rsf_core_capture_vk_create(COMMAND_RETIREMENT),
        sharpen: Sharpen::default(),
    };
    *out = Box::into_raw(Box::new(owner)).cast();
    0
}

unsafe extern "C" fn close(owner: Handle) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner.cast::<Native>()) });
    }
}

unsafe extern "C" fn create_context(owner: Handle, setup: &Setup) -> i32 {
    let owner = unsafe { native(owner) };
    unsafe {
        (sdk(owner).create)(
            setup.instance,
            setup.physical_device,
            setup.device,
            &mut owner.context,
        )
    }
}

unsafe extern "C" fn initialize(owner: Handle, init: &XessInit) -> i32 {
    let owner = unsafe { native(owner) };
    unsafe { (sdk(owner).initialize)(owner.context, init) }
}

unsafe extern "C" fn resolution(owner: Handle, init: &XessInit, out: *mut xess_2d_t) -> i32 {
    let owner = unsafe { native(owner) };
    unsafe {
        (sdk(owner).resolution)(
            owner.context,
            &init.outputResolution,
            init.qualitySetting,
            out,
            out.add(1),
            out.add(2),
        )
    }
}

unsafe extern "C" fn destroy_context(owner: Handle) -> i32 {
    let owner = unsafe { native(owner) };
    if owner.context.is_null() {
        return XESS_SUCCESS;
    }
    let result = unsafe { (sdk(owner).destroy)(owner.context) };
    if result == XESS_SUCCESS {
        owner.context = ptr::null_mut();
    }
    result
}

unsafe extern "C" fn bind_device(owner: Handle, setup: &Setup) -> i32 {
    let owner = unsafe { native(owner) };
    let mut gpu = ptr::null_mut();
    let result = unsafe { rsf_core_xessvk_graphics_bind(setup, owner.capture, &mut gpu) };
    if result == 0 {
        unsafe { rsf_core_xessvk_graphics_destroy(owner.gpu) };
        owner.gpu = gpu;
    }
    result
}

unsafe extern "C" fn image_user(owner: Handle) -> Handle {
    unsafe { rsf_core_xessvk_graphics_images(native(owner).gpu) }
}

unsafe extern "C" fn velocity(owner: Handle, x: f32, y: f32) -> i32 {
    let owner = unsafe { native(owner) };
    unsafe { (sdk(owner).velocity)(owner.context, x, y) }
}

unsafe extern "C" fn record_barrier(owner: Handle, commands: Handle, plan: *const Barrier) {
    let gpu = unsafe { native(owner) }.gpu;
    unsafe { rsf_core_xessvk_graphics_barrier(&*gpu, commands, &*plan) };
}

unsafe extern "C" fn record_copy(owner: Handle, commands: Handle, plan: *const ImageCopy) {
    let gpu = unsafe { native(owner) }.gpu;
    unsafe { rsf_core_xessvk_graphics_copy(&*gpu, commands, &*plan) };
}

unsafe extern "C" fn execute(owner: Handle, commands: Handle, data: &XessExecute) -> i32 {
    let owner = unsafe { native(owner) };
    unsafe { (sdk(owner).execute)(owner.context, commands, data) }
}

unsafe extern "C" fn sharpen(
    owner: Handle,
    setup: &Setup,
    commands: Handle,
    frame: &Frame,
    output: u64,
) -> i32 {
    let owner = unsafe { native(owner) };
    i32::from(
        unsafe { rsf_sharpenvk_host_run(&mut owner.sharpen, setup, commands, frame, output) } != 0,
    )
}

unsafe extern "C" fn clear_sharpen(owner: Handle) {
    unsafe { rsf_sharpenvk_host_clear(&mut native(owner).sharpen) };
}

unsafe extern "C" fn capture_provider(owner: Handle, provider: u64, backend: u32) {
    unsafe { rsf_core_capture_vk_provider(native(owner).capture, provider, backend, 0) };
}

unsafe extern "C" fn capture_record(
    owner: Handle,
    commands: Handle,
    image: &Image,
    record: &rsf_frame_record,
    role: *const c_char,
    reset: u32,
) {
    let capture = unsafe { native(owner) }.capture;
    let _ = unsafe {
        rsf_core_capture_vk_record(
            capture,
            commands,
            image,
            record,
            role,
            reset,
            1,
            ptr::null_mut(),
            0,
        )
    };
}

unsafe extern "C" {
    fn rsf_capture_sr_parameters(
        record: *const rsf_frame_record,
        backend: u32,
        x: f32,
        y: f32,
        exposure: f32,
        reset: u32,
    );
}

unsafe extern "C" fn capture_parameters(frame: &Frame, backend: u32, reset: u32) {
    // The FrameDump entry keeps its once-only service binding.
    unsafe {
        rsf_capture_sr_parameters(
            frame.record,
            backend,
            frame.motion_scale_x,
            frame.motion_scale_y,
            frame.pre_exposure,
            reset,
        )
    };
}

unsafe extern "C" fn retire_commands(owner: Handle, commands: Handle) {
    let owner = unsafe { native(owner) };
    unsafe {
        rsf_sharpenvk_host_retire(&mut owner.sharpen, commands);
        rsf_core_capture_vk_retire(owner.capture, commands);
    }
}

unsafe extern "C" fn flush_capture(owner: Handle) {
    let _ = unsafe { rsf_core_capture_vk_flush(native(owner).capture, 0, ptr::null_mut()) };
}

// Selected when the build has the XeSS Vulkan SDK (the xess-vk feature); native_sr_vulkan.c
// also names it.
#[unsafe(no_mangle)]
pub(super) static rsf_xessvk_native_ops: XessOps = XessOps {
    loader: Loader {
        load: rsf_core_vk_sdk_load,
        unload: rsf_core_vk_sdk_unload,
    },
    allocate_table,
    pin: rsf_core_vk_sdk_pin,
    drop_table,
    instance_requirements,
    device_extensions,
    device_features,
    version,
    open,
    close,
    create_context,
    initialize,
    resolution,
    destroy_context,
    bind_device,
    image_user,
    images: &rsf_core_vkfg_platform,
    velocity,
    barrier: record_barrier,
    copy: record_copy,
    execute,
    sharpen,
    clear_sharpen,
    capture_provider,
    capture_record,
    capture_parameters,
    retire_commands,
    flush_capture,
    bind_entries,
    lookup: None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};

    #[test]
    fn bound_entries_follow_export_order() {
        unsafe extern "C" fn version(out: *mut xess_version_t) -> i32 {
            unsafe { (*out).major = 7 };
            XESS_SUCCESS
        }
        let mut handle = ptr::null_mut();
        assert_eq!(unsafe { allocate_table(&mut handle) }, 0);
        let mut entries: Entries = [ptr::null_mut(); 10];
        for (index, entry) in entries.iter_mut().enumerate() {
            *entry = (index + 1) as Handle;
        }
        entries[9] = version as *const () as Handle;
        unsafe { bind_entries(handle, &entries) };
        let mut value = xess_version_t {
            major: 0,
            minor: 0,
            patch: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { super::version(handle, &mut value) }, 0);
        assert_eq!(value.major, 7);
        assert_eq!(unsafe { table(handle) }.create as usize, 4);
        unsafe { drop_table(handle) };
    }

    #[test]
    fn context_retires_only_after_sdk_success() {
        static RESULT: AtomicI32 = AtomicI32::new(-1);
        unsafe extern "C" fn destroy(_: Context) -> i32 {
            RESULT.load(Ordering::Relaxed)
        }
        let mut handle = ptr::null_mut();
        unsafe { allocate_table(&mut handle) };
        let mut entries: Entries = [1 as Handle; 10];
        entries[6] = destroy as *const () as Handle;
        unsafe { bind_entries(handle, &entries) };
        let mut owner = ptr::null_mut();
        assert_eq!(unsafe { open(handle, &mut owner) }, 0);
        unsafe { native(owner).context = 0x40 as Handle };
        assert_eq!(unsafe { destroy_context(owner) }, -1);
        assert!(!unsafe { native(owner) }.context.is_null());
        RESULT.store(XESS_SUCCESS, Ordering::Relaxed);
        assert_eq!(unsafe { destroy_context(owner) }, XESS_SUCCESS);
        assert!(unsafe { native(owner) }.context.is_null());
        assert_eq!(unsafe { destroy_context(owner) }, XESS_SUCCESS);
        unsafe {
            close(owner);
            drop_table(handle);
        }
    }
}
