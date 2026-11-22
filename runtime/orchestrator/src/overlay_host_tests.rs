// SPDX-License-Identifier: MIT
use super::*;
use crate::com_ref::test::{Fake, add_ref, fake, raw};
use std::cell::RefCell;
use std::ffi::CStr;

const RENDERER: usize = 0x2000;

#[derive(Default)]
struct World {
    calls: Vec<String>,
    logs: Vec<String>,
    load: u32,
    load_error: u32,
    panel_null: bool,
    renderer_result: i32,
    window: usize,
    install_result: i32,
    visible: u32,
    tick: u64,
    counter: i64,
    chain_device: usize,
    chain_device_hr: i32,
    buffer: usize,
    owner: usize,
    size: (u32, u32),
    view: usize,
    view_hr: i32,
    context: usize,
    saved_view: usize,
    frame_result: i32,
    close_requested: u32,
    updates: Vec<u32>,
    frees: Vec<u32>,
    upload_result: i32,
    draw_result: i32,
    alpha: f32,
    delta: f32,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World::default());
}

fn world<T>(change: impl FnOnce(&mut World) -> T) -> T {
    WORLD.with_borrow_mut(change)
}

fn call(name: impl Into<String>) {
    world(|world| world.calls.push(name.into()));
}

fn calls() -> Vec<String> {
    world(|world| std::mem::take(&mut world.calls))
}

fn logs() -> Vec<String> {
    world(|world| std::mem::take(&mut world.logs))
}

/* Returned COM pointers transfer a reference, like the native leaves. */
fn give(object: usize) -> Handle {
    let object = object as Handle;
    if !object.is_null() {
        unsafe { add_ref(object.cast()) };
    }
    object
}

unsafe extern "C" fn capture(_: Handle, message: *const c_char) {
    let line = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    world(|world| world.logs.push(line));
}

unsafe extern "C" fn create(abi: u32) -> *mut rsf_overlay {
    assert_eq!(abi, RSF_OVERLAY_ABI_VERSION);
    call("create");
    if world(|world| world.panel_null) {
        null_mut()
    } else {
        ptr::dangling_mut()
    }
}
unsafe extern "C" fn destroy(_: *mut rsf_overlay) {
    call("destroy");
}
unsafe extern "C" fn frame(
    _: *mut rsf_overlay,
    input: *const rsf_overlay_input,
    _: *const rsf_overlay_stats,
    data: *mut rsf_overlay_draw_data,
    intent: *mut rsf_overlay_intent,
) -> i32 {
    call("frame");
    let input = unsafe { &*input };
    assert_eq!(input.struct_size as usize, size_of::<rsf_overlay_input>());
    world(|world| {
        world.alpha = input.startup_hint_alpha;
        world.delta = input.delta_seconds;
        unsafe {
            (*data).vertex_count = 3;
            (*data).index_count = 6;
            (*intent).close_requested = world.close_requested;
            (*intent).quality_changed = 1;
        }
        world.frame_result
    })
}
unsafe extern "C" fn texture_updates(
    _: *mut rsf_overlay,
    updates: *mut rsf_overlay_texture_update,
    capacity: u32,
) -> u32 {
    call("updates");
    let count = world(|world| (!world.updates.is_empty()).then(|| world.updates.remove(0)));
    let count = count.unwrap_or(0).min(capacity);
    for index in 0..count {
        unsafe { (*updates.add(index as usize)).id = u64::from(index) };
    }
    count
}
unsafe extern "C" fn textures_to_free(_: *mut rsf_overlay, ids: *mut u64, capacity: u32) -> u32 {
    call("frees");
    let count = world(|world| (!world.frees.is_empty()).then(|| world.frees.remove(0)));
    let count = count.unwrap_or(0).min(capacity);
    for index in 0..count {
        unsafe { ids.add(index as usize).write(u64::from(index)) };
    }
    count
}

