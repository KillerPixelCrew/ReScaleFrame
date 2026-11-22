// SPDX-License-Identifier: MIT
// Expected values are the Vulkan header constants written out, not the
// module's names for them, so a wrong constant cannot agree with itself.
use super::*;
use crate::vulkan_sr_abi::rsf_vk_function;
use std::cell::{Cell, RefCell};
use std::ffi::CStr;

const FAMILY: u32 = 3;
const IGNORED: u32 = 0xffff_ffff;
const EXTERNAL: u32 = 0xffff_fffe;
const MEMORY: u32 = 0x8000 | 0x1_0000;
const ALL: u32 = 0x1_0000;

fn expected(image: u64, old: u32, new: u32, from: u32, to: u32, src: u32, dst: u32) -> Step {
    Step::Barrier(Barrier {
        image,
        old_layout: old,
        new_layout: new,
        source_family: from,
        target_family: to,
        source_access: src,
        target_access: dst,
        source_stages: ALL,
        target_stages: ALL,
    })
}

fn source(format: u32, width: u32, height: u32) -> Image {
    Image {
        device: ptr::null_mut(),
        image: 0x10,
        format,
        width,
        height,
        layout: 5,
        copy_source: 1,
    }
}

fn shared(format: u32, width: u32, height: u32) -> SharedImage {
    SharedImage {
        image: 0x20,
        dx12_resource: ptr::null_mut(),
        format,
        width,
        height,
    }
}

#[test]
fn first_guide_copy_discards_target_and_releases_it_external() {
    let steps = copy_steps(
        FAMILY,
        &source(100, 640, 360),
        &shared(100, 640, 360),
        false,
    );
    assert_eq!(
        steps,
        [
            expected(0x10, 5, 6, IGNORED, IGNORED, MEMORY, MEMORY),
            expected(0x20, 0, 7, FAMILY, FAMILY, 0, MEMORY),
            Step::Copy(Transfer {
                source: 0x10,
                target: 0x20,
                source_extent: [640, 360],
                target_extent: [640, 360],
            }),
            expected(0x10, 6, 5, IGNORED, IGNORED, MEMORY, MEMORY),
            expected(0x20, 7, 1, FAMILY, EXTERNAL, MEMORY, 0),
        ]
    );
}

#[test]
fn initialized_target_is_acquired_from_external_general() {
    let steps = copy_steps(FAMILY, &source(83, 640, 360), &shared(83, 640, 360), true);
    assert_eq!(steps[1], expected(0x20, 1, 7, EXTERNAL, FAMILY, 0, MEMORY));
    assert!(matches!(steps[2], Step::Copy(_)));
    assert_eq!(steps[4], expected(0x20, 7, 1, FAMILY, EXTERNAL, MEMORY, 0));
}

#[test]
fn rgba_colour_into_bgra_shared_image_blits_full_extents() {
    let steps = copy_steps(
        FAMILY,
        &source(37, 1920, 1080),
        &shared(44, 1920, 1080),
        true,
    );
    assert_eq!(
        steps[2],
        Step::Blit(Transfer {
            source: 0x10,
            target: 0x20,
            source_extent: [1920, 1080],
            target_extent: [1920, 1080],
        })
    );
    let same = copy_steps(
        FAMILY,
        &source(44, 1920, 1080),
        &shared(44, 1920, 1080),
        true,
    );
    assert!(matches!(same[2], Step::Copy(_)));
}

#[test]
fn placeholder_clear_discards_then_releases_external() {
    assert_eq!(
        clear_steps(FAMILY, 0x30),
        [
            expected(0x30, 0, 7, FAMILY, FAMILY, 0, MEMORY),
            Step::Clear(0x30),
            expected(0x30, 7, 1, FAMILY, EXTERNAL, MEMORY, 0),
        ]
    );
}

#[test]
fn blit_admission_needs_rgba_source_and_bgra_target_features() {
    assert!(blit_formats_supported(0x400, 0x800));
    assert!(blit_formats_supported(0xffff_ffff, 0xffff_ffff));
    assert!(!blit_formats_supported(0, 0x800));
    assert!(!blit_formats_supported(0x400, 0));
    assert!(!blit_formats_supported(0x800, 0x400));
}

