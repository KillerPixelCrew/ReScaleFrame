// SPDX-License-Identifier: MIT
#[path = "fsr_abi.rs"]
mod abi;
use super::*;
use crate::d3d12_sr::ffx_abi::*;
use abi::*;
use std::cell::UnsafeCell;
use std::sync::OnceLock;

const FFX_OK: u32 = FFX_API_RETURN_OK as u32;

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
    create_entry: Entry,
    // Context creation retains the linked descriptions and their backing storage.
    description: UnsafeCell<CreateDescription>,
    chain: Cell<Raw>,
}

#[repr(C)]
struct Native {
    check: &'static Checks,
    capture: &'static CapturePlatform,
    sdk: &'static SdkApi,
    generation_version: u32,
    create_chain:
        unsafe extern "C" fn(Entry, *const Setup, *mut Raw, *mut Raw, *mut u32, *mut Raw) -> i32,
    close_chain: unsafe extern "C" fn(Raw),
    resource: unsafe extern "C" fn(Raw, *mut FfxApiResource),
    surface_format: unsafe extern "C" fn(u32) -> u32,
    chain_extent: unsafe extern "C" fn(Raw, *mut Extent) -> i32,
    whole_texture: unsafe extern "C" fn(*const Resource) -> u32,
    copy: unsafe extern "C" fn(Raw, Raw, u32, Raw, u32),
    present_count: unsafe extern "C" fn(Raw, *mut u32) -> i32,
}

const _: () = assert!(size_of::<Native>() == 12 * size_of::<Raw>());

static NATIVE: OnceLock<&Native> = OnceLock::new();
static OPS: OnceLock<Ops> = OnceLock::new();

fn platform() -> &'static Native {
    NATIVE.get().unwrap()
}

fn header(kind: u32) -> ffxApiHeader {
    ffxApiHeader {
        type_: kind.into(),
        pNext: null_mut(),
    }
}

unsafe extern "C" fn resolve(setup: &Setup, _: Raw, _: Raw, out: &mut Raw) -> i32 {
    let sdk = Sdk::default();
    unsafe { (*sdk.description.get()).backend.device = setup.chain.d3d12_device };
    *out = crate::allocate_handle(sdk).cast();
    if out.is_null() { INIT_FAILED } else { OK }
}

unsafe extern "C" fn close(bindings: Raw) {
    let sdk = unsafe { Box::from_raw(bindings.cast::<Sdk>()) };
    if !sdk.chain.get().is_null() {
        unsafe { (platform().close_chain)(sdk.chain.get()) };
    }
}

unsafe extern "C" fn bind(bindings: Raw, entries: *const Entry) {
    let sdk = unsafe { &mut *bindings.cast::<Sdk>() };
    let [create, destroy, query, dispatch, configure] = unsafe { *entries.cast::<[Entry; 5]>() };
    sdk.create_entry = create;
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
        return UNSUPPORTED;
    };
    *count = versions.len() as u64;
    OK
}

unsafe extern "C" fn versions(
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

unsafe extern "C" fn create_generation(
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
    data.version.version = platform().generation_version;
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

unsafe extern "C" fn provider_version(bindings: Raw, mut context: Raw, out: *mut u64) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let mut actual = ffxQueryGetProviderVersion {
        header: header(FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION),
        ..Default::default()
    };
    let code = unsafe { sdk.query.unwrap()(&mut context, (&raw mut actual).cast()) };
    unsafe { out.write(actual.versionId) };
    code
}

unsafe extern "C" fn create_chain(
    bindings: Raw,
    setup: *const Setup,
    context: *mut Raw,
    chain: *mut Raw,
    result: *mut u32,
) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    unsafe {
        (platform().create_chain)(
            sdk.create_entry,
            setup,
            context,
            chain,
            result,
            sdk.chain.as_ptr(),
        )
    }
}

unsafe extern "C" fn destroy_context(bindings: Raw, context: *mut Raw) -> u32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    unsafe { sdk.destroy.unwrap()(context, std::ptr::null()) }
}

unsafe extern "C" fn wait(bindings: Raw, mut context: Raw) -> u32 {
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
    let mut resource = FfxApiResource::default();
    unsafe { (platform().resource)(texture, &mut resource) };
    resource
}

unsafe extern "C" fn configure(bindings: Raw, mut context: Raw, plan: *const Configuration) -> u32 {
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

unsafe extern "C" fn prepare(bindings: Raw, mut context: Raw, plan: *const PreparePlan) -> u32 {
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

unsafe extern "C" fn generate(
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

unsafe extern "C" fn present_facts(raw: Raw, out: *mut PresentFacts) {
    let desc = unsafe { &*raw.cast::<ffxCallbackDescFrameGenerationPresent>() };
    unsafe {
        out.write(PresentFacts {
            command: desc.commandList,
            source: raw_resource(desc.currentBackBuffer),
            output: raw_resource(desc.outputSwapChainBuffer),
        });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fsr12_probe_native(checks: &Checks, setup: &Setup) -> u32 {
    rsf_core_fsr12_probe(checks, setup, probe_versions)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr12_create_native(
    native: &'static Native,
    setup: *const Setup,
    out: *mut Raw,
    chain: *mut Raw,
) -> i32 {
    let native = NATIVE.get_or_init(|| native);
    let ops = OPS.get_or_init(|| Ops {
        check: native.check,
        capture: native.capture,
        sdk: native.sdk,
        resolve,
        close,
        bind,
        versions,
        surface_format: native.surface_format,
        create_generation,
        provider_version,
        create_chain,
        destroy_context,
        wait,
        chain_extent: native.chain_extent,
        whole_texture: native.whole_texture,
        configure,
        prepare,
        generate,
        present_facts,
        copy: native.copy,
        present_count: native.present_count,
    });
    unsafe { rsf_core_fsr12_create(ops, setup, out, chain) }
}
