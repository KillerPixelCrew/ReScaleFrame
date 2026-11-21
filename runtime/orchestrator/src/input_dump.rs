// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::render_abi::{rsf_backend_log_fn, rsf_dlss_frame, rsf_fg_frame, rsf_sr_frame};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION,
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_PRIMARY,
    RSF_GAME_RENDER_SR, RSF_GAME_RENDER_UI_COMPOSITE, rsf_camera_frame, rsf_frame_record,
    rsf_game_depth_domains, rsf_game_render_pass,
};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};

mod abi;
mod legacy;
mod metadata;

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const WRITE_FAILED: i32 = -6;
const RAW: u32 = 0;
pub(super) const DECODED_MOTION: u32 = 2;
const HDR: u32 = 3;
const DEPTH: u32 = 4;
const SCALAR: u32 = 5;
const ALPHA: u32 = 6;
const DENSE_MOTION: u32 = 7;
const D3D11: u32 = 1;
const MAX_IMAGES: usize = 96;
pub(super) const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Clock {
    tick: u64,
    process: u32,
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
    key_down: u32,
}

#[repr(C)]
#[derive(Default)]
struct Surface {
    width: u32,
    height: u32,
    format: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Encoding {
    raw: i32,
    preview: i32,
}

impl Encoding {
    const INVALID: Self = Self {
        raw: INVALID,
        preview: INVALID,
    };

    fn success(self) -> bool {
        self.raw == 0 && self.preview == 0
    }
}

#[repr(C)]
pub(super) struct Pixels {
    pub(super) struct_size: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) format: u32,
    pub(super) row_pitch: u32,
    pub(super) data_size: u64,
    pub(super) data: *const c_void,
}

#[repr(C)]
pub(super) struct TextureOptions {
    struct_size: u32,
    pub(super) abi_version: u32,
    pub(super) prefix: *const c_char,
    pub(super) view: u32,
    pub(super) scale: f32,
    pub(super) log: rsf_backend_log_fn,
    pub(super) log_user: *mut c_void,
}

impl TextureOptions {
    pub(super) const ABI_VERSION: u32 = 2;

    pub(super) fn new(prefix: &CStr, view: u32, scale: f32, log: Log) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            abi_version: Self::ABI_VERSION,
            prefix: prefix.as_ptr(),
            view,
            scale,
            log: log.callback,
            log_user: log.user as *mut c_void,
        }
    }
}

#[repr(C)]
struct NamedRequest {
    struct_size: u32,
    abi_version: u32,
    directory: *const c_char,
    name: *const c_char,
    view: u32,
    scale: f32,
    log: rsf_backend_log_fn,
    log_user: *mut c_void,
}

#[repr(C)]
struct NamedReport {
    struct_size: u32,
    width: u32,
    height: u32,
    dxgi_format: u32,
}

#[repr(C)]
struct Platform {
    struct_size: u32,
    clock: unsafe extern "C" fn(*mut Clock),
    local_time: unsafe extern "C" fn(*mut Clock),
    sha256: unsafe extern "C" fn(*const u16, *mut c_char),
    open_texture: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut Surface) -> *mut c_void,
    backbuffer: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    encode_texture:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *const TextureOptions, u32, *mut Encoding),
    preview_pixels: unsafe extern "C" fn(*const Pixels, *const TextureOptions) -> i32,
    depth_domains_requested: unsafe extern "C" fn(*const rsf_game_depth_domains) -> u32,
}

static PLATFORM: OnceLock<&'static Platform> = OnceLock::new();

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dump_bind_platform(platform: &'static Platform) {
    assert_eq!(platform.struct_size as usize, size_of::<Platform>());
    let _ = PLATFORM.set(platform);
}

fn platform() -> &'static Platform {
    PLATFORM
        .get()
        .expect("FrameDump platform bound by the public ABI")
}

fn clock() -> Clock {
    let mut facts = Clock::default();
    unsafe { (platform().clock)(&mut facts) };
    facts
}

#[derive(Clone, Copy, Default)]
pub(super) struct Log {
    callback: rsf_backend_log_fn,
    // An opaque caller-supplied token. Rust never dereferences it.
    user: usize,
}

impl Log {
    pub(super) fn new(callback: rsf_backend_log_fn, user: *mut c_void) -> Self {
        Self {
            callback,
            user: user as usize,
        }
    }

    pub(super) fn say(self, text: &str) {
        if let Some(callback) = self.callback
            && let Ok(message) = CString::new(text)
        {
            unsafe { callback(self.user as *mut c_void, message.as_ptr()) };
        }
    }
}

