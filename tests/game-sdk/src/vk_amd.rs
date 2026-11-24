// SPDX-License-Identifier: MIT
use super::*;
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::{ABI_MISMATCH, INVALID_ARGUMENT, NOT_READY, NOT_SUPPORTED, OK};
use std::ffi::CStr;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

const ABI: u32 = RSF_LATENCYVK_DEVICE_ABI_VERSION;
const INPUT: i32 = 0;
const PRESENT: i32 = 1;
const MODE_ONLY: i32 = -1;
const ON: i32 = 1;
const OFF: i32 = 2;
const AMD: u32 = 0x1002;
const EXTENSION: &CStr = c"VK_AMD_anti_lag";
const FEATURES_TYPE: i32 = 1_000_476_000;
const DATA_TYPE: i32 = 1_000_476_001;
const PRESENT_TYPE: i32 = 1_000_476_002;

// Vulkan-Headers 1.4.357 VK_AMD_anti_lag declarations. These types are newer
// than the workspace's ash registry. The mock independently reads the ABI.
#[repr(C)]
struct Feature {
    kind: i32,
    next: *mut c_void,
    enabled: u32,
}
#[repr(C)]
struct Presentation {
    kind: i32,
    next: *mut c_void,
    stage: i32,
    index: u64,
}
#[repr(C)]
struct Data {
    kind: i32,
    next: *const c_void,
    mode: i32,
    max_fps: u32,
    present: *const Presentation,
}
const _: () = {
    assert!(size_of::<Feature>() == 24 && std::mem::offset_of!(Feature, enabled) == 16);
    assert!(size_of::<Presentation>() == 32 && std::mem::offset_of!(Presentation, index) == 24);
    assert!(size_of::<Data>() == 32 && std::mem::offset_of!(Data, present) == 24);
};

#[derive(Clone, Copy)]
struct Mark {
    frame: u64,
    stage: i32,
    mode: i32,
    fps: u32,
}

static VENDOR: AtomicU32 = AtomicU32::new(AMD);
static EXT_SUPPORTED: AtomicBool = AtomicBool::new(true);
static FEATURE_SUPPORTED: AtomicBool = AtomicBool::new(true);
static UPDATE_AVAILABLE: AtomicBool = AtomicBool::new(true);
static BLOCKING_FRAME: AtomicU64 = AtomicU64::new(0);
static STAMP: AtomicU64 = AtomicU64::new(100);
static MARKS: Mutex<Vec<Mark>> = Mutex::new(Vec::new());
static SLEEP: Gate = Gate::new();

unsafe extern "system" fn properties(
    _: vk::PhysicalDevice,
    value: *mut vk::PhysicalDeviceProperties,
) {
    unsafe { (*value).vendor_id = VENDOR.load(Relaxed) };
}
unsafe extern "system" fn extensions(
    _: vk::PhysicalDevice,
    _: *const c_char,
    count: *mut u32,
    values: *mut vk::ExtensionProperties,
) -> vk::Result {
    let wanted = u32::from(EXT_SUPPORTED.load(Relaxed));
    if !values.is_null() && wanted != 0 && unsafe { *count } != 0 {
        let raw_name = unsafe { &mut (*values).extension_name };
        for (dst, src) in raw_name.iter_mut().zip(EXTENSION.to_bytes_with_nul()) {
            *dst = *src as c_char;
        }
    }
    unsafe { count.write(wanted) };
    vk::Result::SUCCESS
}
unsafe extern "system" fn features(
    _: vk::PhysicalDevice,
    root: *mut vk::PhysicalDeviceFeatures2<'_>,
) {
    let feature = unsafe { &mut *(*root).p_next.cast::<Feature>() };
    verify(feature.kind == FEATURES_TYPE);
    feature.enabled = u32::from(FEATURE_SUPPORTED.load(Relaxed));
}
unsafe extern "system" fn update(device: vk::Device, data: *const Data) {
    if !verify(device.as_raw() == 3 && !data.is_null()) {
        return;
    }
    let data = unsafe { &*data };
    verify(data.kind == DATA_TYPE);
    let (frame, stage) = unsafe { data.present.as_ref() }.map_or((0, MODE_ONLY), |info| {
        verify(info.kind == PRESENT_TYPE);
        (info.index, info.stage)
    });
    MARKS.lock().unwrap().push(Mark {
        frame,
        stage,
        mode: data.mode,
        fps: data.max_fps,
    });
    if stage == INPUT && frame == BLOCKING_FRAME.load(Relaxed) {
        verify(SLEEP.pause());
    }
}
unsafe extern "system" fn device_proc(
    _: vk::Device,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    if UPDATE_AVAILABLE.load(Relaxed) && unsafe { CStr::from_ptr(name) } == c"vkAntiLagUpdateAMD" {
        Some(unsafe {
            std::mem::transmute::<
                unsafe extern "system" fn(vk::Device, *const Data),
                unsafe extern "system" fn(),
            >(update)
        })
    } else {
        None
    }
}
unsafe extern "C" fn instance_proc(_: *mut c_void, name: *const c_char) -> rsf_vk_function {
    // The public resolver transports opaque function addresses. The Runtime
    // casts them back to the matching Vulkan system ABI before calling them.
    macro_rules! address {
        ($function:ident) => {
            rsf_test_support::vk_entry($function as *const ())
        };
    }
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkGetPhysicalDeviceProperties" => address!(properties),
        b"vkGetPhysicalDeviceFeatures2" => address!(features),
        b"vkEnumerateDeviceExtensionProperties" => address!(extensions),
        b"vkGetDeviceProcAddr" => address!(device_proc),
        _ => None,
    }
}

