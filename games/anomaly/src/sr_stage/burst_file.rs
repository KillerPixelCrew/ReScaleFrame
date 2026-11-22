// SPDX-License-Identifier: MIT
use super::staging::{Mapping, map as map_staging};
use super::{GpuLease, Platform};
use std::ffi::c_void;
use std::fs::{File, remove_file};
use std::io::{self, BufWriter, Write};
use std::path::Path;

pub(super) fn write_new(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<()> {
    let mut file = BufWriter::new(File::create_new(path)?);
    let result = write(&mut file).and_then(|()| file.flush());
    drop(file);
    if result.is_err() {
        let _ = remove_file(path);
    }
    result
}

fn write_rows(path: &Path, mapping: &Mapping<'_>) -> bool {
    let Some(rows) = mapping.rows() else {
        return false;
    };
    write_new(path, |file| {
        for bytes in rows {
            file.write_all(bytes)?;
        }
        Ok(())
    })
    .is_ok()
}

// Maps one held staging copy, writes its packed rows and unmaps it, even when the write fails.
pub(super) fn staging(
    platform: &Platform,
    context: *mut c_void,
    texture: &Option<GpuLease>,
    path: &Path,
) -> (bool, u32, u32, u32) {
    let Some(texture) = texture else {
        return (false, 0, 0, 0);
    };
    let (layout, mapping) = map_staging(platform, context, texture);
    let written = mapping.is_some_and(|mapping| write_rows(path, &mapping));
    (written, layout.width, layout.height, layout.format)
}
