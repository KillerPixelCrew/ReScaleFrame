// SPDX-License-Identifier: MIT
use super::{Clock, Surface};
use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetTickCount64};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F10};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_read_clock(out: &mut Clock) {
    let mut foreground = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground);
        out.process = GetCurrentProcessId();
        out.tick = GetTickCount64();
        out.key_down =
            u32::from(foreground == out.process && GetAsyncKeyState(i32::from(VK_F10)) < 0);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_read_local_time(out: &mut Clock) {
    let mut now = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut now) };
    out.year = u32::from(now.wYear);
    out.month = u32::from(now.wMonth);
    out.day = u32::from(now.wDay);
    out.hour = u32::from(now.wHour);
    out.minute = u32::from(now.wMinute);
    out.second = u32::from(now.wSecond);
    out.millisecond = u32::from(now.wMilliseconds);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_open_texture(
    context: *mut c_void,
    source: *mut c_void,
    surface: &mut Surface,
) -> *mut c_void {
    let (Some(context), Some(source)) = (unsafe {
        (
            ID3D11DeviceContext::from_raw_borrowed(&context),
            IUnknown::from_raw_borrowed(&source),
        )
    }) else {
        return ptr::null_mut();
    };
    if unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return ptr::null_mut();
    }
    let Ok(texture) = source.cast::<ID3D11Texture2D>() else {
        return ptr::null_mut();
    };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    *surface = Surface {
        width: desc.Width,
        height: desc.Height,
        format: desc.Format.0 as u32,
    };
    texture.into_raw()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_backbuffer(chain: *mut c_void) -> *mut c_void {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.ok())
        .map_or(ptr::null_mut(), Interface::into_raw)
}
