// SPDX-License-Identifier: MIT
//! Native capsule and exported entry points of the FSR 4 frame-generation bridge.
//!
//! The capsule keeps the provider module, both SDK contexts, the create
//! descriptors the SDK retains until those contexts are destroyed, the DXGI
//! swapchain description and the COM references filled by the command
//! callbacks. The owner in fsr4_fg_bridge.rs reaches it only through the
//! private platform table and releases it last.

use super::commands::{self, Graphics};
use super::sdk;
use super::{
    Bridge, BridgeCreate, BridgeFrame, BridgeInfo, GeneratedFn, Handle, INVALID, Platform,
    QueueDesc, SwapchainDesc, Version, rsf_core_fsr4fg_create, rsf_core_fsr4fg_probe,
};
use crate::d3d12_generation::fsr_abi::*;
use crate::d3d12_sr::ffx_abi::{self, *};
use crate::d3d12_sr::ffx_resource;
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::ptr;
use windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_DESC1;

// Function-like macros and inline helpers of the pinned FidelityFX 2.4.0 headers.
const BACKEND_DX12: u64 = 0x2; // FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12
const SWAPCHAIN_VERSION_DX12: u64 = 0x3000b;
const SWAPCHAIN_FOR_HWND_DX12: u64 = 0x30006;
const FRAMEGENERATION_VERSION: u32 = 0x0100_0002; // FFX_FRAMEGENERATION_VERSION (4.0.2)
const SWAPCHAIN_DX12_VERSION: u32 = 0x0100_0000; // FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION
// ffxApiGetSurfaceFormatDX12(DXGI_FORMAT_B8G8R8A8_UNORM).
const SURFACE_BGRA8_UNORM: u32 = ffx_abi::FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM as u32;

const DEVICE: u32 = 1 << 0;
const GENERATION: u32 = 1 << 1;
const CHAIN: u32 = 1 << 2;
const SWAPCHAIN: u32 = 1 << 3;
const LOCAL_FENCE: u32 = 1 << 4;
const SHARED_FENCE: u32 = 1 << 5;

/// ffxCreateBackendDX12Desc.
#[repr(C)]
struct BackendDesc {
    header: ffxApiHeader,
    device: Handle,
}

/// ffxCreateContextDescFrameGenerationSwapChainVersionDX12.
#[repr(C)]
struct ChainVersion {
    header: ffxApiHeader,
    version: u32,
}

/// ffxCreateContextDescFrameGenerationSwapChainForHwndDX12.
#[repr(C)]
struct ChainForHwnd {
    header: ffxApiHeader,
    swapchain: *mut Handle,
    hwnd: Handle,
    desc: *mut DXGI_SWAP_CHAIN_DESC1,
    fullscreen_desc: Handle,
    dxgi_factory: Handle,
    game_queue: Handle,
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<BackendDesc>() == 24 && offset_of!(BackendDesc, device) == 16);
    assert!(size_of::<ChainVersion>() == 24 && offset_of!(ChainVersion, version) == 16);
    assert!(size_of::<ChainForHwnd>() == 64 && offset_of!(ChainForHwnd, hwnd) == 24);
    assert!(offset_of!(ChainForHwnd, desc) == 32 && offset_of!(ChainForHwnd, game_queue) == 56);
    assert!(SURFACE_BGRA8_UNORM == 14);
};

// Every field is valid when zeroed; the SDK keeps pointers into this
// allocation, so it never moves after `allocate`.
#[repr(C)]
struct Native {
    graphics: Graphics,
    module: Handle,
    generation: ffxContext,
    swapchain: ffxContext,
    create: PfnFfxCreateContext,
    destroy: PfnFfxDestroyContext,
    configure: PfnFfxConfigure,
    dispatch: PfnFfxDispatch,
    query: PfnFfxQuery,
    override_version: ffxOverrideVersion,
    backend: BackendDesc,
    api_version: ffxCreateContextDescFrameGenerationVersion,
    description: ffxCreateContextDescFrameGeneration,
    chain_version: ChainVersion,
    chain_description: ChainForHwnd,
    dxgi_description: DXGI_SWAP_CHAIN_DESC1,
    telemetry: Handle,
    generated: Option<GeneratedFn>,
}

/// # Safety
/// `native` must be a live capsule from `allocate` with no other borrower.
unsafe fn capsule<'a>(native: Handle) -> &'a mut Native {
    unsafe { &mut *native.cast::<Native>() }
}

unsafe extern "C" fn allocate(telemetry: Handle, generated: GeneratedFn) -> Handle {
    let native = unsafe { alloc_zeroed(Layout::new::<Native>()) }.cast::<Native>();
    if let Some(native) = unsafe { native.as_mut() } {
        native.telemetry = telemetry;
        native.generated = Some(generated);
    }
    native.cast()
}

