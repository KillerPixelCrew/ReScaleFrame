// SPDX-License-Identifier: MIT
//! Tessellated motion chain device fixture. Uses the LOCAL deployment from tools/anomaly_motion_deploy.py
//! (licensed shader bodies, never tracked): RSF_ANOMALY_MOTION_DEPLOY, else
//! <repo>/.local/anomaly/motion-deploy-tess, else <repo>/.local/anomaly/motion-deploy. Exits 77 when no
//! deployment with the dx11/tess.hs and dx11/tess.ds roots exists, when d3dcompiler_43 is missing or when
//! there is no hardware D3D11 device. Runs on the NVIDIA adapter (vendor 0x10de) when present, with the debug
//! layer when it is installed.
//!
//! What it proves, for X-Ray's G-buffer tessellation pass (r4_enable_tessellation: VS -> DX11\tess.hs ->
//! DX11\tess.ds -> PS, uber_deffer.cpp r_TessPass):
//! 1. The four real roots (deffer_model_bump.vs SKIN_n, dx11/tess.hs, dx11/tess.ds, deffer_base_bump.ps) of
//!    every tessellation permutation in the user's shader cache go through the deployed-tree loader with the
//!    engine's compiler: original reproduced byte for byte, motion variant reflection-checked.
//! 2. Linkage: the all-variant chain links; a stock hull, a stock domain, a stock VS or the non-tessellation
//!    VS variant in the chain is refused.
//! 3. A skinned mover drawn through the chain: an octahedron whose vertex normals point outward, so PN
//!    tessellation (triLOD 8) bulges it towards a sphere. Its previous palette (SKIN_1..4) or bone-composed
//!    previous world (SKIN_0) is a rigid motion of the current one, the previous camera a different view and
//!    projection. PN construction, displacement along the normal and the edge rule are equivariant under rigid
//!    motion, so the exact previous clip position of every visible sample is P_prev * V_prev * B_prev * x, with
//!    x reconstructed from the pixel centre and the D32 depth. The motion target must equal that CPU model at
//!    every covered pixel with validity 1, the native MRT0/MRT1 and depth must be bit-identical to the stock
//!    chain, the debug layer must stay silent, and no history must write all-zero motion. The fixture also
//!    requires a flat-triangle previous position to be far from the measured vectors, so a chain that ignored
//!    the patch could not pass.
//!
//! Diagnostics (environment): RSF_TESS_HM=1 adds the TESS_HM cases; RSF_TESS_DIAG=1 lists misses instead of
//! stopping; RSF_TESS_IDENTITY=1 makes the previous state equal the current one (all motion must be zero);
//! RSF_TESS_DUMP=<dir> writes the stock and motion hull/domain bytecode; RSF_TESS_CREATE=stock|motion[-vhdp]
//! only creates one shader set of the HM case; RSF_TESS_RAW=<file.hs.*|file.ds.*> creates one dumped shader on
//! a fresh device; RSF_TESS_NODEBUG=1 skips the debug layer.
use super::super::*;
use super::harness::{
    Built, ENGINE_COMPILER, FixtureTargets, Gpu, Half4, Keep, Motion, Outcome, Overrides, PIXELS,
    SIZE, Shaders, Signature, checks, define, deployment, domain_shader, engine_compiler,
    engine_defines, ensure, file_bytes, half, hull_shader, pixel_shader, uncovered, vertex_shader,
    zero,
};
use super::model::model_upload;
use super::scene::{
    apply, identity_bones, octahedron, rotation_x, rotation_y, translation, uv_motion,
};
use super::schedule::{TessCase, hm_cases, tess_cases};
use crate::shader_family::{TESS_DOMAIN, TESS_HULL};
use crate::shader_loader::{
    Reflection, rsf_anomaly_shader_linkable, rsf_anomaly_shader_tess_linkable,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameA, GetModuleHandleExA,
};

fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

struct Fixture {
    gpu: Gpu,
    shaders: Shaders,
}

fn common_defines(case: &TessCase) -> Vec<(String, String)> {
    let mut defines = engine_defines();
    defines.extend(case.defines.iter().map(|name| define(*name, "1")));
    defines
}

