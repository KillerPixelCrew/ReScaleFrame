// SPDX-License-Identifier: MIT
// Synthetic OpenGL pipeline contracts: a recording platform and reference-counted fake COM objects.
// No GL context, D3D12 device, DXGI window or vendor SDK is involved, so these establish ownership,
// ordering, admission and fallback policy only.
use super::*;
use crate::sys::RSF_FG_FIXED;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

// Read only through the COM calls in com_ref.cpp.
#[allow(dead_code)]
#[repr(C)]
struct Vtable {
    query: unsafe extern "system" fn(*mut Fake, *const c_void, *mut Handle) -> i32,
    add_ref: unsafe extern "system" fn(*mut Fake) -> u32,
    release: unsafe extern "system" fn(*mut Fake) -> u32,
}

// IUnknown layout: the vtable pointer first.
#[allow(dead_code)]
#[repr(C)]
struct Fake {
    vtable: &'static Vtable,
    refs: Cell<u32>,
    name: &'static str,
}

unsafe extern "system" fn query(_: *mut Fake, _: *const c_void, out: *mut Handle) -> i32 {
    unsafe { out.write(ptr::null_mut()) };
    0x8000_4002_u32 as i32
}

unsafe extern "system" fn add_ref(this: *mut Fake) -> u32 {
    let refs = unsafe { &(*this).refs };
    refs.set(refs.get() + 1);
    refs.get()
}

unsafe extern "system" fn release(this: *mut Fake) -> u32 {
    let fake = unsafe { &*this };
    assert!(fake.refs.get() > 0, "over-released fake");
    fake.refs.set(fake.refs.get() - 1);
    if fake.refs.get() == 0 {
        event(format!("release {}", fake.name));
    }
    fake.refs.get()
}

static VTABLE: Vtable = Vtable {
    query,
    add_ref,
    release,
};

fn fake_com(name: &'static str) -> *mut Fake {
    Box::into_raw(Box::new(Fake {
        vtable: &VTABLE,
        refs: Cell::new(1),
        name,
    }))
}

#[derive(Default)]
struct World {
    refused_preset: Option<u32>,
    events: Vec<String>,
    logs: Vec<String>,
    next: usize,
    names: HashMap<usize, String>,
    live: HashSet<usize>,
    texture_extents: HashMap<usize, (u32, u32, u32)>,
    failing_backends: HashSet<u32>,
    created_backends: Vec<u32>,
    fail_texture: Option<usize>,
    fail_sharpening: bool,
    textures_created: usize,
    resize_refused: bool,
    chain_resize_refused: bool,
    destroy_refused: bool,
    chain_unowned: bool,
    close_refused: bool,
    present_backend: u32,
    cpu_frames: HashMap<u64, bool>,
    cpu_events: Vec<rsf_game_cpu_event>,
    window_pass: Option<Pass>,
    chain: usize,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World::default());
}

fn world<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}

fn event(text: String) {
    world(|world| world.events.push(text));
}

fn events() -> Vec<String> {
    world(|world| std::mem::take(&mut world.events))
}

fn handle(kind: &str) -> Handle {
    world(|world| {
        world.next += 1;
        let id = 0x1000 + world.next * 0x10;
        world.names.insert(id, format!("{kind}{}", world.next));
        world.live.insert(id);
        id as Handle
    })
}

fn name(object: Handle) -> String {
    world(|world| {
        world
            .names
            .get(&(object as usize))
            .cloned()
            .unwrap_or_else(|| format!("{object:p}"))
    })
}

fn retire(object: Handle) {
    let gone = world(|world| world.live.remove(&(object as usize)));
    assert!(gone, "released a handle that is not live");
}

fn live(prefix: &str) -> usize {
    world(|world| {
        world
            .live
            .iter()
            .filter(|id| world.names[id].starts_with(prefix))
            .count()
    })
}

const PARENT: Handle = 0x10 as Handle;
const SHARING: Handle = 0x20 as Handle;
const DEVICE: Handle = 0x30 as Handle;
const QUEUE: Handle = 0x40 as Handle;
const LIST: Handle = 0x50 as Handle;
const HOST: Handle = 0x60 as Handle;

