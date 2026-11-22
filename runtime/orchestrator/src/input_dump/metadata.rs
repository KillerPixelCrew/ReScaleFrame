// SPDX-License-Identifier: MIT
use super::{Capture, Image, ImageState, Pixels, RAW};
use crate::capture_metadata::Quoted;
use crate::sys::{rsf_dlss_frame, rsf_fg_frame, rsf_sr_frame};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_GAME_CAMERA_ABI_VERSION, rsf_camera_frame, rsf_frame_record,
    rsf_game_render_pass,
};
use std::fmt::{self, Display, Write};
use std::fs;
use std::io::{self, BufWriter, Write as _};
use std::path::Path;

const EXPECTED: &[&str] = &[
    "sr_source_jittered",
    "sr_color",
    "sr_depth",
    "sr_scene_depth",
    "sr_motion",
    "sr_exposure",
    "sr_engine_exposure",
    "sr_backend_output",
    "sr_game_motion",
    "sr_motion_decoded",
    "sr_output",
    "sr_opaque_color",
    "sr_transparency_layer",
    "sr_reactive_mask",
    "sr_transparency_mask",
    "sr_bias_mask",
    "sr_cloud_depth",
    "fg_depth",
    "fg_motion",
    "hudless_color",
    "hud_color",
    "hud_alpha",
    "hud_composed_color",
    "fg_scene_color",
    "fg_ui",
    "final_color",
];

pub(super) const LEGACY_REQUIRED: &[&str] = &[
    "sr_source_jittered",
    "sr_color",
    "sr_depth",
    "sr_motion",
    "sr_game_motion",
    "sr_motion_decoded",
    "sr_output",
    "fg_depth",
    "fg_motion",
    "hudless_color",
    "hud_color",
    "hud_alpha",
    "hud_composed_color",
    "fg_scene_color",
    "final_color",
];

pub(super) fn fill_missing(images: &mut Vec<Image>) {
    for &name in EXPECTED {
        if !images.iter().any(|image| image.name == name) {
            images.push(Image::new(
                name,
                "Not produced at the matched handoff in this frame",
                RAW,
                1.0,
                ImageState::Unavailable,
            ));
        }
    }
}

struct Float(f32);
impl Display for Float {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_finite() {
            self.0.fmt(f)
        } else {
            f.write_str("null")
        }
    }
}

struct List<T>(T);
impl<T: Display, const N: usize> Display for List<[T; N]> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('[')?;
        for (index, value) in self.0.iter().enumerate() {
            if index != 0 {
                f.write_char(',')?;
            }
            value.fmt(f)?;
        }
        f.write_char(']')
    }
}

fn floats<const N: usize>(values: [f32; N]) -> List<[Float; N]> {
    List(values.map(Float))
}

struct Nullable<T>(Option<T>);
impl<T: Display> Display for Nullable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(value) => value.fmt(f),
            None => f.write_str("null"),
        }
    }
}

struct Object(String);
impl Object {
    fn new() -> Self {
        Self("{".into())
    }

    fn field(&mut self, key: &str, value: impl Display) {
        if self.0.len() != 1 {
            self.0.push(',');
        }
        write!(self.0, "\"{key}\":{value}").expect("writing into a String");
    }

    fn save(self, bundle: &Path, name: &str) -> io::Result<()> {
        fs::write(bundle.join(name), format!("{self}\n"))
    }
}
impl Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)?;
        f.write_char('}')
    }
}

fn write_capture_file(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<fs::File>) -> io::Result<()>,
) -> io::Result<()> {
    let mut file = BufWriter::new(fs::File::create_new(path)?);
    let result = write(&mut file).and_then(|()| file.flush());
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

pub(super) fn raw_pixels<'a>(
    prefix: &Path,
    pixels: &Pixels,
    rows: impl Iterator<Item = &'a [u8]>,
    row: u32,
    api: u32,
    native_format: u32,
) -> io::Result<()> {
    let with_suffix = |suffix: &str| {
        let mut path = prefix.as_os_str().to_owned();
        path.push(suffix);
        path
    };
    let raw = with_suffix(".bin");
    write_capture_file(Path::new(&raw), |file| {
        for source in rows {
            file.write_all(source)?;
        }
        Ok(())
    })?;

    let mut metadata = Object::new();
    metadata.field("width", pixels.width);
    metadata.field("height", pixels.height);
    metadata.field("format", pixels.format);
    metadata.field("native_api", api);
    metadata.field("native_format", native_format);
    metadata.field("row_pitch", row);
    metadata.field("source_row_pitch", pixels.row_pitch);
    metadata.field("bytes", u64::from(row) * u64::from(pixels.height));
    let sidecar = with_suffix("_raw.json");
    let result = write_capture_file(Path::new(&sidecar), |file| writeln!(file, "{metadata}"));
    if result.is_err() {
        let _ = fs::remove_file(raw);
    }
    result
}

