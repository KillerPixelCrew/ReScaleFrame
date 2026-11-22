// SPDX-License-Identifier: MIT
use crate::com_ref;
use crate::d3d11_output::SavedBindings;
use crate::d3d11_output_platform::{rsf_d3d11_output_clear_state, rsf_d3d11_output_copy_resource};
use crate::graphics_d3d11::{rsf_d3d11_state_restore, rsf_d3d11_state_save};
use crate::sr_session::has_capacity;
use crate::sys::{rsf_depth_scale_frame as Frame, rsf_depth_scale_setup as Setup};
use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
use windows::core::IUnknown;
use windows::core::Interface;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

#[path = "depth_scale/d3d11.rs"]
mod d3d11;

type Handle = *mut c_void;
type Pipeline = [Handle; 14];
type Surfaces = [Handle; 5];
const ABI: u32 = 1;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const RESOURCE_FAILED: i32 = -3;
const BUSY: i32 = -4;
const JITTER_LIMIT: f32 = 16384.0;

#[repr(C)]
struct Constants {
    source: [f32; 4],
    output: [f32; 4],
    jitter: [f32; 2],
    bit: u32,
    padding: u32,
}

const _: () = assert!(size_of::<Constants>() == 48);

const SNAPSHOT: usize = 0;
const OUTPUT: usize = 1;

struct Reserved {
    surfaces: [IUnknown; 5],
    size: [u32; 2],
}

#[derive(Default)]
struct Transaction {
    reserved: Option<Reserved>,
    // The leased native texture, from a successful prepare until restore or discard.
    original: Option<IUnknown>,
    committed: bool,
}

// One graphics-thread transaction over a native depth/stencil texture. Other threads only read
// `thread`; the transaction is borrowed by the owner thread for each call.
pub(super) struct DepthScale {
    thread: u32,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    pipeline: [IUnknown; 14],
    transaction: RefCell<Transaction>,
}

pub(super) fn rect_valid(rect: &[i32; 4], [width, height]: [u32; 2]) -> bool {
    rect[0] >= 0
        && rect[1] >= 0
        && rect[2] > rect[0]
        && rect[3] > rect[1]
        && rect[2] as u32 <= width
        && rect[3] as u32 <= height
}

pub(super) fn jitter_valid(jitter: [f32; 2]) -> bool {
    jitter
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= JITTER_LIMIT)
}

impl DepthScale {
    pub(super) unsafe fn new(context: Handle, setup: &Setup) -> Result<Box<Self>, i32> {
        let device = unsafe { d3d11::immediate_device(context) }.ok_or(INVALID)?;
        let native_context =
            unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.ok_or(INVALID)?;
        let pipeline = d3d11::make_pipeline(&device, setup).ok_or(RESOURCE_FAILED)?;
        let scale = crate::allocate_handle(Self {
            thread: unsafe { GetCurrentThreadId() },
            context: native_context.clone(),
            device,
            pipeline,
            transaction: RefCell::default(),
        });
        if scale.is_null() {
            return Err(RESOURCE_FAILED);
        }
        Ok(unsafe { Box::from_raw(scale) })
    }

    pub(super) fn reserve(&self, texture: Handle) -> bool {
        if texture.is_null() {
            return false;
        }
        let Ok(mut transaction) = self.transaction.try_borrow_mut() else {
            return false;
        };
        self.reserve_surfaces(&mut transaction, texture)
    }

    pub(super) fn retire(self: Box<Self>) {
        // Preserve the native handle's rule: a failed or wrong-thread restore
        // must leave the outstanding depth lease alive.
        if !self.owner() || self.restore() != OK {
            std::mem::forget(self);
        }
    }

    fn owner(&self) -> bool {
        self.thread == unsafe { GetCurrentThreadId() }
    }

    fn ready(&self) -> bool {
        unsafe { self.device.GetDeviceRemovedReason().is_ok() }
    }

    fn describe(&self, texture: Handle) -> Option<[u32; 2]> {
        unsafe { d3d11::describe(&self.device, texture) }
    }