unsafe extern "C" fn release(native: Handle) {
    let owner = unsafe { capsule(native) };
    unsafe {
        sdk::release_provider_objects(&owner.graphics, owner.module);
        dealloc(native.cast(), Layout::new::<Native>());
    }
}

macro_rules! graphics {
    ($($name:ident => $target:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {$(
        unsafe extern "C" fn $name(native: Handle $(, $arg: $ty)*) $(-> $ret)? {
            unsafe { commands::$target(&mut capsule(native).graphics $(, $arg)*) }
        }
    )*};
}

graphics! {
    create_factory => create_factory(flags: u32) -> i32;
    find_adapter => find_adapter(luid: u64) -> i32;
    create_device => create_device(level: u32) -> i32;
    create_allocator => create_allocator(kind: u32) -> i32;
    create_command_list => create_list(node_mask: u32, kind: u32) -> i32;
    create_completion_fence => create_completion_fence(value: u64, flags: u32) -> i32;
    create_event => create_event(manual: u32, initial: u32) -> i32;
    create_image => shared_image(
        width: u32, height: u32, format: u32, handle: *mut Handle, resource: *mut Handle
    ) -> i32;
    reset_commands => reset_commands() -> i32;
    close_commands => close_commands() -> i32;
    shader_read => shader_read(image: Handle, enter: u32);
    copy_to_backbuffer => copy_to_backbuffer(color: Handle) -> i32;
    wait_shared => wait_shared(value: u64) -> i32;
    execute => execute();
    present => present_dxgi(vsync: u32) -> i32;
    signal => signal(value: u64, shared: u32) -> i32;
    wait_local => wait_local(value: u64) -> i32;
    get_present_count => present_count(count: *mut u32) -> i32;
    release_chain => release_chain();
}

unsafe extern "C" fn get_adapter_vendor(native: Handle, vendor: *mut u32) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { commands::get_adapter_vendor(&native.graphics, &mut *vendor) }
}

unsafe extern "C" fn create_queue(native: Handle, desc: *const QueueDesc) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { commands::create_queue(&mut native.graphics, &*desc) }
}

unsafe extern "C" fn create_fence(native: Handle, handle: *mut Handle) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { commands::shared_fence(&mut native.graphics, &mut *handle) }
}

unsafe extern "C" fn load(native: Handle, path: *const u16) -> i32 {
    let native = unsafe { capsule(native) };
    let mut module = ptr::null_mut();
    let code = unsafe {
        sdk::load_provider(
            path,
            &mut module,
            &mut native.create,
            &mut native.destroy,
            &mut native.configure,
            &mut native.dispatch,
            &mut native.query,
        )
    };
    native.module = module;
    code
}

unsafe extern "C" fn versions(native: Handle, out: *mut Version, count: *mut u64) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { sdk::sdk_versions(native.query, native.graphics.device, out, count) }
}

unsafe extern "C" fn create_generation(
    native: Handle,
    setup: *const BridgeCreate,
    provider: u64,
) -> i32 {
    let native = unsafe { capsule(native) };
    let Some(create) = native.create else {
        return INVALID;
    };
    native.backend.header.type_ = BACKEND_DX12;
    native.backend.device = native.graphics.device;
    unsafe {
        sdk::sdk_generation(
            &*setup,
            provider,
            FRAMEGENERATION_VERSION,
            SURFACE_BGRA8_UNORM,
            &mut native.backend.header,
            &mut native.override_version,
            &mut native.api_version,
            &mut native.description,
        );
        create(
            &mut native.generation,
            &mut native.description.header,
            ptr::null(),
        ) as i32
    }
}

unsafe extern "C" fn get_provider(native: Handle, actual: *mut u64) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { sdk::sdk_provider(native.query, &mut native.generation, actual) }
}

