// SPDX-License-Identifier: MIT
// Stage 3 burst readback: the staging copies of the seam twin and the back buffer, and their one-time map at the end.
// Rust admits the context, source and device, writes the staging descriptor and names each subresource. The D3D11
// facts come from the typed D3D11 calls (sr_d3d11::d3d: QueryInterface, GetDesc, GetDevice, CreateTexture2D); the stage
// leaves keep GetType, the context's GetDevice, the predicated CopySubresourceRegion, Map, Unmap and the DXGI size
// table. The call order is that of the former sr_stage.cpp staging_copy and map_staging.
use super::{GpuLease, Platform, Refused, leaves};
use crate::lease::Lease;
use crate::sr_d3d11::{Texture, d3d, lease};
use std::ffi::c_void;
use std::ptr::null;

#[cfg(test)]
pub(crate) mod tests;

// D3D11_USAGE_STAGING and D3D11_CPU_ACCESS_READ, checked against the SDK in sr_stage.cpp.
const USAGE_STAGING: u32 = 3;
const CPU_ACCESS_READ: u32 = 0x2_0000;
// The burst copies and reads only the top mip of the first array slice: D3D11CalcSubresource(0, 0, mips) is 0 for
// the source, and the copy has one mip and one layer.
const SUBRESOURCE: u32 = 0;

// rsf::same_device: the texture's GetDevice result is the context's device.
fn same_device(texture: *mut c_void, device: &Lease) -> bool {
    lease(unsafe { d3d::texture_device(texture) })
        .is_some_and(|owner| owner.as_ptr() == device.as_ptr())
}

// The staging descriptor for a single-sample source: its size, format and sample quality, one mip and one layer,
// CPU-readable with no bind or misc flags. A multisampled source is refused.
fn readback(source: &Texture) -> Option<Texture> {
    (source.samples == 1).then_some(Texture {
        mips: 1,
        layers: 1,
        usage: USAGE_STAGING,
        bindings: 0,
        cpu_access: CPU_ACCESS_READ,
        misc: 0,
        ..*source
    })
}

// An owned staging copy of a single-sample texture on the context's device, or None. Refuses a null or deferred
// context, a null or non-2D source, a multisampled source, a source on another device and a failed create, each
// before any GPU work. The copy runs with predication cleared and restored by the leaf.
pub(super) fn copy(
    platform: &Platform,
    context: *mut c_void,
    source: *mut c_void,
) -> Option<GpuLease> {
    if !platform.bound {
        return None;
    }
    if context.is_null() || source.is_null() || !leaf!(platform, immediate_context(context)) {
        return None;
    }
    let texture = lease(unsafe { d3d::as_texture(source) })?;
    let mut description = Texture::default();
    unsafe { d3d::describe_texture(texture.as_ptr(), &mut description) };
    let description = readback(&description)?;
    let device = lease(leaf!(platform, context_device(context)))?;
    if !same_device(texture.as_ptr(), &device) {
        return None;
    }
    let copy = lease(unsafe { d3d::texture(device.as_ptr(), &description, null(), 0) })?;
    leaf!(
        platform,
        copy_subresource(
            context,
            copy.as_ptr(),
            SUBRESOURCE,
            texture.as_ptr(),
            SUBRESOURCE
        )
    );
    // The device and then the source reference drop here, as the former ComPtr locals did.
    Some(GpuLease(copy))
}

// What a staging texture reports, mapped or not: the size and format for the summary.
#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) format: u32,
    pub(super) pixel_bytes: u32,
}

// A held staging texture mapped for reading. Dropping it unmaps the same subresource.
pub(super) struct Mapping<'a> {
    context: *mut c_void,
    texture: &'a GpuLease,
    pub(super) layout: Layout,
    data: *const c_void,
    row_pitch: u32,
}

impl Mapping<'_> {
    pub(super) fn rows(&self) -> Option<impl Iterator<Item = &[u8]>> {
        let row = self.layout.width.checked_mul(self.layout.pixel_bytes)?;
        let height = self.layout.height;
        if self.data.is_null() || row == 0 || height == 0 || self.row_pitch < row {
            return None;
        }
        let span = u64::from(self.row_pitch) * u64::from(height - 1) + u64::from(row);
        if span > isize::MAX as u64 || self.data.addr().checked_add(span as usize).is_none() {
            return None;
        }
        Some((0..height).map(move |y| {
            let offset = y as usize * self.row_pitch as usize;
            // The returned rows borrow this mapping and skip native row padding.
            unsafe { std::slice::from_raw_parts(self.data.cast::<u8>().add(offset), row as usize) }
        }))
    }
}

impl Drop for Mapping<'_> {
    fn drop(&mut self) {
        // A mapping exists only on a bound stage.
        #[allow(unused_unsafe)]
        unsafe {
            leaves::unmap(self.context, self.texture.0.as_ptr(), SUBRESOURCE)
        };
    }
}

// Describes a held staging texture and maps it for reading when the format has a known pixel size, the context is
// immediate and the texture belongs to the context's device. The layout keeps the size and format on refusal.
pub(super) fn map<'a>(
    platform: &Platform,
    context: *mut c_void,
    texture: &'a GpuLease,
) -> (Layout, Option<Mapping<'a>>) {
    let mut layout = Layout {
        width: 0,
        height: 0,
        format: 0,
        pixel_bytes: 0,
    };
    if !platform.bound {
        return (layout, None);
    }
    let handle = texture.0.as_ptr();
    let mut description = Texture::default();
    unsafe { d3d::describe_texture(handle, &mut description) };
    layout.width = description.size.width;
    layout.height = description.size.height;
    layout.format = description.format;
    layout.pixel_bytes = leaf!(platform, pixel_bytes(description.format));
    let device = lease(leaf!(platform, context_device(context)));
    if layout.pixel_bytes == 0
        || !leaf!(platform, immediate_context(context))
        || !device
            .as_ref()
            .is_some_and(|device| same_device(handle, device))
    {
        return (layout, None);
    }
    let (mut data, mut row_pitch) = (null(), 0);
    if !leaf!(
        platform,
        map_read(context, handle, SUBRESOURCE, &mut data, &mut row_pitch)
    ) {
        return (layout, None);
    }
    let mapping = Mapping {
        context,
        texture,
        layout,
        data,
        row_pitch,
    };
    // The context's device reference drops after Map, as the former ComPtr did.
    drop(device);
    (layout, Some(mapping))
}
