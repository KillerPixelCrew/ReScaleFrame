// SPDX-License-Identifier: MIT
//! Native capsule and exported entry points of the FSR 4 DX12 bridge.
//!
//! The capsule holds the signed-runtime module, the matching DX12 adapter and
//! device, the SDK context, the queue and command objects and the INT8
//! compatibility lease. Imported images and their resources stay with the
//! caller. Only the owner in owner.rs releases the capsule; the lease must
//! outlive the SDK context, so the owner drops it after context destruction.

use super::super::{
    BridgeCreate, BridgeFrame, BridgeInfo, Context, DEVICE, Dispatch, EFFECT, EVENT, FAILED,
    Handle, LOCAL_FENCE, LegacyFrame, Native, PLATFORM_ABI_VERSION, Platform, QUEUE, SHARED_FENCE,
    Session, Texture, Version, standalone,
};
use super::abi::{FfxApiResource, PfnFfxCreateContext, PfnFfxDestroyContext, PfnFfxDispatch};
use super::abi::{PfnFfxQuery, ffxApiHeader, ffxContext};
use super::commands;
use crate::abi::{rsf_fsr4_compat_lease as Lease, rsf_fsr4_compat_log_fn as LogFn};
use core::ptr;
use std::alloc::{Layout, alloc, dealloc};

// The orchestrator's port of the SDK's ffxApiGetResourceDX12 helper.
type Raw = Handle;
#[path = "../../orchestrator/src/d3d12_sr/fsr_abi.rs"]
mod ffx_abi;
#[path = "../../orchestrator/src/d3d12_sr/ffx_resource.rs"]
mod ffx_resource;

// Function-like macros in the pinned FidelityFX 2.4.0 headers.
const BACKEND_DX12: u64 = 0x2; // FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12
const UPSCALER_VERSION: u32 = 0x0100_1002; // FFX_UPSCALER_VERSION (4.1.2)
const ALLOCATORS: usize = 3;

// The values owner.rs and sdk_commands.rs repeat from DXGI, Win32 and Vulkan.
const _: () = {
    use super::super::{DEVICE_REMOVED, R32_FLOAT, RG16_FLOAT, RGBA16_FLOAT, UNEXPECTED};
    use ash::vk::Format;
    use windows::Win32::{
        Foundation::E_UNEXPECTED,
        Graphics::Dxgi::{Common::*, DXGI_ERROR_DEVICE_REMOVED},
    };
    assert!(size_of::<FfxApiResource>() == size_of::<ffx_abi::FfxApiResource>());
    assert!(RGBA16_FLOAT == DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32);
    assert!(
        RG16_FLOAT == DXGI_FORMAT_R16G16_FLOAT.0 as u32
            && R32_FLOAT == DXGI_FORMAT_R32_FLOAT.0 as u32
    );
    assert!(UNEXPECTED == E_UNEXPECTED.0 && DEVICE_REMOVED == DXGI_ERROR_DEVICE_REMOVED.0);
    assert!(Format::R16G16B16A16_SFLOAT.as_raw() == 97 && Format::R16G16_SFLOAT.as_raw() == 83);
    assert!(Format::R32_SFLOAT.as_raw() == 100 && Format::D32_SFLOAT.as_raw() == 126);
};

unsafe extern "C" {
    // Defined by the compatibility archive's native module.
    fn rsf_fsr4_compat_acquire_native_adapter(
        module: Handle,
        adapter: Handle,
        device: Handle,
        log: LogFn,
        user: Handle,
    ) -> *const Lease;
}

/// ffxCreateBackendDX12Desc: the header and the borrowed device.
#[repr(C)]
struct BackendDesc {
    header: ffxApiHeader,
    device: Handle,
}

const _: () = assert!(size_of::<BackendDesc>() == 24);

struct Capsule {
    compatibility: *const Lease,
    module: Handle,
    factory: Handle,
    adapter: Handle,
    device: Handle,
    queue: Handle,
    shared_fence: Handle,
    completion_fence: Handle,
    completion_event: Handle,
    allocators: [Handle; ALLOCATORS],
    commands: Handle,
    effect: ffxContext,
    create: PfnFfxCreateContext,
    destroy: PfnFfxDestroyContext,
    dispatch: PfnFfxDispatch,
    query: PfnFfxQuery,
}

impl Capsule {
    const EMPTY: Self = Self {
        compatibility: ptr::null(),
        module: ptr::null_mut(),
        factory: ptr::null_mut(),
        adapter: ptr::null_mut(),
        device: ptr::null_mut(),
        queue: ptr::null_mut(),
        shared_fence: ptr::null_mut(),
        completion_fence: ptr::null_mut(),
        completion_event: ptr::null_mut(),
        allocators: [ptr::null_mut(); ALLOCATORS],
        commands: ptr::null_mut(),
        effect: ptr::null_mut(),
        create: None,
        destroy: None,
        dispatch: None,
        query: None,
    };
}

