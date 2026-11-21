// SPDX-License-Identifier: MIT
//! Rust owner for the standalone VulkanStoryFsr3.dll runtime binding and SR context.
//!
//! The managed `Fsr3Native` caller loads the signed FidelityFX SDK 1.1.4 Vulkan
//! runtime and this bridge, then opens the bridge with the SDK module. Rust
//! retains that binding for the process, admits SR creation, planning and
//! evaluation, and owns checked SR retirement. Typed FidelityFX descriptors,
//! the Vulkan loader lookup and the resource mapping stay native behind the
//! private table in `bridge_platform.h`. Frame generation and WSI stay native
//! and read the same bound SDK table through `rsf_core_fsr3_sdk`.

use std::alloc::{Layout, alloc, dealloc};
use std::ffi::{CStr, c_char, c_void};
use std::mem::offset_of;
use std::ptr::{self, NonNull};
use std::sync::OnceLock;

type Handle = *mut c_void;

const PLATFORM_ABI_VERSION: u32 = 1;
const VULKAN_LOADER: u32 = 1 << 0;
const VULKAN_BUFFER_REQUIREMENTS: u32 = 1 << 1;
const VULKAN_READY: u32 = VULKAN_LOADER | VULKAN_BUFFER_REQUIREMENTS;

// VkFormat values the SR path accepts.
const R8_UNORM: i32 = 9;
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

/// Matches `rsf_fsr3_sdk`. Entries are opaque here; only native leaves call them.
// Native frame generation reads `configure`; Rust never does.
#[allow(dead_code)]
#[derive(Clone, Copy)]
#[repr(C)]
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

/// Matches `VulkanStoryFsr3Image`.
// Layout mirror: the native resource mapping reads the handle and extent.
#[allow(dead_code)]
#[repr(C)]
struct Image {
    image: u64,
    width: u32,
    height: u32,
    format: i32,
}

/// Matches `VulkanStoryFsr3Frame` and the managed `Fsr3Frame`.
// Layout mirror: the native dispatch leaf reads the commands and constants.
#[allow(dead_code)]
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

    fn frame_time_ms(&self) -> f32 {
        // NaN and non-positive deltas take the default, as `x > 0 ? x : d` did.
        if self.delta_ms > 0.0 {
            self.delta_ms
        } else {
            DEFAULT_FRAME_TIME_MS
        }
    }
}

/// Matches `rsf_fsr3sr_create`.
#[repr(C)]
struct Request {
    device: Handle,
    physical: Handle,
    render_width: u32,
    render_height: u32,
    width: u32,
    height: u32,
}

/// Opaque `rsf_fsr3sr_native` capsule.
#[repr(C)]
struct Native {
    _private: [u8; 0],
}

/// Matches `rsf_fsr3sr_platform`.
#[repr(C)]
struct Platform {
    struct_size: u32,
    abi_version: u32,
    resolve: unsafe extern "C" fn(Handle, *const c_char) -> Handle,
    allocate: unsafe extern "C" fn() -> *mut Native,
    release: unsafe extern "C" fn(*mut Native),
    bind_vulkan: unsafe extern "C" fn(Handle) -> u32,
    create: unsafe extern "C" fn(*mut Native, Handle, *const Request) -> i32,
    plan: unsafe extern "C" fn(Handle, u32, u32, u32, *mut u32, *mut u32) -> i32,
    evaluate: unsafe extern "C" fn(*mut Native, Handle, *const Frame, f32) -> i32,
    has_effect: unsafe extern "C" fn(*const Native) -> u32,
    destroy: unsafe extern "C" fn(*mut Native, Handle) -> i32,
}

const _: () = {
    assert!(size_of::<Image>() == 24);
    assert!(size_of::<Frame>() == 184);
    assert!(offset_of!(Frame, jitter_x) == 152);
    assert!(offset_of!(Frame, composition) == 128);
    assert!(size_of::<Sdk>() == 48 && offset_of!(Sdk, dispatch) == 32);
    assert!(size_of::<Request>() == 32 && offset_of!(Request, render_width) == 16);
    assert!(size_of::<Platform>() == 8 + 9 * size_of::<usize>());
    assert!(offset_of!(Platform, resolve) == 8);
    assert!(offset_of!(Platform, create) == 40);
    assert!(offset_of!(Platform, destroy) == 72);
};