unsafe extern "C" fn objects(native: Handle) -> u32 {
    let native = unsafe { capsule(native) };
    let graphics = &native.graphics;
    let local =
        !graphics.queue.is_null() && !graphics.completion.is_null() && !graphics.event.is_null();
    [
        (!graphics.device.is_null(), DEVICE),
        (!native.generation.is_null(), GENERATION),
        (!graphics.chain.is_null(), CHAIN),
        (!native.swapchain.is_null(), SWAPCHAIN),
        (local, LOCAL_FENCE),
        (!graphics.shared.is_null(), SHARED_FENCE),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(0, |objects, (_, bit)| objects | bit)
}

unsafe extern "C" fn create_swapchain(
    native: Handle,
    window: Handle,
    setup: *const SwapchainDesc,
    first_attempt: u32,
) -> i32 {
    let native = unsafe { capsule(native) };
    let Some(create) = native.create else {
        return INVALID;
    };
    unsafe {
        commands::dxgi_description(
            &mut native.dxgi_description,
            &*setup,
            first_attempt,
        )
    };
    native.chain_version.header.type_ = SWAPCHAIN_VERSION_DX12;
    native.chain_version.version = SWAPCHAIN_DX12_VERSION;
    let chain = &mut native.chain_description;
    chain.header.type_ = SWAPCHAIN_FOR_HWND_DX12;
    chain.header.pNext = &raw mut native.chain_version.header;
    chain.swapchain = &raw mut native.graphics.chain;
    chain.hwnd = window;
    chain.desc = &raw mut native.dxgi_description;
    chain.dxgi_factory = native.graphics.factory;
    chain.game_queue = native.graphics.queue;
    unsafe { create(&mut native.swapchain, &mut chain.header, ptr::null()) as i32 }
}

/// The SDK's generation callback: forwards to the provider and reports the
/// generated count, or zero on failure, to the owner's telemetry.
unsafe extern "C" fn generate(
    desc: *mut ffxDispatchDescFrameGeneration,
    user: Handle,
) -> ffxReturnCode_t {
    let native = unsafe { capsule(user) };
    let Some(dispatch) = native.dispatch else {
        return INVALID as ffxReturnCode_t;
    };
    let code = unsafe { dispatch(&mut native.generation, &raw const (*desc).header) };
    let count = if code == 0 {
        unsafe { (*desc).numGeneratedFrames }
    } else {
        0
    };
    if let Some(generated) = native.generated {
        unsafe { generated(native.telemetry, code as i32, count) };
    }
    code
}

unsafe extern "C" fn describe_resource(resource: Handle, state: u32, out: *mut FfxApiResource) {
    unsafe {
        out.write(ffx_resource::describe(resource, state));
    }
}

unsafe extern "C" fn configure(
    native: Handle,
    sequence: u64,
    width: u32,
    height: u32,
    enabled: u32,
    hudless: Handle,
) -> i32 {
    let user = native;
    let native = unsafe { capsule(native) };
    unsafe {
        sdk::sdk_configure(
            native.graphics.chain,
            sequence,
            width,
            height,
            enabled,
            Some(generate),
            user,
            &mut native.generation,
            native.configure,
            hudless,
            describe_resource,
        )
    }
}

unsafe extern "C" fn prepare(
    native: Handle,
    frame: *const BridgeFrame,
    sequence: u64,
    reset: u32,
) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe {
        sdk::sdk_prepare(
            &*frame,
            sequence,
            reset,
            native.graphics.commands,
            &mut native.generation,
            native.dispatch,
            describe_resource,
        )
    }
}

unsafe extern "C" fn wait_presents(native: Handle) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { sdk::sdk_wait_presents(native.dispatch, &mut native.swapchain) }
}

fn destroy_context(destroy: PfnFfxDestroyContext, context: &mut ffxContext) -> i32 {
    let Some(destroy) = destroy else {
        return INVALID;
    };
    let code = unsafe { destroy(context, ptr::null()) };
    if code == 0 {
        *context = ptr::null_mut();
    }
    code as i32
}

unsafe extern "C" fn destroy_generation(native: Handle) -> i32 {
    let native = unsafe { capsule(native) };
    destroy_context(native.destroy, &mut native.generation)
}

unsafe extern "C" fn destroy_swapchain(native: Handle) -> i32 {
    let native = unsafe { capsule(native) };
    destroy_context(native.destroy, &mut native.swapchain)
}

static PLATFORM: Platform = Platform {
    allocate,
    release,
    create_factory,
    find_adapter,
    get_adapter_vendor,
    create_device,
    load,
    versions,
    create_generation,
    get_provider,
    create_queue,
    create_allocator,
    create_command_list,
    create_completion_fence,
    create_event,
    objects,
    create_swapchain,
    create_image,
    create_fence,
    describe_texture: commands::describe_texture,
    reset_commands,
    close_commands,
    configure,
    shader_read,
    prepare,
    copy_to_backbuffer,
    wait_shared,
    execute,
    present,
    wait_presents,
    signal,
    wait_local,
    get_present_count,
    destroy_generation,
    release_chain,
    destroy_swapchain,
};

/// Creates a matching DX12 device and queries its exact major-4 providers
/// without a window or WSI calls.
#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgProbe(
    luid: u64,
    directory: *const u16,
    info: *mut BridgeInfo,
) -> i32 {
    unsafe { rsf_core_fsr4fg_probe(&PLATFORM, luid, directory, info) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4FgCreate(setup: *const BridgeCreate, output: *mut *mut Bridge) -> i32 {
    unsafe { rsf_core_fsr4fg_create(&PLATFORM, setup, output) }
}
