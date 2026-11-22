// SPDX-License-Identifier: MIT
//! Fake Vulkan loader and real Win32 handles. No Vulkan device is required.
use super::*;
use crate::vulkan_sr_abi::rsf_vk_function;
use ash::vk::{self, Handle as _};
use std::cell::RefCell;
use std::ffi::{CStr, c_char};
use windows_sys::Win32::Foundation::GetHandleInformation;
use windows_sys::Win32::System::Threading::CreateEventW;

#[derive(Default)]
struct State {
    calls: Vec<&'static CStr>,
    missing: Option<&'static CStr>,
    last_handle: Handle,
    images: u32,
    memory: u32,
    semaphores: u32,
    com: u32,
    callback_error: i32,
    missing_handle: bool,
    missing_com: bool,
    wrong_memory: bool,
    bind_error: bool,
    semaphore_error: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}
static EVENTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn event() -> Handle {
    let handle = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
    assert!(!handle.is_null());
    STATE.with_borrow_mut(|state| state.last_handle = handle);
    handle
}

fn handle_closed() {
    let handle = STATE.with_borrow(|state| state.last_handle);
    assert!(!handle.is_null());
    let mut flags = 0;
    assert_eq!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
}

fn live() -> [u32; 4] {
    STATE.with_borrow(|state| [state.images, state.memory, state.com, state.semaphores])
}

