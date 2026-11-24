// SPDX-License-Identifier: MIT
//! Live AC7 device fixture cadence and assertions over the production presentation owners.
use crate::present_fixture_startup::validation_clean;
use crate::present_fixture_time::{frequency, milliseconds, stamp};
use crate::sys::rsf_native_cpu_frame;
use crate::sys::*;
use rsf_game_sys::backend_result::INVALID_ARGUMENT;
use std::time::Instant;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D12::ID3D12Device;
use windows::Win32::System::Threading::Sleep;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, MSG, PM_NOREMOVE, PeekMessageW, WM_MOUSEMOVE,
};
use windows::core::Interface;

const SESSION: u64 = 42;

fn event(id: u64, stage: u32) -> rsf_game_cpu_event {
    rsf_test_support::cpu_event(SESSION, id, stage, stamp(), frequency())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11_fixture_cpu_event(id: u64, stage: u32) {
    let event = event(id, stage);
    if unsafe { rsf_native_cpu_event(&event) } != 0 {
        unsafe { rsf_native_fg_cpu(&event) };
    }
}

fn document_input(run: &rsf_ac7_loop, window: HWND, id: u64) -> bool {
    let mut message = MSG::default();
    let _ = unsafe { PeekMessageW(&mut message, Some(window), 0, 0, PM_NOREMOVE) };
    rsf_d3d11_fixture_cpu_event(id, RSF_GAME_CPU_INPUT_SAMPLE);

    let mut input = event(id, RSF_GAME_CPU_INPUT_EVENT);
    input.input_kind = RSF_GAME_INPUT_MOUSE | RSF_GAME_INPUT_KEYBOARD;
    input.message_id = WM_MOUSEMOVE;
    if unsafe { rsf_native_cpu_event(&input) } == 0 {
        return false;
    }
    unsafe { rsf_native_fg_cpu(&input) };

    input.input_kind = RSF_GAME_INPUT_CONTROLLER;
    input.message_id = 0;
    if unsafe { rsf_native_cpu_event(&input) } == 0 {
        return false;
    }
    // The SDK controller marker is once per frame.
    unsafe {
        rsf_native_fg_cpu(&input);
        rsf_native_fg_cpu(&input);
    }

    let mut record: rsf_native_cpu_frame = unsafe { std::mem::zeroed() };
    record.struct_size = size_of::<rsf_native_cpu_frame>() as u32;
    (unsafe { (run.read_cpu.unwrap())(SESSION, id, &mut record) } != 0)
        && record.stage_mask == 3
        && record.input_mask == 7
        && record.input_events[..3] == [1, 1, 1]
}

fn latency() -> (i32, rsf_streamline_latency_status) {
    let mut status = rsf_streamline_latency_status {
        struct_size: size_of::<rsf_streamline_latency_status>() as u32,
        ..Default::default()
    };
    let result =
        unsafe { rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &mut status) };
    (result, status)
}

fn fg_status(run: &rsf_ac7_loop) -> Option<rsf_native_fg_status> {
    let mut status = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    (unsafe { (run.status.unwrap())(&mut status) } != 0).then_some(status)
}

