// SPDX-License-Identifier: MIT
// FidelityFX 3 Vulkan SDK leaf for the owner in vulkan_ffx_fg.rs. A leaf binds
// the SDK module exports, holds both SDK contexts and the replacement chain,
// registers the corrected SDK's WSI hooks and owns the frame capture. The host
// table (NATIVE_HOST below, or a fixture's) supplies module loading, guarded host
// Vulkan calls and the latency scopes.
//
// Until the owner publishes the leaf to the WSI route only the opening thread
// reaches it. Afterwards the hooks, the backend resolver and the generation
// callback read it while the owner drives the SDK, including reentrantly from
// inside an SDK call. Only the SDK-written contexts, the chain and the
// replacement table change after publication, so they are UnsafeCell fields
// and every later step takes a shared reference.
use super::{BGRA8, FEATURE_FAILED, Frame, Handle, INIT_FAILED, Image, OK, RGBA8, Record, Setup};
use crate::dx12_vulkan_platform::typed;
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_create, rsf_core_capture_vk_destroy,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_initialize, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record, rsf_core_capture_vk_wants,
};
use crate::sdk_module::Loader;
use crate::sys::{rsf_vk_function, rsf_vk_get_proc};
use crate::vulkan_dispatch::succeeded;
use crate::vulkan_fg_copy::ImageCopy;
use crate::vulkan_fg_copy::{Barrier, rsf_core_vkfg_record_barrier, rsf_core_vkfg_record_copy};
use crate::vulkan_fg_resources::device::{
    Device, rsf_core_vkfg_device_destroy, rsf_core_vkfg_device_memory,
    rsf_core_vkfg_device_wait_timing, rsf_core_vkfg_load_device,
};
use crate::vulkan_fg_wsi::{
    QueuePresent, Scope, rsf_core_vkfg_device_proc, rsf_core_vkfg_drain,
    rsf_core_vkfg_graphics_submit, rsf_core_vkfg_wsi_acquire, rsf_core_vkfg_wsi_create,
    rsf_core_vkfg_wsi_destroy, rsf_core_vkfg_wsi_images, rsf_core_vkfg_wsi_present,
};
use crate::vulkan_timing::{timing_begin, timing_end};
use ash::vk::{self, Handle as _};
use rsf_game_sys::RSF_FRAME_FLAG_RESET;
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;

use crate::ffx_vk_abi as abi;
use abi::{
    FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION, FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK,
    FFX_API_CREATE_CONTEXT_DESC_TYPE_FGSWAPCHAIN_VK,
    FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION,
    FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_HUDLESS,
    FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK,
    FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE,
    FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_CAMERAINFO,
    FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK, FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION,
    FFX_API_RESOURCE_STATE_COMPUTE_READ, FFX_API_RESOURCE_TYPE_TEXTURE2D,
    FFX_API_RESOURCE_USAGE_READ_ONLY, FFX_API_RETURN_ERROR_PARAMETER, FFX_API_RETURN_OK,
    FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM, FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM,
    FFX_API_SURFACE_FORMAT_R16G16_FLOAT, FFX_API_SURFACE_FORMAT_R32_FLOAT,
    FFX_API_SURFACE_FORMAT_UNKNOWN, FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE,
    FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED,
    FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION, FfxApiDimensions2D,
    FfxApiFloatCoords2D, FfxApiRect2D, FfxApiResource, FfxApiResourceDescription,
    FfxApiResourceDescription__bindgen_ty_1, FfxApiResourceDescription__bindgen_ty_2,
    FfxApiResourceDescription__bindgen_ty_3, PFN_vkQueueSubmitFFXAPI, PFN_vkVoidFunction,
    PfnFfxConfigure, PfnFfxCreateContext, PfnFfxDestroyContext, PfnFfxDispatch, PfnFfxQuery,
    VkAllocationCallbacks, VkDevice, VkFence, VkQueueInfoFFXAPI, VkResult, VkSubmitInfo,
    VkSwapchainCreateInfoKHR, VkSwapchainKHR, ffxApiHeader, ffxConfigureDescFrameGeneration,
    ffxContext, ffxCreateBackendVKDesc, ffxCreateContextDescFrameGeneration,
    ffxCreateContextDescFrameGenerationHudless, ffxCreateContextDescFrameGenerationSwapChainVK,
    ffxDispatchDescFrameGeneration, ffxDispatchDescFrameGenerationPrepare,
    ffxDispatchDescFrameGenerationPrepareCameraInfo,
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK,
    ffxQueryDescSwapchainReplacementFunctionsVK, ffxQueryGetProviderVersion, ffxReturnCode_t,
};

use rsf_game_sys::backend_result::MISSING_ENTRY_POINT;
const SDK_OK: ffxReturnCode_t = FFX_API_RETURN_OK as ffxReturnCode_t;
const WSI_ABI_VERSION: u32 = 2;
const SHADER_READ: u32 = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32;
const GENERAL: u32 = vk::ImageLayout::GENERAL.as_raw() as u32;
const COMMAND_RETIREMENT: u32 = 1;

const _: () = assert!(FFX_API_RETURN_OK == 0);

/// Host services (`NATIVE_HOST` in production, the FFX fixture's table in tests). The table outlives
/// every owner created with it.
#[repr(C)]
pub(super) struct Host {
    pub(super) loader: Loader,
    pub(super) symbol: unsafe extern "C" fn(Handle, *const c_char) -> rsf_vk_function,
    pub(super) resolve:
        unsafe extern "C" fn(rsf_vk_get_proc, Handle, *const c_char, *mut rsf_vk_function) -> i32,
    pub(super) memory_properties: unsafe extern "C" fn(rsf_vk_function, Handle, Handle) -> i32,
    pub(super) without_dlssg: Scope,
    pub(super) wsi_call: Scope,
    pub(super) queue_present: QueuePresent,
}

