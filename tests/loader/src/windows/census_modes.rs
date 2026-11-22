// SPDX-License-Identifier: MIT
use super::vendor_census_asi_abi::*;
use super::{named, wide};
use serde_json::Value;
use std::{
    ffi::{CStr, c_char},
    fs,
    mem::transmute,
    path::Path,
};
use windows_sys::Win32::{
    Foundation::FARPROC,
    System::{Environment::SetEnvironmentVariableA, LibraryLoader::LoadLibraryW},
};

type Install = unsafe extern "C" fn(*const rsf_census_options) -> i32;
type CensusOp = unsafe extern "C" fn() -> i32;
type Counters = unsafe extern "C" fn(*mut rsf_census_counters) -> i32;
type Jitter = unsafe extern "C" fn(i32) -> i32;
type DispatchTwice = unsafe extern "C" fn(*mut [i32; 2]);
type NativeProbe = unsafe extern "C" fn() -> i32;

macro_rules! expect {
    ($checks:expr, $condition:expr) => {
        $checks.check($condition, stringify!($condition));
    };
}

mod functional;

#[derive(Default)]
struct Checks(u32);

impl Checks {
    fn check(&mut self, pass: bool, message: &str) {
        if !pass {
            eprintln!("FAIL {message}");
            self.0 += 1;
        }
    }

    fn read(&mut self, dir: &Path, leaf: &str) -> Vec<u8> {
        match fs::read(dir.join(leaf)) {
            Ok(data) => data,
            Err(error) => {
                eprintln!("FAIL reading {leaf}: {error}");
                self.0 += 1;
                Vec::new()
            }
        }
    }

    fn summary(&mut self, dir: &Path) -> Value {
        let data = self.read(dir, "census-summary.json");
        match serde_json::from_slice(&data) {
            Ok(summary) => summary,
            Err(error) => {
                eprintln!("FAIL parsing census-summary.json: {error}");
                self.0 += 1;
                Value::Null
            }
        }
    }

    fn records(&mut self, data: &[u8]) -> Vec<Value> {
        data.split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .enumerate()
            .filter_map(|(index, line)| match serde_json::from_slice(line) {
                Ok(row) => Some(row),
                Err(error) => {
                    eprintln!("FAIL parsing census.jsonl line {}: {error}", index + 1);
                    self.0 += 1;
                    None
                }
            })
            .collect()
    }

    fn counters(&mut self, get: Counters) -> rsf_census_counters {
        let mut out = rsf_census_counters {
            struct_size: size_of::<rsf_census_counters>() as u32,
            abi_version: RSF_CENSUS_ABI_VERSION,
            ..Default::default()
        };
        self.check(unsafe { get(&mut out) } == 0, "read census counters");
        out
    }

    fn near(&mut self, value: &Value, expected: f64, field: &str) {
        self.check(
            value
                .as_f64()
                .is_some_and(|actual| (actual - expected).abs() <= 1e-5 * (1.0 + expected.abs())),
            &format!("{field}: expected {expected}, got {value}"),
        );
    }
}

fn entries(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn by_fn<'a>(rows: &'a [Value], name: &str) -> Vec<&'a Value> {
    rows.iter().filter(|row| row["fn"] == name).collect()
}

fn hex(value: &Value) -> Option<u64> {
    u64::from_str_radix(value.as_str()?.strip_prefix("0x")?, 16).ok()
}

fn includes(value: &Value, item: &str) -> bool {
    entries(value).iter().any(|value| value == item)
}

fn exe_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

fn first_args<'a>(checks: &mut Checks, rows: &'a [Value], operation: &str) -> &'a Value {
    if let Some(args) = rows
        .iter()
        .find(|row| row["fn"] == operation)
        .and_then(|row| row.get("args"))
    {
        args
    } else {
        checks.check(false, &format!("missing arguments for {operation}"));
        &Value::Null
    }
}

