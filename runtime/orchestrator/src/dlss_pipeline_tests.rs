// SPDX-License-Identifier: MIT
// Synthetic D3D11 SR pipeline contracts: fake COM objects, the fake leaves and the legacy
// adapter over a fake bridge. No device, window, SDK or shader is touched.
use super::*;
use crate::com_ref::test::Fake;
use crate::sr_legacy::fake as legacy;
use crate::sys::rsf_motion_resolve_params;
use std::cell::{Cell, RefCell};
use std::ptr;

// Leaked: fake objects outlive every pipeline field that retains them.
fn fake() -> Raw {
    Box::into_raw(Box::new(Fake::new())).cast()
}

fn refs(object: Raw) -> u32 {
    unsafe { (*object.cast::<Fake>()).refs.get() }
}

fn handle(value: usize) -> Raw {
    ptr::without_provenance_mut(value)
}

const CONTEXT: usize = 0x10;
const SCENE: usize = 0x5000;
const DEPTH: usize = 0x5100;
const MOTION: usize = 0x5200;
const DECODED: usize = 0x2000;
const RESOLVED_MOTION: usize = 0x3100;
const RESOLVED_DEPTH: usize = 0x3200;
const ENCODED: usize = 0x4100;

struct World {
    logs: Vec<String>,
    calls: Vec<String>,
    host: usize,
    load: i32,
    support: i32,
    supported: u32,
    plan: rsf_dlss_plan,
    plan_result: i32,
    release_result: i32,
    decode_create: i32,
    decode_run: i32,
    evaluate: i32,
    layer_evaluate: i32,
    texture_fails: bool,
    textures: Vec<(Raw, TextureDesc)>,
    zeroed_pitch: u32,
    frames: Vec<DlssFrame>,
    captures: Vec<DlssFrame>,
    layer_frames: Vec<DlssFrame>,
    transport: bool,
    dumps: Vec<(String, u32, &'static str, Raw)>,
    reentered: Option<i32>,
    reenter: bool,
    surface_retry: bool,
}

impl World {
    fn new() -> Self {
        Self {
            logs: Vec::new(),
            calls: Vec::new(),
            host: 0,
            load: 0,
            support: 0,
            supported: 1,
            plan: rsf_dlss_plan {
                render_width: 640,
                render_height: 360,
                render_width_min: 320,
                render_height_min: 180,
                render_width_max: 640,
                render_height_max: 360,
                ..plan_request(0, 0, 0)
            },
            plan_result: 0,
            release_result: 0,
            decode_create: 0,
            decode_run: 0,
            evaluate: 0,
            layer_evaluate: 0,
            texture_fails: false,
            textures: Vec::new(),
            zeroed_pitch: 0,
            frames: Vec::new(),
            captures: Vec::new(),
            layer_frames: Vec::new(),
            transport: false,
            dumps: Vec::new(),
            reentered: None,
            reenter: false,
            surface_retry: false,
        }
    }
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World::new());
    static PIPE: Cell<*const Pipeline> = const { Cell::new(ptr::null()) };
}

fn with<R>(run: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with_borrow_mut(run)
}

fn call(name: impl Into<String>) {
    with(|world| world.calls.push(name.into()));
}

fn calls(name: &str) -> usize {
    with(|world| world.calls.iter().filter(|call| *call == name).count())
}

fn logged(text: &str) -> usize {
    with(|world| world.logs.iter().filter(|line| line.contains(text)).count())
}

unsafe extern "C" fn sink(_: Raw, message: *const c_char) {
    let line = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    with(|world| world.logs.push(line));
}

