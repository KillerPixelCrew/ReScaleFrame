// SPDX-License-Identifier: MIT
// Synthetic bridge contracts: a recording platform and reference-counted fake COM objects. No GPU,
// D3D runtime or vendor SDK is involved, so these establish ordering, lease and retirement
// policy only.
use super::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

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
    let refs = unsafe { &(*this).refs };
    assert!(refs.get() > 0, "over-released fake");
    refs.set(refs.get() - 1);
    refs.get()
}

static VTABLE: Vtable = Vtable {
    query,
    add_ref,
    release,
};

struct World {
    events: Vec<String>,
    logs: Vec<String>,
    names: HashMap<usize, String>,
    // Objects the platform created for the bridge, each with one reference or open handle.
    created: Vec<Handle>,
    descs: HashMap<usize, (TextureDesc, Handle)>,
    children: HashMap<usize, (Handle, Handle)>,
    surfaces: usize,
    evaluated: Option<rsf_sr_frame>,
    wait_fails: bool,
    warm_up_wait_fails: bool,
    queue_wait_fails: bool,
    warm_signal_fails: bool,
    device_ready: bool,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World {
        events: Vec::new(),
        logs: Vec::new(),
        names: HashMap::new(),
        created: Vec::new(),
        descs: HashMap::new(),
        children: HashMap::new(),
        surfaces: 0,
        evaluated: None,
        wait_fails: false,
        warm_up_wait_fails: false,
        queue_wait_fails: false,
        warm_signal_fails: false,
        device_ready: true,
    });
}

fn world<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}

fn event(text: String) {
    world(|world| world.events.push(text));
}

fn take_events() -> Vec<String> {
    world(|world| std::mem::take(&mut world.events))
}

fn object(name: &str) -> Handle {
    let fake: &'static Fake = Box::leak(Box::new(Fake {
        vtable: &VTABLE,
        refs: Cell::new(1),
    }));
    let handle = ptr::from_ref(fake).cast_mut().cast::<c_void>();
    world(|world| world.names.insert(handle as usize, name.to_owned()));
    handle
}

// An object the bridge receives ownership of.
fn created(name: &str) -> Handle {
    let handle = object(name);
    world(|world| world.created.push(handle));
    handle
}

fn name(handle: Handle) -> String {
    world(|world| world.names.get(&(handle as usize)).cloned()).unwrap_or_else(|| "?".into())
}

fn find(name: &str) -> Handle {
    world(|world| {
        world
            .names
            .iter()
            .find(|(_, candidate)| candidate.as_str() == name)
            .map(|(&handle, _)| handle as Handle)
    })
    .unwrap_or_else(|| panic!("no fake named {name}"))
}

fn refs(handle: Handle) -> u32 {
    unsafe { (*handle.cast::<Fake>()).refs.get() }
}

fn close(handle: Handle) {
    let refs = unsafe { &(*handle.cast::<Fake>()).refs };
    assert_eq!(refs.get(), 1, "{} closed twice", name(handle));
    refs.set(0);
}

fn texture(device: Handle, name: &str, width: u32, height: u32, format: u32) -> Handle {
    let texture = object(name);
    let desc = TextureDesc {
        width,
        height,
        format,
        samples: 1,
        array_size: 1,
    };
    world(|world| world.descs.insert(texture as usize, (desc, device)));
    texture
}

const SLOT_NAMES: [&str; SLOTS] = [
    "colour",
    "depth",
    "motion",
    "exposure",
    "output",
    "reactive",
    "transparency",
];