unsafe extern "C" fn load_panel(out: *mut rsf_overlay_host_panel, error: *mut u32) -> u32 {
    call("load");
    let (status, code) = world(|world| (world.load, world.load_error));
    if status == RSF_OVERLAY_HOST_PANEL_LOADED as u32 {
        unsafe {
            out.write(rsf_overlay_host_panel {
                create: Some(create),
                destroy: Some(destroy),
                frame: Some(frame),
                texture_updates: Some(texture_updates),
                textures_to_free: Some(textures_to_free),
            })
        };
    } else {
        unsafe { error.write(code) };
    }
    status
}
unsafe extern "C" fn chain_device(_: Handle, device: *mut Handle) -> i32 {
    call("chain_device");
    let (object, hr) = world(|world| (world.chain_device, world.chain_device_hr));
    unsafe { device.write(give(object)) };
    hr
}
unsafe extern "C" fn chain_buffer(_: Handle, texture: *mut Handle) -> i32 {
    call("chain_buffer");
    unsafe { texture.write(give(world(|world| world.buffer))) };
    0
}
unsafe extern "C" fn chain_window(_: Handle) -> Handle {
    call("chain_window");
    world(|world| world.window) as Handle
}
unsafe extern "C" fn same_object(first: Handle, second: Handle) -> u32 {
    (!first.is_null() && first == second) as u32
}
unsafe extern "C" fn immediate_context(_: Handle) -> Handle {
    give(world(|world| world.context))
}
unsafe extern "C" fn texture_device(_: Handle) -> Handle {
    give(world(|world| world.owner))
}
unsafe extern "C" fn texture_desc(_: Handle, out: *mut TextureDesc) {
    let (width, height) = world(|world| world.size);
    unsafe {
        out.write(TextureDesc {
            width,
            height,
            format: 28,
            bind_flags: 0x20,
        })
    };
}
unsafe extern "C" fn create_view(_: Handle, _: Handle, view: *mut Handle) -> i32 {
    call("create_view");
    let (object, hr) = world(|world| (world.view, world.view_hr));
    unsafe { view.write(if hr < 0 { null_mut() } else { give(object) }) };
    hr
}
unsafe extern "C" fn save_targets(_: Handle, out: *mut Targets) {
    call("save_targets");
    unsafe { (*out).views[0] = give(world(|world| world.saved_view)) };
}
unsafe extern "C" fn bind_target(_: Handle, view: Handle) {
    call(format!("bind {}", view as usize));
}
unsafe extern "C" fn restore_targets(_: Handle, saved: *const Targets) {
    call(format!("restore {}", unsafe { (*saved).views[0] } as usize));
}
unsafe extern "C" fn renderer_create(_: Handle, log: Log, _: Handle, out: *mut Handle) -> i32 {
    call("renderer_create");
    assert!(log.is_some());
    let result = world(|world| world.renderer_result);
    if result == 0 {
        unsafe { out.write(RENDERER as Handle) };
    }
    result
}
unsafe extern "C" fn renderer_upload(
    renderer: Handle,
    _: Handle,
    _: *const rsf_overlay_texture_update,
) -> i32 {
    assert_eq!(renderer as usize, RENDERER);
    call("upload");
    world(|world| world.upload_result)
}
unsafe extern "C" fn renderer_free(_: Handle, _: u64) {
    call("free");
}
unsafe extern "C" fn renderer_draw(
    _: Handle,
    _: Handle,
    data: *const rsf_overlay_draw_data,
    width: u32,
    height: u32,
) -> i32 {
    call(format!("draw {width}x{height} {}", unsafe {
        (*data).vertex_count
    }));
    world(|world| world.draw_result)
}
unsafe extern "C" fn renderer_destroy(_: Handle) {
    call("renderer_destroy");
}
unsafe extern "C" fn input_install(_: Handle, key: u32, log: Log, _: Handle) -> i32 {
    call(format!("install {key:#x}"));
    assert!(log.is_some());
    world(|world| world.install_result)
}
unsafe extern "C" fn input_uninstall() {
    call("uninstall");
}
unsafe extern "C" fn input_visible() -> u32 {
    world(|world| world.visible)
}
unsafe extern "C" fn input_set_visible(visible: u32) {
    call(format!("set_visible {visible}"));
    world(|world| world.visible = visible);
}
unsafe extern "C" fn input_collect(
    out: *mut rsf_overlay_input,
    width: u32,
    height: u32,
    delta: f32,
) {
    unsafe {
        (*out).display_width = width;
        (*out).display_height = height;
        (*out).delta_seconds = delta;
    }
}
unsafe extern "C" fn tick() -> u64 {
    world(|world| world.tick)
}
unsafe extern "C" fn counter(value: *mut i64) -> u32 {
    unsafe { value.write(world(|world| world.counter)) };
    1
}
unsafe extern "C" fn frequency(value: *mut i64) -> u32 {
    unsafe { value.write(1000) };
    1
}
unsafe extern "C" fn thread_id() -> u32 {
    7
}

