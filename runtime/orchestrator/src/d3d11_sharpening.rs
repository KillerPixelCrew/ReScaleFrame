// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::{
    RestoreBindings, SaveBindings, SavedBindings, same_device, texture_desc,
};
use crate::d3d11_output_abi::{
    RSF_D3D11_BIND_SHADER_RESOURCE, RSF_D3D11_FORMAT_RGBA16_FLOAT, RSF_D3D11_FORMAT_RGBA32_FLOAT,
    rsf_d3d11_output_child_device, rsf_d3d11_output_create_srv, rsf_d3d11_output_is_immediate,
    rsf_d3d11_output_srv_format, rsf_d3d11_texture_desc as Texture,
};
use crate::sharpening_policy::{FILTERS, effective_mode};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

type Handle = *mut c_void;

#[repr(C)]
struct Draw {
    views: [Handle; 4],
    output: Handle,
    scratch: Handle,
    target: Handle,
    constants: Handle,
    shader: Handle,
    extent: [u32; 6],
    exposure_texture: u32,
    mode: u32,
}

#[repr(C)]
struct Platform {
    shader: unsafe extern "C" fn(Handle, u32) -> Handle,
    constants: unsafe extern "C" fn(Handle) -> Handle,
    scratch: unsafe extern "C" fn(Handle, u32, u32, u32, u32, &mut [Handle; 2]),
    save: SaveBindings,
    restore: RestoreBindings,
    dispatch: unsafe extern "C" fn(Handle, *const c_void, *const c_void, &Draw),
}