    fn bindings(&self) -> Option<SavedBindings> {
        unsafe {
            SavedBindings::new(
                self.context.as_raw(),
                rsf_d3d11_state_save,
                rsf_d3d11_state_restore,
            )
        }
    }

    fn clear(&self) {
        unsafe { rsf_d3d11_output_clear_state(self.context.as_raw()) };
    }

    fn copy(&self, destination: &IUnknown, source: Handle) {
        unsafe {
            rsf_d3d11_output_copy_resource(self.context.as_raw(), destination.as_raw(), source)
        };
    }

    // Allocation only: the private snapshot and output follow the native texture's size.
    fn reserve_surfaces(&self, transaction: &mut Transaction, texture: Handle) -> bool {
        if transaction.original.is_some() {
            return false;
        }
        let Some(size) = self.describe(texture) else {
            return false;
        };
        if !self.ready() {
            return false;
        }
        if transaction
            .reserved
            .as_ref()
            .is_some_and(|reserved| reserved.size == size)
        {
            return true;
        }
        // Build the complete replacement set before replacing the reusable resources.
        let Some(surfaces) = (unsafe { d3d11::make_surfaces(&self.device, texture) }) else {
            return false;
        };
        transaction.reserved = Some(Reserved { surfaces, size });
        true
    }

    pub(super) fn render(&self, frame: &Frame, capture: bool) -> i32 {
        let Ok(mut transaction) = self.transaction.try_borrow_mut() else {
            return BUSY;
        };
        let original = transaction
            .original
            .as_ref()
            .map_or(null_mut(), Interface::as_raw);
        let held = if capture {
            !original.is_null()
        } else {
            original != frame.texture
        };
        if transaction.committed || held {
            return BUSY;
        }
        let texture = frame.texture;
        if texture.is_null() || !jitter_valid(frame.jitter_pixels) {
            return INVALID;
        }
        let Some(size) = self.describe(texture) else {
            return INVALID;
        };
        if !rect_valid(&frame.source_rect, size) || !rect_valid(&frame.output_rect, size) {
            return INVALID;
        }
        if capture && !self.reserve_surfaces(&mut transaction, texture) {
            return RESOURCE_FAILED;
        }
        let Some(reserved) = transaction.reserved.as_ref() else {
            return RESOURCE_FAILED;
        };
        let Some(bindings) = self.bindings() else {
            return RESOURCE_FAILED;
        };
        let surfaces = &reserved.surfaces;
        self.clear();
        if capture {
            self.copy(&surfaces[SNAPSHOT], texture);
        }
        self.copy(&surfaces[OUTPUT], surfaces[SNAPSHOT].as_raw());
        let pipeline: Pipeline = std::array::from_fn(|i| self.pipeline[i].as_raw());
        let views: Surfaces = std::array::from_fn(|i| surfaces[i].as_raw());
        let mut constants = Constants {
            source: frame.source_rect.map(|value| value as f32),
            output: frame.output_rect.map(|value| value as f32),
            jitter: frame.jitter_pixels,
            bit: 0,
            padding: 0,
        };
        d3d11::bind(&self.context, &pipeline, &views, &frame.output_rect);
        // Depth first, then each stencil bit through its own write mask.
        for step in 0..=8 {
            constants.bit = if step == 0 { 0 } else { 1 << (step - 1) };
            d3d11::draw(&self.context, &pipeline, &constants, step);
        }
        if !self.ready() {
            return RESOURCE_FAILED;
        }
        drop(bindings);
        transaction.original = Some(unsafe { com_ref::retain(texture) });
        OK
    }

    pub(super) fn commit(&self) -> i32 {
        let Ok(mut transaction) = self.transaction.try_borrow_mut() else {
            return BUSY;
        };
        if transaction.committed {
            return BUSY;
        }
        let (Some(original), Some(reserved)) = (&transaction.original, &transaction.reserved)
        else {
            return INVALID;
        };
        let Some(bindings) = self.bindings() else {
            return RESOURCE_FAILED;
        };
        self.clear();
        self.copy(original, reserved.surfaces[OUTPUT].as_raw());
        drop(bindings);
        transaction.committed = true;
        if self.ready() { OK } else { RESOURCE_FAILED }
    }