unsafe extern "C" fn session_create(setup: *const rsf_sr_session_setup, out: *mut Handle) -> i32 {
    let open = unsafe { (*setup).open };
    assert_eq!(open.api, RSF_API_D3D12);
    assert_eq!(name(open.device), "device12");
    unsafe { out.write(created("session")) };
    OK
}
unsafe extern "C" fn session_select(_: Handle, backend: u32, _: u32, _: u64) -> i32 {
    event(format!("select {backend}"));
    OK
}
unsafe extern "C" fn session_set_auto_exposure(_: Handle, enabled: u32) -> i32 {
    event(format!("auto exposure {enabled}"));
    OK
}
unsafe extern "C" fn session_evaluate(_: Handle, list: Handle, frame: *const rsf_sr_frame) -> i32 {
    assert_eq!(name(list), "list");
    world(|world| world.evaluated = Some(unsafe { *frame }));
    event("evaluate".into());
    OK
}
unsafe extern "C" fn session_warm_up(_: Handle, _: Handle, frame: *const rsf_sr_frame) -> i32 {
    world(|world| world.evaluated = Some(unsafe { *frame }));
    event("warm up".into());
    OK
}
unsafe extern "C" fn session_get_status(_: Handle, status: *mut rsf_sr_session_status) -> i32 {
    unsafe { (*status).effective = 3 };
    OK
}
unsafe extern "C" fn session_destroy(session: Handle) {
    event("destroy session".into());
    close(session);
}
unsafe extern "C" fn validate_frame(frame: *const rsf_sr_frame) -> i32 {
    if frame.is_null() { INVALID } else { OK }
}
unsafe extern "C" fn immediate_context(_: Handle) -> Handle {
    created("immediate")
}
unsafe extern "C" fn query_context4(_: Handle) -> Handle {
    created("context")
}
unsafe extern "C" fn create_device12(_: Handle) -> Handle {
    created("device12")
}
unsafe extern "C" fn create_queue(_: Handle) -> Handle {
    created("queue")
}
unsafe extern "C" fn create_commands(_: Handle, allocator: *mut Handle, list: *mut Handle) -> i32 {
    unsafe {
        allocator.write(created("allocator"));
        list.write(created("list"));
    }
    1
}
unsafe extern "C" fn create_output(_: Handle, width: u32, height: u32, _: u32) -> Handle {
    created(&format!("output12 {width}x{height}"))
}
unsafe extern "C" fn create_texture(_: Handle, _: u32, _: u32, _: u32) -> Handle {
    created("warm texture")
}
unsafe extern "C" fn create_fence(_: Handle) -> Handle {
    created("warm fence")
}
unsafe extern "C" fn create_event() -> Handle {
    created("event")
}
unsafe extern "C" fn close_event(handle: Handle) {
    event(format!("close {}", name(handle)));
    close(handle);
}
unsafe extern "C" fn describe(texture: Handle, device: Handle, out: *mut TextureDesc) -> i32 {
    match world(|world| world.descs.get(&(texture as usize)).copied()) {
        Some((desc, owner)) if owner == device => {
            unsafe { out.write(desc) };
            1
        }
        _ => 0,
    }
}
unsafe extern "C" fn same_context(pointer: Handle, context: Handle) -> i32 {
    i32::from(pointer == context)
}
unsafe extern "C" fn wait_fence(fence: Handle, value: u64, _: Handle, _: u32) -> i32 {
    let fence = name(fence);
    event(format!("wait {fence} {value}"));
    let failed = world(|world| {
        if fence == "warm fence" {
            world.warm_up_wait_fails
        } else {
            world.wait_fails
        }
    });
    i32::from(!failed)
}
unsafe extern "C" fn flush(_: Handle) {
    event("flush".into());
}
unsafe extern "C" fn copy_region(_: Handle, to: Handle, from: Handle, width: u32, height: u32) {
    event(format!(
        "copy {} -> {} {width}x{height}",
        name(from),
        name(to)
    ));
}
unsafe extern "C" fn copy_resource(_: Handle, to: Handle, from: Handle) {
    event(format!("copy {} -> {}", name(from), name(to)));
}
unsafe extern "C" fn context_signal(_: Handle, fence: Handle, value: u64) -> i32 {
    assert_eq!(name(fence), "fence11");
    event(format!("d3d11 signal {value}"));
    1
}
unsafe extern "C" fn context_wait(_: Handle, fence: Handle, value: u64) -> i32 {
    assert_eq!(name(fence), "fence11");
    event(format!("d3d11 wait {value}"));
    1
}
unsafe extern "C" fn reset_commands(_: Handle, _: Handle) -> i32 {
    event("reset".into());
    1
}
unsafe extern "C" fn transition(_: Handle, resource: Handle, before: u32, after: u32) {
    event(format!(
        "barrier {} {before:#x}->{after:#x}",
        name(resource)
    ));
}
unsafe extern "C" fn copy_transitioned(
    _: Handle,
    to: Handle,
    _: u32,
    _: u32,
    from: Handle,
    _: u32,
    _: u32,
) {
    event(format!("copy {} -> {}", name(from), name(to)));
}
unsafe extern "C" fn submit(_: Handle, _: Handle) -> i32 {
    event("submit".into());
    1
}
unsafe extern "C" fn queue_wait(_: Handle, _: Handle, value: u64) -> i32 {
    event(format!("d3d12 wait {value}"));
    i32::from(!world(|world| world.queue_wait_fails))
}
unsafe extern "C" fn queue_signal(_: Handle, fence: Handle, value: u64) -> i32 {
    if name(fence) == "warm fence" {
        event(format!("d3d12 signal warm fence {value}"));
        return i32::from(!world(|world| world.warm_signal_fails));
    }
    event(format!("d3d12 signal {value}"));
    1
}
unsafe extern "C" fn device_ready(_: Handle) -> i32 {
    i32::from(world(|world| world.device_ready))
}
unsafe extern "C" fn surface_create(
    _: Handle,
    _: Handle,
    width: u32,
    height: u32,
    format: u32,
    _: Log,
    _: Handle,
) -> Handle {
    let slot = world(|world| {
        world.surfaces += 1;
        SLOT_NAMES[(world.surfaces - 1) % SLOTS]
    });
    let surface = created(&format!("{slot} surface"));
    let children = (object(&format!("{slot}.11")), object(&format!("{slot}.12")));
    world(|world| world.children.insert(surface as usize, children));
    event(format!("create {slot} {width}x{height} {format}"));
    surface
}
unsafe extern "C" fn surface_destroy(surface: Handle) {
    event(format!("destroy {}", name(surface)));
    close(surface);
}
fn child(handle: Handle, d3d12: bool) -> Handle {
    let (d3d11, other) = world(|world| world.children[&(handle as usize)]);
    if d3d12 { other } else { d3d11 }
}
unsafe extern "C" fn surface_d3d11(surface: Handle) -> Handle {
    child(surface, false)
}
unsafe extern "C" fn surface_d3d12(surface: Handle) -> Handle {
    child(surface, true)
}
unsafe extern "C" fn fence_create(_: Handle, _: Handle, _: Log, _: Handle) -> Handle {
    let fence = created("fence");
    let children = (object("fence11"), object("fence12"));
    world(|world| world.children.insert(fence as usize, children));
    fence
}
unsafe extern "C" fn fence_destroy(fence: Handle) {
    event("destroy fence".into());
    close(fence);
}
unsafe extern "C" fn fence_d3d11(fence: Handle) -> Handle {
    child(fence, false)
}
unsafe extern "C" fn fence_d3d12(fence: Handle) -> Handle {
    child(fence, true)
}
unsafe extern "C" fn dump_poll_requested() -> i32 {
    0
}
unsafe extern "C" fn dump_inputs(_: Handle, _: *const rsf_sr_frame, _: u32, _: u32) {}
unsafe extern "C" fn dump_wants(_: *const rsf_frame_record) -> i32 {
    0
}
unsafe extern "C" fn dump_result(_: Handle, _: *const rsf_sr_frame, _: u32, _: i32, _: u32) {}

