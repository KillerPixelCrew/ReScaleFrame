// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::dxgi_format::{RGBA16_FLOAT, RGBA32_FLOAT};
use crate::sharpening_policy::{FILTERS, effective_mode};
use std::ffi::c_void;

type Handle = *mut c_void;
const SLOT_COUNT: usize = 3;
const GROUP_EDGE: u32 = 16;
const TEXTURE2D: u32 = 3;
const DENY_SHADER_RESOURCE: u32 = 0x8;

// Mirrors the private platform contract in graphics/src/sharpening/d3d12_platform.h.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Texture {
    width: u64,
    height: u32,
    format: u32,
    dimension: u32,
    samples: u32,
    flags: u32,
    array_size: u16,
    mip_levels: u16,
}

impl Texture {
    fn shader_readable(&self) -> bool {
        self.dimension == TEXTURE2D
            && self.array_size == 1
            && self.mip_levels == 1
            && self.samples == 1
            && (1..=u64::from(u32::MAX)).contains(&self.width)
            && self.height != 0
            && self.flags & DENY_SHADER_RESOURCE == 0
    }

    fn scratch_key(&self) -> (u64, u32, u32) {
        (self.width, self.height, self.format)
    }
}

#[repr(C)]
struct Call {
    commands: Handle,
    textures: [Handle; 4],
    options: *const c_void,
    frame: *const c_void,
    mode: u32,
    active: u32,
    slot: u32,
}

#[repr(C)]
struct Dispatch {
    root: Handle,
    pipeline: Handle,
    heap: Handle,
    scratch: Handle,
    descriptions: [Texture; 4],
    mode: u32,
    groups_x: u32,
    groups_y: u32,
}

#[repr(C)]
struct Platform {
    child_device: unsafe extern "C" fn(Handle) -> Handle,
    describe: unsafe extern "C" fn(Handle, &mut Texture),
    create_root: unsafe extern "C" fn(Handle) -> Handle,
    create_pipeline: unsafe extern "C" fn(Handle, Handle, u32) -> Handle,
    create_heap: unsafe extern "C" fn(Handle) -> Handle,
    create_scratch: unsafe extern "C" fn(Handle, u32, u32, u32) -> Handle,
    dispatch: unsafe extern "C" fn(Handle, &Call, &Dispatch),
    copy_back: unsafe extern "C" fn(Handle, Handle, Handle),
}

const _: () = {
    assert!(size_of::<Texture>() == 32 && std::mem::offset_of!(Texture, array_size) == 28);
    assert!(size_of::<Call>() == 72 && std::mem::offset_of!(Call, mode) == 56);
    assert!(size_of::<Dispatch>() == 176 && std::mem::offset_of!(Dispatch, mode) == 160);
    assert!(size_of::<Platform>() == 8 * size_of::<Handle>());
};

#[derive(Default)]
struct Slot {
    scratch: Option<ComRef>,
    heap: Option<ComRef>,
}

#[derive(Default)]
struct Sharpen12 {
    slots: [Slot; SLOT_COUNT],
    pipelines: [Option<ComRef>; FILTERS.len()],
    root: Option<ComRef>,
    device: Option<ComRef>,
    dispatches: u64,
}

impl Sharpen12 {
    fn prepare(
        &mut self,
        platform: &Platform,
        device: &ComRef,
        slot: usize,
        descriptions: [Texture; 4],
        mode: u32,
    ) -> Option<Dispatch> {
        let bound = self.device.get_or_insert_with(|| device.clone());
        let device = device.as_ptr();
        if bound.as_ptr() != device {
            return None;
        }
        if self.root.is_none() {
            self.root = Some(unsafe { ComRef::from_owned((platform.create_root)(device)) }?);
        }
        let root = self.root.as_ref()?.as_ptr();
        for (pipeline, filter) in self.pipelines.iter_mut().zip(FILTERS) {
            if pipeline.is_none() {
                *pipeline = Some(unsafe {
                    ComRef::from_owned((platform.create_pipeline)(device, root, filter))
                }?);
            }
        }
        let selected = FILTERS.iter().position(|&filter| filter == mode)?;
        let pipeline = self.pipelines[selected].as_ref()?.as_ptr();
        let slot = &mut self.slots[slot];
        if slot.heap.is_none() {
            slot.heap = Some(unsafe { ComRef::from_owned((platform.create_heap)(device)) }?);
        }
        let output = descriptions[0];
        let mut old = Texture::default();
        if let Some(scratch) = &slot.scratch {
            unsafe { (platform.describe)(scratch.as_ptr(), &mut old) };
        }
        if old.scratch_key() != output.scratch_key() {
            let next = unsafe {
                ComRef::from_owned((platform.create_scratch)(
                    device,
                    output.width as u32,
                    output.height,
                    output.format,
                ))
            }?;
            // A failed replacement leaves the caller's retired slot intact.
            slot.scratch = Some(next);
        }
        Some(Dispatch {
            root,
            pipeline,
            heap: slot.heap.as_ref()?.as_ptr(),
            scratch: slot.scratch.as_ref()?.as_ptr(),
            descriptions,
            mode,
            groups_x: (output.width as u32).div_ceil(GROUP_EDGE),
            groups_y: output.height.div_ceil(GROUP_EDGE),
        })
    }
}

fn record(owner: &mut *mut Sharpen12, platform: &Platform, call: &Call) -> Option<()> {
    if call.active == 0 {
        return Some(());
    }
    let [output, _, depth, exposure] = call.textures;
    let slot = call.slot as usize;
    if call.commands.is_null() || output.is_null() || slot >= SLOT_COUNT {
        return None;
    }
    let device = unsafe { ComRef::from_owned((platform.child_device)(call.commands)) }?;
    let mut descriptions = [Texture::default(); 4];
    for (resource, desc) in call.textures.into_iter().zip(&mut descriptions) {
        if resource.is_null() {
            continue;
        }
        let resource_device = unsafe { ComRef::from_owned((platform.child_device)(resource)) }?;
        if resource_device.as_ptr() != device.as_ptr() {
            return None;
        }
        unsafe { (platform.describe)(resource, desc) };
        if !desc.shader_readable() {
            return None;
        }
    }
    if !matches!(descriptions[0].format, RGBA16_FLOAT | RGBA32_FLOAT)
        || (!exposure.is_null() && (descriptions[3].width != 1 || descriptions[3].height != 1))
    {
        return None;
    }
    let mode = effective_mode(call.mode, !depth.is_null())?;
    if owner.is_null() {
        *owner = crate::allocate_handle(Sharpen12::default());
    }
    let owner = unsafe { owner.as_mut() }?;
    let dispatch = owner.prepare(platform, &device, slot, descriptions, mode)?;
    unsafe { (platform.dispatch)(device.as_ptr(), call, &dispatch) };
    owner.dispatches = owner.dispatches.wrapping_add(1);
    unsafe { (platform.copy_back)(call.commands, output, dispatch.scratch) };
    Some(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sharpen12_run(
    owner: &mut *mut Sharpen12,
    platform: &Platform,
    call: &Call,
) -> u32 {
    u32::from(record(owner, platform, call).is_some())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sharpen12_dispatches(owner: *const Sharpen12) -> u64 {
    unsafe { owner.as_ref() }.map_or(0, |owner| owner.dispatches)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sharpen12_destroy(owner: *mut Sharpen12) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}
