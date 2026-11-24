// SPDX-License-Identifier: MIT
use super::{HEIGHT, Result, WIDTH, check, enabled, gpu::*};
use crate::{BUSY, sys::*};
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_INPUT_EVENT, RSF_GAME_CPU_INPUT_KEYED,
    RSF_GAME_CPU_WINDOW_MESSAGE, RSF_GAME_LATENCY_SLEEP, RSF_GAME_RENDER_FINAL_SCENE,
    RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_WINDOW, RSF_SCREEN_FLIGHT, rsf_game_cpu_event,
    rsf_game_host_services, rsf_game_latency_event, rsf_game_render_pass,
};
use rsf_test_support::Window;
use std::{ptr, thread, time::Duration};
use windows::{
    Win32::{
        Graphics::{Direct3D12::*, Dxgi::IDXGISwapChain3},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW, ShowCursor, WS_OVERLAPPEDWINDOW},
    },
    core::{Interface, PCWSTR, w},
};
use windows_sys::Win32::System::{
    Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    WindowsProgramming::WritePrivateProfileStringW,
};

fn fixture_window(class: PCWSTR, title: PCWSTR) -> Result<Window> {
    Ok(Window::new(
        class,
        title,
        WS_OVERLAPPEDWINDOW,
        WIDTH as i32,
        HEIGHT as i32,
    )?)
}

const RENDER_TARGET: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_RENDER_TARGET;
const PRESENT: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_PRESENT;

fn scene_pass(pass: &rsf_game_render_pass, image: &ID3D12Resource) -> rsf_game_render_pass {
    // The SDK pass is a borrowed record. The textures and camera are valid for this call.
    rsf_game_render_pass {
        role: RSF_GAME_RENDER_FINAL_SCENE,
        color_input: image.as_raw(),
        scene_surface: image.as_raw(),
        pass_key: 1,
        ..unsafe { ptr::read(pass) }
    }
}

pub(super) fn record_scene(
    gpu: &Gpu,
    host: &rsf_game_host_services,
    pass: &rsf_game_render_pass,
    output: &ID3D12Resource,
) {
    if let Some(render) = host.render_pass {
        gpu.barrier(output, COPY_DEST, RENDER_TARGET);
        let scene = scene_pass(pass, output);
        unsafe { render(host.user, gpu.list.as_raw(), &scene, 0) };
        gpu.barrier(output, RENDER_TARGET, COPY_DEST);
    }
}

pub(super) fn fill_slots(
    gpu: &Gpu,
    host: &rsf_game_host_services,
    pass: &mut rsf_game_render_pass,
) -> Result<()> {
    let Some(upscale) = host.process_upscale else {
        return Ok(());
    };
    for frame in 2..=3 {
        let mut settings = rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            ..Default::default()
        };
        check(
            unsafe { rsf_render_settings_get(&mut settings) } == 0,
            "read live sharpening",
        )?;
        settings.sharpening_mode = frame as u32;
        settings.sharpening_sharpness = if frame == 2 { 0.2 } else { 0.8 };
        check(
            unsafe { rsf_render_settings_apply(&settings) } == 0,
            "apply live sharpening",
        )?;
        eprintln!(
            "Live host sharpening mode {} applied without draining unsubmitted recordings.",
            settings.sharpening_mode
        );
        pass.native_frame = frame;
        check(
            unsafe { upscale(host.user, gpu.list.as_raw(), pass) } == 0,
            "host backpressure replay refused a free slot",
        )?;
    }
    pass.native_frame = 4;
    check(
        unsafe { upscale(host.user, gpu.list.as_raw(), pass) } == BUSY,
        "fourth unsubmitted recording did not return BUSY",
    )?;
    eprintln!(
        "PASS: three unsubmitted recordings retained; fourth refused without a producer wait."
    );
    Ok(())
}