pub(super) unsafe extern "C" fn gl_open(_: Log, _: Handle, out: *mut Handle) -> i32 {
    event("gl_open".into());
    unsafe { out.write(SHARING) };
    OK
}
pub(super) unsafe extern "C" fn gl_graphics(
    _: Handle,
    device: *mut Handle,
    queue: *mut Handle,
) -> i32 {
    unsafe {
        device.write(DEVICE);
        queue.write(QUEUE);
    }
    OK
}
pub(super) unsafe extern "C" fn gl_texture_create(
    _: Handle,
    width: u32,
    height: u32,
    format: u32,
    out: *mut Handle,
) -> i32 {
    let fail = world(|world| {
        world.textures_created += 1;
        world.fail_texture == Some(world.textures_created)
    });
    if fail {
        return FEATURE_FAILED;
    }
    let texture = handle("texture");
    world(|world| {
        world
            .texture_extents
            .insert(texture as usize, (width, height, format))
    });
    unsafe { out.write(texture) };
    OK
}
pub(super) unsafe extern "C" fn gl_texture_get(
    texture: Handle,
    info: *mut rsf_gl_texture_info,
) -> i32 {
    let info = unsafe { &mut *info };
    assert_eq!(info.struct_size as usize, size_of::<rsf_gl_texture_info>());
    let (width, height, format) = world(|world| world.texture_extents[&(texture as usize)]);
    info.texture = texture as u32;
    info.width = width;
    info.height = height;
    info.format = format;
    info.resource = texture;
    OK
}
pub(super) unsafe extern "C" fn gl_texture_destroy(texture: Handle) {
    event(format!("destroy {}", name(texture)));
    retire(texture);
}
pub(super) unsafe extern "C" fn gl_begin(
    _: Handle,
    count: u32,
    textures: *const Handle,
    list: *mut Handle,
    slot: *mut u32,
) -> i32 {
    let names: Vec<_> = unsafe { std::slice::from_raw_parts(textures, count as usize) }
        .iter()
        .map(|t| name(*t))
        .collect();
    event(format!("begin {}", names.join(",")));
    unsafe {
        list.write(LIST);
        slot.write(1);
    }
    OK
}
pub(super) unsafe extern "C" fn gl_submit(_: Handle, count: u32, _: *const Handle) -> i32 {
    event(format!("submit {count}"));
    OK
}
pub(super) unsafe extern "C" fn gl_drain(_: Handle) -> i32 {
    event("drain".into());
    OK
}
pub(super) unsafe extern "C" fn gl_close(_: Handle) -> i32 {
    event("gl_close".into());
    if world(|world| world.close_refused) {
        FEATURE_FAILED
    } else {
        OK
    }
}
pub(super) extern "C" fn window_valid(window: Handle) -> c_int {
    c_int::from(window == PARENT)
}
pub(super) extern "C" fn window_create(parent: Handle) -> Handle {
    assert_eq!(parent, PARENT);
    handle("window")
}
pub(super) extern "C" fn window_destroy(window: Handle) -> c_int {
    event(format!("destroy {}", name(window)));
    if world(|world| world.destroy_refused) {
        return 0;
    }
    retire(window);
    1
}
pub(super) extern "C" fn window_child_of(_: Handle, parent: Handle) -> c_int {
    c_int::from(parent == PARENT)
}
pub(super) extern "C" fn window_resize(_: Handle, width: u32, height: u32) -> c_int {
    event(format!("window {width}x{height}"));
    c_int::from(!world(|world| world.resize_refused))
}
pub(super) extern "C" fn window_show(window: Handle) {
    event(format!("show {}", name(window)));
}
pub(super) unsafe extern "C" fn chain_create(
    queue: Handle,
    _: Handle,
    buffers: u32,
    format: u32,
    out: *mut Handle,
) -> i32 {
    assert_eq!(
        (queue, buffers, format),
        (QUEUE, CHAIN_BUFFERS, CHAIN_FORMAT)
    );
    let chain = fake_com("chain");
    world(|world| world.chain = chain as usize);
    unsafe { out.write(chain.cast()) };
    OK
}
pub(super) unsafe extern "C" fn chain_associate(_: Handle, _: Handle) {}
pub(super) unsafe extern "C" fn chain_resize(
    _: Handle,
    _: u32,
    width: u32,
    height: u32,
    _: u32,
) -> i32 {
    event(format!("chain {width}x{height}"));
    if world(|world| world.chain_resize_refused) {
        FEATURE_FAILED
    } else {
        OK
    }
}
pub(super) unsafe extern "C" fn chain_back_buffer(_: Handle) -> Handle {
    fake_com("back buffer").cast()
}
pub(super) unsafe extern "C" fn chain_present(_: Handle, interval: u32) -> i32 {
    event(format!("present {interval}"));
    OK
}
pub(super) unsafe extern "C" fn adapter_identity(
    _: Handle,
    vendor: *mut u32,
    software: *mut u32,
) -> c_int {
    unsafe {
        vendor.write(0x10de);
        software.write(0);
    }
    1
}
pub(super) unsafe extern "C" fn transition(_: Handle, resource: Handle, from: u32, to: u32) {
    let resource = if world(|world| world.names.contains_key(&(resource as usize))) {
        name(resource)
    } else {
        "back buffer".into()
    };
    event(format!("{resource} {from:#x}->{to:#x}"));
}
pub(super) unsafe extern "C" fn copy_resource(_: Handle, _: Handle, source: Handle) {
    event(format!("copy {}", name(source)));
}
pub(super) unsafe extern "C" fn sr_create(setup: *const rsf_sr12_setup, out: *mut Handle) -> i32 {
    let setup = unsafe { &*setup };
    assert_eq!(
        (setup.device, setup.input_sdr, setup.streamline_host),
        (DEVICE, 1, HOST)
    );
    let backend = setup.backend;
    let fails = world(|world| {
        world.created_backends.push(backend);
        world.failing_backends.contains(&backend)
    });
    if fails {
        return NOT_READY;
    }
    unsafe { out.write(handle(&format!("sr{backend}."))) };
    OK
}
pub(super) unsafe extern "C" fn sr_plan(_: Handle, width: *mut u32, height: *mut u32) -> i32 {
    unsafe {
        width.write(640);
        height.write(360);
    }
    OK
}
pub(super) unsafe extern "C" fn sr_evaluate(
    sr: Handle,
    _: Handle,
    pass: *const Pass,
    slot: u32,
) -> i32 {
    let pass = unsafe { &*pass };
    event(format!(
        "evaluate {} frame {} slot {slot} generation {}",
        name(sr),
        pass.source_frame_id,
        pass.resource_generation
    ));
    OK
}
pub(super) unsafe extern "C" fn sr_set_sharpening(
    sr: Handle,
    options: *const rsf_sharpening_options,
) -> i32 {
    assert!(sharpening_policy::valid(unsafe { &*options }));
    event(format!("sharpen {}", name(sr)));
    if world(|world| world.fail_sharpening) {
        FEATURE_FAILED
    } else {
        OK
    }
}
pub(super) unsafe extern "C" fn sr_destroy(sr: Handle) {
    event(format!("destroy {}", name(sr)));
    retire(sr);
}
pub(super) unsafe extern "C" fn fg_install(setup: *const rsf_fg12_setup) -> i32 {
    let setup = unsafe { &*setup };
    assert_eq!(setup.ui_mode, RSF_UI_MODE_BACKBUFFER_HUDLESS);
    event("fg_install".into());
    OK
}
pub(super) unsafe extern "C" fn fg_options(_: *const rsf_native_fg_options) {}
pub(super) unsafe extern "C" fn fg_cpu(event: *const rsf_game_cpu_event) {
    let event = unsafe { *event };
    world(|world| world.cpu_events.push(event));
}
pub(super) unsafe extern "C" fn fg_hudless(_: Handle, _: *const Pass) {}
pub(super) unsafe extern "C" fn fg_submitted(_: u64, frame: u64) {
    event(format!("submitted {frame}"));
}
pub(super) unsafe extern "C" fn fg_window(pass: *const Pass) {
    let pass = unsafe { *pass };
    event(format!("window pass {}", pass.source_frame_id));
    world(|world| world.window_pass = Some(pass));
}
pub(super) unsafe extern "C" fn fg_status(_: *mut rsf_native_fg_status) -> c_int {
    1
}
pub(super) extern "C" fn present_host() -> Handle {
    HOST
}
pub(super) unsafe extern "C" fn present_is_owner(chain: Handle) -> c_int {
    c_int::from(!chain.is_null() && !world(|world| world.chain_unowned))
}
pub(super) extern "C" fn present_backend() -> u32 {
    world(|world| world.present_backend)
}
pub(super) extern "C" fn present_backend_choices() -> u32 {
    (1 << 1) | (1 << 3) | (1 << 5)
}
pub(super) extern "C" fn present_request(backend: u32) -> i32 {
    event(format!("request fg {backend}"));
    OK
}
pub(super) extern "C" fn present_switch_result() -> i32 {
    OK
}
pub(super) unsafe extern "C" fn present_abort(_: u64) -> i32 {
    OK
}
pub(super) unsafe extern "C" fn pcl_message() -> u32 {
    0
}
pub(super) unsafe extern "C" fn cpu_read(
    _: u64,
    frame: u64,
    out: *mut rsf_native_cpu_frame,
) -> c_int {
    match world(|world| world.cpu_frames.get(&frame).copied()) {
        Some(ended) => {
            unsafe { (*out).ended = u32::from(ended) };
            1
        }
        None => 0,
    }
}
pub(super) unsafe extern "C" fn latency_completed(_: u64) -> c_int {
    0
}
pub(super) unsafe extern "C" fn latency_end_session(_: u64) {
    event("latency end".into());
}
pub(super) unsafe extern "C" fn dump_cancel(_: u64) {
    event("dump cancel".into());
}

