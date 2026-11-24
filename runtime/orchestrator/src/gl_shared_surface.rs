// SPDX-License-Identifier: MIT
// OpenGL and D3D12 sharing owner behind rescaleframe/gl_shared_surface.h. This owner decides WGL
// owner admission, format validation, semaphore, handle and resource lifetime, command slot and
// fence synchronization, the GL/D3D12 queue handoff, failure recovery and teardown.
// Its native leaves (native_dx.rs, native_gl.rs) use typed WGL, OpenGL, DXGI and D3D12 calls.
//
// Every transaction leaves GL through a semaphore signal and returns through a GL wait on a value
// the D3D12 queue signals after the transaction's commands. A queue signal that fails after
// ExecuteCommandLists leaves the transaction unsignalled: its slot, its resource leases and the GL
// acquire stay pending until a later queue signal succeeds. The executed list is never closed or
// submitted again, and GL never waits on a value that the queue has not been asked to signal.
use crate::com_ref;
use crate::d3d12_fence::FenceState;
use crate::sr_session::has_capacity;
use crate::sys::{
    RSF_GL_R32F, RSF_GL_RG16F, RSF_GL_RGBA8, RSF_GL_RGBA16F, rsf_gl_setup, rsf_gl_texture_info,
};
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INVALID_ARGUMENT, NOT_READY, NOT_SUPPORTED, OK,
};
use rsf_graphics::dump_options::Log;
use std::ffi::{CStr, c_void};
use std::ptr::{self, NonNull};
use std::sync::{Mutex, MutexGuard};
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

// GL operations return glGetError after their call. D3D12 and DXGI calls return an HRESULT.
// A returned COM interface transfers one reference. Tests swap in fakes of the same names.
#[cfg(not(test))]
mod native_dx;
#[cfg(not(test))]
mod native_gl;
#[cfg(not(test))]
use native_dx as native;
#[cfg(test)]
use tests::native;

const SETUP_ABI_VERSION: u32 = 1;
const SLOTS: usize = 3;
// Textures per transaction.
const BATCH: usize = 16;
const CPU_WAIT_MS: u32 = 10_000;
const GL_NO_ERROR: u32 = 0;
// GL error flags are finite; a context that keeps reporting is not drained further.
const STICKY_ERRORS: usize = 32;
// GL_LAYOUT_GENERAL_EXT. Resources enter and leave D3D12 in COMMON.
const LAYOUT_GENERAL: u32 = 0x958d;

// DXGI formats and GL internal formats.
const DXGI_R16G16B16A16_FLOAT: u32 = 10;
const DXGI_R16G16_FLOAT: u32 = 34;
const DXGI_R32_FLOAT: u32 = 41;
const DXGI_R8G8B8A8_UNORM: u32 = 28;
const GL_RGBA16F: u32 = 0x881a;
const GL_R32F: u32 = 0x822e;
const GL_RG16F: u32 = 0x822f;
const GL_RGBA8: u32 = 0x8058;
const FEATURE_LEVEL_11_0: u32 = 0xb000;

// native_gl converts these entries to their typed OpenGL signatures at the call site.
const GL_ENTRY_POINTS: [&CStr; 13] = [
    c"glGetStringi",
    c"glGetUnsignedBytevEXT",
    c"glCreateMemoryObjectsEXT",
    c"glDeleteMemoryObjectsEXT",
    c"glMemoryObjectParameterivEXT",
    c"glImportMemoryWin32HandleEXT",
    c"glTexStorageMem2DEXT",
    c"glGenSemaphoresEXT",
    c"glDeleteSemaphoresEXT",
    c"glImportSemaphoreWin32HandleEXT",
    c"glSemaphoreParameterui64vEXT",
    c"glSignalSemaphoreEXT",
    c"glWaitSemaphoreEXT",
];

type Handle = *mut c_void;

// Native GL leaves and the fixture borrow the resolved entry points.
#[repr(C)]
pub(crate) struct GlProcs([*const c_void; GL_ENTRY_POINTS.len()]);

