// SPDX-License-Identifier: MIT
use super::*;
use std::sync::atomic::{AtomicPtr, Ordering};

// The SDK session is process-wide. Serialize the tests that load it.
static SERIAL: Mutex<()> = Mutex::new(());

type Edit = fn(&mut Frame);

#[derive(Debug, PartialEq)]
enum Call {
    Load,
    Unload,
    Requirements,
    Features,
    Open,
    Create,
    Init(u32, u32),
    Bind,
    Provider(u64),
    Allocate(u64, u32, u32, u32),
    Velocity,
    Barrier(Barrier),
    Copy(ImageCopy),
    Execute(u32, u32, u32, u32, u32),
    Parameters(u32),
    Capture(String, u64),
    Sharpen,
    Log(String),
    Retire(usize),
    Flush,
    Destroy,
    Clear,
    ReleaseView(u64),
    ReleaseImage(u64),
    ReleaseMemory(u64),
    Close,
}

struct Fake {
    calls: Vec<Call>,
    range: [u32; 6],
    requirements: i32,
    velocity: i32,
    execute: i32,
    destroy: i32,
    sharpen: i32,
    fail_allocation: u32,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            range: [1130, 636, 1024, 576, 1280, 720],
            requirements: XESS_SUCCESS,
            velocity: XESS_SUCCESS,
            execute: XESS_SUCCESS,
            destroy: XESS_SUCCESS,
            sharpen: 1,
            fail_allocation: 0,
        }
    }
}

static STATE: AtomicPtr<Fake> = AtomicPtr::new(ptr::null_mut());

fn state<'a>() -> &'a mut Fake {
    unsafe { &mut *STATE.load(Ordering::Relaxed) }
}

fn push(call: Call) {
    state().calls.push(call);
}

macro_rules! leaf {
    (|$($arg:tt: $ty:ty),*| -> $ret:ty $body:block) => {{
        unsafe extern "C" fn callback($($arg: $ty),*) -> $ret $body
        callback
    }};
}

fn handle() -> Handle {
    ptr::dangling_mut::<u64>().cast()
}

static IMAGES: ImageOps = ImageOps {
    local_memory_types: Some(leaf!(|_: Handle| -> u32 { 1 })),
    create_image: Some(leaf!(|_: Handle,
                              width: u32,
                              height: u32,
                              _: u32,
                              usage: u32,
                              image: *mut u64|
     -> i32 {
        let state = state();
        let id = 10
            + state
                .calls
                .iter()
                .filter(|call| matches!(call, Call::Allocate(..)))
                .count() as u64;
        state.calls.push(Call::Allocate(id, width, height, usage));
        unsafe { image.write(id) };
        0
    })),
    requirements: Some(leaf!(|_: Handle,
                              image: u64,
                              size: *mut u64,
                              types: *mut u32|
     -> () {
        unsafe {
            size.write(image);
            types.write(1);
        }
    })),
    allocate: Some(leaf!(|_: Handle,
                          size: u64,
                          _: u32,
                          memory: *mut u64|
     -> i32 {
        unsafe { memory.write(size + 100) };
        0
    })),
    bind: Some(leaf!(|_: Handle, _: u64, _: u64| -> i32 { 0 })),
    create_view: Some(leaf!(|_: Handle,
                             image: u64,
                             _: u32,
                             view: *mut u64|
     -> i32 {
        if state().fail_allocation as u64 == image {
            return -2;
        }
        unsafe { view.write(image + 200) };
        0
    })),
    destroy_view: Some(leaf!(|_: Handle, view: u64| -> () {
        push(Call::ReleaseView(view));
    })),
    destroy_image: Some(leaf!(|_: Handle, image: u64| -> () {
        push(Call::ReleaseImage(image));
    })),
    free_memory: Some(leaf!(|_: Handle, memory: u64| -> () {
        push(Call::ReleaseMemory(memory));
    })),
    create_fence: None,
    destroy_fence: None,
    wait_fence: None,
    reset_fence: None,
    submit_fence: None,
};