pub(super) fn present_host() -> Raw {
    handle(with(|world| world.host))
}
pub(super) unsafe extern "C" fn share_host(_: Raw, _: rsf_dlssp_log_fn, _: Raw) -> i32 {
    call("share_host");
    with(|world| world.load)
}
pub(super) unsafe extern "C" fn load(setup: *const rsf_dlss_setup) -> i32 {
    let setup = unsafe { &*setup };
    assert!(setup.log.is_some() && setup.abi_version == RSF_DLSS_ABI_VERSION);
    let text = |value| {
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned()
    };
    call(format!(
        "load {} {}",
        text(setup.interposer_path_utf8),
        text(setup.plugin_directory_utf8)
    ));
    with(|world| world.load)
}
pub(super) unsafe extern "C" fn set_device(_: Raw) -> i32 {
    call("set_device");
    0
}
pub(super) unsafe extern "C" fn query_support(out: *mut rsf_dlss_support) -> i32 {
    let (result, supported) = with(|world| (world.support, world.supported));
    unsafe {
        (*out).supported = supported;
        (*out).detected_driver_major = 560;
        (*out).required_driver_major = 580;
        (*out).driver_out_of_date = 1;
    }
    result
}
pub(super) unsafe extern "C" fn plan(out: *mut rsf_dlss_plan) -> i32 {
    let request = unsafe { &mut *out };
    assert_eq!(request.struct_size as usize, size_of::<rsf_dlss_plan>());
    let (width, height, quality) = (request.output_width, request.output_height, request.quality);
    call(format!("plan {width}x{height} q{quality}"));
    let (range, result) = with(|world| (world.plan, world.plan_result));
    *request = rsf_dlss_plan {
        output_width: width,
        output_height: height,
        quality,
        ..range
    };
    result
}
pub(super) unsafe extern "C" fn release_resources() -> i32 {
    call("release_resources");
    with(|world| world.release_result)
}
pub(super) unsafe extern "C" fn release_viewport(viewport: u32) -> i32 {
    call(format!("release_viewport {viewport}"));
    0
}
pub(super) unsafe extern "C" fn shutdown() -> i32 {
    call("shutdown");
    0
}
pub(super) unsafe extern "C" fn evaluate_layer(_: Raw, frame: *const DlssFrame) -> i32 {
    let frame = unsafe { *frame };
    with(|world| {
        world.layer_frames.push(frame);
        world.layer_evaluate
    })
}
pub(super) fn preset_valid(preset: u32) -> i32 {
    i32::from(preset == 0 || preset == 5)
}
pub(super) unsafe extern "C" fn fg_evaluate(_: Raw, frame: *const DlssFrame) -> i32 {
    if with(|world| world.reenter) {
        // Another frame-thread entry from inside a leaf, as a vendor callback could make it.
        let result = unsafe { (*PIPE.get()).stop() };
        with(|world| world.reentered = Some(result));
    }
    let frame = unsafe { *frame };
    with(|world| {
        world.frames.push(frame);
        world.evaluate
    })
}
pub(super) unsafe extern "C" fn fg_capture(_: Raw, frame: *const DlssFrame) {
    let frame = unsafe { *frame };
    with(|world| world.captures.push(frame));
}
pub(super) unsafe extern "C" fn native_sr_release() {
    // A surface published from inside the release must not survive it.
    if with(|world| world.surface_retry) {
        crate::native_sr_state::clear_surface();
        let published = unsafe { &*PIPE.get() }.set_surface([1280, 720]);
        call(if published == 0 {
            "native_sr_release"
        } else {
            "surface republished during release"
        });
        return;
    }
    call("native_sr_release");
}
pub(super) unsafe extern "C" fn texture_desc(texture: Raw, out: *mut TextureDesc) {
    let desc = with(|world| {
        world
            .textures
            .iter()
            .find(|(raw, _)| *raw == texture)
            .map(|(_, desc)| *desc)
    })
    .expect("pipeline texture");
    unsafe { out.write(desc) };
}
pub(super) unsafe extern "C" fn create_texture(_: Raw, desc: *const TextureDesc) -> Raw {
    if with(|world| world.texture_fails) {
        return null_mut();
    }
    let texture = fake();
    let desc = unsafe { *desc };
    with(|world| world.textures.push((texture, desc)));
    texture
}
pub(super) unsafe extern "C" fn clear_black(_: Raw, _: Raw) {
    call("clear");
}
pub(super) unsafe extern "C" fn create_zeroed(_: Raw, desc: *const TextureDesc, pitch: u32) -> Raw {
    let texture = fake();
    let desc = unsafe { *desc };
    with(|world| {
        world.zeroed_pitch = pitch;
        world.textures.push((texture, desc));
    });
    texture
}
pub(super) unsafe extern "C" fn decode_create(
    _: Raw,
    width: u32,
    height: u32,
    format: u32,
    log: rsf_dlssp_log_fn,
    out: *mut Raw,
) -> i32 {
    assert!(log.is_some());
    call(format!("decode_create {width}x{height} format {format}"));
    let result = with(|world| world.decode_create);
    unsafe {
        out.write(if result == 0 {
            handle(0x1000)
        } else {
            null_mut()
        })
    };
    result
}
pub(super) unsafe extern "C" fn decode_run(_: Raw, _: Raw, _: Raw, params: *const DecodeParams) -> i32 {
    assert_eq!(unsafe { (*params).output_scale_x }, 1.0);
    with(|world| world.decode_run)
}
pub(super) unsafe extern "C" fn decode_texture(_: Raw) -> Raw {
    handle(DECODED)
}
pub(super) unsafe extern "C" fn decode_destroy(_: Raw) {
    call("decode_destroy");
}
pub(super) unsafe extern "C" fn resolve_create(_: Raw, width: u32, height: u32) -> Raw {
    call(format!("resolve_create {width}x{height}"));
    handle(0x3000)
}
pub(super) unsafe extern "C" fn resolve_run(
    _: Raw,
    _: Raw,
    motion: Raw,
    depth: Raw,
    params: *const rsf_motion_resolve_params,
) -> i32 {
    assert_eq!((motion, depth), (handle(DECODED), handle(DEPTH)));
    assert_eq!(unsafe { (*params).has_sentinel }, 1);
    1
}
pub(super) unsafe extern "C" fn resolve_motion(_: Raw) -> Raw {
    handle(RESOLVED_MOTION)
}
pub(super) unsafe extern "C" fn resolve_depth(_: Raw) -> Raw {
    handle(RESOLVED_DEPTH)
}
pub(super) unsafe extern "C" fn resolve_destroy(_: Raw) {
    call("resolve_destroy");
}
pub(super) unsafe extern "C" fn fidelity_create(_: Raw) -> Raw {
    null_mut()
}
pub(super) unsafe extern "C" fn fidelity_run(
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: u32,
    _: *const f32,
) -> i32 {
    1
}
pub(super) unsafe extern "C" fn fidelity_destroy(_: Raw) {}
pub(super) unsafe extern "C" fn sharpen_destroy(_: Raw) {}
pub(super) unsafe extern "C" fn transport_create(_: Raw) -> Raw {
    if with(|world| world.transport) {
        handle(0x4000)
    } else {
        null_mut()
    }
}
pub(super) unsafe extern "C" fn transport_encode(
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: u32,
    _: u32,
    out: *mut Raw,
) -> i32 {
    unsafe { out.write(handle(ENCODED)) };
    1
}
pub(super) unsafe extern "C" fn transport_decode(
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: u32,
    _: u32,
    _: Raw,
    _: u32,
    _: u32,
    _: *const f32,
) -> i32 {
    call("transport_decode");
    1
}
pub(super) unsafe extern "C" fn transport_destroy(_: Raw) {
    call("transport_destroy");
}
pub(super) unsafe extern "C" fn sharpen(
    _: *mut Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: *const Sharpening,
    _: *const rsf_dlssp_sharpen_frame,
) -> i32 {
    1
}
pub(super) unsafe extern "C" fn alternate_create(
    _: Raw,
    width: u32,
    height: u32,
    directories: *const *const c_char,
    _: f32,
    _: u32,
    _: u32,
    _: rsf_dlssp_log_fn,
    _: Raw,
    out: *mut Raw,
) -> i32 {
    let fsr3 = unsafe { CStr::from_ptr(*directories.add(1)) };
    call(format!(
        "alternate_create {width}x{height} {}",
        fsr3.to_string_lossy()
    ));
    unsafe { out.write(legacy::adapter().cast()) };
    0
}
pub(super) unsafe extern "C" fn input_dump(
    _: Raw,
    _: *const c_char,
    _: *const c_char,
    _: Raw,
    _: u32,
    _: f32,
) {
}
unsafe fn dump(kind: &'static str, prefix: *const c_char, view: u32, texture: Raw) -> i32 {
    let prefix = unsafe { CStr::from_ptr(prefix) }
        .to_string_lossy()
        .into_owned();
    with(|world| world.dumps.push((prefix, view, kind, texture)));
    0
}
pub(super) unsafe extern "C" fn dump_bytes(
    _: Raw,
    _: Raw,
    texture: Raw,
    prefix: *const c_char,
    view: u32,
    _: f32,
    _: rsf_dlssp_log_fn,
) -> i32 {
    unsafe { dump("bytes", prefix, view, texture) }
}
pub(super) unsafe extern "C" fn dump_image(
    _: Raw,
    _: Raw,
    texture: Raw,
    prefix: *const c_char,
    view: u32,
    _: f32,
    _: rsf_dlssp_log_fn,
) -> i32 {
    unsafe { dump("image", prefix, view, texture) }
}