fn ffx_records(checks: &mut Checks, rows: &[Value]) {
    let creates = by_fn(rows, "ffxCreateContext");
    expect!(checks, creates.len() == 2);
    if let [upscale, swapchain] = creates.as_slice() {
        let chain = &upscale["args"]["chain"];
        expect!(checks, entries(chain).len() == 2);
        expect!(checks, chain[0]["name"] == "create_upscale");
        expect!(
            checks,
            chain[0]["max_render_width"] == 1280 && chain[0]["max_upscale_height"] == 1080
        );
        expect!(checks, chain[1]["name"] == "create_backend_dx12");
        expect!(checks, hex(&chain[1]["device"]) == Some(0xde71ce));
        expect!(checks, hex(&upscale["args"]["context"]) == Some(0xf00d));
        let sc = &swapchain["args"]["chain"];
        expect!(
            checks,
            entries(sc).len() == 1 && sc[0]["name"] == "create_swapchain_for_hwnd_dx12"
        );
        expect!(checks, sc[0]["swapchain_desc1"]["width"] == 3840);
        expect!(
            checks,
            sc[0]["swapchain_desc1"]["format_name"] == "R10G10B10A2_UNORM"
        );
        expect!(checks, sc[0]["swapchain_desc1"]["buffer_count"] == 3);
        expect!(checks, hex(&sc[0]["game_queue"]) == Some(0x9e3e));
    }
    let dispatches = by_fn(rows, "ffxDispatch");
    expect!(checks, dispatches.len() == 2);
    if let [upscale, generated] = dispatches.as_slice() {
        expect!(checks, hex(&upscale["ret"]) == Some(0x40));
        let chain = &upscale["args"]["chain"];
        expect!(
            checks,
            entries(chain).len() == 1 && chain[0]["name"] == "dispatch_upscale"
        );
        let r = &chain[0]["resources"];
        expect!(checks, r["color"]["d3d12"]["width"] == 1280);
        expect!(checks, r["depth"]["d3d12"]["format_name"] == "D32_FLOAT");
        expect!(checks, r["output"]["d3d12"]["width"] == 1920);
        expect!(checks, r["output"]["state"] == "0x4");
        let s = &chain[0]["scalars"];
        checks.near(&s["jitter_x"], 0.5, "upscale jitter x");
        checks.near(&s["jitter_y"], -0.25, "upscale jitter y");
        expect!(
            checks,
            s["render_width"] == 1280 && s["upscale_height"] == 1080
        );
        expect!(checks, s["enable_sharpening"] == true && s["reset"] == true);
        checks.near(&s["frame_time_delta_ms"], 8.3, "frame delta");
        checks.near(&s["camera_far"], 10000.0, "camera far");
        checks.near(&s["camera_fov_vertical"], 0.9, "camera FOV");
        expect!(checks, s["flags"] == "0x2");
        let chain = &generated["args"]["chain"];
        expect!(
            checks,
            entries(chain).len() == 1 && chain[0]["name"] == "dispatch_framegeneration"
        );
        let fg = &chain[0];
        expect!(
            checks,
            fg["num_generated_frames"] == 1 && fg["reset"] == true
        );
        expect!(checks, fg["backbuffer_transfer_function"] == 2);
        expect!(checks, fg["frame_id"] == 1234);
        checks.near(&fg["max_luminance"], 1000.0, "frame-generation luminance");
    }
    let configs = by_fn(rows, "ffxConfigure");
    expect!(checks, configs.len() == 1);
    if let [config] = configs.as_slice() {
        let node = &config["args"]["chain"][0];
        expect!(checks, node["name"] == "configure_framegeneration");
        expect!(
            checks,
            hex(&node["swap_chain"]) == Some(0x5c5c) && node["frame_generation_enabled"] == true
        );
        expect!(checks, node["frame_id"] == 99);
    }
    expect!(checks, by_fn(rows, "ffxQuery").len() == 1);
    expect!(checks, by_fn(rows, "ffxDestroyContext").len() == 1);
}

