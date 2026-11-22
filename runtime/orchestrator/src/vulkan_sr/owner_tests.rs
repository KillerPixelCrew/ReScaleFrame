// SPDX-License-Identifier: MIT
use super::*;
use crate::vulkan_dlss_sr_abi::{rsf_vkdlss_commands, rsf_vkdlss_platform, rsf_vkdlss_tag};
use crate::vulkan_fg_resources_abi::rsf_vkfg_image;
use crate::vulkan_latency_abi::{RSF_SLVK_ABI_VERSION, rsf_slvk_call};
use crate::vulkan_streamline_camera::rsf_slvk_sdk_constants;
use ash::vk::{self, Handle as _};

const FAILED: i32 = -9;

#[derive(Debug, PartialEq)]
enum Call {
    Create(u64, u32),
    Plan,
    Allocate(u64, u32, u32, u32),
    Barrier(Barrier),
    ClearColorZero(u64, u32),
    Dispatch(u32, u64, u64, f32, f32),
    Parameters(u32),
    Capture(String),
    Sharpen,
    Log,
    Copy(ImageCopy),
    DlssConstants(u32),
    DlssOptions(u32),
    DlssStage,
    DlssBias,
    DlssParameters(u32),
    DlssPublish,
    Retire(usize),
    Flush,
    Destroy,
    Clear,
    Release(u64, u64),
    ReleaseImage(u64),
    ReleaseMemory(u64),
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

unsafe extern "C" fn supported_entry() {}

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

fn fake_dlss_commands() -> rsf_vkdlss_commands {
    unsafe extern "system" fn barrier(
        _: vk::CommandBuffer,
        src: vk::PipelineStageFlags,
        dst: vk::PipelineStageFlags,
        _: vk::DependencyFlags,
        _: u32,
        _: *const vk::MemoryBarrier<'_>,
        _: u32,
        _: *const vk::BufferMemoryBarrier<'_>,
        count: u32,
        barriers: *const vk::ImageMemoryBarrier<'_>,
    ) {
        assert!(src == vk::PipelineStageFlags::ALL_COMMANDS && dst == src);
        assert_eq!(count, 1);
        let image = unsafe { &*barriers };
        assert_eq!(image.subresource_range.layer_count, 1);
        assert!(image.subresource_range.aspect_mask == vk::ImageAspectFlags::COLOR);
        assert_eq!(image.src_queue_family_index, vk::QUEUE_FAMILY_IGNORED);
        assert_eq!(image.dst_queue_family_index, vk::QUEUE_FAMILY_IGNORED);
        assert_eq!(
            image.src_access_mask.is_empty(),
            image.old_layout == vk::ImageLayout::UNDEFINED
        );
    }

    unsafe extern "system" fn copy(
        commands: vk::CommandBuffer,
        _: vk::Image,
        src: vk::ImageLayout,
        _: vk::Image,
        dst: vk::ImageLayout,
        count: u32,
        region: *const vk::ImageCopy,
    ) {
        assert!(
            src == vk::ImageLayout::TRANSFER_SRC_OPTIMAL
                && dst == vk::ImageLayout::TRANSFER_DST_OPTIMAL
        );
        assert_eq!(count, 1);
        let extent = unsafe { (*region).extent };
        assert_eq!(extent.depth, 1);
        let call = if (extent.width, extent.height) == (128, 72) {
            Call::DlssPublish
        } else {
            assert_eq!((extent.width, extent.height), (64, 36));
            Call::DlssStage
        };
        unsafe { fake(commands.as_raw() as Handle).calls.push(call) };
    }

    unsafe extern "system" fn blit(
        commands: vk::CommandBuffer,
        _: vk::Image,
        src: vk::ImageLayout,
        _: vk::Image,
        dst: vk::ImageLayout,
        count: u32,
        region: *const vk::ImageBlit,
        filter: vk::Filter,
    ) {
        assert!(
            src == vk::ImageLayout::TRANSFER_SRC_OPTIMAL
                && dst == vk::ImageLayout::TRANSFER_DST_OPTIMAL
        );
        assert_eq!(count, 1);
        assert!(filter == vk::Filter::NEAREST);
        let last = unsafe { (*region).src_offsets[1] };
        assert_eq!((last.x, last.y, last.z), (64, 36, 1));
        unsafe { fake(commands.as_raw() as Handle).calls.push(Call::DlssBias) };
    }

    unsafe {
        rsf_vkdlss_commands {
            barrier: Some(std::mem::transmute::<
                vk::PFN_vkCmdPipelineBarrier,
                unsafe extern "C" fn(),
            >(barrier)),
            copy: Some(std::mem::transmute::<
                vk::PFN_vkCmdCopyImage,
                unsafe extern "C" fn(),
            >(copy)),
            blit: Some(std::mem::transmute::<
                vk::PFN_vkCmdBlitImage,
                unsafe extern "C" fn(),
            >(blit)),
        }
    }
}

static DLSS: rsf_vkdlss_platform = rsf_vkdlss_platform {
    open: Some(leaf!(|setup: *const Setup,
                      native: *mut Handle,
                      w: *mut u32,
                      h: *mut u32,
                      commands: *mut rsf_vkdlss_commands|
     -> i32 {
        unsafe {
            native.write((*setup).device);
            w.write(64);
            h.write(36);
            commands.write(fake_dlss_commands());
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
                           constants: *const rsf_slvk_sdk_constants|
     -> i32 {
        assert_ne!(token, 0);
        let reset = unsafe { (*constants).reset };
        unsafe {
            fake(native)
                .calls
                .push(Call::DlssConstants(u32::from(reset)))
        };
        0
    })),
    options: Some(leaf!(|native: Handle,
                         _: *const Frame,
                         preset: u32|
     -> i32 {
        unsafe { fake(native).calls.push(Call::DlssOptions(preset)) };
        0
    })),
    tag: Some(leaf!(|_: Handle,
                     _: usize,
                     _: Handle,
                     tags: *const rsf_vkdlss_tag|
     -> i32 {
        let tags = unsafe { &*tags.cast::<[rsf_vkdlss_tag; 5]>() };
        assert_eq!(tags.each_ref().map(|tag| tag.kind), [3, 0, 1, 4, 29]);
        assert_eq!(unsafe { (*tags[0].image).initialized }, 1);
        assert_eq!(unsafe { (*tags[3].image).initialized }, 1);
        assert_eq!(
            (tags[3].layout, tags[3].width, tags[3].height),
            (GENERAL, 128, 72)
        );
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
    preset_valid: Some(leaf!(|preset: u32| -> i32 { i32::from(preset == 11) })),
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
    destroy_image: Some(leaf!(|native: Handle, image: u64| -> () {
        unsafe { fake(native) }
            .calls
            .push(Call::ReleaseImage(image));
    })),
    free_memory: Some(leaf!(|native: Handle, memory: u64| -> () {
        unsafe { fake(native) }
            .calls
            .push(Call::ReleaseMemory(memory));
    })),
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
        allocate: leaf!(|setup: &Setup| -> Handle { setup.device }),
        close: leaf!(|handle: Handle| -> () { unsafe { fake(handle).calls.push(Call::Close) } }),
        symbol: leaf!(|_: Handle, _: *const c_char| -> rsf_vk_function { Some(supported_entry) }),
        bind_sdk: leaf!(|_: Handle, entries: &FfxExports| -> () {
            assert!(entries.iter().all(Option::is_some));
        }),
        lookup: leaf!(|_: DeviceProc,
                       _: Handle,
                       _: *const c_char,
                       out: *mut rsf_vk_function|
         -> i32 {
            unsafe { out.write(Some(supported_entry)) };
            1
        }),
        initialize_graphics: leaf!(|_: Handle,
                                    _: &Setup,
                                    entries: &FfxDeviceEntries,
                                    memory: rsf_vk_function|
         -> i32 {
            assert!(entries.iter().all(Option::is_some) && memory.is_some());
            0
        }),
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
        barrier: leaf!(|handle: Handle, _: Handle, plan: *const Barrier| -> () {
            unsafe { fake(handle).calls.push(Call::Barrier(*plan)) }
        }),
        clear_color_zero: leaf!(|handle: Handle, _: Handle, image: u64, layout: u32| -> () {
            unsafe { fake(handle).calls.push(Call::ClearColorZero(image, layout)) }
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
        copy: leaf!(|handle: Handle, _: Handle, plan: *const ImageCopy| -> () {
            unsafe { fake(handle).calls.push(Call::Copy(*plan)) }
        }),
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
        capture_parameters: leaf!(|frame: &Frame, _: u32, reset: u32| -> () {
            unsafe { fake(frame.log_user).calls.push(Call::Parameters(reset)) }
        }),
        retire_commands: leaf!(|handle: Handle, commands: Handle| -> () {
            unsafe { fake(handle).calls.push(Call::Retire(commands as usize)) }
        }),
        flush_capture: leaf!(|handle: Handle| -> () {
            unsafe { fake(handle).calls.push(Call::Flush) }
        }),
    },
    dump_begin: leaf!(|_: &rsf_frame_record, _: u32| -> i32 { 1 }),
    dump_wants: leaf!(|_: &rsf_frame_record| -> i32 { 1 }),
    dump_result: leaf!(|_: &rsf_frame_record, _: *const c_char, _: u32, _: i32| -> () {}),
};

unsafe extern "C" fn unused() {}

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
    let mut sharpening = sharpening_policy::defaults();
    sharpening.mode = SHARPENING_VENDOR;
    sharpening.sharpness = 0.75;
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
                unsafe { rsf_core_srvk_evaluate(owner, commands, &refused) },
                INVALID
            );
            assert_eq!(state.calls.len(), previous);
        }
        let previous = state.calls.len();
        assert_eq!(
            unsafe { rsf_core_srvk_evaluate(owner, commands, &frame) },
            state.dispatch_result
        );
        let mut expected = vec![Call::Capture("sr-source-jittered".into())];
        if sequence != 1 {
            expected.push(Call::Capture("sr-bias-mask".into()));
        }
        expected.push(Call::Capture("sr-reactive-mask".into()));
        if sequence == 1 {
            expected.extend([
                Call::Barrier(Barrier::with_access(15, UNDEFINED, TRANSFER_DST, 0, 0x1000)),
                Call::ClearColorZero(15, TRANSFER_DST),
                Call::Barrier(Barrier::with_access(
                    15,
                    TRANSFER_DST,
                    SHADER_READ,
                    0x1000,
                    0x20,
                )),
                Call::Barrier(Barrier::with_access(14, UNDEFINED, GENERAL, 0, 0x40)),
            ]);
        }
        let reset = u32::from(sequence == 1 || sequence == 3);
        expected.extend([
            Call::Parameters(reset),
            Call::Capture("sr-linear-color".into()),
            Call::Capture("sr-depth".into()),
            Call::Capture("sr-motion-uv".into()),
            Call::Dispatch(reset, 5, 15, f32::MAX, 0.75),
        ]);
        if !failure {
            expected.push(Call::Capture("sr-output-linear".into()));
            expected.push(Call::Sharpen);
            if sequence == 1 {
                expected.push(Call::Log);
            }
            expected.extend([
                Call::Barrier(Barrier::with_access(14, GENERAL, TRANSFER_SRC, 0x40, 0x800)),
                Call::Copy(ImageCopy::transfer(14, 4, [128, 72])),
                Call::Barrier(Barrier::with_access(14, TRANSFER_SRC, GENERAL, 0x800, 0x40)),
            ]);
        }
        assert_eq!(&state.calls[previous..], expected.as_slice());
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
        Call::ReleaseImage(14),
        Call::ReleaseMemory(114),
        Call::ReleaseImage(15),
        Call::ReleaseMemory(115),
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
            Call::ReleaseImage(15),
            Call::Destroy,
            Call::Clear,
            Call::ReleaseImage(14),
            Call::ReleaseMemory(114),
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
            Call::Destroy,
            Call::Clear,
            Call::Close,
        ]
    );
    assert!(owner.is_null());
}

#[test]
fn ffx_create_preserves_cleanup_owner_when_the_effect_refuses_rollback() {
    let mut state = Fake {
        fail_allocation: true,
        destroy_result: FAILED,
        ..Fake::default()
    };
    let setup = state.setup(RSF_SR_FSR3);
    let mut owner = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_core_srvk_create(Some(&PLATFORM), &setup, &mut owner) },
        INIT_FAILED
    );
    assert!(!owner.is_null());
    let owner = owner.cast::<Owner>();
    assert_eq!(state.calls.last(), Some(&Call::Destroy));
    assert!(!state.calls.contains(&Call::Close));
    assert!(!state.calls.contains(&Call::ReleaseImage(14)));
    assert!(
        DISPATCH
            .lock()
            .unwrap()
            .contains_key(&(setup.device as usize))
    );