/// # Safety
/// `native` must be a live capsule from `allocate` with no other borrower.
unsafe fn capsule<'a>(native: Native) -> &'a mut Capsule {
    unsafe { &mut *native.cast::<Capsule>() }
}

unsafe extern "C" fn allocate() -> Native {
    let capsule = unsafe { alloc(Layout::new::<Capsule>()) }.cast::<Capsule>();
    if !capsule.is_null() {
        unsafe { capsule.write(Capsule::EMPTY) };
    }
    capsule.cast()
}

unsafe extern "C" fn release_compatibility(native: Native) {
    let native = unsafe { capsule(native) };
    unsafe { crate::rsf_fsr4_compat_release(native.compatibility) };
    native.compatibility = ptr::null();
}

/// Frees COM owners, the event, the module and the capsule in the previous
/// checked-destruction order.
unsafe extern "C" fn release(native: Native) {
    if native.is_null() {
        return;
    }
    unsafe { release_compatibility(native) };
    let owner = unsafe { capsule(native) };
    let objects = [
        owner.commands,
        owner.allocators[0],
        owner.allocators[1],
        owner.allocators[2],
        owner.completion_fence,
        owner.shared_fence,
        owner.queue,
        owner.device,
        owner.adapter,
        owner.factory,
    ];
    unsafe {
        super::rsf_core_fsr4sr_release_provider_objects(
            &objects,
            owner.completion_event,
            owner.module,
        );
        dealloc(native.cast(), Layout::new::<Capsule>());
    }
}

unsafe extern "C" fn objects(native: Native) -> u32 {
    let native = unsafe { capsule(native) };
    [
        (!native.device.is_null(), DEVICE),
        (!native.queue.is_null(), QUEUE),
        (!native.effect.is_null(), EFFECT),
        (!native.completion_fence.is_null(), LOCAL_FENCE),
        (!native.completion_event.is_null(), EVENT),
        (!native.shared_fence.is_null(), SHARED_FENCE),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(0, |objects, (_, bit)| objects | bit)
}

/// Creates the DXGI factory and retains the adapter whose LUID matches the
/// Vulkan device. Returns zero with its PCI vendor, -5 when no adapter
/// matches, or an HRESULT.
unsafe extern "C" fn find_adapter(native: Native, luid: u64, vendor: *mut u32) -> i32 {
    let native = unsafe { capsule(native) };
    let mut factory = ptr::null_mut();
    let mut adapter = ptr::null_mut();
    let code = unsafe {
        commands::rsf_core_fsr4sr_find_adapter(luid, &mut factory, &mut adapter, &mut *vendor)
    };
    native.factory = factory;
    native.adapter = adapter;
    code
}

unsafe extern "C" fn create_device(native: Native) -> i32 {
    let native = unsafe { capsule(native) };
    let mut device = ptr::null_mut();
    let mut queue = ptr::null_mut();
    let code =
        unsafe { commands::rsf_core_fsr4sr_create_device(native.adapter, &mut device, &mut queue) };
    native.device = device;
    native.queue = queue;
    code
}

/// Loads the signed upscaler from a full NUL-terminated path and resolves its
/// four exports: zero, -7 when the module does not load, or -8 when an export
/// is missing.
unsafe extern "C" fn load(native: Native, path: *const u16) -> i32 {
    let native = unsafe { capsule(native) };
    let mut module = ptr::null_mut();
    let code = unsafe {
        super::rsf_core_fsr4sr_load_provider(
            path,
            &mut module,
            &mut native.create,
            &mut native.destroy,
            &mut native.dispatch,
            &mut native.query,
        )
    };
    native.module = module;
    code
}

/// Takes the pinned-SDK INT8 capability lease for NVIDIA and Intel. It must be
/// active before version enumeration, which filters providers by device support.
unsafe extern "C" fn acquire_compatibility(native: Native) -> u32 {
    unsafe { release_compatibility(native) };
    let native = unsafe { capsule(native) };
    native.compatibility = unsafe {
        rsf_fsr4_compat_acquire_native_adapter(
            native.module,
            native.adapter,
            native.device,
            None,
            ptr::null_mut(),
        )
    };
    u32::from(!native.compatibility.is_null())
}

unsafe extern "C" fn versions(native: Native, out: *mut Version, count: *mut u64) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { super::rsf_core_fsr4sr_sdk_versions(native.query, native.device, out, count) }
}

