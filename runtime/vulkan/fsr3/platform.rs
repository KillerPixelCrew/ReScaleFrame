// SPDX-License-Identifier: MIT
//! Windows and pinned FidelityFX Vulkan ABI for the standalone bridge.

use super::*;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

use crate::sdk_abi as abi;
use abi::*;

const BGRA8_UNORM: i32 = 44;
const _: () = {
    assert!(size_of::<VkSwapchainKHR>() == size_of::<Vk>());
    assert!(offset_of!(VkSwapchainCreateInfoKHR, oldSwapchain) == OLD_SWAPCHAIN_OFFSET);
};

static DEVICE_PROC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
const VULKAN_DLL: [u16; 13] = rsf_game_sys::ffi_util::ascii_wide(b"vulkan-1.dll\0");

// The owner resolves and publishes all required addresses before making a call.
macro_rules! sdk_proc {
    ($address:expr, $ty:ty) => {
        std::mem::transmute::<Handle, $ty>($address).expect("bound FidelityFX export")
    };
}

pub(super) unsafe fn resolve(module: Handle, name: *const c_char) -> Handle {
    unsafe { GetProcAddress(module, name.cast()) }
        .map_or(ptr::null_mut(), |address| address as Handle)
}

fn bind_loader() -> bool {
    let module = unsafe { GetModuleHandleW(VULKAN_DLL.as_ptr()) };
    let proc = if module.is_null() {
        ptr::null_mut()
    } else {
        unsafe { resolve(module, c"vkGetDeviceProcAddr".as_ptr()) }
    };
    DEVICE_PROC.store(proc, Ordering::Release);
    !proc.is_null()
}

unsafe extern "C" fn device_proc(device: VkDevice, name: *const c_char) -> PFN_vkVoidFunction {
    let address = DEVICE_PROC.load(Ordering::Acquire);
    let get = unsafe { std::mem::transmute::<Handle, PFN_vkGetDeviceProcAddr>(address) }?;
    let result = unsafe { get(device, name) };
    // The 1.1.4 backend requires the KHR alias even on devices that expose
    // only the promoted Vulkan 1.1 entry.
    if result.is_none()
        && !name.is_null()
        && unsafe { CStr::from_ptr(name) } == c"vkGetBufferMemoryRequirements2KHR"
    {
        unsafe { get(device, c"vkGetBufferMemoryRequirements2".as_ptr()) }
    } else {
        result
    }
}

fn header(kind: u32) -> ffxApiHeader {
    ffxApiHeader {
        type_: u64::from(kind),
        pNext: ptr::null_mut(),
    }
}

fn dimensions(width: u32, height: u32) -> FfxApiDimensions2D {
    FfxApiDimensions2D { width, height }
}

fn backend(device: Handle, physical: Handle) -> ffxCreateBackendVKDesc {
    ffxCreateBackendVKDesc {
        header: header(FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK),
        vkDevice: device.cast(),
        vkPhysicalDevice: physical.cast(),
        vkDeviceProcAddr: Some(device_proc),
    }
}

fn resource(image: &Image, output: bool) -> FfxApiResource {
    // These describe host-owned images in their prepared Vulkan usage.
    let format = match image.format {
        R16G16B16A16_SFLOAT => FFX_API_SURFACE_FORMAT_R16G16B16A16_FLOAT,
        R16G16_SFLOAT => FFX_API_SURFACE_FORMAT_R16G16_FLOAT,
        D32_SFLOAT => FFX_API_SURFACE_FORMAT_R32_FLOAT,
        R8G8B8A8_UNORM => FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM,
        R8_UNORM => FFX_API_SURFACE_FORMAT_R8_UNORM,
        _ => FFX_API_SURFACE_FORMAT_UNKNOWN,
    };
    let usage = if output {
        FFX_API_RESOURCE_USAGE_UAV
    } else if image.format == D32_SFLOAT {
        FFX_API_RESOURCE_USAGE_DEPTHTARGET
    } else {
        FFX_API_RESOURCE_USAGE_READ_ONLY
    };
    FfxApiResource {
        resource: ptr::with_exposed_provenance_mut(image.image as usize),
        description: FfxApiResourceDescription {
            type_: FFX_API_RESOURCE_TYPE_TEXTURE2D as u32,
            format: format as u32,
            __bindgen_anon_1: FfxApiResourceDescription__bindgen_ty_1 { width: image.width },
            __bindgen_anon_2: FfxApiResourceDescription__bindgen_ty_2 {
                height: image.height,
            },
            __bindgen_anon_3: FfxApiResourceDescription__bindgen_ty_3 { depth: 1 },
            mipCount: 1,
            flags: 0,
            usage: usage as u32,
        },
        state: if output {
            FFX_API_RESOURCE_STATE_UNORDERED_ACCESS
        } else {
            FFX_API_RESOURCE_STATE_COMPUTE_READ
        } as u32,
    }
}