#[derive(Default)]
struct Lookups {
    missing: Option<&'static CStr>,
    calls: Vec<(usize, String)>,
}

thread_local! {
    static LOOKUPS: RefCell<Lookups> = RefCell::new(Lookups::default());
}

unsafe extern "C" fn unused_proc() {}

unsafe extern "C" fn test_proc(object: Handle, name: *const c_char) -> rsf_vk_function {
    let name = unsafe { CStr::from_ptr(name) };
    LOOKUPS.with_borrow_mut(|lookups| {
        lookups
            .calls
            .push((object.addr(), name.to_string_lossy().into_owned()));
        (lookups.missing != Some(name)).then_some(unused_proc as unsafe extern "C" fn())
    })
}

#[test]
fn required_dispatch_stops_at_device_miss_and_queries_both_instance_entries() {
    let mut graphics: Graphics = unsafe { std::mem::zeroed() };
    graphics.device = ptr::without_provenance_mut(1);
    graphics.instance = ptr::without_provenance_mut(2);
    graphics.device_proc = Some(test_proc);
    graphics.instance_proc = Some(test_proc);

    for (missing, device_count, instance_count) in [
        (None, 22, 2),
        (Some(c"vkCreateCommandPool"), 7, 0),
        (Some(c"vkGetPhysicalDeviceMemoryProperties"), 22, 2),
    ] {
        LOOKUPS.with_borrow_mut(|lookups| {
            lookups.calls.clear();
            lookups.missing = missing;
        });
        assert_eq!(platform::dispatch_available(&graphics), missing.is_none());
        LOOKUPS.with_borrow(|lookups| {
            assert_eq!(lookups.calls.len(), device_count + instance_count);
            assert!(
                lookups.calls[..device_count]
                    .iter()
                    .all(|(object, _)| *object == 1)
            );
            if instance_count != 0 {
                let instance = &lookups.calls[device_count..];
                assert_eq!(
                    instance[0],
                    (2, "vkGetPhysicalDeviceMemoryProperties".into())
                );
                assert_eq!(
                    instance[1],
                    (2, "vkGetPhysicalDeviceFormatProperties".into())
                );
            }
        });
    }
}

struct PresentTrace {
    reads: Cell<u32>,
    calls: Cell<u32>,
}

unsafe extern "C" fn read_present(
    native: *const c_void,
    count: u32,
    chain: &mut u64,
    index: &mut u32,
    waits: *mut u64,
) {
    let trace = unsafe { &*native.cast::<PresentTrace>() };
    trace.reads.set(trace.reads.get() + 1);
    *chain = 7;
    *index = 2;
    for i in 0..count {
        unsafe { waits.add(i as usize).write(10 + u64::from(i)) };
    }
}

unsafe extern "C" fn present_callback(
    owner: Handle,
    info: &PresentInfo,
    source: *const Record,
    explicit: u32,
) -> i32 {
    let trace = unsafe { &*owner.cast::<PresentTrace>() };
    trace.calls.set(trace.calls.get() + 1);
    assert_eq!(trace.calls.get(), trace.reads.get());
    assert_eq!((info.chain, info.index, explicit), (7, 2, 1));
    assert_eq!(source.addr(), 8);
    let waits = unsafe { slice::from_raw_parts(info.waits, info.wait_count as usize) };
    assert!(
        waits
            .iter()
            .copied()
            .eq(10..10 + u64::from(info.wait_count))
    );
    if !info.result.is_null() {
        unsafe { info.result.write(OUT_OF_DATE) };
    }
    NOT_READY
}