pub(in crate::vulkan_sr) static OPS: XessOps = XessOps {
    loader: Loader {
        load: leaf!(|_: *const u16| -> Handle {
            push(Call::Load);
            handle()
        }),
        unload: leaf!(|_: Handle| -> () { push(Call::Unload) }),
    },
    allocate_table: leaf!(|out: &mut Handle| -> i32 {
        *out = handle();
        0
    }),
    pin: leaf!(|_: Handle| -> u32 { 1 }),
    drop_table: leaf!(|_: Handle| -> () {}),
    instance_requirements: leaf!(|_: Handle,
                                  count: *mut u32,
                                  _: Names,
                                  minimum: *mut u32|
     -> i32 {
        push(Call::Requirements);
        unsafe {
            count.write(2);
            minimum.write(7);
        }
        state().requirements
    }),
    device_extensions: leaf!(
        |_: Handle, _: Handle, _: Handle, count: *mut u32, _: Names| -> i32 {
            push(Call::Requirements);
            unsafe { count.write(3) };
            state().requirements
        }
    ),
    device_features: leaf!(|_: Handle, _: Handle, _: Handle, _: *mut Handle| -> i32 {
        push(Call::Features);
        XESS_SUCCESS
    }),
    version: leaf!(|_: Handle, version: &mut [u16; 4]| -> i32 {
        *version = [2, 1, 3, 4];
        XESS_SUCCESS
    }),
    open: leaf!(|library: Handle, out: &mut Handle| -> i32 {
        push(Call::Open);
        *out = library;
        0
    }),
    close: leaf!(|_: Handle| -> () { push(Call::Close) }),
    create_context: leaf!(|_: Handle, _: &Setup| -> i32 {
        push(Call::Create);
        XESS_SUCCESS
    }),
    initialize: leaf!(|_: Handle, init: &XessInit| -> i32 {
        push(Call::Init(init.quality, init.flags));
        XESS_SUCCESS
    }),
    resolution: leaf!(|_: Handle, _: &XessInit, range: &mut [u32; 6]| -> i32 {
        *range = state().range;
        XESS_SUCCESS
    }),
    destroy_context: leaf!(|_: Handle| -> i32 {
        push(Call::Destroy);
        state().destroy
    }),
    bind_device: leaf!(|_: Handle, _: &Setup| -> i32 {
        push(Call::Bind);
        0
    }),
    image_user: leaf!(|native: Handle| -> Handle { native }),
    images: &IMAGES,
    velocity: leaf!(|_: Handle, _: f32, _: f32| -> i32 {
        push(Call::Velocity);
        state().velocity
    }),
    barrier: leaf!(|_: Handle, _: Handle, plan: *const Barrier| -> () {
        push(Call::Barrier(unsafe { *plan }));
    }),
    copy: leaf!(|_: Handle, _: Handle, plan: *const ImageCopy| -> () {
        push(Call::Copy(unsafe { *plan }));
    }),
    execute: leaf!(|_: Handle, _: Handle, call: &XessExecute| -> i32 {
        assert_eq!(
            [call.color.width, call.depth.height, call.motion.width],
            [1280, 720, 1280]
        );
        assert_eq!([call.color.view, call.output.view], [210, 213]);
        assert_eq!(call.exposure_scale, 0.5);
        push(Call::Execute(
            call.input_width,
            call.input_height,
            call.output.width,
            call.output.height,
            call.reset,
        ));
        state().execute
    }),
    sharpen: leaf!(
        |_: Handle, _: &Setup, _: Handle, _: &Frame, output: u64| -> i32 {
            assert_eq!(output, 13);
            push(Call::Sharpen);
            state().sharpen
        }
    ),
    clear_sharpen: leaf!(|_: Handle| -> () { push(Call::Clear) }),
    capture_provider: leaf!(|_: Handle, version: u64, backend: u32| -> () {
        assert_eq!(backend, RSF_SR_XESS);
        push(Call::Provider(version))
    }),
    capture_record: leaf!(|_: Handle,
                           _: Handle,
                           image: &Image,
                           _: &rsf_frame_record,
                           role: *const c_char,
                           _: u32|
     -> () {
        let role = unsafe { CStr::from_ptr(role) }
            .to_string_lossy()
            .into_owned();
        push(Call::Capture(role, image.image))
    }),
    capture_parameters: leaf!(|_: &Frame, backend: u32, reset: u32| -> () {
        assert_eq!(backend, RSF_SR_XESS);
        push(Call::Parameters(reset))
    }),
    retire_commands: leaf!(|_: Handle, commands: Handle| -> () {
        push(Call::Retire(commands as usize))
    }),
    flush_capture: leaf!(|_: Handle| -> () { push(Call::Flush) }),
    bind_entries: leaf!(|_: Handle, _: &Entries| -> () {}),
    lookup: Some(leaf!(|_: Handle, _: *const c_char| -> Handle { handle() })),
};

