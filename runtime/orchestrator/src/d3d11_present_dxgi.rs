// SPDX-License-Identifier: MIT
//! Typed DXGI operations on the facade receiver and its leased physical chain.
use super::{
    DXGI_ERROR_INVALID_CALL, E_POINTER, S_OK, rsf_core_d3d11p_present, rsf_core_d3d11p_resize,
};
use crate::d3d11_present_abi::{Chain, Raw};
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, IDXGIDeviceSubObject, IDXGIObject, IDXGISwapChain,
    IDXGISwapChain1, IDXGISwapChain2, IDXGISwapChain3, IDXGISwapChain4,
};
use windows::core::{BOOL, GUID, IUnknown, Interface};

pub(super) unsafe fn rsf_d3d11p_facade_query_interface(
    facade: Raw,
    iid: *const GUID,
    out: *mut Raw,
) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { out.write(null_mut()) };
    const INTERFACES: [GUID; 8] = [
        IUnknown::IID,
        IDXGIObject::IID,
        IDXGIDeviceSubObject::IID,
        IDXGISwapChain::IID,
        IDXGISwapChain1::IID,
        IDXGISwapChain2::IID,
        IDXGISwapChain3::IID,
        IDXGISwapChain4::IID,
    ];
    if !INTERFACES.contains(unsafe { &*iid }) {
        return windows::Win32::Foundation::E_NOINTERFACE.0;
    }
    let facade_unknown = unsafe { IUnknown::from_raw_borrowed(&facade) }.unwrap();
    unsafe {
        out.write(facade);
        (facade_unknown.vtable().AddRef)(facade);
    }
    S_OK
}

#[unsafe(no_mangle)]
#[inline(never)]
pub(super) unsafe extern "C" fn rsf_d3d11p_facade_present1(
    chain: *mut Chain,
    sync: u32,
    flags: u32,
    parameters: *const DXGI_PRESENT_PARAMETERS,
) -> i32 {
    if let Some(parameters) = unsafe { parameters.as_ref() }
        && (parameters.DirtyRectsCount != 0 || !parameters.pScrollRect.is_null())
    {
        return DXGI_ERROR_INVALID_CALL;
    }
    unsafe { rsf_core_d3d11p_present(chain, sync, flags) }
}

#[unsafe(no_mangle)]
#[inline(never)]
pub(super) unsafe extern "C" fn rsf_d3d11p_facade_resize1(
    chain: *mut Chain,
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
    masks: *const u32,
    queues: *const Raw,
) -> i32 {
    if !masks.is_null() || !queues.is_null() {
        return DXGI_ERROR_INVALID_CALL;
    }
    unsafe { rsf_core_d3d11p_resize(chain, count, width, height, format, flags) }
}

pub(super) unsafe extern "C" fn query_buffer_interface(
    receiver: Raw,
    iid: *const c_void,
    out: *mut Raw,
) -> i32 {
    let object = unsafe { IUnknown::from_raw_borrowed(&receiver) }.unwrap();
    unsafe { object.query(iid.cast(), out) }.0
}

pub(super) unsafe extern "C" fn query_chain4(receiver: Raw, out: *mut Raw) -> i32 {
    unsafe {
        out.write(null_mut());
        query_buffer_interface(
            receiver,
            std::ptr::from_ref(&IDXGISwapChain4::IID).cast(),
            out,
        )
    }
}

pub(super) unsafe extern "C" fn get_fullscreen(receiver: Raw, fullscreen: *mut i32) -> i32 {
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&receiver) }.unwrap();
    let mut value = BOOL(0);
    let result = unsafe { (chain.vtable().GetFullscreenState)(receiver, &mut value, null_mut()) };
    unsafe { fullscreen.write(value.0) };
    result.0
}

pub(super) unsafe extern "C" fn set_fullscreen(receiver: Raw, fullscreen: i32, target: Raw) -> i32 {
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&receiver) }.unwrap();
    unsafe { (chain.vtable().SetFullscreenState)(receiver, BOOL(fullscreen), target) }.0
}

pub(super) unsafe extern "C" fn physical_present(receiver: Raw, sync: u32, flags: u32) -> i32 {
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&receiver) }.unwrap();
    unsafe { (chain.vtable().Present)(receiver, sync, DXGI_PRESENT(flags)) }.0
}

pub(super) unsafe extern "C" fn get_physical_buffer(
    receiver: Raw,
    index: u32,
    iid: *const c_void,
    out: *mut Raw,
) -> i32 {
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&receiver) }.unwrap();
    unsafe { (chain.vtable().GetBuffer)(receiver, index, iid.cast(), out) }.0
}

pub(super) unsafe extern "C" fn acquire_backbuffer(receiver: Raw, out: *mut Raw) -> i32 {
    let chain = unsafe { IDXGISwapChain3::from_raw_borrowed(&receiver) }.unwrap();
    let index = unsafe { (chain.vtable().GetCurrentBackBufferIndex)(receiver) };
    unsafe {
        out.write(null_mut());
        get_physical_buffer(
            receiver,
            index,
            std::ptr::from_ref(&ID3D12Resource::IID).cast(),
            out,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{fake, raw, release};
    use std::ptr::null;

    #[test]
    fn facade_interfaces_share_one_live_identity() {
        let object = fake();
        let receiver = raw(&object);
        let mut out = null_mut();
        assert_eq!(
            unsafe { rsf_d3d11p_facade_query_interface(receiver, &IDXGISwapChain4::IID, &mut out) },
            S_OK
        );
        assert_eq!(out, receiver);
        assert_eq!(unsafe { release(out.cast()) }, 1);
        assert_eq!(
            unsafe { rsf_d3d11p_facade_query_interface(receiver, &IDXGISwapChain1::IID, &mut out) },
            S_OK
        );
        assert_eq!(out, receiver);
        assert_eq!(unsafe { release(out.cast()) }, 1);
        assert_eq!(
            unsafe { rsf_d3d11p_facade_query_interface(receiver, &ID3D12Resource::IID, &mut out) },
            windows::Win32::Foundation::E_NOINTERFACE.0
        );
        assert!(out.is_null());
        assert_eq!(
            unsafe { rsf_d3d11p_facade_query_interface(receiver, &IUnknown::IID, null_mut()) },
            E_POINTER
        );
        assert_eq!(object.refs.get(), 1);
    }

    #[test]
    fn unsupported_present_and_resize_records_refuse_before_entering_owner() {
        let parameters = DXGI_PRESENT_PARAMETERS {
            DirtyRectsCount: 1,
            ..Default::default()
        };
        assert_eq!(
            unsafe { rsf_d3d11p_facade_present1(null_mut(), 1, 0, &parameters) },
            DXGI_ERROR_INVALID_CALL
        );
        assert_eq!(
            unsafe { rsf_d3d11p_facade_resize1(null_mut(), 2, 64, 32, 28, 0, &1, null()) },
            DXGI_ERROR_INVALID_CALL
        );
        assert_eq!(
            unsafe { rsf_d3d11p_facade_resize1(null_mut(), 2, 64, 32, 28, 0, null(), &null_mut()) },
            DXGI_ERROR_INVALID_CALL
        );
    }
}
