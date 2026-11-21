// SPDX-License-Identifier: MIT
//! The creation owner against recording leaves. No Vulkan structure is built here; requests,
//! instances and devices are addresses. The Vulkan-typed clone, copy and create effects are covered
//! by the native `godot_vulkan_creation_contract` test.
use super::*;
use crate::platform::{VK_CREATION_VERSION, VK_SCOPE_INSTANCE, VkLatencyRequirements};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

const INSTANCE: usize = 0x11;
const PHYSICAL: usize = 0x12;
const DEVICE: usize = 0x13;
const ENGINE_REQUEST: usize = 0x100;
const RECORD: usize = 0x200;
const FEATURES: usize = 0x300;
const SDK_FEATURES: usize = 0x400;
const LATENCY_FEATURES: usize = 0x500;
const DERIVED_INSTANCE: usize = 0x600;
/// A derived device request is this plus its stage.
const DERIVED_DEVICE: usize = 0x700;

static INSTANCE_NAMES: [&CStr; 2] = [c"VK_EXT_fixture_instance", c"VK_EXT_fixture_instance"];
static DEVICE_NAMES: [&CStr; 3] = [
    c"VK_KHR_swapchain",
    c"VK_EXT_mutable_descriptor_type",
    c"VK_EXT_mutable_descriptor_type",
];
static LATENCY_NAMES: [&CStr; 1] = [c"VK_AMD_anti_lag"];

thread_local! {
    static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static XESS_INSTANCE: Cell<bool> = const { Cell::new(true) };
    static FACTS: Cell<VkChainFacts> = Cell::new(VkChainFacts::default());
    /// The XeSS clone: `None` throws, a null head refuses.
    static CLONE: Cell<Option<usize>> = const { Cell::new(Some(FEATURES)) };
    static XESS_DEVICE: Cell<bool> = const { Cell::new(true) };
    static INTEROP: Cell<bool> = const { Cell::new(false) };
    static LATENCY: Cell<bool> = const { Cell::new(false) };
    static RESULTS: RefCell<VecDeque<i32>> = const { RefCell::new(VecDeque::new()) };
}

fn record(call: String) {
    CALLS.with_borrow_mut(|calls| calls.push(call));
}

fn take_calls() -> Vec<String> {
    CALLS.with_borrow_mut(std::mem::take)
}

