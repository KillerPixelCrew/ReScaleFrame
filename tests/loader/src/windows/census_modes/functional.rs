// SPDX-License-Identifier: MIT
use super::{
    CensusOp, Checks, Counters, Install, NativeProbe, RSF_CENSUS_ABI_VERSION, by_fn, directory,
    entries, exe_name, hex, includes, rsf_census_counters, rsf_census_options,
};
use crate::windows::census_facts_abi::rsf_test_census_functional_facts;
use crate::windows::census_process::executable_anchor;
use serde_json::Value;
use std::ffi::c_char;
use windows_sys::Win32::System::Threading::Sleep;

type Probe = unsafe extern "C" fn(*mut rsf_test_census_functional_facts) -> i32;

fn fsr_records(checks: &mut Checks, rows: &[Value], exe: &str, size: u32) {
    let records = by_fn(rows, "ffxFsr2ContextDispatch");
    expect!(checks, records.len() >= 7);
    let (mut ok, mut failed, mut named) = (0, 0, 0);
    for row in records {
        expect!(checks, row["module"] == exe);
        expect!(
            checks,
            hex(&row["rva"]).is_some_and(|rva| rva > 0x1000 && rva < u64::from(size))
        );
        expect!(checks, row["api"] == "fsr2");
        expect!(checks, row["tid"].as_u64().is_some_and(|tid| tid > 0));
        expect!(checks, hex(&row["ret"]) == Some(0x1234_abcd));
        named += u32::from(row["thread"] == "census-worker");
        let args = &row["args"];
        match args["layout_check"].as_str() {
            Some("ok") => {
                ok += 1;
                let r = &args["resources"];
                expect!(checks, r.is_object());
                expect!(checks, r["color"]["name"] == "FSR2_InputColor");
                expect!(checks, r["depth"]["name"] == "FSR2_InputDepth");
                expect!(checks, r["output"]["name"] == "FSR2_OutputUpscaledColor");
                expect!(
                    checks,
                    r["color"]["width"] == 1280 && r["output"]["height"] == 1080
                );
                expect!(checks, r["depth"]["is_depth"] == true);
                expect!(
                    checks,
                    r["color"]["d3d12"]["format_name"] == "R16G16B16A16_FLOAT"
                );
                expect!(checks, r["depth"]["d3d12"]["format_name"] == "D32_FLOAT");
                expect!(checks, r["output"]["d3d12"]["width"] == 1920);
                expect!(checks, r["reactive"].get("ptr").is_some_and(Value::is_null));
                let scalars = &args["scalars"];
                expect!(checks, scalars.is_object());
                for (key, expected) in [
                    ("jitter_x", 0.25),
                    ("jitter_y", -0.375),
                    ("mv_scale_x", 1280.0),
                    ("sharpness", 0.5),
                    ("frame_time_delta_ms", 16.6),
                    ("pre_exposure", 1.0),
                    ("camera_far", 5000.0),
                    ("camera_fov_vertical", 1.0),
                ] {
                    checks.near(&scalars[key], expected, key);
                }
                expect!(
                    checks,
                    scalars["render_width"] == 1280 && scalars["render_height"] == 720
                );
                expect!(
                    checks,
                    scalars["enable_sharpening"] == true && scalars["reset"] == false
                );
            }
            Some("failed") => {
                failed += 1;
                expect!(checks, args["raw_bytes_len"] == 1352);
                expect!(checks, args.get("resources").is_none());
            }
            _ => {}
        }
    }
    expect!(checks, ok >= 4);
    expect!(checks, failed == 1);
    expect!(checks, named == 1);
}

