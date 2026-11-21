// SPDX-License-Identifier: MIT
//! Shared session owner for the integrated Runtime and standalone XeSS-FG DLL.

use std::alloc::{Layout, alloc};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::sync::atomic::{AtomicI32, Ordering};

type Handle = *mut c_void;
type Step = unsafe extern "C" fn(Handle) -> i32;

#[repr(C)]
struct Modules {
    fg: Handle,
    xell: Handle,
}

#[repr(C)]
struct Platform {
    struct_size: u32,
    abi_version: u32,
    allocate: unsafe extern "C" fn(*mut Session) -> Handle,
    release_native: unsafe extern "C" fn(Handle),
    load_runtime: unsafe extern "C" fn(Handle, *const u16, *mut Modules) -> i32,
    unload_module: unsafe extern "C" fn(Handle),
    create_factory: Step,
    find_adapter: unsafe extern "C" fn(Handle, u64) -> i32,
    create_device: Step,
    create_queue: Step,
    create_xell: Step,
    create_fg: unsafe extern "C" fn(Handle, Handle) -> i32,
    set_latency: Step,
    set_default_sleep_mode: Step,
    log_create_error: unsafe extern "C" fn(*const c_char, i32, i32),
    init_swapchain: unsafe extern "C" fn(Handle, Handle, u32, u32, u32, u32) -> i32,
    query_limit: unsafe extern "C" fn(Handle, *mut u32) -> i32,
    get_swapchain: Step,
    enable_ui: Step,
    create_completion_fence: Step,
    open_event: unsafe extern "C" fn() -> Handle,
    create_recorders: Step,
    start_capture: unsafe extern "C" fn(Handle, Handle) -> i32,
    set_generated: unsafe extern "C" fn(Handle, u32) -> i32,
    read_completion: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    arm_completion: unsafe extern "C" fn(Handle, u64, Handle) -> i32,
    wait_event: unsafe extern "C" fn(Handle, u32) -> i32,
    disable_fg: Step,
    flush_capture: Step,
    release_swapchain: unsafe extern "C" fn(Handle),
    destroy_fg: Step,
    destroy_xell: Step,
}

const _: () = {
    assert!(size_of::<Modules>() == 16);
    assert!(size_of::<Platform>() == 248 && align_of::<Platform>() == 8);
    assert!(std::mem::offset_of!(Platform, allocate) == 8);
    assert!(std::mem::offset_of!(Platform, init_swapchain) == 112);
    assert!(std::mem::offset_of!(Platform, read_completion) == 184);
    assert!(std::mem::offset_of!(Platform, destroy_xell) == 240);
};

const ABI_VERSION: u32 = 1;
const INVALID_ARGUMENT: i32 = -10001;
const ABI_MISMATCH: i32 = -10002;
const WAIT_MS: u32 = 10_000;
const WAIT_INCOMPLETE: i32 = -3;

thread_local! {
    static CREATE_STAGE: Cell<&'static CStr> = const { Cell::new(c"not attempted") };
}

#[derive(PartialEq, Eq)]
enum Phase {
    Creating,
    Ready,
    Starting,
    PresenterReady,
    Started,
    Retiring,
    Prepared,
}

// The host serializes startup, presentation-side calls and retirement. The
// native XeLL sleep and marker paths borrow their typed bindings independently.
struct Session {
    platform: &'static Platform,
    native: Handle,
    modules: Modules,
    event: Option<OwnedHandle>,
    max_generated: u32,
    phase: Phase,
    failure: AtomicI32,
}

impl Session {
    fn create_step(&self, stage: &'static CStr, step: Step) -> i32 {
        CREATE_STAGE.set(stage);
        unsafe { step(self.native) }
    }

    fn create(&mut self, luid: u64, directory: *const u16) -> i32 {
        let ops = self.platform;
        let native = self.native;
        CREATE_STAGE.set(c"runtime DLL/export loading");
        let code = unsafe { (ops.load_runtime)(native, directory, &mut self.modules) };
        if code != 0 {
            return code;
        }
        let code = self.create_step(c"CreateDXGIFactory2", ops.create_factory);
        if code != 0 {
            return code;
        }
        CREATE_STAGE.set(c"EnumAdapterByLuid");
        let code = unsafe { (ops.find_adapter)(native, luid) };
        if code != 0 {
            return code;
        }
        for (stage, step) in [
            (c"D3D12CreateDevice", ops.create_device),
            (c"CreateCommandQueue", ops.create_queue),
            (c"xellD3D12CreateContext", ops.create_xell),
        ] {
            let code = self.create_step(stage, step);
            if code != 0 {
                return code;
            }
        }
        CREATE_STAGE.set(c"xefgSwapChainD3D12CreateContext");
        let code = unsafe { (ops.create_fg)(native, self.modules.fg) };
        if code != 0 {
            return code;
        }
        let code = self.create_step(c"xefgSwapChainSetLatencyReduction", ops.set_latency);
        if code != 0 {
            return code;
        }
        self.create_step(c"xellSetSleepMode", ops.set_default_sleep_mode)
    }