static PLATFORM: Platform = Platform {
    session_create,
    session_select,
    session_set_auto_exposure,
    session_evaluate,
    session_warm_up,
    session_get_status,
    session_destroy,
    validate_frame,
    immediate_context,
    query_context4,
    create_device12,
    create_queue,
    create_commands,
    create_output,
    create_texture,
    create_fence,
    create_event,
    close_event,
    describe,
    same_context,
    wait_fence,
    flush,
    copy_region,
    copy_resource,
    context_signal,
    context_wait,
    reset_commands,
    transition,
    copy_transitioned,
    submit,
    queue_wait,
    queue_signal,
    device_ready,
    surface_create,
    surface_destroy,
    surface_d3d11,
    surface_d3d12,
    fence_create,
    fence_destroy,
    fence_d3d11,
    fence_d3d12,
    dump_poll_requested,
    dump_inputs,
    dump_wants,
    dump_result,
};

unsafe extern "C" fn log(_: Handle, message: *const std::ffi::c_char) {
    let message = unsafe { std::ffi::CStr::from_ptr(message) };
    let message = message.to_string_lossy().into_owned();
    world(|world| world.logs.push(message));
}

struct Scene {
    device: Handle,
    bridge: *mut Bridge,
    textures: Vec<Handle>,
    record: Box<rsf_frame_record>,
    frame: rsf_sr_frame,
}

