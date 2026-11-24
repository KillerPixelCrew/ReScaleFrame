// SPDX-License-Identifier: MIT
//! Application-facing COM identity. Native leaves access the backing graphics state
//! through the facade while the owner and its physical-chain leases remain in Rust.
use super::dxgi::{
    query_buffer_interface, rsf_d3d11p_facade_present1, rsf_d3d11p_facade_query_interface,
    rsf_d3d11p_facade_resize1,
};
use super::{
    DXGI_ERROR_INVALID_CALL, E_POINTER, Physical, owner, rsf_core_d3d11p_acquire_physical,
    rsf_core_d3d11p_application_presents, rsf_core_d3d11p_chain_destroy,
    rsf_core_d3d11p_duplicate_application_ready, rsf_core_d3d11p_get_buffer,
    rsf_core_d3d11p_max_latency, rsf_core_d3d11p_physical_pending, rsf_core_d3d11p_present,
    rsf_core_d3d11p_release_physical, rsf_core_d3d11p_render_index, rsf_core_d3d11p_resize,
    rsf_core_d3d11p_set_fullscreen, rsf_core_d3d11p_set_max_latency, settings,
};
// The facade state's leaves; the unit test below fakes them.
#[cfg(not(test))]
use super::native::{state_describe, state_destroy, state_device, state_parent};
use crate::allocate_handle;
use crate::d3d11_present_abi::{Chain, Raw};
use std::ffi::c_void;
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(test)]
use tests::{state_describe, state_destroy, state_device, state_parent};
use windows::Win32::Foundation::{E_NOINTERFACE, HANDLE, HWND, S_OK};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_UNSPECIFIED, DXGI_COLOR_SPACE_TYPE, DXGI_FORMAT, DXGI_MODE_DESC,
    DXGI_MODE_ROTATION,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_FRAME_STATISTICS, DXGI_HDR_METADATA_TYPE, DXGI_MATRIX_3X2_F, DXGI_PRESENT,
    DXGI_PRESENT_PARAMETERS, DXGI_RGBA, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FULLSCREEN_DESC, IDXGIDeviceSubObject_Vtbl, IDXGIObject,
    IDXGIObject_Vtbl, IDXGISwapChain, IDXGISwapChain_Vtbl, IDXGISwapChain1, IDXGISwapChain1_Vtbl,
    IDXGISwapChain2, IDXGISwapChain2_Vtbl, IDXGISwapChain3, IDXGISwapChain3_Vtbl, IDXGISwapChain4,
    IDXGISwapChain4_Vtbl,
};
use windows::core::{BOOL, GUID, HRESULT, IUnknown_Vtbl, Interface};

#[repr(C)]
struct Facade {
    vtable: &'static IDXGISwapChain4_Vtbl,
    references: AtomicU32,
    chain: *mut Chain,
    state: Raw,
}

const _: () = assert!(size_of::<IDXGISwapChain4_Vtbl>() == 41 * size_of::<Raw>());

impl Facade {
    // DXGI supplies this pointer under the caller's COM lease.
    unsafe fn borrow<'a>(this: Raw) -> &'a Self {
        unsafe { &*this.cast::<Self>() }
    }

    fn snapshot_description(&self) -> DXGI_SWAP_CHAIN_DESC {
        let mut description = DXGI_SWAP_CHAIN_DESC::default();
        unsafe { state_describe(self.state, &mut description) };
        description
    }

    fn native12(&self) -> bool {
        unsafe { owner(self.chain) }.is_some_and(|owner| owner.native12())
    }

    fn physical(&self, initialize: bool) -> Result<Lease, HRESULT> {
        let mut token = ptr::null();
        let mut chain = null_mut();
        let result = unsafe {
            rsf_core_d3d11p_acquire_physical(
                self.chain,
                u32::from(initialize),
                &mut token,
                &mut chain,
            )
        };
        if chain.is_null() {
            Err(HRESULT(result))
        } else {
            Ok(Lease { token, chain })
        }
    }

    fn forward(&self, call: impl FnOnce(Raw) -> HRESULT) -> HRESULT {
        match self.physical(true) {
            Ok(lease) => call(lease.chain),
            Err(result) => result,
        }
    }
}

