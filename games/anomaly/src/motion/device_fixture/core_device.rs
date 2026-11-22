// SPDX-License-Identifier: MIT
//! Motion MRT device fixture. Uses the LOCAL deployment from tools/anomaly_motion_deploy.py (licensed shader
//! bodies, never tracked; RSF_ANOMALY_MOTION_DEPLOY or <repo>/.local/anomaly/motion-deploy). Without it the
//! test exits 77.
//!
//! 1. Every allowlisted root compiles through the deployed-tree loader with the engine's d3dcompiler_43: the
//!    stock permutation is compiled from original/, handed in as "engine bytecode", reproduced byte for byte,
//!    and its motion variant compiled from motion/ and reflection-checked. A one-byte change of the engine
//!    bytecode is refused.
//! 2. deffer_base_flat VS/PS: the stock pair and the variant pair draw the same triangle into a real G-buffer
//!    (RGBA16F position, RGBA8 albedo, D24S8) with the RGBA16F motion target of the Rust motion owner as RTV2
//!    and b13 from the production packers. Native MRT0/1 must be bit-identical; motion must equal the CPU
//!    value of the supplied previous WVP; missing history must write all-zero motion.
//! 3. The invalidation replay: a stock-shader occluder drawn over the valid motion leaves stale vectors; the
//!    replay zeroes exactly the samples it won; an occluder behind the surface changes nothing; the OM state
//!    is restored, also when the replayed draw unwinds.
//! 4. Foliage. Every deployed tree/detail root exposes the constants the draw-time reader locates from the
//!    stock reflection. A real deffer_tree_flat variant draws a sheared crown with wind history: the motion
//!    equals the CPU model of the wind difference, the base does not move, an unchanged wind is valid zero, a
//!    camera pan is the camera vector, and missing history is zero. A real deffer_detail_w_flat variant draws
//!    two plants whose previous rows were batched into other groups: the production FoliageHistory remaps
//!    them, the motion equals the CPU model including the plant that grew, and a plant without history stays
//!    zero.
use super::super::*;
use super::foliage::{detail_bytes, foliage_layout};
use super::harness::{
    Built, ENGINE_COMPILER, FixtureTargets, Gpu, Half4, Keep, Motion, Outcome, Overrides, PIXELS,
    SIZE, Shaders, Signature, checks, define, deployment, engine_compiler, engine_defines, ensure,
    file_bytes, half, pixel_shader, raw, report, uncovered, vertex_shader, zero,
};
use crate::motion_device::{
    REPLAY_READY, ReplayScope, rsf_anomaly_motion_replay_begin, rsf_anomaly_motion_replay_end,
    rsf_anomaly_motion_replay_platform_get,
};
use crate::shader_loader::deployed::{
    ORIGINAL_MISMATCH, REPLACED, ShaderRequest, keeps_buffer_sizes, outcome_name,
};
use crate::shader_loader::rsf_anomaly_shader_linkable;
use crate::sys::{
    RSF_MOTION_CORE, RSF_MOTION_FOLIAGE, RSF_MOTION_MULTISAMPLED, RSF_MOTION_PIXEL,
    RSF_MOTION_READY,
};
use std::panic::{AssertUnwindSafe, catch_unwind};
use windows::Win32::Graphics::Direct3D::{D3D_FEATURE_LEVEL_11_0, D3D_REGISTER_COMPONENT_FLOAT32};
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;

struct Root {
    stage: u8,
    family: String,
    name: String,
}

fn roots(text: &[u8]) -> Vec<Root> {
    String::from_utf8_lossy(text)
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["root", stage, family, name, ..] => Some(Root {
                    stage: stage.as_bytes()[0],
                    family: (*family).into(),
                    name: (*name).into(),
                }),
                _ => None,
            }
        })
        .collect()
}

fn root_defines(root: &Root) -> Vec<(String, String)> {
    let mut defines = engine_defines();
    if root.family == "core_model" {
        defines.push(define("SKIN_NONE", "1"));
    }
    // The tessellation pass roots as uber_deffer.cpp r_TessPass compiles them for PN materials;
    // the tess chain fixture draws the full chain.
    if root.family == "tess_hull" || root.family == "tess_domain" {
        defines.push(define("TESS_PN", "1"));
    }
    defines
}

struct Fixture {
    gpu: Gpu,
    shaders: Shaders,
}

impl Fixture {
    fn build(&self, root: &Root, defines: &[(String, String)]) -> Outcome<Built> {
        self.shaders.try_build(root.stage, &root.name, defines)
    }

    fn debug_errors(&self) -> Outcome {
        let (errors, first) = self.gpu.debug_errors();
        if errors == 0 {
            Ok(())
        } else {
            Err(format!("debug layer: {first}"))
        }
    }

    fn read<T: Copy + Default>(&self, texture: *mut std::ffi::c_void) -> Outcome<Vec<T>> {
        self.gpu.read(texture)
    }
}

fn replaced(name: &str, built: &Built) -> Outcome {
    ensure!(
        built.prepared.outcome == REPLACED,
        "{name}: {} {}",
        outcome_name(built.prepared.outcome),
        built.prepared.detail
    );
    Ok(())
}