fn platform() -> Platform {
    Platform {
        load_panel: Some(load_panel),
        chain_device: Some(chain_device),
        chain_buffer: Some(chain_buffer),
        chain_window: Some(chain_window),
        same_object: Some(same_object),
        immediate_context: Some(immediate_context),
        texture_device: Some(texture_device),
        texture_desc: Some(texture_desc),
        create_view: Some(create_view),
        save_targets: Some(save_targets),
        bind_target: Some(bind_target),
        restore_targets: Some(restore_targets),
        renderer_create: Some(renderer_create),
        renderer_upload: Some(renderer_upload),
        renderer_free: Some(renderer_free),
        renderer_draw: Some(renderer_draw),
        renderer_destroy: Some(renderer_destroy),
        input_install: Some(input_install),
        input_uninstall: Some(input_uninstall),
        input_visible: Some(input_visible),
        input_set_visible: Some(input_set_visible),
        input_collect: Some(input_collect),
        tick: Some(tick),
        counter: Some(counter),
        frequency: Some(frequency),
        thread_id: Some(thread_id),
    }
}

/* One presenting device with its chain, back buffer, context and views. */
struct Scene {
    device: Box<Fake>,
    other: Box<Fake>,
    buffer: Box<Fake>,
    context: Box<Fake>,
    view: Box<Fake>,
    saved: Box<Fake>,
}

impl Scene {
    fn new() -> Self {
        let scene = Self {
            device: fake(),
            other: fake(),
            buffer: fake(),
            context: fake(),
            view: fake(),
            saved: fake(),
        };
        let addresses = [
            raw(&scene.device),
            raw(&scene.buffer),
            raw(&scene.context),
            raw(&scene.view),
            raw(&scene.saved),
        ]
        .map(|object| object as usize);
        world(|world| {
            *world = World {
                load: RSF_OVERLAY_HOST_PANEL_LOADED as u32,
                window: 0x40,
                chain_device: addresses[0],
                buffer: addresses[1],
                owner: addresses[0],
                context: addresses[2],
                view: addresses[3],
                saved_view: addresses[4],
                size: (64, 32),
                ..World::default()
            }
        });
        set_logger(Some(capture), null_mut());
        scene
    }

    fn balanced(&self) -> bool {
        [
            &self.device,
            &self.other,
            &self.buffer,
            &self.context,
            &self.view,
            &self.saved,
        ]
        .iter()
        .all(|object| object.refs.get() == 1)
    }
}

const CHAIN: Handle = 0x10 as Handle;

fn stats(hud: bool) -> rsf_overlay_stats {
    rsf_overlay_stats {
        show_performance_hud: hud as u32,
        ..Default::default()
    }
}

fn started(p: &Platform) -> Host {
    let mut host = Host::new();
    assert_eq!(host.start(p, CHAIN), 1);
    calls();
    logs();
    host
}

