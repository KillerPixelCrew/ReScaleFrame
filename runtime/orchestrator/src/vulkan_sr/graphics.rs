// SPDX-License-Identifier: MIT
//! The FidelityFX Vulkan SR leaves of vulkan_sr.rs: the SDK capsule, its guarded SDK calls and
//! the Vulkan graphics, sharpening and capture state. `NATIVE` in vulkan_sr.rs names them.
use super::ffx_descriptors::abi::{
    FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE, FFX_API_DESC_TYPE_OVERRIDE_VERSION,
    FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION, FFX_API_QUERY_DESC_TYPE_GET_VERSIONS,
    FFX_API_RETURN_OK, PFN_vkVoidFunction, PfnFfxCreateContext, PfnFfxDestroyContext,
    PfnFfxDispatch, PfnFfxQuery, VkDevice, ffxApiHeader, ffxContext, ffxCreateBackendVKDesc,
    ffxCreateContextDescUpscale, ffxDispatchDescUpscale, ffxQueryDescHeader,
    ffxQueryDescUpscaleGetRenderResolutionFromQualityMode, ffxQueryGetProviderVersion,
};
use super::ffx_descriptors::{dispatch_desc, header, plan_desc, write_create_desc};
use super::{
    DISPATCH, FfxDeviceEntries, FfxDispatch, FfxExports, Frame, Handle, INIT_FAILED, Image,
    NOT_SUPPORTED, Setup, lookup_device_proc, rsf_vk_function,
};
use crate::dx12_vulkan_platform::typed;
use crate::ffx_version::{self, MAX_VERSIONS, Version};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_create, rsf_core_capture_vk_destroy,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_initialize, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record, rsf_core_capture_vk_retire, rsf_core_capture_vk_wants,
};
use crate::vulkan_fg_copy::ImageCopy;
use crate::vulkan_fg_copy::{
    Barrier, COLOR_RANGE, rsf_core_vkfg_record_barrier, rsf_core_vkfg_record_copy,
};
use crate::vulkan_fg_resources::device::{
    Device, rsf_core_vkfg_bind_images, rsf_core_vkfg_device_destroy, rsf_core_vkfg_device_memory,
};
use crate::vulkan_sharpening::Host as Sharpen;
use crate::vulkan_sharpening::{
    rsf_sharpenvk_host_clear, rsf_sharpenvk_host_destroy, rsf_sharpenvk_host_retire,
    rsf_sharpenvk_host_run,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::FEATURE_FAILED;
use rsf_game_sys::rsf_frame_record;
use rsf_streamline_vk::sdk_api::guarded_or;
use std::ffi::{c_char, c_void};
use std::ptr;

const SDK_OK: u32 = FFX_API_RETURN_OK as u32;

/// The SDK exports and context. The entry types are C-unwind, so each call below runs inside
/// `guarded_or`, whose C leaf (sdk_guard.c) turns a C++ exception into the given result.
#[derive(Default)]
struct Sdk {
    create: PfnFfxCreateContext,
    destroy: PfnFfxDestroyContext,
    query: PfnFfxQuery,
    dispatch: PfnFfxDispatch,
    effect: ffxContext,
}

/// `ffxQueryDescGetVersions` and `ffxOverrideVersion` of ffx_api.h, which the pinned Vulkan
/// bindings do not list.
#[repr(C)]
struct GetVersions {
    header: ffxQueryDescHeader,
    create_desc_type: u64,
    device: *mut c_void,
    output_count: *mut u64,
    version_ids: *mut u64,
    version_names: *mut *const c_char,
}

#[repr(C)]
struct OverrideVersion {
    header: ffxApiHeader,
    version_id: u64,
}

const _: () = assert!(size_of::<GetVersions>() == 56 && size_of::<OverrideVersion>() == 24);

#[derive(Default)]
struct Native {
    sdk: Sdk,
    graphics: Option<Graphics>,
}

impl Native {
    fn graphics(&self) -> &Graphics {
        self.graphics
            .as_ref()
            .expect("initialized Vulkan SR graphics")
    }

    fn graphics_mut(&mut self) -> &mut Graphics {
        self.graphics
            .as_mut()
            .expect("initialized Vulkan SR graphics")
    }
}

/// The capsule behind a handle from [`allocate`]; the owner serializes every call on it.
fn native<'a>(owner: Handle) -> &'a mut Native {
    unsafe { &mut *owner.cast::<Native>() }
}

pub(super) unsafe extern "C" fn allocate(_setup: &Setup) -> Handle {
    crate::allocate_handle(Native::default()).cast()
}

pub(super) unsafe extern "C" fn close(owner: Handle) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner.cast::<Native>()) });
    }
}