#[derive(Clone, Copy, Default)]
struct PassIdentity {
    session: u64,
    source: u64,
    view: u64,
    native: u64,
    family: u64,
    scope: u64,
    viewport: u64,
    render_rect: [i32; 4],
    output_rect: [i32; 4],
}

impl From<&rsf_game_render_pass> for PassIdentity {
    fn from(pass: &rsf_game_render_pass) -> Self {
        Self {
            session: pass.session_id,
            source: pass.source_frame_id,
            view: pass.view_key,
            native: pass.native_frame,
            family: pass.family_key,
            scope: pass.scope_id,
            viewport: pass.viewport_key,
            render_rect: pass.render_rect,
            output_rect: pass.output_rect,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct WindowIdentity {
    // Window callbacks have their own scope and may not carry the SR family.
    session: u64,
    source: u64,
    native: u64,
    view: u64,
    viewport: u64,
    window: u64,
    rhi_viewport: u64,
    scope: u64,
}

impl From<&rsf_game_render_pass> for WindowIdentity {
    fn from(pass: &rsf_game_render_pass) -> Self {
        Self {
            session: pass.session_id,
            source: pass.source_frame_id,
            native: pass.native_frame,
            view: pass.view_key,
            viewport: pass.viewport_key,
            window: pass.window_key,
            rhi_viewport: pass.rhi_viewport_key,
            scope: pass.scope_id,
        }
    }
}

#[derive(PartialEq, Eq)]
enum ImageState {
    Unavailable,
    Reserved,
    Encoding,
    Finished,
}

struct Image {
    name: String,
    description: String,
    view: u32,
    scale: f32,
    state: ImageState,
    surface: Surface,
    api: u32,
    native_format: u32,
    encoding: Encoding,
}

impl Image {
    fn new(name: &str, description: &str, view: u32, scale: f32, state: ImageState) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            view,
            scale: if scale == 0.0 { 1.0 } else { scale },
            state,
            surface: Surface::default(),
            api: D3D11,
            native_format: 0,
            encoding: Encoding::INVALID,
        }
    }

    fn pending(&self) -> bool {
        matches!(self.state, ImageState::Reserved | ImageState::Encoding)
    }

    fn captured(&self) -> bool {
        self.state == ImageState::Finished && self.encoding.success()
    }
}

struct Capture {
    ticket: u64,
    bundle: PathBuf,
    started: u64,
    record: Option<rsf_frame_record>,
    identity: Option<PassIdentity>,
    legacy_camera: Option<rsf_camera_frame>,
    in_sr: bool,
    backend: u32,
    sr_result: i32,
    constants: bool,
    fg_constants: bool,
    window: Option<WindowIdentity>,
    present_result: Option<i32>,
    images: Vec<Image>,
    gpu_completed: HashMap<(u32, usize), usize>,
}

impl Capture {
    fn new(ticket: u64, bundle: PathBuf, started: u64) -> Self {
        Self {
            ticket,
            bundle,
            started,
            record: None,
            identity: None,
            legacy_camera: None,
            in_sr: false,
            backend: 0,
            sr_result: -1,
            constants: false,
            fg_constants: false,
            window: None,
            present_result: None,
            images: Vec::new(),
            gpu_completed: HashMap::new(),
        }
    }

    fn session(&self) -> u64 {
        self.record.map_or_else(
            || self.identity.map_or(0, |id| id.session),
            |record| record.session_id,
        )
    }

    fn has_target(&self) -> bool {
        self.record.is_some() || self.identity.is_some()
    }

    fn wants(&self, record: &rsf_frame_record) -> bool {
        self.record.is_some_and(|selected| {
            (record.flags ^ selected.flags) & RSF_FRAME_FLAG_AMBIGUOUS_ID == 0
                && (
                    record.session_id,
                    record.frame_id,
                    record.view_id,
                    record.resource_generation,
                ) == (
                    selected.session_id,
                    selected.frame_id,
                    selected.view_id,
                    selected.resource_generation,
                )
        })
    }

    fn matches_pass(&self, pass: &rsf_game_render_pass) -> bool {
        if pass.flags & RSF_GAME_RENDER_PRIMARY == 0 || pass.session_id != self.session() {
            return false;
        }
        if let Some(record) = &self.record {
            let source = if record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID != 0 {
                pass.source_frame_id == 0
                    && pass.native_frame == record.frame_id
                    && pass.flags & RSF_GAME_RENDER_NO_FG != 0
            } else {
                pass.source_frame_id == record.frame_id
            };
            source
                && pass.view_key as u32 == record.view_id
                && self
                    .identity
                    .is_none_or(|id| id.native == pass.native_frame && id.view == pass.view_key)
        } else {
            self.identity.is_some_and(|id| {
                id.source == pass.source_frame_id
                    && id.native == pass.native_frame
                    && id.view == pass.view_key
                    && id.family == pass.family_key
            })
        }
    }

