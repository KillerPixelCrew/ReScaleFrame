// SPDX-License-Identifier: MIT
//! The swap owner on fake CRenderTarget, CRT, CTexture, RCache and Device memory laid out with the sr_profile.rs
//! offsets, run by cargo test.

use super::*;
use std::ptr;
mod scenarios;

const CRT_SIZE: usize = 0x50;
const TEXTURE_SIZE: usize = 0xb0;
const FORMATS: [u32; SLOTS] = [21, 21, 113, 21, 113, 21, 113, 111, 111];
const WORLD: [(u32, u32, bool, bool); SLOTS] = [
    (0x3a8, 21, false, false),
    (0x3b8, 21, false, false),
    (0x378, 113, false, false),
    (0x380, 21, false, false),
    (0x388, 113, false, false),
    (0x3b0, 21, false, false),
    (0x420, 113, true, false),
    (0x590, 0, true, true),
    (0x588, 0, true, false),
];
const TEXTURE_RANGES: [(u32, u32); 6] = [
    (0x658, 16),
    (0x6d8, 4),
    (0x6f8, 16),
    (0x778, 16),
    (0x7f8, 16),
    (0x878, 16),
];

fn layout() -> Layout {
    Layout {
        crt_surface: 0x10,
        crt_rtv: 0x18,
        crt_dsv: 0x20,
        crt_uav: 0x28,
        crt_texture: 0x30,
        crt_width: 0x38,
        crt_height: 0x3c,
        crt_format: 0x40,
        texture_surface: 0x50,
        texture_desc_cache: 0x70,
        texture_desc: 0x78,
        texture_srv: 0xa8,
        target_width: 0x8,
        target_height: 0xc,
        backend_texture_list: 0x640,
        backend_ctable: 0x5c8,
        backend_textures: TEXTURE_RANGES
            .map(|(offset, count)| rsf_anomaly_swap_range { offset, count }),
        ssao_viewport_width: 0x8,
        format_a16b16g16r16f: 113,
        format_r16f: 111,
        format_r32f: 114,
        world: WORLD.map(|(offset, d3d_format, optional, half_allowed)| {
            rsf_anomaly_swap_world_target {
                offset,
                d3d_format,
                optional,
                half_allowed,
            }
        }),
        absent: [0x358, 0x360, 0x368, 0x390],
    }
}

/// Fmatrix::build_projection(fov, aspect, near, far): LH, D3D depth, _34 = 1.
fn perspective(fov: f32, aspect: f32, near: f32) -> Matrix {
    let h = 1.0 / (fov * 0.5).tan();
    let w = h / aspect;
    let q = 1000.0 / (1000.0 - near);
    [
        w,
        0.,
        0.,
        0.,
        0.,
        h,
        0.,
        0.,
        0.,
        0.,
        q,
        1.,
        0.,
        0.,
        -q * near,
        0.,
    ]
}

const VIEW: Matrix = [
    0.8, 0.0, -0.6, 0.0, 0.0, 1.0, 0.0, 0.0, 0.6, 0.0, 0.8, 0.0, -3.0, -1.5, 7.25, 1.0,
];

#[derive(Clone, Copy, PartialEq)]
#[repr(C, align(16))]
struct Engine {
    target: [u8; 0x600],
    crt: [[u8; CRT_SIZE]; SLOTS],
    texture: [[u8; TEXTURE_SIZE]; SLOTS],
    backend: [u8; 0x900],
    device_size: [u32; 2],
    base_zb: usize,
    base_rt: usize,
    viewport: [f32; 6],
    guard: i32,
    view: Matrix,
    project: Matrix,
    full: Matrix,
    sampler_bias: f32,
    objects: [u64; 64],
}

fn put<T: Copy>(bytes: &mut [u8], offset: u32, value: T) {
    assert!(offset as usize + size_of::<T>() <= bytes.len());
    // Bounds checked above; unaligned plain data.
    unsafe {
        bytes
            .as_mut_ptr()
            .add(offset as usize)
            .cast::<T>()
            .write_unaligned(value)
    }
}

fn get<T: Copy>(bytes: &[u8], offset: u32) -> T {
    assert!(offset as usize + size_of::<T>() <= bytes.len());
    // Bounds checked above; every T read here accepts any bit pattern.
    unsafe {
        bytes
            .as_ptr()
            .add(offset as usize)
            .cast::<T>()
            .read_unaligned()
    }
}

#[derive(Default)]
struct Host {
    writes: usize,
    fail_write_at: Option<usize>,
    partial_write_at: Option<usize>,
    fail_address: *mut c_void,
    ignore_write_address: *mut c_void,
    upload_calls: usize,
    fail_upload_at: Option<usize>,
    bias_calls: usize,
    fail_bias_at: Option<usize>,
    refuse_bias: bool,
    uploads: Vec<Matrix>,
    biases: Vec<f32>,
    viewports: Vec<(u32, u32)>,
    makes: usize,
    make_result: u32,
    wrong_format: bool,
    request: Option<Request>,
    drops: usize,
    round_trips: usize,
    fail_pass: bool,
    engine: *const Engine,
    original_before_draw: Option<Engine>,
    seam_views: Option<[Fields; 6]>,
    upscales: usize,
    recovery_inputs: Option<rsf_anomaly_swap_upscale>,
    published: *const View,
    observed_twin_generation: u64,
    positions: Vec<(f32, f32)>,
    constant: *mut c_void,
    refuse_position: bool,
    sampler: usize,
    twin_objects: [[u64; 32]; 2],
}

fn host(user: *mut c_void) -> &'static mut Host {
    // The test owns the host for the whole case and calls on one thread.
    unsafe { &mut *user.cast::<Host>() }
}

