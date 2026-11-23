// SPDX-License-Identifier: MIT
use super::*;
use crate::sys::rsf_vkfg_platform as ImageOps;
use rsf_game_sys::rsf_frame_record;
use std::cell::RefCell;
use std::collections::BTreeSet;

const DEVICE: usize = 0x1000;
const OUTPUT: u64 = 0x2000;
const MOTION: u64 = 0x3000;
const DEPTH: u64 = 0x4000;
const EXPOSURE: u64 = 0x5000;

#[derive(Default)]
struct Fake {
    events: Vec<String>,
    next: u64,
    live: BTreeSet<u64>,
    inactive: bool,
    fail: Option<&'static str>,
    passes: Vec<(usize, PassCopy)>,
    facts: Vec<(u32, f32, [u32; 2])>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PassCopy {
    set: u64,
    pipeline: u64,
    sampled: [u64; 4],
    groups: [u32; 2],
    scratch_initialized: u32,
}

thread_local! {
    static FAKE: RefCell<Fake> = RefCell::new(Fake::default());
}

fn with<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
    FAKE.with_borrow_mut(f)
}

fn events() -> Vec<String> {
    with(|fake| std::mem::take(&mut fake.events))
}

fn create(kind: &'static str, out: *mut u64) -> i32 {
    with(|fake| {
        if fake.fail == Some(kind) {
            unsafe { out.write(0) };
            return -2;
        }
        fake.next += 1;
        let handle = 0x10_0000 + fake.next;
        fake.live.insert(handle);
        fake.events.push(format!("+{kind}"));
        unsafe { out.write(handle) };
        0
    })
}

fn destroy(kind: &str, handle: u64) {
    with(|fake| {
        assert!(fake.live.remove(&handle), "{kind} {handle:#x} was not live");
        fake.events.push(format!("-{kind}"));
    })
}

fn leaf() -> *mut c_void {
    ptr::dangling_mut::<u64>().cast()
}

unsafe extern "C" fn local_types(_: *mut c_void) -> u32 {
    1
}
unsafe extern "C" fn create_image(
    _: *mut c_void,
    _: u32,
    _: u32,
    format: u32,
    usage: u32,
    out: *mut u64,
) -> i32 {
    assert_eq!((format, usage), (RGBA16_FLOAT, SCRATCH_USAGE));
    create("image", out)
}
unsafe extern "C" fn requirements(_: *mut c_void, _: u64, size: *mut u64, types: *mut u32) {
    unsafe {
        size.write(256);
        types.write(1);
    }
}
unsafe extern "C" fn allocate(_: *mut c_void, _: u64, _: u32, out: *mut u64) -> i32 {
    create("memory", out)
}
unsafe extern "C" fn bind(_: *mut c_void, _: u64, _: u64) -> i32 {
    0
}
unsafe extern "C" fn scratch_view(_: *mut c_void, _: u64, _: u32, out: *mut u64) -> i32 {
    create("scratch-view", out)
}
unsafe extern "C" fn destroy_view(_: *mut c_void, view: u64) {
    destroy("view", view);
}
unsafe extern "C" fn destroy_image(_: *mut c_void, image: u64) {
    destroy("image", image);
}
unsafe extern "C" fn free_memory(_: *mut c_void, memory: u64) {
    destroy("memory", memory);
}

static IMAGES: ImageOps = ImageOps {
    local_memory_types: Some(local_types),
    create_image: Some(create_image),
    requirements: Some(requirements),
    allocate: Some(allocate),
    bind: Some(bind),
    create_view: Some(scratch_view),
    destroy_view: Some(destroy_view),
    destroy_image: Some(destroy_image),
    free_memory: Some(free_memory),
    create_fence: None,
    destroy_fence: None,
    wait_fence: None,
    reset_fence: None,
    submit_fence: None,
};

