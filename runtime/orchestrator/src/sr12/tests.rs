// SPDX-License-Identifier: MIT
// Synthetic D3D12 SR contracts: recording native fakes and reference-counted fake COM objects.
// No D3D12 device, shader compiler or vendor SDK is involved, so these establish admission,
// ownership, ordering, history and handoff policy only.
use super::*;
use crate::com_ref::test::{Fake as ComHeader, release};
use std::cell::{Cell, RefCell};

// Read only through the COM calls in com_ref.cpp.
#[repr(C)]
struct Fake {
    com: ComHeader,
    name: String,
    desc: TextureDesc,
    device: Cell<usize>,
}

// The header is the first field of the enclosing fake allocation.
unsafe fn last_release(header: *mut ComHeader) {
    event(format!(
        "release {}",
        unsafe { &*header.cast::<Fake>() }.name
    ));
}

fn fake(name: &str, desc: TextureDesc, device: Handle) -> Handle {
    Box::into_raw(Box::new(Fake {
        com: ComHeader::with_last_release(Some(last_release)),
        name: name.to_owned(),
        desc,
        device: Cell::new(device as usize),
    }))
    .cast()
}

fn object<'a>(handle: Handle) -> &'a Fake {
    unsafe { &*handle.cast::<Fake>() }
}

fn name(handle: Handle) -> String {
    if handle.is_null() {
        "null".into()
    } else {
        object(handle).name.clone()
    }
}

fn texture(width: u64, height: u32, format: u32, srv_format: u32) -> TextureDesc {
    TextureDesc {
        width,
        height,
        dimension: TEXTURE2D,
        array_size: 1,
        samples: 1,
        format,
        srv_format,
    }
}

#[derive(Default)]
struct World {
    events: Vec<String>,
    logs: Vec<String>,
    device: usize,
    created: usize,
    vendor_result: i32,
    plan: (u32, u32),
    capture_enabled: bool,
    capture_wants: bool,
    sharpen_result: i32,
    readback: Vec<u8>,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World { sharpen_result: 1, ..World::default() });
}

fn event(text: String) {
    WORLD.with(|world| world.borrow_mut().events.push(text));
}

fn take_events() -> Vec<String> {
    WORLD.with(|world| std::mem::take(&mut world.borrow_mut().events))
}

fn with<T>(f: impl FnOnce(&mut World) -> T) -> T {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}

fn created(prefix: &str, desc: TextureDesc) -> Handle {
    let (device, index) = with(|world| {
        world.created += 1;
        (world.device, world.created)
    });
    fake(&format!("{prefix}{index}"), desc, device as Handle)
}

const DLSS_CONTEXT: usize = 0x5100;
const SESSION: usize = 0x5200;
const CAPTURE: usize = 0x5300;
const SHARPEN: usize = 0x5400;

