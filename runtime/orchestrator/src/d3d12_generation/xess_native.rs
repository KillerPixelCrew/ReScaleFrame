// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::backend_result::MISSING_ENTRY_POINT;

use rsf_vk_xess_fg::sdk::*;
use std::ffi::CStr;
use windows::Win32::{
    Foundation::HANDLE,
    Graphics::{Direct3D12::*, Dxgi::*},
};
use windows::core::{GUID, Interface};
use windows_sys::Win32::{
    Foundation::S_OK,
    System::{LibraryLoader::GetProcAddress, Threading::CreateEventW},
};

type Create = unsafe extern "C" fn(Raw, *mut Raw) -> i32;
type Destroy = unsafe extern "C" fn(Raw) -> i32;
type SetUnsigned = unsafe extern "C" fn(Raw, u32) -> i32;
type InitChain = unsafe extern "C" fn(
    Raw,
    Raw,
    *const DXGI_SWAP_CHAIN_DESC1,
    *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    Raw,
    Raw,
    *const xefg_swapchain_d3d12_init_params_t,
) -> i32;
type ChainPtr = unsafe extern "C" fn(Raw, *const GUID, *mut Raw) -> i32;

struct Sdk {
    create_fg: Create,
    create_xell: Create,
    destroy_fg: Destroy,
    destroy_xell: Destroy,
    properties: unsafe extern "C" fn(Raw, *mut xefg_swapchain_properties_t) -> i32,
    bind_xell: unsafe extern "C" fn(Raw, Raw) -> i32,
    version: unsafe extern "C" fn(*mut xefg_swapchain_version_t) -> i32,
    init_chain: InitChain,
    chain_ptr: ChainPtr,
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
            unsafe { GetProcAddress(module, name.as_ptr().cast()) }.ok_or(MISSING_ENTRY_POINT)
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
            init_chain: entry!(fg, c"xefgSwapChainD3D12InitFromSwapChainDesc"),
            chain_ptr: entry!(fg, c"xefgSwapChainD3D12GetSwapChainPtr"),
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
    let setup = unsafe { &*setup };
    let Ok(factory) =
        (unsafe { CreateDXGIFactory2::<IDXGIFactory2>(DXGI_CREATE_FACTORY_FLAGS(0)) })
    else {
        return INIT_FAILED;
    };
    let desc = chain_description(&setup.chain);
    let mut flags = 0;
    if setup.depth_inverted != 0 {
        flags |= XEFG_SWAPCHAIN_INIT_FLAG_INVERTED_DEPTH as u32;
    }
    if setup.motion_jittered != 0 {
        flags |= XEFG_SWAPCHAIN_INIT_FLAG_JITTERED_MV as u32;
    }
    let init = xefg_swapchain_d3d12_init_params_t {
        maxInterpolatedFrames: reserved,
        initFlags: flags,
        uiMode: match setup.chain.ui_mode {
            HUDLESS_UI => XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS_UITEXTURE,
            HUDLESS => XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS,
            _ => XEFG_SWAPCHAIN_UI_MODE_NONE,
        },
        ..Default::default()
    };
    let result = unsafe {
        (sdk.init_chain)(
            context,
            setup.chain.hwnd,
            &desc,
            std::ptr::null(),
            setup.chain.d3d12_queue,
            factory.as_raw(),
            &init,
        )
    };
    if result != XEFG_SWAPCHAIN_RESULT_SUCCESS {
        return INIT_FAILED;
    }
    let mut chain = null_mut();
    let result = unsafe { (sdk.chain_ptr)(context, &IDXGISwapChain4::IID, &mut chain) };
    if result != XEFG_SWAPCHAIN_RESULT_SUCCESS {
        drop(unsafe { ComRef::from_owned(chain) });
        return INIT_FAILED;
    }
    unsafe { out.write(chain) };
    OK
}

unsafe extern "C" fn create_fence(device: Raw) -> Raw {
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }.unwrap();
    unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }
        .map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn create_event() -> Raw {
    unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) }
}

unsafe extern "C" fn drain(queue: Raw, fence: Raw, event: Raw, serial: u64) -> i32 {
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.unwrap();
    let fence = unsafe { ID3D12Fence::from_raw_borrowed(&fence) }.unwrap();
    if let Err(err) = unsafe { queue.Signal(fence, serial) } {
        return err.code().0;
    }
    if serial != 0 {
        let result = crate::d3d12_fence::wait_for_value(
            serial,
            10_000,
            || unsafe { fence.GetCompletedValue() },
            || {
                unsafe { fence.SetEventOnCompletion(serial, HANDLE(event)) }
                    .map_or_else(|err| err.code().0, |()| S_OK)
            },
            |remaining| unsafe { crate::d3d12_fence::wait_event(event, remaining) },
        );
        if result < 0 {
            return result;
        }
    }
    match crate::d3d12_fence::FenceState::check(unsafe { fence.GetCompletedValue() }, serial) {
        crate::d3d12_fence::FenceState::DeviceRemoved => DXGI_ERROR_DEVICE_REMOVED.0,
        crate::d3d12_fence::FenceState::Pending => DXGI_ERROR_DEVICE_HUNG.0,
        crate::d3d12_fence::FenceState::Complete => S_OK,
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

static OPS: Ops = Ops {
    check: &admission::NATIVE_CHECKS,
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
    create_fence,
    create_event,
    drain,
};

fn rsf_core_xeg12_native_provider(compiled: u32) -> *const Provider {
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
    unsafe { rsf_core_xeg12_create(&OPS, setup, out, chain) }
}

// The xess-fg feature is CMake's XeSS-FG header detection (runtime/orchestrator/CMakeLists.txt).
#[unsafe(no_mangle)]
extern "C" fn rsf_generation_xess() -> *const Provider {
    rsf_core_xeg12_native_provider(u32::from(cfg!(feature = "xess-fg")))
}