fn setup() -> Setup {
    let mut motion: DecodeParams = unsafe { std::mem::zeroed() };
    motion.struct_size = size_of::<DecodeParams>() as u32;
    motion.scale_x = 1.0;
    motion.scale_y = 1.0;
    motion.zero_means_unwritten = 1;
    motion.invalid_value = -1000.0;
    Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: ABI,
        streamline_directory_utf8: c"C:\\sl\\/".as_ptr(),
        streamline_log_directory_utf8: ptr::null(),
        require_signature: 1,
        output_width: 1280,
        output_height: 720,
        quality: 2,
        motion,
        log: Some(sink),
        log_user: null_mut(),
        engine: 0,
        engine_version_utf8: c"fixture".as_ptr(),
        project_id_utf8: c"project".as_ptr(),
        fsr2_directory_utf8: ptr::null(),
        fsr3_directory_utf8: c"C:\\ffx".as_ptr(),
        fsr4_directory_utf8: ptr::null(),
        xess_directory_utf8: ptr::null(),
        view_space_to_meters: 1.0,
        standard_depth: 0,
    }
}

fn camera() -> Camera {
    let mut camera: Camera = unsafe { std::mem::zeroed() };
    camera.struct_size = size_of::<Camera>() as u32;
    camera.abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION;
    camera.has_jitter = 1;
    camera.jitter_pixels = [0.25, -0.25];
    camera.near_plane = 0.1;
    camera.vertical_fov = 1.0;
    camera.aspect_ratio = 1.0;
    camera
}

fn frame(camera: &Camera, render: [u32; 2]) -> Frame {
    Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: ABI,
        scene_color: handle(SCENE),
        depth: handle(DEPTH),
        game_motion: handle(MOTION),
        exposure: null_mut(),
        render_width: render[0],
        render_height: render[1],
        camera,
        ..unsafe { std::mem::zeroed() }
    }
}

fn status(pipeline: &Pipeline) -> Status {
    let mut status = Status {
        struct_size: size_of::<Status>() as u32,
        ..Default::default()
    };
    assert_eq!(unsafe { pipeline.status(&mut status) }, OK);
    status
}

// A started pipeline over a fresh fake world. The device outlives the test.
fn started(pipeline: &Pipeline) -> Raw {
    PIPE.set(pipeline);
    let device = fake();
    assert_eq!(unsafe { pipeline.start(device, &setup()) }, OK);
    device
}

fn run(pipeline: &Pipeline, frame: &Frame) -> i32 {
    unsafe { pipeline.on_frame(handle(CONTEXT), frame) }
}

