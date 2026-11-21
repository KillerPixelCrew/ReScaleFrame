// SPDX-License-Identifier: MIT
use super::*;
use crate::vulkan_dlss_sr_abi::{rsf_vkdlss_images, rsf_vkdlss_platform};
use crate::vulkan_fg_resources_abi::rsf_vkfg_image;
use crate::vulkan_latency_abi::{RSF_SLVK_ABI_VERSION, rsf_slvk_call};

const FAILED: i32 = -9;

#[derive(Debug, PartialEq)]
enum Call {
    Create(u64, u32),
    Plan,
    Allocate(u64, u32, u32, u32),
    Mask,
    Output,
    Dispatch(u32, u64, u64, f32, f32),
    Capture(String),
    Sharpen,
    Log,
    Copy(u64, u64),
    DlssConstants(u32),
    DlssOptions(u32),
    DlssStage,
    DlssParameters(u32),
    DlssPublish,
    Retire(usize),
    Flush,
    Destroy,
    Clear,
    Release(u64, u64),
    Close,
}

struct Fake {
    calls: Vec<Call>,
    effective: u64,
    fail_allocation: bool,
    dispatch_result: i32,
    destroy_result: i32,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            effective: 33,
            fail_allocation: false,
            dispatch_result: 0,
            destroy_result: 0,
        }
    }
}

unsafe fn fake<'a>(handle: Handle) -> &'a mut Fake {
    unsafe { &mut *handle.cast() }
}

unsafe extern "C" fn proc(_: Handle, _: *const c_char) -> rsf_vk_function {
    None
}

impl Fake {
    fn setup(&mut self, backend: u32) -> Setup {
        let device = ptr::from_mut(self).cast();
        Setup {
            struct_size: size_of::<Setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: device,
            physical_device: device,
            device,
            instance_proc: Some(proc),
            device_proc: Some(proc),
            backend,
            quality: 1,
            output_width: 128,
            output_height: 72,
            depth_inverted: 1,
            depth_infinite: 1,
            motion_jittered: 1,
            runtime_directory_utf8: c"C:\\payload".as_ptr(),
        }
    }
}

// The stubs have the same calling convention as the C++ platform table.
macro_rules! leaf {
    (|$($arg:tt: $ty:ty),*| -> $ret:ty $body:block) => {{
        unsafe extern "C" fn callback($($arg: $ty),*) -> $ret $body
        callback
    }};
}