const _: () = assert!(size_of::<Host>() == 8 * size_of::<Handle>());

/// Resolves `name` through a host Vulkan resolver, which may throw: INIT_FAILED when a C++
/// exception was contained. The FFX fixture (fixture/fake_sdk.rs) builds its host table with this entry.
pub(crate) unsafe extern "C" fn rsf_ffxgvk_guard_resolve(
    resolve: rsf_vk_get_proc,
    object: Handle,
    name: *const c_char,
    out: *mut rsf_vk_function,
) -> i32 {
    let Some(resolve) = resolve else {
        return INIT_FAILED;
    };
    // The host callback, called with the C-unwind ABI so the guard can contain an exception.
    type Resolve = unsafe extern "C" fn(Handle, *const c_char) -> rsf_vk_function;
    type ResolveUnwind = unsafe extern "C-unwind" fn(Handle, *const c_char) -> rsf_vk_function;
    let resolve = unsafe { std::mem::transmute::<Resolve, ResolveUnwind>(resolve) };
    rsf_streamline_vk::sdk_api::guarded_or(INIT_FAILED, || {
        unsafe { out.write(resolve(object, name)) };
        OK
    })
}

/// Calls the vkGetPhysicalDeviceMemoryProperties entry admission resolved, containing a C++
/// exception as INIT_FAILED. The FFX fixture (fixture/fake_sdk.rs) builds its host table with this entry.
pub(crate) unsafe extern "C" fn rsf_ffxgvk_guard_memory(
    query: rsf_vk_function,
    physical_device: Handle,
    properties: Handle,
) -> i32 {
    let Some(query) = query else {
        return INIT_FAILED;
    };
    type Query = unsafe extern "C-unwind" fn(Handle, Handle);
    let query = unsafe { std::mem::transmute::<unsafe extern "C" fn(), Query>(query) };
    rsf_streamline_vk::sdk_api::guarded_or(INIT_FAILED, || {
        unsafe { query(physical_device, properties) };
        OK
    })
}

unsafe extern "C" fn without_dlssg(call: crate::vulkan_fg_wsi::Callback, user: Handle) -> i32 {
    unsafe { crate::vulkan_latency::rsf_slvk_without_dlssg(Some(call), user) }
}

unsafe extern "C" fn wsi_call(call: crate::vulkan_fg_wsi::Callback, user: Handle) -> i32 {
    unsafe { crate::vulkan_latency::rsf_latencyvk_wsi_call(Some(call), user) }
}

/// The production host table, selected when CMake found the FidelityFX Vulkan headers.
pub(super) static NATIVE_HOST: Host = Host {
    loader: Loader {
        load: crate::sdk_module::rsf_core_vk_sdk_load,
        unload: crate::sdk_module::rsf_core_vk_sdk_unload,
    },
    symbol: crate::sdk_module::rsf_vkffx_symbol,
    resolve: rsf_ffxgvk_guard_resolve,
    memory_properties: rsf_ffxgvk_guard_memory,
    without_dlssg,
    wsi_call,
    queue_present: crate::vulkan_latency::rsf_latencyvk_queue_present,
};

/// The corrected SDK's rsf_ffx_wsi_dispatch (runtime/vulkan/fsr3/wsi-dispatch.h).
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct WsiDispatch {
    pub(super) struct_size: u32,
    pub(super) abi_version: u32,
    pub(super) device: vk::Device,
    pub(super) create: Option<vk::PFN_vkCreateSwapchainKHR>,
    pub(super) destroy: Option<vk::PFN_vkDestroySwapchainKHR>,
    pub(super) present: Option<vk::PFN_vkQueuePresentKHR>,
    pub(super) acquire: Option<vk::PFN_vkAcquireNextImageKHR>,
    pub(super) images: Option<vk::PFN_vkGetSwapchainImagesKHR>,
}

type SetWsiDispatch = unsafe extern "C" fn(*const WsiDispatch) -> i32;

const _: () = {
    assert!(size_of::<WsiDispatch>() == 56);
    assert!(std::mem::offset_of!(WsiDispatch, device) == 8);
    assert!(std::mem::offset_of!(WsiDispatch, create) == 16);
    assert!(std::mem::offset_of!(WsiDispatch, images) == 48);
};

#[derive(Clone, Copy, Default)]
struct Sdk {
    configure: PfnFfxConfigure,
    create: PfnFfxCreateContext,
    destroy: PfnFfxDestroyContext,
    query: PfnFfxQuery,
    dispatch: PfnFfxDispatch,
}

// open binds every SDK, device and WSI entry before it returns the leaf, so a
// published leaf never holds a null entry.
macro_rules! bound {
    ($entry:expr) => {
        $entry.expect("bound FidelityFX Vulkan entry")
    };
}

