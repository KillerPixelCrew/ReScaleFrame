// SPDX-License-Identifier: MIT
//! Native capsule and exported entry points of the FSR 4 DX12 bridge.
//!
//! The capsule holds the signed-runtime module, the matching DX12 adapter and
//! device, the SDK context, the queue and command objects and the INT8
//! compatibility lease. Imported images and their resources stay with the
//! caller. Only the owner in owner.rs releases the capsule; the lease must
//! outlive the SDK context, so the owner drops it after context destruction.

use super::{
    BridgeCreate, BridgeFrame, BridgeInfo, Context, DEVICE, Dispatch, EFFECT, EVENT, FAILED,
    Handle, LOCAL_FENCE, LegacyFrame, QUEUE, SHARED_FENCE, Session, Texture, Version, commands, dx12,
    sdk, standalone,
};
use crate::CompatLease as Lease;
use crate::ffx_abi::{
    PfnFfxCreateContext, PfnFfxDestroyContext, PfnFfxDispatch, PfnFfxQuery, ffxApiHeader,
    ffxContext,
};
use core::ptr;

// Function-like macros in the pinned FidelityFX 2.4.0 headers.
const BACKEND_DX12: u64 = 0x2; // FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12
const UPSCALER_VERSION: u32 = 0x0100_1002; // FFX_UPSCALER_VERSION (4.1.2)
const ALLOCATORS: usize = 3;

/// ffxCreateBackendDX12Desc: the header and the borrowed device.
#[repr(C)]
struct BackendDesc {
    header: ffxApiHeader,
    device: Handle,
}

const _: () = assert!(size_of::<BackendDesc>() == 24);