impl GlProcs {
    fn load() -> Option<Self> {
        let mut gl = Self([ptr::null(); GL_ENTRY_POINTS.len()]);
        for (slot, name) in gl.0.iter_mut().zip(GL_ENTRY_POINTS) {
            let entry = unsafe { native::gl_lookup(name.as_ptr()) };
            // Drivers can report a WGL miss as null, 1, 2, 3 or -1.
            if entry.addr() <= 3 || entry.addr() == usize::MAX {
                return None;
            }
            *slot = entry;
        }
        for name in [c"GL_EXT_memory_object_win32", c"GL_EXT_semaphore_win32"] {
            let count = native::gl_extension_count();
            let found = (0..count).any(|index| {
                let entry = unsafe { native::gl_extension_at(&gl, index as u32) };
                !entry.is_null() && unsafe { CStr::from_ptr(entry) } == name
            });
            if !found {
                return None;
            }
        }
        Some(gl)
    }
}

fn failed(result: i32) -> bool {
    result < 0
}

// The D3D12 and GL formats behind each public RSF_GL_* format.
fn native_format(format: u32) -> Option<(u32, u32)> {
    match format {
        RSF_GL_RGBA16F => Some((DXGI_R16G16B16A16_FLOAT, GL_RGBA16F)),
        RSF_GL_R32F => Some((DXGI_R32_FLOAT, GL_R32F)),
        RSF_GL_RG16F => Some((DXGI_R16G16_FLOAT, GL_RG16F)),
        RSF_GL_RGBA8 => Some((DXGI_R8G8B8A8_UNORM, GL_RGBA8)),
        _ => None,
    }
}

// An NT or event handle closed through its leaf.
struct NtHandle {
    handle: NonNull<c_void>,
}

impl NtHandle {
    fn new(handle: Handle) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self { handle })
    }

    fn get(&self) -> Handle {
        self.handle.as_ptr()
    }
}

impl Drop for NtHandle {
    fn drop(&mut self) {
        unsafe { native::handle_close(self.handle.as_ptr()) };
    }
}

// Textures named by one begin or submit call, validated against their owner.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Batch {
    names: [u32; BATCH],
    resources: [Handle; BATCH],
    count: usize,
}

impl Batch {
    // Drops a destroyed texture from a pending GL acquire. Its D3D12 resource stays leased.
    fn forget(&mut self, name: u32) {
        let mut kept = 0;
        for i in 0..self.count {
            if self.names[i] != name {
                self.names[kept] = self.names[i];
                self.resources[kept] = self.resources[i];
                kept += 1;
            }
        }
        self.count = kept;
    }
}

struct Slot {
    allocator: IUnknown,
    list: IUnknown,
    // The fence value that proves this slot's last submission complete.
    completion: u64,
    // References to the resources its last transaction released to D3D12, held until completion is
    // proven. Preallocated for one batch.
    leases: Vec<IUnknown>,
}

// A transaction whose queue work has ended without a successful completion signal. GL still waits
// to reacquire its textures, and its slot cannot be reset.
#[derive(Clone, Copy)]
struct Handoff {
    slot: usize,
    batch: Batch,
}

#[derive(Clone, Copy)]
enum Phase {
    Idle,
    Recording(Handoff),
    Unsignalled(Handoff),
    AwaitingAcquire {
        handoff: Handoff,
        value: u64,
        submitted: bool,
    },
    ReleaseFailed,
}

struct State {
    serial: u64,
    slots: Vec<Slot>,
    next: usize,
    phase: Phase,
}

pub(crate) struct Context {
    gl: GlProcs,
    gl_context: Handle,
    thread: u32,
    log: Log,
    // Fields drop in declaration order, after the semaphore is deleted.
    state: Mutex<State>,
    semaphore: u32,
    _fence_handle: NtHandle,
    event: NtHandle,
    fence: IUnknown,
    queue: IUnknown,
    device: IUnknown,
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe { native::semaphore_delete(&self.gl, self.semaphore) };
    }
}

