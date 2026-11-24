// SPDX-License-Identifier: MIT
//! Synthetic split-frame, external image and checked-retirement contract.
use super::*;
use crate::sys::rsf_sharpening_options;
use ash::vk;
use std::cell::RefCell;
use windows_sys::Win32::System::Threading::CreateEventW;

const ADAPTER: u64 = 19;
const VERSION: u64 = 29;
const TIMELINE: u64 = 39;
const FAMILY: u32 = 7;
const RENDER: (u32, u32) = (192, 108);
const OUTPUT: (u32, u32) = (320, 180);
const FORMATS: [u32; 4] = [RGBA16_FLOAT, R32_FLOAT, RG16_FLOAT, RGBA16_FLOAT];
const STAGE: u32 = vk::PipelineStageFlags::ALL_COMMANDS.as_raw();
const SHADER_LAYOUT: u32 = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32;
const PRODUCER: Handle = ptr::without_provenance_mut(1);
const RESUMED: Handle = ptr::without_provenance_mut(2);
const DEVICE: Handle = ptr::without_provenance_mut(3);
const BRIDGE: Handle = ptr::without_provenance_mut(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Wait,
    Idle,
    Prepare,
    Flush,
    Clear,
    Image,
    Memory,
    Com,
    Timeline,
    Importer,
    Bridge,
    Graphics,
}

#[derive(Default)]
struct Calls {
    images: usize,
    resources: usize,
    queries: usize,
    retires: u32,
    sharpened: u32,
    sdk_error: i32,
    wait_error: i32,
    idle_error: i32,
    prepare_error: i32,
    destroy_error: i32,
    bind_error_at: u64,
    repeat_commands: bool,
    flush_error: bool,
    expected_done: u64,
    sharpness: f32,
    barriers: Vec<(Handle, Barrier)>,
    copies: Vec<(Handle, u64, u64)>,
    captures: Vec<(Handle, u64, Vec<u8>, u32)>,
    splits: Vec<[u64; 3]>,
    dispatches: Vec<BridgeFrame>,
    retirement: Vec<Step>,
    failures: u32,
}
thread_local! {
    static CALLS: RefCell<Calls> = RefCell::new(Calls::default());
}
fn with<T>(f: impl FnOnce(&mut Calls) -> T) -> T {
    CALLS.with(|c| f(&mut c.borrow_mut()))
}
macro_rules! leaf {
    (|$($name:tt : $ty:ty),*| $(-> $ret:ty)? { $($body:tt)* }) => {{
        unsafe extern "C" fn callback($($name: $ty),*) $(-> $ret)? { $($body)* }
        Some(callback)
    }};
}

pub(crate) unsafe fn open(setup: *const Setup, owner: *mut Handle) -> i32 {
    with(|c| check!(c, unsafe { (*setup).device } == DEVICE));
    unsafe { owner.write(DEVICE) };
    0
}

fn event(out: *mut Handle) -> i32 {
    let handle = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
    unsafe { out.write(handle) };
    if handle.is_null() {
        OUT_OF_HOST_MEMORY
    } else {
        0
    }
}

fn extent(index: usize) -> (u32, u32) {
    if index == 3 { OUTPUT } else { RENDER }
}

// The importer leaves dx12_vulkan_interop calls in the fixture-leaves archive. Unit tests keep the
// real importer leaves, so these are unused there.
#[cfg_attr(test, allow(dead_code, unused_imports))]
pub(crate) mod interop {
    use super::*;

    pub(crate) use super::open;

    pub(crate) unsafe extern "C" fn adapter_luid(_: *const Setup, out: *mut u64) -> i32 {
        unsafe { out.write(ADAPTER) };
        0
    }

    pub(crate) unsafe extern "C" fn close(_: Handle) {
        with(|c| c.retirement.push(Step::Importer))
    }

    pub(crate) unsafe extern "C" fn timeline_importable(_: Handle) -> u32 {
        1
    }

    pub(crate) unsafe extern "C" fn create_timeline(_: Handle, out: *mut u64) -> i32 {
        unsafe { out.write(TIMELINE) };
        0
    }