struct Lease {
    token: *const Physical,
    chain: Raw,
}

impl Drop for Lease {
    fn drop(&mut self) {
        unsafe { rsf_core_d3d11p_release_physical(self.token) };
    }
}

fn switching() -> bool {
    settings().runtime_switching != 0
}

pub(super) fn rsf_d3d11p_facade_create(chain: *mut Chain, state: Raw) -> Raw {
    allocate_handle(Facade {
        vtable: &VTABLE,
        references: AtomicU32::new(1),
        chain,
        state,
    })
    .cast()
}

pub(super) unsafe extern "C" fn rsf_d3d11p_facade_state(this: Raw) -> Raw {
    unsafe { Facade::borrow(this) }.state
}

pub(super) unsafe extern "C" fn rsf_d3d11p_facade_chain(this: Raw) -> *mut Chain {
    unsafe { Facade::borrow(this) }.chain
}

unsafe extern "system" fn query_interface(this: Raw, iid: *const GUID, out: *mut Raw) -> HRESULT {
    HRESULT(unsafe { rsf_d3d11p_facade_query_interface(this, iid, out) })
}

unsafe extern "system" fn add_ref(this: Raw) -> u32 {
    unsafe { Facade::borrow(this) }
        .references
        .fetch_add(1, Ordering::SeqCst)
        + 1
}

#[inline(never)]
pub(super) unsafe extern "C" fn rsf_d3d11p_facade_release(this: Raw) -> u32 {
    let facade = unsafe { Facade::borrow(this) };
    let remaining = facade
        .references
        .fetch_sub(1, Ordering::SeqCst)
        .wrapping_sub(1);
    if remaining == 0 {
        let (chain, state) = (facade.chain, facade.state);
        // The owner may use native leaves while draining. Keep both addresses live
        // until the final GPU retirement callback returns.
        unsafe {
            rsf_core_d3d11p_chain_destroy(chain);
            state_destroy(state);
            drop(Box::from_raw(this.cast::<Facade>()));
        }
    }
    remaining
}

#[inline(never)]
unsafe extern "system" fn release(this: Raw) -> u32 {
    let result = unsafe { rsf_d3d11p_facade_release(this) };
    unsafe { ptr::read_volatile(&result) }
}

macro_rules! physical_method {
    ($interface:ty, $entry:ident ($physical:ident $(, $arg:ident)*)) => {{
        let interface = <$interface>::from_raw_borrowed(&$physical).unwrap();
        (interface.vtable().$entry)($physical $(, $arg)*)
    }};
}

macro_rules! forward {
    ($name:ident, $interface:ty, $method:ident $(, $arg:ident: $ty:ty)*) => {
        unsafe extern "system" fn $name(this: Raw $(, $arg: $ty)*) -> HRESULT {
            let facade = unsafe { Facade::borrow(this) };
            facade.forward(|physical| unsafe {
                physical_method!($interface, $method(physical $(, $arg)*))
            })
        }
    };
}

forward!(set_private_data, IDXGIObject, SetPrivateData, key: *const GUID, bytes: u32, data: *const c_void);
forward!(set_private_interface, IDXGIObject, SetPrivateDataInterface, key: *const GUID, value: Raw);
forward!(get_private_data, IDXGIObject, GetPrivateData, key: *const GUID, bytes: *mut u32, data: Raw);
forward!(resize_target, IDXGISwapChain, ResizeTarget, mode: *const DXGI_MODE_DESC);
forward!(get_containing_output, IDXGISwapChain, GetContainingOutput, out: *mut Raw);
forward!(get_restrict_to_output, IDXGISwapChain1, GetRestrictToOutput, out: *mut Raw);
forward!(set_background_color, IDXGISwapChain1, SetBackgroundColor, color: *const DXGI_RGBA);
forward!(get_background_color, IDXGISwapChain1, GetBackgroundColor, color: *mut DXGI_RGBA);
forward!(set_rotation, IDXGISwapChain1, SetRotation, value: DXGI_MODE_ROTATION);
forward!(get_rotation, IDXGISwapChain1, GetRotation, value: *mut DXGI_MODE_ROTATION);
forward!(set_source_size, IDXGISwapChain2, SetSourceSize, width: u32, height: u32);
forward!(get_source_size, IDXGISwapChain2, GetSourceSize, width: *mut u32, height: *mut u32);
forward!(set_matrix_transform, IDXGISwapChain2, SetMatrixTransform, value: *const DXGI_MATRIX_3X2_F);
forward!(get_matrix_transform, IDXGISwapChain2, GetMatrixTransform, out: *mut DXGI_MATRIX_3X2_F);
forward!(check_color_space, IDXGISwapChain3, CheckColorSpaceSupport, value: DXGI_COLOR_SPACE_TYPE, out: *mut u32);
forward!(set_hdr_metadata, IDXGISwapChain4, SetHDRMetaData, kind: DXGI_HDR_METADATA_TYPE, size: u32, metadata: *const c_void);