pub(crate) struct Texture {
    // Not retained: textures are destroyed before their context closes.
    owner: *const Context,
    texture: u32,
    memory: u32,
    width: u32,
    height: u32,
    format: u32,
    // Kept through the imported object's lifetime; some drivers import on their GL server thread
    // after the creating call returns. Closed before the resource reference is released.
    _handle: NtHandle,
    resource: IUnknown,
}

impl Context {
    // All calls run on the original WGL context and its thread.
    fn owner(&self) -> bool {
        self.thread == unsafe { GetCurrentThreadId() } && self.gl_context == native::gl_current()
    }

    fn lock(&self) -> Option<MutexGuard<'_, State>> {
        // Admission already pins the owner thread, so contention here is reentrancy.
        crate::sync::try_lock(&self.state)
    }

    fn failure(&self, operation: &str, code: u32) {
        self.log.say(&format!(
            "OpenGL sharing: {operation} failed (0x{code:08x})"
        ));
    }

    // The game owns the context too. A sticky error from its previous frame is reported
    // separately rather than attributed to the next interop import.
    fn prior_errors(&self) {
        for _ in 0..STICKY_ERRORS {
            let error = native::gl_error();
            if error == GL_NO_ERROR {
                break;
            }
            self.failure("pre-existing GL operation before interop", error);
        }
    }

    fn batch(&self, count: u32, textures: *const *mut Texture) -> Option<Batch> {
        let count = count as usize;
        if count > BATCH || (count != 0 && textures.is_null()) {
            return None;
        }
        let mut batch = Batch {
            names: [0; BATCH],
            resources: [ptr::null_mut(); BATCH],
            count,
        };
        for i in 0..count {
            let texture = unsafe { textures.add(i).read().as_ref() }?;
            if !ptr::eq(texture.owner, self) {
                return None;
            }
            batch.names[i] = texture.texture;
            batch.resources[i] = texture.resource.as_raw();
        }
        Some(batch)
    }

    fn wait_cpu(&self, value: u64) -> bool {
        if value == 0 {
            return true;
        }
        let completed = unsafe { native::fence_completed(self.fence.as_raw()) };
        match FenceState::check(completed, value) {
            FenceState::Complete => true,
            FenceState::DeviceRemoved => false,
            FenceState::Pending => unsafe {
                native::fence_wait(self.fence.as_raw(), value, self.event.get(), CPU_WAIT_MS) != 0
            },
        }
    }

    fn signal(&self, value: u64) -> i32 {
        unsafe { native::queue_signal(self.queue.as_raw(), self.fence.as_raw(), value) }
    }

    // GL releases the batch to D3D12 at `value` and flushes so the queue can observe it.
    fn release(&self, value: u64, batch: &Batch) -> u32 {
        self.prior_errors();
        let layouts = [LAYOUT_GENERAL; BATCH];
        unsafe {
            native::release(
                &self.gl,
                self.semaphore,
                value,
                batch.count as u32,
                batch.names.as_ptr(),
                layouts.as_ptr(),
            )
        }
    }

    // GL reacquires the batch once the fence reaches `value`.
    fn acquire(&self, value: u64, batch: &Batch) -> u32 {
        self.prior_errors();
        let layouts = [LAYOUT_GENERAL; BATCH];
        unsafe {
            native::acquire(
                &self.gl,
                self.semaphore,
                value,
                batch.count as u32,
                batch.names.as_ptr(),
                layouts.as_ptr(),
            )
        }
    }

    // Completes an unsignalled handoff with a queue signal ordered after its commands. Only then
    // does GL wait to reacquire the textures and the slot gain a completion value.
    fn settle(&self, state: &mut State) -> bool {
        if let Phase::Unsignalled(handoff) = state.phase {
            state.serial += 1;
            let value = state.serial;
            let signalled = self.signal(value);
            if failed(signalled) {
                self.failure("D3D12 completion signal", signalled as u32);
                return false;
            }
            state.phase = Phase::AwaitingAcquire {
                handoff,
                value,
                submitted: true,
            };
        }
        let Phase::AwaitingAcquire {
            handoff,
            value,
            submitted,
        } = state.phase
        else {
            return matches!(state.phase, Phase::Idle);
        };
        let result = self.acquire(value, &handoff.batch);
        if result != GL_NO_ERROR {
            self.failure("GL semaphore acquire", result);
            return false;
        }
        if submitted {
            state.slots[handoff.slot].completion = value;
            state.next = (handoff.slot + 1) % SLOTS;
        }
        state.phase = Phase::Idle;
        true
    }

    fn begin(&self, state: &mut State, batch: &Batch, list: *mut Handle, slot: *mut u32) -> i32 {
        if !self.settle(state) {
            return FEATURE_FAILED;
        }
        let index = state.next;
        if !self.wait_cpu(state.slots[index].completion) {
            return FEATURE_FAILED;
        }
        let current = &mut state.slots[index];
        current.leases.clear();
        if failed(unsafe { native::slot_reset(current.allocator.as_raw(), current.list.as_raw()) })
        {
            return FEATURE_FAILED;
        }
        state.serial += 1;
        let ready = state.serial;
        let released = self.release(ready, batch);
        let current = &mut state.slots[index];
        if released != GL_NO_ERROR {
            unsafe { native::list_close(current.list.as_raw()) };
            state.phase = Phase::ReleaseFailed;
            self.failure("GL semaphore release", released);
            return FEATURE_FAILED;
        }
        let handoff = Handoff {
            slot: index,
            batch: *batch,
        };
        let waited = unsafe { native::queue_wait(self.queue.as_raw(), self.fence.as_raw(), ready) };
        if failed(waited) {
            self.failure("D3D12 queue wait", waited as u32);
            unsafe { native::list_close(current.list.as_raw()) };
            // GL's own signal satisfies `ready`, so the textures return to GL unchanged.
            state.phase = Phase::AwaitingAcquire {
                handoff,
                value: ready,
                submitted: false,
            };
            self.settle(state);
            return FEATURE_FAILED;
        }
        for &resource in &batch.resources[..batch.count] {
            current.leases.push(unsafe { com_ref::retain(resource) });
        }
        state.phase = Phase::Recording(handoff);
        unsafe {
            list.write(current.list.as_raw());
            slot.write(index as u32);
        }
        OK
    }

    fn submit(&self, state: &mut State, batch: &Batch) -> i32 {
        let Phase::Recording(handoff) = state.phase else {
            return INVALID_ARGUMENT;
        };
        if handoff.batch != *batch {
            return INVALID_ARGUMENT;
        }
        let list = state.slots[handoff.slot].list.as_raw();
        state.phase = Phase::Unsignalled(handoff);
        let closed = unsafe { native::list_close(list) };
        if failed(closed) {
            // Nothing executes. The queue still holds its wait for GL, so the handoff is
            // completed by a signal ordered after that wait.
            self.failure("D3D12 command list close", closed as u32);
            self.settle(state);
            return FEATURE_FAILED;
        }
        unsafe { native::queue_execute(self.queue.as_raw(), list) };
        if self.settle(state) {
            OK
        } else {
            FEATURE_FAILED
        }
    }

    fn drain(&self, state: &mut State) -> i32 {
        if matches!(state.phase, Phase::Recording(_)) {
            return INVALID_ARGUMENT;
        }
        // A refused GL release never installed a D3D12 wait. Drain can retire the
        // resources, but the context cannot record another ownership transfer.
        if !matches!(state.phase, Phase::ReleaseFailed) && !self.settle(state) {
            return FEATURE_FAILED;
        }
        native::finish();
        state.serial += 1;
        let complete = state.serial;
        if failed(self.signal(complete)) || !self.wait_cpu(complete) {
            return FEATURE_FAILED;
        }
        for slot in &mut state.slots {
            slot.leases.clear();
        }
        OK
    }
}