fn image_metadata(image: &Image) -> Object {
    let mut item = Object::new();
    item.field("name", Quoted(&image.name));
    item.field("description", Quoted(&image.description));
    item.field(
        "status",
        Quoted(match image.state {
            ImageState::Unavailable => "unavailable",
            ImageState::Reserved | ImageState::Encoding => "pending",
            ImageState::Finished if image.captured() => "captured",
            ImageState::Finished => "failed",
        }),
    );
    item.field("width", image.surface.width);
    item.field("height", image.surface.height);
    item.field("dxgi_format", image.surface.format);
    item.field("native_api", image.api);
    item.field("native_format", image.native_format);
    item.field("preview_view", image.view);
    item.field("preview_scale", Float(image.scale));
    item.field("raw_result", image.encoding.raw);
    item.field("preview_result", image.encoding.preview);
    for (key, suffix, result) in [
        ("raw", ".bin", image.encoding.raw),
        ("raw_metadata", "_raw.json", image.encoding.raw),
        ("png", ".png", image.encoding.preview),
    ] {
        let file = (result == 0).then(|| format!("{}{suffix}", image.name));
        item.field(key, Nullable(file.as_deref().map(Quoted)));
    }
    item
}

const PREVIEW_VIEWS: &str = concat!(
    r#"{"0":"stored RGB clamped to 0..1","1":"UE encoded velocity decoded to NDC; zero gray, unwritten blue","#,
    r#""2":"decoded motion: zero gray, R=X G=Y, blue marks invalid sentinel","3":"linear HDR: Reinhard then sRGB","#,
    r#""4":"device depth: log2(1+65535*z)/16; reversed Z near white, sky black","5":"first channel grayscale","6":"source alpha grayscale","#,
    r#""7":"dense displacement: zero gray, R=X G=Y, units in image description, no unwritten sentinel"}"#,
);

pub(super) fn manifest(capture: &Capture, game: &str, hash: &str, status: &str) -> io::Result<()> {
    let native = capture.identity.unwrap_or_default();
    let frame_id = capture
        .record
        .map_or(native.source, |record| record.frame_id);
    let view = capture.identity.map_or_else(
        || capture.record.map_or(0, |record| u64::from(record.view_id)),
        |id| id.view,
    );
    let generic = capture.record.is_some();
    let mut object = Object::new();
    object.field("schema", 1);
    object.field("game", Quoted(game));
    object.field("executable_sha256", Quoted(hash));
    object.field("rsf_version", Quoted(env!("CARGO_PKG_VERSION")));
    object.field("status", Quoted(status));
    object.field("frame_id", frame_id);
    object.field(
        "source_frame_id",
        capture
            .record
            .as_ref()
            .map_or(Nullable(Some(frame_id)), source_frame),
    );
    object.field("session_id", capture.session());
    object.field(
        "native_frame",
        Nullable(capture.identity.map(|id| id.native).or_else(|| {
            capture
                .record
                .as_ref()
                .filter(|record| source_frame(record).0.is_none())
                .map(|record| record.frame_id)
        })),
    );
    object.field("view_key", view);
    object.field("family_key", native.family);
    object.field("scope_id", native.scope);
    object.field("viewport_key", native.viewport);
    object.field("backend", capture.backend);
    object.field("sr_result", Nullable(capture.sr_result));
    object.field("fg_result", Nullable(capture.fg_result));
    object.field(
        "matched_present",
        capture.window.is_some() || capture.present_result.is_some(),
    );
    object.field("present_result", capture.present_result.unwrap_or(0));
    let constants = if generic {
        "frame.json"
    } else {
        "sr_constants.json"
    };
    object.field(
        "constants",
        Nullable(capture.constants.then_some(Quoted(constants))),
    );
    object.field(
        "fg_constants",
        Nullable(capture.fg_constants.then_some(Quoted("fg_constants.json"))),
    );
    object.field("preview_alpha", Quoted("opaque"));
    object.field("raw_encoding", Quoted("little-endian packed rows; original GPU storage, subresource 0; format is DXGI-compatible encoding, native_api/native_format preserve origin"));
    object.field("preview_views", PREVIEW_VIEWS);
    let mut images = String::from("[\n");
    for (index, image) in capture.images.iter().enumerate() {
        if index != 0 {
            images.push_str(",\n");
        }
        write!(images, "{}", image_metadata(image)).expect("writing into a String");
    }
    images.push_str("\n]");
    object.field("images", images);
    object.save(&capture.bundle, "manifest.json")
}

