// SPDX-License-Identifier: MIT
//! Identity bindings for an SR12 context's three recording slots.
use core::{ffi::c_void, ptr::null_mut};

#[derive(Clone, Copy)]
pub struct RecordingSlots([*mut c_void; 3]);

impl RecordingSlots {
    pub const EMPTY: Self = Self([null_mut(); 3]);

    pub fn bind(&mut self, commands: *mut c_void) -> Option<u32> {
        if commands.is_null() {
            return None;
        }
        if let Some(index) = self.0.iter().position(|&list| list == commands) {
            return Some(index as u32);
        }
        let (index, free) = self
            .0
            .iter_mut()
            .enumerate()
            .find(|(_, list)| list.is_null())?;
        *free = commands;
        Some(index as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::RecordingSlots;
    use core::{ffi::c_void, ptr::null_mut};

    #[test]
    fn bindings_reuse_the_original_slot_and_refuse_exhaustion() {
        let mut slots = RecordingSlots::EMPTY;
        let list = |id: usize| id as *mut c_void;
        assert_eq!(slots.bind(null_mut()), None);
        assert_eq!(slots.bind(list(1)), Some(0));
        assert_eq!(slots.bind(list(2)), Some(1));
        assert_eq!(slots.bind(list(1)), Some(0));
        assert_eq!(slots.bind(list(3)), Some(2));
        assert_eq!(slots.bind(list(4)), None);
        assert_eq!(slots.bind(list(3)), Some(2));
        slots = RecordingSlots::EMPTY;
        assert_eq!(slots.bind(list(4)), Some(0));
    }
}