fn create_context_device(luid: &[u8; 8]) -> Result<IUnknown, i32> {
    let mut factory = ptr::null_mut();
    let result = unsafe { native::factory_create(&mut factory) };
    let factory = unsafe { com_ref::owned(factory) }
        .filter(|_| !failed(result))
        .ok_or(NOT_SUPPORTED)?;

    let mut adapter = ptr::null_mut();
    let result = unsafe { native::adapter_find(factory.as_raw(), luid.as_ptr(), &mut adapter) };
    let adapter = unsafe { com_ref::owned(adapter) }
        .filter(|_| !failed(result))
        .ok_or(NOT_SUPPORTED)?;

    let mut device = ptr::null_mut();
    let result =
        unsafe { native::device_create(adapter.as_raw(), FEATURE_LEVEL_11_0, &mut device) };
    unsafe { com_ref::owned(device) }
        .filter(|_| !failed(result))
        .ok_or(NOT_SUPPORTED)
}

fn open(setup: &rsf_gl_setup, gl_context: Handle) -> Result<Context, i32> {
    let log = Log::new(setup.log, setup.user);
    let say = |text: &str| log.say(text);
    let Some(gl) = GlProcs::load() else {
        say("OpenGL sharing requires EXT_memory_object_win32 and EXT_semaphore_win32.");
        return Err(NOT_SUPPORTED);
    };
    let mut luid = [0u8; 8];
    unsafe { native::gl_luid(&gl, luid.as_mut_ptr()) };
    if native::gl_error() != GL_NO_ERROR {
        return Err(FEATURE_FAILED);
    }
    let (device, queue) = if !setup.device.is_null() {
        let device = unsafe { com_ref::retain(setup.device) };
        let queue = unsafe { com_ref::retain(setup.queue) };
        let mut native = [0u8; 8];
        unsafe { native::device_luid(device.as_raw(), native.as_mut_ptr()) };
        let mut owner = ptr::null_mut();
        let found = unsafe { native::queue_device(queue.as_raw(), &mut owner) };
        let owner = unsafe { com_ref::owned(owner) };
        if luid != native || failed(found) || owner.is_none_or(|owner| owner != device) {
            return Err(INVALID_ARGUMENT);
        }
        (device, queue)
    } else {
        let device = create_context_device(&luid)?;
        let mut queue = ptr::null_mut();
        let created = unsafe { native::queue_create(device.as_raw(), &mut queue) };
        let queue = unsafe { com_ref::owned(queue) }
            .filter(|_| !failed(created))
            .ok_or(FEATURE_FAILED)?;
        (device, queue)
    };
    let mut fence = ptr::null_mut();
    let created = unsafe { native::fence_create(device.as_raw(), &mut fence) };
    let fence = unsafe { com_ref::owned(fence) }
        .filter(|_| !failed(created))
        .ok_or(FEATURE_FAILED)?;
    let mut shared = ptr::null_mut();
    let created = unsafe { native::shared_handle(device.as_raw(), fence.as_raw(), &mut shared) };
    let fence_handle = NtHandle::new(shared)
        .filter(|_| !failed(created))
        .ok_or(FEATURE_FAILED)?;
    let event = NtHandle::new(unsafe { native::event_create() }).ok_or(FEATURE_FAILED)?;
    let mut slots = Vec::with_capacity(SLOTS);
    for _ in 0..SLOTS {
        let (mut allocator, mut list) = (ptr::null_mut(), ptr::null_mut());
        let created = unsafe { native::slot_create(device.as_raw(), &mut allocator, &mut list) };
        let allocator = unsafe { com_ref::owned(allocator) };
        let list = unsafe { com_ref::owned(list) };
        let (Some(allocator), Some(list)) = (allocator, list) else {
            return Err(FEATURE_FAILED);
        };
        if failed(created) {
            return Err(FEATURE_FAILED);
        }
        slots.push(Slot {
            allocator,
            list,
            completion: 0,
            leases: Vec::with_capacity(BATCH),
        });
    }
    // The semaphore imports last, so every earlier failure leaves no GL object behind.
    let mut semaphore = 0;
    let imported = unsafe { native::semaphore_import(&gl, fence_handle.get(), &mut semaphore) };
    if imported != GL_NO_ERROR {
        unsafe { native::semaphore_delete(&gl, semaphore) };
        return Err(FEATURE_FAILED);
    }
    say("OpenGL sharing: exact context adapter, D3D12 resource memory, shared GPU timeline fence.");
    Ok(Context {
        gl,
        gl_context,
        thread: unsafe { GetCurrentThreadId() },
        log,
        state: Mutex::new(State {
            serial: 0,
            slots,
            next: 0,
            phase: Phase::Idle,
        }),
        semaphore,
        _fence_handle: fence_handle,
        event,
        fence,
        queue,
        device,
    })
}