#[test]
fn start_validates_plans_and_stop_releases_every_lease() {
    let pipeline = Pipeline::new();
    let device = fake();
    let mut request = setup();
    unsafe {
        assert_eq!(pipeline.start(null_mut(), &request), INVALID);
        request.struct_size -= 4;
        assert_eq!(pipeline.start(device, &request), INVALID);
        request = setup();
        request.abi_version = 4;
        assert_eq!(pipeline.start(device, &request), ABI_MISMATCH);
        request = setup();
        request.motion.scale_y = 0.0;
        assert_eq!(pipeline.start(device, &request), INVALID);
    }
    assert!(
        with(|world| world.calls.is_empty()),
        "refusals touch nothing"
    );

    assert_eq!(unsafe { pipeline.start(device, &setup()) }, OK);
    assert_eq!(refs(device), 2, "the pipeline retains the game's device");
    let state = status(&pipeline);
    assert_eq!(
        (
            state.running,
            state.dlss_supported,
            state.render_width,
            state.render_height
        ),
        (1, 1, 640, 360)
    );
    assert_eq!(
        (state.output_width, state.output_height, state.backend),
        (1280, 720, RSF_SR_DLSS)
    );
    assert_eq!(pipeline.set_surface([1280, 720]), 1);
    assert_eq!(pipeline.set_surface([1280, 721]), 0);
    assert_eq!(pipeline.set_surface([0, 720]), 0);
    assert_eq!(
        with(|world| world.calls.clone()),
        [
            "load C:\\sl\\sl.interposer.dll C:\\sl",
            "set_device",
            "plan 1280x720 q2",
            "clear"
        ]
    );
    assert_eq!(logged("loading streamline from C:\\sl"), 1);
    assert_eq!(
        logged("pipeline ready: backend 1 renders 640x360 for 1280x720"),
        1
    );
    let output = pipeline.output_texture();
    let desc = with(|world| world.textures[0].1);
    assert_eq!(output, with(|world| world.textures[0].0));
    assert_eq!(
        (desc.width, desc.height, desc.format),
        (1280, 720, RSF_D3D11_FORMAT_RGBA16_FLOAT)
    );
    assert_eq!(desc.bind_flags, 32 | 8 | 128);
    assert_eq!(
        unsafe { pipeline.start(device, &setup()) },
        ALREADY_RUNNING
    );

    with(|world| {
        world.calls.clear();
        world.surface_retry = true;
    });
    PIPE.set(&pipeline);
    assert_eq!(unsafe { pipeline.stop() }, OK);
    PIPE.set(ptr::null());
    with(|world| world.surface_retry = false);
    assert_eq!(
        with(|world| world.calls.clone()),
        [
            "native_sr_release",
            "release_resources",
            "release_viewport 1",
            "shutdown"
        ]
    );
    assert_eq!((refs(device), refs(output)), (1, 0));
    assert!(pipeline.output_texture().is_null());
    assert_eq!(status(&pipeline).running, 0);
    assert_eq!(pipeline.set_surface([1280, 720]), 0);
    let lines = with(|world| world.logs.len());
    assert_eq!(unsafe { pipeline.set_preset(5) }, OK);
    assert_eq!(with(|world| world.logs.len()), lines, "no line after stop");
    assert_eq!(unsafe { pipeline.stop() }, NOT_RUNNING);
}

#[test]
fn refused_start_tears_down_what_it_acquired() {
    let pipeline = Pipeline::new();
    let device = fake();
    with(|world| world.support = DLSS_NOT_SUPPORTED);
    assert_eq!(
        unsafe { pipeline.start(device, &setup()) },
        NOT_SUPPORTED
    );
    assert_eq!(refs(device), 1);
    assert_eq!(calls("shutdown"), 1);
    assert_eq!(
        logged("driver 560.0 against a required 580.0, driver out of date"),
        1
    );
    assert_eq!(status(&pipeline).running, 0);

    with(|world| {
        world.support = 0;
        world.load = -4;
        world.host = 0x77;
        world.calls.clear();
    });
    assert_eq!(
        unsafe { pipeline.start(device, &setup()) },
        STREAMLINE_FAILED
    );
    assert_eq!(
        with(|world| world.calls.clone()),
        ["share_host", "native_sr_release"]
    );
    assert_eq!(refs(device), 1);

    with(|world| {
        world.load = 0;
        world.texture_fails = true;
    });
    assert_eq!(
        unsafe { pipeline.start(device, &setup()) },
        RESOURCE_FAILED
    );
    assert_eq!(refs(device), 1);
    assert_eq!(logged("the output target could not be created"), 1);
}

