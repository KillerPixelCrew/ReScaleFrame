// SPDX-License-Identifier: MIT
//! Names and writes completed image readbacks and copied camera records.
#[cfg(not(test))]
use super::metadata::InputId;
use super::recordings::Pipeline;
use super::shots::Image;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

pub(super) fn output_directory(directory: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(directory))
}

fn input_stem(pid: u32, sequence: u32, index: u32, role: &str) -> String {
    format!("input-{pid}-{sequence:03}-{index:03}-{role}")
}

pub(super) fn pipeline_fields(pipeline: &Pipeline, known: bool) -> impl fmt::Display + '_ {
    fmt::from_fn(move |out| {
        if !known {
            return out.write_str("\"pipeline_known\":false");
        }
        write!(
            out,
            concat!(
                "\"pipeline_known\":true,\"ps_crc32\":\"{:08x}\",\"vs_crc32\":\"{:08x}\",\"blend\":{},",
                "\"src\":{},\"dst\":{},\"op\":{},\"src_alpha\":{},\"dst_alpha\":{},\"op_alpha\":{},\"write_mask\":{}"
            ),
            pipeline.ps,
            pipeline.vs,
            pipeline.blend,
            pipeline.src,
            pipeline.dst,
            pipeline.op,
            pipeline.src_alpha,
            pipeline.dst_alpha,
            pipeline.op_alpha,
            pipeline.write_mask,
        )
    })
}

impl Image {
    fn file_stem(&self, pid: u32, role: &str) -> String {
        if role == "draw" {
            format!("native-{pid}-{:03}-{:03}", self.sequence, self.ordinal)
        } else {
            input_stem(pid, self.sequence, self.input_index, role)
        }
    }

    fn metadata(&self, role: &str) -> String {
        format!(
            concat!(
                "{{\"sequence\":{},\"draw\":{},\"width\":{},\"height\":{},\"row_pitch\":{},\"format\":{},",
                "\"role\":\"{}\",\"input_index\":{},\"footprint_format\":{},\"state_before\":{},\"subresource\":{},",
                "\"command_list\":\"0x{:x}\",\"generation\":{},\"submission\":{},\"queue\":\"0x{:x}\",\"fence_value\":{},",
                "\"frame_counter_observation\":{},\"tick\":{},\"resource\":\"0x{:x}\",\"caller\":\"0x{:x}\",",
                "\"vertices\":{},\"instances\":{},\"pso\":\"0x{:x}\",{}}}\n"
            ),
            self.sequence,
            self.ordinal,
            self.width,
            self.height,
            self.pitch,
            self.format,
            role,
            self.input_index,
            self.footprint_format,
            self.state_before,
            self.subresource,
            self.list,
            self.generation,
            self.submission,
            self.queue,
            self.fence_value,
            self.frame,
            self.tick,
            self.resource,
            self.caller,
            self.vertices,
            self.instances,
            self.pso,
            pipeline_fields(&self.pipeline, self.pipeline_known),
        )
    }
}

/// Writes the mapped pixels and their metadata. The role is a filename and JSON word.
pub(super) fn write_image(
    image: &Image,
    directory: &Path,
    pid: u32,
    role: &str,
    pixels: &[u8],
) -> bool {
    let mut path = directory.join(image.file_stem(pid, role));
    path.set_extension("bin");
    if fs::write(&path, pixels).is_err() {
        return false;
    }
    path.set_extension("json");
    fs::write(path, image.metadata(role)).is_ok()
}

#[cfg(not(test))]
pub(super) fn write_camera(directory: &Path, pid: u32, id: InputId, camera: &[u8]) -> bool {
    let mut path = directory.join(input_stem(pid, id.sequence, id.index, "camera"));
    path.set_extension("bin");
    fs::write(path, camera).is_ok()
}