unsafe extern "C" fn proc(_: Handle, _: *const c_char) -> rsf_vk_function {
    None
}

unsafe extern "C" fn frame_log(_: Handle, message: *const c_char) {
    push(Call::Log(
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned(),
    ));
}

fn setup() -> Setup {
    Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        instance: handle(),
        physical_device: handle(),
        device: handle(),
        instance_proc: Some(proc),
        device_proc: Some(proc),
        backend: RSF_SR_XESS,
        quality: 5,
        output_width: 1920,
        output_height: 1080,
        depth_inverted: 1,
        depth_infinite: 0,
        motion_jittered: 0,
        runtime_directory_utf8: c"C:\\payload".as_ptr(),
    }
}

fn requirements() -> XessSetup {
    XessSetup {
        struct_size: size_of::<XessSetup>() as u32,
        abi_version: REQUIREMENTS_ABI_VERSION,
        runtime_directory_utf8: c"C:\\payload".as_ptr(),
    }
}

fn record([width, height]: [u32; 2], frame: u64) -> rsf_frame_record {
    let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 1;
    record.view_id = 1;
    record.resource_generation = 1;
    record.frame_id = frame;
    record.render_width = width;
    record.render_height = height;
    record.output_width = 1920;
    record.output_height = 1080;
    record.frame_time_ms = 16.0;
    let camera = &mut record.camera;
    camera.struct_size = size_of_val(camera) as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = width;
    camera.render_height = height;
    camera.output_width = 1920;
    camera.output_height = 1080;
    camera.depth_inverted = 1;
    camera.near_plane = 0.1;
    camera.far_plane = 100.0;
    camera.vertical_fov_radians = 1.0;
    camera.jitter_pixels = [0.25, -0.5];
    record
}

fn frame(record: &rsf_frame_record) -> Frame {
    let image = |id, format| Image {
        device: handle(),
        image: id,
        format,
        width: record.render_width,
        height: record.render_height,
        layout: SHADER_READ,
        copy_source: 1,
    };
    Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        record,
        color: image(20, RGBA16_FLOAT),
        depth: image(21, R32_FLOAT),
        motion: image(22, RG16_FLOAT),
        output: Image {
            width: 1920,
            height: 1080,
            layout: TRANSFER_DST,
            ..image(23, RGBA16_FLOAT)
        },
        motion_scale_x: 1.0,
        motion_scale_y: -1.0,
        pre_exposure: 2.0,
        log: Some(frame_log),
        ..unsafe { std::mem::zeroed() }
    }
}

fn with_state(test: impl FnOnce(&mut Fake)) {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut fake = Fake::default();
    STATE.store(&mut fake, Ordering::Relaxed);
    test(&mut fake);
    STATE.store(ptr::null_mut(), Ordering::Relaxed);
}