    pub(crate) unsafe extern "C" fn import_timeline(_: Handle, value: u64, handle: Handle) -> i32 {
        with(|c| check!(c, value == TIMELINE && !handle.is_null()));
        0
    }

    pub(crate) unsafe extern "C" fn destroy_timeline(_: Handle, value: u64) {
        with(|c| {
            check!(c, value == TIMELINE);
            c.retirement.push(Step::Timeline);
        });
    }

    pub(crate) unsafe extern "C" fn image_properties(
        _: Handle,
        format: u32,
        usage: u32,
        allowed: *mut u32,
        width: *mut u32,
        height: *mut u32,
    ) -> i32 {
        with(|c| {
            check!(c, FORMATS.get(c.queries) == Some(&format));
            check!(
                c,
                usage
                    == if c.queries == 3 {
                        STORAGE_USAGE | SAMPLED_USAGE
                    } else {
                        0
                    }
            );
            c.queries += 1;
        });
        unsafe {
            allowed.write(1);
            width.write(OUTPUT.0);
            height.write(OUTPUT.1);
        };
        0
    }

    pub(crate) unsafe extern "C" fn create_image(
        _: Handle,
        format: u32,
        width: u32,
        height: u32,
        usage: u32,
        out: *mut u64,
    ) -> i32 {
        let index = with(|c| {
            let index = c.images;
            c.images += 1;
            check!(c, FORMATS.get(index) == Some(&format));
            check!(
                c,
                usage
                    == if index == 3 {
                        STORAGE_USAGE | SAMPLED_USAGE
                    } else {
                        0
                    }
            );
            check!(c, (width, height) == extent(index));
            index
        });
        unsafe { out.write(100 + index as u64) };
        0
    }

    pub(crate) unsafe extern "C" fn image_requirements(
        _: Handle,
        _: u64,
        bytes: *mut u64,
        types: *mut u32,
    ) {
        unsafe {
            bytes.write(256);
            types.write(0b1101);
        }
    }

    pub(crate) unsafe extern "C" fn handle_memory_types(
        _: Handle,
        _: Handle,
        out: *mut u32,
    ) -> i32 {
        unsafe { out.write(0b0110) };
        0
    }

    pub(crate) unsafe extern "C" fn device_local_types(_: Handle) -> u32 {
        0b1100
    }

    pub(crate) unsafe extern "C" fn allocate_memory(
        _: Handle,
        image: u64,
        _: Handle,
        bytes: u64,
        kind: u32,
        out: *mut u64,
    ) -> i32 {
        with(|c| check!(c, bytes == 256 && kind == 2));
        unsafe { out.write(image + 100) };
        0
    }

    pub(crate) unsafe extern "C" fn bind_image(_: Handle, image: u64, memory: u64) -> i32 {
        with(|c| {
            check!(c, memory == image + 100);
            if image == c.bind_error_at { UNKNOWN } else { 0 }
        })
    }

    pub(crate) unsafe extern "C" fn destroy_image(_: Handle, _: u64) {
        with(|c| c.retirement.push(Step::Image))
    }

    pub(crate) unsafe extern "C" fn free_memory(_: Handle, _: u64) {
        with(|c| c.retirement.push(Step::Memory))
    }
}

pub(crate) unsafe fn close(_: Handle) {
    with(|c| c.retirement.push(Step::Graphics))
}

pub(crate) unsafe fn bridge_create(setup: *const BridgeCreate, out: *mut Handle) -> i32 {
    let setup = unsafe { &*setup };
    with(|c| {
        check!(
            c,
            setup.struct_size as usize == size_of::<BridgeCreate>()
                && setup.abi_version == RSF_FSR4_BRIDGE_ABI_VERSION
        );
        check!(
            c,
            setup.adapter_luid == ADAPTER
                && setup.depth_inverted == 1
                && setup.motion_jittered == 1
        );
        check!(
            c,
            (setup.render_width, setup.render_height) == OUTPUT
                && (setup.output_width, setup.output_height) == OUTPUT
        );
        let expected: Vec<_> = "C:\\signedé".encode_utf16().chain([0]).collect();
        check!(
            c,
            unsafe { std::slice::from_raw_parts(setup.runtime_directory, expected.len()) }
                == expected
        );
    });
    unsafe { out.write(BRIDGE) };
    0
}

