// SPDX-License-Identifier: MIT
//! Typed Windows graphics leases and the guarded native CPU and host callback boundary.

use rsf_game_sys::{rsf_game_host_services, rsf_game_render_config, rsf_game_render_pass};
use std::ffi::{c_char, c_void};
use std::ptr::{self, NonNull};
use winapi::Interface;
use winapi::shared::dxgi::IDXGISwapChain;
use winapi::um::d3d11::{
    D3D11_TEXTURE2D_DESC, D3D11_VIEWPORT, ID3D11DeviceContext, ID3D11Texture2D,
};
use winapi::um::unknwnbase::IUnknown;

unsafe extern "C" {
    pub(crate) fn rsf_skyrim_render_allowed() -> bool;
    pub(crate) fn rsf_skyrim_render_identity(pass: &mut rsf_game_render_pass) -> bool;
    pub(crate) fn rsf_skyrim_render_eligible() -> bool;
    pub(crate) fn rsf_skyrim_render_diagnostic(stage: *const c_char, pass: &rsf_game_render_pass);
    pub(crate) fn rsf_skyrim_call_render_pass(
        host: &rsf_game_host_services,
        context: *mut c_void,
        pass: &rsf_game_render_pass,
        begin: u32,
    ) -> bool;
    pub(crate) fn rsf_skyrim_call_upscale(
        host: &rsf_game_host_services,
        context: *mut c_void,
        pass: &rsf_game_render_pass,
    ) -> bool;
    pub(crate) fn rsf_skyrim_call_render_config(
        host: &rsf_game_host_services,
        config: &mut rsf_game_render_config,
    );
}

pub(crate) struct ComRef<T: Interface>(NonNull<T>);

impl<T: Interface> ComRef<T> {
    // All callers use an interface from a verified live native graphics owner.
    pub(crate) unsafe fn retain(object: *mut T) -> Option<Self> {
        let object = unsafe { Self::from_owned(object) }?;
        unsafe { (*object.as_raw().cast::<IUnknown>()).AddRef() };
        Some(object)
    }

    // COM getters transfer a reference to the caller on success.
    pub(crate) unsafe fn from_owned(object: *mut T) -> Option<Self> {
        NonNull::new(object).map(Self)
    }

    pub(crate) fn as_raw(&self) -> *mut T {
        self.0.as_ptr()
    }

    pub(crate) fn query<U: Interface>(&self) -> Option<ComRef<U>> {
        let mut object = ptr::null_mut();
        let result = unsafe {
            (*self.as_raw().cast::<IUnknown>()).QueryInterface(&U::uuidof(), &mut object)
        };
        if result < 0 {
            return None;
        }
        unsafe { ComRef::from_owned(object.cast()) }
    }
}

impl<T: Interface> Clone for ComRef<T> {
    fn clone(&self) -> Self {
        unsafe { (*self.as_raw().cast::<IUnknown>()).AddRef() };
        Self(self.0)
    }
}

impl<T: Interface> Drop for ComRef<T> {
    fn drop(&mut self) {
        unsafe { (*self.as_raw().cast::<IUnknown>()).Release() };
    }
}

impl ComRef<ID3D11Texture2D> {
    pub(crate) fn desc(&self) -> D3D11_TEXTURE2D_DESC {
        let mut desc = unsafe { std::mem::zeroed() };
        unsafe { (*self.as_raw()).GetDesc(&mut desc) };
        desc
    }
}

impl ComRef<IDXGISwapChain> {
    pub(crate) fn backbuffer(&self) -> Option<ComRef<ID3D11Texture2D>> {
        let mut texture = ptr::null_mut();
        let status =
            unsafe { (*self.as_raw()).GetBuffer(0, &ID3D11Texture2D::uuidof(), &mut texture) };
        if status < 0 {
            return None;
        }
        unsafe { ComRef::from_owned(texture.cast()) }
    }
}

pub(crate) unsafe fn viewport(context: *mut ID3D11DeviceContext) -> (D3D11_VIEWPORT, u32) {
    let mut viewport = unsafe { std::mem::zeroed() };
    let mut count = 1;
    unsafe { (*context).RSGetViewports(&mut count, &mut viewport) };
    (viewport, count)
}

pub(crate) fn thread() -> u32 {
    unsafe { winapi::um::processthreadsapi::GetCurrentThreadId() }
}