// The fixture supplies preset refusal; production forwards to the native header predicate.
pub(super) unsafe extern "C" fn dlss_preset_valid(preset: u32) -> c_int {
    c_int::from(world(|world| world.refused_preset != Some(preset)))
}
pub(super) extern "C" fn fg_default_backend(vendor: u32, software: u32) -> u32 {
    u32::from(vendor == 0x10de && software == 0)
}
pub(super) unsafe extern "C" fn depth_domains_requested(pass: *const Pass) -> u32 {
    let domains = unsafe { &(*pass).depth_domains };
    let bytes = unsafe {
        std::slice::from_raw_parts(ptr::from_ref(domains).cast::<u8>(), size_of_val(domains))
    };
    u32::from(bytes.iter().any(|byte| *byte != 0))
}
pub(super) unsafe extern "C" fn overlay_configure(
    _: *const crate::sys::rsf_overlay_service_setup,
) -> i32 {
    NOT_READY
}
pub(super) unsafe extern "C" fn overlay_present(
    _: Handle,
    _: Handle,
    _: Handle,
    _: Handle,
    _: Handle,
    _: Log,
    _: Handle,
) -> i32 {
    NOT_READY
}
pub(super) extern "C" fn overlay_notify(_: u32) -> i32 {
    NOT_READY
}