pub(crate) unsafe fn bridge_plan(
    owner: Handle,
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    with(|c| check!(c, owner == BRIDGE && quality == 2));
    unsafe {
        width.write(RENDER.0);
        height.write(RENDER.1);
    };
    0
}

pub(crate) unsafe fn bridge_info(owner: *const c_void, out: *mut Info) -> i32 {
    let info = unsafe { &mut *out };
    with(|c| {
        check!(
            c,
            owner == BRIDGE.cast_const()
                && info.struct_size as usize == size_of::<Info>()
                && info.abi_version == RSF_FSR4_BRIDGE_ABI_VERSION
        )
    });
    info.adapter_luid = ADAPTER;
    info.provider_version = VERSION;
    info.vendor_id = 0x8086;
    info.int8_compatibility = 1;
    0
}

pub(crate) unsafe extern "C" fn create_fence(owner: Handle, out: *mut Handle) -> i32 {
    with(|c| check!(c, owner == BRIDGE));
    event(out)
}

pub(crate) unsafe fn create_image(
    owner: Handle,
    width: u32,
    height: u32,
    format: u32,
    writable: u32,
    handle: *mut Handle,
    out: *mut Handle,
) -> i32 {
    let index = with(|c| {
        let index = c.resources;
        c.resources += 1;
        check!(c, owner == BRIDGE && FORMATS.get(index) == Some(&format));
        check!(
            c,
            writable == u32::from(index == 3) && (width, height) == extent(index)
        );
        index
    });
    unsafe { out.write(ptr::without_provenance_mut(300 + index)) };
    event(handle)
}

pub(crate) unsafe extern "C" fn release_image(_: Handle) {
    with(|c| c.retirement.push(Step::Com))
}

pub(crate) unsafe fn capture_initialize(
    _: Handle,
    _: *const Setup,
    provider: u64,
) -> i32 {
    with(|c| check!(c, provider == VERSION));
    0
}

pub(crate) unsafe fn capture_begin(record: *const Record) {
    let r = unsafe { &*record };
    with(|c| check!(c, r.session_id == 10 && r.view_id == 11));
}

pub(crate) unsafe fn capture_record(
    _: Handle,
    commands: Handle,
    image: *const Image,
    _: *const Record,
    role: *const std::ffi::c_char,
    reset: u32,
) {
    let (image, role) = unsafe { ((*image).image, CStr::from_ptr(role).to_bytes().to_vec()) };
    with(|c| c.captures.push((commands, image, role, reset)));
}

pub(crate) unsafe fn capture_parameters(_: *const Frame, reset: u32) {
    with(|c| check!(c, c.dispatches.last().is_some_and(|f| f.reset == reset)));
}

pub(crate) unsafe fn capture_flush(_: Handle) -> i32 {
    with(|c| {
        c.retirement.push(Step::Flush);
        i32::from(!c.flush_error)
    })
}

pub(crate) unsafe fn transition(_: Handle, commands: Handle, plan: *const Barrier) {
    let plan = unsafe { *plan };
    with(|c| {
        check!(
            c,
            plan.source_stages == STAGE && plan.target_stages == STAGE
        );
        c.barriers.push((commands, plan));
    });
}

pub(crate) unsafe fn copy(
    _: Handle,
    commands: Handle,
    source: u64,
    target: u64,
    width: u32,
    height: u32,
) {
    with(|c| {
        check!(
            c,
            (width, height) == if commands == PRODUCER { RENDER } else { OUTPUT }
        );
        c.copies.push((commands, source, target));
    });
}

