// SPDX-License-Identifier: MIT
//! CPU doubles for the bridge state machine, called by the native state fixture.
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

macro_rules! leaf {
    (|$($name:tt : $ty:ty),*| $(-> $ret:ty)? { $($body:tt)* }) => {{
        unsafe extern "C" fn callback($($name: $ty),*) $(-> $ret)? { $($body)* }
        callback
    }};
}

static PLATFORM: Platform = Platform {
    allocate: leaf!(|telemetry: Handle, notify: GeneratedFn| -> Handle {
        with(|c| c.notify = Some((telemetry, notify)));
        fake(1)
    }),
    release: leaf!(|_: Handle| { with(|c| c.releases += 1) }),
    create_factory: leaf!(|_: Handle, flags: u32| -> i32 {
        with(|c| check!(c, flags == 0));
        0
    }),
    find_adapter: leaf!(|_: Handle, luid: u64| -> i32 {
        with(|c| check!(c, luid == LUID));
        0
    }),
    get_adapter_vendor: leaf!(|_: Handle, vendor: *mut u32| -> i32 {
        unsafe { vendor.write(0x1002) };
        0
    }),
    create_device: leaf!(|_: Handle, level: u32| -> i32 {
        with(|c| {
            check!(c, level == 0xc000);
            c.objects |= DEVICE;
        });
        0
    }),
    load: leaf!(|_: Handle, _: *const u16| -> i32 { 0 }),
    versions: leaf!(|_: Handle, out: *mut Version, count: *mut u64| -> i32 {
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
    }),
    create_generation: leaf!(|_: Handle, _: *const BridgeCreate, provider: u64| -> i32 {
        with(|c| {
            check!(c, provider == PROVIDER);
            c.objects |= GENERATION;
        });
        0
    }),
    get_provider: leaf!(|_: Handle, out: *mut u64| -> i32 {
        unsafe { out.write(with(|c| c.reported_provider)) };
        0
    }),
    create_queue: leaf!(|_: Handle, desc: *const QueueDesc| -> i32 {
        let d = unsafe { &*desc };
        with(|c| {
            check!(
                c,
                d.list_type == 0 && d.priority == 0 && d.flags == 0 && d.node_mask == 0
            )
        });
        0
    }),
    create_allocator: leaf!(|_: Handle, kind: u32| -> i32 {
        with(|c| check!(c, kind == 0));
        0
    }),
    create_command_list: leaf!(|_: Handle, mask: u32, kind: u32| -> i32 {
        with(|c| check!(c, mask == 0 && kind == 0));
        0
    }),
    create_completion_fence: leaf!(|_: Handle, value: u64, flags: u32| -> i32 {
        with(|c| check!(c, value == 0 && flags == 0));
        0
    }),
    create_event: leaf!(|_: Handle, manual: u32, initial: u32| -> i32 {
        with(|c| {
            check!(c, manual == 0 && initial == 0);
            c.objects |= LOCAL_FENCE;
        });
        0
    }),
    objects: leaf!(|_: Handle| -> u32 { with(|c| c.objects) }),
    create_swapchain: leaf!(|_: Handle,
                             _: Handle,
                             desc: *const SwapchainDesc,
                             first: u32|
     -> i32 {
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
    }),
    create_image: leaf!(|_: Handle,
                         _: u32,
                         _: u32,
                         _: u32,
                         _: *mut Handle,
                         _: *mut Handle|
     -> i32 { ERROR }),
    create_fence: leaf!(|_: Handle, out: *mut Handle| -> i32 {
        unsafe { out.write(fake(2)) };
        with(|c| c.objects |= SHARED_FENCE);
        0
    }),
    describe_texture: leaf!(|resource: Handle, out: *mut Texture| {
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
    }),
    reset_commands: leaf!(|_: Handle| -> i32 {
        with(|c| c.steps.push(b'R'));
        0
    }),
    close_commands: leaf!(|_: Handle| -> i32 {
        with(|c| c.steps.push(b'C'));
        0
    }),
    configure: leaf!(|_: Handle,
                      sequence: u64,
                      width: u32,
                      height: u32,
                      enabled: u32,
                      image: Handle|
     -> i32 {
        with(|c| {
            c.sequence += 1;
            check!(c, (sequence, width, height) == (c.sequence, 640, 360));
            check!(c, enabled == 0 || image == fake(HUDLESS));
            c.steps.push(b'F');
            c.configure_result
        })
    }),
    shader_read: leaf!(|_: Handle, image: Handle, enter: u32| {
        with(|c| {
            c.steps.push(if enter != 0 {
                b'+'
            } else if image == fake(HUDLESS) {
                b'H'
            } else {
                b'-'
            })
        });
    }),
    prepare: leaf!(
        |_: Handle, _: *const BridgeFrame, sequence: u64, reset: u32| -> i32 {
            with(|c| {
                check!(c, sequence == c.sequence);
                c.resets.push(reset);
                c.steps.push(b'G');
            });
            0
        }
    ),
    copy_to_backbuffer: leaf!(|_: Handle, image: Handle| -> i32 {
        with(|c| {
            check!(c, image == fake(COLOR));
            c.steps.push(b'B');
        });
        0
    }),
    wait_shared: leaf!(|_: Handle, ready: u64| -> i32 {
        with(|c| {
            check!(c, ready == c.done + 1);
            c.steps.push(b'W');
        });
        0
    }),
    execute: leaf!(|_: Handle| { with(|c| c.steps.push(b'X')) }),
    present: leaf!(|_: Handle, vsync: u32| -> i32 {
        with(|c| {
            check!(c, vsync == 1);
            c.steps.push(b'P');
            c.presents += 1;
        });
        0
    }),
    wait_presents: leaf!(|_: Handle| -> i32 {
        with(|c| {
            c.steps.push(b'A');
            c.wait_result
        })
    }),
    signal: leaf!(|_: Handle, value: u64, shared: u32| -> i32 {
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
    }),
    wait_local: leaf!(|_: Handle, value: u64| -> i32 {
        with(|c| {
            check!(c, value == c.completion);
            c.steps.push(b'L');
        });
        0
    }),
    get_present_count: leaf!(|_: Handle, out: *mut u32| -> i32 {
        unsafe { out.write(with(|c| c.presents)) };
        0
    }),
    destroy_generation: leaf!(|_: Handle| -> i32 {
        with(|c| {
            c.destroys_generation += 1;
            if c.fail_generation_destroy {
                return ERROR;
            }
            c.objects &= !GENERATION;
            0
        })
    }),
    release_chain: leaf!(|_: Handle| {
        with(|c| {
            c.releases_chain += 1;
            c.objects &= !CHAIN;
        })
    }),
    destroy_swapchain: leaf!(|_: Handle| -> i32 {
        with(|c| {
            c.destroys_swapchain += 1;
            if c.fail_swapchain_destroy {
                return ERROR;
            }
            c.objects &= !SWAPCHAIN;
            0
        })
    }),
};

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
    (
        unsafe { rsf_core_fsr4fg_create(&PLATFORM, &setup, &mut bridge) },
        bridge,
    )
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
    let mut source: Record = unsafe { std::mem::zeroed() };
    source.struct_size = size_of::<Record>() as u32;
    source.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    source.frame_id = 1;
    source.session_id = 1;
    source.render_width = 320;
    source.render_height = 180;
    source.output_width = 640;
    source.output_height = 360;
    source.frame_time_ms = 16.7;
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

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_fsr4fg_bridge_state() -> i32 {
    with(|c| c.failures = 0);
    retirement();
    terminal();
    i32::from(with(|c| c.failures) != 0)
}
