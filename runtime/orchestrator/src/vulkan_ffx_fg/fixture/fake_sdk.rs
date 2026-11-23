// SPDX-License-Identifier: MIT
//! Fake FidelityFX 3 SDK module for the production Rust SDK leaf. The fixture
//! host table replaces module loading and symbol lookup; everything else, the
//! SDK descriptor records, the WSI hooks, the backend resolver, the game-queue
//! submit and the generation callback, comes from the production leaf. The
//! WSI registration keeps the last registered hooks, as the corrected SDK's
//! copied table entry does, so a late hook can still be driven after teardown.
use super::fake_gpu::{
    rsf_fixture_ffx_acquire, rsf_fixture_ffx_images, rsf_fixture_ffx_lock, rsf_fixture_ffx_lookup,
    rsf_fixture_ffx_present_count, rsf_fixture_ffx_submit, rsf_fixture_ffx_unlock,
    rsf_fixture_wsi_entry, rsf_fixture_wsi_lookup, rsf_fixture_wsi_route_state, state,
};
use super::rsf_fixture_ffx_refused_second_owner;
use crate::sdk_module::Loader;
use crate::sys::{RSF_FGVK_ABI_VERSION, RSF_SRVK_ABI_VERSION, rsf_fgvk_setup, rsf_vk_function};
use crate::vulkan_ffx_fg::sdk::{Host, rsf_ffxgvk_guard_memory, rsf_ffxgvk_guard_resolve};
use crate::vulkan_ffx_fg::{
    Owner, rsf_core_ffxgvk_create_sdk, rsf_core_ffxgvk_wsi_borrow, rsf_core_ffxgvk_wsi_return,
};
use crate::vulkan_fg_resources::Platform as rsf_vkfg_platform;
use crate::vulkan_fg_resources::device::{
    DEVICE_PLATFORM, Device, rsf_core_vkfg_device_memory, rsf_core_vkfg_load_device,
};
use crate::vulkan_fg_wsi::Callback;
use ash::vk::{self, Handle};
use rsf_game_sys::backend_result::{NOT_SUPPORTED as FIXTURE_NOT_SUPPORTED, OK as FIXTURE_OK};
use rsf_vk_fsr3::sdk_abi as sdk;
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;

const DEVICE: usize = 1;
const INSTANCE: usize = 7;
const PHYSICAL_DEVICE: usize = 8;
const MODULE: usize = 9;
const CHAIN: usize = 3;
const SWAPCHAIN_CONTEXT: usize = 4;
const EFFECT_CONTEXT: usize = 5;
const GRAPHICS_QUEUE: usize = 20;
const SDK_OK: sdk::ffxReturnCode_t = sdk::FFX_API_RETURN_OK as sdk::ffxReturnCode_t;
const SDK_ERROR: sdk::ffxReturnCode_t = sdk::FFX_API_RETURN_ERROR as sdk::ffxReturnCode_t;
const SDK_PARAMETER: sdk::ffxReturnCode_t =
    sdk::FFX_API_RETURN_ERROR_PARAMETER as sdk::ffxReturnCode_t;

fn require(value: bool, text: &str) {
    if !value {
        eprintln!("{text}");
        std::process::exit(1);
    }
}

fn handle<T>(value: usize) -> *mut T {
    ptr::with_exposed_provenance_mut(value)
}

/// The corrected SDK's rsf_ffx_wsi_dispatch.
#[repr(C)]
#[derive(Clone, Copy)]
struct WsiDispatch {
    struct_size: u32,
    abi_version: u32,
    device: vk::Device,
    create: Option<vk::PFN_vkCreateSwapchainKHR>,
    destroy: Option<vk::PFN_vkDestroySwapchainKHR>,
    present: Option<vk::PFN_vkQueuePresentKHR>,
    acquire: Option<vk::PFN_vkAcquireNextImageKHR>,
    images: Option<vk::PFN_vkGetSwapchainImagesKHR>,
}

