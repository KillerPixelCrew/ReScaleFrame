// SPDX-License-Identifier: MIT
//! Drive actual FRAME, SUBMISSION, FG_INPUTS, SR, FINAL_SCENE and WINDOW leases.
use super::*;
use rsf_game_sys::RSF_D3D11_HOST_ABI_VERSION;
use rsf_game_sys::{
    RSF_D3D11_AUXILIARY_COLOUR, RSF_D3D11_AUXILIARY_RESTORE_CONTENTS, RSF_ERROR_BUSY,
    RSF_ERROR_INVALID_ARGUMENT, RSF_ERROR_NOT_READY, RSF_OK, rsf_d3d11_auxiliary_pass,
    rsf_d3d11_auxiliary_scope, rsf_d3d11_auxiliary_surface,
};
use rsf_game_sys::{
    RSF_DEPTH_DOMAINS_VERSION, RSF_FRAME_SOURCE_RENDERED, RSF_GAME_CPU_FRAME_BEGIN,
    RSF_GAME_CPU_FRAME_END, RSF_GAME_CPU_INPUT_SAMPLE, RSF_GAME_CPU_SIMULATION_END,
    RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_FINAL_SCENE,
    RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RESET,
    RSF_GAME_RENDER_SR, RSF_GAME_RENDER_SUBMISSION, RSF_GAME_RENDER_WINDOW, RSF_SCREEN_GAMEPLAY,
};
use std::ptr::{with_exposed_provenance, with_exposed_provenance_mut};
use windows::Win32::Foundation::DXGI_STATUS_OCCLUDED;

#[path = "d3d11_host_device_fg_phases.rs"]
mod phases;

#[derive(Default)]
struct State {
    source: u64,
    captured: u64,
    sr_evaluated: u64,
    app_presents: u64,
    failed: bool,
    unavailable: bool,
    occluded: bool,
}

struct Frame {
    reconstruct: bool,
    primary: bool,
    reset: bool,
    expect_capture: i32,
    scoped: bool,
    native_reinsertion: bool,
    active_width: u32,
    active_height: u32,
    native_pixel: u32,
    outside_marker: u32,
    cleanup: bool,
    refuse_inputs_late: bool,
    refuse_depth_late: bool,
}

impl State {
    fn verify(&mut self, ok: bool, label: &str) {
        if !ok {
            eprintln!("FAIL source={}: {label}", self.source);
            self.failed = true;
        }
    }
}