struct Native {
    setup: Setup,
    host: &'static Host,
    sdk: Sdk,
    swapchain: UnsafeCell<ffxContext>,
    effect: UnsafeCell<ffxContext>,
    chain: UnsafeCell<VkSwapchainKHR>,
    functions: UnsafeCell<ffxQueryDescSwapchainReplacementFunctionsVK>,
    wsi_dispatch: Option<SetWsiDispatch>,
    native_present: Option<vk::PFN_vkQueuePresentKHR>,
    native_acquire: Option<vk::PFN_vkAcquireNextImageKHR>,
    native_images: Option<vk::PFN_vkGetSwapchainImagesKHR>,
    native_create: Option<vk::PFN_vkCreateSwapchainKHR>,
    native_destroy: Option<vk::PFN_vkDestroySwapchainKHR>,
    gpu: *mut Device,
    barrier: Option<vk::PFN_vkCmdPipelineBarrier>,
    copy: Option<vk::PFN_vkCmdCopyImage>,
    idle: Option<vk::PFN_vkDeviceWaitIdle>,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    allocator: *mut VkAllocationCallbacks,
    width: u32,
    height: u32,
    // Each readback must identify a recording that executed, even after a
    // command buffer address is reused. The owner flushes after a full drain.
    capture: *mut Capture,
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            rsf_core_capture_vk_destroy(self.capture);
            rsf_core_vkfg_device_destroy(self.gpu);
        }
    }
}

impl Native {
    fn device(&self) -> vk::Device {
        vk::Device::from_raw(self.setup.graphics.device as usize as u64)
    }

    fn sdk_device(&self) -> VkDevice {
        self.setup.graphics.device.cast()
    }

    fn chain(&self) -> VkSwapchainKHR {
        unsafe { *self.chain.get() }
    }

    fn functions(&self) -> ffxQueryDescSwapchainReplacementFunctionsVK {
        unsafe { *self.functions.get() }
    }
}

// A published leaf is only read through shared references; see the file header.
unsafe fn leaf<'a>(native: Handle) -> &'a Native {
    unsafe { &*native.cast::<Native>() }
}

fn header(kind: u32, next: *mut ffxApiHeader) -> ffxApiHeader {
    ffxApiHeader {
        type_: u64::from(kind),
        pNext: next,
    }
}

fn dimensions(width: u32, height: u32) -> FfxApiDimensions2D {
    FfxApiDimensions2D { width, height }
}

// The owner admits only these formats: an RGBA8 or BGRA8 swapchain, R32 depth,
// RG16 motion and an RGBA8 HUD-less copy. ffxApiGetSurfaceFormatVK maps each
// of them to the same SDK format.
fn surface_format(format: u32) -> u32 {
    (match vk::Format::from_raw(format as i32) {
        vk::Format::R8G8B8A8_UNORM => FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM,
        vk::Format::B8G8R8A8_UNORM => FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM,
        vk::Format::R16G16_SFLOAT => FFX_API_SURFACE_FORMAT_R16G16_FLOAT,
        vk::Format::R32_SFLOAT => FFX_API_SURFACE_FORMAT_R32_FLOAT,
        _ => FFX_API_SURFACE_FORMAT_UNKNOWN,
    }) as u32
}

fn resource(image: &Image) -> FfxApiResource {
    FfxApiResource {
        resource: ptr::with_exposed_provenance_mut(image.image as usize),
        description: FfxApiResourceDescription {
            type_: FFX_API_RESOURCE_TYPE_TEXTURE2D as u32,
            format: surface_format(image.format),
            __bindgen_anon_1: FfxApiResourceDescription__bindgen_ty_1 { width: image.width },
            __bindgen_anon_2: FfxApiResourceDescription__bindgen_ty_2 {
                height: image.height,
            },
            __bindgen_anon_3: FfxApiResourceDescription__bindgen_ty_3 { depth: 1 },
            mipCount: 1,
            flags: 0,
            usage: FFX_API_RESOURCE_USAGE_READ_ONLY as u32,
        },
        state: FFX_API_RESOURCE_STATE_COMPUTE_READ as u32,
    }
}

fn result(sdk: ffxReturnCode_t, failure: i32) -> i32 {
    if sdk == SDK_OK { OK } else { failure }
}

// Scoped WSI route borrow. The hooks carry no user pointer, so each borrows
// the leaf for the whole callback, including the forwarded Vulkan call. The
// route keeps the leaf alive meanwhile and holds no lock.
struct Borrow(*const Native);

impl Borrow {
    fn new(device: usize, match_device: bool) -> Self {
        let native = super::rsf_core_ffxgvk_wsi_borrow(
            ptr::without_provenance_mut(device),
            u32::from(match_device),
        );
        Self(native.cast())
    }

    fn leaf(&self) -> Option<&Native> {
        unsafe { self.0.as_ref() }
    }
}

impl Drop for Borrow {
    fn drop(&mut self) {
        if !self.0.is_null() {
            super::rsf_core_ffxgvk_wsi_return();
        }
    }
}

fn device_key(device: vk::Device) -> usize {
    device.as_raw() as usize
}

unsafe extern "system" fn serialized_create(
    device: vk::Device,
    info: *const vk::SwapchainCreateInfoKHR<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    chain: *mut vk::SwapchainKHR,
) -> vk::Result {
    let borrow = Borrow::new(device_key(device), true);
    match borrow
        .leaf()
        .and_then(|leaf| Some((leaf.host.without_dlssg, leaf.native_create?)))
    {
        Some((scope, call)) => unsafe {
            rsf_core_vkfg_wsi_create(scope, call, device, info, allocator, chain)
        },
        None => vk::Result::ERROR_INITIALIZATION_FAILED,
    }
}

unsafe extern "system" fn serialized_destroy(
    device: vk::Device,
    chain: vk::SwapchainKHR,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    let borrow = Borrow::new(device_key(device), true);
    if let Some((scope, call)) = borrow
        .leaf()
        .and_then(|leaf| Some((leaf.host.without_dlssg, leaf.native_destroy?)))
    {
        unsafe { rsf_core_vkfg_wsi_destroy(scope, call, device, chain, allocator) };
    }
}