// Records the SDK sees from the production leaf. The scenario, the SDK calls
// and the reentrant callbacks all run on the initiating thread.
struct Sdk {
    swapchain_slot: *mut sdk::ffxContext,
    effect_slot: *mut sdk::ffxContext,
    native: *mut c_void,
    chain_live: bool,
    callback: sdk::FfxApiFrameGenerationDispatchFunc,
    callback_user: *mut c_void,
    device_proc: sdk::PFN_vkGetDeviceProcAddr,
    submit: sdk::PFN_vkQueueSubmitFFXAPI,
    hooks: Option<WsiDispatch>,
    gpu: *mut Device,
}

thread_local! {
    static SDK: UnsafeCell<Sdk> = const {
        UnsafeCell::new(Sdk {
            swapchain_slot: ptr::null_mut(),
            effect_slot: ptr::null_mut(),
            native: ptr::null_mut(),
            chain_live: false,
            callback: None,
            callback_user: ptr::null_mut(),
            device_proc: None,
            submit: None,
            hooks: None,
            gpu: ptr::null_mut(),
        })
    };
}

// Present can synchronously reenter teardown, so use a raw pointer.
fn recorded() -> *mut Sdk {
    SDK.with(UnsafeCell::get)
}

fn routed() -> *mut c_void {
    let native = rsf_core_ffxgvk_wsi_borrow(ptr::without_provenance_mut(DEVICE), 1);
    if !native.is_null() {
        rsf_core_ffxgvk_wsi_return();
    }
    native
}

unsafe extern "C" fn create_context(
    context: *mut sdk::ffxContext,
    header: *mut sdk::ffxCreateContextDescHeader,
    _: *const sdk::ffxAllocationCallbacks,
) -> sdk::ffxReturnCode_t {
    let sdk_state = recorded();
    match unsafe { (*header).type_ } as u32 {
        sdk::FFX_API_CREATE_CONTEXT_DESC_TYPE_FGSWAPCHAIN_VK => unsafe {
            let create = &*header.cast::<sdk::ffxCreateContextDescFrameGenerationSwapChainVK>();
            require(
                create.device.addr() == DEVICE
                    && create.physicalDevice.addr() == PHYSICAL_DEVICE
                    && create.gameQueue.queue.addr() == GRAPHICS_QUEUE
                    && create.gameQueue.submitFunc.is_some()
                    && create.imageAcquireQueue.queue.addr() == GRAPHICS_QUEUE + 3,
                "the swapchain context receives the device, queues and game-queue submit",
            );
            require(
                !routed().is_null(),
                "the route lends the leaf before the SDK calls its WSI hooks",
            );
            create.swapchain.write(handle(CHAIN));
            context.write(handle(SWAPCHAIN_CONTEXT));
            (*sdk_state).swapchain_slot = context;
            (*sdk_state).native = routed();
            (*sdk_state).chain_live = true;
            (*sdk_state).submit = create.gameQueue.submitFunc;
        },
        sdk::FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION => unsafe {
            let create = &*header.cast::<sdk::ffxCreateContextDescFrameGeneration>();
            let hudless = &*create
                .header
                .pNext
                .cast::<sdk::ffxCreateContextDescFrameGenerationHudless>();
            let backend = &*hudless.header.pNext.cast::<sdk::ffxCreateBackendVKDesc>();
            require(
                hudless.header.type_
                    == u64::from(sdk::FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_HUDLESS)
                    && backend.header.type_
                        == u64::from(sdk::FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK)
                    && backend.vkDevice.addr() == DEVICE
                    && backend.vkDeviceProcAddr.is_some(),
                "the effect chains HUD-less and backend descriptors",
            );
            require(
                create.flags == sdk::FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED as u32
                    && create.displaySize.width == 640
                    && create.backBufferFormat == sdk::FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM as u32,
                "the effect describes the fixture swapchain and depth convention",
            );
            context.write(handle(EFFECT_CONTEXT));
            (*sdk_state).effect_slot = context;
            (*sdk_state).device_proc = backend.vkDeviceProcAddr;
        },
        _ => require(false, "unexpected SDK context"),
    }
    SDK_OK
}

