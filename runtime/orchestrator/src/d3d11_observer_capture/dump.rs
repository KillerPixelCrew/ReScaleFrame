// SPDX-License-Identifier: MIT
use super::{Constant, DumpResult, Handle, Immediate, OK, Request};
use crate::com_ref::ComRef;
use crate::input_dump::{DECODED_MOTION, Log, TextureOptions};
use crate::render_abi::rsf_backend_log_fn;
use rsf_game_sys::sr_abi::rsf_motion_decode_params as DecodeParams;
use std::ffi::{CStr, CString};
use std::fs;
use std::ptr::{NonNull, null_mut};

const FOREIGN_CONSTANT: i32 = -6;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct DumpSettings {
    log: rsf_backend_log_fn,
    log_user: Handle,
    decode_motion: u32,
    scale: f32,
    bias: f32,
    invalid: f32,
}

impl DumpSettings {
    pub(super) const EMPTY: Self = Self {
        log: None,
        log_user: null_mut(),
        decode_motion: 0,
        scale: 0.0,
        bias: 0.0,
        invalid: 0.0,
    };
}

#[repr(C)]
pub(super) struct DumpPlatform {
    pub(super) immediate: Immediate,
    texture: unsafe extern "C" fn(Handle, Handle, Handle, &TextureOptions) -> i32,
    read_buffer: unsafe extern "C" fn(Handle, Handle, Handle, Handle, u32) -> i32,
    create_decode:
        unsafe extern "C" fn(Handle, Handle, rsf_backend_log_fn, Handle, *mut Handle) -> i32,
    decode: unsafe extern "C" fn(Handle, Handle, Handle, &DecodeParams) -> i32,
    decoded_texture: unsafe extern "C" fn(Handle) -> Handle,
    destroy_decode: unsafe extern "C" fn(Handle),
}

const _: () = {
    assert!(size_of::<DumpSettings>() == 32);
    assert!(size_of::<DumpPlatform>() == 7 * size_of::<Handle>());
};

struct Decode {
    handle: NonNull<std::ffi::c_void>,
    destroy: unsafe extern "C" fn(Handle),
}

impl Drop for Decode {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.handle.as_ptr()) };
    }
}

struct Writer<'a> {
    platform: &'a DumpPlatform,
    settings: &'a DumpSettings,
    log: Log,
    device: Handle,
    context: Handle,
}

impl Writer<'_> {
    fn texture(&self, source: Handle, prefix: &CStr, view: u32) -> i32 {
        let options = TextureOptions::new(prefix, view, 0.0, self.log);
        unsafe { (self.platform.texture)(self.device, self.context, source, &options) }
    }

    fn decoded(&self, source: Handle, prefix: &str) {
        let mut handle = null_mut();
        let result = unsafe {
            (self.platform.create_decode)(
                self.device,
                source,
                self.settings.log,
                self.settings.log_user,
                &mut handle,
            )
        };
        let pass = NonNull::new(handle).map(|handle| Decode {
            handle,
            destroy: self.platform.destroy_decode,
        });
        let Some(pass) = pass.filter(|_| result == OK) else {
            self.log
                .say(&format!("motion decode unavailable, result {result}"));
            return;
        };
        let params = DecodeParams {
            struct_size: size_of::<DecodeParams>() as u32,
            scale_x: self.settings.scale,
            scale_y: self.settings.scale,
            bias_x: self.settings.bias,
            bias_y: self.settings.bias,
            output_scale_x: 1.0,
            output_scale_y: 1.0,
            invalid_value: self.settings.invalid,
            zero_means_unwritten: 1,
            ..DecodeParams::default()
        };
        let result =
            unsafe { (self.platform.decode)(pass.handle.as_ptr(), self.context, source, &params) };
        if result != OK {
            self.log
                .say(&format!("motion decode refused, result {result}"));
            return;
        }
        let decoded = unsafe { (self.platform.decoded_texture)(pass.handle.as_ptr()) };
        let path = CString::new(format!("{prefix}_decoded")).expect("prefix is a C string");
        self.texture(decoded, &path, DECODED_MOTION);
    }

    fn constant(&self, entry: &Constant, prefix: &str) -> u32 {
        let bytes = entry.bytes;
        self.log.say(&format!("constant buffer of {bytes} bytes"));
        let mut contents = vec![0u8; bytes as usize];
        let read = unsafe {
            (self.platform.read_buffer)(
                self.device,
                self.context,
                entry.buffer.as_ptr(),
                contents.as_mut_ptr().cast(),
                bytes,
            )
        };
        if read == FOREIGN_CONSTANT {
            self.log
                .say("constant buffer skipped, it belongs to another device");
        }
        if read != OK {
            return 0;
        }
        let path = format!("{prefix}_cb{bytes}.bin");
        if let Err(error) = fs::write(&path, contents) {
            self.log
                .say(&format!("constant buffer write failed for {path}: {error}"));
            return 0;
        }
        bytes
    }
}

pub(super) fn write(
    platform: &DumpPlatform,
    settings: &DumpSettings,
    request: &Request,
    device: &ComRef,
    context: Option<&ComRef>,
    textures: &[ComRef],
    buffers: &[Constant],
) -> DumpResult {
    let log = Log::new(settings.log, settings.log_user);
    let mut result = DumpResult::default();
    log.say(&format!(
        "dump begin: {} textures, {} constant buffers",
        textures.len(),
        buffers.len()
    ));
    if let Some(context) = context {
        if let Ok(prefix) = request.prefix.to_str() {
            let writer = Writer {
                platform,
                settings,
                log,
                device: device.as_ptr(),
                context: context.as_ptr(),
            };
            for (index, texture) in textures.iter().enumerate() {
                let path = format!("{prefix}_{index}");
                let native_path = CString::new(path.as_str()).expect("prefix is a C string");
                let number = index + 1;
                log.say(&format!("texture {number} of {}", textures.len()));
                let written = writer.texture(texture.as_ptr(), &native_path, request.view);
                if written == OK {
                    result.textures += 1;
                } else {
                    log.say(&format!("texture {number} refused, result {written}"));
                }
                if settings.decode_motion != 0 {
                    writer.decoded(texture.as_ptr(), &path);
                }
            }
            for entry in buffers {
                result.constant_bytes = result
                    .constant_bytes
                    .wrapping_add(writer.constant(entry, prefix));
            }
        } else {
            log.say("dump abandoned: output prefix must be UTF-8");
        }
    } else {
        log.say("dump abandoned: no device context");
    }
    log.say(&format!(
        "dump end: {} textures, {} constant bytes",
        result.textures, result.constant_bytes
    ));
    result
}