fn every_root(fx: &Fixture, list: &[Root]) -> Outcome {
    let mut count = 0;
    for root in list {
        replaced(&root.name, &fx.build(root, &root_defines(root))?)?;
        count += 1;
    }
    // Skinned and first-person-relevant permutations.
    for root in list.iter().filter(|root| root.family == "core_model") {
        for skin in 0..=4 {
            let mut defines = root_defines(root);
            *defines.last_mut().unwrap() = define(format!("SKIN_{skin}"), "1");
            let built = fx.build(root, &defines)?;
            ensure!(
                built.prepared.outcome == REPLACED && built.prepared.skin == skin,
                "{} SKIN_{skin}: {}",
                root.name,
                built.prepared.detail
            );
            count += 1;
        }
    }
    // A single changed byte in the "engine" bytecode must refuse: the inputs read from the engine are unproven.
    let first = &list[0];
    let source = fx.shaders.source(&first.name)?;
    let defines = root_defines(first);
    let request = ShaderRequest {
        target: super::harness::target_of(first.stage),
        defines: &defines,
        source: &source,
    };
    let compiled = fx
        .shaders
        .api
        .compile(&fx.shaders.deployment.join("original"), &request);
    ensure!(
        compiled.as_ref().is_ok_and(|code| !code.is_empty()),
        "compile original before tampering"
    );
    let mut stock = compiled?;
    let middle = stock.len() / 2;
    stock[middle] ^= 1;
    let shaders = &fx.shaders;
    let refused = shaders
        .api
        .prepare(&shaders.manifest, &shaders.deployment, &request, &stock);
    ensure!(
        refused.outcome != REPLACED && refused.variant.is_empty(),
        "tampered engine bytecode refused"
    );
    // Valid bytecode of another permutation (other root, same stage) for this root's source: the recompile differs.
    let other = list[1..].iter().find(|root| root.stage == first.stage);
    ensure!(other.is_some(), "second root of the stage");
    let other = other.unwrap();
    let foreign = fx.build(other, &root_defines(other))?;
    let refused = shaders.api.prepare(
        &shaders.manifest,
        &shaders.deployment,
        &request,
        &foreign.stock,
    );
    ensure!(
        refused.outcome == ORIGINAL_MISMATCH && refused.variant.is_empty(),
        "bytecode of another permutation refused"
    );
    println!(
        "roots compiled: {count} variants through the deployed-tree loader ({} roots)",
        list.len()
    );
    Ok(())
}

/// Two triangles restricted to x in [x0, x1] at depth z, every other input 0.5.
fn triangle(gpu: &Gpu, signature: &Signature, x0: f32, x1: f32, z: f32) -> Outcome<ID3D11Buffer> {
    let corners = [
        [x0, -1.],
        [x0, 1.],
        [x1, -1.],
        [x1, -1.],
        [x0, 1.],
        [x1, 1.],
    ];
    let inputs = signature.inputs.len();
    let mut data = vec![0.5f32; 6 * inputs * 4];
    for (v, corner) in corners.iter().enumerate() {
        for i in 0..inputs {
            let out = &mut data[(v * inputs + i) * 4..][..4];
            match signature.semantic(i) {
                b"POSITION" => out.copy_from_slice(&[corner[0], corner[1], z, 1.]),
                b"TEXCOORD" => out.copy_from_slice(&[0.25, 0.75, 0., 0.]),
                _ => {}
            }
        }
    }
    gpu.vertex_buffer(&data)
}

fn pixel_bytes() -> Outcome<[u8; PIXEL_BYTES]> {
    let mut bytes = std::mem::MaybeUninit::uninit();
    let packed = rsf_anomaly_motion_pack_pixel(
        &PixelInputs {
            viewport: [0., 0., SIZE as f32, SIZE as f32],
            jitter: [0.; 4],
        },
        &mut bytes,
    );
    ensure!(packed, "pack PS b13");
    Ok(unsafe { bytes.assume_init() })
}

