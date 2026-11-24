// SPDX-License-Identifier: MIT
//! Rust owner for the standalone VulkanStoryFsr3.dll runtime binding and its
//! SR, frame-generation and swapchain owners.
//!
//! The managed `Fsr3Native` caller loads the signed FidelityFX SDK 1.1.4 Vulkan
//! runtime and this bridge, then opens the bridge with the SDK module. Rust
//! retains that binding for the process, admits every SR, interpolation and
//! swapchain call, owns the interpolation/swapchain pairing with its checked
//! disable, drain and UI release ordering, and owns checked retirement of all
//! three owners. The Windows platform module builds descriptors and invokes the
//! signed SDK using bindings generated from the pinned Vulkan ABI.

use std::ffi::{CStr, c_char, c_void};
use std::io::Write as _;
use std::mem::offset_of;
use std::ptr::{self, NonNull};
use std::sync::OnceLock;

use rsf_game_sys::ffi_util::try_box;

#[cfg(not(test))]
#[path = "platform.rs"]
mod platform_win32;
// Production calls the SDK leaves in platform.rs; unit tests call same-named fakes.
#[cfg(not(test))]
use platform_win32 as leaf;
#[cfg(test)]
use tests as leaf;
/// The pinned FidelityFX SDK 1.1.4 Vulkan records (generate-bindings.py), shared with the Runtime.
pub mod sdk_abi;
// The swapchain pass-through refusal.
use sdk_abi::{VK_ERROR_INITIALIZATION_FAILED, VkSwapchainCreateInfoKHR};

type Handle = *mut c_void;

/// Non-dispatchable Vulkan handle (`VkSwapchainKHR`, `VkSemaphore`, `VkFence`).
type Vk = u64;

// VkFormat values the SR and frame-generation paths accept.
const R8_UNORM: i32 = 9;
const R8G8B8A8_UNORM: i32 = 37;
const R16G16_SFLOAT: i32 = 83;
const R16G16B16A16_SFLOAT: i32 = 97;
const D32_SFLOAT: i32 = 126;

/// Frame time used when the caller supplies no positive delta.
const DEFAULT_FRAME_TIME_MS: f32 = 16.667;

const EXPORTS: [&CStr; 5] = [
    c"ffxCreateContext",
    c"ffxDestroyContext",
    c"ffxQuery",
    c"ffxDispatch",
    c"ffxConfigure",
];

/// The bound runtime. Entries are opaque here; only the platform leaves and the
/// presentation callback call them.
#[derive(Clone, Copy)]
struct Sdk {
    module: Handle,
    create: Handle,
    destroy: Handle,
    query: Handle,
    dispatch: Handle,
    configure: Handle,
}

// SAFETY: the table holds process-retained module and export addresses that
// never change after the single successful bind.
unsafe impl Send for Sdk {}
// SAFETY: as above; readers only copy the addresses.
unsafe impl Sync for Sdk {}

static SDK: OnceLock<Sdk> = OnceLock::new();
static UNBOUND: Sdk = Sdk {
    module: ptr::null_mut(),
    create: ptr::null_mut(),
    destroy: ptr::null_mut(),
    query: ptr::null_mut(),
    dispatch: ptr::null_mut(),
    configure: ptr::null_mut(),
};

fn sdk() -> &'static Sdk {
    SDK.get().unwrap_or(&UNBOUND)
}

/// Matches the managed `Fsr3Image`.
#[repr(C)]
struct Image {
    image: u64,
    width: u32,
    height: u32,
    format: i32,
}

/// Matches the managed `Fsr3Frame`.
// platform.rs reads the commands and constants; the unit tests replace it.
#[cfg_attr(test, allow(dead_code))]
#[repr(C)]
struct Frame {
    commands: Handle,
    color: Image,
    depth: Image,
    motion: Image,
    output: Image,
    reactive: Image,
    composition: Image,
    jitter_x: f32,
    jitter_y: f32,
    delta_ms: f32,
    near_plane: f32,
    far_plane: f32,
    fov_radians: f32,
    reset: u32,
}

impl Frame {
    fn formats_supported(&self) -> bool {
        self.color.format == R16G16B16A16_SFLOAT
            && self.depth.format == D32_SFLOAT
            && self.motion.format == R16G16_SFLOAT
            && self.output.format == R16G16B16A16_SFLOAT
            && self.reactive.format == R8_UNORM
            && self.composition.format == R8_UNORM
    }
}

// NaN and non-positive deltas take the default.
fn frame_time_ms(delta_ms: f32) -> f32 {
    if delta_ms > 0.0 {
        delta_ms
    } else {
        DEFAULT_FRAME_TIME_MS
    }
}

/// Matches the managed `Fsr3FgFrame`.
// platform.rs's configure, prepare, generate and UI leaves read the resources, constants and
// camera vectors; the unit tests replace it.
#[cfg_attr(test, allow(dead_code))]
#[repr(C)]
struct FgFrame {
    commands: Handle,
    color: Image,
    depth: Image,
    motion: Image,
    output: Image,
    hudless: Image,
    jitter_x: f32,
    jitter_y: f32,
    delta_ms: f32,
    near_plane: f32,
    far_plane: f32,
    fov_radians: f32,
    frame_id: u64,
    reset: u32,
    camera: [f32; 12],
    ui: Image,
    swapchain_context: Handle,
}

impl FgFrame {
    fn formats_supported(&self) -> bool {
        self.color.format == R8G8B8A8_UNORM
            && self.ui.format == R8G8B8A8_UNORM
            && self.depth.format == D32_SFLOAT
            && self.motion.format == R16G16_SFLOAT
    }
}

/// The arguments of `rsf_vk_fsr3_swapchain_create`.
// platform.rs's create leaf reads the queue family; the unit tests replace it.
#[cfg_attr(test, allow(dead_code))]
struct WsiRequest {
    physical: Handle,
    device: Handle,
    game: Handle,
    async_compute: Handle,
    present: Handle,
    acquire: Handle,
    info: *const u8,
    queue_family: u32,
}

impl WsiRequest {
    /// # Safety
    /// A non-null `info` addresses a readable `VkSwapchainCreateInfoKHR`.
    unsafe fn admitted(&self) -> bool {
        !self.physical.is_null()
            && !self.device.is_null()
            && !self.game.is_null()
            && !self.async_compute.is_null()
            && !self.present.is_null()
            && !self.acquire.is_null()
            && !self.info.is_null()
            && unsafe { (*self.info.cast::<VkSwapchainCreateInfoKHR>()).oldSwapchain }.is_null()
    }
}

/// The arguments of `rsf_vk_fsr3_fg_create`.
// platform.rs's create leaf maps the back-buffer format; the unit tests replace it.
#[cfg_attr(test, allow(dead_code))]
struct FgRequest {
    device: Handle,
    physical: Handle,
    width: u32,
    height: u32,
    back_buffer_format: i32,
}

/// The arguments of `rsf_vk_fsr3_create`.
struct Request {
    device: Handle,
    physical: Handle,
    render_width: u32,
    render_height: u32,
    width: u32,
    height: u32,
}

// The managed records' layouts.
const _: () = {
    assert!(size_of::<Image>() == 24);
    assert!(size_of::<Frame>() == 184);
    assert!(offset_of!(Frame, jitter_x) == 152);
    assert!(offset_of!(Frame, composition) == 128);
    assert!(size_of::<FgFrame>() == 248);
    assert!(offset_of!(FgFrame, delta_ms) == 136 && offset_of!(FgFrame, frame_id) == 152);
    assert!(offset_of!(FgFrame, ui) == 216 && offset_of!(FgFrame, swapchain_context) == 240);
};

/// Opaque SR capsule (platform.rs's `SrCapsule`).
struct Native {
    _private: [u8; 0],
}

/// Opaque presentation capsule (platform.rs's `WsiCapsule`).
struct WsiNative {
    _private: [u8; 0],
}

