// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use std::ptr::{null, null_mut};

pub type Handle = *mut c_void;

pub struct Com(pub Handle);

impl Drop for Com {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let release: unsafe extern "system" fn(Handle) -> u32 =
                unsafe { std::mem::transmute(slot(self.0, 2)) };
            unsafe { release(self.0) };
        }
    }
}

pub unsafe fn slot(object: Handle, index: usize) -> Handle {
    let table = unsafe { *object.cast::<*const Handle>() };
    unsafe { *table.add(index) }
}

unsafe extern "system" {
    fn D3D11CreateDevice(
        adapter: Handle,
        driver: u32,
        software: Handle,
        flags: u32,
        levels: *const u32,
        level_count: u32,
        sdk: u32,
        device: *mut Handle,
        feature_level: *mut u32,
        context: *mut Handle,
    ) -> i32;
}

pub fn create_device(driver: u32) -> Option<(Com, Com)> {
    let mut device = Com(null_mut());
    let mut context = Com(null_mut());
    let result = unsafe {
        D3D11CreateDevice(
            null_mut(),
            driver,
            null_mut(),
            0,
            null(),
            0,
            7,
            &mut device.0,
            null_mut(),
            &mut context.0,
        )
    };
    (result >= 0 && !device.0.is_null() && !context.0.is_null()).then_some((device, context))
}

#[repr(C)]
#[derive(Default)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub array_size: u32,
    pub format: u32,
    pub samples: u32,
    pub sample_quality: u32,
    pub usage: u32,
    pub bind_flags: u32,
    pub cpu_access_flags: u32,
    pub misc_flags: u32,
}

pub unsafe fn create_texture(device: Handle, desc: &TextureDesc) -> Com {
    type Create =
        unsafe extern "system" fn(Handle, *const TextureDesc, *const c_void, *mut Handle) -> i32;
    let create: Create = unsafe { std::mem::transmute(slot(device, 5)) };
    let mut texture = Com(null_mut());
    unsafe { create(device, desc, null(), &mut texture.0) };
    texture
}

pub struct Checks(pub bool);

impl Checks {
    pub fn check(&mut self, passed: bool, message: &str) {
        if !passed {
            eprintln!("{message}");
            self.0 = false;
        }
    }

    pub fn status(&self, fixture: &str) -> i32 {
        eprintln!("{fixture}: {}", if self.0 { "pass" } else { "fail" });
        i32::from(!self.0)
    }
}

pub fn stage(message: &str) {
    eprintln!("[stage] {message}");
}