pub(super) fn reuse_slot(
    gpu: &Gpu,
    host: &rsf_game_host_services,
    pass: &mut rsf_game_render_pass,
    output: &ID3D12Resource,
    readback: &Readback,
) -> Result<()> {
    let Some(upscale) = host.process_upscale else {
        return Ok(());
    };
    check(
        host.submit.is_some(),
        "missing host submission receipt callback",
    )?;
    gpu.reset()?;
    gpu.barrier(output, COPY_SOURCE, COPY_DEST);
    pass.native_frame = 5;
    check(
        unsafe { upscale(host.user, gpu.list.as_raw(), pass) } == 0,
        "retired SR recording slot was not reusable",
    )?;
    gpu.copy_to_readback(output, readback);
    gpu.submit(host, 2)?;
    gpu.wait(2)?;
    eprintln!("PASS: submitted recording slots retired and reused on a reset command list.");
    Ok(())
}

pub(super) fn native_present(gpu: &Gpu, host: &rsf_game_host_services) -> Result<()> {
    let Some(present) = host.present else {
        return Ok(());
    };
    let window = fixture_window(w!("RSFRDR2SRHostFixture"), w!("RSF fixture"))?;
    let chain = gpu.swapchain(window.handle, 2)?;
    unsafe {
        present(host.user, chain.as_raw(), 0, 0, 1);
        let presented = chain.Present(0, Default::default());
        present(host.user, chain.as_raw(), 0, 0, 0);
        presented.ok()?;
    }
    gpu.wait(3)?;
    drop(chain);
    eprintln!("PASS: host bookkeeping surrounds an actual DXGI Present.");
    Ok(())
}

pub(super) fn live_switches(
    gpu: &Gpu,
    host: &rsf_game_host_services,
    pass: &mut rsf_game_render_pass,
    output: &ID3D12Resource,
    readback: &Readback,
    configuration: &[u16],
) -> Result<()> {
    let Some(upscale) = host.process_upscale else {
        return Ok(());
    };
    let registration = unsafe { GetModuleHandleW(w!("sl.interposer.dll")) }?;
    for (completion, backend) in (4..).zip([RSF_SR_FSR3, RSF_SR_DLSS]) {
        let request: Vec<_> = backend.to_string().encode_utf16().chain([0]).collect();
        check(
            unsafe {
                WritePrivateProfileStringW(
                    w!("RDR2").as_ptr(),
                    w!("SRBackend").as_ptr(),
                    request.as_ptr(),
                    configuration.as_ptr(),
                ) != 0
            },
            "write fixture SRBackend override",
        )?;
        // Exercise the public live INI poll.
        thread::sleep(Duration::from_millis(1100));
        gpu.reset()?;
        gpu.barrier(output, COPY_SOURCE, COPY_DEST);
        pass.native_frame += 1;
        check(
            unsafe { upscale(host.user, gpu.list.as_raw(), pass) } == 0,
            "live backend switch refused",
        )?;
        check(
            unsafe { GetModuleHandleW(w!("sl.interposer.dll")) }? == registration,
            "NGX registration changed during switch",
        )?;
        gpu.copy_to_readback(output, readback);
        gpu.submit(host, completion)?;
        gpu.wait(completion)?;
        eprintln!(
            "PASS: backend {backend} accepted after a live switch; the same NGX registration remains loaded."
        );
    }
    Ok(())
}

fn timestamps() -> Result<(u64, u64)> {
    let (mut now, mut frequency) = (0, 0);
    check(
        unsafe {
            QueryPerformanceCounter(&mut now) != 0 && QueryPerformanceFrequency(&mut frequency) != 0
        },
        "read latency fixture timestamps",
    )?;
    Ok((now as u64, frequency as u64))
}

fn latency(
    host: &rsf_game_host_services,
    session: u64,
    source: u64,
    markers: &[u32],
) -> Result<()> {
    let call = host
        .process_latency
        .ok_or("missing host latency callback")?;
    for &marker in markers {
        let (timestamp_qpc, qpc_frequency) = timestamps()?;
        let event = rsf_game_latency_event {
            struct_size: size_of::<rsf_game_latency_event>() as u32,
            marker,
            session_id: session,
            source_frame_id: source,
            timestamp_qpc,
            qpc_frequency,
        };
        check(
            unsafe { call(host.user, &event) } == 0,
            "host latency marker refused",
        )?;
    }
    Ok(())
}