#[test]
fn chain_start_builds_in_order_and_a_refusal_holds_until_stop() {
    let scene = Scene::new();
    let p = platform();
    world(|world| {
        world.load = RSF_OVERLAY_HOST_PANEL_NOT_LOADED as u32;
        world.load_error = 126;
    });
    let mut host = Host::new();
    assert_eq!(host.start(&p, CHAIN), 0);
    assert!(host.failed);
    assert_eq!(calls(), ["chain_device", "load"]);
    let lines = logs();
    assert!(lines[0].starts_with("overlay: swap chain 0000000000000010 selects device "));
    assert!(lines[1].contains("did not load, error 126. Set RSF_OVERLAY_DLL"));
    assert_eq!(host.start(&p, CHAIN), 0);
    assert!(
        calls().is_empty(),
        "a refused start is not retried every frame"
    );

    host.stop(&p);
    world(|world| world.load = RSF_OVERLAY_HOST_PANEL_LOADED as u32);
    assert_eq!(host.start(&p, CHAIN), 1);
    assert_eq!(
        calls(),
        [
            "chain_device",
            "load",
            "create",
            "renderer_create",
            "chain_window",
            "install 0x2d"
        ]
    );
    assert!(logs()[1].starts_with("overlay: ready. Insert opens it"));
    assert_eq!(host.start(&p, CHAIN), 1);
    assert!(calls().is_empty());
    assert_eq!(
        scene.device.refs.get(),
        2,
        "the host holds the selected device"
    );

    host.stop(&p);
    assert_eq!(
        calls(),
        ["set_visible 0", "uninstall", "renderer_destroy", "destroy"]
    );
    assert!(scene.balanced());
    assert_eq!(host.start(&p, CHAIN), 1);
    assert!(
        !calls().contains(&"load".to_owned()),
        "the panel DLL stays loaded"
    );
    host.stop(&p);
}

#[test]
fn a_refused_step_rolls_back_what_it_built() {
    let scene = Scene::new();
    let p = platform();
    world(|world| world.install_result = -5);
    let mut host = Host::new();
    assert_eq!(host.start(&p, CHAIN), 0);
    assert_eq!(
        calls(),
        [
            "chain_device",
            "load",
            "create",
            "renderer_create",
            "chain_window",
            "install 0x2d",
            "renderer_destroy",
            "destroy"
        ]
    );
    assert!(logs()[1].contains("input hook, result -5."));
    assert!(scene.balanced());

    for (missing, line) in [
        (RSF_OVERLAY_HOST_PANEL_NO_PATH, "cannot work out where"),
        (
            RSF_OVERLAY_HOST_PANEL_MISSING_EXPORTS,
            "does not export the five",
        ),
    ] {
        let mut host = Host::new();
        world(|world| world.load = missing as u32);
        assert_eq!(host.start(&p, CHAIN), 0);
        assert!(logs()[1].contains(line));
    }

    world(|world| {
        world.load = RSF_OVERLAY_HOST_PANEL_LOADED as u32;
        world.panel_null = true;
    });
    let mut host = Host::new();
    let device = raw(&scene.device);
    assert_eq!(
        host.start_device(&p, device, 0x40 as Handle, Some(capture), null_mut()),
        0
    );
    assert_eq!(logs(), ["overlay: panel ABI mismatch"]);
    calls();
    assert!(scene.balanced());
}

