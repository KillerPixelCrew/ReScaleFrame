// SPDX-License-Identifier: MIT
use super::*;
use crate::dlss_vulkan_capture_ledger::tests::source;
use std::collections::VecDeque;

const VK_TIMEOUT: i32 = 2;
const VK_DEVICE_LOST: i32 = -4;

#[derive(Default)]
struct Harness {
    events: Vec<String>,
    failures: Vec<(u64, String, i32)>,
    metadata: Vec<(u64, i32, u32, u32)>,
    disabled: bool,
    register_result: i32,
    callback: Option<(Callback, Handle)>,
    wants: bool,
    target_index: u32,
    target_unavailable: bool,
    image_result: Option<(i32, u32, u64)>,
    readbacks: u64,
    record_adds: bool,
    record_result: u32,
    submit_result: i32,
    waits: VecDeque<i32>,
    next_handle: u64,
}

unsafe fn harness<'a>(native: Handle) -> &'a mut Harness {
    unsafe { &mut *native.cast::<Harness>() }
}

unsafe fn event(native: Handle, text: String) {
    unsafe { harness(native) }.events.push(text);
}

unsafe extern "C" fn open(setup: *const Setup, out: *mut Handle) -> i32 {
    let native = unsafe { (*setup).queue_user };
    let disabled = unsafe { harness(native) }.disabled;
    unsafe { out.write(if disabled { ptr::null_mut() } else { native }) };
    OK
}

unsafe extern "C" fn close(native: Handle) {
    unsafe { event(native, "close".into()) };
}

unsafe extern "C" fn register(native: Handle, callback: Option<Callback>, user: Handle) -> i32 {
    let test = unsafe { harness(native) };
    test.events.push(format!("register {}", callback.is_some()));
    if test.register_result == VK_SUCCESS {
        test.callback = callback.map(|callback| (callback, user));
    }
    test.register_result
}

unsafe extern "C" fn create_pool(native: Handle, family: u32, out: *mut u64) -> i32 {
    let test = unsafe { harness(native) };
    test.events.push(format!("create pool {family}"));
    test.next_handle += 1;
    unsafe { out.write(test.next_handle) };
    VK_SUCCESS
}

unsafe extern "C" fn destroy_pool(native: Handle, pool: u64) {
    unsafe { event(native, format!("destroy pool {pool}")) };
}

unsafe extern "C" fn allocate(native: Handle, pool: u64, out: *mut Handle) -> i32 {
    unsafe { event(native, format!("allocate {pool}")) };
    unsafe { out.write(ptr::without_provenance_mut(pool as usize + 100)) };
    VK_SUCCESS
}

unsafe extern "C" fn reset_pool(native: Handle, pool: u64) -> i32 {
    unsafe { event(native, format!("reset pool {pool}")) };
    VK_SUCCESS
}

unsafe extern "C" fn begin(native: Handle, _: Handle) -> i32 {
    unsafe { event(native, "begin".into()) };
    VK_SUCCESS
}

unsafe extern "C" fn end(native: Handle, _: Handle) -> i32 {
    unsafe { event(native, "end".into()) };
    VK_SUCCESS
}

unsafe extern "C" fn create_fence(native: Handle, out: *mut u64) -> i32 {
    let test = unsafe { harness(native) };
    test.next_handle += 1;
    test.events
        .push(format!("create fence {}", test.next_handle));
    unsafe { out.write(test.next_handle) };
    VK_SUCCESS
}

unsafe extern "C" fn destroy_fence(native: Handle, fence: u64) {
    unsafe { event(native, format!("destroy fence {fence}")) };
}

unsafe extern "C" fn reset_fence(native: Handle, fence: u64) -> i32 {
    unsafe { event(native, format!("reset fence {fence}")) };
    VK_SUCCESS
}

