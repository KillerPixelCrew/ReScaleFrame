// SPDX-License-Identifier: MIT
use super::*;
use crate::vulkan_fg_copy::{Barrier, ImageCopy};

#[derive(Default)]
struct Harness {
    events: Vec<&'static str>,
    captures: Vec<(u64, u32)>,
    fence_result: i32,
    present_result: i32,
    configure_result: i32,
    configure_api_error: i32,
    detach_result: i32,
    release_result: i32,
    sdk_value: u64,
}

unsafe fn harness<'a>(user: Handle) -> &'a mut Harness {
    unsafe { &mut *user.cast::<Harness>() }
}

unsafe extern "C" fn submit_fence(user: Handle, _: u64) -> i32 {
    let test = unsafe { harness(user) };
    test.events.push("fence");
    test.fence_result
}

unsafe extern "C" fn wait_fence(_: Handle, _: u64) -> i32 {
    OK
}

unsafe extern "C" fn destroy_fence(user: Handle, _: u64) {
    unsafe { harness(user) }.events.push("destroy fence");
}

static GPU: Gpu = Gpu {
    local_memory_types: None,
    create_image: None,
    requirements: None,
    allocate: None,
    bind: None,
    create_view: None,
    destroy_view: None,
    destroy_image: None,
    free_memory: None,
    create_fence: None,
    destroy_fence: Some(destroy_fence),
    wait_fence: Some(wait_fence),
    reset_fence: Some(wait_fence),
    submit_fence: Some(submit_fence),
};

unsafe extern "C" fn describe(_: *const c_void, out: *mut Swapchain) {
    unsafe {
        out.write(Swapchain {
            width: 1280,
            height: 720,
            format: BGRA8,
            layers: 1,
            has_surface: 1,
        })
    };
}

unsafe extern "C" fn open(setup: *const Setup, out: *mut Handle) -> i32 {
    unsafe { out.write((*setup).queue_user) };
    OK
}

unsafe extern "C" fn close(user: Handle) {
    unsafe { harness(user) }.events.push("close");
}

unsafe extern "C" fn prepare_chain(
    user: Handle,
    support: *mut ChainSupport,
    gpu: *mut Handle,
) -> i32 {
    unsafe {
        support.write(ChainSupport {
            usage: USAGE_TRANSFER_DST,
            capture_enabled: 0,
        });
        gpu.write(user);
    }
    OK
}

unsafe extern "C" fn create_chain(_: Handle, _: u32, chain: *mut u64) -> i32 {
    unsafe { chain.write(3) };
    OK
}

unsafe extern "C" fn destroy_chain(user: Handle, _: u64) {
    unsafe { harness(user) }.events.push("destroy chain");
}

unsafe extern "C" fn start_capture(_: Handle) -> i32 {
    OK
}

unsafe extern "C" fn images(_: Handle, _: u64, count: *mut u32, _: *mut u64) -> i32 {
    unsafe { count.write(3) };
    OK
}

unsafe extern "C" fn acquire(_: Handle, _: u64, _: u64, _: u64, _: u64, _: *mut u32) -> i32 {
    WSI_ERROR
}

unsafe extern "C" fn valid_present(chain: u64, _: *const c_void) -> u32 {
    u32::from(chain == 3)
}

unsafe extern "C" fn present(user: Handle, _: *const c_void) -> i32 {
    unsafe { harness(user) }.present_result
}

unsafe extern "C" fn worker(call: *const rsf_slvk_call) -> i32 {
    let call = unsafe { &*call };
    unsafe { (call.callback.unwrap())(ptr::null_mut(), call.frame_id as usize, call.user) }
}

unsafe extern "C" fn sdk_initialize(_: Handle, _: usize, _: Handle) -> i32 {
    OK
}

unsafe extern "C" fn sdk_configure(_: Handle, _: usize, user: Handle) -> i32 {
    let call = unsafe { &*user.cast::<SdkCall>() };
    let test = unsafe { harness(call.native) };
    test.events.push(if call.count == 0 { "off" } else { "on" });
    if test.configure_api_error != 0 {
        rsf_core_dlgvk_report_api_error(test.configure_api_error);
    }
    test.configure_result
}

unsafe extern "C" fn sdk_sample(_: Handle, _: usize, user: Handle) -> i32 {
    let call = unsafe { &mut *user.cast::<SdkCall>() };
    let test = unsafe { harness(call.native) };
    call.sample = Sample {
        semaphore: 7,
        value: test.sdk_value,
        max_generated: 3,
        ..Default::default()
    };
    OK
}

