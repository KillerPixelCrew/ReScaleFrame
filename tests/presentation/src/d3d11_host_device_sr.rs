// SPDX-License-Identifier: MIT
//! The real shared-SR fixture's cold source and first nine planned sources.
use super::*;
use rsf_game_sys::RSF_D3D11_HOST_ABI_VERSION;
use rsf_game_sys::rsf_d3d11_auxiliary_pass;
use rsf_game_sys::rsf_dlss_pipeline_status;
use rsf_game_sys::{
    RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_FRAME_END, RSF_GAME_RENDER_AFTER_SIMULATION,
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RECONFIGURE,
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_game_render_config,
};
use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::ptr::dangling_mut;
use windows::core::PCWSTR;

const OK: i32 = 0;
const NOT_READY: i32 = -3;
const DLSS: u32 = 1;
// Result macros in the public dlss_pipeline.h.
const PIPELINE_INVALID_ARGUMENT: i32 = -1;
const PIPELINE_EVALUATE_FAILED: i32 = -10;

fn bundles(root: &Path) -> Result<Vec<PathBuf>, &'static str> {
    let mut bundles = Vec::new();
    if root.try_exists().map_err(|_| "probe capture root")? {
        for entry in fs::read_dir(root).map_err(|_| "list capture bundles")? {
            let entry = entry.map_err(|_| "read capture bundle entry")?;
            if entry.path().is_dir() {
                bundles.push(entry.path());
            }
        }
    }
    bundles.sort();
    Ok(bundles)
}

unsafe fn snapshot(r: &rsf_d3d11_fixture_resources) -> Result<Vec<u8>, &'static str> {
    let mut all = Vec::new();
    for (texture, stride) in [(r.colour, 4), (r.motion, 4), (r.depth, 4), (r.output, 8)] {
        let texture_ref = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) }.unwrap();
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture_ref.GetDesc(&mut desc) };
        let start = all.len();
        all.resize(
            start + desc.Width as usize * desc.Height as usize * stride as usize,
            0,
        );
        let bytes = &mut all[start..];
        require(
            unsafe {
                rsf_d3d11_host_fixture_read_texture(
                    r.device,
                    r.context,
                    texture,
                    stride,
                    bytes.as_mut_ptr(),
                    bytes.len(),
                )
            } != 0,
            "read cold texture snapshot",
        )?;
    }
    Ok(all)
}

