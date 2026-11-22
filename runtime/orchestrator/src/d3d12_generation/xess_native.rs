// SPDX-License-Identifier: MIT
use super::*;

#[path = "xess_abi.rs"]
mod abi;
use abi::*;
use std::ffi::CStr;
use std::sync::OnceLock;
use windows_sys::Win32::{Foundation::FARPROC, System::LibraryLoader::GetProcAddress};

const MISSING_ENTRY: i32 = -5;

type Create = unsafe extern "C" fn(Raw, *mut Raw) -> i32;
type Destroy = unsafe extern "C" fn(Raw) -> i32;
type SetUnsigned = unsafe extern "C" fn(Raw, u32) -> i32;

struct Sdk {
    create_fg: Create,
    create_xell: Create,
    destroy_fg: Destroy,
    destroy_xell: Destroy,
    properties: unsafe extern "C" fn(Raw, *mut xefg_swapchain_properties_t) -> i32,
    bind_xell: unsafe extern "C" fn(Raw, Raw) -> i32,
    version: unsafe extern "C" fn(*mut xefg_swapchain_version_t) -> i32,
    init_chain: unsafe extern "system" fn() -> isize,
    chain_ptr: unsafe extern "system" fn() -> isize,
    compose: unsafe extern "C" fn(Raw, i32) -> i32,
    count: SetUnsigned,
    enabled: SetUnsigned,
    sleep_mode: unsafe extern "C" fn(Raw, *const xell_sleep_params_t) -> i32,
    sleep: SetUnsigned,
    marker: unsafe extern "C" fn(Raw, u32, i32) -> i32,
    present_id: SetUnsigned,
    tag: unsafe extern "C" fn(Raw, Raw, u32, *const xefg_swapchain_d3d12_resource_data_t) -> i32,
    constants: unsafe extern "C" fn(Raw, u32, *const xefg_swapchain_frame_constant_data_t) -> i32,
    present_status: unsafe extern "C" fn(Raw, *mut xefg_swapchain_present_status_t) -> i32,
}

impl Sdk {
    #[allow(clippy::missing_transmute_annotations)] // The typed SDK table supplies the destination.
    fn resolve(fg: Raw, xell: Raw) -> Result<Self, i32> {
        fn symbol(module: Raw, name: &CStr) -> Result<unsafe extern "system" fn() -> isize, i32> {
            unsafe { GetProcAddress(module, name.as_ptr().cast()) }.ok_or(MISSING_ENTRY)
        }

        macro_rules! entry {
            ($module:ident, $name:literal) => {{
                // Bindings keeps both DLL references until after their SDK objects are destroyed.
                unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, _>(symbol(
                        $module, $name,
                    )?)
                }
            }};
        }
        Ok(Self {
            create_fg: entry!(fg, c"xefgSwapChainD3D12CreateContext"),
            create_xell: entry!(xell, c"xellD3D12CreateContext"),
            destroy_fg: entry!(fg, c"xefgSwapChainDestroy"),
            destroy_xell: entry!(xell, c"xellDestroyContext"),
            properties: entry!(fg, c"xefgSwapChainGetProperties"),
            bind_xell: entry!(fg, c"xefgSwapChainSetLatencyReduction"),
            version: entry!(fg, c"xefgSwapChainGetVersion"),
            init_chain: symbol(fg, c"xefgSwapChainD3D12InitFromSwapChainDesc")?,
            chain_ptr: symbol(fg, c"xefgSwapChainD3D12GetSwapChainPtr")?,
            compose: entry!(fg, c"xefgSwapChainSetUiCompositionState"),
            count: entry!(fg, c"xefgSwapChainSetNumInterpolatedFrames"),
            enabled: entry!(fg, c"xefgSwapChainSetEnabled"),
            sleep_mode: entry!(xell, c"xellSetSleepMode"),
            sleep: entry!(xell, c"xellSleep"),
            marker: entry!(xell, c"xellAddMarkerData"),
            present_id: entry!(fg, c"xefgSwapChainSetPresentId"),
            tag: entry!(fg, c"xefgSwapChainD3D12TagFrameResource"),
            constants: entry!(fg, c"xefgSwapChainTagFrameConstants"),
            present_status: entry!(fg, c"xefgSwapChainGetLastPresentStatus"),
        })
    }
}

#[repr(C)]
struct Native {
    check: &'static Checks,
    create_chain: unsafe extern "C" fn(FARPROC, FARPROC, Raw, *const Setup, u32, *mut Raw) -> i32,
    create_fence: unsafe extern "C" fn(Raw) -> Raw,
    create_event: unsafe extern "C" fn() -> Raw,
    drain: unsafe extern "C" fn(Raw, Raw, Raw, u64) -> i32,
}

const _: () = assert!(size_of::<Native>() == 5 * size_of::<Raw>());

static NATIVE: OnceLock<&Native> = OnceLock::new();
static OPS: OnceLock<Ops> = OnceLock::new();

unsafe extern "C" fn resolve(_: &Setup, fg: Raw, xell: Raw, out: &mut Raw) -> i32 {
    let sdk = match Sdk::resolve(fg, xell) {
        Ok(sdk) => sdk,
        Err(result) => return result,
    };
    *out = crate::allocate_handle(sdk).cast();
    if out.is_null() { INIT_FAILED } else { OK }
}

unsafe extern "C" fn close(bindings: Raw) {
    drop(unsafe { Box::from_raw(bindings.cast::<Sdk>()) });
}