unsafe extern "system" fn get_parent(this: Raw, iid: *const GUID, out: *mut Raw) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    let parent = unsafe { state_parent(facade.state) };
    HRESULT(unsafe { query_buffer_interface(parent, iid.cast(), out) })
}

unsafe extern "system" fn get_device(this: Raw, iid: *const GUID, out: *mut Raw) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    let device = unsafe { state_device(facade.state) };
    HRESULT(unsafe { query_buffer_interface(device, iid.cast(), out) })
}

// A real call and volatile return load keep public detours from following a
// tail-jump into the private continuations also used by linked wrapper dispatch.
#[inline(never)]
unsafe extern "system" fn present(this: Raw, sync: u32, flags: DXGI_PRESENT) -> HRESULT {
    let chain = unsafe { Facade::borrow(this) }.chain;
    let result = HRESULT(unsafe { rsf_core_d3d11p_present(chain, sync, flags.0) });
    unsafe { ptr::read_volatile(&result) }
}

unsafe extern "system" fn get_buffer(
    this: Raw,
    index: u32,
    iid: *const GUID,
    out: *mut Raw,
) -> HRESULT {
    let chain = unsafe { Facade::borrow(this) }.chain;
    HRESULT(unsafe { rsf_core_d3d11p_get_buffer(chain, index, iid.cast(), out) })
}

#[inline(never)]
unsafe extern "system" fn set_fullscreen(this: Raw, fullscreen: BOOL, target: Raw) -> HRESULT {
    let chain = unsafe { Facade::borrow(this) }.chain;
    let result = HRESULT(unsafe { rsf_core_d3d11p_set_fullscreen(chain, fullscreen.0, target) });
    unsafe { ptr::read_volatile(&result) }
}

unsafe extern "system" fn get_fullscreen(
    this: Raw,
    enabled: *mut BOOL,
    target: *mut Raw,
) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    if unsafe { rsf_core_d3d11p_physical_pending(facade.chain) } == 0 {
        return facade.forward(|physical| unsafe {
            physical_method!(
                IDXGISwapChain,
                GetFullscreenState(physical, enabled, target)
            )
        });
    }
    if enabled.is_null() && target.is_null() {
        return windows::Win32::Foundation::E_INVALIDARG;
    }
    if !enabled.is_null() {
        let windowed = facade.snapshot_description().Windowed.0 != 0;
        unsafe { enabled.write(BOOL(i32::from(!windowed))) };
    }
    if !target.is_null() {
        unsafe { target.write(null_mut()) };
    }
    S_OK
}

unsafe extern "system" fn get_desc(this: Raw, out: *mut DXGI_SWAP_CHAIN_DESC) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    if out.is_null() {
        return HRESULT(E_POINTER);
    }
    if facade.native12() && !switching() {
        return facade.forward(|physical| unsafe {
            physical_method!(IDXGISwapChain, GetDesc(physical, out))
        });
    }
    unsafe { out.write(facade.snapshot_description()) };
    S_OK
}

#[inline(never)]
unsafe extern "system" fn resize(
    this: Raw,
    count: u32,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    flags: u32,
) -> HRESULT {
    let chain = unsafe { Facade::borrow(this) }.chain;
    let result = HRESULT(unsafe {
        rsf_core_d3d11p_resize(chain, count, width, height, format.0 as u32, flags)
    });
    unsafe { ptr::read_volatile(&result) }
}