fn ngx_records(checks: &mut Checks, rows: &[Value], exe: &str) {
    let creates = by_fn(rows, "NVSDK_NGX_D3D12_CreateFeature");
    expect!(checks, creates.len() == 1);
    if let [row] = creates.as_slice() {
        let args = &row["args"];
        expect!(checks, args["feature"] == "SuperSampling");
        let c = &args["create"];
        expect!(checks, c.is_object());
        expect!(checks, c["Width"] == 1280 && c["Height"] == 720);
        expect!(checks, c["OutWidth"] == 1920 && c["OutHeight"] == 1080);
        expect!(
            checks,
            c["PerfQualityValue"] == 2 && c["quality"] == "MaxQuality"
        );
        let names = &c["create_flag_names"];
        expect!(
            checks,
            [
                "IsHDR",
                "MVLowRes",
                "MVJittered",
                "DepthInverted",
                "AutoExposure"
            ]
            .into_iter()
            .all(|name| includes(names, name))
        );
        expect!(checks, !includes(names, "AlphaUpscaling"));
        expect!(checks, args["handle"].is_string());
        expect!(
            checks,
            hex(&args["command_list_or_context"]) == Some(0x1111)
        );
    }
    let evals = by_fn(rows, "NVSDK_NGX_D3D12_EvaluateFeature");
    expect!(checks, evals.len() == 2);
    if let Some(row) = evals.first() {
        let args = &row["args"];
        expect!(
            checks,
            hex(&args["command_list_or_context"]) == Some(0x2222)
        );
        let r = &args["resources"];
        expect!(checks, r.is_object());
        expect!(checks, r["Color"]["format_name"] == "R16G16B16A16_FLOAT");
        expect!(
            checks,
            r["Color"]["width"] == 1280 && r["Color"]["height"] == 720
        );
        expect!(checks, r["Output"]["width"] == 1920);
        expect!(checks, r["Depth"]["format_name"] == "D32_FLOAT");
        expect!(checks, r["MotionVectors"]["format_name"] == "R16G16_FLOAT");
        expect!(checks, r["ExposureTexture"]["format_name"] == "R32_FLOAT");
        expect!(checks, r.get("TransparencyMask").is_none());
        let s = &args["scalars"];
        expect!(checks, s.is_object());
        for (key, expected) in [
            ("Jitter.Offset.X", -0.125),
            ("Jitter.Offset.Y", 0.0625),
            ("MV.Scale.X", 1280.0),
            ("Sharpness", 0.35),
            ("DLSS.Pre.Exposure", 1.5),
            ("FrameTimeDeltaInMsec", 16.6),
        ] {
            checks.near(&s[key], expected, key);
        }
        expect!(checks, s["Reset"] == 1);
        expect!(checks, s["DLSS.Render.Subrect.Dimensions.Width"] == 1280);
        expect!(checks, s["DLSS.Input.Color.Subrect.Base.X"] == 16);
        expect!(checks, row["module"] == exe);
    }
}

fn other_records(checks: &mut Checks, rows: &[Value]) {
    let queries = by_fn(rows, "nvapi_QueryInterface");
    expect!(checks, queries.len() >= 3);
    let mut saw = false;
    for q in queries {
        if q["args"]["id"] == "0xd9984c05" {
            saw = true;
            expect!(checks, q["args"]["reflex_entry"] == "NvapiSetLatencyMarker");
        }
    }
    expect!(checks, saw);
    let markers = by_fn(rows, "NvapiSetLatencyMarker");
    expect!(checks, markers.len() == 1);
    if let [row] = markers.as_slice() {
        let a = &row["args"];
        expect!(checks, a["frame_id"] == 77);
        expect!(
            checks,
            a["marker_type"] == 4 && a["marker"] == "PRESENT_START"
        );
        expect!(checks, hex(&a["device_or_queue"]) == Some(0xd3d3));
        expect!(checks, hex(&row["ret"]) == Some(0x5151));
    }
    let modes = by_fn(rows, "NvapiSetSleepMode");
    expect!(checks, modes.len() == 1);
    if let [row] = modes.as_slice() {
        let a = &row["args"];
        expect!(
            checks,
            a["low_latency_mode"] == true && a["low_latency_boost"] == true
        );
        expect!(checks, a["minimum_interval_us"] == 8333);
    }
    let jitters = by_fn(rows, "xessSetJitterScale");
    expect!(checks, jitters.len() == 1);
    if let [row] = jitters.as_slice() {
        checks.near(&row["args"]["x"], 0.25, "XeSS jitter x");
        checks.near(&row["args"]["y"], -0.5, "XeSS jitter y");
    }
    let dispatch = by_fn(rows, "xessD3D12Execute");
    expect!(checks, dispatch.len() == 2);
    if let Some(row) = dispatch.first() {
        let r = &row["args"]["resources"];
        expect!(checks, r["color"]["format_name"] == "R16G16B16A16_FLOAT");
        expect!(checks, r["velocity"]["format_name"] == "R16G16_FLOAT");
        expect!(checks, r["output"]["width"] == 1920);
        expect!(
            checks,
            r["exposure_scale"].get("ptr").is_none() || r["exposure_scale"].is_null()
        );
        checks.near(
            &row["args"]["scalars"]["jitter_x"],
            0.125,
            "XeSS execution jitter x",
        );
        expect!(checks, row["args"]["scalars"]["input_width"] == 1280);
        expect!(checks, hex(&row["ret"]) == Some(0x77));
    }
}

fn present_modified(summary: &Value) -> bool {
    entries(&summary["hooks"])
        .iter()
        .any(|hook| hook["function"] == "DxgiSwapChainPresent" && hook["entry_modified"] == true)
}