macro_rules! route {
    ($name:ident($($arg:ident: $ty:ty),*)) => {
        unsafe extern "C" fn $name(bindings: Raw, $($arg: $ty),*) -> i32 {
            let sdk = unsafe { &*bindings.cast::<Sdk>() };
            unsafe { (sdk.$name)($($arg),*) }
        }
    };
}
route!(create_fg(device: Raw, out: *mut Raw));
route!(create_xell(device: Raw, out: *mut Raw));
route!(destroy_fg(context: Raw));
route!(destroy_xell(context: Raw));
route!(bind_xell(context: Raw, latency: Raw));
route!(enabled(context: Raw, enabled: u32));
route!(count(context: Raw, count: u32));
route!(sleep(context: Raw, frame: u32));
route!(present_id(context: Raw, frame: u32));

unsafe extern "C" fn max_count(bindings: Raw, context: Raw, out: *mut u32) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let mut properties = xefg_swapchain_properties_t::default();
    let result = unsafe { (sdk.properties)(context, &mut properties) };
    unsafe { out.write(properties.maxSupportedInterpolations) };
    result
}

unsafe extern "C" fn version(bindings: Raw, out: *mut Version) {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let mut version = xefg_swapchain_version_t::default();
    unsafe {
        (sdk.version)(&mut version);
        out.write(Version {
            major: version.major.into(),
            minor: version.minor.into(),
            patch: version.patch.into(),
        });
    }
}

unsafe extern "C" fn create_chain(
    bindings: Raw,
    context: Raw,
    setup: *const Setup,
    reserved: u32,
    out: *mut Raw,
) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let native = NATIVE.get().unwrap();
    unsafe {
        (native.create_chain)(
            Some(sdk.init_chain),
            Some(sdk.chain_ptr),
            context,
            setup,
            reserved,
            out,
        )
    }
}

unsafe extern "C" fn compose(bindings: Raw, context: Raw, enabled: u32) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let mode = if enabled != 0 {
        XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_ENABLED
    } else {
        XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_DISABLED
    };
    unsafe { (sdk.compose)(context, mode) }
}

unsafe extern "C" fn sleep_mode(
    bindings: Raw,
    context: Raw,
    interval: u32,
    low_latency: u32,
) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let mut mode = xell_sleep_params_t {
        minimumIntervalUs: interval,
        ..Default::default()
    };
    mode.set_bLowLatencyMode(u32::from(low_latency != 0));
    unsafe { (sdk.sleep_mode)(context, &mode) }
}

unsafe extern "C" fn marker(bindings: Raw, context: Raw, frame: u32, marker: u32) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    unsafe { (sdk.marker)(context, frame, marker as i32) }
}

unsafe extern "C" fn tag(
    bindings: Raw,
    context: Raw,
    command: Raw,
    frame: u32,
    kind: u32,
    resource: *const Resource,
) -> i32 {
    const TYPES: [i32; 4] = [
        XEFG_SWAPCHAIN_RES_DEPTH,
        XEFG_SWAPCHAIN_RES_MOTION_VECTOR,
        XEFG_SWAPCHAIN_RES_HUDLESS_COLOR,
        XEFG_SWAPCHAIN_RES_UI,
    ];
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let resource = unsafe { &*resource };
    let data = xefg_swapchain_d3d12_resource_data_t {
        type_: TYPES[kind as usize],
        validity: XEFG_SWAPCHAIN_RV_ONLY_NOW,
        resourceBase: xefg_swapchain_2d_t {
            x: resource.x,
            y: resource.y,
        },
        resourceSize: xefg_swapchain_2d_t {
            x: resource.width,
            y: resource.height,
        },
        pResource: resource.resource,
        incomingState: resource.state,
    };
    unsafe { (sdk.tag)(context, command, frame, &data) }
}

unsafe extern "C" fn constants(
    bindings: Raw,
    context: Raw,
    id: u32,
    frame: *const Frame,
    reset: u32,
) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let frame = unsafe { &*frame };
    let camera = unsafe { &(*frame.record).camera };
    let data = xefg_swapchain_frame_constant_data_t {
        viewMatrix: camera.world_to_view,
        projectionMatrix: camera.view_to_clip,
        jitterOffsetX: camera.jitter_pixels[0],
        jitterOffsetY: camera.jitter_pixels[1],
        motionVectorScaleX: frame.motion_scale_x,
        motionVectorScaleY: frame.motion_scale_y,
        resetHistory: reset,
        // Simulation delta does not measure Present cadence. Zero is unspecified.
        frameRenderTime: 0.0,
    };
    unsafe { (sdk.constants)(context, id, &data) }
}

unsafe extern "C" fn present_status(bindings: Raw, context: Raw, out: *mut Present) -> i32 {
    let sdk = unsafe { &*bindings.cast::<Sdk>() };
    let mut status = xefg_swapchain_present_status_t::default();
    let result = unsafe { (sdk.present_status)(context, &mut status) };
    unsafe {
        out.write(Present {
            interpolation: status.frameGenResult,
            enabled: status.isFrameGenEnabled,
            frames: status.framesPresented,
        });
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xeg12_create_native(
    native: &'static Native,
    setup: *const Setup,
    out: *mut Raw,
    chain: *mut Raw,
) -> i32 {
    let native = NATIVE.get_or_init(|| native);
    let ops = OPS.get_or_init(|| Ops {
        check: native.check,
        resolve,
        close,
        create_fg,
        create_xell,
        destroy_fg,
        destroy_xell,
        max_count,
        bind_xell,
        version,
        create_chain,
        compose,
        count,
        enabled,
        sleep_mode,
        sleep,
        marker,
        present_id,
        tag,
        constants,
        present_status,
        create_fence: native.create_fence,
        create_event: native.create_event,
        drain: native.drain,
    });
    unsafe { rsf_core_xeg12_create(ops, setup, out, chain) }
}
