// SPDX-License-Identifier: MIT
use crate::frame_capture_vk::general_precision;
use rsf_game_sys::rsf_frame_record;
use std::fmt::{self, Display, Write as _};
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::Path;

pub(super) struct Quoted<'a>(pub &'a str);

impl Display for Quoted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        for character in self.0.chars() {
            match character {
                '"' | '\\' => {
                    f.write_char('\\')?;
                    f.write_char(character)?;
                }
                '\u{0}'..='\u{1f}' => write!(f, "\\u{:04x}", u32::from(character))?,
                _ => f.write_char(character)?,
            }
        }
        f.write_char('"')
    }
}

pub(super) fn legacy_float(value: f32, precision: i32) -> String {
    if value.is_finite() {
        general_precision(f64::from(value), precision)
    } else {
        "null".into()
    }
}

/// `fields` follow the identity; `trailer` follows the camera matrices (the D3D12 legacy files
/// end with the SDK dispatch reset).
pub(super) fn legacy_json(
    record: &rsf_frame_record,
    role: &str,
    fields: &str,
    trailer: &str,
) -> String {
    let camera = &record.camera;
    let f = |value| legacy_float(value, 9);
    let mut json = format!(
        "{{\n\"role\":{},\n\"frame\":{},\n\"session\":{},\n\"view\":{},\n\"generation\":{},\n\"flags\":{},\n\"frame_time_ms\":{}{fields},\n\"render_size\":[{},{}],\n\"output_size\":[{},{}],\n\"jitter_pixels\":[{},{}],\n\"matrix_convention\":\"row_vectors\"",
        Quoted(role),
        record.frame_id,
        record.session_id,
        record.view_id,
        record.resource_generation,
        record.flags,
        f(record.frame_time_ms),
        camera.render_width,
        camera.render_height,
        camera.output_width,
        camera.output_height,
        f(camera.jitter_pixels[0]),
        f(camera.jitter_pixels[1]),
    );
    for (key, values) in [
        ("view_to_clip", &camera.view_to_clip),
        ("clip_to_view", &camera.clip_to_view),
        ("view_to_world", &camera.view_to_world),
        ("world_to_view", &camera.world_to_view),
        ("clip_to_previous_clip", &camera.clip_to_previous_clip),
    ] {
        let joined: Vec<String> = values.iter().copied().map(f).collect();
        let _ = write!(json, ",\n\"{key}\":[{}]", joined.join(","));
    }
    json.push_str(trailer);
    json.push_str("\n}\n");
    json
}

pub(super) fn legacy_stem(record: &rsf_frame_record, role: &str) -> String {
    let mut stem = format!(
        "{}-{}-v{}-g{}-",
        record.session_id, record.frame_id, record.view_id, record.resource_generation
    );
    for byte in role.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
            stem.push(char::from(byte));
        } else {
            let _ = write!(stem, "%{byte:02X}");
        }
    }
    stem
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create_new(path)?;
    let result = file.write_all(bytes).and_then(|()| file.flush());
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn write_with_suffix(stem: &str, mut write: impl FnMut(&str) -> io::Result<()>) -> io::Result<()> {
    match write(stem) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        result => return result,
    }
    for attempt in 1..=u32::MAX {
        match write(&format!("{stem}-repeat{attempt}")) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            result => return result,
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

pub(super) fn write_unique(
    directory: &Path,
    stem: &str,
    suffix: &str,
    bytes: &[u8],
) -> io::Result<()> {
    write_with_suffix(stem, |name| {
        write_new(&directory.join(format!("{name}.{suffix}")), bytes)
    })
}

pub(super) fn write_legacy(
    directory: &Path,
    record: &rsf_frame_record,
    role: &str,
    ordinal: u32,
    bytes: &[u8],
    json: &str,
) -> io::Result<()> {
    let stem = format!("{}-{ordinal}", legacy_stem(record, role));
    write_with_suffix(&stem, |name| {
        let raw = directory.join(format!("{name}.raw"));
        write_new(&raw, bytes)?;
        let result = write_new(&directory.join(format!("{name}.json")), json.as_bytes());
        if result.is_err() {
            let _ = fs::remove_file(raw);
        }
        result
    })
}