const _: () = {
    assert!(size_of::<Draw>() == 104);
    assert!(size_of::<Platform>() == 6 * size_of::<Handle>());
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct ScratchKey {
    width: u32,
    height: u32,
    format: u32,
    sample_quality: u32,
}

struct Scratch {
    target: ComRef,
    texture: ComRef,
    key: ScratchKey,
}

struct Device {
    shaders: [Option<ComRef>; 3],
    constants: Option<ComRef>,
    scratch: Option<Scratch>,
    device: ComRef,
}

impl Device {
    fn prepare(&mut self, platform: &Platform, output: &Texture) -> Option<()> {
        let device = self.device.as_ptr();
        for (shader, mode) in self.shaders.iter_mut().zip(FILTERS) {
            if shader.is_none() {
                *shader = Some(unsafe { ComRef::from_owned((platform.shader)(device, mode)) }?);
            }
        }
        if self.constants.is_none() {
            self.constants = Some(unsafe { ComRef::from_owned((platform.constants)(device)) }?);
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
            let mut pair = [null_mut(); 2];
            unsafe {
                (platform.scratch)(
                    device,
                    key.width,
                    key.height,
                    key.format,
                    key.sample_quality,
                    &mut pair,
                )
            };
            let [Some(texture), Some(target)] =
                pair.map(|value| unsafe { ComRef::from_owned(value) })
            else {
                return None;
            };
            // A texture without its view must never be cached as a usable scratch target.
            self.scratch = Some(Scratch {
                target,
                texture,
                key,
            });
        }
        Some(())
    }
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
        platform: &Platform,
        context: Handle,
        inputs: &[Handle; 4],
        request: u32,
        options: *const c_void,
        frame: *const c_void,
    ) -> Option<()> {
        let [output, motion, depth, exposure] = *inputs;
        if context.is_null()
            || output.is_null()
            || unsafe { rsf_d3d11_output_is_immediate(context) } == 0
        {
            return None;
        }
        let mode = effective_mode(request, !depth.is_null())?;
        let device = unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(context)) }?;
        let native = device.as_ptr();
        let desc = input_texture(native, output)?;
        if desc.width == 0
            || desc.height == 0
            || desc.mips != 1
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
            let format = unsafe { rsf_d3d11_output_srv_format(input.format) };
            views[index] = Some(unsafe {
                ComRef::from_owned(rsf_d3d11_output_create_srv(native, texture, format))
            }?);
        }
        if cache
            .as_ref()
            .is_none_or(|current| current.device.as_ptr() != native)
        {
            *cache = Some(Device {
                device,
                shaders: [const { None }; 3],
                constants: None,
                scratch: None,
            });
        }
        let cache = cache.as_mut()?;
        cache.prepare(platform, &desc)?;
        let scratch = cache.scratch.as_ref()?;
        let draw = Draw {
            views: views
                .each_ref()
                .map(|view| view.as_ref().map_or(null_mut(), ComRef::as_ptr)),
            output,
            scratch: scratch.texture.as_ptr(),
            target: scratch.target.as_ptr(),
            constants: cache.constants.as_ref()?.as_ptr(),
            shader: cache.shaders[(mode - 1) as usize].as_ref()?.as_ptr(),
            extent,
            exposure_texture: u32::from(!exposure.is_null()),
            mode,
        };
        let _bindings = unsafe { SavedBindings::new(context, platform.save, platform.restore) }?;
        unsafe { (platform.dispatch)(context, options, frame, &draw) };
        self.dispatches.fetch_add(1, Ordering::Relaxed);
        Some(())
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_sharpen11_create() -> *mut Owner {
    crate::allocate_handle(Owner::default())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sharpen11_run(
    owner: &Owner,
    platform: &Platform,
    context: Handle,
    inputs: &[Handle; 4],
    mode: u32,
    options: *const c_void,
    frame: *const c_void,
) -> u32 {
    u32::from(
        owner
            .run(platform, context, inputs, mode, options, frame)
            .is_some(),
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sharpen11_count(owner: *const Owner) -> u64 {
    unsafe { owner.as_ref() }.map_or(0, |owner| owner.dispatches.load(Ordering::Relaxed))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sharpen11_destroy(owner: *mut Owner) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{fake, raw};
    use std::cell::Cell;

    thread_local! {
        static SCRATCH_PAIR: Cell<[Handle; 2]> = const { Cell::new([null_mut(); 2]) };
    }

    unsafe extern "C" fn shader(_: Handle, _: u32) -> Handle {
        null_mut()
    }
    unsafe extern "C" fn constants(_: Handle) -> Handle {
        null_mut()
    }
    unsafe extern "C" fn scratch(_: Handle, _: u32, _: u32, _: u32, _: u32, out: &mut [Handle; 2]) {
        *out = SCRATCH_PAIR.get().map(|value| {
            if value.is_null() {
                value
            } else {
                unsafe { ComRef::retain(value) }.into_raw()
            }
        });
    }
    unsafe extern "C" fn save(_: Handle, _: *mut crate::d3d11_output_abi::rsf_d3d11_state) -> u32 {
        0
    }
    unsafe extern "C" fn restore(_: Handle, _: *mut crate::d3d11_output_abi::rsf_d3d11_state) {}
    unsafe extern "C" fn dispatch(_: Handle, _: *const c_void, _: *const c_void, _: &Draw) {}

    #[test]
    fn scratch_view_refusal_releases_texture_and_allows_same_extent_retry() {
        let (device, texture, target) = (fake(), fake(), fake());
        let retained = unsafe { ComRef::retain(raw(&device)) };
        let mut cached = Device {
            shaders: std::array::from_fn(|_| Some(retained.clone())),
            constants: Some(retained.clone()),
            scratch: None,
            device: retained,
        };
        let platform = Platform {
            shader,
            constants,
            scratch,
            save,
            restore,
            dispatch,
        };
        let desc = Texture {
            width: 16,
            height: 16,
            format: RSF_D3D11_FORMAT_RGBA16_FLOAT,
            ..Texture::default()
        };
        SCRATCH_PAIR.set([raw(&texture), null_mut()]);
        assert!(cached.prepare(&platform, &desc).is_none());
        assert!(cached.scratch.is_none());
        assert_eq!(texture.refs.get(), 1);

        SCRATCH_PAIR.set([raw(&texture), raw(&target)]);
        assert!(cached.prepare(&platform, &desc).is_some());
        let scratch = cached.scratch.as_ref().unwrap();
        assert_eq!(scratch.texture.as_ptr(), raw(&texture));
        assert_eq!(scratch.target.as_ptr(), raw(&target));
    }
}