unsafe extern "C" fn sdk_release(_: Handle, _: usize, user: Handle) -> i32 {
    let call = unsafe { &*user.cast::<SdkCall>() };
    let test = unsafe { harness(call.native) };
    test.events.push("release");
    test.release_result
}

unsafe extern "C" fn barrier(_: Handle, _: Handle, _: *const Barrier) {}
unsafe extern "C" fn copy(_: Handle, _: Handle, _: *const ImageCopy) {}
unsafe extern "C" fn capture(
    _: Handle,
    _: Handle,
    _: *const SourceImage,
    _: *const Record,
    _: u32,
    _: u64,
) {
}
unsafe extern "C" fn remember(_: Handle, _: *const Record, _: *const Status, _: u32) {}
unsafe extern "C" fn group(user: Handle, id: u64, complete: u32) {
    unsafe { harness(user) }.captures.push((id, complete));
}
unsafe extern "C" fn flush(user: Handle) {
    unsafe { harness(user) }.events.push("flush");
}
unsafe extern "C" fn detach(user: Handle) -> i32 {
    let test = unsafe { harness(user) };
    test.events.push("detach");
    test.detach_result
}
unsafe extern "C" fn epoch() -> u64 {
    0
}
unsafe extern "C" fn idle(user: Handle) -> i32 {
    unsafe { harness(user) }.events.push("idle");
    OK
}
unsafe extern "C" fn wait_inputs(user: Handle, semaphore: u64, value: u64) -> i32 {
    let test = unsafe { harness(user) };
    assert_eq!((semaphore, value), (7, test.sdk_value));
    test.events.push("wait SDK");
    OK
}
unsafe extern "C" fn lock(_: Handle) {}
unsafe extern "C" fn proc(_: Handle, _: *const std::ffi::c_char) -> Option<unsafe extern "C" fn()> {
    None
}
unsafe extern "C" fn queue_submit(_: Handle, _: u32, _: *const c_void, _: u64) -> i32 {
    OK
}

const PLATFORM: Platform = Platform {
    gpu: &GPU,
    describe,
    open,
    close,
    prepare_chain,
    create_chain,
    destroy_chain,
    start_capture,
    images,
    acquire,
    valid_present,
    present,
    invoke: worker,
    initialize: Some(sdk_initialize),
    configure: Some(sdk_configure),
    sample: Some(sdk_sample),
    constants: None,
    tag: None,
    release: Some(sdk_release),
    barrier,
    copy,
    capture,
    remember,
    capture_group: group,
    flush,
    detach_capture: detach,
    timeout_epoch: epoch,
    idle,
    wait_inputs,
};

fn create(test: &mut Harness) -> *mut Owner {
    let mut setup: Setup = unsafe { std::mem::zeroed() };
    setup.struct_size = size_of::<Setup>() as u32;
    setup.abi_version = RSF_FGVK_ABI_VERSION;
    let graphics = &mut setup.graphics;
    graphics.struct_size = size_of_val(graphics) as u32;
    graphics.abi_version = RSF_SRVK_ABI_VERSION;
    graphics.instance = ptr::without_provenance_mut(1);
    graphics.physical_device = graphics.instance;
    graphics.device = graphics.instance;
    graphics.instance_proc = Some(proc);
    graphics.device_proc = Some(proc);
    graphics.output_width = 1280;
    graphics.output_height = 720;
    setup.queues[0] = graphics.device;
    setup.create_info = graphics.device;
    setup.queue_user = ptr::from_mut(test).cast();
    setup.graphics_submit = Some(queue_submit);
    setup.lock_queues = Some(lock);
    setup.unlock_queues = Some(lock);
    let mut owner = ptr::null_mut();
    let mut chain = 0;
    assert_eq!(
        unsafe { rsf_core_dlgvk_create(&PLATFORM, &setup, &mut owner, &mut chain) },
        OK
    );
    assert_eq!(chain, 3);
    assert_eq!(unsafe { rsf_core_dlgvk_enable(owner, 1, 3) }, OK);
    owner
}