unsafe extern "C" fn query(
    context: *mut sdk::ffxContext,
    header: *mut sdk::ffxQueryDescHeader,
) -> sdk::ffxReturnCode_t {
    let sdk_state = recorded();
    macro_rules! entry {
        ($function:expr, $ty:ty) => {
            // Expanded inside the unsafe query arm below.
            Some(std::mem::transmute::<*const (), $ty>(
                $function as *const (),
            ))
        };
    }
    match unsafe { (*header).type_ } as u32 {
        sdk::FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK => unsafe {
            require(
                context == (*sdk_state).swapchain_slot,
                "replacement functions come from the swapchain context",
            );
            let functions = &mut *header.cast::<sdk::ffxQueryDescSwapchainReplacementFunctionsVK>();
            functions.pOutQueuePresentKHR = Some(present);
            functions.pOutGetSwapchainImagesKHR = entry!(
                rsf_fixture_ffx_images,
                unsafe extern "C" fn(
                    sdk::VkDevice,
                    sdk::VkSwapchainKHR,
                    *mut u32,
                    *mut sdk::VkImage,
                ) -> sdk::VkResult
            );
            functions.pOutAcquireNextImageKHR = entry!(
                rsf_fixture_ffx_acquire,
                unsafe extern "C" fn(
                    sdk::VkDevice,
                    sdk::VkSwapchainKHR,
                    u64,
                    sdk::VkSemaphore,
                    sdk::VkFence,
                    *mut u32,
                ) -> sdk::VkResult
            );
            functions.pOutDestroySwapchainFFXAPI = Some(destroy_chain);
            functions.pOutGetLastPresentCountFFXAPI = entry!(
                rsf_fixture_ffx_present_count,
                unsafe extern "C" fn(sdk::VkSwapchainKHR) -> u64
            );
        },
        sdk::FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION => unsafe {
            require(
                context == (*sdk_state).effect_slot,
                "the provider version comes from the effect",
            );
            (*header.cast::<sdk::ffxQueryGetProviderVersion>()).versionId = 1;
        },
        _ => require(false, "unexpected SDK query"),
    }
    SDK_OK
}

unsafe extern "C" fn dispatch(
    context: *mut sdk::ffxContext,
    header: *const sdk::ffxDispatchDescHeader,
) -> sdk::ffxReturnCode_t {
    let fixture = state();
    let sdk_state = recorded();
    unsafe {
        match (*header).type_ as u32 {
            sdk::FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE => {
                let request = &*header.cast::<sdk::ffxDispatchDescFrameGenerationPrepare>();
                require(
                    context == (*sdk_state).effect_slot,
                    "prepare reaches the effect context",
                );
                require(
                    request.motionVectorScale.x == 640.0
                        && request.depth.resource.addr() == 12
                        && request.cameraFar == 100.0,
                    "prepare describes the admitted guides and camera",
                );
                // A failing SDK call can already have recorded GPU work.
                (*fixture).sdk_records += 1;
                if (*fixture).reject_prepare {
                    SDK_PARAMETER
                } else {
                    SDK_OK
                }
            }
            sdk::FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION => {
                let request = &*header.cast::<sdk::ffxDispatchDescFrameGeneration>();
                if (*fixture).check_reset {
                    require(
                        request.reset == (*fixture).expected_reset,
                        "camera reset reaches SDK dispatch without changing activation",
                    );
                }
                if (*fixture).reject_generate {
                    return SDK_ERROR;
                }
                (*fixture).generated_frame = request.frameID;
                (*fixture).generated += 1;
                SDK_OK
            }
            sdk::FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK => {
                require(
                    context == (*sdk_state).swapchain_slot
                        && (*fixture).locks == (*fixture).unlocks + 1,
                    "presents drain on the swapchain under the queue lock",
                );
                (*fixture).drains += 1;
                if (*fixture).reject_drain {
                    SDK_ERROR
                } else {
                    SDK_OK
                }
            }
            _ => {
                require(false, "unexpected SDK dispatch");
                SDK_ERROR
            }
        }
    }
}