fn draw_scene(fx: &Fixture) -> Outcome {
    let gpu = &fx.gpu;
    let vs_root = Root {
        stage: b'v',
        family: "core_static".into(),
        name: "deffer_base_flat.vs".into(),
    };
    let ps_root = Root {
        stage: b'p',
        family: "core_pixel".into(),
        name: "deffer_base_flat.ps".into(),
    };
    let vs = fx.build(&vs_root, &root_defines(&vs_root))?;
    let ps = fx.build(&ps_root, &root_defines(&ps_root))?;
    ensure!(
        vs.prepared.outcome == REPLACED && ps.prepared.outcome == REPLACED,
        "flat pair variants"
    );
    ensure!(
        rsf_anomaly_shader_linkable(vs.reflection()?.view(), ps.reflection()?.view()),
        "variant pair links"
    );
    let stock_vs_reflection = fx.shaders.reflect(&vs.stock)?;
    ensure!(
        !rsf_anomaly_shader_linkable(stock_vs_reflection.view(), ps.reflection()?.view()),
        "stock VS cannot feed the motion PS"
    );

    let stock_vs = vertex_shader(gpu, &vs.stock, "stock VS")?;
    let motion_vs = vertex_shader(gpu, &vs.prepared.variant, "motion VS")?;
    let stock_ps = pixel_shader(gpu, &ps.stock, "stock PS")?;
    let motion_ps = pixel_shader(gpu, &ps.prepared.variant, "motion PS")?;
    let vs_signature = Signature::new(&vs.stock)?;
    let ps_signature = Signature::new(&ps.stock)?;
    let layout = gpu.layout(&vs_signature, &vs.stock)?;
    let stride = vs_signature.stride();
    let full_vb = triangle(gpu, &vs_signature, -1., 1., 0.5)?;
    let near_vb = triangle(gpu, &vs_signature, -1., 0., 0.25)?;
    let behind_vb = triangle(gpu, &vs_signature, 0., 1., 0.75)?;
    let targets = FixtureTargets::new(gpu, true)?;
    let mut constants = Keep::new();

    let mut motion = Motion::new(gpu, &targets)?;
    let desc = targets.position_desc();
    ensure!(motion.view().width == SIZE, "motion target follows RT0");
    ensure!(
        motion.target(
            desc.Width,
            desc.Height,
            desc.SampleDesc.Count,
            desc.ArraySize
        ) == RSF_MOTION_READY,
        "reused"
    );
    ensure!(
        motion.target(desc.Width, desc.Height, 4, desc.ArraySize) == RSF_MOTION_MULTISAMPLED,
        "MSAA refused"
    );
    let view = motion.view();

    let mut setup = |variant: bool, vb: &ID3D11Buffer, three: bool| -> Outcome {
        unsafe { gpu.context.ClearState() };
        gpu.bind_input(&super::draw::Input {
            layout: layout.as_raw(),
            vertex_buffer: vb.as_raw(),
            vs: if variant {
                motion_vs.as_raw()
            } else {
                stock_vs.as_raw()
            },
            hs: std::ptr::null_mut(),
            ds: std::ptr::null_mut(),
            ps: if variant {
                motion_ps.as_raw()
            } else {
                stock_ps.as_raw()
            },
            stride,
            tessellated: false,
        });
        constants.bind_core(gpu, &vs_signature.reflection, true)?;
        constants.bind_core(gpu, &ps_signature.reflection, false)?;
        gpu.bind_output(&targets.output(if three {
            view.rtv
        } else {
            std::ptr::null_mut()
        }));
        Ok(())
    };
    let clear = || gpu.clear(&targets.output(view.rtv), true);
    let mut upload = |valid: bool| -> Outcome {
        let mut previous = Mat4::IDENTITY;
        previous.0[3] = 0.1; // previous clip x = x + 0.1
        previous.0[7] = -0.2; // previous clip y = y - 0.2
        let history = CoreHistory {
            draw: valid,
            palette: false,
            rigid: false,
        };
        let mut vertex = std::mem::MaybeUninit::uninit();
        let packed = unsafe {
            rsf_anomaly_motion_pack_core(&previous, std::ptr::null(), 0, &history, &mut vertex)
        };
        ensure!(packed, "pack VS b13");
        let vertex = unsafe { vertex.assume_init() };
        let pixel = pixel_bytes()?;
        ensure!(
            motion.upload(gpu, RSF_MOTION_CORE, &vertex)
                && motion.upload(gpu, RSF_MOTION_PIXEL, &pixel),
            "b13 upload"
        );
        Ok(())
    };
    let position = targets.position.as_raw();
    let albedo = targets.albedo.as_raw();

    // Stock reference.
    clear();
    setup(false, &full_vb, false)?;
    gpu.draw(6);
    let stock_position = fx.read::<Half4>(position)?;
    let stock_albedo = fx.read::<u32>(albedo)?;
    fx.debug_errors()?;
    // Variant with history.
    clear();
    setup(true, &full_vb, true)?;
    upload(true)?;
    gpu.draw(6);
    let drawn_position = fx.read::<Half4>(position)?;
    let drawn_albedo = fx.read::<u32>(albedo)?;
    let mut vectors = fx.read::<Half4>(view.texture)?;
    fx.debug_errors()?;
    let mut covered = 0;
    for i in 0..PIXELS {
        ensure!(
            drawn_position[i] == stock_position[i] && drawn_albedo[i] == stock_albedo[i],
            "native MRT0/1 bit-identical at {i}"
        );
        if uncovered(&stock_position[i]) {
            continue;
        }
        covered += 1;
        let v = vectors[i];
        ensure!(
            (half(v[0]) - -0.05).abs() < 2e-4
                && (half(v[1]) - -0.1).abs() < 2e-4
                && half(v[2]) == 1.
                && half(v[3]) == 0.,
            "motion (-0.05,-0.1,1,0) at {i}: {},{},{}",
            half(v[0]),
            half(v[1]),
            half(v[2])
        );
    }
    ensure!(covered == PIXELS, "full coverage");

    // Stock occluder in front of the left half: the stock PS leaves stale vectors; the replay invalidates them.
    setup(false, &near_vb, true)?;
    gpu.draw(6);
    let stale = fx.read::<Half4>(view.texture)?;
    ensure!(
        half(stale[(SIZE / 2 * SIZE + 4) as usize][2]) == 1.,
        "fixture: a stock draw leaves the old vector (the reason for the replay)"
    );
    let occluded_position = fx.read::<Half4>(position)?;
    let check_restored = || -> Outcome {
        let mut bound: [Option<ID3D11RenderTargetView>; 3] = Default::default();
        let mut depth = None;
        unsafe {
            gpu.context
                .OMGetRenderTargets(Some(&mut bound), Some(&mut depth))
        };
        let mut shader = None;
        unsafe { gpu.context.PSGetShader(&mut shader, None, None) };
        let mut state = None;
        let mut reference = 0;
        unsafe {
            gpu.context
                .OMGetDepthStencilState(Some(&mut state), Some(&mut reference))
        };
        ensure!(
            raw(bound[0].as_ref()) == targets.position_rtv.as_raw()
                && raw(bound[1].as_ref()) == targets.albedo_rtv.as_raw()
                && raw(bound[2].as_ref()) == view.rtv
                && raw(depth.as_ref()) == targets.dsv.as_raw()
                && raw(shader.as_ref()) == stock_ps.as_raw()
                && raw(state.as_ref()) == targets.less.as_raw()
                && reference == 0,
            "OM state and PS restored"
        );
        Ok(())
    };
    // The saved graphics state is restored by the scope's drop, also during an unwind out of the redraw.
    let platform = rsf_anomaly_motion_replay_platform_get();
    let replay = |redraw: &mut dyn FnMut()| -> u8 {
        let mut scope = std::mem::MaybeUninit::<ReplayScope>::uninit();
        let result = unsafe {
            rsf_anomaly_motion_replay_begin(platform, &view, gpu.context_raw(), scope.as_mut_ptr())
        };
        if result == REPLAY_READY {
            struct End(*mut ReplayScope);
            impl Drop for End {
                fn drop(&mut self) {
                    unsafe { rsf_anomaly_motion_replay_end(self.0) };
                }
            }
            let _end = End(scope.as_mut_ptr());
            redraw();
        }
        result
    };
    struct ReplayAborted;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        replay(&mut || std::panic::panic_any(ReplayAborted))
    }))
    .is_err();
    std::panic::set_hook(hook);
    ensure!(
        unwound,
        "replay callback unwound out of the restoring scope"
    );
    check_restored()?;
    let redraw = &mut || unsafe { gpu.context.Draw(6, 0) };
    ensure!(replay(redraw) == REPLAY_READY, "replay");
    check_restored()?;
    // Behind the surface: the occluder fails the depth test, so the replay's EQUAL test fails too.
    setup(false, &behind_vb, true)?;
    gpu.draw(6);
    ensure!(replay(redraw) == REPLAY_READY, "replay behind");
    vectors = fx.read::<Half4>(view.texture)?;
    let after_position = fx.read::<Half4>(position)?;
    fx.debug_errors()?;
    let (mut invalidated, mut kept) = (0, 0);
    for y in 0..SIZE as usize {
        for x in 0..SIZE as usize {
            let i = y * SIZE as usize + x;
            let v = vectors[i];
            ensure!(
                after_position[i] == occluded_position[i],
                "the replay writes no native target"
            );
            if x < SIZE as usize / 2 {
                ensure!(zero(&v), "occluded sample invalidated");
                invalidated += 1;
            } else {
                ensure!(
                    half(v[2]) == 1. && (half(v[0]) + 0.05).abs() < 2e-4,
                    "visible sample kept"
                );
                kept += 1;
            }
        }
    }
    // Missing history: all-zero motion, native output unchanged.
    clear();
    setup(true, &full_vb, true)?;
    upload(false)?;
    gpu.draw(6);
    vectors = fx.read::<Half4>(view.texture)?;
    let invalid_position = fx.read::<Half4>(position)?;
    for i in 0..PIXELS {
        ensure!(zero(&vectors[i]), "missing history is zero");
        ensure!(
            invalid_position[i] == stock_position[i],
            "native unchanged without history"
        );
    }
    fx.debug_errors()?;
    let view = motion.view();
    println!(
        "draw: {covered} covered, {invalidated} invalidated by replay, {kept} kept, uploads={} skipped={}",
        view.uploads, view.upload_skips
    );
    Ok(())
}