fn foreign_restore(user: Raw, cleanup: &rsf_d3d11_auxiliary_scope) -> bool {
    // The joined thread only borrows the scope and process-owned host.
    let scope = std::ptr::from_ref(cleanup).expose_provenance();
    let user = user.expose_provenance();
    let other = std::thread::spawn(move || unsafe {
        rsf_d3d11_host_restore_auxiliaries(
            with_exposed_provenance_mut(user),
            with_exposed_provenance(scope),
        )
    });
    matches!(other.join(), Ok(RSF_ERROR_NOT_READY))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_check_empty_aux(
    host: *const rsf_game_host_services,
    context: Raw,
) -> i32 {
    let host = unsafe { &*host };
    let cleanup = rsf_d3d11_auxiliary_scope {
        struct_size: size_of::<rsf_d3d11_auxiliary_scope>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        session_id: host.session_id,
        native_frame: 1,
        view_key: 1,
    };
    let restore = |scope: &rsf_d3d11_auxiliary_scope| unsafe {
        rsf_d3d11_host_restore_auxiliaries(host.user, scope)
    };
    let wrong_session = rsf_d3d11_auxiliary_scope {
        session_id: cleanup.session_id + 1,
        ..cleanup
    };
    let invalid = rsf_d3d11_auxiliary_pass {
        struct_size: size_of::<rsf_d3d11_auxiliary_pass>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        pass: std::ptr::null(),
        surface_count: 0,
        surfaces: std::ptr::null(),
    };
    let failure = if restore(&cleanup) != RSF_OK || restore(&cleanup) != RSF_OK {
        "verified owner acknowledges empty cleanup before preparation and on repeat"
    } else if restore(&wrong_session) != RSF_ERROR_NOT_READY {
        "empty cleanup refuses a foreign session"
    } else if !foreign_restore(host.user, &cleanup) {
        "empty cleanup refuses a foreign graphics thread"
    } else if unsafe { rsf_d3d11_host_upscale_auxiliaries(host.user, context, &invalid) }
        != RSF_ERROR_INVALID_ARGUMENT
        || restore(&cleanup) != RSF_OK
    {
        "failure before auxiliary preparation acknowledges no retained lease"
    } else {
        return 0;
    };
    eprintln!("FAIL: {failure}");
    1
}

unsafe fn check_cleanup(
    r: &rsf_d3d11_fixture_resources,
    device: &ID3D11Device,
    request: &rsf_d3d11_auxiliary_pass,
    result: i32,
    state: &mut State,
) {
    let host = unsafe { &*r.host };
    let cleanup = rsf_d3d11_auxiliary_scope {
        struct_size: size_of::<rsf_d3d11_auxiliary_scope>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        session_id: host.session_id,
        native_frame: state.source,
        view_key: 1,
    };
    let restore = |scope: &rsf_d3d11_auxiliary_scope| unsafe {
        rsf_d3d11_host_restore_auxiliaries(host.user, scope)
    };
    let upscale = |request: &rsf_d3d11_auxiliary_pass| unsafe {
        rsf_d3d11_host_upscale_auxiliaries(host.user, r.context, request)
    };
    state.verify(
        result == RSF_OK,
        "warmed auxiliary transaction commits before cleanup checks",
    );
    for (wrong, label) in [
        (
            rsf_d3d11_auxiliary_scope {
                native_frame: cleanup.native_frame + 1,
                ..cleanup
            },
            "pending cleanup refuses wrong frame",
        ),
        (
            rsf_d3d11_auxiliary_scope {
                view_key: cleanup.view_key + 1,
                ..cleanup
            },
            "pending cleanup refuses wrong view",
        ),
        (
            rsf_d3d11_auxiliary_scope {
                session_id: cleanup.session_id + 1,
                ..cleanup
            },
            "pending cleanup refuses foreign session",
        ),
    ] {
        state.verify(restore(&wrong) == RSF_ERROR_NOT_READY, label);
    }
    state.verify(
        foreign_restore(host.user, &cleanup),
        "pending cleanup refuses foreign thread",
    );
    state.verify(
        upscale(request) == RSF_ERROR_BUSY,
        "refused cleanup leaves the original pending transaction owned",
    );
    state.verify(
        restore(&cleanup) == RSF_OK && restore(&cleanup) == RSF_OK,
        "successful cleanup and empty repeat acknowledge OK",
    );
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&r.output) }.unwrap();
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.MiscFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut invalid_mirror = None;
    let created = unsafe { device.CreateTexture2D(&desc, None, Some(&mut invalid_mirror)) };
    state.verify(
        created.is_ok(),
        "create unsupported readable mirror for rollback",
    );
    let mut rollback_pass = unsafe { *request.pass };
    rollback_pass.color_output_readable = invalid_mirror
        .as_ref()
        .map_or(null_mut(), Interface::as_raw);
    let invalid = rsf_d3d11_auxiliary_pass {
        pass: &rollback_pass,
        ..*request
    };
    state.verify(
        upscale(&invalid) != RSF_OK,
        "second destination failure rolls back partially prepared auxiliary transaction",
    );
    state.verify(
        restore(&cleanup) == RSF_OK && restore(&cleanup) == RSF_OK,
        "rolled-back empty cleanup and repeat acknowledge OK",
    );
}