unsafe extern "system" fn get_frame_statistics(
    this: Raw,
    out: *mut DXGI_FRAME_STATISTICS,
) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    let result = facade.forward(|physical| unsafe {
        physical_method!(IDXGISwapChain, GetFrameStatistics(physical, out))
    });
    if result.is_ok() && !out.is_null() && facade.native12() && switching() {
        unsafe { (*out).PresentCount = rsf_core_d3d11p_application_presents(facade.chain) };
    }
    result
}

unsafe extern "system" fn get_last_present_count(this: Raw, out: *mut u32) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    if (facade.native12() && switching())
        || unsafe { rsf_core_d3d11p_physical_pending(facade.chain) } != 0
    {
        if out.is_null() {
            return HRESULT(E_POINTER);
        }
        unsafe { out.write(rsf_core_d3d11p_application_presents(facade.chain)) };
        return S_OK;
    }
    facade.forward(|physical| unsafe {
        physical_method!(IDXGISwapChain, GetLastPresentCount(physical, out))
    })
}

unsafe extern "system" fn get_desc1(this: Raw, out: *mut DXGI_SWAP_CHAIN_DESC1) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    if out.is_null() {
        return HRESULT(E_POINTER);
    }
    if facade.native12() && !switching() {
        return facade.forward(|physical| unsafe {
            physical_method!(IDXGISwapChain1, GetDesc1(physical, out))
        });
    }
    let description = facade.snapshot_description();
    unsafe {
        out.write(DXGI_SWAP_CHAIN_DESC1 {
            Width: description.BufferDesc.Width,
            Height: description.BufferDesc.Height,
            Format: description.BufferDesc.Format,
            Stereo: BOOL(0),
            SampleDesc: description.SampleDesc,
            BufferUsage: description.BufferUsage,
            BufferCount: description.BufferCount,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: description.SwapEffect,
            AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
            Flags: description.Flags,
        });
    }
    S_OK
}

unsafe extern "system" fn get_fullscreen_desc(
    this: Raw,
    out: *mut DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    if out.is_null() {
        return HRESULT(E_POINTER);
    }
    if facade.native12() && !switching() {
        return facade.forward(|physical| unsafe {
            physical_method!(IDXGISwapChain1, GetFullscreenDesc(physical, out))
        });
    }
    let description = facade.snapshot_description();
    unsafe {
        out.write(DXGI_SWAP_CHAIN_FULLSCREEN_DESC {
            RefreshRate: description.BufferDesc.RefreshRate,
            ScanlineOrdering: description.BufferDesc.ScanlineOrdering,
            Scaling: description.BufferDesc.Scaling,
            Windowed: description.Windowed,
        });
    }
    S_OK
}

unsafe extern "system" fn get_hwnd(this: Raw, out: *mut HWND) -> HRESULT {
    if out.is_null() {
        return HRESULT(E_POINTER);
    }
    let facade = unsafe { Facade::borrow(this) };
    unsafe { out.write(facade.snapshot_description().OutputWindow) };
    S_OK
}

unsafe extern "system" fn get_core_window(_: Raw, _: *const GUID, out: *mut Raw) -> HRESULT {
    if !out.is_null() {
        unsafe { out.write(null_mut()) };
    }
    E_NOINTERFACE
}

#[inline(never)]
unsafe extern "system" fn present1(
    this: Raw,
    sync: u32,
    flags: DXGI_PRESENT,
    parameters: *const DXGI_PRESENT_PARAMETERS,
) -> HRESULT {
    let chain = unsafe { Facade::borrow(this) }.chain;
    let result = HRESULT(unsafe { rsf_d3d11p_facade_present1(chain, sync, flags.0, parameters) });
    unsafe { ptr::read_volatile(&result) }
}

unsafe extern "system" fn is_temporary_mono_supported(this: Raw) -> BOOL {
    let facade = unsafe { Facade::borrow(this) };
    facade.physical(false).map_or(BOOL(0), |lease| {
        let physical = lease.chain;
        unsafe { physical_method!(IDXGISwapChain1, IsTemporaryMonoSupported(physical)) }
    })
}