unsafe extern "system" fn serialized_acquire(
    device: vk::Device,
    chain: vk::SwapchainKHR,
    timeout: u64,
    semaphore: vk::Semaphore,
    fence: vk::Fence,
    index: *mut u32,
) -> vk::Result {
    let start = timing_begin();
    let result = {
        let borrow = Borrow::new(device_key(device), true);
        match borrow.leaf().and_then(|leaf| {
            Some((
                leaf.host.without_dlssg,
                leaf.host.wsi_call,
                leaf.native_acquire?,
            ))
        }) {
            Some((scope, synchronized, call)) => unsafe {
                rsf_core_vkfg_wsi_acquire(
                    scope,
                    synchronized,
                    call,
                    device,
                    chain,
                    timeout,
                    semaphore,
                    fence,
                    index,
                )
            },
            None => vk::Result::ERROR_INITIALIZATION_FAILED,
        }
    };
    timing_end("native_acquire_total", 0, start);
    result
}

unsafe extern "system" fn serialized_images(
    device: vk::Device,
    chain: vk::SwapchainKHR,
    count: *mut u32,
    images: *mut vk::Image,
) -> vk::Result {
    let borrow = Borrow::new(device_key(device), true);
    match borrow.leaf().and_then(|leaf| {
        Some((
            leaf.host.without_dlssg,
            leaf.host.wsi_call,
            leaf.native_images?,
        ))
    }) {
        Some((scope, synchronized, call)) => unsafe {
            rsf_core_vkfg_wsi_images(scope, synchronized, call, device, chain, count, images)
        },
        None => vk::Result::ERROR_INITIALIZATION_FAILED,
    }
}

unsafe extern "system" fn serialized_present(
    queue: vk::Queue,
    info: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    let start = timing_begin();
    let result = {
        // The queue hooks receive no device.
        let borrow = Borrow::new(0, false);
        match borrow.leaf().and_then(|leaf| {
            Some((
                leaf.host.without_dlssg,
                leaf.host.queue_present,
                leaf.native_present?,
            ))
        }) {
            Some((scope, synchronized, call)) => unsafe {
                rsf_core_vkfg_wsi_present(scope, synchronized, call, queue, info)
            },
            None => vk::Result::ERROR_INITIALIZATION_FAILED,
        }
    };
    timing_end("native_present_total", 0, start);
    result
}

// The SDK backend's vkDeviceProcAddr, also without a user pointer.
unsafe extern "C" fn device_proc(device: VkDevice, name: *const c_char) -> PFN_vkVoidFunction {
    let borrow = Borrow::new(device.addr(), true);
    let leaf = borrow.leaf()?;
    let mut entry = None;
    let resolved = unsafe {
        rsf_core_vkfg_device_proc(
            leaf.host.resolve,
            leaf.setup.graphics.device_proc,
            device.cast(),
            name,
            &mut entry,
        )
    };
    if resolved == OK { entry } else { None }
}

// The SDK's game-queue submit function, also without a user pointer.
unsafe extern "C" fn graphics_submit(
    count: u32,
    submits: *const VkSubmitInfo,
    fence: VkFence,
) -> VkResult {
    let borrow = Borrow::new(0, false);
    match borrow.leaf() {
        Some(leaf) => unsafe {
            rsf_core_vkfg_graphics_submit(
                &leaf.setup,
                count,
                submits.cast(),
                vk::Fence::from_raw(fence.addr() as u64),
            )
        },
        None => vk::Result::ERROR_DEVICE_LOST,
    }
    .as_raw()
}

// The SDK generation callback runs inside Present with the leaf as user data.
unsafe extern "C" fn generate(
    data: *mut ffxDispatchDescFrameGeneration,
    user: *mut c_void,
) -> ffxReturnCode_t {
    const PARAMETER: ffxReturnCode_t = FFX_API_RETURN_ERROR_PARAMETER as ffxReturnCode_t;
    let mut record: Record = unsafe { std::mem::zeroed() };
    let Some(data) = (unsafe { data.as_mut() }) else {
        return PARAMETER;
    };
    if unsafe { super::rsf_core_ffxgvk_generation_record(user, data.frameID, &mut record) } == 0 {
        return PARAMETER;
    }
    let leaf = unsafe { leaf(user) };
    data.reset |= record.flags & RSF_FRAME_FLAG_RESET != 0;
    let dispatched = unsafe { bound!(leaf.sdk.dispatch)(leaf.effect.get(), &mut data.header) };
    if dispatched != SDK_OK {
        return dispatched;
    }
    super::rsf_core_ffxgvk_note_generated(user);
    if unsafe { rsf_core_capture_vk_wants(leaf.capture, data.frameID) } != 0 {
        let count = data.numGeneratedFrames.min(data.outputs.len() as u32) as usize;
        for source in &data.outputs[..count] {
            let description = &source.description;
            // The generated output uses the actual swapchain channel order.
            let format = if description.format == FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM as u32 {
                BGRA8
            } else {
                RGBA8
            };
            let image = Image {
                device: leaf.setup.graphics.device,
                image: source.resource.addr() as u64,
                format,
                width: unsafe { description.__bindgen_anon_1.width },
                height: unsafe { description.__bindgen_anon_2.height },
                layout: GENERAL,
                copy_source: 1,
            };
            unsafe {
                rsf_core_capture_vk_record(
                    leaf.capture,
                    data.commandList,
                    &image,
                    &record,
                    c"fg-generated-before-ui".as_ptr(),
                    u32::from(data.reset),
                    0,
                    ptr::null_mut(),
                    0,
                )
            };
        }
    }
    dispatched
}

