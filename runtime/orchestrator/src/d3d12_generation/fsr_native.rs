// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d12_generation::fsr_abi::*;
use crate::d3d12_sr::{ffx_abi::*, ffx_resource};
use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use windows::{
    Win32::Graphics::{
        Direct3D12::*,
        Dxgi::{Common::DXGI_FORMAT, *},
    },
    core::Interface,
};

#[derive(Default)]
struct CreateDescription {
    generation: ffxCreateContextDescFrameGeneration,
    version: ffxCreateContextDescFrameGenerationVersion,
    backend: ffxCreateBackendDX12Desc,
    provider: ffxOverrideVersion,
}

#[derive(Default)]
struct Sdk {
    create: PfnFfxCreateContext,
    destroy: PfnFfxDestroyContext,
    query: PfnFfxQuery,
    dispatch: PfnFfxDispatch,
    configure: PfnFfxConfigure,
    // Context creation retains the linked descriptions and their backing storage.
    description: UnsafeCell<CreateDescription>,
    chain: Cell<*mut SwapchainBindings>,
}

// Pointers in the create chain stay stable until the SDK context is destroyed.
struct SwapchainBindings {
    desc: DXGI_SWAP_CHAIN_DESC1,
    _factory: IDXGIFactory2,
    chain: Raw,
    version: ffxCreateContextDescFrameGenerationSwapChainVersionDX12,
    create: ffxCreateContextDescFrameGenerationSwapChainForHwndDX12,
}

pub(super) static SDK: SdkApi = SdkApi {
    generation: [
        FFX_FRAMEGENERATION_VERSION_MAJOR,
        FFX_FRAMEGENERATION_VERSION_MINOR,
        FFX_FRAMEGENERATION_VERSION_PATCH,
    ],
    swapchain: [
        FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MAJOR,
        FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MINOR,
        FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_PATCH,
    ],
};

fn header(kind: u32) -> ffxApiHeader {
    ffxApiHeader {
        type_: kind.into(),
        pNext: null_mut(),
    }
}

pub(super) unsafe extern "C" fn resolve(setup: &Setup, _: Raw, _: Raw, out: &mut Raw) -> i32 {
    let sdk = Sdk::default();
    unsafe { (*sdk.description.get()).backend.device = setup.chain.d3d12_device };
    *out = crate::allocate_handle(sdk).cast();
    if out.is_null() { INIT_FAILED } else { OK }
}

pub(super) unsafe extern "C" fn close(bindings: Raw) {
    let sdk = unsafe { Box::from_raw(bindings.cast::<Sdk>()) };
    let chain = sdk.chain.get();
    if !chain.is_null() {
        drop(unsafe { Box::from_raw(chain) });
    }
}

pub(super) unsafe extern "C" fn bind(bindings: Raw, entries: *const Entry) {
    let sdk = unsafe { &mut *bindings.cast::<Sdk>() };
    let [create, destroy, query, dispatch, configure] = unsafe { *entries.cast::<[Entry; 5]>() };
    // The owner has verified all five exports in the retained SDK module.
    unsafe {
        sdk.create = std::mem::transmute::<Entry, PfnFfxCreateContext>(create);
        sdk.destroy = std::mem::transmute::<Entry, PfnFfxDestroyContext>(destroy);
        sdk.query = std::mem::transmute::<Entry, PfnFfxQuery>(query);
        sdk.dispatch = std::mem::transmute::<Entry, PfnFfxDispatch>(dispatch);
        sdk.configure = std::mem::transmute::<Entry, PfnFfxConfigure>(configure);
    }
}

fn query_versions(
    query: PfnFfxQuery,
    setup: &Setup,
    versions: &mut [Version; MAX_VERSIONS],
    count: &mut u64,
) -> i32 {
    let Some(versions) = (unsafe {
        crate::d3d12_sr::query_d3d12(
            query,
            setup.chain.d3d12_device,
            RSF_FFX_CREATE_GENERATION as u64,
            versions,
        )
    }) else {
        return NOT_SUPPORTED;
    };
    *count = versions.len() as u64;
    OK
}

pub(super) unsafe extern "C" fn versions(
    bindings: Raw,
    setup: &Setup,
    out: &mut [Version; MAX_VERSIONS],
    count: &mut u64,
) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    query_versions(sdk.query, setup, out, count)
}

unsafe extern "C" fn probe_versions(
    query: Raw,
    setup: &Setup,
    out: &mut [Version; MAX_VERSIONS],
    count: &mut u64,
) -> i32 {
    let query = unsafe { std::mem::transmute::<Raw, PfnFfxQuery>(query) };
    query_versions(query, setup, out, count)
}