pub(super) unsafe extern "C" fn bind(owner: Handle, exports: &FfxExports) {
    let [create, destroy, query, dispatch] = *exports;
    // Admission resolved these four non-null exports by their exact SDK names.
    native(owner).sdk = unsafe {
        Sdk {
            create: typed(create),
            destroy: typed(destroy),
            query: typed(query),
            dispatch: typed(dispatch),
            effect: ptr::null_mut(),
        }
    };
}

pub(super) unsafe extern "C" fn versions(
    owner: Handle,
    out: &mut [Version; MAX_VERSIONS],
    count: &mut u64,
) -> i32 {
    let Some(query) = native(owner).sdk.query else {
        return INIT_FAILED;
    };
    guarded_or(INIT_FAILED, || {
        let mut ids = [0_u64; MAX_VERSIONS];
        let mut names = [ptr::null::<c_char>(); MAX_VERSIONS];
        *count = MAX_VERSIONS as u64;
        let mut request = GetVersions {
            header: header(FFX_API_QUERY_DESC_TYPE_GET_VERSIONS, ptr::null_mut()),
            create_desc_type: u64::from(FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE),
            device: ptr::null_mut(),
            output_count: &raw mut *count,
            version_ids: ids.as_mut_ptr(),
            version_names: names.as_mut_ptr(),
        };
        if unsafe { query(ptr::null_mut(), &raw mut request.header) } != SDK_OK
            || *count > MAX_VERSIONS as u64
        {
            return NOT_SUPPORTED;
        }
        for (slot, (&id, &name)) in out
            .iter_mut()
            .zip(ids.iter().zip(&names))
            .take(*count as usize)
        {
            *slot = unsafe { ffx_version::from_sdk(id, name) };
        }
        0
    })
}

pub(super) unsafe extern "C" fn create_effect(
    owner: Handle,
    setup: &Setup,
    id: u64,
    flags: u32,
) -> i32 {
    let sdk = &mut native(owner).sdk;
    let Some(create) = sdk.create else {
        return INIT_FAILED;
    };
    guarded_or(INIT_FAILED, || {
        let mut version = OverrideVersion {
            header: header(FFX_API_DESC_TYPE_OVERRIDE_VERSION, ptr::null_mut()),
            version_id: id,
        };
        let mut backend = ffxCreateBackendVKDesc::default();
        let mut desc = ffxCreateContextDescUpscale::default();
        write_create_desc(
            setup,
            flags,
            &raw mut version.header,
            &mut backend,
            &mut desc,
        );
        if unsafe { create(&mut sdk.effect, &raw mut desc.header, ptr::null()) } == SDK_OK {
            0
        } else {
            INIT_FAILED
        }
    })
}

pub(super) unsafe extern "C" fn effective_version(owner: Handle, out: &mut u64) -> i32 {
    let sdk = &mut native(owner).sdk;
    let Some(query) = sdk.query else {
        return INIT_FAILED;
    };
    guarded_or(INIT_FAILED, || {
        let mut desc = ffxQueryGetProviderVersion {
            header: header(
                FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION,
                ptr::null_mut(),
            ),
            ..Default::default()
        };
        if unsafe { query(&mut sdk.effect, &raw mut desc.header) } != SDK_OK {
            return NOT_SUPPORTED;
        }
        *out = desc.versionId;
        0
    })
}

