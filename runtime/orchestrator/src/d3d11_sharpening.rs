// SPDX-License-Identifier: MIT
use crate::com_ref;
use crate::d3d11_output::{SavedBindings, same_device, texture_desc};
use crate::d3d11_output_platform::{
    rsf_d3d11_output_child_device, rsf_d3d11_output_create_srv, rsf_d3d11_output_is_immediate,
    rsf_d3d11_output_srv_format,
};
use crate::graphics_d3d11::{
    compute_shader, constant_buffer, dispatch_compute, rsf_d3d11_state_restore,
    rsf_d3d11_state_save, shader_code, target_view,
};
use crate::sharpening_policy::{FILTERS, effective_mode};
use crate::sys::rsf_sharpening_options as Options;
use crate::sys::{
    RSF_D3D11_BIND_SHADER_RESOURCE, RSF_D3D11_FORMAT_RGBA16_FLOAT, RSF_D3D11_FORMAT_RGBA32_FLOAT,
    RSF_D3D11_USAGE_IMMUTABLE, rsf_d3d11_texture_desc as Texture,
};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCOMPILE_ENABLE_STRICTNESS;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::core::IUnknown;
use windows::core::Interface;

type Handle = *mut c_void;

pub(super) use crate::sys::rsf_sharpen11_frame as Frame;
const CONSTANT_WORDS: usize = 32;
const UAV_SLOTS: usize = 8;

unsafe extern "C" {
    fn rsf_sharpening_hlsl(mode: u32) -> *const c_char;
    pub(super) fn rsf_sharpen11_active(options: &Options) -> u32;
    pub(super) fn rsf_sharpen11_pack(
        options: &Options,
        frame: &Frame,
        extent: *const u32,
        exposure_texture: u32,
        mode: u32,
        packed: *mut u32,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ScratchKey {
    width: u32,
    height: u32,
    format: u32,
    sample_quality: u32,
}

struct Scratch {
    target: IUnknown,
    texture: IUnknown,
    key: ScratchKey,
}

struct Device {
    shaders: [Option<IUnknown>; 3],
    constants: Option<IUnknown>,
    scratch: Option<Scratch>,
    device: IUnknown,
}

impl Device {
    fn prepare(&mut self, output: &Texture) -> Option<()> {
        let raw = self.device.as_raw();
        let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }?;
        for (shader, mode) in self.shaders.iter_mut().zip(FILTERS) {
            if shader.is_none() {
                let source = unsafe { rsf_sharpening_hlsl(mode) };
                if source.is_null() {
                    return None;
                }
                let bytes = unsafe { CStr::from_ptr(source) }.to_bytes();
                let code = shader_code(
                    bytes,
                    c"Sharpening",
                    c"CSMain",
                    c"cs_5_0",
                    D3DCOMPILE_ENABLE_STRICTNESS,
                )
                .ok()?;
                *shader = Some(compute_shader(device, &code)?.into());
            }
        }
        if self.constants.is_none() {
            self.constants = Some(constant_buffer(device, CONSTANT_WORDS as u32 * 4, false)?.into());
        }
        let key = ScratchKey {
            width: output.width,
            height: output.height,
            format: output.format,
            sample_quality: output.sample_quality,
        };
        if self
            .scratch
            .as_ref()
            .is_none_or(|scratch| scratch.key != key)
        {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: key.width,
                Height: key.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT(key.format as i32),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: key.sample_quality,
                },
                BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                ..Default::default()
            };
            let mut texture = None;
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.ok()?;
            let texture = texture?;
            self.scratch = Some(complete_scratch(key, texture.clone().into(), || {
                Some(target_view(device, &texture, 0)?.into())
            })?);
        }
        Some(())
    }
}

// A texture without its view must never become the cached scratch target.
fn complete_scratch(
    key: ScratchKey,
    texture: IUnknown,
    create_view: impl FnOnce() -> Option<IUnknown>,
) -> Option<Scratch> {
    Some(Scratch {
        key,
        texture,
        target: create_view()?,
    })
}

#[derive(Default)]
struct Owner {
    device: Mutex<Option<Device>>,
    dispatches: AtomicU64,
}

fn input_texture(device: Handle, texture: Handle) -> Option<Texture> {
    if !unsafe { same_device(device, texture) } {
        return None;
    }
    let desc = unsafe { texture_desc(texture) };
    (desc.samples == 1 && desc.layers == 1 && desc.bind_flags & RSF_D3D11_BIND_SHADER_RESOURCE != 0)
        .then_some(desc)
}