pub(super) unsafe extern "C" fn close(native: Handle) {
    if !native.is_null() {
        drop(unsafe { Box::from_raw(native.cast::<Native>()) });
    }
}

/// Typed `T` (`typed` pairs each name with its signature): MISSING_ENTRY_POINT when absent.
unsafe fn required<T: Copy>(function: rsf_vk_function) -> Result<T, i32> {
    unsafe { typed(function) }.ok_or(MISSING_ENTRY_POINT)
}

impl Native {
    // Resolves through the host's guarded resolver; `aliases` adds the promoted core names.
    unsafe fn lookup_device(&self, name: &CStr, aliases: bool) -> Result<rsf_vk_function, i32> {
        let graphics = &self.setup.graphics;
        let mut out = None;
        succeeded(unsafe {
            if aliases {
                rsf_core_vkfg_device_proc(
                    self.host.resolve,
                    graphics.device_proc,
                    graphics.device,
                    name.as_ptr(),
                    &mut out,
                )
            } else {
                (self.host.resolve)(
                    graphics.device_proc,
                    graphics.device,
                    name.as_ptr(),
                    &mut out,
                )
            }
        })?;
        Ok(out)
    }

    /// Binds the SDK exports, the device and WSI entries, the memory properties, the resource
    /// table and the capture, in that order, before the leaf is published.
    unsafe fn bind(&mut self, module: Handle) -> Result<(), i32> {
        let host = self.host;
        let symbol = |name: &CStr| unsafe { (host.symbol)(module, name.as_ptr()) };
        let sdk = [
            c"ffxConfigure",
            c"ffxCreateContext",
            c"ffxDestroyContext",
            c"ffxQuery",
            c"ffxDispatch",
        ]
        .map(symbol);
        if sdk.iter().any(Option::is_none) {
            return Err(MISSING_ENTRY_POINT);
        }
        let [configure, create, destroy, query, dispatch] = sdk;
        unsafe {
            self.sdk = Sdk {
                configure: typed(configure),
                create: typed(create),
                destroy: typed(destroy),
                query: typed(query),
                dispatch: typed(dispatch),
            };
            self.barrier = Some(required(
                self.lookup_device(c"vkCmdPipelineBarrier", true)?,
            )?);
            self.copy = Some(required(self.lookup_device(c"vkCmdCopyImage", true)?)?);
            self.idle = Some(required(self.lookup_device(c"vkDeviceWaitIdle", true)?)?);
        }
        let graphics = &self.setup.graphics;
        let mut memory = None;
        succeeded(unsafe {
            (self.host.resolve)(
                graphics.instance_proc,
                graphics.instance,
                c"vkGetPhysicalDeviceMemoryProperties".as_ptr(),
                &mut memory,
            )
        })?;
        if memory.is_none() {
            return Err(MISSING_ENTRY_POINT);
        }
        succeeded(unsafe {
            (self.host.memory_properties)(
                memory,
                graphics.physical_device,
                (&raw mut self.memory_properties).cast(),
            )
        })?;
        let device = unsafe { rsf_core_vkfg_load_device(&self.setup.graphics, &self.setup, false) };
        let Some(device) = (unsafe { device.as_mut() }) else {
            return Err(MISSING_ENTRY_POINT);
        };
        self.gpu = device;
        rsf_core_vkfg_device_memory(device, &self.memory_properties);
        let graphics = &self.setup.graphics;
        let ready = unsafe {
            rsf_core_capture_vk_initialize(
                self.capture,
                graphics.device,
                graphics.device_proc,
                &self.memory_properties,
            )
        };
        if ready == 0 {
            return Err(INIT_FAILED);
        }
        self.wsi_dispatch = Some(unsafe { required(symbol(c"rsfFfxSetVulkanWsiDispatch"))? });
        // All five WSI names use the original device resolver, without alias promotion.
        let mut wsi = [None; 5];
        for (entry, name) in wsi.iter_mut().zip([
            c"vkQueuePresentKHR",
            c"vkAcquireNextImageKHR",
            c"vkGetSwapchainImagesKHR",
            c"vkCreateSwapchainKHR",
            c"vkDestroySwapchainKHR",
        ]) {
            *entry = unsafe { self.lookup_device(name, false)? };
        }
        if wsi.iter().any(Option::is_none) {
            return Err(INIT_FAILED);
        }
        let [present, acquire, images, create, destroy] = wsi;
        unsafe {
            self.native_present = typed(present);
            self.native_acquire = typed(acquire);
            self.native_images = typed(images);
            self.native_create = typed(create);
            self.native_destroy = typed(destroy);
        }
        Ok(())
    }
}