#[test]
fn session_admits_one_sdk_and_maps_requirement_results() {
    with_state(|fake| unsafe {
        let mut setup = requirements();
        setup.struct_size -= 1;
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), ptr::null()), INVALID);
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &setup), INVALID);
        setup.struct_size += 1;
        setup.abi_version += 1;
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &setup), ABI_MISMATCH);
        setup.abi_version -= 1;
        setup.runtime_directory_utf8 = c"".as_ptr();
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &setup), INVALID);
        setup.runtime_directory_utf8 = c"relative".as_ptr();
        assert_eq!(rsf_core_xessvk_initialize(None, &setup), NOT_COMPILED);
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &setup), INVALID);

        let (mut count, mut names, mut minimum) = (9, ptr::dangling::<*const c_char>(), 9);
        assert_eq!(
            rsf_core_xessvk_instance_requirements(Some(&OPS), &mut count, &mut names, &mut minimum),
            NOT_READY
        );
        assert_eq!((count, names.is_null(), minimum), (0, true, 0));
        assert_eq!(
            rsf_core_xessvk_instance_requirements(None, &mut count, &mut names, &mut minimum),
            NOT_COMPILED
        );
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);

        let setup = requirements();
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &setup), 0);
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &setup), NOT_READY);
        assert_eq!(
            rsf_core_xessvk_instance_requirements(Some(&OPS), &mut count, &mut names, &mut minimum),
            0
        );
        assert_eq!((count, minimum), (2, 7));
        // Warnings are not success for the creation chain.
        fake.requirements = 1;
        assert_eq!(
            rsf_core_xessvk_instance_requirements(Some(&OPS), &mut count, &mut names, &mut minimum),
            INIT_FAILED
        );
        let mut features = ptr::null_mut();
        fake.requirements = XESS_UNSUPPORTED_DRIVER;
        let (instance, physical) = (handle(), handle());
        assert_eq!(
            rsf_core_xessvk_device_requirements(
                Some(&OPS),
                instance,
                physical,
                &mut count,
                &mut names,
                &mut features
            ),
            NOT_SUPPORTED
        );
        fake.requirements = XESS_SUCCESS;
        assert_eq!(
            rsf_core_xessvk_device_requirements(
                Some(&OPS),
                instance,
                physical,
                &mut count,
                &mut names,
                &mut features
            ),
            0
        );
        assert_eq!(count, 3);
        assert_eq!(
            rsf_core_xessvk_device_requirements(
                Some(&OPS),
                ptr::null_mut(),
                physical,
                &mut count,
                &mut names,
                &mut features
            ),
            INVALID
        );
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
        assert_eq!(
            fake.calls,
            [
                Call::Load,
                Call::Requirements,
                Call::Requirements,
                Call::Requirements,
                Call::Requirements,
                Call::Features,
                Call::Unload
            ]
        );
    });
}

