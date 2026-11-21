// SPDX-License-Identifier: MIT
use super::*;

// Private ABI with xess_backend.cpp.
#[repr(C)]
struct Ops {
    load: Load,
    bind: unsafe extern "C" fn(Raw, *mut Raw) -> i32,
    unbind: unsafe extern "C" fn(Raw),
    version: unsafe extern "C" fn(Raw, *mut u16) -> i32,
    create: unsafe extern "C" fn(Raw, Raw, *mut Raw) -> i32,
    destroy: unsafe extern "C" fn(Raw, Raw),
    create_mask: unsafe extern "C" fn(Raw, *const u32, *mut Raw) -> i32,
    release_mask: unsafe extern "C" fn(Raw),
    initialize: unsafe extern "C" fn(Raw, Raw, *const Open) -> i32,
    plan: unsafe extern "C" fn(Raw, Raw, *const u32, u32, *mut u32) -> i32,
    velocity: unsafe extern "C" fn(Raw, Raw, f32, f32) -> i32,
    execute: unsafe extern "C" fn(Raw, Raw, Raw, *const Frame, Raw, f32) -> i32,
}

const _: () = assert!(size_of::<Ops>() == 12 * size_of::<Raw>());

pub(super) struct Owner {
    ops: &'static Ops,
    sdk: Bindings,
    context: Raw,
    zero_mask: Raw,
    auto_exposure: bool,
}

impl Owner {
    fn open(desc: &Open, output: Extent, ops: &'static Ops) -> Result<(Backend, Version), i32> {
        let sdk = Bindings::open(
            desc,
            ops.load,
            |module, out| unsafe { (ops.bind)(module, out) },
            ops.unbind,
        )?;
        let mut owner = Self {
            ops,
            sdk,
            context: null_mut(),
            zero_mask: null_mut(),
            auto_exposure: desc.auto_exposure != 0,
        };
        let mut parts = [0; 4];
        checked(unsafe { (ops.version)(owner.sdk.raw, parts.as_mut_ptr()) })?;
        let [major, minor, patch, reserved] = parts;
        let id = (u64::from(major) << 48)
            | (u64::from(minor) << 32)
            | (u64::from(patch) << 16)
            | u64::from(reserved);
        if desc.version_id != 0 && desc.version_id != id {
            return Err(NOT_SUPPORTED);
        }
        checked(unsafe { (ops.create)(owner.sdk.raw, desc.device, &mut owner.context) })?;
        if owner.context.is_null() {
            return Err(INIT_FAILED);
        }
        // Responsive-mask mode requires a texture at every evaluation.
        checked(unsafe { (ops.create_mask)(desc.device, output.as_ptr(), &mut owner.zero_mask) })?;
        if owner.zero_mask.is_null() {
            return Err(INIT_FAILED);
        }
        checked(unsafe { (ops.initialize)(owner.sdk.raw, owner.context, desc) })?;
        let version = Version::new(id, format!("XeSS {major}.{minor}.{patch}").as_bytes());
        Ok((Backend::Xess(owner), version))
    }

    pub(super) fn plan(
        &self,
        output: &Extent,
        quality: u32,
        width: *mut u32,
        height: *mut u32,
    ) -> i32 {
        if quality > ULTRA_QUALITY {
            return INVALID;
        }
        let mut input = [0; 2];
        let result = unsafe {
            (self.ops.plan)(
                self.sdk.raw,
                self.context,
                output.as_ptr(),
                quality,
                input.as_mut_ptr(),
            )
        };
        if result == OK {
            unsafe {
                width.write(input[0]);
                height.write(input[1]);
            }
        }
        result
    }

    pub(super) fn evaluate(&self, commands: Raw, frame: &Frame) -> i32 {
        if !self.auto_exposure && frame.exposure.resource.is_null() {
            return INVALID;
        }
        let result = unsafe {
            (self.ops.velocity)(
                self.sdk.raw,
                self.context,
                frame.motion_scale_x,
                frame.motion_scale_y,
            )
        };
        if result != OK {
            return result;
        }
        let mask = if frame.reactive.resource.is_null() {
            self.zero_mask
        } else {
            frame.reactive.resource
        };
        unsafe {
            (self.ops.execute)(
                self.sdk.raw,
                self.context,
                commands,
                frame,
                mask,
                1.0 / frame.pre_exposure,
            )
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe { (self.ops.destroy)(self.sdk.raw, self.context) };
        }
        if !self.zero_mask.is_null() {
            unsafe { (self.ops.release_mask)(self.zero_mask) };
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d12_sr_provider_xess_open(
    desc: &Open,
    out: *mut Raw,
    ops: &'static Ops,
) -> i32 {
    open_session(desc, out, |output| Owner::open(desc, output, ops))
}
