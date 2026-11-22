// SPDX-License-Identifier: MIT
// Exercises plan construction and the D3D11 seed/finish path with stand-in
// textures. The game's rendering and the resulting frame require separate evidence.
mod d3d11;
use rsf_orchestrator::sys;

use d3d11::{Checks, Com, Handle, TextureDesc, create_device, create_texture, stage};
use crate::sys::{rsf_frame_tap_plan as Plan, rsf_frame_tap_substitution as Item};
use crate::sys::{
    RSF_PROMOTE_ABI_VERSION, rsf_promote_frame_tail as Tail, rsf_promote_setup as Setup,
    rsf_promote_status as Status,
};
use std::ffi::{CStr, c_char};
use std::ptr::null_mut;

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_SCALED: i32 = -4;
const B8G8R8A8_TYPELESS: u32 = 90;
const B8G8R8A8_UNORM_SRGB: u32 = 91;
const R8G8B8A8_TYPELESS: u32 = 27;
const R8G8B8A8_UNORM: u32 = 28;
const R16G16B16A16_FLOAT: u32 = 10;
const R11G11B10_FLOAT: u32 = 26;

unsafe extern "C" {
    fn rsf_promote_create(setup: *const Setup, out: *mut Handle) -> i32;
    fn rsf_promote_destroy(promote: Handle);
    fn rsf_promote_prepare(promote: Handle, tail: *const Tail) -> i32;
    fn rsf_promote_fill_plan(promote: Handle, plan: *mut Plan) -> i32;
    fn rsf_promote_get_status(promote: Handle, status: *mut Status) -> i32;
    fn rsf_promote_seed(promote: Handle, context: Handle, reconstructed: u32) -> i32;
    fn rsf_promote_finish(promote: Handle, context: Handle) -> i32;
}

struct Promotion(Handle);

impl Drop for Promotion {
    fn drop(&mut self) {
        unsafe { rsf_promote_destroy(self.0) }
    }
}

unsafe extern "C" fn collect(user: Handle, message: *const c_char) {
    let log = unsafe { &mut *user.cast::<Vec<String>>() };
    log.push(
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned(),
    );
}

fn logged(log: &[String], text: &str) -> bool {
    log.iter().any(|line| line.contains(text))
}

fn target(device: Handle, width: u32, height: u32, format: u32) -> Com {
    unsafe {
        create_texture(
            device,
            &TextureDesc {
                width,
                height,
                mip_levels: 1,
                array_size: 1,
                format,
                samples: 1,
                bind_flags: 0x28,
                ..TextureDesc::default()
            },
        )
    }
}

fn plan() -> Plan {
    let mut plan: Plan = unsafe { std::mem::zeroed() };
    plan.struct_size = size_of::<Plan>() as u32;
    plan
}

fn entries(plan: &Plan) -> &[Item] {
    &plan.items[..(plan.count as usize).min(plan.items.len())]
}

fn entry(plan: &Plan, texture: Handle) -> Option<&Item> {
    entries(plan).iter().find(|item| item.texture == texture)
}