pub(crate) unsafe extern "C" fn read(
    _: *mut c_void,
    address: *const c_void,
    output: *mut c_void,
    size: usize,
) -> bool {
    if address.is_null() {
        return false;
    }
    // Test addresses point into the boxed fake engine.
    unsafe { ptr::copy_nonoverlapping(address.cast::<u8>(), output.cast::<u8>(), size) };
    true
}

pub(crate) unsafe extern "C" fn write(
    user: *mut c_void,
    address: *mut c_void,
    input: *const c_void,
    size: usize,
) -> bool {
    let host = host(user);
    let index = host.writes;
    host.writes += 1;
    if address.is_null() || host.fail_address == address || host.fail_write_at == Some(index) {
        return false;
    }
    if host.ignore_write_address == address {
        return true;
    }
    if host.partial_write_at == Some(index) {
        unsafe { ptr::copy_nonoverlapping(input.cast::<u8>(), address.cast::<u8>(), size / 2) };
        return false;
    }
    // Test addresses point into the boxed fake engine.
    unsafe { ptr::copy_nonoverlapping(input.cast::<u8>(), address.cast::<u8>(), size) };
    true
}

pub(crate) unsafe extern "C" fn make_twins(
    user: *mut c_void,
    request: *const Request,
    out: *mut Twins,
) -> u32 {
    let host = host(user);
    host.makes += 1;
    // Both records live for the call.
    let (request, out) = unsafe { (&*request, &mut *out) };
    host.request = Some(*request);
    if host.make_result != RSF_ANOMALY_SWAP_MAKE_CREATED {
        return host.make_result;
    }
    let objects = host.twin_objects.as_mut_ptr().cast::<u64>();
    let object = |index: usize| objects.wrapping_add(index).cast::<c_void>();
    for slot in 0..SLOTS {
        let spec = request.spec[slot];
        if !spec.present {
            continue;
        }
        let f = &mut out.targets[slot];
        f.surface = object(slot * 4);
        f.texture_surface = f.surface;
        f.desc_cache = f.surface;
        f.rtv = object(slot * 4 + 1);
        f.srv = object(slot * 4 + 2);
        f.uav = if request.engine[slot].fields.uav.is_null() {
            null_mut()
        } else {
            object(slot * 4 + 3)
        };
        f.width = spec.width;
        f.height = spec.height;
        f.format = if spec.promote {
            113
        } else {
            request.engine[slot].fields.format
        };
        if host.wrong_format && slot == 0 {
            f.format = 0;
        }
        f.desc = [0x77; 44];
    }
    if request.stage == 2 {
        out.depth_dsv = object(60);
        out.depth_srv = object(61);
        out.depth_width = request.render_width;
        out.depth_height = request.render_height;
        out.base_rt = object(62);
    }
    RSF_ANOMALY_SWAP_MAKE_CREATED
}

pub(crate) unsafe extern "C" fn drop_twins(user: *mut c_void) {
    host(user).drops += 1;
}

unsafe extern "C" fn round_trip(
    user: *mut c_void,
    twin: *const Fields,
    engine: *const Fields,
) -> bool {
    let host = host(user);
    host.round_trips += 1;
    host.original_before_draw = Some(unsafe { *host.engine });
    let mut views = [Fields::default(); 6];
    views[0] = unsafe { *twin };
    views[3] = unsafe { *engine };
    host.seam_views = Some(views);
    !host.fail_pass
}

unsafe extern "C" fn upscale(user: *mut c_void, inputs: *const rsf_anomaly_swap_upscale) -> bool {
    let host = host(user);
    host.upscales += 1;
    host.observed_twin_generation = unsafe { (*host.published).counters.twin_generations };
    let inputs = unsafe { &*inputs };
    host.original_before_draw = Some(unsafe { *host.engine });
    host.seam_views = Some(
        [
            inputs.generic0_twin,
            inputs.position_twin,
            inputs.generic1_twin,
            inputs.generic0,
            inputs.position,
            inputs.generic1,
        ]
        .map(|fields| unsafe { *fields }),
    );
    if inputs.spatial_recovery {
        // The target records are valid during this call.
        for fields in [
            inputs.generic0_twin,
            inputs.position_twin,
            inputs.generic1_twin,
        ] {
            assert!(!unsafe { (*fields).srv }.is_null());
        }
        for fields in [inputs.generic0, inputs.position, inputs.generic1] {
            assert!(!unsafe { (*fields).rtv }.is_null());
        }
        host.recovery_inputs = Some(*inputs);
    }
    !host.fail_pass
}

pub(crate) unsafe extern "C" fn set_viewport(user: *mut c_void, width: u32, height: u32) {
    host(user).viewports.push((width, height));
}

pub(crate) unsafe extern "C" fn upload_projection(
    user: *mut c_void,
    matrix: *const c_void,
) -> bool {
    let host = host(user);
    let call = host.upload_calls;
    host.upload_calls += 1;
    if host.fail_upload_at == Some(call) {
        return false;
    }
    // The owner passes &Device.mProject in the fake engine.
    host.uploads
        .push(unsafe { matrix.cast::<Matrix>().read_unaligned() });
    true
}

pub(crate) unsafe extern "C" fn set_mip_bias(user: *mut c_void, value: f32) -> bool {
    let host = host(user);
    let call = host.bias_calls;
    host.bias_calls += 1;
    if host.refuse_bias || host.fail_bias_at == Some(call) {
        return false;
    }
    host.biases.push(value);
    // SetMipLODBias stores the value in SSManager like the engine.
    unsafe { (host.sampler as *mut f32).write(value) };
    true
}