#[test]
fn frames_decode_resolve_and_carry_history_resets() {
    let pipeline = Pipeline::new();
    started(&pipeline);
    let camera = camera();
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), OK);
    assert_eq!(calls("decode_create 640x360 format 16"), 1);
    assert_eq!(calls("resolve_create 640x360"), 1);
    let first = with(|world| world.frames[0]);
    assert_eq!(first.reset, 1, "a rebuilt feature starts without history");
    assert_eq!(
        (first.motion, first.depth),
        (handle(RESOLVED_MOTION), handle(RESOLVED_DEPTH))
    );
    assert_eq!(first.camera_motion_included, 1);
    assert_eq!(
        (first.motion_scale_x, first.motion_scale_y),
        (1.0 / 640.0, 1.0 / 360.0)
    );
    assert_eq!(
        (first.color_in, first.color_out),
        (handle(SCENE), pipeline.output_texture())
    );
    assert_eq!(first.motion_invalid_value, -1000.0);
    assert_eq!(
        logged("DLSS colour transport could not create its GPU passes"),
        1
    );
    assert_eq!(with(|world| world.captures.len()), 1);

    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), OK);
    assert_eq!(with(|world| world.frames[1].reset), 0);
    assert_eq!(unsafe { pipeline.set_preset(7) }, INVALID);
    assert_eq!(unsafe { pipeline.set_preset(5) }, OK);
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), OK);
    let preset = with(|world| world.frames[2]);
    assert_eq!((preset.reset, preset.preset), (1, 5));

    // Outside the planned range: refused, counted, said once.
    for _ in 0..2 {
        assert_eq!(run(&pipeline, &frame(&camera, [100, 100])), INVALID);
    }
    assert_eq!(
        logged("frame renders at 100x100, outside the 320x180 to 640x360"),
        1
    );
    let state = status(&pipeline);
    assert_eq!(
        (
            state.frames_evaluated,
            state.frames_refused,
            state.last_result
        ),
        (3, 2, INVALID)
    );

    // A render size inside the range rebuilds the decode and the feature.
    with(|world| world.calls.clear());
    assert_eq!(run(&pipeline, &frame(&camera, [480, 270])), OK);
    assert_eq!(
        &with(|world| world.calls.clone())[..4],
        [
            "decode_destroy",
            "release_resources",
            "decode_create 480x270 format 16",
            "resolve_create 480x270"
        ]
    );
    assert_eq!(with(|world| world.frames[3].reset), 1);

    // Uncounted refusals.
    let mut stale = camera;
    stale.abi_version = 2;
    assert_eq!(run(&pipeline, &frame(&stale, [480, 270])), ABI_MISMATCH);
    let mut short = frame(&camera, [480, 270]);
    short.struct_size -= 8;
    assert_eq!(run(&pipeline, &short), INVALID);
    assert_eq!(
        status(&pipeline).frames_evaluated + status(&pipeline).frames_refused,
        6
    );
}

#[test]
fn transport_replaces_colour_and_exposure_and_decodes_the_output() {
    let pipeline = Pipeline::new();
    with(|world| world.transport = true);
    started(&pipeline);
    let camera = camera();
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), OK);
    let frame = with(|world| world.frames[0]);
    assert_eq!((frame.color_in, frame.color_encoded), (handle(ENCODED), 1));
    assert!(frame.exposure.is_null());
    assert_eq!(calls("transport_decode"), 1);
    assert_eq!(unsafe { pipeline.stop() }, OK);
    assert_eq!(calls("transport_destroy"), 1);
}

#[test]
fn repeated_failures_give_up_until_a_setting_restarts_history() {
    let pipeline = Pipeline::new();
    started(&pipeline);
    let camera = camera();
    with(|world| world.evaluate = -9);
    for _ in 0..FAILURE_LIMIT {
        assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), EVALUATE_FAILED);
    }
    assert_eq!(logged("SR did not evaluate this frame (result -9)"), 1);
    assert_eq!(
        logged("SR failed to evaluate 120 frames in a row (last result -9)"),
        1
    );
    assert_eq!(
        logged(
            "a capture layer such as RenderDoc is the usual reason: it wraps the device and NGX refuses it"
        ),
        1
    );
    pipeline.say("x".repeat(600));
    assert_eq!(
        with(|world| world.logs.last().map(String::len)),
        Some(LINE_LIMIT),
        "cut where rsf_logf cuts it"
    );
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), EVALUATE_FAILED);
    assert_eq!(
        with(|world| world.frames.len()),
        FAILURE_LIMIT as usize,
        "given up: not asked"
    );
    assert_eq!(with(|world| world.captures.len()), 0);

    with(|world| world.evaluate = 0);
    assert_eq!(unsafe { pipeline.set_preset(5) }, OK);
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), OK);
    assert_eq!(with(|world| world.frames.last().map(|f| f.reset)), Some(1));
    assert_eq!(
        status(&pipeline).frames_refused,
        u64::from(FAILURE_LIMIT) + 1
    );
}

#[test]
fn a_decode_that_cannot_build_is_not_retried_for_the_same_size() {
    let pipeline = Pipeline::new();
    started(&pipeline);
    let camera = camera();
    with(|world| world.decode_create = -3);
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), RESOURCE_FAILED);
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), RESOURCE_FAILED);
    assert_eq!(calls("decode_create 640x360 format 16"), 1);
    assert_eq!(
        logged("the motion decode pass could not be built (result -3)"),
        1
    );
    with(|world| world.decode_create = 0);
    assert_eq!(run(&pipeline, &frame(&camera, [480, 270])), OK);
}

