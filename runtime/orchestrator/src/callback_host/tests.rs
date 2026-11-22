// SPDX-License-Identifier: MIT
// Synthetic callback host contracts: a recording platform, reference-counted fake COM objects,
// a fake plugin session and temporary log and preferences files. No D3D12 device, vendor SDK,
// plugin DLL, overlay or window is involved, so these establish admission, lifecycle order,
// configuration, slot, scene, settings and overlay routing policy only.
use super::*;
use std::cell::{Cell, RefCell};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, WritePrivateProfileStringW,
};

#[allow(dead_code)]
#[repr(C)]
struct Vtable {
    query: unsafe extern "system" fn(*mut Fake, *const c_void, *mut Handle) -> i32,
    add_ref: unsafe extern "system" fn(*mut Fake) -> u32,
    release: unsafe extern "system" fn(*mut Fake) -> u32,
}

#[repr(C)]
struct Fake {
    vtable: &'static Vtable,
    refs: Cell<u32>,
    device: Handle,
}

unsafe extern "system" fn query(_: *mut Fake, _: *const c_void, out: *mut Handle) -> i32 {
    unsafe { out.write(null_mut()) };
    0x8000_4002_u32 as i32
}
unsafe extern "system" fn add_ref(this: *mut Fake) -> u32 {
    let refs = unsafe { &(*this).refs };
    refs.set(refs.get() + 1);
    refs.get()
}
unsafe extern "system" fn release(this: *mut Fake) -> u32 {
    let refs = unsafe { &(*this).refs };
    assert!(refs.get() > 0, "over-released");
    refs.set(refs.get() - 1);
    refs.get()
}
static VTABLE: Vtable = Vtable {
    query,
    add_ref,
    release,
};

fn fake(device: Handle) -> Handle {
    Box::into_raw(Box::new(Fake {
        vtable: &VTABLE,
        refs: Cell::new(1),
        device,
    }))
    .cast()
}
fn refs(handle: Handle) -> u32 {
    unsafe { &*handle.cast::<Fake>() }.refs.get()
}

struct World {
    refused_preset: Option<u32>,
    events: Vec<String>,
    now: u64,
    vendor: u32,
    adapter_ok: bool,
    policy_ok: bool,
    drained_generation: u32,
    create_result: i32,
    evaluate_result: i32,
    contexts: usize,
    plan: (u32, u32),
    presented: Option<u32>,
    present_backend: u32,
    switch_result: i32,
    request_result: i32,
    fg_status: Option<FgStatus>,
    intent: Option<Intent>,
    stats: Option<Stats>,
    drawn: i32,
    prepare_result: i32,
    prepare_handle: Handle,
    start_result: i32,
    start_commands: Handle,
    quiesce_result: i32,
    stop_result: i32,
    services: Option<Services>,
}

impl Default for World {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            now: 0,
            vendor: 0x10de,
            adapter_ok: true,
            policy_ok: true,
            refused_preset: None,
            drained_generation: 0,
            create_result: BACKEND_OK,
            evaluate_result: BACKEND_OK,
            contexts: 0,
            plan: (1280, 720),
            presented: Some(41),
            present_backend: 0,
            switch_result: 0,
            request_result: BACKEND_OK,
            fg_status: None,
            intent: None,
            stats: None,
            drawn: 1,
            prepare_result: OK,
            prepare_handle: 0x9000 as Handle,
            start_result: OK,
            start_commands: null_mut(),
            quiesce_result: OK,
            stop_result: OK,
            services: None,
        }
    }
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World::default());
}

fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}
fn reset() {
    with(|world| *world = World::default());
}
fn event(text: String) {
    with(|world| world.events.push(text));
}
fn events() -> Vec<String> {
    with(|world| std::mem::take(&mut world.events))
}