/// Opaque interpolation capsule (platform.rs's `FgCapsule`).
struct FgNative {
    _private: [u8; 0],
}

/// The exported `rsf_vk_fsr3_context`. The host serializes calls on one
/// context and retires its Vulkan work before destruction.
struct Context {
    native: NonNull<Native>,
}

/// Binds the five FidelityFX exports from a borrowed runtime module.
///
/// One runtime identity is retained; the caller keeps the module loaded.
/// Returns 0 on success or for the same module, -1 for null, -2 for another
/// runtime or a missing export. Missing exports leave the bridge unbound.
///
/// # Safety
/// `runtime` is null or a loaded module.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_fsr3_open(runtime: Handle) -> i32 {
    if runtime.is_null() {
        return -1;
    }
    let same = |bound: &Sdk| if bound.module == runtime { 0 } else { -2 };
    if let Some(bound) = SDK.get() {
        return same(bound);
    }
    let mut entries = [ptr::null_mut(); EXPORTS.len()];
    for (entry, name) in entries.iter_mut().zip(EXPORTS) {
        *entry = unsafe { leaf::resolve(runtime, name.as_ptr()) };
        if entry.is_null() {
            return -2;
        }
    }
    let [create, destroy, query, dispatch, configure] = entries;
    let table = Sdk {
        module: runtime,
        create,
        destroy,
        query,
        dispatch,
        configure,
    };
    match SDK.set(table) {
        Ok(()) => 0,
        // A concurrent open bound first; report against that identity.
        Err(_) => same(sdk()),
    }
}

/// Queries the render size for a display size and FidelityFX quality mode.
///
/// Returns the SDK result, or -1 without a bound query or output slots.
///
/// # Safety
/// Non-null outputs must be writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_plan(
    display_width: u32,
    display_height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    let Some(sdk) = SDK.get() else {
        return -1;
    };
    if render_width.is_null() || render_height.is_null() {
        return -1;
    }
    unsafe {
        leaf::plan(
            sdk.query,
            display_width,
            display_height,
            quality,
            render_width,
            render_height,
        )
    }
}

/// Creates an HDR/auto-exposure SR context with independent input and output maxima.
///
/// Returns 0 and writes `result` on success. Failures leave `result` unwritten:
/// -1 for missing prerequisites, -2 for allocation failure, -3 when the Vulkan
/// loader or the buffer-requirements entry is absent, or the SDK result.
///
/// # Safety
/// `result` is null or writable.
unsafe fn create_sr(request: &Request, result: *mut *mut Context) -> i32 {
    let Some(sdk) = SDK.get() else {
        return -1;
    };
    if result.is_null()
        || request.device.is_null()
        || request.physical.is_null()
        || request.render_width == 0
        || request.render_height == 0
        || request.width == 0
        || request.height == 0
    {
        return -1;
    }
    let Some(native) = NonNull::new(unsafe { leaf::allocate() }) else {
        return -2;
    };
    let Some(context) = try_box(Context { native }) else {
        unsafe { leaf::release(native.as_ptr()) };
        return -2;
    };
    let failed = |code: i32| {
        unsafe { leaf::release(native.as_ptr()) };
        code
    };
    if !unsafe { leaf::bind_vulkan(request.device) } {
        return failed(-3);
    }
    let code = unsafe { leaf::create(native.as_ptr(), sdk.create, request) };
    if code != 0 {
        return failed(code);
    }
    unsafe { result.write(Box::into_raw(context)) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_create(
    device: Handle,
    physical: Handle,
    render_width: u32,
    render_height: u32,
    width: u32,
    height: u32,
    result: *mut *mut Context,
) -> i32 {
    let request = Request {
        device,
        physical,
        render_width,
        render_height,
        width,
        height,
    };
    unsafe { create_sr(&request, result) }
}

/// Records SR with independent R8 reactive and composition masks.
///
/// Returns the SDK result, -1 for a missing context or frame, and
/// -2 when a required image format differs.
///
/// # Safety
/// Non-null pointers must be a live context and a readable 184-byte frame.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_evaluate_with_composition(
    context: *mut Context,
    frame: *const Frame,
) -> i32 {
    let (Some(context), Some(frame)) = (unsafe { context.as_ref() }, unsafe { frame.as_ref() })
    else {
        return -1;
    };
    let dispatch = SDK.get().expect("live SR context has a bound SDK").dispatch;
    if !frame.formats_supported() {
        return -2;
    }
    unsafe {
        leaf::evaluate(
            context.native.as_ptr(),
            dispatch,
            frame,
            frame_time_ms(frame.delta_ms),
        )
    }
}

/// Destroys the SR effect, then releases the owner after SDK success.
///
/// The caller first completes Vulkan work that references the effect. Returns
/// 0 for null or success, or the SDK release result. A nonzero result keeps
/// the context attached.
///
/// # Safety
/// `context` is null or a live context from `create_sr`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_destroy(context: *mut Context) -> i32 {
    let Some(owner) = (unsafe { context.as_ref() }) else {
        return 0;
    };
    let destroy = SDK.get().expect("live SR context has a bound SDK").destroy;
    let native = owner.native.as_ptr();
    let effect = unsafe { leaf::has_effect(native) };
    let code = if effect {
        unsafe { leaf::destroy(native, destroy) }
    } else {
        0
    };
    if code != 0 {
        return code;
    }
    unsafe {
        leaf::release(native);
        drop(Box::from_raw(context));
    }
    0
}

/// The exported `rsf_vk_fsr3_swapchain`: one SDK presentation context, its
/// replacement function table (in the capsule), the device it was created on
/// and the current chain.
///
/// `active_fg` borrows the interpolation owner registered on this chain until
/// its checked disable, drain and UI release succeed. The host serializes
/// calls on linked owners; all access goes through raw pointers because the
/// two owners point at each other.
struct Swapchain {
    native: NonNull<WsiNative>,
    device: Handle,
    chain: Vk,
    active_fg: *mut FgContext,
    cleanup_only: bool,
}

/// The exported `rsf_vk_fsr3_fg_context`: one interpolation effect (in the
/// capsule), its display extent and its borrowed swapchain association.
///
/// `chain` and `swapchain` are set together after a successful proxy
/// configure and cleared together only after a successful disable.
struct FgContext {
    native: NonNull<FgNative>,
    width: u32,
    height: u32,
    chain: Vk,
    swapchain: *mut Swapchain,
}

/// # Safety
/// `swapchain` is a live owner.
unsafe fn has_context(swapchain: *mut Swapchain) -> bool {
    unsafe { leaf::wsi_has_context((*swapchain).native.as_ptr()) }
}

/// Creates the SDK presentation context and queries its replacement table.
///
/// Returns -1 for any missing prerequisite or a non-null `oldSwapchain` without
/// touching `result`; otherwise clears `result` first. Then -2 for allocation
/// failure, the SDK creation result, or after releasing the context, the
/// query result or -3 for an incomplete table. If rollback cannot release the
/// context, writes a cleanup-only owner to `result`. Retry with destroy before
/// releasing the underlying device.
///
/// # Safety
/// `request.info` is null or readable; `result` is null or writable.
unsafe fn create_swapchain(request: &WsiRequest, result: *mut *mut Swapchain) -> i32 {
    let Some(sdk) = SDK.get() else {
        return -1;
    };
    if result.is_null() || !unsafe { request.admitted() } {
        return -1;
    }
    unsafe { result.write(ptr::null_mut()) };
    let Some(native) = NonNull::new(unsafe { leaf::wsi_allocate() }) else {
        return -2;
    };
    let Some(owner) = try_box(Swapchain {
        native,
        device: request.device,
        chain: 0,
        active_fg: ptr::null_mut(),
        cleanup_only: false,
    }) else {
        unsafe { leaf::wsi_release(native.as_ptr()) };
        return -2;
    };
    let owner = Box::into_raw(owner);
    // Copies the two pointers so the SDK can still write through `owner`.
    let failed = move |code: i32| {
        unsafe {
            leaf::wsi_release(native.as_ptr());
            drop(Box::from_raw(owner));
        }
        code
    };
    // The SDK writes the created chain through the owner's slot.
    let code = unsafe {
        leaf::wsi_create(
            native.as_ptr(),
            sdk.create,
            request,
            &raw mut (*owner).chain,
        )
    };
    if code != 0 {
        return failed(code);
    }
    let code = unsafe { leaf::wsi_query(native.as_ptr(), sdk.query) };
    if code != 0 || !unsafe { leaf::wsi_complete(native.as_ptr()) } {
        let error = if code != 0 { code } else { -3 };
        if unsafe { leaf::wsi_destroy(native.as_ptr(), sdk.destroy) } == 0 {
            return failed(error);
        }
        unsafe {
            (*owner).cleanup_only = true;
            (*owner).chain = 0;
            result.write(owner);
        }
        return error;
    }
    unsafe { result.write(owner) };
    0
}