fn dimensions(object: &mut Object, record: &rsf_frame_record) {
    object.field("render", List([record.render_width, record.render_height]));
    object.field("output", List([record.output_width, record.output_height]));
}

fn camera(object: &mut Object, camera: &rsf_camera_frame, include_fov: bool) {
    object.field("jitter_pixels", floats(camera.jitter_pixels));
    object.field(
        "previous_jitter_pixels",
        floats(camera.previous_jitter_pixels),
    );
    object.field("view_to_clip", floats(camera.view_to_clip));
    object.field("clip_to_view", floats(camera.clip_to_view));
    object.field("view_to_world", floats(camera.view_to_world));
    object.field("world_to_view", floats(camera.world_to_view));
    object.field("clip_to_previous", floats(camera.clip_to_previous_clip));
    object.field("near_plane", Float(camera.near_plane));
    object.field("far_plane", Float(camera.far_plane));
    if include_fov {
        object.field("vertical_fov_radians", Float(camera.vertical_fov_radians));
    }
    object.field("depth_inverted", camera.depth_inverted);
    object.field("motion_jittered", camera.motion_jittered);
}

pub(super) fn camera_available(camera: &rsf_camera_frame) -> bool {
    camera.struct_size as usize >= size_of::<rsf_camera_frame>()
        && camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
}

fn source_frame(record: &rsf_frame_record) -> Nullable<u64> {
    Nullable((record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID == 0).then_some(record.frame_id))
}

fn record_identity(object: &mut Object, record: &rsf_frame_record) {
    object.field("session_id", record.session_id);
    object.field("frame_id", record.frame_id);
    object.field("source_frame_id", source_frame(record));
    object.field("view_id", record.view_id);
    object.field("resource_generation", record.resource_generation);
}

pub(super) fn frame(bundle: &Path, record: &rsf_frame_record, backend: u32) -> io::Result<()> {
    let mut object = Object::new();
    record_identity(&mut object, record);
    object.field("flags", record.flags);
    object.field("screen", record.screen);
    object.field("backend", backend);
    let has_camera = camera_available(&record.camera);
    object.field("camera_available", has_camera);
    dimensions(&mut object, record);
    object.field("frame_time_ms", Float(record.frame_time_ms));
    if has_camera {
        camera(&mut object, &record.camera, false);
    }
    object.save(bundle, "frame.json")
}

pub(super) fn engine_pass(
    bundle: &Path,
    pass: &rsf_game_render_pass,
    depth_requested: u32,
) -> io::Result<()> {
    let mut object = Object::new();
    object.field("session_id", pass.session_id);
    object.field("source_frame_id", pass.source_frame_id);
    object.field("native_frame", pass.native_frame);
    object.field("view_key", pass.view_key);
    object.field("history_key", pass.history_key);
    object.field("submission_id", pass.submission_id);
    object.field("viewport_key", pass.viewport_key);
    object.field("window_key", pass.window_key);
    object.field("rhi_viewport_key", pass.rhi_viewport_key);
    object.field("scope_id", pass.scope_id);
    object.field("camera_valid", pass.camera_valid);
    object.field("flags", pass.flags);
    object.field("motion_camera_included", pass.motion_camera_included);
    object.field("motion_to_uv", floats(pass.motion_to_uv));
    object.field("render_rect", List(pass.render_rect));
    object.field("output_rect", List(pass.output_rect));

    let depth = &pass.depth_domains;
    let mut depth_meta = Object::new();
    depth_meta.field("requested", depth_requested);
    depth_meta.field("struct_size", depth.struct_size);
    depth_meta.field("version", depth.version);
    depth_meta.field("domain_count", depth.domain_count);
    depth_meta.field("clear_enabled", depth.clear_enabled);
    depth_meta.field("clear_value", Float(depth.clear_value));
    depth_meta.field("reserved", List(depth.reserved));
    // Both inline slots are facts even for malformed descriptors.
    depth_meta.field(
        "domains",
        List(depth.domains.map(|domain| {
            let mut item = Object::new();
            item.field("classify_min", Float(domain.classify_min));
            item.field("classify_max", Float(domain.classify_max));
            item.field("endpoint_flags", domain.endpoint_flags);
            item.field("reserved", domain.reserved);
            item.field("viewport_xywh", floats(domain.viewport_xywh));
            item.field("viewport_min_depth", Float(domain.viewport_min_depth));
            item.field("viewport_max_depth", Float(domain.viewport_max_depth));
            item.field(
                "source_clip_to_target_clip",
                floats(domain.source_clip_to_target_clip),
            );
            item
        })),
    );
    object.field("depth_domains", depth_meta);
    object.save(bundle, "engine_pass.json")
}

