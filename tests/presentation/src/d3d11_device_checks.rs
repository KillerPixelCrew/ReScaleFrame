// SPDX-License-Identifier: MIT
//! Draw and readback checks for the linked D3D11 presentation fixture.
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT,
    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_INVALID_CALL, DXGI_PRESENT, DXGI_SWAP_CHAIN_FLAG, IDXGISwapChain,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::Threading::Sleep;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GWL_STYLE, GetWindowRect, HWND_TOP, MSG, PM_REMOVE, PeekMessageW,
    SWP_FRAMECHANGED, SetWindowLongPtrW, SetWindowPos, TranslateMessage, WS_POPUP, WS_VISIBLE,
};
use windows::core::Interface;

type Raw = *mut c_void;

fn texture(device: &ID3D11Device, desc: &D3D11_TEXTURE2D_DESC) -> Option<ID3D11Texture2D> {
    let mut texture = None;
    unsafe { device.CreateTexture2D(desc, None, Some(&mut texture)) }.ok()?;
    texture
}

/// Four SR textures followed by five hint textures.
pub(super) fn create_inputs(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
) -> Option<Vec<ID3D11Texture2D>> {
    let mut inputs = Vec::with_capacity(9);
    for (index, format) in [
        DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R32_FLOAT,
        DXGI_FORMAT_R16G16_FLOAT,
        DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R32_FLOAT,
        DXGI_FORMAT_R32_FLOAT,
        DXGI_FORMAT_R32_FLOAT,
    ]
    .into_iter()
    .enumerate()
    {
        let bind = if index < 4 {
            D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0
        } else {
            D3D11_BIND_SHADER_RESOURCE.0
        };
        let input = texture(
            device,
            &D3D11_TEXTURE2D_DESC {
                Width: 512,
                Height: 512,
                MipLevels: 1,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BindFlags: bind as u32,
                ..Default::default()
            },
        )?;
        if index < 4 {
            let mut view: Option<ID3D11UnorderedAccessView> = None;
            unsafe { device.CreateUnorderedAccessView(&input, None, Some(&mut view)) }.ok()?;
            let red = match index {
                0 => 0.25,
                1 => 0.5,
                _ => 0.0,
            };
            unsafe { context.ClearUnorderedAccessViewFloat(&view?, &[red, 0.0, 0.0, 1.0]) };
        }
        inputs.push(input);
    }
    Some(inputs)
}

fn stage(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
    copy_bytes: impl FnOnce(Raw),
) -> Option<()> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };
    desc.BindFlags = 0;
    desc.MiscFlags = 0;
    desc.Usage = D3D11_USAGE_STAGING;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let staging = texture(device, &desc)?;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context.CopyResource(&staging, source);
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .ok()?;
    }
    if !mapped.pData.is_null() {
        copy_bytes(mapped.pData);
    }
    unsafe { context.Unmap(&staging, 0) };
    (!mapped.pData.is_null()).then_some(())
}

/// The red channel of the first texel of an R16G16B16A16_FLOAT texture, or zero when it cannot be read.
pub(super) fn red_half(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
) -> u16 {
    let mut red = 0;
    let _ = stage(device, context, source, |data| {
        red = unsafe { data.cast::<u16>().read_unaligned() }
    });
    red
}

/// Prints the first texel of the resource behind `target`.
pub(super) fn flip_source(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    target: &ID3D11RenderTargetView,
    frame: u64,
) {
    let Ok(source) =
        (unsafe { target.GetResource() }).and_then(|res| res.cast::<ID3D11Texture2D>())
    else {
        return;
    };
    let _ = stage(device, context, &source, |data| {
        let [r, g, b, a] = unsafe { data.cast::<[u8; 4]>().read() };
        println!("source RGBA={r},{g},{b},{a} callback={frame}");
    });
}

fn draw_backbuffer(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    chain: &IDXGISwapChain,
    color: &[f32; 4],
) -> Option<(ID3D11Texture2D, ID3D11RenderTargetView)> {
    let buffer = unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.ok()?;
    let mut target = None;
    unsafe { device.CreateRenderTargetView(&buffer, None, Some(&mut target)) }.ok()?;
    let target = target?;
    unsafe { context.ClearRenderTargetView(&target, color) };
    Some((buffer, target))
}

/// Draws and presents the back buffer once, or twelve times while holding it across a resize the
/// chain must refuse. True when every call behaved.
pub(super) fn clear_and_present(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    chain: &IDXGISwapChain,
    held_resize: bool,
) -> bool {
    if !held_resize {
        let Some(_held) = draw_backbuffer(device, context, chain, &[0.2, 0.3, 0.4, 1.0]) else {
            return false;
        };
        return unsafe { chain.Present(0, DXGI_PRESENT(0)) }.is_ok();
    }
    for frame in 0..12 {
        let Some(_held) = draw_backbuffer(
            device,
            context,
            chain,
            &[frame as f32 / 12.0, 0.2, 0.6, 1.0],
        ) else {
            return false;
        };
        if unsafe { chain.Present(u32::from(frame == 0), DXGI_PRESENT(0)) }.is_err() {
            return false;
        }
        match unsafe {
            chain.ResizeBuffers(1, 1024, 576, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
        } {
            Err(error) if error.code() == DXGI_ERROR_INVALID_CALL => {}
            _ => return false,
        }
    }
    true
}

fn pump() {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn window_rect(hwnd: HWND, when: &str) -> RECT {
    let mut rect = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    println!(
        "borderless: {when:<18} rect {},{} {}x{}",
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top
    );
    rect
}

pub(super) fn borderless(chain: &IDXGISwapChain, window: HWND) -> i32 {
    let Ok(buffer) = (unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }) else {
        return 1;
    };
    drop(buffer); // Drop the temporary reference before the actual resize.
    pump();
    window_rect(window, "after chain");
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // The window is only sized from the result; a failed query leaves an empty rectangle.
    let _ = unsafe {
        GetMonitorInfoW(
            MonitorFromWindow(window, MONITOR_DEFAULTTOPRIMARY),
            &mut monitor,
        )
    };
    let extent = monitor.rcMonitor;
    unsafe {
        SetWindowLongPtrW(window, GWL_STYLE, (WS_VISIBLE.0 | WS_POPUP.0) as isize);
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            0,
            0,
            extent.right - extent.left,
            extent.bottom - extent.top,
            SWP_FRAMECHANGED,
        );
    }
    pump();
    window_rect(window, "after borderless");
    if unsafe { chain.ResizeBuffers(0, 0, 0, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)) }
        .is_err()
        || unsafe { chain.Present(0, DXGI_PRESENT(0)) }.is_err()
    {
        return 1;
    }
    unsafe { Sleep(300) };
    pump();
    let final_rect = window_rect(window, "after present");
    let origin = final_rect.left == 0
        && final_rect.top == 0
        && final_rect.right - final_rect.left == extent.right - extent.left;
    if origin {
        println!("PASS: borderless window at the monitor origin with the bridge");
    } else {
        println!("FAIL: borderless window is not at the monitor origin with the bridge");
    }
    i32::from(!origin)
}