pub(super) unsafe extern "C" fn dlss_create(
    _: Handle,
    _: *const rsf_dlss_setup,
    out: *mut Handle,
) -> i32 {
    event("dlss-create".into());
    unsafe { out.write(DLSS_CONTEXT as Handle) };
    OK
}
pub(super) unsafe extern "C" fn dlss_create_shared(
    _: Handle,
    _: *const rsf_dlss_setup,
    _: Handle,
    out: *mut Handle,
) -> i32 {
    event("dlss-create-shared".into());
    unsafe { out.write(DLSS_CONTEXT as Handle) };
    OK
}
pub(super) unsafe extern "C" fn dlss_create_registered(
    _: Handle,
    _: *const rsf_dlss_setup,
    _: Handle,
    out: *mut Handle,
) -> i32 {
    event("dlss-create-registered".into());
    unsafe { out.write(DLSS_CONTEXT as Handle) };
    OK
}
pub(super) unsafe extern "C" fn dlss_plan(
    _: Handle,
    w: u32,
    h: u32,
    q: u32,
    rw: *mut u32,
    rh: *mut u32,
) -> i32 {
    event(format!("dlss-plan {w}x{h} q{q}"));
    let (width, height) = with(|world| world.plan);
    unsafe {
        rw.write(width);
        rh.write(height);
    }
    OK
}
pub(super) unsafe extern "C" fn dlss_set_sdr(_: Handle, sdr: u32) {
    event(format!("dlss-sdr {sdr}"));
}
pub(super) unsafe extern "C" fn dlss_evaluate(
    _: Handle,
    list: Handle,
    frame: *const rsf_sr_frame,
) -> i32 {
    let frame = unsafe { &*frame };
    event(format!(
        "evaluate {} reset={} color={} output={} exposure={}",
        name(list),
        frame.reset,
        name(frame.color.resource),
        name(frame.output.resource),
        name(frame.exposure.resource)
    ));
    with(|world| world.vendor_result)
}
pub(super) unsafe extern "C" fn dlss_evaluate_borrowed(
    context: Handle,
    list: Handle,
    frame: *const rsf_sr_frame,
    _: Handle,
) -> i32 {
    event("borrowed".into());
    unsafe { dlss_evaluate(context, list, frame) }
}
pub(super) unsafe extern "C" fn dlss_present(_: Handle, _: Handle, _: u32, _: u32, _: u32) -> i32 {
    event("dlss-present".into());
    OK
}
pub(super) unsafe extern "C" fn dlss_destroy(_: Handle) {
    event("dlss-destroy".into());
}
pub(super) unsafe extern "C" fn session_create(
    setup: *const rsf_sr_session_setup,
    out: *mut Handle,
) -> i32 {
    let open = unsafe { &(*setup).open };
    event(format!(
        "session-create hdr={} auto={}",
        open.hdr, open.auto_exposure
    ));
    unsafe { out.write(SESSION as Handle) };
    OK
}
pub(super) unsafe extern "C" fn session_select(
    _: Handle,
    backend: u32,
    quality: u32,
    _: u64,
) -> i32 {
    event(format!("session-select {backend} q{quality}"));
    OK
}
pub(super) unsafe extern "C" fn session_get_status(
    _: Handle,
    status: *mut rsf_sr_session_status,
) -> i32 {
    let (width, height) = with(|world| world.plan);
    unsafe {
        (*status).render_width = width;
        (*status).render_height = height;
    }
    OK
}
pub(super) unsafe extern "C" fn session_destroy(_: Handle) {
    event("session-destroy".into());
}
pub(super) unsafe extern "C" fn create_pipeline(
    _: Handle,
    _: rsf_backend_log_fn,
    _: Handle,
    root: *mut Handle,
    pipeline: *mut Handle,
) -> i32 {
    unsafe {
        root.write(created("root", TextureDesc::default()));
        pipeline.write(created("pipeline", TextureDesc::default()));
    }
    1
}
pub(super) unsafe extern "C" fn create_heap(_: Handle) -> Handle {
    created("heap", TextureDesc::default())
}
pub(super) unsafe extern "C" fn create_texture(_: Handle, w: u32, h: u32, format: u32) -> Handle {
    event(format!("texture {w}x{h} f{format}"));
    created("texture", texture(u64::from(w), h, format, format))
}
pub(super) unsafe extern "C" fn create_exposure_texture(_: Handle) -> Handle {
    event("unit-exposure".into());
    created("exposure", texture(1, 1, R32_FLOAT, R32_FLOAT))
}
pub(super) unsafe extern "C" fn create_exposure_upload(_: Handle, bytes: u64) -> Handle {
    assert_eq!(bytes, EXPOSURE_UPLOAD_BYTES);
    created("upload", TextureDesc::default())
}
pub(super) unsafe extern "C" fn write_exposure_upload(upload: Handle, value: f32) -> i32 {
    assert!(name(upload).starts_with("upload"));
    assert_eq!(value, 1.0);
    1
}
pub(super) unsafe extern "C" fn create_readback(_: Handle, bytes: u64) -> Handle {
    event(format!("readback {bytes}"));
    created("readback", TextureDesc::default())
}
pub(super) unsafe extern "C" fn describe_stub(resource: Handle, out: *mut TextureDesc) {
    event(format!("describe {}", name(resource)));
    unsafe { out.write(object(resource).desc) };
}
pub(super) unsafe extern "C" fn owned_by(child: Handle, device: Handle) -> i32 {
    i32::from(object(child).device.get() == device as usize)
}
pub(super) unsafe extern "C" fn write_srv(
    _: Handle,
    heap: Handle,
    index: u32,
    resource: Handle,
    format: u32,
) {
    event(format!(
        "srv {} {index} {} f{format}",
        name(heap),
        name(resource)
    ));
}
pub(super) unsafe extern "C" fn write_uav(
    _: Handle,
    heap: Handle,
    index: u32,
    resource: Handle,
    format: u32,
) {
    event(format!(
        "uav {} {index} {} f{format}",
        name(heap),
        name(resource)
    ));
}
pub(super) unsafe extern "C" fn dispatch(
    list: Handle,
    heap: Handle,
    _: Handle,
    _: Handle,
    constants: *const u32,
    x: u32,
    y: u32,
) {
    let c = unsafe { std::slice::from_raw_parts(constants, 6) };
    event(format!(
        "dispatch {} {} {x}x{y} extent={}x{} scale={},{} inputs_only={} external={}",
        name(list),
        name(heap),
        c[0],
        c[1],
        f32::from_bits(c[2]),
        f32::from_bits(c[3]),
        c[4],
        c[5]
    ));
}
pub(super) unsafe extern "C" fn transition(_: Handle, resource: Handle, before: u32, after: u32) {
    event(format!(
        "transition {} {before:#x}->{after:#x}",
        name(resource)
    ));
}
pub(super) unsafe extern "C" fn copy_region(
    _: Handle,
    target: Handle,
    source: Handle,
    w: u32,
    h: u32,
) {
    event(format!("copy {}<-{} {w}x{h}", name(target), name(source)));
}
pub(super) unsafe extern "C" fn copy_unit_exposure(_: Handle, target: Handle, _: Handle) {
    event(format!("copy-exposure {}", name(target)));
}
pub(super) unsafe extern "C" fn copy_motion_tile(
    _: Handle,
    _: Handle,
    offset: u64,
    _: Handle,
    x: u32,
    y: u32,
) {
    event(format!("tile {offset} {x},{y}"));
}
pub(super) unsafe extern "C" fn map(_: Handle, bytes: u64) -> *const u8 {
    with(|world| {
        world.readback.resize(bytes as usize, 0);
        world.readback.as_ptr()
    })
}
pub(super) unsafe extern "C" fn unmap(_: Handle) {}
pub(super) unsafe extern "C" fn depth_domains(pass: *const Pass) -> u32 {
    unsafe { (*pass).depth_domains.domain_count }
}
pub(super) extern "C" fn capture_create() -> Handle {
    CAPTURE as Handle
}
pub(super) unsafe extern "C" fn capture_destroy(_: Handle) {
    event("capture-destroy".into());
}
pub(super) unsafe extern "C" fn capture_enabled(_: Handle) -> i32 {
    i32::from(with(|world| world.capture_enabled))
}
pub(super) unsafe extern "C" fn capture_configure(
    _: Handle,
    directory: *const c_char,
    _: u64,
    frames: u32,
) -> i32 {
    i32::from(!directory.is_null() && frames > 0)
}
pub(super) unsafe extern "C" fn capture_flush(_: Handle, slot: u32) -> i32 {
    event(format!("flush {slot:#x}"));
    1
}
pub(super) unsafe extern "C" fn capture_record(
    _: Handle,
    _: Handle,
    source: Handle,
    state: u32,
    record: *const rsf_frame_record,
    role: *const c_char,
    slot: u32,
    ordinal: u32,
    _: u32,
    _: u32,
) -> i32 {
    let role = unsafe { CStr::from_ptr(role) }.to_string_lossy();
    let frame = unsafe { (*record).frame_id };
    if unsafe { (*record).flags } & RSF_FRAME_FLAG_AMBIGUOUS_ID != 0 {
        event(format!("ambiguous readback {frame} {role}"));
    }
    event(format!(
        "record {role} {} {state:#x} frame={frame} slot={slot} ordinal={ordinal}",
        name(source)
    ));
    1
}
pub(super) unsafe extern "C" fn capture_event(
    _: Handle,
    _: *const rsf_frame_record,
    _: *const c_char,
    _: u64,
    _: u32,
    _: i32,
) -> i32 {
    1
}
pub(super) unsafe extern "C" fn capture_wants(_: Handle, _: u64) -> i32 {
    i32::from(with(|world| world.capture_wants))
}
pub(super) unsafe extern "C" fn capture_force_reset(_: Handle, _: u64) -> i32 {
    0
}
pub(super) extern "C" fn sharpen_create() -> Handle {
    SHARPEN as Handle
}
pub(super) unsafe extern "C" fn sharpen_destroy(_: Handle) {
    event("sharpen-destroy".into());
}
pub(super) unsafe extern "C" fn sharpen_run(
    _: Handle,
    _: Handle,
    output: Handle,
    _: Handle,
    _: Handle,
    _: Handle,
    _: *const rsf_sharpening_options,
    _: u32,
    _: u32,
    _: f32,
    _: f32,
    _: u32,
) -> i32 {
    event(format!("sharpen {}", name(output)));
    with(|world| world.sharpen_result)
}
pub(super) unsafe extern "C" fn dump_begin_record(record: *const rsf_frame_record, _: u32) -> i32 {
    let record = unsafe { &*record };
    if record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID != 0 {
        event(format!("ambiguous begin {}", record.frame_id));
        return 1;
    }
    0
}
pub(super) unsafe extern "C" fn dump_pass_metadata(_: *const Pass) {}
pub(super) unsafe extern "C" fn dump_sr_frame(_: *const rsf_sr_frame, _: u32, _: u32) {}
pub(super) unsafe extern "C" fn dump_result(
    record: *const rsf_frame_record,
    _: *const c_char,
    _: u32,
    _: i32,
) {
    let record = unsafe { &*record };
    if record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID != 0 {
        event(format!("ambiguous result {}", record.frame_id));
    }
}
pub(super) unsafe fn fg_handoff(_: Handle, _: &rsf_frame_record, _: Handle, _: Handle) {
    event("fg-handoff".into());
}