pub(super) unsafe extern "C" fn create_generation(
    bindings: Raw,
    plan: *const GenerationPlan,
    out: *mut Raw,
) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let plan = unsafe { &*plan };
    // The owner serializes replacement with retiring the old generation context.
    let data = unsafe { &mut *sdk.description.get() };
    data.provider.header = header(FFX_API_DESC_TYPE_OVERRIDE_VERSION);
    data.provider.versionId = plan.version_id;
    data.backend.header = header(RSF_FFX_CREATE_BACKEND_DX12 as u32);
    data.backend.header.pNext = (&raw mut data.provider).cast();
    data.version.header = header(RSF_FFX_CREATE_GENERATION_VERSION as u32);
    data.version.version = RSF_FFX_GENERATION_VERSION as u32;
    data.version.header.pNext = (&raw mut data.backend).cast();
    data.generation = ffxCreateContextDescFrameGeneration {
        header: ffxApiHeader {
            pNext: (&raw mut data.version).cast(),
            ..header(RSF_FFX_CREATE_GENERATION as u32)
        },
        flags: plan.flags,
        displaySize: FfxApiDimensions2D {
            width: plan.width,
            height: plan.height,
        },
        maxRenderSize: FfxApiDimensions2D {
            width: plan.max_render_width,
            height: plan.max_render_height,
        },
        backBufferFormat: plan.format,
    };
    unsafe { sdk.create.unwrap()(out, (&raw mut data.generation).cast(), std::ptr::null()) }
}

pub(super) unsafe extern "C" fn provider_version(
    bindings: Raw,
    mut context: Raw,
    out: *mut u64,
) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let mut actual = ffxQueryGetProviderVersion {
        header: header(FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION),
        ..Default::default()
    };
    let code = unsafe { sdk.query.unwrap()(&mut context, (&raw mut actual).cast()) };
    unsafe { out.write(actual.versionId) };
    code
}

pub(super) unsafe extern "C" fn create_chain(
    bindings: Raw,
    setup: *const Setup,
    context: *mut Raw,
    chain: *mut Raw,
    result: *mut u32,
) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let setup = unsafe { &*setup };
    let factory = match unsafe { CreateDXGIFactory2::<IDXGIFactory2>(DXGI_CREATE_FACTORY_FLAGS(0)) }
    {
        Ok(factory) => factory,
        Err(error) => {
            Log::new(setup).write(format_args!(
                "FSR FG create DXGI factory failed HRESULT=0x{:08x}",
                error.code().0 as u32
            ));
            return INIT_FAILED;
        }
    };
    let factory_ptr = factory.as_raw();
    let storage = crate::allocate_handle(SwapchainBindings {
        desc: chain_description(&setup.chain),
        _factory: factory,
        chain: null_mut(),
        version: ffxCreateContextDescFrameGenerationSwapChainVersionDX12 {
            header: header(RSF_FFX_CREATE_SWAPCHAIN_VERSION as u32),
            version: RSF_FFX_SWAPCHAIN_VERSION as u32,
        },
        create: Default::default(),
    });
    if storage.is_null() {
        return INIT_FAILED;
    }
    // Store the pointer so cleanup can recover the allocation even after a failed create.
    sdk.chain.set(storage);
    unsafe {
        (*storage).create = ffxCreateContextDescFrameGenerationSwapChainForHwndDX12 {
            header: ffxApiHeader {
                pNext: (&raw mut (*storage).version).cast(),
                ..header(RSF_FFX_CREATE_SWAPCHAIN_HWND as u32)
            },
            swapchain: &raw mut (*storage).chain,
            hwnd: setup.chain.hwnd,
            desc: &raw mut (*storage).desc,
            fullscreenDesc: null_mut(),
            dxgiFactory: factory_ptr,
            gameQueue: setup.chain.d3d12_queue,
        };
        result.write(sdk.create.unwrap()(
            context,
            (&raw mut (*storage).create).cast(),
            std::ptr::null(),
        ));
        chain.write((*storage).chain);
    }
    OK
}

pub(super) unsafe extern "C" fn surface_format(format: u32) -> u32 {
    ffx_resource::surface_format(DXGI_FORMAT(format as i32))
}

pub(super) unsafe extern "C" fn chain_extent(chain: Raw, out: *mut Extent) -> i32 {
    let chain = unsafe { IDXGISwapChain4::from_raw_borrowed(&chain) }.unwrap();
    match unsafe { chain.GetDesc1() } {
        Ok(desc) => {
            unsafe {
                out.write(Extent {
                    width: desc.Width,
                    height: desc.Height,
                    format: ffx_resource::surface_format(desc.Format),
                })
            };
            OK
        }
        Err(error) => error.code().0,
    }
}