// The fakes gl_pipeline.rs calls in place of gl_pipeline/linked.rs.
pub(super) mod native {
    pub(in crate::gl_pipeline) use super::{
        adapter_identity, chain_associate, chain_back_buffer, chain_create, chain_present,
        chain_resize, copy_resource, cpu_read, depth_domains_requested, dlss_preset_valid,
        dump_cancel, fg_cpu, fg_default_backend, fg_hudless, fg_install, fg_options, fg_status,
        fg_submitted, fg_window, gl_begin, gl_close, gl_drain, gl_graphics, gl_open, gl_submit,
        gl_texture_create, gl_texture_destroy, gl_texture_get, latency_completed,
        latency_end_session, overlay_configure, overlay_notify, overlay_present, pcl_message,
        present_abort, present_backend, present_backend_choices, present_host, present_is_owner,
        present_request, present_switch_result, sr_create, sr_destroy, sr_evaluate, sr_plan,
        sr_set_sharpening, transition, window_child_of, window_create, window_destroy,
        window_resize, window_show, window_valid,
    };
}

unsafe extern "C" fn record_log(_: Handle, message: *const c_char) {
    let text = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    world(|world| world.logs.push(text));
}

fn setup() -> rsf_gl_pipeline_setup {
    rsf_gl_pipeline_setup {
        struct_size: size_of::<rsf_gl_pipeline_setup>() as u32,
        hwnd: PARENT,
        runtime_directory_utf8: c"C:/RSF".as_ptr(),
        log: Some(record_log),
        user: ptr::null_mut(),
        engine_version_utf8: c"1.0".as_ptr(),
        project_id_utf8: c"project".as_ptr(),
        presentation_hwnd: ptr::null_mut(),
    }
}

fn open_pipeline() -> *mut Pipeline {
    let setup = setup();
    let mut pipeline = ptr::null_mut();
    assert_eq!(unsafe { rsf_gl_pipeline_open(&setup, &mut pipeline) }, OK);
    assert!(!pipeline.is_null());
    assert_eq!(events(), ["gl_open", "fg_install"]);
    pipeline
}

