// SPDX-License-Identifier: MIT
//! Real hardware and WARP device creation for the SR fixtures.
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;

pub(crate) struct Device {
    pub(crate) device: ID3D11Device,
    pub(crate) context: ID3D11DeviceContext,
    pub(crate) info: Option<ID3D11InfoQueue>,
    pub(crate) level: D3D_FEATURE_LEVEL,
}

impl Device {
    fn create(
        adapter: Option<&IDXGIAdapter>,
        driver: D3D_DRIVER_TYPE,
        flags: D3D11_CREATE_DEVICE_FLAG,
        levels: Option<&[D3D_FEATURE_LEVEL]>,
    ) -> Option<Self> {
        let (device, context) =
            rsf_test_support::d3d11_device_on(adapter, driver, flags, levels).ok()?;
        let level = unsafe { device.GetFeatureLevel() };
        let info = if flags == D3D11_CREATE_DEVICE_DEBUG {
            device.cast().ok()
        } else {
            None
        };
        Some(Self {
            device,
            context,
            level,
            info,
        })
    }

    pub(crate) fn hardware_or_warp(roundtrip: bool) -> Option<Self> {
        let levels = [D3D_FEATURE_LEVEL_11_0];
        let levels = roundtrip.then_some(levels.as_slice());
        let mut attempts = vec![
            (
                D3D_DRIVER_TYPE_HARDWARE,
                D3D11_CREATE_DEVICE_DEBUG,
                "hardware",
            ),
            (
                D3D_DRIVER_TYPE_HARDWARE,
                D3D11_CREATE_DEVICE_FLAG(0),
                "hardware",
            ),
        ];
        if roundtrip {
            attempts.push((D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_DEBUG, "WARP"));
        }
        attempts.push((D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_FLAG(0), "WARP"));
        for (driver, flags, kind) in attempts {
            if let Some(device) = Self::create(None, driver, flags, levels) {
                let name = device
                    .device
                    .cast::<IDXGIDevice>()
                    .and_then(|dxgi| unsafe { dxgi.GetAdapter() })
                    .and_then(|adapter| unsafe { adapter.GetDesc() })
                    .map(|desc| wide_name(&desc.Description))
                    .unwrap_or_default();
                let debug = if flags == D3D11_CREATE_DEVICE_DEBUG {
                    "on"
                } else {
                    "off"
                };
                println!("device: {name} ({kind}, debug layer {debug})");
                return Some(device);
            }
        }
        None
    }

    pub(crate) fn hardware(vendor: u32, debug: bool) -> Option<Self> {
        let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }.ok()?;
        let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        for index in 0.. {
            let adapter = unsafe { factory.EnumAdapters1(index) }.ok()?;
            let desc = unsafe { adapter.GetDesc1() }.ok()?;
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
                || (vendor != 0 && desc.VendorId != vendor)
            {
                continue;
            }
            let adapter = adapter.cast().ok()?;
            let plain = D3D11_CREATE_DEVICE_FLAG(0);
            let preferred = if debug {
                D3D11_CREATE_DEVICE_DEBUG
            } else {
                plain
            };
            for flags in [preferred, plain] {
                if let Some(device) = Self::create(
                    Some(&adapter),
                    D3D_DRIVER_TYPE_UNKNOWN,
                    flags,
                    Some(&levels),
                ) {
                    let name = wide_name(&desc.Description);
                    let debug = if flags == D3D11_CREATE_DEVICE_DEBUG {
                        "on"
                    } else {
                        "off"
                    };
                    println!(
                        "device: {name} (vendor 0x{:04x}, debug layer {debug})",
                        desc.VendorId
                    );
                    return Some(device);
                }
            }
        }
        None
    }

    pub(crate) fn texture_desc(
        &self,
        [width, height]: [u32; 2],
        format: DXGI_FORMAT,
        bindings: u32,
        samples: u32,
        initial: Option<&D3D11_SUBRESOURCE_DATA>,
    ) -> Option<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: samples,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bindings,
            ..Default::default()
        };
        let mut texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, initial.map(std::ptr::from_ref), Some(&mut texture))
        }
        .ok()?;
        texture
    }

    pub(crate) fn texture(
        &self,
        size: [u32; 2],
        format: DXGI_FORMAT,
        bindings: u32,
    ) -> Option<ID3D11Texture2D> {
        self.texture_desc(size, format, bindings, 1, None)
    }

    pub(crate) fn data_texture<T>(
        &self,
        size: [u32; 2],
        format: DXGI_FORMAT,
        bindings: u32,
        data: &[T],
        pitch: u32,
    ) -> Option<ID3D11Texture2D> {
        if size_of_val(data) < pitch as usize * size[1] as usize {
            return None;
        }
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr().cast(),
            SysMemPitch: pitch,
            SysMemSlicePitch: 0,
        };
        self.texture_desc(size, format, bindings, 1, Some(&initial))
    }

    pub(crate) fn rtv(
        &self,
        texture: &ID3D11Texture2D,
        desc: Option<&D3D11_RENDER_TARGET_VIEW_DESC>,
    ) -> Option<ID3D11RenderTargetView> {
        let mut view = None;
        unsafe {
            self.device.CreateRenderTargetView(
                texture,
                desc.map(std::ptr::from_ref),
                Some(&mut view),
            )
        }
        .ok()?;
        view
    }

    pub(crate) fn srv(
        &self,
        texture: &ID3D11Texture2D,
        desc: Option<&D3D11_SHADER_RESOURCE_VIEW_DESC>,
    ) -> Option<ID3D11ShaderResourceView> {
        let mut view = None;
        unsafe {
            self.device.CreateShaderResourceView(
                texture,
                desc.map(std::ptr::from_ref),
                Some(&mut view),
            )
        }
        .ok()?;
        view
    }

    pub(crate) fn dsv(
        &self,
        texture: &ID3D11Texture2D,
        desc: Option<&D3D11_DEPTH_STENCIL_VIEW_DESC>,
    ) -> Option<ID3D11DepthStencilView> {
        let mut view = None;
        unsafe {
            self.device.CreateDepthStencilView(
                texture,
                desc.map(std::ptr::from_ref),
                Some(&mut view),
            )
        }
        .ok()?;
        view
    }

    pub(crate) fn debug_errors(&self) -> u64 {
        let Some(info) = &self.info else { return 0 };
        let mut errors = 0;
        for index in 0..unsafe { info.GetNumStoredMessages() } {
            let mut size = 0;
            if unsafe { info.GetMessage(index, None, &mut size) }.is_err()
                || size < size_of::<D3D11_MESSAGE>()
            {
                errors += 1;
                continue;
            }
            let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
            let raw = storage.as_mut_ptr().cast::<D3D11_MESSAGE>();
            if unsafe { info.GetMessage(index, Some(raw), &mut size) }.is_err() {
                errors += 1;
                continue;
            }
            let message = unsafe { &*raw };
            if message.Severity.0 <= D3D11_MESSAGE_SEVERITY_ERROR.0 {
                let text = if message.pDescription.is_null() {
                    &[]
                } else {
                    unsafe {
                        std::slice::from_raw_parts(
                            message.pDescription,
                            message.DescriptionByteLength,
                        )
                    }
                };
                println!(
                    "  debug layer: {}",
                    String::from_utf8_lossy(text).trim_end_matches('\0')
                );
                errors += 1;
            }
        }
        unsafe { info.ClearStoredMessages() };
        errors
    }
}

fn wide_name(description: &[u16]) -> String {
    String::from_utf16_lossy(description)
        .trim_end_matches('\0')
        .to_owned()
}