fn vk_records(checks: &mut Checks, rows: &[Value], summary: &Value) {
    let instance = first_args(checks, rows, "vkCreateInstance");
    expect!(
        checks,
        instance["application"] == "CensusGame" && instance["engine"] == "CensusEngine"
    );
    expect!(
        checks,
        instance["api_major"] == 1 && instance["api_minor"] == 3
    );
    expect!(
        checks,
        includes(&instance["extensions"], "VK_KHR_win32_surface")
    );
    expect!(checks, hex(&instance["instance"]) == Some(0x1001));
    let device = first_args(checks, rows, "vkCreateDevice");
    expect!(checks, entries(&device["queue_families"]).len() == 2);
    expect!(
        checks,
        device["queue_families"][1]["family"] == 1 && device["queue_families"][1]["count"] == 2
    );
    expect!(checks, includes(&device["extensions"], "VK_KHR_swapchain"));
    let chain = first_args(checks, rows, "vkCreateSwapchainKHR");
    expect!(checks, chain["width"] == 2560 && chain["height"] == 1440);
    expect!(
        checks,
        chain["image_format"] == 50 && chain["min_image_count"] == 3
    );
    expect!(checks, chain["present_mode_name"] == "MAILBOX");
    expect!(checks, hex(&chain["swapchain"]) == Some(0x4004));
    let presents = by_fn(rows, "vkQueuePresentKHR");
    expect!(checks, presents.len() == 3);
    let executable = exe_name();
    for (index, present) in presents.into_iter().enumerate() {
        expect!(checks, present["present"] == index + 1);
        expect!(checks, entries(&present["args"]["swapchains"]).len() == 1);
        expect!(checks, present["args"]["image_indices"][0] == 1);
        expect!(
            checks,
            executable
                .as_deref()
                .is_some_and(|exe| present["module"] == exe)
        );
    }
    let submits = by_fn(rows, "vkQueueSubmit");
    expect!(checks, submits.len() >= 2);
    if let Some(first) = submits.first() {
        expect!(checks, first["args"]["batches"] == 2);
        expect!(checks, first["args"]["command_buffers_in_first_16"] == 4);
    }
    let submit2 = first_args(checks, rows, "vkQueueSubmit2");
    expect!(checks, submit2["command_buffers_in_first_16"] == 2);
    let queues = entries(&summary["queues"]);
    expect!(checks, queues.len() == 2);
    let mut seen = 0;
    for queue in queues {
        expect!(checks, queue["kind"] == "vulkan");
        match hex(&queue["queue"]) {
            Some(0x3000) => {
                seen += 1;
                expect!(checks, queue["family"] == 0 && queue["index"] == 0);
                expect!(
                    checks,
                    queue["submit_calls"] == 6 && queue["lists_or_batches"] == 9
                );
                expect!(checks, queue["calls_per_present"]["2"] == 3);
            }
            Some(0x3011) => {
                seen += 1;
                expect!(checks, queue["family"] == 1 && queue["index"] == 1);
                expect!(
                    checks,
                    queue["submit_calls"] == 3 && queue["calls_per_present"]["1"] == 3
                );
            }
            _ => {}
        }
    }
    expect!(checks, seen == 2);
    expect!(checks, summary["present_index"] == 3);
}

fn sl_records(checks: &mut Checks, rows: &[Value], summary: &Value) {
    let init = first_args(checks, rows, "slInit");
    expect!(
        checks,
        init["application_id"] == 0x1234 && init["engine"] == 1
    );
    expect!(
        checks,
        init["engine_version"] == "5.4" && init["project_id"] == "census-project"
    );
    expect!(
        checks,
        init["log_level"] == 2 && init["show_console"] == true
    );
    expect!(
        checks,
        entries(&init["features_to_load"]).len() == 2 && init["features_to_load"][1] == 1000
    );
    expect!(checks, init["path_to_logs_and_data"] == r"C:\census\logs");
    let reflex = first_args(checks, rows, "slReflexSleep");
    expect!(checks, reflex["frame"] == 42);
    let constants = first_args(checks, rows, "slSetConstants");
    expect!(
        checks,
        constants["frame"] == 42 && constants["viewport"] == 7
    );
    let data = &constants["constants"];
    expect!(checks, data["struct_version"] == 1);
    for (index, key) in [
        "camera_view_to_clip",
        "clip_to_camera_view",
        "clip_to_lens_clip",
        "clip_to_prev_clip",
        "prev_clip_to_clip",
    ]
    .into_iter()
    .enumerate()
    {
        let values = entries(&data[key]);
        expect!(checks, values.len() == 16);
        for (component, value) in values.iter().enumerate() {
            checks.near(value, (index * 16 + component) as f64 - 17.25, key);
        }
    }
    for (index, key) in ["camera_pos", "camera_up", "camera_right", "camera_fwd"]
        .into_iter()
        .enumerate()
    {
        let values = entries(&data[key]);
        expect!(checks, values.len() == 3);
        for (component, value) in values.iter().enumerate() {
            checks.near(value, (6 + index * 3 + component) as f64 + 0.125, key);
        }
    }
    checks.near(&data["camera_near"], 0.1, "camera near");
    checks.near(&data["camera_far"], 10000.0, "camera far");
    expect!(checks, data["depth_inverted"] == true);
    let pcl = first_args(checks, rows, "slPCLSetMarker");
    expect!(
        checks,
        pcl["marker"] == 4 && pcl["marker_name"] == "PresentStart" && pcl["frame"] == 42
    );
    let options = first_args(checks, rows, "slDLSSGSetOptions");
    expect!(
        checks,
        options["viewport"] == 7 && options["mode_name"] == "On"
    );
    expect!(checks, options["num_frames_to_generate"] == 2);
    expect!(
        checks,
        options["color_width"] == 1920 && options["color_height"] == 1080
    );
    let thunks = entries(&summary["hooks"])
        .iter()
        .filter(|hook| hook["api"] == "streamline" && hook["binding"] == "getprocaddress_thunk")
        .count();
    expect!(checks, thunks == 3);
}

