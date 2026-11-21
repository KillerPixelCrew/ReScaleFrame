// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::render_abi::rsf_backend_log_fn as LogFn;
use crate::sr_session::has_capacity;
use rsf_game_sys::sr_abi::rsf_motion_decode_params as Params;
use std::ffi::{CString, c_char, c_void};
use std::ptr::{self, null_mut};
use std::sync::{Mutex, TryLockError};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

type Handle = *mut c_void;
const ABI: u32 = 1;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const RESOURCE_FAILED: i32 = -4;
const SOURCE_MISMATCH: i32 = -5;
const CACHE_IDLE_MS: u64 = 2000;
const GROUP_EDGE: u32 = 8;
const PARAMS_V1: usize = std::mem::offset_of!(Params, validity_enabled);

#[repr(C)]
#[derive(Clone, Copy)]
struct Setup {
    struct_size: u32,
    abi_version: u32,
    width: u32,
    height: u32,
    output_format: u32,
    log: LogFn,
    log_user: Handle,
}

type Resources = [Handle; 4];

#[repr(C)]
#[derive(Default)]
struct Extent {
    width: u32,
    height: u32,
    slices: u32,
    samples: u32,
}

#[repr(C)]
struct Constants {
    scale: [f32; 2],
    bias: [f32; 2],
    output_scale: [f32; 2],
    invalid: f32,
    zero_means_unwritten: u32,
    size: [u32; 2],
    validity_enabled: u32,
    validity_channel: u32,
    validity_threshold: f32,
    padding: [u32; 3],
}

#[repr(C)]
struct Platform {
    make: unsafe extern "C" fn(Handle, &Setup, u32, &mut Resources) -> i32,
    context: unsafe extern "C" fn(Handle, Handle) -> i32,
    describe: unsafe extern "C" fn(Handle, Handle, &mut Extent) -> i32,
    source_view: unsafe extern "C" fn(Handle, Handle) -> Handle,
    dispatch:
        unsafe extern "C" fn(Handle, &Resources, Handle, *const c_void, usize, u32, u32) -> i32,
    log: unsafe extern "C" fn(LogFn, Handle, *const c_char),
}

const _: () = {
    assert!(size_of::<Setup>() == 40 && size_of::<Extent>() == 16);
    assert!(size_of::<Constants>() == 64 && PARAMS_V1 == 36);
    assert!(size_of::<Platform>() == 6 * size_of::<Handle>());
};

struct Source {
    view: ComRef,
    texture: ComRef,
    used: u64,
}

struct Pass {
    sources: Mutex<[Option<Source>; 2]>,
    resources: [ComRef; 4],
    device: ComRef,
    setup: Setup,
    platform: &'static Platform,
}

impl Pass {
    fn say(&self, message: &str) {
        if self.setup.log.is_some()
            && let Ok(message) = CString::new(message)
        {
            unsafe { (self.platform.log)(self.setup.log, self.setup.log_user, message.as_ptr()) };
        }
    }

    fn source_view(
        &self,
        slots: &mut [Option<Source>; 2],
        texture: Handle,
        now: u64,
    ) -> Option<Handle> {
        for slot in slots.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|entry| now.wrapping_sub(entry.used) > CACHE_IDLE_MS)
            {
                *slot = None;
            }
        }
        if let Some(entry) = slots
            .iter_mut()
            .flatten()
            .find(|entry| entry.texture.as_ptr() == texture)
        {
            entry.used = now;
            return Some(entry.view.as_ptr());
        }
        let slot = slots.iter().position(Option::is_none).unwrap_or_else(|| {
            usize::from(slots[1].as_ref().unwrap().used < slots[0].as_ref().unwrap().used)
        });
        let view = unsafe {
            ComRef::from_owned((self.platform.source_view)(self.device.as_ptr(), texture))
        }?;
        let handle = view.as_ptr();
        slots[slot] = Some(Source {
            view,
            texture: unsafe { ComRef::retain(texture) },
            used: now,
        });
        Some(handle)
    }
}

unsafe fn params_copy(input: *const Params) -> Option<Params> {
    if input.is_null() {
        return None;
    }
    let size = unsafe { input.cast::<u32>().read() } as usize;
    if size < PARAMS_V1 {
        return None;
    }
    // The original API only reads the validity fields on a complete record.
    let bytes = if size >= size_of::<Params>() {
        size_of::<Params>()
    } else {
        PARAMS_V1
    };
    let mut params = Params::default();
    unsafe {
        ptr::copy_nonoverlapping(
            input.cast::<u8>(),
            ptr::from_mut(&mut params).cast::<u8>(),
            bytes,
        );
    }
    Some(params)
}

