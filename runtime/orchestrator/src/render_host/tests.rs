// SPDX-License-Identifier: MIT
// Synthetic render host contracts: a recording platform, reference-counted fake COM objects and a
// temporary INI section. No D3D12 device, vendor SDK, overlay or window is involved, so these
// establish admission, settings, slot, fence, scene, status and overlay routing policy only.
use super::*;
use crate::com_ref::test::add_ref;
use std::cell::RefCell;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, WritePrivateProfileStringW,
};

type LogFn = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;

// IUnknown layout: the shared fake first, so a COM pointer is also a pointer to this record.
#[repr(C)]
struct Fake {
    com: com_ref::test::Fake,
    name: String,
    device: Handle,
}

fn fake(name: &str, device: Handle) -> Handle {
    Box::into_raw(Box::new(Fake {
        com: com_ref::test::Fake::new(),
        name: name.to_owned(),
        device,
    }))
    .cast()
}
fn object<'a>(handle: Handle) -> &'a Fake {
    unsafe { &*handle.cast::<Fake>() }
}
fn refs(handle: Handle) -> u32 {
    object(handle).com.refs.get()
}

#[derive(Default)]
struct World {
    refused_preset: Option<u32>,
    events: Vec<String>,
    now: u64,
    completed: u64,
    wait_result: i32,
    signal_ok: bool,
    vendor: u32,
    evaluate_result: i32,
    create_result: i32,
    present_owner: i32,
    present_choices: u32,
    request_result: i32,
    fg_capture_result: i32,
    fg_status: Option<FgStatus>,
    intent: Option<Intent>,
    stats: Option<Stats>,
    observer_result: i32,
    contexts: u32,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World {
        signal_ok: true,
        wait_result: 1,
        vendor: 0x10de,
        present_choices: (1 << 1) | (1 << 3),
        ..World::default()
    });
}

crate::host_test_fakes::host_test_fakes!();

fn has_event(events: &[String], prefix: &str) -> bool {
    events.iter().any(|event| event.starts_with(prefix))
}

