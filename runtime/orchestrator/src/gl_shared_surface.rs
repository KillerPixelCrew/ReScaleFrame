// SPDX-License-Identifier: MIT
// OpenGL and D3D12 sharing owner behind rescaleframe/gl_shared_surface.h. This owner decides WGL
// owner admission, format validation, semaphore, handle and resource lifetime, command slot and
// fence synchronization, the GL/D3D12 queue handoff, failure recovery and teardown.
// gl_shared_surface.cpp supplies one native leaf per WGL, OpenGL, DXGI or D3D12 operation.
//
// Every transaction leaves GL through a semaphore signal and returns through a GL wait on a value
// the D3D12 queue signals after the transaction's commands. A queue signal that fails after
// ExecuteCommandLists leaves the transaction unsignalled: its slot, its resource leases and the GL
// acquire stay pending until a later queue signal succeeds. The executed list is never closed or
// submitted again, and GL never waits on a value that the queue has not been asked to signal.
use crate::com_ref::ComRef;
use crate::gl_pipeline_abi::{
    RSF_GL_R32F, RSF_GL_RG16F, RSF_GL_RGBA8, RSF_GL_RGBA16F, rsf_gl_setup, rsf_gl_texture_info,
};
use crate::render_abi::rsf_backend_log_fn;
use crate::sr_session::has_capacity;
use std::ffi::{CString, c_int, c_void};
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};
use std::sync::{Mutex, MutexGuard, TryLockError};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;

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

// DXGI formats and GL internal formats, pinned by static_asserts in gl_shared_surface.cpp.
const DXGI_R16G16B16A16_FLOAT: u32 = 10;
const DXGI_R16G16_FLOAT: u32 = 34;
const DXGI_R32_FLOAT: u32 = 41;
const DXGI_R8G8B8A8_UNORM: u32 = 28;
const GL_RGBA16F: u32 = 0x881a;
const GL_R32F: u32 = 0x822e;
const GL_RG16F: u32 = 0x822f;
const GL_RGBA8: u32 = 0x8058;

type Handle = *mut c_void;
type Log = rsf_backend_log_fn;

// Storage for the resolved GL extension entry points. Only the native leaves read it; its size
// matches `Gl` in gl_shared_surface.cpp.
#[repr(C)]
pub(crate) struct GlProcs([*const c_void; 13]);

// Field order matches `Leaves` in gl_shared_surface.cpp. GL leaves return glGetError after their
// call. D3D12 and DXGI leaves return an HRESULT; created COM interfaces carry one reference.
#[repr(C)]
pub(crate) struct Leaves {
    gl_current: unsafe extern "C" fn() -> Handle,
    gl_load: unsafe extern "C" fn(*mut GlProcs) -> c_int,
    gl_error: unsafe extern "C" fn() -> u32,
    gl_luid: unsafe extern "C" fn(*const GlProcs, *mut u8),
    semaphore_import: unsafe extern "C" fn(*const GlProcs, Handle, *mut u32) -> u32,
    semaphore_delete: unsafe extern "C" fn(*const GlProcs, u32),
    memory_import: unsafe extern "C" fn(*const GlProcs, Handle, *mut u32) -> u32,
    memory_delete: unsafe extern "C" fn(*const GlProcs, u32),
    texture_store: unsafe extern "C" fn(*const GlProcs, u32, u32, u32, u32, *mut u32) -> u32,
    texture_delete: unsafe extern "C" fn(u32),
    release: unsafe extern "C" fn(*const GlProcs, u32, u64, u32, *const u32, *const u32),
    acquire: unsafe extern "C" fn(*const GlProcs, u32, u64, u32, *const u32, *const u32),
    finish: unsafe extern "C" fn(),
    device_create: unsafe extern "C" fn(*const u8, *mut Handle) -> i32,
    queue_create: unsafe extern "C" fn(Handle, *mut Handle) -> i32,
    device_luid: unsafe extern "C" fn(Handle, *mut u8),
    queue_device: unsafe extern "C" fn(Handle, *mut Handle) -> i32,
    fence_create: unsafe extern "C" fn(Handle, *mut Handle) -> i32,
    shared_handle: unsafe extern "C" fn(Handle, Handle, *mut Handle) -> i32,
    event_create: unsafe extern "C" fn() -> Handle,
    handle_close: unsafe extern "C" fn(Handle),
    slot_create: unsafe extern "C" fn(Handle, *mut Handle, *mut Handle) -> i32,
    slot_reset: unsafe extern "C" fn(Handle, Handle) -> i32,
    list_close: unsafe extern "C" fn(Handle) -> i32,
    fence_completed: unsafe extern "C" fn(Handle) -> u64,
    fence_wait: unsafe extern "C" fn(Handle, u64, Handle, u32) -> c_int,
    queue_wait: unsafe extern "C" fn(Handle, Handle, u64) -> i32,
    queue_execute: unsafe extern "C" fn(Handle, Handle),
    queue_signal: unsafe extern "C" fn(Handle, Handle, u64) -> i32,
    resource_create: unsafe extern "C" fn(Handle, u32, u32, u32, *mut Handle) -> i32,
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

fn message(log: Log, user: Handle, text: &str) {
    if let (Some(log), Ok(text)) = (log, CString::new(text)) {
        unsafe { log(user, text.as_ptr()) };
    }
}

// An NT or event handle closed through its leaf.
struct NtHandle {
    leaves: &'static Leaves,
    handle: NonNull<c_void>,
}

impl NtHandle {
    fn new(leaves: &'static Leaves, handle: Handle) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self { leaves, handle })
    }

