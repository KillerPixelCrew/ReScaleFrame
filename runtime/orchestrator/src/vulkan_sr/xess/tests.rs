// SPDX-License-Identifier: MIT
use super::super::owner_tests::PLATFORM;
use super::*;
use std::cell::RefCell;

// The SDK session is process-wide. Serialize the tests that load it.
static SERIAL: Mutex<()> = Mutex::new(());

type Edit = fn(&mut Frame);

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Load,
    Unload,
    Requirements,
    Features,
    Open,
    Create,
    Init(i32, u32),
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

thread_local! {
    static STATE: RefCell<Fake> = RefCell::new(Fake::default());
}

fn edit_state<R>(edit: impl FnOnce(&mut Fake) -> R) -> R {
    STATE.with(|state| edit(&mut state.borrow_mut()))
}

fn calls() -> Vec<Call> {
    edit_state(|fake| fake.calls.clone())
}

fn push(call: Call) {
    edit_state(|fake| fake.calls.push(call));
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
        let id = edit_state(|state| {
            let id = 10
                + state
                    .calls
                    .iter()
                    .filter(|call| matches!(call, Call::Allocate(..)))
                    .count() as u64;
            state.calls.push(Call::Allocate(id, width, height, usage));
            id
        });
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
        if edit_state(|fake| fake.fail_allocation) as u64 == image {
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
        edit_state(|fake| fake.requirements)
    }),
    device_extensions: leaf!(
        |_: Handle, _: Handle, _: Handle, count: *mut u32, _: Names| -> i32 {
            push(Call::Requirements);
            unsafe { count.write(3) };
            edit_state(|fake| fake.requirements)
        }
    ),
    device_features: leaf!(|_: Handle, _: Handle, _: Handle, _: *mut Handle| -> i32 {
        push(Call::Features);
        XESS_SUCCESS
    }),
    version: leaf!(|_: Handle, version: &mut xess_version_t| -> i32 {
        *version = xess_version_t {
            major: 2,
            minor: 1,
            patch: 3,
            reserved: 4,
        };
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
        push(Call::Init(init.qualitySetting, init.initFlags));
        XESS_SUCCESS
    }),
    resolution: leaf!(|_: Handle, _: &XessInit, range: *mut xess_2d_t| -> i32 {
        let [rx, ry, min_x, min_y, max_x, max_y] = edit_state(|fake| fake.range);
        unsafe {
            range.write(xess_2d_t { x: rx, y: ry });
            range.add(1).write(xess_2d_t { x: min_x, y: min_y });
            range.add(2).write(xess_2d_t { x: max_x, y: max_y });
        }
        XESS_SUCCESS
    }),
    destroy_context: leaf!(|_: Handle| -> i32 {
        push(Call::Destroy);
        edit_state(|fake| fake.destroy)
    }),
    bind_device: leaf!(|_: Handle, _: &Setup| -> i32 {
        push(Call::Bind);
        0
    }),
    image_user: leaf!(|native: Handle| -> Handle { native }),
    images: &IMAGES,
    velocity: leaf!(|_: Handle, _: f32, _: f32| -> i32 {
        push(Call::Velocity);
        edit_state(|fake| fake.velocity)
    }),
    barrier: leaf!(|_: Handle, _: Handle, plan: *const Barrier| -> () {
        push(Call::Barrier(unsafe { *plan }));
    }),
    copy: leaf!(|_: Handle, _: Handle, plan: *const ImageCopy| -> () {
        push(Call::Copy(unsafe { *plan }));
    }),
    execute: leaf!(|_: Handle, _: Handle, call: &XessExecute| -> i32 {
        assert_eq!(
            [
                call.colorTexture.width,
                call.depthTexture.height,
                call.velocityTexture.width
            ],
            [1280, 720, 1280]
        );
        assert_eq!(
            [
                call.colorTexture.imageView as usize,
                call.outputTexture.imageView as usize
            ],
            [210, 213]
        );
        assert_eq!(call.exposureScale, 0.5);
        push(Call::Execute(
            call.inputWidth,
            call.inputHeight,
            call.outputTexture.width,
            call.outputTexture.height,
            call.resetHistory,
        ));
        edit_state(|fake| fake.execute)
    }),
    sharpen: leaf!(
        |_: Handle, _: &Setup, _: Handle, _: &Frame, output: u64| -> i32 {
            assert_eq!(output, 13);
            push(Call::Sharpen);
            edit_state(|fake| fake.sharpen)
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

fn with_state(test: impl FnOnce()) {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    edit_state(|fake| *fake = Fake::default());
    test();
}

#[test]
fn session_admits_one_sdk_and_maps_requirement_results() {
    with_state(|| unsafe {
        let mut setup = requirements();
        setup.struct_size -= 1;
        assert_eq!(
            rsf_core_xessvk_initialize(Some(&OPS), ptr::null()),
            INVALID_ARGUMENT
        );
        assert_eq!(
            rsf_core_xessvk_initialize(Some(&OPS), &setup),
            INVALID_ARGUMENT
        );
        setup.struct_size += 1;
        setup.abi_version += 1;
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &setup), ABI_MISMATCH);
        setup.abi_version -= 1;
        setup.runtime_directory_utf8 = c"".as_ptr();
        assert_eq!(
            rsf_core_xessvk_initialize(Some(&OPS), &setup),
            INVALID_ARGUMENT
        );
        setup.runtime_directory_utf8 = c"relative".as_ptr();
        assert_eq!(rsf_core_xessvk_initialize(None, &setup), NOT_COMPILED);
        assert_eq!(
            rsf_core_xessvk_initialize(Some(&OPS), &setup),
            INVALID_ARGUMENT
        );

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
        edit_state(|fake| fake.requirements = 1);
        assert_eq!(
            rsf_core_xessvk_instance_requirements(Some(&OPS), &mut count, &mut names, &mut minimum),
            INIT_FAILED
        );
        let mut features = ptr::null_mut();
        edit_state(|fake| fake.requirements = XESS_UNSUPPORTED_DRIVER);
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
        edit_state(|fake| fake.requirements = XESS_SUCCESS);
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
            INVALID_ARGUMENT
        );
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
        assert_eq!(
            calls(),
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
    with_state(|| unsafe {
        let mut owner = ptr::null_mut();
        let setup = setup();
        assert_eq!(
            rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner),
            NOT_READY
        );
        assert!(owner.is_null());
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &requirements()), 0);
        assert_eq!(rsf_core_srvk_create(None, &setup, &mut owner), NOT_COMPILED);
        edit_state(|fake| fake.calls.clear());
        assert_eq!(rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner), 0);
        assert_eq!(
            calls(),
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
        assert_eq!(rsf_srvk_plan(owner.cast(), &mut width, &mut height), 0);
        assert_eq!((width, height), (1130, 636));

        let commands = ptr::dangling_mut::<u16>().cast();
        let run = |extent, id, edit: &dyn Fn(&mut Frame)| {
            let record = record(extent, id);
            let mut input = frame(&record);
            edit(&mut input);
            edit_state(|fake| fake.calls.clear());
            rsf_srvk_evaluate(owner.cast(), commands, &input)
        };
        let extent = [1128, 632];
        let output_copy = ImageCopy::transfer(13, 23, [1920, 1080]);
        assert_eq!(run(extent, 1, &|_| {}), 0);
        assert_eq!(
            calls(),
            [
                Call::Log(
                    "XeSS input range recommended 1130x636 minimum 1024x576 maximum 1280x720 actual 1128x632"
                        .into()
                ),
                Call::Velocity,
                Call::Barrier(Barrier::same_queue(20, SHADER_READ, TRANSFER_SRC)),
                Call::Barrier(Barrier::same_queue(10, UNDEFINED, TRANSFER_DST)),
                Call::Copy(ImageCopy::transfer(20, 10, extent)),
                Call::Barrier(Barrier::same_queue(20, TRANSFER_SRC, SHADER_READ)),
                Call::Barrier(Barrier::same_queue(10, TRANSFER_DST, SHADER_READ)),
                Call::Barrier(Barrier::same_queue(21, SHADER_READ, TRANSFER_SRC)),
                Call::Barrier(Barrier::same_queue(11, UNDEFINED, TRANSFER_DST)),
                Call::Copy(ImageCopy::transfer(21, 11, extent)),
                Call::Barrier(Barrier::same_queue(21, TRANSFER_SRC, SHADER_READ)),
                Call::Barrier(Barrier::same_queue(11, TRANSFER_DST, SHADER_READ)),
                Call::Barrier(Barrier::same_queue(22, SHADER_READ, TRANSFER_SRC)),
                Call::Barrier(Barrier::same_queue(12, UNDEFINED, TRANSFER_DST)),
                Call::Copy(ImageCopy::transfer(22, 12, extent)),
                Call::Barrier(Barrier::same_queue(22, TRANSFER_SRC, SHADER_READ)),
                Call::Barrier(Barrier::same_queue(12, TRANSFER_DST, SHADER_READ)),
                Call::Barrier(Barrier::same_queue(13, UNDEFINED, GENERAL)),
                Call::Execute(1128, 632, 1920, 1080, 1),
                Call::Parameters(1),
                Call::Capture("sr-source-jittered".into(), 20),
                Call::Capture("sr-linear-color".into(), 20),
                Call::Capture("sr-depth".into(), 21),
                Call::Capture("sr-motion-uv".into(), 22),
                Call::Capture("sr-output-linear".into(), 13),
                Call::Sharpen,
                Call::Barrier(Barrier::same_queue(13, GENERAL, TRANSFER_SRC)),
                Call::Copy(output_copy),
                Call::Barrier(Barrier::same_queue(13, TRANSFER_SRC, GENERAL)),
            ]
        );
        assert_eq!(run(extent, 2, &|_| {}), 0);
        assert!(calls().contains(&Call::Execute(1128, 632, 1920, 1080, 0)));
        assert!(
            calls().contains(&Call::Barrier(Barrier::same_queue(
                10,
                SHADER_READ,
                TRANSFER_DST
            ))) && calls().contains(&Call::Barrier(Barrier::same_queue(13, GENERAL, GENERAL)))
        );
        // A changed active extent inside the SDK range resets history.
        assert_eq!(run([1280, 720], 3, &|_| {}), 0);
        assert!(calls().contains(&Call::Execute(1280, 720, 1920, 1080, 1)));

        let refusals: [([u32; 2], Edit); 15] = [
            ([1023, 632], |_| {}),
            ([1128, 575], |_| {}),
            ([1281, 632], |_| {}),
            ([1128, 721], |_| {}),
            ([1128, 632], |frame| frame.depth.width -= 1),
            ([1128, 632], |frame| frame.motion.height -= 1),
            ([1128, 632], |frame| frame.color.format = R8_UNORM),
            ([1128, 632], |frame| frame.output.layout = GENERAL),
            ([1128, 632], |frame| {
                frame.reactive = Image {
                    image: 24,
                    format: R8_UNORM,
                    width: 1,
                    ..frame.color
                };
            }),
            ([1128, 632], |frame| frame.record = ptr::null()),
            ([1128, 632], |frame| frame.depth.format = D32_FLOAT),
            ([1128, 632], |frame| frame.motion.copy_source = 0),
            ([1128, 632], |frame| frame.motion.image = frame.depth.image),
            ([1128, 632], |frame| frame.output.image = frame.color.image),
            ([1128, 632], |frame| frame.pre_exposure = f32::from_bits(1)),
        ];
        for (extent, edit) in refusals {
            assert_eq!(run(extent, 4, &edit), INVALID_ARGUMENT);
            assert!(calls().is_empty());
        }
        let mut jitter = record([1128, 632], 4);
        jitter.camera.jitter_pixels[0] = 0.51;
        let input = frame(&jitter);
        assert_eq!(
            rsf_srvk_evaluate(owner.cast(), commands, &input),
            INVALID_ARGUMENT
        );
        assert_eq!(
            rsf_srvk_evaluate(owner.cast(), ptr::null_mut(), &input),
            INVALID_ARGUMENT
        );

        // Velocity refusal records nothing and keeps history.
        edit_state(|fake| fake.velocity = -4);
        assert_eq!(run([1280, 720], 4, &|_| {}), FEATURE_FAILED);
        assert_eq!(calls(), [Call::Velocity]);
        edit_state(|fake| fake.velocity = XESS_SUCCESS);
        edit_state(|fake| fake.execute = -6);
        assert_eq!(run([1280, 720], 4, &|_| {}), FEATURE_FAILED);
        assert_eq!(
            calls().last(),
            Some(&Call::Execute(1280, 720, 1920, 1080, 0)),
        );
        // A refused dispatch forgets history, even for the next sequential frame.
        edit_state(|fake| fake.execute = XESS_SUCCESS);
        edit_state(|fake| fake.sharpen = 0);
        assert_eq!(run([1280, 720], 5, &|_| {}), 0);
        assert!(calls().contains(&Call::Execute(1280, 720, 1920, 1080, 1)));
        assert!(calls().contains(&Call::Copy(output_copy)));
        assert_eq!(run([1280, 720], 6, &|_| {}), 0);
        assert!(calls().contains(&Call::Copy(output_copy)));
        assert!(calls().contains(&Call::Execute(1280, 720, 1920, 1080, 0)));
        let flagged = |frame: u64| {
            let mut value = record([1280, 720], frame);
            value.flags = RSF_FRAME_FLAG_RESET;
            let input = frame_input(&value);
            edit_state(|fake| fake.calls.clear());
            assert_eq!(rsf_srvk_evaluate(owner.cast(), commands, &input), 0);
            calls().contains(&Call::Execute(1280, 720, 1920, 1080, 1))
        };
        assert!(flagged(7));

        rsf_srvk_retire_commands(owner.cast(), commands);
        assert_eq!(calls().last(), Some(&Call::Retire(commands as usize)));
        edit_state(|fake| fake.calls.clear());
        edit_state(|fake| fake.destroy = -6);
        assert_eq!(rsf_srvk_destroy(owner.cast()), FEATURE_FAILED);
        assert_eq!(calls(), [Call::Flush, Call::Destroy]);
        edit_state(|fake| fake.destroy = XESS_SUCCESS);
        edit_state(|fake| fake.calls.clear());
        assert_eq!(rsf_srvk_destroy(owner.cast()), 0);
        assert_eq!(
            calls(),
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
    with_state(|| unsafe {
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &requirements()), 0);
        let mut owner = ptr::null_mut();
        let setup = setup();
        // The recommendation must sit inside the SDK's own range.
        edit_state(|fake| fake.range = [1000, 636, 1024, 576, 1280, 720]);
        edit_state(|fake| fake.calls.clear());
        assert_eq!(
            rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner),
            INIT_FAILED
        );
        assert_eq!(
            calls(),
            [
                Call::Open,
                Call::Create,
                Call::Init(104, XESS_AUTOEXPOSURE | XESS_INVERTED_DEPTH),
                Call::Destroy,
                Call::Clear,
                Call::Close,
            ]
        );
        edit_state(|fake| fake.range = Fake::default().range);
        edit_state(|fake| fake.fail_allocation = 12);
        edit_state(|fake| fake.calls.clear());
        assert_eq!(
            rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner),
            INIT_FAILED
        );
        assert!(owner.is_null());
        assert!(calls().ends_with(&[
            Call::ReleaseImage(12),
            Call::ReleaseMemory(112),
            Call::Destroy,
            Call::Clear,
            Call::ReleaseView(210),
            Call::ReleaseImage(10),
            Call::ReleaseMemory(110),
            Call::ReleaseView(211),
            Call::ReleaseImage(11),
            Call::ReleaseMemory(111),
            Call::Close,
        ]));
        edit_state(|fake| {
            fake.calls.clear();
            fake.destroy = -6;
        });
        assert_eq!(
            rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner),
            INIT_FAILED
        );
        assert!(!owner.is_null());
        assert_eq!(calls().last(), Some(&Call::Destroy));
        assert!(!calls().contains(&Call::Close));
        assert!(!calls().contains(&Call::ReleaseImage(10)));
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), NOT_READY);

        let record = record([1128, 632], 1);
        let input = frame(&record);
        let (mut width, mut height) = (0, 0);
        assert_eq!(
            rsf_srvk_plan(owner.cast(), &mut width, &mut height),
            NOT_READY
        );
        assert_eq!(
            rsf_srvk_evaluate(owner.cast(), handle(), &input),
            NOT_READY
        );
        edit_state(|fake| fake.destroy = XESS_SUCCESS);
        assert_eq!(rsf_srvk_destroy(owner.cast()), 0);
        assert_eq!(calls().last(), Some(&Call::Close));
        // Cleanup released the last reference to the SDK context.
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
    });
}