unsafe extern "C" fn tick() -> u64 {
    with(|world| world.now)
}
unsafe extern "C" fn device_of(child: Handle) -> Handle {
    let device = unsafe { &*child.cast::<Fake>() }.device;
    unsafe { add_ref(device.cast()) };
    device
}
unsafe extern "C" fn adapter(_: Handle, out: *mut Adapter) -> i32 {
    let (vendor, ok) = with(|world| (world.vendor, world.adapter_ok));
    if ok {
        unsafe {
            out.write(Adapter {
                vendor,
                device: 0x2204,
                software: 0,
            })
        };
    }
    i32::from(ok)
}
unsafe extern "C" fn last_present_count(_: Handle, out: *mut u32) -> i32 {
    match with(|world| world.presented) {
        Some(count) => {
            unsafe { out.write(count) };
            1
        }
        None => 0,
    }
}
unsafe extern "C" fn resource_desc(resource: Handle, out: *mut ResourceDesc) {
    unsafe {
        out.write(ResourceDesc {
            width: resource as u64,
            height: 2,
            format: 10,
            samples: 1,
        })
    };
}
unsafe extern "C" fn depth_domains(pass: *const Pass) -> u32 {
    unsafe { (*pass).depth_domains.domain_count }
}
unsafe extern "C" fn dlss_preset_valid(preset: u32) -> u32 {
    u32::from(with(|world| world.refused_preset != Some(preset)))
}
unsafe extern "C" fn sr12_create(setup: *const SrSetup, out: *mut Handle) -> i32 {
    let setup = unsafe { &*setup };
    let dlss = &setup.dlss;
    let text = |value: *const c_char| {
        if value.is_null() {
            "-".to_owned()
        } else {
            unsafe { CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned()
        }
    };
    event(format!(
        "create abi={} backend={} quality={} {}x{}->{}x{} depth={} preset={} signature={} interposer={} engine={} {} {} fsr={} xess={}",
        setup.abi_version,
        setup.backend,
        setup.quality,
        setup.render_width,
        setup.render_height,
        setup.output_width,
        setup.output_height,
        setup.inverted_depth,
        dlss.preset,
        dlss.require_signature,
        text(dlss.interposer_path_utf8),
        dlss.engine,
        text(dlss.engine_version_utf8),
        text(dlss.project_id_utf8),
        text(setup.fsr3_directory_utf8),
        text(setup.xess_directory_utf8),
    ));
    let result = with(|world| world.create_result);
    if result == BACKEND_OK {
        let context = with(|world| {
            world.contexts += 1;
            world.contexts
        });
        unsafe { out.write(context as Handle) };
    }
    result
}
unsafe extern "C" fn sr12_plan(_: Handle, width: *mut u32, height: *mut u32) -> i32 {
    let (w, h) = with(|world| world.plan);
    unsafe {
        width.write(w);
        height.write(h);
    }
    BACKEND_OK
}
unsafe extern "C" fn sr12_destroy(context: Handle) {
    event(format!("destroy {}", context as usize));
}
unsafe extern "C" fn sr12_evaluate(context: Handle, _: Handle, _: *const Pass, slot: u32) -> i32 {
    event(format!("evaluate {} slot={slot}", context as usize));
    with(|world| world.evaluate_result)
}
unsafe extern "C" fn sr12_set_sharpening(context: Handle, options: *const Sharpening) -> i32 {
    event(format!("sharpen {} {}", context as usize, unsafe {
        (*options).sharpness
    }));
    BACKEND_OK
}
unsafe extern "C" fn fg_cpu(event_: *const CpuEvent) {
    event(format!("fg cpu {}", unsafe { (*event_).source_frame_id }));
}
unsafe extern "C" fn fg_submitted(session: u64, frame: u64) {
    event(format!("fg submitted {session} {frame}"));
}
unsafe extern "C" fn fg_hudless(_: Handle, pass: *const Pass) {
    event(format!("fg hudless {}", unsafe { (*pass).source_frame_id }));
}
unsafe extern "C" fn fg_window(pass: *const Pass) {
    event(format!("fg window {}", unsafe { (*pass).source_frame_id }));
}
unsafe extern "C" fn fg_status(out: *mut FgStatus) -> i32 {
    match with(|world| world.fg_status) {
        Some(status) => {
            unsafe { out.write(status) };
            1
        }
        None => 0,
    }
}
unsafe extern "C" fn fg_options(options: *const FgOptions) {
    let o = unsafe { &*options };
    event(format!(
        "fg options mode={} generated={} reflex={} limit={}",
        o.mode, o.generated_frames, o.reflex_mode, o.frame_limit_us
    ));
}
unsafe extern "C" fn fg_choice_save(backend: u32) -> i32 {
    event(format!("fg save {backend}"));
    OK
}
unsafe extern "C" fn present_host() -> Handle {
    0x4000 as Handle
}
unsafe extern "C" fn present_request(backend: u32) -> i32 {
    event(format!("fg request {backend}"));
    with(|world| world.request_result)
}
unsafe extern "C" fn present_backend() -> u32 {
    with(|world| world.present_backend)
}
unsafe extern "C" fn present_switch_result() -> i32 {
    with(|world| world.switch_result)
}
unsafe extern "C" fn overlay_frame(
    _: Handle,
    _: Handle,
    _: Handle,
    stats: *const Stats,
    intent: *mut Intent,
    log: LogFn,
    user: Handle,
) -> i32 {
    assert!(log.is_some() && !user.is_null());
    event("overlay".into());
    with(|world| {
        world.stats = Some(unsafe { *stats });
        if let Some(requested) = world.intent.take() {
            unsafe { intent.write(requested) };
        }
        world.drawn
    })
}
unsafe extern "C" fn overlay_stop() {
    event("overlay stop".into());
}
unsafe extern "C" fn overlay_fill_fg(stats: *mut Stats, fg: *const FgStatus) {
    unsafe {
        (*stats).fg_backend = 3;
        (*stats).fg_requested_backend = 3;
        (*stats).fg_available = u32::from(!fg.is_null());
    }
}
unsafe extern "C" fn input_dump_cancel(session: u64) {
    event(format!("dump cancel {session}"));
}
unsafe extern "C" fn session_prepare(options: *const SessionOptions, out: *mut Handle) -> i32 {
    let options = unsafe { &*options };
    assert_eq!(options.struct_size as usize, size_of::<SessionOptions>());
    assert_eq!(options.abi_version, SESSION_ABI_VERSION);
    event("prepare".into());
    let (result, handle) = with(|world| {
        world.services = Some(options.services);
        (world.prepare_result, world.prepare_handle)
    });
    unsafe { out.write(handle) };
    result
}
unsafe extern "C" fn session_start(_: Handle) -> i32 {
    event("start".into());
    let (result, commands, services) =
        with(|world| (world.start_result, world.start_commands, world.services));
    if !commands.is_null()
        && let Some(services) = services
    {
        unsafe { (services.render_pass.unwrap())(services.user, commands, &sr((1920, 1080)), 1) };
    }
    result
}
unsafe extern "C" fn session_quiesce(_: Handle) -> i32 {
    event("quiesce".into());
    with(|world| world.quiesce_result)
}
unsafe extern "C" fn session_stop(_: Handle) -> i32 {
    event("stop".into());
    with(|world| world.stop_result)
}

static PLATFORM: Platform = Platform {
    struct_size: size_of::<Platform>() as u32,
    tick,
    list_device: device_of,
    queue_device: device_of,
    adapter,
    last_present_count,
    resource_desc,
    depth_domains,
    dlss_preset_valid,
    sr12_create,
    sr12_plan,
    sr12_destroy,
    sr12_evaluate,
    sr12_set_sharpening,
    fg_cpu,
    fg_submitted,
    fg_hudless,
    fg_window,
    fg_status,
    fg_options,
    fg_choice_save,
    present_host,
    present_request,
    present_backend,
    present_switch_result,
    overlay_frame,
    overlay_stop,
    overlay_fill_fg,
    input_dump_cancel,
    session_prepare,
    session_start,
    session_quiesce,
    session_stop,
};

unsafe extern "C" fn policy(spatial: u32, quality: u32) -> c_int {
    event(format!("policy {spatial} {quality}"));
    c_int::from(with(|world| world.policy_ok))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

struct Files {
    log: Vec<u16>,
    preferences: Vec<u16>,
    plugin: Vec<u16>,
}

impl Files {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir();
        let base = format!("rsf-callback-host-{}-{name}", std::process::id());
        let log = directory.join(format!("{base}.log"));
        let preferences = directory.join(format!("{base}.ini"));
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(&preferences);
        Self {
            log: wide(log.to_str().unwrap()),
            preferences: wide(preferences.to_str().unwrap()),
            plugin: wide("C:\\Plugins\\Adapter.dll"),
        }
    }
    fn path(text: &[u16]) -> String {
        String::from_utf16_lossy(&text[..text.len() - 1])
    }
    fn log_text(&self) -> String {
        std::fs::read_to_string(Self::path(&self.log)).unwrap_or_default()
    }
    fn write(&self, key: &str, value: &str) {
        assert_ne!(
            unsafe {
                WritePrivateProfileStringW(
                    SECTION,
                    wide(key).as_ptr(),
                    wide(value).as_ptr(),
                    self.preferences.as_ptr(),
                )
            },
            0
        );
    }
    fn integer(&self, key: &str) -> i32 {
        unsafe { GetPrivateProfileIntW(SECTION, wide(key).as_ptr(), -1, self.preferences.as_ptr()) }
    }
    fn preferences_text(&self) -> String {
        std::fs::read(Self::path(&self.preferences))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(Self::path(&self.log));
        let _ = std::fs::remove_file(Self::path(&self.preferences));
    }
}

const PROBE: Probe = Probe {
    struct_size: size_of::<Probe>() as u32,
    pe_machine: 0x8664,
    reserved: 0,
    executable_name_utf8: c"Game.exe".as_ptr(),
    sha256_hex: c"00".as_ptr(),
};

fn setup(files: &Files, backend: u32, quality: u32) -> Setup {
    Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: HOST_ABI_VERSION,
        plugin_path: files.plugin.as_ptr(),
        log_path: files.log.as_ptr(),
        preferences_path: files.preferences.as_ptr(),
        probe: &PROBE,
        backend,
        quality,
        dlss_preset: 6,
        sr_backend_choices: (1 << 1) | (1 << 3) | (1 << 5) | (1 << 6),
        engine_type: 4,
        engine_version_utf8: c"6000.0".as_ptr(),
        project_id_utf8: c"project".as_ptr(),
        streamline_directory_utf8: c"C:\\SL".as_ptr(),
        fsr2_directory_utf8: null(),
        fsr3_directory_utf8: c"C:\\FSR".as_ptr(),
        fsr4_directory_utf8: null(),
        xess_directory_utf8: c"C:\\XeSS".as_ptr(),
        set_engine_spatial: Some(policy),
    }
}