unsafe extern "system" fn set_max_latency(this: Raw, value: u32) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    if !facade.native12() {
        return HRESULT(DXGI_ERROR_INVALID_CALL);
    }
    if switching() {
        return HRESULT(unsafe { rsf_core_d3d11p_set_max_latency(facade.chain, value) });
    }
    facade.forward(|physical| unsafe {
        physical_method!(IDXGISwapChain2, SetMaximumFrameLatency(physical, value))
    })
}

unsafe extern "system" fn get_max_latency(this: Raw, out: *mut u32) -> HRESULT {
    let facade = unsafe { Facade::borrow(this) };
    if out.is_null() {
        return HRESULT(E_POINTER);
    }
    if !facade.native12() {
        unsafe { out.write(1) };
        return S_OK;
    }
    if switching() {
        return HRESULT(unsafe { rsf_core_d3d11p_max_latency(facade.chain, out) });
    }
    facade.forward(|physical| unsafe {
        physical_method!(IDXGISwapChain2, GetMaximumFrameLatency(physical, out))
    })
}

unsafe extern "system" fn get_waitable_object(this: Raw) -> HANDLE {
    let facade = unsafe { Facade::borrow(this) };
    if !facade.native12() {
        return HANDLE(null_mut());
    }
    if switching() {
        // This duplicate belongs to the application and outlives provider switches.
        return HANDLE(unsafe { rsf_core_d3d11p_duplicate_application_ready(facade.chain) });
    }
    facade.physical(false).map_or(HANDLE(null_mut()), |lease| {
        let physical = lease.chain;
        unsafe { physical_method!(IDXGISwapChain2, GetFrameLatencyWaitableObject(physical)) }
    })
}

unsafe extern "system" fn get_backbuffer_index(this: Raw) -> u32 {
    let facade = unsafe { Facade::borrow(this) };
    if !facade.native12() {
        // D3D11 keeps a view of logical slot zero across Present.
        return 0;
    }
    if switching() {
        return unsafe { rsf_core_d3d11p_render_index(facade.chain) };
    }
    facade.physical(false).map_or(0, |lease| {
        let physical = lease.chain;
        unsafe { physical_method!(IDXGISwapChain3, GetCurrentBackBufferIndex(physical)) }
    })
}

#[inline(never)]
pub(super) unsafe extern "C" fn rsf_d3d11p_facade_color_space(this: Raw, value: u32) -> i32 {
    let facade = unsafe { Facade::borrow(this) };
    let value = DXGI_COLOR_SPACE_TYPE(value as i32);
    facade
        .forward(|physical| unsafe {
            physical_method!(IDXGISwapChain3, SetColorSpace1(physical, value))
        })
        .0
}

#[inline(never)]
unsafe extern "system" fn set_color_space(this: Raw, value: DXGI_COLOR_SPACE_TYPE) -> HRESULT {
    let result = HRESULT(unsafe { rsf_d3d11p_facade_color_space(this, value.0 as u32) });
    unsafe { ptr::read_volatile(&result) }
}

#[inline(never)]
unsafe extern "system" fn resize1(
    this: Raw,
    count: u32,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    flags: u32,
    masks: *const u32,
    queues: *const Raw,
) -> HRESULT {
    let chain = unsafe { Facade::borrow(this) }.chain;
    let result = HRESULT(unsafe {
        rsf_d3d11p_facade_resize1(
            chain,
            count,
            width,
            height,
            format.0 as u32,
            flags,
            masks,
            queues,
        )
    });
    unsafe { ptr::read_volatile(&result) }
}