    let (mut width, mut height) = (0, 0);
    assert_eq!(
        unsafe { rsf_core_srvk_plan(owner, &mut width, &mut height) },
        NOT_READY
    );
    let record = frame_record();
    let input = frame_input(&setup, &record);
    assert_eq!(
        unsafe { rsf_core_srvk_evaluate(owner, setup.device, &input) },
        NOT_READY
    );
    state.destroy_result = 0;
    assert_eq!(unsafe { rsf_core_srvk_destroy(owner) }, 0);
    assert!(state.calls.ends_with(&[
        Call::Clear,
        Call::ReleaseImage(14),
        Call::ReleaseMemory(114),
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
        assert_eq!(
            (MemoryChoice::PreferDeviceLocal { type_count: count }).select(required, local),
            chosen
        );
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
    let commands = ptr::from_mut(&mut state).cast();
    let mut record = frame_record();
    for (frame_id, result, reset) in [(1, 0, 1), (2, FAILED, 0), (3, 0, 1)] {
        record.frame_id = frame_id;
        state.dispatch_result = result;
        state.calls.clear();
        let mut frame = frame_input(&setup, &record);
        frame.depth.format = R32_FLOAT;
        if frame_id != 1 {
            frame.bias_current_color.image = 0;
        }
        assert_eq!(
            unsafe { rsf_core_srvk_evaluate(owner, commands, &frame) },
            result
        );
        let mut expected = vec![
            Call::DlssConstants(reset),
            Call::DlssOptions(11),
            Call::DlssStage,
            Call::DlssStage,
            Call::DlssStage,
        ];
        if frame_id == 1 {
            expected.push(Call::DlssBias);
        }
        if result == 0 {
            expected.push(Call::DlssParameters(reset));
            expected.extend(
                [
                    "sr-source-jittered",
                    "sr-linear-color",
                    "sr-depth",
                    "sr-motion-uv",
                ]
                .map(|role| Call::Capture(role.to_owned())),
            );
            if frame_id == 1 {
                expected.push(Call::Capture("sr-bias-mask".to_owned()));
            }
            expected.push(Call::Capture("sr-output-linear".to_owned()));
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