#[derive(Default)]
struct SrCapsule {
    effect: ffxContext,
    upscale: ffxCreateContextDescUpscale,
    backend: ffxCreateBackendVKDesc,
}

#[derive(Default)]
struct WsiCapsule {
    context: ffxContext,
    functions: ffxQueryDescSwapchainReplacementFunctionsVK,
}

#[derive(Default)]
struct FgCapsule {
    effect: ffxContext,
    generation: ffxCreateContextDescFrameGeneration,
    hudless: ffxCreateContextDescFrameGenerationHudless,
    backend: ffxCreateBackendVKDesc,
    width: u32,
    height: u32,
}

fn boxed<T: Default, Opaque>() -> *mut Opaque {
    try_box(T::default()).map_or(ptr::null_mut(), |value| Box::into_raw(value).cast())
}

unsafe fn unbox<T, Opaque>(owner: *mut Opaque) {
    drop(unsafe { Box::from_raw(owner.cast::<T>()) });
}

pub(super) unsafe fn allocate() -> *mut Native {
    boxed::<SrCapsule, _>()
}
pub(super) unsafe fn release(owner: *mut Native) {
    unsafe { unbox::<SrCapsule, _>(owner) }
}
pub(super) unsafe fn wsi_allocate() -> *mut WsiNative {
    boxed::<WsiCapsule, _>()
}
pub(super) unsafe fn wsi_release(owner: *mut WsiNative) {
    unsafe { unbox::<WsiCapsule, _>(owner) }
}
pub(super) unsafe fn fg_allocate() -> *mut FgNative {
    boxed::<FgCapsule, _>()
}
pub(super) unsafe fn fg_release(owner: *mut FgNative) {
    unsafe { unbox::<FgCapsule, _>(owner) }
}

pub(super) unsafe fn bind_vulkan(device: Handle) -> u32 {
    if !bind_loader() {
        return 0;
    }
    let requirements =
        unsafe { device_proc(device.cast(), c"vkGetBufferMemoryRequirements2KHR".as_ptr()) };
    VULKAN_LOADER
        | if requirements.is_some() {
            VULKAN_BUFFER_REQUIREMENTS
        } else {
            0
        }
}

pub(super) unsafe fn fg_bind_vulkan() -> u32 {
    u32::from(bind_loader())
}

pub(super) unsafe fn create(owner: *mut Native, address: Handle, request: *const Request) -> i32 {
    let owner = owner.cast::<SrCapsule>();
    unsafe {
        let request = &*request;
        (*owner).backend = backend(request.device, request.physical);
        (*owner).upscale = ffxCreateContextDescUpscale {
            header: ffxApiHeader {
                pNext: &raw mut (*owner).backend.header,
                ..header(FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE)
            },
            flags: (FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE | FFX_UPSCALE_ENABLE_AUTO_EXPOSURE)
                as u32,
            maxRenderSize: dimensions(request.render_width, request.render_height),
            maxUpscaleSize: dimensions(request.width, request.height),
            ..Default::default()
        };
        sdk_proc!(address, PfnFfxCreateContext)(
            &raw mut (*owner).effect,
            &raw mut (*owner).upscale.header,
            ptr::null(),
        ) as i32
    }
}