unsafe extern "C" fn wait_fence(native: Handle, fence: u64, timeout: u64) -> i32 {
    let test = unsafe { harness(native) };
    assert_eq!(timeout, WAIT_NS);
    test.events.push(format!("wait {fence}"));
    test.waits.pop_front().unwrap_or(VK_SUCCESS)
}

unsafe extern "C" fn wants(native: Handle, _: u64) -> u32 {
    u32::from(unsafe { harness(native) }.wants)
}

unsafe extern "C" fn readbacks(native: Handle) -> u64 {
    unsafe { harness(native) }.readbacks
}

unsafe extern "C" fn target(packet: *const Packet, out: *mut Target) {
    let test = unsafe { harness((*packet).device) };
    test.events.push("target".into());
    unsafe {
        out.write(Target {
            chain: 4,
            index: test.target_index,
            selected: u32::from(!test.target_unavailable),
            usage: TRANSFER_SOURCE,
            layers: 1,
            format: BGRA8,
            width: 640,
            height: 360,
        })
    };
}

unsafe extern "C" fn images(
    packet: *const Packet,
    chain: u64,
    count: *mut u32,
    output: *mut u64,
) -> i32 {
    assert_eq!(chain, 4);
    let test = unsafe { harness((*packet).device) };
    if output.is_null() {
        test.events.push("image count".into());
        unsafe { count.write(1) };
        return VK_SUCCESS;
    }
    test.events.push("images".into());
    assert_eq!(unsafe { count.read() }, 1);
    let (result, returned, image) = test.image_result.unwrap_or((VK_SUCCESS, 1, 77));
    unsafe {
        count.write(returned);
        output.write(image);
    }
    result
}

unsafe extern "C" fn record(
    native: Handle,
    _: Handle,
    image: *const Image,
    record: *const Record,
    generated: i32,
) -> u32 {
    let test = unsafe { harness(native) };
    let image = unsafe { &*image };
    assert_eq!(image.device, native);
    assert_eq!(
        (
            image.image,
            image.format,
            image.width,
            image.height,
            image.layout,
            image.copy_source
        ),
        (77, BGRA8, 640, 360, PRESENT_LAYOUT, 1),
    );
    test.events.push(format!("record {} {generated}", unsafe {
        (*record).frame_id
    }));
    if test.record_adds {
        test.readbacks += 1;
    }
    test.record_result
}

unsafe extern "C" fn submit(packet: *const Packet, _: Handle, fence: u64) -> i32 {
    let native = unsafe { (*packet).device };
    let test = unsafe { harness(native) };
    test.events.push(format!("submit {fence}"));
    test.submit_result
}

unsafe extern "C" fn present_leaf(packet: *const Packet, waits: u32) -> i32 {
    unsafe { event((*packet).device, format!("present {waits}")) };
    VK_SUCCESS
}

unsafe extern "C" fn flush(native: Handle) -> u32 {
    unsafe { event(native, "flush".into()) };
    1
}

unsafe extern "C" fn failure(native: Handle, source: u64, reason: *const c_char, result: i32) {
    let reason = unsafe { CStr::from_ptr(reason) }.to_string_lossy().into();
    unsafe { harness(native) }
        .failures
        .push((source, reason, result));
}

unsafe extern "C" fn trace(native: Handle, _: *const Packet, _: *const Source, found: u32) {
    unsafe { event(native, format!("trace {found}")) };
}

unsafe extern "C" fn metadata(
    native: Handle,
    _: *const Packet,
    _: *const Source,
    sequence: u64,
    result: i32,
    ready: u32,
    flushed: u32,
) {
    unsafe { harness(native) }
        .metadata
        .push((sequence, result, ready, flushed));
}

unsafe extern "C" fn ignore_readback(_: Handle, _: Handle) {}

static PLATFORM: Platform = Platform {
    open,
    close,
    register,
    create_pool,
    destroy_pool,
    allocate,
    reset_pool,
    begin,
    end,
    create_fence,
    destroy_fence,
    reset_fence,
    wait_fence,
    complete_readback: ignore_readback,
    wants,
    readbacks,
    target,
    images,
    record,
    submit,
    present: present_leaf,
    flush,
    failure,
    trace,
    metadata,
};

