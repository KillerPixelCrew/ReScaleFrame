// SPDX-License-Identifier: MIT
//! CPU doubles for the bridge state machine: the leaves the owner calls in the fixture-leaves
//! archive, driven by the native state fixture.
use super::*;
use std::cell::RefCell;

const LUID: u64 = 201;
const PROVIDER: u64 = 202;
const ERROR: i32 = -93;
const COLOR: usize = 10;
const DEPTH: usize = 11;
const MOTION: usize = 12;
const HUDLESS: usize = 13;

#[derive(Default)]
struct Calls {
    objects: u32,
    reported_provider: u64,
    sequence: u64,
    done: u64,
    completion: u64,
    presents: u32,
    releases: u32,
    destroys_generation: u32,
    releases_chain: u32,
    destroys_swapchain: u32,
    configure_result: i32,
    wait_result: i32,
    fail_generation_destroy: bool,
    fail_swapchain_destroy: bool,
    notify: Option<(Handle, GeneratedFn)>,
    resets: Vec<u32>,
    steps: Vec<u8>,
    failures: u32,
}

thread_local! {
    static CALLS: RefCell<Calls> = RefCell::new(Calls { reported_provider: PROVIDER, ..Calls::default() });
}

fn with<T>(f: impl FnOnce(&mut Calls) -> T) -> T {
    CALLS.with(|calls| f(&mut calls.borrow_mut()))
}

fn fake(id: usize) -> Handle {
    ptr::without_provenance_mut(id)
}

pub(super) unsafe fn allocate(telemetry: Handle, notify: GeneratedFn) -> Handle {
    with(|c| c.notify = Some((telemetry, notify)));
    fake(1)
}

pub(super) unsafe fn release(_: Handle) {
    with(|c| c.releases += 1)
}

pub(super) unsafe fn create_factory(_: Handle, flags: u32) -> i32 {
    with(|c| check!(c, flags == 0));
    0
}

pub(super) unsafe fn find_adapter(_: Handle, luid: u64) -> i32 {
    with(|c| check!(c, luid == LUID));
    0
}

pub(super) unsafe fn get_adapter_vendor(_: Handle, vendor: *mut u32) -> i32 {
    unsafe { vendor.write(0x1002) };
    0
}

pub(super) unsafe fn create_device(_: Handle, level: u32) -> i32 {
    with(|c| {
        check!(c, level == 0xc000);
        c.objects |= DEVICE;
    });
    0
}

pub(super) unsafe fn load(_: Handle, _: *const u16) -> i32 {
    0
}

pub(super) unsafe fn versions(_: Handle, out: *mut Version, count: *mut u64) -> i32 {
    with(|c| check!(c, unsafe { count.read() } >= 1));
    unsafe {
        count.write(1);
        out.write(Version {
            id: PROVIDER,
            name: c"4.0.1".as_ptr(),
            major: 4,
            minor: 0,
            patch: 1,
        });
    }
    0
}

pub(super) unsafe fn create_generation(_: Handle, _: *const BridgeCreate, provider: u64) -> i32 {
    with(|c| {
        check!(c, provider == PROVIDER);
        c.objects |= GENERATION;
    });
    0
}

pub(super) unsafe fn get_provider(_: Handle, out: *mut u64) -> i32 {
    unsafe { out.write(with(|c| c.reported_provider)) };
    0
}

pub(super) unsafe fn create_queue(_: Handle, desc: *const QueueDesc) -> i32 {
    let d = unsafe { &*desc };
    with(|c| {
        check!(
            c,
            d.list_type == 0 && d.priority == 0 && d.flags == 0 && d.node_mask == 0
        )
    });
    0
}

pub(super) unsafe fn create_allocator(_: Handle, kind: u32) -> i32 {
    with(|c| check!(c, kind == 0));
    0
}

pub(super) unsafe fn create_command_list(_: Handle, mask: u32, kind: u32) -> i32 {
    with(|c| check!(c, mask == 0 && kind == 0));
    0
}

pub(super) unsafe fn create_completion_fence(_: Handle, value: u64, flags: u32) -> i32 {
    with(|c| check!(c, value == 0 && flags == 0));
    0
}