fn options(backend: u32, quality: u32) -> rsf_gl_pipeline_options {
    rsf_gl_pipeline_options {
        struct_size: size_of::<rsf_gl_pipeline_options>() as u32,
        sr_backend: backend,
        quality,
        fg_backend: RSF_FG_BACKEND_AUTO,
        generation: rsf_native_fg_options {
            struct_size: size_of::<rsf_native_fg_options>() as u32,
            mode: RSF_FG_FIXED,
            generated_frames: 1,
            reflex_mode: 0,
            frame_limit_us: 0,
            debug: 0,
        },
        ..Default::default()
    }
}

fn surfaces(pipeline: *mut Pipeline) -> rsf_gl_pipeline_surfaces {
    let mut out = MaybeUninit::<rsf_gl_pipeline_surfaces>::uninit();
    unsafe {
        ptr::addr_of_mut!((*out.as_mut_ptr()).struct_size)
            .write(size_of::<rsf_gl_pipeline_surfaces>() as u32);
        assert_eq!(rsf_gl_pipeline_get_surfaces(pipeline, out.as_mut_ptr()), OK);
        out.assume_init()
    }
}

fn status(pipeline: *mut Pipeline) -> rsf_gl_pipeline_status {
    let mut out = MaybeUninit::<rsf_gl_pipeline_status>::uninit();
    unsafe {
        ptr::addr_of_mut!((*out.as_mut_ptr()).struct_size)
            .write(size_of::<rsf_gl_pipeline_status>() as u32);
        assert_eq!(rsf_gl_pipeline_status_get(pipeline, out.as_mut_ptr()), OK);
        out.assume_init()
    }
}

fn pass(frame: u64) -> Pass {
    let mut pass: Pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<Pass>() as u32;
    pass.source_frame_id = frame;
    pass.view_key = 17;
    pass
}

#[test]
fn refused_sharpening_keeps_the_previous_surfaces_and_window() {
    let pipeline = open_pipeline();
    let options = options(RSF_SR_DLSS, 1);
    let configure = |w, h| unsafe { rsf_gl_pipeline_configure(pipeline, &options, w, h) };
    assert_eq!(configure(640, 480), OK);
    let previous = surfaces(pipeline);
    events();
    world(|world| world.fail_sharpening = true);
    assert_eq!(configure(1280, 720), FEATURE_FAILED);
    assert!(!events().iter().any(|call| call.starts_with("window ")));
    assert_eq!(surfaces(pipeline).composed, previous.composed);
    assert_eq!(status(pipeline).sr_result, FEATURE_FAILED);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
}

#[test]
fn refused_chain_resize_restores_the_published_window_extent() {
    let pipeline = open_pipeline();
    let options = options(RSF_SR_DLSS, 1);
    let configure = |w, h| unsafe { rsf_gl_pipeline_configure(pipeline, &options, w, h) };
    assert_eq!(configure(640, 480), OK);
    let previous = surfaces(pipeline);
    events();
    world(|world| world.chain_resize_refused = true);
    assert_eq!(configure(1280, 720), FEATURE_FAILED);
    let calls = events();
    assert!(
        calls
            .windows(3)
            .any(|calls| calls == ["window 1280x720", "chain 1280x720", "window 640x480"])
    );
    assert_eq!(surfaces(pipeline).composed, previous.composed);
    assert_eq!(configure(640, 480), OK);
    assert_eq!(status(pipeline).sr_result, OK);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
}