    fn fill_sr_metadata(&mut self, frame: &rsf_frame_record, backend: u32) {
        let Some(mut selected) = self.record else {
            return;
        };
        let extent = |record: &rsf_frame_record| {
            [
                record.render_width,
                record.render_height,
                record.output_width,
                record.output_height,
            ]
        };
        let dimensions = extent(&selected);
        if self.present_result.is_some() || extent(frame) != dimensions {
            return;
        }
        let camera = frame.camera;
        let has_camera = |camera: &rsf_camera_frame| {
            camera.struct_size as usize >= size_of::<rsf_camera_frame>()
                && camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
        };
        let fill_camera = !has_camera(&selected.camera)
            && has_camera(&camera)
            && [
                camera.render_width,
                camera.render_height,
                camera.output_width,
                camera.output_height,
            ] == dimensions;
        let backend = if self.backend == 0 {
            backend
        } else {
            self.backend
        };
        if !fill_camera && backend == self.backend {
            return;
        }
        if fill_camera {
            selected.camera = camera;
        }
        if metadata::frame(&self.bundle, &selected, backend).is_ok() {
            self.record = Some(selected);
            self.backend = backend;
            self.constants = true;
        }
    }

    fn matches_window(&self, pass: &rsf_game_render_pass) -> bool {
        let source = match &self.record {
            Some(record) if record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID == 0 => record.frame_id,
            Some(_) => return false,
            None => self.identity.map_or(0, |id| id.source),
        };
        source != 0
            && source == pass.source_frame_id
            && self.session() == pass.session_id
            && self.identity.is_none_or(|id| {
                (id.viewport == pass.viewport_key || self.record.is_some() && id.viewport == 0)
                    && (pass.native_frame == 0 || id.native == pass.native_frame)
                    && (pass.view_key == 0 || id.view == pass.view_key)
            })
            && (pass.view_key == 0
                || self
                    .record
                    .is_none_or(|record| record.view_id == pass.view_key as u32))
    }

    fn add_image(&mut self, image: Image) -> Option<usize> {
        if self.present_result.is_some()
            || self.images.len() >= MAX_IMAGES
            || self.images.iter().any(|found| found.name == image.name)
        {
            return None;
        }
        let index = self.images.len();
        self.images.push(image);
        Some(index)
    }

    fn index(&self, ticket: u64, name: &str) -> Option<usize> {
        if self.ticket != ticket {
            return None;
        }
        self.images.iter().position(|image| image.name == name)
    }

    fn completion_status(&self) -> Option<&'static str> {
        let present = self.present_result?;
        if self.images.iter().any(Image::pending) {
            return None;
        }
        let images_failed = self
            .images
            .iter()
            .any(|image| image.state == ImageState::Finished && !image.encoding.success());
        let required = if self.record.is_some() {
            &["final_color"][..]
        } else {
            metadata::LEGACY_REQUIRED
        };
        let all_required = required.iter().all(|name| {
            self.images
                .iter()
                .any(|image| image.name == *name && image.captured())
        });
        Some(
            if present < 0
                || self.sr_result != 0
                || !self.constants
                || images_failed
                || !all_required
            {
                "partial"
            } else {
                "complete"
            },
        )
    }
}

#[derive(Default)]
struct Owner {
    directory: Option<PathBuf>,
    game: String,
    hash: String,
    log: Log,
    messages: Vec<(Log, String)>,
    key_down: bool,
    last_ticket: u64,
    capture: Option<Capture>,
}

static OWNER: LazyLock<Mutex<Owner>> = LazyLock::new(|| Mutex::new(Owner::default()));

fn owner<T>(operation: impl FnOnce(&mut Owner) -> T) -> T {
    let (result, messages) = {
        let mut owner = OWNER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = operation(&mut owner);
        (result, std::mem::take(&mut owner.messages))
    };
    // Progress sinks can query the capture ABI again. Never invoke one under the lock.
    for (log, line) in messages {
        log.say(&line);
    }
    result
}

impl Owner {
    fn say(&mut self, message: impl Into<String>) {
        if self.log.callback.is_some() {
            self.messages.push((self.log, message.into()));
        }
    }