unsafe extern "C" fn open(setup: *const Setup, out: *mut *mut c_void) -> i32 {
    assert_eq!(unsafe { (*setup).device } as usize, DEVICE);
    with(|fake| {
        fake.events.push("open".into());
        if fake.fail == Some("open") {
            return -3;
        }
        unsafe { out.write(leaf()) };
        0
    })
}
unsafe extern "C" fn close(_: *mut c_void) {
    with(|fake| fake.events.push("close".into()));
}
unsafe extern "C" fn image_user(device: *mut c_void) -> *mut c_void {
    device
}
unsafe extern "C" fn create_layouts(_: *mut c_void, out: *mut Layouts) -> i32 {
    let out = unsafe { &mut *out };
    match create("set-layout", &mut out.descriptor_set) {
        0 => create("pipeline-layout", &mut out.pipeline),
        error => error,
    }
}
unsafe extern "C" fn release_layouts(_: *mut c_void, layouts: *mut Layouts) {
    let layouts = unsafe { &mut *layouts };
    for handle in [layouts.pipeline, layouts.descriptor_set] {
        if handle != 0 {
            destroy("layout", handle);
        }
    }
    *layouts = Layouts::default();
}
unsafe extern "C" fn create_pipeline(_: *mut c_void, layout: u64, mode: u32, out: *mut u64) -> i32 {
    assert!(with(|fake| fake.live.contains(&layout)));
    let result = create(if mode == 2 { "pipeline2" } else { "pipeline" }, out);
    with(|fake| fake.events.push(format!("mode{mode}")));
    result
}
unsafe extern "C" fn destroy_pipeline(_: *mut c_void, pipeline: u64) {
    destroy("pipeline", pipeline);
}
unsafe extern "C" fn create_uniform(_: *mut c_void, layout: u64, out: *mut Uniform) -> i32 {
    assert!(with(|fake| fake.live.contains(&layout)));
    let out = unsafe { &mut *out };
    for (kind, handle) in [
        ("pool", &mut out.pool),
        ("set", &mut out.set),
        ("buffer", &mut out.buffer),
        ("uniform-memory", &mut out.memory),
    ] {
        let result = create(kind, handle);
        if result != 0 {
            return result;
        }
    }
    out.mapped = leaf();
    0
}
unsafe extern "C" fn release_uniform(_: *mut c_void, uniform: *mut Uniform) {
    let uniform = unsafe { &mut *uniform };
    // The descriptor set is freed with its pool.
    if uniform.set != 0 {
        with(|fake| assert!(fake.live.remove(&uniform.set)));
    }
    for handle in [uniform.buffer, uniform.memory, uniform.pool] {
        if handle != 0 {
            destroy("uniform", handle);
        }
    }
    *uniform = Uniform::default();
}
unsafe extern "C" fn input_view(_: *mut c_void, image: u64, _: u32, out: *mut u64) -> i32 {
    create(if image == DEPTH { "depth-view" } else { "view" }, out)
}
unsafe extern "C" fn options(request: *const Options, out: *mut Options) -> u32 {
    let Some(request) = (unsafe { request.as_ref() }) else {
        return 0;
    };
    unsafe { out.write(*request) };
    u32::from(with(|fake| !fake.inactive))
}
unsafe extern "C" fn write_constants(mapped: *mut c_void, _: *const Options, facts: *const Facts) {
    assert!(!mapped.is_null());
    let facts = unsafe { &*facts };
    with(|fake| {
        fake.facts.push((
            facts.effective_mode,
            facts.exposure,
            [facts.width, facts.height],
        ))
    });
}
unsafe extern "C" fn record(_: *mut c_void, commands: *mut c_void, pass: *const Pass) {
    let pass = unsafe { &*pass };
    with(|fake| {
        for handle in [
            pass.set,
            pass.buffer,
            pass.pipeline,
            pass.scratch,
            pass.scratch_view,
        ] {
            assert!(fake.live.contains(&handle));
        }
        fake.passes.push((
            commands as usize,
            PassCopy {
                set: pass.set,
                pipeline: pass.pipeline,
                sampled: pass.sampled,
                groups: [pass.groups_x, pass.groups_y],
                scratch_initialized: pass.scratch_initialized,
            },
        ));
    });
}
const PLATFORM: &Platform = &Platform {
    images: &IMAGES,
    open: Some(open),
    close: Some(close),
    image_user: Some(image_user),
    create_layouts: Some(create_layouts),
    release_layouts: Some(release_layouts),
    create_pipeline: Some(create_pipeline),
    destroy_pipeline: Some(destroy_pipeline),
    create_uniform: Some(create_uniform),
    release_uniform: Some(release_uniform),
    create_view: Some(input_view),
    options: Some(options),
    write_constants: Some(write_constants),
    record: Some(record),
};

struct Harness {
    owner: *mut Owner,
    setup: Setup,
    record: Box<rsf_frame_record>,
}

impl Harness {
    fn new() -> Self {
        with(|fake| *fake = Fake::default());
        let mut setup: Setup = unsafe { std::mem::zeroed() };
        setup.device = DEVICE as *mut c_void;
        let mut record: Box<rsf_frame_record> = Box::new(unsafe { std::mem::zeroed() });
        record.output_width = 33;
        record.output_height = 17;
        Self {
            owner: unsafe { rsf_core_sharpenvk_create(PLATFORM) },
            setup,
            record,
        }
    }