#[test]
fn rejected_switch_keeps_published_session_and_a_retry_rebuilds() {
    let pipeline = open_pipeline();
    let dlss = options(RSF_SR_DLSS, 1);
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &dlss, 1920, 1080) },
        OK
    );
    assert_eq!(
        events(),
        [
            "request fg 1",
            "drain",
            "sharpen sr1.2",
            "window 1920x1080",
            "chain 1920x1080"
        ]
    );
    let published = surfaces(pipeline);
    assert_eq!(
        (
            published.render_width,
            published.render_height,
            published.output_width
        ),
        (640, 360, 1920)
    );
    let formats = world(|world| {
        [
            published.scene,
            published.depth,
            published.motion,
            published.output,
            published.hudless,
            published.composed,
        ]
        .map(|t| world.texture_extents[&(t as usize)])
    });
    assert_eq!(
        formats,
        [
            (640, 360, RSF_GL_RGBA16F),
            (640, 360, RSF_GL_R32F),
            (640, 360, RSF_GL_RG16F),
            (1920, 1080, RSF_GL_RGBA16F),
            (1920, 1080, RSF_GL_RGBA8),
            (1920, 1080, RSF_GL_RGBA8)
        ]
    );

    // The third replacement surface fails: the new session and its two surfaces are released, the
    // published ones stay.
    world(|world| {
        world.present_backend = 1;
        world.fail_texture = Some(world.textures_created + 3);
    });
    let fsr3 = options(RSF_SR_FSR3, 1);
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &fsr3, 1920, 1080) },
        FEATURE_FAILED
    );
    assert_eq!(
        events(),
        [
            "drain",
            "destroy sr3.9",
            "destroy texture10",
            "destroy texture11"
        ]
    );
    assert_eq!(surfaces(pipeline).scene, published.scene);
    assert_eq!((live("sr1."), live("sr3."), live("texture")), (1, 0, 6));
    let failed = status(pipeline);
    assert_eq!(
        (failed.requested_sr, failed.effective_sr, failed.sr_result),
        (RSF_SR_FSR3, RSF_SR_DLSS, FEATURE_FAILED)
    );

    // The same request is retried rather than treated as already applied.
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &fsr3, 1920, 1080) },
        OK
    );
    let switched = events();
    assert_eq!(switched[..3], ["drain", "sharpen sr3.12", "destroy sr1.2"]);
    assert!(
        switched[3..9]
            .iter()
            .all(|line| line.starts_with("destroy texture"))
    );
    assert_eq!(status(pipeline).effective_sr, RSF_SR_FSR3);
    assert_eq!((live("sr1."), live("sr3."), live("texture")), (0, 1, 6));

    // Only sharpening changes: no drain and no rebuild.
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &fsr3, 1920, 1080) },
        OK
    );
    assert_eq!(events(), ["sharpen sr3.12"]);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
}

#[test]
fn automatic_selection_keeps_first_created_backend_then_falls_back_to_off() {
    let pipeline = open_pipeline();
    world(|world| world.failing_backends = [RSF_SR_DLSS, RSF_SR_FSR3].into());
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &options(RSF_SR_AUTO, 1), 1280, 720) },
        OK
    );
    let created = world(|world| std::mem::take(&mut world.created_backends));
    assert_eq!(created, [RSF_SR_DLSS, RSF_SR_FSR3, RSF_SR_XESS]);
    let selected = status(pipeline);
    assert_eq!(
        (
            selected.requested_sr,
            selected.effective_sr,
            selected.sr_result
        ),
        (RSF_SR_AUTO, RSF_SR_XESS, OK)
    );

    world(|world| world.failing_backends = [RSF_SR_DLSS, RSF_SR_FSR3, RSF_SR_XESS].into());
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &options(RSF_SR_AUTO, 2), 1280, 720) },
        OK
    );
    let created = world(|world| std::mem::take(&mut world.created_backends));
    assert_eq!(
        created,
        [RSF_SR_DLSS, RSF_SR_FSR3, RSF_SR_XESS, RSF_SR_NONE]
    );
    let fallback = status(pipeline);
    assert_eq!(
        (
            fallback.effective_sr,
            fallback.sr_result,
            fallback.evaluated
        ),
        (RSF_SR_NONE, NOT_READY, 0)
    );

    // Off still evaluates for normalization, but is not counted as SR.
    events();
    assert_eq!(unsafe { rsf_gl_pipeline_evaluate(pipeline, &pass(3)) }, OK);
    assert_eq!(status(pipeline).evaluated, 0);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
}