struct Test {
    harness: Box<Harness>,
    owner: *mut Owner,
}

impl Test {
    fn open(harness: Harness) -> (Self, i32) {
        let mut harness = Box::new(harness);
        let mut setup: Setup = unsafe { std::mem::zeroed() };
        setup.queue_user = ptr::from_mut(&mut *harness).cast();
        let mut owner = ptr::without_provenance_mut(1);
        let result = unsafe { rsf_core_dlss_capture_open(&PLATFORM, &setup, &mut owner) };
        (Self { harness, owner }, result)
    }

    fn enabled() -> Self {
        let (test, result) = Self::open(Harness {
            wants: true,
            record_result: 1,
            record_adds: true,
            ..Harness::default()
        });
        assert_eq!(result, OK);
        assert!(!test.owner.is_null());
        test
    }

    fn native(&mut self) -> Handle {
        ptr::from_mut(&mut *self.harness).cast()
    }

    fn packet(&mut self, frame: u64, family: u32, generated: i32) -> Packet {
        let fake = ptr::without_provenance(8);
        Packet {
            struct_size: size_of::<Packet>() as u32,
            abi_version: PACKET_ABI_VERSION,
            device: self.native(),
            queue: ptr::without_provenance_mut(2),
            present_info: fake,
            swapchain_info: fake,
            native_present: 1,
            source_frame: frame,
            present_frame: 0,
            generated_index: generated,
            boundary_valid: 1,
            queue_family: family,
            native_images: 1,
            native_submit: 1,
            native_wsi_call: 1,
        }
    }

    fn remember(&self, frame: u64, view: u32) {
        let source = source(frame, view);
        unsafe { rsf_core_dlss_capture_remember(self.owner, &source.record, 3, &source.sdk, 1) };
    }

    fn present(&mut self, packet: &Packet) -> i32 {
        let (callback, user) = self.harness.callback.expect("registered callback");
        unsafe { callback(user, packet) }
    }

    fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.harness.events)
    }
}

impl Drop for Test {
    fn drop(&mut self) {
        unsafe { rsf_core_dlss_capture_close(self.owner) };
    }
}

#[test]
fn disabled_capture_publishes_no_owner() {
    let (mut test, result) = Test::open(Harness {
        disabled: true,
        ..Harness::default()
    });
    assert_eq!(result, OK);
    assert!(test.owner.is_null());
    assert!(test.take().is_empty());
    unsafe {
        rsf_core_dlss_capture_flush(test.owner);
        assert_eq!(rsf_core_dlss_capture_detach(test.owner), OK);
    }
}

#[test]
fn refused_registration_closes_the_unpublished_owner() {
    let (mut test, result) = Test::open(Harness {
        register_result: VK_INITIALIZATION_FAILED,
        ..Harness::default()
    });
    assert_eq!(result, NOT_READY);
    assert!(test.owner.is_null());
    assert_eq!(test.take(), ["register true", "close"]);
}

#[test]
fn unmapped_or_invalid_presents_keep_the_original_waits() {
    let mut test = Test::enabled();
    test.take();
    let packet = test.packet(42, 0, -1);
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert_eq!(test.take(), ["present 1"]);
    test.remember(42, 1);
    let mut boundary = test.packet(42, 0, -1);
    boundary.boundary_valid = 0;
    assert_eq!(test.present(&boundary), VK_SUCCESS);
    assert_eq!(test.take(), ["trace 1", "present 1"]);
    let mut short = test.packet(42, 0, -1);
    short.struct_size -= 4;
    assert_eq!(test.present(&short), VK_INITIALIZATION_FAILED);
    let mut missing = test.packet(42, 0, -1);
    missing.native_wsi_call = 0;
    assert_eq!(test.present(&missing), VK_INITIALIZATION_FAILED);
    assert!(test.take().is_empty());
}