fn names(list: *const *const c_char, count: u32) -> String {
    // SAFETY: the owner passes its own lists with their counts.
    let list = unsafe { extension_list(list, count) };
    let names: Vec<_> = list
        .iter()
        .map(|&name| {
            unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.join(",")
}

fn reset() {
    take_calls();
    XESS_INSTANCE.set(true);
    FACTS.set(VkChainFacts::default());
    CLONE.set(Some(FEATURES));
    XESS_DEVICE.set(true);
    INTEROP.set(false);
    LATENCY.set(false);
    RESULTS.with_borrow_mut(VecDeque::clear);
}

/// The create results in order; success once they run out.
fn results(values: &[i32]) {
    RESULTS.with_borrow_mut(|queue| queue.extend(values));
}

fn next_result() -> i32 {
    RESULTS.with_borrow_mut(|queue| queue.pop_front().unwrap_or(VK_SUCCESS))
}

unsafe extern "C" fn create_marker() {}
unsafe extern "C" fn enumerate_marker() {}
unsafe extern "C" fn destroy_marker() {}
unsafe extern "C" fn other_marker() {}

unsafe extern "C" fn device_proc(device: *mut c_void, name: *const c_char) -> Option<VkFunction> {
    // SAFETY: the owner passes NUL-terminated names.
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    record(format!("device_proc {:#x} {name}", device.addr()));
    let function: VkFunction = if name == "vkDestroyDevice" {
        destroy_marker
    } else {
        other_marker
    };
    Some(function)
}

unsafe extern "C" fn underlying(object: *mut c_void, name: *const c_char) -> Option<VkFunction> {
    // SAFETY: the owner passes NUL-terminated names.
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    record(format!("underlying {:#x} {name}", object.addr()));
    let function: VkFunction = match name.as_str() {
        "vkCreateInstance" | "vkCreateDevice" => create_marker,
        "vkEnumerateDeviceExtensionProperties" => enumerate_marker,
        // SAFETY: the resolver answers vkGetDeviceProcAddr as a void function.
        "vkGetDeviceProcAddr" => unsafe {
            std::mem::transmute::<VkProc, VkFunction>(device_proc as VkProc)
        },
        _ => other_marker,
    };
    Some(function)
}

unsafe extern "C" fn xess_instance_requirements(
    count: *mut u32,
    list: *mut *const *const c_char,
    minimum: *mut u32,
) -> i32 {
    if !XESS_INSTANCE.get() {
        return -7;
    }
    thread_local! {
        static LIST: [*const c_char; 2] = INSTANCE_NAMES.map(CStr::as_ptr);
    }
    // SAFETY: the owner's outputs; the thread-local list lives for the test thread.
    unsafe {
        *count = 2;
        *list = LIST.with(|value| value.as_ptr());
        *minimum = 0x0040_3000;
    }
    0
}

unsafe extern "C" fn derive_instance(
    call: *mut c_void,
    base: *const c_void,
    minimum: u32,
    list: *const *const c_char,
    count: u32,
) -> *const c_void {
    record(format!(
        "derive_instance {:#x} {:#x} {minimum:#x} {}",
        call.addr(),
        base.addr(),
        names(list, count)
    ));
    ptr::without_provenance(DERIVED_INSTANCE)
}

unsafe extern "C" fn create_instance(
    create: VkFunction,
    request: *const c_void,
    _: *const c_void,
    output: *mut c_void,
) -> i32 {
    assert_eq!(create as usize, create_marker as usize);
    record(format!("create_instance {:#x}", request.addr()));
    let result = next_result();
    if result == VK_SUCCESS {
        // SAFETY: the test's output slot.
        unsafe {
            output
                .cast::<*mut c_void>()
                .write(ptr::without_provenance_mut(INSTANCE))
        };
    }
    result
}

unsafe extern "C" fn chain_facts(request: *const c_void, facts: *mut VkChainFacts) {
    record(format!("chain_facts {:#x}", request.addr()));
    // SAFETY: the owner's output.
    unsafe { facts.write(FACTS.get()) };
}

unsafe extern "C" fn xess_features(
    _: *mut c_void,
    request: *const c_void,
    structures: u32,
    features: *mut *mut c_void,
) -> u32 {
    record(format!("xess_features {:#x} {structures}", request.addr()));
    let Some(head) = CLONE.get() else {
        return 0;
    };
    // SAFETY: the owner's output.
    unsafe { features.write(ptr::without_provenance_mut(head)) };
    1
}

unsafe extern "C" fn xess_device_requirements(
    instance: *mut c_void,
    physical: *mut c_void,
    count: *mut u32,
    list: *mut *const *const c_char,
    features: *mut *mut c_void,
) -> i32 {
    thread_local! {
        static LIST: [*const c_char; 3] = DEVICE_NAMES.map(CStr::as_ptr);
    }
    // SAFETY: the owner's arguments.
    let head = unsafe { features.read() };
    record(format!(
        "xess_device_requirements {:#x} {:#x} {:#x}",
        instance.addr(),
        physical.addr(),
        head.addr()
    ));
    if !XESS_DEVICE.get() {
        return -7;
    }
    // SAFETY: the owner's outputs; the thread-local list lives for the test thread.
    unsafe {
        *count = 3;
        *list = LIST.with(|value| value.as_ptr());
        *features = ptr::without_provenance_mut(SDK_FEATURES);
    }
    0
}

unsafe extern "C" fn device_extensions(
    enumerate: VkFunction,
    physical: *mut c_void,
    list: *const *const c_char,
    count: u32,
) -> u32 {
    assert_eq!(enumerate as usize, enumerate_marker as usize);
    record(format!(
        "device_extensions {:#x} {}",
        physical.addr(),
        names(list, count)
    ));
    u32::from(INTEROP.get())
}

unsafe extern "C" fn latency_requirements(
    _: *mut c_void,
    instance: *mut c_void,
    _: *mut c_void,
    proc: Option<VkProc>,
    request: *const c_void,
    requirements: *mut VkLatencyRequirements,
) -> u32 {
    thread_local! {
        static LIST: [*const c_char; 1] = LATENCY_NAMES.map(CStr::as_ptr);
    }
    assert!(proc.is_some());
    record(format!(
        "latency_requirements {:#x} {:#x}",
        instance.addr(),
        request.addr()
    ));
    let value = if LATENCY.get() {
        VkLatencyRequirements {
            result: 0,
            extension_count: 1,
            extensions: LIST.with(|value| value.as_ptr()),
            features: ptr::without_provenance_mut(LATENCY_FEATURES),
        }
    } else {
        VkLatencyRequirements {
            result: -7,
            extension_count: 0,
            extensions: ptr::null(),
            features: ptr::null_mut(),
        }
    };
    // SAFETY: the owner's output.
    unsafe { requirements.write(value) };
    1
}

unsafe extern "C" fn derive_device(
    call: *mut c_void,
    stage: u32,
    derive: *const VkDeviceDerive,
) -> *const c_void {
    // SAFETY: the owner's record.
    let derive = unsafe { &*derive };
    assert_eq!(call.addr(), RECORD);
    record(format!(
        "derive_device {stage} {:#x} {} next={}:{:#x} clear={}",
        derive.base.addr(),
        names(derive.extensions, derive.extension_count),
        derive.replace_next,
        derive.next.addr(),
        derive.clear_enabled_features
    ));
    ptr::without_provenance(DERIVED_DEVICE + stage as usize)
}

unsafe extern "C" fn create_device(
    create: VkFunction,
    physical: *mut c_void,
    request: *const c_void,
    _: *const c_void,
    output: *mut c_void,
) -> i32 {
    assert_eq!(create as usize, create_marker as usize);
    assert_eq!(physical.addr(), PHYSICAL);
    record(format!("create_device {:#x}", request.addr()));
    let result = next_result();
    if result == VK_SUCCESS {
        // SAFETY: the test's output slot.
        unsafe {
            output
                .cast::<*mut c_void>()
                .write(ptr::without_provenance_mut(DEVICE))
        };
    }
    result
}

unsafe extern "C" fn latency_created(
    _: *mut c_void,
    _: *mut c_void,
    device: *mut c_void,
    _: Option<VkProc>,
    request: *const c_void,
) -> i32 {
    record(format!(
        "latency_created {:#x} {:#x}",
        device.addr(),
        request.addr()
    ));
    0
}

unsafe extern "C" fn latency_destroyed(device: *mut c_void) {
    record(format!("latency_destroyed {:#x}", device.addr()));
}

unsafe extern "C" fn destroy_device(destroy: VkFunction, device: *mut c_void, _: *const c_void) {
    assert_eq!(destroy as usize, destroy_marker as usize);
    record(format!("destroy_device {:#x}", device.addr()));
}

static LEAVES: VkCreationLeaves = VkCreationLeaves {
    struct_size: size_of::<VkCreationLeaves>() as u32,
    version: VK_CREATION_VERSION,
    xess_instance_requirements,
    derive_instance,
    create_instance,
    chain_facts,
    xess_features,
    xess_device_requirements,
    device_extensions,
    latency_requirements,
    derive_device,
    create_device,
    latency_created,
    latency_destroyed,
    destroy_device,
};

fn begun(xess: bool) -> Creation {
    reset();
    let creation = Creation::new();
    assert!(creation.begin(Some(underlying), xess, None, ptr::null_mut()));
    creation
}

/// Routes vkCreateDevice for the fixture instance so it becomes the owner.
fn owned(creation: &Creation) {
    // SAFETY: a NUL-terminated name.
    let route = unsafe {
        creation.resolve(
            VK_SCOPE_INSTANCE,
            ptr::without_provenance_mut(INSTANCE),
            c"vkCreateDevice".as_ptr(),
        )
    };
    assert_eq!(route.0, VK_ROUTE_CREATE_DEVICE);
    take_calls();
}

fn instance_call(output: *mut *mut c_void, list: &[*const c_char]) -> VkInstanceCall {
    VkInstanceCall {
        call: ptr::without_provenance_mut(RECORD),
        request: ptr::without_provenance(ENGINE_REQUEST),
        allocator: ptr::null(),
        output: output.cast(),
        extensions: list.as_ptr(),
        extension_count: list.len() as u32,
        reserved: 0,
    }
}

fn device_call(output: *mut *mut c_void, list: &[*const c_char]) -> VkDeviceCall {
    VkDeviceCall {
        call: ptr::without_provenance_mut(RECORD),
        physical_device: ptr::without_provenance_mut(PHYSICAL),
        request: ptr::without_provenance(ENGINE_REQUEST),
        allocator: ptr::null(),
        output: output.cast(),
        extensions: list.as_ptr(),
        extension_count: list.len() as u32,
        reserved: 0,
    }
}

fn engine_device_names() -> [*const c_char; 2] {
    [DEVICE_NAMES[0].as_ptr(), DEVICE_NAMES[1].as_ptr()]
}

#[test]
fn routes_intercept_creation_and_forward_everything_else() {
    reset();
    let creation = Creation::new();
    let instance = ptr::without_provenance_mut(INSTANCE);
    // SAFETY: NUL-terminated or null names for every resolve below.
    unsafe {
        let (route, function) =
            creation.resolve(VK_SCOPE_INSTANCE, instance, c"vkCreateDevice".as_ptr());
        assert_eq!((route, function.is_none()), (VK_ROUTE_NONE, true));
        assert!(!creation.begin(None, true, None, ptr::null_mut()));
        assert!(creation.begin(Some(underlying), true, None, ptr::null_mut()));
        assert_eq!(
            creation.resolve(VK_SCOPE_INSTANCE, instance, ptr::null()).0,
            VK_ROUTE_NONE
        );
        assert_eq!(
            creation
                .resolve(
                    VK_SCOPE_INSTANCE,
                    ptr::null_mut(),
                    c"vkCreateInstance".as_ptr()
                )
                .0,
            VK_ROUTE_CREATE_INSTANCE
        );
        assert_eq!(
            creation
                .resolve(VK_SCOPE_INSTANCE, instance, c"vkGetDeviceProcAddr".as_ptr())
                .0,
            VK_ROUTE_DEVICE_PROC
        );
        assert!(take_calls().is_empty());
        // A null instance keeps the owner.
        assert_eq!(
            creation
                .resolve(
                    VK_SCOPE_INSTANCE,
                    ptr::null_mut(),
                    c"vkCreateDevice".as_ptr()
                )
                .0,
            VK_ROUTE_CREATE_DEVICE
        );
        assert!(creation.owner().is_null());
        assert_eq!(
            creation
                .resolve(VK_SCOPE_INSTANCE, instance, c"vkCreateDevice".as_ptr())
                .0,
            VK_ROUTE_CREATE_DEVICE
        );
        assert_eq!(creation.owner().addr(), INSTANCE);
        let (route, function) =
            creation.resolve(VK_SCOPE_INSTANCE, instance, c"vkFixtureOther".as_ptr());
        assert_eq!(route, VK_ROUTE_FORWARD);
        assert_eq!(function.map(|f| f as usize), Some(other_marker as usize));
        assert_eq!(take_calls(), ["underlying 0x11 vkFixtureOther"]);

        let device = ptr::without_provenance_mut(DEVICE);
        assert_eq!(
            creation
                .resolve(VK_SCOPE_DEVICE, device, c"vkDestroyDevice".as_ptr())
                .0,
            VK_ROUTE_DESTROY_DEVICE
        );
        let (route, function) =
            creation.resolve(VK_SCOPE_DEVICE, device, c"vkQueueSubmit".as_ptr());
        assert_eq!(route, VK_ROUTE_FORWARD);
        assert_eq!(function.map(|f| f as usize), Some(other_marker as usize));
        assert_eq!(
            take_calls(),
            [
                "underlying 0x11 vkGetDeviceProcAddr",
                "device_proc 0x13 vkQueueSubmit"
            ]
        );
    }
}

#[test]
fn instance_requirements_merge_once_and_fall_back_to_the_engine_request() {
    let creation = begun(true);
    let engine = [INSTANCE_NAMES[0].as_ptr()];
    let mut output = ptr::null_mut();
    let call = instance_call(&raw mut output, &engine);
    assert_eq!(creation.create_instance(&LEAVES, &call), VK_SUCCESS);
    assert_eq!(
        take_calls(),
        [
            "underlying 0x0 vkCreateInstance",
            "derive_instance 0x200 0x100 0x403000 VK_EXT_fixture_instance",
            "create_instance 0x600",
        ]
    );
    assert_eq!(
        (output.addr(), creation.owner().addr()),
        (INSTANCE, INSTANCE)
    );

    // A driver refusal of the requirements retries the engine's request and ends preparation.
    for refusal in [VK_ERROR_EXTENSION_NOT_PRESENT, VK_ERROR_INCOMPATIBLE_DRIVER] {
        let creation = begun(true);
        results(&[refusal]);
        assert_eq!(creation.create_instance(&LEAVES, &call), VK_SUCCESS);
        let calls = take_calls();
        assert_eq!(
            &calls[2..],
            ["create_instance 0x600", "create_instance 0x100"]
        );
        assert!(!creation.prepare_xess.load(Ordering::Acquire));
    }
    // Another failure is returned unchanged.
    let creation = begun(true);
    results(&[-2]);
    assert_eq!(creation.create_instance(&LEAVES, &call), -2);
    assert_eq!(take_calls().len(), 3);
    assert!(creation.prepare_xess.load(Ordering::Acquire));
    // An SDK refusal creates the engine's request and ends preparation.
    let creation = begun(true);
    XESS_INSTANCE.set(false);
    assert_eq!(creation.create_instance(&LEAVES, &call), VK_SUCCESS);
    assert_eq!(
        take_calls(),
        ["underlying 0x0 vkCreateInstance", "create_instance 0x100"]
    );
    assert!(!creation.prepare_xess.load(Ordering::Acquire));
    // Without the request the call fails before any leaf.
    let missing = VkInstanceCall {
        request: ptr::null(),
        ..call
    };
    assert_eq!(
        creation.create_instance(&LEAVES, &missing),
        VK_ERROR_INITIALIZATION_FAILED
    );
}

#[test]
fn disabled_xess_creates_through_the_interop_and_latency_stages() {
    let creation = begun(false);
    owned(&creation);
    let engine = engine_device_names();
    let mut output = ptr::null_mut();
    assert_eq!(
        creation.create_device(&LEAVES, &device_call(&raw mut output, &engine)),
        VK_SUCCESS
    );
    assert_eq!(
        take_calls(),
        [
            "underlying 0x11 vkEnumerateDeviceExtensionProperties",
            "device_extensions 0x12 VK_KHR_external_memory_win32,VK_KHR_external_semaphore_win32",
            "underlying 0x11 vkCreateDevice",
            "latency_requirements 0x11 0x100",
            "create_device 0x100",
            "latency_created 0x13 0x100",
        ]
    );
    assert!(!creation.xess_ready() && !creation.interop_ready());
}

#[test]
fn xess_individual_features_are_added_and_every_stage_merges_its_extensions() {
    let creation = begun(true);
    owned(&creation);
    FACTS.set(VkChainFacts {
        cloneable: 1,
        enabled_features: 1,
        timeline_semaphore: 1,
        ..VkChainFacts::default()
    });
    INTEROP.set(true);
    let engine = engine_device_names();
    let mut output = ptr::null_mut();
    assert_eq!(
        creation.create_device(&LEAVES, &device_call(&raw mut output, &engine)),
        VK_SUCCESS
    );
    let structures = VK_XESS_ENABLED_FEATURES
        | VK_XESS_FLOAT16_INT8
        | VK_XESS_SCALAR_BLOCK_LAYOUT
        | VK_XESS_INTEGER_DOT_PRODUCT;
    assert_eq!(
        take_calls(),
        [
            "chain_facts 0x100".to_owned(),
            format!("xess_features 0x100 {structures}"),
            "xess_device_requirements 0x11 0x12 0x300".to_owned(),
            "derive_device 0 0x100 VK_KHR_swapchain,VK_EXT_mutable_descriptor_type next=1:0x400 clear=1"
                .to_owned(),
            "underlying 0x11 vkEnumerateDeviceExtensionProperties".to_owned(),
            "device_extensions 0x12 VK_KHR_external_memory_win32,VK_KHR_external_semaphore_win32"
                .to_owned(),
            "chain_facts 0x700".to_owned(),
            "derive_device 1 0x700 VK_KHR_swapchain,VK_EXT_mutable_descriptor_type,VK_KHR_external_memory_win32,VK_KHR_external_semaphore_win32 next=0:0x0 clear=0"
                .to_owned(),
            "underlying 0x11 vkCreateDevice".to_owned(),
            "latency_requirements 0x11 0x701".to_owned(),
            "create_device 0x701".to_owned(),
            "latency_created 0x13 0x701".to_owned(),
        ]
    );
    assert!(creation.xess_ready() && creation.interop_ready());
    creation.stop();
    assert!(!creation.xess_ready() && !creation.interop_ready());
    assert!(!creation.prepare_xess.load(Ordering::Acquire));
}

#[test]
fn aggregate_and_existing_root_chains_add_only_what_is_missing() {
    let cases = [
        (
            VkChainFacts {
                cloneable: 1,
                vulkan12: 1,
                vulkan13: 1,
                ..VkChainFacts::default()
            },
            Some(VK_XESS_FEATURES2),
        ),
        (
            VkChainFacts {
                cloneable: 1,
                features2: 1,
                float16_int8: 1,
                scalar_block_layout: 1,
                integer_dot_product: 1,
                ..VkChainFacts::default()
            },
            Some(0),
        ),
        (
            VkChainFacts {
                cloneable: 1,
                features2: 1,
                enabled_features: 1,
                ..VkChainFacts::default()
            },
            None,
        ),
        (VkChainFacts::default(), None),
    ];
    for (facts, structures) in cases {
        assert_eq!(xess_structures(&facts), structures, "{facts:?}");
    }
}

#[test]
fn xess_refusals_create_the_engine_request() {
    let engine = engine_device_names();
    let mut output = ptr::null_mut();
    let fallback = [
        "underlying 0x11 vkEnumerateDeviceExtensionProperties",
        "device_extensions 0x12 VK_KHR_external_memory_win32,VK_KHR_external_semaphore_win32",
        "underlying 0x11 vkCreateDevice",
        "latency_requirements 0x11 0x100",
        "create_device 0x100",
        "latency_created 0x13 0x100",
    ];
    // An uncloneable chain, a refused clone and a refused SDK query.
    for case in 0..3 {
        let creation = begun(true);
        owned(&creation);
        FACTS.set(VkChainFacts {
            cloneable: u32::from(case != 0),
            ..VkChainFacts::default()
        });
        CLONE.set(Some(if case == 1 { 0 } else { FEATURES }));
        XESS_DEVICE.set(case != 2);
        let call = device_call(&raw mut output, &engine);
        assert_eq!(creation.create_device(&LEAVES, &call), VK_SUCCESS);
        let calls = take_calls();
        assert!(
            calls.ends_with(&fallback.map(str::to_owned)),
            "{case}: {calls:?}"
        );
        assert!(!creation.xess_ready());
    }
    // A driver refusal of the patched request.
    for refusal in [VK_ERROR_FEATURE_NOT_PRESENT, VK_ERROR_EXTENSION_NOT_PRESENT] {
        let creation = begun(true);
        owned(&creation);
        FACTS.set(VkChainFacts {
            cloneable: 1,
            ..VkChainFacts::default()
        });
        results(&[refusal]);
        let call = device_call(&raw mut output, &engine);
        assert_eq!(creation.create_device(&LEAVES, &call), VK_SUCCESS);
        let creates: Vec<_> = take_calls()
            .into_iter()
            .filter(|call| call.starts_with("create_device"))
            .collect();
        assert_eq!(creates, ["create_device 0x700", "create_device 0x100"]);
        assert!(!creation.xess_ready());
    }
    // Another failure is returned without a retry.
    let creation = begun(true);
    owned(&creation);
    FACTS.set(VkChainFacts {
        cloneable: 1,
        ..VkChainFacts::default()
    });
    results(&[-2]);
    let call = device_call(&raw mut output, &engine);
    assert_eq!(creation.create_device(&LEAVES, &call), -2);
    assert!(!creation.xess_ready());
    // A clone that threw ends the call as the entry point's catch did.
    let creation = begun(true);
    owned(&creation);
    FACTS.set(VkChainFacts {
        cloneable: 1,
        ..VkChainFacts::default()
    });
    CLONE.set(None);
    assert_eq!(
        creation.create_device(&LEAVES, &call),
        VK_ERROR_OUT_OF_HOST_MEMORY
    );
}

#[test]
fn interop_needs_timeline_semaphores_and_retries_without_its_extensions() {
    let engine = engine_device_names();
    let mut output = ptr::null_mut();
    let creation = begun(false);
    owned(&creation);
    INTEROP.set(true);
    let call = device_call(&raw mut output, &engine);
    assert_eq!(creation.create_device(&LEAVES, &call), VK_SUCCESS);
    assert!(
        !take_calls()
            .iter()
            .any(|call| call.starts_with("derive_device"))
    );
    assert!(!creation.interop_ready());

    FACTS.set(VkChainFacts {
        timeline_semaphore: 1,
        ..VkChainFacts::default()
    });
    results(&[VK_ERROR_EXTENSION_NOT_PRESENT]);
    assert_eq!(creation.create_device(&LEAVES, &call), VK_SUCCESS);
    let creates: Vec<_> = take_calls()
        .into_iter()
        .filter(|call| call.starts_with("create_device"))
        .collect();
    assert_eq!(creates, ["create_device 0x701", "create_device 0x100"]);
    assert!(!creation.interop_ready());
    assert_eq!(creation.create_device(&LEAVES, &call), VK_SUCCESS);
    assert!(creation.interop_ready());
}

#[test]
fn latency_requirements_are_optional_and_the_actual_request_is_reported() {
    let engine = engine_device_names();
    let mut output = ptr::null_mut();
    let creation = begun(false);
    owned(&creation);
    LATENCY.set(true);
    results(&[VK_ERROR_FEATURE_NOT_PRESENT]);
    let call = device_call(&raw mut output, &engine);
    assert_eq!(creation.create_device(&LEAVES, &call), VK_SUCCESS);
    let calls = take_calls();
    assert_eq!(
        &calls[3..],
        [
            "latency_requirements 0x11 0x100",
            "derive_device 2 0x100 VK_KHR_swapchain,VK_EXT_mutable_descriptor_type,VK_AMD_anti_lag next=1:0x500 clear=0",
            "create_device 0x702",
            "create_device 0x100",
            "latency_created 0x13 0x100",
        ]
    );
    assert_eq!(creation.create_device(&LEAVES, &call), VK_SUCCESS);
    assert_eq!(
        take_calls().last().map(String::as_str),
        Some("latency_created 0x13 0x702")
    );
}

#[test]
fn destroy_stops_latency_before_the_device_is_destroyed() {
    let creation = begun(false);
    owned(&creation);
    creation.destroy_device(&LEAVES, ptr::without_provenance_mut(DEVICE), ptr::null());
    assert_eq!(
        take_calls(),
        [
            "underlying 0x11 vkGetDeviceProcAddr",
            "device_proc 0x13 vkDestroyDevice",
            "latency_destroyed 0x13",
            "destroy_device 0x13",
        ]
    );
}