unsafe extern "C" fn configure(
    context: *mut sdk::ffxContext,
    header: *const sdk::ffxConfigureDescHeader,
) -> sdk::ffxReturnCode_t {
    let fixture = state();
    let sdk_state = recorded();
    unsafe {
        let request = &*header.cast::<sdk::ffxConfigureDescFrameGeneration>();
        require(
            context == (*sdk_state).effect_slot
                && request.header.type_
                    == u64::from(sdk::FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION)
                && request.swapChain.addr() == CHAIN
                && !request.allowAsyncWorkloads,
            "configure targets the owned presenter",
        );
        let enabled = request.frameGenerationEnabled;
        if enabled {
            require(
                request.frameGenerationCallback.is_some()
                    && request.frameGenerationCallbackUserContext == (*sdk_state).native
                    && !request.HUDLessColor.resource.is_null(),
                "enabled presenter receives its callback and leased HUD-less input",
            );
        }
        if (*fixture).enabled != enabled {
            (*fixture).activations += 1;
        }
        (*fixture).enabled = enabled;
        (*fixture).configured_frame = request.frameID;
        (*sdk_state).callback = request.frameGenerationCallback;
        (*sdk_state).callback_user = request.frameGenerationCallbackUserContext;
        (*fixture).configurations += 1;
    }
    SDK_OK
}

unsafe extern "C" fn destroy_context(
    context: *mut sdk::ffxContext,
    _: *const sdk::ffxAllocationCallbacks,
) -> sdk::ffxReturnCode_t {
    let fixture = state();
    let sdk_state = recorded();
    unsafe {
        if context == (*sdk_state).effect_slot {
            if (*fixture).reject_effect_destroy {
                return SDK_ERROR;
            }
            (*fixture).effects_destroyed += 1;
        } else {
            require(
                context == (*sdk_state).swapchain_slot && !(*sdk_state).chain_live,
                "swapchain context outlives its chain",
            );
            if (*fixture).reject_swapchain_destroy {
                return SDK_ERROR;
            }
            (*fixture).swapchains_destroyed += 1;
        }
    }
    SDK_OK
}

unsafe extern "C" fn present(
    queue: sdk::VkQueue,
    _: *const sdk::VkPresentInfoKHR,
) -> sdk::VkResult {
    let fixture = state();
    let sdk_state = recorded();
    require(
        queue.addr() == GRAPHICS_QUEUE,
        "presents through the graphics queue",
    );
    unsafe {
        (*fixture).present_calls += 1;
        if (*fixture).enabled {
            // FrameInterpolationSwapChainVK calls the generation callback inside queuePresent.
            let callback = (*sdk_state).callback.expect("configured callback");
            let user = (*sdk_state).callback_user;
            let mut request = sdk::ffxDispatchDescFrameGeneration {
                header: sdk::ffxApiHeader {
                    type_: u64::from(sdk::FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION),
                    pNext: ptr::null_mut(),
                },
                frameID: (*fixture).configured_frame + 1,
                ..std::mem::zeroed()
            };
            require(
                callback(&mut request, user) == SDK_PARAMETER,
                "generation callback refuses a different frame than the configured source",
            );
            request.frameID = (*fixture).configured_frame;
            request.numGeneratedFrames = 1;
            let expected = if (*fixture).reject_generate {
                SDK_ERROR
            } else {
                SDK_OK
            };
            require(
                callback(&mut request, user) == expected,
                "generation callback returns the SDK result for the leased source frame",
            );
        }
        (*fixture).present_result
    }
}

