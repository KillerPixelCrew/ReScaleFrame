// SPDX-License-Identifier: MIT
use crate::com_ref::{ComRef, vtable};
use crate::sr_session::has_capacity;
use crate::ui_layer_abi::*;
use std::ffi::{CString, c_void};
use std::mem::MaybeUninit;
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::{
    Direct3D::D3D_SRV_DIMENSION_TEXTURE2D, Direct3D11::*, Dxgi::Common::*,
};

type Handle = *mut c_void;
const RING: usize = RSF_UI_LAYER_RING as usize;
const OK: rsf_ui_layer_result = 0;
const INVALID: rsf_ui_layer_result = -1;
const ABI_MISMATCH: rsf_ui_layer_result = -2;
const RESOURCE_FAILED: rsf_ui_layer_result = -3;

unsafe fn create_texture(
    device: Handle,
    setup: &rsf_ui_layer_setup,
    shared: bool,
) -> Option<ComRef> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: setup.width,
        Height: setup.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: if setup.srgb != 0 {
            DXGI_FORMAT_R8G8B8A8_TYPELESS
        } else {
            DXGI_FORMAT_R8G8B8A8_UNORM
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET
            | D3D11_BIND_SHADER_RESOURCE
            | D3D11_BIND_UNORDERED_ACCESS)
            .0 as u32,
        MiscFlags: if shared {
            (D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE).0 as u32
        } else {
            0
        },
        ..Default::default()
    };
    let mut texture = null_mut();
    let status = unsafe {
        (vtable::<ID3D11Device>(&device).CreateTexture2D)(device, &desc, null(), &mut texture)
    };
    unsafe { ComRef::from_result(status, texture) }
}

unsafe fn create_views(device: Handle, texture: Handle, srgb: bool) -> Option<(ComRef, ComRef)> {
    let api = unsafe { vtable::<ID3D11Device>(&device) };
    // Encode on the diverted draw and read the stored bits for the composite.
    let target_desc = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let source_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: 1,
            },
        },
    };
    let mut target = null_mut();
    let status = unsafe {
        (api.CreateRenderTargetView)(
            device,
            texture,
            if srgb { &target_desc } else { null() },
            &mut target,
        )
    };
    let target = unsafe { ComRef::from_result(status, target) }?;
    let mut source = null_mut();
    let status = unsafe {
        (api.CreateShaderResourceView)(
            device,
            texture,
            if srgb { &source_desc } else { null() },
            &mut source,
        )
    };
    let source = unsafe { ComRef::from_result(status, source) }?;
    Some((target, source))
}

unsafe fn immediate_context(device: Handle) -> Option<ComRef> {
    let mut context = null_mut();
    unsafe { (vtable::<ID3D11Device>(&device).GetImmediateContext)(device, &mut context) };
    unsafe { ComRef::from_owned(context) }
}

unsafe fn clear_target(context: Handle, target: Handle) {
    unsafe {
        (vtable::<ID3D11DeviceContext>(&context).ClearRenderTargetView)(
            context,
            target,
            [0.0; 4].as_ptr(),
        )
    };
}

struct Slot {
    source: ComRef,
    target: ComRef,
    texture: ComRef,
}

struct Layer {
    slots: [Slot; RING],
    status: rsf_ui_layer_status,
    started: bool,
    _device: ComRef,
}