#[test]
fn evaluation_and_presentation_carry_session_frame_identity() {
    let pipeline = open_pipeline();
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &options(RSF_SR_XESS, 1), 800, 600) },
        OK
    );
    events();
    let mut domains = pass(9);
    domains.depth_domains.domain_count = 1;
    assert_eq!(
        unsafe { rsf_gl_pipeline_evaluate(pipeline, &domains) },
        INVALID_ARGUMENT
    );
    let mut short = pass(9);
    short.struct_size -= 8;
    assert_eq!(
        unsafe { rsf_gl_pipeline_evaluate(pipeline, &short) },
        INVALID_ARGUMENT
    );
    assert_eq!(unsafe { rsf_gl_pipeline_evaluate(pipeline, &pass(9)) }, OK);
    let evaluated = events();
    assert_eq!(evaluated[0], "begin texture3,texture4,texture5,texture6");
    assert_eq!(evaluated[1], "texture3 0x0->0x40");
    assert_eq!(evaluated[4], "texture6 0x0->0x400");
    assert_eq!(evaluated[5], "evaluate sr5.2 frame 9 slot 1 generation 1");
    assert_eq!(
        evaluated[9..],
        ["texture6 0x400->0x0", "submit 4", "submitted 9"]
    );
    assert_eq!(status(pipeline).evaluated, 1);

    assert_eq!(
        unsafe { rsf_gl_pipeline_present(pipeline, 0, 0) },
        INVALID_ARGUMENT
    );
    for frame in [9, 10] {
        assert_eq!(unsafe { rsf_gl_pipeline_present(pipeline, frame, 1) }, OK);
        let window = world(|world| world.window_pass).unwrap();
        assert_eq!(window.view_key, if frame == 9 { 17 } else { 0 });
    }
    let presented = events();
    assert_eq!(
        presented[..11],
        [
            "begin texture8",
            "texture8 0x0->0x800",
            "back buffer 0x0->0x400",
            "copy texture8",
            "back buffer 0x400->0x0",
            "texture8 0x800->0x0",
            "submit 1",
            "window pass 9",
            "present 1",
            "release back buffer",
            "show window1"
        ]
    );
    assert_eq!(
        presented
            .iter()
            .filter(|line| line.starts_with("show"))
            .count(),
        1
    );
    let window = world(|world| world.window_pass).unwrap();
    assert_eq!(
        (window.role, window.source_frame_id, window.view_key),
        (RSF_GAME_RENDER_WINDOW, 10, 0)
    );
    assert_eq!(window.swapchain as usize, world(|world| world.chain));
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
}

#[test]
fn close_releases_sr_before_surfaces_and_the_presentation_host() {
    let pipeline = open_pipeline();
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &options(RSF_SR_DLSS, 1), 640, 480) },
        OK
    );
    events();
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
    let closed = events();
    assert_eq!(closed[..2], ["drain", "destroy sr1.2"]);
    assert!(
        closed[2..8]
            .iter()
            .all(|line| line.starts_with("destroy texture"))
    );
    assert_eq!(
        closed[8..],
        [
            "release chain",
            "gl_close",
            "latency end",
            "dump cancel",
            "destroy window1"
        ]
    );
    assert_eq!((live("sr"), live("texture"), live("window")), (0, 0, 0));

    // The handle stays valid: graphics calls refuse, status and a repeated close succeed.
    let mut out = surfaces_template();
    assert_eq!(
        unsafe { rsf_gl_pipeline_get_surfaces(pipeline, &mut out) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &options(RSF_SR_DLSS, 1), 640, 480) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_gl_pipeline_present(pipeline, 4, 0) },
        INVALID_ARGUMENT
    );
    assert_eq!(status(pipeline).effective_sr, RSF_SR_DLSS);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
    assert!(events().is_empty());
}

fn surfaces_template() -> rsf_gl_pipeline_surfaces {
    rsf_gl_pipeline_surfaces {
        struct_size: size_of::<rsf_gl_pipeline_surfaces>() as u32,
        ..Default::default()
    }
}

#[test]
fn failed_chain_open_returns_a_cleanup_handle_if_retirement_refuses() {
    world(|world| {
        world.chain_unowned = true;
        world.close_refused = true;
    });
    let mut pipeline = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_gl_pipeline_open(&setup(), &mut pipeline) },
        INIT_FAILED
    );
    assert!(!pipeline.is_null());
    assert_eq!(
        unsafe { rsf_gl_pipeline_present(pipeline, 1, 0) },
        NOT_READY
    );
    assert_eq!(
        unsafe { rsf_gl_pipeline_input(pipeline, 1, 1, 0) },
        NOT_READY
    );
    events();
    world(|world| world.close_refused = false);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
    assert_eq!(
        events(),
        ["gl_close", "latency end", "dump cancel", "destroy window1"]
    );
    world(|world| world.chain_unowned = false);
    assert_eq!(unsafe { rsf_gl_pipeline_open(&setup(), &mut pipeline) }, OK);
    assert_eq!(events(), ["gl_open"]);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
}