fn code(result: Result<Host, u32>) -> u32 {
    result.err().unwrap_or(0)
}

unsafe extern "C" fn request_policy(spatial: u32, quality: u32, out: *mut u32) -> c_int {
    unsafe { out.write(42) };
    unsafe { policy(spatial, quality) }
}

unsafe extern "C" fn drained_generation(out: *mut u32) -> c_int {
    unsafe { out.write(with(|world| world.drained_generation)) };
    1
}

unsafe extern "C" fn spatial_extent(
    quality: u32,
    _: u32,
    _: u32,
    width: *mut u32,
    height: *mut u32,
) -> c_int {
    unsafe {
        width.write(900 + quality);
        height.write(500 + quality);
    }
    1
}

fn setup_v2(files: &Files, backend: u32, quality: u32) -> SetupV2 {
    let mut base = setup(files, backend, quality);
    base.struct_size = size_of::<SetupV2>() as u32;
    base.abi_version = RSF_CALLBACK_HOST_ABI_VERSION_V2;
    SetupV2 {
        base,
        request_policy: Some(request_policy),
        drained_generation: Some(drained_generation),
        spatial_extent: Some(spatial_extent),
        spatial_name_utf8: c"Fixture spatial".as_ptr(),
    }
}

fn host(files: &Files, backend: u32, quality: u32) -> Box<Host> {
    reset();
    let setup = setup(files, backend, quality);
    let host = Box::new(unsafe { describe(&setup, &PLATFORM) }.ok().unwrap());
    events();
    host
}

const SESSION: u64 = 11;

fn pass(role: u32, output: (u32, u32), render: (u32, u32)) -> Pass {
    let mut pass: Pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<Pass>() as u32;
    pass.role = role;
    pass.session_id = SESSION;
    pass.source_frame_id = 5;
    pass.view_key = 3;
    pass.native_frame = 9;
    pass.camera_valid = 1;
    pass.camera.output_width = output.0;
    pass.camera.output_height = output.1;
    pass.camera.render_width = render.0;
    pass.camera.render_height = render.1;
    pass.camera.depth_inverted = 1;
    pass.camera.jitter_pixels = [0.25, -0.125];
    pass.swapchain = 0x6000 as Handle;
    pass
}

fn sr(output: (u32, u32)) -> Pass {
    pass(RSF_GAME_RENDER_SR, output, (1280, 720))
}

fn run(host: &Host, commands: Handle, pass: &Pass, begin: bool) {
    unsafe { render(host.user(), commands, pass, u32::from(begin)) };
}

fn config(host: &Host) -> Config {
    let mut output = Config {
        struct_size: size_of::<Config>() as u32,
        ..empty_plan()
    };
    assert_eq!(unsafe { configuration(host.user(), &mut output) }, 1);
    output
}

// Start and stop share the one published host.
static LIFECYCLE_TESTS: Mutex<()> = Mutex::new(());