/// A failed rollback can return a cleanup-only owner for SwapchainDestroy.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_create(
    physical: Handle,
    device: Handle,
    game: Handle,
    async_compute: Handle,
    present: Handle,
    acquire: Handle,
    queue_family: u32,
    info: *const u8,
    result: *mut *mut Swapchain,
) -> i32 {
    let request = WsiRequest {
        physical,
        device,
        game,
        async_compute,
        present,
        acquire,
        info,
        queue_family,
    };
    unsafe { create_swapchain(&request, result) }
}

/// Returns the owner's current chain, or `VK_NULL_HANDLE` for null.
///
/// # Safety
/// `value` is null or a live swapchain owner.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_handle(value: *mut Swapchain) -> Vk {
    if value.is_null() {
        0
    } else {
        unsafe { (*value).chain }
    }
}

/// Creates a replacement chain through the same context; the owner's chain
/// updates only on `VK_SUCCESS`.
///
/// # Safety
/// `value` is null or a live owner; non-null `info` and `chain` are valid.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_recreate(
    value: *mut Swapchain,
    info: *const u8,
    chain: *mut Vk,
) -> i32 {
    if value.is_null() || info.is_null() || chain.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    unsafe {
        if (*value).cleanup_only {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let disabled = fg_disable((*value).active_fg);
        if disabled != 0 {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let code = leaf::wsi_recreate((*value).native.as_ptr(), (*value).device, info, chain);
        if code == 0 {
            (*value).chain = chain.read();
        }
        code
    }
}

/// Enumerates proxy images with Vulkan count/query semantics.
///
/// # Safety
/// `value` is null or a live owner; the pointers follow the Vulkan contract.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_get_images(
    value: *mut Swapchain,
    chain: Vk,
    count: *mut u32,
    images: *mut Vk,
) -> i32 {
    if value.is_null() || count.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    unsafe {
        if (*value).cleanup_only {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        leaf::wsi_get_images(
            (*value).native.as_ptr(),
            (*value).device,
            chain,
            count,
            images,
        )
    }
}

/// Acquires a proxy image.
///
/// # Safety
/// `value` is null or a live owner; the handles follow the Vulkan contract.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_acquire(
    value: *mut Swapchain,
    chain: Vk,
    timeout: u64,
    semaphore: Vk,
    fence: Vk,
    index: *mut u32,
) -> i32 {
    if value.is_null() || index.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    unsafe {
        if (*value).cleanup_only {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        leaf::wsi_acquire(
            (*value).native.as_ptr(),
            (*value).device,
            chain,
            timeout,
            semaphore,
            fence,
            index,
        )
    }
}

/// Queues presentation through the proxy.
///
/// # Safety
/// `value` is null or a live owner; `info` is null or a valid present info.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_present(
    value: *mut Swapchain,
    queue: Handle,
    info: *const c_void,
) -> i32 {
    if value.is_null() || info.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    unsafe {
        if (*value).cleanup_only {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        leaf::wsi_present((*value).native.as_ptr(), queue, info)
    }
}

/// Disables interpolation linked to `chain`, then waits for proxy presents.
///
/// Returns 0 for a null chain, -1 for a missing owner, context or dispatch,
/// the disable result, or the wait result. A failure keeps every link.
///
/// # Safety
/// `value` is null or a live owner whose `active_fg` is null or live.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_prepare_destroy(
    value: *mut Swapchain,
    chain: Vk,
) -> i32 {
    if chain == 0 {
        return 0;
    }
    if value.is_null() || !unsafe { has_context(value) } || unsafe { (*value).cleanup_only } {
        return -1;
    }
    let dispatch = SDK.get().expect("live swapchain has a bound SDK").dispatch;
    unsafe {
        let fg = (*value).active_fg;
        if !fg.is_null() && (*fg).chain == chain {
            return fg_disable(fg);
        }
        leaf::wsi_wait((*value).native.as_ptr(), dispatch)
    }
}

/// Prepares and destroys `chain` through its owner, clearing the current
/// chain only when it matches.
///
/// Returns 0 for success or a null chain, the preparation error, or -1 when
/// the replacement destroy entry is absent.
///
/// # Safety
/// As `rsf_vk_fsr3_swapchain_prepare_destroy`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_destroy_chain_checked(
    value: *mut Swapchain,
    chain: Vk,
) -> i32 {
    let prepared = unsafe { rsf_vk_fsr3_swapchain_prepare_destroy(value, chain) };
    if prepared != 0 {
        return prepared;
    }
    if chain == 0 {
        return 0;
    }
    // Preparation succeeded for a nonzero chain, so the owner is live.
    unsafe {
        let code = leaf::wsi_destroy_chain((*value).native.as_ptr(), (*value).device, chain);
        if code != 0 {
            return code;
        }
        if (*value).chain == chain {
            (*value).chain = 0;
        }
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_destroy_chain(value: *mut Swapchain, chain: Vk) {
    let code = unsafe { rsf_vk_fsr3_swapchain_destroy_chain_checked(value, chain) };
    if code != 0 {
        let _ = writeln!(
            std::io::stderr().lock(),
            "[FidelityFX] swapchain destruction retained ownership after error {code}"
        );
    }
}

/// Returns the SDK's present count, or zero without an owner, chain or entry.
///
/// # Safety
/// `value` is null or a live owner.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_last_present_count(
    value: *mut Swapchain,
    chain: Vk,
) -> u64 {
    if value.is_null() || chain == 0 || unsafe { (*value).cleanup_only } {
        return 0;
    }
    unsafe { leaf::wsi_last_present_count((*value).native.as_ptr(), chain) }
}

/// Disables the linked interpolation, drains the current chain and releases
/// the SDK context, then the owner.
///
/// Returns 0 for null or success, the preparation error, or the SDK release result. Every
/// failure keeps the owner attached.
///
/// # Safety
/// `value` is null or a live owner whose `active_fg` is null or live.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_swapchain_destroy(value: *mut Swapchain) -> i32 {
    if value.is_null() {
        return 0;
    }
    unsafe {
        let prepared = rsf_vk_fsr3_swapchain_prepare_destroy(value, (*value).chain);
        if prepared != 0 {
            return prepared;
        }
        let destroy = SDK.get().expect("live swapchain has a bound SDK").destroy;
        let native = (*value).native.as_ptr();
        let code = if has_context(value) {
            leaf::wsi_destroy(native, destroy)
        } else {
            0
        };
        if code != 0 {
            return code;
        }
        leaf::wsi_release(native);
        drop(Box::from_raw(value));
    }
    0
}