fn dxgi_records(checks: &mut Checks, rows: &[Value], summary: &Value) {
    expect!(checks, by_fn(rows, "CreateDXGIFactory1").len() == 1);
    expect!(checks, by_fn(rows, "D3D12CreateDevice").len() == 1);
    let queue = by_fn(rows, "D3d12DeviceCreateCommandQueue");
    expect!(checks, queue.len() == 1);
    if let Some(row) = queue.first() {
        expect!(checks, row["args"]["type_name"] == "DIRECT");
    }
    let chain = by_fn(rows, "DxgiFactoryCreateSwapChainForHwnd");
    expect!(checks, chain.len() == 1);
    if let Some(row) = chain.first() {
        let desc = &row["args"]["desc"];
        expect!(checks, desc["width"] == 640 && desc["height"] == 360);
        expect!(checks, desc["format_name"] == "B8G8R8A8_UNORM");
        expect!(checks, desc["buffer_count"] == 2);
        expect!(
            checks,
            desc["swap_effect"]
                == ::windows::Win32::Graphics::Dxgi::DXGI_SWAP_EFFECT_FLIP_DISCARD.0
        );
    }
    let modified = present_modified(summary);
    let present = by_fn(rows, "DxgiSwapChainPresent");
    let present1 = by_fn(rows, "DxgiSwapChainPresent1");
    if modified {
        println!(
            "NOTE: another hook layer overwrote the Present detour (overlay in process); present-count assertions relaxed, entry_modified reported"
        );
        expect!(checks, !present.is_empty() || !present1.is_empty());
    } else {
        expect!(checks, present.len() == 3);
        expect!(checks, present1.len() == 1);
        if let [first, last, test] = present.as_slice() {
            expect!(checks, first["present"] == 1);
            expect!(checks, first["args"]["swapchain_desc"]["width"] == 640);
            expect!(
                checks,
                first["args"]["swapchain_desc"]["format_name"] == "B8G8R8A8_UNORM"
            );
            expect!(checks, first["args"].get("present_flags").is_some());
            expect!(checks, first["args"].get("last_color_space_set").is_some());
            expect!(checks, last["present"] == 3);
            expect!(checks, test["present"] == 3);
            expect!(
                checks,
                includes(&test["args"]["present_flag_names"], "TEST")
            );
        }
        if let Some(row) = present1.first() {
            expect!(checks, row["present"] == 2);
        }
    }
    expect!(
        checks,
        by_fn(rows, "DxgiSwapChainSetColorSpace1").len() == 1
    );
    expect!(checks, by_fn(rows, "DxgiSwapChainResizeBuffers").len() == 1);
    expect!(
        checks,
        by_fn(rows, "D3d12QueueExecuteCommandLists").len() >= 4
    );
    let queues = entries(&summary["queues"]);
    expect!(checks, queues.len() == 1);
    if let [queue] = queues {
        expect!(checks, queue["kind"] == "d3d12");
        expect!(checks, queue["type"] == 0);
        expect!(checks, queue["submit_calls"] == 6);
        if !modified {
            expect!(checks, queue["calls_per_present"]["2"] == 3);
        }
    }
}

fn check_summary(
    checks: &mut Checks,
    summary: &Value,
    exe: &str,
    facts: &rsf_test_census_functional_facts,
) {
    expect!(checks, summary["sites"].is_array());
    let (mut total, mut helper, mut pointer, mut helper_threads) = (0, 0, 0, 0);
    let mut found = false;
    for site in entries(&summary["sites"]) {
        if site["function"] != "ffxFsr2ContextDispatch" {
            continue;
        }
        found = true;
        expect!(checks, site["module"] == exe);
        expect!(checks, site["api"] == "fsr2");
        let count = site["count"].as_u64().unwrap_or(0);
        total += count;
        if entries(&site["threads"]).len() == 2 {
            helper = count;
            helper_threads = 2;
        } else {
            pointer = count;
        }
        expect!(
            checks,
            site["first_present"]
                .as_u64()
                .zip(site["last_present"].as_u64())
                .is_some_and(|(first, last)| first <= last)
        );
    }
    expect!(checks, found && total == facts.dispatch_calls);
    expect!(checks, helper_threads == 2 && pointer == 2);
    expect!(checks, helper == facts.dispatch_calls.saturating_sub(2));
    if facts.had_dxgi != 0 && !present_modified(summary) {
        expect!(checks, summary["histograms"].is_array());
        for row in entries(&summary["histograms"]) {
            if row["function"] != "ffxFsr2ContextDispatch" {
                continue;
            }
            expect!(checks, row["calls"] == facts.dispatch_calls);
            expect!(checks, row["calls_per_present"]["2"] == 2);
            let first = facts.dispatch_calls.saturating_sub(4).to_string();
            expect!(checks, row["calls_per_present"][first.as_str()] == 1);
        }
        expect!(checks, summary["present_index"] == 3);
    }
    expect!(
        checks,
        entries(&summary["threads"])
            .iter()
            .any(|thread| thread["name"] == "census-worker")
    );
    let hooks = entries(&summary["hooks"]);
    expect!(checks, hooks.len() > 5);
    let modules = &summary["modules_hooked"];
    expect!(
        checks,
        ["ffx_fsr2_api_x64.dll", "_nvngx.dll", "libxess.dll"]
            .into_iter()
            .all(|module| includes(modules, module))
    );
    expect!(
        checks,
        hooks
            .iter()
            .any(|hook| hook["function"] == "nvapi_QueryInterface"
                && hook["binding"] == "getprocaddress_thunk")
    );
}