    fn get(&self) -> Handle {
        self.handle.as_ptr()
    }
}

impl Drop for NtHandle {
    fn drop(&mut self) {
        unsafe { (self.leaves.handle_close)(self.handle.as_ptr()) };
    }
}

// Textures named by one begin or submit call, validated against their owner.
#[derive(Clone, Copy)]
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
    allocator: ComRef,
    list: ComRef,
    // The fence value that proves this slot's last submission complete.
    completion: u64,
    // References to the resources its last transaction released to D3D12, held until completion is
    // proven. Preallocated for one batch.
    leases: Vec<ComRef>,
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
    Recording(usize),
    Unsignalled(Handoff),
}

struct State {
    serial: u64,
    slots: Vec<Slot>,
    next: usize,
    phase: Phase,
}

pub(crate) struct Context {
    leaves: &'static Leaves,
    gl: GlProcs,
    gl_context: Handle,
    thread: u32,
    log: Log,
    user: Handle,
    // Fields drop in declaration order, after the semaphore is deleted.
    state: Mutex<State>,
    semaphore: u32,
    _fence_handle: NtHandle,
    event: NtHandle,
    fence: ComRef,
    queue: ComRef,
    device: ComRef,
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe { (self.leaves.semaphore_delete)(&self.gl, self.semaphore) };
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
    resource: ComRef,
}

impl Context {
    // All calls run on the original WGL context and its thread.
    fn owner(&self) -> bool {
        self.thread == unsafe { GetCurrentThreadId() }
            && self.gl_context == unsafe { (self.leaves.gl_current)() }
    }

