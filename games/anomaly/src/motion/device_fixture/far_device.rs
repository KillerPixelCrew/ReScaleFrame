// SPDX-License-Identifier: MIT
//! Far skinned characters: the G-buffer VS/PS pairs X-Ray draws a skinned model with on either side of
//! r__dtex_range. Uses the LOCAL deployment from tools/anomaly_motion_deploy.py (licensed shader bodies, never
//! tracked): RSF_ANOMALY_MOTION_DEPLOY, else <repo>/.local/anomaly/motion-deploy. Exits 77 without it,
//! without d3dcompiler_43 or without a hardware D3D11 device. Runs on the NVIDIA adapter (vendor 0x10de) when
//! present, with the debug layer when it is installed.
//!
//! Engine facts this covers (docs/research/anomaly-far-character-motion.md):
//! * CRender::rimp_select_sh_dynamic (xrRenderPC_R4/r4.cpp) picks SE_R2_NORMAL_HQ when distance minus the
//!   bounding radius is below r__dtex_range (50 m), else SE_R2_NORMAL_LQ. CBlender_deffer_model /
//!   CBlender_Model_EbB compile both with uber_deffer (xrRender/uber_deffer.cpp): HQ is deffer_model_bump-hq /
//!   _d-hq (or the tessellation chain), LQ is deffer_model_bump / deffer_model_flat with deffer_base_bump /
//!   _aref_bump / _flat / _aref_flat. A character beyond 50 m is therefore drawn ONLY through the LQ pairs.
//! * Every SKIN_n permutation of those roots is in the user's shader cache (appdata/shaders_cache/r4).
//!
//! What it proves, per pair and SKIN mode, through the deployed-tree loader (original reproduced byte for
//! byte), linkable(), decide_core, the b13 packers and the Rust motion owner:
//! 1. The variant pair links; the stock VS cannot feed the variant PS.
//! 2. A skinned mover whose previous palette (SKIN_1..4) or bone-composed previous world (SKIN_0) is a rigid
//!    motion of the current one, under a different previous camera: the motion target equals the CPU model at
//!    every covered pixel with validity 1, and the native MRT0/MRT1/depth are bit-identical to the stock pair.
//! 3. The far case: the same mover as a 0.6 x 1.8 m character 100 m away under a 10 degree (scope-like)
//!    perspective projection, moved 5 cm sideways: a few pixels of coverage and a sub-pixel vector. The vector
//!    must still match the CPU model to 0.02 px and every covered pixel must be valid.
//! 4. Missing history writes all-zero motion (validity 0) and leaves the native output unchanged.
use super::super::*;
use super::harness::{
    Built, ENGINE_COMPILER, FixtureTargets, Gpu, Half4, Keep, Motion, Outcome, Overrides, PIXELS,
    SIZE, Shaders, Signature, checks, define, deployment, engine_compiler, engine_defines, ensure,
    half, pixel_shader, report, uncovered, vertex_shader, zero,
};
use super::model::model_upload;
use super::scene::{
    apply, floats, identity_bones, invert, octahedron, perspective, rotation_x, rotation_y,
    translation, uv_motion,
};
use super::schedule::{FarCase, far_cases};
use crate::shader_loader::rsf_anomaly_shader_linkable;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;

/// Near: an octahedron under the identity (orthographic) camera, object = view = clip.
/// Far: a 0.6 x 1.8 x 0.6 m octahedron 100 m away under a 10 degree perspective (a scoped distant NPC).
struct Scene {
    name: &'static str,
    centre: [f64; 3],
    radius: [f64; 3],
    /// Current camera: view identity, this projection.
    projection: Mat4,
    /// Previous palette or bone-composed world relative to the identity current one.
    previous_bone: Mat4,
    previous_camera: CameraSample,
    tolerance_px: f64,
    min_covered: usize,
}