#[test]
fn owner_sizes_the_cache_and_keys_history_on_identity_and_extent() {
    with_state(|fake| unsafe {
        let mut owner = ptr::null_mut();
        let setup = setup();
        assert_eq!(
            rsf_core_xessvk_create(Some(&OPS), &setup, &mut owner),
            NOT_READY
        );
        assert!(owner.is_null());
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &requirements()), 0);
        assert_eq!(
            rsf_core_xessvk_create(None, &setup, &mut owner),
            NOT_COMPILED
        );
        fake.calls.clear();
        assert_eq!(rsf_core_xessvk_create(Some(&OPS), &setup, &mut owner), 0);
        assert_eq!(
            fake.calls,
            [
                Call::Open,
                Call::Create,
                Call::Init(104, XESS_AUTOEXPOSURE | XESS_INVERTED_DEPTH),
                Call::Bind,
                Call::Provider(0x0002_0001_0003_0004),
                Call::Allocate(10, 1280, 720, 6),
                Call::Allocate(11, 1280, 720, 6),
                Call::Allocate(12, 1280, 720, 6),
                Call::Allocate(13, 1920, 1080, 15),
            ]
        );
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), NOT_READY);
        let (mut width, mut height) = (0, 0);
        assert_eq!(rsf_core_xessvk_plan(owner, &mut width, &mut height), 0);
        assert_eq!((width, height), (1130, 636));

        let commands = ptr::dangling_mut::<u16>().cast();
        let run = |fake: &mut Fake, extent, id, edit: &dyn Fn(&mut Frame)| {
            let record = record(extent, id);
            let mut input = frame(&record);
            edit(&mut input);
            fake.calls.clear();
            rsf_core_xessvk_evaluate(owner, commands, &input)
        };
        let extent = [1128, 632];
        let output_copy = ImageCopy::transfer(13, 23, [1920, 1080]);
        assert_eq!(run(fake, extent, 1, &|_| {}), 0);
        assert_eq!(
            fake.calls,
            [
                Call::Log(
                    "XeSS input range recommended 1130x636 minimum 1024x576 maximum 1280x720 actual 1128x632"
                        .into()
                ),
                Call::Velocity,
                Call::Barrier(Barrier::new(20, SHADER_READ, TRANSFER_SRC)),
                Call::Barrier(Barrier::new(10, UNDEFINED, TRANSFER_DST)),
                Call::Copy(ImageCopy::transfer(20, 10, extent)),
                Call::Barrier(Barrier::new(20, TRANSFER_SRC, SHADER_READ)),
                Call::Barrier(Barrier::new(10, TRANSFER_DST, SHADER_READ)),
                Call::Barrier(Barrier::new(21, SHADER_READ, TRANSFER_SRC)),
                Call::Barrier(Barrier::new(11, UNDEFINED, TRANSFER_DST)),
                Call::Copy(ImageCopy::transfer(21, 11, extent)),
                Call::Barrier(Barrier::new(21, TRANSFER_SRC, SHADER_READ)),
                Call::Barrier(Barrier::new(11, TRANSFER_DST, SHADER_READ)),
                Call::Barrier(Barrier::new(22, SHADER_READ, TRANSFER_SRC)),
                Call::Barrier(Barrier::new(12, UNDEFINED, TRANSFER_DST)),
                Call::Copy(ImageCopy::transfer(22, 12, extent)),
                Call::Barrier(Barrier::new(22, TRANSFER_SRC, SHADER_READ)),
                Call::Barrier(Barrier::new(12, TRANSFER_DST, SHADER_READ)),
                Call::Barrier(Barrier::new(13, UNDEFINED, GENERAL)),
                Call::Execute(1128, 632, 1920, 1080, 1),
                Call::Parameters(1),
                Call::Capture("sr-source-jittered".into(), 20),
                Call::Capture("sr-linear-color".into(), 20),
                Call::Capture("sr-depth".into(), 21),
                Call::Capture("sr-motion-uv".into(), 22),
                Call::Sharpen,
                Call::Capture("sr-output-linear".into(), 13),
                Call::Barrier(Barrier::new(13, GENERAL, TRANSFER_SRC)),
                Call::Copy(output_copy),
                Call::Barrier(Barrier::new(13, TRANSFER_SRC, GENERAL)),
            ]
        );
        assert_eq!(run(fake, extent, 2, &|_| {}), 0);
        assert!(
            fake.calls
                .contains(&Call::Execute(1128, 632, 1920, 1080, 0))
        );
        assert!(
            fake.calls
                .contains(&Call::Barrier(Barrier::new(10, SHADER_READ, TRANSFER_DST)))
                && fake
                    .calls
                    .contains(&Call::Barrier(Barrier::new(13, GENERAL, GENERAL)))
        );
        // A changed active extent inside the SDK range resets history.
        assert_eq!(run(fake, [1280, 720], 3, &|_| {}), 0);
        assert!(
            fake.calls
                .contains(&Call::Execute(1280, 720, 1920, 1080, 1))
        );

        let refusals: [([u32; 2], Edit); 10] = [
            ([1023, 632], |_| {}),
            ([1128, 575], |_| {}),
            ([1281, 632], |_| {}),
            ([1128, 721], |_| {}),
            ([1128, 632], |frame| frame.depth.width -= 1),
            ([1128, 632], |frame| frame.depth.format = D32_FLOAT),
            ([1128, 632], |frame| frame.motion.copy_source = 0),
            ([1128, 632], |frame| frame.motion.image = frame.depth.image),
            ([1128, 632], |frame| frame.output.image = frame.color.image),
            ([1128, 632], |frame| frame.pre_exposure = f32::from_bits(1)),
        ];
        for (extent, edit) in refusals {
            assert_eq!(run(fake, extent, 4, &edit), INVALID);
            assert!(fake.calls.is_empty());
        }
        let mut jitter = record([1128, 632], 4);
        jitter.camera.jitter_pixels[0] = 0.51;
        let input = frame(&jitter);
        assert_eq!(rsf_core_xessvk_evaluate(owner, commands, &input), INVALID);
        assert_eq!(
            rsf_core_xessvk_evaluate(owner, ptr::null_mut(), &input),
            INVALID
        );

        // Velocity refusal records nothing and keeps history.
        fake.velocity = -4;
        assert_eq!(run(fake, [1280, 720], 4, &|_| {}), FEATURE_FAILED);
        assert_eq!(fake.calls, [Call::Velocity]);
        fake.velocity = XESS_SUCCESS;
        fake.execute = -6;
        assert_eq!(run(fake, [1280, 720], 4, &|_| {}), FEATURE_FAILED);
        assert_eq!(
            fake.calls.last(),
            Some(&Call::Execute(1280, 720, 1920, 1080, 0)),
        );
        // A refused dispatch forgets history, even for the next sequential frame.
        fake.execute = XESS_SUCCESS;
        fake.sharpen = 0;
        assert_eq!(run(fake, [1280, 720], 5, &|_| {}), 0);
        assert!(
            fake.calls
                .contains(&Call::Execute(1280, 720, 1920, 1080, 1))
        );
        assert!(fake.calls.contains(&Call::Copy(output_copy)));
        assert_eq!(run(fake, [1280, 720], 6, &|_| {}), 0);
        assert!(fake.calls.contains(&Call::Copy(output_copy)));
        assert!(
            fake.calls
                .contains(&Call::Execute(1280, 720, 1920, 1080, 0))
        );
        let mut flagged = |frame: u64| {
            let mut value = record([1280, 720], frame);
            value.flags = RSF_FRAME_FLAG_RESET;
            let input = frame_input(&value);
            fake.calls.clear();
            assert_eq!(rsf_core_xessvk_evaluate(owner, commands, &input), 0);
            fake.calls
                .contains(&Call::Execute(1280, 720, 1920, 1080, 1))
        };
        assert!(flagged(7));

        rsf_core_xessvk_retire_commands(owner, commands);
        assert_eq!(fake.calls.last(), Some(&Call::Retire(commands as usize)));
        fake.calls.clear();
        fake.destroy = -6;
        assert_eq!(rsf_core_xessvk_destroy(owner), FEATURE_FAILED);
        assert_eq!(fake.calls, [Call::Flush, Call::Destroy]);
        fake.destroy = XESS_SUCCESS;
        fake.calls.clear();
        assert_eq!(rsf_core_xessvk_destroy(owner), 0);
        assert_eq!(
            fake.calls,
            [
                Call::Flush,
                Call::Destroy,
                Call::Clear,
                Call::ReleaseView(210),
                Call::ReleaseImage(10),
                Call::ReleaseMemory(110),
                Call::ReleaseView(211),
                Call::ReleaseImage(11),
                Call::ReleaseMemory(111),
                Call::ReleaseView(212),
                Call::ReleaseImage(12),
                Call::ReleaseMemory(112),
                Call::ReleaseView(213),
                Call::ReleaseImage(13),
                Call::ReleaseMemory(113),
                Call::Close,
            ]
        );
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
    });
}

