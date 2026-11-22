// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use std::{ptr, slice};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

#[path = "colour_transport.rs"]
mod colour_transport;
#[path = "motion_resolve.rs"]
mod motion_resolve;

type Handle = *mut c_void;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Pass {
    raw: Handle,
    destroy: unsafe extern "C" fn(Handle),
}

impl Drop for Pass {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.raw) };
    }
}

struct Warp {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

impl Warp {
    fn new() -> Option<Self> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                Default::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .ok()?;
        Some(Self {
            device: device?,
            context: context?,
        })
    }

    fn desc(size: [u32; 2], format: DXGI_FORMAT, bind: D3D11_BIND_FLAG) -> D3D11_TEXTURE2D_DESC {
        D3D11_TEXTURE2D_DESC {
            Width: size[0],
            Height: size[1],
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BindFlags: bind.0 as u32,
            ..Default::default()
        }
    }

    fn texture(
        &self,
        desc: &D3D11_TEXTURE2D_DESC,
        data: Option<&D3D11_SUBRESOURCE_DATA>,
    ) -> TestResult<ID3D11Texture2D> {
        let mut texture = None;
        unsafe {
            self.device
                .CreateTexture2D(desc, data.map(ptr::from_ref), Some(&mut texture))
        }?;
        texture.ok_or_else(|| "CreateTexture2D returned a null texture".into())
    }

    fn initialized<T>(
        &self,
        desc: &D3D11_TEXTURE2D_DESC,
        pixels: &[T],
        pitch: u32,
    ) -> TestResult<ID3D11Texture2D> {
        if desc.MipLevels != 1
            || desc.ArraySize != 1
            || size_of_val(pixels) < pitch as usize * desc.Height as usize
        {
            return Err("Texture fixture data does not cover the declared surface".into());
        }
        self.texture(
            desc,
            Some(&D3D11_SUBRESOURCE_DATA {
                pSysMem: pixels.as_ptr().cast(),
                SysMemPitch: pitch,
                SysMemSlicePitch: 0,
            }),
        )
    }

    // T must accept all bit patterns and match the texel component layout.
    unsafe fn read<T: Copy>(
        &self,
        texture: &ID3D11Texture2D,
        desc: &D3D11_TEXTURE2D_DESC,
        components: usize,
    ) -> TestResult<Vec<T>> {
        let mut staging_desc = *desc;
        staging_desc.BindFlags = 0;
        staging_desc.MiscFlags = 0;
        staging_desc.Usage = D3D11_USAGE_STAGING;
        staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let staging = self.texture(&staging_desc, None)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let row_samples = desc.Width as usize * components;
        let mut pixels = Vec::with_capacity(row_samples * desc.Height as usize);
        unsafe {
            self.context.CopyResource(&staging, texture);
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            if mapped.pData.is_null() || mapped.RowPitch as usize / size_of::<T>() < row_samples {
                self.context.Unmap(&staging, 0);
                return Err("Invalid mapped texture data".into());
            }
            for row in 0..desc.Height as usize {
                let data = mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize);
                pixels.extend_from_slice(slice::from_raw_parts(data.cast::<T>(), row_samples));
            }
            self.context.Unmap(&staging, 0);
        }
        Ok(pixels)
    }
}

fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let fraction = u32::from(bits & 0x3ff);
    let magnitude = match exponent {
        0 if fraction == 0 => 0,
        0 => {
            let shift = fraction.leading_zeros() - 21;
            ((113 - shift) << 23) | (((fraction << shift) & 0x3ff) << 13)
        }
        31 => 0x7f80_0000 | (fraction << 13),
        _ => ((exponent + 112) << 23) | (fraction << 13),
    };
    f32::from_bits(sign | magnitude)
}

fn run_warp(name: &str, run: impl FnOnce(&Warp) -> TestResult) -> i32 {
    let Some(warp) = Warp::new() else {
        return 77;
    };
    match run(&warp) {
        Ok(()) => {
            println!("{name} passed");
            0
        }
        Err(error) => {
            eprintln!("{name} FAILED: {error}");
            1
        }
    }
}