fn resource(texture: Handle) -> rsf_backend_resource {
    rsf_backend_resource {
        struct_size: size_of::<rsf_backend_resource>() as u32,
        resource: texture,
        ..unsafe { std::mem::zeroed() }
    }
}

impl Scene {
    fn new() -> Self {
        let device = object("device11");
        let mut setup: rsf_sr_session_setup = unsafe { std::mem::zeroed() };
        setup.struct_size = size_of::<rsf_sr_session_setup>() as u32;
        setup.abi_version = RSF_SR_SESSION_ABI_VERSION;
        setup.open.struct_size = size_of::<rsf_sr_open_desc>() as u32;
        setup.open.abi_version = RSF_BACKEND_ABI_VERSION;
        setup.open.api = API_D3D11;
        setup.open.device = device;
        setup.open.output_width = 128;
        setup.open.output_height = 128;
        setup.open.hdr = 1;
        setup.open.auto_exposure = 1;
        setup.open.log = Some(log);
        let mut bridge = ptr::null_mut();
        let result = unsafe { rsf_core_sr_bridge_create(&setup, &mut bridge, &PLATFORM) };
        assert_eq!(result, OK);
        assert_eq!(refs(device), 2, "the bridge holds the game device");
        let textures = vec![
            texture(device, "colour", 128, 128, COLOR_FORMAT),
            texture(device, "depth", 128, 128, SCALAR_FORMAT),
            texture(device, "motion", 128, 128, MOTION_FORMAT),
            texture(device, "output", 128, 128, COLOR_FORMAT),
        ];
        let mut record: Box<rsf_frame_record> = Box::new(unsafe { std::mem::zeroed() });
        record.frame_id = 1;
        (record.render_width, record.render_height) = (64, 64);
        (record.output_width, record.output_height) = (128, 128);
        let mut frame: rsf_sr_frame = unsafe { std::mem::zeroed() };
        frame.struct_size = size_of::<rsf_sr_frame>() as u32;
        frame.record = &*record;
        frame.color = resource(textures[0]);
        frame.depth = resource(textures[1]);
        frame.motion = resource(textures[2]);
        frame.output = resource(textures[3]);
        take_events();
        Self {
            device,
            bridge,
            textures,
            record,
            frame,
        }
    }

    fn evaluate(&mut self) -> i32 {
        self.frame.record = &*self.record;
        unsafe { rsf_sr_bridge_evaluate(self.bridge, find("immediate"), &self.frame) }
    }

    // Destroys the bridge and checks that every object it owned was released exactly once.
    fn destroy(self) {
        unsafe { rsf_sr_bridge_destroy(self.bridge) };
        for handle in world(|world| world.created.clone()) {
            assert_eq!(refs(handle), 0, "{} still referenced", name(handle));
        }
        assert_eq!(refs(self.device), 1);
        for texture in self.textures {
            assert_eq!(
                refs(texture),
                1,
                "{} lease outlived the bridge",
                name(texture)
            );
        }
    }
}