fn skin_defines(mut defines: Vec<(String, String)>, skin: i32) -> Vec<(String, String)> {
    defines.push(define(format!("SKIN_{skin}"), "1"));
    defines
}

/// The four roots of one tessellation permutation.
fn build_chain(fx: &Fixture, case: &TessCase) -> Outcome<[Built; 4]> {
    let common = common_defines(case);
    let vertex = skin_defines(common.clone(), case.skin);
    Ok([
        fx.shaders.build(b'v', "deffer_model_bump.vs", &vertex)?,
        fx.shaders.build(b'h', "dx11/tess.hs", &common)?,
        fx.shaders.build(b'd', "dx11/tess.ds", &common)?,
        fx.shaders.build(b'p', "deffer_base_bump.ps", &common)?,
    ])
}

struct Chain {
    vs: Built,
    hs: Built,
    ds: Built,
    ps: Built,
    stock_vs: ID3D11VertexShader,
    motion_vs: ID3D11VertexShader,
    stock_hs: ID3D11HullShader,
    motion_hs: ID3D11HullShader,
    stock_ds: ID3D11DomainShader,
    motion_ds: ID3D11DomainShader,
    stock_ps: ID3D11PixelShader,
    motion_ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    vertices: ID3D11Buffer,
    stride: u32,
    count: u32,
}

// An octahedron (radius 0.6 in x/y, 0.3 in z, centre z 0.5) with outward vertex normals, 8 patches of 3
// control points. Ortho identity camera: object = view = clip; visible x, y in [-1, 1], depth = z.
const CENTRE: [f64; 3] = [0., 0., 0.5];
const RADIUS_XY: f64 = 0.6;
const RADIUS_Z: f64 = 0.3;

fn make_chain(fx: &Fixture, case: &TessCase) -> Outcome<Chain> {
    let [vs, hs, ds, ps] = build_chain(fx, case)?;
    ensure!(vs.prepared.skin == case.skin, "VS skin mode");
    if let Some(dump) = std::env::var_os("RSF_TESS_DUMP") {
        for built in [&hs, &ds] {
            let stage = if built.stage == b'h' { "hs" } else { "ds" };
            let tag = std::path::Path::new(&dump).join(format!("{}.{stage}", case.name));
            let _ = std::fs::write(
                tag.with_extension(format!("{stage}.stock.dxbc")),
                &built.stock,
            );
            let _ = std::fs::write(
                tag.with_extension(format!("{stage}.motion.dxbc")),
                &built.prepared.variant,
            );
        }
    }
    ensure!(
        hs.prepared.family == Some(TESS_HULL) && ds.prepared.family == Some(TESS_DOMAIN),
        "hull/domain families from the manifest"
    );
    let gpu = &fx.gpu;
    let signature = Signature::new(&vs.stock)?;
    let layout = gpu.layout(&signature, &vs.stock)?;
    let (vertices, count) = octahedron(
        gpu,
        &signature,
        CENTRE,
        [RADIUS_XY, RADIUS_XY, RADIUS_Z],
        case.skin,
    )?;
    Ok(Chain {
        stock_vs: vertex_shader(gpu, &vs.stock, "stock VS")?,
        stock_hs: hull_shader(gpu, &hs.stock, "stock HS")?,
        stock_ds: domain_shader(gpu, &ds.stock, "stock DS")?,
        stock_ps: pixel_shader(gpu, &ps.stock, "stock PS")?,
        motion_vs: vertex_shader(gpu, &vs.prepared.variant, "motion VS")?,
        motion_hs: hull_shader(gpu, &hs.prepared.variant, "motion HS")?,
        motion_ds: domain_shader(gpu, &ds.prepared.variant, "motion DS")?,
        motion_ps: pixel_shader(gpu, &ps.prepared.variant, "motion PS")?,
        stride: signature.stride(),
        layout,
        vertices,
        count,
        vs,
        hs,
        ds,
        ps,
    })
}

