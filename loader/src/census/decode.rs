// SPDX-License-Identifier: MIT
use super::decode_abi::{rsf_census_read_resource, rsf_census_resource_desc};
use super::json::{
    rsf_census_json_close, rsf_census_json_hex, rsf_census_json_open, rsf_census_json_raw,
    rsf_census_json_string, rsf_census_json_u64,
};
use super::json_abi::rsf_census_json;
use std::ffi::{c_char, c_void};

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_census_dxgi_format_name(format: u32) -> *const c_char {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_census_json_resource(
    json: &mut rsf_census_json,
    key: *const c_char,
    resource: *const c_void,
    d3d12: bool,
    read: rsf_census_read_resource,
) {
    if resource.is_null() {
        unsafe { rsf_census_json_raw(json, key, c"null".as_ptr()) };
        return;
    }

    let mut desc = rsf_census_resource_desc::default();
    // The callback returns copied facts and contains exceptions from any native COM calls.
    let readable = read.is_some_and(|read| unsafe { read(resource, &mut desc) });
    unsafe {
        rsf_census_json_open(json, key, b'{' as c_char);
        rsf_census_json_hex(json, c"ptr".as_ptr(), resource as u64);
        if desc.dimension_read {
            rsf_census_json_u64(json, c"dimension".as_ptr(), desc.dimension.into());
        }
        if desc.texture_read {
            rsf_census_json_u64(json, c"format".as_ptr(), desc.format.into());
            rsf_census_json_string(
                json,
                c"format_name".as_ptr(),
                rsf_census_dxgi_format_name(desc.format),
            );
            rsf_census_json_u64(json, c"width".as_ptr(), desc.width);
            rsf_census_json_u64(json, c"height".as_ptr(), desc.height.into());
            if d3d12 {
                rsf_census_json_u64(json, c"depth_or_layers".as_ptr(), desc.layers.into());
                rsf_census_json_u64(json, c"mips".as_ptr(), desc.mips.into());
                rsf_census_json_u64(json, c"samples".as_ptr(), desc.samples.into());
                rsf_census_json_hex(json, c"flags".as_ptr(), desc.flags.into());
            } else {
                rsf_census_json_u64(json, c"mips".as_ptr(), desc.mips.into());
                rsf_census_json_u64(json, c"array_size".as_ptr(), desc.layers.into());
                rsf_census_json_hex(json, c"bind_flags".as_ptr(), desc.flags.into());
            }
        }
        if !readable {
            rsf_census_json_raw(json, c"desc_unreadable".as_ptr(), c"true".as_ptr());
        }
        rsf_census_json_close(json, b'}' as c_char);
    }
}