fn frame_input(record: &rsf_frame_record) -> Frame {
    frame(record)
}

#[test]
fn refused_creation_unwinds_partial_state_and_releases_the_sdk_reference() {
    with_state(|fake| unsafe {
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &requirements()), 0);
        let mut owner = ptr::null_mut();
        let mut setup = setup();
        setup.backend = RSF_SR_FSR3;
        assert_eq!(
            rsf_core_xessvk_create(Some(&OPS), &setup, &mut owner),
            INVALID
        );
        setup.backend = RSF_SR_XESS;
        // The recommendation must sit inside the SDK's own range.
        fake.range = [1000, 636, 1024, 576, 1280, 720];
        fake.calls.clear();
        assert_eq!(
            rsf_core_xessvk_create(Some(&OPS), &setup, &mut owner),
            INIT_FAILED
        );
        assert_eq!(
            fake.calls,
            [
                Call::Open,
                Call::Create,
                Call::Init(104, XESS_AUTOEXPOSURE | XESS_INVERTED_DEPTH),
                Call::Clear,
                Call::Destroy,
                Call::Close,
            ]
        );
        fake.range = Fake::default().range;
        fake.fail_allocation = 12;
        fake.calls.clear();
        assert_eq!(
            rsf_core_xessvk_create(Some(&OPS), &setup, &mut owner),
            INIT_FAILED
        );
        assert!(owner.is_null());
        assert!(fake.calls.ends_with(&[
            Call::ReleaseImage(12),
            Call::ReleaseMemory(112),
            Call::Clear,
            Call::Destroy,
            Call::ReleaseView(210),
            Call::ReleaseImage(10),
            Call::ReleaseMemory(110),
            Call::ReleaseView(211),
            Call::ReleaseImage(11),
            Call::ReleaseMemory(111),
            Call::Close,
        ]));
        // No context keeps the SDK reference after a refused creation.
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
    });
}