// The fakes sr12.rs calls in place of sr12/native.rs.
pub(super) mod native {
    pub(in crate::sr12) use super::{
        capture_configure, capture_create, capture_destroy, capture_enabled, capture_event,
        capture_flush, capture_force_reset, capture_record, capture_wants, copy_motion_tile,
        copy_region, copy_unit_exposure, create_exposure_texture, create_exposure_upload,
        create_heap, create_pipeline, create_readback, create_texture, depth_domains,
        describe_stub as describe, dispatch, dlss_create, dlss_create_registered,
        dlss_create_shared, dlss_destroy, dlss_evaluate, dlss_evaluate as session_evaluate,
        dlss_evaluate_borrowed, dlss_plan, dlss_present, dlss_set_sdr, dump_begin_record,
        dump_pass_metadata, dump_result, dump_sr_frame, fg_handoff, map, owned_by, session_create,
        session_destroy, session_get_status, session_select, sharpen_create, sharpen_destroy,
        sharpen_run, transition, unmap, write_exposure_upload, write_srv, write_uav,
    };
}
unsafe extern "C" fn log(user: *mut c_void, message: *const c_char) {
    let _ = user;
    let text = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    with(|world| world.logs.push(text));
}

struct Scene {
    device: Handle,
    list: Handle,
    color: Handle,
    depth: Handle,
    motion: Handle,
    output: Handle,
    exposure: Handle,
}