#[test]
fn capture_consumes_waits_in_its_fenced_copy_before_a_waitless_present() {
    let mut test = Test::enabled();
    test.remember(42, 1);
    test.take();
    let packet = test.packet(42, 2, 1);
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert_eq!(
        test.take(),
        [
            "trace 1",
            "target",
            "image count",
            "images",
            "create pool 2",
            "allocate 1",
            "create fence 2",
            "reset pool 1",
            "reset fence 2",
            "begin",
            "record 42 1",
            "end",
            "submit 2",
            "wait 2",
            "flush",
            "present 0",
        ]
    );
    assert_eq!(test.harness.metadata, [(1, VK_SUCCESS, 1, 1)]);
    // The family's completed command buffer and fence are reused.
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert_eq!(
        test.take(),
        [
            "trace 1",
            "target",
            "image count",
            "images",
            "reset pool 1",
            "reset fence 2",
            "begin",
            "record 42 1",
            "end",
            "submit 2",
            "wait 2",
            "flush",
            "present 0",
        ]
    );
    assert_eq!(test.harness.metadata[1].0, 2);
    assert!(test.harness.failures.is_empty());
    unsafe { rsf_core_dlss_capture_close(test.owner) };
    test.owner = ptr::null_mut();
    assert_eq!(test.take(), ["destroy fence 2", "destroy pool 1", "close"]);
}

#[test]
fn timed_out_copy_blocks_reuse_and_flush_until_its_own_fence_completes() {
    let mut test = Test::enabled();
    test.remember(42, 1);
    test.harness.waits = [VK_TIMEOUT].into();
    let packet = test.packet(42, 0, -1);
    assert_eq!(test.present(&packet), VK_INITIALIZATION_FAILED);
    let events = test.take();
    assert!(events.ends_with(&["submit 2".into(), "wait 2".into()]));
    assert_eq!(test.harness.failures, [(42, "wait".into(), VK_TIMEOUT)]);
    // Another family completes, but the pending copy still forbids flushing.
    let other = test.packet(42, 1, -1);
    assert_eq!(test.present(&other), VK_SUCCESS);
    assert!(!test.take().contains(&"flush".into()));
    assert_eq!(test.harness.metadata, [(1, VK_SUCCESS, 0, 0)]);
    // The pending family waits its own fence before resetting its pool.
    test.harness.waits = [VK_DEVICE_LOST].into();
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert_eq!(
        test.take(),
        [
            "trace 1",
            "target",
            "image count",
            "images",
            "wait 2",
            "present 1"
        ]
    );
    assert_eq!(
        test.harness.failures.last().unwrap(),
        &(42, "capture-commands".into(), VK_INITIALIZATION_FAILED)
    );
    assert_eq!(test.present(&packet), VK_SUCCESS);
    let events = test.take();
    assert_eq!(events[2..4], ["wait 2".to_string(), "reset pool 1".into()]);
    assert!(events.ends_with(&["flush".into(), "present 0".into()]));
}

#[test]
fn unresolved_real_image_presents_with_its_waits_before_any_command_reset() {
    let mut test = Test::enabled();
    test.remember(42, 1);
    test.harness.target_index = 1;
    test.take();
    let packet = test.packet(42, 0, -1);
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert_eq!(
        test.take(),
        ["trace 1", "target", "image count", "present 1"]
    );
    assert_eq!(
        test.harness.failures,
        [(42, "real-images-count".into(), VK_SUCCESS)]
    );
}