pub(super) unsafe fn plan(
    address: Handle,
    display_width: u32,
    display_height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    let mut request = ffxQueryDescUpscaleGetRenderResolutionFromQualityMode {
        header: header(FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE),
        displayWidth: display_width,
        displayHeight: display_height,
        qualityMode: quality,
        pOutRenderWidth: render_width,
        pOutRenderHeight: render_height,
    };
    unsafe { sdk_proc!(address, PfnFfxQuery)(ptr::null_mut(), &mut request.header) as i32 }
}

pub(super) unsafe fn evaluate(
    owner: *mut Native,
    address: Handle,
    frame: *const Frame,
    time: f32,
) -> i32 {
    let frame = unsafe { &*frame };
    let command = ffxDispatchDescUpscale {
        header: header(FFX_API_DISPATCH_DESC_TYPE_UPSCALE),
        commandList: frame.commands,
        color: resource(&frame.color, false),
        depth: resource(&frame.depth, false),
        motionVectors: resource(&frame.motion, false),
        reactive: resource(&frame.reactive, false),
        transparencyAndComposition: resource(&frame.composition, false),
        output: resource(&frame.output, true),
        jitterOffset: FfxApiFloatCoords2D {
            x: frame.jitter_x,
            y: frame.jitter_y,
        },
        motionVectorScale: FfxApiFloatCoords2D { x: 1.0, y: 1.0 },
        renderSize: dimensions(frame.color.width, frame.color.height),
        upscaleSize: dimensions(frame.output.width, frame.output.height),
        frameTimeDelta: time,
        preExposure: 1.0,
        reset: frame.reset != 0,
        cameraNear: frame.near_plane,
        cameraFar: frame.far_plane,
        cameraFovAngleVertical: frame.fov_radians,
        viewSpaceToMetersFactor: 1.0,
        ..Default::default()
    };
    unsafe {
        sdk_proc!(address, PfnFfxDispatch)(
            &raw mut (*owner.cast::<SrCapsule>()).effect,
            &command.header,
        ) as i32
    }
}

pub(super) unsafe fn has_effect(owner: *const Native) -> u32 {
    u32::from(!unsafe { (*owner.cast::<SrCapsule>()).effect }.is_null())
}

pub(super) unsafe fn destroy(owner: *mut Native, address: Handle) -> i32 {
    unsafe {
        sdk_proc!(address, PfnFfxDestroyContext)(
            &raw mut (*owner.cast::<SrCapsule>()).effect,
            ptr::null(),
        ) as i32
    }
}

pub(super) unsafe fn wsi_create(
    owner: *mut WsiNative,
    address: Handle,
    request: *const WsiRequest,
    chain: *mut Vk,
) -> i32 {
    let request = unsafe { &*request };
    let mut create_info = unsafe { request.info.cast::<VkSwapchainCreateInfoKHR>().read() };
    create_info.oldSwapchain = ptr::null_mut();
    let queue = |handle: Handle| VkQueueInfoFFXAPI {
        queue: handle.cast(),
        familyIndex: request.queue_family,
        submitFunc: None,
    };
    let mut desc = ffxCreateContextDescFrameGenerationSwapChainVK {
        header: header(FFX_API_CREATE_CONTEXT_DESC_TYPE_FGSWAPCHAIN_VK),
        physicalDevice: request.physical.cast(),
        device: request.device.cast(),
        swapchain: chain.cast(),
        allocator: ptr::null_mut(),
        createInfo: create_info,
        gameQueue: queue(request.game),
        asyncComputeQueue: queue(request.async_compute),
        presentQueue: queue(request.present),
        imageAcquireQueue: queue(request.acquire),
    };
    unsafe {
        sdk_proc!(address, PfnFfxCreateContext)(
            &raw mut (*owner.cast::<WsiCapsule>()).context,
            &mut desc.header,
            ptr::null(),
        ) as i32
    }
}

pub(super) unsafe fn wsi_query(owner: *mut WsiNative, address: Handle) -> i32 {
    let owner = owner.cast::<WsiCapsule>();
    unsafe {
        (*owner).functions.header = header(FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK);
        sdk_proc!(address, PfnFfxQuery)(
            &raw mut (*owner).context,
            &raw mut (*owner).functions.header,
        ) as i32
    }
}

