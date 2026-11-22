// SPDX-License-Identifier: MIT
//! WGL exports and EXT memory/semaphore operations on the current context.
use super::{GlProcs, Handle};
use std::ffi::{c_char, c_void};
use std::ptr::null;
use windows_sys::Win32::Graphics::OpenGL::*;

const NUM_EXTENSIONS: u32 = 0x821d;
const DEVICE_LUID: u32 = 0x9599;
const MEMORY_DEDICATED: u32 = 0x9581;
const D3D12_RESOURCE: u32 = 0x958a;
const D3D12_FENCE: u32 = 0x9594;
const FENCE_VALUE: u32 = 0x9595;
const CLAMP_TO_EDGE: i32 = 0x812f;

macro_rules! entry {
    ($gl:expr, $index:literal, $signature:ty) => {
        {
            let table = &*$gl;
            std::mem::transmute::<*const c_void, $signature>(table.0[$index])
        }
    };
}

pub(super) extern "C" fn current() -> Handle {
    unsafe { wglGetCurrentContext() }
}

pub(super) unsafe extern "C" fn lookup(name: *const c_char) -> *const c_void {
    unsafe { wglGetProcAddress(name.cast()) }.map_or(null(), |proc| proc as *const c_void)
}

pub(super) extern "C" fn extension_count() -> i32 {
    let mut count = 0;
    unsafe { glGetIntegerv(NUM_EXTENSIONS, &mut count) };
    count
}

pub(super) unsafe extern "C" fn extension_at(gl: *const GlProcs, index: u32) -> *const c_char {
    let string_index = unsafe { entry!(gl, 0, unsafe extern "system" fn(u32, u32) -> *const u8) };
    unsafe { string_index(GL_EXTENSIONS, index) }.cast()
}

pub(super) extern "C" fn error() -> u32 {
    unsafe { glGetError() }
}

pub(super) unsafe extern "C" fn luid(gl: *const GlProcs, luid: *mut u8) {
    let query = unsafe { entry!(gl, 1, unsafe extern "system" fn(u32, *mut u8)) };
    unsafe { query(DEVICE_LUID, luid) };
}

pub(super) unsafe extern "C" fn semaphore_import(gl: *const GlProcs, handle: Handle, name: *mut u32) -> u32 {
    unsafe {
        let generate = entry!(gl, 7, unsafe extern "system" fn(i32, *mut u32));
        let import = entry!(gl, 9, unsafe extern "system" fn(u32, u32, Handle));
        generate(1, name);
        import(*name, D3D12_FENCE, handle);
        glGetError()
    }
}

pub(super) unsafe extern "C" fn semaphore_delete(gl: *const GlProcs, name: u32) {
    unsafe { entry!(gl, 8, unsafe extern "system" fn(i32, *const u32))(1, &name) };
}

pub(super) unsafe extern "C" fn memory_import(gl: *const GlProcs, handle: Handle, memory: *mut u32) -> u32 {
    unsafe {
        let create = entry!(gl, 2, unsafe extern "system" fn(i32, *mut u32));
        let parameter = entry!(gl, 4, unsafe extern "system" fn(u32, u32, *const i32));
        let import = entry!(gl, 5, unsafe extern "system" fn(u32, u64, u32, Handle));
        create(1, memory);
        parameter(*memory, MEMORY_DEDICATED, &(GL_TRUE as i32));
        import(*memory, 0, D3D12_RESOURCE, handle);
        glGetError()
    }
}

pub(super) unsafe extern "C" fn memory_delete(gl: *const GlProcs, name: u32) {
    unsafe { entry!(gl, 3, unsafe extern "system" fn(i32, *const u32))(1, &name) };
}

pub(super) unsafe extern "C" fn texture_store(
    gl: *const GlProcs, memory: u32, format: u32, width: u32, height: u32, texture: *mut u32,
) -> u32 {
    let (Ok(width), Ok(height)) = (i32::try_from(width), i32::try_from(height)) else {
        return GL_INVALID_VALUE;
    };
    let mut previous = 0;
    unsafe {
        let store = entry!(gl, 6, unsafe extern "system" fn(u32, i32, u32, i32, i32, u32, u64));
        glGetIntegerv(GL_TEXTURE_BINDING_2D, &mut previous);
        glGenTextures(1, texture);
        glBindTexture(GL_TEXTURE_2D, *texture);
        store(GL_TEXTURE_2D, 1, format, width, height, memory, 0);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST as i32);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST as i32);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, CLAMP_TO_EDGE);
        glBindTexture(GL_TEXTURE_2D, previous as u32);
        glGetError()
    }
}

pub(super) extern "C" fn texture_delete(texture: u32) {
    unsafe { glDeleteTextures(1, &texture) };
}

type Transfer = unsafe extern "system" fn(u32, u32, *const u32, u32, *const u32, *const u32);

unsafe fn set_fence(gl: *const GlProcs, semaphore: u32, value: u64) -> u32 {
    unsafe {
        entry!(gl, 10, unsafe extern "system" fn(u32, u32, *const u64))(semaphore, FENCE_VALUE, &value);
        glGetError()
    }
}

pub(super) unsafe extern "C" fn release(
    gl: *const GlProcs, semaphore: u32, value: u64, count: u32, textures: *const u32, layouts: *const u32,
) -> u32 {
    unsafe {
        let status = set_fence(gl, semaphore, value);
        if status != GL_NO_ERROR {
            return status;
        }
        entry!(gl, 11, Transfer)(semaphore, 0, null(), count, textures, layouts);
        let status = glGetError();
        if status != GL_NO_ERROR {
            return status;
        }
        glFlush();
        glGetError()
    }
}

pub(super) unsafe extern "C" fn acquire(
    gl: *const GlProcs, semaphore: u32, value: u64, count: u32, textures: *const u32, layouts: *const u32,
) -> u32 {
    unsafe {
        let status = set_fence(gl, semaphore, value);
        if status != GL_NO_ERROR {
            return status;
        }
        entry!(gl, 12, Transfer)(semaphore, 0, null(), count, textures, layouts);
        glGetError()
    }
}

pub(super) extern "C" fn finish() {
    unsafe { glFinish() };
}