fn inputs(host: &rsf_game_host_services, session: u64, source: u64, window: &Window) -> Result<()> {
    let call = host.cpu_event.ok_or("missing host CPU callback")?;
    let (timestamp_qpc, qpc_frequency) = timestamps()?;
    let received = rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage: RSF_GAME_CPU_INPUT_EVENT,
        session_id: session,
        source_frame_id: source,
        timestamp_qpc,
        qpc_frequency,
        render_expected: 1,
        input_kind: 7,
        message_id: 0,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags: RSF_GAME_CPU_INPUT_KEYED,
    };
    unsafe {
        call(host.user, &received);
        call(host.user, &received);
        if rsf_native_latency_ping(window.handle.0) != 0 {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let dequeued = rsf_game_cpu_event {
                    input_kind: 0,
                    message_id: message.message,
                    flags: received.flags | RSF_GAME_CPU_WINDOW_MESSAGE,
                    ..received
                };
                call(host.user, &dequeued);
            }
        }
    }
    Ok(())
}

fn fg_status() -> Result<rsf_native_fg_status> {
    let mut status = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    check(
        unsafe { rsf_fg12_status(&mut status) } != 0,
        "missing FG status",
    )?;
    Ok(status)
}

fn overlay(ordinal: u64) -> Result<()> {
    if enabled("RSF_RDR2_TEST_OVERLAY") {
        if ordinal == 4 || ordinal == 20 {
            unsafe { rsf_render_overlay_toggle() };
        }
        if (4..20).contains(&ordinal) {
            check(
                unsafe { rsf_render_overlay_visible() } != 0,
                "fixture overlay did not become visible",
            )?;
            check(
                (0..512).any(|_| unsafe { ShowCursor(false) } < 0),
                "overlay cursor hide loop failed to terminate",
            )?;
        }
    }
    Ok(())
}