pub(super) unsafe fn wsi_complete(owner: *const WsiNative) -> u32 {
    let ops = unsafe { &(*owner.cast::<WsiCapsule>()).functions };
    u32::from(
        ops.pOutCreateSwapchainFFXAPI.is_some()
            && ops.pOutDestroySwapchainFFXAPI.is_some()
            && ops.pOutGetSwapchainImagesKHR.is_some()
            && ops.pOutAcquireNextImageKHR.is_some()
            && ops.pOutQueuePresentKHR.is_some(),
    )
}

pub(super) unsafe fn wsi_has_context(owner: *const WsiNative) -> u32 {
    u32::from(!unsafe { (*owner.cast::<WsiCapsule>()).context }.is_null())
}

pub(super) unsafe fn wsi_destroy(owner: *mut WsiNative, address: Handle) -> i32 {
    unsafe {
        sdk_proc!(address, PfnFfxDestroyContext)(
            &raw mut (*owner.cast::<WsiCapsule>()).context,
            ptr::null(),
        ) as i32
    }
}

pub(super) unsafe fn wsi_wait(owner: *mut WsiNative, address: Handle) -> i32 {
    let wait = ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK {
        header: header(FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK),
    };
    unsafe {
        sdk_proc!(address, PfnFfxDispatch)(
            &raw mut (*owner.cast::<WsiCapsule>()).context,
            &wait.header,
        ) as i32
    }
}

pub(super) unsafe fn wsi_register_ui(
    owner: *mut WsiNative,
    address: Handle,
    frame: *const FgFrame,
) -> i32 {
    let mut ui = ffxConfigureDescFrameGenerationSwapChainRegisterUiResourceVK {
        header: header(FFX_API_CONFIGURE_DESC_TYPE_FGSWAPCHAIN_REGISTERUIRESOURCE_VK),
        ..Default::default()
    };
    if let Some(frame) = unsafe { frame.as_ref() } {
        ui.uiResource = resource(&frame.ui, false);
        ui.flags = (FFX_FRAMEGENERATION_UI_COMPOSITION_FLAG_USE_PREMUL_ALPHA
            | FFX_FRAMEGENERATION_UI_COMPOSITION_FLAG_ENABLE_INTERNAL_UI_DOUBLE_BUFFERING)
            as u32;
    }
    unsafe {
        sdk_proc!(address, PfnFfxConfigure)(
            &raw mut (*owner.cast::<WsiCapsule>()).context,
            &ui.header,
        ) as i32
    }
}

fn vk_handle<T>(handle: Vk) -> *mut T {
    ptr::with_exposed_provenance_mut(handle as usize)
}

pub(super) unsafe fn wsi_recreate(
    owner: *mut WsiNative,
    device: Handle,
    request: *const u8,
    chain: *mut Vk,
) -> i32 {
    let owner = owner.cast::<WsiCapsule>();
    unsafe {
        let Some(recreate) = (*owner).functions.pOutCreateSwapchainFFXAPI else {
            return super::VK_ERROR_INITIALIZATION_FAILED;
        };
        recreate(
            device.cast(),
            request.cast(),
            ptr::null(),
            chain.cast(),
            (*owner).context,
        )
    }
}

pub(super) unsafe fn wsi_get_images(
    owner: *mut WsiNative,
    device: Handle,
    chain: Vk,
    count: *mut u32,
    images: *mut Vk,
) -> i32 {
    unsafe {
        let Some(images_fn) = (*owner.cast::<WsiCapsule>())
            .functions
            .pOutGetSwapchainImagesKHR
        else {
            return super::VK_ERROR_INITIALIZATION_FAILED;
        };
        images_fn(device.cast(), vk_handle(chain), count, images.cast())
    }
}

pub(super) unsafe fn wsi_acquire(
    owner: *mut WsiNative,
    device: Handle,
    chain: Vk,
    timeout: u64,
    semaphore: Vk,
    fence: Vk,
    index: *mut u32,
) -> i32 {
    unsafe {
        let Some(acquire) = (*owner.cast::<WsiCapsule>())
            .functions
            .pOutAcquireNextImageKHR
        else {
            return super::VK_ERROR_INITIALIZATION_FAILED;
        };
        acquire(
            device.cast(),
            vk_handle(chain),
            timeout,
            vk_handle(semaphore),
            vk_handle(fence),
            index,
        )
    }
}