#[test]
fn admission_refuses_in_the_native_order() {
    reset();
    let files = Files::new("admission");
    let describe = |setup: &Setup| code(unsafe { describe(setup, &PLATFORM) });
    assert_eq!(code(unsafe { super::describe(null(), &PLATFORM) }), 1);
    let short = Setup {
        struct_size: size_of::<Setup>() as u32 - 8,
        ..setup(&files, 1, 1)
    };
    assert_eq!(describe(&short), 1);
    let refused = [
        Setup {
            abi_version: 2,
            ..setup(&files, 1, 1)
        },
        Setup {
            plugin_path: null(),
            ..setup(&files, 1, 1)
        },
        Setup {
            log_path: null(),
            ..setup(&files, 1, 1)
        },
        Setup {
            preferences_path: null(),
            ..setup(&files, 1, 1)
        },
        Setup {
            probe: null(),
            ..setup(&files, 1, 1)
        },
        Setup {
            engine_version_utf8: null(),
            ..setup(&files, 1, 1)
        },
        Setup {
            project_id_utf8: null(),
            ..setup(&files, 1, 1)
        },
        Setup {
            set_engine_spatial: None,
            ..setup(&files, 1, 1)
        },
        Setup {
            sr_backend_choices: 0,
            ..setup(&files, 1, 1)
        },
    ];
    for setup in &refused {
        assert_eq!(describe(setup), 1);
    }
    with(|world| world.refused_preset = Some(7));
    for (backend, quality, preset) in [(1, 1, 7), (8, 1, 0), (3, 6, 0), (1, 5, 0)] {
        let invalid = Setup {
            dlss_preset: preset,
            ..setup(&files, backend, quality)
        };
        assert_eq!(describe(&invalid), 2, "{backend} {quality} {preset}");
    }
    assert!(
        events().is_empty(),
        "nothing is consulted before validation"
    );
    assert_eq!(describe(&setup(&files, SR_SPATIAL, 5)), 0);
    assert_eq!(events(), ["policy 1 5"]);
    let missing = wide("Z:\\rsf-missing-directory\\host.log");
    let unwritable = Setup {
        log_path: missing.as_ptr(),
        ..setup(&files, 1, 1)
    };
    assert_eq!(describe(&unwritable), 4);
    assert!(events().is_empty());
    with(|world| world.policy_ok = false);
    // Auto asks the adapter about native rendering until the adapter is known.
    assert_eq!(describe(&setup(&files, SR_AUTO, 2)), 8);
    assert_eq!(events(), ["policy 0 2"]);
}

#[test]
fn start_publishes_services_and_stop_retires_in_order() {
    let _serial = LIFECYCLE_TESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let files = Files::new("lifecycle");
    reset();
    with(|world| world.now = 1000);
    let setup = setup(&files, 3, 1);
    assert_eq!(unsafe { start(&setup, &PLATFORM) }, 0);
    assert_eq!(events(), ["policy 0 1", "prepare", "start"]);
    let services = with(|world| world.services).unwrap();
    assert_eq!(services.struct_size as usize, size_of::<Services>());
    assert_eq!(services.abi_version, RSF_GAME_ABI_VERSION);
    assert_eq!(services.session_id, 1001);
    assert!(!services.user.is_null());
    assert!(services.log.is_some() && services.render_config.is_some());
    assert!(services.render_pass.is_some() && services.cpu_event.is_some());
    assert!(services.process_upscale.is_none() && services.present.is_none());
    assert_eq!(unsafe { start(&setup, &PLATFORM) }, 1);
    assert!(events().is_empty());

    // The plugin drives the host only through the services it was given.
    let device = fake(null_mut());
    let list = fake(device);
    let pass = sr((1920, 1080));
    unsafe { (services.render_pass.unwrap())(services.user, list, &pass, 1) };
    let mut cpu: CpuEvent = unsafe { std::mem::zeroed() };
    cpu.source_frame_id = 77;
    unsafe { (services.cpu_event.unwrap())(services.user, &cpu) };
    let mut plan = Config {
        struct_size: size_of::<Config>() as u32,
        ..empty_plan()
    };
    assert_eq!(
        unsafe { (services.render_config.unwrap())(services.user, &mut plan) },
        1
    );
    assert_eq!((plan.enabled, plan.render_width), (1, 1280));
    let short = &mut plan as *mut Config;
    unsafe { short.cast::<u32>().write(8) };
    assert_eq!(
        unsafe { (services.render_config.unwrap())(services.user, short) },
        0
    );
    events();

    assert_eq!(stop(), 0);
    assert_eq!(
        events(),
        [
            "quiesce",
            "stop",
            "overlay stop",
            "destroy 1",
            "dump cancel 0"
        ]
    );
    assert_eq!(stop(), 0);
    assert!(events().is_empty());
    assert_eq!(refs(device), 1);
    let log = files.log_text();
    assert!(
        log.contains("Adapter lifecycle activated; awaiting actual D3D12 render callback.\r\n")
    );
    assert!(log.contains("Native D3D12 provider 3 preparation result 0, 1280x720 -> 1920x1080"));
    assert!(log.ends_with("Native renderer SR stopped after managed/native/GPU retirement.\r\n"));
}