pub(super) unsafe extern "C" fn whole_texture(resource: *const Resource) -> u32 {
    let resource = unsafe { &*resource };
    let texture = unsafe { ID3D12Resource::from_raw_borrowed(&resource.resource) }.unwrap();
    let desc = unsafe { texture.GetDesc() };
    u32::from(
        resource.x == 0
            && resource.y == 0
            && u64::from(resource.width) == desc.Width
            && resource.height == desc.Height,
    )
}

pub(super) unsafe extern "C" fn present_count(chain: Raw, count: *mut u32) -> i32 {
    let chain = unsafe { IDXGISwapChain4::from_raw_borrowed(&chain) }.unwrap();
    match unsafe { chain.GetLastPresentCount() } {
        Ok(value) => {
            unsafe { count.write(value) };
            OK
        }
        Err(error) => error.code().0,
    }
}

pub(super) unsafe extern "C" fn copy(
    command: Raw,
    output: Raw,
    output_state: u32,
    source: Raw,
    source_state: u32,
) {
    let command = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&command) }.unwrap();
    let output = unsafe { ID3D12Resource::from_raw_borrowed(&output) }.unwrap();
    let source = unsafe { ID3D12Resource::from_raw_borrowed(&source) }.unwrap();
    let transition = |texture: &ID3D12Resource, before, after| {
        if before != after {
            // The callback lends the resource through command recording.
            let borrow = unsafe { ID3D12Resource::from_raw(texture.as_raw()) };
            let barrier = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: ManuallyDrop::new(Some(borrow)),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: before,
                        StateAfter: after,
                    }),
                },
            };
            unsafe { command.ResourceBarrier(&[barrier]) };
        }
    };
    let output_state = D3D12_RESOURCE_STATES(output_state as i32);
    let source_state = D3D12_RESOURCE_STATES(source_state as i32);
    transition(source, source_state, D3D12_RESOURCE_STATE_COPY_SOURCE);
    transition(output, output_state, D3D12_RESOURCE_STATE_COPY_DEST);
    unsafe { command.CopyResource(output, source) };
    transition(output, D3D12_RESOURCE_STATE_COPY_DEST, output_state);
    transition(source, D3D12_RESOURCE_STATE_COPY_SOURCE, source_state);
}

pub(super) unsafe extern "C" fn destroy_context(bindings: Raw, context: *mut Raw) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    unsafe { sdk.destroy.unwrap()(context, std::ptr::null()) }
}

pub(super) unsafe extern "C" fn wait(bindings: Raw, mut context: Raw) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let desc = header(RSF_FFX_WAIT_GENERATION_PRESENT as u32);
    unsafe { sdk.dispatch.unwrap()(&mut context, &desc) }
}

unsafe extern "C" fn on_generate(desc: *mut ffxDispatchDescFrameGeneration, user: Raw) -> u32 {
    unsafe { rsf_core_fsr12_generate(user, desc.cast()) }
}

unsafe extern "C" fn on_present(
    desc: *mut ffxCallbackDescFrameGenerationPresent,
    user: Raw,
) -> u32 {
    unsafe { rsf_core_fsr12_present(user, desc.cast(), u32::from((*desc).isGeneratedFrame)) }
}

fn resource(texture: Raw) -> FfxApiResource {
    unsafe { ffx_resource::describe(texture, FFX_API_RESOURCE_STATE_COMPUTE_READ as u32) }
}

pub(super) unsafe extern "C" fn configure(
    bindings: Raw,
    mut context: Raw,
    plan: *const Configuration,
) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let plan = unsafe { &*plan };
    let mut desc = ffxConfigureDescFrameGeneration {
        header: header(RSF_FFX_CONFIGURE_GENERATION as u32),
        swapChain: plan.chain,
        frameID: plan.sequence,
        frameGenerationEnabled: plan.enabled != 0,
        generationRect: FfxApiRect2D {
            left: plan.left,
            top: plan.top,
            width: plan.width,
            height: plan.height,
        },
        ..Default::default()
    };
    if plan.generation_callback != 0 {
        desc.frameGenerationCallback = Some(on_generate);
        desc.frameGenerationCallbackUserContext = plan.user;
    }
    if plan.present_callback != 0 {
        desc.presentCallback = Some(on_present);
        desc.presentCallbackUserContext = plan.user;
    }
    if !plan.hudless.is_null() {
        desc.HUDLessColor = resource(plan.hudless);
    }
    unsafe { sdk.configure.unwrap()(&mut context, (&raw const desc).cast()) }
}