    fn frame(&self, mode: u32) -> Frame {
        let mut frame: Frame = unsafe { std::mem::zeroed() };
        frame.record = &*self.record;
        frame.pre_exposure = 2.0;
        frame.sharpening = Options {
            struct_size: size_of::<Options>() as u32,
            abi_version: 1,
            mode,
            sharpness: 0.5,
            ..unsafe { std::mem::zeroed() }
        };
        let image = |image, format, width, height| Image {
            device: DEVICE as *mut c_void,
            image,
            format,
            width,
            height,
            layout: SHADER_READ,
            copy_source: 0,
        };
        frame.motion = image(MOTION, 83, 33, 17);
        frame.depth = image(DEPTH, R32_FLOAT, 33, 17);
        frame
    }

    fn run(&self, commands: usize, frame: &Frame) -> bool {
        unsafe {
            rsf_core_sharpenvk_record(
                self.owner,
                &self.setup,
                commands as *mut c_void,
                frame,
                OUTPUT,
            ) != 0
        }
    }

    fn retire(&self, commands: usize) {
        unsafe { rsf_core_sharpenvk_retire(self.owner, commands as *mut c_void) };
    }

    fn clear(&self) {
        unsafe { rsf_core_sharpenvk_clear(self.owner) };
    }

    fn dispatches(&self) -> u64 {
        unsafe { self.owner.as_ref() }.map_or(0, |owner| owner.dispatches)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        unsafe { rsf_core_sharpenvk_destroy(self.owner) };
        with(|fake| assert!(fake.live.is_empty(), "leaked {:x?}", fake.live));
    }
}

#[test]
fn inactive_options_touch_nothing() {
    let h = Harness::new();
    with(|fake| fake.inactive = true);
    assert!(h.run(0, &h.frame(1)));
    let blank: Frame = unsafe { std::mem::zeroed() };
    assert!(h.run(0, &blank));
    assert!(events().is_empty());
    assert_eq!(h.dispatches(), 0);
}

#[test]
fn admission_refuses_invalid_inputs_before_binding_the_device() {
    let h = Harness::new();
    let cases: [fn(&mut Frame); 6] = [
        |f| f.record = ptr::null(),
        |f| f.motion.layout = 1,
        |f| f.depth.device = ptr::null_mut(),
        |f| f.depth.height = 0,
        |f| {
            f.exposure = f.depth;
            f.exposure.image = EXPOSURE;
            f.exposure.width = 2;
        },
        |f| {
            f.exposure = f.depth;
            f.exposure.image = EXPOSURE;
            f.exposure.width = 1;
            f.exposure.height = 1;
            f.exposure.format = RGBA16_FLOAT;
        },
    ];
    for (index, edit) in cases.into_iter().enumerate() {
        let mut frame = h.frame(1);
        edit(&mut frame);
        assert!(!h.run(1, &frame), "case {index}");
    }
    assert!(!h.run(0, &h.frame(2)));
    assert!(
        unsafe {
            rsf_core_sharpenvk_record(h.owner, &h.setup, ptr::dangling_mut(), &h.frame(1), 0)
        } == 0
    );
    let mut zero = h.record.clone();
    zero.output_width = 0;
    let mut frame = h.frame(1);
    frame.record = &*zero;
    assert!(!h.run(1, &frame));
    assert!(events().is_empty(), "no device work before admission");
}