fn near_scene() -> Scene {
    let centre = [0., 0., 0.5];
    let to_origin = translation(-centre[0], -centre[1], -centre[2]);
    let back = translation(centre[0] + 0.04, centre[1] + 0.02, centre[2]);
    let mut projection = Mat4::IDENTITY;
    projection.0[0] = 1.1;
    projection.0[5] = 0.9;
    projection.0[3] = -0.01;
    Scene {
        name: "near",
        centre,
        radius: [0.6, 0.6, 0.3],
        projection: Mat4::IDENTITY,
        previous_bone: back.compose(
            &rotation_y(0.35)
                .compose(&rotation_x(-0.2))
                .compose(&to_origin),
        ),
        previous_camera: CameraSample {
            view: translation(0.03, -0.02, 0.),
            projection,
        },
        tolerance_px: 3e-4 * f64::from(SIZE),
        min_covered: 600,
    }
}

fn far_scene() -> Scene {
    let centre = [0., 0., 100.];
    let projection = perspective(10f64.to_radians(), 0.2, 1000.);
    // Walking: 5 cm sideways and a slight turn of the body since the previous frame; the camera turned a little.
    let to_origin = translation(-centre[0], -centre[1], -centre[2]);
    let back = translation(centre[0] - 0.05, centre[1], centre[2]);
    Scene {
        name: "far 100 m, 10 deg",
        centre,
        radius: [0.3, 0.9, 0.3],
        projection,
        previous_bone: back.compose(&rotation_y(0.1).compose(&to_origin)),
        previous_camera: CameraSample {
            view: rotation_y(0.0004),
            projection,
        },
        tolerance_px: 0.02,
        min_covered: 6,
    }
}

struct Shaderset {
    vs: Built,
    ps: Built,
    stock_vs: ID3D11VertexShader,
    motion_vs: ID3D11VertexShader,
    stock_ps: ID3D11PixelShader,
    motion_ps: ID3D11PixelShader,
}

struct Fixture {
    gpu: Gpu,
    shaders: Shaders,
}

fn make_shaders(fx: &Fixture, case: &FarCase) -> Outcome<Shaderset> {
    let mut vs_defines = engine_defines();
    vs_defines.push(define(format!("SKIN_{}", case.skin), "1"));
    let vs = fx.shaders.build(b'v', case.vertex, &vs_defines)?;
    let ps = fx.shaders.build(b'p', case.pixel, &engine_defines())?;
    ensure!(
        vs.prepared.skin == case.skin,
        "{}: VS skin mode",
        case.vertex
    );
    ensure!(
        vs.prepared.family == Some(CORE_MODEL),
        "{}: core_model family",
        case.vertex
    );
    let gpu = &fx.gpu;
    Ok(Shaderset {
        stock_vs: vertex_shader(gpu, &vs.stock, "stock VS")?,
        stock_ps: pixel_shader(gpu, &ps.stock, "stock PS")?,
        motion_vs: vertex_shader(gpu, &vs.prepared.variant, "motion VS")?,
        motion_ps: pixel_shader(gpu, &ps.prepared.variant, "motion PS")?,
        vs,
        ps,
    })
}

struct Mesh {
    layout: ID3D11InputLayout,
    vertices: ID3D11Buffer,
    stride: u32,
    count: u32,
}

fn make_mesh(fx: &Fixture, shaders: &Shaderset, scene: &Scene, skin: i32) -> Outcome<Mesh> {
    let signature = Signature::new(&shaders.vs.stock)?;
    let layout = fx.gpu.layout(&signature, &shaders.vs.stock)?;
    let (vertices, count) = octahedron(&fx.gpu, &signature, scene.centre, scene.radius, skin)?;
    Ok(Mesh {
        layout,
        vertices,
        stride: signature.stride(),
        count,
    })
}

struct Frame {
    position: Vec<Half4>,
    motion: Vec<Half4>,
    albedo: Vec<u32>,
    depth: Vec<f32>,
}