/// Opens a leaf over `module` and binds its entries; a failed step closes the unpublished leaf.
pub(super) unsafe extern "C" fn open(
    setup: *const Setup,
    host: &'static Host,
    module: Handle,
    width: u32,
    height: u32,
    out: *mut Handle,
    resources: *mut Handle,
) -> i32 {
    // The owner validated the setup; the host table outlives the owner.
    let setup = unsafe { setup.read() };
    let allocator = setup.allocator.cast_mut().cast();
    let native = crate::allocate_handle(Native {
        setup,
        host,
        sdk: Sdk::default(),
        swapchain: UnsafeCell::new(ptr::null_mut()),
        effect: UnsafeCell::new(ptr::null_mut()),
        chain: UnsafeCell::new(ptr::null_mut()),
        functions: UnsafeCell::new(unsafe { std::mem::zeroed() }),
        wsi_dispatch: None,
        native_present: None,
        native_acquire: None,
        native_images: None,
        native_create: None,
        native_destroy: None,
        gpu: ptr::null_mut(),
        barrier: None,
        copy: None,
        idle: None,
        memory_properties: vk::PhysicalDeviceMemoryProperties::default(),
        allocator,
        width,
        height,
        capture: rsf_core_capture_vk_create(COMMAND_RETIREMENT),
    });
    let Some(leaf) = (unsafe { native.as_mut() }) else {
        return INIT_FAILED;
    };
    match unsafe { leaf.bind(module) } {
        Ok(()) => {
            unsafe {
                resources.write(leaf.gpu.cast());
                out.write(native.cast());
            }
            OK
        }
        Err(result) => {
            unsafe { close(native.cast()) };
            result
        }
    }
}

pub(super) unsafe extern "C" fn set_wsi(native: Handle, enabled: u32) -> i32 {
    let leaf = unsafe { leaf(native) };
    let on = enabled != 0;
    let dispatch = WsiDispatch {
        struct_size: size_of::<WsiDispatch>() as u32,
        abi_version: WSI_ABI_VERSION,
        device: leaf.device(),
        create: on.then_some(serialized_create as vk::PFN_vkCreateSwapchainKHR),
        destroy: on.then_some(serialized_destroy as vk::PFN_vkDestroySwapchainKHR),
        present: on.then_some(serialized_present as vk::PFN_vkQueuePresentKHR),
        acquire: on.then_some(serialized_acquire as vk::PFN_vkAcquireNextImageKHR),
        images: on.then_some(serialized_images as vk::PFN_vkGetSwapchainImagesKHR),
    };
    unsafe { bound!(leaf.wsi_dispatch)(&dispatch) }
}

pub(super) unsafe extern "C" fn create_swapchain(
    native: Handle,
    chain: *mut u64,
    has_context: *mut u32,
) -> i32 {
    let leaf = unsafe { leaf(native) };
    let setup = &leaf.setup;
    let graphics = &setup.graphics;
    let queue = |index: usize, submit: PFN_vkQueueSubmitFFXAPI| VkQueueInfoFFXAPI {
        queue: setup.queues[index].cast(),
        familyIndex: setup.queue_family,
        submitFunc: submit,
    };
    let mut create = ffxCreateContextDescFrameGenerationSwapChainVK {
        header: header(
            FFX_API_CREATE_CONTEXT_DESC_TYPE_FGSWAPCHAIN_VK,
            ptr::null_mut(),
        ),
        physicalDevice: graphics.physical_device.cast(),
        device: leaf.sdk_device(),
        swapchain: leaf.chain.get(),
        allocator: leaf.allocator,
        // Creation steps run only inside rsf_ffxgvk_create, which borrows create_info.
        createInfo: unsafe { setup.create_info.cast::<VkSwapchainCreateInfoKHR>().read() },
        gameQueue: queue(0, Some(graphics_submit)),
        asyncComputeQueue: queue(1, None),
        presentQueue: queue(2, None),
        imageAcquireQueue: queue(3, None),
    };
    // The SDK calls the create, image and destroy hooks synchronously in here.
    let created =
        unsafe { bound!(leaf.sdk.create)(leaf.swapchain.get(), &mut create.header, ptr::null()) };
    unsafe {
        chain.write(leaf.chain().addr() as u64);
        has_context.write(u32::from(!(*leaf.swapchain.get()).is_null()));
    }
    result(created, INIT_FAILED)
}

pub(super) unsafe extern "C" fn query_swapchain(native: Handle) -> i32 {
    let leaf = unsafe { leaf(native) };
    let functions = leaf.functions.get();
    let queried = unsafe {
        (*functions).header = header(
            FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK,
            ptr::null_mut(),
        );
        bound!(leaf.sdk.query)(leaf.swapchain.get(), &raw mut (*functions).header)
    };
    let functions = leaf.functions();
    if queried != SDK_OK
        || functions.pOutGetSwapchainImagesKHR.is_none()
        || functions.pOutAcquireNextImageKHR.is_none()
        || functions.pOutQueuePresentKHR.is_none()
        || functions.pOutDestroySwapchainFFXAPI.is_none()
    {
        return MISSING_ENTRY_POINT;
    }
    OK
}

pub(super) unsafe extern "C" fn create_effect(native: Handle, has_context: *mut u32) -> i32 {
    let leaf = unsafe { leaf(native) };
    let graphics = &leaf.setup.graphics;
    let info = unsafe {
        &*leaf
            .setup
            .create_info
            .cast::<vk::SwapchainCreateInfoKHR<'_>>()
    };
    let mut backend = ffxCreateBackendVKDesc {
        header: header(FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK, ptr::null_mut()),
        vkDevice: leaf.sdk_device(),
        vkPhysicalDevice: graphics.physical_device.cast(),
        vkDeviceProcAddr: Some(device_proc),
    };
    let mut hudless = ffxCreateContextDescFrameGenerationHudless {
        header: header(
            FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_HUDLESS,
            &raw mut backend.header,
        ),
        hudlessBackBufferFormat: FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM as u32,
    };
    let flag = |enabled: u32, bit: i32| if enabled != 0 { bit as u32 } else { 0 };
    let size = dimensions(leaf.width, leaf.height);
    let mut create = ffxCreateContextDescFrameGeneration {
        header: header(
            FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION,
            &raw mut hudless.header,
        ),
        flags: flag(
            graphics.depth_inverted,
            FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED,
        ) | flag(
            graphics.depth_infinite,
            FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE,
        ) | flag(
            graphics.motion_jittered,
            FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION,
        ),
        displaySize: size,
        maxRenderSize: size,
        backBufferFormat: surface_format(info.image_format.as_raw() as u32),
    };
    let created =
        unsafe { bound!(leaf.sdk.create)(leaf.effect.get(), &mut create.header, ptr::null()) };
    unsafe { has_context.write(u32::from(!(*leaf.effect.get()).is_null())) };
    result(created, INIT_FAILED)
}