// ---- foliage (trees and grass) --------------------------------------------------------------------------------

/// Every deployed foliage root must expose what the draw-time reader needs under the engine's compile flags.
fn foliage_layouts(fx: &Fixture, list: &[Root]) -> Outcome {
    let (mut trees, mut details) = (0, 0);
    for root in list {
        let tree = root.family == "foliage_tree";
        if !tree && root.family != "foliage_detail" {
            continue;
        }
        let built = fx.build(root, &root_defines(root))?;
        ensure!(
            built.prepared.outcome == REPLACED,
            "{}: {}",
            root.name,
            built.prepared.detail
        );
        let still = root.name == "deffer_detail_s_flat.vs";
        let family = if tree { FOLIAGE_TREE } else { FOLIAGE_DETAIL };
        let layout = foliage_layout(
            built.prepared.stock.as_ref().map(|s| s.view()),
            family,
            true,
            still,
        );
        ensure!(
            layout.is_ok(),
            "{}: {}",
            root.name,
            layout.err().unwrap().to_string_lossy()
        );
        if tree {
            trees += 1;
        } else {
            details += 1;
        }
        ensure!(
            keeps_buffer_sizes(built.stock_reflection()?, built.reflection()?),
            "{} buffers kept",
            root.name
        );
    }
    ensure!(
        trees >= 10 && details == 2,
        "all deployed foliage roots checked"
    );
    println!("foliage roots: {trees} tree, {details} detail layouts usable");
    Ok(())
}

fn cyclic(x: f32) -> f32 {
    use std::f32::consts::SQRT_2;
    let f = 2. * SQRT_2 * (x - x.floor()) - SQRT_2;
    f * f - 1.
}

struct Rig {
    stock_vs: ID3D11VertexShader,
    motion_vs: ID3D11VertexShader,
    stock_ps: ID3D11PixelShader,
    motion_ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    vs_signature: Signature,
    ps_signature: Signature,
    vs: Built,
    stride: u32,
    targets: FixtureTargets,
    /// Keeps the engine constant buffers alive.
    engine: Keep,
}