fn fact(source: u64, stage: u32) -> Event {
    event(
        77,
        source,
        stage,
        STAMP.fetch_add(1, Relaxed) + 1,
        1_000_000,
        RSF_GAME_CPU_EVENT_VERSION,
        RSF_GAME_CPU_INPUT_KEYED,
    )
}
fn cpu(source: u64, stage: u32) {
    unsafe { rsf_latencyvk_cpu(&fact(source, stage)) };
}
fn stages(source: u64, stages: &[u32]) {
    for &stage in stages {
        cpu(source, stage);
    }
}
fn count(stage: i32, source: u64) -> usize {
    MARKS
        .lock()
        .unwrap()
        .iter()
        .filter(|mark| mark.stage == stage && mark.frame == source)
        .count()
}
fn last() -> Mark {
    *MARKS.lock().unwrap().last().unwrap()
}
fn present(source: u64) {
    unsafe {
        rsf_latencyvk_render(source);
        rsf_latencyvk_present(1);
        rsf_latencyvk_present(0);
    }
}
fn arm(frame: u64) {
    BLOCKING_FRAME.store(frame, Relaxed);
    SLEEP.arm();
}

pub(super) fn run() -> i32 {
    let mut query = rsf_latencyvk_device_query {
        struct_size: size_of::<rsf_latencyvk_device_query>() as u32,
        abi_version: ABI,
        instance: identity(1),
        physical_device: identity(2),
        instance_proc: Some(instance_proc),
        features: null_mut(),
    };
    let mut required = rsf_latencyvk_device_requirements {
        struct_size: size_of::<rsf_latencyvk_device_requirements>() as u32,
        abi_version: ABI,
        ..Default::default()
    };
    // The fixture owns every packet and Vulkan node for the synchronous calls.
    unsafe {
        assert_eq!(
            rsf_latencyvk_require_device(null(), &mut required),
            INVALID_ARGUMENT
        );
        let short = rsf_latencyvk_device_query {
            struct_size: query.struct_size - 1,
            ..query
        };
        assert_eq!(
            rsf_latencyvk_require_device(&short, &mut required),
            INVALID_ARGUMENT
        );
        let wrong = rsf_latencyvk_device_query {
            abi_version: 99,
            ..query
        };
        assert_eq!(
            rsf_latencyvk_require_device(&wrong, &mut required),
            ABI_MISMATCH
        );
        for (vendor, extension, features) in [
            (0x10de, true, true),
            (0x8086, true, true),
            (AMD, false, true),
            (AMD, true, false),
        ] {
            VENDOR.store(vendor, Relaxed);
            EXT_SUPPORTED.store(extension, Relaxed);
            FEATURE_SUPPORTED.store(features, Relaxed);
            assert_eq!(rsf_latencyvk_require_device(&query, &mut required), OK);
            assert_eq!(required.extension_count, 0);
        }
        FEATURE_SUPPORTED.store(true, Relaxed);
        assert_eq!(rsf_latencyvk_require_device(&query, &mut required), OK);
        assert_eq!(required.extension_count, 1);
        assert_eq!(CStr::from_ptr(*required.extensions), EXTENSION);
        let feature = required.features.cast::<Feature>();
        assert!(!feature.is_null() && (*feature).enabled != 0 && (*feature).next.is_null());
        let mut tail = vk::PhysicalDeviceVulkan11Features::default();
        let tail_ptr = (&raw mut tail).cast();
        let mut copied = vk::PhysicalDeviceFeatures2 {
            p_next: tail_ptr,
            ..Default::default()
        };
        query.features = (&raw mut copied).cast();
        assert_eq!(rsf_latencyvk_require_device(&query, &mut required), OK);
        assert_eq!((copied.p_next, (*feature).next), (feature.cast(), tail_ptr));
        assert_eq!(rsf_latencyvk_require_device(&query, &mut required), OK);
        assert_eq!((*feature).next, tail_ptr);
        let mut cyclic = vk::PhysicalDeviceFeatures2::default();
        let cyclic_ptr = (&raw mut cyclic).cast();
        cyclic.p_next = cyclic_ptr;
        query.features = cyclic_ptr;
        assert_eq!(
            rsf_latencyvk_require_device(&query, &mut required),
            INVALID_ARGUMENT
        );
        assert_eq!(
            (required.extension_count, required.features, cyclic.p_next),
            (0, cyclic_ptr, cyclic_ptr)
        );
        query.features = (&raw mut copied).cast();
        assert_eq!(rsf_latencyvk_require_device(&query, &mut required), OK);
        let mut info = vk::DeviceCreateInfo {
            p_next: feature.cast(),
            enabled_extension_count: required.extension_count,
            pp_enabled_extension_names: required.extensions,
            ..Default::default()
        };
        let created = rsf_latencyvk_created_device {
            struct_size: size_of::<rsf_latencyvk_created_device>() as u32,
            abi_version: ABI,
            instance: query.instance,
            physical_device: query.physical_device,
            device: identity(3),
            instance_proc: Some(instance_proc),
            create_info: (&raw const info).cast(),
        };
        let short = rsf_latencyvk_created_device {
            struct_size: 4,
            ..created
        };
        assert_eq!(rsf_latencyvk_device_created(&short), INVALID_ARGUMENT);
        VENDOR.store(0x8086, Relaxed);
        assert_eq!(rsf_latencyvk_device_created(&created), NOT_SUPPORTED);
        VENDOR.store(AMD, Relaxed);
        info.enabled_extension_count = 0;
        assert_eq!(rsf_latencyvk_device_created(&created), NOT_SUPPORTED);
        info.enabled_extension_count = 1;
        (*feature).enabled = 0;
        assert_eq!(rsf_latencyvk_device_created(&created), NOT_SUPPORTED);
        (*feature).enabled = 1;
        UPDATE_AVAILABLE.store(false, Relaxed);
        assert_eq!(rsf_latencyvk_device_created(&created), NOT_SUPPORTED);
        UPDATE_AVAILABLE.store(true, Relaxed);
        assert_eq!(rsf_latencyvk_device_created(&created), OK);
        std::hint::black_box(&info);
        pacing_and_retirement(&created);
    }
    assert_eq!(CALLBACK_FAILURES.load(Relaxed), 0);
    0
}