    fn lock(&self) -> Option<MutexGuard<'_, State>> {
        // Admission already pins the owner thread, so contention here is reentrancy.
        match self.state.try_lock() {
            Ok(state) => Some(state),
            Err(TryLockError::Poisoned(state)) => Some(state.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    fn message(&self, text: &str) {
        message(self.log, self.user, text);
    }

    fn failure(&self, operation: &str, code: u32) {
        self.message(&format!(
            "OpenGL sharing: {operation} failed (0x{code:08x})"
        ));
    }

    // The game owns the context too. A sticky error from its previous frame is reported
    // separately rather than attributed to the next interop import.
    fn prior_errors(&self) {
        for _ in 0..STICKY_ERRORS {
            let error = unsafe { (self.leaves.gl_error)() };
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
            batch.resources[i] = texture.resource.as_ptr();
        }
        Some(batch)
    }

    fn wait_cpu(&self, value: u64) -> bool {
        let l = self.leaves;
        unsafe {
            (l.fence_completed)(self.fence.as_ptr()) >= value
                || (l.fence_wait)(self.fence.as_ptr(), value, self.event.get(), CPU_WAIT_MS) != 0
        }
    }

    fn signal(&self, value: u64) -> i32 {
        unsafe { (self.leaves.queue_signal)(self.queue.as_ptr(), self.fence.as_ptr(), value) }
    }

    // GL releases the batch to D3D12 at `value` and flushes so the queue can observe it.
    fn release(&self, value: u64, batch: &Batch) {
        let layouts = [LAYOUT_GENERAL; BATCH];
        unsafe {
            (self.leaves.release)(
                &self.gl,
                self.semaphore,
                value,
                batch.count as u32,
                batch.names.as_ptr(),
                layouts.as_ptr(),
            )
        };
    }

    // GL reacquires the batch once the fence reaches `value`.
    fn acquire(&self, value: u64, batch: &Batch) {
        let layouts = [LAYOUT_GENERAL; BATCH];
        unsafe {
            (self.leaves.acquire)(
                &self.gl,
                self.semaphore,
                value,
                batch.count as u32,
                batch.names.as_ptr(),
                layouts.as_ptr(),
            )
        };
    }

    // Completes an unsignalled handoff with a queue signal ordered after its commands. Only then
    // does GL wait to reacquire the textures and the slot gain a completion value.
    fn settle(&self, state: &mut State) -> bool {
        let Phase::Unsignalled(handoff) = state.phase else {
            return true;
        };
        state.serial += 1;
        let complete = state.serial;
        let signalled = self.signal(complete);
        if failed(signalled) {
            self.failure("D3D12 completion signal", signalled as u32);
            return false;
        }
        self.acquire(complete, &handoff.batch);
        state.slots[handoff.slot].completion = complete;
        state.next = (handoff.slot + 1) % SLOTS;
        state.phase = Phase::Idle;
        true
    }

    fn begin(&self, state: &mut State, batch: &Batch, list: *mut Handle, slot: *mut u32) -> i32 {
        let l = self.leaves;
        if !self.settle(state) {
            return FEATURE_FAILED;
        }
        let index = state.next;
        if !self.wait_cpu(state.slots[index].completion) {
            return FEATURE_FAILED;
        }
        let current = &mut state.slots[index];
        current.leases.clear();
        if failed(unsafe { (l.slot_reset)(current.allocator.as_ptr(), current.list.as_ptr()) }) {
            return FEATURE_FAILED;
        }
        state.serial += 1;
        let ready = state.serial;
        self.release(ready, batch);
        let current = &mut state.slots[index];
        let waited = unsafe { (l.queue_wait)(self.queue.as_ptr(), self.fence.as_ptr(), ready) };
        if failed(waited) {
            self.failure("D3D12 queue wait", waited as u32);
            unsafe { (l.list_close)(current.list.as_ptr()) };
            // GL's own signal satisfies `ready`, so the textures return to GL unchanged.
            self.acquire(ready, batch);
            return FEATURE_FAILED;
        }
        for &resource in &batch.resources[..batch.count] {
            current.leases.push(unsafe { ComRef::retain(resource) });
        }
        state.phase = Phase::Recording(index);
        unsafe {
            list.write(current.list.as_ptr());
            slot.write(index as u32);
        }
        OK
    }

    fn submit(&self, state: &mut State, batch: &Batch) -> i32 {
        let l = self.leaves;
        let Phase::Recording(index) = state.phase else {
            return INVALID;
        };
        let list = state.slots[index].list.as_ptr();
        state.phase = Phase::Unsignalled(Handoff {
            slot: index,
            batch: *batch,
        });
        let closed = unsafe { (l.list_close)(list) };
        if failed(closed) {
            // Nothing executes. The queue still holds its wait for GL, so the handoff is
            // completed by a signal ordered after that wait.
            self.failure("D3D12 command list close", closed as u32);
            self.settle(state);
            return FEATURE_FAILED;
        }
        unsafe { (l.queue_execute)(self.queue.as_ptr(), list) };
        if self.settle(state) {
            OK
        } else {
            FEATURE_FAILED
        }
    }

    fn drain(&self, state: &mut State) -> i32 {
        if matches!(state.phase, Phase::Recording(_)) {
            return INVALID;
        }
        if !self.settle(state) {
            return FEATURE_FAILED;
        }
        unsafe { (self.leaves.finish)() };
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

fn open(setup: &rsf_gl_setup, leaves: &'static Leaves, gl_context: Handle) -> Result<Context, i32> {
    let l = leaves;
    let say = |text: &str| message(setup.log, setup.user, text);
    let mut gl = GlProcs([ptr::null(); 13]);
    if unsafe { (l.gl_load)(&mut gl) } == 0 {
        say("OpenGL sharing requires EXT_memory_object_win32 and EXT_semaphore_win32.");
        return Err(NOT_SUPPORTED);
    }
    let mut luid = [0u8; 8];
    unsafe { (l.gl_luid)(&gl, luid.as_mut_ptr()) };
    if unsafe { (l.gl_error)() } != GL_NO_ERROR {
        return Err(FEATURE_FAILED);
    }
    let (device, queue) = if !setup.device.is_null() {
        let device = unsafe { ComRef::retain(setup.device) };
        let queue = unsafe { ComRef::retain(setup.queue) };
        let mut native = [0u8; 8];
        unsafe { (l.device_luid)(device.as_ptr(), native.as_mut_ptr()) };
        let mut owner = ptr::null_mut();
        let found = unsafe { (l.queue_device)(queue.as_ptr(), &mut owner) };
        let owner = unsafe { ComRef::from_owned(owner) };
        if luid != native || failed(found) || owner.is_none_or(|owner| owner.0 != device.0) {
            return Err(INVALID);
        }
        (device, queue)
    } else {
        let mut device = ptr::null_mut();
        let created = unsafe { (l.device_create)(luid.as_ptr(), &mut device) };
        let device = unsafe { ComRef::from_owned(device) }
            .filter(|_| !failed(created))
            .ok_or(NOT_SUPPORTED)?;
        let mut queue = ptr::null_mut();
        let created = unsafe { (l.queue_create)(device.as_ptr(), &mut queue) };
        let queue = unsafe { ComRef::from_owned(queue) }
            .filter(|_| !failed(created))
            .ok_or(FEATURE_FAILED)?;
        (device, queue)
    };
    let mut fence = ptr::null_mut();
    let created = unsafe { (l.fence_create)(device.as_ptr(), &mut fence) };
    let fence = unsafe { ComRef::from_owned(fence) }
        .filter(|_| !failed(created))
        .ok_or(FEATURE_FAILED)?;
    let mut shared = ptr::null_mut();
    let created = unsafe { (l.shared_handle)(device.as_ptr(), fence.as_ptr(), &mut shared) };
    let fence_handle = NtHandle::new(l, shared)
        .filter(|_| !failed(created))
        .ok_or(FEATURE_FAILED)?;
    let event = NtHandle::new(l, unsafe { (l.event_create)() }).ok_or(FEATURE_FAILED)?;
    let mut slots = Vec::with_capacity(SLOTS);
    for _ in 0..SLOTS {
        let (mut allocator, mut list) = (ptr::null_mut(), ptr::null_mut());
        let created = unsafe { (l.slot_create)(device.as_ptr(), &mut allocator, &mut list) };
        let allocator = unsafe { ComRef::from_owned(allocator) };
        let list = unsafe { ComRef::from_owned(list) };
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
    let imported = unsafe { (l.semaphore_import)(&gl, fence_handle.get(), &mut semaphore) };
    if imported != GL_NO_ERROR {
        unsafe { (l.semaphore_delete)(&gl, semaphore) };
        return Err(FEATURE_FAILED);
    }
    say("OpenGL sharing: exact context adapter, D3D12 resource memory, shared GPU timeline fence.");
    Ok(Context {
        leaves,
        gl,
        gl_context,
        thread: unsafe { GetCurrentThreadId() },
        log: setup.log,
        user: setup.user,
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
    let l = owner.leaves;
    owner.prior_errors();
    let (native, internal) = native_format(format).ok_or(INVALID)?;
    let mut resource = ptr::null_mut();
    let created =
        unsafe { (l.resource_create)(owner.device.as_ptr(), width, height, native, &mut resource) };
    let resource = unsafe { ComRef::from_owned(resource) };
    let Some(resource) = resource.filter(|_| !failed(created)) else {
        owner.failure("shared D3D12 resource", created as u32);
        return Err(FEATURE_FAILED);
    };
    let mut shared = ptr::null_mut();
    let created =
        unsafe { (l.shared_handle)(owner.device.as_ptr(), resource.as_ptr(), &mut shared) };
    let handle = NtHandle::new(l, shared)
        .filter(|_| !failed(created))
        .ok_or(FEATURE_FAILED)?;
    let mut memory = 0;
    let imported = unsafe { (l.memory_import)(&owner.gl, handle.get(), &mut memory) };
    if imported != GL_NO_ERROR {
        owner.failure("D3D12 memory import", imported);
        unsafe { (l.memory_delete)(&owner.gl, memory) };
        return Err(FEATURE_FAILED);
    }
    let mut texture = 0;
    let stored =
        unsafe { (l.texture_store)(&owner.gl, memory, internal, width, height, &mut texture) };
    if stored != GL_NO_ERROR {
        owner.failure("shared texture storage", stored);
        unsafe {
            (l.texture_delete)(texture);
            (l.memory_delete)(&owner.gl, memory);
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
        return INVALID;
    };
    let Some(mut state) = context.lock() else {
        return NOT_READY;
    };
    run(context, &mut state)
}

/// # Safety
/// `setup` is null or readable for its `struct_size`; `out` is null or writable. `leaves` is the
/// static table from gl_shared_surface.cpp.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_gl_open(
    setup: *const rsf_gl_setup,
    out: *mut *mut Context,
    leaves: *const Leaves,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    let Some(leaves) = (unsafe { leaves.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { setup.read() };
    let gl_context = unsafe { (leaves.gl_current)() };
    if gl_context.is_null() || setup.device.is_null() != setup.queue.is_null() {
        return INVALID;
    }
    if setup.abi_version != SETUP_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let storage = crate::allocate_handle(MaybeUninit::<Context>::uninit());
    if storage.is_null() {
        return FEATURE_FAILED;
    }
    match open(&setup, leaves, gl_context) {
        Ok(context) => {
            unsafe {
                storage.cast::<Context>().write(context);
                out.write(storage.cast());
            }
            OK
        }
        Err(result) => {
            unsafe { drop(Box::from_raw(storage)) };
            result
        }
    }
}

/// Borrows the D3D12 device and queue; no reference is added.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_graphics(
    context: *mut Context,
    device: *mut Handle,
    queue: *mut Handle,
) -> i32 {
    let Some(context) = (unsafe { context.as_ref() }).filter(|context| context.owner()) else {
        return INVALID;
    };
    if device.is_null() || queue.is_null() {
        return INVALID;
    }
    unsafe {
        device.write(context.device.as_ptr());
        queue.write(context.queue.as_ptr());
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_texture_create(
    context: *mut Context,
    width: u32,
    height: u32,
    format: u32,
    out: *mut *mut Texture,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    let Some(context) = (unsafe { context.as_ref() }).filter(|context| context.owner()) else {
        return INVALID;
    };
    if width == 0 || height == 0 {
        return INVALID;
    }
    let storage = crate::allocate_handle(MaybeUninit::<Texture>::uninit());
    if storage.is_null() {
        return FEATURE_FAILED;
    }
    match create_texture(context, width, height, format) {
        Ok(texture) => {
            unsafe {
                storage.cast::<Texture>().write(texture);
                out.write(storage.cast());
            }
            OK
        }
        Err(result) => {
            unsafe { drop(Box::from_raw(storage)) };
            result
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_texture_get(
    texture: *mut Texture,
    info: *mut rsf_gl_texture_info,
) -> i32 {
    let Some(texture) = (unsafe { texture.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(info) } {
        return INVALID;
    }
    unsafe {
        info.write(rsf_gl_texture_info {
            struct_size: size_of::<rsf_gl_texture_info>() as u32,
            texture: texture.texture,
            width: texture.width,
            height: texture.height,
            format: texture.format,
            resource: texture.resource.as_ptr(),
        })
    };
    OK
}

/// Releases the textures to D3D12 and returns an open list. Submit even when recording refuses.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_begin(
    context: *mut Context,
    count: u32,
    textures: *const *mut Texture,
    list: *mut Handle,
    slot: *mut u32,
) -> i32 {
    if list.is_null() || slot.is_null() {
        return INVALID;
    }
    with_state(context, |context, state| {
        if matches!(state.phase, Phase::Recording(_)) {
            return INVALID;
        }
        unsafe { list.write(ptr::null_mut()) };
        match context.batch(count, textures) {
            Some(batch) => context.begin(state, &batch, list, slot),
            None => INVALID,
        }
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_submit(
    context: *mut Context,
    count: u32,
    textures: *const *mut Texture,
) -> i32 {
    with_state(context, |context, state| {
        if !matches!(state.phase, Phase::Recording(_)) {
            return INVALID;
        }
        match context.batch(count, textures) {
            Some(batch) => context.submit(state, &batch),
            None => INVALID,
        }
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_drain(context: *mut Context) -> i32 {
    with_state(context, |context, state| context.drain(state))
}

/// Destroys a texture on its owner's thread. A texture still named by an unsignalled handoff
/// leaves that GL acquire; its D3D12 resource stays leased until completion is proven.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_texture_destroy(texture: *mut Texture) {
    let Some(value) = (unsafe { texture.as_ref() }) else {
        return;
    };
    let Some(owner) = (unsafe { value.owner.as_ref() }).filter(|owner| owner.owner()) else {
        return;
    };
    let Some(mut state) = owner.lock() else {
        return;
    };
    if let Phase::Unsignalled(handoff) = &mut state.phase {
        handoff.batch.forget(value.texture);
    }
    drop(state);
    let l = owner.leaves;
    unsafe {
        (l.texture_delete)(value.texture);
        (l.memory_delete)(&owner.gl, value.memory);
        drop(Box::from_raw(texture));
    }
}

/// Drains, then releases the semaphore, handles and D3D12 objects. A refused drain keeps
/// everything and the close can be retried.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_close(context: *mut Context) -> i32 {
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