impl Scene {
    fn new(render: (u32, u32), output: (u32, u32)) -> Self {
        let device = fake("device", TextureDesc::default(), ptr::null_mut());
        with(|world| {
            *world = World {
                device: device as usize,
                vendor_result: OK,
                plan: render,
                sharpen_result: 1,
                ..World::default()
            }
        });
        let (rw, rh) = (u64::from(render.0), render.1);
        Self {
            device,
            list: fake("list", TextureDesc::default(), device),
            color: fake("color", texture(rw, rh, RGBA16_FLOAT, RGBA16_FLOAT), device),
            depth: fake("depth", texture(rw, rh, D32_FLOAT, R32_FLOAT), device),
            motion: fake("motion", texture(rw, rh, RG16_FLOAT, RG16_FLOAT), device),
            output: fake(
                "engine-output",
                texture(u64::from(output.0), output.1, RGBA16_FLOAT, RGBA16_FLOAT),
                device,
            ),
            exposure: fake("game-exposure", texture(1, 1, R16_FLOAT, R16_FLOAT), device),
        }
    }

    fn setup(&self, backend: u32, abi: u32, output: (u32, u32)) -> Setup {
        let mut setup = Setup {
            struct_size: size_of::<Setup>() as u32,
            abi_version: abi,
            device: self.device,
            backend,
            output_width: output.0,
            output_height: output.1,
            ..Setup::default()
        };
        setup.log = Some(log);
        setup
    }

    fn pass(&self, frame: u64, render: (u32, u32), output: (u32, u32)) -> Pass {
        let mut pass: Pass = unsafe { std::mem::zeroed() };
        pass.struct_size = size_of::<Pass>() as u32;
        pass.session_id = 7;
        pass.native_frame = frame;
        pass.source_frame_id = frame + 100;
        pass.view_key = 1;
        pass.history_key = 11;
        pass.resource_generation = 1;
        pass.color_input = self.color;
        pass.depth = self.depth;
        pass.motion = self.motion;
        pass.color_output = self.output;
        pass.exposure = self.exposure;
        pass.camera_valid = 1;
        pass.camera.struct_size = size_of::<rsf_camera_frame>() as u32;
        pass.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        (pass.camera.render_width, pass.camera.render_height) = render;
        (pass.camera.output_width, pass.camera.output_height) = output;
        pass.motion_to_uv = [0.5, -0.5];
        pass
    }

    fn release(self) {
        for handle in [
            self.list,
            self.color,
            self.depth,
            self.motion,
            self.output,
            self.exposure,
            self.device,
        ] {
            unsafe { release(handle.cast()) };
        }
    }
}

fn create_with(setup: &Setup) -> (i32, *mut Sr12) {
    let mut owner = ptr::null_mut();
    let result = unsafe { rsf_core_sr12_create(setup, &mut owner) };
    (result, owner)
}

fn position(events: &[String], prefix: &str) -> usize {
    events
        .iter()
        .position(|event| event.starts_with(prefix))
        .unwrap_or_else(|| panic!("missing {prefix} in {events:#?}"))
}

const RENDER: (u32, u32) = (960, 540);
const OUTPUT: (u32, u32) = (1920, 1080);

