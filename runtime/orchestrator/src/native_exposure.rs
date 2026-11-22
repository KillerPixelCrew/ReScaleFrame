// SPDX-License-Identifier: MIT
use crate::com_ref;
use crate::d3d11_output::{FullscreenBlit, GraphicsCache, texture_desc};
use crate::native_regions::SourceView;
use crate::sys::*;
use std::ffi::c_void;
use std::ptr;
use windows::core::{IUnknown, Interface};

#[derive(Debug, PartialEq, Eq)]
enum ExposureInput {
    Scalar,
    Convert(u32),
}

fn exposure_input(desc: &rsf_d3d11_texture_desc) -> Option<ExposureInput> {
    if (desc.width, desc.height, desc.layers, desc.samples) != (1, 1, 1, 1) {
        return None;
    }
    if desc.format == RSF_D3D11_FORMAT_R32_FLOAT {
        return Some(ExposureInput::Scalar);
    }
    let format = unsafe { rsf_d3d11_output_srv_format(desc.format) };
    matches!(
        format,
        RSF_D3D11_FORMAT_RGBA32_FLOAT
            | RSF_D3D11_FORMAT_RGBA16_FLOAT
            | RSF_D3D11_FORMAT_RG32_FLOAT
            | RSF_D3D11_FORMAT_RG16_FLOAT
    )
    .then_some(ExposureInput::Convert(format))
}

struct Exposure {
    blit: FullscreenBlit,
    target: IUnknown,
    scalar: IUnknown,
    sources: [Option<SourceView>; 2],
    next: usize,
    device: IUnknown,
}

// SAFETY: the cache owns only D3D11 device children and a fullscreen pass.
// Graphics access is serialized, and teardown runs after the owner drains.
unsafe impl Send for Exposure {}

impl Exposure {
    unsafe fn new(device: IUnknown) -> Option<Self> {
        let desc = rsf_d3d11_texture_desc {
            width: 1,
            height: 1,
            mips: 1,
            layers: 1,
            format: RSF_D3D11_FORMAT_R32_FLOAT,
            samples: 1,
            usage: RSF_D3D11_USAGE_DEFAULT,
            bind_flags: RSF_D3D11_BIND_RENDER_TARGET | RSF_D3D11_BIND_SHADER_RESOURCE,
            ..Default::default()
        };
        let scalar =
            unsafe { com_ref::owned(rsf_d3d11_output_create_texture(device.as_raw(), &desc)) }?;
        let target = unsafe {
            com_ref::owned(rsf_d3d11_output_create_rtv(
                device.as_raw(),
                scalar.as_raw(),
            ))
        }?;
        let blit = unsafe { FullscreenBlit::new_copy(&device, false) }?;
        Some(Self {
            blit,
            target,
            scalar,
            sources: [None, None],
            next: 0,
            device,
        })
    }

    unsafe fn source_view(&mut self, input: *mut c_void, format: u32) -> Option<*mut c_void> {
        if let Some(view) = self
            .sources
            .iter()
            .flatten()
            .find_map(|entry| entry.matching(input, format))
        {
            return Some(view);
        }
        let output = unsafe {
            SourceView::get(
                &mut self.sources[self.next],
                self.device.as_raw(),
                input,
                format,
            )
        }?;
        self.next = (self.next + 1) % self.sources.len();
        Some(output)
    }
}

static EXPOSURE: GraphicsCache<Option<Exposure>> = GraphicsCache::new(None);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_scalar_exposure(
    context: *mut c_void,
    input: *mut c_void,
) -> *mut c_void {
    if context.is_null() || input.is_null() {
        return ptr::null_mut();
    }
    // The enclosing SR pass validates the device and borrows this texture.
    let Some(format) = exposure_input(&unsafe { texture_desc(input) }) else {
        return ptr::null_mut();
    };
    let ExposureInput::Convert(format) = format else {
        return input;
    };
    EXPOSURE
        .with(|cache| {
            let device = unsafe { com_ref::owned(rsf_d3d11_output_child_device(context)) }?;
            let exposure = if let Some(exposure) = cache
                .as_mut()
                .filter(|entry| entry.device.as_raw() == device.as_raw())
            {
                exposure
            } else {
                *cache = None;
                cache.insert(unsafe { Exposure::new(device) }?)
            };
            let view = unsafe { exposure.source_view(input, format) }?;
            unsafe {
                exposure
                    .blit
                    .draw(context, exposure.target.as_raw(), view, 1, 1)
            }
            .then_some(exposure.scalar.as_raw())
        })
        .flatten()
        .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_core_native_exposure_release() {
    EXPOSURE.with(|cache| *cache = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_uses_scalar_or_converts_only_supported_single_sample_textures() {
        let mut desc = rsf_d3d11_texture_desc {
            width: 1,
            height: 1,
            layers: 1,
            samples: 1,
            format: RSF_D3D11_FORMAT_R32_FLOAT,
            ..Default::default()
        };
        assert_eq!(exposure_input(&desc), Some(ExposureInput::Scalar));
        for format in [
            RSF_D3D11_FORMAT_RGBA32_FLOAT,
            RSF_D3D11_FORMAT_RGBA16_FLOAT,
            RSF_D3D11_FORMAT_RG32_FLOAT,
            RSF_D3D11_FORMAT_RG16_FLOAT,
        ] {
            desc.format = format;
            assert_eq!(exposure_input(&desc), Some(ExposureInput::Convert(format)));
        }
        desc.format = RSF_D3D11_FORMAT_R11G11B10_FLOAT;
        assert_eq!(exposure_input(&desc), None);
        desc.format = RSF_D3D11_FORMAT_R32_FLOAT;
        desc.samples = 2;
        assert_eq!(exposure_input(&desc), None);
        desc.samples = 1;
        desc.width = 2;
        assert_eq!(exposure_input(&desc), None);
    }
}