pub(crate) unsafe fn split(
    handoff: *const Handoff,
    commands: Handle,
    sem: u64,
    prior: u64,
    ready: u64,
    done: u64,
    out: *mut Handle,
) -> i32 {
    with(|c| {
        check!(
            c,
            unsafe { (*handoff).user == DEVICE && (*out).is_null() }
                && commands == PRODUCER
                && sem == TIMELINE
        );
        c.splits.push([prior, ready, done]);
        unsafe { out.write(if c.repeat_commands { PRODUCER } else { RESUMED }) };
    });
    0
}

pub(crate) unsafe fn bridge_sharpness(owner: Handle, amount: f32) -> i32 {
    with(|c| {
        check!(c, owner == BRIDGE);
        c.sharpness = amount;
    });
    0
}

pub(crate) unsafe fn bridge_evaluate(owner: Handle, frame: *const BridgeFrame) -> i32 {
    let frame = unsafe { *frame };
    with(|c| {
        check!(
            c,
            owner == BRIDGE
                && frame.struct_size as usize == size_of::<BridgeFrame>()
                && frame.abi_version == RSF_FSR4_BRIDGE_ABI_VERSION
        );
        check!(
            c,
            [
                frame.color.addr(),
                frame.depth.addr(),
                frame.motion.addr(),
                frame.output.addr()
            ] == [300, 301, 302, 303]
        );
        check!(
            c,
            c.splits
                .last()
                .is_some_and(|s| s[1] == frame.ready_value && s[2] == frame.done_value)
        );
        check!(
            c,
            (
                frame.motion_scale_x,
                frame.motion_scale_y,
                frame.jitter_x,
                frame.pre_exposure
            ) == (-192.0, 108.0, 0.25, 2.0)
        );
        c.dispatches.push(frame);
        c.expected_done = frame.done_value;
        c.sdk_error
    })
}

pub(crate) unsafe fn sharpen(
    _: Handle,
    _: *const Setup,
    commands: Handle,
    _: *const Frame,
    image: u64,
) {
    with(|c| {
        check!(c, commands == RESUMED && image == 103);
        check!(
            c,
            c.barriers
                .last()
                .is_some_and(|(commands, plan)| *commands == RESUMED
                    && plan.image == image
                    && plan.source_family == FAMILY_EXTERNAL
                    && plan.target_family == FAMILY
                    && plan.old_layout == GENERAL
                    && plan.new_layout == GENERAL)
        );
        c.sharpened += 1;
    });
}

pub(crate) unsafe fn retire(_: Handle, commands: Handle) {
    with(|c| {
        check!(c, commands == RESUMED);
        c.retires += 1;
    })
}

pub(crate) unsafe fn clear_sharpen(_: Handle) {
    with(|c| c.retirement.push(Step::Clear))
}

pub(crate) unsafe fn wait(_: Handle, sem: u64, target: u64, timeout: u64) -> i32 {
    with(|c| {
        check!(
            c,
            sem == TIMELINE && target == c.expected_done && timeout == WAIT_NS
        );
        c.retirement.push(Step::Wait);
        c.wait_error
    })
}

pub(crate) unsafe fn idle(_: Handle) -> i32 {
    with(|c| {
        c.retirement.push(Step::Idle);
        c.idle_error
    })
}

pub(crate) unsafe fn bridge_prepare_destroy(owner: Handle) -> i32 {
    with(|c| {
        check!(c, owner == BRIDGE);
        c.retirement.push(Step::Prepare);
        c.prepare_error
    })
}

pub(crate) unsafe fn bridge_destroy(owner: Handle) -> i32 {
    with(|c| {
        check!(c, owner == BRIDGE);
        c.retirement.push(Step::Bridge);
        c.destroy_error
    })
}