static DLSS: rsf_vkdlss_platform = rsf_vkdlss_platform {
    open: Some(leaf!(|setup: *const Setup,
                      native: *mut Handle,
                      w: *mut u32,
                      h: *mut u32|
     -> i32 {
        unsafe {
            native.write((*setup).device);
            w.write(64);
            h.write(36);
        }
        0
    })),
    close: Some(leaf!(|native: Handle| -> () {
        unsafe { fake(native).calls.push(Call::Close) }
    })),
    invoke: Some(leaf!(|call: *const rsf_slvk_call| -> i32 {
        let call = unsafe { &*call };
        assert_eq!(call.abi_version, RSF_SLVK_ABI_VERSION);
        assert_eq!(call.session_id == 0, call.frame_id == 0);
        unsafe { (call.callback.unwrap())(ptr::null_mut(), call.frame_id as usize, call.user) }
    })),
    allocate: Some(leaf!(|native: Handle,
                          image: *mut rsf_vkfg_image,
                          width: u32,
                          height: u32,
                          format: u32,
                          usage: u32|
     -> u32 {
        let state = unsafe { fake(native) };
        let id = state.calls.len() as u64 + 1;
        state.calls.push(Call::Allocate(id, width, height, usage));
        unsafe {
            image.write(rsf_vkfg_image {
                image: id,
                width,
                height,
                format,
                usage,
                ..Default::default()
            })
        };
        1
    })),
    free_image: Some(leaf!(|native: Handle, image: *mut rsf_vkfg_image| -> () {
        unsafe { fake(native).calls.push(Call::Release((*image).image, 0)) };
    })),
    constants: Some(leaf!(|native: Handle,
                           token: usize,
                           input: *const Frame,
                           reset: u32|
     -> i32 {
        assert_eq!(token as u64, unsafe { (*(*input).record).frame_id });
        unsafe { fake(native).calls.push(Call::DlssConstants(reset)) };
        0
    })),
    options: Some(leaf!(|native: Handle,
                         _: *const Frame,
                         preset: u32|
     -> i32 {
        unsafe { fake(native).calls.push(Call::DlssOptions(preset)) };
        0
    })),
    stage: Some(leaf!(|native: Handle,
                       _: Handle,
                       _: *const Image,
                       _: *mut rsf_vkfg_image,
                       _: u32|
     -> () {
        unsafe { fake(native).calls.push(Call::DlssStage) };
    })),
    initialize_output: Some(leaf!(|_: Handle,
                                   _: Handle,
                                   _: *const rsf_vkfg_image|
     -> () {})),
    tag: Some(leaf!(|_: Handle,
                     _: usize,
                     _: Handle,
                     _: *const Frame,
                     images: *const rsf_vkdlss_images|
     -> i32 {
        let images = unsafe { &*images };
        assert_eq!(images.color.initialized, 1);
        assert_eq!(images.output.initialized, 1);
        0
    })),
    evaluate: Some(leaf!(|native: Handle, _: usize, _: Handle| -> i32 {
        unsafe { fake(native).dispatch_result }
    })),
    capture_parameters: Some(leaf!(|native: Handle, _: *const Frame, reset: u32| -> () {
        unsafe { fake(native).calls.push(Call::DlssParameters(reset)) };
    })),
    capture_record: Some(leaf!(|native: Handle,
                                _: Handle,
                                _: *const Image,
                                _: *const rsf_frame_record,
                                role: *const c_char,
                                _: u32|
     -> () {
        unsafe {
            fake(native).calls.push(Call::Capture(
                std::ffi::CStr::from_ptr(role)
                    .to_string_lossy()
                    .into_owned(),
            ))
        };
    })),
    sharpen: Some(leaf!(|native: Handle,
                         _: Handle,
                         _: *const Frame,
                         _: u64|
     -> () {
        unsafe { fake(native).calls.push(Call::Sharpen) };
    })),
    publish: Some(leaf!(|native: Handle,
                         _: Handle,
                         _: *const Frame,
                         _: u64|
     -> () {
        unsafe { fake(native).calls.push(Call::DlssPublish) };
    })),
    release: Some(leaf!(|_: Handle, token: usize, native: Handle| -> i32 {
        assert_eq!(token, 0);
        unsafe { fake(native).destroy_result }
    })),
    flush: Some(leaf!(|native: Handle| -> () {
        unsafe { fake(native).calls.push(Call::Flush) }
    })),
    retire: Some(leaf!(|native: Handle, commands: Handle| -> () {
        unsafe { fake(native).calls.push(Call::Retire(commands as usize)) }
    })),
};

// Types 1 and 2 are compatible inside the three reported types; 2 and 3 are device-local.
static FFX_IMAGES: ImageOps = ImageOps {
    local_memory_types: Some(leaf!(|_: Handle| -> u32 { 0b1100 })),
    create_image: Some(leaf!(|native: Handle,
                              width: u32,
                              height: u32,
                              format: u32,
                              usage: u32,
                              image: *mut u64|
     -> i32 {
        let id = if format == R8_UNORM { 15 } else { 14 };
        unsafe {
            fake(native)
                .calls
                .push(Call::Allocate(id, width, height, usage));
            image.write(id);
        }
        0
    })),
    requirements: Some(leaf!(|_: Handle,
                              image: u64,
                              size: *mut u64,
                              types: *mut u32|
     -> () {
        unsafe {
            size.write(image);
            types.write(0b10110);
        }
    })),
    allocate: Some(leaf!(|native: Handle,
                          size: u64,
                          kind: u32,
                          memory: *mut u64|
     -> i32 {
        assert_eq!(kind, 2);
        if unsafe { fake(native) }.fail_allocation && size == 15 {
            return -2;
        }
        unsafe { memory.write(size + 100) };
        0
    })),
    bind: Some(leaf!(|_: Handle, image: u64, memory: u64| -> i32 {
        assert_eq!(memory, image + 100);
        0
    })),
    create_view: None,
    destroy_view: None,
    destroy_image: None,
    free_memory: None,
    create_fence: None,
    destroy_fence: None,
    wait_fence: None,
    reset_fence: None,
    submit_fence: None,
};