fn payload_mode(
    dir: &CStr,
    install: Install,
    flush: CensusOp,
    shutdown: CensusOp,
    probe: NativeProbe,
    name: &str,
    inspect: impl FnOnce(&mut Checks, &Path, &[Value]),
) -> i32 {
    let Some(path) = dir.to_str().ok().map(Path::new) else {
        return 2;
    };
    let mut checks = Checks::default();
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: dir.as_ptr(),
        full_record_cap: 8,
        summary_period_ms: 50,
        flags: RSF_CENSUS_FLAG_NO_STATIC_SCAN,
        ..Default::default()
    };
    let installed = unsafe { install(&options) } == 0;
    checks.check(installed, "install census for vendor payloads");
    if installed {
        expect!(checks, unsafe { probe() } == 0);
        expect!(checks, unsafe { flush() } == 0);
        let data = checks.read(path, "census.jsonl");
        let rows = checks.records(&data);
        inspect(&mut checks, path, &rows);
        expect!(checks, unsafe { shutdown() } == 0);
    }
    println!("vendor_census {name}: {} failure(s)", checks.0);
    i32::from(checks.0 != 0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_vulkan(
    dir: *const c_char,
    install: Install,
    flush: CensusOp,
    shutdown: CensusOp,
    probe: NativeProbe,
) -> i32 {
    payload_mode(
        unsafe { CStr::from_ptr(dir) },
        install,
        flush,
        shutdown,
        probe,
        "vulkan_streamline",
        |checks, path, rows| {
            let summary = checks.summary(path);
            vk_records(checks, rows, &summary);
            sl_records(checks, rows, &summary);
        },
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_ffx(
    dir: *const c_char,
    install: Install,
    flush: CensusOp,
    shutdown: CensusOp,
    probe: NativeProbe,
) -> i32 {
    payload_mode(
        unsafe { CStr::from_ptr(dir) },
        install,
        flush,
        shutdown,
        probe,
        "fidelityfx_api",
        |checks, _, rows| ffx_records(checks, rows),
    )
}

unsafe fn directory<'a>(text: *const c_char) -> Option<&'a Path> {
    unsafe { CStr::from_ptr(text) }.to_str().ok().map(Path::new)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_budget(
    dir: *const c_char,
    install: Install,
    flush: CensusOp,
    get: Counters,
    shutdown: CensusOp,
    jitter: Jitter,
) -> i32 {
    let Some(path) = (unsafe { directory(dir) }) else {
        return 2;
    };
    let mut checks = Checks::default();
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: dir,
        byte_budget: 40_000,
        full_record_cap: 1_000_000,
        sample_interval: 1,
        summary_period_ms: 50,
        flags: RSF_CENSUS_FLAG_NO_STATIC_SCAN | RSF_CENSUS_FLAG_NO_DXGI | RSF_CENSUS_FLAG_NO_VULKAN,
    };
    checks.check(
        unsafe { install(&options) } == 0,
        "install census for budget scenario",
    );
    for index in 0..5000 {
        checks.check(
            unsafe { jitter(index) } == 0,
            "direct imported jitter function succeeds",
        );
    }
    checks.check(unsafe { flush() } == 0, "flush census records");
    let counters = checks.counters(get);
    let data = checks.read(path, "census.jsonl");
    checks.check(data.len() <= 40_000, "JSONL respects the byte budget");
    checks.check(
        counters.bytes_written == data.len() as u64,
        "counter matches JSONL byte length",
    );
    checks.check(
        counters.bytes_written > 30_000,
        "the scenario actually uses the budget",
    );
    checks.check(counters.calls_seen >= 5000, "all jitter calls count");
    let dropped = counters.records_dropped_budget + counters.records_dropped_queue;
    checks.check(dropped > 0, "the scenario drops records");
    checks.check(
        counters.records_dropped_size == 0,
        "the scenario drops no oversized records",
    );
    checks.check(
        counters.records_written + counters.records_dropped_budget == counters.records_queued,
        "queued count matches written and budget-dropped records",
    );
    let _ = checks.records(&data);
    let summary = checks.summary(path);
    let counted: u64 = summary["sites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|site| site["function"] == "ffxFsr2GetJitterOffset")
        .filter_map(|site| site["count"].as_u64())
        .sum();
    checks.check(counted == 5000, "summary retains every jitter call");
    checks.check(
        summary["budget"]["exhausted"] == true,
        "summary reports exhaustion",
    );
    checks.check(
        summary["budget"]["bytes_written"]
            .as_u64()
            .is_some_and(|bytes| bytes <= 40_000),
        "summary respects the byte budget",
    );
    checks.check(unsafe { shutdown() } == 0, "shut down census");
    println!(
        "vendor_census budget: {} failure(s), {} bytes written, {dropped} records dropped",
        checks.0, counters.bytes_written
    );
    i32::from(checks.0 != 0)
}

struct Asi {
    result: CensusOp,
    flush: CensusOp,
    counters: Counters,
    shutdown: CensusOp,
}

fn load_asi() -> Option<Asi> {
    let path = std::env::current_exe()
        .ok()?
        .with_file_name("rsf_census_asi.dll");
    let path = wide(path);
    let module = unsafe { LoadLibraryW(path.as_ptr()) };
    if module.is_null() {
        return None;
    }
    type Entry = unsafe extern "system" fn() -> isize;
    let resolve: [FARPROC; 4] = [
        named(module, c"asi_install_result"),
        named(module, c"asi_flush"),
        named(module, c"asi_counters"),
        named(module, c"asi_shutdown"),
    ];
    Some(Asi {
        result: unsafe { transmute::<Entry, CensusOp>(resolve[0]?) },
        flush: unsafe { transmute::<Entry, CensusOp>(resolve[1]?) },
        counters: unsafe { transmute::<Entry, Counters>(resolve[2]?) },
        shutdown: unsafe { transmute::<Entry, CensusOp>(resolve[3]?) },
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_dllmain(dir: *const c_char, dispatch: DispatchTwice) -> i32 {
    let Some(path) = (unsafe { directory(dir) }) else {
        return 2;
    };
    let mut checks = Checks::default();
    checks.check(
        unsafe { SetEnvironmentVariableA(c"RSF_CENSUS_OUT".as_ptr().cast(), dir.cast()) } != 0,
        "configure the ASI fixture output",
    );
    let Some(asi) = load_asi() else {
        eprintln!("FAIL load the ASI fixture and its required exports");
        return 1;
    };
    checks.check(
        unsafe { (asi.result)() } == 0,
        "ASI installs under the loader lock",
    );
    let mut receipts = [0; 2];
    unsafe { dispatch(&mut receipts) };
    checks.check(
        receipts == [0x1234_abcd; 2],
        "both native FSR2 dispatch return values survive",
    );
    checks.check(unsafe { (asi.flush)() } == 0, "flush ASI census records");
    let counters = checks.counters(asi.counters);
    checks.check(
        counters.hooks_installed > 0 && counters.modules_hooked > 0,
        "hook modules already mapped",
    );
    let data = checks.read(path, "census.jsonl");
    let rows = checks.records(&data);
    let dispatches: Vec<_> = rows
        .iter()
        .filter(|row| row["fn"] == "ffxFsr2ContextDispatch")
        .collect();
    checks.check(
        dispatches.len() == 2,
        "both native FSR2 dispatches are recorded",
    );
    if let Some(first) = dispatches.first() {
        checks.check(
            first["args"]["layout_check"] == "ok",
            "first dispatch layout is accepted",
        );
        let module = std::env::current_exe().ok().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
        checks.check(
            module
                .as_deref()
                .is_some_and(|module| first["module"] == module),
            "first dispatch belongs to the fixture executable",
        );
    }
    let summary = checks.summary(path);
    checks.check(
        summary["modules_hooked"].as_array().is_some_and(|modules| {
            modules
                .iter()
                .any(|module| module == "ffx_fsr2_api_x64.dll")
        }),
        "summary identifies the already mapped FSR2 fixture",
    );
    checks.check(unsafe { (asi.shutdown)() } == 0, "shut down ASI census");
    println!("vendor_census dllmain: {} failure(s)", checks.0);
    i32::from(checks.0 != 0)
}