fn token_live(id: u64) -> bool {
    !unsafe { rsf_streamline_host_token(rsf_d3d11_present_host(), id) }.is_null()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11_fixture_token_matches(id: u64, expected: u32) -> i32 {
    i32::from(token_live(id) == (expected != 0))
}

fn verify_retirement(run: &rsf_ac7_loop, frame_count: u64, pcl: u32, frame_limit_us: u32) -> bool {
    let probe = frame_count + 2;
    let host = unsafe { rsf_d3d11_present_host() };
    if pcl == 0
        || unsafe { rsf_streamline_host_begin(host, probe) } != 0
        || unsafe { rsf_streamline_host_input(host, probe, 0, pcl) } != 0
        || unsafe { rsf_streamline_host_abort(host, probe) } != 0
    {
        return false;
    }
    println!(
        "PASS: keyboard/mouse dequeue records, controller marker deduplication and SDK-registered PCL ping routing"
    );

    let unpresented = frame_count + 3;
    rsf_d3d11_fixture_cpu_event(unpresented, RSF_GAME_CPU_FRAME_BEGIN);
    rsf_d3d11_fixture_cpu_event(unpresented, RSF_GAME_CPU_PACING);
    for stage in RSF_GAME_CPU_INPUT_SAMPLE..=RSF_GAME_CPU_FRAME_END {
        rsf_d3d11_fixture_cpu_event(unpresented, stage);
    }
    unsafe {
        (*run.source).source_frame_id = unpresented;
        rsf_native_fg_submission(run.source, 1);
        rsf_native_fg_submission(run.source, 0);
        (*run.source).role = RSF_GAME_RENDER_WINDOW;
        (*run.source).swapchain = run.chain;
    }
    if !token_live(unpresented) {
        return false;
    }
    unsafe { rsf_native_fg_window_end(run.source) };
    if token_live(unpresented) {
        return false;
    }

    let abort_started = Instant::now();
    rsf_d3d11_fixture_cpu_event(unpresented + 1, RSF_GAME_CPU_FRAME_BEGIN);
    if abort_started.elapsed().as_millis() >= 500 || !token_live(unpresented + 1) {
        return false;
    }
    let start = stamp();
    rsf_d3d11_fixture_cpu_event(unpresented + 1, RSF_GAME_CPU_PACING);
    let released = milliseconds(start, frequency());
    rsf_d3d11_fixture_cpu_event(unpresented + 2, RSF_GAME_CPU_FRAME_BEGIN);
    let start = stamp();
    rsf_d3d11_fixture_cpu_event(unpresented + 2, RSF_GAME_CPU_PACING);
    let held = milliseconds(start, frequency());
    println!("pacing join: released {released:.2} ms, bounded wait {held:.2} ms");
    if frame_limit_us == 0 && (released >= 18.0 || !(18.0..200.0).contains(&held)) {
        return false;
    }
    for id in unpresented + 1..=unpresented + 2 {
        if unsafe { rsf_streamline_host_abort(host, id) } != 0 {
            return false;
        }
        rsf_d3d11_fixture_cpu_event(id, RSF_GAME_CPU_FRAME_END);
    }
    println!(
        "PASS: an abandoned window releases the next frame's sleep; a frame without completion holds it for one bounded wait"
    );
    true
}

#[derive(Default)]
struct Samples {
    sum: f64,
    frames: u32,
}
impl Samples {
    fn add(&mut self, sample: f64) {
        self.sum += sample;
        self.frames += 1;
    }
    fn mean(&self) -> f64 {
        if self.frames != 0 {
            self.sum / f64::from(self.frames)
        } else {
            0.0
        }
    }
}

fn request_switch(
    id: u64,
    provider: u32,
    options: &rsf_native_fg_options,
    activity: &rsf_native_fg_status,
    samples: &mut [u32; 6],
) -> bool {
    let vendor = &activity.vendor;
    println!(
        "adapter source={id} backend={provider} mode={} active={} result={} generated={} max={}",
        vendor.effective_mode,
        vendor.active,
        activity.last_result,
        vendor.effective_generated_frames,
        vendor.max_generated_frames
    );
    if vendor.active != 0 {
        let Some(count) = samples.get_mut(provider as usize) else {
            return false;
        };
        *count += 1;
        if vendor.effective_generated_frames
            != options.generated_frames.min(vendor.max_generated_frames)
        {
            println!(
                "multiplier mismatch source={id} effective={} requested={} max={}",
                vendor.effective_generated_frames,
                options.generated_frames,
                vendor.max_generated_frames
            );
            return false;
        }
    }
    let next = match id {
        13 | 53 => Some(RSF_FG_BACKEND_FSR3),
        25 | 65 => Some(RSF_FG_BACKEND_XESS),
        37 => Some(0),
        43 | 75 => Some(RSF_FG_BACKEND_DLSS),
        _ => None,
    };
    next.is_none_or(|next| unsafe { rsf_d3d11_present_request(next) } == 0)
        && activity.last_result == 0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_run_loop(run: &rsf_ac7_loop) -> i32 {
    let switching = run.switching != 0;
    let pacing = run.pacing != 0;
    let order_only = run.ordering_only != 0;
    let window = HWND(run.window);
    let (begin, draw, finish) = (
        run.begin.unwrap(),
        run.draw_present.unwrap(),
        run.finish.unwrap(),
    );
    let frame_count = if pacing {
        240
    } else if switching {
        120
    } else {
        16
    };
    let mut paths = [false; 2];
    let mut providers = [0; 6];
    let mut sleeps_expected = 1;
    let (mut active, mut inactive) = (Samples::default(), Samples::default());
    let qpc_frequency = frequency();
    let options = unsafe { &mut *run.options };

    for id in 2..2 + frame_count {
        let provider = unsafe { rsf_d3d11_present_backend() };
        let direct = id >= 2 + frame_count / 2;
        let (screen, reflex) = if id < 2 + frame_count / 3 {
            (RSF_SCREEN_FLIGHT, RSF_REFLEX_OFF)
        } else if id < 2 + 2 * frame_count / 3 {
            (RSF_SCREEN_HANGAR, RSF_REFLEX_ON)
        } else {
            (RSF_SCREEN_BRIEFING, RSF_REFLEX_BOOST)
        };
        options.reflex_mode = reflex;
        if switching {
            options.generated_frames = if id % 12 < 6 { 1 } else { 3 };
        }
        unsafe { rsf_native_fg_options_set(options) };
        let started = stamp();
        rsf_d3d11_fixture_cpu_event(id, RSF_GAME_CPU_FRAME_BEGIN);
        let (reserved_result, reserved) = latency();
        if reserved_result != 0 || reserved.sleep_calls != sleeps_expected {
            println!(
                "sleep count source={id} actual={} expected={sleeps_expected}",
                reserved.sleep_calls
            );
            return 1;
        }
        unsafe {
            (*run.source).screen = screen;
            (*run.source).source_frame_id = id;
            (*run.source).submission_id = id;
            (*run.source).native_frame = id;
            (*run.source).scope_id = id;
        }
        // Native BeginFrame can execute before the game thread's Reflex sleep.
        if unsafe { begin(run.user, run.source, u32::from(direct)) } == 0 {
            return 1;
        }
        rsf_d3d11_fixture_cpu_event(id, RSF_GAME_CPU_PACING);
        let Some(pacing_status) = fg_status(run) else {
            return 1;
        };
        if pacing_status.vendor.pacing_owner == RSF_PACING_REFLEX {
            sleeps_expected += 1;
        }
        let (paced_result, paced) = latency();
        if paced_result != 0
            || paced.sleep_calls != sleeps_expected
            || (!switching
                && unsafe { rsf_streamline_host_sleep(rsf_d3d11_present_host(), id) }
                    != INVALID_ARGUMENT)
            || !document_input(run, window, id)
        {
            return 1;
        }
        rsf_d3d11_fixture_cpu_event(id, RSF_GAME_CPU_SIMULATION_BEGIN);
        let late = id & 1 != 0;
        if !late {
            rsf_d3d11_fixture_cpu_event(id, RSF_GAME_CPU_SIMULATION_END);
        }
        let (mut evaluated, mut presented) = (0, 0);
        let passes = unsafe {
            draw(
                run.user,
                run.source,
                u32::from(direct),
                u32::from(late),
                &mut evaluated,
                &mut presented,
            )
        };
        if passes < 0 {
            return 1;
        }
        rsf_d3d11_fixture_cpu_event(id, RSF_GAME_CPU_FRAME_END);
        if passes == 0 || evaluated != 0 || presented < 0 || token_live(id) {
            let gpu = unsafe { ID3D12Device::from_raw_borrowed(&run.gpu) }.unwrap();
            let device_reason = unsafe { (gpu.vtable().GetDeviceRemovedReason)(run.gpu) };
            println!(
                "AC7 adapter failed source={id} SR={evaluated} Present={:08x} device={:08x} passes={passes}",
                presented as u32, device_reason.0 as u32
            );
            validation_clean(gpu);
            return 1;
        }
        let Some(activity) = fg_status(run) else {
            return 1;
        };
        let vendor = &activity.vendor;
        // A switched provider's pacing status describes the new owner, while eligibility belongs to the old frame.
        let expected_reflex = if vendor.effective_mode == RSF_FG_OFF || reflex != RSF_REFLEX_OFF {
            reflex
        } else {
            RSF_REFLEX_ON
        };
        if provider == RSF_FG_BACKEND_DLSS
            && unsafe { rsf_d3d11_present_backend() } == provider
            && vendor.effective_reflex != expected_reflex
        {
            println!(
                "Reflex mismatch source={id} effective={} requested={reflex} mode={}",
                vendor.effective_reflex, vendor.effective_mode
            );
            return 1;
        }
        if switching && !request_switch(id, provider, options, &activity, &mut providers) {
            return 1;
        }
        let enabled = vendor.active != 0;
        paths[direct as usize] |= enabled;
        let elapsed = milliseconds(started, qpc_frequency);
        if enabled {
            active.add(elapsed);
        } else {
            inactive.add(elapsed);
        }
        if !pacing {
            unsafe { Sleep(16) };
        }
    }

    println!(
        "frame limit: requested {} us between rendered frames; generating {} frames mean {:.2} ms, not generating {} frames mean {:.2} ms",
        options.frame_limit_us,
        active.frames,
        active.mean(),
        inactive.frames,
        inactive.mean()
    );
    if unsafe { finish(run.user) } == 0 {
        return 1;
    }
    let mut presenter = rsf_observer_status {
        struct_size: size_of::<rsf_observer_status>() as u32,
        ..Default::default()
    };
    if unsafe { rsf_observer_get_status(&mut presenter) } != 0
        || u64::from(presenter.frames_presented) != 12 + frame_count
    {
        return 1;
    }
    println!("PASS: application frame count matches source Presents");
    println!(
        "PASS: {frame_count} sampled/direct queued frames across flight/hangar/briefing; wrong source/viewport and missing sampled binding rejected; all CPU tokens retired"
    );
    println!(
        "present paths: sampled active={} direct active={}",
        paths[0] as u32, paths[1] as u32
    );
    if !order_only && !paths.into_iter().all(|active| active) {
        return 1;
    }
    let Some(observed) = fg_status(run) else {
        return 1;
    };
    let vendor = observed.vendor;
    println!(
        "synthetic enabled FG: tagged={} mode={} generated={} active={} aggregate presents={} result={} SDK status={}",
        observed.tagged_frames,
        vendor.effective_mode,
        vendor.effective_generated_frames,
        vendor.active,
        vendor.total_presented,
        observed.last_result,
        vendor.vendor_status
    );
    if observed.tagged_frames == 0
        || observed.last_result != 0
        || vendor.vendor_status != 0
        || (!order_only && !switching && vendor.active == 0)
    {
        return 1;
    }
    let (result, latency) = latency();
    println!(
        "latency sleeps={} markers={}",
        latency.sleep_calls, latency.marker_calls
    );
    if result != 0
        || latency.sleep_calls != sleeps_expected
        || latency.marker_calls != frame_count * 7
    {
        return 1;
    }
    if switching {
        let (dlss, fsr, xess) = (providers[1], providers[3], providers[5]);
        println!(
            "AC7 adapter active samples: DLSS={dlss} FSR3={fsr} XeSS={xess}; live multiplier requests 2x/4x clamp to SDK limit"
        );
        if fsr == 0 || xess == 0 {
            return 1;
        }
        if dlss == 0 {
            if run.visible != 0 && unsafe { GetForegroundWindow() } == window {
                return 1;
            }
            println!(
                "NOT RUN: DLSS activity acceptance requires a foreground game; this fixture was denied foreground ownership"
            );
        }
    }
    if !verify_retirement(
        run,
        frame_count,
        latency.pcl_message_id,
        options.frame_limit_us,
    ) {
        return 1;
    }
    options.mode = RSF_FG_OFF;
    unsafe { rsf_native_fg_options_set(options) };
    0
}
