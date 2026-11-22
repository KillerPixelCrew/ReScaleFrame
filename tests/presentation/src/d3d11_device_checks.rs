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

unsafe fn borrowed<T: Interface>(value: &Raw) -> &T {
    unsafe { T::from_raw_borrowed(value) }.unwrap()
}

fn texture(device: &ID3D11Device, desc: &D3D11_TEXTURE2D_DESC) -> Option<ID3D11Texture2D> {
    let mut texture = None;
    unsafe { device.CreateTexture2D(desc, None, Some(&mut texture)) }.ok()?;
    texture
}

fn create_inputs(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_inputs(device: Raw, context: Raw, output: *mut Raw) -> i32 {
    let inputs = unsafe {
        create_inputs(
            borrowed::<ID3D11Device>(&device),
            borrowed::<ID3D11DeviceContext>(&context),
        )
    };
    let Some(inputs) = inputs else {
        return 1;
    };
    for (index, input) in inputs.into_iter().enumerate() {
        unsafe { output.add(index).write(input.into_raw()) };
    }
    0
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

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11_fixture_red_half(
    device: Raw,
    context: Raw,
    source: Raw,
) -> u16 {
    let mut red = 0;
    let _ = unsafe {
        stage(
            borrowed::<ID3D11Device>(&device),
            borrowed::<ID3D11DeviceContext>(&context),
            borrowed::<ID3D11Texture2D>(&source),
            |data| red = data.cast::<u16>().read_unaligned(),
        )
    };
    red
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_flip_source(
    device: Raw,
    context: Raw,
    target: Raw,
    frame: u64,
) {
    let target = unsafe { borrowed::<ID3D11RenderTargetView>(&target) };
    let Ok(source) =
        (unsafe { target.GetResource() }).and_then(|res| res.cast::<ID3D11Texture2D>())
    else {
        return;
    };
    let _ = unsafe {
        stage(
            borrowed::<ID3D11Device>(&device),
            borrowed::<ID3D11DeviceContext>(&context),
            &source,
            |data| {
                let [r, g, b, a] = data.cast::<[u8; 4]>().read();
                println!("source RGBA={r},{g},{b},{a} callback={frame}");
            },
        )
    };
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_clear_and_present(
    device: Raw,
    context: Raw,
    chain: Raw,
    held_resize: u32,
) -> i32 {
    let (device, context, chain) = unsafe {
        (
            borrowed::<ID3D11Device>(&device),
            borrowed::<ID3D11DeviceContext>(&context),
            borrowed::<IDXGISwapChain>(&chain),
        )
    };
    if held_resize == 0 {
        let Some(_held) = draw_backbuffer(device, context, chain, &[0.2, 0.3, 0.4, 1.0]) else {
            return 1;
        };
        return i32::from(unsafe { chain.Present(0, DXGI_PRESENT(0)) }.is_err());
    }
    for frame in 0..12 {
        let Some(_held) = draw_backbuffer(
            device,
            context,
            chain,
            &[frame as f32 / 12.0, 0.2, 0.6, 1.0],
        ) else {
            return 1;
        };
        if unsafe { chain.Present(u32::from(frame == 0), DXGI_PRESENT(0)) }.is_err() {
            return 1;
        }
        match unsafe {
            chain.ResizeBuffers(1, 1024, 576, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
        } {
            Err(error) if error.code() == DXGI_ERROR_INVALID_CALL => {}
            _ => return 1,
        }
    }
    0
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_borderless(chain: Raw, window: Raw) -> i32 {
    let chain = unsafe { borrowed::<IDXGISwapChain>(&chain) };
    let window = HWND(window);
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
    unsafe {
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