#[test]
fn evaluate_orders_cross_api_transfers_and_releases_every_reference() {
    let mut scene = Scene::new();
    assert_eq!(scene.evaluate(), OK);
    let events = take_events();
    let reset = events.iter().position(|event| event == "reset").unwrap();
    assert_eq!(events[0], "wait fence12 0", "surfaces wait for retirement");
    assert_eq!(
        events[1..8],
        [
            "create colour 64x64 10",
            "create depth 64x64 41",
            "create motion 64x64 34",
            "create exposure 1x1 41",
            "create output 128x128 10",
            "create reactive 64x64 41",
            "create transparency 64x64 41",
        ]
    );
    assert_eq!(events[8], "wait fence12 0", "allocator reuse waits");
    assert_eq!(
        events[reset..],
        [
            "reset",
            "copy colour -> colour.11 64x64",
            "copy depth -> depth.11 64x64",
            "copy motion -> motion.11 64x64",
            "d3d11 signal 1",
            "flush",
            "d3d12 wait 1",
            "barrier colour.12 0x0->0x40",
            "barrier depth.12 0x0->0x40",
            "barrier motion.12 0x0->0x40",
            "evaluate",
            "barrier colour.12 0x40->0x0",
            "barrier depth.12 0x40->0x0",
            "barrier motion.12 0x40->0x0",
            "copy output12 128x128 -> output.12",
            "submit",
            "d3d12 signal 2",
            "d3d11 wait 2",
            "copy output.11 -> output",
            "d3d11 signal 3",
        ]
    );
    let translated = world(|world| world.evaluated.unwrap());
    assert_eq!(name(translated.color.resource), "colour.12");
    assert_eq!(translated.color.state, STATE_SHADER_RESOURCE);
    assert!(translated.exposure.resource.is_null());
    assert!(translated.reactive.resource.is_null());
    assert_eq!(name(translated.output.resource), "output12 128x128");
    assert_eq!(translated.output.state, STATE_UNORDERED_ACCESS);

    // The next frame reuses the surfaces and the next allocator; teardown flushes the pending
    // output signal before waiting on it.
    scene.record.frame_id = 2;
    assert_eq!(scene.evaluate(), OK);
    let events = take_events();
    assert_eq!(events[0], "wait fence12 0");
    assert!(!events.iter().any(|event| event.starts_with("create")));
    unsafe { rsf_sr_bridge_destroy(scene.bridge) };
    scene.bridge = ptr::null_mut();
    let events = take_events();
    assert_eq!(events[..3], ["flush", "wait fence12 6", "destroy session"]);
    assert_eq!(events.last().unwrap(), "close event");
    scene.destroy();
}

#[test]
fn resize_retires_surfaces_before_replacing_them() {
    let mut scene = Scene::new();
    assert_eq!(scene.evaluate(), OK);
    take_events();
    scene.record.frame_id = 2;
    (scene.record.render_width, scene.record.render_height) = (96, 96);
    assert_eq!(scene.evaluate(), OK);
    let events = take_events();
    assert_eq!(
        events[..4],
        [
            "flush",
            "wait fence12 3",
            "destroy colour surface",
            "destroy depth surface"
        ]
    );
    assert!(events.contains(&"create colour 96x96 10".to_owned()));
    scene.destroy();
}

#[test]
fn refused_inputs_issue_no_copy_and_unusable_masks_are_dropped() {
    let mut scene = Scene::new();
    let wide_motion = texture(scene.device, "wide motion", 128, 128, 16);
    scene.frame.motion = resource(wide_motion);
    assert_eq!(scene.evaluate(), INVALID);
    assert!(take_events().is_empty(), "no lease before validation");

    let foreign = texture(object("other device"), "foreign", 128, 128, COLOR_FORMAT);
    scene.frame.motion = resource(scene.textures[2]);
    scene.frame.color = resource(foreign);
    assert_eq!(scene.evaluate(), INVALID);
    assert!(take_events().is_empty());

    scene.frame.color = resource(scene.textures[0]);
    let reactive = texture(scene.device, "reactive", 128, 128, COLOR_FORMAT);
    let transparency = texture(scene.device, "transparency", 64, 64, SCALAR_FORMAT);
    scene.frame.reactive = resource(reactive);
    scene.frame.transparency = resource(transparency);
    assert_eq!(scene.evaluate(), OK);
    let events = take_events();
    assert!(events.contains(&"copy transparency -> transparency.11 64x64".to_owned()));
    assert!(!events.iter().any(|event| event.contains("reactive.")));
    let translated = world(|world| world.evaluated.unwrap());
    assert!(translated.reactive.resource.is_null());
    assert_eq!(translated.reactive.struct_size, 0);
    assert_eq!(name(translated.transparency.resource), "transparency.12");
    scene.textures.extend([wide_motion, reactive, transparency]);
    scene.destroy();
    assert_eq!(refs(foreign), 1);
}