pub(super) fn result(
    bundle: &Path,
    record: &rsf_frame_record,
    stage: &str,
    backend: u32,
    result: i32,
) -> io::Result<()> {
    let mut object = Object::new();
    record_identity(&mut object, record);
    object.field("backend", backend);
    object.field("result", result);
    object.save(bundle, &format!("{stage}_status.json"))
}

pub(super) fn sr_parameters(
    bundle: &Path,
    frame: &rsf_sr_frame,
    record: &rsf_frame_record,
    backend: u32,
    input_sdr: u32,
) -> io::Result<()> {
    let mut object = Object::new();
    record_identity(&mut object, record);
    object.field("backend", backend);
    object.field("input_sdr", input_sdr);
    object.field("reset", frame.reset);
    object.field("jitter_pixels", floats([frame.jitter_x, frame.jitter_y]));
    object.field(
        "motion_scale",
        floats([frame.motion_scale_x, frame.motion_scale_y]),
    );
    object.field("pre_exposure", Float(frame.pre_exposure));
    object.field("view_space_to_meters", Float(frame.view_space_to_meters));
    object.save(bundle, "sr_parameters.json")
}

pub(super) fn sr_constants(
    capture: &Capture,
    frame: &rsf_dlss_frame,
    backend: u32,
) -> io::Result<()> {
    let mut object = Object::new();
    object.field("render", List([frame.render_width, frame.render_height]));
    object.field("output", List([frame.output_width, frame.output_height]));
    object.field("backend", backend);
    object.field("quality", frame.quality);
    object.field("preset", frame.preset);
    object.field("color_encoded", frame.color_encoded);
    object.field("depth_inverted", frame.depth_inverted);
    object.field("camera_motion_included", frame.camera_motion_included);
    object.field("reset", frame.reset);
    object.field("jitter_pixels", floats([frame.jitter_x, frame.jitter_y]));
    object.field(
        "motion_scale",
        floats([frame.motion_scale_x, frame.motion_scale_y]),
    );
    object.field("motion_invalid_value", Float(frame.motion_invalid_value));
    object.field("view_to_clip", floats(frame.camera_view_to_clip));
    object.field("clip_to_view", floats(frame.clip_to_camera_view));
    object.field("clip_to_previous", floats(frame.clip_to_prev_clip));
    object.field("previous_to_clip", floats(frame.prev_clip_to_clip));
    object.field("position", floats(frame.camera_position));
    object.field("up", floats(frame.camera_up));
    object.field("right", floats(frame.camera_right));
    object.field("forward", floats(frame.camera_forward));
    object.field("near_plane", Float(frame.near_plane));
    object.field("far_plane", Float(frame.far_plane));
    object.field("vertical_fov_radians", Float(frame.vertical_fov));
    object.field("aspect_ratio", Float(frame.aspect_ratio));
    let camera = capture
        .record
        .as_ref()
        .map(|record| &record.camera)
        .or(capture.legacy_camera.as_ref())
        .expect("SR constants require a selected pass or frame");
    object.field(
        "previous_jitter_pixels",
        floats(camera.previous_jitter_pixels),
    );
    object.field("view_to_world", floats(camera.view_to_world));
    object.field("world_to_view", floats(camera.world_to_view));
    object.field("motion_jittered", camera.motion_jittered);
    object.field("frame_time_seconds", Float(camera.frame_time_seconds));
    let pass = capture.identity.unwrap_or_default();
    object.field("render_rect", List(pass.render_rect));
    object.field("output_rect", List(pass.output_rect));
    object.save(&capture.bundle, "sr_constants.json")
}

pub(super) fn fg_constants(
    bundle: &Path,
    frame: &rsf_fg_frame,
    record: &rsf_frame_record,
    result: i32,
) -> io::Result<()> {
    let mut object = Object::new();
    record_identity(&mut object, record);
    object.field("prepare_result", result);
    object.field("flags", record.flags);
    object.field("screen", record.screen);
    object.field("input_qpc", record.input_qpc);
    object.field("source_provenance", record.source_provenance);
    object.field("source_qpc", record.source_qpc);
    object.field("source_qpc_frequency", record.source_qpc_frequency);
    object.field("interpolate", frame.interpolate);
    object.field("frame_time_ms", Float(record.frame_time_ms));
    dimensions(&mut object, record);
    object.field(
        "motion_scale",
        floats([frame.motion_scale_x, frame.motion_scale_y]),
    );
    camera(&mut object, &record.camera, true);
    object.save(bundle, "fg_constants.json")
}
