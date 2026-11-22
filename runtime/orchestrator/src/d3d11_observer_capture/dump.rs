// SPDX-License-Identifier: MIT
use super::{Constant, DumpResult, Handle, OK, Request};
use crate::input_dump::{
    DECODED_MOTION, ENCODED_MOTION, Log, MotionEncoding, MotionTextureOptions, TextureOptions,
};
use crate::sys::rsf_backend_log_fn;
use crate::sys::rsf_observer_options;
use crate::sys::{RSF_MOTION_DECODE_ABI_VERSION, rsf_motion_decode_setup as DecodeSetup};
use rsf_game_sys::rsf_motion_decode_params as DecodeParams;
use std::ffi::{CStr, CString};
use std::fs;
use std::ptr::{NonNull, null_mut};
use windows::core::{IUnknown, Interface};

const FOREIGN_CONSTANT: i32 = -6;

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

    pub(super) fn new(options: &rsf_observer_options, log: rsf_backend_log_fn) -> Self {
        Self {
            log,
            log_user: null_mut(),
            decode_motion: options.decode_motion,
            scale: options.motion_scale,
            bias: options.motion_bias,
            invalid: options.motion_invalid_value,
        }
    }
}

unsafe extern "C" {
    fn rsf_observer11_guard_texture_extent(
        source: Handle,
        width: &mut u32,
        height: &mut u32,
    ) -> i32;
    fn rsf_dump_texture(
        device: Handle,
        context: Handle,
        source: Handle,
        options: *const TextureOptions,
        info: Handle,
    ) -> i32;
    fn rsf_read_constant_buffer(
        device: Handle,
        context: Handle,
        buffer: Handle,
        out: Handle,
        bytes: u32,
    ) -> i32;
    fn rsf_motion_decode_create(device: Handle, setup: &DecodeSetup, out: &mut Handle) -> i32;
    fn rsf_motion_decode_run(
        pass: Handle,
        context: Handle,
        source: Handle,
        params: &DecodeParams,
    ) -> i32;
    fn rsf_motion_decode_texture(pass: Handle) -> Handle;
    fn rsf_motion_decode_destroy(pass: Handle);
}

struct Decode(NonNull<std::ffi::c_void>);

impl Drop for Decode {
    fn drop(&mut self) {
        unsafe { rsf_motion_decode_destroy(self.0.as_ptr()) };
    }
}

struct Writer<'a> {
    settings: &'a DumpSettings,
    log: Log,
    device: Handle,
    context: Handle,
}

impl Writer<'_> {
    fn texture(&self, source: Handle, prefix: &CStr, view: u32) -> i32 {
        let options = TextureOptions::new(prefix, view, 0.0, self.log);
        if view == ENCODED_MOTION && self.settings.decode_motion != 0 {
            let extended = MotionTextureOptions::new(
                options,
                MotionEncoding {
                    scale: [self.settings.scale; 2],
                    bias: [self.settings.bias; 2],
                    zero_x_unwritten: 1,
                },
            );
            unsafe {
                rsf_dump_texture(
                    self.device,
                    self.context,
                    source,
                    std::ptr::from_ref(&extended).cast(),
                    null_mut(),
                )
            }
        } else {
            unsafe { rsf_dump_texture(self.device, self.context, source, &options, null_mut()) }
        }
    }

    fn decoded(&self, source: Handle, prefix: &str) {
        let (mut width, mut height) = (0, 0);
        if unsafe { rsf_observer11_guard_texture_extent(source, &mut width, &mut height) } == 0 {
            self.log
                .say("motion decode unavailable, texture description refused");
            return;
        }
        let setup = DecodeSetup {
            struct_size: size_of::<DecodeSetup>() as u32,
            abi_version: RSF_MOTION_DECODE_ABI_VERSION,
            width,
            height,
            output_format: 0,
            log: self.settings.log,
            log_user: self.settings.log_user,
        };
        let mut handle = null_mut();
        let result = unsafe { rsf_motion_decode_create(self.device, &setup, &mut handle) };
        let pass = NonNull::new(handle).map(Decode);
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
            unsafe { rsf_motion_decode_run(pass.0.as_ptr(), self.context, source, &params) };
        if result != OK {
            self.log
                .say(&format!("motion decode refused, result {result}"));
            return;
        }
        let decoded = unsafe { rsf_motion_decode_texture(pass.0.as_ptr()) };
        let path = CString::new(format!("{prefix}_decoded")).expect("prefix is a C string");
        self.texture(decoded, &path, DECODED_MOTION);
    }

    fn constant(&self, entry: &Constant, prefix: &str) -> u32 {
        let bytes = entry.bytes;
        self.log.say(&format!("constant buffer of {bytes} bytes"));
        let mut contents = vec![0u8; bytes as usize];
        let read = unsafe {
            rsf_read_constant_buffer(
                self.device,
                self.context,
                entry.buffer.as_raw(),
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
    settings: &DumpSettings,
    request: &Request,
    device: &IUnknown,
    context: Option<&IUnknown>,
    textures: &[IUnknown],
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
                settings,
                log,
                device: device.as_raw(),
                context: context.as_raw(),
            };
            for (index, texture) in textures.iter().enumerate() {
                let path = format!("{prefix}_{index}");
                let native_path = CString::new(path.as_str()).expect("prefix is a C string");
                let number = index + 1;
                log.say(&format!("texture {number} of {}", textures.len()));
                let written = writer.texture(texture.as_raw(), &native_path, request.view);
                if written == OK {
                    result.textures += 1;
                } else {
                    log.say(&format!("texture {number} refused, result {written}"));
                }
                if settings.decode_motion != 0 {
                    writer.decoded(texture.as_raw(), &path);
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