/// # Safety
/// A non-null table must stay valid for the process lifetime.
unsafe fn platform(ops: *const Platform) -> Option<&'static Platform> {
    let ops = unsafe { ops.as_ref() }?;
    (ops.struct_size as usize >= size_of::<Platform>() && ops.abi_version == PLATFORM_ABI_VERSION)
        .then_some(ops)
}

/// The exported `VulkanStoryFsr3Context`. The host serializes calls on one
/// context and retires its Vulkan work before destruction.
struct Context {
    ops: &'static Platform,
    native: NonNull<Native>,
}

/// Returns the bound SDK table, or an all-null table before a successful open.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_fsr3_sdk() -> *const Sdk {
    sdk()
}

/// Binds the five FidelityFX exports from a borrowed runtime module.
///
/// One runtime identity is retained; the caller keeps the module loaded.
/// Returns 0 on success or for the same module, -1 for null, -2 for another
/// runtime or a missing export. Missing exports leave the bridge unbound.
///
/// # Safety
/// `ops` is the static platform table; `runtime` is null or a loaded module.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr3_open(ops: *const Platform, runtime: Handle) -> i32 {
    if runtime.is_null() {
        return -1;
    }
    let same = |bound: &Sdk| if bound.module == runtime { 0 } else { -2 };
    if let Some(bound) = SDK.get() {
        return same(bound);
    }
    let Some(ops) = (unsafe { platform(ops) }) else {
        return -1;
    };
    let mut entries = [ptr::null_mut(); EXPORTS.len()];
    for (entry, name) in entries.iter_mut().zip(EXPORTS) {
        *entry = unsafe { (ops.resolve)(runtime, name.as_ptr()) };
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
/// `ops` is the static platform table; non-null outputs must be writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr3_plan(
    ops: *const Platform,
    display_width: u32,
    display_height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    let query = sdk().query;
    if query.is_null() || render_width.is_null() || render_height.is_null() {
        return -1;
    }
    let Some(ops) = (unsafe { platform(ops) }) else {
        return -1;
    };
    unsafe {
        (ops.plan)(
            query,
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
/// `ops` is the static platform table; `request` and `result` are null or valid.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr3_create(
    ops: *const Platform,
    request: *const Request,
    result: *mut *mut Context,
) -> i32 {
    let create = sdk().create;
    let Some(request) = (unsafe { request.as_ref() }) else {
        return -1;
    };
    if create.is_null()
        || result.is_null()
        || request.device.is_null()
        || request.physical.is_null()
        || request.render_width == 0
        || request.render_height == 0
        || request.width == 0
        || request.height == 0
    {
        return -1;
    }
    let Some(ops) = (unsafe { platform(ops) }) else {
        return -1;
    };
    let Some(native) = NonNull::new(unsafe { (ops.allocate)() }) else {
        return -2;
    };
    let context = unsafe { alloc(Layout::new::<Context>()) }.cast::<Context>();
    if context.is_null() {
        unsafe { (ops.release)(native.as_ptr()) };
        return -2;
    }
    let failed = |code: i32| {
        unsafe {
            (ops.release)(native.as_ptr());
            dealloc(context.cast(), Layout::new::<Context>());
        }
        code
    };
    let vulkan = unsafe { (ops.bind_vulkan)(request.device) };
    if vulkan & VULKAN_READY != VULKAN_READY {
        return failed(-3);
    }
    let code = unsafe { (ops.create)(native.as_ptr(), create, request) };
    if code != 0 {
        return failed(code);
    }
    unsafe {
        context.write(Context { ops, native });
        result.write(context);
    }
    0
}

/// Records SR with independent R8 reactive and composition masks.
///
/// Returns the SDK result, -1 for a missing context, frame or dispatch, and
/// -2 when a required image format differs.
///
/// # Safety
/// Non-null pointers must be a live context and a readable 184-byte frame.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr3_evaluate(context: *mut Context, frame: *const Frame) -> i32 {
    let dispatch = sdk().dispatch;
    let (Some(context), Some(frame)) = (unsafe { context.as_ref() }, unsafe { frame.as_ref() })
    else {
        return -1;
    };
    if dispatch.is_null() {
        return -1;
    }
    if !frame.formats_supported() {
        return -2;
    }
    unsafe {
        (context.ops.evaluate)(
            context.native.as_ptr(),
            dispatch,
            frame,
            frame.frame_time_ms(),
        )
    }
}

/// Destroys the SR effect, then releases the owner after SDK success.
///
/// The caller first completes Vulkan work that references the effect. Returns
/// 0 for null or success, -1 when a live effect has no destroy export, or the
/// SDK release result; a nonzero result keeps the context attached.
///
/// # Safety
/// `context` is null or a live context from `rsf_core_fsr3_create`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr3_destroy(context: *mut Context) -> i32 {
    let Some(owner) = (unsafe { context.as_ref() }) else {
        return 0;
    };
    let destroy = sdk().destroy;
    let native = owner.native.as_ptr();
    let effect = unsafe { (owner.ops.has_effect)(native) } != 0;
    if effect && destroy.is_null() {
        return -1;
    }
    let code = if effect {
        unsafe { (owner.ops.destroy)(native, destroy) }
    } else {
        0
    };
    if code != 0 {
        return code;
    }
    unsafe {
        (owner.ops.release)(native);
        drop(Box::from_raw(context));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
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

    unsafe extern "C" fn resolve(module: Handle, name: *const c_char) -> Handle {
        let name = unsafe { CStr::from_ptr(name) };
        if module as usize == MISSING_CONFIGURE && name == c"ffxConfigure" {
            return ptr::null_mut();
        }
        let index = EXPORTS.iter().position(|export| *export == name).unwrap();
        mark(index + 1)
    }

    unsafe extern "C" fn allocate() -> *mut Native {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Box::into_raw(Box::new(Fake { effect: false })).cast()
    }

    unsafe extern "C" fn release(native: *mut Native) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
        drop(unsafe { Box::from_raw(native.cast::<Fake>()) });
    }

    unsafe extern "C" fn bind_vulkan(device: Handle) -> u32 {
        match device as usize {
            NO_LOADER => 0,
            NO_BUFFER_REQUIREMENTS => VULKAN_LOADER,
            _ => VULKAN_LOADER | VULKAN_BUFFER_REQUIREMENTS,
        }
    }

    unsafe extern "C" fn create(
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

    unsafe extern "C" fn plan(
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

    unsafe extern "C" fn evaluate(
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

    unsafe extern "C" fn has_effect(native: *const Native) -> u32 {
        u32::from(unsafe { &*native.cast::<Fake>() }.effect)
    }

    unsafe extern "C" fn destroy(native: *mut Native, destroy: Handle) -> i32 {
        assert_eq!(destroy, mark(2));
        if FAIL_DESTROY.load(Ordering::SeqCst) {
            return 5;
        }
        unsafe { &mut *native.cast::<Fake>() }.effect = false;
        0
    }

    static PLATFORM: Platform = Platform {
        struct_size: size_of::<Platform>() as u32,
        abi_version: PLATFORM_ABI_VERSION,
        resolve,
        allocate,
        release,
        bind_vulkan,
        create,
        plan,
        evaluate,
        has_effect,
        destroy,
    };

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

    // One sequence: the SDK binding is process-wide.
    #[test]
    fn binds_once_then_owns_sr_admission_and_checked_retirement() {
        let ops = &raw const PLATFORM;
        let (mut width, mut height) = (0, 0);
        let mut context = ptr::null_mut();
        unsafe {
            // Unbound: every SDK-dependent entry refuses with -1.
            assert!(rsf_core_fsr3_sdk().read().module.is_null());
            assert_eq!(
                rsf_core_fsr3_plan(ops, 8, 8, 2, &mut width, &mut height),
                -1
            );
            assert_eq!(rsf_core_fsr3_create(ops, &request(1, 8), &mut context), -1);
            assert_eq!(
                rsf_core_fsr3_evaluate(ptr::null_mut(), &frame(1.0, R8_UNORM)),
                -1
            );
            assert_eq!(rsf_core_fsr3_destroy(ptr::null_mut()), 0);

            assert_eq!(rsf_core_fsr3_open(ops, ptr::null_mut()), -1);
            assert_eq!(rsf_core_fsr3_open(ops, MISSING_CONFIGURE as Handle), -2);
            assert!(rsf_core_fsr3_sdk().read().module.is_null());
            assert_eq!(rsf_core_fsr3_open(ptr::null(), 0x1000 as Handle), -1);
            assert_eq!(rsf_core_fsr3_open(ops, 0x1000 as Handle), 0);
            assert_eq!(rsf_core_fsr3_open(ptr::null(), 0x1000 as Handle), 0);
            assert_eq!(rsf_core_fsr3_open(ops, 0x3000 as Handle), -2);
            let bound = rsf_core_fsr3_sdk().read();
            assert_eq!(bound.module as usize, 0x1000);
            assert_eq!(bound.configure, mark(5));

            assert_eq!(
                rsf_core_fsr3_plan(ops, 8, 8, 2, &mut width, ptr::null_mut()),
                -1
            );
            assert_eq!(rsf_core_fsr3_plan(ops, 8, 6, 2, &mut width, &mut height), 0);
            assert_eq!((width, height), (4, 3));

            // Refusals leave the output slot unwritten and free partial owners.
            assert_eq!(rsf_core_fsr3_create(ops, &request(1, 0), &mut context), -1);
            assert_eq!(rsf_core_fsr3_create(ops, ptr::null(), &mut context), -1);
            assert_eq!(
                rsf_core_fsr3_create(ops, &request(1, 8), ptr::null_mut()),
                -1
            );
            assert_eq!(
                rsf_core_fsr3_create(ptr::null(), &request(1, 8), &mut context),
                -1
            );
            assert_eq!(
                rsf_core_fsr3_create(ops, &request(NO_LOADER, 8), &mut context),
                -3
            );
            assert_eq!(
                rsf_core_fsr3_create(ops, &request(NO_BUFFER_REQUIREMENTS, 8), &mut context),
                -3
            );
            assert_eq!(
                rsf_core_fsr3_create(ops, &request(1, SDK_REFUSES), &mut context),
                7
            );
            assert!(context.is_null());
            assert_eq!(LIVE.load(Ordering::SeqCst), 0);

            assert_eq!(rsf_core_fsr3_create(ops, &request(1, 8), &mut context), 0);
            assert!(!context.is_null());
            assert_eq!(rsf_core_fsr3_evaluate(context, ptr::null()), -1);
            assert_eq!(rsf_core_fsr3_evaluate(context, &frame(1.0, 37)), -2);
            assert_eq!(EVALUATIONS.load(Ordering::SeqCst), 0);
            for (delta, expected) in [
                (2.5, 2.5),
                (0.0, DEFAULT_FRAME_TIME_MS),
                (-1.0, DEFAULT_FRAME_TIME_MS),
                (f32::NAN, DEFAULT_FRAME_TIME_MS),
            ] {
                assert_eq!(rsf_core_fsr3_evaluate(context, &frame(delta, R8_UNORM)), 0);
                assert_eq!(
                    f32::from_bits(FRAME_TIME_BITS.load(Ordering::SeqCst)),
                    expected
                );
            }

            // A failed SDK release keeps the owner attached for a retry.
            FAIL_DESTROY.store(true, Ordering::SeqCst);
            assert_eq!(rsf_core_fsr3_destroy(context), 5);
            assert_eq!(LIVE.load(Ordering::SeqCst), 1);
            FAIL_DESTROY.store(false, Ordering::SeqCst);
            assert_eq!(rsf_core_fsr3_destroy(context), 0);
            assert_eq!(LIVE.load(Ordering::SeqCst), 0);
        }
    }
}