pub(super) unsafe fn create_event(_: Handle, manual: u32, initial: u32) -> i32 {
    with(|c| {
        check!(c, manual == 0 && initial == 0);
        c.objects |= LOCAL_FENCE;
    });
    0
}

pub(super) unsafe fn objects(_: Handle) -> u32 {
    with(|c| c.objects)
}

pub(super) unsafe fn create_swapchain(
    _: Handle,
    _: Handle,
    desc: *const SwapchainDesc,
    first: u32,
) -> i32 {
    let d = unsafe { &*desc };
    with(|c| {
        check!(c, (d.width, d.height, d.format) == (640, 360, 87));
        check!(
            c,
            (d.buffer_count, d.buffer_usage, d.swap_effect) == (3, 0x20, 4)
        );
        check!(c, (d.sample_count, d.sample_quality, d.stereo) == (1, 0, 0));
        check!(c, (d.scaling, d.alpha_mode, d.flags, first) == (0, 0, 0, 1));
        c.objects |= CHAIN | SWAPCHAIN;
    });
    0
}

pub(super) unsafe fn create_image(
    _: Handle,
    _: u32,
    _: u32,
    _: u32,
    _: *mut Handle,
    _: *mut Handle,
) -> i32 {
    ERROR
}

pub(super) unsafe fn create_fence(_: Handle, out: *mut Handle) -> i32 {
    unsafe { out.write(fake(2)) };
    with(|c| c.objects |= SHARED_FENCE);
    0
}

pub(super) unsafe fn describe_texture(resource: Handle, out: *mut Texture) {
    let (width, height, format) = match resource.addr() {
        DEPTH => (320, 180, 41),
        MOTION => (320, 180, 34),
        _ => {
            with(|c| check!(c, resource == fake(COLOR) || resource == fake(HUDLESS)));
            (640, 360, 87)
        }
    };
    unsafe {
        out.write(Texture {
            width,
            height,
            format,
            dimension: 3,
            layers: 1,
            samples: 1,
        })
    };
}

pub(super) unsafe fn reset_commands(_: Handle) -> i32 {
    with(|c| c.steps.push(b'R'));
    0
}

pub(super) unsafe fn close_commands(_: Handle) -> i32 {
    with(|c| c.steps.push(b'C'));
    0
}

pub(super) unsafe fn configure(
    _: Handle,
    sequence: u64,
    width: u32,
    height: u32,
    enabled: u32,
    image: Handle,
) -> i32 {
    with(|c| {
        c.sequence += 1;
        check!(c, (sequence, width, height) == (c.sequence, 640, 360));
        check!(c, enabled == 0 || image == fake(HUDLESS));
        c.steps.push(b'F');
        c.configure_result
    })
}

pub(super) unsafe fn shader_read(_: Handle, image: Handle, enter: u32) {
    with(|c| {
        c.steps.push(if enter != 0 {
            b'+'
        } else if image == fake(HUDLESS) {
            b'H'
        } else {
            b'-'
        })
    });
}

pub(super) unsafe fn prepare(_: Handle, _: *const BridgeFrame, sequence: u64, reset: u32) -> i32 {
    with(|c| {
        check!(c, sequence == c.sequence);
        c.resets.push(reset);
        c.steps.push(b'G');
    });
    0
}

pub(super) unsafe fn copy_to_backbuffer(_: Handle, image: Handle) -> i32 {
    with(|c| {
        check!(c, image == fake(COLOR));
        c.steps.push(b'B');
    });
    0
}

pub(super) unsafe fn wait_shared(_: Handle, ready: u64) -> i32 {
    with(|c| {
        check!(c, ready == c.done + 1);
        c.steps.push(b'W');
    });
    0
}

pub(super) unsafe fn execute(_: Handle) {
    with(|c| c.steps.push(b'X'))
}

pub(super) unsafe fn present(_: Handle, vsync: u32) -> i32 {
    with(|c| {
        check!(c, vsync == 1);
        c.steps.push(b'P');
        c.presents += 1;
    });
    0
}

pub(super) unsafe fn wait_presents(_: Handle) -> i32 {
    with(|c| {
        c.steps.push(b'A');
        c.wait_result
    })
}