unsafe extern "C" fn destroy_chain(
    _: sdk::VkDevice,
    chain: sdk::VkSwapchainKHR,
    _: *const sdk::VkAllocationCallbacks,
    context: *mut c_void,
) {
    let sdk_state = recorded();
    require(
        chain.addr() == CHAIN && context.addr() == SWAPCHAIN_CONTEXT,
        "chain destroyed with its swapchain context",
    );
    unsafe {
        (*sdk_state).chain_live = false;
        (*state()).chains_destroyed += 1;
    }
}

unsafe extern "C" fn set_wsi(setup: *const WsiDispatch) -> i32 {
    let setup = unsafe { &*setup };
    require(
        setup.struct_size as usize == size_of::<WsiDispatch>()
            && setup.abi_version == 2
            && setup.device.as_raw() as usize == DEVICE,
        "the dispatch uses this device and the corrected SDK ABI",
    );
    let fixture = rsf_fixture_wsi_route_state();
    if setup.create.is_some() {
        require(
            setup.destroy.is_some()
                && setup.present.is_some()
                && setup.acquire.is_some()
                && setup.images.is_some(),
            "registration publishes every typed host hook",
        );
        require(
            routed().is_null(),
            "the route publishes its leaf only after registration succeeds",
        );
        unsafe {
            (*recorded()).hooks = Some(*setup);
            (*fixture).registered += 1;
        }
        return 0;
    }
    require(
        setup.destroy.is_none()
            && setup.present.is_none()
            && setup.acquire.is_none()
            && setup.images.is_none(),
        "teardown clears every hook",
    );
    require(
        routed().is_null(),
        "the route lends no leaf during unregistration",
    );
    require(
        rsf_fixture_ffx_refused_second_owner() == FIXTURE_NOT_SUPPORTED,
        "admission is held through unregistration",
    );
    unsafe {
        if (*fixture).refuse_unregister {
            return -1;
        }
        (*fixture).unregistered += 1;
    }
    0
}

unsafe extern "C" fn load(_: *const u16) -> *mut c_void {
    handle(MODULE)
}

unsafe extern "C" fn unload(module: *mut c_void) {
    require(module.addr() == MODULE, "the leaf unloads its module");
}

unsafe extern "C" fn symbol(module: *mut c_void, name: *const c_char) -> rsf_vk_function {
    require(
        module.addr() == MODULE,
        "SDK exports come from the loaded module",
    );
    let entry: *const () = match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"ffxConfigure" => configure as *const (),
        b"ffxCreateContext" => create_context as *const (),
        b"ffxDestroyContext" => destroy_context as *const (),
        b"ffxQuery" => query as *const (),
        b"ffxDispatch" => dispatch as *const (),
        b"rsfFfxSetVulkanWsiDispatch" => set_wsi as *const (),
        _ => return None,
    };
    Some(unsafe { std::mem::transmute::<*const (), unsafe extern "C" fn()>(entry) })
}

unsafe extern "C" fn scope(call: Callback, user: *mut c_void) -> i32 {
    unsafe { call(user) }
}

unsafe extern "C" fn queue_present(
    call: rsf_vk_function,
    queue: *mut c_void,
    info: *const c_void,
) -> i32 {
    let present = unsafe {
        std::mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkQueuePresentKHR>(
            call.expect("native present"),
        )
    };
    unsafe { present(vk::Queue::from_raw(queue.addr() as u64), info.cast()) }.as_raw()
}

// The production guards (vulkan_ffx_fg/sdk.rs) contain resolver exceptions.
static HOST: Host = Host {
    loader: Loader { load, unload },
    symbol,
    resolve: rsf_ffxgvk_guard_resolve,
    memory_properties: rsf_ffxgvk_guard_memory,
    without_dlssg: scope,
    wsi_call: scope,
    queue_present,
};

unsafe extern "system" fn memory_properties(
    physical_device: vk::PhysicalDevice,
    out: *mut vk::PhysicalDeviceMemoryProperties,
) {
    require(
        physical_device.as_raw() as usize == PHYSICAL_DEVICE,
        "memory properties come from the fixture GPU",
    );
    let mut memory = vk::PhysicalDeviceMemoryProperties {
        memory_type_count: 1,
        ..Default::default()
    };
    memory.memory_types[0].property_flags = vk::MemoryPropertyFlags::DEVICE_LOCAL;
    unsafe { out.write(memory) };
}

