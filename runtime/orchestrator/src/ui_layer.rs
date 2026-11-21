// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::sr_session::has_capacity;
use crate::ui_layer_abi::*;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::null_mut;

type Handle = *mut c_void;
type ClearTarget = unsafe extern "C" fn(context: Handle, target: Handle);
const RING: usize = RSF_UI_LAYER_RING as usize;
const OK: rsf_ui_layer_result = 0;
const INVALID: rsf_ui_layer_result = -1;
const ABI_MISMATCH: rsf_ui_layer_result = -2;
const RESOURCE_FAILED: rsf_ui_layer_result = -3;

#[repr(C)]
struct Platform {
    create_texture: unsafe extern "C" fn(Handle, *const rsf_ui_layer_setup, u32) -> Handle,
    create_views: unsafe extern "C" fn(Handle, Handle, u32, *mut Handle, *mut Handle),
    immediate_context: unsafe extern "C" fn(Handle) -> Handle,
    clear_target: ClearTarget,
    log_ready: unsafe extern "C" fn(*const rsf_ui_layer_setup, u32),
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
    clear_target: ClearTarget,
    _device: ComRef,
}

impl Layer {
    fn current(&self) -> Option<&Slot> {
        self.started.then(|| &self.slots[self.status.slot as usize])
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ui_layer_create(
    device: Handle,
    setup: *const rsf_ui_layer_setup,
    out: *mut *mut Layer,
    platform: &Platform,
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
        let create_texture = |shared| unsafe {
            ComRef::from_owned((platform.create_texture)(device, &setup, u32::from(shared)))
        };
        let mut texture = create_texture(shareable);
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
            texture = create_texture(shareable);
        }
        let Some(texture) = texture else {
            return RESOURCE_FAILED;
        };
        let mut target = null_mut();
        let mut source = null_mut();
        unsafe {
            (platform.create_views)(
                device,
                texture.as_ptr(),
                setup.srgb,
                &mut target,
                &mut source,
            )
        };
        let views = unsafe { (ComRef::from_owned(target), ComRef::from_owned(source)) };
        let (Some(target), Some(source)) = views else {
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
    if let Some(context) = unsafe { ComRef::from_owned((platform.immediate_context)(device)) } {
        for slot in &slots {
            unsafe { (platform.clear_target)(context.as_ptr(), slot.target.as_ptr()) };
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
            clear_target: platform.clear_target,
            _device: device_ref,
        },
    );
    unsafe { (platform.log_ready)(&setup, shareable) };
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

    let clear = state.clear_target;
    let target = state.slots[state.status.slot as usize].target.as_ptr();
    // The clear can pass through a native hook. No Rust reference crosses that call.
    unsafe { clear(context, target) };
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
