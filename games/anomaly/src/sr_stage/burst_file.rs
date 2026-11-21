// SPDX-License-Identifier: MIT
use super::{GpuLease, Platform};
use crate::sr_stage_abi::rsf_anomaly_sr_mapped_texture as Texture;
use std::ffi::c_void;
use std::fs::{File, remove_file};
use std::io::{self, BufWriter, Write};
use std::path::Path;

struct Mapping {
    context: *mut c_void,
    texture: *mut c_void,
    unmap: unsafe extern "C" fn(*mut c_void, *mut c_void),
    layout: Texture,
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe { (self.unmap)(self.context, self.texture) };
    }
}

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

fn write_rows(path: &Path, mapping: &Mapping) -> bool {
    let texture = &mapping.layout;
    let Some(row) = texture.width.checked_mul(texture.pixel_bytes) else {
        return false;
    };
    if texture.data.is_null() || row == 0 || texture.height == 0 || texture.row_pitch < row {
        return false;
    }
    let span = u64::from(texture.row_pitch) * u64::from(texture.height - 1) + u64::from(row);
    if span > isize::MAX as u64 || texture.data.addr().checked_add(span as usize).is_none() {
        return false;
    }
    write_new(path, |file| {
        for y in 0..texture.height {
            let offset = y as usize * texture.row_pitch as usize;
            // The native map lends these rows until Unmap. Padding is skipped.
            let bytes = unsafe {
                std::slice::from_raw_parts(texture.data.cast::<u8>().add(offset), row as usize)
            };
            file.write_all(bytes)?;
        }
        Ok(())
    })
    .is_ok()
}

pub(super) fn staging(
    platform: &Platform,
    context: *mut c_void,
    texture: &Option<GpuLease>,
    path: &Path,
) -> (bool, u32, u32, u32) {
    let (Some(texture), Some(map), Some(unmap)) =
        (texture, platform.map_staging, platform.unmap_staging)
    else {
        return (false, 0, 0, 0);
    };
    let texture = texture.0.as_ptr();
    let mut mapped = Texture::default();
    let is_mapped = unsafe { map(context, texture, &mut mapped) };
    let (width, height, format) = (mapped.width, mapped.height, mapped.format);
    let written = if is_mapped {
        let mapping = Mapping {
            context,
            texture,
            unmap,
            layout: mapped,
        };
        write_rows(path, &mapping)
    } else {
        false
    };
    (written, width, height, format)
}
