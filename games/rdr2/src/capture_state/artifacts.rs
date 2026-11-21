// SPDX-License-Identifier: MIT
//! Names and writes completed image readbacks and copied camera records.
use super::recordings::Pipeline;
use std::ffi::{CStr, OsString, c_char};
use std::fmt;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::slice;

pub(super) fn output_directory(directory: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(directory))
}

fn input_stem(pid: u32, sequence: u32, index: u32, role: &str) -> String {
    format!("input-{pid}-{sequence:03}-{index:03}-{role}")
}

impl fmt::Display for Pipeline {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            concat!(
                "\"ps_crc32\":\"{:08x}\",\"vs_crc32\":\"{:08x}\",\"blend\":{},",
                "\"src\":{},\"dst\":{},\"op\":{},\"src_alpha\":{},\"dst_alpha\":{},\"op_alpha\":{},\"write_mask\":{}"
            ),
            self.ps,
            self.vs,
            self.blend,
            self.src,
            self.dst,
            self.op,
            self.src_alpha,
            self.dst_alpha,
            self.op_alpha,
            self.write_mask,
        )
    }
}

#[repr(C)]
struct Readback {
    list: u64,
    generation: u64,
    submission: u64,
    queue: u64,
    fence_value: u64,
    tick: u64,
    resource: u64,
    caller: u64,
    sequence: u32,
    ordinal: u32,
    width: u32,
    height: u32,
    pitch: u32,
    format: u32,
    input_index: u32,
    footprint_format: u32,
    state_before: u32,
    subresource: u32,
    frame: u32,
    vertices: u32,
    instances: u32,
    pipeline: Pipeline,
}

const _: () = assert!(size_of::<Readback>() == 160);

impl Readback {
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
                "\"vertices\":{},\"instances\":{},{}}}\n"
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
            self.pipeline,
        )
    }
}

/// The caller lends the directory array, known role string and mapped pixels for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_write_image(
    image: &Readback,
    directory: *const u16,
    directory_len: usize,
    pid: u32,
    role: *const c_char,
    data: *const u8,
    bytes: usize,
) -> bool {
    if data.is_null()
        || bytes == 0
        || bytes > isize::MAX as usize
        || (image.pitch as usize).checked_mul(image.height as usize) != Some(bytes)
    {
        return false;
    }
    let Ok(role) = (unsafe { CStr::from_ptr(role) }).to_str() else {
        return false;
    };
    // A role is an ASCII word used in both a filename and a JSON string.
    if role.is_empty() || !role.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return false;
    }
    let directory = unsafe { slice::from_raw_parts(directory, directory_len) };
    let mut path = output_directory(directory).join(image.file_stem(pid, role));
    let pixels = unsafe { slice::from_raw_parts(data, bytes) };
    path.set_extension("bin");
    if fs::write(&path, pixels).is_err() {
        return false;
    }
    path.set_extension("json");
    fs::write(path, image.metadata(role)).is_ok()
}

/// The caller lends the directory units and copied camera bytes for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_write_camera(
    directory: *const u16,
    directory_len: usize,
    pid: u32,
    sequence: u32,
    index: u32,
    data: *const u8,
    bytes: usize,
) -> bool {
    let directory = unsafe { slice::from_raw_parts(directory, directory_len) };
    let mut path = output_directory(directory).join(input_stem(pid, sequence, index, "camera"));
    path.set_extension("bin");
    let camera = unsafe { slice::from_raw_parts(data, bytes) };
    fs::write(path, camera).is_ok()
}