pub(super) unsafe extern "C" fn foreground() -> u32 {
    1
}
pub(super) unsafe extern "C" fn child_device(child: Handle) -> Handle {
    let device = object(child).device;
    unsafe { add_ref(device.cast()) };
    device
}
pub(super) unsafe extern "C" fn queue_direct(queue: Handle) -> i32 {
    i32::from(object(queue).name.contains("direct"))
}
pub(super) unsafe extern "C" fn create_fence(device: Handle) -> Handle {
    event("fence".into());
    fake("fence", device)
}
pub(super) unsafe extern "C" fn fence_completed(_: Handle) -> u64 {
    with(|world| world.completed)
}
pub(super) unsafe extern "C" fn fence_wait(_: Handle, value: u64, timeout: u32) -> i32 {
    event(format!("wait {value} {timeout}"));
    with(|world| {
        if world.wait_result > 0 {
            world.completed = value;
        }
        world.wait_result
    })
}
pub(super) unsafe extern "C" fn queue_signal(queue: Handle, _: Handle, value: u64) -> i32 {
    event(format!("signal {} {value}", object(queue).name));
    i32::from(with(|world| world.signal_ok))
}
pub(super) unsafe extern "C" fn adapter(_: Handle, out: *mut Adapter) -> i32 {
    let vendor = with(|world| world.vendor);
    unsafe {
        out.write(Adapter {
            vendor,
            device: 0x1234,
            software: 0,
        })
    };
    1
}
pub(super) unsafe extern "C" fn native_object(object: Handle) -> Handle {
    unsafe { add_ref(object.cast()) };
    object
}
pub(super) unsafe extern "C" fn depth_domains(_: *const Pass) -> u32 {
    0
}
pub(super) extern "C" fn fg_default_backend(vendor: u32, _: u32) -> u32 {
    if vendor == 0x10de { 1 } else { 3 }
}
pub(super) unsafe extern "C" fn sr12_create(setup: *const SrSetup, out: *mut Handle) -> i32 {
    let setup = unsafe { &*setup };
    let result = with(|world| {
        world.contexts += 1;
        world.create_result
    });
    event(format!(
        "create backend={} quality={} {}x{}->{}x{} exposure={} registration={}",
        setup.backend,
        setup.quality,
        setup.render_width,
        setup.render_height,
        setup.output_width,
        setup.output_height,
        setup.external_exposure,
        u32::from(!setup.dlss_registration.is_null())
    ));
    if result == BACKEND_OK {
        unsafe { out.write(with(|world| world.contexts as usize) as Handle) };
    }
    result
}
pub(super) unsafe extern "C" fn sr12_evaluate(
    context: Handle,
    _: Handle,
    pass: *const Pass,
    slot: u32,
) -> i32 {
    let reset = unsafe { (*pass).flags } & RSF_GAME_RENDER_RESET != 0;
    event(format!(
        "evaluate {} slot={slot} reset={}",
        context as usize,
        u32::from(reset)
    ));
    with(|world| world.evaluate_result)
}
pub(super) unsafe extern "C" fn sr12_set_sharpening(_: Handle, options: *const Sharpening) -> i32 {
    event(format!("sharpen {}", unsafe { (*options).sharpness }));
    BACKEND_OK
}
pub(super) unsafe extern "C" fn sr12_present(
    _: Handle,
    _: Handle,
    _: u32,
    _: u32,
    begin: u32,
) -> i32 {
    event(format!("sr present {begin}"));
    BACKEND_OK
}
pub(super) unsafe extern "C" fn sr12_capture(_: Handle, _: *const c_char, _: u64, _: u32) -> i32 {
    BACKEND_OK
}
pub(super) unsafe extern "C" fn sr12_capture_flush(_: Handle, slot: u32) -> i32 {
    event(format!("flush {slot}"));
    BACKEND_OK
}
pub(super) unsafe extern "C" fn sr12_capture_scene(
    _: Handle,
    _: Handle,
    _: *const Pass,
    slot: u32,
) -> i32 {
    event(format!("capture scene {slot}"));
    BACKEND_OK
}
pub(super) unsafe extern "C" fn registration(
    _: Handle,
    _: *const DlssSetup,
    out: *mut Handle,
) -> i32 {
    event("registration".into());
    unsafe { out.write(0x5000 as Handle) };
    BACKEND_OK
}
pub(super) unsafe extern "C" fn registration_present(
    _: Handle,
    _: Handle,
    _: u32,
    _: u32,
    begin: u32,
) -> i32 {
    event(format!("registration present {begin}"));
    BACKEND_OK
}
pub(super) unsafe extern "C" fn fg_install(setup: *const FgSetup) -> i32 {
    let setup = unsafe { &*setup };
    event(format!(
        "fg install mode={} switching={}",
        setup.options.mode, setup.runtime_switching
    ));
    BACKEND_OK
}
pub(super) unsafe extern "C" fn fg_options(options: *const FgOptions) {
    let options = unsafe { &*options };
    event(format!(
        "fg options mode={} generated={} reflex={} limit={}",
        options.mode, options.generated_frames, options.reflex_mode, options.frame_limit_us
    ));
}
pub(super) unsafe extern "C" fn fg_latency(_: *const LatencyEvent) -> i32 {
    BACKEND_OK
}
pub(super) unsafe extern "C" fn fg_cpu(_: *const CpuEvent) {
    event("fg cpu".into());
}
pub(super) unsafe extern "C" fn fg_status_try(out: *mut FgStatus) -> i32 {
    match with(|world| world.fg_status) {
        Some(status) => {
            unsafe { out.write(status) };
            1
        }
        None => 0,
    }
}
pub(super) unsafe extern "C" fn fg_capture_outputs(directory: *const c_char) -> i32 {
    let directory = unsafe { std::ffi::CStr::from_ptr(directory) };
    event(format!("fg capture {}", directory.to_str().unwrap()));
    with(|world| world.fg_capture_result)
}
pub(super) extern "C" fn present_host() -> Handle {
    null_mut()
}
pub(super) extern "C" fn present_has_owner() -> i32 {
    with(|world| world.present_owner)
}
pub(super) extern "C" fn present_backend_choices() -> u32 {
    with(|world| world.present_choices)
}
pub(super) extern "C" fn present_backend() -> u32 {
    0
}
pub(super) unsafe extern "C" fn overlay_present(
    _: Handle,
    queue: Handle,
    _: Handle,
    stats: *const Stats,
    intent: *mut Intent,
    _: LogFn,
    _: Handle,
) -> i32 {
    event(format!("overlay {}", object(queue).name));
    with(|world| {
        world.stats = Some(unsafe { *stats });
        if let Some(requested) = world.intent.take() {
            unsafe { intent.write(requested) };
        }
    });
    1
}
pub(super) unsafe extern "C" fn overlay_fill_fg(stats: *mut Stats, _: *const FgStatus) {
    unsafe { (*stats).fg_backend = 3 };
}
pub(super) unsafe extern "C" fn observer_create(
    setup: *const ObserverSetup,
    out: *mut Handle,
) -> i32 {
    assert_eq!(
        unsafe { (*setup).struct_size } as usize,
        size_of::<ObserverSetup>()
    );
    event("observer".into());
    unsafe { out.write(0x7000 as Handle) };
    with(|world| world.observer_result)
}
pub(super) extern "C" fn observer_attach(observer: Handle, object: Handle) -> i32 {
    event(format!("attach {} {}", observer as usize, object as usize));
    OK
}
pub(super) extern "C" fn diagnostics_enable(_: LogFn, _: Handle) -> i32 {
    event("diagnostics".into());
    OK
}

// The host calls these fakes by the production leaf names.
pub(super) use self::{
    observer_attach as observer_device, observer_attach as observer_factory,
    registration as registration_create, registration as registration_borrow,
};
unsafe extern "C" fn record_log(_: *mut c_void, message: *const c_char) {
    let text = unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy();
    event(format!("log {text}"));
}

struct Temporary {
    ini: crate::temporary_ini::TemporaryIni,
    directory: Vec<u16>,
}

impl Temporary {
    fn new(name: &str) -> Self {
        let ini = crate::temporary_ini::TemporaryIni::new("render-host", name);
        let directory = ini.directory_wide();
        Self { ini, directory }
    }

