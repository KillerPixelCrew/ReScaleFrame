// SPDX-License-Identifier: MIT
//! Drive resource-only recovery on the real shared-SR device fixture.
use super::abi::*;
use super::rsf_d3d11_host_fixture_cpu;
use rsf_game_sys::d3d11_chain_abi::RSF_D3D11_HOST_ABI_VERSION;
use rsf_game_sys::d3d11_host_abi::rsf_d3d11_auxiliary_pass;
use rsf_game_sys::d3d11_reduced_abi::*;
use rsf_game_sys::{
    RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_FRAME_END, RSF_GAME_RENDER_RECONFIGURE,
    rsf_game_host_services, rsf_game_render_config, rsf_game_render_pass,
};
use std::ffi::c_void;
use std::ptr::{dangling_mut, with_exposed_provenance, with_exposed_provenance_mut};

const OK: i32 = 0;
const INVALID: i32 = -1;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;

type Raw = *mut c_void;

struct Callbacks {
    present: unsafe extern "C" fn(Raw) -> i32,
    disable_sr: unsafe extern "C" fn(Raw) -> i32,
    settings_user: Raw,
}

fn require(ok: bool, check: &'static str) -> Result<(), &'static str> {
    ok.then_some(()).ok_or(check)
}

unsafe fn run_source(
    host: &rsf_game_host_services,
    context: Raw,
    chain: Raw,
    motion: Raw,
    mut raster: rsf_game_render_pass,
    source: u64,
    callbacks: &Callbacks,
) -> Result<(), &'static str> {
    let user = host.user;
    let process = host.process_upscale.ok_or("process_upscale callback")?;
    let config = host.render_config.ok_or("render_config callback")?;
    let recover = |packet: &mut rsf_d3d11_reduced_frame| unsafe {
        rsf_d3d11_host_reduced_frame(user, context, packet)
    };
    unsafe { rsf_d3d11_host_fixture_cpu(host, source, RSF_GAME_CPU_FRAME_BEGIN, 0) };
    raster.native_frame = source;
    raster.source_frame_id = source;
    raster.submission_id = source;
    raster.scope_id = source;
    raster.camera_valid = 0;
    raster.depth_domains = unsafe { std::mem::zeroed() };

    // Apply reactivation's requested generation before querying and reserving.
    let mut planning = raster;
    planning.flags |= RSF_GAME_RENDER_RECONFIGURE;
    require(
        unsafe { process(user, context, &planning) } == NOT_READY,
        "resource-only planning",
    )?;
    let mut plan = rsf_game_render_config {
        struct_size: size_of::<rsf_game_render_config>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    require(
        unsafe { config(user, &mut plan) } != 0 && plan.enabled != 0,
        "resource plan enabled",
    )?;
    raster.render_rect[2] = plan.render_width as i32;
    raster.render_rect[3] = plan.render_height as i32;
    raster.camera.render_width = plan.render_width;
    raster.camera.render_height = plan.render_height;
    let raw = rsf_d3d11_auxiliary_surface {
        struct_size: size_of::<rsf_d3d11_auxiliary_surface>() as u32,
        kind: RSF_D3D11_AUXILIARY_COLOUR,
        texture: motion,
        source_rect: raster.render_rect.map(|edge| edge as f32),
        output_rect: raster.output_rect,
        ..Default::default()
    };
    let mut recovery = rsf_d3d11_reduced_frame {
        struct_size: size_of::<rsf_d3d11_reduced_frame>() as u32,
        abi_version: RSF_D3D11_REDUCED_FRAME_ABI_VERSION,
        operation: RSF_D3D11_REDUCED_RESERVE,
        owner: &raster,
        surface_count: 1,
        surfaces: &raw,
        ..Default::default()
    };
    require(recover(&mut recovery) == OK, "reserve resource-only frame")?;

    let mut wrong_session = raster;
    wrong_session.session_id += 1;
    let mut resized = raster;
    resized.output_rect[2] -= 1;
    let mut changed_generation = raster;
    changed_generation.resource_generation += 1;
    let mut changed_view = changed_generation;
    changed_view.view_key += 1;
    for (owner, label) in [
        (&wrong_session, "foreign session refused"),
        (&resized, "changed output rectangle refused"),
        (&changed_generation, "changed resource generation refused"),
        (&changed_view, "changed view key refused"),
    ] {
        let mut refused = rsf_d3d11_reduced_frame {
            operation: RSF_D3D11_REDUCED_SEAL,
            owner,
            ..recovery
        };
        require(
            unsafe { rsf_d3d11_host_reduced_frame(user, dangling_mut(), &mut refused) }
                == NOT_READY,
            label,
        )?;
    }
    // Both the request and its borrowed descriptors stay live until join.
    let packet = (&raw const recovery).expose_provenance();
    let user_on_other_thread = user.expose_provenance();
    let other = std::thread::scope(|scope| {
        scope
            .spawn(move || {
                let mut request = rsf_d3d11_reduced_frame {
                    operation: RSF_D3D11_REDUCED_SEAL,
                    ..unsafe { *with_exposed_provenance::<rsf_d3d11_reduced_frame>(packet) }
                };
                unsafe {
                    rsf_d3d11_host_reduced_frame(
                        with_exposed_provenance_mut(user_on_other_thread),
                        dangling_mut(),
                        &mut request,
                    )
                }
            })
            .join()
    });
    require(
        matches!(other, Ok(NOT_READY)),
        "foreign graphics thread refused",
    )?;

    if source == 12 {
        recovery.operation = RSF_D3D11_REDUCED_CANCEL;
        require(
            recover(&mut recovery) == OK
                && recovery.result_flags & RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED != 0
                && recovery.result_flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED == 0,
            "cancel an unused reservation",
        )?;
        recovery.operation = RSF_D3D11_REDUCED_RESERVE;
        recovery.token = 0;
        require(
            recover(&mut recovery) == OK,
            "reserve again after cancellation",
        )?;
    }
    recovery.operation = RSF_D3D11_REDUCED_SEAL;
    require(recover(&mut recovery) == OK, "seal produced source")?;
    if source == 11 {
        require(
            unsafe { process(user, context, &raster) } == INVALID,
            "camera-less pass refused for temporal SR",
        )?;
    } else {
        require(
            unsafe { (callbacks.disable_sr)(callbacks.settings_user) } == OK,
            "disable SR after seal",
        )?;
        let mut temporal = raster;
        temporal.camera_valid = 1;
        let mut auxiliary = raw;
        auxiliary.jitter_pixels[0] = temporal.camera.jitter_pixels[0];
        let request = rsf_d3d11_auxiliary_pass {
            struct_size: size_of::<rsf_d3d11_auxiliary_pass>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            pass: &temporal,
            surface_count: 1,
            surfaces: &auxiliary,
        };
        require(
            unsafe { rsf_d3d11_host_upscale_auxiliaries(user, context, &request) } == NOT_READY,
            "temporal take cannot alter a frozen spatial repair",
        )?;
        temporal.flags |= RSF_GAME_RENDER_RECONFIGURE;
        require(
            unsafe { process(user, context, &temporal) } == BUSY,
            "planning blocked until the sealed transaction drains",
        )?;
    }
    recovery.operation = RSF_D3D11_REDUCED_FINISH;
    recovery.temporal_result = OK;
    recovery.result_flags = u32::MAX;
    require(
        recover(&mut recovery) == OK
            && recovery.result_flags & RSF_D3D11_REDUCED_SPATIAL_OUTPUT != 0
            && recovery.result_flags & RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED == 0
            && recovery.temporal_result == NOT_READY,
        "spatial completion replaces caller result fields",
    )?;
    require(
        unsafe { rsf_d3d11_host_set_active(user, 0) } == OK,
        "suspend before retirement",
    )?;
    recovery.operation = RSF_D3D11_REDUCED_RETIRE;
    require(
        recover(&mut recovery) == OK
            && recovery.result_flags & RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED != 0
            && unsafe { rsf_d3d11_host_set_active(user, 1) } == OK,
        "retire the suspended owner and reactivate",
    )?;
    require(
        unsafe { (callbacks.present)(chain) } >= 0,
        "Present recovery frame",
    )?;
    unsafe { rsf_d3d11_host_fixture_cpu(host, source, RSF_GAME_CPU_FRAME_END, 0) };
    println!("PASS no-camera/frozen configuration recovery source={source}");
    Ok(())
}

pub(super) unsafe fn recover(
    r: &rsf_d3d11_fixture_resources,
    api: &rsf_d3d11_fixture_sr_calls,
    base: &rsf_game_render_pass,
) -> Result<(), &'static str> {
    let host = unsafe { &*r.host };
    let callbacks = Callbacks {
        present: api.present.ok_or("Present callback")?,
        disable_sr: api.disable_sr.ok_or("disable SR callback")?,
        settings_user: api.user,
    };
    for source in 11..=12 {
        if let Err(check) = unsafe {
            run_source(
                host, r.context, r.chain, r.motion, *base, source, &callbacks,
            )
        } {
            eprintln!("FAIL resource-only recovery source={source}: {check}");
            return Err("resource-only recovery");
        }
    }
    Ok(())
}