impl Owner {
    fn run(
        &self,
        context: Handle,
        inputs: &[Handle; 4],
        options: &Options,
        frame: &Frame,
    ) -> Option<()> {
        let [output, motion, depth, exposure] = *inputs;
        if context.is_null()
            || output.is_null()
            || unsafe { rsf_d3d11_output_is_immediate(context) } == 0
        {
            return None;
        }
        let mode = effective_mode(options.mode, !depth.is_null())?;
        let device = unsafe { com_ref::owned(rsf_d3d11_output_child_device(context)) }?;
        let native = device.as_raw();
        let desc = input_texture(native, output)?;
        if desc.width == 0
            || desc.height == 0
            || desc.mips != 1
            || desc.usage == RSF_D3D11_USAGE_IMMUTABLE
            || !matches!(
                desc.format,
                RSF_D3D11_FORMAT_RGBA16_FLOAT | RSF_D3D11_FORMAT_RGBA32_FLOAT
            )
        {
            return None;
        }

        let mut cache = self.device.try_lock().ok()?;
        let mut views = [const { None }; 4];
        let mut extent = [desc.width, desc.height, 0, 0, 0, 0];
        for (index, texture) in [output, motion, depth, exposure].into_iter().enumerate() {
            if texture.is_null() {
                continue;
            }
            let input = if index == 0 {
                desc
            } else {
                input_texture(native, texture)?
            };
            match index {
                1 => extent[2..4].copy_from_slice(&[input.width, input.height]),
                2 => extent[4..6].copy_from_slice(&[input.width, input.height]),
                3 if input.width != 1 || input.height != 1 => return None,
                _ => {}
            }
            let format = rsf_d3d11_output_srv_format(input.format);
            views[index] = Some(unsafe {
                com_ref::owned(rsf_d3d11_output_create_srv(native, texture, format))
            }?);
        }
        if cache
            .as_ref()
            .is_none_or(|current| current.device.as_raw() != native)
        {
            *cache = Some(Device {
                device,
                shaders: [const { None }; 3],
                constants: None,
                scratch: None,
            });
        }
        let cache = cache.as_mut()?;
        cache.prepare(&desc)?;
        let scratch = cache.scratch.as_ref()?;
        let sources = views
            .each_ref()
            .map(|view| view.as_ref().map_or(null_mut(), Interface::as_raw));
        let shader = cache.shaders[(mode - 1) as usize].as_ref()?.as_raw();
        let buffer = cache.constants.as_ref()?.as_raw();
        let scratch_texture = scratch.texture.as_raw();
        let context_native = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }?;
        let shader = unsafe { ID3D11ComputeShader::from_raw_borrowed(&shader) }?;
        let native_buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&buffer) }?;
        let output_texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&output) }?;
        let scratch_texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&scratch_texture) }?;
        let _bindings =
            unsafe { SavedBindings::new(context, rsf_d3d11_state_save, rsf_d3d11_state_restore) }?;
        let mut constants = [0; CONSTANT_WORDS];
        unsafe {
            rsf_sharpen11_pack(
                options,
                frame,
                extent.as_ptr(),
                u32::from(!exposure.is_null()),
                mode,
                constants.as_mut_ptr(),
            );
            // Detach inherited output bindings before binding the same texture as an SRV.
            let vtable = context_native.vtable();
            let empty = [null_mut(); UAV_SLOTS];
            (vtable.OMSetRenderTargetsAndUnorderedAccessViews)(
                context,
                0,
                std::ptr::null(),
                null_mut(),
                0,
                UAV_SLOTS as u32,
                empty.as_ptr(),
                std::ptr::null(),
            );
            (vtable.CSSetUnorderedAccessViews)(
                context,
                0,
                UAV_SLOTS as u32,
                empty.as_ptr(),
                std::ptr::null(),
            );
            context_native.UpdateSubresource(
                native_buffer,
                0,
                None,
                constants.as_ptr().cast(),
                0,
                0,
            );
        }
        dispatch_compute(
            context_native,
            shader,
            &sources,
            &[scratch.target.as_raw()],
            None,
            [extent[0].div_ceil(16), extent[1].div_ceil(16)],
            true,
            Some(buffer),
        );
        unsafe { context_native.CopyResource(output_texture, scratch_texture) };
        self.dispatches.fetch_add(1, Ordering::Relaxed);
        Some(())
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_sharpen11_run(
    owner: &mut Handle,
    context: Handle,
    inputs: &[Handle; 4],
    options: &Options,
    frame: &Frame,
) -> u32 {
    if unsafe { rsf_sharpen11_active(options) } == 0 {
        return 1;
    }
    if owner.is_null() {
        *owner = crate::allocate_handle(Owner::default()).cast();
    }
    let Some(owner) = (unsafe { owner.cast::<Owner>().as_ref() }) else {
        return 0;
    };
    u32::from(owner.run(context, inputs, options, frame).is_some())
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_sharpen11_count(owner: Handle) -> u64 {
    unsafe { owner.cast::<Owner>().as_ref() }
        .map_or(0, |owner| owner.dispatches.load(Ordering::Relaxed))
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_sharpen11_destroy(owner: Handle) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner.cast::<Owner>()) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{fake, raw};
    #[test]
    fn scratch_view_refusal_releases_texture_and_allows_same_extent_retry() {
        let (texture, target) = (fake(), fake());
        let key = ScratchKey {
            width: 16,
            height: 16,
            format: RSF_D3D11_FORMAT_RGBA16_FLOAT,
            sample_quality: 0,
        };
        let retain_texture = || unsafe { com_ref::retain(raw(&texture)) };
        assert!(complete_scratch(key, retain_texture(), || None).is_none());
        assert_eq!(texture.refs.get(), 1);

        let scratch = complete_scratch(key, retain_texture(), || {
            Some(unsafe { com_ref::retain(raw(&target)) })
        })
        .unwrap();
        assert_eq!(scratch.texture.as_raw(), raw(&texture));
        assert_eq!(scratch.target.as_raw(), raw(&target));
    }
}