pub(crate) unsafe extern "C" fn correct_position_constant(
    user: *mut c_void,
    constant: *mut c_void,
    x: f32,
    y: f32,
) -> u32 {
    let host = host(user);
    host.positions.push((x, y));
    host.constant = constant;
    u32::from(!host.refuse_position)
}

struct Case {
    engine: *mut Engine,
    host: *mut Host,
    swap: *mut Handle,
    original: Engine,
    size: [u32; 2],
}

/// The fake engine host under the production names (`sr_swap::host`).
pub(crate) mod swap_host {
    pub(crate) use super::{
        correct_position_constant, drop_twins, make_twins, read, set_mip_bias, set_viewport,
        upload_projection, write,
    };
}

impl Drop for Case {
    fn drop(&mut self) {
        // Each pointer came from Box::into_raw in Case::new.
        unsafe {
            crate::sr_swap::destroy(self.swap);
            drop(Box::from_raw(self.host));
            drop(Box::from_raw(self.engine));
        }
    }
}

impl Case {
    fn new(optional: bool) -> Self {
        // Plain data: all-zero is a valid fake engine before it is filled below.
        let engine: *mut Engine = Box::into_raw(Box::new(unsafe { std::mem::zeroed() }));
        let e = unsafe { &mut *engine };
        let objects = e.objects.as_mut_ptr();
        let object = |index: usize| objects.wrapping_add(index) as usize;
        for slot in 0..SLOTS {
            let present = !WORLD[slot].2 || optional;
            let crt = e.crt[slot].as_mut_ptr() as usize;
            let texture = e.texture[slot].as_mut_ptr() as usize;
            e.crt[slot].fill(0xa0 + slot as u8);
            e.texture[slot].fill(0xc0 + slot as u8);
            let c = &mut e.crt[slot];
            put(c, 0x10, object(slot * 4));
            put(c, 0x18, object(slot * 4 + 1));
            put(c, 0x20, 0usize);
            put(
                c,
                0x28,
                if slot == 0 || slot == 8 {
                    object(slot * 4 + 3)
                } else {
                    0
                },
            );
            put(c, 0x30, texture);
            put(c, 0x38, 1920u32);
            put(c, 0x3c, 1080u32);
            put(c, 0x40, FORMATS[slot]);
            let t = &mut e.texture[slot];
            put(t, 0x50, object(slot * 4));
            put(t, 0x70, object(slot * 4));
            put(t, 0xa8, object(slot * 4 + 2));
            put(&mut e.target, WORLD[slot].0, if present { crt } else { 0 });
        }
        put(&mut e.target, 0x8, 1920u32);
        put(&mut e.target, 0xc, 1080u32);
        e.backend.fill(0x5a);
        e.device_size = [1920, 1080];
        e.base_zb = object(60);
        e.base_rt = object(63);
        e.viewport[5] = 1.0;
        e.project = perspective(1.2, 1920.0 / 1080.0, 0.2);
        e.view = VIEW;
        e.full = engine_mul(&e.project, &e.view);
        e.sampler_bias = 0.25;
        let host = Box::into_raw(Box::new(Host {
            sampler: &raw mut e.sampler_bias as usize,
            engine,
            ..Host::default()
        }));
        let swap = create(&layout(), host.cast());
        assert!(!swap.is_null());
        unsafe { (*host).published = crate::sr_swap::view_of(swap) };
        let case = Self {
            engine,
            host,
            swap,
            original: *e,
            size: [1920, 1080],
        };
        crate::sr_swap::set_enabled(case.swap, true);
        case
    }

    // The fake engine, host and handle are separate boxes the owner and its callbacks reach through raw pointers, as
    // engine memory is reached in the game. Each borrow lasts one statement between owner calls.
    #[allow(clippy::mut_from_ref)]
    fn engine(&self) -> &mut Engine {
        // Owned by the case; only touched between owner calls.
        unsafe { &mut *self.engine }
    }

    #[allow(clippy::mut_from_ref)]
    fn host(&self) -> &mut Host {
        // Owned by the case; only touched between owner calls.
        unsafe { &mut *self.host }
    }

    fn view(&self) -> View {
        unsafe { *crate::sr_swap::view_of(self.swap) }
    }

    /// Every engine byte except RCache, whose texture and constant caches each exchange clears on purpose.
    fn stock(&self) -> bool {
        let mut now = *self.engine();
        now.backend = self.original.backend;
        now == self.original
    }

    fn rebase(&mut self) {
        self.original = *self.engine();
    }

    fn facts(&self) -> Facts {
        let e = self.engine;
        Facts {
            render_target: (&raw mut (unsafe { &mut *e }).target).cast(),
            backend: (&raw mut (unsafe { &mut *e }).backend).cast(),
            device: std::ptr::without_provenance_mut(0x1234),
            device_width: self.size[0],
            device_height: self.size[1],
            ..Facts::default()
        }
    }

    fn facts2(&self) -> Facts {
        self.facts_at_scale(0.67)
    }

    fn facts_at_scale(&self, scale: f32) -> Facts {
        let e = self.engine();
        let [ow, oh] = self.size;
        let plan = crate::sr_stage::math::make_plan(ow, oh, scale).unwrap();
        Facts {
            stage: 2,
            render_width: plan.render_width,
            render_height: plan.render_height,
            device_size: (&raw mut e.device_size).cast(),
            base_zb: (&raw mut e.base_zb).cast(),
            base_rt: (&raw mut e.base_rt).cast(),
            ssao_viewport: (&raw mut e.viewport).cast(),
            ssao_viewport_guard: (&raw mut e.guard).cast(),
            ..self.facts()
        }
    }