#[test]
fn format_and_extent_rules() {
    assert_eq!(
        depth_view_format(R32_FLOAT_X8X24_TYPELESS),
        R32_FLOAT_X8X24_TYPELESS
    );
    assert_eq!(
        depth_view_format(R24_UNORM_X8_TYPELESS),
        R24_UNORM_X8_TYPELESS
    );
    assert_eq!(depth_view_format(R32_TYPELESS), R32_FLOAT);
    let desc = |format, srv| texture(64, 64, format, srv);
    assert_eq!(
        accepted_format(COLOR, &desc(R11G11B10_FLOAT, R11G11B10_FLOAT)),
        Some(R11G11B10_FLOAT)
    );
    assert_eq!(
        accepted_format(COLOR, &desc(RGBA32_FLOAT, RGBA32_FLOAT)),
        None
    );
    assert_eq!(
        accepted_format(DEPTH, &desc(D24_UNORM_S8_UINT, R24_UNORM_X8_TYPELESS)),
        Some(R24_UNORM_X8_TYPELESS)
    );
    assert_eq!(accepted_format(DEPTH, &desc(R16_FLOAT, R16_FLOAT)), None);
    assert_eq!(accepted_format(MOTION, &desc(RG32_FLOAT, RG32_FLOAT)), None);
    assert_eq!(
        accepted_format(super::OUTPUT, &desc(RGBA16_FLOAT, RGBA16_FLOAT)),
        Some(RGBA16_FLOAT)
    );
    assert!(exposure_accepted(&texture(1, 1, R16_FLOAT, R16_FLOAT)));
    assert!(!exposure_accepted(&texture(2, 1, R32_FLOAT, R32_FLOAT)));
    assert!(!exposure_accepted(&TextureDesc {
        samples: 4,
        ..texture(1, 1, R32_FLOAT, R32_FLOAT)
    }));
    // Each 8x8 tile at a quarter point must lie inside the texture.
    for (width, height) in [(32, 32), (33, 47), (960, 540)] {
        assert!(diagnostic_fits(width, height));
        for tile in 0..4 {
            let (x, y) = tile_origin(tile, width, height);
            assert!(
                x + TILE <= width && y + TILE <= height,
                "{width}x{height} tile {tile}"
            );
        }
    }
    // The C++ owner accepted 16..31, where the far tiles crossed the right and bottom edges.
    assert_eq!(tile_origin(3, 16, 16), (12, 12));
    assert!(!diagnostic_fits(16, 64) && !diagnostic_fits(64, 31));
}

#[test]
fn half_conversion_and_motion_statistics() {
    assert_eq!(half_to_f32(0x3c00), 1.0);
    assert_eq!(half_to_f32(0xc000), -2.0);
    assert_eq!(half_to_f32(0x0001), 2f32.powi(-24));
    assert_eq!(half_to_f32(0x8000).to_bits(), 0x8000_0000);
    assert!(half_to_f32(0x7c00).is_infinite());
    assert!(half_to_f32(0x7e00).is_nan());
    let mut bytes = vec![0u8; READBACK_BYTES as usize];
    // (3, 4) at one sample, a NaN at another.
    bytes[0..4].copy_from_slice(&[0x00, 0x42, 0x00, 0x44]);
    bytes[2048 + 256..2048 + 260].copy_from_slice(&[0x00, 0x7e, 0x00, 0x00]);
    let (mean, maximum, invalid) = motion_statistics(&bytes);
    assert_eq!((maximum, invalid), (5.0, 1));
    assert_eq!(mean, 5.0 / 256.0);
}

#[test]
fn create_refuses_before_any_platform_call() {
    let scene = Scene::new(RENDER, OUTPUT);
    let mut owner = ptr::dangling_mut::<Sr12>();
    assert_eq!(
        unsafe { rsf_core_sr12_create(ptr::null(), &mut owner) },
        INVALID_ARGUMENT
    );
    assert!(owner.is_null());
    assert_eq!(
        unsafe { rsf_core_sr12_create(ptr::null(), ptr::null_mut()) },
        INVALID_ARGUMENT
    );
    let valid = scene.setup(RSF_SR_NONE, 2, OUTPUT);
    let cases = [
        (
            Setup {
                struct_size: offset_of!(Setup, engine_input_extents) as u32 - 4,
                ..valid
            },
            INVALID_ARGUMENT,
        ),
        (
            Setup {
                abi_version: 3,
                ..valid
            },
            ABI_MISMATCH,
        ),
        (
            Setup {
                struct_size: size_of::<Setup>() as u32 - 8,
                ..valid
            },
            INVALID_ARGUMENT,
        ),
        (
            Setup {
                backend: RSF_SR_XESS + 1,
                ..valid
            },
            INVALID_ARGUMENT,
        ),
        (
            Setup {
                quality: MAX_QUALITY + 1,
                ..valid
            },
            INVALID_ARGUMENT,
        ),
        (
            Setup {
                output_width: 0,
                ..valid
            },
            INVALID_ARGUMENT,
        ),
        (
            Setup {
                device: ptr::null_mut(),
                ..valid
            },
            INVALID_ARGUMENT,
        ),
        (
            Setup {
                engine_input_extents: 1,
                render_width: OUTPUT.0 + 1,
                render_height: OUTPUT.1,
                ..valid
            },
            INVALID_ARGUMENT,
        ),
    ];
    for (setup, expected) in cases {
        let (result, owner) = create_with(&setup);
        assert_eq!((result, owner.is_null()), (expected, true));
    }
    assert!(take_events().is_empty());
    scene.release();
}