impl Layer {
    fn current(&self) -> Option<&Slot> {
        self.started.then(|| &self.slots[self.status.slot as usize])
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_layer_create(
    device: Handle,
    setup: *const rsf_ui_layer_setup,
    out: *mut *mut Layer,
) -> rsf_ui_layer_result {
    if device.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { setup.read() };
    if setup.abi_version != RSF_UI_LAYER_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if setup.width == 0 || setup.height == 0 {
        return INVALID;
    }
    unsafe { out.write(null_mut()) };

    let allocation = crate::allocate_handle(MaybeUninit::<Layer>::uninit());
    if allocation.is_null() {
        return RESOURCE_FAILED;
    }
    let allocation = unsafe { Box::from_raw(allocation) };
    let device_ref = unsafe { ComRef::retain(device) };
    let mut shareable = setup.shareable != 0;
    let mut slots = [const { None }; RING];
    for slot in &mut slots {
        let mut texture = unsafe { create_texture(device, &setup, shareable) };
        if texture.is_none() && shareable {
            if let Some(log) = setup.log {
                unsafe {
                    log(
                        setup.log_user,
                        c"ui layer: shared creation refused, retrying without sharing".as_ptr(),
                    )
                };
            }
            // Retry this slot once. Remaining slots use the same local allocation mode.
            shareable = false;
            texture = unsafe { create_texture(device, &setup, shareable) };
        }
        let Some(texture) = texture else {
            return RESOURCE_FAILED;
        };
        let Some((target, source)) =
            (unsafe { create_views(device, texture.as_ptr(), setup.srgb != 0) })
        else {
            return RESOURCE_FAILED;
        };
        *slot = Some(Slot {
            source,
            target,
            texture,
        });
    }
    let slots = slots.map(Option::unwrap);

    // Initialize every slot before giving its handles to the caller.
    let mut clears = 0;
    if let Some(context) = unsafe { immediate_context(device) } {
        for slot in &slots {
            unsafe { clear_target(context.as_ptr(), slot.target.as_ptr()) };
            clears += 1;
        }
    }
    let shareable = u32::from(shareable);
    let layer = Box::write(
        allocation,
        Layer {
            slots,
            status: rsf_ui_layer_status {
                struct_size: size_of::<rsf_ui_layer_status>() as u32,
                width: setup.width,
                height: setup.height,
                slot: 0,
                shareable,
                written_this_frame: 0,
                frames_begun: 0,
                frames_written: 0,
                clears,
            },
            started: false,
            _device: device_ref,
        },
    );
    if let Some(log) = setup.log {
        let sharing = if shareable != 0 {
            "shareable"
        } else {
            "not shared"
        };
        let message = CString::new(format!(
            "ui layer: {}x{} R8G8B8A8_UNORM, {RING} deep, {sharing}",
            setup.width, setup.height
        ))
        .expect("numeric UI layer summary has no NUL");
        unsafe { log(setup.log_user, message.as_ptr()) };
    }
    unsafe { out.write(Box::into_raw(layer)) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_layer_destroy(layer: *mut Layer) {
    if !layer.is_null() {
        drop(unsafe { Box::from_raw(layer) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_layer_begin_frame(
    layer: *mut Layer,
    context: Handle,
) -> rsf_ui_layer_result {
    if context.is_null() {
        return INVALID;
    }
    let Some(state) = (unsafe { layer.as_mut() }) else {
        return INVALID;
    };
    // Advance before clearing so the just-finished slot stays intact for presentation.
    if state.started {
        state.status.slot = (state.status.slot + 1) % RSF_UI_LAYER_RING;
    }
    state.started = true;
    state.status.written_this_frame = 0;
    state.status.frames_begun = state.status.frames_begun.wrapping_add(1);

    let target = state.slots[state.status.slot as usize].target.as_ptr();
    // The clear can pass through a native hook. No Rust reference crosses that call.
    unsafe { clear_target(context, target) };
    let status = unsafe { &mut (*layer).status };
    status.clears = status.clears.wrapping_add(1);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_layer_target(layer: *const Layer) -> Handle {
    unsafe { layer.as_ref() }
        .and_then(Layer::current)
        .map_or(null_mut(), |slot| slot.target.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_layer_source(layer: *const Layer) -> Handle {
    unsafe { layer.as_ref() }
        .and_then(Layer::current)
        .map_or(null_mut(), |slot| slot.source.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_layer_texture(layer: *const Layer) -> Handle {
    unsafe { layer.as_ref() }
        .and_then(Layer::current)
        .map_or(null_mut(), |slot| slot.texture.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_layer_mark_written(layer: *mut Layer) {
    if let Some(layer) = unsafe { layer.as_mut() }
        && layer.started
        && layer.status.written_this_frame == 0
    {
        layer.status.written_this_frame = 1;
        layer.status.frames_written = layer.status.frames_written.wrapping_add(1);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ui_layer_get_status(
    layer: *const Layer,
    status: *mut rsf_ui_layer_status,
) -> rsf_ui_layer_result {
    let Some(layer) = (unsafe { layer.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(status) } {
        return INVALID;
    }
    let mut snapshot = layer.status;
    snapshot.struct_size = unsafe { status.cast::<u32>().read() };
    unsafe { status.write(snapshot) };
    OK
}