fn create_texture(owner: &Context, width: u32, height: u32, format: u32) -> Result<Texture, i32> {
    owner.prior_errors();
    let (native, internal) = native_format(format).ok_or(INVALID_ARGUMENT)?;
    let mut resource = ptr::null_mut();
    let created = unsafe {
        native::resource_create(owner.device.as_raw(), width, height, native, &mut resource)
    };
    let resource = unsafe { com_ref::owned(resource) };
    let Some(resource) = resource.filter(|_| !failed(created)) else {
        owner.failure("shared D3D12 resource", created as u32);
        return Err(FEATURE_FAILED);
    };
    let mut shared = ptr::null_mut();
    let created =
        unsafe { native::shared_handle(owner.device.as_raw(), resource.as_raw(), &mut shared) };
    let handle = NtHandle::new(shared)
        .filter(|_| !failed(created))
        .ok_or(FEATURE_FAILED)?;
    let mut memory = 0;
    let imported = unsafe { native::memory_import(&owner.gl, handle.get(), &mut memory) };
    if imported != GL_NO_ERROR {
        owner.failure("D3D12 memory import", imported);
        unsafe { native::memory_delete(&owner.gl, memory) };
        return Err(FEATURE_FAILED);
    }
    let mut texture = 0;
    let stored =
        unsafe { native::texture_store(&owner.gl, memory, internal, width, height, &mut texture) };
    if stored != GL_NO_ERROR {
        owner.failure("shared texture storage", stored);
        unsafe {
            native::texture_delete(texture);
            native::memory_delete(&owner.gl, memory);
        }
        return Err(FEATURE_FAILED);
    }
    Ok(Texture {
        owner,
        texture,
        memory,
        width,
        height,
        format,
        _handle: handle,
        resource,
    })
}

