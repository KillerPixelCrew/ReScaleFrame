// SPDX-License-Identifier: MIT
use super::*;
use crate::engine_slots::rsf_anomaly_slot_set;
use crate::native_profile::RECORDED as NATIVE;
use crate::runtime_module::Module;
use std::ffi::{CStr, c_char};
use std::ptr::{copy_nonoverlapping, null_mut};
use windows_sys::Win32::System::LibraryLoader::{DONT_RESOLVE_DLL_REFERENCES, LoadLibraryExW};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
};

unsafe extern "system" fn replacement(_: *mut c_void) {}

struct Mapping(*mut u8);
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe { VirtualFree(self.0.cast(), 0, MEM_RELEASE) };
    }
}

/// The launcher supplies CRT argv and the native memory primitives.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_input_fixture(
    argc: i32,
    argv: *const *const c_char,
    read: Read,
    exchange: Exchange,
) -> i32 {
    if argc < 1 || argv.is_null() {
        return 1;
    }
    let args = unsafe { std::slice::from_raw_parts(argv, argc as usize) };
    let build = usize::from(
        args.get(1)
            .is_some_and(|arg| !arg.is_null() && unsafe { CStr::from_ptr(*arg) } == c"avx"),
    );
    let native = &NATIVE[build];
    let Some(profile) = RECORDED.get(build) else {
        return 1;
    };
    let path = if argc > 2 {
        let executable = native.executable.to_str().expect("ASCII executable name");
        format!("D:/Anomaly 1.5.3/bin/{executable}")
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let path = path.first().map_or(std::ptr::null(), std::ptr::from_ref);
    match contract(
        profile,
        native.image_size,
        native.device_frame_rva,
        read,
        exchange,
        path,
    ) {
        Ok(()) => 0,
        Err(reason) => {
            eprintln!("Anomaly input: {reason}");
            1
        }
    }
}

fn contract(
    profile: &InputProfile,
    image_size: u32,
    counter_rva: u32,
    read: Read,
    exchange: Exchange,
    path: *const u16,
) -> Result<(), String> {
    let mapped = if path.is_null() {
        None
    } else {
        let handle = unsafe { LoadLibraryExW(path, null_mut(), DONT_RESOLVE_DLL_REFERENCES) };
        let module =
            unsafe { Module::from_owned(handle) }.ok_or("mapping the shipped executable failed")?;
        Some((module, handle.cast::<u8>()))
    };
    let synthetic = if mapped.is_none() {
        let base = unsafe {
            VirtualAlloc(
                null_mut(),
                image_size as usize,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if base.is_null() {
            return Err("allocating the synthetic image failed".into());
        }
        Some(Mapping(base.cast()))
    } else {
        None
    };
    let base = mapped
        .as_ref()
        .map_or_else(|| synthetic.as_ref().unwrap().0, |m| m.1);
    let image = Image::new(base, image_size, read);
    let frame = &profile.sites[0];
    let slot = unsafe { base.add(profile.slot_rva as usize).cast::<*mut u8>() };
    let original = unsafe { base.add(frame.rva as usize) };
    let install = || {
        super::install(
            &image,
            profile,
            counter_rva,
            replacement as *mut c_void,
            exchange,
        )
    };

    if synthetic.is_some() {
        let hex = unsafe { CStr::from_ptr(profile.frame_code) }.to_bytes();
        let body: Vec<u8> = hex
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        unsafe {
            for site in &profile.sites {
                copy_nonoverlapping(
                    site.expected.as_ptr(),
                    base.add(site.rva as usize),
                    site.expected.len(),
                );
            }
            copy_nonoverlapping(body.as_ptr(), original, body.len());
            slot.write(original);
        }
        let rejects = |expected: &CStr| {
            install().is_err_and(|reason| unsafe { CStr::from_ptr(reason) } == expected)
        };
        for site in &profile.sites {
            let byte = unsafe { base.add(site.rva as usize) };
            unsafe { *byte ^= 1 };
            let refused = rejects(unsafe { CStr::from_ptr(site.name) });
            unsafe { *byte ^= 1 };
            if !refused {
                return Err(format!(
                    "incorrect result for {}",
                    unsafe { CStr::from_ptr(site.name) }.to_string_lossy()
                ));
            }
        }
        let tail = unsafe { original.add(body.len() - 1) };
        unsafe { *tail ^= 1 };
        let refused = rejects(c"CInput OnFrame complete body/vtable guard");
        unsafe { *tail ^= 1 };
        if !refused {
            return Err("changed OnFrame body reached installation".into());
        }
        unsafe { slot.write(base.add(profile.sites[2].rva as usize)) };
        let refused = rejects(c"CInput primary slot 0");
        unsafe { slot.write(original) };
        if !refused {
            return Err("changed primary slot reached installation".into());
        }
    }
    if let Err(reason) = install() {
        let message = unsafe { CStr::from_ptr(reason) }
            .to_string_lossy()
            .into_owned();
        if !rsf_anomaly_slot_set(INPUT as u32, false) {
            std::mem::forget(mapped);
            std::mem::forget(synthetic);
        }
        return Err(message);
    }
    let replaced = unsafe { slot.read() } != original;
    let disabled = rsf_anomaly_slot_set(INPUT as u32, false);
    let restored = unsafe { slot.read() } == original;
    let reenabled = rsf_anomaly_slot_set(INPUT as u32, true);
    let retired = rsf_anomaly_slot_set(INPUT as u32, false);
    if replaced
        && disabled
        && restored
        && reenabled
        && retired
        && unsafe { slot.read() } == original
    {
        Ok(())
    } else {
        // Keep the image mapped if the fixture could not prove restoration.
        std::mem::forget(mapped);
        std::mem::forget(synthetic);
        Err("slot install, restore, or re-enable failed".into())
    }
}