#[allow(clippy::too_many_arguments)]
fn draw(
    fx: &Fixture,
    case: &FarCase,
    scene: &Scene,
    shaders: &Shaderset,
    mesh: &Mesh,
    targets: &FixtureTargets,
    motion: Option<&mut Motion>,
    history: bool,
) -> Outcome<Frame> {
    let gpu = &fx.gpu;
    let mut keep = Keep::new();
    unsafe { gpu.context.ClearState() };
    let view = motion.as_ref().map(|motion| motion.view());
    let output = targets.output(view.as_ref().map_or(std::ptr::null_mut(), |view| view.rtv));
    gpu.clear(&output, false);
    let variant = motion.is_some();
    gpu.bind_input(&super::draw::Input {
        layout: mesh.layout.as_raw(),
        vertex_buffer: mesh.vertices.as_raw(),
        vs: if variant {
            shaders.motion_vs.as_raw()
        } else {
            shaders.stock_vs.as_raw()
        },
        hs: std::ptr::null_mut(),
        ds: std::ptr::null_mut(),
        ps: if variant {
            shaders.motion_ps.as_raw()
        } else {
            shaders.stock_ps.as_raw()
        },
        stride: mesh.stride,
        tessellated: false,
    });
    // Current palette: every bone identity (rows). Current camera: view identity, the scene projection (m_WVP).
    let overrides: Overrides = [
        ("sbones_array", identity_bones()),
        ("m_WVP", floats(&scene.projection)),
        ("dt_params", vec![1., 1., 0., 0.]),
    ]
    .into_iter()
    .collect();
    keep.bind_stage(gpu, b'v', false, &shaders.vs.stock, &overrides)?;
    keep.bind_stage(gpu, b'p', false, &shaders.ps.stock, &overrides)?;
    if let Some(motion) = motion {
        // The production policy, packers and Rust motion owner uploads, as the flush runs them for a swapped pair.
        let uploaded = model_upload(
            motion.owner(),
            gpu.context_raw(),
            &scene.previous_camera.view.0,
            &scene.previous_camera.projection.0,
            &scene.previous_bone.0,
            history,
            false,
            case.skin,
            SIZE,
        );
        ensure!(
            uploaded.is_ok(),
            "{}",
            uploaded
                .err()
                .map_or("model b13 uploads".into(), |e| e.to_string_lossy())
        );
    }
    gpu.bind_output(&output);
    gpu.draw(mesh.count);
    let frame = Frame {
        position: gpu.read(targets.position.as_raw())?,
        albedo: gpu.read(targets.albedo.as_raw())?,
        depth: gpu.read(targets.depth.as_raw())?,
        motion: match view {
            Some(view) => gpu.read(view.texture)?,
            None => Vec::new(),
        },
    };
    let stage = if variant {
        " motion draw"
    } else {
        " stock draw"
    };
    gpu.require_clean_debug_layer(&format!("{}+{}{stage}", case.vertex, case.pixel))?;
    Ok(frame)
}

/// Worst error of the pair against the CPU model, in pixels.
fn run_scene(fx: &Fixture, case: &FarCase, shaders: &Shaderset, scene: &Scene) -> Outcome<f64> {
    let name = format!(
        "{} {}+{} SKIN_{} {}",
        case.element, case.vertex, case.pixel, case.skin, scene.name
    );
    let mesh = make_mesh(fx, shaders, scene, case.skin)?;
    let targets = FixtureTargets::new(&fx.gpu, false)?;
    let mut motion = Motion::new(&fx.gpu, &targets)?;

    let stock = draw(fx, case, scene, shaders, &mesh, &targets, None, true)?;
    let moved = draw(
        fx,
        case,
        scene,
        shaders,
        &mesh,
        &targets,
        Some(&mut motion),
        true,
    )?;
    // Current clip -> object (the current palette is identity, view identity): inverse of the projection.
    // Previous clip of that object point: previous projection * previous view * previous bone.
    let to_object = invert(&scene.projection);
    let previous_clip_of = scene
        .previous_camera
        .projection
        .compose(&scene.previous_camera.view.compose(&scene.previous_bone));
    let size = f64::from(SIZE);
    let (mut covered, mut worst_px, mut largest_px) = (0, 0f64, 0f64);
    for y in 0..SIZE as usize {
        for x in 0..SIZE as usize {
            let i = y * SIZE as usize + x;
            ensure!(
                moved.position[i] == stock.position[i]
                    && moved.albedo[i] == stock.albedo[i]
                    && moved.depth[i].to_bits() == stock.depth[i].to_bits(),
                "{name}: native MRT0/MRT1/depth bit-identical at {i}"
            );
            let m = moved.motion[i];
            if uncovered(&stock.position[i]) {
                ensure!(zero(&m), "{name}: uncovered pixel stays cleared");
                continue;
            }
            covered += 1;
            let (u, v) = ((x as f64 + 0.5) / size, (y as f64 + 0.5) / size);
            let mut object = apply(
                &to_object,
                [u * 2. - 1., 1. - v * 2., f64::from(stock.depth[i]), 1.],
            );
            let w = object[3];
            for c in &mut object {
                *c /= w;
            }
            let (expected_x, expected_y) = uv_motion(u, v, apply(&previous_clip_of, object));
            let (mx, my) = (f64::from(half(m[0])), f64::from(half(m[1])));
            ensure!(half(m[2]) == 1. && m[3] == 0, "{name}: validity 1 at {i}");
            let error_px = (mx - expected_x).abs().max((my - expected_y).abs()) * size;
            worst_px = worst_px.max(error_px);
            largest_px = largest_px
                .max(expected_x.abs() * size)
                .max(expected_y.abs() * size);
            ensure!(
                error_px < scene.tolerance_px,
                "{name}: motion equals the CPU model at {i} (measured {},{} px, expected {},{} px)",
                mx * size,
                my * size,
                expected_x * size,
                expected_y * size
            );
        }
    }
    ensure!(covered >= scene.min_covered, "{name}: coverage {covered}");
    ensure!(
        largest_px > 4. * scene.tolerance_px,
        "{name}: the scene moves more than the tolerance"
    );

    let none = draw(
        fx,
        case,
        scene,
        shaders,
        &mesh,
        &targets,
        Some(&mut motion),
        false,
    )?;
    for i in 0..PIXELS {
        ensure!(zero(&none.motion[i]), "{name}: missing history is zero");
        ensure!(
            none.position[i] == stock.position[i],
            "{name}: native unchanged without history"
        );
    }
    println!(
        "  {name:<74} {covered:4} px covered, motion up to {largest_px:.3} px, worst error {worst_px:.4} px"
    );
    Ok(worst_px)
}