    fn start(
        &mut self,
        window: Handle,
        width: u32,
        height: u32,
        inverted_depth: u32,
        generated: u32,
        unsupported: i32,
    ) -> i32 {
        let integrated = generated != 0;
        if window.is_null() || width == 0 || height == 0 || self.phase != Phase::Ready {
            return if integrated { INVALID_ARGUMENT } else { -1 };
        }
        let ops = self.platform;
        let native = self.native;
        if integrated {
            let mut max = 0;
            let code = unsafe { (ops.query_limit)(native, &mut max) };
            if code < 0 {
                return code;
            }
            if generated > max {
                return unsupported;
            }
        }
        // Once the SDK is given the HWND, a partial start must be retired.
        self.phase = Phase::Starting;
        let code = unsafe {
            (ops.init_swapchain)(
                native,
                window,
                width,
                height,
                u32::from(integrated),
                inverted_depth,
            )
        };
        if code < 0 {
            return code;
        }
        let code = unsafe { (ops.query_limit)(native, &mut self.max_generated) };
        if code < 0 || self.max_generated == 0 {
            return if code < 0 { code } else { -4 };
        }
        for step in [
            ops.get_swapchain,
            ops.enable_ui,
            ops.create_completion_fence,
        ] {
            let code = unsafe { step(native) };
            if code < 0 {
                return code;
            }
        }
        // CreateEventW transfers a Win32 event reference and returns null on failure.
        let event = unsafe { (ops.open_event)() };
        if event.is_null() {
            return -3;
        }
        self.event = Some(unsafe { OwnedHandle::from_raw_handle(event) });
        let code = unsafe { (ops.create_recorders)(native) };
        if code < 0 {
            return code;
        }
        self.phase = Phase::PresenterReady;
        let code = unsafe { (ops.start_capture)(native, self.modules.fg) };
        if code < 0 {
            return code;
        }
        let code = if integrated {
            unsafe { (ops.set_generated)(native, generated) }
        } else {
            0
        };
        if code >= 0 {
            self.phase = Phase::Started;
        }
        code
    }

    fn event(&self) -> Handle {
        self.event
            .as_ref()
            .map_or(ptr::null_mut(), AsRawHandle::as_raw_handle)
    }

    fn retain_failure(&self, code: i32) -> i32 {
        self.failure
            .compare_exchange(0, code, Ordering::Relaxed, Ordering::Relaxed)
            .map_or_else(|first| first, |_| code)
    }

    fn read_completion(&self) -> Result<u64, i32> {
        let mut completed = 0;
        let code = unsafe { (self.platform.read_completion)(self.native, &mut completed) };
        if code != 0 {
            return Err(self.retain_failure(code));
        }
        Ok(completed)
    }

    fn wait(&self, target: u64) -> Result<(), i32> {
        let failure = self.failure.load(Ordering::Relaxed);
        if failure != 0 {
            return Err(failure);
        }
        if target == 0 || self.read_completion()? >= target {
            return Ok(());
        }
        let event = self.event();
        if event.is_null() {
            return Err(-2);
        }
        let code = unsafe { (self.platform.arm_completion)(self.native, target, event) };
        if code < 0 {
            return Err(code);
        }
        let code = unsafe { (self.platform.wait_event)(event, WAIT_MS) };
        if code != 0 || self.read_completion()? < target {
            return Err(WAIT_INCOMPLETE);
        }
        Ok(())
    }