pub(super) unsafe extern "C" fn query_provider(native: Handle) -> i32 {
    let leaf = unsafe { leaf(native) };
    let mut provider = ffxQueryGetProviderVersion {
        header: header(
            FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION,
            ptr::null_mut(),
        ),
        versionId: 0,
        versionName: ptr::null(),
    };
    let queried = unsafe { bound!(leaf.sdk.query)(leaf.effect.get(), &mut provider.header) };
    if queried != SDK_OK || provider.versionId == 0 {
        return INIT_FAILED;
    }
    unsafe { rsf_core_capture_vk_provider(leaf.capture, provider.versionId, 3, 1) };
    OK
}

unsafe fn destroy_context(leaf: &Native, context: *mut ffxContext) -> i32 {
    unsafe {
        if !(*context).is_null() && bound!(leaf.sdk.destroy)(context, ptr::null()) != SDK_OK {
            return FEATURE_FAILED;
        }
        context.write(ptr::null_mut());
    }
    OK
}

pub(super) unsafe extern "C" fn destroy_effect(native: Handle) -> i32 {
    let leaf = unsafe { leaf(native) };
    unsafe { destroy_context(leaf, leaf.effect.get()) }
}

pub(super) unsafe extern "C" fn destroy_chain(native: Handle) {
    let leaf = unsafe { leaf(native) };
    let chain = leaf.chain();
    if !chain.is_null()
        && let Some(destroy) = leaf.functions().pOutDestroySwapchainFFXAPI
    {
        unsafe {
            destroy(
                leaf.sdk_device(),
                chain,
                leaf.allocator,
                *leaf.swapchain.get(),
            )
        };
    }
    unsafe { leaf.chain.get().write(ptr::null_mut()) };
}

pub(super) unsafe extern "C" fn destroy_swapchain(native: Handle) -> i32 {
    let leaf = unsafe { leaf(native) };
    unsafe { destroy_context(leaf, leaf.swapchain.get()) }
}

pub(super) unsafe extern "C" fn images(native: Handle, count: *mut u32, output: *mut u64) -> i32 {
    let leaf = unsafe { leaf(native) };
    let images = bound!(leaf.functions().pOutGetSwapchainImagesKHR);
    unsafe { images(leaf.sdk_device(), leaf.chain(), count, output.cast()) }
}

pub(super) unsafe extern "C" fn acquire(
    native: Handle,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
    frame: u64,
) -> i32 {
    let leaf = unsafe { leaf(native) };
    let acquire = bound!(leaf.functions().pOutAcquireNextImageKHR);
    let start = timing_begin();
    let result = unsafe {
        acquire(
            leaf.sdk_device(),
            leaf.chain(),
            timeout,
            ptr::with_exposed_provenance_mut(semaphore as usize),
            ptr::with_exposed_provenance_mut(fence as usize),
            index,
        )
    };
    timing_end("ffx_acquire", frame, start);
    result
}

pub(super) unsafe extern "C" fn prepare(
    native: Handle,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    let leaf = unsafe { leaf(native) };
    // The owner admitted this frame and its record for the call.
    let frame = unsafe { &*frame };
    let record = unsafe { &*frame.record };
    let camera = &record.camera;
    let world = &camera.view_to_world;
    let mut info = ffxDispatchDescFrameGenerationPrepareCameraInfo {
        header: header(
            FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_CAMERAINFO,
            ptr::null_mut(),
        ),
        cameraPosition: [world[12], world[13], world[14]],
        cameraUp: [world[4], world[5], world[6]],
        cameraRight: [world[0], world[1], world[2]],
        cameraForward: [-world[8], -world[9], -world[10]],
    };
    let [jitter_x, jitter_y] = camera.jitter_pixels;
    let mut data = ffxDispatchDescFrameGenerationPrepare {
        header: header(
            FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE,
            &raw mut info.header,
        ),
        frameID: record.frame_id,
        flags: 0,
        commandList: commands,
        renderSize: dimensions(record.render_width, record.render_height),
        jitterOffset: FfxApiFloatCoords2D {
            x: jitter_x,
            y: jitter_y,
        },
        motionVectorScale: FfxApiFloatCoords2D {
            x: frame.motion_scale_x,
            y: frame.motion_scale_y,
        },
        frameTimeDelta: record.frame_time_ms,
        unused_reset: false,
        cameraNear: camera.near_plane,
        cameraFar: camera.far_plane,
        cameraFovAngleVertical: camera.vertical_fov_radians,
        viewSpaceToMetersFactor: 1.0,
        depth: resource(&frame.depth),
        motionVectors: resource(&frame.motion),
    };
    let start = timing_begin();
    let dispatched = unsafe { bound!(leaf.sdk.dispatch)(leaf.effect.get(), &mut data.header) };
    timing_end("ffx_prepare_sdk", record.frame_id, start);
    dispatched as i32
}

fn command_buffer(commands: Handle) -> vk::CommandBuffer {
    vk::CommandBuffer::from_raw(commands as usize as u64)
}