#[test]
fn gpu_failure_faults_the_bridge_until_backend_none() {
    let mut scene = Scene::new();
    world(|world| world.queue_wait_fails = true);
    assert_eq!(scene.evaluate(), FEATURE_FAILED);
    world(|world| world.queue_wait_fails = false);
    scene.record.frame_id = 2;
    assert_eq!(scene.evaluate(), NOT_READY);
    assert_eq!(
        unsafe { rsf_sr_bridge_select(scene.bridge, 3, 0, 0) },
        NOT_READY
    );
    assert_eq!(
        unsafe { rsf_sr_bridge_set_auto_exposure(scene.bridge, 0) },
        NOT_READY
    );
    assert_eq!(
        unsafe { rsf_sr_bridge_warm_up(scene.bridge, 64, 64, 128, 128, 0) },
        NOT_READY
    );
    take_events();
    assert_eq!(
        unsafe { rsf_sr_bridge_select(scene.bridge, SR_NONE, 0, 0) },
        OK
    );
    assert_eq!(take_events(), ["wait fence12 1", "select 0"]);
    scene.destroy();
}

#[test]
fn timed_out_teardown_retains_the_bridge_unless_the_device_is_removed() {
    let mut scene = Scene::new();
    assert_eq!(scene.evaluate(), OK);
    world(|world| world.wait_fails = true);
    unsafe { rsf_sr_bridge_destroy(scene.bridge) };
    assert!(
        !take_events()
            .iter()
            .any(|event| event.starts_with("destroy"))
    );
    assert_eq!(refs(find("device12")), 1);
    assert_eq!(
        refs(scene.textures[0]),
        2,
        "the retained bridge keeps its input lease"
    );
    assert!(world(|world| world
        .logs
        .last()
        .unwrap()
        .contains("retaining")));

    world(|world| world.device_ready = false);
    scene.destroy();
}