/// Diagnostics (RSF_TESS_CREATE=stock|motion[-vhdp]): only create one shader set of a case.
fn create_probe(fx: &Fixture, case: &TessCase, which: &str) -> Outcome {
    let built = build_chain(fx, case)?;
    let (set, only) = which.split_once('-').unwrap_or((which, "vhdp"));
    let gpu = &fx.gpu;
    for b in &built {
        if !only.as_bytes().contains(&b.stage) {
            continue;
        }
        let code = if set == "stock" {
            &b.stock
        } else {
            &b.prepared.variant
        };
        println!(
            "    create {which} {} ({} bytes)",
            b.stage as char,
            code.len()
        );
        match b.stage {
            b'v' => drop(vertex_shader(gpu, code, "VS")?),
            b'h' => drop(hull_shader(gpu, code, "HS")?),
            b'd' => drop(domain_shader(gpu, code, "DS")?),
            _ => drop(pixel_shader(gpu, code, "PS")?),
        }
    }
    unsafe { gpu.context.Flush() };
    // Let deferred driver compilation finish inside the process.
    std::thread::sleep(std::time::Duration::from_millis(500));
    println!("    created {which} set of {}", case.name);
    Ok(())
}

fn view(built: &Built) -> Outcome<&Reflection> {
    Ok(built.reflection()?.view())
}

/// Linkage rules on the real reflections.
fn linkage(fx: &Fixture, case: &TessCase, chain: &Chain) -> Outcome {
    let (vs, hs, ds, ps) = (
        view(&chain.vs)?,
        view(&chain.hs)?,
        view(&chain.ds)?,
        view(&chain.ps)?,
    );
    ensure!(
        rsf_anomaly_shader_tess_linkable(vs, hs, ds, ps),
        "{}: the variant chain links",
        case.name
    );
    let stock_hs = fx.shaders.reflect(&chain.hs.stock)?;
    ensure!(
        !rsf_anomaly_shader_tess_linkable(vs, stock_hs.view(), ds, ps),
        "stock hull refused (it drops the RSF control data)"
    );
    let stock_ds = fx.shaders.reflect(&chain.ds.stock)?;
    ensure!(
        !rsf_anomaly_shader_tess_linkable(vs, hs, stock_ds.view(), ps),
        "stock domain refused (the PS reads TEXCOORD12/13)"
    );
    let stock_vs = fx.shaders.reflect(&chain.vs.stock)?;
    ensure!(
        !rsf_anomaly_shader_tess_linkable(stock_vs.view(), hs, ds, ps),
        "stock VS refused (no previous control data)"
    );
    // The VS variant of the same root WITHOUT the tessellation defines (the far, non-tessellated element) does
    // not write TEXCOORD14/15 and must not feed the hull stage.
    let plain = fx.shaders.build(
        b'v',
        "deffer_model_bump.vs",
        &skin_defines(engine_defines(), case.skin),
    )?;
    ensure!(
        !rsf_anomaly_shader_tess_linkable(view(&plain)?, hs, ds, ps),
        "non-tessellation VS variant refused in the chain"
    );
    // The stock domain shader cannot feed the motion PS (what the stage used to do with the VS/PS pair under
    // HS/DS).
    ensure!(
        !rsf_anomaly_shader_linkable(stock_ds.view(), ps),
        "stock domain cannot feed the motion PS"
    );
    Ok(())
}

/// The current palette is identity (bone 0); the previous one a rigid motion about the mover's centre.
fn previous_bone() -> Mat4 {
    if flag("RSF_TESS_IDENTITY") {
        return Mat4::IDENTITY;
    }
    let to_origin = translation(-CENTRE[0], -CENTRE[1], -CENTRE[2]);
    let back = translation(CENTRE[0] + 0.04, CENTRE[1] + 0.02, CENTRE[2]);
    back.compose(
        &rotation_y(0.35)
            .compose(&rotation_x(-0.2))
            .compose(&to_origin),
    )
}