static VTABLE: IDXGISwapChain4_Vtbl = IDXGISwapChain4_Vtbl {
    base__: IDXGISwapChain3_Vtbl {
        base__: IDXGISwapChain2_Vtbl {
            base__: IDXGISwapChain1_Vtbl {
                base__: IDXGISwapChain_Vtbl {
                    base__: IDXGIDeviceSubObject_Vtbl {
                        base__: IDXGIObject_Vtbl {
                            base__: IUnknown_Vtbl {
                                QueryInterface: query_interface,
                                AddRef: add_ref,
                                Release: release,
                            },
                            SetPrivateData: set_private_data,
                            SetPrivateDataInterface: set_private_interface,
                            GetPrivateData: get_private_data,
                            GetParent: get_parent,
                        },
                        GetDevice: get_device,
                    },
                    Present: present,
                    GetBuffer: get_buffer,
                    SetFullscreenState: set_fullscreen,
                    GetFullscreenState: get_fullscreen,
                    GetDesc: get_desc,
                    ResizeBuffers: resize,
                    ResizeTarget: resize_target,
                    GetContainingOutput: get_containing_output,
                    GetFrameStatistics: get_frame_statistics,
                    GetLastPresentCount: get_last_present_count,
                },
                GetDesc1: get_desc1,
                GetFullscreenDesc: get_fullscreen_desc,
                GetHwnd: get_hwnd,
                GetCoreWindow: get_core_window,
                Present1: present1,
                IsTemporaryMonoSupported: is_temporary_mono_supported,
                GetRestrictToOutput: get_restrict_to_output,
                SetBackgroundColor: set_background_color,
                GetBackgroundColor: get_background_color,
                SetRotation: set_rotation,
                GetRotation: get_rotation,
            },
            SetSourceSize: set_source_size,
            GetSourceSize: get_source_size,
            SetMaximumFrameLatency: set_max_latency,
            GetMaximumFrameLatency: get_max_latency,
            GetFrameLatencyWaitableObject: get_waitable_object,
            SetMatrixTransform: set_matrix_transform,
            GetMatrixTransform: get_matrix_transform,
        },
        GetCurrentBackBufferIndex: get_backbuffer_index,
        CheckColorSpaceSupport: check_color_space,
        SetColorSpace1: set_color_space,
        ResizeBuffers1: resize1,
    },
    SetHDRMetaData: set_hdr_metadata,
};

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::IUnknown;

    struct State {
        description: DXGI_SWAP_CHAIN_DESC,
        destroys: AtomicU32,
    }

    pub(super) unsafe fn state_describe(state: Raw, out: *mut DXGI_SWAP_CHAIN_DESC) {
        unsafe { out.write((*state.cast::<State>()).description) };
    }

    pub(super) unsafe fn state_parent(_: Raw) -> Raw {
        null_mut()
    }

    pub(super) unsafe fn state_device(_: Raw) -> Raw {
        null_mut()
    }

    pub(super) unsafe fn state_destroy(state: Raw) {
        unsafe { &*state.cast::<State>() }
            .destroys
            .fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn generated_interfaces_access_one_facade_and_destroy_its_state_once() {
        let mut description = DXGI_SWAP_CHAIN_DESC::default();
        description.BufferDesc.Width = 640;
        description.BufferDesc.Height = 360;
        description.BufferCount = 2;
        let mut state = State {
            description,
            destroys: AtomicU32::new(0),
        };
        let raw = rsf_d3d11p_facade_create(null_mut(), ptr::from_mut(&mut state).cast());
        assert!(!raw.is_null());
        let chain = unsafe { IDXGISwapChain4::from_raw(raw) };
        let identity: IUnknown = chain.cast().unwrap();
        let legacy: IDXGISwapChain = chain.cast().unwrap();
        assert_eq!(identity.as_raw(), raw);
        assert_eq!(legacy.as_raw(), raw);
        let desc = unsafe { legacy.GetDesc() }.unwrap();
        assert_eq!(desc.BufferDesc.Width, 640);
        let desc1 = unsafe { chain.GetDesc1() }.unwrap();
        assert_eq!((desc1.Height, desc1.BufferCount), (360, 2));
        assert_eq!(unsafe { chain.GetCurrentBackBufferIndex() }, 0);
        assert_eq!(unsafe { chain.GetMaximumFrameLatency() }.unwrap(), 1);
        drop((chain, legacy));
        assert_eq!(state.destroys.load(Ordering::SeqCst), 0);
        drop(identity);
        assert_eq!(state.destroys.load(Ordering::SeqCst), 1);
    }
}