    fn write(&self, key: &str, value: &str) {
        use crate::settings_store::wide;
        let section = wide("Adapter");
        assert_ne!(
            unsafe {
                WritePrivateProfileStringW(
                    section.as_ptr(),
                    wide(key).as_ptr(),
                    wide(value).as_ptr(),
                    self.ini.wide().as_ptr(),
                )
            },
            0
        );
    }
    fn integer(&self, key: &str) -> u32 {
        use crate::settings_store::wide;
        unsafe {
            GetPrivateProfileIntW(
                wide("Adapter").as_ptr(),
                wide(key).as_ptr(),
                -1,
                self.ini.wide().as_ptr(),
            ) as u32
        }
    }
    fn contents(&self) -> String {
        self.ini.contents().unwrap().expect("settings file exists")
    }
}

const SECTION: &[u16] = &[65, 100, 97, 112, 116, 101, 114, 0]; // "Adapter"
const SESSION: u64 = 7;

fn setup(ini: &Temporary, owner: u32) -> Setup {
    Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: HOST_ABI_VERSION,
        configuration_path: ini.ini.wide().as_ptr(),
        runtime_directory: ini.directory.as_ptr(),
        configuration_section: SECTION.as_ptr(),
        presentation_owner: owner,
        sr_backend_choices: rsf_game_sys::RSF_SR_CHOICES_ALL,
        engine_type: 2,
        application_id: 9,
        engine_version_utf8: c"Fixture".as_ptr(),
        project_id_utf8: null(),
        depth_inverted: 1,
        units_to_meters: 1.0,
        ui_mode: 0,
        accept_window: None,
        user: null_mut(),
    }
}

fn services() -> Services {
    let mut services: Services = unsafe { std::mem::zeroed() };
    services.struct_size = size_of::<Services>() as u32;
    services.abi_version = RSF_GAME_ABI_VERSION;
    services.session_id = SESSION;
    services.log = Some(record_log);
    services
}

// A host installed with the recording platform but not published.
fn host(ini: &Temporary, owner: u32) -> Box<Host> {
    let setup = setup(ini, owner);
    let host = Box::new(unsafe { describe(&setup, &services()) });
    assert_eq!(install(&host, &setup), OK);
    host
}

fn pass(native_frame: u64, render: (u32, u32), output: (u32, u32)) -> Pass {
    let mut pass: Pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<Pass>() as u32;
    pass.session_id = SESSION;
    pass.camera_valid = 1;
    pass.native_frame = native_frame;
    pass.camera.render_width = render.0;
    pass.camera.render_height = render.1;
    pass.camera.output_width = output.0;
    pass.camera.output_height = output.1;
    pass
}

#[test]
fn admission_refuses_short_and_mismatched_requests() {
    let ini = Temporary::new("admission");
    let mut services = services();
    let valid = setup(&ini, 0);
    let refuse = |setup: &Setup, services: &mut Services| unsafe { create(setup, services) };
    assert_eq!(unsafe { create(null(), &mut services) }, INVALID);
    let short = Setup {
        struct_size: size_of::<Setup>() as u32 - 8,
        ..setup(&ini, 0)
    };
    assert_eq!(refuse(&short, &mut services), INVALID);
    let empty_section = [0u16];
    for broken in [
        Setup {
            configuration_section: empty_section.as_ptr(),
            ..setup(&ini, 0)
        },
        Setup {
            engine_version_utf8: null(),
            ..setup(&ini, 0)
        },
        Setup {
            presentation_owner: 2,
            ..setup(&ini, 0)
        },
        Setup {
            sr_backend_choices: 0,
            ..setup(&ini, 0)
        },
    ] {
        assert_eq!(refuse(&broken, &mut services), INVALID);
    }
    let mut short_services = Services {
        struct_size: 8,
        ..services
    };
    assert_eq!(refuse(&valid, &mut short_services), INVALID);
    let mut no_session = Services {
        session_id: 0,
        ..services
    };
    assert_eq!(refuse(&valid, &mut no_session), INVALID);
    let version = Setup {
        abi_version: 2,
        ..setup(&ini, 0)
    };
    assert_eq!(refuse(&version, &mut services), ABI_MISMATCH);
    let mut old = Services {
        abi_version: RSF_GAME_ABI_VERSION - 1,
        ..services
    };
    assert_eq!(refuse(&valid, &mut old), ABI_MISMATCH);
    assert!(services.process_upscale.is_none());
    assert!(events().is_empty());
}

#[test]
fn creation_refuses_busy_before_installing_and_releases_admission_after_refusal() {
    let ini = Temporary::new("creation-admission");
    let setup = setup(&ini, ENGINE_PRESENTATION);
    let mut services = services();
    with(|world| world.observer_result = -1);
    {
        let _installation = CREATION.lock().unwrap();
        assert_eq!(unsafe { create(&setup, &mut services) }, BUSY);
        assert!(events().is_empty());
        assert!(services.user.is_null() && services.process_upscale.is_none());
    }
    for _ in 0..2 {
        assert_eq!(unsafe { create(&setup, &mut services) }, NOT_READY);
        assert_eq!(events(), ["observer"]);
        assert!(services.user.is_null() && services.process_upscale.is_none());
    }
}