    fn facts3(&self, x: f32, y: f32) -> Facts {
        let e = self.engine();
        Facts {
            jitter: true,
            jitter_x: x,
            jitter_y: y,
            device_view: (&raw mut e.view).cast(),
            device_project: (&raw mut e.project).cast(),
            device_full_transform: (&raw mut e.full).cast(),
            mip_bias: true,
            mip_bias_delta: (1286.0f64 / 1920.0).log2() as f32,
            sampler_bias: (&raw mut e.sampler_bias).cast(),
            ..self.facts2()
        }
    }

    fn prepare(&self, facts: &Facts) -> bool {
        crate::sr_swap::prepare(self.swap, Some(facts))
    }

    // The seam operation the stage lends, here the fake host's passes.
    fn seam(&self) -> bool {
        let user = self.host.cast();
        // The fake engine and host outlive the handle.
        unsafe {
            run_seam(self.swap, |pass| match pass {
                SeamPass::RoundTrip(twin, engine) => round_trip(user, twin, engine),
                SeamPass::Upscale(inputs) => upscale(user, inputs),
            })
        }
    }

    fn resize(&mut self, [width, height]: [u32; 2], half: bool) {
        self.size = [width, height];
        let e = self.engine();
        e.device_size = [width, height];
        put(&mut e.target, 0x8, width);
        put(&mut e.target, 0xc, height);
        for (slot, record) in e.crt.iter_mut().enumerate() {
            let divisor = if half && slot == 7 { 2 } else { 1 };
            put(record, 0x38, width / divisor);
            put(record, 0x3c, height / divisor);
        }
        self.rebase();
    }

    fn ssao(&mut self, [width, height]: [f32; 2]) {
        let e = self.engine();
        e.guard = i32::MIN + 1;
        [e.viewport[2], e.viewport[3]] = [width, height];
        self.rebase();
    }

    fn restored(&self, reason: u32) -> u64 {
        self.view().counters.restores[reason as usize]
    }

    fn skipped(&self, reason: u32) -> u64 {
        self.view().counters.skipped[reason as usize]
    }

    fn before_draw_stock(&self) -> bool {
        self.host().original_before_draw.is_some_and(|mut bytes| {
            bytes.backend = self.original.backend;
            bytes == self.original
        })
    }

    fn twin_installed(&self, slot: usize) -> bool {
        let twin = self.view().twins.targets[slot];
        let e = self.engine();
        get::<*mut c_void>(&e.crt[slot], 0x10) == twin.surface
            && get::<*mut c_void>(&e.texture[slot], 0xa8) == twin.srv
            && get::<u32>(&e.crt[slot], 0x40) == twin.format
            && get::<u32>(&e.crt[slot], 0x38) == twin.width
            && get::<u8>(&e.texture[slot], 0x78) == 0x77
            // The reference count and CTexture pointer are never exchanged.
            && get::<u8>(&e.crt[slot], 0) == 0xa0 + slot as u8
    }

    fn texture_cache_cleared(&self) -> bool {
        let backend = &self.engine().backend;
        get::<usize>(backend, 0x640) == 0
            && TEXTURE_RANGES.iter().all(|&(offset, count)| {
                (0..count).all(|i| get::<usize>(backend, offset + i * 8) == 0)
            })
            && backend[0x63f] == 0x5a
    }
}

#[cfg_attr(test, test)]
fn jitter_follows_the_row_vector_rule() {
    let p = perspective(1.2, 1.7778, 0.2);
    let (x, y, w, h) = (0.25f32, -1.0f32 / 6.0, 1286u32, 724u32);
    let j = jitter_projection(&p, x, y, w, h).expect("finite");
    let expected_31 =
        (f64::from(p[8]) + 2.0 * f64::from(x) / f64::from(w) * f64::from(p[11])) as f32;
    let expected_32 =
        (f64::from(p[9]) - 2.0 * f64::from(y) / f64::from(h) * f64::from(p[11])) as f32;
    assert_eq!((j[8], j[9]), (expected_31, expected_32));
    for i in [0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15] {
        assert_eq!(j[i].to_bits(), p[i].to_bits());
    }
    // Clip-space shift of a point: x_clip moves by 2x/W * w, y_clip by -2y/H * w.
    let point = [1.0f32, 2.0, 3.0, 1.0];
    let clip = |m: &Matrix| -> [f64; 4] {
        std::array::from_fn(|c| {
            (0..4)
                .map(|r| f64::from(point[r]) * f64::from(m[r * 4 + c]))
                .sum()
        })
    };
    let (a, b) = (clip(&p), clip(&j));
    assert!((b[0] / b[3] - a[0] / a[3] - 2.0 * f64::from(x) / f64::from(w)).abs() < 1e-6);
    assert!((b[1] / b[3] - a[1] / a[3] + 2.0 * f64::from(y) / f64::from(h)).abs() < 1e-6);
    let mut bad = p;
    bad[5] = f32::INFINITY;
    assert!(jitter_projection(&bad, 0.1, 0.1, w, h).is_none());
    assert!(jitter_projection(&p, f32::NAN, 0.1, w, h).is_none());
    assert!(jitter_projection(&p, 0.1, 0.1, 1, h).is_none());
    assert!(jitter_projection(&p, 0.1, 0.1, w, 0).is_none());
    assert!(jitter_projection(&p, 0.1, 0.1, w, 16385).is_none());
    assert_eq!(jitter_projection(&p, 0.0, 0.0, w, h), Some(p));
}