#[test]
fn present_marshaling_checks_shape_before_reading_borrowed_arrays() {
    let trace = PresentTrace {
        reads: Cell::new(0),
        calls: Cell::new(0),
    };
    let owner = ptr::from_ref(&trace).cast_mut().cast();
    let mut input = PresentInput {
        native: owner,
        swapchain_count: 1,
        wait_count: 17,
        has_extension: 0,
        has_image: 1,
        has_waits: 1,
        has_result: 1,
    };
    let submit = |input: &PresentInput, result: &mut i32| unsafe {
        marshal_present(
            present_callback,
            owner,
            input,
            read_present,
            ptr::without_provenance(8),
            1,
            result,
        )
    };
    let mut result = 0;
    assert_eq!(submit(&input, &mut result), INIT_FAILED);
    input.wait_count = 1;
    input.has_waits = 0;
    assert_eq!(submit(&input, &mut result), INIT_FAILED);
    assert_eq!(
        (trace.reads.get(), trace.calls.get(), result),
        (0, 0, INCOMPLETE)
    );

    input.wait_count = 16;
    input.has_waits = 1;
    assert_eq!(submit(&input, &mut result), NOT_READY);
    assert_eq!(
        (trace.reads.get(), trace.calls.get(), result),
        (1, 1, OUT_OF_DATE)
    );

    input.wait_count = 0;
    input.has_waits = 0;
    input.has_result = 0;
    assert_eq!(submit(&input, &mut result), NOT_READY);
    assert_eq!(
        (trace.reads.get(), trace.calls.get(), result),
        (2, 2, INCOMPLETE)
    );

    // A missing VkPresentInfoKHR is refused before any read, even with a well-formed shape.
    input.native = ptr::null();
    input.has_result = 1;
    result = 0;
    assert_eq!(submit(&input, &mut result), INIT_FAILED);
    assert_eq!(
        (trace.reads.get(), trace.calls.get(), result),
        (2, 2, INCOMPLETE)
    );
}

fn chain_facts() -> Swapchain {
    Swapchain {
        width: 640,
        height: 360,
        format: 37,
        usage: 0x10,
        color_space: 0,
        layers: 1,
        sharing_mode: 0,
        min_images: 3,
        flags: 0,
        present_mode: 2,
        extension_count: 0,
        extension_type: 0,
        old_chain: 0,
    }
}

#[test]
fn swapchain_admits_no_extension_or_only_the_nv_latency_create_info() {
    const LATENCY_NV: u32 = 1_000_505_007;
    assert!(chain_facts().supported());
    let latency = Swapchain {
        extension_count: 1,
        extension_type: LATENCY_NV,
        ..chain_facts()
    };
    assert!(latency.supported());
    for (count, kind) in [(1, 1_000_001_000), (2, LATENCY_NV), (2, 5)] {
        let chained = Swapchain {
            extension_count: count,
            extension_type: kind,
            ..chain_facts()
        };
        assert!(!chained.supported(), "{count} {kind}");
    }
    let replacement = Swapchain {
        old_chain: 0x55,
        ..chain_facts()
    };
    assert!(!replacement.supported());
    let bgra = Swapchain {
        format: 44,
        ..chain_facts()
    };
    assert!(bgra.supported());
}

#[test]
fn only_fifo_present_modes_request_vsync() {
    // IMMEDIATE, MAILBOX, FIFO, FIFO_RELAXED, SHARED_DEMAND_REFRESH.
    for (mode, vsync) in [(0, 0), (1, 0), (2, 1), (3, 1), (1_000_111_000, 0)] {
        let facts = Swapchain {
            present_mode: mode,
            ..chain_facts()
        };
        assert_eq!(facts.vsync(), vsync, "{mode}");
        assert!(facts.supported());
    }
}

#[test]
fn present_waits_preserve_binary_order_and_append_only_a_prior_timeline_value() {
    let first = PresentSubmit::new(&[8, 7], 6, 0, 1);
    assert_eq!((first.wait_count, first.signal, first.ready), (2, 6, 1));
    assert_eq!(&first.waits[..3], &[8, 7, 0]);
    assert_eq!(&first.values[..3], &[0, 0, 0]);
    assert_eq!(&first.stages[..3], &[ALL, ALL, 0]);

    let binary: [u64; 16] = std::array::from_fn(|i| 20 + i as u64);
    let next = PresentSubmit::new(&binary, 6, 2, 3);
    assert_eq!((next.wait_count, next.signal, next.ready), (17, 6, 3));
    assert_eq!(&next.waits[..16], &binary);
    assert_eq!(next.waits[16], 6);
    assert_eq!(&next.values[..16], &[0; 16]);
    assert_eq!(next.values[16], 2);
    assert_eq!(next.stages, [ALL; 17]);
}