    fn retire_consumers(&mut self, target: u64) -> Result<(), i32> {
        let ops = self.platform;
        let native = self.native;
        let disable = matches!(self.phase, Phase::PresenterReady | Phase::Started);
        self.phase = Phase::Retiring;
        if disable {
            let code = unsafe { (ops.disable_fg)(native) };
            if code < 0 {
                return Err(code);
            }
        }
        self.wait(target)?;
        let code = unsafe { (ops.flush_capture)(native) };
        if code != 0 {
            return Err(code);
        }
        // The SDK refuses destruction with an outstanding DXGI wrapper reference.
        unsafe { (ops.release_swapchain)(native) };
        for step in [ops.destroy_fg, ops.destroy_xell] {
            let code = unsafe { step(native) };
            if code < 0 {
                return Err(code);
            }
        }
        Ok(())
    }

    fn prepare_destroy(&mut self, target: u64) -> i32 {
        let failure = self.failure.load(Ordering::Relaxed);
        if failure != 0 || self.phase == Phase::Prepared {
            return failure;
        }
        match self.retire_consumers(target) {
            Ok(()) => {
                self.phase = Phase::Prepared;
                0
            }
            Err(code) => self.retain_failure(code),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.native.is_null() {
            unsafe { (self.platform.release_native)(self.native) };
        }
        drop(self.event.take());
        // COM and the native callback storage are gone before the SDK DLLs unload.
        for module in [self.modules.fg, self.modules.xell] {
            if !module.is_null() {
                unsafe { (self.platform.unload_module)(module) };
            }
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_bridge_create(
    platform: *const Platform,
    luid: u64,
    directory: *const u16,
    output: *mut Handle,
) -> i32 {
    CREATE_STAGE.set(c"argument validation");
    if output.is_null() || luid == 0 {
        return -1;
    }
    unsafe { output.write(ptr::null_mut()) };
    if platform.is_null() || unsafe { (*platform).struct_size } < size_of::<Platform>() as u32 {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*platform).abi_version } != ABI_VERSION {
        return ABI_MISMATCH;
    }
    // Both C++ consumers lend a static table with every callback populated.
    let platform = unsafe { &*platform };
    CREATE_STAGE.set(c"native context allocation");
    let session = unsafe { alloc(Layout::new::<Session>()).cast::<Session>() };
    if session.is_null() {
        return -2;
    }
    unsafe {
        session.write(Session {
            platform,
            native: ptr::null_mut(),
            modules: Modules {
                fg: ptr::null_mut(),
                xell: ptr::null_mut(),
            },
            event: None,
            max_generated: 0,
            phase: Phase::Creating,
            failure: AtomicI32::new(0),
        });
    }
    let native = unsafe { (platform.allocate)(session) };
    if native.is_null() {
        drop(unsafe { Box::from_raw(session) });
        return -2;
    }
    let owner = unsafe { &mut *session };
    owner.native = native;
    let code = owner.create(luid, directory);
    if code != 0 {
        let stage = CREATE_STAGE.get().as_ptr();
        unsafe { (platform.log_create_error)(stage, code, 0) };
        let cleanup = owner.prepare_destroy(0);
        if cleanup == 0 {
            drop(unsafe { Box::from_raw(session) });
        } else {
            unsafe { output.write(native) };
            unsafe { (platform.log_create_error)(stage, code, cleanup) };
        }
        return code;
    }
    owner.phase = Phase::Ready;
    unsafe { output.write(native) };
    CREATE_STAGE.set(c"ready");
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_create_stage() -> *const c_char {
    CREATE_STAGE.get().as_ptr()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_note_vulkan_lookup() {
    CREATE_STAGE.set(c"Vulkan adapter LUID lookup");
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_start(
    owner: &mut Session,
    window: Handle,
    width: u32,
    height: u32,
    inverted_depth: u32,
    generated: u32,
    unsupported: i32,
) -> i32 {
    owner.start(
        window,
        width,
        height,
        inverted_depth,
        generated,
        unsupported,
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_started(owner: &Session) -> u32 {
    u32::from(owner.phase == Phase::Started)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_max_generated(owner: &Session) -> u32 {
    owner.max_generated
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_event(owner: &Session) -> Handle {
    owner.event()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_wait(owner: &Session, target: u64) -> i32 {
    owner.wait(target).err().unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_retain_failure(owner: &Session, code: i32) {
    owner.retain_failure(code);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_bridge_prepare_destroy(owner: &mut Session, target: u64) -> i32 {
    owner.prepare_destroy(target)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_bridge_destroy(session: *mut Session, target: u64) -> i32 {
    let code = unsafe { &mut *session }.prepare_destroy(target);
    if code == 0 {
        drop(unsafe { Box::from_raw(session) });
    }
    code
}