struct Fixture {
    setup: Setup,
    handoff: Handoff,
    record: Record,
    frame: Frame,
    owner: *mut Owner,
    next: Handle,
}
impl Fixture {
    fn new() -> Self {
        with(|c| {
            *c = Calls {
                failures: c.failures,
                ..Calls::default()
            }
        });
        let setup = Setup {
            struct_size: size_of::<Setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: ptr::without_provenance_mut(5),
            physical_device: ptr::without_provenance_mut(6),
            device: DEVICE,
            backend: RSF_SR_FSR4,
            quality: 2,
            output_width: OUTPUT.0,
            output_height: OUTPUT.1,
            depth_inverted: 1,
            motion_jittered: 1,
            runtime_directory_utf8: c"C:\\signedé".as_ptr(),
            instance_proc: leaf!(|_: Handle, _: *const std::ffi::c_char| -> rsf_vk_function {
                None
            }),
            device_proc: leaf!(|_: Handle, _: *const std::ffi::c_char| -> rsf_vk_function { None }),
            ..unsafe { std::mem::zeroed() }
        };
        let handoff = Handoff {
            struct_size: size_of::<Handoff>() as u32,
            abi_version: RSF_SRVK_SPLIT_TRACKED_ABI_VERSION,
            timeline_enabled: 1,
            queue_family: FAMILY,
            user: DEVICE,
            split_record: leaf!(|_: Handle,
                                 _: Handle,
                                 _: u64,
                                 _: u64,
                                 _: u64,
                                 _: u64,
                                 _: *mut Handle|
             -> i32 { -1 }),
        };
        let mut record: Record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<Record>() as u32;
        record.abi_version = rsf_game_sys::RSF_GAME_FRAME_ABI_VERSION;
        record.session_id = 10;
        record.view_id = 11;
        record.frame_id = 42;
        record.resource_generation = 12;
        (record.render_width, record.render_height) = RENDER;
        (record.output_width, record.output_height) = OUTPUT;
        record.frame_time_ms = 16.5;
        let camera = &mut record.camera;
        camera.struct_size = size_of_val(camera) as u32;
        camera.abi_version = rsf_game_sys::RSF_GAME_CAMERA_ABI_VERSION;
        (camera.render_width, camera.render_height) = RENDER;
        (camera.output_width, camera.output_height) = OUTPUT;
        camera.depth_inverted = 1;
        camera.motion_jittered = 1;
        camera.jitter_pixels = [0.25, -0.25];
        camera.near_plane = 0.1;
        camera.far_plane = 1000.0;
        camera.vertical_fov_radians = 1.0;
        let image = |image, index| {
            let (width, height) = extent(index);
            Image {
                device: DEVICE,
                image,
                format: FORMATS[index],
                width,
                height,
                layout: if index == 3 {
                    TRANSFER_DST
                } else {
                    SHADER_LAYOUT
                },
                copy_source: u32::from(index != 3),
            }
        };
        let frame = Frame {
            struct_size: size_of::<Frame>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            color: image(20, 0),
            depth: image(21, 1),
            motion: image(22, 2),
            output: image(23, 3),
            // The owner admits only a valid record (sharpening_policy::valid), as callers send.
            sharpening: rsf_sharpening_options {
                mode: SHARPENING_VENDOR,
                sharpness: 0.7,
                ..crate::sharpening_policy::defaults()
            },
            motion_scale_x: -192.0,
            motion_scale_y: 108.0,
            pre_exposure: 2.0,
            ..unsafe { std::mem::zeroed() }
        };
        Self {
            setup,
            handoff,
            record,
            frame,
            owner: ptr::null_mut(),
            next: ptr::null_mut(),
        }
    }
    fn create(&mut self) -> i32 {
        unsafe { create(&self.setup, &self.handoff, &mut self.owner) }
    }
    fn evaluate(&mut self) -> i32 {
        self.frame.record = &raw const self.record;
        unsafe { rsf_fsr4vk_evaluate(self.owner, PRODUCER, &self.frame, &mut self.next) }
    }
    fn destroy(&self) -> i32 {
        unsafe { rsf_fsr4vk_destroy(self.owner) }
    }
    fn submitted(&self) {
        let submit = |cmd| unsafe { rsf_fsr4vk_submitted(self.owner, cmd) };
        expect!(submit(RESUMED) == INVALID);
        expect!(submit(ptr::without_provenance_mut(789)) == INVALID);
        expect!(submit(PRODUCER) == 0);
        expect!(submit(PRODUCER) == INVALID);
        expect!(self.destroy() == NOT_READY && with(|c| c.retirement.is_empty()));
        expect!(submit(RESUMED) == 0);
    }
}