#[test]
fn layer_feature_owns_its_output_motion_and_history() {
    let pipeline = Pipeline::new();
    let mut output = null_mut();
    unsafe {
        assert_eq!(
            pipeline.prepare_layer(0, 4, &mut output),
            INVALID
        );
        assert_eq!(
            pipeline.prepare_layer(256, 128, &mut output),
            NOT_RUNNING
        );
    }
    started(&pipeline);
    assert_eq!(
        unsafe { pipeline.prepare_layer(256, 128, &mut output) },
        OK
    );
    assert_eq!(output, pipeline.layer_texture());
    assert_eq!(with(|world| world.zeroed_pitch), 1024);
    let motion = with(|world| world.textures[2]);
    assert_eq!(
        (motion.1.format, motion.1.bind_flags),
        (RSF_D3D11_FORMAT_RG16_FLOAT, 8)
    );
    let state = status(&pipeline);
    assert_eq!((state.layer_width, state.layer_height), (256, 128));
    let mut again = null_mut();
    assert_eq!(
        unsafe { pipeline.prepare_layer(256, 128, &mut again) },
        OK
    );
    assert_eq!(again, output, "the same lease while the size holds");

    let camera = camera();
    let mut layer = Layer {
        struct_size: size_of::<Layer>() as u32,
        abi_version: ABI,
        color: handle(0x6000),
        depth: handle(0x6100),
        width: 255,
        height: 128,
        camera: &camera,
    };
    let context = handle(CONTEXT);
    assert_eq!(
        unsafe { pipeline.on_layer(context, &layer) },
        INVALID
    );
    layer.width = 256;
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), OK);
    for reset in [1, 0] {
        assert_eq!(unsafe { pipeline.on_layer(context, &layer) }, OK);
        let submitted = with(|world| *world.layer_frames.last().unwrap());
        assert_eq!(
            (submitted.viewport, submitted.alpha, submitted.reset),
            (1, 1, reset)
        );
        assert_eq!(submitted.frame_index, 1, "the scene frames seen so far");
        assert_eq!(
            (submitted.color_out, submitted.quality),
            (output, QUALITY_NATIVE)
        );
    }
    let state = status(&pipeline);
    assert_eq!(
        (state.layer_frames_evaluated, state.layer_frames_refused),
        (2, 1)
    );

    let mut moved = null_mut();
    assert_eq!(
        unsafe { pipeline.prepare_layer(512, 256, &mut moved) },
        OK
    );
    assert_ne!(moved, output);
    assert_eq!(calls("release_viewport 1"), 1);
    assert_eq!(refs(output), 0);
}

#[test]
fn resize_prepares_before_commit_and_refusal_keeps_identity() {
    let pipeline = Pipeline::new();
    started(&pipeline);
    let (mut width, mut height) = (0, 0);
    let output = pipeline.output_texture();
    let clears = calls("clear");
    unsafe {
        assert_eq!(
            pipeline.resize_output(0, 720, &mut width, &mut height),
            INVALID
        );
        assert_eq!(
            pipeline.resize_output(16385, 720, &mut width, &mut height),
            INVALID
        );
        assert_eq!(
            pipeline.resize_output(1280, 720, &mut width, &mut height),
            OK
        );
    }
    assert_eq!((width, height), (640, 360));
    with(|world| world.plan_result = -8);
    assert_eq!(
        unsafe { pipeline.resize_output(1920, 1080, &mut width, &mut height) },
        STREAMLINE_FAILED
    );
    assert_eq!(pipeline.output_texture(), output);
    assert_eq!(calls("clear"), clears);
    assert_eq!(status(&pipeline).output_width, 1280);

    with(|world| {
        world.plan_result = 0;
        world.plan.render_width = 960;
        world.plan.render_height = 540;
        world.plan.render_width_max = 960;
        world.plan.render_height_max = 540;
    });
    let camera = camera();
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), OK);
    assert_eq!(
        unsafe { pipeline.resize_output(1920, 1080, &mut width, &mut height) },
        OK
    );
    assert_eq!((width, height), (960, 540));
    let resized = pipeline.output_texture();
    assert_ne!(resized, output);
    assert_eq!(calls("clear"), clears + 1);
    assert_eq!(refs(output), 0);
    assert_eq!(with(|world| world.textures[1].1.width), 1920);
    let state = status(&pipeline);
    assert_eq!((state.output_width, state.render_width), (1920, 960));
    assert_eq!(run(&pipeline, &frame(&camera, [960, 540])), OK);
    let submitted = with(|world| *world.frames.last().unwrap());
    assert_eq!(
        (submitted.reset, submitted.color_out, submitted.output_width),
        (1, resized, 1920)
    );
}

#[test]
fn resize_leaves_a_cold_viewport_alone_until_dlss_has_seen_a_frame() {
    let pipeline = Pipeline::new();
    started(&pipeline);
    with(|world| world.release_result = -9);

    let mut width = 0;
    let mut height = 0;
    assert_eq!(
        unsafe { pipeline.resize_output(1600, 900, &mut width, &mut height) },
        OK
    );
    assert_eq!(calls("release_resources"), 0);

    let camera = camera();
    assert_eq!(run(&pipeline, &frame(&camera, [width, height])), OK);
    let output = pipeline.output_texture();
    assert_eq!(
        unsafe { pipeline.resize_output(1920, 1080, &mut width, &mut height) },
        STREAMLINE_FAILED
    );
    assert_eq!(calls("release_resources"), 1);
    assert_eq!(pipeline.output_texture(), output);
    assert_eq!(status(&pipeline).output_width, 1600);
}