#[test]
fn present_fades_the_startup_hint_then_hides_and_resets_frame_time() {
    let scene = Scene::new();
    let p = platform();
    let mut host = started(&p);
    let hidden = stats(false);
    let mut intent = rsf_overlay_intent::default();
    world(|world| {
        world.tick = 1000;
        world.counter = 5000;
    });
    assert_eq!(host.present(&p, CHAIN, &hidden, &mut intent), 1);
    assert_eq!(world(|world| world.alpha), 1.0);
    assert_eq!(intent.quality_changed, 1);
    world(|world| {
        world.tick = 8500;
        world.counter = 5016;
    });
    assert_eq!(host.present(&p, CHAIN, &hidden, &mut intent), 1);
    assert_eq!(world(|world| (world.alpha, world.delta)), (0.5, 0.016));
    world(|world| world.tick = 9000);
    calls();
    assert_eq!(host.present(&p, CHAIN, &hidden, &mut intent), 0);
    assert!(calls().is_empty());
    assert_eq!(host.last_frame, 0);

    world(|world| world.counter = 9000);
    assert_eq!(host.present(&p, CHAIN, &stats(true), &mut intent), 1);
    assert_eq!(
        world(|world| (world.alpha, world.delta)),
        (0.0, DEFAULT_DELTA)
    );
    host.stop(&p);
    assert!(scene.balanced());
}

#[test]
fn present_draws_between_saved_targets_and_releases_every_reference() {
    let scene = Scene::new();
    let p = platform();
    let mut host = started(&p);
    world(|world| {
        world.visible = 1;
        world.close_requested = 1;
    });
    let mut intent = rsf_overlay_intent::default();
    assert_eq!(host.present(&p, CHAIN, &stats(false), &mut intent), 1);
    let view = raw(&scene.view) as usize;
    let saved = raw(&scene.saved) as usize;
    assert_eq!(
        calls(),
        [
            "chain_device".to_owned(),
            "chain_buffer".into(),
            "create_view".into(),
            "frame".into(),
            "set_visible 0".into(),
            "updates".into(),
            "save_targets".into(),
            format!("bind {view}"),
            "draw 64x32 3".into(),
            format!("restore {saved}"),
            "frees".into(),
        ]
    );
    assert_eq!(intent.close_requested, 1);
    let trace = logs();
    assert!(trace[0].starts_with("overlay frame: visible on thread 7"));
    assert!(trace[1].ends_with("64x32 format 28 bind 0x20. Creating view"));
    assert!(trace[2].starts_with("overlay view: CreateRenderTargetView returned hr 0x00000000"));
    assert_eq!(scene.view.refs.get(), 1);
    assert_eq!(scene.saved.refs.get(), 1);
    assert_eq!(scene.buffer.refs.get(), 1);
    assert_eq!(scene.context.refs.get(), 1);

    world(|world| world.draw_result = -4);
    let mut untouched = rsf_overlay_intent::default();
    assert_eq!(host.present(&p, CHAIN, &stats(true), &mut untouched), 0);
    assert_eq!(
        untouched.close_requested, 0,
        "a refused draw reports no intent"
    );
    assert!(
        logs()
            .last()
            .unwrap()
            .ends_with("would not draw, result -4")
    );
    host.stop(&p);
    assert!(scene.balanced());
}

#[test]
fn present_admits_only_the_selected_device_and_its_back_buffer() {
    let scene = Scene::new();
    let p = platform();
    let mut host = started(&p);
    let hud = stats(true);
    let other = raw(&scene.other) as usize;
    world(|world| world.chain_device = other);
    assert_eq!(host.present(&p, CHAIN, &hud, null_mut()), 0);
    assert!(!host.failed);
    assert!(
        logs()[1].starts_with("overlay: skipping swap chain 0000000000000010 on another device")
    );
    assert_eq!(host.trace_frames, TRACE_FRAMES - 1);

    let device = raw(&scene.device) as usize;
    world(|world| {
        world.chain_device = device;
        world.owner = other;
        world.visible = 1;
    });
    calls();
    assert_eq!(host.present(&p, CHAIN, &hud, null_mut()), 0);
    assert!(host.failed);
    assert!(calls().contains(&"set_visible 0".to_owned()));
    assert!(logs().last().unwrap().ends_with("Closing panel"));
    assert_eq!(host.present(&p, CHAIN, &hud, null_mut()), 0);
    assert!(calls().is_empty());

    host.stop(&p);
    world(|world| {
        world.owner = device;
        world.view_hr = -2147024882;
    });
    assert_eq!(host.start(&p, CHAIN), 1);
    logs();
    assert_eq!(host.present(&p, CHAIN, &hud, null_mut()), 0);
    assert!(host.failed);
    assert_eq!(
        logs().last().unwrap(),
        "overlay: no back-buffer view, hr 0x8007000e. Closing panel"
    );
    host.stop(&p);
    assert!(scene.balanced());
}