pub(super) static PLATFORM: Platform = Platform {
    dlss: Some(&DLSS),
    xess: Some(&super::xess::tests::OPS),
    ffx: FfxOps {
        loader: Loader {
            load: leaf!(|_: *const u16| -> Handle { ptr::without_provenance_mut(1) }),
            unload: leaf!(|_: Handle| -> () {}),
        },
        open: leaf!(|setup: &Setup, _: Handle, out: &mut Handle| -> i32 {
            *out = setup.device;
            0
        }),
        close: leaf!(|handle: Handle| -> () { unsafe { fake(handle).calls.push(Call::Close) } }),
        initialize_graphics: leaf!(|_: Handle, _: &Setup| -> i32 { 0 }),
        versions: leaf!(|_: Handle,
                         versions: &mut [Version; MAX_VERSIONS],
                         count: &mut u64|
         -> i32 {
            for (index, (id, major, minor, patch)) in [
                (29, 2, 9, 0),
                (32, 3, 2, 9),
                (33, 3, 3, 0),
                (34, 3, 3, 0),
                (41, 4, 1, 0),
            ]
            .into_iter()
            .enumerate()
            {
                versions[index] = Version {
                    id,
                    major,
                    minor,
                    patch,
                    ..Version::default()
                };
            }
            *count = 5;
            0
        }),
        create_effect: leaf!(
            |handle: Handle, _: &Setup, version: u64, flags: u32| -> i32 {
                unsafe { fake(handle).calls.push(Call::Create(version, flags)) };
                0
            }
        ),
        effective_version: leaf!(|handle: Handle, effective: &mut u64| -> i32 {
            *effective = unsafe { fake(handle).effective };
            0
        }),
        destroy_effect: leaf!(|handle: Handle| -> i32 {
            let state = unsafe { fake(handle) };
            state.calls.push(Call::Destroy);
            state.destroy_result
        }),
        plan: leaf!(
            |handle: Handle, _: &Setup, width: &mut u32, height: &mut u32| -> i32 {
                unsafe { fake(handle).calls.push(Call::Plan) };
                *width = 64;
                *height = 36;
                0
            }
        ),
        image_user: leaf!(|handle: Handle| -> Handle { handle }),
        images: &FFX_IMAGES,
        memory_type_count: leaf!(|_: Handle| -> u32 { 3 }),
        release: leaf!(|handle: Handle, image: u64, memory: u64| -> () {
            unsafe { fake(handle).calls.push(Call::Release(image, memory)) }
        }),
        initialize_mask: leaf!(|handle: Handle, _: Handle, _: u64| -> () {
            unsafe { fake(handle).calls.push(Call::Mask) }
        }),
        initialize_output: leaf!(|handle: Handle, _: Handle, _: u64| -> () {
            unsafe { fake(handle).calls.push(Call::Output) }
        }),
        dispatch: leaf!(|handle: Handle, _: Handle, call: &FfxDispatch| -> i32 {
            let state = unsafe { fake(handle) };
            state.calls.push(Call::Dispatch(
                call.reset,
                call.reactive.image,
                call.composition.image,
                call.far_plane,
                call.sharpness,
            ));
            state.dispatch_result
        }),
        copy_output: leaf!(
            |handle: Handle, _: Handle, output: &Image, target: &Image| -> () {
                unsafe {
                    fake(handle)
                        .calls
                        .push(Call::Copy(output.image, target.image))
                }
            }
        ),
        sharpen: leaf!(
            |handle: Handle, _: &Setup, _: Handle, _: &Frame, _: u64| -> i32 {
                unsafe { fake(handle).calls.push(Call::Sharpen) };
                0
            }
        ),
        clear_sharpen: leaf!(|handle: Handle| -> () {
            unsafe { fake(handle).calls.push(Call::Clear) }
        }),
        log: leaf!(|frame: &Frame, _: *const c_char| -> () {
            unsafe { fake(frame.log_user).calls.push(Call::Log) }
        }),
        capture_provider: leaf!(|_: Handle, _: u64, _: u32| -> () {}),
        capture_record: leaf!(|handle: Handle,
                               _: Handle,
                               _: &Image,
                               _: &rsf_frame_record,
                               role: *const c_char,
                               _: u32|
         -> () {
            let role = unsafe { CStr::from_ptr(role) }
                .to_string_lossy()
                .into_owned();
            unsafe { fake(handle).calls.push(Call::Capture(role)) };
        }),
        capture_wants: leaf!(|_: Handle, _: u64| -> i32 { 1 }),
        capture_parameters: leaf!(|_: &Frame, _: u32, _: u32| -> () {}),
        retire_commands: leaf!(|handle: Handle, commands: Handle| -> () {
            unsafe { fake(handle).calls.push(Call::Retire(commands as usize)) }
        }),
        flush_capture: leaf!(|handle: Handle| -> () {
            unsafe { fake(handle).calls.push(Call::Flush) }
        }),
    },
    fallback_probe: leaf!(|call: &rsf_srvk_fallback, out: &mut Blit| -> i32 {
        out.barrier = Some(unused);
        out.blit = Some(unused);
        out.features = call.output.copy_source;
        0
    }),
    fallback_record: leaf!(|_: &rsf_srvk_fallback, _: &Blit| -> i32 { 123 }),
    dump_begin: leaf!(|_: &rsf_frame_record, _: u32| -> i32 { 1 }),
    dump_wants: leaf!(|_: &rsf_frame_record| -> i32 { 1 }),
    dump_result: leaf!(|_: &rsf_frame_record, _: *const c_char, _: u32, _: i32| -> () {}),
};