pub(super) unsafe fn signal(_: Handle, value: u64, shared: u32) -> i32 {
    with(|c| {
        if shared != 0 {
            check!(c, value == c.done + 2);
            c.done = value;
            c.steps.push(b'D');
        } else {
            c.completion += 1;
            check!(c, value == c.completion);
            c.steps.push(b'S');
        }
    });
    0
}

pub(super) unsafe fn wait_local(_: Handle, value: u64) -> i32 {
    with(|c| {
        check!(c, value == c.completion);
        c.steps.push(b'L');
    });
    0
}

pub(super) unsafe fn get_present_count(_: Handle, out: *mut u32) -> i32 {
    unsafe { out.write(with(|c| c.presents)) };
    0
}

pub(super) unsafe fn destroy_generation(_: Handle) -> i32 {
    with(|c| {
        c.destroys_generation += 1;
        if c.fail_generation_destroy {
            return ERROR;
        }
        c.objects &= !GENERATION;
        0
    })
}

pub(super) unsafe fn release_chain(_: Handle) {
    with(|c| {
        c.releases_chain += 1;
        c.objects &= !CHAIN;
    })
}

pub(super) unsafe fn destroy_swapchain(_: Handle) -> i32 {
    with(|c| {
        c.destroys_swapchain += 1;
        if c.fail_swapchain_destroy {
            return ERROR;
        }
        c.objects &= !SWAPCHAIN;
        0
    })
}

fn create() -> (i32, *mut Bridge) {
    let directory = [b'X' as u16, b':' as u16, b'\\' as u16, 0];
    let setup = BridgeCreate {
        struct_size: size_of::<BridgeCreate>() as u32,
        abi_version: BRIDGE_ABI,
        adapter_luid: LUID,
        runtime_directory: directory.as_ptr(),
        width: 640,
        height: 360,
        depth_inverted: 1,
        depth_infinite: 0,
        motion_jittered: 0,
        generated_count: 1,
    };
    let mut bridge = ptr::null_mut();
    (unsafe { RsfFsr4FgCreate(&setup, &mut bridge) }, bridge)
}

fn reset(provider: u64) {
    with(|c| {
        *c = Calls {
            reported_provider: provider,
            failures: c.failures,
            ..Calls::default()
        }
    });
}

fn retirement() {
    reset(PROVIDER + 1);
    with(|c| c.fail_generation_destroy = true);
    let (code, bridge) = create();
    expect!(code == UNSUPPORTED && !bridge.is_null());
    expect!(unsafe { RsfFsr4FgStart(bridge, fake(3)) } == INVALID);
    let counts = with(|c| (c.destroys_generation, c.releases));
    expect!(counts == (1, 0));
    with(|c| c.fail_generation_destroy = false);
    expect!(unsafe { RsfFsr4FgDestroy(bridge) } == 0 && with(|c| c.releases) == 1);

    reset(PROVIDER);
    let (code, bridge) = create();
    expect!(code == 0 && !bridge.is_null());
    if bridge.is_null() {
        return;
    }
    expect!(unsafe { RsfFsr4FgStart(bridge, fake(3)) } == 0);
    with(|c| c.fail_swapchain_destroy = true);
    expect!(unsafe { RsfFsr4FgPrepareDestroy(bridge) } == ERROR);
    expect!(with(|c| (c.destroys_generation, c.releases_chain, c.releases)) == (1, 1, 0));
    with(|c| c.fail_swapchain_destroy = false);
    expect!(unsafe { RsfFsr4FgPrepareDestroy(bridge) } == 0);
    let retired = with(|c| c.completion);
    expect!(unsafe { RsfFsr4FgDrain(bridge) } == 0 && with(|c| c.completion) == retired);
    expect!(unsafe { RsfFsr4FgDestroy(bridge) } == 0 && with(|c| c.releases) == 1);
    expect!(
        with(|c| (
            c.destroys_generation,
            c.releases_chain,
            c.destroys_swapchain
        )) == (1, 1, 2)
    );
}