// Reproduced at 05facc0: a poisoned panel was closed but, with the performance HUD on, laid out
// again and reported its failure on every later Present.
#[test]
fn a_layout_failure_is_reported_once_even_with_the_performance_hud() {
    let scene = Scene::new();
    let p = platform();
    let mut host = started(&p);
    host.trace_frames = 0;
    world(|world| world.frame_result = -3);
    let context = raw(&scene.context);
    let target = raw(&scene.view);
    let hud = stats(true);
    world(|world| world.visible = 1);
    assert_eq!(
        host.draw_target(&p, context, target, 64, 32, &hud, null_mut()),
        0
    );
    assert_eq!(world(|world| world.visible), 0);
    logs(); // Drop the supplied-target trace before checking for further layout attempts.
    for _ in 0..4 {
        assert_eq!(host.present(&p, CHAIN, &hud, null_mut()), 0);
        assert_eq!(
            host.draw_target(&p, context, target, 64, 32, &hud, null_mut()),
            0
        );
    }
    assert!(logs().is_empty());
    assert_eq!(calls().iter().filter(|call| *call == "frame").count(), 1);
    host.stop(&p);
    assert!(scene.balanced());
}

#[test]
fn textures_drain_in_batches_until_one_comes_back_short() {
    let scene = Scene::new();
    let p = platform();
    let mut host = started(&p);
    host.trace_frames = 0;
    world(|world| {
        world.visible = 1;
        world.updates = vec![8, 3];
        world.frees = vec![8, 2];
        world.upload_result = -1;
    });
    assert_eq!(host.present(&p, CHAIN, &stats(false), null_mut()), 1);
    let calls = calls();
    let count = |name: &str| calls.iter().filter(|call| *call == name).count();
    assert_eq!(
        (
            count("updates"),
            count("upload"),
            count("frees"),
            count("free")
        ),
        (2, 11, 2, 10)
    );
    assert_eq!(
        logs()
            .iter()
            .filter(|line| line.contains("atlas update was refused"))
            .count(),
        11
    );
    host.stop(&p);
    assert!(scene.balanced());
}

#[test]
fn draw_target_keeps_the_device_hint_clock_and_retraces_each_opening() {
    let scene = Scene::new();
    let p = platform();
    let mut host = Host::new();
    world(|world| world.tick = 5000);
    let device = raw(&scene.device);
    assert_eq!(
        host.start_device(&p, device, 0x40 as Handle, Some(capture), null_mut()),
        1
    );
    assert_eq!(host.hint_begin, 5000);
    calls();
    logs();
    let foreign = raw(&scene.other);
    assert_eq!(
        host.start_device(&p, foreign, 0x40 as Handle, Some(capture), null_mut()),
        0
    );
    host.stop_device(&p, foreign);
    assert!(host.started && calls().is_empty());
    let context = raw(&scene.context);
    let target = raw(&scene.view);
    let mut intent = rsf_overlay_intent::default();
    world(|world| world.tick = 12500);
    assert_eq!(
        host.draw_target(&p, context, target, 64, 32, &stats(false), &mut intent),
        1
    );
    assert_eq!(world(|world| world.alpha), 0.5);
    world(|world| world.tick = 13000);
    calls();
    assert_eq!(
        host.draw_target(&p, context, target, 64, 32, &stats(false), &mut intent),
        0
    );
    assert!(calls().is_empty());
    assert!(logs().is_empty(), "hidden frames are not traced");

    host.trace_frames = 0;
    world(|world| {
        world.visible = 1;
        world.draw_result = -4;
    });
    intent = rsf_overlay_intent::default();
    assert_eq!(
        host.draw_target(&p, context, target, 64, 32, &stats(false), &mut intent),
        0
    );
    assert_eq!(
        intent.quality_changed, 1,
        "draw_target reports intent even when the draw fails"
    );
    assert_eq!(
        host.trace_frames,
        TRACE_FRAMES - 1,
        "opening the panel traces again"
    );
    assert_eq!(
        logs().last().unwrap(),
        "D3D11-on-12 overlay: draw completed result=-4"
    );
    host.stop_device(&p, device);
    assert!(scene.balanced());
}