#[cfg_attr(test, test)]
fn full_transform_composes_row_vectors() {
    let p = perspective(1.2, 1.7778, 0.2);
    let full = engine_mul(&p, &VIEW);
    let times = |v: [f64; 4], m: &Matrix| -> [f64; 4] {
        std::array::from_fn(|c| (0..4).map(|r| v[r] * f64::from(m[r * 4 + c])).sum())
    };
    for v in [[1.0, 2.0, 3.0, 1.0], [-5.0, 0.5, 20.0, 1.0]] {
        let (direct, chained) = (times(v, &full), times(times(v, &VIEW), &p));
        for i in 0..4 {
            assert!((direct[i] - chained[i]).abs() <= 1e-4 * (1.0 + chained[i].abs()));
        }
    }
}

#[cfg_attr(test, test)]
fn first_person_projection_is_jittered_once() {
    let main = jitter_projection(&perspective(1.2, 1.7778, 0.2), 0.25, -0.1, 1286, 724).unwrap();
    let fresh = perspective(0.9, 1.7778, 0.05);
    let fp = jitter_projection(&fresh, 0.25, -0.1, 1286, 724).unwrap();
    assert_eq!(
        first_person_action(&fresh, &main, None),
        FirstPerson::Jitter
    );
    assert_eq!(
        first_person_action(&main, &main, None),
        FirstPerson::AlreadyJittered
    );
    assert_eq!(
        first_person_action(&fp, &main, Some(&fp)),
        FirstPerson::AlreadyJittered
    );
    // Without the record of what was written, a jittered first-person matrix has _31/_32 != 0: left alone.
    assert_eq!(first_person_action(&fp, &main, None), FirstPerson::Foreign);
    let twice = jitter_projection(&fp, 0.25, -0.1, 1286, 724).unwrap();
    assert!((twice[8] - 2.0 * fp[8]).abs() < 1e-8);
    assert_eq!(
        first_person_action(&twice, &main, Some(&fp)),
        FirstPerson::Foreign
    );
}

#[cfg_attr(test, test)]
fn twin_extents_follow_the_engine_division() {
    assert_eq!(twin_extent(1920, 1920, 1286, false), Some((1286, false)));
    assert_eq!(twin_extent(960, 1920, 1286, true), Some((643, true)));
    assert_eq!(twin_extent(960, 1920, 1286, false), None);
    assert_eq!(twin_extent(959, 1919, 1286, true), Some((643, true)));
    assert_eq!(twin_extent(539, 1079, 723, true), Some((361, true)));
    assert_eq!(twin_extent(480, 1920, 1286, true), None);
    assert_eq!(twin_extent(0, 1920, 1286, true), None);
    assert_eq!(twin_extent(1920, 0, 1286, true), None);
    assert_eq!(twin_extent(1920, 1920, 2000, true), None);
    assert_eq!(twin_extent(0, 1, 1, true), None);
    assert_eq!(twin_extent(1, 1, 1, true), Some((1, false)));
    assert_eq!(twin_extent(1, 3, 1, true), None);
    assert_eq!(twin_extent(1920, 1920, 1920, true), Some((1920, false)));
    assert_eq!(twin_extent(960, 1920, 1920, true), Some((960, true)));
}

#[cfg_attr(test, test)]
fn stage1_window_swaps_and_restores_engine_bytes() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts()));
    assert!(c.view().open && c.twin_installed(0) && c.twin_installed(1));
    assert!(c.texture_cache_cleared());
    // Stage 1 leaves the other targets, the constant table and the globals alone.
    assert_eq!(get::<u8>(&c.engine().backend, 0x5c8), 0x5a);
    for slot in 2..SLOTS {
        assert_eq!(
            get::<usize>(&c.engine().crt[slot], 0x10),
            get::<usize>(&c.original.crt[slot], 0x10)
        );
    }
    crate::sr_swap::prepared(c.swap);
    assert!(c.host().viewports.is_empty());
    crate::sr_swap::rm_near(c.swap);
    c.engine().backend.fill(0x5a);
    assert!(c.seam());
    assert!(c.stock() && !c.view().open && c.host().round_trips == 1);
    assert!(c.before_draw_stock() && c.texture_cache_cleared());
    let refs = c.host().seam_views.unwrap();
    assert_eq!(refs[0].surface, c.view().twins.targets[0].surface);
    assert_eq!(
        refs[3].surface,
        get::<*mut c_void>(&c.original.crt[0], 0x10)
    );
    assert_eq!(refs[3].rtv, get::<*mut c_void>(&c.original.crt[0], 0x18));
    assert!(!c.seam());
    crate::sr_swap::combine_end(c.swap);
    crate::sr_swap::frame_end(c.swap);
    crate::sr_swap::rm_near(c.swap);
    assert!(c.stock() && c.view().refusal.is_null());
    let n = c.view().counters;
    assert_eq!(
        (n.prepares, n.swapped, n.seams, n.seams_without_window),
        (1, 1, 1, 1)
    );
    assert_eq!(n.restores[RSF_ANOMALY_SWAP_RESTORE_SEAM as usize], 1);
    assert_eq!(
        (n.eligible, n.rm_near_in_window, n.twin_generations),
        (1, 1, 1)
    );
    for _ in 0..3 {
        assert!(c.prepare(&c.facts()) && c.seam() && c.stock());
    }
    assert_eq!((c.host().makes, c.view().counters.swapped), (1, 4));
    let request = c.host().request.unwrap();
    assert!(request.stage == 1 && request.spec[0].promote && !request.spec[2].present);
    crate::sr_swap::shutdown(c.swap);
    assert!(c.stock() && c.host().drops == 1);
}