fn make_rig(fx: &Fixture, vs_root: &Root, ps_root: &Root) -> Outcome<Rig> {
    let gpu = &fx.gpu;
    let vs = fx.build(vs_root, &root_defines(vs_root))?;
    let ps = fx.build(ps_root, &root_defines(ps_root))?;
    ensure!(
        vs.prepared.outcome == REPLACED && ps.prepared.outcome == REPLACED,
        "{} pair variants",
        vs_root.name
    );
    ensure!(
        rsf_anomaly_shader_linkable(vs.reflection()?.view(), ps.reflection()?.view()),
        "{} variant links to the core pixel variant",
        vs_root.name
    );
    let vs_signature = Signature::new(&vs.stock)?;
    let ps_signature = Signature::new(&ps.stock)?;
    Ok(Rig {
        stock_vs: vertex_shader(gpu, &vs.stock, "stock VS")?,
        motion_vs: vertex_shader(gpu, &vs.prepared.variant, "motion VS")?,
        stock_ps: pixel_shader(gpu, &ps.stock, "stock PS")?,
        motion_ps: pixel_shader(gpu, &ps.prepared.variant, "motion PS")?,
        layout: gpu.layout(&vs_signature, &vs.stock)?,
        stride: vs_signature.stride(),
        vs_signature,
        ps_signature,
        vs,
        targets: FixtureTargets::new(gpu, true)?,
        engine: Keep::new(),
    })
}

/// A quad of two triangles in the vertex layout of the foliage VS: POSITION float4, TEXCOORD0 int4
/// (0, 0, frac, row offset).
#[allow(clippy::too_many_arguments)]
fn foliage_quad(
    gpu: &Gpu,
    rig: &Rig,
    x: [f32; 2],
    y: [f32; 2],
    z: f32,
    frac: i32,
    row: i32,
) -> Outcome<ID3D11Buffer> {
    let corners = [
        [x[0], y[0]],
        [x[0], y[1]],
        [x[1], y[0]],
        [x[1], y[0]],
        [x[0], y[1]],
        [x[1], y[1]],
    ];
    let signature = &rig.vs_signature;
    let inputs = signature.inputs.len();
    let mut data = vec![[0u32; 4]; 6 * inputs];
    for (v, corner) in corners.iter().enumerate() {
        for i in 0..inputs {
            let mut floats = [0.5f32; 4];
            let mut ints = [0i32; 4];
            match signature.semantic(i) {
                b"POSITION" => floats = [corner[0], corner[1], z, 1.],
                b"TEXCOORD" => [ints[2], ints[3]] = [frac, row],
                _ => {}
            }
            data[v * inputs + i] =
                if signature.inputs[i].ComponentType == D3D_REGISTER_COMPONENT_FLOAT32 {
                    floats.map(f32::to_bits)
                } else {
                    ints.map(|value| value as u32)
                };
        }
    }
    gpu.vertex_buffer(&data)
}

struct FoliageDraw {
    position: Vec<Half4>,
    vectors: Vec<Half4>,
    albedo: Vec<u32>,
}

/// Draws `quads` (one quad each, 6 vertices) with the stock pair or the variant pair and `overrides` as the
/// engine's VS constants. The variant draw uploads `vertex` as VS b13 and a unit-viewport PS b13.
fn draw_foliage_pair(
    fx: &Fixture,
    rig: &mut Rig,
    motion: &mut Motion,
    vertex: Option<&[u8; FOLIAGE_BYTES]>,
    overrides: &Overrides,
    quads: &[ID3D11Buffer],
) -> Outcome<FoliageDraw> {
    let gpu = &fx.gpu;
    let variant = vertex.is_some();
    unsafe { gpu.context.ClearState() };
    let view = motion.view();
    let targets = rig.targets.output(view.rtv);
    gpu.clear(&targets, true);
    gpu.bind_input(&super::draw::Input {
        layout: rig.layout.as_raw(),
        vertex_buffer: std::ptr::null_mut(),
        vs: if variant {
            rig.motion_vs.as_raw()
        } else {
            rig.stock_vs.as_raw()
        },
        hs: std::ptr::null_mut(),
        ds: std::ptr::null_mut(),
        ps: if variant {
            rig.motion_ps.as_raw()
        } else {
            rig.stock_ps.as_raw()
        },
        stride: rig.stride,
        tessellated: false,
    });
    rig.engine
        .bind_core(gpu, &rig.vs_signature.reflection, true)?;
    let mut keep = Keep::new();
    keep.patch_core(gpu, &rig.vs_signature.reflection, overrides)?;
    rig.engine
        .bind_core(gpu, &rig.ps_signature.reflection, false)?;
    let mut bound = targets;
    if !variant {
        bound.motion = std::ptr::null_mut();
    }
    gpu.bind_output(&bound);
    if let Some(vertex) = vertex {
        let pixel = pixel_bytes()?;
        ensure!(
            motion.upload(gpu, RSF_MOTION_FOLIAGE, vertex)
                && motion.upload(gpu, RSF_MOTION_PIXEL, &pixel),
            "foliage b13 upload"
        );
    }
    for quad in quads {
        gpu.draw_quad(quad, rig.stride);
    }
    let out = FoliageDraw {
        position: fx.read(rig.targets.position.as_raw())?,
        albedo: fx.read(rig.targets.albedo.as_raw())?,
        vectors: if variant {
            fx.read(view.texture)?
        } else {
            Vec::new()
        },
    };
    fx.debug_errors()?;
    drop(keep);
    Ok(out)
}