// Runs an owner-thread call with the synchronization state.
fn with_state(context: *mut Context, run: impl FnOnce(&Context, &mut State) -> i32) -> i32 {
    let Some(context) = (unsafe { context.as_ref() }).filter(|context| context.owner()) else {
        return INVALID_ARGUMENT;
    };
    let Some(mut state) = context.lock() else {
        return NOT_READY;
    };
    run(context, &mut state)
}

/// # Safety
/// `setup` is null or readable for its `struct_size` and `out` is null or writable.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_open(
    setup: *const rsf_gl_setup,
    out: *mut *mut Context,
) -> i32 {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { setup.read() };
    let gl_context = native::gl_current();
    if gl_context.is_null() || setup.device.is_null() != setup.queue.is_null() {
        return INVALID_ARGUMENT;
    }
    if setup.abi_version != SETUP_ABI_VERSION {
        return ABI_MISMATCH;
    }
    crate::initialize_handle(unsafe { &mut *out }, FEATURE_FAILED, || {
        match open(&setup, gl_context) {
            Ok(context) => (OK, Some(context)),
            Err(result) => (result, None),
        }
    })
}

/// Borrows the D3D12 device and queue; no reference is added.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_graphics(
    context: *mut Context,
    device: *mut Handle,
    queue: *mut Handle,
) -> i32 {
    let Some(context) = (unsafe { context.as_ref() }).filter(|context| context.owner()) else {
        return INVALID_ARGUMENT;
    };
    if device.is_null() || queue.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe {
        device.write(context.device.as_raw());
        queue.write(context.queue.as_raw());
    }
    OK
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_texture_create(
    context: *mut Context,
    width: u32,
    height: u32,
    format: u32,
    out: *mut *mut Texture,
) -> i32 {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    let Some(context) = (unsafe { context.as_ref() }).filter(|context| context.owner()) else {
        return INVALID_ARGUMENT;
    };
    if width == 0 || height == 0 {
        return INVALID_ARGUMENT;
    }
    crate::initialize_handle(
        unsafe { &mut *out },
        FEATURE_FAILED,
        || match create_texture(context, width, height, format) {
            Ok(texture) => (OK, Some(texture)),
            Err(result) => (result, None),
        },
    )
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_texture_get(
    texture: *mut Texture,
    info: *mut rsf_gl_texture_info,
) -> i32 {
    let Some(texture) = (unsafe { texture.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(info) } {
        return INVALID_ARGUMENT;
    }
    unsafe {
        info.write(rsf_gl_texture_info {
            struct_size: size_of::<rsf_gl_texture_info>() as u32,
            texture: texture.texture,
            width: texture.width,
            height: texture.height,
            format: texture.format,
            resource: texture.resource.as_raw(),
        })
    };
    OK
}

/// Releases the textures to D3D12 and returns an open list. Submit even when recording refuses.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_begin(
    context: *mut Context,
    count: u32,
    textures: *const *mut Texture,
    list: *mut Handle,
    slot: *mut u32,
) -> i32 {
    if list.is_null() || slot.is_null() {
        return INVALID_ARGUMENT;
    }
    with_state(context, |context, state| {
        if matches!(state.phase, Phase::Recording(_)) {
            return INVALID_ARGUMENT;
        }
        unsafe { list.write(ptr::null_mut()) };
        match context.batch(count, textures) {
            Some(batch) => context.begin(state, &batch, list, slot),
            None => INVALID_ARGUMENT,
        }
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_submit(
    context: *mut Context,
    count: u32,
    textures: *const *mut Texture,
) -> i32 {
    with_state(context, |context, state| {
        if !matches!(state.phase, Phase::Recording(_)) {
            return INVALID_ARGUMENT;
        }
        match context.batch(count, textures) {
            Some(batch) => context.submit(state, &batch),
            None => INVALID_ARGUMENT,
        }
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_drain(context: *mut Context) -> i32 {
    with_state(context, |context, state| context.drain(state))
}

/// Destroys a texture on its owner's thread. A texture still named by an unsignalled handoff
/// leaves that GL acquire; its D3D12 resource stays leased until completion is proven.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_texture_destroy(texture: *mut Texture) {
    let Some(value) = (unsafe { texture.as_ref() }) else {
        return;
    };
    let Some(owner) = (unsafe { value.owner.as_ref() }).filter(|owner| owner.owner()) else {
        return;
    };
    let Some(mut state) = owner.lock() else {
        return;
    };
    if matches!(state.phase, Phase::Recording(handoff)
        if handoff.batch.names[..handoff.batch.count].contains(&value.texture))
    {
        return;
    }
    match &mut state.phase {
        Phase::Unsignalled(handoff) | Phase::AwaitingAcquire { handoff, .. } => {
            handoff.batch.forget(value.texture);
        }
        _ => {}
    }
    drop(state);
    unsafe {
        native::texture_delete(value.texture);
        native::memory_delete(&owner.gl, value.memory);
        drop(Box::from_raw(texture));
    }
}

/// Drains, then releases the semaphore, handles and D3D12 objects. A refused drain keeps
/// everything and the close can be retried.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_gl_close(context: *mut Context) -> i32 {
    if context.is_null() {
        return OK;
    }
    let drained = unsafe { rsf_gl_drain(context) };
    if drained != OK {
        return drained;
    }
    unsafe { drop(Box::from_raw(context)) };
    OK
}

#[cfg(test)]
mod tests;