#[test]
fn accepted_frame_normalizes_hands_off_and_reinserts_in_order() {
    let scene = Scene::new(RENDER, OUTPUT);
    let (result, owner) = create_with(&scene.setup(RSF_SR_DLSS, 1, OUTPUT));
    assert_eq!(result, OK);
    let created = take_events();
    assert!(created.contains(&"dlss-plan 1920x1080 q0".to_owned()));
    assert!(created.contains(&"texture 1920x1080 f10".to_owned()));
    let mut width = 0;
    let mut height = 0;
    assert_eq!(
        unsafe { rsf_core_sr12_plan(owner, &mut width, &mut height) },
        OK
    );
    assert_eq!((width, height), RENDER);

    let pass = scene.pass(5, RENDER, OUTPUT);
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 1) },
        OK
    );
    let events = take_events();
    let dispatched = position(&events, "dispatch");
    assert_eq!(
        events[dispatched],
        "dispatch list heap4 120x68 extent=960x540 scale=-480,270 inputs_only=0 external=0"
    );
    assert!(position(&events, "flush 0x1") < dispatched);
    assert!(events.contains(&"srv heap4 1 depth f41".to_owned()));
    let evaluated = position(&events, "evaluate list reset=1");
    let handoff = position(&events, "fg-handoff");
    let sharpened = position(&events, "sharpen");
    let copied = position(&events, "copy engine-output<-");
    assert!(
        dispatched < evaluated && evaluated < handoff && handoff < sharpened && sharpened < copied
    );
    // Normalized inputs return to UAV before the private output is copied out.
    let returned = events
        .iter()
        .rposition(|event| event.ends_with("0x40->0x8"))
        .unwrap();
    assert!(sharpened < returned && returned < copied);

    // A consecutive frame continues history; a changed history key resets it.
    let next = scene.pass(6, RENDER, OUTPUT);
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &next, 2) },
        OK
    );
    let events = take_events();
    assert!(
        events
            .iter()
            .any(|event| event.starts_with("evaluate list reset=0"))
    );
    assert!(!events.iter().any(|event| event.starts_with("describe")));
    let mut moved = scene.pass(7, RENDER, OUTPUT);
    moved.history_key = 12;
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &moved, 0) },
        OK
    );
    assert!(
        take_events()
            .iter()
            .any(|event| event.starts_with("evaluate list reset=1"))
    );

    unsafe { rsf_core_sr12_destroy(owner) };
    let events = take_events();
    assert!(position(&events, "flush 0xffffffff") < position(&events, "dlss-destroy"));
    assert!(position(&events, "dlss-destroy") < position(&events, "release texture"));
    scene.release();
}

#[test]
fn refused_vendor_evaluation_keeps_engine_output_and_fg_inputs() {
    let scene = Scene::new(RENDER, OUTPUT);
    let (_, owner) = create_with(&scene.setup(RSF_SR_DLSS, 1, OUTPUT));
    take_events();
    with(|world| world.vendor_result = FEATURE_FAILED);
    let pass = scene.pass(5, RENDER, OUTPUT);
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        FEATURE_FAILED
    );
    let events = take_events();
    assert!(!events.iter().any(|event| event == "fg-handoff"
        || event.starts_with("copy ")
        || event.starts_with("sharpen")));
    // The refused frame breaks history even for the next consecutive frame.
    with(|world| world.vendor_result = OK);
    let next = scene.pass(6, RENDER, OUTPUT);
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &next, 1) },
        OK
    );
    assert!(
        take_events()
            .iter()
            .any(|event| event.starts_with("evaluate list reset=1"))
    );
    // A capture for that frame is refused on another recording, accepted on its own.
    let other = fake("other-list", TextureDesc::default(), scene.device);
    let mut scene_pass = scene.pass(6, RENDER, OUTPUT);
    scene_pass.color_input = scene.output;
    scene_pass.pass_key = 9;
    assert_eq!(
        unsafe { rsf_core_sr12_capture_scene(owner, other, &scene_pass, 1) },
        STALE_RESOURCES
    );
    assert_eq!(
        unsafe { rsf_core_sr12_capture_scene(owner, scene.list, &scene_pass, 1) },
        NOT_READY
    );
    with(|world| world.capture_wants = true);
    assert_eq!(
        unsafe { rsf_core_sr12_capture_scene(owner, scene.list, &scene_pass, 1) },
        OK
    );
    assert!(take_events().iter().any(
        |event| event == "record fg-scene-output engine-output 0x4 frame=106 slot=1 ordinal=9"
    ));
    unsafe { rsf_core_sr12_destroy(owner) };
    unsafe { release(other.cast()) };
    scene.release();
}