fn ndc_x(i: usize) -> f32 {
    (i as f32 + 0.5) / SIZE as f32 * 2. - 1.
}

fn ndc_y(j: usize) -> f32 {
    1. - (j as f32 + 0.5) / SIZE as f32 * 2.
}

fn constants(consts: [f32; 4], time: f32, amplitude: f32) -> FoliageConstants {
    FoliageConstants {
        consts,
        wave: [0., 0., 0., time],
        wind: [amplitude, 0., 0., 0.],
    }
}

fn tree_bytes(
    clip: &Mat4,
    xform: &[f32; 12],
    previous: &FoliageConstants,
) -> Outcome<[u8; FOLIAGE_BYTES]> {
    let mut bytes = std::mem::MaybeUninit::uninit();
    let packed = rsf_anomaly_motion_pack_tree(clip, xform, previous, &mut bytes);
    ensure!(packed, "pack tree history");
    Ok(unsafe { bytes.assume_init() })
}

fn invalid_foliage() -> [u8; FOLIAGE_BYTES] {
    let mut bytes = std::mem::MaybeUninit::uninit();
    rsf_anomaly_motion_pack_foliage_invalid(&mut bytes);
    unsafe { bytes.assume_init() }
}

fn foliage_motion(fx: &Fixture, rig: &Rig, family: u8) -> Outcome<Motion> {
    let layout = foliage_layout(
        rig.vs.prepared.stock.as_ref().map(|s| s.view()),
        family,
        false,
        false,
    );
    ensure!(
        layout.is_ok(),
        "{}",
        layout
            .err()
            .map_or("foliage layout".into(), |e| e.to_string_lossy())
    );
    Motion::new(&fx.gpu, &rig.targets)
}

fn draw_tree(fx: &Fixture) -> Outcome {
    let vs_root = Root {
        stage: b'v',
        family: "foliage_tree".into(),
        name: "deffer_tree_flat.vs".into(),
    };
    let ps_root = Root {
        stage: b'p',
        family: "core_pixel".into(),
        name: "deffer_base_flat.ps".into(),
    };
    let mut rig = make_rig(fx, &vs_root, &ps_root)?;
    let mut motion = foliage_motion(fx, &rig, FOLIAGE_TREE)?;
    let quads = [foliage_quad(
        &fx.gpu,
        &rig,
        [-1., 1.],
        [-1., 1.],
        0.5,
        1024,
        0,
    )?];
    // Engine constants as FTreeVisual::Render stages them: scale 1/2048, wave (0, 0, 0, time), wind
    // (amplitude, 0, 0, 0). m_xform and the matrices stay identity, so the quad's y is the height above the
    // trunk base (m_xform._24 = 0).
    let scale = [1. / 2048., 1. / 2048., 0., 0.];
    let overrides = |c: &FoliageConstants| -> Overrides {
        [("consts", c.consts), ("wave", c.wave), ("wind", c.wind)]
            .into_iter()
            .map(|(name, values)| (name, values.to_vec()))
            .collect()
    };
    let identity_xform = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.];
    let now = constants(scale, 0.30, 0.6);
    let before = constants(scale, 0.80, 0.4);

    let stock = draw_foliage_pair(fx, &mut rig, &mut motion, None, &overrides(&now), &quads)?;
    // 1. Wind in the previous frame differs: the crown moves, the base (y = 0) does not, native targets are
    //    untouched.
    let bytes = tree_bytes(&Mat4::IDENTITY, &identity_xform, &before)?;
    let drawn = draw_foliage_pair(
        fx,
        &mut rig,
        &mut motion,
        Some(&bytes),
        &overrides(&now),
        &quads,
    )?;
    let frac = 1024. * (1. / 2048.);
    let (mut moving, mut still) = (0, 0);
    let mut largest = 0f32;
    for j in 0..SIZE as usize {
        for i in 0..SIZE as usize {
            let k = j * SIZE as usize + i;
            ensure!(
                drawn.position[k] == stock.position[k] && drawn.albedo[k] == stock.albedo[k],
                "native MRT0/1 bit-identical for the tree"
            );
            let v = drawn.vectors[k];
            // The sway shears the quad, so a corner can stay uncovered.
            if uncovered(&stock.position[k]) {
                ensure!(v[2] == 0, "uncovered tree pixels stay invalid");
                continue;
            }
            let y = ndc_y(j);
            let expected = (now.wind[0] * y * cyclic(now.wave[3]) * frac
                - before.wind[0] * y * cyclic(before.wave[3]) * frac)
                / 2.;
            let (mx, my) = (half(v[0]), half(v[1]));
            ensure!(
                (mx - expected).abs() < 4e-4
                    && my.abs() < 2e-4
                    && half(v[2]) == 1.
                    && half(v[3]) == 0.,
                "tree wind motion at {i},{j}: {mx} vs {expected} valid {} y {my}",
                half(v[2])
            );
            largest = largest.max(mx.abs());
            if expected.abs() > 2e-3 {
                moving += 1;
            } else {
                still += 1;
            }
        }
    }
    ensure!(
        largest > 5e-3 && moving > PIXELS / 4,
        "the wind produced real motion across the crown"
    );
    ensure!(still >= 1, "rows near the trunk base barely move");
    // 2. Same wind and a static camera: valid and exactly zero. The trunk is not wind.
    let bytes = tree_bytes(&Mat4::IDENTITY, &identity_xform, &now)?;
    let drawn = draw_foliage_pair(
        fx,
        &mut rig,
        &mut motion,
        Some(&bytes),
        &overrides(&now),
        &quads,
    )?;
    let mut covered = 0;
    for k in 0..PIXELS {
        if uncovered(&stock.position[k]) {
            continue;
        }
        covered += 1;
        let v = drawn.vectors[k];
        ensure!(
            half(v[0]).abs() < 1e-4 && half(v[1]).abs() < 1e-4 && half(v[2]) == 1.,
            "static tree with unchanged wind is valid zero motion"
        );
    }
    ensure!(covered > PIXELS / 2, "the tree covers most of the target");
    // 3. A camera pan with the same wind is the camera vector (previous clip x = x + 0.1: -0.05), crown and
    //    base alike.
    let mut panned = Mat4::IDENTITY;
    panned.0[3] = 0.1;
    let bytes = tree_bytes(&panned, &identity_xform, &now)?;
    let drawn = draw_foliage_pair(
        fx,
        &mut rig,
        &mut motion,
        Some(&bytes),
        &overrides(&now),
        &quads,
    )?;
    for k in 0..PIXELS {
        if uncovered(&stock.position[k]) {
            continue;
        }
        let v = drawn.vectors[k];
        ensure!(
            (half(v[0]) + 0.05).abs() < 2e-4 && half(v[1]).abs() < 2e-4 && half(v[2]) == 1.,
            "camera pan on a tree"
        );
    }
    // 4. No history: native output unchanged, motion all zero.
    let bytes = invalid_foliage();
    let drawn = draw_foliage_pair(
        fx,
        &mut rig,
        &mut motion,
        Some(&bytes),
        &overrides(&now),
        &quads,
    )?;
    for k in 0..PIXELS {
        ensure!(zero(&drawn.vectors[k]), "tree without history is zero");
        ensure!(
            drawn.position[k] == stock.position[k],
            "native unchanged without history"
        );
    }
    println!(
        "tree: wind motion up to {largest:.4} UV over {moving} crown pixels, static/pan/no-history cases pass"
    );
    Ok(())
}