#[test]
fn diagnostics_save_live_capture_and_mark_graphics_for_restart() {
    let ini = Temporary::new("diagnostic-options");
    let host = host(&ini, 0);
    events();
    let set = |index: usize, value| {
        host.diagnostic_intent(&Intent {
            host_option_changed: 1,
            host_option_index: index as u32,
            host_option_value: value,
            ..Intent::default()
        });
    };
    let active_directory = r"C:\試験🚀";
    assert_eq!(host.set_capture_directory(active_directory.into()), OK);
    set(GRAPHICS_DIAGNOSTICS, 1);
    let state = host.lock();
    let [graphics] = host.diagnostic_options(state.graphics_diagnostics);
    assert_eq!((graphics.value, graphics.restart), (1, 1));
    assert!(state.capture_update);
    drop(state);
    assert_eq!(ini.integer("GraphicsDiagnostics"), 1);
    let log = events();
    assert!(!log.iter().any(|event| event == "diagnostics"));
    assert!(log.contains(&format!("fg capture {active_directory}")));

    host.lock().capture_update = false;
    with(|world| world.fg_capture_result = BACKEND_NOT_READY);
    assert_eq!(host.set_capture_directory(String::new()), BACKEND_NOT_READY);
    let state = host.lock();
    assert_eq!(state.capture_directory.to_str().unwrap(), active_directory);
    assert!(state.capture_enabled);
    assert!(!state.capture_update);
    drop(state);

    with(|world| world.fg_capture_result = BACKEND_OK);
    assert_eq!(host.set_capture_directory(String::new()), OK);
    assert!(!host.lock().capture_enabled);
    assert!(host.lock().capture_update);
    assert_eq!(events(), ["fg capture ", "fg capture "]);
}

#[test]
fn quality_follows_the_actual_input_ratio() {
    assert_eq!(infer_quality(1920, 1920), 0);
    assert_eq!(infer_quality(1280, 1920), 1);
    assert_eq!(infer_quality(1114, 1920), 2);
    assert_eq!(infer_quality(960, 1920), 3);
    assert_eq!(infer_quality(640, 1920), 4);
    assert_eq!(infer_quality(0, 1920), 0);
}

#[test]
fn install_reads_one_shot_requests_and_owns_fg_or_observer() {
    let ini = Temporary::new("install");
    ini.write("GraphicsDiagnostics", "1");
    ini.write("SRCaptureDirectory", "C:\\captures");
    let installed = host(&ini, 0);
    let log = events();
    assert!(log.contains(&"diagnostics".to_string()));
    assert!(log.contains(&"fg install mode=0 switching=1".to_string()));
    assert!(has_event(&log, "log Shared FG presentation installed"));
    assert!(installed.fg_installed());
    assert_eq!(
        installed.lock().capture_directory.to_str().unwrap(),
        "C:\\captures"
    );
    let vendors = &installed.vendors;
    for fsr in [&vendors.fsr2, &vendors.fsr3, &vendors.fsr4] {
        assert!(fsr.to_str().unwrap().ends_with("/fidelityfx"));
    }

    let engine = host(&ini, ENGINE_PRESENTATION);
    let log = events();
    assert!(log.contains(&"observer".to_string()) && !has_event(&log, "fg install"));
    assert!(!engine.fg_installed() && engine.observer.load(Ordering::Relaxed) as usize == 0x7000);
    with(|world| world.observer_result = -1);
    let refused = setup(&ini, ENGINE_PRESENTATION);
    let refused_host = unsafe { describe(&refused, &services()) };
    assert_eq!(install(&refused_host, &refused), NOT_READY);
    with(|world| world.observer_result = 0);
}

#[test]
fn process_resolves_auto_configures_and_reserves_slots() {
    let ini = Temporary::new("process");
    let host = host(&ini, 0);
    events();
    let device = fake("device", null_mut());
    let list = fake("list", device);
    let mut first = pass(10, (960, 540), (1920, 1080));
    // Missing camera, foreign session and short passes are refused before any state changes.
    first.camera_valid = 0;
    assert_eq!(host.process(list, &first), INVALID);
    first.camera_valid = 1;
    first.session_id = 8;
    assert_eq!(host.process(list, &first), INVALID);
    first.session_id = SESSION;
    assert_eq!(host.process(null_mut(), &first), INVALID);
    assert!(events().is_empty());

    assert_eq!(host.process(list, &first), BACKEND_OK);
    let log = events();
    assert!(log.contains(&"fence".to_string()));
    assert!(has_event(&log, "log Auto SR selected backend 1"));
    assert!(log.contains(&"registration".to_string()));
    assert!(log.contains(
        &"create backend=1 quality=3 960x540->1920x1080 exposure=0 registration=1".to_string()
    ));
    assert!(log.contains(&"evaluate 1 slot=0 reset=1".to_string()));
    assert_eq!(host.automatic_backend.load(Ordering::Relaxed), RSF_SR_DLSS);
    // One reference held by the host, one by the fake's creator.
    assert_eq!(refs(device), 2);

    // The same inputs reuse the context and continue history in the next free slot.
    let second = pass(11, (960, 540), (1920, 1080));
    assert_eq!(host.process(list, &second), BACKEND_OK);
    assert_eq!(events(), ["evaluate 1 slot=1 reset=0"]);
    // A changed extent cannot drain while recordings are unsubmitted.
    let resized = pass(12, (1280, 720), (1920, 1080));
    assert_eq!(host.process(list, &resized), BUSY);
    // A list from another device is refused.
    let other = fake("other", null_mut());
    let foreign = fake("foreign list", other);
    assert_eq!(host.process(foreign, &second), NATIVE_REFUSED);
    assert_eq!(refs(other), 1);
    // A refused evaluation resets the next one.
    with(|world| world.evaluate_result = -2);
    assert_eq!(host.process(list, &pass(13, (960, 540), (1920, 1080))), -2);
    with(|world| world.evaluate_result = 0);
    events();
    // Three unsubmitted slots: the producer is never blocked.
    assert_eq!(
        host.process(list, &pass(14, (960, 540), (1920, 1080))),
        BUSY
    );
    assert!(host.lock().reset);
}

