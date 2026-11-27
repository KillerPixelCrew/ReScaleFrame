// SPDX-License-Identifier: MIT
use super::json::Json;
use super::platform;
use std::ffi::{CStr, c_char};
use windows::Win32::Graphics::{
    Direct3D11::{D3D11_RESOURCE_DIMENSION_TEXTURE2D, D3D11_TEXTURE2D_DESC},
    Direct3D12::D3D12_RESOURCE_DESC,
};

const _: () = {
    assert!(size_of::<D3D12_RESOURCE_DESC>() == 56 && align_of::<D3D12_RESOURCE_DESC>() == 8);
    assert!(size_of::<D3D11_TEXTURE2D_DESC>() == 44 && align_of::<D3D11_TEXTURE2D_DESC>() == 4);
};

pub(super) fn rsf_census_dxgi_format_name(format: u32) -> *const c_char {
    match format {
        0 => c"UNKNOWN",
        1 => c"R32G32B32A32_TYPELESS",
        2 => c"R32G32B32A32_FLOAT",
        3 => c"R32G32B32A32_UINT",
        6 => c"R32G32B32_FLOAT",
        9 => c"R16G16B16A16_TYPELESS",
        10 => c"R16G16B16A16_FLOAT",
        11 => c"R16G16B16A16_UNORM",
        13 => c"R16G16B16A16_SNORM",
        15 => c"R32G32_TYPELESS",
        16 => c"R32G32_FLOAT",
        19 => c"R32G8X24_TYPELESS",
        20 => c"D32_FLOAT_S8X24_UINT",
        21 => c"R32_FLOAT_X8X24_TYPELESS",
        23 => c"R10G10B10A2_TYPELESS",
        24 => c"R10G10B10A2_UNORM",
        26 => c"R11G11B10_FLOAT",
        27 => c"R8G8B8A8_TYPELESS",
        28 => c"R8G8B8A8_UNORM",
        29 => c"R8G8B8A8_UNORM_SRGB",
        31 => c"R8G8B8A8_SNORM",
        33 => c"R16G16_TYPELESS",
        34 => c"R16G16_FLOAT",
        35 => c"R16G16_UNORM",
        37 => c"R16G16_SNORM",
        39 => c"R32_TYPELESS",
        40 => c"D32_FLOAT",
        41 => c"R32_FLOAT",
        42 => c"R32_UINT",
        44 => c"R24G8_TYPELESS",
        45 => c"D24_UNORM_S8_UINT",
        46 => c"R24_UNORM_X8_TYPELESS",
        48 => c"R8G8_TYPELESS",
        49 => c"R8G8_UNORM",
        53 => c"R16_TYPELESS",
        54 => c"R16_FLOAT",
        55 => c"D16_UNORM",
        56 => c"R16_UNORM",
        60 => c"R8_TYPELESS",
        61 => c"R8_UNORM",
        67 => c"R9G9B9E5_SHAREDEXP",
        87 => c"B8G8R8A8_UNORM",
        88 => c"B8G8R8X8_UNORM",
        90 => c"B8G8R8A8_TYPELESS",
        91 => c"B8G8R8A8_UNORM_SRGB",
        _ => c"other",
    }
    .as_ptr()
}

impl Json<'_> {
    /// A D3D resource's description as a JSON object, read through the platform's guarded calls.
    pub(super) fn d3d_resource(&mut self, key: &CStr, resource: u64, d3d12: bool) {
        if resource == 0 {
            self.raw(key, c"null");
            return;
        }
        self.open(key, b'{');
        self.hex(c"ptr", resource);
        let readable = if d3d12 {
            self.describe12(resource)
        } else {
            self.describe11(resource)
        };
        if !readable {
            self.flag(c"desc_unreadable", true);
        }
        self.close(b'}');
    }

    fn texture_fields(&mut self, format: u32, width: u64, height: u32) {
        self.number(c"format", format);
        unsafe { self.text_ptr(c"format_name", rsf_census_dxgi_format_name(format)) };
        self.number(c"width", width);
        self.number(c"height", height);
    }

    fn describe12(&mut self, resource: u64) -> bool {
        let mut desc = D3D12_RESOURCE_DESC::default();
        if !platform()
            .resource12_desc
            .is_some_and(|read| unsafe { read(resource as _, (&raw mut desc).cast()) })
        {
            return false;
        }
        self.number(c"dimension", desc.Dimension.0 as u32);
        self.texture_fields(desc.Format.0 as u32, desc.Width, desc.Height);
        self.number(c"depth_or_layers", desc.DepthOrArraySize);
        self.number(c"mips", desc.MipLevels);
        self.number(c"samples", desc.SampleDesc.Count);
        self.hex(c"flags", u64::from(desc.Flags.0 as u32));
        true
    }

    fn describe11(&mut self, resource: u64) -> bool {
        let mut dimension = 0;
        if !platform()
            .resource11_dimension
            .is_some_and(|read| unsafe { read(resource as _, &mut dimension) })
        {
            return false;
        }
        self.number(c"dimension", dimension as u32);
        if dimension != D3D11_RESOURCE_DIMENSION_TEXTURE2D.0 {
            return true;
        }
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        if !platform()
            .resource11_desc
            .is_some_and(|read| unsafe { read(resource as _, (&raw mut desc).cast()) })
        {
            return false;
        }
        self.texture_fields(desc.Format.0 as u32, desc.Width.into(), desc.Height);
        self.number(c"mips", desc.MipLevels);
        self.number(c"array_size", desc.ArraySize);
        self.hex(c"bind_flags", desc.BindFlags.into());
        true
    }
}