struct Plant {
    tx: f32,
    sun: f32,
    hemi: f32,
}

fn group(plant: &Plant, scale: f32) -> [f32; 16] {
    let Plant { tx, sun, hemi } = *plant;
    [
        scale, 0., 0., tx, 0., scale, 0., 0., 0., 0., scale, 0.5, sun, sun, sun, hemi,
    ]
}

fn draw_detail(fx: &Fixture) -> Outcome {
    let vs_root = Root {
        stage: b'v',
        family: "foliage_detail".into(),
        name: "deffer_detail_w_flat.vs".into(),
    };
    let ps_root = Root {
        stage: b'p',
        family: "core_pixel".into(),
        name: "deffer_base_flat.ps".into(),
    };
    let mut rig = make_rig(fx, &vs_root, &ps_root)?;
    let mut motion = foliage_motion(fx, &rig, FOLIAGE_DETAIL)?;

    let a = Plant {
        tx: -0.5,
        sun: 0.3,
        hemi: 0.7,
    };
    let b = Plant {
        tx: 0.5,
        sun: 0.2,
        hemi: 0.6,
    };
    let scale = [1. / 16384., 1. / 16384., 0.5, 0.25];
    let overrides = |c: &FoliageConstants, rows: &[f32]| -> Overrides {
        let mut map: Overrides = [("consts", c.consts), ("wave", c.wave), ("dir2D", c.wind)]
            .into_iter()
            .map(|(name, values)| (name, values.to_vec()))
            .collect();
        map.insert("array", rows.to_vec());
        map
    };
    let now = constants(scale, 0.30, 0.08);
    let before = constants(scale, 0.80, 0.05);
    // The previous frame drew plant A before plant B. The current frame reverses the group order.
    let previous_groups = [group(&a, 0.8), group(&b, 1.0)];
    let current_groups = [group(&b, 1.0), group(&a, 1.0)];
    let upload_for = |include_b: bool| -> Outcome<(Vec<f32>, [u8; FOLIAGE_BYTES])> {
        let mut rows = std::mem::MaybeUninit::uninit();
        let mut bytes = std::mem::MaybeUninit::uninit();
        let packed = detail_bytes(
            SIZE,
            include_b,
            &before,
            &now,
            &previous_groups,
            &current_groups,
            &mut rows,
            &mut bytes,
        );
        ensure!(
            packed.is_ok(),
            "{}",
            packed
                .err()
                .map_or("packed detail history".into(), |e| e.to_string_lossy())
        );
        let rows = unsafe { rows.assume_init() };
        Ok((rows.as_flattened().to_vec(), unsafe { bytes.assume_init() }))
    };
    let quads = [
        // plant B (offset 0)
        foliage_quad(&fx.gpu, &rig, [-0.5, 0.5], [-1., 1.], 0., 8192, 0)?,
        // plant A (offset 4)
        foliage_quad(&fx.gpu, &rig, [-0.5, 0.5], [-1., 1.], 0., 8192, 4)?,
    ];
    // Quad B must land on the right half and quad A on the left: each shifts by its own translation via the
    // rows (tx = 0.5 / -0.5).
    let (current, bytes) = upload_for(true)?;
    let stock = draw_foliage_pair(
        fx,
        &mut rig,
        &mut motion,
        None,
        &overrides(&now, &current),
        &quads,
    )?;
    let drawn = draw_foliage_pair(
        fx,
        &mut rig,
        &mut motion,
        Some(&bytes),
        &overrides(&now, &current),
        &quads,
    )?;
    let frac = 8192. / 16384.;
    let (mut left, mut right) = (0, 0);
    let mut largest = 0f32;
    for j in 0..SIZE as usize {
        for i in 0..SIZE as usize {
            let k = j * SIZE as usize + i;
            ensure!(
                drawn.position[k] == stock.position[k] && drawn.albedo[k] == stock.albedo[k],
                "native MRT0/1 bit-identical for the grass"
            );
            let v = drawn.vectors[k];
            if uncovered(&stock.position[k]) {
                ensure!(v[2] == 0, "uncovered pixels stay invalid");
                continue;
            }
            let (x, y) = (ndc_x(i), ndc_y(j));
            // The two quads may overlap there once the sway shears them.
            if x.abs() < 0.1 {
                continue;
            }
            // Plant A (tx = -0.5, previous scale 0.8) is the left half, B the right half.
            let is_a = x < 0.;
            let plant = if is_a { &a } else { &b };
            let (s_now, s_prev) = (1f32, if is_a { 0.8 } else { 1. });
            let y_obj = y / s_now;
            let res_now = now.wind[0] * (s_now * y_obj) * cyclic(now.wave[3]) * frac;
            let x_obj = (x - plant.tx - res_now) / s_now;
            let prev_x = s_prev * x_obj
                + plant.tx
                + before.wind[0] * (s_prev * y_obj) * cyclic(before.wave[3]) * frac;
            let prev_y = s_prev * y_obj;
            let (ex, ey) = ((x - prev_x) / 2., -(y - prev_y) / 2.);
            let (mx, my) = (half(v[0]), half(v[1]));
            ensure!(
                (mx - ex).abs() < 5e-4 && (my - ey).abs() < 5e-4 && half(v[2]) == 1.,
                "grass motion at {i},{j}: {mx},{my} vs {ex},{ey}"
            );
            largest = largest.max(mx.abs());
            if is_a {
                left += 1;
            } else {
                right += 1;
            }
        }
    }
    ensure!(
        left > 100 && right > 100 && largest > 4e-3,
        "both plants moved with the wind"
    );

    // A plant the previous frame did not draw has no history; its neighbour keeps its own.
    let (current2, bytes2) = upload_for(false)?;
    let partial = draw_foliage_pair(
        fx,
        &mut rig,
        &mut motion,
        Some(&bytes2),
        &overrides(&now, &current2),
        &quads,
    )?;
    let (mut a_valid, mut b_zero) = (0, 0);
    for j in 0..SIZE as usize {
        for i in 0..SIZE as usize {
            let k = j * SIZE as usize + i;
            if uncovered(&stock.position[k]) || ndc_x(i).abs() < 0.1 {
                continue;
            }
            if ndc_x(i) < 0. {
                ensure!(half(partial.vectors[k][2]) == 1., "plant A valid");
                a_valid += 1;
            } else {
                ensure!(
                    zero(&partial.vectors[k]),
                    "a plant without history is zero, not a guess"
                );
                ensure!(partial.position[k] == stock.position[k], "native unchanged");
                b_zero += 1;
            }
        }
    }
    ensure!(
        a_valid > 100 && b_zero > 100,
        "partial batch covered both plants"
    );
    // Whole batch without history (first frame, route change): all zero.
    let none = draw_foliage_pair(
        fx,
        &mut rig,
        &mut motion,
        Some(&invalid_foliage()),
        &overrides(&now, &current),
        &quads,
    )?;
    for vector in &none.vectors {
        ensure!(zero(vector), "no history is zero");
    }
    println!(
        "grass: remapped batch motion up to {largest:.4} UV ({left} + {right} pixels), partial and empty history cases pass"
    );
    Ok(())
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
    let Some((gpu, level)) = super::default_device() else {
        println!("SKIP: no D3D11 device");
        return 77;
    };
    let setup = || -> Outcome<Option<(Fixture, Vec<Root>)>> {
        ensure!(level.0 >= D3D_FEATURE_LEVEL_11_0.0, "feature level 11");
        // The engine's compiler; RSF_ANOMALY_MOTION_COMPILER overrides it for experiments.
        let compiler = std::env::var("RSF_ANOMALY_MOTION_COMPILER")
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| ENGINE_COMPILER.into());
        let Some(api) = engine_compiler(&compiler) else {
            println!("SKIP: {compiler} not available");
            return Ok(None);
        };
        println!("compiler: {compiler}");
        let manifest = Shaders::manifest(&deployment)?;
        let list = roots(&file_bytes(&deployment.join("manifest.txt"))?);
        ensure!(list.len() == manifest.entries().len(), "manifest roots");
        let shaders = Shaders {
            api,
            manifest,
            deployment: deployment.clone(),
        };
        Ok(Some((Fixture { gpu, shaders }, list)))
    };
    let (fx, list) = match setup() {
        Ok(Some(ready)) => ready,
        Ok(None) => return 77,
        Err(error) => {
            eprintln!("FAILED {error}");
            return 1;
        }
    };
    let phases: [&dyn Fn() -> Outcome; 5] = [
        &|| every_root(&fx, &list),
        &|| foliage_layouts(&fx, &list),
        &|| draw_scene(&fx),
        &|| draw_tree(&fx),
        &|| draw_detail(&fx),
    ];
    for phase in phases {
        if !report(phase()) {
            return 1;
        }
    }
    println!("anomaly motion device: {} checks passed", checks());
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_fixture() -> i32 {
    run()
}