#[test]
fn submission_retires_snapshotted_slots_on_one_direct_queue() {
    let ini = Temporary::new("submit");
    let host = host(&ini, 0);
    let device = fake("device", null_mut());
    let list = fake("list", device);
    let queue = fake("direct queue", device);
    let mut frame = pass(20, (960, 540), (1920, 1080));
    frame.color_input = 0x10 as Handle;
    frame.scene_surface = 0x20 as Handle;
    frame.source_frame_id = 99;
    assert_eq!(host.process(list, &frame), BACKEND_OK);
    frame.role = RSF_GAME_RENDER_FINAL_SCENE;
    unsafe { render_pass(host.user(), list, &frame, 0) };
    events();
    let lists = [list];
    host.submit(queue, 5, 1, lists.as_ptr(), true);
    // An unrelated recording is not tracked.
    host.submit(queue, 6, 0, null(), true);
    host.submit(queue, 6, 0, null(), false);
    assert!(events().is_empty());
    host.submit(queue, 5, 1, lists.as_ptr(), false);
    assert_eq!(
        events(),
        [
            "signal direct queue 1",
            &format!("fg submitted {SESSION} 99")
        ]
    );
    {
        let s = host.lock();
        assert!(!s.slots[0].pending && s.slots[0].fence_value == 1);
        assert!(s.scenes[20].submitted);
    }
    assert!(host.observed_commands[0].load(Ordering::Relaxed).is_null());

    // The matching Present reaches FG; a different surface does not.
    frame.role = RSF_GAME_RENDER_WINDOW;
    unsafe { render_pass(host.user(), null_mut(), &frame, 1) };
    assert!(events().contains(&"fg window 99".to_string()));
    frame.scene_surface = 0x21 as Handle;
    unsafe { render_pass(host.user(), null_mut(), &frame, 1) };
    assert!(!has_event(&events(), "fg window"));

    // A second DIRECT queue poisons the host.
    let second = pass(21, (960, 540), (1920, 1080));
    assert_eq!(host.process(list, &second), BACKEND_OK);
    let other = fake("direct other", device);
    host.submit(other, 8, 1, lists.as_ptr(), true);
    host.submit(other, 8, 1, lists.as_ptr(), false);
    assert!(host.poisoned.load(Ordering::Relaxed));
    assert!(has_event(&events(), "log Native renderer SR refused"));
    assert_eq!(host.process(list, &second), NOT_READY);
}

#[test]
fn fence_failures_poison_and_present_routes_to_the_registration() {
    let ini = Temporary::new("fence");
    let host = host(&ini, 0);
    let device = fake("device", null_mut());
    let list = fake("list", device);
    let queue = fake("direct queue", device);
    assert_eq!(
        host.process(list, &pass(1, (960, 540), (1920, 1080))),
        BACKEND_OK
    );
    let lists = [list];
    host.submit(queue, 1, 1, lists.as_ptr(), true);
    host.submit(queue, 1, 1, lists.as_ptr(), false);
    events();
    // A resize drains submitted work through the fence wait.
    assert_eq!(
        host.process(list, &pass(2, (1280, 720), (1920, 1080))),
        BACKEND_OK
    );
    let log = events();
    assert!(log.contains(&"wait 1 15000".to_string()));
    assert!(log.contains(&"destroy 1".to_string()));
    assert!(log.contains(
        &"create backend=1 quality=1 1280x720->1920x1080 exposure=0 registration=1".to_string()
    ));
    // NGX registration presents are bookkept; the test flag is not an application Present.
    unsafe { present(host.user(), null_mut(), 1, PRESENT_TEST, 0) };
    unsafe { present(host.user(), null_mut(), 1, 0, 0) };
    assert_eq!(host.lock().application_presents, 1);
    assert!(events().contains(&"registration present 0".to_string()));
    // A device-removed fence must refuse slot reuse even at the current extent.
    host.submit(queue, 2, 1, lists.as_ptr(), true);
    host.submit(queue, 2, 1, lists.as_ptr(), false);
    events();
    with(|world| world.completed = u64::MAX);
    assert_eq!(
        host.process(list, &pass(3, (1280, 720), (1920, 1080))),
        NOT_READY
    );
    assert!(!has_event(&events(), "evaluate"));
    assert!(!host.wait(&host.lock(), 5));
    assert!(host.poisoned.load(Ordering::Relaxed));
    with(|world| world.completed = 0);
}