unsafe extern "C" fn unused() {}
unsafe extern "C" fn sharpen_valid(_: &rsf_sharpening_options) -> i32 {
    1
}

fn frame_record() -> rsf_frame_record {
    let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 2;
    record.frame_id = 1;
    record.render_width = 64;
    record.render_height = 36;
    record.output_width = 128;
    record.output_height = 72;
    record.frame_time_ms = 16.0;
    let camera = &mut record.camera;
    camera.struct_size = size_of_val(camera) as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = record.render_width;
    camera.render_height = record.render_height;
    camera.output_width = record.output_width;
    camera.output_height = record.output_height;
    camera.depth_inverted = 1;
    camera.motion_jittered = 1;
    camera.near_plane = 0.1;
    camera.vertical_fov_radians = 1.0;
    record
}

fn frame_input(setup: &Setup, record: &rsf_frame_record) -> Frame {
    let image = |id, format| Image {
        device: setup.device,
        image: id,
        format,
        width: record.render_width,
        height: record.render_height,
        layout: SHADER_READ,
        copy_source: 1,
    };
    let sharpening = rsf_sharpening_options {
        struct_size: size_of::<rsf_sharpening_options>() as u32,
        mode: SHARPENING_VENDOR,
        sharpness: 0.75,
        ..unsafe { std::mem::zeroed() }
    };
    Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        record,
        color: image(1, RGBA16_FLOAT),
        depth: image(2, D32_FLOAT),
        motion: image(3, RG16_FLOAT),
        output: Image {
            width: record.output_width,
            height: record.output_height,
            layout: TRANSFER_DST,
            ..image(4, RGBA16_FLOAT)
        },
        reactive: image(5, R8_UNORM),
        bias_current_color: image(7, R8_UNORM),
        motion_scale_x: -64.0,
        motion_scale_y: 36.0,
        pre_exposure: 1.0,
        sharpening,
        log_user: setup.device,
        ..unsafe { std::mem::zeroed() }
    }
}