pub(super) unsafe fn wsi_present(owner: *mut WsiNative, queue: Handle, info: *const c_void) -> i32 {
    unsafe {
        let Some(present) = (*owner.cast::<WsiCapsule>()).functions.pOutQueuePresentKHR else {
            return super::VK_ERROR_INITIALIZATION_FAILED;
        };
        present(queue.cast(), info.cast())
    }
}

pub(super) unsafe fn wsi_destroy_chain(owner: *mut WsiNative, device: Handle, chain: Vk) -> i32 {
    let owner = owner.cast::<WsiCapsule>();
    unsafe {
        let Some(destroy) = (*owner).functions.pOutDestroySwapchainFFXAPI else {
            return -1;
        };
        destroy(
            device.cast(),
            vk_handle(chain),
            ptr::null(),
            (*owner).context,
        );
    }
    0
}

pub(super) unsafe fn wsi_last_present_count(owner: *mut WsiNative, chain: Vk) -> u64 {
    unsafe {
        (*owner.cast::<WsiCapsule>())
            .functions
            .pOutGetLastPresentCountFFXAPI
    }
    .map_or(0, |count| unsafe { count(vk_handle(chain)) })
}

pub(super) unsafe fn fg_create(
    owner: *mut FgNative,
    address: Handle,
    request: *const FgRequest,
) -> i32 {
    let owner = owner.cast::<FgCapsule>();
    unsafe {
        let request = &*request;
        (*owner).width = request.width;
        (*owner).height = request.height;
        (*owner).backend = backend(request.device, request.physical);
        // The SDK retains this linked descriptor chain. Build it in place.
        (*owner).hudless = ffxCreateContextDescFrameGenerationHudless {
            header: ffxApiHeader {
                pNext: &raw mut (*owner).backend.header,
                ..header(FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_HUDLESS)
            },
            hudlessBackBufferFormat: FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM as u32,
        };
        let color = if request.back_buffer_format == BGRA8_UNORM {
            FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM
        } else {
            FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM
        };
        (*owner).generation = ffxCreateContextDescFrameGeneration {
            header: ffxApiHeader {
                pNext: &raw mut (*owner).hudless.header,
                ..header(FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION)
            },
            displaySize: dimensions(request.width, request.height),
            maxRenderSize: dimensions(request.width, request.height),
            backBufferFormat: color as u32,
            ..Default::default()
        };
        sdk_proc!(address, PfnFfxCreateContext)(
            &raw mut (*owner).effect,
            &raw mut (*owner).generation.header,
            ptr::null(),
        ) as i32
    }
}

pub(super) unsafe fn fg_has_effect(owner: *const FgNative) -> u32 {
    u32::from(!unsafe { (*owner.cast::<FgCapsule>()).effect }.is_null())
}

pub(super) unsafe fn fg_destroy(owner: *mut FgNative, address: Handle) -> i32 {
    unsafe {
        sdk_proc!(address, PfnFfxDestroyContext)(
            &raw mut (*owner.cast::<FgCapsule>()).effect,
            ptr::null(),
        ) as i32
    }
}

unsafe extern "C" fn generate_on_present(
    params: *mut ffxDispatchDescFrameGeneration,
    user: Handle,
) -> ffxReturnCode_t {
    let address = sdk().dispatch;
    if user.is_null() || params.is_null() || address.is_null() {
        return FFX_API_RETURN_ERROR_PARAMETER as u32;
    }
    // The proxy drains callbacks before releasing the capsule.
    unsafe {
        sdk_proc!(address, PfnFfxDispatch)(
            &raw mut (*user.cast::<FgCapsule>()).effect,
            &raw const (*params).header,
        )
    }
}

pub(super) unsafe fn fg_disable(owner: *mut FgNative, address: Handle, chain: Vk) -> i32 {
    let config = ffxConfigureDescFrameGeneration {
        header: header(FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION),
        swapChain: vk_handle(chain),
        ..Default::default()
    };
    unsafe {
        sdk_proc!(address, PfnFfxConfigure)(
            &raw mut (*owner.cast::<FgCapsule>()).effect,
            &config.header,
        ) as i32
    }
}