fn previous_camera() -> CameraSample {
    if flag("RSF_TESS_IDENTITY") {
        return CameraSample {
            view: Mat4::IDENTITY,
            projection: Mat4::IDENTITY,
        };
    }
    // The camera moved, and its (affine) projection differs: the DS must use it.
    let mut projection = Mat4::IDENTITY;
    projection.0[0] = 1.1;
    projection.0[5] = 0.9;
    projection.0[3] = -0.01;
    CameraSample {
        view: translation(0.03, -0.02, 0.),
        projection,
    }
}

struct Frame {
    position: Vec<Half4>,
    motion: Vec<Half4>,
    albedo: Vec<u32>,
    depth: Vec<f32>,
}

fn draw(
    fx: &Fixture,
    case: &TessCase,
    chain: &Chain,
    targets: &FixtureTargets,
    motion: Option<&mut Motion>,
    history: bool,
) -> Outcome<Frame> {
    let gpu = &fx.gpu;
    let variant = motion.is_some();
    let mut keep = Keep::new();
    unsafe { gpu.context.ClearState() };
    let view = motion.as_ref().map(|motion| motion.view());
    let output = targets.output(view.as_ref().map_or(std::ptr::null_mut(), |view| view.rtv));
    gpu.clear(&output, false);
    gpu.bind_input(&super::draw::Input {
        layout: chain.layout.as_raw(),
        vertex_buffer: chain.vertices.as_raw(),
        vs: if variant {
            chain.motion_vs.as_raw()
        } else {
            chain.stock_vs.as_raw()
        },
        hs: if variant {
            chain.motion_hs.as_raw()
        } else {
            chain.stock_hs.as_raw()
        },
        ds: if variant {
            chain.motion_ds.as_raw()
        } else {
            chain.stock_ds.as_raw()
        },
        ps: if variant {
            chain.motion_ps.as_raw()
        } else {
            chain.stock_ps.as_raw()
        },
        stride: chain.stride,
        tessellated: true,
    });
    // Current palette: bone 0 identity (rows), every other row identity too (SKIN_2 reads rows 0 and 3).
    let overrides: Overrides = [
        ("triLOD", vec![8.]),
        ("sbones_array", identity_bones()),
        ("dt_params", vec![1., 1., 0., 0.]),
    ]
    .into_iter()
    .collect();
    for built in [&chain.vs, &chain.hs, &chain.ds, &chain.ps] {
        keep.bind_stage(gpu, built.stage, true, &built.stock, &overrides)?;
    }
    if let Some(motion) = motion {
        // The production policy, packers and Rust motion owner uploads, as the flush runs them for a swapped
        // chain. SKIN_0 (CSkeletonX RM_SINGLE): the engine composes the single bone into the world matrix, so the
        // previous bone-composed world carries the motion and there is no palette.
        let camera = previous_camera();
        let bone = previous_bone();
        let uploaded = model_upload(
            motion.owner(),
            gpu.context_raw(),
            &camera.view.0,
            &camera.projection.0,
            &bone.0,
            history,
            true,
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
    gpu.draw(chain.count);
    let frame = Frame {
        position: gpu.read(targets.position.as_raw())?,
        albedo: gpu.read(targets.albedo.as_raw())?,
        depth: gpu.read(targets.depth.as_raw())?,
        motion: match view {
            Some(view) => gpu.read(view.texture)?,
            None => Vec::new(),
        },
    };
    gpu.require_clean_debug_layer(if variant {
        "motion chain draw"
    } else {
        "stock chain draw"
    })?;
    Ok(frame)
}

fn run_case(fx: &Fixture, case: &TessCase) -> Outcome {
    let chain = make_chain(fx, case)?;
    linkage(fx, case, &chain)?;
    let targets = FixtureTargets::new(&fx.gpu, false)?;
    let mut motion = Motion::new(&fx.gpu, &targets)?;

    let stock = draw(fx, case, &chain, &targets, None, true)?;
    let moved = draw(fx, case, &chain, &targets, Some(&mut motion), true)?;
    let size = f64::from(SIZE);
    // The flat octahedron covers |x|/0.6 + |y|/0.6 <= 1; PN tessellation must bulge it beyond that.
    let (mut covered, mut bulge) = (0, 0);
    for y in 0..SIZE as usize {
        for x in 0..SIZE as usize {
            if uncovered(&stock.position[y * SIZE as usize + x]) {
                continue;
            }
            covered += 1;
            let (nx, ny) = (
                (x as f64 + 0.5) / size * 2. - 1.,
                1. - (y as f64 + 0.5) / size * 2.,
            );
            if nx.abs() / RADIUS_XY + ny.abs() / RADIUS_XY > 1.08 {
                bulge += 1;
            }
        }
    }
    ensure!(
        covered > 600 && bulge > 60,
        "{}: PN tessellation active ({covered} covered, {bulge} outside the flat octahedron)",
        case.name
    );

    let camera = previous_camera();
    let previous_clip_of = camera
        .projection
        .compose(&camera.view.compose(&previous_bone()));
    let diagnose = flag("RSF_TESS_DIAG");
    let (mut worst, mut largest, mut flat_worst) = (0f64, 0f64, 0f64);
    for y in 0..SIZE as usize {
        for x in 0..SIZE as usize {
            let i = y * SIZE as usize + x;
            ensure!(
                moved.position[i] == stock.position[i]
                    && moved.albedo[i] == stock.albedo[i]
                    && moved.depth[i].to_bits() == stock.depth[i].to_bits(),
                "{}: native MRT0/MRT1/depth bit-identical at {i}",
                case.name
            );
            let m = moved.motion[i];
            if uncovered(&stock.position[i]) {
                ensure!(zero(&m), "uncovered pixel stays cleared");
                continue;
            }
            let (u, v) = ((x as f64 + 0.5) / size, (y as f64 + 0.5) / size);
            let current = [u * 2. - 1., 1. - v * 2., f64::from(stock.depth[i]), 1.];
            let (expected_x, expected_y) = uv_motion(u, v, apply(&previous_clip_of, current));
            let (mx, my) = (f64::from(half(m[0])), f64::from(half(m[1])));
            ensure!(
                half(m[2]) == 1. && m[3] == 0,
                "{}: validity 1 at {i}",
                case.name
            );
            let error = (mx - expected_x).abs().max((my - expected_y).abs());
            worst = worst.max(error);
            largest = largest.max(expected_x.abs()).max(expected_y.abs());
            if diagnose && error >= 3e-4 {
                println!(
                    "    miss x={x} y={y} depth={:.5} measured {mx:.5},{my:.5} expected {expected_x:.5},{expected_y:.5}",
                    stock.depth[i]
                );
                continue;
            }
            ensure!(
                error < 3e-4,
                "{}: motion equals the CPU model at {i} (measured {mx},{my} expected {expected_x},{expected_y})",
                case.name
            );
            // Flat model: the previous position of the FLAT triangle point under this sample cannot be
            // reconstructed per pixel exactly; as a bound, the motion of the point projected back onto the
            // octahedron along the current view ray.
            let sum = current[0].abs() / RADIUS_XY + current[1].abs() / RADIUS_XY;
            if sum < 1. {
                // The visible (near) faces.
                let flat_z = CENTRE[2] - RADIUS_Z * (1. - sum);
                let flat = apply(&previous_clip_of, [current[0], current[1], flat_z, 1.]);
                let (fx_, fy) = uv_motion(u, v, flat);
                flat_worst = flat_worst.max((fx_ - mx).abs().max((fy - my).abs()));
            }
        }
    }
    println!(
        "  {}: {covered} covered ({bulge} by the PN bulge), motion up to {largest:.4} UV, worst error {worst:.2e} UV ({:.4} px); a flat-triangle previous position would be off by up to {flat_worst:.4} UV ({:.2} px)",
        case.name,
        worst * size,
        flat_worst * size
    );
    ensure!(
        flat_worst > 20. * worst,
        "{}: the fixture distinguishes the PN previous surface from a flat one",
        case.name
    );

    // No history: all-zero motion, native output unchanged.
    let none = draw(fx, case, &chain, &targets, Some(&mut motion), false)?;
    for i in 0..PIXELS {
        ensure!(
            zero(&none.motion[i]),
            "{}: missing history is zero",
            case.name
        );
        ensure!(
            none.position[i] == stock.position[i],
            "native unchanged without history"
        );
    }
    Ok(())
}

/// RSF_TESS_RAW=<dxbc file>: create that one hull or domain shader on a fresh device and nothing else (no
/// compiler loaded), to tell a driver problem with the bytecode from a problem of this process.
fn raw_create(path: &str) -> i32 {
    let Some((gpu, _)) = super::hardware(!flag("RSF_TESS_NODEBUG")) else {
        return 77;
    };
    let created = file_bytes(std::path::Path::new(path)).and_then(|code| {
        if path.contains(".hs.") {
            hull_shader(&gpu, &code, "raw HS").map(drop)
        } else {
            domain_shader(&gpu, &code, "raw DS").map(drop)
        }
    });
    if let Err(error) = created {
        eprintln!("FAILED {error}");
        return 1;
    }
    unsafe { gpu.context.Flush() };
    std::thread::sleep(std::time::Duration::from_millis(500));
    println!("raw create ok");
    0
}

/// A crash inside the driver or the compiler would otherwise end the test with no output at all.
unsafe extern "system" fn report_crash(info: *const EXCEPTION_POINTERS) -> i32 {
    let record = unsafe { info.as_ref().and_then(|info| info.ExceptionRecord.as_ref()) };
    let (code, address) = record.map_or((0, std::ptr::null()), |record| {
        (
            record.ExceptionCode as u32,
            record.ExceptionAddress.cast_const(),
        )
    });
    let mut module = std::ptr::null_mut();
    let mut name = [0u8; 260];
    if !address.is_null()
        && unsafe {
            GetModuleHandleExA(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address.cast(),
                &mut module,
            )
        } != 0
    {
        unsafe { GetModuleFileNameA(module, name.as_mut_ptr(), name.len() as u32) };
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let name = if end == 0 {
        "?".into()
    } else {
        String::from_utf8_lossy(&name[..end])
    };
    eprintln!(
        "CRASH 0x{code:08x} at {name}+0x{:x}",
        (address as usize).wrapping_sub(module as usize)
    );
    // EXCEPTION_CONTINUE_SEARCH
    0
}

fn run() -> i32 {
    unsafe { SetUnhandledExceptionFilter(Some(report_crash)) };
    if let Some(path) = std::env::var("RSF_TESS_RAW")
        .ok()
        .filter(|path| !path.is_empty())
    {
        return raw_create(&path);
    }
    let deployment = deployment(&["motion-deploy-tess", "motion-deploy"]);
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
    let families = manifest.entries().iter().map(|entry| entry.family);
    if !(families.clone().any(|family| family == TESS_HULL)
        && families.clone().any(|family| family == TESS_DOMAIN))
    {
        println!(
            "SKIP: {} has no dx11/tess.hs + dx11/tess.ds roots (regenerate it with tools/anomaly_motion_deploy.py)",
            deployment.display()
        );
        return 77;
    }
    let Some((gpu, debug_layer)) = super::hardware(!flag("RSF_TESS_NODEBUG")) else {
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
    let diagnose = flag("RSF_TESS_DIAG");
    let failed = |error: String| {
        if diagnose {
            println!("  CASE FAILED {error}");
        } else {
            eprintln!("FAILED {error}");
        }
    };
    if let Some(which) = std::env::var("RSF_TESS_CREATE")
        .ok()
        .filter(|which| !which.is_empty())
    {
        let [case, _] = hm_cases();
        return match create_probe(&fx, &case, &which) {
            Ok(()) => 0,
            Err(error) => {
                failed(error);
                1
            }
        };
    }
    for case in tess_cases(flag("RSF_TESS_HM")) {
        if let Err(error) = run_case(&fx, &case) {
            failed(error);
            if !diagnose {
                return 1;
            }
        }
    }
    let debug = if debug_layer { "on, clean" } else { "off" };
    println!(
        "anomaly motion tess chain device: {} checks passed (debug layer {debug})",
        checks()
    );
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_tess_chain_device_fixture() -> i32 {
    run()
}