#[test]
fn backend_switches_reuse_the_legacy_owner_and_refusals_keep_the_active_backend() {
    let pipeline = Pipeline::new();
    started(&pipeline);
    let (mut width, mut height) = (0, 0);
    unsafe {
        assert_eq!(
            pipeline.select_backend(9, &mut width, &mut height),
            INVALID
        );
        legacy::SELECT.set(-7);
        assert_eq!(
            pipeline.select_backend(3, &mut width, &mut height),
            NOT_SUPPORTED
        );
    }
    let state = status(&pipeline);
    assert_eq!(
        (
            state.backend,
            state.requested_backend,
            state.last_switch_result
        ),
        (RSF_SR_DLSS, 3, -7)
    );
    assert_eq!(calls("alternate_create 1280x720 C:\\ffx"), 1);

    legacy::SELECT.set(0);
    assert_eq!(
        unsafe { pipeline.select_backend(3, &mut width, &mut height) },
        OK
    );
    assert_eq!((width, height), (320, 180));
    assert_eq!(
        calls("alternate_create 1280x720 C:\\ffx"),
        1,
        "the adapter is kept"
    );
    assert_eq!(logged("backend 3 prepared in"), 2);
    let camera = camera();
    let evaluations = legacy::EVALUATIONS.get();
    assert_eq!(run(&pipeline, &frame(&camera, [320, 180])), OK);
    assert_eq!(legacy::EVALUATIONS.get(), evaluations + 1);
    assert!(
        with(|world| world.frames.is_empty()),
        "the DLSS feature is not asked"
    );
    let capture = with(|world| world.captures[0]);
    assert_eq!(
        (capture.depth, capture.motion),
        (legacy::DEPTH, legacy::MOTION)
    );
    assert_eq!(capture.camera_motion_included, 1);
    let mut layer = null_mut();
    assert_eq!(
        unsafe { pipeline.prepare_layer(64, 64, &mut layer) },
        OK
    );
    let layer_frame = Layer {
        struct_size: size_of::<Layer>() as u32,
        abi_version: ABI,
        color: handle(0x6000),
        depth: handle(0x6100),
        width: 64,
        height: 64,
        camera: &camera,
    };
    assert_eq!(
        unsafe { pipeline.on_layer(handle(CONTEXT), &layer_frame) },
        NOT_RUNNING,
        "the layer feature is DLSS only"
    );

    legacy::RENDER.set([400, 225]);
    assert_eq!(
        unsafe { pipeline.set_quality(1, &mut width, &mut height) },
        OK
    );
    assert_eq!((status(&pipeline).render_width, width), (400, 400));

    with(|world| {
        world.calls.clear();
        world.plan.render_width_min = 0;
        world.plan.render_height_min = 0;
        world.plan.render_width_max = 0;
        world.plan.render_height_max = 0;
    });
    assert_eq!(
        unsafe { pipeline.select_backend(RSF_SR_DLSS, &mut width, &mut height) },
        OK
    );
    assert_eq!(with(|world| world.calls.clone()), ["plan 1280x720 q1"]);
    assert_eq!(status(&pipeline).backend, RSF_SR_DLSS);
    assert_eq!(run(&pipeline, &frame(&camera, [640, 360])), OK);
    let destroyed = legacy::DESTROYED.get();
    assert_eq!(unsafe { pipeline.stop() }, OK);
    assert_eq!(legacy::DESTROYED.get(), destroyed + 1);
    assert_eq!(status(&pipeline).backend, RSF_SR_DLSS);
}

#[test]
fn reentrant_frame_thread_calls_refuse_instead_of_racing() {
    let pipeline = Pipeline::new();
    started(&pipeline);
    with(|world| world.reenter = true);
    assert_eq!(run(&pipeline, &frame(&camera(), [640, 360])), OK);
    assert_eq!(with(|world| world.reentered), Some(NOT_RUNNING));
    assert_eq!(status(&pipeline).running, 1, "the nested stop did nothing");
    // The cross-thread entries never wait on the frame owner.
    assert_eq!(unsafe { pipeline.request_dump(c"x".as_ptr()) }, OK);
}

