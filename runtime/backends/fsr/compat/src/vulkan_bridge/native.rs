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
    Handle, LOCAL_FENCE, LegacyFrame, QUEUE, SHARED_FENCE, Session, Texture, Version, dx12, sdk,
    standalone,
};
use crate::CompatLease as Lease;
use crate::ffx_abi::{
    FFX_API_RESOURCE_STATE_COMPUTE_READ, FFX_API_RESOURCE_STATE_UNORDERED_ACCESS,
    PfnFfxCreateContext, PfnFfxDestroyContext, PfnFfxDispatch, PfnFfxQuery, ffxApiHeader,
    ffxContext,
};
use crate::ffx_resource;
use ash::vk;
use core::ptr;
use rsf_game_sys::win32::typed_export;
use windows::{
    Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0},
        Graphics::{
            Direct3D::D3D_FEATURE_LEVEL_12_0,
            Direct3D12::*,
            Dxgi::{
                Common::*, CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_NOT_FOUND,
                IDXGIFactory4,
            },
        },
        System::Threading::{CreateEventW, WaitForSingleObject},
    },
    core::{Interface, PCWSTR},
};

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

/// Frees the lease, COM owners, the event and the module in that order. The owner drops the capsule only after the SDK
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
        let factory =
            match unsafe { CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0)) } {
                Ok(factory) => factory,
                Err(error) => return error.code().0,
            };
        self.factory = factory.into_raw();
        let factory: &IDXGIFactory4 = unsafe { dx12::borrow(&self.factory) };
        let mut index = 0_u32;
        loop {
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(adapter) => adapter,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return -5,
                Err(error) => return error.code().0,
            };
            let description = match unsafe { adapter.GetDesc1() } {
                Ok(description) => description,
                Err(error) => return error.code().0,
            };
            let id = (u64::from(description.AdapterLuid.HighPart as u32) << 32)
                | u64::from(description.AdapterLuid.LowPart);
            if id == luid {
                self.adapter = adapter.into_raw();
                *vendor = description.VendorId;
                return 0;
            }
            index = index.wrapping_add(1);
        }
    }

    pub(super) fn create_device(&mut self) -> i32 {
        let code =
            unsafe { dx12::create_device(self.adapter, D3D_FEATURE_LEVEL_12_0, &mut self.device) };
        if code != 0 {
            return code;
        }
        let device: &ID3D12Device = unsafe { dx12::borrow(&self.device) };
        let queue = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        };
        dx12::store_interface(
            unsafe { device.CreateCommandQueue::<ID3D12CommandQueue>(&queue) },
            &mut self.queue,
        )
    }

    /// Loads the signed upscaler from a full NUL-terminated path and resolves its
    /// four exports: zero, -7 when the module does not load, or -8 when an export
    /// is missing.
    pub(super) fn load(&mut self, path: *const u16) -> i32 {
        let Ok(module) = (unsafe { dx12::load_module(path) }) else {
            return -7;
        };
        self.module = module.0;
        // Each export uses its SDK signature on Windows x64.
        unsafe {
            self.create =
                typed_export::<PfnFfxCreateContext>(module.0, c"ffxCreateContext").flatten();
            self.destroy =
                typed_export::<PfnFfxDestroyContext>(module.0, c"ffxDestroyContext").flatten();
            self.dispatch = typed_export::<PfnFfxDispatch>(module.0, c"ffxDispatch").flatten();
            self.query = typed_export::<PfnFfxQuery>(module.0, c"ffxQuery").flatten();
        }
        if self.create.is_some()
            && self.destroy.is_some()
            && self.dispatch.is_some()
            && self.query.is_some()
        {
            0
        } else {
            -8
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
        let device: &ID3D12Device = unsafe { dx12::borrow(&self.device) };
        let fence = unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) };
        let code = dx12::store_interface(fence, &mut self.completion_fence);
        if code != 0 {
            return code;
        }
        let Ok(event) = (unsafe { CreateEventW(None, false, false, PCWSTR::null()) }) else {
            return -10;
        };
        self.completion_event = event.0;
        let kind = D3D12_COMMAND_LIST_TYPE_DIRECT;
        for output in &mut self.allocators {
            let allocator =
                unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(kind) };
            let code = dx12::store_interface(allocator, output);
            if code != 0 {
                return code;
            }
        }
        let allocator = unsafe { dx12::borrow(&self.allocators[0]) };
        match dx12::create_list(device, allocator, 0, kind) {
            Ok(list) => {
                let result = unsafe { list.Close() };
                self.commands = list.into_raw();
                dx12::status(result)
            }
            Err(error) => error.code().0,
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
        let (format, mut flags) = match vk_format {
            VK_RGBA16_FLOAT => (DXGI_FORMAT_R16G16B16A16_FLOAT, D3D12_RESOURCE_FLAG_NONE),
            VK_RG16_FLOAT => (DXGI_FORMAT_R16G16_FLOAT, D3D12_RESOURCE_FLAG_NONE),
            VK_R32_FLOAT => (DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_FLAG_NONE),
            VK_D32_FLOAT => (
                DXGI_FORMAT_D32_FLOAT,
                D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
            ),
            _ => return -2,
        };
        if writable != 0 {
            if flags != D3D12_RESOURCE_FLAG_NONE {
                return -3;
            }
            flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
        }
        let device = unsafe { dx12::borrow(&self.device) };
        match dx12::create_shared_texture(device, width, height, format, flags) {
            Ok((shared, image)) => {
                unsafe {
                    handle.write(shared);
                    resource.write(image);
                }
                0
            }
            Err(error) => error.code().0,
        }
    }

    /// Creates or reuses the capsule-owned shared fence and exports a new NT handle.
    pub(super) fn create_fence(&mut self, handle: *mut Handle) -> i32 {
        unsafe { dx12::create_shared_fence(self.device, &mut self.shared_fence, &mut *handle) }
    }

    pub(super) fn completed(&self) -> u64 {
        unsafe { dx12::borrow::<ID3D12Fence>(&self.completion_fence).GetCompletedValue() }
    }

    /// Zero when the event fired, an HRESULT when it could not be armed, or -2 on timeout.
    pub(super) fn wait_event(&self, value: u64, milliseconds: u32) -> i32 {
        let fence: &ID3D12Fence = unsafe { dx12::borrow(&self.completion_fence) };
        let event = HANDLE(self.completion_event);
        if let Err(error) = unsafe { fence.SetEventOnCompletion(value, event) } {
            return error.code().0;
        }
        if unsafe { WaitForSingleObject(event, milliseconds) } == WAIT_OBJECT_0 {
            0
        } else {
            -2
        }
    }

    pub(super) fn record(&mut self, frame: &Dispatch) -> i32 {
        let Some(&allocator) = self.allocators.get(frame.slot as usize) else {
            return FAILED;
        };
        let Some(dispatch) = self.dispatch else {
            return FAILED;
        };
        let list: &ID3D12GraphicsCommandList = unsafe { dx12::borrow(&self.commands) };
        if let Err(error) = unsafe { dx12::reset_commands(list, allocator) } {
            return error.code().0;
        }
        const COMMON: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COMMON;
        const READ: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        const WRITE: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
        let resources = [
            (frame.color, READ),
            (frame.depth, READ),
            (frame.motion, READ),
            (frame.output, WRITE),
        ];
        for (image, state) in &resources {
            dx12::transition(list, unsafe { dx12::borrow(image) }, COMMON, *state);
        }

        let mut desc = sdk::dispatch(frame, self.commands);
        // FidelityFX owns the DXGI format and resource-description conventions.
        const COMPUTE_READ: u32 = FFX_API_RESOURCE_STATE_COMPUTE_READ as u32;
        const UNORDERED_ACCESS: u32 = FFX_API_RESOURCE_STATE_UNORDERED_ACCESS as u32;
        unsafe {
            desc.color = ffx_resource::describe(frame.color, COMPUTE_READ);
            desc.depth = ffx_resource::describe(frame.depth, COMPUTE_READ);
            desc.motionVectors = ffx_resource::describe(frame.motion, COMPUTE_READ);
            desc.output = ffx_resource::describe(frame.output, UNORDERED_ACCESS);
        }
        let code = unsafe { dispatch(&mut self.effect, &raw const desc.header) } as i32;
        if code != 0 {
            let _ = unsafe { list.Close() };
            return code;
        }
        for (image, state) in &resources {
            dx12::transition(list, unsafe { dx12::borrow(image) }, *state, COMMON);
        }
        dx12::status(unsafe { list.Close() })
    }

    pub(super) fn wait_shared(&self, value: u64) -> i32 {
        unsafe { dx12::queue_fence(self.queue, self.shared_fence, value, false) }
    }

    pub(super) fn execute(&self) {
        unsafe { dx12::execute(self.queue, self.commands) };
    }

    pub(super) fn signal(&self, value: u64, shared: bool) -> i32 {
        let fence = if shared {
            self.shared_fence
        } else {
            self.completion_fence
        };
        unsafe { dx12::queue_fence(self.queue, fence, value, true) }
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

// The Vulkan formats a caller can import, as their VkFormat values.
const VK_RGBA16_FLOAT: u32 = vk::Format::R16G16B16A16_SFLOAT.as_raw() as u32;
const VK_RG16_FLOAT: u32 = vk::Format::R16G16_SFLOAT.as_raw() as u32;
const VK_R32_FLOAT: u32 = vk::Format::R32_SFLOAT.as_raw() as u32;
const VK_D32_FLOAT: u32 = vk::Format::D32_SFLOAT.as_raw() as u32;

/// Describes a caller's DX12 image for frame admission.
pub(super) fn describe(image: Handle) -> Texture {
    let desc = unsafe { dx12::borrow::<ID3D12Resource>(&image).GetDesc() };
    Texture {
        width: desc.Width,
        height: desc.Height,
        format: desc.Format.0 as u32,
    }
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
    unsafe { dx12::release_image(resource) };
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