fn pacing_and_retirement(created: &rsf_latencyvk_created_device) {
    // Retire all borrowed calls before the fixture-owned create-info expires.
    unsafe {
        assert_ne!(rsf_native_latency_claim(1), 0);
        assert_eq!(rsf_latencyvk_options(1, 10000), NOT_SUPPORTED);
        stages(1, &[0, 1, 2, 3, 4]);
        assert!(MARKS.lock().unwrap().is_empty());
        rsf_native_latency_release_claim(1);
        assert_eq!(rsf_latencyvk_options(2, 10000), OK);
        let first = 0x1_0000_0032;
        let next = first + 1;
        stages(first, &[0, 1, 1, 2, 3, 4]);
        assert_eq!(count(INPUT, first), 1);
        assert_eq!((last().mode, last().fps), (ON, 100));

        cpu(next, 0);
        arm(next);
        std::thread::scope(|scope| {
            let sleeper = scope.spawn(|| cpu(next, 1));
            assert!(SLEEP.entered());
            rsf_latencyvk_render(first);
            rsf_latencyvk_present(1);
            rsf_latencyvk_present(1);
            rsf_latencyvk_present(0);
            assert_eq!((count(PRESENT, first), count(PRESENT, next)), (1, 0));
            SLEEP.release();
            sleeper.join().unwrap();
        });
        stages(next, &[2, 3]);
        let mut skipped = fact(next, 4);
        skipped.flags |= RSF_GAME_CPU_RENDER_SKIPPED;
        skipped.render_expected = 0;
        rsf_latencyvk_cpu(&skipped);
        present(next);
        assert_eq!(count(PRESENT, next), 0);
        assert_eq!(rsf_latencyvk_options(0, 10000), OK);
        stages(next + 1, &[0, 1, 2, 3, 4]);
        present(next + 1);
        assert_eq!((count(INPUT, next + 1), count(PRESENT, next + 1)), (0, 0));
        let disabled = last();
        assert_eq!(
            (disabled.mode, disabled.fps, disabled.stage),
            (OFF, 0, MODE_ONLY)
        );
        let mut status = status_request();
        assert_eq!(rsf_latencyvk_get_status(&mut status), OK);
        assert_eq!(
            (status.owner, status.mode, status.sleep_calls),
            (RSF_PACING_ANTILAG_VULKAN, 0, 2)
        );

        assert_eq!(rsf_latencyvk_options(1, 10000), OK);
        let (older, retiring) = (next + 2, next + 3);
        stages(older, &[0, 1, 2, 3]);
        cpu(retiring, 0);
        arm(retiring);
        let destroyed = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let sleeper = scope.spawn(|| cpu(retiring, 1));
            assert!(SLEEP.entered());
            assert_eq!(rsf_latencyvk_shutdown(), NOT_READY);
            let destroy = scope.spawn(|| {
                rsf_latencyvk_device_destroyed(identity(3));
                destroyed.store(true, Relaxed);
            });
            let deadline = Instant::now() + Duration::from_secs(2);
            let rebind = loop {
                let result = rsf_latencyvk_device_created(created);
                if result != OK || Instant::now() >= deadline {
                    break result;
                }
                std::thread::yield_now();
            };
            assert_eq!(rebind, NOT_READY);
            assert!(!destroyed.load(Relaxed));
            present(older);
            assert_eq!(count(PRESENT, older), 1);
            assert!(!destroyed.load(Relaxed));
            SLEEP.release();
            sleeper.join().unwrap();
            destroy.join().unwrap();
        });
        assert!(destroyed.load(Relaxed) && rsf_native_latency_has_claim(2) == 0);
        assert_eq!(rsf_latencyvk_options(1, 0), NOT_SUPPORTED);
        assert_eq!(rsf_latencyvk_shutdown(), OK);
        assert_eq!(rsf_latencyvk_device_created(created), OK);
        assert_eq!(rsf_latencyvk_options(1, 10000), OK);
        assert_ne!(rsf_native_latency_has_claim(2), 0);
        assert_eq!(rsf_latencyvk_presentation(identity(4)), OK);
        assert_eq!(rsf_latencyvk_get_status(&mut status), OK);
        assert_eq!(status.owner, RSF_PACING_XELL);
        rsf_latencyvk_device_destroyed(identity(3));
        assert!(rsf_native_latency_has_claim(2) != 0 && rsf_native_latency_claim(1) == 0);
        assert_eq!(rsf_latencyvk_get_status(&mut status), OK);
        assert_eq!(status.owner, RSF_PACING_XELL);
        assert_eq!(rsf_latencyvk_shutdown(), OK);
        assert_eq!(rsf_native_latency_has_claim(2), 0);
        rsf_native_cpu_release();
    }
}
