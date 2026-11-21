// SPDX-License-Identifier: MIT
use super::*;
use std::ffi::{CStr, CString};

const MAX_VERSIONS: usize = 32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProviderVersion {
    id: u64,
    name: *const c_char,
    major: u32,
    minor: u32,
    patch: u32,
}

// Private ABI with fsr_backend.cpp.
#[repr(C)]
struct Ops {
    load: Load,
    bind: unsafe extern "C" fn(Raw, *const Open, u32, *mut Raw) -> i32,
    unbind: unsafe extern "C" fn(Raw),
    versions: unsafe extern "C" fn(Raw, Raw, *mut ProviderVersion, *mut u64) -> i32,
    create: unsafe extern "C" fn(Raw, *const Open, u64, *mut Raw) -> i32,
    destroy: unsafe extern "C" fn(Raw, *mut Raw),
    effective_version: unsafe extern "C" fn(Raw, *mut Raw, *mut u64) -> i32,
    plan: unsafe extern "C" fn(Raw, *mut Raw, *const u32, u32, *mut u32, *mut u32) -> i32,
    dispatch: unsafe extern "C" fn(Raw, *mut Raw, Raw, *const Frame, f32) -> i32,
}

const _: () = {
    assert!(size_of::<Ops>() == 9 * size_of::<Raw>());
    assert!(size_of::<ProviderVersion>() == 32);
    assert!(std::mem::offset_of!(ProviderVersion, major) == 16);
};

pub(super) struct Owner {
    ops: &'static Ops,
    sdk: Bindings,
    context: Raw,
    major: u32,
}

impl Owner {
    fn open(desc: &Open, ops: &'static Ops) -> Result<(Backend, Version), i32> {
        let enable_int8 = u32::from(desc.fsr_major == 4);
        let sdk = Bindings::open(
            desc,
            ops.load,
            |module, out| unsafe { (ops.bind)(module, desc, enable_int8, out) },
            ops.unbind,
        )?;
        let mut owner = Self {
            ops,
            sdk,
            context: null_mut(),
            major: desc.fsr_major,
        };
        let version = owner.select_version(desc)?;
        checked(unsafe { (ops.create)(owner.sdk.raw, desc, version.id, &mut owner.context) })?;
        if owner.context.is_null() {
            return Err(NOT_SUPPORTED);
        }
        let mut effective = 0;
        checked(unsafe {
            (ops.effective_version)(owner.sdk.raw, &mut owner.context, &mut effective)
        })?;
        if effective != version.id {
            return Err(NOT_SUPPORTED);
        }
        if let Some(log) = desc.log {
            unsafe { log(desc.log_user, version.name.as_ptr()) };
        }
        Ok((Backend::Fsr(owner), version))
    }

    fn select_version(&self, desc: &Open) -> Result<Version, i32> {
        let mut versions = [ProviderVersion::default(); MAX_VERSIONS];
        let mut count = 0;
        checked(unsafe {
            (self.ops.versions)(self.sdk.raw, desc.device, versions.as_mut_ptr(), &mut count)
        })?;
        let versions = versions.get(..count as usize).ok_or(NOT_SUPPORTED)?;
        let mut best: Option<&ProviderVersion> = None;
        for candidate in versions.iter().filter(|entry| {
            entry.major == self.major && (desc.version_id == 0 || entry.id == desc.version_id)
        }) {
            // IDs are opaque. Keep the first entry on ties, including a zero ID.
            if best.is_none_or(|best| (candidate.minor, candidate.patch) > (best.minor, best.patch))
            {
                best = Some(candidate);
            }
        }
        if let Some(best) = best.filter(|best| best.id != 0) {
            let name = unsafe { CStr::from_ptr(best.name) };
            return Ok(Version::new(best.id, name.to_bytes()));
        }
        if let Some(log) = desc.log {
            if let Ok(message) = CString::new(format!(
                "FSR{} unavailable: runtime exposes no matching provider on this device ({count} available)",
                self.major
            )) {
                unsafe { log(desc.log_user, message.as_ptr()) };
            }
            const PREFIX: &[u8] = b"FSR available provider: ";
            for version in versions {
                let name = if version.name.is_null() {
                    c"unnamed"
                } else {
                    unsafe { CStr::from_ptr(version.name) }
                };
                let mut message = [0; 192];
                copy_bytes(&mut message[..PREFIX.len()], PREFIX);
                copy_bytes(&mut message[PREFIX.len()..191], name.to_bytes());
                unsafe { log(desc.log_user, message.as_ptr()) };
            }
        }
        Err(NOT_SUPPORTED)
    }

    pub(super) fn plan(
        &mut self,
        output: &Extent,
        quality: u32,
        width: *mut u32,
        height: *mut u32,
    ) -> i32 {
        if quality > ULTRA_PERFORMANCE {
            return INVALID;
        }
        // FSR2 can dispatch 1:1 but its quality query does not accept Native AA.
        if quality == 0 && self.major == 2 {
            unsafe {
                width.write(output[0]);
                height.write(output[1]);
            }
            return OK;
        }
        unsafe {
            (self.ops.plan)(
                self.sdk.raw,
                &mut self.context,
                output.as_ptr(),
                quality,
                width,
                height,
            )
        }
    }

    pub(super) fn evaluate(&mut self, commands: Raw, frame: &Frame) -> i32 {
        let far = unsafe { (*frame.record).camera.far_plane };
        let far = if far > 0.0 { far } else { f32::MAX };
        unsafe { (self.ops.dispatch)(self.sdk.raw, &mut self.context, commands, frame, far) }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe { (self.ops.destroy)(self.sdk.raw, &mut self.context) };
        }
        // Bindings drops the INT8 compatibility lease before unloading the DLL.
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d12_sr_provider_fsr_open(
    desc: &Open,
    out: *mut Raw,
    ops: &'static Ops,
) -> i32 {
    if !(2..=4).contains(&desc.fsr_major) {
        return INVALID;
    }
    open_session(desc, out, |_| Owner::open(desc, ops))
}