/// Creates the upscaler context with the provider forced through ffxOverrideVersion.
unsafe extern "C" fn create_context(native: Native, setup: *const Context) -> i32 {
    let native = unsafe { capsule(native) };
    let mut backend = BackendDesc {
        header: ffxApiHeader {
            type_: BACKEND_DX12,
            pNext: ptr::null_mut(),
        },
        device: native.device,
    };
    unsafe {
        super::rsf_core_fsr4sr_sdk_create(
            native.create,
            &mut native.effect,
            &mut backend.header,
            UPSCALER_VERSION,
            &*setup,
        )
    }
}

unsafe extern "C" fn provider(native: Native, out: *mut Version, parsed: *mut u32) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { super::rsf_core_fsr4sr_sdk_provider(native.query, &mut native.effect, out, parsed) }
}

/// Creates the local completion fence, its event (-10 on failure), three
/// allocators and a closed list. Partial results stay owned by the capsule.
unsafe extern "C" fn create_commands(native: Native) -> i32 {
    let native = unsafe { capsule(native) };
    let mut objects = commands::CommandObjects {
        completion: ptr::null_mut(),
        event: ptr::null_mut(),
        allocators: [ptr::null_mut(); ALLOCATORS],
        list: ptr::null_mut(),
    };
    let result = unsafe { commands::rsf_core_fsr4sr_create_commands(native.device, &mut objects) };
    native.completion_fence = objects.completion;
    native.completion_event = objects.event;
    native.allocators = objects.allocators;
    native.commands = objects.list;
    result
}

unsafe extern "C" fn plan(
    native: Native,
    quality: u32,
    display_width: u32,
    display_height: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe {
        super::rsf_core_fsr4sr_sdk_plan(
            native.query,
            &mut native.effect,
            quality,
            display_width,
            display_height,
            width,
            height,
        )
    }
}

/// Creates a committed shared DX12 texture and NT handle for dedicated Vulkan
/// import. The owner validated the arguments and cleared both outputs; the
/// caller owns the handle and the resource.
unsafe extern "C" fn create_image(
    native: Native,
    width: u32,
    height: u32,
    vk_format: u32,
    writable: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe {
        commands::rsf_core_fsr4sr_shared_image(
            native.device,
            width,
            height,
            vk_format,
            writable,
            handle,
            resource,
        )
    }
}

/// Creates or reuses the capsule-owned shared fence and exports a new NT handle.
unsafe extern "C" fn create_fence(native: Native, handle: *mut Handle) -> i32 {
    let native = unsafe { capsule(native) };
    let mut fence = native.shared_fence;
    let result =
        unsafe { commands::rsf_core_fsr4sr_shared_fence(native.device, &mut fence, &mut *handle) };
    native.shared_fence = fence;
    result
}

unsafe extern "C" fn describe(image: Handle, out: *mut Texture) {
    unsafe { commands::rsf_core_fsr4sr_describe(image, &mut *out) };
}

unsafe extern "C" fn completed(native: Native) -> u64 {
    unsafe { commands::rsf_core_fsr4sr_completed(capsule(native).completion_fence) }
}

/// Zero when the event fired, an HRESULT when it could not be armed, or -2 on timeout.
unsafe extern "C" fn wait_event(native: Native, value: u64, milliseconds: u32) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe {
        commands::rsf_core_fsr4sr_wait_event(
            native.completion_fence,
            native.completion_event,
            value,
            milliseconds,
        )
    }
}

unsafe extern "C" fn describe_resource(resource: Handle, state: u32, out: *mut FfxApiResource) {
    unsafe {
        out.cast::<ffx_abi::FfxApiResource>()
            .write(ffx_resource::describe(resource, state));
    }
}

unsafe extern "C" fn record(native: Native, frame: *const Dispatch) -> i32 {
    let native = unsafe { capsule(native) };
    let frame = unsafe { &*frame };
    let Some(&allocator) = native.allocators.get(frame.slot as usize) else {
        return FAILED;
    };
    unsafe {
        commands::rsf_core_fsr4sr_record(
            native.commands,
            allocator,
            frame,
            &mut native.effect,
            native.dispatch,
            describe_resource,
        )
    }
}

unsafe extern "C" fn wait_shared(native: Native, value: u64) -> i32 {
    let native = unsafe { capsule(native) };
    unsafe { commands::rsf_core_fsr4sr_queue_fence(native.queue, native.shared_fence, value, 0) }
}

unsafe extern "C" fn execute(native: Native) {
    let native = unsafe { capsule(native) };
    unsafe { commands::rsf_core_fsr4sr_execute(native.queue, native.commands) };
}

unsafe extern "C" fn signal(native: Native, value: u64, shared: u32) -> i32 {
    let native = unsafe { capsule(native) };
    let fence = if shared != 0 {
        native.shared_fence
    } else {
        native.completion_fence
    };
    unsafe { commands::rsf_core_fsr4sr_queue_fence(native.queue, fence, value, 1) }
}