#[cfg_attr(test, test)]
fn upscale_can_read_the_published_twin_generation_during_the_seam() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts2()));
    crate::sr_swap::prepared(c.swap);
    let generation = c.view().counters.twin_generations;
    assert!(generation > 0);
    assert!(c.seam());
    assert_eq!(c.host().observed_twin_generation, generation);
    assert!(c.stock());
}

#[cfg_attr(test, test)]
fn stage3_window_jitters_main_and_first_person_once() {
    let mut c = Case::new(true);
    c.engine().guard = i32::MIN + 7;
    c.engine().viewport[2] = 960.0;
    c.engine().viewport[3] = 540.0;
    c.rebase();
    let main = c.engine().project;
    let (x, y) = (0.25f32, -1.0f32 / 6.0);
    assert!(c.prepare(&c.facts3(x, y)));
    let jittered = jitter_projection(&main, x, y, 1286, 724).unwrap();
    assert_eq!(c.engine().project, jittered);
    assert_eq!(c.engine().full, engine_mul(&jittered, &VIEW));
    assert_eq!(c.host().uploads, [jittered]);
    assert!(c.view().jitter_applied && c.view().mip_bias_applied);
    assert_eq!(c.engine().device_size, [1286, 724]);
    assert_eq!(
        (c.engine().viewport[2], c.engine().viewport[3]),
        (643.0, 362.0)
    );
    assert!((0..SLOTS).all(|slot| c.twin_installed(slot)));
    crate::sr_swap::prepared(c.swap);
    assert_eq!(c.host().viewports, [(1286, 724)]);
    // A first-person site rebuilds the projection and calls rmNear: jittered once, never twice.
    let saved = (c.engine().project, c.engine().full);
    let hud = perspective(0.9, 1920.0 / 1080.0, 0.05);
    c.engine().project = hud;
    c.engine().full = engine_mul(&hud, &VIEW);
    crate::sr_swap::rm_near(c.swap);
    let hud_jittered = jitter_projection(&hud, x, y, 1286, 724).unwrap();
    assert_eq!(c.engine().project, hud_jittered);
    crate::sr_swap::rm_near(c.swap);
    assert_eq!(c.host().uploads.len(), 2);
    (c.engine().project, c.engine().full) = saved;
    crate::sr_swap::rm_near(c.swap);
    let n = c.view().counters;
    assert_eq!(
        (n.fp_jittered, n.fp_already, n.rm_near_in_window),
        (1, 2, 3)
    );
    // The deferred position constant receives the window's raster jitter.
    crate::sr_swap::position_constant(c.swap, null_mut());
    assert_eq!(c.host().positions, [(x, y)]);
    assert!(c.seam());
    assert!(c.stock() && c.host().uploads.last() == Some(&main));
    assert_eq!(c.host().biases.last(), Some(&0.25));
    assert_eq!(c.host().viewports.last(), Some(&(1920, 1080)));
    assert!(c.view().refusal.is_null());
}

#[cfg_attr(test, test)]
fn sun_cascades_borrow_the_unjittered_camera() {
    let c = Case::new(true);
    let (main, full) = (c.engine().project, c.engine().full);
    assert!(c.prepare(&c.facts3(0.25, 0.1)));
    let (jittered, jittered_full) = (c.engine().project, c.engine().full);
    let token = crate::sr_swap::sun_begin(c.swap);
    assert!(token);
    assert_eq!((c.engine().project, c.engine().full), (main, full));
    // Reentrant draws neither re-jitter nor take a second token.
    crate::sr_swap::rm_near(c.swap);
    assert_eq!(c.engine().project, main);
    let nested = crate::sr_swap::sun_begin(c.swap);
    assert!(!nested);
    crate::sr_swap::sun_end(c.swap, nested);
    assert_eq!(c.engine().project, main);
    crate::sr_swap::sun_end(c.swap, token);
    assert_eq!(
        (c.engine().project, c.engine().full),
        (jittered, jittered_full)
    );
    assert_eq!(c.host().uploads.last(), Some(&jittered));
    let n = c.view().counters;
    assert_eq!(
        (n.sun_calls, n.sun_unjittered, n.sun_restored, n.sun_refused),
        (2, 1, 1, 0)
    );
    assert!(c.seam() && c.stock() && c.view().refusal.is_null());
    // Quiesce during the wrapped call invalidates the token.
    assert!(c.prepare(&c.facts3(0.25, 0.1)));
    let token = crate::sr_swap::sun_begin(c.swap);
    crate::sr_swap::shutdown(c.swap);
    let uploads = c.host().upload_calls;
    crate::sr_swap::sun_end(c.swap, token);
    assert!(c.stock() && c.host().upload_calls == uploads);
}

#[cfg_attr(test, test)]
fn failed_first_person_jitter_restores_the_camera_and_uploaded_projection() {
    for fail_upload in [false, true] {
        let c = Case::new(true);
        assert!(c.prepare(&c.facts3(0.25, 0.1)));
        let main = (c.engine().project, c.engine().full);
        let hud = perspective(0.9, 1920.0 / 1080.0, 0.05);
        let full = engine_mul(&hud, &VIEW);
        (c.engine().project, c.engine().full) = (hud, full);
        if fail_upload {
            c.host().fail_upload_at = Some(c.host().upload_calls);
        } else {
            c.host().fail_write_at = Some(c.host().writes + 1);
        }
        crate::sr_swap::rm_near(c.swap);
        assert_eq!((c.engine().project, c.engine().full), (hud, full));
        assert_eq!(c.host().uploads.last(), Some(&hud));
        assert_eq!(c.view().counters.fp_jittered, 0);
        assert!(!c.view().refusal.is_null());
        (c.engine().project, c.engine().full) = main;
        assert!(c.seam() && c.stock());
    }
}