#[test]
fn refused_plugins_are_stopped_or_kept_for_retry() {
    let _serial = LIFECYCLE_TESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let files = Files::new("refused");
    let setup = setup(&files, 1, 1);

    reset();
    with(|world| {
        world.prepare_result = -3;
        world.prepare_handle = null_mut();
    });
    assert_eq!(unsafe { start(&setup, &PLATFORM) }, 10);
    assert_eq!(events(), ["policy 0 1", "prepare"]);
    assert!(
        files
            .log_text()
            .contains("Native renderer plugin preparation/activation refused.")
    );

    let device = fake(null_mut());
    let commands = fake(device);
    with(|world| {
        world.prepare_result = OK;
        world.prepare_handle = 0x9000 as Handle;
        world.start_result = -5;
        world.start_commands = commands;
    });
    assert_eq!(unsafe { start(&setup, &PLATFORM) }, 10);
    let trace = events();
    let names: Vec<_> = trace.iter().map(String::as_str).collect();
    assert!(names.starts_with(&["policy 0 1", "prepare", "start"]));
    assert!(names.ends_with(&["quiesce", "stop", "destroy 1"]));

    // A busy prepare returns a live session; while it cannot stop, the host stays published.
    with(|world| {
        world.prepare_result = -4;
        world.stop_result = -4;
    });
    assert_eq!(unsafe { start(&setup, &PLATFORM) }, 9);
    assert_eq!(events(), ["policy 0 1", "prepare", "quiesce", "stop"]);
    assert_eq!(unsafe { start(&setup, &PLATFORM) }, 1);
    with(|world| world.quiesce_result = -4);
    assert_eq!(stop(), 1);
    assert_eq!(events(), ["quiesce"]);
    with(|world| world.quiesce_result = OK);
    assert_eq!(stop(), 1);
    assert_eq!(events(), ["quiesce", "stop"]);
    with(|world| world.stop_result = OK);
    assert_eq!(stop(), 0);
    assert_eq!(
        events(),
        ["quiesce", "stop", "overlay stop", "dump cancel 0"]
    );
}

#[test]
fn auto_resolves_from_the_owning_adapter_and_prepares_the_vendor_context() {
    let files = Files::new("auto");
    for (vendor, backend) in [(0x10de, 1), (0x8086, 5), (0x1002, 3)] {
        let host = host(&files, SR_AUTO, 2);
        with(|world| world.vendor = vendor);
        let device = fake(null_mut());
        let list = fake(device);
        assert_eq!(config(&host).enabled, 0);
        run(&host, list, &sr((1920, 1080)), true);
        assert_eq!(
            events(),
            [
                "policy 0 2".to_owned(),
                format!(
                    "create abi=1 backend={backend} quality=2 0x0->1920x1080 depth=1 preset=6 signature=1 interposer=C:\\SL\\sl.interposer.dll engine=4 6000.0 project fsr=C:\\FSR xess=C:\\XeSS"
                ),
                "sharpen 1 0.4".into(),
                "evaluate 1 slot=0".into(),
            ]
        );
        let plan = config(&host);
        assert_eq!(
            (
                plan.struct_size as usize,
                plan.enabled,
                plan.output_width,
                plan.output_height,
                plan.render_width,
                plan.render_height
            ),
            (size_of::<Config>(), 1, 1920, 1080, 1280, 720)
        );
        assert_eq!(refs(device), 1);
        assert!(files.log_text().contains(&format!(
            "Auto SR selected backend {backend} from owning D3D12 adapter vendor={vendor:#x} device=0x2204"
        )));
    }
}

#[test]
fn unidentified_adapters_and_refused_policy_keep_native_rendering() {
    let files = Files::new("native");
    let device = fake(null_mut());
    let list = fake(device);
    let host = host(&files, SR_AUTO, 1);
    with(|world| world.adapter_ok = false);
    run(&host, list, &sr((1920, 1080)), true);
    assert!(events().is_empty());
    assert_eq!(host.lock().last_result, BACKEND_NOT_SUPPORTED);
    assert_eq!(config(&host).enabled, 0);
    // The attempt is not repeated until the output or configuration changes.
    run(&host, list, &sr((1920, 1080)), true);
    assert!(events().is_empty());
    assert!(
        files
            .log_text()
            .contains("Automatic SR refused: owning D3D12 adapter could not be identified.")
    );

    let host = self::host(&files, SR_AUTO, 1);
    with(|world| world.policy_ok = false);
    run(&host, list, &sr((1920, 1080)), true);
    assert_eq!(events(), ["policy 0 1"]);
    assert_eq!(config(&host).enabled, 0);

    // A disabled host plans native rendering and still prepares generation inputs.
    let host = self::host(&files, 0, 1);
    run(&host, list, &sr((1920, 1080)), true);
    assert!(events().is_empty());
    assert_eq!(config(&host).enabled, 0);
    assert_eq!(refs(device), 1);
}

#[test]
fn spatial_upscaling_is_planned_for_the_engine() {
    reset();
    let files = Files::new("spatial");
    let list = fake(fake(null_mut()));
    for (quality, width, height) in [(0, 900, 500), (2, 902, 502), (5, 905, 505)] {
        let setup = setup_v2(&files, SR_SPATIAL, quality);
        let host = unsafe { describe(&setup.base, &PLATFORM) }.ok().unwrap();
        events();
        run(&host, list, &sr((1920, 1080)), true);
        assert!(events().is_empty(), "no vendor context");
        let plan = config(&host);
        assert_eq!(
            (plan.enabled, plan.render_width, plan.render_height),
            (1, width, height)
        );
    }
    let log = files.log_text();
    assert_eq!(
        log.matches("Engine spatial upscaling plan accepted.\r\n")
            .count(),
        3
    );
}

#[test]
fn preset_change_waits_for_its_drain_even_during_resize() {
    reset();
    let files = Files::new("preset-drain");
    let setup = setup_v2(&files, SR_DLSS, 2);
    let host = unsafe { describe(&setup.base, &PLATFORM) }.ok().unwrap();
    let device = fake(null_mut());
    let queue = fake(device);
    let list = fake(device);
    run(&host, list, &sr((1920, 1080)), true);
    events();
    window(
        &host,
        queue,
        Some(Intent {
            dlss_preset_changed: 1,
            dlss_preset: 10,
            ..intent()
        }),
    );
    assert_eq!(events(), ["fg window 5", "overlay", "policy 0 2"]);
    assert_eq!(config(&host).enabled, 0);
    let mut flagged = sr((2560, 1440));
    flagged.flags = RSF_GAME_RENDER_RECONFIGURE;
    run(&host, list, &flagged, true);
    assert!(events().is_empty());
    with(|world| world.drained_generation = 42);
    run(&host, list, &flagged, true);
    let rebuilt = events();
    assert_eq!(rebuilt[0], "destroy 1");
    assert!(rebuilt[1].contains("preset=10"));
    assert_eq!(config(&host).enabled, 1);
}