#[test]
fn ffx_owner_reuses_staging_and_only_publishes_an_accepted_output() {
    let mut state = Fake::default();
    let setup = state.setup(RSF_SR_FSR3);
    let mut owner = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner) },
        0
    );
    let owner = owner.cast();
    assert_eq!(
        state.calls,
        [
            Call::Create(33, 61),
            Call::Plan,
            Call::Allocate(14, 128, 72, 15),
            Call::Allocate(15, 64, 36, 6),
        ]
    );
    let (mut width, mut height) = (0, 0);
    assert_eq!(
        unsafe { rsf_core_srvk_plan(owner, &mut width, &mut height) },
        0
    );
    assert_eq!((width, height), (64, 36));
    assert_eq!(
        unsafe { rsf_core_srvk_dlss_preset(owner, 11) },
        NOT_SUPPORTED
    );

    let commands = ptr::dangling_mut::<u8>().cast();
    let mut record = frame_record();
    for (sequence, failure) in [(1, false), (2, true), (3, false), (4, false)] {
        record.frame_id = sequence;
        state.dispatch_result = if failure { FAILED } else { 0 };
        let mut frame = frame_input(&setup, &record);
        if sequence == 1 {
            frame.struct_size = std::mem::offset_of!(Frame, bias_current_color) as u32;
            let mut refused = frame_input(&setup, &record);
            refused.depth.width += 1;
            let previous = state.calls.len();
            assert_eq!(
                unsafe { rsf_core_srvk_evaluate(owner, commands, &refused, sharpen_valid) },
                INVALID
            );
            assert_eq!(state.calls.len(), previous);
        }
        let previous = state.calls.len();
        assert_eq!(
            unsafe { rsf_core_srvk_evaluate(owner, commands, &frame, sharpen_valid) },
            state.dispatch_result
        );
        let calls = &state.calls[previous..];
        assert_eq!(calls.contains(&Call::Mask), sequence == 1);
        assert_eq!(calls.contains(&Call::Output), sequence == 1);
        assert_eq!(
            calls.contains(&Call::Capture("sr-bias-mask".into())),
            sequence != 1
        );
        assert!(calls.contains(&Call::Dispatch(
            u32::from(sequence == 1 || sequence == 3),
            5,
            15,
            f32::MAX,
            0.75
        )));
        assert_eq!(calls.contains(&Call::Copy(14, 4)), !failure);
        assert_eq!(
            calls.contains(&Call::Capture("sr-output-linear".into())),
            !failure
        );
        assert_eq!(calls.contains(&Call::Log), sequence == 1);
    }
    unsafe { rsf_core_srvk_retire_commands(owner, commands) };
    assert_eq!(state.calls.last(), Some(&Call::Retire(commands as usize)));

    state.destroy_result = FAILED;
    assert_eq!(unsafe { rsf_core_srvk_destroy(owner) }, FAILED);
    assert!(state.calls.ends_with(&[Call::Flush, Call::Destroy]));
    assert!(
        DISPATCH
            .lock()
            .unwrap()
            .contains_key(&(setup.device as usize))
    );
    assert!(!state.calls.contains(&Call::Close));
    state.destroy_result = 0;
    assert_eq!(unsafe { rsf_core_srvk_destroy(owner) }, 0);
    assert!(state.calls.ends_with(&[
        Call::Destroy,
        Call::Clear,
        Call::Release(14, 114),
        Call::Release(15, 115),
        Call::Close
    ]));
    assert!(
        !DISPATCH
            .lock()
            .unwrap()
            .contains_key(&(setup.device as usize))
    );
}

#[test]
fn ffx_creation_refusal_unwinds_partial_resources_and_native_aa_skips_the_query() {
    let mut state = Fake {
        effective: 29,
        fail_allocation: true,
        ..Fake::default()
    };
    let mut setup = state.setup(RSF_SR_FSR2);
    setup.quality = 0;
    let mut owner = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner) },
        INIT_FAILED
    );
    assert!(owner.is_null());
    assert_eq!(
        state.calls,
        [
            Call::Create(29, 61),
            Call::Allocate(14, 128, 72, 15),
            Call::Allocate(15, 128, 72, 6),
            Call::Clear,
            Call::Destroy,
            Call::Release(14, 114),
            Call::Release(15, 0),
            Call::Close,
        ]
    );
    assert!(
        !DISPATCH
            .lock()
            .unwrap()
            .contains_key(&(setup.device as usize))
    );

    state.calls.clear();
    state.effective = 33;
    assert_eq!(
        unsafe { rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner) },
        NOT_SUPPORTED
    );
    assert_eq!(
        state.calls,
        [
            Call::Create(29, 61),
            Call::Clear,
            Call::Destroy,
            Call::Release(0, 0),
            Call::Release(0, 0),
            Call::Close,
        ]
    );
    assert!(owner.is_null());
}

#[test]
fn ffx_memory_type_prefers_first_device_local_else_last_compatible() {
    // (memoryTypeBits, memoryTypeCount, device-local types) -> chosen index
    for (required, count, local, chosen) in [
        (0b1110, 4, 0b1100, Some(2)),
        (0b1110, 4, 0b0001, Some(3)),
        (0b1110, 3, 0b1000, Some(2)),
        (0b0001, 1, 0b0000, Some(0)),
        (0b1000, 3, 0b1000, None),
        (0b0000, 32, u32::MAX, None),
        (u32::MAX, 32, 0, Some(31)),
        (u32::MAX, 0, u32::MAX, None),
    ] {
        assert_eq!(memory_type(required, count, local), chosen);
    }
}