unsafe fn run(
    r: &rsf_d3d11_fixture_resources,
    api: &rsf_d3d11_fixture_fg_calls,
    frame: &Frame,
    state: &mut State,
) -> Result<(), &'static str> {
    let host = unsafe { &*r.host };
    let render = host.render_pass.ok_or("render callback")?;
    let upscale = host.process_upscale.ok_or("SR callback")?;
    let get_buffer = api.get_buffer.ok_or("application chain buffer callback")?;
    let present = api.present.ok_or("application chain Present callback")?;
    let has_provider_session = api
        .has_provider_session
        .ok_or("provider session callback")?;
    let backend = api.backend.ok_or("backend callback")?;
    let pump = api.pump_messages.ok_or("message dispatch callback")?;
    let lease =
        |pass: &rsf_game_render_pass, begin| unsafe { render(host.user, r.context, pass, begin) };
    let pair = |pass: &rsf_game_render_pass| {
        lease(pass, 1);
        lease(pass, 0);
    };
    let cpu = |source, stage| unsafe {
        rsf_d3d11_host_fixture_cpu(
            host,
            source,
            stage,
            u32::from(stage == RSF_GAME_CPU_FRAME_END),
        )
    };
    state.source += 1;
    let source = state.source;
    cpu(source, RSF_GAME_CPU_FRAME_BEGIN);
    cpu(source, RSF_GAME_CPU_SIMULATION_END);
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&r.context) }.unwrap();
    let device = unsafe { ID3D11Device::from_raw_borrowed(&r.device) }.unwrap();
    let colour = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&r.colour_view) }.unwrap();
    let motion = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&r.motion_view) }.unwrap();
    let depth = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&r.depth_view) }.unwrap();
    let reinsertion = frame.native_reinsertion;
    if reinsertion {
        let mut pixels = vec![frame.outside_marker; r.width as usize * r.height as usize];
        for row in pixels
            .chunks_exact_mut(r.width as usize)
            .take(frame.active_height as usize)
        {
            row[..frame.active_width as usize].fill(frame.native_pixel);
        }
        let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&r.colour) }.unwrap();
        unsafe {
            context.UpdateSubresource(texture, 0, None, pixels.as_ptr().cast(), r.width * 4, 0)
        };
    } else {
        unsafe { context.ClearRenderTargetView(colour, &SCENE_COLOUR) };
    }
    unsafe {
        context.ClearRenderTargetView(motion, &[0.0; 4]);
        context.ClearDepthStencilView(depth, D3D11_CLEAR_DEPTH.0, 0.5, 0);
    }

    let mut pass = native_pass(
        r,
        host.session_id,
        source,
        frame.active_width,
        frame.active_height,
    );
    pass.screen = RSF_SCREEN_GAMEPLAY;
    pass.flags = RSF_GAME_RENDER_AFTER_SIMULATION
        | if frame.primary {
            RSF_GAME_RENDER_PRIMARY
        } else {
            RSF_GAME_RENDER_NO_FG
        };
    if frame.reset {
        pass.flags |= RSF_GAME_RENDER_RESET;
    }
    if reinsertion {
        pass.color_output = r.colour;
    }
    pass.role = RSF_GAME_RENDER_FRAME;
    lease(&pass, 1);
    pass.role = RSF_GAME_RENDER_SUBMISSION;
    lease(&pass, 1);
    if frame.scoped {
        let domains = &mut pass.depth_domains;
        domains.struct_size = size_of_val(domains) as u32;
        domains.version = RSF_DEPTH_DOMAINS_VERSION;
        domains.domain_count = 1;
        let domain = &mut domains.domains[0];
        domain.classify_max = 1.0;
        domain.viewport_max_depth = 1.0;
        domain.endpoint_flags = 3;
        domain.viewport_xywh[2] = frame.active_width as f32;
        domain.viewport_xywh[3] = frame.active_height as f32;
        domain.source_clip_to_target_clip = identity();
    }
    pass.role = RSF_GAME_RENDER_FG_INPUTS;
    pair(&pass);
    let held = unsafe { rsf_native_fg_inputs_ready(source) } != 0;
    state.captured += u64::from(held);
    if frame.expect_capture > 0 && !held {
        match unsafe { has_provider_session(api.user) } {
            0 => state.unavailable = true,
            1 => state.verify(false, "requested native FG inputs were not retained"),
            _ => state.verify(false, "provider session callback raised an exception"),
        }
    }
    state.verify(
        frame.expect_capture != 0 || !held,
        "FG Off still retained pure FG inputs",
    );

    // Retain FG inputs before SR mutates the scene. SR metadata grants no FG eligibility.
    let mut sr_pass = pass;
    sr_pass.role = RSF_GAME_RENDER_SR;
    sr_pass.flags |= RSF_GAME_RENDER_NO_FG;
    if frame.scoped {
        sr_pass.depth_domains.domains[0].source_clip_to_target_clip[14] = 0.125;
    }
    let sr_result = if frame.cleanup {
        let auxiliary = rsf_d3d11_auxiliary_surface {
            struct_size: size_of::<rsf_d3d11_auxiliary_surface>() as u32,
            kind: RSF_D3D11_AUXILIARY_COLOUR,
            texture: r.motion,
            source_rect: pass.render_rect.map(|edge| edge as f32),
            output_rect: pass.output_rect,
            flags: RSF_D3D11_AUXILIARY_RESTORE_CONTENTS,
            ..Default::default()
        };
        let request = rsf_d3d11_auxiliary_pass {
            struct_size: size_of::<rsf_d3d11_auxiliary_pass>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            pass: &sr_pass,
            surface_count: 1,
            surfaces: &auxiliary,
        };
        let result = unsafe { rsf_d3d11_host_upscale_auxiliaries(host.user, r.context, &request) };
        unsafe { check_cleanup(r, device, &request, result, state) };
        result
    } else {
        unsafe { upscale(host.user, r.context, &sr_pass) }
    };
    if frame.reconstruct {
        state.sr_evaluated += u64::from(sr_result == RSF_OK);
        if reinsertion {
            state.verify(
                sr_result == RSF_OK,
                "warmed reduced SR must reinsert into native colour allocation",
            );
        }
    }
    if frame.refuse_depth_late {
        let mut malformed = pass;
        malformed.depth_domains.version += 1;
        pair(&malformed);
    }
    if frame.refuse_inputs_late {
        let mut refused = pass;
        refused.flags |= RSF_GAME_RENDER_NO_FG;
        pair(&refused);
        // An eligible duplicate cannot undo the real refusal.
        pair(&pass);
    }

    let mut backbuffer = null_mut();
    if unsafe { get_buffer(r.chain, &mut backbuffer) } < 0 || backbuffer.is_null() {
        return Err("GetBuffer for final scene");
    }
    let backbuffer = unsafe { ID3D11Texture2D::from_raw(backbuffer) };
    let mut target = None;
    unsafe { device.CreateRenderTargetView(&backbuffer, None, Some(&mut target)) }
        .map_err(|_| "CreateRenderTargetView for final scene")?;
    let target = target.ok_or("CreateRenderTargetView returned no view")?;
    unsafe { context.ClearRenderTargetView(&target, &SCENE_COLOUR) };
    pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    pass.scene_surface = backbuffer.as_raw();
    pair(&pass);
    pass.role = RSF_GAME_RENDER_SUBMISSION;
    lease(&pass, 0);
    pass.role = RSF_GAME_RENDER_WINDOW;
    pass.scene_surface = null_mut();
    lease(&pass, 1);
    let result = unsafe { present(r.chain) };
    state.app_presents += 1;
    if let Some(outer_counts) = api.outer_counts {
        let mut observed = rsf_native_present_record {
            struct_size: size_of::<rsf_native_present_record>() as u32,
            ..Default::default()
        };
        state.verify(
            unsafe { rsf_native_window_last_present(&mut observed) } != 0
                && observed.matched != 0
                && observed.source_frame_id == source,
            "actual inner Present matches the outer WINDOW canonical identity",
        );
        let (mut pre, mut post, mut dispatched) = (0, 0, 0);
        unsafe { outer_counts(api.user, &mut pre, &mut post, &mut dispatched) };
        state.verify(
            u64::from(pre) == state.app_presents + 1 && post == pre && dispatched == pre,
            "outer pre/post work survives every source and generation switch",
        );
    }
    if result < 0 {
        eprintln!("FAIL Present HRESULT=0x{:08x}", result as u32);
        state.failed = true;
    }
    state.occluded |= result == DXGI_STATUS_OCCLUDED.0;
    lease(&pass, 0);
    pass.role = RSF_GAME_RENDER_FRAME;
    lease(&pass, 0);
    cpu(source, RSF_GAME_CPU_FRAME_END);
    let mut cpu_frame = rsf_native_cpu_frame {
        struct_size: size_of::<rsf_native_cpu_frame>() as u32,
        ..Default::default()
    };
    let cpu_ok = unsafe { rsf_native_cpu_read(host.session_id, source, &mut cpu_frame) } != 0;
    state.verify(cpu_ok, "source CPU snapshot query");
    state.verify(
        cpu_frame.pacing_qpc == 0,
        "source CPU pacing QPC must remain zero",
    );
    state.verify(
        cpu_frame.input_ready_qpc == 0,
        "source CPU input-ready QPC must remain zero",
    );
    state.verify(
        cpu_frame.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE as usize] == 0,
        "source CPU physical input sample must remain unknown",
    );
    let mut fg = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    if unsafe { rsf_native_fg_status_get(&mut fg) } == 0 {
        return Err("per-source FG status query");
    }
    let backend = unsafe { backend(api.user) };
    state.verify(backend != u32::MAX, "backend callback raised an exception");
    println!(
        "source={source} SR={sr_result} held={} backend={backend} reason={} result={} tagged={} present_callbacks={} generated_callbacks={} statistics_flags=0x{:x} vendor_generated_presented={}",
        u32::from(held),
        fg.reason,
        fg.last_result,
        fg.tagged_frames,
        fg.vendor.present_callbacks,
        fg.vendor.generated_callbacks,
        fg.vendor.valid_statistics,
        fg.vendor.generated_presented
    );
    println!(
        "source_status={} provenance={} input_qpc={} source_qpc={} frequency={} requested_mode={} generated={} reflex={} limit_us={} effective_mode={} effective_reflex={} pacing_owner={} paced={}",
        fg.source_frame_id,
        fg.source_provenance,
        fg.input_qpc,
        fg.source_qpc,
        fg.source_qpc_frequency,
        fg.requested.mode,
        fg.requested.generated_frames,
        fg.requested.reflex_mode,
        fg.requested.frame_limit_us,
        fg.vendor.effective_mode,
        fg.vendor.effective_reflex,
        fg.vendor.pacing_owner,
        fg.vendor.latency_paced_frames
    );
    state.verify(
        fg.input_qpc == 0,
        "FG status physical input QPC must remain unknown",
    );
    state.verify(
        fg.vendor.latency_paced_frames == 0,
        "provider latency paced count must remain zero",
    );
    state.verify(
        fg.vendor.pacing_owner == RSF_PACING_NONE,
        "provider pacing owner must be NONE",
    );
    state.verify(
        fg.vendor.effective_reflex == RSF_REFLEX_OFF,
        "provider effective Reflex must be Off",
    );
    state.verify(
        fg.requested.reflex_mode == RSF_REFLEX_OFF,
        "requested Reflex must be Off",
    );
    state.verify(
        fg.requested.frame_limit_us == 0,
        "requested limiter must be zero",
    );
    if fg.source_frame_id == source && fg.tagged_frames != 0 {
        state.verify(
            fg.source_provenance == RSF_FRAME_SOURCE_RENDERED,
            "tagged current source must declare rendered provenance",
        );
        state.verify(
            fg.source_qpc != 0,
            "tagged current source must carry native source QPC",
        );
        state.verify(
            fg.source_qpc_frequency != 0,
            "tagged current source must carry source QPC frequency",
        );
    }
    if unsafe { pump() } != RSF_OK {
        return Err("native message callback raised an exception");
    }
    // Keep the exact backbuffer/view lease alive through WINDOW end and the receipts.
    drop((target, backbuffer));
    Ok(())
}