pub(super) fn cold_generation(
    gpu: &Gpu,
    host: &rsf_game_host_services,
    mut pass: rsf_game_render_pass,
) -> Result<()> {
    let window = fixture_window(w!("sgaWindow"), w!("RDR2 integration fixture"))?;
    // The RDR2 host intercepts this creation before its first SR registration.
    let created = gpu.swapchain(window.handle, 3)?;
    let chain: IDXGISwapChain3 = created.cast()?;
    check(
        fg_status()?.available != 0,
        "cold RDR2 generation unavailable",
    )?;
    let heap = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
        NumDescriptors: 1,
        ..Default::default()
    };
    let rtv: ID3D12DescriptorHeap = unsafe { gpu.device.CreateDescriptorHeap(&heap) }?;
    let completion: ID3D12Fence = unsafe { gpu.device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }?;
    let upscale = host
        .process_upscale
        .ok_or("missing host upscale callback")?;
    let render = host.render_pass.ok_or("missing host render callback")?;
    let present = host.present.ok_or("missing host Present callback")?;
    check(host.submit.is_some(), "missing host submission callback")?;
    pass.screen = RSF_SCREEN_FLIGHT;
    pass.flags = RSF_GAME_RENDER_PRIMARY;
    let session = pass.session_id;

    // Loading overflows the CPU bookkeeping ring, then an actual ordered Present retires it.
    for frame in 1..=140 {
        latency(
            host,
            session,
            frame,
            &[
                RSF_GAME_LATENCY_SLEEP,
                RSF_FIXTURE_UNITY_INPUT_SAMPLE as u32,
                RSF_FIXTURE_UNITY_SIM_START as u32,
                RSF_FIXTURE_UNITY_SIM_END as u32,
            ],
        )?;
    }
    unsafe { chain.Present(0, Default::default()) }.ok()?;
    latency(host, session, 140, &[RSF_FIXTURE_UNITY_PRESENT_END as u32])?;
    for ordinal in 0..40 {
        let frame = ordinal + 141;
        if ordinal != 0 {
            gpu.reset()?;
        }
        pass.native_frame = frame;
        pass.source_frame_id = frame;
        latency(
            host,
            session,
            frame,
            &[
                RSF_GAME_LATENCY_SLEEP,
                RSF_FIXTURE_UNITY_INPUT_SAMPLE as u32,
                RSF_FIXTURE_UNITY_SIM_START as u32,
            ],
        )?;
        inputs(host, session, frame, &window)?;
        latency(
            host,
            session,
            frame,
            &[
                RSF_FIXTURE_UNITY_SIM_END as u32,
                RSF_FIXTURE_UNITY_SUBMIT_START as u32,
            ],
        )?;
        check(
            unsafe { upscale(host.user, gpu.list.as_raw(), &pass) } == 0,
            "cold FG upscale refused",
        )?;

        let target: ID3D12Resource = unsafe { chain.GetBuffer(chain.GetCurrentBackBufferIndex()) }?;
        let handle = unsafe { rtv.GetCPUDescriptorHandleForHeapStart() };
        unsafe { gpu.device.CreateRenderTargetView(&target, None, handle) };
        gpu.barrier(&target, PRESENT, RENDER_TARGET);
        unsafe {
            gpu.list
                .ClearRenderTargetView(handle, &[0.25, 0.5, 0.75, 1.0], None)
        };
        let mut scene = scene_pass(&pass, &target);
        scene.swapchain = chain.as_raw();
        unsafe { render(host.user, gpu.list.as_raw(), &scene, 0) };
        gpu.barrier(&target, RENDER_TARGET, PRESENT);
        gpu.submit(host, frame)?;
        latency(
            host,
            session,
            frame,
            &[RSF_FIXTURE_UNITY_PRESENT_START as u32],
        )?;
        scene.role = RSF_GAME_RENDER_WINDOW;
        unsafe { render(host.user, ptr::null_mut(), &scene, 1) };
        overlay(ordinal)?;
        unsafe {
            present(host.user, chain.as_raw(), 0, 0, 1);
            let presented = chain.Present(0, Default::default());
            present(host.user, chain.as_raw(), 0, 0, 0);
            presented.ok()?;
        }
        latency(
            host,
            session,
            frame,
            &[
                RSF_FIXTURE_UNITY_PRESENT_END as u32,
                RSF_FIXTURE_UNITY_SUBMIT_END as u32,
            ],
        )?;
        wait(&gpu.queue, &completion, frame)?;
    }
    let status = fg_status()?;
    eprintln!(
        "RDR2 cold FG: available={} tagged={} mode={} reason={} result={}.",
        status.available,
        status.tagged_frames,
        status.vendor.configured_mode,
        status.reason,
        status.last_result,
    );
    let mut latency = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..Default::default()
    };
    check(
        unsafe { rsf_native_latency_status_get(&mut latency) } != 0,
        "missing core latency status",
    )?;
    let events = &latency.events;
    let [keyboard, mouse, controller] = events.input_events;
    eprintln!(
        "Core latency: owner={} paced={} kinds={keyboard}/{mouse}/{controller} pings={} order={} duplicates={}.",
        latency.owner,
        latency.paced_frames,
        events.pings,
        events.order_violations,
        events.duplicates,
    );
    check(
        events.input_events == [40; 3]
            && events.order_violations == 0
            && latency.paced_frames >= 180
            && (latency.pcl_available == 0 || events.pings != 0),
        "cold FG latency counters differed",
    )?;
    // The RDR2 host and the cold presentation interface live until process exit.
    std::mem::forget((chain, created, window));
    gpu.validate()?;
    check(
        status.available != 0
            && status.tagged_frames != 0
            && status.last_result == 0
            && status.vendor.configured_mode != RSF_FG_OFF,
        "cold FG output was not accepted",
    )
}