/// Creates an interpolation effect with display extent and HUD-less RGBA
/// format metadata.
///
/// Returns 0 and writes `result` on success. Failures leave `result`
/// unwritten: -1 for missing prerequisites, -2 for allocation failure, -3
/// without the Vulkan loader, or the SDK result.
///
/// # Safety
/// `result` is null or writable.
unsafe fn create_fg(request: &FgRequest, result: *mut *mut FgContext) -> i32 {
    let Some(sdk) = SDK.get() else {
        return -1;
    };
    let create = sdk.create;
    if result.is_null()
        || request.device.is_null()
        || request.physical.is_null()
        || request.width == 0
        || request.height == 0
    {
        return -1;
    }
    let Some(native) = NonNull::new(unsafe { leaf::fg_allocate() }) else {
        return -2;
    };
    let Some(owner) = try_box(FgContext {
        native,
        width: request.width,
        height: request.height,
        chain: 0,
        swapchain: ptr::null_mut(),
    }) else {
        unsafe { leaf::fg_release(native.as_ptr()) };
        return -2;
    };
    let failed = |code: i32| {
        unsafe { leaf::fg_release(native.as_ptr()) };
        code
    };
    if !unsafe { leaf::fg_bind_vulkan() } {
        return failed(-3);
    }
    let code = unsafe { leaf::fg_create(native.as_ptr(), create, request) };
    if code != 0 {
        return failed(code);
    }
    unsafe { result.write(Box::into_raw(owner)) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_fg_create(
    device: Handle,
    physical: Handle,
    width: u32,
    height: u32,
    back_buffer_format: i32,
    result: *mut *mut FgContext,
) -> i32 {
    let request = FgRequest {
        device,
        physical,
        width,
        height,
        back_buffer_format,
    };
    unsafe { create_fg(&request, result) }
}

/// Disables proxy interpolation, drains the linked chain's presents and
/// clears its UI registration, then unlinks both owners.
///
/// The stored association is authoritative, so the swapchain argument is not
/// read. Returns 0 for null or an unlinked context, -1 for missing
/// prerequisites or a stale pairing, or the first SDK failure; every failure
/// keeps the links so tagged inputs and the effect stay live.
///
/// # Safety
/// `context` is null or a live owner whose linked swapchain is live.
unsafe fn fg_disable(context: *mut FgContext) -> i32 {
    if context.is_null() {
        return 0;
    }
    let sdk = SDK.get().expect("live FG context has a bound SDK");
    unsafe {
        let chain = (*context).chain;
        if chain == 0 {
            return 0;
        }
        let native = (*context).native.as_ptr();
        if !leaf::fg_has_effect(native) {
            return -1;
        }
        let swapchain = (*context).swapchain;
        if swapchain.is_null() || !has_context(swapchain) || (*swapchain).chain != chain {
            return -1;
        }
        let code = leaf::fg_disable(native, sdk.configure, chain);
        if code != 0 {
            return code;
        }
        let presenter = (*swapchain).native.as_ptr();
        let waited = leaf::wsi_wait(presenter, sdk.dispatch);
        if waited != 0 {
            return waited;
        }
        let cleared = leaf::wsi_register_ui(presenter, sdk.configure, ptr::null());
        if cleared != 0 {
            return cleared;
        }
        if (*swapchain).active_fg == context {
            (*swapchain).active_fg = ptr::null_mut();
        }
        (*context).swapchain = ptr::null_mut();
        (*context).chain = 0;
        code
    }
}

/// Exported disable. See `fg_disable`.
///
/// # Safety
/// As `fg_disable`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_fg_disable(
    context: *mut FgContext,
    _swapchain: *mut Swapchain,
) -> i32 {
    unsafe { fg_disable(context) }
}

/// Configures interpolation and records preparation for the frame's path.
///
/// A null `swapchain_context` records one direct generated output on the host
/// command buffer. Otherwise a context linked elsewhere is disabled first,
/// the proxy callback is configured, the pair is linked and the frame's UI is
/// registered. Returns -1 for missing prerequisites, -2 for unsupported
/// resources or path metadata, or the first SDK failure.
///
/// # Safety
/// `context` is null or a live owner; `frame` is null or a readable 248-byte
/// record whose `swapchain_context` is null or a live swapchain owner.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_fg_evaluate(
    context: *mut FgContext,
    frame: *const FgFrame,
) -> i32 {
    let Some(record) = (unsafe { frame.as_ref() }) else {
        return -1;
    };
    if context.is_null() {
        return -1;
    }
    let sdk = SDK.get().expect("live FG context has a bound SDK");
    let swapchain = record.swapchain_context.cast::<Swapchain>();
    let direct = swapchain.is_null();
    unsafe {
        // Short-circuit order matches the native check: the swapchain owner
        // is read only after every format and the command buffer pass.
        let admitted = record.formats_supported()
            && !record.commands.is_null()
            && if direct {
                record.output.image != 0
                    && record.output.format == R8G8B8A8_UNORM
                    && record.output.width == (*context).width
                    && record.output.height == (*context).height
            } else {
                has_context(swapchain)
                    && (*swapchain).chain != 0
                    && ((*swapchain).active_fg.is_null() || (*swapchain).active_fg == context)
            };
        if !admitted {
            return -2;
        }
        let linked = (*context).swapchain;
        if !linked.is_null() && linked != swapchain {
            let disabled = fg_disable(context);
            if disabled != 0 {
                return disabled;
            }
        }
        let native = (*context).native.as_ptr();
        let chain = if direct { 0 } else { (*swapchain).chain };
        let code = leaf::fg_configure(native, sdk.configure, frame, chain);
        if code != 0 {
            return code;
        }
        if !direct {
            (*context).chain = chain;
            (*context).swapchain = swapchain;
            (*swapchain).active_fg = context;
        }
        let code = leaf::fg_prepare(native, sdk.dispatch, frame, frame_time_ms(record.delta_ms));
        if code != 0 {
            return code;
        }
        if direct {
            return leaf::fg_generate(native, sdk.dispatch, frame);
        }
        leaf::wsi_register_ui((*swapchain).native.as_ptr(), sdk.configure, frame)
    }
}

/// Requires a direct frame, then evaluates it.
///
/// Returns -1 for a null frame or a non-null `swapchain_context`.
///
/// # Safety
/// As `rsf_vk_fsr3_fg_evaluate`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_fg_evaluate_direct(
    context: *mut FgContext,
    frame: *const FgFrame,
) -> i32 {
    match unsafe { frame.as_ref() } {
        Some(record) if record.swapchain_context.is_null() => unsafe {
            rsf_vk_fsr3_fg_evaluate(context, frame)
        },
        _ => -1,
    }
}