#[test]
fn dlss_owner_routing_preserves_history_and_checked_release() {
    let mut state = Fake::default();
    let setup = state.setup(RSF_SR_DLSS);
    let mut raw = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_core_srvk_create(None, &setup, &mut raw) },
        NOT_COMPILED
    );
    assert_eq!(
        unsafe { rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut raw) },
        0
    );
    let owner = raw.cast();
    assert_eq!(
        state.calls,
        [
            Call::Allocate(1, 64, 36, 6),
            Call::Allocate(2, 64, 36, 6),
            Call::Allocate(3, 64, 36, 6),
            Call::Allocate(4, 64, 36, 6),
            Call::Allocate(5, 128, 72, 15),
        ]
    );
    let (mut width, mut height) = (0, 0);
    assert_eq!(
        unsafe { rsf_core_srvk_plan(owner, &mut width, &mut height) },
        0
    );
    assert_eq!([width, height], [64, 36]);
    assert_eq!(unsafe { rsf_core_srvk_dlss_preset(owner, 11) }, 0);
    assert_eq!(unsafe { rsf_core_srvk_dlss_preset(owner, 99) }, INVALID);
    let commands = ptr::dangling_mut::<u8>().cast();
    let mut record = frame_record();
    for (frame_id, result, reset) in [(1, 0, 1), (2, FAILED, 0), (3, 0, 1)] {
        record.frame_id = frame_id;
        state.dispatch_result = result;
        state.calls.clear();
        let mut frame = frame_input(&setup, &record);
        frame.depth.format = R32_FLOAT;
        frame.bias_current_color.image = 0;
        assert_eq!(
            unsafe { rsf_core_srvk_evaluate(owner, commands, &frame, sharpen_valid) },
            result
        );
        let mut expected = vec![
            Call::DlssConstants(reset),
            Call::DlssOptions(11),
            Call::DlssStage,
            Call::DlssStage,
            Call::DlssStage,
        ];
        if result == 0 {
            expected.push(Call::DlssParameters(reset));
            expected.extend(
                [
                    "sr-source-jittered",
                    "sr-linear-color",
                    "sr-depth",
                    "sr-motion-uv",
                    "sr-output-linear",
                ]
                .map(|role| Call::Capture(role.to_owned())),
            );
            expected.extend([Call::Sharpen, Call::DlssPublish]);
        }
        assert_eq!(state.calls, expected);
    }
    state.calls.clear();
    unsafe { rsf_core_srvk_retire_commands(owner, commands) };
    state.destroy_result = FAILED;
    assert_eq!(unsafe { rsf_core_srvk_destroy(owner) }, FAILED);
    assert_eq!(state.calls, [Call::Retire(commands as usize)]);
    state.destroy_result = 0;
    assert_eq!(unsafe { rsf_core_srvk_destroy(owner) }, 0);
    assert_eq!(
        state.calls,
        [
            Call::Retire(commands as usize),
            Call::Flush,
            Call::Release(1, 0),
            Call::Release(2, 0),
            Call::Release(3, 0),
            Call::Release(5, 0),
            Call::Release(4, 0),
            Call::Close,
        ]
    );
}

#[test]
fn fallback_checks_admission_and_linear_blit_capabilities_before_recording() {
    let mut state = Fake::default();
    let setup = state.setup(RSF_SR_FSR3);
    let record = frame_record();
    let frame = frame_input(&setup, &record);
    let mut call = rsf_srvk_fallback {
        struct_size: size_of::<rsf_srvk_fallback>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        instance: setup.instance,
        physical_device: setup.physical_device,
        device: setup.device,
        instance_proc: setup.instance_proc,
        device_proc: setup.device_proc,
        command_buffer: setup.device,
        color: frame.color,
        output: frame.output,
    };
    assert_eq!(
        unsafe { rsf_core_srvk_spatial_fallback(None, &call) },
        NOT_SUPPORTED
    );
    assert_eq!(
        unsafe { rsf_core_srvk_spatial_fallback(Some(&PLATFORM), &call) },
        NOT_SUPPORTED
    );
    call.output.copy_source = LINEAR_BLIT;
    assert_eq!(
        unsafe { rsf_core_srvk_spatial_fallback(Some(&PLATFORM), &call) },
        123
    );
    call.output.image = call.color.image;
    assert_eq!(
        unsafe { rsf_core_srvk_spatial_fallback(Some(&PLATFORM), &call) },
        INVALID
    );
}