#[cfg_attr(test, test)]
fn pending_camera_and_sampler_restoration_retains_twins_until_quiesce_retries() {
    let c = Case::new(true);
    let saved = (c.engine().project, c.engine().full);
    let facts = c.facts3(0.25, 0.1);
    assert!(c.prepare(&facts));
    c.host().fail_address = (&raw mut c.engine().full).cast();
    c.host().refuse_bias = true;
    assert!(!c.seam());
    assert_eq!(c.host().upscales, 0);
    assert!(!crate::sr_swap::shutdown(c.swap));
    assert_eq!(c.host().drops, 0);

    // Another H1 cannot erase the pending camera or sampler snapshot.
    assert!(!c.prepare(&facts));
    c.host().fail_address = null_mut();
    assert!(!crate::sr_swap::shutdown(c.swap));
    assert_eq!((c.engine().project, c.engine().full), saved);
    assert_eq!(c.host().drops, 0);

    c.host().refuse_bias = false;
    assert!(crate::sr_swap::shutdown(c.swap));
    assert_eq!(c.engine().sampler_bias, 0.25);
    assert_eq!(c.host().drops, 1);
    assert_eq!(
        c.view().counters.restores[RSF_ANOMALY_SWAP_RESTORE_SEAM as usize],
        1
    );
}

#[cfg_attr(test, test)]
fn sun_refusal_returns_the_frame_to_output_size() {
    for failure in 0..3 {
        let c = Case::new(true);
        assert!(c.prepare(&c.facts3(0.25, 0.1)));
        crate::sr_swap::prepared(c.swap);
        // u_setrt inside the window set CRenderTarget::dwWidth/dwHeight to the twin size.
        put(&mut c.engine().target, 0x8, 1286u32);
        put(&mut c.engine().target, 0xc, 724u32);
        let token = if failure == 0 {
            c.host().fail_write_at = Some(c.host().writes);
            crate::sr_swap::sun_begin(c.swap)
        } else {
            let token = crate::sr_swap::sun_begin(c.swap);
            assert!(token);
            if failure == 1 {
                c.host().fail_write_at = Some(c.host().writes);
            } else {
                c.host().fail_upload_at = Some(c.host().upload_calls);
            }
            token
        };
        crate::sr_swap::sun_end(c.swap, token);
        assert!(c.view().open && !c.view().refusal.is_null());
        assert_eq!(c.host().viewports, [(1286, 724)]);
        assert!(c.seam());
        assert!(c.stock(), "failure {failure}");
        assert!(!c.view().open && !c.view().refusal.is_null());
        assert_eq!(c.host().viewports, [(1286, 724), (1920, 1080)]);
        let n = c.view().counters;
        assert_eq!(n.restores[RSF_ANOMALY_SWAP_RESTORE_SEAM as usize], 1);
        assert_eq!((n.sun_refused, n.sun_restored), (1, 0));
        assert_eq!(c.host().upscales, 1);
        let inputs = c.host().recovery_inputs.expect("spatial seam");
        assert_eq!(inputs.depth_srv, c.view().twins.depth_srv);
        assert_eq!(inputs.engine_dsv as usize, c.engine().base_zb);
        assert_eq!((inputs.render_width, inputs.render_height), (1286, 724));
        assert_eq!((inputs.output_width, inputs.output_height), (1920, 1080));
    }
}

#[cfg_attr(test, test)]
fn scope_frames_stay_stock() {
    let c = Case::new(true);
    let facts = Facts {
        scope: true,
        ..c.facts3(0.25, 0.1)
    };
    assert!(!c.prepare(&facts));
    let writes = c.host().writes;
    assert!(!crate::sr_swap::sun_begin(c.swap));
    crate::sr_swap::rm_near(c.swap);
    crate::sr_swap::position_constant(c.swap, null_mut());
    assert!(!c.seam());
    crate::sr_swap::combine_end(c.swap);
    assert!(c.stock() && c.host().writes == writes && c.host().makes == 0);
    let n = c.view().counters;
    assert_eq!(n.skipped[RSF_ANOMALY_SWAP_SKIP_SCOPE as usize], 1);
    assert_eq!(
        (n.position_stock, n.rm_near_in_window, n.sun_unjittered),
        (1, 0, 0)
    );
    assert!(c.view().refusal.is_null() && !c.view().jitter_applied);
    // The next main-view frame opens normally.
    assert!(c.prepare(&c.facts3(0.25, 0.1)) && c.seam() && c.stock());
}

#[cfg_attr(test, test)]
fn a_failed_exchange_rolls_every_write_back() {
    let probe = Case::new(true);
    probe.engine().guard = 1;
    assert!(probe.prepare(&probe.facts2()));
    // 9 slots of 11 field writes, Device width and height, HW.pBaseZB, HW.pBaseRT and the viewport pair, then the
    // texture list, six texture ranges and the constant table.
    let exchange = SLOTS * 11 + 5;
    assert_eq!(probe.host().writes, exchange + 8);
    for at in 0..exchange + 8 {
        let mut c = Case::new(true);
        c.engine().guard = 1;
        c.rebase();
        c.host().fail_write_at = Some(at);
        assert!(!c.prepare(&c.facts2()));
        assert!(
            c.stock() && !c.view().open && !c.view().refusal.is_null(),
            "write {at}"
        );
        assert_eq!(
            c.view().counters.restores[RSF_ANOMALY_SWAP_RESTORE_ROLLBACK as usize],
            1
        );
        assert!(c.host().viewports.is_empty());
    }
}