/// Disables any linked proxy, releases the effect, then the owner.
///
/// Returns 0 for null or success, otherwise the disable or SDK release
/// result, which keeps the owner.
///
/// # Safety
/// `context` is null or a live owner; the host has retired direct-mode work.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_fsr3_fg_destroy(context: *mut FgContext) -> i32 {
    if context.is_null() {
        return 0;
    }
    unsafe {
        let disabled = fg_disable(context);
        if disabled != 0 {
            return disabled;
        }
        let native = (*context).native.as_ptr();
        let destroy = SDK.get().expect("live FG context has a bound SDK").destroy;
        let code = if leaf::fg_has_effect(native) {
            leaf::fg_destroy(native, destroy)
        } else {
            0
        };
        if code != 0 {
            return code;
        }
        leaf::fg_release(native);
        drop(Box::from_raw(context));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

    struct Fake {
        effect: bool,
    }

    static LIVE: AtomicI32 = AtomicI32::new(0);
    static EVALUATIONS: AtomicU32 = AtomicU32::new(0);
    static FRAME_TIME_BITS: AtomicU32 = AtomicU32::new(0);
    static FAIL_DESTROY: AtomicBool = AtomicBool::new(false);
    static MARK: [u8; 8] = [0; 8];

    const MISSING_CONFIGURE: usize = 0x2000;
    const NO_BUFFER_REQUIREMENTS: usize = 2;
    const NO_LOADER: usize = 3;
    const SDK_REFUSES: u32 = 13;

    fn mark(index: usize) -> Handle {
        MARK[index..].as_ptr().cast_mut().cast()
    }

    pub(super) unsafe fn resolve(module: Handle, name: *const c_char) -> Handle {
        let name = unsafe { CStr::from_ptr(name) };
        if module as usize == MISSING_CONFIGURE && name == c"ffxConfigure" {
            return ptr::null_mut();
        }
        let index = EXPORTS.iter().position(|export| *export == name).unwrap();
        mark(index + 1)
    }

    pub(super) unsafe fn allocate() -> *mut Native {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Box::into_raw(Box::new(Fake { effect: false })).cast()
    }

    pub(super) unsafe fn release(native: *mut Native) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
        drop(unsafe { Box::from_raw(native.cast::<Fake>()) });
    }

    pub(super) unsafe fn bind_vulkan(device: Handle) -> bool {
        !matches!(device as usize, NO_LOADER | NO_BUFFER_REQUIREMENTS)
    }

    pub(super) unsafe fn create(
        native: *mut Native,
        create: Handle,
        request: *const Request,
    ) -> i32 {
        assert_eq!(create, mark(1));
        if unsafe { &*request }.width == SDK_REFUSES {
            return 7;
        }
        unsafe { &mut *native.cast::<Fake>() }.effect = true;
        0
    }

    pub(super) unsafe fn plan(
        query: Handle,
        width: u32,
        height: u32,
        quality: u32,
        render_width: *mut u32,
        render_height: *mut u32,
    ) -> i32 {
        assert_eq!(query, mark(3));
        unsafe {
            render_width.write(width / quality);
            render_height.write(height / quality);
        }
        0
    }

    pub(super) unsafe fn evaluate(
        _: *mut Native,
        dispatch: Handle,
        _: *const Frame,
        frame_time: f32,
    ) -> i32 {
        assert_eq!(dispatch, mark(4));
        EVALUATIONS.fetch_add(1, Ordering::SeqCst);
        FRAME_TIME_BITS.store(frame_time.to_bits(), Ordering::SeqCst);
        0
    }

    pub(super) unsafe fn has_effect(native: *const Native) -> bool {
        unsafe { &*native.cast::<Fake>() }.effect
    }

    pub(super) unsafe fn destroy(native: *mut Native, destroy: Handle) -> i32 {
        assert_eq!(destroy, mark(2));
        if FAIL_DESTROY.load(Ordering::SeqCst) {
            return 5;
        }
        unsafe { &mut *native.cast::<Fake>() }.effect = false;
        0
    }

    fn image(format: i32) -> Image {
        Image {
            image: 1,
            width: 8,
            height: 8,
            format,
        }
    }

    fn frame(delta_ms: f32, composition: i32) -> Frame {
        Frame {
            commands: mark(0),
            color: image(R16G16B16A16_SFLOAT),
            depth: image(D32_SFLOAT),
            motion: image(R16G16_SFLOAT),
            output: image(R16G16B16A16_SFLOAT),
            reactive: image(R8_UNORM),
            composition: image(composition),
            jitter_x: 0.0,
            jitter_y: 0.0,
            delta_ms,
            near_plane: 0.1,
            far_plane: 100.0,
            fov_radians: 1.0,
            reset: 0,
        }
    }

    fn request(device: usize, width: u32) -> Request {
        Request {
            device: device as Handle,
            physical: 0x10 as Handle,
            render_width: 4,
            render_height: 4,
            width,
            height: 8,
        }
    }

    // Frame generation and WSI fakes. Every SDK-facing leaf appends its name
    // to LOG, and FAIL makes each selected leaf return a code once.
    struct FakeWsi {
        context: bool,
        complete: bool,
    }

    struct FakeFg {
        effect: bool,
    }

    static LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    static FAIL: Mutex<Vec<(&'static str, i32)>> = Mutex::new(Vec::new());
    static LIVE_WSI: AtomicI32 = AtomicI32::new(0);
    static LIVE_FG: AtomicI32 = AtomicI32::new(0);
    static INCOMPLETE: AtomicBool = AtomicBool::new(false);

    const CHAIN_A: Vk = 0xA0;
    const CHAIN_B: Vk = 0xB0;
    const VK_ERROR_OUT_OF_DATE: i32 = -1_000_001_004;

    fn step(name: &'static str) -> i32 {
        LOG.lock().unwrap().push(name);
        let mut fail = FAIL.lock().unwrap();
        if let Some(index) = fail.iter().position(|&(failing, _)| failing == name) {
            fail.remove(index).1
        } else {
            0
        }
    }

    fn fail_next(name: &'static str, code: i32) {
        FAIL.lock().unwrap().push((name, code));
    }

    fn take_log() -> Vec<&'static str> {
        std::mem::take(&mut *LOG.lock().unwrap())
    }

    fn wsi(native: *const WsiNative) -> &'static mut FakeWsi {
        unsafe { &mut *native.cast_mut().cast::<FakeWsi>() }
    }

    fn fg(native: *const FgNative) -> &'static mut FakeFg {
        unsafe { &mut *native.cast_mut().cast::<FakeFg>() }
    }

    pub(super) unsafe fn wsi_allocate() -> *mut WsiNative {
        LIVE_WSI.fetch_add(1, Ordering::SeqCst);
        Box::into_raw(Box::new(FakeWsi {
            context: false,
            complete: false,
        }))
        .cast()
    }

    pub(super) unsafe fn wsi_release(native: *mut WsiNative) {
        LIVE_WSI.fetch_sub(1, Ordering::SeqCst);
        drop(unsafe { Box::from_raw(native.cast::<FakeWsi>()) });
    }

    pub(super) unsafe fn wsi_create(
        native: *mut WsiNative,
        create: Handle,
        request: *const WsiRequest,
        chain: *mut Vk,
    ) -> i32 {
        assert_eq!(create, mark(1));
        assert_eq!(unsafe { &*request }.queue_family, 2);
        let code = step("wsi_create");
        if code == 0 {
            wsi(native).context = true;
            unsafe { chain.write(CHAIN_A) };
        }
        code
    }

    pub(super) unsafe fn wsi_query(native: *mut WsiNative, query: Handle) -> i32 {
        assert_eq!(query, mark(3));
        let code = step("wsi_query");
        if code == 0 {
            wsi(native).complete = !INCOMPLETE.load(Ordering::SeqCst);
        }
        code
    }

    pub(super) unsafe fn wsi_complete(native: *const WsiNative) -> bool {
        wsi(native).complete
    }

    pub(super) unsafe fn wsi_has_context(native: *const WsiNative) -> bool {
        wsi(native).context
    }

    pub(super) unsafe fn wsi_destroy(native: *mut WsiNative, destroy: Handle) -> i32 {
        assert_eq!(destroy, mark(2));
        let code = step("wsi_destroy");
        if code == 0 {
            wsi(native).context = false;
        }
        code
    }

    pub(super) unsafe fn wsi_wait(_: *mut WsiNative, dispatch: Handle) -> i32 {
        assert_eq!(dispatch, mark(4));
        step("wsi_wait")
    }

    pub(super) unsafe fn wsi_register_ui(
        _: *mut WsiNative,
        configure: Handle,
        frame: *const FgFrame,
    ) -> i32 {
        assert_eq!(configure, mark(5));
        step(if frame.is_null() {
            "wsi_clear_ui"
        } else {
            "wsi_register_ui"
        })
    }

    pub(super) unsafe fn wsi_recreate(
        _: *mut WsiNative,
        device: Handle,
        _: *const u8,
        chain: *mut Vk,
    ) -> i32 {
        assert_eq!(device as usize, 0x20);
        let code = step("wsi_recreate");
        unsafe { chain.write(if code == 0 { CHAIN_B } else { 0 }) };
        code
    }

    pub(super) unsafe fn wsi_get_images(
        _: *mut WsiNative,
        _: Handle,
        _: Vk,
        count: *mut u32,
        _: *mut Vk,
    ) -> i32 {
        unsafe { count.write(3) };
        step("wsi_get_images")
    }

    pub(super) unsafe fn wsi_acquire(
        _: *mut WsiNative,
        _: Handle,
        _: Vk,
        _: u64,
        _: Vk,
        _: Vk,
        index: *mut u32,
    ) -> i32 {
        unsafe { index.write(1) };
        step("wsi_acquire")
    }

    pub(super) unsafe fn wsi_present(
        _: *mut WsiNative,
        _: Handle,
        _: *const c_void,
    ) -> i32 {
        step("wsi_present")
    }

    pub(super) unsafe fn wsi_destroy_chain(_: *mut WsiNative, _: Handle, _: Vk) -> i32 {
        step("wsi_destroy_chain")
    }

    pub(super) unsafe fn wsi_last_present_count(_: *mut WsiNative, chain: Vk) -> u64 {
        chain + 1
    }

    pub(super) unsafe fn fg_allocate() -> *mut FgNative {
        LIVE_FG.fetch_add(1, Ordering::SeqCst);
        Box::into_raw(Box::new(FakeFg { effect: false })).cast()
    }

    pub(super) unsafe fn fg_release(native: *mut FgNative) {
        LIVE_FG.fetch_sub(1, Ordering::SeqCst);
        drop(unsafe { Box::from_raw(native.cast::<FakeFg>()) });
    }

    pub(super) unsafe fn fg_bind_vulkan() -> bool {
        step("fg_bind_vulkan") == 0
    }

    pub(super) unsafe fn fg_create(
        native: *mut FgNative,
        create: Handle,
        _: *const FgRequest,
    ) -> i32 {
        assert_eq!(create, mark(1));
        let code = step("fg_create");
        if code == 0 {
            fg(native).effect = true;
        }
        code
    }

    pub(super) unsafe fn fg_has_effect(native: *const FgNative) -> bool {
        fg(native).effect
    }

    pub(super) unsafe fn fg_destroy(native: *mut FgNative, destroy: Handle) -> i32 {
        assert_eq!(destroy, mark(2));
        let code = step("fg_destroy");
        if code == 0 {
            fg(native).effect = false;
        }
        code
    }

    pub(super) unsafe fn fg_disable(
        _: *mut FgNative,
        configure: Handle,
        chain: Vk,
    ) -> i32 {
        assert_eq!(configure, mark(5));
        assert_ne!(chain, 0);
        step("fg_disable")
    }

    pub(super) unsafe fn fg_configure(
        _: *mut FgNative,
        configure: Handle,
        _: *const FgFrame,
        chain: Vk,
    ) -> i32 {
        assert_eq!(configure, mark(5));
        step(if chain == 0 {
            "fg_configure_direct"
        } else {
            "fg_configure_proxy"
        })
    }

    pub(super) unsafe fn fg_prepare(
        _: *mut FgNative,
        dispatch: Handle,
        _: *const FgFrame,
        frame_time: f32,
    ) -> i32 {
        assert_eq!(dispatch, mark(4));
        FRAME_TIME_BITS.store(frame_time.to_bits(), Ordering::SeqCst);
        step("fg_prepare")
    }

    pub(super) unsafe fn fg_generate(
        _: *mut FgNative,
        dispatch: Handle,
        _: *const FgFrame,
    ) -> i32 {
        assert_eq!(dispatch, mark(4));
        step("fg_generate")
    }

    /// A `VkSwapchainCreateInfoKHR`-sized buffer; index 12 is `oldSwapchain`.
    type Info = [u64; 13];

    fn wsi_request(info: &Info, game: usize) -> WsiRequest {
        WsiRequest {
            physical: 0x10 as Handle,
            device: 0x20 as Handle,
            game: game as Handle,
            async_compute: 0x31 as Handle,
            present: 0x32 as Handle,
            acquire: 0x33 as Handle,
            info: info.as_ptr().cast(),
            queue_family: 2,
        }
    }

    fn fg_request(width: u32) -> FgRequest {
        FgRequest {
            device: 0x20 as Handle,
            physical: 0x10 as Handle,
            width,
            height: 8,
            back_buffer_format: R8G8B8A8_UNORM,
        }
    }

    fn fg_frame(swapchain: *mut Swapchain, delta_ms: f32, color: i32) -> FgFrame {
        FgFrame {
            commands: mark(0),
            color: image(color),
            depth: image(D32_SFLOAT),
            motion: image(R16G16_SFLOAT),
            output: image(R8G8B8A8_UNORM),
            hudless: image(R8G8B8A8_UNORM),
            jitter_x: 0.0,
            jitter_y: 0.0,
            delta_ms,
            near_plane: 0.1,
            far_plane: 100.0,
            fov_radians: 1.0,
            frame_id: 1,
            reset: 0,
            camera: [0.0; 12],
            ui: image(R8G8B8A8_UNORM),
            swapchain_context: swapchain.cast(),
        }
    }

    /// Before Open every SDK-dependent FG/WSI entry refuses and null owners
    /// take their documented results, with no leaf call.
    fn fg_wsi_unbound() {
        let info: Info = [0; 13];
        let (mut swapchain, mut context) = (ptr::null_mut(), ptr::null_mut());
        let (mut chain, mut count) = (0, 0);
        let none = ptr::null_mut::<Swapchain>();
        unsafe {
            assert_eq!(
                create_swapchain(&wsi_request(&info, 0x30), &mut swapchain),
                -1
            );
            assert_eq!(create_fg(&fg_request(8), &mut context), -1);
            assert_eq!(
                rsf_vk_fsr3_fg_evaluate(context, &fg_frame(none, 1.0, R8G8B8A8_UNORM)),
                -1
            );
            assert_eq!(rsf_vk_fsr3_fg_evaluate_direct(context, ptr::null()), -1);
            assert_eq!(rsf_vk_fsr3_swapchain_handle(none), 0);
            assert_eq!(
                rsf_vk_fsr3_swapchain_recreate(none, info.as_ptr().cast(), &mut chain),
                VK_ERROR_INITIALIZATION_FAILED
            );
            assert_eq!(
                rsf_vk_fsr3_swapchain_get_images(none, CHAIN_A, &mut count, ptr::null_mut()),
                VK_ERROR_INITIALIZATION_FAILED
            );
            assert_eq!(
                rsf_vk_fsr3_swapchain_acquire(none, CHAIN_A, u64::MAX, 0, 0, &mut count),
                VK_ERROR_INITIALIZATION_FAILED
            );
            assert_eq!(
                rsf_vk_fsr3_swapchain_present(none, mark(0), info.as_ptr().cast()),
                VK_ERROR_INITIALIZATION_FAILED
            );
            assert_eq!(rsf_vk_fsr3_swapchain_prepare_destroy(none, 0), 0);
            assert_eq!(rsf_vk_fsr3_swapchain_prepare_destroy(none, CHAIN_A), -1);
            assert_eq!(rsf_vk_fsr3_swapchain_destroy_chain_checked(none, 0), 0);
            assert_eq!(
                rsf_vk_fsr3_swapchain_destroy_chain_checked(none, CHAIN_A),
                -1
            );
            assert_eq!(rsf_vk_fsr3_swapchain_last_present_count(none, CHAIN_A), 0);
            assert_eq!(rsf_vk_fsr3_swapchain_destroy(none), 0);
            assert_eq!(rsf_vk_fsr3_fg_disable(ptr::null_mut(), none), 0);
            assert_eq!(rsf_vk_fsr3_fg_destroy(ptr::null_mut()), 0);
        }
        assert!(swapchain.is_null() && context.is_null());
        assert!(take_log().is_empty());
    }

    /// The bound FG/WSI sequence: creation refusals and partial-owner release,
    /// pass-throughs, direct and proxy evaluation, the pairing and its checked
    /// disable/drain/UI-release order, and retention on every failure.
    fn fg_wsi_bound() {
        let mut info: Info = [0; 13];
        let sentinel = NonNull::<Swapchain>::dangling().as_ptr();
        let mut swapchain = sentinel;
        let mut context = ptr::null_mut();
        let (mut chain, mut count, mut index) = (0, 0, 0);
        unsafe {
            // Refusals before `result` is cleared.
            info[12] = 0x99;
            assert_eq!(
                create_swapchain(&wsi_request(&info, 0x30), &mut swapchain),
                -1
            );
            info[12] = 0;
            assert_eq!(create_swapchain(&wsi_request(&info, 0), &mut swapchain), -1);
            assert_eq!(swapchain, sentinel);

            // SDK failures clear `result`, release partial owners, and release
            // the context after a failed or incomplete query.
            fail_next("wsi_create", 9);
            assert_eq!(
                create_swapchain(&wsi_request(&info, 0x30), &mut swapchain),
                9
            );
            assert!(swapchain.is_null());
            assert_eq!(take_log(), ["wsi_create"]);
            fail_next("wsi_query", 4);
            assert_eq!(
                create_swapchain(&wsi_request(&info, 0x30), &mut swapchain),
                4
            );
            assert_eq!(take_log(), ["wsi_create", "wsi_query", "wsi_destroy"]);
            INCOMPLETE.store(true, Ordering::SeqCst);
            assert_eq!(
                create_swapchain(&wsi_request(&info, 0x30), &mut swapchain),
                -3
            );
            INCOMPLETE.store(false, Ordering::SeqCst);
            assert_eq!(take_log(), ["wsi_create", "wsi_query", "wsi_destroy"]);
            assert_eq!(LIVE_WSI.load(Ordering::SeqCst), 0);

            fail_next("wsi_query", 4);
            fail_next("wsi_destroy", 11);
            assert_eq!(
                create_swapchain(&wsi_request(&info, 0x30), &mut swapchain),
                4
            );
            assert!(!swapchain.is_null());
            assert_eq!(LIVE_WSI.load(Ordering::SeqCst), 1);
            assert_eq!(rsf_vk_fsr3_swapchain_handle(swapchain), 0);
            assert_eq!(
                rsf_vk_fsr3_swapchain_recreate(swapchain, info.as_ptr().cast(), &mut chain),
                VK_ERROR_INITIALIZATION_FAILED
            );
            assert_eq!(rsf_vk_fsr3_swapchain_destroy(swapchain), 0);
            assert_eq!(
                take_log(),
                ["wsi_create", "wsi_query", "wsi_destroy", "wsi_destroy"]
            );
            assert_eq!(LIVE_WSI.load(Ordering::SeqCst), 0);

            assert_eq!(
                create_swapchain(&wsi_request(&info, 0x30), &mut swapchain),
                0
            );
            assert_eq!(take_log(), ["wsi_create", "wsi_query"]);
            let sw = swapchain;
            assert_eq!(rsf_vk_fsr3_swapchain_handle(sw), CHAIN_A);

            // Pass-throughs; a failed recreate keeps the current chain.
            fail_next("wsi_recreate", VK_ERROR_OUT_OF_DATE);
            assert_eq!(
                rsf_vk_fsr3_swapchain_recreate(sw, info.as_ptr().cast(), &mut chain),
                VK_ERROR_OUT_OF_DATE
            );
            assert_eq!(rsf_vk_fsr3_swapchain_handle(sw), CHAIN_A);
            assert_eq!(
                rsf_vk_fsr3_swapchain_get_images(sw, CHAIN_A, &mut count, ptr::null_mut()),
                0
            );
            assert_eq!(count, 3);
            assert_eq!(
                rsf_vk_fsr3_swapchain_get_images(sw, CHAIN_A, ptr::null_mut(), ptr::null_mut()),
                VK_ERROR_INITIALIZATION_FAILED
            );
            assert_eq!(
                rsf_vk_fsr3_swapchain_acquire(sw, CHAIN_A, u64::MAX, 0, 0, &mut index),
                0
            );
            assert_eq!(index, 1);
            assert_eq!(
                rsf_vk_fsr3_swapchain_present(sw, mark(0), ptr::null()),
                VK_ERROR_INITIALIZATION_FAILED
            );
            assert_eq!(
                rsf_vk_fsr3_swapchain_present(sw, mark(0), info.as_ptr().cast()),
                0
            );
            assert_eq!(
                rsf_vk_fsr3_swapchain_last_present_count(sw, CHAIN_A),
                CHAIN_A + 1
            );
            assert_eq!(rsf_vk_fsr3_swapchain_last_present_count(sw, 0), 0);
            assert_eq!(
                take_log(),
                [
                    "wsi_recreate",
                    "wsi_get_images",
                    "wsi_acquire",
                    "wsi_present"
                ]
            );

            // Interpolation creation refusals leave `result` unwritten.
            assert_eq!(create_fg(&fg_request(0), &mut context), -1);
            fail_next("fg_bind_vulkan", 1);
            assert_eq!(create_fg(&fg_request(8), &mut context), -3);
            fail_next("fg_create", 7);
            assert_eq!(create_fg(&fg_request(8), &mut context), 7);
            assert!(context.is_null());
            assert_eq!(LIVE_FG.load(Ordering::SeqCst), 0);
            assert_eq!(create_fg(&fg_request(8), &mut context), 0);
            let fg = context;
            assert_eq!(
                take_log(),
                [
                    "fg_bind_vulkan",
                    "fg_bind_vulkan",
                    "fg_create",
                    "fg_bind_vulkan",
                    "fg_create"
                ]
            );

            // Direct admission and evaluation.
            let none = ptr::null_mut();
            assert_eq!(rsf_vk_fsr3_fg_evaluate(fg, &fg_frame(none, 1.0, 97)), -2);
            let mut narrow = fg_frame(none, 1.0, R8G8B8A8_UNORM);
            narrow.output.width = 4;
            assert_eq!(rsf_vk_fsr3_fg_evaluate(fg, &narrow), -2);
            assert_eq!(
                rsf_vk_fsr3_fg_evaluate_direct(fg, &fg_frame(sw, 1.0, R8G8B8A8_UNORM)),
                -1
            );
            assert!(take_log().is_empty());
            assert_eq!(
                rsf_vk_fsr3_fg_evaluate_direct(fg, &fg_frame(none, 0.0, R8G8B8A8_UNORM)),
                0
            );
            assert_eq!(
                f32::from_bits(FRAME_TIME_BITS.load(Ordering::SeqCst)),
                DEFAULT_FRAME_TIME_MS
            );
            assert_eq!(
                take_log(),
                ["fg_configure_direct", "fg_prepare", "fg_generate"]
            );

            // Proxy evaluation links the pair and registers UI.
            let proxy = fg_frame(sw, 2.5, R8G8B8A8_UNORM);
            assert_eq!(rsf_vk_fsr3_fg_evaluate(fg, &proxy), 0);
            assert_eq!(
                take_log(),
                ["fg_configure_proxy", "fg_prepare", "wsi_register_ui"]
            );
            let mut contender = ptr::null_mut();
            assert_eq!(create_fg(&fg_request(8), &mut contender), 0);
            assert_eq!(take_log(), ["fg_bind_vulkan", "fg_create"]);
            assert_eq!(rsf_vk_fsr3_fg_evaluate(contender, &proxy), -2);
            assert_eq!(rsf_vk_fsr3_fg_destroy(contender), 0);
            assert_eq!(take_log(), ["fg_destroy"]);
            // Another chain drains without disabling.
            assert_eq!(rsf_vk_fsr3_swapchain_prepare_destroy(sw, 0x77), 0);
            assert_eq!(take_log(), ["wsi_wait"]);
            // Failed drain or UI release keeps the link.
            fail_next("wsi_wait", 6);
            assert_eq!(rsf_vk_fsr3_swapchain_prepare_destroy(sw, CHAIN_A), 6);
            assert_eq!(take_log(), ["fg_disable", "wsi_wait"]);
            fail_next("wsi_clear_ui", 8);
            assert_eq!(rsf_vk_fsr3_swapchain_prepare_destroy(sw, CHAIN_A), 8);
            assert_eq!(take_log(), ["fg_disable", "wsi_wait", "wsi_clear_ui"]);
            // A direct frame first disables the linked proxy.
            assert_eq!(
                rsf_vk_fsr3_fg_evaluate(fg, &fg_frame(none, 1.0, R8G8B8A8_UNORM)),
                0
            );
            assert_eq!(
                take_log(),
                [
                    "fg_disable",
                    "wsi_wait",
                    "wsi_clear_ui",
                    "fg_configure_direct",
                    "fg_prepare",
                    "fg_generate"
                ]
            );
            assert_eq!(rsf_vk_fsr3_swapchain_prepare_destroy(sw, CHAIN_A), 0);
            assert_eq!(take_log(), ["wsi_wait"]);

            // Recreate must retire the old pairing before changing chains.
            assert_eq!(rsf_vk_fsr3_fg_evaluate(fg, &proxy), 0);
            take_log();
            fail_next("wsi_wait", 6);
            assert_eq!(
                rsf_vk_fsr3_swapchain_recreate(sw, info.as_ptr().cast(), &mut chain),
                VK_ERROR_INITIALIZATION_FAILED
            );
            assert_eq!(rsf_vk_fsr3_swapchain_handle(sw), CHAIN_A);
            assert_eq!(take_log(), ["fg_disable", "wsi_wait"]);
            assert_eq!(
                rsf_vk_fsr3_swapchain_recreate(sw, info.as_ptr().cast(), &mut chain),
                0
            );
            assert_eq!(
                take_log(),
                ["fg_disable", "wsi_wait", "wsi_clear_ui", "wsi_recreate"]
            );
            assert_eq!(
                (chain, rsf_vk_fsr3_swapchain_handle(sw)),
                (CHAIN_B, CHAIN_B)
            );
            assert_eq!(rsf_vk_fsr3_fg_disable(fg, none), 0);
            assert!(take_log().is_empty());
            assert_eq!(rsf_vk_fsr3_fg_evaluate(fg, &proxy), 0);
            take_log();

            // Destroying the old chain neither disables nor clears the current one.
            assert_eq!(rsf_vk_fsr3_swapchain_destroy_chain_checked(sw, CHAIN_A), 0);
            assert_eq!(take_log(), ["wsi_wait", "wsi_destroy_chain"]);
            assert_eq!(rsf_vk_fsr3_swapchain_handle(sw), CHAIN_B);

            // A failed context release keeps the swapchain owner after the
            // linked interpolation was disabled and the chain drained.
            fail_next("wsi_destroy", 11);
            assert_eq!(rsf_vk_fsr3_swapchain_destroy(sw), 11);
            assert_eq!(
                take_log(),
                ["fg_disable", "wsi_wait", "wsi_clear_ui", "wsi_destroy"]
            );
            assert_eq!(LIVE_WSI.load(Ordering::SeqCst), 1);
            assert_eq!(rsf_vk_fsr3_swapchain_destroy_chain_checked(sw, CHAIN_B), 0);
            assert_eq!(rsf_vk_fsr3_swapchain_handle(sw), 0);
            assert_eq!(rsf_vk_fsr3_fg_evaluate(fg, &proxy), -2);
            take_log();
            // With no chain left, retirement skips the drain.
            assert_eq!(rsf_vk_fsr3_swapchain_destroy(sw), 0);
            assert_eq!(take_log(), ["wsi_destroy"]);
            assert_eq!(LIVE_WSI.load(Ordering::SeqCst), 0);

            fail_next("fg_destroy", 12);
            assert_eq!(rsf_vk_fsr3_fg_destroy(fg), 12);
            assert_eq!(LIVE_FG.load(Ordering::SeqCst), 1);
            assert_eq!(rsf_vk_fsr3_fg_destroy(fg), 0);
            assert_eq!(take_log(), ["fg_destroy", "fg_destroy"]);
            assert_eq!(LIVE_FG.load(Ordering::SeqCst), 0);
        }
    }

    // One sequence: the SDK binding is process-wide.
    #[test]
    fn binds_once_then_owns_sr_fg_and_wsi() {
        let (mut width, mut height) = (0, 0);
        let mut context = ptr::null_mut();
        unsafe {
            // Unbound: every SDK-dependent entry refuses with -1.
            assert!(sdk().module.is_null());
            assert_eq!(rsf_vk_fsr3_plan(8, 8, 2, &mut width, &mut height), -1);
            assert_eq!(create_sr(&request(1, 8), &mut context), -1);
            assert_eq!(
                rsf_vk_fsr3_evaluate_with_composition(ptr::null_mut(), &frame(1.0, R8_UNORM)),
                -1
            );
            assert_eq!(rsf_vk_fsr3_destroy(ptr::null_mut()), 0);
            fg_wsi_unbound();

            assert_eq!(rsf_vk_fsr3_open(ptr::null_mut()), -1);
            assert_eq!(rsf_vk_fsr3_open(MISSING_CONFIGURE as Handle), -2);
            assert!(sdk().module.is_null());
            assert_eq!(rsf_vk_fsr3_open(0x1000 as Handle), 0);
            assert_eq!(rsf_vk_fsr3_open(0x3000 as Handle), -2);
            let bound = sdk();
            assert_eq!(bound.module as usize, 0x1000);
            assert_eq!(bound.configure, mark(5));

            assert_eq!(rsf_vk_fsr3_plan(8, 8, 2, &mut width, ptr::null_mut()), -1);
            assert_eq!(rsf_vk_fsr3_plan(8, 6, 2, &mut width, &mut height), 0);
            assert_eq!((width, height), (4, 3));

            // Refusals leave the output slot unwritten and free partial owners.
            assert_eq!(create_sr(&request(1, 0), &mut context), -1);
            assert_eq!(create_sr(&request(1, 8), ptr::null_mut()), -1);
            assert_eq!(create_sr(&request(NO_LOADER, 8), &mut context), -3);
            assert_eq!(
                create_sr(&request(NO_BUFFER_REQUIREMENTS, 8), &mut context),
                -3
            );
            assert_eq!(create_sr(&request(1, SDK_REFUSES), &mut context), 7);
            assert!(context.is_null());
            assert_eq!(LIVE.load(Ordering::SeqCst), 0);

            assert_eq!(create_sr(&request(1, 8), &mut context), 0);
            assert!(!context.is_null());
            assert_eq!(
                rsf_vk_fsr3_evaluate_with_composition(context, ptr::null()),
                -1
            );
            assert_eq!(
                rsf_vk_fsr3_evaluate_with_composition(context, &frame(1.0, 37)),
                -2
            );
            assert_eq!(EVALUATIONS.load(Ordering::SeqCst), 0);
            for (delta, expected) in [
                (2.5, 2.5),
                (0.0, DEFAULT_FRAME_TIME_MS),
                (-1.0, DEFAULT_FRAME_TIME_MS),
                (f32::NAN, DEFAULT_FRAME_TIME_MS),
            ] {
                assert_eq!(
                    rsf_vk_fsr3_evaluate_with_composition(context, &frame(delta, R8_UNORM)),
                    0
                );
                assert_eq!(
                    f32::from_bits(FRAME_TIME_BITS.load(Ordering::SeqCst)),
                    expected
                );
            }

            // A failed SDK release keeps the owner attached for a retry.
            FAIL_DESTROY.store(true, Ordering::SeqCst);
            assert_eq!(rsf_vk_fsr3_destroy(context), 5);
            assert_eq!(LIVE.load(Ordering::SeqCst), 1);
            FAIL_DESTROY.store(false, Ordering::SeqCst);
            assert_eq!(rsf_vk_fsr3_destroy(context), 0);
            assert_eq!(LIVE.load(Ordering::SeqCst), 0);
        }
        fg_wsi_bound();
    }
}
