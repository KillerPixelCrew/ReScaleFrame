// SPDX-License-Identifier: MIT
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read as _;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;

const FILE_SHARE_READ: u32 = 1;
const MAX_FILE: u64 = 1 << 28;
use rsf_game_sys::pe32_plus::Pe32Plus;

pub(super) struct ModuleFile {
    bytes: Vec<u8>,
}

impl ModuleFile {
    pub(super) fn open(path: &[u16]) -> Result<Self, &'static str> {
        let path = OsString::from_wide(path);
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
            .map_err(|_| "could not be opened")?;
        let length = file.metadata().map_err(|_| "could not be read")?.len();
        if !(1..MAX_FILE).contains(&length) {
            return Err("could not be read");
        }
        let length = length as usize;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| "could not be read")?;
        bytes.resize(length, 0);
        file.read_exact(&mut bytes)
            .map_err(|_| "could not be read")?;
        Pe32Plus::parse(&bytes).ok_or("has no usable headers")?;
        Ok(Self { bytes })
    }

    pub(super) fn preferred_base(&self) -> u64 {
        self.image().preferred_base()
    }

    pub(super) fn span(&self, rva: usize, count: usize) -> Option<&[u8]> {
        self.image().span(rva, count).map(|(bytes, _)| bytes)
    }

    fn image(&self) -> Pe32Plus<'_> {
        Pe32Plus::parse(&self.bytes).expect("validated module file")
    }
}