#[test]
fn settings_apply_validates_then_commits_one_section_write() {
    let ini = Temporary::new("settings");
    ini.write("Unrelated", "kept");
    let host = host(&ini, 0);
    with(|world| world.present_owner = 1);
    events();
    let saved = host.settings_get().unwrap();
    assert_eq!(saved.sr_backend, RSF_SR_AUTO);
    assert_eq!(saved.fg_backend, RSF_FG_BACKEND_AUTO);
    assert_eq!(saved.sr_quality, 2);
    assert_eq!(saved.sharpening_sharpness, 0.4);
    let before = ini.contents();
    with(|world| world.refused_preset = Some(7));
    for change in [
        |s: &mut Settings| s.sr_enabled = 2,
        |s: &mut Settings| s.latency = 3,
        |s: &mut Settings| s.sr_quality = 5,
        |s: &mut Settings| s.generated_frames = 0,
        |s: &mut Settings| s.generated_frames = 16,
        |s: &mut Settings| s.dlss_preset = 7,
        // This host has no engine scaler to hand a spatial plan to.
        |s: &mut Settings| s.sr_backend = 6,
        |s: &mut Settings| s.sr_backend = 40,
        |s: &mut Settings| s.fg_backend = 0,
        |s: &mut Settings| s.fg_backend = 2,
        |s: &mut Settings| s.sharpening_sharpness = 9.0,
    ] {
        let mut invalid = saved;
        change(&mut invalid);
        assert_eq!(host.settings_apply(&invalid), INVALID);
    }
    assert_eq!(ini.contents(), before);
    // Busy observers neither read nor write.
    {
        let _held = host.lock();
        assert_eq!(host.settings_apply(&saved), BUSY);
        assert_eq!(host.settings_get().err(), Some(BUSY));
    }
    let mut request = saved;
    request.sr_backend = 3;
    request.fg_enabled = 1;
    request.fg_backend = 5;
    request.generated_frames = 3;
    request.latency = 2;
    request.frame_limit_us = 8333;
    request.sharpening_sharpness = 0.75;
    with(|world| world.now = 10);
    assert_eq!(host.settings_apply(&request), OK);
    let log = events();
    assert!(log.contains(&"fg options mode=1 generated=3 reflex=2 limit=8333".to_string()));
    assert!(log.contains(&"fg request 5".to_string()));
    assert!(has_event(
        &log,
        "log Native renderer FG backend request=5 result=0."
    ));
    assert!(ini.contents().contains("Unrelated=kept"));
    assert_eq!(ini.integer("SRBackend"), 3);
    let s = host.lock();
    assert_eq!((s.requested_backend, s.backend), (3, 3));
    assert_eq!(s.sharpening.sharpness, 0.75);
    assert_eq!(s.fg_backend_requested, 5);
    assert_eq!(s.poll_tick, 1010);
    drop(s);
    assert_eq!(host.settings_get().unwrap().generated_frames, 3);
    with(|world| world.present_owner = 0);
    // Auto for both providers is a valid saved choice.
    let automatic = Settings {
        sr_backend: RSF_SR_AUTO,
        fg_backend: RSF_FG_BACKEND_AUTO,
        ..request
    };
    assert_eq!(host.settings_apply(&automatic), OK);
}

#[test]
fn polling_passes_the_stored_fg_mode_and_keeps_the_public_boolean() {
    let ini = Temporary::new("fg-mode");
    let host = host(&ini, 0);
    with(|world| world.fg_status = Some(Host::empty_fg()));
    for (stored, mode) in [("2", 2), ("3", 3), ("1", 1), ("0", 0)] {
        ini.write("FG", stored);
        events();
        {
            let mut s = host.lock();
            s.poll_tick = 0;
            host.poll(&mut s);
        }
        let expected = format!("fg options mode={mode} generated=1 reflex=1 limit=0");
        assert!(events().contains(&expected), "FG={stored}");
        // The public settings and status keep the boolean.
        let enabled = u32::from(mode != 0);
        assert_eq!(
            host.settings_get().unwrap().fg_enabled,
            enabled,
            "FG={stored}"
        );
        assert_eq!(
            host.status_get().unwrap().fg_requested,
            enabled,
            "FG={stored}"
        );
        let mut update = host.settings_get().unwrap();
        update.status_overlay ^= 1;
        assert_eq!(host.settings_apply(&update), OK);
        assert_eq!(ini.integer("FG"), mode);
    }
    // A section without FG keeps FG12 Off.
    let fresh = Temporary::new("fg-mode-missing");
    let host = self::host(&fresh, 0);
    events();
    host.poll(&mut host.lock());
    assert!(events().contains(&"fg options mode=0 generated=1 reflex=1 limit=0".to_string()));
}