fn check_promoted(checks: &mut Checks, plan: &Plan, texture: Handle, label: &str, gate: Handle) {
    let Some(item) = entry(plan, texture) else {
        checks.check(false, &format!("The plan must name the {label}."));
        return;
    };
    checks.check(
        !item.render_view.is_null() && !item.shader_view.is_null(),
        &format!("The {label} needs both views."),
    );
    checks.check(
        item.after_target == gate,
        &format!("The {label} must use its requested gate."),
    );
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_main() -> i32 {
    stage("validating arguments");
    let mut checks = Checks(true);
    let mut log: Vec<String> = Vec::new();
    let mut setup = Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: RSF_PROMOTE_ABI_VERSION,
        output_width: 512,
        output_height: 288,
        log: Some(collect),
        log_user: (&raw mut log).cast(),
        ..Setup::default()
    };
    let mut promotion = Promotion(null_mut());
    checks.check(
        unsafe { rsf_promote_create(&setup, &mut promotion.0) } == INVALID_ARGUMENT,
        "Creating without a device must be refused.",
    );

    stage("creating device");
    let Some((device, context)) = create_device(1) else {
        // D3D_DRIVER_TYPE_HARDWARE
        eprintln!("no D3D11 device available, skipping");
        return 0;
    };
    setup.device = device.0;
    setup.abi_version += 1;
    checks.check(
        unsafe { rsf_promote_create(&setup, &mut promotion.0) } == ABI_MISMATCH,
        "An incompatible ABI must be rejected.",
    );
    setup.abi_version = RSF_PROMOTE_ABI_VERSION;
    checks.check(
        unsafe { rsf_promote_create(&setup, &mut promotion.0) } == OK && !promotion.0.is_null(),
        "Creating must succeed.",
    );

    stage("creating a frame's tail");
    // Typeless pooled textures need typed replacement views. The composite carries an explicit
    // sRGB hint. The interface and chain exercise the default format selection.
    let composite = target(device.0, 256, 144, B8G8R8A8_TYPELESS);
    let layer_a = target(device.0, 256, 144, R8G8B8A8_TYPELESS);
    let layer_b = target(device.0, 256, 144, R8G8B8A8_UNORM);
    let chain = target(device.0, 256, 144, B8G8R8A8_TYPELESS);
    let scene_color = target(device.0, 256, 144, R16G16B16A16_FLOAT);
    let reconstruction = target(device.0, 512, 288, R16G16B16A16_FLOAT);
    if [
        &composite,
        &layer_a,
        &layer_b,
        &chain,
        &scene_color,
        &reconstruction,
    ]
    .iter()
    .any(|texture| texture.0.is_null())
    {
        checks.check(false, "The stand-in textures must be created.");
        return 1;
    }

    let mut tail = Tail {
        struct_size: size_of::<Tail>() as u32,
        composite: composite.0,
        ui_target_count: 2,
        chain_target_count: 1,
        scene_color: scene_color.0,
        reconstruction: reconstruction.0,
        render_width: 256,
        render_height: 144,
        composite_view_format: B8G8R8A8_UNORM_SRGB,
        ..Tail::default()
    };
    tail.ui_targets[..2].copy_from_slice(&[layer_a.0, layer_b.0]);
    tail.chain_targets[0] = chain.0;
    let promote = promotion.0;

    stage("native temporal reconstruction and oversize refusal");
    let mut unscaled = Tail {
        render_width: 512,
        render_height: 288,
        ..tail
    };
    checks.check(
        unsafe { rsf_promote_prepare(promote, &unscaled) } == OK,
        "Native/DLAA must still reinsert its temporal reconstruction at equal resolution.",
    );
    unscaled.render_width = 1024;
    unscaled.render_height = 576;
    checks.check(
        unsafe { rsf_promote_prepare(promote, &unscaled) } == NOT_SCALED,
        "An input larger than output must not silently select downsampling.",
    );
    checks.check(
        logged(&log, "nothing to promote"),
        "The oversize refusal must state its reason.",
    );

    stage("preparing");
    checks.check(
        unsafe { rsf_promote_prepare(promote, &tail) } == OK,
        "Preparing must succeed.",
    );
    let mut status = Status {
        struct_size: size_of::<Status>() as u32,
        ..Status::default()
    };
    checks.check(
        unsafe { rsf_promote_get_status(promote, &mut status) } == OK,
        "Status must be readable.",
    );
    checks.check(status.ready == 1, "Status must say it is ready.");
    checks.check(
        status.ui_targets_promoted == 2,
        "Both interface layers must be reported as promoted.",
    );
    checks.check(
        status.chain_targets_promoted == 1,
        "The chain target must be reported as promoted.",
    );
    checks.check(
        status.render_width == 256 && status.output_width == 512,
        "Status must carry both resolutions.",
    );

    stage("building the plan");
    let mut full = plan();
    checks.check(
        unsafe { rsf_promote_fill_plan(promote, &mut full) } == OK,
        "Filling the plan must succeed.",
    );
    checks.check(
        full.count == 5,
        "The plan must cover the composite, two interface layers, the chain and scene colour.",
    );
    checks.check(
        full.viewport_scale_x == 2.0 && full.viewport_scale_y == 2.0,
        "The viewport scale must be the ratio of the two resolutions.",
    );
    checks.check(
        entries(&full)
            .first()
            .is_some_and(|item| item.texture == composite.0),
        "The composite comes first: it is the gate scene colour waits on.",
    );
    checks.check(
        entries(&full)
            .last()
            .is_some_and(|item| item.texture == scene_color.0),
        "Scene colour comes last, after everything it is gated behind.",
    );

    check_promoted(&mut checks, &full, composite.0, "composite", composite.0);
    check_promoted(
        &mut checks,
        &full,
        layer_a.0,
        "first interface layer",
        null_mut(),
    );
    check_promoted(
        &mut checks,
        &full,
        layer_b.0,
        "second interface layer",
        null_mut(),
    );
    check_promoted(&mut checks, &full, chain.0, "chain target", composite.0);

    let scene_item = entry(&full, scene_color.0);
    checks.check(scene_item.is_some(), "The plan must name scene colour.");
    if let Some(item) = scene_item {
        checks.check(
            !item.shader_view.is_null() && item.render_view.is_null(),
            "Scene colour is read and never written by the substitution.",
        );
        checks.check(
            item.after_target == composite.0,
            "Scene colour must be gated on the composite.",
        );
    }

    stage("a layer offered twice is promoted once");
    let mut doubled = Tail {
        ui_target_count: 3,
        ..tail
    };
    doubled.ui_targets[2] = layer_a.0;
    checks.check(
        unsafe { rsf_promote_prepare(promote, &doubled) } == OK,
        "A duplicate in the set must not fail the whole tail.",
    );
    checks.check(
        unsafe { rsf_promote_get_status(promote, &mut status) } == OK
            && status.ui_targets_promoted == 2,
        "A layer named twice is one replacement.",
    );

    stage("preparing again without interface layers or a chain");
    log.clear();
    let bare = Tail {
        ui_target_count: 0,
        chain_target_count: 0,
        ..tail
    };
    checks.check(
        unsafe { rsf_promote_prepare(promote, &bare) } == OK,
        "An unidentified interface must still allow the scene to be promoted.",
    );
    checks.check(
        logged(&log, "magnified with the scene"),
        "Log the unpromoted interface.",
    );
    checks.check(logged(&log, "no chain target"), "Log the missing chain.");
    checks.check(
        unsafe { rsf_promote_get_status(promote, &mut status) } == OK
            && status.ui_targets_promoted == 0
            && status.chain_targets_promoted == 0,
        "Status must say nothing beyond the composite was promoted.",
    );

    let mut smaller = plan();
    checks.check(
        unsafe { rsf_promote_fill_plan(promote, &mut smaller) } == OK,
        "A plan without interface layers must still build.",
    );
    checks.check(
        smaller.count == 2,
        "That plan must cover the composite and scene colour only.",
    );
    checks.check(
        entry(&smaller, layer_a.0).is_none() && entry(&smaller, chain.0).is_none(),
        "It must not name a texture that was not identified.",
    );

    stage("the recombine route");
    let composed = target(device.0, 256, 144, R11G11B10_FLOAT);
    checks.check(
        !composed.0.is_null(),
        "The recombined stand-in must be created.",
    );
    let recombine = Tail {
        composed: composed.0,
        ..tail
    };
    checks.check(
        unsafe { rsf_promote_prepare(promote, &recombine) } == OK,
        "A tail naming the recombine must prepare.",
    );
    checks.check(
        unsafe { rsf_promote_get_status(promote, &mut status) } == OK && status.at_recombine == 1,
        "Status must say the reconstruction goes in at the recombine.",
    );
    let mut routed = plan();
    checks.check(
        unsafe { rsf_promote_fill_plan(promote, &mut routed) } == OK,
        "The recombine plan must build.",
    );
    check_promoted(
        &mut checks,
        &routed,
        composed.0,
        "recombined target",
        null_mut(),
    );
    check_promoted(
        &mut checks,
        &routed,
        scene_color.0,
        "scene colour",
        composed.0,
    );
    checks.check(
        entries(&routed)
            .iter()
            .any(|item| item.after_target == composite.0 && item.texture != scene_color.0),
        "The composite must open the finish gate before tonemap.",
    );
    checks.check(
        unsafe { rsf_promote_seed(promote, context.0, 1) } == OK,
        "Seeding scene colour with the reconstruction must succeed.",
    );
    checks.check(
        unsafe { rsf_promote_seed(promote, context.0, 0) } == OK,
        "Seeding the fallback with the game's scene colour must succeed.",
    );
    checks.check(
        unsafe { rsf_promote_finish(promote, context.0) } == OK,
        "Finishing the recombined result must succeed.",
    );
    checks.check(
        unsafe { rsf_promote_prepare(promote, &tail) } == OK
            && unsafe { rsf_promote_get_status(promote, &mut status) } == OK
            && status.at_recombine == 0,
        "A tail without the recombine must go back to the tonemap route.",
    );
    checks.check(
        unsafe { rsf_promote_seed(promote, context.0, 1) } == OK,
        "Seeding on the tonemap route must do nothing and succeed.",
    );

    stage("releasing");
    drop(promotion);
    checks.status("scene_promote")
}