fn evaluate_and_retire() {
    let mut f = Fixture::new();
    expect!(f.create() == 0 && !f.owner.is_null() && with(|c| c.images == 4 && c.resources == 4));
    if f.owner.is_null() {
        return;
    }
    let (mut width, mut height) = (0, 0);
    expect!(
        unsafe { rsf_fsr4vk_plan(f.owner, &mut width, &mut height) } == 0
            && (width, height) == RENDER
    );
    let mut info = Info {
        struct_size: size_of::<Info>() as u32,
        abi_version: RSF_FSR4_BRIDGE_ABI_VERSION,
        ..unsafe { std::mem::zeroed() }
    };
    expect!(
        unsafe { rsf_fsr4vk_info(f.owner, &mut info) } == 0 && info.provider_version == VERSION
    );
    f.frame.depth.copy_source = 0;
    expect!(f.evaluate() == INVALID && f.next == PRODUCER && with(|c| c.barriers.is_empty()));
    f.frame.depth.copy_source = 1;
    expect!(f.evaluate() == 0 && f.next == RESUMED && with(|c| c.sharpness == 0.7));
    expect!(with(|c| c.splits.last() == Some(&[0, 1, 2])));
    expect!(with(|c| c.dispatches.last().is_some_and(|r| r.reset == 1)
        && c.sharpened == 1
        && c.copies.len() == 4));
    expect!(with(|c| c.copies.last() == Some(&(RESUMED, 103, 23))));
    expect!(with(|c| c.captures.len() == 5
        && c.captures.last().is_some_and(
            |(list, image, role, _)| *list == RESUMED
                && *image == 103
                && role.as_slice() == b"sr-output-linear"
        )));
    expect!(with(|c| c.captures.iter().all(|capture| capture.3 == 1)));
    expect!(with(|c| c.barriers.get(1).is_some_and(|(_, p)| p
        .source_family
        == FAMILY_IGNORED
        && p.target_family == FAMILY_IGNORED
        && p.old_layout == UNDEFINED
        && p.source_access == 0)));
    f.record.frame_id += 1;
    expect!(f.evaluate() == NOT_READY && f.next == PRODUCER && with(|c| c.splits.len() == 1));
    f.submitted();
    unsafe { rsf_fsr4vk_retire_commands(f.owner, RESUMED) };
    expect!(with(|c| c.retires == 1));

    f.record.frame_id -= 1;
    expect!(f.evaluate() == INVALID && f.next == PRODUCER && with(|c| c.splits.len() == 1));
    f.record.frame_id += 1;
    f.record.view_id += 1;
    expect!(f.evaluate() == INVALID && f.next == PRODUCER && with(|c| c.splits.len() == 1));
    f.record.view_id -= 1;
    let before = with(|c| {
        c.sdk_error = 71;
        c.barriers.len()
    });
    expect!(
        f.evaluate() == 71
            && f.next == RESUMED
            && with(|c| c.dispatches.last().is_some_and(|r| r.reset == 0))
    );
    expect!(with(|c| c.splits.last() == Some(&[2, 3, 4])));
    expect!(with(|c| c.barriers.get(before + 1).is_some_and(
        |(_, p)| p.source_family == FAMILY_EXTERNAL
            && p.target_family == FAMILY
            && p.old_layout == GENERAL
            && p.source_access == 0
    )));
    expect!(with(|c| c.sharpened == 1
        && c.copies.len() == 7
        && c.captures.len() == 9));
    f.submitted();

    with(|c| c.sdk_error = 0);
    f.record.frame_id += 1;
    expect!(f.evaluate() == 0 && with(|c| c.dispatches.last().is_some_and(|r| r.reset == 1)));
    f.submitted();
    f.record.frame_id += 1;
    f.record.resource_generation += 1;
    expect!(f.evaluate() == 0 && with(|c| c.dispatches.last().is_some_and(|r| r.reset == 1)));
    f.submitted();
    let gap = with(|c| c.captures.len());
    f.record.frame_id += 2;
    expect!(f.evaluate() == 0 && with(|c| c.dispatches.last().is_some_and(|r| r.reset == 1)));
    expect!(with(|c| c
        .captures
        .iter()
        .skip(gap)
        .all(|capture| capture.3 == 1)));
    f.submitted();

    use Step::*;
    let timeout = vk::Result::TIMEOUT.as_raw();
    with(|c| c.wait_error = timeout);
    expect!(f.destroy() == timeout && with(|c| c.retirement == [Wait]));
    with(|c| {
        c.wait_error = 0;
        c.idle_error = -78;
        c.retirement.clear();
    });
    expect!(f.destroy() == -78 && with(|c| c.retirement == [Wait, Idle]));
    with(|c| {
        c.idle_error = 0;
        c.prepare_error = -79;
        c.retirement.clear();
    });
    expect!(f.destroy() == -79 && with(|c| c.retirement == [Wait, Idle, Prepare]));
    f.record.frame_id += 1;
    expect!(f.evaluate() == INVALID && f.next.is_null());
    with(|c| {
        c.prepare_error = 0;
        c.flush_error = true;
        c.retirement.clear();
    });
    expect!(f.destroy() == UNKNOWN && with(|c| c.retirement == [Wait, Idle, Prepare, Flush]));
    expect!(unsafe { rsf_fsr4vk_plan(f.owner, &mut width, &mut height) } == INVALID);
    with(|c| {
        c.flush_error = false;
        c.destroy_error = -80;
        c.retirement.clear();
    });
    expect!(
        f.destroy() == -80
            && with(|c| c.retirement
                == [
                    Wait, Idle, Prepare, Flush, Clear, Image, Memory, Com, Image, Memory, Com,
                    Image, Memory, Com, Image, Memory, Com, Timeline, Importer, Bridge
                ])
    );
    with(|c| {
        c.destroy_error = 0;
        c.retirement.clear();
    });
    expect!(
        f.destroy() == 0
            && with(|c| c.retirement == [Idle, Prepare, Flush, Clear, Bridge, Graphics])
    );
}

