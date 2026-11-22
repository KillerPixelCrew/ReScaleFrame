// SPDX-License-Identifier: MIT
use super::{BUFFER, Handle, LAYOUT, STAGE_BASE, TEXTURE, original};
use crate::d3d11_observer_abi::{
    RSF_OBSERVER_MAX_LAYOUT_ELEMENTS, RSF_OBSERVER_STAGE_COMPUTE, RSF_OBSERVER_STAGE_DOMAIN,
    RSF_OBSERVER_STAGE_GEOMETRY, RSF_OBSERVER_STAGE_HULL, RSF_OBSERVER_STAGE_PIXEL,
    RSF_OBSERVER_STAGE_VERTEX, rsf_observer_layout_element as Layout,
};
use crate::d3d11_observer_callbacks::{self as callbacks, TextureFacts};
use crate::d3d11_observer_present::{present, rsf_observer11_present_platform};
use crate::scene_abi::rsf_observer_present_event as Event;
use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BUFFER_DESC, D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_INSTANCE_DATA,
    D3D11_SO_DECLARATION_ENTRY, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, ID3D11Device_Vtbl,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, IDXGISwapChain_Vtbl, IDXGISwapChain1_Vtbl,
};
use windows::core::HRESULT;

const FAIL: HRESULT = HRESULT(0x8000_4005u32 as i32);

macro_rules! original_method {
    ($name:ident, $method:ident, ($($arg:ty),*) -> $result:ty) => {
        // Like the previous native detours, these forward exceptions from the original.
        type $name = unsafe extern "system-unwind" fn($($arg),*) -> $result;
        const _: fn(&ID3D11Device_Vtbl) -> unsafe extern "system" fn($($arg),*) -> $result =
            |table| table.$method;
    };
}

original_method!(Texture, CreateTexture2D,
    (Handle, *const D3D11_TEXTURE2D_DESC, *const D3D11_SUBRESOURCE_DATA, *mut Handle) -> HRESULT);
original_method!(Buffer, CreateBuffer,
    (Handle, *const D3D11_BUFFER_DESC, *const D3D11_SUBRESOURCE_DATA, *mut Handle) -> HRESULT);
original_method!(InputLayout, CreateInputLayout,
    (Handle, *const D3D11_INPUT_ELEMENT_DESC, u32, *const c_void, usize, *mut Handle) -> HRESULT);
original_method!(GeometrySo, CreateGeometryShaderWithStreamOutput,
    (Handle, *const c_void, usize, *const D3D11_SO_DECLARATION_ENTRY, u32, *const u32, u32, u32, Handle, *mut Handle) -> HRESULT);
type Shader =
    unsafe extern "system-unwind" fn(Handle, *const c_void, usize, Handle, *mut Handle) -> HRESULT;
type SdkShader =
    unsafe extern "system" fn(Handle, *const c_void, usize, Handle, *mut Handle) -> HRESULT;

const _: fn(
    &IDXGISwapChain_Vtbl,
) -> unsafe extern "system" fn(Handle, u32, DXGI_PRESENT) -> HRESULT = |table| table.Present;
const _: fn(
    &IDXGISwapChain1_Vtbl,
) -> unsafe extern "system" fn(
    Handle,
    u32,
    DXGI_PRESENT,
    *const DXGI_PRESENT_PARAMETERS,
) -> HRESULT = |table| table.Present1;

unsafe fn output(result: HRESULT, out: *mut Handle) -> Option<Handle> {
    if result.is_err() {
        return None;
    }
    unsafe { out.as_ref() }
        .copied()
        .filter(|value| !value.is_null())
}

pub(super) unsafe extern "system-unwind" fn hook_create_texture2d(
    device: Handle,
    desc: *const D3D11_TEXTURE2D_DESC,
    initial: *const D3D11_SUBRESOURCE_DATA,
    out: *mut Handle,
) -> HRESULT {
    let address = original(TEXTURE);
    if address.is_null() {
        return FAIL;
    }
    let call: Texture = unsafe { std::mem::transmute(address) };
    let result = unsafe { call(device, desc, initial, out) };
    if let Some(texture) = unsafe { output(result, out) }
        && let Some(desc) = unsafe { desc.as_ref() }
    {
        let facts = TextureFacts {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
            mip_levels: desc.MipLevels,
            array_size: desc.ArraySize,
            sample_count: desc.SampleDesc.Count,
            bind_flags: desc.BindFlags,
            misc_flags: desc.MiscFlags,
        };
        unsafe { callbacks::texture_created(device, texture, &facts) };
    }
    result
}

pub(super) unsafe extern "system-unwind" fn hook_create_buffer(
    device: Handle,
    desc: *const D3D11_BUFFER_DESC,
    initial: *const D3D11_SUBRESOURCE_DATA,
    out: *mut Handle,
) -> HRESULT {
    let address = original(BUFFER);
    if address.is_null() {
        return FAIL;
    }
    let call: Buffer = unsafe { std::mem::transmute(address) };
    let result = unsafe { call(device, desc, initial, out) };
    if let Some(buffer) = unsafe { output(result, out) }
        && let Some(desc) = unsafe { desc.as_ref() }
    {
        let initial = unsafe { initial.as_ref() }.map_or(ptr::null(), |data| data.pSysMem);
        unsafe {
            callbacks::buffer_created(device, buffer, initial, desc.ByteWidth, desc.BindFlags)
        };
    }
    result
}