unsafe extern "system" fn properties(
    _: vk::PhysicalDevice,
    out: *mut vk::PhysicalDeviceProperties2<'_>,
) {
    let identity = unsafe { &mut *(*out).p_next.cast::<vk::PhysicalDeviceIDProperties<'_>>() };
    identity.device_luid_valid = vk::TRUE;
    identity.device_luid = 7u64.to_ne_bytes();
}

unsafe extern "system" fn image_properties(
    _: vk::PhysicalDevice,
    query: *const vk::PhysicalDeviceImageFormatInfo2<'_>,
    out: *mut vk::ImageFormatProperties2<'_>,
) -> vk::Result {
    let query = unsafe { &*query };
    assert_eq!(query.usage.as_raw(), 3);
    unsafe {
        (*out).image_format_properties.max_extent = vk::Extent3D {
            width: 4096,
            height: 4096,
            depth: 1,
        };
        (*(*out)
            .p_next
            .cast::<vk::ExternalImageFormatProperties<'_>>())
        .external_memory_properties
        .external_memory_features = vk::ExternalMemoryFeatureFlags::IMPORTABLE;
    }
    vk::Result::SUCCESS
}

unsafe extern "system" fn memory_properties(
    _: vk::PhysicalDevice,
    out: *mut vk::PhysicalDeviceMemoryProperties,
) {
    let out = unsafe { &mut *out };
    out.memory_type_count = 2;
    let index = STATE.with_borrow(|state| usize::from(state.wrong_memory));
    out.memory_types[index].property_flags = vk::MemoryPropertyFlags::DEVICE_LOCAL;
}

unsafe extern "system" fn semaphore_properties(
    _: vk::PhysicalDevice,
    query: *const vk::PhysicalDeviceExternalSemaphoreInfo<'_>,
    out: *mut vk::ExternalSemaphoreProperties<'_>,
) {
    assert_eq!(unsafe { (*query).handle_type.as_raw() }, 8);
    let kind = unsafe { &*(*query).p_next.cast::<vk::SemaphoreTypeCreateInfo<'_>>() };
    assert_eq!(kind.semaphore_type.as_raw(), 1);
    unsafe { (*out).external_semaphore_features = vk::ExternalSemaphoreFeatureFlags::IMPORTABLE };
}

unsafe extern "system" fn create_image(
    _: vk::Device,
    info: *const vk::ImageCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Image,
) -> vk::Result {
    let info = unsafe { &*info };
    assert!(!info.p_next.is_null());
    assert_eq!(
        [info.mip_levels, info.array_layers, info.extent.depth],
        [1; 3]
    );
    STATE.with_borrow_mut(|state| state.images += 1);
    // Distinct handles allow pair release to exercise identity admission.
    let image = STATE.with_borrow(|state| 10 + state.images as u64);
    unsafe { out.write(vk::Image::from_raw(image)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn destroy_image(
    _: vk::Device,
    _: vk::Image,
    _: *const vk::AllocationCallbacks<'_>,
) {
    STATE.with_borrow_mut(|state| state.images -= 1);
}

unsafe extern "system" fn requirements(
    _: vk::Device,
    _: vk::Image,
    out: *mut vk::MemoryRequirements,
) {
    unsafe {
        out.write(vk::MemoryRequirements {
            size: 4096,
            alignment: 256,
            memory_type_bits: 1,
        })
    };
}

unsafe extern "system" fn handle_properties(
    _: vk::Device,
    ty: vk::ExternalMemoryHandleTypeFlags,
    handle: vk::HANDLE,
    out: *mut vk::MemoryWin32HandlePropertiesKHR<'_>,
) -> vk::Result {
    assert_eq!(ty.as_raw(), 0x40);
    assert_eq!(
        handle,
        STATE.with_borrow(|state| state.last_handle as isize)
    );
    unsafe { (*out).memory_type_bits = 1 };
    vk::Result::SUCCESS
}

unsafe extern "system" fn allocate(
    _: vk::Device,
    info: *const vk::MemoryAllocateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::DeviceMemory,
) -> vk::Result {
    let info = unsafe { &*info };
    let import = unsafe { &*info.p_next.cast::<vk::ImportMemoryWin32HandleInfoKHR<'_>>() };
    assert_eq!(
        import.handle,
        STATE.with_borrow(|state| state.last_handle as isize)
    );
    assert!(!import.p_next.is_null());
    assert_eq!(info.allocation_size, 4096);
    assert_eq!(info.memory_type_index, 0);
    STATE.with_borrow_mut(|state| state.memory += 1);
    unsafe { out.write(vk::DeviceMemory::from_raw(20)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn free_memory(
    _: vk::Device,
    _: vk::DeviceMemory,
    _: *const vk::AllocationCallbacks<'_>,
) {
    STATE.with_borrow_mut(|state| state.memory -= 1);
}

unsafe extern "system" fn bind(
    _: vk::Device,
    _: vk::Image,
    _: vk::DeviceMemory,
    _: u64,
) -> vk::Result {
    STATE.with_borrow(|state| {
        if state.bind_error {
            vk::Result::ERROR_UNKNOWN
        } else {
            vk::Result::SUCCESS
        }
    })
}

unsafe extern "system" fn create_semaphore(
    _: vk::Device,
    info: *const vk::SemaphoreCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Semaphore,
) -> vk::Result {
    let ty = unsafe { &*(*info).p_next.cast::<vk::SemaphoreTypeCreateInfo<'_>>() };
    assert_eq!(ty.semaphore_type.as_raw(), 1);
    STATE.with_borrow_mut(|state| state.semaphores += 1);
    unsafe { out.write(vk::Semaphore::from_raw(30)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn destroy_semaphore(
    _: vk::Device,
    _: vk::Semaphore,
    _: *const vk::AllocationCallbacks<'_>,
) {
    STATE.with_borrow_mut(|state| state.semaphores -= 1);
}

unsafe extern "system" fn import_semaphore(
    _: vk::Device,
    info: *const vk::ImportSemaphoreWin32HandleInfoKHR<'_>,
) -> vk::Result {
    let info = unsafe { &*info };
    STATE.with_borrow(|state| {
        assert_eq!(info.handle, state.last_handle as isize);
        assert_eq!(info.flags.as_raw(), 0);
        if state.semaphore_error {
            vk::Result::ERROR_INVALID_EXTERNAL_HANDLE
        } else {
            vk::Result::SUCCESS
        }
    })
}

unsafe extern "C" fn resolver(_: Handle, name: *const c_char) -> rsf_vk_function {
    let name = unsafe { CStr::from_ptr(name) };
    macro_rules! entries {
        ($($name:literal => $func:ident : $ty:ident),+ $(,)?) => {
            $(if name == $name {
                return STATE.with_borrow_mut(|state| {
                    state.calls.push($name);
                    if state.missing == Some($name) {
                        None
                    } else {
                        Some(unsafe { std::mem::transmute::<vk::$ty, unsafe extern "C" fn()>($func) })
                    }
                });
            })+
        };
    }
    entries! {
        c"vkGetPhysicalDeviceProperties2" => properties: PFN_vkGetPhysicalDeviceProperties2,
        c"vkGetPhysicalDeviceImageFormatProperties2" => image_properties: PFN_vkGetPhysicalDeviceImageFormatProperties2,
        c"vkGetPhysicalDeviceMemoryProperties" => memory_properties: PFN_vkGetPhysicalDeviceMemoryProperties,
        c"vkGetPhysicalDeviceExternalSemaphoreProperties" => semaphore_properties: PFN_vkGetPhysicalDeviceExternalSemaphoreProperties,
        c"vkCreateImage" => create_image: PFN_vkCreateImage,
        c"vkDestroyImage" => destroy_image: PFN_vkDestroyImage,
        c"vkGetImageMemoryRequirements" => requirements: PFN_vkGetImageMemoryRequirements,
        c"vkGetMemoryWin32HandlePropertiesKHR" => handle_properties: PFN_vkGetMemoryWin32HandlePropertiesKHR,
        c"vkAllocateMemory" => allocate: PFN_vkAllocateMemory,
        c"vkFreeMemory" => free_memory: PFN_vkFreeMemory,
        c"vkBindImageMemory" => bind: PFN_vkBindImageMemory,
        c"vkCreateSemaphore" => create_semaphore: PFN_vkCreateSemaphore,
        c"vkDestroySemaphore" => destroy_semaphore: PFN_vkDestroySemaphore,
        c"vkImportSemaphoreWin32HandleKHR" => import_semaphore: PFN_vkImportSemaphoreWin32HandleKHR,
    }
    None
}

unsafe extern "C" fn image_factory(
    _: Handle,
    width: u32,
    height: u32,
    format: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    assert_eq!([width, height, format], [32, 24, R32_FLOAT]);
    unsafe { handle.write(event()) };
    STATE.with_borrow_mut(|state| {
        if !state.missing_com {
            state.com += 1;
            unsafe { resource.write(ptr::without_provenance_mut(state.com as usize)) };
        }
        state.callback_error
    })
}

unsafe extern "C" fn release(_: Handle) {
    STATE.with_borrow_mut(|state| state.com -= 1);
}

unsafe extern "C" fn fence_factory(_: Handle, out: *mut Handle) -> i32 {
    if !STATE.with_borrow(|state| state.missing_handle) {
        unsafe { out.write(event()) };
    }
    STATE.with_borrow(|state| state.callback_error)
}

fn setup() -> rsf_srvk_setup {
    let mut graphics: rsf_srvk_setup = unsafe { std::mem::zeroed() };
    graphics.struct_size = size_of::<rsf_srvk_setup>() as u32;
    graphics.abi_version = RSF_SRVK_ABI_VERSION;
    graphics.device = ptr::without_provenance_mut(1);
    graphics.physical_device = ptr::without_provenance_mut(2);
    graphics.instance = ptr::without_provenance_mut(3);
    graphics.device_proc = Some(resolver);
    graphics.instance_proc = Some(resolver);
    graphics
}

fn callbacks() -> Callbacks {
    Callbacks {
        struct_size: size_of::<Callbacks>() as u32,
        abi_version: RSF_DX12VK_INTEROP_ABI_VERSION,
        adapter_luid: 7,
        user: ptr::null_mut(),
        create_image: Some(image_factory),
        release_image: Some(release),
        create_fence: Some(fence_factory),
    }
}

#[test]
fn admits_version_adapter_timeline_and_every_dispatch_entry_before_importing() {
    let graphics = setup();
    let mut callbacks = callbacks();
    let create = |callbacks: &Callbacks, timeline, expected| {
        let mut owner = ptr::without_provenance_mut(1);
        assert_eq!(
            unsafe { rsf_dx12vk_interop_create(&graphics, callbacks, timeline, &mut owner) },
            expected
        );
        assert!(owner.is_null());
        assert_eq!(live(), [0; 4]);
    };
    callbacks.struct_size -= 1;
    create(&callbacks, 1, INVALID);
    callbacks.struct_size += 1;
    callbacks.abi_version += 1;
    create(&callbacks, 1, ABI_MISMATCH);
    callbacks.abi_version -= 1;
    callbacks.adapter_luid = 8;
    create(&callbacks, 1, INVALID_EXTERNAL_HANDLE);
    callbacks.adapter_luid = 7;
    create(&callbacks, 0, INVALID);
    STATE.with_borrow_mut(|state| {
        state.calls.clear();
        state.missing = Some(c"vkGetPhysicalDeviceImageFormatProperties2");
    });
    create(
        &callbacks,
        1,
        vk::Result::ERROR_EXTENSION_NOT_PRESENT.as_raw(),
    );
    STATE.with_borrow(|state| {
        assert_eq!(state.calls.len(), 14);
        assert_eq!(
            state.calls.last(),
            Some(&c"vkImportSemaphoreWin32HandleKHR")
        );
    });
}

#[test]
fn destroys_a_partially_imported_timeline_and_closes_the_factory_handle() {
    let _events = EVENTS.lock().unwrap();
    let graphics = setup();
    let callbacks = callbacks();
    for (error, missing_handle, semaphore_error) in
        [(-77, false, false), (0, true, false), (0, false, true)]
    {
        STATE.with_borrow_mut(|state| {
            state.callback_error = error;
            state.missing_handle = missing_handle;
            state.semaphore_error = semaphore_error;
        });
        let mut owner = ptr::null_mut();
        let result = unsafe { rsf_dx12vk_interop_create(&graphics, &callbacks, 1, &mut owner) };
        assert_eq!(
            result,
            if error == 0 {
                INVALID_EXTERNAL_HANDLE
            } else {
                error
            }
        );
        assert!(owner.is_null());
        assert_eq!(live(), [0; 4]);
        if !missing_handle {
            handle_closed();
        }
    }
}

#[test]
fn releases_partial_imports_and_allows_replacement_only_for_known_guides() {
    let _events = EVENTS.lock().unwrap();
    let graphics = setup();
    let callbacks = callbacks();
    let mut owner = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_dx12vk_interop_create(&graphics, &callbacks, 1, &mut owner) },
        0
    );
    assert_eq!(unsafe { rsf_dx12vk_interop_timeline(owner) }, 30);
    handle_closed();

    let import = || {
        let mut output: Output = unsafe { std::mem::zeroed() };
        let result = unsafe { rsf_dx12vk_interop_image(owner, R32_FLOAT, 32, 24, &mut output) };
        handle_closed();
        (result, output)
    };
    for (callback, missing_com, wrong_memory, bind_error, expected) in [
        (-78, false, false, false, -78),
        (0, true, false, false, INVALID_EXTERNAL_HANDLE),
        (0, false, true, false, INCOMPATIBLE_DRIVER),
        (0, false, false, true, vk::Result::ERROR_UNKNOWN.as_raw()),
    ] {
        STATE.with_borrow_mut(|state| {
            state.callback_error = callback;
            state.missing_com = missing_com;
            state.wrong_memory = wrong_memory;
            state.bind_error = bind_error;
        });
        let (result, output) = import();
        assert_eq!(result, expected);
        assert_eq!(output.image, 0);
        assert_eq!(live(), [0, 0, 0, 1]);
    }
    STATE.with_borrow_mut(|state| state.bind_error = false);
    let (result, depth) = import();
    assert_eq!(result, 0);
    assert_eq!(
        [depth.width, depth.height, depth.format],
        [32, 24, R32_FLOAT]
    );
    assert!(!depth.dx12_resource.is_null());
    let (result, motion) = import();
    assert_eq!(result, 0);
    assert_eq!(
        unsafe { rsf_dx12vk_interop_release_guides(owner, depth.image, 42) },
        INVALID
    );
    assert_eq!(
        unsafe { rsf_dx12vk_interop_release_guides(owner, depth.image, depth.image) },
        INVALID
    );
    assert_eq!(unsafe { rsf_dx12vk_interop_destroy(owner, 0) }, INVALID);
    assert_eq!(live(), [2, 2, 2, 1]);
    assert_eq!(
        unsafe { rsf_dx12vk_interop_release_guides(owner, depth.image, motion.image) },
        0
    );
    assert_eq!(live(), [0, 0, 0, 1]);
    assert_eq!(import().0, 0);
    assert_eq!(unsafe { rsf_dx12vk_interop_destroy(owner, 1) }, 0);
    assert_eq!(live(), [0; 4]);
}
