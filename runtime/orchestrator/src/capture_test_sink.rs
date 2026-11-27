// SPDX-License-Identifier: MIT
use crate::capture_pixels::Pixels;
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::{CStr, c_char};

type Reserved = (u64, String, String, u32, f32);
type Published = (u64, String, u32, u32, u64, Vec<u8>);

#[derive(Default)]
pub(super) struct Sink {
    pub reserve_none: bool,
    pub next_ticket: u64,
    pub reserved: Vec<Reserved>,
    pub identities: Vec<(u64, u64, u32, u32)>,
    pub failed: Vec<(u64, String, i32)>,
    pub published: Vec<Published>,
    pub storage: Vec<(u32, u32, u32, u32)>,
}

unsafe fn text(value: *const c_char) -> String {
    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

impl Sink {
    pub unsafe fn reserve(
        &mut self,
        record: *const Record,
        name: *const c_char,
        description: *const c_char,
        view: u32,
        scale: f32,
    ) -> u64 {
        if self.reserve_none {
            return 0;
        }
        let (record, name, description) = unsafe { (&*record, text(name), text(description)) };
        self.next_ticket += 1;
        self.identities.push((
            record.session_id,
            record.frame_id,
            record.view_id,
            record.resource_generation,
        ));
        self.reserved
            .push((self.next_ticket, name, description, view, scale));
        self.next_ticket
    }

    pub unsafe fn failed(&mut self, ticket: u64, name: *const c_char, result: i32) {
        self.failed.push((ticket, unsafe { text(name) }, result));
    }

    pub unsafe fn pixels(
        &mut self,
        ticket: u64,
        name: *const c_char,
        pixels: *const Pixels,
        api: u32,
        native: u32,
    ) -> i32 {
        let (name, pixels) = unsafe { (text(name), &*pixels) };
        assert_eq!(pixels.struct_size as usize, size_of::<Pixels>());
        let bytes = unsafe {
            std::slice::from_raw_parts(pixels.data.cast::<u8>(), pixels.data_size as usize)
        }
        .to_vec();
        self.storage
            .push((api, native, pixels.width, pixels.height));
        self.published.push((
            ticket,
            name,
            pixels.format,
            pixels.row_pitch,
            pixels.data_size,
            bytes,
        ));
        1
    }
}