pub(super) unsafe extern "C" fn destroy_effect(owner: Handle) -> i32 {
    let sdk = &mut native(owner).sdk;
    if sdk.effect.is_null() {
        return 0;
    }
    let Some(destroy) = sdk.destroy else {
        return FEATURE_FAILED;
    };
    guarded_or(FEATURE_FAILED, || {
        if unsafe { destroy(&mut sdk.effect, ptr::null()) } != SDK_OK {
            return FEATURE_FAILED;
        }
        sdk.effect = ptr::null_mut();
        0
    })
}

pub(super) unsafe extern "C" fn plan(
    owner: Handle,
    setup: &Setup,
    width: &mut u32,
    height: &mut u32,
) -> i32 {
    let sdk = &mut native(owner).sdk;
    let Some(query) = sdk.query else {
        return INIT_FAILED;
    };
    guarded_or(INIT_FAILED, || {
        let mut desc: ffxQueryDescUpscaleGetRenderResolutionFromQualityMode =
            plan_desc(setup, &raw mut *width, &raw mut *height);
        if unsafe { query(&mut sdk.effect, &raw mut desc.header) } == SDK_OK {
            0
        } else {
            NOT_SUPPORTED
        }
    })
}

pub(super) unsafe extern "C" fn dispatch(
    owner: Handle,
    commands: Handle,
    call: &FfxDispatch<'_>,
) -> i32 {
    let sdk = &mut native(owner).sdk;
    let Some(dispatch) = sdk.dispatch else {
        return FEATURE_FAILED;
    };
    guarded_or(FEATURE_FAILED, || {
        let desc: ffxDispatchDescUpscale = unsafe { dispatch_desc(commands, call) };
        if unsafe { dispatch(&mut sdk.effect, &raw const desc.header) } == SDK_OK {
            0
        } else {
            FEATURE_FAILED
        }
    })
}
pub(super) unsafe extern "C" fn sdk_device_proc(
    device: VkDevice,
    name: *const c_char,
) -> PFN_vkVoidFunction {
    let device = device.cast();
    let (proc, lookup) = DISPATCH
        .lock()
        .unwrap()
        .get(&(device as usize))
        .and_then(|entry| Some((entry.proc, entry.lookup?)))?;
    unsafe { lookup_device_proc(device, name, proc, lookup) }
}

struct Graphics {
    images: *mut Device,
    capture: *mut Capture,
    sharpen: Sharpen,
    memory_types: u32,
    barrier: vk::PFN_vkCmdPipelineBarrier,
    clear: vk::PFN_vkCmdClearColorImage,
    copy: vk::PFN_vkCmdCopyImage,
}

impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe {
            rsf_sharpenvk_host_destroy(&mut self.sharpen);
            rsf_core_capture_vk_destroy(self.capture);
            rsf_core_vkfg_device_destroy(self.images);
        }
    }
}

fn create(
    setup: &Setup,
    entries: &FfxDeviceEntries,
    properties: rsf_vk_function,
) -> Option<Graphics> {
    // Admission resolves these slots by their exact Vulkan command names.
    let properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties = unsafe { typed(properties) }?;
    let [
        create,
        destroy,
        requirements,
        allocate,
        free,
        bind,
        barrier,
        clear,
        copy,
    ] = *entries;
    let mut graphics = Graphics {
        images: ptr::null_mut(),
        capture: ptr::null_mut(),
        sharpen: Sharpen::default(),
        memory_types: 0,
        barrier: unsafe { typed(barrier) }?,
        clear: unsafe { typed(clear) }?,
        copy: unsafe { typed(copy) }?,
    };
    graphics.images = rsf_core_vkfg_bind_images(
        setup.device,
        &[create, destroy, requirements, allocate, free, bind],
    );
    let images = unsafe { graphics.images.as_mut() }?;
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(setup.physical_device as usize as u64),
            &mut memory,
        );
    }
    graphics.memory_types = memory.memory_type_count;
    rsf_core_vkfg_device_memory(images, &memory);
    graphics.capture = rsf_core_capture_vk_create(1);
    let ready = unsafe {
        rsf_core_capture_vk_initialize(graphics.capture, setup.device, setup.device_proc, &memory)
    };
    (ready != 0).then_some(graphics)
}

fn command_buffer(commands: Handle) -> vk::CommandBuffer {
    vk::CommandBuffer::from_raw(commands as usize as u64)
}