fn create_failure_and_legacy() {
    use Step::*;
    let mut f = Fixture::new();
    with(|c| {
        c.bind_error_at = 101;
        c.prepare_error = -90;
    });
    expect!(f.create() == UNKNOWN && !f.owner.is_null());
    expect!(with(|c| c.retirement == [Idle, Prepare]));
    expect!(f.evaluate() == INVALID && f.next.is_null());
    with(|c| {
        c.prepare_error = 0;
        c.retirement.clear();
    });
    expect!(
        f.destroy() == 0
            && with(|c| c.retirement
                == [
                    Idle, Prepare, Flush, Clear, Image, Memory, Com, Image, Memory, Com, Timeline,
                    Importer, Bridge, Graphics
                ])
    );

    let mut legacy = Fixture::new();
    legacy.handoff.abi_version = RSF_SRVK_SPLIT_ABI_VERSION;
    expect!(legacy.create() == 0 && legacy.evaluate() == 0 && legacy.next == RESUMED);
    expect!(unsafe { rsf_fsr4vk_submitted(legacy.owner, PRODUCER) } == INVALID);
    expect!(legacy.destroy() == 0);

    let mut terminal = Fixture::new();
    expect!(terminal.create() == 0);
    with(|c| c.repeat_commands = true);
    expect!(
        terminal.evaluate() == INITIALIZATION_FAILED
            && terminal.next.is_null()
            && with(|c| c.dispatches.is_empty())
    );
    expect!(terminal.evaluate() == INVALID && terminal.next.is_null());
    // The terminal owner remains live through process exit.
    expect!(terminal.destroy() == NOT_READY && with(|c| c.retirement.is_empty()));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_fsr4sr_state() -> i32 {
    with(|c| c.failures = 0);
    evaluate_and_retire();
    create_failure_and_legacy();
    i32::from(with(|c| c.failures) != 0)
}