#[test]
fn quality_five_spatial_accepts_an_enable_only_change() {
    reset();
    let files = Files::new("spatial-enable");
    let setup = setup_v2(&files, SR_SPATIAL, 5);
    let host = unsafe { describe(&setup.base, &PLATFORM) }.ok().unwrap();
    let device = fake(null_mut());
    let queue = fake(device);
    let list = fake(device);
    run(&host, list, &sr((1920, 1080)), true);
    assert_eq!(config(&host).enabled, 1);
    events();
    window(
        &host,
        queue,
        Some(Intent {
            enabled_changed: 1,
            enabled: 0,
            ..intent()
        }),
    );
    assert_eq!(events(), ["fg window 5", "overlay", "policy 1 5"]);
    assert_eq!(config(&host).enabled, 0);
    assert!(!host.lock().enabled);
}

#[test]
fn recording_slots_bind_three_lists_and_count_results() {
    let files = Files::new("slots");
    let host = host(&files, 1, 1);
    let device = fake(null_mut());
    let lists = [fake(device), fake(device), fake(device), fake(device)];
    run(&host, lists[0], &sr((1920, 1080)), true);
    events();
    for (index, &list) in lists.iter().enumerate() {
        run(&host, list, &sr((1920, 1080)), true);
        let expected: Vec<String> = if index < 3 {
            vec![format!("evaluate 1 slot={index}")]
        } else {
            Vec::new()
        };
        assert_eq!(events(), expected);
    }
    run(&host, lists[1], &sr((1920, 1080)), true);
    assert_eq!(events(), ["evaluate 1 slot=1"]);

    with(|world| world.evaluate_result = -8);
    let mut refused = sr((1920, 1080));
    refused.color_input = 0x100 as Handle;
    refused.motion = 0x300 as Handle;
    refused.flags = RSF_GAME_RENDER_RESET;
    for _ in 0..5 {
        run(&host, lists[0], &refused, true);
    }
    let log = files.log_text();
    assert_eq!(log.matches("SR input ").count(), 2, "first refusal only");
    assert!(log.contains("SR input 0: 256x2 format=10 samples=1; camera=1280x720 plan=1280x720"));
    assert!(log.contains("SR input 2: 768x2 format=10 samples=1; camera=1280x720 plan=1280x720"));
    assert_eq!(log.matches("result -8,").count(), 3);
    assert!(log.contains(
        "Native renderer SR view 3 frame 9 result -8, accepted=5 refused=1 reset=1 jitter=0.250,-0.125"
    ));
    let state = host.lock();
    assert_eq!(
        (state.accepted, state.refused, state.last_result),
        (5, 5, -8)
    );
    drop(state);
    with(|world| world.evaluate_result = BACKEND_OK);
    for _ in 0..595 {
        run(&host, lists[2], &sr((1920, 1080)), true);
    }
    let log = files.log_text();
    assert!(log.contains("accepted=3 refused=0"));
    assert!(!log.contains("accepted=4 refused"));
    assert!(log.contains("accepted=600 refused=5"));
}

#[test]
fn generation_inputs_use_a_backend_zero_context_at_render_extent() {
    let files = Files::new("inputs");
    let host = host(&files, 3, 1);
    let device = fake(null_mut());
    let list = fake(device);
    let inputs = pass(RSF_GAME_RENDER_FG_INPUTS, (1920, 1080), (1280, 720));
    run(&host, list, &inputs, true);
    let created = events();
    assert_eq!(created.len(), 4);
    assert!(created[0].starts_with("create abi=1 backend=3 quality=1 0x0->1920x1080"));
    assert_eq!(created[1], "sharpen 1 0.4");
    assert!(created[2].starts_with("create abi=1 backend=0 quality=0 1280x720->1920x1080"));
    assert_eq!(created[3], "evaluate 2 slot=0");
    run(&host, list, &inputs, true);
    assert_eq!(events(), ["evaluate 2 slot=0"]);
    let smaller = pass(RSF_GAME_RENDER_FG_INPUTS, (1920, 1080), (960, 540));
    run(&host, list, &smaller, true);
    let recreated = events();
    assert_eq!(recreated[0], "destroy 2");
    assert!(recreated[1].starts_with("create abi=1 backend=0 quality=0 960x540->1920x1080"));
    assert_eq!(recreated[2], "evaluate 3 slot=0");
    // Without a camera the pass is neither normalized nor upscaled.
    let mut invalid = smaller;
    invalid.camera_valid = 0;
    run(&host, list, &invalid, true);
    assert!(events().is_empty());
    assert_eq!(refs(device), 1);
}