#[test]
fn inputs_are_validated_against_extents_formats_and_generation() {
    let scene = Scene::new(RENDER, OUTPUT);
    let (_, owner) = create_with(&scene.setup(RSF_SR_DLSS, 1, OUTPUT));
    let mut pass = scene.pass(5, RENDER, OUTPUT);
    pass.motion = scene.depth;
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        INVALID_ARGUMENT
    );
    let mut pass = scene.pass(5, RENDER, OUTPUT);
    pass.camera.render_width = RENDER.0 - 1;
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        INVALID_ARGUMENT
    );
    let mut pass = scene.pass(5, RENDER, OUTPUT);
    pass.motion_to_uv[1] = f32::NAN;
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        INVALID_ARGUMENT
    );
    let foreign = fake("foreign-list", TextureDesc::default(), ptr::null_mut());
    let pass = scene.pass(5, RENDER, OUTPUT);
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, foreign, &pass, 0) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 3) },
        INVALID_ARGUMENT
    );
    take_events();
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        OK
    );
    assert!(
        !take_events()
            .iter()
            .any(|event| event.starts_with("describe"))
    );
    let mut regenerated = scene.pass(6, RENDER, OUTPUT);
    regenerated.resource_generation = 2;
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &regenerated, 1) },
        OK
    );
    let events = take_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.starts_with("describe"))
            .count(),
        4
    );
    assert!(
        events
            .iter()
            .any(|event| event.starts_with("evaluate list reset=1"))
    );
    unsafe { rsf_core_sr12_destroy(owner) };
    unsafe { release(foreign.cast()) };
    scene.release();
}

#[test]
fn sdr_session_initializes_unit_exposure_once() {
    let scene = Scene::new(RENDER, OUTPUT);
    let mut setup = scene.setup(RSF_SR_XESS, 1, OUTPUT);
    setup.input_sdr = 1;
    let (result, owner) = create_with(&setup);
    assert_eq!(result, OK);
    let events = take_events();
    assert!(events.contains(&"session-create hdr=0 auto=0".to_owned()));
    assert!(events.contains(&"unit-exposure".to_owned()));
    for (frame, slot) in [(5, 0), (6, 1)] {
        let pass = scene.pass(frame, RENDER, OUTPUT);
        assert_eq!(
            unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, slot) },
            OK
        );
    }
    let events = take_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.starts_with("copy-exposure"))
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .all(|event| !event.starts_with("evaluate") || event.contains("exposure=exposure"))
    );
    unsafe { rsf_core_sr12_destroy(owner) };
    assert!(take_events().contains(&"session-destroy".to_owned()));
    scene.release();
}

#[test]
fn inferred_frame_id_carries_the_same_dump_provenance() {
    let scene = Scene::new(RENDER, OUTPUT);
    // Without a vendor the owner takes its input extent from the setup, defaulting to the output.
    let mut setup = scene.setup(RSF_SR_NONE, 1, OUTPUT);
    (setup.render_width, setup.render_height) = RENDER;
    let (result, owner) = create_with(&setup);
    assert_eq!(result, OK);
    take_events();
    let mut pass = scene.pass(5, RENDER, OUTPUT);
    pass.source_frame_id = 0;
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        OK
    );
    let events = take_events();
    let frame = pass.native_frame;
    for receipt in [
        format!("ambiguous begin {frame}"),
        format!("ambiguous readback {frame} sr-source-jittered"),
        format!("ambiguous result {frame}"),
    ] {
        assert!(events.contains(&receipt), "missing {receipt}");
    }
    unsafe { rsf_core_sr12_destroy(owner) };
    scene.release();
}

#[test]
fn external_exposure_is_validated_and_normalized() {
    let scene = Scene::new(RENDER, OUTPUT);
    let mut setup = scene.setup(RSF_SR_DLSS, 2, OUTPUT);
    setup.external_exposure = 1;
    let (result, owner) = create_with(&setup);
    assert_eq!(result, OK);
    take_events();
    let mut pass = scene.pass(5, RENDER, OUTPUT);
    pass.exposure = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        INVALID_ARGUMENT
    );
    let pass = scene.pass(5, RENDER, OUTPUT);
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        OK
    );
    let events = take_events();
    assert!(events.contains(&"srv heap3 3 game-exposure f54".to_owned()));
    assert!(
        events
            .iter()
            .any(|event| event.starts_with("uav heap3 7 texture"))
    );
    assert!(events.iter().any(|event| event.ends_with("external=1")));
    assert!(
        !events
            .iter()
            .any(|event| event.starts_with("copy-exposure"))
    );
    unsafe { rsf_core_sr12_destroy(owner) };
    scene.release();
}