    // Queue the exact original contents back when committed, then release the lease.
    pub(super) fn restore(&self) -> i32 {
        let Ok(mut transaction) = self.transaction.try_borrow_mut() else {
            return BUSY;
        };
        if transaction.committed {
            let (Some(original), Some(reserved)) = (&transaction.original, &transaction.reserved)
            else {
                return RESOURCE_FAILED;
            };
            let Some(bindings) = self.bindings() else {
                return RESOURCE_FAILED;
            };
            self.clear();
            self.copy(original, reserved.surfaces[SNAPSHOT].as_raw());
            drop(bindings);
            if !self.ready() {
                return RESOURCE_FAILED;
            }
        }
        transaction.committed = false;
        transaction.original = None;
        OK
    }

    pub(super) fn discard(&self) -> i32 {
        let Ok(mut transaction) = self.transaction.try_borrow_mut() else {
            return BUSY;
        };
        transaction.committed = false;
        transaction.original = None;
        OK
    }
}

// Owner-thread access. Wrong-thread calls never touch graphics or the transaction.
unsafe fn owned<'a>(scale: *const DepthScale) -> Result<&'a DepthScale, i32> {
    match unsafe { scale.as_ref() } {
        None => Err(INVALID),
        Some(scale) if !scale.owner() => Err(BUSY),
        Some(scale) => Ok(scale),
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_depth_scale_create(
    context: Handle,
    setup: *const Setup,
    out: *mut *mut DepthScale,
) -> i32 {
    if !out.is_null() {
        unsafe { out.write(null_mut()) };
    }
    if context.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != ABI {
        return ABI_MISMATCH;
    }
    match unsafe { DepthScale::new(context, setup) } {
        Ok(scale) => {
            unsafe { out.write(Box::into_raw(scale)) };
            OK
        }
        Err(status) => status,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_depth_scale_reserve(scale: *const DepthScale, texture: Handle) -> u32 {
    let Ok(scale) = (unsafe { owned(scale) }) else {
        return 0;
    };
    scale.reserve(texture).into()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_depth_scale_prepare(scale: *const DepthScale, frame: *const Frame) -> i32 {
    if scale.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != ABI {
        return ABI_MISMATCH;
    }
    match unsafe { owned(scale) } {
        Ok(scale) => scale.render(frame, true),
        Err(status) => status,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_depth_scale_commit(scale: *const DepthScale) -> i32 {
    unsafe { owned(scale) }.map_or_else(|status| status, DepthScale::commit)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_depth_scale_restore(scale: *const DepthScale) -> i32 {
    unsafe { owned(scale) }.map_or_else(|status| status, DepthScale::restore)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_depth_scale_discard(scale: *const DepthScale) -> i32 {
    unsafe { owned(scale) }.map_or_else(|status| status, DepthScale::discard)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_depth_scale_destroy(scale: *mut DepthScale) -> i32 {
    if scale.is_null() {
        return OK;
    }
    let status = unsafe { rsf_depth_scale_restore(scale) };
    if status == OK {
        drop(unsafe { Box::from_raw(scale) });
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_stay_inside_the_allocation() {
        assert!(rect_valid(&[0, 0, 16, 16], [16, 16]));
        assert!(!rect_valid(&[0, 0, 17, 16], [16, 16]));
        assert!(!rect_valid(&[-1, 0, 8, 8], [16, 16]));
        assert!(!rect_valid(&[4, 0, 4, 8], [16, 16]));
    }

    #[test]
    fn jitter_is_finite_and_bounded() {
        assert!(jitter_valid([0.5, -16384.0]));
        assert!(!jitter_valid([f32::NAN, 0.0]));
        assert!(!jitter_valid([0.0, 16384.5]));
    }
}
