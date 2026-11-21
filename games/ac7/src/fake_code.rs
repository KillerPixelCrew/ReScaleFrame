// SPDX-License-Identifier: MIT
//! A sparse code image for the site and patch tests. Addresses are plain numbers; nothing is
//! dereferenced.
use crate::sites::Code;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Image {
    bytes: RefCell<HashMap<usize, u8>>,
    pub(crate) log: RefCell<Vec<String>>,
    pub(crate) writes: Cell<u32>,
    /// Address whose write fails, as a refused code protection change would.
    pub(crate) protected: Cell<Option<usize>>,
}

impl Image {
    pub(crate) fn put(&self, address: usize, bytes: &[u8]) {
        let mut image = self.bytes.borrow_mut();
        for (offset, byte) in bytes.iter().enumerate() {
            image.insert(address + offset, *byte);
        }
    }

    pub(crate) fn has(&self, address: usize, bytes: &[u8]) -> bool {
        let image = self.bytes.borrow();
        bytes
            .iter()
            .enumerate()
            .all(|(offset, byte)| image.get(&(address + offset)) == Some(byte))
    }

    pub(crate) fn logged(&self, message: &str) -> bool {
        self.log.borrow().iter().any(|line| line == message)
    }
}

impl Code for Image {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        self.has(address.addr(), expected)
    }

    fn write(&self, site: *mut u8, expected: &[u8], replacement: &[u8]) -> bool {
        if expected.is_empty()
            || expected.len() != replacement.len()
            || self.protected.get() == Some(site.addr())
            || !self.has(site.addr(), expected)
        {
            return false;
        }
        self.put(site.addr(), replacement);
        self.writes.set(self.writes.get() + 1);
        true
    }

    fn log(&self, message: &str) {
        self.log.borrow_mut().push(message.to_owned());
    }
}