fn static_findings(checks: &mut Checks, summary: &Value, anchor: u64) {
    let findings = &summary["static_findings"];
    expect!(checks, findings.is_array());
    expect!(
        checks,
        entries(findings).iter().any(|entry| {
            entry["vendor"] == "fsr2"
                && entry["what"]
                    .as_str()
                    .is_some_and(|what| what.starts_with("xref to FSR2_InputColor"))
                && hex(&entry["function_rva"]) == Some(anchor)
                && hex(&entry["rva"]).is_some_and(|rva| (anchor..anchor + 0x40).contains(&rva))
        })
    );
}

fn counters(checks: &mut Checks, calls: &rsf_census_counters, rows: &[Value]) {
    expect!(checks, by_fn(rows, "ffxFsr2GetJitterOffset").len() == 15);
    for (expected, seq) in rows.iter().filter_map(|row| row.get("seq")).enumerate() {
        expect!(checks, *seq == expected);
    }
    expect!(
        checks,
        calls.records_dropped_queue == 0
            && calls.records_dropped_budget == 0
            && calls.records_dropped_size == 0
    );
    expect!(
        checks,
        calls.records_written == rows.len().saturating_sub(1) as u64
    );
    expect!(checks, calls.calls_seen > 100);
    expect!(
        checks,
        calls.hooks_installed > 10 && calls.sites > 10 && calls.static_findings > 0
    );
}

pub(super) unsafe fn rsf_test_census_functional(
    dir: *const c_char,
    install: Install,
    flush: CensusOp,
    get: Counters,
    shutdown: CensusOp,
    vtable_probe: NativeProbe,
    probe: Probe,
    after_shutdown: NativeProbe,
) -> i32 {
    let Some(path) = (unsafe { directory(dir) }) else {
        return 2;
    };
    let mut checks = Checks::default();
    expect!(checks, unsafe { vtable_probe() } == 0);
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: dir,
        byte_budget: 8 << 20,
        full_record_cap: 4,
        sample_interval: 10,
        summary_period_ms: 50,
        ..Default::default()
    };
    let wrong_version = rsf_census_options {
        abi_version: 99,
        ..options
    };
    expect!(checks, unsafe { install(&wrong_version) } == -2);
    let short = rsf_census_options {
        struct_size: 4,
        ..options
    };
    expect!(checks, unsafe { install(&short) } == -1);
    let installed = unsafe { install(&options) } == 0;
    expect!(checks, installed);
    if installed {
        expect!(checks, unsafe { install(&options) } == -3);
        let mut facts = rsf_test_census_functional_facts::default();
        expect!(checks, unsafe { probe(&mut facts) } == 0);
        let mut sample = rsf_census_counters {
            struct_size: size_of::<rsf_census_counters>() as u32,
            abi_version: RSF_CENSUS_ABI_VERSION,
            ..Default::default()
        };
        for _ in 0..400 {
            unsafe { get(&mut sample) };
            if sample.static_findings > 0 {
                break;
            }
            unsafe { Sleep(50) };
        }
        expect!(checks, unsafe { flush() } == 0);
        let snapshot = checks.counters(get);
        let data = checks.read(path, "census.jsonl");
        let rows = checks.records(&data);
        expect!(
            checks,
            rows.first()
                .is_some_and(|row| row["type"] == "census_start")
        );
        let summary = checks.summary(path);
        let executable = exe_name();
        expect!(checks, executable.is_some());
        let executable = executable.as_deref().unwrap_or("");
        let image = executable_anchor(facts.anchor);
        expect!(checks, image.is_some());
        let (image_size, anchor_rva) = image.unwrap_or_default();
        fsr_records(&mut checks, &rows, executable, image_size);
        ngx_records(&mut checks, &rows, executable);
        other_records(&mut checks, &rows);
        check_summary(&mut checks, &summary, executable, &facts);
        if facts.had_dxgi != 0 {
            dxgi_records(&mut checks, &rows, &summary);
        }
        static_findings(&mut checks, &summary, anchor_rva);
        counters(&mut checks, &snapshot, &rows);
        expect!(checks, unsafe { shutdown() } == 0);
        expect!(checks, unsafe { after_shutdown() } == 0x1234_abcd);
    }
    println!("vendor_census functional: {} failure(s)", checks.0);
    i32::from(checks.0 != 0)
}