    fn publish(&mut self, status: &str) {
        if let Some(capture) = &self.capture
            && metadata::manifest(capture, &self.game, &self.hash, status).is_err()
        {
            self.say("website dump: manifest write failed");
        }
    }

    fn finish(&mut self, status: &str) {
        if let Some(capture) = &mut self.capture {
            metadata::fill_missing(&mut capture.images);
            let path = capture.bundle.clone();
            self.publish(status);
            self.capture = None;
            self.say(format!("website dump {status}: {}", path.display()));
        }
    }

    fn finish_ready(&mut self) {
        if let Some(status) = self.capture.as_ref().and_then(Capture::completion_status) {
            self.finish(status);
        }
    }

    fn configure(&mut self, directory: Option<PathBuf>, game: String, hash: String, log: Log) {
        self.log = log;
        self.finish("cancelled_on_reconfigure");
        self.directory = directory.map(|dir| dir.join("website"));
        self.game = game;
        self.hash = hash;
        self.say("website input dump: F10 captures the next source frame as named opaque PNGs and raw buffers");
    }

    fn configure_default(&mut self) -> Option<()> {
        let executable = std::env::current_exe().ok()?;
        let fallback = executable.file_stem()?;
        let game = Path::new(&self.game)
            .file_name()
            .filter(|name| *name != "..")
            .unwrap_or(fallback);
        let directory =
            if let Some(path) = std::env::var_os("RSF_DUMP_DIR").filter(|path| !path.is_empty()) {
                PathBuf::from(path)
            } else {
                PathBuf::from(std::env::var_os("LOCALAPPDATA").filter(|path| !path.is_empty())?)
                    .join("ReScaleFrame")
                    .join(game)
            };
        let game = game.to_string_lossy().into_owned();
        let path: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
        let mut hash = [0; 65];
        unsafe { (platform().sha256)(path.as_ptr(), hash.as_mut_ptr()) };
        let hash = unsafe { CStr::from_ptr(hash.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        self.configure(Some(directory), game, hash, self.log);
        Some(())
    }

    fn request(&mut self, now: &Clock) {
        let Some(directory) = &self.directory else {
            return;
        };
        if self.capture.is_some() {
            return;
        }
        // The high 56 bits of a submission token hold the capture ticket.
        let ticket = self.last_ticket + 1;
        if ticket > u64::MAX >> 8 {
            return;
        }
        let mut now = *now;
        unsafe { (platform().local_time)(&mut now) };
        let name = format!(
            "scene-{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}-{}-{}-{ticket}",
            now.year,
            now.month,
            now.day,
            now.hour,
            now.minute,
            now.second,
            now.millisecond,
            now.tick,
            now.process
        );
        let bundle = directory.join(name);
        match fs::create_dir_all(directory).and_then(|()| fs::create_dir(&bundle)) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                self.say("website dump: could not create a unique directory");
                return;
            }
            Err(_) => {
                self.say("website dump stopped after an allocation or file failure; partial files retained");
                return;
            }
        }
        self.last_ticket = ticket;
        self.capture = Some(Capture::new(ticket, bundle.clone(), now.tick));
        self.publish("waiting_for_sr");
        self.say(format!("website dump armed: {}", bundle.display()));
    }

    fn poll(&mut self, now: &Clock) {
        if self.directory.is_none() && self.configure_default().is_none() {
            return;
        }
        if self
            .capture
            .as_ref()
            .is_some_and(|capture| now.tick.saturating_sub(capture.started) > 30000)
        {
            self.finish("incomplete_waiting_for_gpu_or_frame");
        }
        let down = now.key_down != 0;
        if down && !self.key_down {
            self.request(now);
        }
        self.key_down = down;
    }

    fn complete(&mut self, result: i32) {
        if let Some(capture) = &mut self.capture {
            // The first matching completion owns Present for this bundle.
            capture.present_result.get_or_insert(result);
            self.finish_ready();
            if self.capture.is_some() {
                self.publish("waiting_for_gpu");
            }
        }
    }

    fn finish_image(&mut self, work: &Work, result: ImageResult) -> bool {
        let Some(capture) = self
            .capture
            .as_mut()
            .filter(|capture| capture.ticket == work.ticket)
        else {
            return false;
        };
        let image = &mut capture.images[work.index];
        image.state = ImageState::Finished;
        image.surface = result.surface;
        image.api = result.api;
        image.native_format = result.native_format;
        image.view = result.view;
        image.encoding = result.encoding;
        self.finish_ready();
        result.encoding.success()
    }