#[test]
fn shared_srvk_owner_routes_xess_and_refuses_presets() {
    with_state(|| unsafe {
        assert_eq!(rsf_core_xessvk_initialize(Some(&OPS), &requirements()), 0);
        let mut owner = ptr::null_mut();
        assert_eq!(
            rsf_core_srvk_create(Some(&PLATFORM), &setup(), &mut owner),
            0
        );
        let owner = owner.cast::<Owner>();
        let (mut width, mut height) = (0, 0);
        assert_eq!(rsf_srvk_plan(owner, &mut width, &mut height), 0);
        assert_eq!((width, height), (1130, 636));
        assert_eq!(rsf_srvk_dlss_preset(owner, 1), NOT_SUPPORTED);
        let record = record([1100, 600], 1);
        let mut input = frame(&record);
        edit_state(|fake| fake.calls.clear());
        let commands = ptr::dangling_mut::<u32>().cast();
        input.sharpening.struct_size = 1;
        assert_eq!(
            rsf_srvk_evaluate(owner, commands, &input),
            INVALID_ARGUMENT
        );
        assert!(calls().is_empty());
        input.sharpening.struct_size = 0;
        assert_eq!(rsf_srvk_evaluate(owner, commands, &input), 0);
        assert!(calls().contains(&Call::Execute(1100, 600, 1920, 1080, 1)));
        rsf_srvk_retire_commands(owner, commands);
        assert_eq!(calls().last(), Some(&Call::Retire(commands as usize)));
        assert_eq!(rsf_srvk_destroy(owner), 0);
        assert_eq!(calls().last(), Some(&Call::Close));
        assert_eq!(rsf_core_xessvk_shutdown(Some(&OPS)), 0);
    });
}