unsafe extern "C" fn instance_proc(instance: *mut c_void, name: *const c_char) -> rsf_vk_function {
    require(
        instance.addr() == INSTANCE,
        "instance lookups use the fixture instance",
    );
    (unsafe { CStr::from_ptr(name) } == c"vkGetPhysicalDeviceMemoryProperties").then(|| unsafe {
        std::mem::transmute::<vk::PFN_vkGetPhysicalDeviceMemoryProperties, unsafe extern "C" fn()>(
            memory_properties,
        )
    })
}

// The WSI names resolve to the native fakes; the promoted-name pair counts
// lookups for the backend resolver; everything else is the fake GPU.
unsafe extern "C" fn device_proc(device: *mut c_void, name: *const c_char) -> rsf_vk_function {
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkQueuePresentKHR"
        | b"vkAcquireNextImageKHR"
        | b"vkGetSwapchainImagesKHR"
        | b"vkCreateSwapchainKHR"
        | b"vkDestroySwapchainKHR" => unsafe { rsf_fixture_wsi_entry(name) },
        b"vkBindImageMemory2KHR" | b"vkBindImageMemory2" => unsafe {
            rsf_fixture_wsi_lookup(device, name)
        },
        _ => unsafe { rsf_fixture_ffx_lookup(device, name) },
    }
}

struct Setup {
    // Boxed so create_info keeps its address after the setup moves.
    _info: Box<vk::SwapchainCreateInfoKHR<'static>>,
    value: rsf_fgvk_setup,
}

impl Setup {
    fn new() -> Self {
        let info = Box::new(
            vk::SwapchainCreateInfoKHR::default()
                .image_extent(vk::Extent2D {
                    width: 640,
                    height: 360,
                })
                .image_format(vk::Format::R8G8B8A8_UNORM)
                .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
                .image_array_layers(1),
        );
        let mut value = rsf_fgvk_setup {
            struct_size: size_of::<rsf_fgvk_setup>() as u32,
            abi_version: RSF_FGVK_ABI_VERSION,
            ..Default::default()
        };
        let graphics = &mut value.graphics;
        graphics.struct_size = size_of_val(graphics) as u32;
        graphics.abi_version = RSF_SRVK_ABI_VERSION;
        graphics.instance = handle(INSTANCE);
        graphics.physical_device = handle(PHYSICAL_DEVICE);
        graphics.device = handle(DEVICE);
        graphics.instance_proc = Some(instance_proc);
        graphics.device_proc = Some(device_proc);
        graphics.runtime_directory_utf8 = c"C:\\fake".as_ptr();
        graphics.depth_inverted = 1;
        value.queues = std::array::from_fn(|index| handle(GRAPHICS_QUEUE + index));
        value.create_info = ptr::from_ref(&*info).cast();
        value.graphics_submit = Some(rsf_fixture_ffx_submit);
        value.lock_queues = Some(rsf_fixture_ffx_lock);
        value.unlock_queues = Some(rsf_fixture_ffx_unlock);
        Self { _info: info, value }
    }
}

pub(super) unsafe fn rsf_fixture_ffx_start(
    route: bool,
    out: *mut *mut Owner,
    chain: *mut u64,
) -> i32 {
    let setup = Setup::new();
    let result = unsafe { rsf_core_ffxgvk_create_sdk(&HOST, &setup.value, out, chain) };
    if route && result == FIXTURE_OK {
        unsafe { (*rsf_fixture_wsi_route_state()).owner = *out };
    }
    result
}

pub(super) extern "C" fn rsf_fixture_ffx_native() -> *mut c_void {
    unsafe { (*recorded()).native }
}

pub(super) extern "C" fn rsf_fixture_ffx_routed() -> *mut c_void {
    routed()
}

pub(super) extern "C" fn rsf_fixture_ffx_chain() -> u64 {
    CHAIN as u64
}

