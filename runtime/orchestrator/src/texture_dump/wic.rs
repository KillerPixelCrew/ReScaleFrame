// SPDX-License-Identifier: MIT
use std::ffi::CStr;
use std::ptr;
use windows::Win32::Foundation::{GENERIC_WRITE, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::IPropertyBag2;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::{IUnknown, PCWSTR};

struct Apartment(bool);

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub(super) fn write_png(path: &CStr, width: u32, height: u32, bgra: &[u8]) -> Option<()> {
    let path: Vec<u16> = path.to_str().ok()?.encode_utf16().chain([0]).collect();
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
        return None;
    }
    let _apartment = Apartment(initialized.is_ok());
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(
            &CLSID_WICImagingFactory,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
        .ok()?;
        let stream = factory.CreateStream().ok()?;
        stream
            .InitializeFromFilename(PCWSTR(path.as_ptr()), GENERIC_WRITE.0)
            .ok()?;
        let encoder = factory
            .CreateEncoder(&GUID_ContainerFormatPng, ptr::null())
            .ok()?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, ptr::null_mut()).ok()?;
        let frame = frame?;
        frame.Initialize(None::<&IPropertyBag2>).ok()?;
        frame.SetSize(width, height).ok()?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format).ok()?;
        if format != GUID_WICPixelFormat32bppBGRA {
            return None;
        }
        frame.WritePixels(height, width * 4, bgra).ok()?;
        frame.Commit().ok()?;
        encoder.Commit().ok()?;
    }
    Some(())
}