/// Destroys the SDK consumer: -1 when the context exists without its destroy export.
unsafe extern "C" fn destroy_context(native: Native) -> i32 {
    let native = unsafe { capsule(native) };
    if native.effect.is_null() {
        return 0;
    }
    let Some(destroy) = native.destroy else {
        return FAILED;
    };
    let mut owned = native.effect;
    let result = unsafe { destroy(&mut owned, ptr::null()) } as i32;
    if result != 0 {
        return result;
    }
    native.effect = ptr::null_mut();
    0
}

static PLATFORM: Platform = Platform {
    struct_size: size_of::<Platform>() as u32,
    abi_version: PLATFORM_ABI_VERSION,
    allocate,
    release,
    objects,
    find_adapter,
    create_device,
    own_directory: super::rsf_core_fsr4sr_own_directory,
    load,
    acquire_compatibility,
    release_compatibility,
    versions,
    create_context,
    provider,
    create_commands,
    plan,
    create_image,
    create_fence,
    describe,
    completed,
    wait_event,
    record,
    wait_shared,
    execute,
    signal,
    destroy_context,
};

/// Checks the Vulkan adapter vendor required by this bridge. The check is
/// preliminary; INT8 compatibility and signed-provider selection are verified
/// during create. Zero for an AMD, NVIDIA or Intel adapter, otherwise a
/// negative availability code.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_probe(physical: Handle) -> i32 {
    unsafe { standalone::rsf_core_fsr4sr_probe_physical(physical) }
}

/// Waits for the most recent tracked submission on this DX12 queue. Zero when
/// complete or `value` is null.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_wait_idle(value: *mut Session) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_wait_idle(value) }
}

/// Drains tracked DX12 work and destroys the SDK consumer before imported
/// resources are retired. Failure is persisted and native ownership retained.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_prepare_destroy(value: *mut Session) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_prepare_destroy(value) }
}

/// Checked release: the caller first drains Vulkan users and disposes imported
/// shared images and fence imports. Failure leaves the owner attached.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_destroy_checked(value: *mut Session) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_destroy(value) }
}

/// Legacy void destruction that preserves ownership when checked preparation fails.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_destroy(value: *mut Session) {
    unsafe { super::super::rsf_core_fsr4sr_destroy(value) };
}

/// Creates the signed FSR4 DX12 context on the adapter matching the Vulkan
/// device LUID. A failed checked cleanup can return a retained owner.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_create(
    physical: Handle,
    render_width: u32,
    render_height: u32,
    display_width: u32,
    display_height: u32,
    output: *mut *mut Session,
) -> i32 {
    unsafe {
        standalone::rsf_core_fsr4sr_create_physical(
            &PLATFORM,
            physical,
            render_width,
            render_height,
            display_width,
            display_height,
            output,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgeCreate(
    setup: *const BridgeCreate,
    output: *mut *mut Session,
) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_create_public(&PLATFORM, setup, output) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgeInfo(value: *const Session, info: *mut BridgeInfo) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_info(value, info) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgePlan(
    value: *mut Session,
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_plan(value, quality, width, height) }
}

/// Creates a committed DX12 texture and NT handle for dedicated Vulkan import.
/// The caller owns and closes the handle and releases the resource after both
/// graphics APIs finish. Outputs are cleared before allocation.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_create_shared_image(
    value: *mut Session,
    width: u32,
    height: u32,
    vk_format: u32,
    writable: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    unsafe {
        super::super::rsf_core_fsr4sr_create_image(
            value, width, height, vk_format, writable, handle, resource,
        )
    }
}

/// Releases a caller-owned DX12 image reference; null is accepted. Does not
/// wait, so Vulkan and DX12 users must already be complete.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_release_image(resource: Handle) {
    unsafe { commands::rsf_core_fsr4sr_release_image(resource) };
}

/// Creates or reuses the bridge-owned shared fence and exports a new NT handle
/// that the caller closes after its permanent Vulkan timeline import.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_create_shared_fence(
    value: *mut Session,
    handle: *mut Handle,
) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_create_fence(value, handle) }
}

/// Records and submits reconstruction after a GPU wait for the ready value.
/// Valid ready and done values always receive an ordered handoff.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_evaluate(value: *mut Session, frame: *const LegacyFrame) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_evaluate_legacy(value, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgeEvaluate(value: *mut Session, frame: *const BridgeFrame) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_evaluate(value, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgeSharpness(value: *mut Session, amount: f32) -> i32 {
    unsafe { super::super::rsf_core_fsr4sr_sharpness(value, amount) }
}