#[test]
fn refused_window_destroy_retries_only_the_window() {
    let pipeline = open_pipeline();
    world(|world| world.destroy_refused = true);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, FEATURE_FAILED);
    assert!(events().iter().any(|event| event == "gl_close"));
    world(|world| world.destroy_refused = false);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
    assert_eq!(events(), ["destroy window1"]);
}

#[test]
fn failed_close_keeps_graphics_retired_until_cleanup_succeeds() {
    let pipeline = open_pipeline();
    let options = options(RSF_SR_DLSS, 1);
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &options, 640, 480) },
        OK
    );
    world(|world| world.close_refused = true);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, FEATURE_FAILED);
    events();
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &options, 640, 480) },
        NOT_READY
    );
    assert!(events().is_empty());
    world(|world| world.close_refused = false);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
    assert_eq!(
        events(),
        ["gl_close", "latency end", "dump cancel", "destroy window1"]
    );
}

#[test]
fn input_attaches_only_to_open_cpu_frames() {
    let pipeline = open_pipeline();
    assert_eq!(
        unsafe { rsf_gl_pipeline_input(pipeline, 0, 8, 0) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_gl_pipeline_input(pipeline, 0, 0, 0) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_gl_pipeline_input(pipeline, 5, 1, 0) },
        NOT_READY
    );
    world(|world| world.cpu_frames = [(5, false), (6, true)].into());
    assert_eq!(
        unsafe { rsf_gl_pipeline_input(pipeline, 6, 1, 0) },
        NOT_READY
    );
    assert_eq!(unsafe { rsf_gl_pipeline_input(pipeline, 5, 7, 0) }, OK);
    assert_eq!(unsafe { rsf_gl_pipeline_input(pipeline, 0, 0, 0x400) }, OK);
    assert_eq!(unsafe { rsf_gl_pipeline_input(pipeline, 0, 2, 0) }, OK);
    unsafe { rsf_gl_pipeline_cpu(pipeline, 5, 3) };
    let sent = world(|world| std::mem::take(&mut world.cpu_events));
    let summary: Vec<_> = sent
        .iter()
        .map(|e| {
            (
                e.stage,
                e.source_frame_id,
                e.input_kind,
                e.message_id,
                e.flags,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (RSF_GAME_CPU_INPUT_EVENT, 5, 7, 0, RSF_GAME_CPU_INPUT_KEYED),
            (
                RSF_GAME_CPU_INPUT_EVENT,
                0,
                0,
                0x400,
                RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE
            ),
            (
                RSF_GAME_CPU_INPUT_EVENT,
                0,
                2,
                0,
                RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_INPUT_UNFRAMED
            ),
            (3, 5, 0, 0, RSF_GAME_CPU_INPUT_KEYED)
        ]
    );
    assert!(sent.iter().all(
        |e| e.session_id == sent[0].session_id && e.event_version == RSF_GAME_CPU_EVENT_VERSION
    ));
    assert_eq!(
        unsafe { rsf_gl_pipeline_abort(pipeline, 0) },
        INVALID_ARGUMENT
    );
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
}

#[test]
fn rejected_options_do_not_touch_the_graphics_owner() {
    let pipeline = open_pipeline();
    world(|world| world.refused_preset = Some(7));
    let mut invalid = [
        options(RSF_SR_SPATIAL, 1),
        options(8, 1),
        options(RSF_SR_DLSS, 6),
        options(RSF_SR_DLSS, 1),
        options(RSF_SR_DLSS, 1),
    ];
    invalid[3].dlss_preset = 7;
    invalid[4].sharpening = sharpening_policy::defaults();
    invalid[4].sharpening.sharpness = 2.0;
    for options in &invalid {
        assert_eq!(
            unsafe { rsf_gl_pipeline_configure(pipeline, options, 640, 480) },
            INVALID_ARGUMENT
        );
    }
    assert_eq!(
        unsafe { rsf_gl_pipeline_configure(pipeline, &options(RSF_SR_DLSS, 1), 0, 480) },
        INVALID_ARGUMENT
    );
    assert!(events().is_empty());
    assert_eq!(status(pipeline).requested_sr, 0);
    assert_eq!(unsafe { rsf_gl_pipeline_close(pipeline) }, OK);
}