unsafe fn run(
    r: &rsf_d3d11_fixture_resources,
    api: &rsf_d3d11_fixture_sr_calls,
    root: &Path,
    source: u64,
) -> Result<rsf_game_render_pass, &'static str> {
    let host = unsafe { &*r.host };
    let process = host.process_upscale.ok_or("SR callback")?;
    let config = host.render_config.ok_or("render config callback")?;
    let token = api.has_token.ok_or("token callback")?;
    let set_quality = api.set_quality.ok_or("quality callback")?;
    let status = api.sr_status.ok_or("overlay status callback")?;
    let present = api.present.ok_or("Present callback")?;
    let cancel = api.cancel_dump.ok_or("cancel capture callback")?;
    let evaluate = |pass: &rsf_game_render_pass| unsafe { process(host.user, r.context, pass) };
    let recover = |request: &mut rsf_d3d11_reduced_frame| unsafe {
        rsf_d3d11_host_reduced_frame(host.user, r.context, request)
    };
    let mut facts = rsf_native_cpu_frame {
        struct_size: size_of::<rsf_native_cpu_frame>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { rsf_d3d11_host_fixture_cpu(host, source, RSF_GAME_CPU_FRAME_BEGIN, 0) };
    let cpu_owned = unsafe { rsf_native_cpu_read(host.session_id, source, &mut facts) } != 0;
    let token_owned = unsafe { token(api.user, source) };
    require(
        if source == 1 {
            !cpu_owned && token_owned == 0
        } else {
            cpu_owned && facts.stage_mask == 1 && token_owned == 1
        },
        "cold/owned CPU-token admission",
    )?;
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&r.context) }.unwrap();
    let colour = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&r.colour_view) }.unwrap();
    let motion = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&r.motion_view) }.unwrap();
    let output = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&r.output_view) }.unwrap();
    let depth = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&r.depth_view) }.unwrap();
    unsafe {
        context.ClearRenderTargetView(colour, &SCENE_COLOUR);
        context.ClearRenderTargetView(motion, &[0.0; 4]);
        context.ClearRenderTargetView(output, &[3.0, 0.0, 3.0, 1.0]);
        context.ClearDepthStencilView(depth, (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL).0, 0.5, 0);
    }
    let mut pass = native_pass(r, host.session_id, source, 640, 360);
    pass.role = RSF_GAME_RENDER_SR;
    pass.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_NO_FG;
    pass.camera.jitter_pixels[0] = if source % 2 == 1 { 0.25 } else { -0.25 };
    if source == 4 {
        require(
            unsafe { set_quality(api.user, 0) } == OK,
            "switch Quality to Native",
        )?;
    }

    let failed_evaluation = source == 6;
    let before = bundles(root)?;
    if !failed_evaluation {
        require(
            unsafe { rsf_d3d11_host_request_capture(host.user) } == 0,
            "arm capture",
        )?;
    }
    let mut planning = pass;
    planning.flags |= RSF_GAME_RENDER_RECONFIGURE;
    require(
        evaluate(&planning) == NOT_READY && evaluate(&planning) == NOT_READY,
        "two unchanged plans",
    )?;
    if source > 1 {
        let mut no_camera = planning;
        no_camera.camera_valid = 0;
        require(evaluate(&no_camera) == NOT_READY, "camera-less plan")?;
        let mut cfg = rsf_game_render_config {
            struct_size: size_of::<rsf_game_render_config>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        require(
            unsafe { config(host.user, &mut cfg) } != 0 && cfg.enabled != 0,
            "ready render config",
        )?;
        pass.camera.render_width = cfg.render_width;
        pass.camera.render_height = cfg.render_height;
        pass.render_rect[2] = cfg.render_width as i32;
        pass.render_rect[3] = cfg.render_height as i32;
    }
    require(
        bundles(root)? == before,
        "planning preserved the capture request",
    )?;
    if failed_evaluation {
        pass.depth = null_mut();
    }
    if source == 9 {
        pass.flags |= RSF_GAME_RENDER_RESET;
    }

    let ticketed = matches!(source, 2 | 6);
    let mut raster = pass;
    raster.camera_valid = 0;
    raster.depth_domains = unsafe { std::mem::zeroed() };
    let raw = rsf_d3d11_auxiliary_surface {
        struct_size: size_of::<rsf_d3d11_auxiliary_surface>() as u32,
        kind: RSF_D3D11_AUXILIARY_COLOUR,
        texture: r.motion,
        source_rect: pass.render_rect.map(|edge| edge as f32),
        output_rect: pass.output_rect,
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
    if source == 1 {
        let original = unsafe { snapshot(r) }?;
        require(
            recover(&mut recovery) == NOT_READY
                && recovery.token == 0
                && recovery.result_flags == 0
                && unsafe { rsf_native_cpu_read(host.session_id, source, &mut facts) } == 0
                && unsafe { token(api.user, source) } == 0
                && unsafe { snapshot(r) }? == original,
            "cold RESERVE refused without retaining a token or mutating native contents",
        )?;
    }
    let result = if ticketed {
        require(
            recover(&mut recovery) == OK && recovery.token != 0,
            "reserve real reduced frame",
        )?;
        let mut wrong_token = recovery;
        wrong_token.operation = RSF_D3D11_REDUCED_SEAL;
        wrong_token.token += 1;
        require(
            unsafe { rsf_d3d11_host_reduced_frame(host.user, dangling_mut(), &mut wrong_token) }
                == NOT_READY,
            "wrong token refused before access",
        )?;
        recovery.operation = RSF_D3D11_REDUCED_SEAL;
        require(recover(&mut recovery) == OK, "seal live raster")?;
        let mut cancelled = recovery;
        cancelled.operation = RSF_D3D11_REDUCED_CANCEL;
        require(
            recover(&mut cancelled) == NOT_READY,
            "cannot cancel sealed frame",
        )?;
        let mut actual = raw;
        actual.jitter_pixels[0] = pass.camera.jitter_pixels[0];
        let temporal = rsf_d3d11_auxiliary_pass {
            struct_size: size_of::<rsf_d3d11_auxiliary_pass>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            pass: &pass,
            surface_count: 1,
            surfaces: &actual,
        };
        let result = unsafe { rsf_d3d11_host_upscale_auxiliaries(host.user, r.context, &temporal) };
        recovery.operation = RSF_D3D11_REDUCED_FINISH;
        let mode = if result == OK {
            RSF_D3D11_REDUCED_TEMPORAL_OUTPUT
        } else {
            RSF_D3D11_REDUCED_SPATIAL_OUTPUT
        };
        let required =
            RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED | RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED | mode;
        require(
            recover(&mut recovery) == OK
                && recovery.temporal_result == result
                && recovery.result_flags & required == required
                && recovery.result_flags & RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED == 0,
            "finish with the actual temporal receipt",
        )?;
        require(recover(&mut recovery) == OK, "repeat finish")?;
        recovery.operation = RSF_D3D11_REDUCED_RETIRE;
        require(
            recover(&mut recovery) == OK
                && recovery.result_flags & RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED != 0
                && recover(&mut recovery) == OK,
            "retire and repeat retirement",
        )?;
        println!(
            "PASS recovery source={source} result={result} flags={}",
            recovery.result_flags
        );
        result
    } else {
        evaluate(&pass)
    };

    println!("Shared SR source={source} result={result}");
    let selected: Vec<_> = bundles(root)?
        .into_iter()
        .filter(|path| !before.contains(path))
        .collect();
    require(selected.len() <= 1, "at most one new bundle")?;
    if source == 1 {
        let mut cold = rsf_dlss_pipeline_status {
            struct_size: size_of::<rsf_dlss_pipeline_status>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        require(
            result == PIPELINE_EVALUATE_FAILED
                && unsafe { rsf_dlss_pipeline_get_status(&mut cold) } == OK
                && cold.frames_evaluated == 0
                && cold.frames_refused == 1
                && unsafe { rsf_native_cpu_read(host.session_id, source, &mut facts) } == 0
                && unsafe { token(api.user, source) } == 0
                && selected
                    .first()
                    .is_some_and(|path| !path.join("sr_constants.json").exists()),
            "cold source refused before assembled backend submission",
        )?;
    } else if failed_evaluation {
        require(
            result == PIPELINE_INVALID_ARGUMENT && selected.is_empty(),
            "real missing-depth refusal",
        )?;
    } else {
        require(result == OK, "successful SR evaluation")?;
        let path = selected
            .first()
            .ok_or("assembled backend constants capture absent")?;
        let status = fs::read_to_string(path.join("sr_status.json"))
            .map_err(|_| "read captured SR status")?;
        require(
            status.contains(&format!("\"source_frame_id\":{source},"))
                && status.contains("\"result\":0"),
            "capture matched the evaluated source",
        )?;
        let frame =
            fs::read_to_string(path.join("frame.json")).map_err(|_| "read captured frame")?;
        require(
            frame.contains(&format!(
                "\"render\":[{},{}]",
                pass.camera.render_width, pass.camera.render_height
            )),
            "capture matched evaluated raster",
        )?;
        let constants = fs::read_to_string(path.join("sr_constants.json"))
            .map_err(|_| "read assembled constants")?;
        let reset = u8::from(matches!(source, 2 | 4 | 7 | 9));
        require(
            constants.contains(&format!("\"reset\":{reset}")),
            "actual provider reset matched source policy",
        )?;
        println!("Plan/evaluate source={source} actual backend reset={reset} verified");
    }
    if !failed_evaluation {
        require(
            unsafe { cancel(host.session_id) } == OK,
            "cancel fixture capture before Present",
        )?;
    }
    if source > 1 {
        planning = pass;
        planning.flags |= RSF_GAME_RENDER_RECONFIGURE;
        require(
            evaluate(&planning) == NOT_READY && evaluate(&planning) == NOT_READY,
            "plans after evaluation preserve report",
        )?;
    }
    let presented = unsafe { present(r.chain) };
    require(presented >= 0, "Present actual application chain")?;
    let (mut published, mut backend) = (0, 0);
    require(
        unsafe { status(api.user, &mut published, &mut backend) } == OK
            && published == result
            && backend == if result == OK { DLSS } else { 0 },
        "published report retains real evaluation",
    )?;
    if source == 1 {
        println!(
            "PASS cold source Present returned 0x{:08x}",
            presented as u32
        );
    }
    unsafe { rsf_d3d11_host_fixture_cpu(host, source, RSF_GAME_CPU_FRAME_END, 0) };
    if source == 10 {
        require(
            unsafe { set_quality(api.user, 1) } == OK
                && evaluate(&planning) == NOT_READY
                && evaluate(&planning) == NOT_READY
                && unsafe { present(r.chain) } >= 0
                && unsafe { status(api.user, &mut published, &mut backend) } == OK
                && published == NOT_READY
                && backend == 0,
            "changed plan invalidates stale successful output",
        )?;
    }
    Ok(pass)
}

unsafe fn run_scenario(
    r: &rsf_d3d11_fixture_resources,
    api: &rsf_d3d11_fixture_sr_calls,
    root: &Path,
) -> Result<i32, &'static str> {
    let mut last = None;
    for source in 1..=10 {
        last = Some(unsafe { run(r, api, root, source) }.inspect_err(|label| {
            eprintln!("FAIL shared-SR fixture source={source}: {label}");
        })?);
    }
    let mut before = rsf_dlss_pipeline_status {
        struct_size: size_of::<rsf_dlss_pipeline_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    require(
        unsafe { rsf_dlss_pipeline_get_status(&mut before) } == OK
            && before.frames_evaluated == 8
            && before.frames_refused == 1,
        "expected eight evaluations and one provider refusal after source 10",
    )?;
    unsafe { recovery::recover(r, api, &last.ok_or("missing source 10 pass")?) }?;

    let mut after = rsf_dlss_pipeline_status {
        struct_size: before.struct_size,
        ..unsafe { std::mem::zeroed() }
    };
    require(
        unsafe { rsf_dlss_pipeline_get_status(&mut after) } == OK
            && after.frames_evaluated == before.frames_evaluated
            && after.frames_refused == before.frames_refused,
        "spatial recovery must not change the SR evaluation or refusal counters",
    )?;
    let mut generation = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    require(
        unsafe { rsf_native_fg_status_get(&mut generation) } != 0
            && generation.requested.mode == RSF_FG_OFF
            && generation.vendor.effective_mode == RSF_FG_OFF
            && generation.tagged_frames == 0,
        "FG remains Off and no frames were tagged",
    )?;
    println!(
        "FG requested={} effective={} tagged={}",
        generation.requested.mode, generation.vendor.effective_mode, generation.tagged_frames
    );
    let mut pixel = [0u16; 4];
    require(
        unsafe {
            rsf_d3d11_host_fixture_read_pixel(
                r.device,
                r.context,
                r.output,
                r.width / 2,
                r.height / 2,
                size_of_val(&pixel) as u32,
                pixel.as_mut_ptr().cast(),
            )
        } != 0,
        "read actual SR output center pixel",
    )?;
    let decode = api.decode_half.ok_or("half decoder callback")?;
    let mut valid = true;
    for (index, (channel, expected)) in pixel.into_iter().zip(SCENE_COLOUR).take(3).enumerate() {
        let value = unsafe { decode(channel) };
        println!("Output channel{index}={value}");
        valid &= value.is_finite() && (value - expected).abs() < 0.15;
    }
    valid &= unsafe { finish_device(r) };
    println!(
        "{}",
        if valid {
            "Shared SR device check passed; FG remained Off"
        } else {
            "Shared SR device check FAILED"
        }
    );
    Ok(i32::from(!valid))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_shared_sr(
    resources: *const rsf_d3d11_fixture_resources,
    calls: *const rsf_d3d11_fixture_sr_calls,
    capture_root: *const u16,
) -> i32 {
    let root = PCWSTR(capture_root);
    let root = PathBuf::from(OsString::from_wide(unsafe { root.as_wide() }));
    unsafe { run_scenario(&*resources, &*calls, &root) }.unwrap_or_else(|label| {
        eprintln!("FAIL shared-SR fixture: {label}");
        1
    })
}