#[test]
fn inputs_only_owner_hands_off_without_vendor_or_reinsertion() {
    let scene = Scene::new((16, 16), (16, 16));
    let (result, owner) = create_with(&scene.setup(RSF_SR_NONE, 1, (16, 16)));
    assert_eq!(result, OK);
    let events = take_events();
    // Colour is not normalized without a vendor or capture.
    assert!(events.contains(&"texture 1x1 f10".to_owned()));
    assert!(!events.contains(&"texture 16x16 f10".to_owned()));
    let pass = scene.pass(5, (16, 16), (16, 16));
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        OK
    );
    let events = take_events();
    assert!(
        events
            .iter()
            .any(|event| event.ends_with("inputs_only=1 external=0"))
    );
    assert!(events.contains(&"fg-handoff".to_owned()));
    assert!(!events.iter().any(|event| event.starts_with("evaluate")
        || event.starts_with("copy ")
        || event.starts_with("sharpen")));
    // The motion diagnostic needs 32 pixels per axis for its quarter-point tiles.
    assert!(
        !events
            .iter()
            .any(|event| event.starts_with("tile") || event.starts_with("readback"))
    );
    unsafe { rsf_core_sr12_destroy(owner) };
    scene.release();
}

#[test]
fn motion_diagnostic_reads_back_after_slot_reuse() {
    let scene = Scene::new(RENDER, OUTPUT);
    let (_, owner) = create_with(&scene.setup(RSF_SR_DLSS, 1, OUTPUT));
    take_events();
    let pass = scene.pass(5, RENDER, OUTPUT);
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, 0) },
        OK
    );
    let events = take_events();
    assert!(events.contains(&"tile 6144 720,405".to_owned()));
    let next = scene.pass(6, RENDER, OUTPUT);
    assert_eq!(
        unsafe { rsf_core_sr12_evaluate(owner, scene.list, &next, 0) },
        OK
    );
    let logs = with(|world| std::mem::take(&mut world.logs));
    assert_eq!(
        logs,
        ["Normalized motion samples: mean=0.000000 max=0.000000 render pixels invalid=0"]
    );
    unsafe { rsf_core_sr12_destroy(owner) };
    scene.release();
}

#[test]
fn sharpening_refusal_is_reported_once_and_options_are_checked() {
    let scene = Scene::new(RENDER, OUTPUT);
    let (_, owner) = create_with(&scene.setup(RSF_SR_DLSS, 1, OUTPUT));
    with(|world| world.sharpen_result = 0);
    for (frame, slot) in [(5, 0), (6, 1)] {
        let pass = scene.pass(frame, RENDER, OUTPUT);
        assert_eq!(
            unsafe { rsf_core_sr12_evaluate(owner, scene.list, &pass, slot) },
            OK
        );
    }
    let logs = with(|world| std::mem::take(&mut world.logs));
    let reported = logs
        .iter()
        .filter(|line| line.starts_with("D3D12 sharpening unavailable"))
        .count();
    assert_eq!(reported, 1);
    let mut options = sharpening_policy::defaults();
    options.struct_size = size_of::<rsf_sharpening_options>() as u32 - 4;
    options.abi_version = RSF_SHARPENING_ABI_VERSION;
    assert_eq!(
        unsafe { rsf_core_sr12_set_sharpening(owner, &options) },
        INVALID_ARGUMENT
    );
    options.struct_size += 4;
    options.abi_version = 2;
    assert_eq!(
        unsafe { rsf_core_sr12_set_sharpening(owner, &options) },
        ABI_MISMATCH
    );
    options.abi_version = RSF_SHARPENING_ABI_VERSION;
    options.mode = 9;
    assert_eq!(
        unsafe { rsf_core_sr12_set_sharpening(owner, &options) },
        INVALID_ARGUMENT
    );
    options.mode = 1;
    assert_eq!(unsafe { rsf_core_sr12_set_sharpening(owner, &options) }, OK);
    assert_eq!(
        unsafe { rsf_core_sr12_capture_flush(owner, 3) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_core_sr12_capture(owner, ptr::null(), 1, 1) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_core_sr12_present(owner, ptr::null_mut(), 0, 0, 0) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_core_sr12_present(owner, scene.list, 0, 0, 0) },
        OK
    );
    assert!(take_events().contains(&"dlss-present".to_owned()));
    unsafe { rsf_core_sr12_destroy(owner) };
    scene.release();
}