#[cfg_attr(test, test)]
fn a_partially_written_cache_range_rolls_back_before_drawing() {
    let c = Case::new(true);
    // 99 target fields, four globals, then the texture-list pointer and PS range.
    c.host().partial_write_at = Some(SLOTS * 11 + 5);
    assert!(!c.prepare(&c.facts2()));
    assert!(c.stock() && !c.view().open && c.texture_cache_cleared());
    assert_eq!(c.view().counters.swapped, 0);
    let mut expected = c.original.backend;
    expected[0x640..0x648].fill(0);
    expected[0x5c8..0x5d0].fill(0);
    for (offset, count) in TEXTURE_RANGES {
        let offset = offset as usize;
        expected[offset..offset + count as usize * POINTER as usize].fill(0);
    }
    assert_eq!(c.engine().backend, expected);
}

#[cfg_attr(test, test)]
fn cache_readback_refusal_keeps_twins_until_the_range_is_clear() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts2()));
    let (offset, _) = TEXTURE_RANGES[0];
    put(&mut c.engine().backend, offset, 1usize);
    c.host().ignore_write_address = c
        .engine()
        .backend
        .as_mut_ptr()
        .wrapping_add(offset as usize)
        .cast();
    assert!(!c.seam());
    assert!(!crate::sr_swap::shutdown(c.swap));
    assert_eq!((c.host().drops, c.host().upscales), (0, 0));

    c.host().ignore_write_address = null_mut();
    assert!(crate::sr_swap::shutdown(c.swap));
    assert!(c.stock() && c.texture_cache_cleared());
    assert_eq!(c.host().drops, 1);
}

#[cfg_attr(test, test)]
fn a_failed_restore_keeps_twins_until_shutdown_can_verify_the_fields() {
    for global in [false, true] {
        let c = Case::new(true);
        assert!(c.prepare(&c.facts2()));
        let address = if global {
            (&raw mut c.engine().base_zb).cast()
        } else {
            c.engine().crt[GENERIC0]
                .as_mut_ptr()
                .wrapping_add(layout().crt_surface as usize)
                .cast()
        };
        c.host().fail_address = address;
        assert!(!c.seam());
        assert!(!c.view().open && !c.stock());
        assert_eq!(
            c.view().counters.restores,
            [0; RSF_ANOMALY_SWAP_RESTORE_COUNT as usize]
        );
        assert!(!crate::sr_swap::shutdown(c.swap));
        assert_eq!((c.host().drops, c.host().upscales), (0, 0));
        assert!(!c.prepare(&c.facts2()));
        c.host().fail_address = null_mut();
        if !global {
            let (offset, _, _, _) = WORLD[GENERIC0];
            let saved = get::<usize>(&c.engine().target, offset);
            put(&mut c.engine().target, offset, 0usize);
            assert!(!crate::sr_swap::shutdown(c.swap));
            assert_eq!(c.host().drops, 0);
            put(&mut c.engine().target, offset, saved);
        }
        assert!(crate::sr_swap::shutdown(c.swap));
        assert!(c.stock());
        assert_eq!(c.host().drops, 1);
        assert_eq!(
            c.view().counters.restores[RSF_ANOMALY_SWAP_RESTORE_SEAM as usize],
            1
        );
        assert!(crate::sr_swap::shutdown(c.swap));
        assert_eq!(c.host().drops, 1);
    }
}

#[cfg_attr(test, test)]
fn layout_and_platform_are_checked_at_creation() {
    let mut wide = layout();
    wide.backend_textures[0].count = MAX_CACHE_SLOTS + 1;
    let c = Case::new(true);
    assert!(create(&wide, c.host.cast()).is_null());
    let swap = create(&layout(), c.host.cast());
    assert!(!swap.is_null());
    crate::sr_swap::set_enabled(swap, true);
    assert!(crate::sr_swap::prepare(swap, Some(&c.facts())));
    let mut drawn = false;
    // The fixture keeps the fake engine and the handle live through the call.
    assert!(unsafe {
        run_seam(swap, |pass| {
            assert!(matches!(pass, SeamPass::RoundTrip(_, _)));
            assert!(c.stock());
            drawn = true;
            true
        })
    });
    assert!(drawn && c.host().round_trips == 0);
    assert!(crate::sr_swap::shutdown(swap));
    unsafe { crate::sr_swap::destroy(swap) };
    assert!(!crate::sr_swap::prepare(null_mut(), Some(&c.facts())));
}

/// The production engine layout (sr_profile.rs) matches this fixture's independent copy.
#[cfg_attr(test, test)]
fn engine_layout_matches_the_fixture() {
    let native = crate::sr_profile::rsf_anomaly_sr_engine_layout();
    let fixture = layout();
    macro_rules! equal {
        ($($field:ident),+ $(,)?) => {
            $(assert_eq!(native.$field, fixture.$field, stringify!($field));)+
        };
    }
    equal!(
        crt_surface,
        crt_rtv,
        crt_dsv,
        crt_uav,
        crt_texture,
        crt_width,
        crt_height,
        crt_format,
        texture_surface,
        texture_desc_cache,
        texture_desc,
        texture_srv,
        target_width,
        target_height,
        backend_texture_list,
        backend_ctable,
        ssao_viewport_width,
        format_a16b16g16r16f,
        format_r16f,
        format_r32f,
        absent,
    );
    for (a, b) in native.backend_textures.iter().zip(fixture.backend_textures) {
        assert_eq!((a.offset, a.count), (b.offset, b.count));
    }
    for (a, b) in native.world.iter().zip(fixture.world) {
        assert_eq!(
            (a.offset, a.d3d_format, a.optional, a.half_allowed),
            (b.offset, b.d3d_format, b.optional, b.half_allowed)
        );
    }
}