unsafe fn rect(owner: *const FgCapsule) -> FfxApiRect2D {
    FfxApiRect2D {
        left: 0,
        top: 0,
        width: unsafe { (*owner).width } as i32,
        height: unsafe { (*owner).height } as i32,
    }
}

pub(super) unsafe fn fg_configure(
    owner: *mut FgNative,
    address: Handle,
    frame: *const FgFrame,
    chain: Vk,
) -> i32 {
    let owner = owner.cast::<FgCapsule>();
    let frame = unsafe { &*frame };
    let mut config = ffxConfigureDescFrameGeneration {
        header: header(FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION),
        frameGenerationEnabled: true,
        generationRect: unsafe { rect(owner) },
        frameID: frame.frame_id,
        ..Default::default()
    };
    if frame.hudless.image != 0 {
        config.HUDLessColor = resource(&frame.hudless, false);
    }
    if chain == 0 {
        config.flags = FFX_FRAMEGENERATION_FLAG_NO_SWAPCHAIN_CONTEXT_NOTIFY as u32;
    } else {
        config.swapChain = vk_handle(chain);
        config.frameGenerationCallback = Some(generate_on_present);
        config.frameGenerationCallbackUserContext = owner.cast();
    }
    unsafe { sdk_proc!(address, PfnFfxConfigure)(&raw mut (*owner).effect, &config.header) as i32 }
}

pub(super) unsafe fn fg_prepare(
    owner: *mut FgNative,
    address: Handle,
    frame: *const FgFrame,
    time: f32,
) -> i32 {
    let frame = unsafe { &*frame };
    let [px, py, pz, ux, uy, uz, rx, ry, rz, fx, fy, fz] = frame.camera;
    let mut camera = ffxDispatchDescFrameGenerationPrepareCameraInfo {
        header: header(FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_CAMERAINFO),
        cameraPosition: [px, py, pz],
        cameraUp: [ux, uy, uz],
        cameraRight: [rx, ry, rz],
        cameraForward: [fx, fy, fz],
    };
    let prepare = ffxDispatchDescFrameGenerationPrepare {
        header: ffxApiHeader {
            pNext: &mut camera.header,
            ..header(FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE)
        },
        frameID: frame.frame_id,
        commandList: frame.commands,
        renderSize: dimensions(frame.depth.width, frame.depth.height),
        jitterOffset: FfxApiFloatCoords2D {
            x: frame.jitter_x,
            y: frame.jitter_y,
        },
        // The source image has already been flipped into upright space.
        motionVectorScale: FfxApiFloatCoords2D { x: 1.0, y: -1.0 },
        frameTimeDelta: time,
        cameraNear: frame.near_plane,
        cameraFar: frame.far_plane,
        cameraFovAngleVertical: frame.fov_radians,
        viewSpaceToMetersFactor: 1.0,
        depth: resource(&frame.depth, false),
        motionVectors: resource(&frame.motion, false),
        ..Default::default()
    };
    unsafe {
        sdk_proc!(address, PfnFfxDispatch)(
            &raw mut (*owner.cast::<FgCapsule>()).effect,
            &prepare.header,
        ) as i32
    }
}

pub(super) unsafe fn fg_generate(
    owner: *mut FgNative,
    address: Handle,
    frame: *const FgFrame,
) -> i32 {
    let owner = owner.cast::<FgCapsule>();
    let frame = unsafe { &*frame };
    let mut command = ffxDispatchDescFrameGeneration {
        header: header(FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION),
        commandList: frame.commands,
        presentColor: resource(&frame.color, false),
        numGeneratedFrames: 1,
        reset: frame.reset != 0,
        backbufferTransferFunction: FFX_API_BACKBUFFER_TRANSFER_FUNCTION_SRGB as u32,
        minMaxLuminance: [0.0, 1.0],
        generationRect: unsafe { rect(owner) },
        frameID: frame.frame_id,
        ..Default::default()
    };
    command.outputs[0] = resource(&frame.output, true);
    unsafe { sdk_proc!(address, PfnFfxDispatch)(&raw mut (*owner).effect, &command.header) as i32 }
}