fn recorded(owner: &mut Owner, frame: u64, commands: Handle) {
    let mut record: Record = unsafe { std::mem::zeroed() };
    record.session_id = 3;
    record.frame_id = frame;
    let slot = &mut owner.slots[slot_index(frame)];
    rsf_core_fgvk_prepare_begin(&mut owner.timeline, &mut slot.frame, &record, commands);
    slot.frame.prepared = 1;
    slot.frame.recorded_prepare = 1;
    rsf_core_fgvk_prepare_accepted(&mut owner.timeline, &mut slot.frame);
    assert_eq!(
        rsf_core_fgvk_hudless_begin(&mut slot.frame, &record, commands),
        1
    );
    slot.frame.hudless = 1;
    slot.frame.recorded_hudless = 1;
    slot.fences = Fences {
        source: 1,
        consumer: 2,
    };
}

#[test]
fn submission_proof_and_refused_teardown_keep_the_owner_live() {
    let mut test = Harness::default();
    let owner = create(&mut test);
    let commands = ptr::without_provenance_mut(100);
    let foreign = ptr::without_provenance_mut(101);
    let info = ptr::without_provenance(102);
    recorded(unsafe { &mut *owner }, 1, commands);
    unsafe { rsf_core_dlgvk_submitted(owner, &foreign, 1) };
    assert_eq!(unsafe { rsf_core_dlgvk_destroy(owner) }, NOT_READY);
    assert!(test.events.is_empty());

    recorded(unsafe { &mut *owner }, 2, commands);
    unsafe { rsf_core_dlgvk_submitted(owner, &commands, 1) };
    assert_eq!(test.captures, [(2, 0), (3, 0)]);
    assert_eq!(test.events, ["fence"]);
    assert_eq!(
        unsafe { rsf_core_dlgvk_present(owner, info, ptr::null(), 0) },
        OK
    );

    test.detach_result = NOT_READY;
    assert_eq!(unsafe { rsf_core_dlgvk_destroy(owner) }, NOT_READY);
    assert_eq!(test.captures, [(2, 0), (3, 0), (4, 1), (5, 1)]);
    assert!(
        test.events
            .ends_with(&["off", "idle", "wait SDK", "flush", "detach"])
    );
    assert!(!test.events.contains(&"release"));

    test.detach_result = OK;
    test.release_result = FEATURE_FAILED;
    assert_eq!(unsafe { rsf_core_dlgvk_destroy(owner) }, FEATURE_FAILED);
    assert!(test.events.ends_with(&["detach", "release"]));
    assert!(!test.events.contains(&"destroy fence"));
    assert!(!test.events.contains(&"close"));

    test.release_result = OK;
    assert_eq!(unsafe { rsf_core_dlgvk_destroy(owner) }, OK);
    assert!(test.events.ends_with(&["destroy chain", "close"]));

    let mut test = Harness {
        present_result: -1000001004,
        ..Default::default()
    };
    let owner = create(&mut test);
    recorded(unsafe { &mut *owner }, 1, commands);
    unsafe { rsf_core_dlgvk_submitted(owner, &commands, 1) };
    assert_eq!(
        unsafe { rsf_core_dlgvk_present(owner, info, ptr::null(), 0) },
        test.present_result
    );
    assert_eq!(unsafe { (*owner).status.presentation.enabled }, 0);
    assert_eq!(unsafe { rsf_core_dlgvk_destroy(owner) }, NOT_READY);
    // A post-error sample can still contain the previous Present's semaphore.
    assert!(!test.events.contains(&"wait SDK"));
    assert!(!test.events.contains(&"detach"));
    assert!(!test.events.contains(&"close"));
    // Test callbacks have no GPU work. Drop the quarantined synthetic owner.
    drop(unsafe { Box::from_raw(owner) });

    let mut test = Harness {
        configure_result: FEATURE_FAILED,
        configure_api_error: -4,
        ..Default::default()
    };
    let owner = create(&mut test);
    assert_eq!(
        unsafe { rsf_core_dlgvk_present(owner, info, ptr::null(), 0) },
        WSI_ERROR
    );
    assert_eq!(unsafe { (*owner).status.asynchronous_present_result }, -4);

    test.configure_result = OK;
    test.configure_api_error = 0;
    assert_eq!(
        unsafe { rsf_core_dlgvk_present(owner, info, ptr::null(), 0) },
        OK
    );
    assert_eq!(unsafe { (*owner).status.asynchronous_present_result }, 0);
    assert_eq!(unsafe { (*owner).status.presentation.result }, OK);
    assert_eq!(unsafe { rsf_core_dlgvk_destroy(owner) }, OK);
}