pub(super) unsafe extern "C" fn prepare(
    bindings: Raw,
    mut context: Raw,
    plan: *const PreparePlan,
) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let plan = unsafe { &*plan };
    let desc = ffxDispatchDescFrameGenerationPrepareV2 {
        header: header(RSF_FFX_PREPARE_GENERATION as u32),
        frameID: plan.sequence,
        flags: 0,
        commandList: plan.command,
        renderSize: FfxApiDimensions2D {
            width: plan.render_width,
            height: plan.render_height,
        },
        jitterOffset: FfxApiFloatCoords2D {
            x: plan.jitter[0],
            y: plan.jitter[1],
        },
        motionVectorScale: FfxApiFloatCoords2D {
            x: plan.motion_scale[0],
            y: plan.motion_scale[1],
        },
        frameTimeDelta: plan.frame_time_ms,
        reset: plan.reset != 0,
        cameraNear: plan.near_plane,
        cameraFar: plan.far_plane,
        cameraFovAngleVertical: plan.vertical_fov,
        viewSpaceToMetersFactor: plan.view_space_to_meters,
        depth: resource(plan.depth),
        motionVectors: resource(plan.motion),
        cameraPosition: plan.position,
        cameraUp: plan.up,
        cameraRight: plan.right,
        cameraForward: plan.forward,
    };
    unsafe { sdk.dispatch.unwrap()(&mut context, (&raw const desc).cast()) }
}

fn raw_resource(resource: FfxApiResource) -> SdkResource {
    SdkResource {
        resource: resource.resource,
        state: resource.state,
    }
}

pub(super) unsafe extern "C" fn generate(
    bindings: Raw,
    mut context: Raw,
    raw: Raw,
    out: *mut Generated,
) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let result = unsafe { sdk.dispatch.unwrap()(&mut context, raw.cast()) };
    let desc = unsafe { &*raw.cast::<ffxDispatchDescFrameGeneration>() };
    unsafe {
        out.write(Generated {
            command: desc.commandList,
            sequence: desc.frameID,
            count: desc.numGeneratedFrames,
            reset: u32::from(desc.reset),
            input: raw_resource(desc.presentColor),
            outputs: desc.outputs.map(raw_resource),
        });
    }
    result
}

pub(super) unsafe extern "C" fn present_facts(raw: Raw, out: *mut PresentFacts) {
    let desc = unsafe { &*raw.cast::<ffxCallbackDescFrameGenerationPresent>() };
    unsafe {
        out.write(PresentFacts {
            command: desc.commandList,
            source: raw_resource(desc.currentBackBuffer),
            output: raw_resource(desc.outputSwapChainBuffer),
        });
    }
}

unsafe fn rsf_core_fsr12_native_supported(
    compiled: u32,
    device: Raw,
    directory: *const c_char,
) -> u32 {
    if compiled == 0 || device.is_null() || directory.is_null() || unsafe { directory.read() } == 0
    {
        return 0;
    }
    // The capability query uses only the runtime directory, device and separate FG major.
    let mut setup: Setup = unsafe { std::mem::zeroed() };
    setup.runtime_directory_utf8 = directory;
    setup.chain.d3d12_device = device;
    setup.feature_major = 4;
    rsf_core_fsr12_probe(&admission::NATIVE_CHECKS, &setup, probe_versions)
}

fn rsf_core_fsr12_native_provider(compiled: u32) -> *const Provider {
    static NATIVE_PROVIDER: Provider = Provider {
        create: Some(create),
        ..PROVIDER
    };
    if compiled == 0 {
        &DISABLED_PROVIDER
    } else {
        &NATIVE_PROVIDER
    }
}

unsafe extern "C" fn create(setup: *const Setup, out: *mut Raw, chain: *mut Raw) -> i32 {
    unsafe { rsf_core_fsr12_create(setup, out, chain) }
}

// The ffx-fg feature is CMake's frame-generation header detection (runtime/orchestrator/CMakeLists.txt).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_generation_fsr() -> *const Provider {
    rsf_core_fsr12_native_provider(u32::from(cfg!(feature = "ffx-fg")))
}

/// # Safety
/// `device` is null or a live ID3D12Device; `directory` is null or NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_generation_fsr4_supported(
    device: Raw,
    directory: *const c_char,
) -> u32 {
    unsafe {
        rsf_core_fsr12_native_supported(u32::from(cfg!(feature = "ffx-fg")), device, directory)
    }
}