fn constants(params: Params, width: u32, height: u32) -> Option<Constants> {
    let values = [
        params.scale_x,
        params.scale_y,
        params.bias_x,
        params.bias_y,
        params.output_scale_x,
        params.output_scale_y,
        params.invalid_value,
    ];
    if !values.into_iter().all(f32::is_finite)
        || (params.validity_enabled != 0
            && (params.validity_channel > 3 || !params.validity_threshold.is_finite()))
    {
        return None;
    }
    Some(Constants {
        scale: [params.scale_x, params.scale_y],
        bias: [params.bias_x, params.bias_y],
        output_scale: [params.output_scale_x, params.output_scale_y],
        invalid: params.invalid_value,
        zero_means_unwritten: params.zero_means_unwritten,
        size: [width, height],
        validity_enabled: u32::from(params.validity_enabled != 0),
        validity_channel: params.validity_channel,
        validity_threshold: params.validity_threshold,
        padding: [0; 3],
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_motion_decode_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Pass,
    platform: &'static Platform,
) -> i32 {
    if device.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.width == 0 || setup.height == 0 {
        return INVALID;
    }
    if setup.abi_version != ABI {
        return ABI_MISMATCH;
    }
    unsafe { out.write(null_mut()) };
    let mut raw = [null_mut(); 4];
    let status = unsafe { (platform.make)(device, setup, size_of::<Constants>() as u32, &mut raw) };
    let resources = raw.map(|handle| unsafe { ComRef::from_owned(handle) });
    if status != OK {
        return status;
    }
    let [Some(shader), Some(target), Some(uav), Some(constants)] = resources else {
        return RESOURCE_FAILED;
    };
    let owner = crate::allocate_handle(Pass {
        sources: Mutex::new([None, None]),
        resources: [shader, target, uav, constants],
        device: unsafe { ComRef::retain(device) },
        setup: *setup,
        platform,
    });
    let Some(pass) = (unsafe { owner.as_ref() }) else {
        return RESOURCE_FAILED;
    };
    pass.say(&format!(
        "motion decode ready for {}x{}",
        setup.width, setup.height
    ));
    unsafe { out.write(owner) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_motion_decode_run(
    pass: *const Pass,
    context: Handle,
    texture: Handle,
    params: *const Params,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return INVALID;
    };
    if context.is_null() || texture.is_null() {
        return INVALID;
    }
    let Some(params) = (unsafe { params_copy(params) }) else {
        return INVALID;
    };
    let platform = pass.platform;
    let device = pass.device.as_ptr();
    if unsafe { (platform.context)(device, context) } == 0 {
        return SOURCE_MISMATCH;
    }
    let (width, height) = (pass.setup.width, pass.setup.height);
    let Some(constants) = constants(params, width, height) else {
        return INVALID;
    };
    let mut extent = Extent::default();
    if unsafe { (platform.describe)(device, texture, &mut extent) } == 0 {
        pass.say("source belongs to another device");
        return SOURCE_MISMATCH;
    }
    if extent.width != width || extent.height != height || extent.slices != 1 || extent.samples != 1
    {
        pass.say(&format!(
            "source is {}x{} but this pass was built for {width}x{height}",
            extent.width, extent.height
        ));
        return SOURCE_MISMATCH;
    }
    let mut sources = match pass.sources.try_lock() {
        Ok(sources) => sources,
        Err(TryLockError::Poisoned(error)) => error.into_inner(),
        Err(TryLockError::WouldBlock) => return RESOURCE_FAILED,
    };
    let now = unsafe { GetTickCount64() };
    let Some(view) = pass.source_view(&mut sources, texture, now) else {
        drop(sources);
        pass.say("source view could not be created");
        return RESOURCE_FAILED;
    };
    let resources = std::array::from_fn(|i| pass.resources[i].as_ptr());
    if unsafe {
        (platform.dispatch)(
            context,
            &resources,
            view,
            ptr::from_ref(&constants).cast(),
            size_of::<Constants>(),
            width.div_ceil(GROUP_EDGE),
            height.div_ceil(GROUP_EDGE),
        )
    } != 0
    {
        OK
    } else {
        RESOURCE_FAILED
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_motion_decode_texture(pass: *const Pass) -> Handle {
    unsafe { pass.as_ref() }.map_or(null_mut(), |pass| pass.resources[1].as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_motion_decode_destroy(pass: *mut Pass) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_prefix_does_not_admit_validity_fields() {
        let input = Params {
            struct_size: (PARAMS_V1 + size_of::<u32>()) as u32,
            zero_means_unwritten: 1,
            validity_enabled: 1,
            validity_channel: 99,
            validity_threshold: f32::NAN,
            ..Params::default()
        };
        let copied = unsafe { params_copy(&input) }.unwrap();
        let values = constants(copied, 16, 8).unwrap();
        assert_eq!(values.zero_means_unwritten, 1);
        assert_eq!(values.validity_enabled, 0);

        let full = Params {
            struct_size: size_of::<Params>() as u32,
            ..input
        };
        assert!(constants(unsafe { params_copy(&full) }.unwrap(), 16, 8).is_none());
    }
}