fn run_case(fx: &Fixture, case: &FarCase) -> Outcome<f64> {
    let shaders = make_shaders(fx, case)?;
    ensure!(
        rsf_anomaly_shader_linkable(
            shaders.vs.reflection()?.view(),
            shaders.ps.reflection()?.view()
        ),
        "{}+{}: the variant pair links",
        case.vertex,
        case.pixel
    );
    let stock = fx.shaders.reflect(&shaders.vs.stock)?;
    ensure!(
        !rsf_anomaly_shader_linkable(stock.view(), shaders.ps.reflection()?.view()),
        "{}: stock VS cannot feed the motion PS",
        case.vertex
    );
    run_scene(fx, case, &shaders, &near_scene())?;
    run_scene(fx, case, &shaders, &far_scene())
}

fn run() -> i32 {
    let deployment = deployment(&["motion-deploy"]);
    if !deployment.join("manifest.txt").exists() {
        println!(
            "SKIP: no local motion deployment at {} (run tools/anomaly_motion_deploy.py)",
            deployment.display()
        );
        return 77;
    }
    let manifest = match Shaders::manifest(&deployment) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("FAILED {error}");
            return 1;
        }
    };
    let Some((gpu, debug_layer)) = super::hardware(true) else {
        println!("SKIP: no hardware D3D11 device");
        return 77;
    };
    let Some(api) = engine_compiler(ENGINE_COMPILER) else {
        println!("SKIP: {ENGINE_COMPILER} not available");
        return 77;
    };
    println!("deployment: {}", deployment.display());
    let fx = Fixture {
        gpu,
        shaders: Shaders {
            api,
            manifest,
            deployment,
        },
    };
    let (mut pairs, mut lq, mut far_worst) = (0, 0, 0f64);
    for case in far_cases() {
        let mut worst = 0.;
        if !report(run_case(&fx, &case).map(|value| worst = value)) {
            return 1;
        }
        far_worst = far_worst.max(worst);
        lq += u32::from(case.element == "LQ");
        pairs += 1;
    }
    let debug = if debug_layer { "on, clean" } else { "off" };
    println!(
        "anomaly motion far device: {pairs} pairs ({lq} LQ), far worst error {far_worst:.4} px, {} checks passed (debug layer {debug})",
        checks()
    );
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_far_device_fixture() -> i32 {
    run()
}