#[test]
fn pass_ends_report_submissions_and_scenes_route_to_generation() {
    let files = Files::new("routing");
    let host = host(&files, 3, 1);
    let list = fake(fake(null_mut()));
    for role in [
        RSF_GAME_RENDER_SR,
        RSF_GAME_RENDER_FG_INPUTS,
        RSF_GAME_RENDER_FINAL_SCENE,
        RSF_GAME_RENDER_SUBMISSION,
    ] {
        run(&host, list, &pass(role, (1920, 1080), (1280, 720)), false);
        assert_eq!(events(), [format!("fg submitted {SESSION} 5")]);
    }
    run(
        &host,
        list,
        &pass(RSF_GAME_RENDER_WINDOW, (1920, 1080), (1280, 720)),
        false,
    );
    run(
        &host,
        list,
        &pass(RSF_GAME_RENDER_SUBMISSION, (1920, 1080), (1280, 720)),
        true,
    );
    assert!(events().is_empty());
    run(
        &host,
        list,
        &pass(RSF_GAME_RENDER_FINAL_SCENE, (1920, 1080), (1280, 720)),
        true,
    );
    assert_eq!(events(), ["fg hudless 5"]);
    // Depth-domain requests, missing lists and short passes are not this host's.
    let mut domains = sr((1920, 1080));
    domains.depth_domains.domain_count = 1;
    run(&host, list, &domains, true);
    run(&host, list, &domains, false);
    run(&host, null_mut(), &sr((1920, 1080)), true);
    let mut short = sr((1920, 1080));
    short.struct_size -= 8;
    run(&host, list, &short, true);
    unsafe { render(host.user(), list, null(), 1) };
    assert!(events().is_empty());
    let mut cpu: CpuEvent = unsafe { std::mem::zeroed() };
    cpu.source_frame_id = 12;
    unsafe { cpu_event(host.user(), &cpu) };
    assert_eq!(events(), ["fg cpu 12"]);
}

fn window(host: &Host, queue: Handle, intent: Option<Intent>) -> Stats {
    with(|world| world.intent = intent);
    run(
        host,
        queue,
        &pass(RSF_GAME_RENDER_WINDOW, (2560, 1440), (0, 0)),
        true,
    );
    with(|world| world.stats.take()).expect("overlay drawn")
}

fn intent() -> Intent {
    Intent {
        struct_size: size_of::<Intent>() as u32,
        ..Intent::default()
    }
}

#[test]
fn the_overlay_reports_host_state_and_persists_panel_intent() {
    let files = Files::new("overlay");
    files.write("DlssPreset", "6");
    let host = host(&files, 1, 1);
    let device = fake(null_mut());
    let queue = fake(device);
    let list = fake(device);
    run(&host, list, &sr((1920, 1080)), true);
    events();

    let stats = window(&host, queue, None);
    assert_eq!(events(), ["fg window 5", "overlay"]);
    assert_eq!(
        unsafe { CStr::from_ptr(stats.backend_name) },
        c"DLSS",
        "backend name"
    );
    assert!(stats.refusal_reason.is_null());
    assert_eq!(
        (stats.backend, stats.requested_backend, stats.quality),
        (1, 1, 1)
    );
    assert_eq!(
        (stats.frames_presented, stats.application_presented_frames),
        (1, 41)
    );
    assert_eq!(
        (stats.frames_evaluated, stats.render_scale_percent),
        (1, 66)
    );
    assert_eq!((stats.output_width, stats.output_height), (2560, 1440));
    assert_eq!((stats.sharpening_available, stats.jitter_active), (1, 1));
    assert_eq!((stats.dlss_preset, stats.fg_available), (6, 0));
    assert_eq!(
        stats.sr_backend_choices,
        (1 << 1) | (1 << 3) | (1 << 5) | (1 << 6)
    );
    assert_eq!(refs(device), 1);

    // Generation options go to FG12 from the status the panel was drawn with.
    let mut status: FgStatus = unsafe { std::mem::zeroed() };
    status.requested.mode = 1;
    status.requested.generated_frames = 2;
    with(|world| world.fg_status = Some(status));
    let stats = window(
        &host,
        queue,
        Some(Intent {
            reflex_changed: 1,
            reflex_mode: 2,
            frame_limit_changed: 1,
            frame_limit_us: 8000,
            performance_hud_changed: 1,
            performance_hud: 1,
            ui_preferences_changed: 1,
            ui_preferences: 0x55,
            ..intent()
        }),
    );
    assert_eq!(stats.fg_available, 1);
    assert_eq!(
        events(),
        [
            "fg window 5",
            "overlay",
            "fg options mode=1 generated=2 reflex=2 limit=8000"
        ]
    );
    assert_eq!(files.integer("OverlayLayout"), 0x55);
    let stats = window(&host, queue, None);
    assert_eq!(
        (stats.show_performance_hud, stats.ui_preferences),
        (1, 0x55)
    );
    events();

    // Valid sharpening is saved and applied, invalid sharpening keeps the saved values.
    let sharpen = Intent {
        sharpening_changed: 1,
        sharpening_mode: 2,
        sharpening_sharpness: 0.75,
        sharpening_depth_scale: 35.0,
        ..intent()
    };
    window(&host, queue, Some(sharpen));
    assert_eq!(events(), ["fg window 5", "overlay", "sharpen 1 0.75"]);
    assert!(files.preferences_text().contains("Sharpness=0.75"));
    window(
        &host,
        queue,
        Some(Intent {
            sharpening_sharpness: 3.0,
            ..sharpen
        }),
    );
    assert_eq!(events(), ["fg window 5", "overlay"]);
    assert!(
        files
            .log_text()
            .contains("Could not save sharpening settings; previous values retained.")
    );
    assert_eq!(host.lock().sharpening.sharpness, 0.75);

    // A preset change is saved under the adapter's existing key and drains the context.
    window(
        &host,
        queue,
        Some(Intent {
            dlss_preset_changed: 1,
            dlss_preset: 9,
            ..intent()
        }),
    );
    events();
    assert_eq!(files.integer("DlssPreset"), 9);
    assert_eq!(
        files
            .preferences_text()
            .to_ascii_lowercase()
            .matches("dlsspreset")
            .count(),
        1
    );
    assert_eq!(config(&host).enabled, 0);
    let stats = window(&host, queue, None);
    assert_eq!(
        unsafe { CStr::from_ptr(stats.refusal_reason) },
        c"Applying requested configuration after GPU completion"
    );
    window(
        &host,
        queue,
        Some(Intent {
            dlss_preset_changed: 1,
            dlss_preset: 40,
            ..intent()
        }),
    );
    assert_eq!(files.integer("DlssPreset"), 9);
    assert_eq!(host.lock().preset, 9);
    events();

    // A panel draw that was refused applies nothing.
    with(|world| world.drawn = 0);
    window(
        &host,
        queue,
        Some(Intent {
            backend_changed: 1,
            backend: 3,
            ..intent()
        }),
    );
    assert_eq!(host.lock().requested_backend, 1);
    with(|world| world.drawn = 1);
    events();
}