#[test]
fn frame_time_defaults_to_sixty_hertz_outside_a_plausible_frame() {
    let p = platform();
    let _scene = Scene::new();
    let mut host = Host::new();
    assert_eq!(host.seconds_since_last_frame(&p), DEFAULT_DELTA);
    host.frequency = 1000;
    world(|world| world.counter = 100);
    assert_eq!(host.seconds_since_last_frame(&p), DEFAULT_DELTA);
    world(|world| world.counter = 120);
    assert_eq!(host.seconds_since_last_frame(&p), 0.02);
    world(|world| world.counter = 400);
    assert_eq!(host.seconds_since_last_frame(&p), DEFAULT_DELTA);
    world(|world| world.counter = 390);
    assert_eq!(host.seconds_since_last_frame(&p), DEFAULT_DELTA);
}

// The only test that touches the process-wide host.
#[test]
fn the_exported_entry_points_share_one_host() {
    let scene = Scene::new();
    let p = platform();
    unsafe {
        rsf_core_overlay_host_toggle(&p);
        assert_eq!(
            logs(),
            ["overlay: not running, so there is nothing to open"]
        );
        assert_eq!(
            rsf_core_overlay_host_start(&p, CHAIN, Some(capture), null_mut()),
            1
        );
        let device = raw(&scene.device);
        assert_eq!(
            rsf_core_overlay_host_start_device(
                &p,
                device,
                0x40 as Handle,
                Some(capture),
                null_mut()
            ),
            1
        );
        assert_eq!(calls().iter().filter(|call| *call == "create").count(), 1);
        assert_eq!(rsf_core_overlay_host_visible(&p), 0);
        rsf_core_overlay_host_toggle(&p);
        assert_eq!(rsf_core_overlay_host_visible(&p), 1);
        let hud = stats(true);
        assert_eq!(
            rsf_core_overlay_host_present(&p, CHAIN, &hud, null_mut()),
            1
        );
        assert_eq!(
            rsf_core_overlay_host_present(&p, CHAIN, ptr::null(), null_mut()),
            0
        );
        let held = hold();
        assert_eq!(
            rsf_core_overlay_host_present(&p, CHAIN, &hud, null_mut()),
            0
        );
        assert!(
            logs()
                .last()
                .unwrap()
                .ends_with("thread 7 skipped, another is already drawing the panel")
        );
        let context = raw(&scene.context);
        let target = raw(&scene.view);
        assert_eq!(
            rsf_core_overlay_host_draw_target(&p, context, target, 8, 8, &hud, null_mut()),
            0
        );
        drop(held);
        rsf_core_overlay_host_stop_device(&p, raw(&scene.other));
        assert_eq!(rsf_core_overlay_host_visible(&p), 1);
        rsf_core_overlay_host_stop(&p);
        assert_eq!(rsf_core_overlay_host_visible(&p), 0);
        assert_eq!(
            rsf_core_overlay_host_present(&p, CHAIN, &hud, null_mut()),
            0
        );
    }
    assert!(scene.balanced());
}