#[test]
fn capture_refuses_missing_selection_and_changed_image_arrays() {
    let mut test = Test::enabled();
    test.remember(42, 1);
    test.take();
    let packet = test.packet(42, 0, -1);
    test.harness.target_unavailable = true;
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert_eq!(test.take(), ["trace 1", "target", "present 1"]);

    test.harness.target_unavailable = false;
    for (code, count, image) in [
        (VK_SUCCESS, 2, 77),
        (VK_SUCCESS, 1, 0),
        (VK_DEVICE_LOST, 1, 77),
    ] {
        test.harness.image_result = Some((code, count, image));
        assert_eq!(test.present(&packet), VK_SUCCESS);
        assert_eq!(
            test.take(),
            ["trace 1", "target", "image count", "images", "present 1"]
        );
    }
    assert_eq!(
        test.harness.failures,
        [
            (
                42,
                "real-chain-copy-contract".into(),
                VK_FORMAT_NOT_SUPPORTED
            ),
            (42, "real-images".into(), VK_INITIALIZATION_FAILED),
            (42, "real-images".into(), VK_INITIALIZATION_FAILED),
            (42, "real-images".into(), VK_DEVICE_LOST),
        ],
    );
}

#[test]
fn unsubmitted_readbacks_stop_recording_and_are_never_flushed() {
    let mut test = Test::enabled();
    test.remember(42, 1);
    test.harness.submit_result = VK_DEVICE_LOST;
    let packet = test.packet(42, 0, -1);
    assert_eq!(test.present(&packet), VK_DEVICE_LOST);
    assert!(test.take().ends_with(&["submit 2".into()]));
    assert_eq!(
        test.harness.failures,
        [(42, "submit".into(), VK_DEVICE_LOST)]
    );
    test.harness.submit_result = VK_SUCCESS;
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert_eq!(test.take(), ["trace 1", "present 1"]);
    unsafe { rsf_core_dlss_capture_flush(test.owner) };
    assert!(test.take().is_empty());
}

#[test]
fn failed_recording_without_a_readback_keeps_capture_available() {
    let mut test = Test::enabled();
    test.remember(42, 1);
    test.harness.record_result = 0;
    test.harness.record_adds = false;
    let packet = test.packet(42, 0, -1);
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert!(test.take().ends_with(&["end".into(), "present 1".into()]));
    assert_eq!(test.harness.failures, [(42, "record".into(), VK_SUCCESS)]);
    test.harness.record_result = 1;
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert!(test.take().ends_with(&["present 0".into()]));
}

#[test]
fn drained_flush_and_checked_detach() {
    let mut test = Test::enabled();
    test.remember(42, 1);
    test.harness.waits = [VK_TIMEOUT].into();
    let packet = test.packet(42, 0, -1);
    assert_eq!(test.present(&packet), VK_INITIALIZATION_FAILED);
    test.take();
    unsafe { rsf_core_dlss_capture_flush(test.owner) };
    assert_eq!(test.take(), ["flush"]);
    test.harness.register_result = VK_INITIALIZATION_FAILED;
    assert_eq!(
        unsafe { rsf_core_dlss_capture_detach(test.owner) },
        NOT_READY
    );
    assert!(test.harness.callback.is_some());
    test.harness.register_result = VK_SUCCESS;
    assert_eq!(unsafe { rsf_core_dlss_capture_detach(test.owner) }, OK);
    assert_eq!(unsafe { rsf_core_dlss_capture_detach(test.owner) }, OK);
    assert_eq!(test.take(), ["register false", "register false"]);
    assert!(test.harness.callback.is_none());
}

#[test]
fn source_collisions_and_unwanted_frames_are_reported_or_skipped() {
    let mut test = Test::enabled();
    test.remember(42, 1);
    test.remember(42, 2);
    assert_eq!(
        test.harness.failures,
        [(
            42,
            "ambiguous-source-identity".into(),
            VK_INITIALIZATION_FAILED
        )]
    );
    test.remember(1 << 32, 1);
    assert_eq!(test.harness.failures[1].1, "source-outside-sdk-token-range");
    test.harness.wants = false;
    test.remember(43, 1);
    test.take();
    let packet = test.packet(43, 0, -1);
    assert_eq!(test.present(&packet), VK_SUCCESS);
    assert_eq!(test.take(), ["present 1"]);
}