pub(super) unsafe extern "system-unwind" fn hook_create_input_layout(
    device: Handle,
    desc: *const D3D11_INPUT_ELEMENT_DESC,
    count: u32,
    signature: *const c_void,
    signature_bytes: usize,
    out: *mut Handle,
) -> HRESULT {
    let address = original(LAYOUT);
    if address.is_null() {
        return FAIL;
    }
    let call: InputLayout = unsafe { std::mem::transmute(address) };
    let result = unsafe { call(device, desc, count, signature, signature_bytes, out) };
    if let Some(layout) = unsafe { output(result, out) }.filter(|_| !desc.is_null()) {
        const LIMIT: usize = RSF_OBSERVER_MAX_LAYOUT_ELEMENTS as usize;
        let mut elements = [const { std::mem::MaybeUninit::<Layout>::uninit() }; LIMIT];
        let copied = count.min(RSF_OBSERVER_MAX_LAYOUT_ELEMENTS);
        let descriptors = unsafe { std::slice::from_raw_parts(desc, copied as usize) };
        for (entry, native) in elements.iter_mut().zip(descriptors) {
            entry.write(Layout {
                semantic_index: native.SemanticIndex,
                format: native.Format.0 as u32,
                input_slot: native.InputSlot,
                byte_offset: native.AlignedByteOffset,
                per_instance: u32::from(native.InputSlotClass == D3D11_INPUT_PER_INSTANCE_DATA),
            });
        }
        unsafe { callbacks::layout_created(layout, elements.as_ptr().cast(), copied, count) };
    }
    result
}

macro_rules! shader_hook {
    ($name:ident, $index:literal, $stage:ident, $method:ident) => {
        const _: fn(&ID3D11Device_Vtbl) -> SdkShader = |table| table.$method;

        pub(super) unsafe extern "system-unwind" fn $name(
            device: Handle,
            bytes: *const c_void,
            size: usize,
            linkage: Handle,
            out: *mut Handle,
        ) -> HRESULT {
            let address = original(STAGE_BASE + $index);
            if address.is_null() {
                return FAIL;
            }
            let call: Shader = unsafe { std::mem::transmute(address) };
            let result = unsafe { call(device, bytes, size, linkage, out) };
            if let Some(shader) = unsafe { output(result, out) } {
                unsafe { callbacks::shader_created(shader, $stage, bytes, size as u32) };
            }
            result
        }
    };
}

shader_hook!(
    hook_create_geometry_shader,
    0,
    RSF_OBSERVER_STAGE_GEOMETRY,
    CreateGeometryShader
);
shader_hook!(
    hook_create_hull_shader,
    1,
    RSF_OBSERVER_STAGE_HULL,
    CreateHullShader
);
shader_hook!(
    hook_create_domain_shader,
    2,
    RSF_OBSERVER_STAGE_DOMAIN,
    CreateDomainShader
);
shader_hook!(
    hook_create_compute_shader,
    3,
    RSF_OBSERVER_STAGE_COMPUTE,
    CreateComputeShader
);
shader_hook!(
    hook_create_vertex_shader,
    5,
    RSF_OBSERVER_STAGE_VERTEX,
    CreateVertexShader
);
shader_hook!(
    hook_create_pixel_shader,
    6,
    RSF_OBSERVER_STAGE_PIXEL,
    CreatePixelShader
);

pub(super) unsafe extern "system-unwind" fn hook_create_geometry_so(
    device: Handle,
    bytes: *const c_void,
    size: usize,
    entries: *const D3D11_SO_DECLARATION_ENTRY,
    entry_count: u32,
    strides: *const u32,
    stride_count: u32,
    stream: u32,
    linkage: Handle,
    out: *mut Handle,
) -> HRESULT {
    let address = original(STAGE_BASE + 4);
    if address.is_null() {
        return FAIL;
    }
    let call: GeometrySo = unsafe { std::mem::transmute(address) };
    let result = unsafe {
        call(
            device,
            bytes,
            size,
            entries,
            entry_count,
            strides,
            stride_count,
            stream,
            linkage,
            out,
        )
    };
    if let Some(shader) = unsafe { output(result, out) } {
        unsafe {
            callbacks::shader_created(shader, RSF_OBSERVER_STAGE_GEOMETRY, bytes, size as u32)
        };
    }
    result
}

unsafe fn present_event(
    chain: Handle,
    interval: u32,
    flags: DXGI_PRESENT,
    method: u32,
    parameters: *const c_void,
) -> HRESULT {
    let mut event = Event {
        struct_size: size_of::<Event>() as u32,
        completed: 0,
        swapchain: chain,
        sync_interval: interval,
        flags: flags.0,
        result: 0,
        method,
    };
    HRESULT(unsafe { present(&mut event, parameters, &rsf_observer11_present_platform) })
}

pub(super) unsafe extern "system" fn hook_present(
    chain: Handle,
    interval: u32,
    flags: DXGI_PRESENT,
) -> HRESULT {
    unsafe { present_event(chain, interval, flags, 0, ptr::null()) }
}

pub(super) unsafe extern "system" fn hook_present1(
    chain: Handle,
    interval: u32,
    flags: DXGI_PRESENT,
    parameters: *const DXGI_PRESENT_PARAMETERS,
) -> HRESULT {
    unsafe { present_event(chain, interval, flags, 1, parameters.cast()) }
}