#[test]
fn warm_up_retains_its_resources_until_gpu_completion() {
    let mut scene = Scene::new();
    assert_eq!(
        unsafe { rsf_sr_bridge_warm_up(scene.bridge, 64, 64, 128, 128, 1) },
        OK
    );
    let events = take_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.starts_with("barrier warm texture"))
            .count(),
        4
    );
    assert_eq!(events.iter().position(|event| event == "warm up"), Some(5));
    assert!(events.contains(&"wait warm fence 1".to_owned()));
    let frame = world(|world| world.evaluated.unwrap());
    assert_eq!(frame.reset, 1);
    assert_eq!(frame.output.state, STATE_UNORDERED_ACCESS);
    let warm = world(|world| {
        let names = &world.names;
        world
            .created
            .iter()
            .copied()
            .filter(|&handle| names[&(handle as usize)].starts_with("warm"))
            .collect::<Vec<_>>()
    });
    for handle in warm {
        assert_eq!(refs(handle), 0);
    }

    // An unobserved completion leaves the dispatch pending with every object it references.
    world(|world| world.warm_up_wait_fails = true);
    let before = world(|world| world.created.len());
    assert_eq!(
        unsafe { rsf_sr_bridge_warm_up(scene.bridge, 64, 64, 128, 128, 0) },
        FEATURE_FAILED
    );
    let pending = world(|world| world.created[before..].to_vec());
    assert_eq!(
        pending.len(),
        8,
        "three inputs, output, allocator, list, fence, event"
    );
    assert!(!take_events().contains(&"close event".to_owned()));

    // Nothing that uses or changes the SDK context runs, nothing new is submitted, and teardown
    // keeps the bridge; each call only checks the warm-up fence.
    let bridge = scene.bridge;
    assert_eq!(
        unsafe { rsf_sr_bridge_warm_up(bridge, 64, 64, 128, 128, 0) },
        NOT_READY
    );
    assert_eq!(unsafe { rsf_sr_bridge_select(bridge, 3, 0, 0) }, NOT_READY);
    assert_eq!(
        unsafe { rsf_sr_bridge_select(bridge, SR_NONE, 0, 0) },
        NOT_READY
    );
    assert_eq!(
        unsafe { rsf_sr_bridge_set_auto_exposure(bridge, 0) },
        NOT_READY
    );
    assert_eq!(scene.evaluate(), NOT_READY);
    unsafe { rsf_sr_bridge_destroy(bridge) };
    for event in take_events() {
        assert!(
            event == "wait warm fence 1" || event == "wait fence12 0",
            "{event} while a warm-up is pending"
        );
    }
    assert_eq!(world(|world| world.created.len()), before + pending.len());
    for &handle in &pending {
        assert_eq!(
            refs(handle),
            1,
            "{} released under pending work",
            name(handle)
        );
    }
    assert_eq!(
        refs(find("session")),
        1,
        "the SDK session outlives the warm-up"
    );

    // Observed completion releases the dispatch and the bridge is usable again.
    world(|world| world.warm_up_wait_fails = false);
    assert_eq!(unsafe { rsf_sr_bridge_select(bridge, 3, 0, 0) }, OK);
    assert_eq!(
        take_events(),
        [
            "wait warm fence 1",
            "close event",
            "wait fence12 0",
            "select 3"
        ]
    );
    for &handle in &pending {
        assert_eq!(refs(handle), 0, "{} not released", name(handle));
    }
    assert_eq!(
        unsafe { rsf_sr_bridge_warm_up(bridge, 64, 64, 128, 128, 0) },
        OK
    );
    assert!(take_events().contains(&"submit".to_owned()));
    scene.destroy();
}

#[test]
fn warm_up_signal_failure_is_accepted_only_after_an_ordered_retry() {
    let mut scene = Scene::new();
    world(|world| world.warm_signal_fails = true);
    assert_eq!(
        unsafe { rsf_sr_bridge_warm_up(scene.bridge, 64, 64, 128, 128, 0) },
        FEATURE_FAILED
    );
    let events = take_events();
    assert!(events.contains(&"d3d12 signal warm fence 1".to_owned()));
    assert!(
        !events
            .iter()
            .any(|event| event.starts_with("wait warm fence"))
    );
    // The fake fence would report completion, but without a signal queued behind the list its
    // value proves nothing, so the bridge retries the signal and does not wait.
    assert_eq!(scene.evaluate(), NOT_READY);
    assert_eq!(take_events(), ["d3d12 signal warm fence 1"]);

    world(|world| world.warm_signal_fails = false);
    assert_eq!(scene.evaluate(), OK);
    let events = take_events();
    assert_eq!(
        events[..3],
        [
            "d3d12 signal warm fence 1",
            "wait warm fence 1",
            "close event"
        ]
    );
    assert!(events.contains(&"evaluate".to_owned()));
    scene.destroy();
}

#[test]
fn device_removal_releases_a_pending_warm_up() {
    let scene = Scene::new();
    world(|world| world.warm_up_wait_fails = true);
    assert_eq!(
        unsafe { rsf_sr_bridge_warm_up(scene.bridge, 64, 64, 128, 128, 0) },
        FEATURE_FAILED
    );
    take_events();
    world(|world| world.device_ready = false);
    assert_eq!(
        unsafe { rsf_sr_bridge_select(scene.bridge, SR_NONE, 0, 0) },
        OK
    );
    assert_eq!(take_events()[..2], ["wait warm fence 1", "close event"]);
    assert!(world(|world| world
        .logs
        .iter()
        .any(|log| log.contains("removed device"))));
    scene.destroy();
}