#[test]
fn shared_srvk_owner_routes_xess_and_refuses_presets() {
    unsafe extern "C" fn sharpening(_: &rsf_sharpening_options) -> i32 {
        1
    }
    with_state(|fake| unsafe {
        let platform = &super::super::owner_tests::PLATFORM;
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &requirements()), 0);
        let mut owner = ptr::null_mut();
        assert_eq!(
            rsf_core_srvk_create(Some(platform), &setup(), &mut owner),
            0
        );
        let owner = owner.cast::<Owner>();
        let (mut width, mut height) = (0, 0);
        assert_eq!(rsf_core_srvk_plan(owner, &mut width, &mut height), 0);
        assert_eq!((width, height), (1130, 636));
        assert_eq!(rsf_core_srvk_dlss_preset(owner, 1), NOT_SUPPORTED);
        let record = record([1100, 600], 1);
        let input = frame(&record);
        fake.calls.clear();
        let commands = ptr::dangling_mut::<u32>().cast();
        assert_eq!(
            rsf_core_srvk_evaluate(owner, commands, &input, sharpening),
            0
        );
        assert!(
            fake.calls
                .contains(&Call::Execute(1100, 600, 1920, 1080, 1))
        );
        rsf_core_srvk_retire_commands(owner, commands);
        assert_eq!(fake.calls.last(), Some(&Call::Retire(commands as usize)));
        assert_eq!(rsf_core_srvk_destroy(owner), 0);
        assert_eq!(fake.calls.last(), Some(&Call::Close));
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
    });
}
