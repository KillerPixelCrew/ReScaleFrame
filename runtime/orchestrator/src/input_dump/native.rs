// SPDX-License-Identifier: MIT
use super::{Clock, Encoding, Pixels, Surface, TextureOptions};
use crate::texture_dump;
use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11Predicate,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::{BOOL, IUnknown, Interface};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetTickCount64};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F10};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub(super) fn clock() -> Clock {
    let mut foreground = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground);
        let process = GetCurrentProcessId();
        Clock {
            tick: GetTickCount64(),
            process,
            key_down: foreground == process && GetAsyncKeyState(i32::from(VK_F10)) < 0,
        }
    }
}

pub(super) fn local_time() -> SYSTEMTIME {
    let mut now = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut now) };
    now
}

// The texture behind `source` when `context` is an immediate context.
pub(super) unsafe fn open_texture(
    context: *mut c_void,
    source: *mut c_void,
) -> Option<(ID3D11Texture2D, Surface)> {
    let (context, source) = unsafe {
        (
            ID3D11DeviceContext::from_raw_borrowed(&context)?,
            IUnknown::from_raw_borrowed(&source)?,
        )
    };
    if unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return None;
    }
    let texture = source.cast::<ID3D11Texture2D>().ok()?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    let surface = Surface {
        width: desc.Width,
        height: desc.Height,
        format: desc.Format.0 as u32,
    };
    Some((texture, surface))
}

pub(super) unsafe fn backbuffer(chain: *mut c_void) -> *mut c_void {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.ok())
        .map_or(ptr::null_mut(), Interface::into_raw)
}

type Handle = *mut c_void;

unsafe extern "C" {
    // runtime/common, linked as the RuntimeCommon archive.
    fn rsf_common_sha256_file(path: *const u16, sharing: u32, output: *mut u8) -> u32;
}

pub(super) fn executable_hash(path: &[u16]) -> String {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let mut digest = [0; 32];
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    if unsafe { rsf_common_sha256_file(path.as_ptr(), sharing, digest.as_mut_ptr()) } == 0 {
        return String::new();
    }
    use std::fmt::Write as _;
    let mut hash = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

struct Predicate<'a> {
    context: &'a ID3D11DeviceContext,
    saved: Option<ID3D11Predicate>,
    value: BOOL,
}

impl Drop for Predicate<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.context.vtable().SetPredication)(
                self.context.as_raw(),
                self.saved
                    .as_ref()
                    .map_or(ptr::null_mut(), Interface::as_raw),
                self.value,
            );
        }
    }
}

pub(super) unsafe fn encode_texture(
    context: Handle,
    texture: &ID3D11Texture2D,
    options: &TextureOptions,
    include_raw: bool,
) -> Encoding {
    let mut result = Encoding::INVALID;
    let Some(immediate) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return result;
    };
    let Ok(device) = (unsafe { immediate.GetDevice() }) else {
        return result;
    };
    let source = texture.as_raw();
    let mut predicate = Predicate {
        context: immediate,
        saved: None,
        value: BOOL(0),
    };
    // Both copies honor predication. Restore the exact native BOOL on exit.
    unsafe {
        immediate.GetPredication(Some(&mut predicate.saved), Some(&mut predicate.value));
        immediate.SetPredication(None::<&ID3D11Predicate>, false);
        if include_raw {
            result.raw =
                texture_dump::rsf_dump_texture_bytes(device.as_raw(), context, source, options);
        }
        result.preview = texture_dump::rsf_dump_texture_png(
            device.as_raw(),
            context,
            source,
            options,
            ptr::null_mut(),
        );
    }
    result
}

pub(super) unsafe fn preview_pixels(pixels: *const Pixels, options: *const TextureOptions) -> i32 {
    unsafe { texture_dump::rsf_dump_pixels_png(pixels, options, ptr::null_mut()) }
}