pub(super) struct Capsule {
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

/// Frees the lease, COM owners, the event and the module in the previous
/// checked-destruction order. The owner drops the capsule only after the SDK
/// context is destroyed.
impl Drop for Capsule {
    fn drop(&mut self) {
        self.release_compatibility();
        let objects = [
            self.commands,
            self.allocators[0],
            self.allocators[1],
            self.allocators[2],
            self.completion_fence,
            self.shared_fence,
            self.queue,
            self.device,
            self.adapter,
            self.factory,
        ];
        unsafe { dx12::release_objects(&objects, self.completion_event, self.module) };
    }
}

impl Capsule {
    pub(super) const EMPTY: Self = Self {
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

    pub(super) fn release_compatibility(&mut self) {
        unsafe { crate::rsf_fsr4_compat_release(self.compatibility) };
        self.compatibility = ptr::null();
    }

    /// The objects the capsule currently holds, as owner.rs object bits.
    pub(super) fn objects(&self) -> u32 {
        [
            (!self.device.is_null(), DEVICE),
            (!self.queue.is_null(), QUEUE),
            (!self.effect.is_null(), EFFECT),
            (!self.completion_fence.is_null(), LOCAL_FENCE),
            (!self.completion_event.is_null(), EVENT),
            (!self.shared_fence.is_null(), SHARED_FENCE),
        ]
        .into_iter()
        .filter(|(held, _)| *held)
        .fold(0, |objects, (_, bit)| objects | bit)
    }

    /// Creates the DXGI factory and retains the adapter whose LUID matches the
    /// Vulkan device. Returns zero with its PCI vendor, -5 when no adapter
    /// matches, or an HRESULT.
    pub(super) fn find_adapter(&mut self, luid: u64, vendor: &mut u32) -> i32 {
        commands::find_adapter(luid, &mut self.factory, &mut self.adapter, vendor)
    }

    pub(super) fn create_device(&mut self) -> i32 {
        unsafe { commands::create_device(self.adapter, &mut self.device, &mut self.queue) }
    }

    /// Loads the signed upscaler from a full NUL-terminated path and resolves its
    /// four exports: zero, -7 when the module does not load, or -8 when an export
    /// is missing.
    pub(super) fn load(&mut self, path: *const u16) -> i32 {
        unsafe {
            sdk::load_provider(
                path,
                &mut self.module,
                &mut self.create,
                &mut self.destroy,
                &mut self.dispatch,
                &mut self.query,
            )
        }
    }

    /// Takes the pinned-SDK INT8 capability lease for NVIDIA and Intel. It must be
    /// active before version enumeration, which filters providers by device support.
    pub(super) fn acquire_compatibility(&mut self) -> bool {
        self.release_compatibility();
        self.compatibility = unsafe {
            crate::acquire_logged(
                self.module,
                self.adapter,
                self.device,
                None,
                ptr::null_mut(),
            )
        };
        !self.compatibility.is_null()
    }

    pub(super) fn versions(&mut self, out: &mut [Version], count: &mut u64) -> i32 {
        unsafe { sdk::versions(self.query, self.device, out.as_mut_ptr(), count) }
    }

    /// Creates the upscaler context with the provider forced through ffxOverrideVersion.
    pub(super) fn create_context(&mut self, setup: &Context) -> i32 {
        let mut backend = BackendDesc {
            header: ffxApiHeader {
                type_: BACKEND_DX12,
                pNext: ptr::null_mut(),
            },
            device: self.device,
        };
        unsafe {
            sdk::create(
                self.create,
                &mut self.effect,
                &mut backend.header,
                UPSCALER_VERSION,
                setup,
            )
        }
    }

    pub(super) fn provider(&mut self, out: &mut Version, parsed: &mut bool) -> i32 {
        unsafe { sdk::provider(self.query, &mut self.effect, out, parsed) }
    }

    /// Creates the local completion fence, its event (-10 on failure), three
    /// allocators and a closed list. Partial results stay owned by the capsule.
    pub(super) fn create_commands(&mut self) -> i32 {
        unsafe {
            commands::create_commands(
                self.device,
                &mut self.completion_fence,
                &mut self.completion_event,
                &mut self.allocators,
                &mut self.commands,
            )
        }
    }

    pub(super) fn plan(
        &mut self,
        quality: u32,
        display_width: u32,
        display_height: u32,
        width: *mut u32,
        height: *mut u32,
    ) -> i32 {
        unsafe {
            sdk::plan(
                self.query,
                &mut self.effect,
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
    pub(super) fn create_image(
        &self,
        width: u32,
        height: u32,
        vk_format: u32,
        writable: u32,
        handle: *mut Handle,
        resource: *mut Handle,
    ) -> i32 {
        unsafe {
            commands::shared_image(self.device, width, height, vk_format, writable, handle, resource)
        }
    }

    /// Creates or reuses the capsule-owned shared fence and exports a new NT handle.
    pub(super) fn create_fence(&mut self, handle: *mut Handle) -> i32 {
        unsafe { commands::shared_fence(self.device, &mut self.shared_fence, &mut *handle) }
    }

    pub(super) fn completed(&self) -> u64 {
        unsafe { commands::completed(self.completion_fence) }
    }

    /// Zero when the event fired, an HRESULT when it could not be armed, or -2 on timeout.
    pub(super) fn wait_event(&self, value: u64, milliseconds: u32) -> i32 {
        unsafe {
            commands::wait_event(
                self.completion_fence,
                self.completion_event,
                value,
                milliseconds,
            )
        }
    }

    pub(super) fn record(&mut self, frame: &Dispatch) -> i32 {
        let Some(&allocator) = self.allocators.get(frame.slot as usize) else {
            return FAILED;
        };
        unsafe {
            commands::record(
                self.commands,
                allocator,
                frame,
                &mut self.effect,
                self.dispatch,
            )
        }
    }

    pub(super) fn wait_shared(&self, value: u64) -> i32 {
        unsafe { commands::queue_fence(self.queue, self.shared_fence, value, false) }
    }

    pub(super) fn execute(&self) {
        unsafe { commands::execute(self.queue, self.commands) };
    }

    pub(super) fn signal(&self, value: u64, shared: bool) -> i32 {
        let fence = if shared {
            self.shared_fence
        } else {
            self.completion_fence
        };
        unsafe { commands::queue_fence(self.queue, fence, value, true) }
    }

    /// Destroys the SDK consumer: -1 when the context exists without its destroy export.
    pub(super) fn destroy_context(&mut self) -> i32 {
        if self.effect.is_null() {
            return 0;
        }
        let Some(destroy) = self.destroy else {
            return FAILED;
        };
        let mut owned = self.effect;
        let result = unsafe { destroy(&mut owned, ptr::null()) } as i32;
        if result != 0 {
            return result;
        }
        self.effect = ptr::null_mut();
        0
    }
}

/// Describes a caller's DX12 image for frame admission.
pub(super) fn describe(image: Handle) -> Texture {
    unsafe { commands::describe(image) }
}

/// Checks the Vulkan adapter vendor required by this bridge. The check is
/// preliminary; INT8 compatibility and signed-provider selection are verified
/// during create. Zero for an AMD, NVIDIA or Intel adapter, otherwise a
/// negative availability code.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_probe(physical: Handle) -> i32 {
    unsafe { standalone::probe_physical(physical) }
}

/// Waits for the most recent tracked submission on this DX12 queue. Zero when
/// complete or `value` is null.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_wait_idle(value: *mut Session) -> i32 {
    unsafe { super::wait_idle(value) }
}

/// Drains tracked DX12 work and destroys the SDK consumer before imported
/// resources are retired. Failure is persisted and native ownership retained.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_prepare_destroy(value: *mut Session) -> i32 {
    unsafe { super::prepare_destroy(value) }
}

/// Checked release: the caller first drains Vulkan users and disposes imported
/// shared images and fence imports. Failure leaves the owner attached.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_destroy_checked(value: *mut Session) -> i32 {
    unsafe { super::destroy(value) }
}

/// Legacy void destruction that preserves ownership when checked preparation fails.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_destroy(value: *mut Session) {
    unsafe { super::destroy(value) };
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
        standalone::create_physical(
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
    unsafe { super::create_public(setup, output) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgeInfo(value: *const Session, info: *mut BridgeInfo) -> i32 {
    unsafe { super::info(value, info) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgePlan(
    value: *mut Session,
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { super::plan(value, quality, width, height) }
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
    unsafe { super::create_image(value, width, height, vk_format, writable, handle, resource) }
}

/// Releases a caller-owned DX12 image reference; null is accepted. Does not
/// wait, so Vulkan and DX12 users must already be complete.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_release_image(resource: Handle) {
    unsafe { commands::release_image(resource) };
}

/// Creates or reuses the bridge-owned shared fence and exports a new NT handle
/// that the caller closes after its permanent Vulkan timeline import.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_create_shared_fence(
    value: *mut Session,
    handle: *mut Handle,
) -> i32 {
    unsafe { super::create_fence(value, handle) }
}

/// Records and submits reconstruction after a GPU wait for the ready value.
/// Valid ready and done values always receive an ordered handoff.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr4_evaluate(value: *mut Session, frame: *const LegacyFrame) -> i32 {
    unsafe { super::evaluate_legacy(value, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgeEvaluate(value: *mut Session, frame: *const BridgeFrame) -> i32 {
    unsafe { super::evaluate(value, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RsfFsr4BridgeSharpness(value: *mut Session, amount: f32) -> i32 {
    unsafe { super::sharpness(value, amount) }
}