pub(super) unsafe extern "C" fn initialize_graphics(
    owner: Handle,
    setup: &Setup,
    entries: &FfxDeviceEntries,
    properties: rsf_vk_function,
) -> i32 {
    let Some(graphics) = create(setup, entries, properties) else {
        return INIT_FAILED;
    };
    native(owner).graphics = Some(graphics);
    0
}

pub(super) unsafe extern "C" fn images(owner: Handle) -> Handle {
    native(owner).graphics().images.cast()
}

pub(super) unsafe extern "C" fn memory_count(owner: Handle) -> u32 {
    native(owner).graphics().memory_types
}

pub(super) unsafe extern "C" fn barrier(owner: Handle, commands: Handle, plan: *const Barrier) {
    let graphics = native(owner).graphics();
    unsafe { rsf_core_vkfg_record_barrier(graphics.barrier, command_buffer(commands), &*plan) };
}

pub(super) unsafe extern "C" fn clear(owner: Handle, commands: Handle, image: u64, layout: u32) {
    let graphics = native(owner).graphics();
    unsafe {
        (graphics.clear)(
            command_buffer(commands),
            vk::Image::from_raw(image),
            vk::ImageLayout::from_raw(layout as i32),
            &vk::ClearColorValue { uint32: [0; 4] },
            1,
            &COLOR_RANGE,
        );
    }
}

pub(super) unsafe extern "C" fn copy(owner: Handle, commands: Handle, plan: *const ImageCopy) {
    let graphics = native(owner).graphics();
    unsafe { rsf_core_vkfg_record_copy(graphics.copy, command_buffer(commands), &*plan) };
}

pub(super) unsafe extern "C" fn sharpen(
    owner: Handle,
    setup: &Setup,
    commands: Handle,
    frame: &Frame,
    output: u64,
) -> i32 {
    let graphics = native(owner).graphics_mut();
    i32::from(
        unsafe { rsf_sharpenvk_host_run(&mut graphics.sharpen, setup, commands, frame, output) }
            != 0,
    )
}

pub(super) unsafe extern "C" fn clear_sharpen(owner: Handle) {
    if let Some(graphics) = native(owner).graphics.as_mut() {
        unsafe { rsf_sharpenvk_host_clear(&mut graphics.sharpen) };
    }
}

pub(super) unsafe extern "C" fn capture_provider(owner: Handle, version: u64, backend: u32) {
    let graphics = native(owner).graphics();
    unsafe { rsf_core_capture_vk_provider(graphics.capture, version, backend, 0) };
}

pub(super) unsafe extern "C" fn capture_record(
    owner: Handle,
    commands: Handle,
    image: &Image,
    record: &rsf_frame_record,
    role: *const c_char,
    reset: u32,
) {
    let graphics = native(owner).graphics();
    unsafe {
        rsf_core_capture_vk_record(
            graphics.capture,
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

pub(super) unsafe extern "C" fn capture_wants(owner: Handle, frame: u64) -> i32 {
    let graphics = native(owner).graphics();
    unsafe { rsf_core_capture_vk_wants(graphics.capture, frame) }
}

pub(super) unsafe extern "C" fn retire(owner: Handle, commands: Handle) {
    let graphics = native(owner).graphics_mut();
    unsafe {
        rsf_sharpenvk_host_retire(&mut graphics.sharpen, commands);
        rsf_core_capture_vk_retire(graphics.capture, commands);
    }
}

pub(super) unsafe extern "C" fn flush(owner: Handle) {
    if let Some(graphics) = native(owner).graphics.as_ref() {
        unsafe { rsf_core_capture_vk_flush(graphics.capture, 0, ptr::null_mut()) };
    }
}

pub(super) unsafe extern "C" fn capture_parameters(frame: &Frame, backend: u32, reset: u32) {
    // Use the same FrameDump entry, including its once-only service binding.
    unsafe {
        crate::input_dump::abi::rsf_capture_sr_parameters(
            frame.record,
            backend,
            frame.motion_scale_x,
            frame.motion_scale_y,
            frame.pre_exposure,
            reset,
        );
    }
}