#[test]
fn status_reports_renderer_refusals_and_rates() {
    let ini = Temporary::new("status");
    // The host reads its settings at install and on poll.
    ini.write("FG", "1");
    ini.write("FGBackend", "3");
    let host = host(&ini, 0);
    // An installed FG owner that cannot report is busy.
    with(|world| world.fg_status = None);
    assert_eq!(host.status_get().err(), Some(BUSY));
    let mut fg = Host::empty_fg();
    fg.available = 1;
    fg.vendor.valid_statistics = RSF_FG_STAT_TOTAL_PRESENTED;
    fg.vendor.configured_mode = RSF_FG_FIXED;
    with(|world| {
        world.fg_status = Some(fg);
        world.now = 1000;
    });
    let text = |status: &Status| {
        unsafe { std::ffi::CStr::from_ptr(status.reason.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    };
    let status = host.status_get().unwrap();
    assert_eq!(status.fg_requested, 1);
    assert_eq!(status.fg_requested_backend, 3);
    assert_eq!(text(&status), "SDK reports inactive");
    let feedback = Feedback {
        struct_size: size_of::<Feedback>() as u32,
        reason: 5,
        session_id: SESSION,
        native_frame: 123,
    };
    unsafe { render_feedback(host.user(), &feedback) };
    let status = host.status_get().unwrap();
    assert_eq!(status.native_frame, 123);
    assert_eq!(text(&status), "Invalid camera or frozen jitter");
    // Feedback from another session is ignored; a stale refusal no longer names the renderer.
    let foreign = Feedback {
        session_id: 8,
        native_frame: 5,
        ..feedback
    };
    unsafe { render_feedback(host.user(), &foreign) };
    with(|world| world.now = 3000);
    host.lock().application_presents = 120;
    fg.vendor.total_presented = 240;
    with(|world| world.fg_status = Some(fg));
    let status = host.status_get().unwrap();
    assert_eq!(status.native_frame, 123);
    assert_eq!(text(&status), "SDK reports inactive");
    assert_eq!((status.base_fps, status.output_fps), (60.0, 120.0));
    assert_eq!(status.output_fps_valid, 1);
    ini.write("FG", "0");
    assert_ne!(host.status_get().unwrap().fg_requested, 0);
    with(|world| world.now += 1000);
    host.poll(&mut host.lock());
    let status = host.status_get().unwrap();
    assert_eq!(text(&status), "Disabled by setting");
    assert_eq!(status.fg_requested_backend, 0);
    host.poisoned.store(true, Ordering::Relaxed);
    ini.write("FG", "1");
    with(|world| world.now += 1000);
    host.poll(&mut host.lock());
    assert_eq!(
        text(&host.status_get().unwrap()),
        "GPU submission or presentation failed"
    );
}

#[test]
fn the_panel_offers_the_common_set_on_this_gpu_and_its_packages() {
    use crate::sr_choices::{COMMON, Packages};
    let packages = Packages::all("render-host");
    let mut ini = Temporary::new("choices");
    ini.directory = crate::settings_store::wide(packages.0.to_str().unwrap());
    let host = host(&ini, 0);
    with(|world| world.fg_status = Some(Host::empty_fg()));
    let device = fake("device", null_mut());
    let queue = fake("direct queue", device);
    host.submit(queue, 1, 0, null(), true);
    let shown = |vendor: u32| {
        with(|world| world.vendor = vendor);
        unsafe { present(host.user(), null_mut(), 1, 0, 1) };
        with(|world| world.stats.take()).unwrap()
    };
    // Spatial is the engine's own scaler, which this host cannot hand a plan to.
    let nvidia = shown(0x10de);
    assert_eq!(nvidia.sr_backend_choices, COMMON);
    assert_eq!(
        shown(0x1002).sr_backend_choices,
        COMMON & !(1 << RSF_SR_DLSS)
    );
    // Every provider and Auto are offered for generation, and Ultra Quality only with XeSS.
    assert_eq!(
        nvidia.fg_backend_choices & rsf_game_sys::RSF_FG_CHOICES_ALL,
        rsf_game_sys::RSF_FG_CHOICES_ALL & (present_backend_choices() | (1 << 7))
    );
    assert_eq!(
        nvidia.sr_quality_choices,
        crate::sr_choices::sr_quality(RSF_SR_AUTO)
    );
}

#[test]
fn overlay_routes_the_panel_through_one_settings_commit() {
    let ini = Temporary::new("overlay");
    ini.write("FG", "1");
    ini.write("FGBackend", "7");
    let host = host(&ini, 0);
    with(|world| world.fg_status = Some(Host::empty_fg()));
    // No queue yet: nothing is drawn.
    unsafe { present(host.user(), null_mut(), 1, 0, 1) };
    assert!(!has_event(&events(), "overlay"));
    let device = fake("device", null_mut());
    let queue = fake("direct queue", device);
    host.submit(queue, 1, 0, null(), true);
    host.automatic_backend.store(RSF_SR_XESS, Ordering::Relaxed);
    let mut intent = Intent {
        struct_size: size_of::<Intent>() as u32,
        ..Intent::default()
    };
    intent.fg_backend_changed = 1;
    intent.fg_backend = 0;
    intent.reflex_changed = 1;
    intent.reflex_mode = 2;
    intent.ui_preferences_changed = 1;
    intent.ui_preferences = 6;
    with(|world| world.intent = Some(intent));
    unsafe { present(host.user(), null_mut(), 1, 0, 1) };
    assert!(events().contains(&"overlay direct queue".to_string()));
    let stats = with(|world| world.stats.take()).unwrap();
    // SR Auto stays the selected choice and is named by the provider it resolved to. FG Auto
    // stays the selected panel choice, which the panel marks without a pending change beside the
    // provider the bridge reports.
    assert_eq!(
        (stats.backend, stats.requested_backend),
        (RSF_SR_AUTO, RSF_SR_AUTO)
    );
    assert_eq!(
        unsafe { std::ffi::CStr::from_ptr(stats.backend_name) },
        c"XeSS"
    );
    assert_eq!(stats.fg_requested_backend, RSF_FG_BACKEND_AUTO);
    assert_eq!(stats.fg_requested_mode, RSF_FG_FIXED);
    assert_eq!(stats.sharpening_available, 1);
    assert_ne!(stats.fg_backend_choices & RSF_OVERLAY_FG_RUNTIME_SWITCH, 0);
    assert_eq!(ini.integer("FG"), 0);
    assert_eq!(ini.integer("FGBackend"), 7);
    assert_eq!(ini.integer("Latency"), 2);
    assert_eq!(ini.integer("OverlayLayout"), 6);
    assert_eq!(host.ui_preferences.load(Ordering::Relaxed), 6);
    // While FG is off the panel selects Off.
    unsafe { present(host.user(), null_mut(), 1, 0, 1) };
    let stats = with(|world| world.stats.take()).unwrap();
    assert_eq!(stats.fg_requested_backend, 0);
    assert_eq!(stats.fg_requested_mode, RSF_FG_OFF);
    assert_eq!(stats.fg_backend, 0);
    // A refused request leaves the saved settings and is reported.
    let mut refused = intent;
    refused.ui_preferences_changed = 0;
    refused.fg_backend_changed = 0;
    refused.reflex_changed = 0;
    refused.quality_changed = 1;
    refused.quality = 9;
    with(|world| world.intent = Some(refused));
    unsafe { present(host.user(), null_mut(), 1, 0, 1) };
    assert!(events().contains(
        &"log RSF overlay settings request refused; saved settings retained.".to_string()
    ));
    assert_eq!(ini.integer("SRQuality"), 2);
    assert_eq!(refs(queue), 2);
}

#[test]
fn latency_and_observer_submissions_reach_their_owners() {
    let ini = Temporary::new("latency");
    let host = host(&ini, 0);
    events();
    let event = LatencyEvent {
        struct_size: size_of::<LatencyEvent>() as u32,
        marker: 3,
        session_id: SESSION,
        source_frame_id: 4,
        timestamp_qpc: 0,
        qpc_frequency: 0,
    };
    assert_eq!(unsafe { process_latency(host.user(), &event) }, BACKEND_OK);
    assert!(has_event(
        &events(),
        "log Native renderer latency boundary=3 source=4"
    ));
    let foreign = LatencyEvent {
        session_id: 9,
        ..event
    };
    assert_eq!(unsafe { process_latency(host.user(), &foreign) }, INVALID);
    unsafe { cpu_event(host.user(), null()) };
    assert_eq!(events(), ["fg cpu"]);
    // Observer submissions of untracked lists are ignored even with the lock held elsewhere.
    let device = fake("device", null_mut());
    let list = fake("list", device);
    let queue = fake("direct queue", device);
    let lists = [list];
    let held = host.lock();
    unsafe { observer_submit(host.user(), queue, 1, 1, lists.as_ptr(), 1) };
    drop(held);
    unsafe { observer_chain(host.user(), queue, null_mut(), null_mut()) };
    assert!(host.graphics_ready.load(Ordering::Relaxed));
    assert!(host.lock().overlay_queue.is_some());
}

#[test]
fn published_entries_refuse_foreign_users_and_short_structures() {
    let mut status: Status = Status::default();
    assert_eq!(unsafe { rsf_render_status_get(null_mut()) }, INVALID);
    status.struct_size = 8;
    assert_eq!(unsafe { rsf_render_status_get(&mut status) }, INVALID);
    let mut settings: Settings = unsafe { std::mem::zeroed() };
    settings.struct_size = size_of::<Settings>() as u32;
    settings.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION + 1;
    assert_eq!(
        unsafe { rsf_render_settings_get(&mut settings) },
        ABI_MISMATCH
    );
    assert_eq!(
        unsafe { rsf_render_settings_apply(&settings) },
        ABI_MISMATCH
    );
    let stranger = 0x1234 as Handle;
    assert_eq!(
        unsafe { rsf_render_host_graphics(stranger, stranger) },
        INVALID
    );
    assert_eq!(
        unsafe { rsf_render_host_device(stranger, stranger) },
        NOT_READY
    );
    assert_eq!(
        unsafe { rsf_render_host_factory(stranger, stranger) },
        NOT_READY
    );
    assert_eq!(rsf_render_host_graphics_ready(stranger), 0);
    assert!(unsafe { rsf_render_host_native_object(stranger, stranger) }.is_null());
}