pub(super) unsafe extern "C" fn barrier(native: Handle, commands: Handle, plan: *const Barrier) {
    let leaf = unsafe { leaf(native) };
    unsafe { rsf_core_vkfg_record_barrier(bound!(leaf.barrier), command_buffer(commands), &*plan) };
}

pub(super) unsafe extern "C" fn copy(native: Handle, commands: Handle, plan: *const ImageCopy) {
    let leaf = unsafe { leaf(native) };
    unsafe { rsf_core_vkfg_record_copy(bound!(leaf.copy), command_buffer(commands), &*plan) };
}

pub(super) unsafe fn configure(
    native: Handle,
    enabled: u32,
    color: u64,
    frame: u64,
    stage: &'static str,
) -> i32 {
    let leaf = unsafe { leaf(native) };
    let enabled = enabled != 0;
    let hudless = Image {
        device: leaf.setup.graphics.device,
        image: color,
        format: RGBA8,
        width: leaf.width,
        height: leaf.height,
        layout: SHADER_READ,
        copy_source: 1,
    };
    let mut config = ffxConfigureDescFrameGeneration {
        header: header(FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION, ptr::null_mut()),
        swapChain: leaf.chain().cast(),
        presentCallback: None,
        presentCallbackUserContext: ptr::null_mut(),
        frameGenerationCallback: Some(generate),
        frameGenerationCallbackUserContext: native,
        frameGenerationEnabled: enabled,
        allowAsyncWorkloads: false,
        HUDLessColor: if enabled {
            resource(&hudless)
        } else {
            unsafe { std::mem::zeroed() }
        },
        flags: 0,
        onlyPresentGenerated: false,
        generationRect: FfxApiRect2D {
            left: 0,
            top: 0,
            width: leaf.width as i32,
            height: leaf.height as i32,
        },
        frameID: frame,
    };
    let start = timing_begin();
    let configured = unsafe { bound!(leaf.sdk.configure)(leaf.effect.get(), &mut config.header) };
    timing_end(stage, frame, start);
    result(configured, FEATURE_FAILED)
}

pub(super) unsafe extern "C" fn present(
    native: Handle,
    info: *const c_void,
    frame: u64,
    count: *mut u64,
) -> i32 {
    let leaf = unsafe { leaf(native) };
    let functions = leaf.functions();
    let present = bound!(functions.pOutQueuePresentKHR);
    let start = timing_begin();
    // FrameInterpolationSwapChainVK calls generate inside this call.
    let presented = unsafe { present(leaf.setup.queues[0].cast(), info.cast()) };
    timing_end("ffx_queue_present", frame, start);
    if let Some(last) = functions.pOutGetLastPresentCountFFXAPI {
        unsafe { count.write(last(leaf.chain())) };
    }
    presented
}

unsafe extern "C" fn wait_for_presents(native: *mut c_void) -> i32 {
    let leaf = unsafe { leaf(native) };
    let mut wait = ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK {
        header: header(
            FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK,
            ptr::null_mut(),
        ),
    };
    unsafe { bound!(leaf.sdk.dispatch)(leaf.swapchain.get(), &mut wait.header) as i32 }
}

pub(super) unsafe extern "C" fn drain(native: Handle) -> i32 {
    let leaf = unsafe { leaf(native) };
    unsafe { rsf_core_vkfg_drain(&leaf.setup, wait_for_presents, native, bound!(leaf.idle)) }
}

pub(super) unsafe extern "C" fn capture_wants(native: Handle, frame: u64) -> u32 {
    let leaf = unsafe { leaf(native) };
    u32::from(unsafe { rsf_core_capture_vk_wants(leaf.capture, frame) } != 0)
}

pub(super) unsafe extern "C" fn capture_record(
    native: Handle,
    commands: Handle,
    image: *const Image,
    record: *const Record,
    role: *const c_char,
) {
    let leaf = unsafe { leaf(native) };
    let reset = unsafe { (*record).flags } & RSF_FRAME_FLAG_RESET;
    unsafe {
        rsf_core_capture_vk_record(
            leaf.capture,
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

pub(super) unsafe extern "C" fn capture_flush(native: Handle) {
    let leaf = unsafe { leaf(native) };
    unsafe { rsf_core_capture_vk_flush(leaf.capture, 0, ptr::null_mut()) };
}

pub(super) unsafe extern "C" fn fence_timing(native: Handle, frame: u64) {
    let leaf = unsafe { leaf(native) };
    let stage = if frame == 0 {
        ptr::null()
    } else {
        c"ffx_slot_fence_wait".as_ptr()
    };
    // The resource table is a separate allocation the owner reaches only
    // through this leaf's sequential steps.
    unsafe { rsf_core_vkfg_device_wait_timing(&mut *leaf.gpu, stage, frame) };
}

/// The SDK leaf's steps the owner calls.
pub(super) mod leaf {
    pub(in crate::vulkan_ffx_fg) use super::{
        acquire, barrier, capture_flush, capture_record, capture_wants, close, configure, copy,
        create_effect, create_swapchain, destroy_chain, destroy_effect, destroy_swapchain, drain,
        fence_timing, images, open, prepare, present, query_provider, query_swapchain, set_wsi,
    };
    use crate::vulkan_fg_resources::{Platform as Resources, device::DEVICE_PLATFORM};
    pub(in crate::vulkan_ffx_fg) use crate::vulkan_timing::{timing_begin, timing_end};

    pub(in crate::vulkan_ffx_fg) fn resources() -> &'static Resources {
        &DEVICE_PLATFORM
    }
}