fn terminal() {
    reset(PROVIDER);
    let (code, bridge) = create();
    expect!(code == 0 && !bridge.is_null());
    if bridge.is_null() {
        return;
    }
    let mut fence = ptr::null_mut();
    expect!(unsafe { RsfFsr4FgCreateFence(bridge.cast(), &mut fence) } == 0 && !fence.is_null());
    expect!(unsafe { RsfFsr4FgStart(bridge, fake(3)) } == 0);
    with(|c| c.steps.clear());
    let mut source = Record {
        frame_time_ms: 16.7,
        ..rsf_test_support::frame_record(1, 1, 0, [320, 180], [640, 360])
    };
    source.camera.struct_size = size_of_val(&source.camera) as u32;
    source.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    source.camera.depth_inverted = 1;
    source.camera.near_plane = 0.1;
    source.camera.far_plane = 1000.0;
    source.camera.vertical_fov_radians = 1.2;

    let mut status = BridgeStatus {
        struct_size: size_of::<BridgeStatus>() as u32,
        abi_version: BRIDGE_ABI,
        ..unsafe { std::mem::zeroed() }
    };
    let present = |source: &Record, enabled, status: &mut BridgeStatus| {
        let ready_value = with(|c| c.done) + 1;
        let frame = BridgeFrame {
            struct_size: size_of::<BridgeFrame>() as u32,
            abi_version: BRIDGE_ABI,
            record: source,
            color: fake(COLOR),
            depth: fake(DEPTH),
            motion: fake(MOTION),
            hudless: fake(HUDLESS),
            motion_scale_x: 320.0,
            motion_scale_y: 180.0,
            ready_value,
            done_value: ready_value + 1,
            enabled,
            vsync: 1,
        };
        unsafe { RsfFsr4FgPresent(bridge, &frame, status) }
    };

    let Some((telemetry, notify)) = with(|c| c.notify) else {
        expect!(false);
        return;
    };
    unsafe { notify(telemetry, 0, 1) };
    expect!(
        present(&source, 1, &mut status) == 0
            && status.fg_enabled == 1
            && status.frames_presented == 1
            && status.generated_callbacks == 1
    );
    expect!(with(|c| c.steps == b"RF+++G--BCWXPASLRHCXWDS"));

    source.frame_id += 1;
    expect!(present(&source, 1, &mut status) == 0);
    source.frame_id += 2;
    expect!(present(&source, 1, &mut status) == 0);
    source.frame_id += 1;
    source.flags = RSF_FRAME_FLAG_RESET;
    unsafe { notify(telemetry, ERROR, 1) };
    expect!(present(&source, 1, &mut status) == ERROR && status.generated_callbacks == 1);
    expect!(with(|c| c.resets == [1, 0, 1, 1]));

    with(|c| c.configure_result = -92);
    expect!(
        present(&source, 0, &mut status) == -92
            && status.result == -92
            && with(|c| c.presents) == 4
            && status.terminal == 0
    );
    with(|c| c.configure_result = 0);
    expect!(present(&source, 1, &mut status) == ERROR);
    expect!(
        present(&source, 0, &mut status) == 0
            && status.fg_enabled == 0
            && status.generated_callbacks == 1
    );

    let done = with(|c| {
        c.wait_result = ERROR;
        c.done
    });
    expect!(
        present(&source, 0, &mut status) == ERROR
            && status.result == ERROR
            && status.terminal == 1
            && with(|c| c.done) == done
    );
    expect!(unsafe { RsfFsr4FgDrain(bridge) } == ERROR && with(|c| c.done) == done);
    expect!(unsafe { RsfFsr4FgPrepareDestroy(bridge) } == ERROR);
    expect!(unsafe { RsfFsr4FgDestroy(bridge) } == ERROR);
    expect!(with(|c| (c.destroys_generation, c.releases_chain, c.releases)) == (0, 0, 0));
}

/// Run first by the FSR4 FG WSI state fixture (vulkan_dx12_fg/fsr4_state_fixture.rs).
pub(crate) fn rsf_fixture_fsr4fg_bridge_state() -> i32 {
    with(|c| c.failures = 0);
    retirement();
    terminal();
    i32::from(with(|c| c.failures) != 0)
}