#[test]
fn request_dump_writes_the_evaluated_frame_inputs_and_record() {
    let pipeline = Pipeline::new();
    assert_eq!(unsafe { pipeline.request_dump(c"x".as_ptr()) }, NOT_RUNNING);
    with(|world| world.transport = true);
    started(&pipeline);
    assert_eq!(unsafe { pipeline.request_dump(c"".as_ptr()) }, INVALID);
    let directory = std::env::temp_dir().join(format!("rsf-sr-pipeline-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let prefix = directory.join("frame").to_string_lossy().into_owned();
    let prefix_c = CString::new(prefix.clone()).unwrap();
    assert_eq!(unsafe { pipeline.request_dump(prefix_c.as_ptr()) }, OK);
    let camera = camera();
    let mut input = frame(&camera, [640, 360]);
    input.exposure = handle(0x4200);
    assert_eq!(run(&pipeline, &input), OK);
    let dumps = with(|world| world.dumps.clone());
    let named = |suffix: &str, kind: &str| {
        dumps
            .iter()
            .filter(|(path, _, k, _)| *path == format!("{prefix}{suffix}") && *k == kind)
            .count()
    };
    for suffix in [
        "_input",
        "_input_submitted",
        "_output",
        "_motion",
        "_motion_decoded",
        "_motion_submitted",
    ] {
        assert_eq!(
            (named(suffix, "bytes"), named(suffix, "image")),
            (1, 1),
            "{suffix}"
        );
    }
    assert_eq!((named("_depth", "bytes"), named("_depth", "image")), (1, 0));
    assert_eq!(
        (
            named("_depth_submitted", "bytes"),
            named("_depth_submitted", "image")
        ),
        (1, 0)
    );
    let raw = |suffix: &str| {
        dumps
            .iter()
            .find(|(path, _, kind, _)| *path == format!("{prefix}{suffix}") && *kind == "bytes")
            .map(|(_, _, _, texture)| *texture)
    };
    assert_eq!(raw("_input"), Some(input.scene_color));
    assert_eq!(raw("_input_submitted"), Some(handle(ENCODED)));
    assert_eq!(raw("_depth"), Some(input.depth));
    assert_eq!(raw("_depth_submitted"), Some(handle(RESOLVED_DEPTH)));
    assert_eq!(raw("_motion_submitted"), Some(handle(RESOLVED_MOTION)));
    assert_eq!(raw("_exposure"), Some(input.exposure));
    assert_eq!(raw("_exposure_submitted"), None);

    let record = std::fs::read_to_string(format!("{prefix}_frame.json")).unwrap();
    assert!(
        record.starts_with(
            "{\"render\":[640,360],\"output\":[1280,720],\"backend\":1,\"jitter\":[0.25,-0.25],\
\"motion_scale\":[0.00156250002,0.00277777785],\"camera_motion_included\":1,\"reset\":1,\"clip_to_previous\":[0,"
        ),
        "{record}"
    );
    let _ = std::fs::remove_dir_all(directory);
    // One request, one dump.
    assert_eq!(run(&pipeline, &input), OK);
    assert_eq!(with(|world| world.dumps.len()), dumps.len());
}

#[test]
fn sharpening_and_preset_settings_are_validated_through_their_owners() {
    let pipeline = Pipeline::new();
    let mut options = sharpening_policy::defaults();
    options.struct_size = 0;
    options.abi_version = 0;
    unsafe {
        assert_eq!(pipeline.set_sharpening(&options), INVALID);
        options.struct_size = size_of::<Sharpening>() as u32;
        assert_eq!(pipeline.set_sharpening(&options), ABI_MISMATCH);
        options.abi_version = RSF_SHARPENING_ABI_VERSION;
        options.sharpness = 2.0;
        assert_eq!(pipeline.set_sharpening(&options), INVALID);
        options.sharpness = 0.5;
        assert_eq!(pipeline.set_sharpening(&options), OK);
    }
    assert_eq!(pipeline.shared().sharpening.map(|o| o.sharpness), Some(0.5));
}

#[test]
fn ranges_default_only_where_the_sdk_reported_none() {
    let plan = rsf_dlss_plan {
        render_width: 10,
        render_height: 20,
        ..plan_request(0, 0, 0)
    };
    assert_eq!(Range::of(plan).min, [10, 20]);
    assert_eq!(Range::of(plan).max, [10, 20]);
}

#[test]
fn alternate_start_skips_streamline_and_can_lazily_switch_to_dlss() {
    let pipeline = Pipeline::new();
    let device = fake();
    with(|world| world.load = -4);
    assert_eq!(
        unsafe { pipeline.start_backend(device, &setup(), 3) },
        OK
    );
    assert_eq!(
        (status(&pipeline).backend, status(&pipeline).dlss_supported),
        (3, 0)
    );
    assert!(!with(|world| world
        .calls
        .iter()
        .any(|call| call.starts_with("load"))));
    let camera = camera();
    let frame = frame(&camera, [320, 180]);
    assert_eq!(run(&pipeline, &frame), OK);
    let output = pipeline.output_texture();
    assert_eq!(
        unsafe { pipeline.select_backend(RSF_SR_DLSS, null_mut(), null_mut()) },
        NOT_SUPPORTED
    );
    assert_eq!(status(&pipeline).backend, 3);
    assert_eq!(pipeline.output_texture(), output);
    assert_eq!(run(&pipeline, &frame), OK);
    with(|world| world.load = 0);
    assert_eq!(
        unsafe { pipeline.select_backend(RSF_SR_DLSS, null_mut(), null_mut()) },
        OK
    );
    assert_eq!(
        (status(&pipeline).backend, status(&pipeline).dlss_supported),
        (RSF_SR_DLSS, 1)
    );
    assert_eq!(pipeline.output_texture(), output);
    assert_eq!(unsafe { pipeline.stop() }, OK);
}

#[test]
fn alternate_quality_change_restarts_after_failure_limit() {
    let pipeline = Pipeline::new();
    assert_eq!(
        unsafe { pipeline.start_backend(fake(), &setup(), 3) },
        OK
    );
    let camera = camera();
    let frame = frame(&camera, [320, 180]);
    legacy::EVALUATE_RESULT.set(-7);
    for _ in 0..FAILURE_LIMIT {
        assert_eq!(run(&pipeline, &frame), EVALUATE_FAILED);
    }
    assert!(pipeline.shared().given_up);
    legacy::EVALUATE_RESULT.set(0);
    assert_eq!(
        unsafe { pipeline.set_quality(1, null_mut(), null_mut()) },
        OK
    );
    assert!(!pipeline.shared().given_up);
    assert_eq!(run(&pipeline, &frame), OK);
    assert_eq!(unsafe { pipeline.stop() }, OK);
}

#[test]
fn production_motion_scale_reconstructs_the_previous_feature_location() {
    for (current, previous) in [(0.4f32, 0.5f32), (0.3, 0.7)] {
        let raw_motion = previous - current;
        let raw_scale = dlss_motion_scale(-1.0, true, false);
        let dense_scale = dlss_motion_scale(0.001, true, true);
        assert!((current + raw_motion * raw_scale - previous).abs() < 1e-6);
        assert!((current + raw_motion * 1000.0 * dense_scale - previous).abs() < 1e-6);
    }
    assert_eq!(dlss_motion_scale(0.5, false, false), 0.5);
}

#[test]
fn dlss_motion_scale_reaches_the_previous_feature_location() {
    for (current, previous) in [(0.4_f32, 0.5_f32), (0.3, 0.7)] {
        let native = previous - current;
        let direct = current + native * dlss_motion_scale(-1.0, true, false);
        let dense = current + native * 1000.0 * dlss_motion_scale(0.001, true, true);
        assert!((direct - previous).abs() < 1e-6);
        assert!((dense - previous).abs() < 1e-6);
    }
    assert_eq!(dlss_motion_scale(0.5, false, false), 0.5);
}