// A resource table over the same fake GPU for the shared allocator scenarios.
pub(super) extern "C" fn rsf_fixture_ffx_gpu() -> *mut c_void {
    let sdk_state = recorded();
    unsafe {
        if (*sdk_state).gpu.is_null() {
            let setup = Setup::new();
            let gpu = rsf_core_vkfg_load_device(&setup.value.graphics, &setup.value, false);
            require(!gpu.is_null(), "fake Vulkan device entries loaded");
            let mut memory = vk::PhysicalDeviceMemoryProperties::default();
            memory_properties(
                vk::PhysicalDevice::from_raw(PHYSICAL_DEVICE as u64),
                &mut memory,
            );
            rsf_core_vkfg_device_memory(&mut *gpu, &memory);
            (*sdk_state).gpu = gpu;
        }
        (*sdk_state).gpu.cast()
    }
}

pub(super) fn rsf_fixture_ffx_platform() -> &'static rsf_vkfg_platform {
    &DEVICE_PLATFORM
}

fn hooks() -> WsiDispatch {
    unsafe { (*recorded()).hooks }.expect("registered WSI hooks")
}

pub(super) extern "C" fn rsf_fixture_wsi_create(device: u64, out: *mut u64) -> i32 {
    let mut chain = vk::SwapchainKHR::null();
    let create = hooks().create.expect("create hook");
    let result = unsafe {
        create(
            vk::Device::from_raw(device),
            ptr::null(),
            ptr::null(),
            &mut chain,
        )
    };
    if result == vk::Result::SUCCESS {
        unsafe { out.write(chain.as_raw()) };
    }
    result.as_raw()
}

pub(super) extern "C" fn rsf_fixture_wsi_destroy(device: u64, chain: u64) {
    let destroy = hooks().destroy.expect("destroy hook");
    unsafe {
        destroy(
            vk::Device::from_raw(device),
            vk::SwapchainKHR::from_raw(chain),
            ptr::null(),
        )
    };
}

pub(super) extern "C" fn rsf_fixture_wsi_acquire(device: u64, chain: u64, out: *mut u32) -> i32 {
    let acquire = hooks().acquire.expect("acquire hook");
    unsafe {
        acquire(
            vk::Device::from_raw(device),
            vk::SwapchainKHR::from_raw(chain),
            0,
            vk::Semaphore::null(),
            vk::Fence::null(),
            out,
        )
    }
    .as_raw()
}

pub(super) extern "C" fn rsf_fixture_wsi_images(device: u64, chain: u64, count: *mut u32) -> i32 {
    let images = hooks().images.expect("image hook");
    unsafe {
        images(
            vk::Device::from_raw(device),
            vk::SwapchainKHR::from_raw(chain),
            count,
            ptr::null_mut(),
        )
    }
    .as_raw()
}

pub(super) extern "C" fn rsf_fixture_wsi_present(queue: u64) -> i32 {
    let present = hooks().present.expect("present hook");
    let info = vk::PresentInfoKHR::default();
    unsafe { present(vk::Queue::from_raw(queue), &info) }.as_raw()
}

pub(super) extern "C" fn rsf_fixture_wsi_device_proc(device: u64) -> i32 {
    let resolve = unsafe { (*recorded()).device_proc }.expect("backend resolver");
    let Some(entry) =
        (unsafe { resolve(handle(device as usize), c"vkBindImageMemory2KHR".as_ptr()) })
    else {
        return 0;
    };
    let images = unsafe { rsf_fixture_wsi_entry(c"vkGetSwapchainImagesKHR".as_ptr()) };
    if images.map(|images| images as usize) == Some(entry as usize) {
        1
    } else {
        -1
    }
}

pub(super) extern "C" fn rsf_fixture_wsi_graphics_submit(fence: u64) -> i32 {
    let submit = unsafe { (*recorded()).submit }.expect("game-queue submit");
    unsafe { submit(0, ptr::null(), handle(fence as usize)) }
}