#[test]
fn records_the_effective_filter_and_reuses_retired_slots() {
    let h = Harness::new();
    let mut frame = h.frame(2);
    frame.exposure = frame.depth;
    frame.exposure.image = EXPOSURE;
    frame.exposure.width = 1;
    frame.exposure.height = 1;
    frame.exposure.format = R16_FLOAT;
    assert!(h.run(7, &frame));
    let created = events();
    assert_eq!(
        created[..8],
        [
            "open",
            "+set-layout",
            "+pipeline-layout",
            "+pipeline",
            "mode1",
            "+pipeline2",
            "mode2",
            "+pipeline"
        ]
    );
    let (commands, depth) = with(|fake| fake.passes[0]);
    assert_eq!(commands, 7);
    assert_eq!(depth.groups, [3, 2]);
    assert_eq!(depth.scratch_initialized, 0);
    assert!(depth.sampled.iter().all(|view| *view != 0));

    // Depth RCAS without depth falls back to the RCAS pipeline and binds
    // the output view in place of the absent inputs.
    frame.depth.image = 0;
    frame.exposure.image = 0;
    frame.pre_exposure = f32::NAN;
    assert!(h.run(8, &frame));
    let (_, fallback) = with(|fake| fake.passes[1]);
    assert_ne!(
        fallback.set, depth.set,
        "in-flight recordings use separate slots"
    );
    assert_ne!(fallback.pipeline, depth.pipeline);
    assert_eq!(fallback.sampled[2], fallback.sampled[0]);
    assert_eq!(fallback.sampled[3], fallback.sampled[0]);
    assert_ne!(fallback.sampled[1], fallback.sampled[0]);
    let facts = with(|fake| fake.facts.clone());
    assert_eq!((facts[0].0, facts[0].1, facts[0].2), (2, 0.5, [33, 17]));
    assert_eq!(facts[1].0, 1);
    assert!(facts[1].1.is_nan());

    events();
    h.retire(7);
    h.retire(0);
    h.retire(99);
    assert_eq!(events(), ["-view", "-view", "-view", "-view"]);
    assert!(h.run(9, &h.frame(3)));
    let (_, reused) = with(|fake| fake.passes[2]);
    assert_eq!(reused.set, depth.set, "the retired slot is reused");
    assert_eq!(reused.scratch_initialized, 1);
    assert_eq!(h.dispatches(), 3);
}

#[test]
fn slots_are_bounded_until_retired() {
    let h = Harness::new();
    let frame = h.frame(1);
    for commands in 1..=64 {
        assert!(h.run(commands, &frame));
    }
    assert!(!h.run(65, &frame));
    h.retire(3);
    assert!(h.run(65, &frame));
    assert_eq!(h.dispatches(), 65);
}

#[test]
fn failed_binding_latches_until_clear() {
    let h = Harness::new();
    let frame = h.frame(1);
    with(|fake| fake.fail = Some("pipeline2"));
    assert!(!h.run(1, &frame));
    with(|fake| fake.fail = None);
    assert!(!h.run(1, &frame));
    assert_eq!(events().iter().filter(|e| *e == "open").count(), 1);
    h.clear();
    assert_eq!(
        events(),
        ["-pipeline", "-layout", "-layout", "close"],
        "partial pipelines and layouts are released"
    );
    assert!(h.run(1, &frame));
}

#[test]
fn unavailable_device_latches_and_a_bound_device_refuses_another() {
    let h = Harness::new();
    let frame = h.frame(1);
    with(|fake| fake.fail = Some("open"));
    assert!(!h.run(1, &frame));
    with(|fake| fake.fail = None);
    assert!(!h.run(1, &frame));
    h.clear();
    assert_eq!(events(), ["open"]);
    assert!(h.run(1, &frame));
    let mut moved = h.setup;
    moved.device = (DEVICE + 1) as *mut c_void;
    let mut foreign = frame;
    foreign.motion.image = 0;
    foreign.depth.image = 0;
    assert!(
        unsafe { rsf_core_sharpenvk_record(h.owner, &moved, 2 as *mut c_void, &foreign, OUTPUT) }
            == 0
    );
    assert_eq!(h.dispatches(), 1);
}

#[test]
fn refused_preparation_and_views_free_the_slot() {
    let h = Harness::new();
    let frame = h.frame(2);
    with(|fake| fake.fail = Some("memory"));
    assert!(!h.run(1, &frame));
    with(|fake| fake.fail = Some("depth-view"));
    assert!(!h.run(1, &frame));
    with(|fake| fake.fail = None);
    let live = with(|fake| fake.live.len());
    // Device objects plus one prepared slot: four uniform handles, scratch
    // image, memory and view. No input view survives the refusal.
    assert_eq!(live, 5 + 7);
    assert!(h.run(2, &frame));
    assert_eq!(with(|fake| fake.passes.len()), 1);
    assert_eq!(h.dispatches(), 1);
}

#[test]
fn clear_releases_slots_before_pipelines_and_device() {
    let h = Harness::new();
    let frame = h.frame(1);
    assert!(h.run(1, &frame));
    assert!(h.run(2, &frame));
    events();
    h.clear();
    let released = events();
    let close = released.iter().position(|e| e == "close").unwrap();
    let first_pipeline = released.iter().position(|e| e == "-pipeline").unwrap();
    let last_slot = released.iter().rposition(|e| e == "-uniform").unwrap();
    assert!(last_slot < first_pipeline && first_pipeline < close);
    assert_eq!(close, released.len() - 1);
    assert!(with(|fake| fake.live.is_empty()));
    h.clear();
    h.retire(1);
    assert!(events().is_empty());
}
