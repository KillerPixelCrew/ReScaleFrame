// SPDX-License-Identifier: MIT
//! Typed Windows graphics leases and the guarded native CPU and host callback boundary.

use rsf_game_sys::{rsf_game_host_services, rsf_game_render_config, rsf_game_render_pass};
use std::ffi::{c_char, c_void};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEXTURE2D_DESC, D3D11_VIEWPORT, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::Interface;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

unsafe extern "C" {
    fn rsf_skyrim_render_pass_guarded(
        host: &rsf_game_host_services,
        context: *mut c_void,
        pass: &rsf_game_render_pass,
        begin: u32,
    ) -> bool;
    fn rsf_skyrim_upscale_guarded(
        host: &rsf_game_host_services,
        context: *mut c_void,
        pass: &rsf_game_render_pass,
    ) -> i32;
    fn rsf_skyrim_render_config_guarded(
        host: &rsf_game_host_services,
        config: &mut rsf_game_render_config,
    ) -> bool;
}

// The native leaves contain foreign exceptions. Rust decides whether to call an optional hook.
pub(crate) unsafe extern "C" fn log_host(
    host: *const rsf_game_host_services,
    message: *const c_char,
) -> bool {
    match unsafe { host.as_ref() } {
        Some(host) => unsafe { rsf_game_sys::rsf_game_log_guarded(host.log, host.user, message) },
        _ => true,
    }
}

pub(crate) unsafe fn call_render_pass(
    host: &rsf_game_host_services,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    begin: u32,
) -> bool {
    host.render_pass.is_some()
        && unsafe { rsf_skyrim_render_pass_guarded(host, context, pass, begin) }
}

pub(crate) unsafe fn call_upscale(
    host: &rsf_game_host_services,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
) -> bool {
    host.process_upscale.is_some()
        && unsafe { rsf_skyrim_upscale_guarded(host, context, pass) } == 0
}

pub(crate) fn call_render_config(
    host: &rsf_game_host_services,
    config: &mut rsf_game_render_config,
) {
    if host.render_config.is_none() || !unsafe { rsf_skyrim_render_config_guarded(host, config) } {
        *config = rsf_game_render_config::request();
    }
}

/// A new reference to a live native COM object, or None for null.
///
/// # Safety
/// `object` is null or a live interface pointer of type `T`, such as one read from a verified
/// native graphics owner.
pub(crate) unsafe fn retain<T: Interface>(object: *mut c_void) -> Option<T> {
    unsafe { T::from_raw_borrowed(&object) }.cloned()
}

pub(crate) fn desc(texture: &ID3D11Texture2D) -> D3D11_TEXTURE2D_DESC {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    desc
}

pub(crate) fn backbuffer(chain: &IDXGISwapChain) -> Option<ID3D11Texture2D> {
    unsafe { chain.GetBuffer(0) }.ok()
}

pub(crate) fn viewport(context: &ID3D11DeviceContext) -> (D3D11_VIEWPORT, u32) {
    let mut viewport = D3D11_VIEWPORT::default();
    let mut count = 1;
    unsafe { context.RSGetViewports(&mut count, Some(&mut viewport)) };
    (viewport, count)
}

pub(crate) fn thread() -> u32 {
    unsafe { GetCurrentThreadId() }
}