#[test]
fn provider_changes_respect_policy_and_wait_for_the_flagged_pass() {
    let files = Files::new("reconfigure");
    let host = host(&files, 1, 1);
    let device = fake(null_mut());
    let queue = fake(device);
    let list = fake(device);
    run(&host, list, &sr((1920, 1080)), true);
    events();

    with(|world| world.policy_ok = false);
    window(
        &host,
        queue,
        Some(Intent {
            backend_changed: 1,
            backend: SR_SPATIAL,
            ..intent()
        }),
    );
    assert_eq!(events(), ["fg window 5", "overlay", "policy 1 1"]);
    assert_eq!(host.lock().requested_backend, 1);
    with(|world| world.policy_ok = true);
    for refused in [
        Intent {
            backend_changed: 1,
            backend: SR_AUTO,
            ..intent()
        },
        Intent {
            quality_changed: 1,
            quality: 5,
            ..intent()
        },
    ] {
        window(&host, queue, Some(refused));
        assert_eq!(events(), ["fg window 5", "overlay"]);
    }
    window(
        &host,
        queue,
        Some(Intent {
            backend_changed: 1,
            backend: 3,
            quality_changed: 1,
            quality: 3,
            enabled_changed: 1,
            enabled: 1,
            ..intent()
        }),
    );
    assert_eq!(events(), ["fg window 5", "overlay", "policy 0 3"]);
    assert_eq!(config(&host).enabled, 0);
    // The current context keeps evaluating until the adapter flags the drained pass.
    run(&host, list, &sr((1920, 1080)), true);
    assert_eq!(events(), ["evaluate 1 slot=0"]);
    let mut flagged = sr((1920, 1080));
    flagged.flags = RSF_GAME_RENDER_RECONFIGURE;
    run(&host, list, &flagged, true);
    let rebuilt = events();
    assert_eq!(rebuilt[0], "destroy 1");
    assert!(rebuilt[1].starts_with("create abi=1 backend=3 quality=3 "));
    assert_eq!(rebuilt.last().unwrap(), "evaluate 2 slot=0");
    assert_eq!(config(&host).enabled, 1);
    assert!(!host.lock().automatic);
    // An output resize rebuilds without a flag.
    run(&host, list, &sr((2560, 1440)), true);
    let resized = events();
    assert_eq!(resized[0], "destroy 2");
    assert!(resized[1].contains("->2560x1440"));
    assert_eq!(refs(device), 1);
}

#[test]
fn generation_provider_choice_is_saved_after_the_switch_lands() {
    let files = Files::new("provider");
    let host = host(&files, 1, 1);
    let queue = fake(fake(null_mut()));
    let request = Intent {
        fg_backend_changed: 1,
        fg_backend: 5,
        ..intent()
    };
    with(|world| world.request_result = -7);
    window(&host, queue, Some(request));
    assert_eq!(events(), ["fg window 5", "overlay", "fg request 5"]);
    assert!(
        files
            .log_text()
            .contains("Generation provider request refused.")
    );
    with(|world| world.request_result = BACKEND_OK);
    window(&host, queue, Some(request));
    assert_eq!(events(), ["fg window 5", "overlay", "fg request 5"]);
    let stats = window(&host, queue, None);
    assert_eq!(stats.fg_requested_backend, 5, "the pending target is shown");
    with(|world| world.present_backend = 5);
    let stats = window(&host, queue, None);
    assert_eq!(
        events(),
        [
            "fg window 5",
            "overlay",
            "fg window 5",
            "fg save 5",
            "overlay"
        ]
    );
    assert_eq!(stats.fg_requested_backend, 3);

    window(&host, queue, Some(request));
    events();
    with(|world| {
        world.present_backend = 3;
        world.switch_result = -9;
    });
    window(&host, queue, None);
    window(&host, queue, None);
    assert_eq!(
        events(),
        ["fg window 5", "overlay", "fg window 5", "overlay"]
    );
    assert!(
        files
            .log_text()
            .contains("Generation provider switch failed; the saved choice is unchanged.")
    );
}

#[test]
fn sharpening_is_polled_from_the_preferences_once_a_second() {
    let files = Files::new("poll");
    files.write("Sharpness", "0.3");
    let host = host(&files, 1, 1);
    assert_eq!(host.lock().sharpening.sharpness, 0.3);
    let list = fake(fake(null_mut()));
    run(&host, list, &sr((1920, 1080)), true);
    assert_eq!(&events()[1..], ["sharpen 1 0.3", "evaluate 1 slot=0"]);
    files.write("Sharpness", "1.7");
    with(|world| world.now = 999);
    run(&host, list, &sr((1920, 1080)), true);
    assert_eq!(events(), ["evaluate 1 slot=0"]);
    with(|world| world.now = 1000);
    run(&host, list, &sr((1920, 1080)), true);
    // Out-of-range saved values are sanitized before they reach the context.
    assert_eq!(events(), ["sharpen 1 1", "evaluate 1 slot=0"]);
}

#[test]
fn log_lines_are_appended_and_shared() {
    let files = Files::new("log");
    std::fs::write(Files::path(&files.log), "earlier\r\n").unwrap();
    let host = host(&files, 1, 1);
    let reader = std::fs::File::open(Files::path(&files.log)).unwrap();
    unsafe { log(host.user(), c"from the plugin".as_ptr()) };
    unsafe { log(host.user(), null()) };
    drop(reader);
    assert_eq!(files.log_text(), "earlier\r\nfrom the plugin\r\n");
}