    fn work(&self, index: usize) -> Option<Work> {
        let capture = self.capture.as_ref()?;
        let image = &capture.images[index];
        Some(Work {
            ticket: capture.ticket,
            index,
            prefix: capture.bundle.join(&image.name),
            view: image.view,
            scale: image.scale,
            log: self.log,
        })
    }
}

struct Work {
    ticket: u64,
    index: usize,
    prefix: PathBuf,
    view: u32,
    scale: f32,
    log: Log,
}

impl Work {
    fn options(&self, prefix: &CString, view: u32) -> TextureOptions {
        TextureOptions::new(prefix, view, self.scale, self.log)
    }
}

struct ImageResult {
    surface: Surface,
    api: u32,
    native_format: u32,
    view: u32,
    encoding: Encoding,
}

fn preview_view(view: u32, format: u32, cpu_pixels: bool) -> u32 {
    // DXGI R16G16B16A16_FLOAT (10), R11G11B10_FLOAT (26), R32G32B32A32_FLOAT (2).
    if view == HDR && !(matches!(format, 10 | 26) || cpu_pixels && format == 2) {
        RAW
    } else {
        view
    }
}

fn prefix(path: &Path) -> Result<CString, std::ffi::NulError> {
    CString::new(path.to_string_lossy().as_bytes())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}

pub(super) unsafe fn checked<'a, T>(input: *const T) -> Option<&'a T> {
    (!input.is_null() && unsafe { input.cast::<u32>().read() } as usize >= size_of::<T>())
        .then(|| unsafe { &*input })
}

unsafe fn record<'a>(input: *const rsf_frame_record) -> Option<&'a rsf_frame_record> {
    unsafe { checked(input) }.filter(|record| record.abi_version == RSF_GAME_FRAME_ABI_VERSION)
}

unsafe fn string<'a>(input: *const c_char) -> Option<&'a str> {
    (!input.is_null())
        .then(|| unsafe { CStr::from_ptr(input) }.to_str().ok())
        .flatten()
}

unsafe fn write_texture(
    work: &Work,
    context: *mut c_void,
    source: *mut c_void,
    raw: bool,
) -> ImageResult {
    let mut surface = Surface::default();
    let mut encoding = Encoding::INVALID;
    let texture =
        unsafe { ComRef::from_owned((platform().open_texture)(context, source, &mut surface)) };
    let view = preview_view(work.view, surface.format, false);
    if let Some(texture) = texture {
        match prefix(&work.prefix) {
            Ok(prefix) => unsafe {
                (platform().encode_texture)(
                    context,
                    texture.as_ptr(),
                    &work.options(&prefix, view),
                    u32::from(raw),
                    &mut encoding,
                )
            },
            Err(_) => {
                encoding = Encoding {
                    raw: WRITE_FAILED,
                    preview: WRITE_FAILED,
                }
            }
        }
    }
    ImageResult {
        api: D3D11,
        native_format: surface.format,
        surface,
        view,
        encoding,
    }
}

struct Texture<'a> {
    name: &'a str,
    description: &'a str,
    source: *mut c_void,
    view: u32,
    scale: f32,
}

impl<'a> Texture<'a> {
    fn new(
        name: &'a str,
        description: &'a str,
        source: *mut c_void,
        view: u32,
        scale: f32,
    ) -> Self {
        Self {
            name,
            description,
            source,
            view,
            scale,
        }
    }
}

// Take the ticket and path while locked. A retirement during encoding can leave
// partial files in that bundle, but cannot publish into a later request.
unsafe fn capture_texture(
    context: *mut c_void,
    texture: Texture<'_>,
    accept: impl FnOnce(&Capture) -> bool,
    legacy: bool,
) -> bool {
    if context.is_null()
        || (!legacy && texture.source.is_null())
        || !valid_name(texture.name)
        || texture.view > DENSE_MOTION
        || !texture.scale.is_finite()
    {
        return false;
    }
    let Some(work) = owner(|owner| {
        let capture = owner.capture.as_mut().filter(|capture| accept(capture))?;
        let state = if texture.source.is_null() {
            ImageState::Unavailable
        } else {
            ImageState::Encoding
        };
        let index = capture.add_image(Image::new(
            texture.name,
            texture.description,
            texture.view,
            texture.scale,
            state,
        ))?;
        (!texture.source.is_null())
            .then(|| owner.work(index))
            .flatten()
    }) else {
        return false;
    };
    if legacy {
        work.log.say(&format!("website dump: {}", texture.name));
    }
    let result = unsafe { write_texture(&work, context, texture.source, true) };
    owner(|owner| owner.finish_image(&work, result))
}
