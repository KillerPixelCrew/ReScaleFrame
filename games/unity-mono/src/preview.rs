// SPDX-License-Identifier: MIT
//! Window admission, graphics ownership and queue retirement for the Unity Editor preview.
//!
//! The preview copies the window pass's image onto its own DXGI swapchain, recorded on the D3D12
//! queue the package bridge lends. This module owns the window record and one graphics record:
//! one reference each to that device and queue, the preview fence, the swapchain and, per ring
//! slot, the command allocator, the command list and the source texture its last submission copied
//! from. It also owns the submission ring, the fault that keeps everything after a refused signal,
//! the drained proof and the checked release.
//!
//! integrations/unity/native/preview.cpp keeps the typed Win32, D3D12 and DXGI calls as leaves
//! (`preview_platform.h`), which never keep a reference past their return. The graphics leaves run
//! under the graphics mutex, which every render-thread caller only tries to take.
//!
//! The C++ kept the device, queue, fence, swapchain and slots as unguarded `ComPtr` globals,
//! released them without any check of its own and read the fence for the drained check outside
//! any lock.
use crate::bridge_abi::RSF_UNITY_FRAMES_SLOTS;
use crate::bridge_interop::Interop;
use rsf_game_sys::rsf_game_render_pass;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

const R8G8B8A8_TYPELESS: u32 = 27;
const R8G8B8A8_UNORM: u32 = 28;
const R8G8B8A8_UNORM_SRGB: u32 = 29;
const SLOTS: usize = RSF_UNITY_FRAMES_SLOTS as usize;
// record results, as preview_platform.h defines them.
const RECORD_REFUSED: u32 = 0;
const RECORD_BROKEN: u32 = 1;
const RECORD_CLOSED: u32 = 2;

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct Texture {
    width: u64,
    height: u32,
    format: u32,
}

/// Transferred `ID3D12Device`, `ID3D12CommandQueue`, `ID3D12Fence` and `IDXGISwapChain3`
/// references. The first three are bound together; the swapchain follows.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Gpu {
    device: *mut c_void,
    queue: *mut c_void,
    fence: *mut c_void,
    chain: *mut c_void,
}

impl Gpu {
    const EMPTY: Self = Self {
        device: ptr::null_mut(),
        queue: ptr::null_mut(),
        fence: ptr::null_mut(),
        chain: ptr::null_mut(),
    };
}

/// One ring slot's transferred allocator and command list, created on the slot's first use, and
/// the source texture its last submission copied from.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot {
    allocator: *mut c_void,
    list: *mut c_void,
    source: *mut c_void,
}

impl Slot {
    const EMPTY: Self = Self {
        allocator: ptr::null_mut(),
        list: ptr::null_mut(),
        source: ptr::null_mut(),
    };

    /// Drops the slot in the order of the C++ `slot = {}` assignment: allocator, list, source.
    fn clear(self, leaves: &Leaves) {
        unref(leaves, self.allocator);
        unref(leaves, self.list);
        unref(leaves, self.source);
    }

    /// Drops a refused fresh slot in the order the C++ local was destroyed: list, allocator.
    fn discard(self, leaves: &Leaves) {
        unref(leaves, self.list);
        unref(leaves, self.allocator);
    }
}

type Candidate = unsafe extern "C" fn(*mut c_void, *mut c_void, *const Rect);

#[repr(C)]
struct Leaves {
    enumerate: unsafe extern "C" fn(*mut c_void, Candidate),
    create_window: unsafe extern "C" fn(*mut c_void, i32, i32) -> *mut c_void,
    position: unsafe extern "C" fn(*mut c_void, *mut c_void, i32, i32, i32, i32) -> bool,
    show: unsafe extern "C" fn(*mut c_void, bool, bool),
    bind_gpu: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut Gpu) -> bool,
    create_chain: unsafe extern "C" fn(*const Gpu, *mut c_void, u32, u32, *mut *mut c_void) -> bool,
    chain_extent: unsafe extern "C" fn(*mut c_void, *mut u32, *mut u32) -> bool,
    resize_chain: unsafe extern "C" fn(*mut c_void, u32, u32) -> bool,
    describe: unsafe extern "C" fn(*mut c_void, *mut Texture),
    completed: unsafe extern "C" fn(*mut c_void) -> u64,
    wait: unsafe extern "C" fn(*mut c_void, u64) -> bool,
    acquire: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> bool,
    create_slot: unsafe extern "C" fn(*mut c_void, *mut Slot) -> bool,
    record: unsafe extern "C" fn(*const Slot, *mut c_void, *mut c_void, bool) -> u32,
    execute: unsafe extern "C" fn(*mut c_void, *mut c_void),
    signal: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> bool,
    present: unsafe extern "C" fn(*mut c_void, *const rsf_game_render_pass) -> bool,
    retain: unsafe extern "C" fn(*mut c_void),
    release: unsafe extern "C" fn(*mut c_void),
}

const _: () = {
    assert!(size_of::<Rect>() == 16 && size_of::<Texture>() == 16);
    assert!(size_of::<Gpu>() == 32 && size_of::<Slot>() == 24);
    assert!(std::mem::offset_of!(Gpu, chain) == 24 && std::mem::offset_of!(Slot, source) == 16);
    assert!(size_of::<Leaves>() == 19 * size_of::<usize>());
};

/// Drops one transferred reference. Null holds none.
fn unref(leaves: &Leaves, object: *mut c_void) {
    if !object.is_null() {
        unsafe { (leaves.release)(object) };
    }
}

#[derive(Clone, Copy)]
struct Window {
    parent: usize,
    handle: usize,
    visible: bool,
}

static WINDOW: Mutex<Window> = Mutex::new(Window {
    parent: 0,
    handle: 0,
    visible: false,
});
static BASE_FRAMES: AtomicU64 = AtomicU64::new(0);

fn window() -> MutexGuard<'static, Window> {
    WINDOW.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Search {
    requested: Rect,
    parent: *mut c_void,
    best_area: u64,
}

/// The native enumeration lends one visible window in this process and its rectangle.
unsafe extern "C" fn consider(user: *mut c_void, candidate: *mut c_void, bounds: *const Rect) {
    let search = unsafe { &mut *user.cast::<Search>() };
    let bounds = unsafe { &*bounds };
    let requested = &search.requested;
    if requested.left < bounds.left
        || requested.top < bounds.top
        || requested.right > bounds.right
        || requested.bottom > bounds.bottom
    {
        return;
    }
    let width = i64::from(bounds.right) - i64::from(bounds.left);
    let height = i64::from(bounds.bottom) - i64::from(bounds.top);
    if width <= 0 || height <= 0 {
        return;
    }
    let area = width as u64 * height as u64;
    if area < search.best_area {
        search.best_area = area;
        search.parent = candidate;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_region(
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    show: bool,
    leaves: &Leaves,
) -> bool {
    if !show || width <= 0 || height <= 0 {
        hide(leaves, false);
        return true;
    }
    let mut region = *window();
    if region.handle == 0 {
        let (Some(right), Some(bottom)) = (left.checked_add(width), top.checked_add(height)) else {
            return false;
        };
        let mut search = Search {
            requested: Rect {
                left,
                top,
                right,
                bottom,
            },
            parent: std::ptr::null_mut(),
            best_area: u64::MAX,
        };
        unsafe { (leaves.enumerate)((&raw mut search).cast(), consider) };
        if search.parent.is_null() {
            return false;
        }
        let handle = unsafe { (leaves.create_window)(search.parent, width, height) };
        if handle.is_null() {
            return false;
        }
        region = Window {
            parent: search.parent as usize,
            handle: handle as usize,
            visible: false,
        };
        *window() = region;
    }
    let positioned = unsafe {
        (leaves.position)(
            region.handle as *mut c_void,
            region.parent as *mut c_void,
            left,
            top,
            width,
            height,
        )
    };
    if positioned {
        window().visible = true;
    }
    positioned
}

fn hide(leaves: &Leaves, asynchronous: bool) {
    let handle = {
        let mut window = window();
        window.visible = false;
        window.handle
    };
    if handle != 0 {
        unsafe { (leaves.show)(handle as *mut c_void, false, asynchronous) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_enabled() -> bool {
    let window = window();
    window.handle != 0 && window.visible
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_owns(candidate: *mut c_void) -> bool {
    !candidate.is_null() && window().handle == candidate as usize
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_parent() -> *mut c_void {
    window().parent as *mut c_void
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_hide(leaves: &Leaves) {
    hide(leaves, true);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_base_frames() -> u64 {
    BASE_FRAMES.load(Ordering::Relaxed)
}

struct Graphics {
    gpu: Gpu,
    slots: [Slot; SLOTS],
    /// A refused signal left a recording with the queue. Keep every reference and refuse reuse.
    faulted: bool,
    /// The last drained check found every submission complete, and nothing was executed since.
    proved: bool,
    ring: Interop,
}

// The references are used only under the graphics mutex: render-thread callers try to take it,
// and the package lifecycle takes it for the drained check and the release.
unsafe impl Send for Graphics {}

static GRAPHICS: Mutex<Graphics> = Mutex::new(Graphics::new());

fn graphics() -> Option<MutexGuard<'static, Graphics>> {
    // A reentrant render callback cannot record a second preview submission.
    match GRAPHICS.try_lock() {
        Ok(graphics) => Some(graphics),
        Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

impl Graphics {
    const fn new() -> Self {
        Self {
            gpu: Gpu::EMPTY,
            slots: [Slot::EMPTY; SLOTS],
            faulted: false,
            proved: false,
            ring: Interop::new(),
        }
    }

    /// Takes one reference each to the lent device and queue and creates the fence. A refused or
    /// incomplete transfer is dropped at once, in the C++ release order.
    fn bind(&mut self, device: *mut c_void, queue: *mut c_void, leaves: &Leaves) -> bool {
        let mut bound = Gpu::EMPTY;
        let accepted = unsafe { (leaves.bind_gpu)(device, queue, &mut bound) };
        if !accepted || bound.device.is_null() || bound.queue.is_null() || bound.fence.is_null() {
            unref(leaves, bound.fence);
            unref(leaves, bound.queue);
            unref(leaves, bound.device);
            return false;
        }
        // The leaf never writes the swapchain, which create_chain adds next.
        self.gpu = bound;
        true
    }

    fn create_chain(&mut self, window: Window, width: u32, height: u32, leaves: &Leaves) -> bool {
        let mut chain = ptr::null_mut();
        let created = unsafe {
            (leaves.create_chain)(
                &self.gpu,
                window.handle as *mut c_void,
                width,
                height,
                &mut chain,
            )
        };
        if !created || chain.is_null() {
            unref(leaves, chain);
            return false;
        }
        self.gpu.chain = chain;
        true
    }

    fn prepare(
        &mut self,
        window: Window,
        device: *mut c_void,
        queue: *mut c_void,
        width: u32,
        height: u32,
        leaves: &Leaves,
    ) -> bool {
        if self.faulted
            || !window.visible
            || window.handle == 0
            || device.is_null()
            || queue.is_null()
            || width == 0
            || height == 0
        {
            return false;
        }
        if self.gpu.device.is_null() && !self.bind(device, queue, leaves) {
            return false;
        }
        if self.gpu.device != device || self.gpu.queue != queue {
            return false;
        }
        if self.gpu.chain.is_null() {
            return self.create_chain(window, width, height, leaves);
        }
        let (mut old_width, mut old_height) = (0, 0);
        if !unsafe { (leaves.chain_extent)(self.gpu.chain, &mut old_width, &mut old_height) } {
            return false;
        }
        (old_width, old_height) == (width, height)
            || unsafe {
                (leaves.wait)(self.gpu.fence, self.ring.submitted())
                    && (leaves.resize_chain)(self.gpu.chain, width, height)
            }
    }

    /// The per-slot proof: the next ring slot, once its last submission completed. Only then may
    /// the slot drop its source reference and reset its allocator.
    fn retired_slot(&self, leaves: &Leaves) -> Option<usize> {
        let (slot, retired) = self.ring.slot();
        unsafe { (leaves.wait)(self.gpu.fence, retired) }.then_some(slot)
    }

    /// Copies `texture` to the current back buffer through slot `index` and executes it.
    fn record(
        &mut self,
        index: usize,
        texture: *mut c_void,
        common: bool,
        leaves: &Leaves,
    ) -> bool {
        // Resolve the destination before touching the slot, so a refusal leaves no open list.
        let mut backbuffer = ptr::null_mut();
        if !unsafe { (leaves.acquire)(self.gpu.chain, &mut backbuffer) } || backbuffer.is_null() {
            unref(leaves, backbuffer);
            return false;
        }
        let recorded = self.record_slot(index, backbuffer, texture, common, leaves);
        unref(leaves, backbuffer);
        recorded
    }

    fn record_slot(
        &mut self,
        index: usize,
        backbuffer: *mut c_void,
        texture: *mut c_void,
        common: bool,
        leaves: &Leaves,
    ) -> bool {
        let Some(slot) = self.slots.get_mut(index) else {
            return false;
        };
        unref(leaves, std::mem::replace(&mut slot.source, ptr::null_mut()));
        if slot.list.is_null() {
            let mut fresh = Slot::EMPTY;
            let created = unsafe { (leaves.create_slot)(self.gpu.device, &mut fresh) };
            if !created || fresh.allocator.is_null() || fresh.list.is_null() {
                fresh.discard(leaves);
                return false;
            }
            *slot = fresh;
        }
        match unsafe { (leaves.record)(&*slot, backbuffer, texture, common) } {
            RECORD_CLOSED => {}
            RECORD_BROKEN => {
                std::mem::replace(slot, Slot::EMPTY).clear(leaves);
                return false;
            }
            RECORD_REFUSED => return false,
            // Not a record result: refuse as a failed reset does.
            _ => return false,
        }
        // The slot holds its source until the slot's next submission proves this one complete.
        unsafe { (leaves.retain)(texture) };
        slot.source = texture;
        self.proved = false;
        unsafe { (leaves.execute)(self.gpu.queue, slot.list) };
        true
    }

    /// Reads the fence without waiting. A device-removal read of `u64::MAX` refuses, as does a
    /// fault. Without a fence the read is 0.
    fn check_drained(&mut self, leaves: &Leaves) -> bool {
        let completed = if self.gpu.fence.is_null() {
            0
        } else {
            unsafe { (leaves.completed)(self.gpu.fence) }
        };
        self.proved = !self.faulted && completed != u64::MAX && self.ring.drained(completed);
        self.proved
    }

    /// Drops every reference in the C++ release order: each slot's allocator, list and source,
    /// then the swapchain, fence, queue and device.
    fn destroy(self, leaves: &Leaves) {
        for slot in self.slots {
            slot.clear(leaves);
        }
        unref(leaves, self.gpu.chain);
        unref(leaves, self.gpu.fence);
        unref(leaves, self.gpu.queue);
        unref(leaves, self.gpu.device);
    }
}

/// Package retirement closes every bridge route to the preview before reading its fence.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_drained(leaves: &Leaves) -> bool {
    graphics().is_some_and(|mut graphics| graphics.check_drained(leaves))
}

/// The checked release. The record is destroyed under the graphics mutex, so no render-thread
/// caller can see it partly released; those callers only try to take the mutex, so a blocking
/// leaf cannot wait on one of them. The ring is forgotten with it.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_release(leaves: &Leaves) -> bool {
    release_checked(
        &mut GRAPHICS.lock().unwrap_or_else(PoisonError::into_inner),
        leaves,
    )
}

/// Destroys the record only with a drained proof taken since its last submission, then leaves a
/// fresh one. A refusal destroys nothing.
fn release_checked(graphics: &mut Graphics, leaves: &Leaves) -> bool {
    if !graphics.proved {
        return false;
    }
    std::mem::replace(graphics, Graphics::new()).destroy(leaves);
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_prepare(
    device: *mut c_void,
    queue: *mut c_void,
    width: u32,
    height: u32,
    leaves: &Leaves,
) -> bool {
    let region = *window();
    graphics()
        .is_some_and(|mut graphics| graphics.prepare(region, device, queue, width, height, leaves))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_present(
    texture: *mut c_void,
    pass: &rsf_game_render_pass,
    common_state: bool,
    leaves: &Leaves,
) -> bool {
    let region = *window();
    let Some(mut graphics) = graphics() else {
        return false;
    };
    if !region.visible || graphics.gpu.chain.is_null() || graphics.faulted || texture.is_null() {
        return false;
    }
    let width = pass.camera.output_width;
    let height = pass.camera.output_height;
    let mut facts = Texture::default();
    unsafe { (leaves.describe)(texture, &mut facts) };
    if facts.width != u64::from(width)
        || facts.height != height
        || !matches!(
            facts.format,
            R8G8B8A8_TYPELESS | R8G8B8A8_UNORM | R8G8B8A8_UNORM_SRGB
        )
    {
        return false;
    }
    let (device, queue) = (graphics.gpu.device, graphics.gpu.queue);
    if !graphics.prepare(region, device, queue, width, height, leaves) {
        return false;
    }
    let Some(slot) = graphics.retired_slot(leaves) else {
        return false;
    };
    if !graphics.record(slot, texture, common_state, leaves) {
        return false;
    }
    let complete = graphics.ring.submit(slot).unwrap_or(0);
    if complete == 0
        || !unsafe { (leaves.signal)(graphics.gpu.queue, graphics.gpu.fence, complete) }
    {
        // The queue already owns this recording. Keep all references and refuse further reuse.
        graphics.faulted = true;
        return false;
    }
    let shown = unsafe { (leaves.present)(graphics.gpu.chain, pass) };
    graphics.ring.advance(slot);
    if shown {
        BASE_FRAMES.fetch_add(1, Ordering::Relaxed);
        if window().visible {
            unsafe { (leaves.show)(region.handle as *mut c_void, true, true) };
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    thread_local! {
        /// The leaves each test's fake leaves called, in order. Tests run on their own threads.
        static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        /// The leaf that refuses, if any. "close" makes record report a failed Close.
        static REFUSE: Cell<&'static str> = const { Cell::new("") };
        static COMPLETED: Cell<u64> = const { Cell::new(0) };
        /// Slots created so far, so each slot's allocator and list are told apart.
        static CREATED: Cell<usize> = const { Cell::new(0) };
    }

    const DEVICE: usize = 1;
    const QUEUE: usize = 2;
    const FENCE: usize = 3;
    const CHAIN: usize = 4;
    const ALLOCATOR: usize = 10;
    const LIST: usize = 20;
    const BACKBUFFER: usize = 30;
    const TEXTURE: usize = 40;

    fn handle(value: usize) -> *mut c_void {
        ptr::without_provenance_mut(value)
    }

    fn called(name: &str) -> bool {
        CALLS.with(|calls| calls.borrow_mut().push(name.to_owned()));
        REFUSE.get() != name
    }

    fn calls() -> Vec<String> {
        CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
    }

    /// A record without a binding, on a thread with no recorded calls or refusals.
    fn fresh() -> Graphics {
        calls();
        REFUSE.set("");
        COMPLETED.set(0);
        CREATED.set(0);
        Graphics::new()
    }

    unsafe extern "C" fn enumerate(_: *mut c_void, _: Candidate) {}
    unsafe extern "C" fn create_window(_: *mut c_void, _: i32, _: i32) -> *mut c_void {
        ptr::null_mut()
    }
    unsafe extern "C" fn position(
        _: *mut c_void,
        _: *mut c_void,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
    ) -> bool {
        false
    }
    unsafe extern "C" fn show(_: *mut c_void, _: bool, _: bool) {}

    unsafe extern "C" fn bind_gpu(device: *mut c_void, queue: *mut c_void, out: *mut Gpu) -> bool {
        if !called("bind_gpu") {
            return false;
        }
        let out = unsafe { &mut *out };
        out.device = device;
        out.queue = queue;
        out.fence = handle(FENCE);
        true
    }

    unsafe extern "C" fn create_chain(
        _: *const Gpu,
        _: *mut c_void,
        width: u32,
        height: u32,
        out: *mut *mut c_void,
    ) -> bool {
        if !called(&format!("create_chain {width}x{height}")) {
            return false;
        }
        unsafe { out.write(handle(CHAIN)) };
        true
    }

    unsafe extern "C" fn chain_extent(_: *mut c_void, width: *mut u32, height: *mut u32) -> bool {
        unsafe {
            width.write(8);
            height.write(8);
        }
        called("chain_extent")
    }

    unsafe extern "C" fn resize_chain(_: *mut c_void, width: u32, height: u32) -> bool {
        called(&format!("resize {width}x{height}"))
    }

    unsafe extern "C" fn describe(_: *mut c_void, _: *mut Texture) {}

    unsafe extern "C" fn completed(_: *mut c_void) -> u64 {
        COMPLETED.get()
    }

    unsafe extern "C" fn wait(_: *mut c_void, value: u64) -> bool {
        called(&format!("wait {value}"))
    }

    unsafe extern "C" fn acquire(_: *mut c_void, out: *mut *mut c_void) -> bool {
        if !called("acquire") {
            return false;
        }
        unsafe { out.write(handle(BACKBUFFER)) };
        true
    }

    /// A refusal transfers the allocator only, as the native leaf does on a refused list.
    unsafe extern "C" fn create_slot(_: *mut c_void, out: *mut Slot) -> bool {
        let index = CREATED.get();
        let out = unsafe { &mut *out };
        out.allocator = handle(ALLOCATOR + index);
        if !called("create_slot") {
            return false;
        }
        out.list = handle(LIST + index);
        CREATED.set(index + 1);
        true
    }

    unsafe extern "C" fn record(_: *const Slot, _: *mut c_void, _: *mut c_void, _: bool) -> u32 {
        if !called("record") {
            RECORD_REFUSED
        } else if REFUSE.get() == "close" {
            RECORD_BROKEN
        } else {
            RECORD_CLOSED
        }
    }

    unsafe extern "C" fn execute(_: *mut c_void, list: *mut c_void) {
        called(&format!("execute {}", list.addr()));
    }

    unsafe extern "C" fn signal(_: *mut c_void, _: *mut c_void, value: u64) -> bool {
        called(&format!("signal {value}"))
    }

    unsafe extern "C" fn present(_: *mut c_void, _: *const rsf_game_render_pass) -> bool {
        called("present")
    }

    unsafe extern "C" fn retain(object: *mut c_void) {
        called(&format!("retain {}", object.addr()));
    }

    unsafe extern "C" fn release(object: *mut c_void) {
        called(&format!("release {}", object.addr()));
    }

    const LEAVES: Leaves = Leaves {
        enumerate,
        create_window,
        position,
        show,
        bind_gpu,
        create_chain,
        chain_extent,
        resize_chain,
        describe,
        completed,
        wait,
        acquire,
        create_slot,
        record,
        execute,
        signal,
        present,
        retain,
        release,
    };

    fn visible() -> Window {
        Window {
            parent: 6,
            handle: 7,
            visible: true,
        }
    }

    fn prepare(graphics: &mut Graphics, width: u32) -> bool {
        graphics.prepare(visible(), handle(DEVICE), handle(QUEUE), width, 8, &LEAVES)
    }

    /// A record with its device, queue, fence and swapchain and no calls recorded.
    fn bound() -> Graphics {
        let mut graphics = fresh();
        assert!(prepare(&mut graphics, 8));
        calls();
        graphics
    }

    /// One submission through the next ring slot, as rsf_core_unity_preview_present makes it.
    fn submit(graphics: &mut Graphics, texture: usize) {
        let slot = graphics.retired_slot(&LEAVES).unwrap();
        assert!(graphics.record(slot, handle(texture), false, &LEAVES));
        graphics.ring.submit(slot).unwrap();
        graphics.ring.advance(slot);
        calls();
    }

    #[test]
    fn the_device_queue_and_fence_bind_together_before_the_swapchain() {
        let mut graphics = fresh();
        REFUSE.set("bind_gpu");
        assert!(!prepare(&mut graphics, 8));
        assert_eq!(graphics.gpu, Gpu::EMPTY);
        assert_eq!(calls(), ["bind_gpu"]);
        REFUSE.set("create_chain 8x8");
        assert!(!prepare(&mut graphics, 8));
        assert_eq!(calls(), ["bind_gpu", "create_chain 8x8"]);
        // The binding stays; only the swapchain is retried.
        REFUSE.set("");
        assert!(prepare(&mut graphics, 8));
        assert_eq!(calls(), ["create_chain 8x8"]);
        assert_eq!(
            graphics.gpu,
            Gpu {
                device: handle(DEVICE),
                queue: handle(QUEUE),
                fence: handle(FENCE),
                chain: handle(CHAIN),
            }
        );
        // Another device or queue is refused without a leaf.
        assert!(!graphics.prepare(visible(), handle(9), handle(QUEUE), 8, 8, &LEAVES));
        assert!(calls().is_empty());
        // A new extent waits for every submission before resizing.
        assert!(prepare(&mut graphics, 16));
        assert_eq!(calls(), ["chain_extent", "wait 0", "resize 16x8"]);
    }

    #[test]
    fn a_slot_is_created_on_first_use_and_holds_its_source_until_reuse() {
        let mut graphics = bound();
        assert_eq!(graphics.retired_slot(&LEAVES), Some(0));
        assert!(graphics.record(0, handle(TEXTURE), false, &LEAVES));
        assert_eq!(
            calls(),
            [
                "wait 0",
                "acquire",
                "create_slot",
                "record",
                "retain 40",
                "execute 20",
                "release 30"
            ]
        );
        assert_eq!(
            graphics.slots[0],
            Slot {
                allocator: handle(ALLOCATOR),
                list: handle(LIST),
                source: handle(TEXTURE),
            }
        );
        // Reuse drops the old source before recording and creates nothing.
        assert!(graphics.record(0, handle(TEXTURE + 1), true, &LEAVES));
        assert_eq!(
            calls(),
            [
                "acquire",
                "release 40",
                "record",
                "retain 41",
                "execute 20",
                "release 30"
            ]
        );
        assert!(!graphics.record(SLOTS, handle(TEXTURE), false, &LEAVES));
        assert_eq!(calls(), ["acquire", "release 30"]);
    }

    #[test]
    fn a_slot_is_reused_only_after_its_completion() {
        let mut graphics = bound();
        for texture in 0..SLOTS {
            submit(&mut graphics, TEXTURE + texture);
        }
        REFUSE.set("wait 1");
        assert_eq!(graphics.retired_slot(&LEAVES), None);
        assert_eq!(calls(), ["wait 1"]);
        // The refused proof left slot 0 and its source untouched.
        assert_eq!(graphics.slots[0].source, handle(TEXTURE));
    }

    #[test]
    fn a_refused_destination_keeps_the_slot() {
        let mut graphics = bound();
        submit(&mut graphics, TEXTURE);
        REFUSE.set("acquire");
        assert!(!graphics.record(0, handle(TEXTURE + 1), false, &LEAVES));
        assert_eq!(calls(), ["acquire"]);
        assert_eq!(graphics.slots[0].source, handle(TEXTURE));
    }

    #[test]
    fn a_partial_slot_is_dropped_at_once() {
        let mut graphics = bound();
        REFUSE.set("create_slot");
        assert!(!graphics.record(0, handle(TEXTURE), false, &LEAVES));
        assert_eq!(
            calls(),
            ["acquire", "create_slot", "release 10", "release 30"]
        );
        assert_eq!(graphics.slots[0], Slot::EMPTY);
    }

    #[test]
    fn a_failed_close_drops_the_slot_and_a_failed_reset_keeps_it() {
        let mut graphics = bound();
        REFUSE.set("close");
        assert!(!graphics.record(0, handle(TEXTURE), false, &LEAVES));
        assert_eq!(
            calls(),
            [
                "acquire",
                "create_slot",
                "record",
                "release 10",
                "release 20",
                "release 30"
            ]
        );
        assert_eq!(graphics.slots[0], Slot::EMPTY);
        REFUSE.set("record");
        assert!(!graphics.record(0, handle(TEXTURE), false, &LEAVES));
        assert_eq!(calls(), ["acquire", "create_slot", "record", "release 30"]);
        assert_eq!(
            graphics.slots[0],
            Slot {
                allocator: handle(ALLOCATOR + 1),
                list: handle(LIST + 1),
                source: ptr::null_mut(),
            }
        );
    }

    #[test]
    fn release_needs_a_drained_proof_taken_since_the_last_submission() {
        let mut graphics = bound();
        submit(&mut graphics, TEXTURE);
        submit(&mut graphics, TEXTURE + 1);
        assert!(!release_checked(&mut graphics, &LEAVES));
        COMPLETED.set(1);
        assert!(!graphics.check_drained(&LEAVES));
        COMPLETED.set(u64::MAX);
        assert!(!graphics.check_drained(&LEAVES));
        COMPLETED.set(2);
        assert!(graphics.check_drained(&LEAVES));
        // A later submission voids the proof.
        submit(&mut graphics, TEXTURE + 2);
        assert!(!release_checked(&mut graphics, &LEAVES));
        assert!(calls().is_empty());
        COMPLETED.set(3);
        assert!(graphics.check_drained(&LEAVES));
        assert!(release_checked(&mut graphics, &LEAVES));
        assert_eq!(
            calls(),
            [
                "release 10",
                "release 20",
                "release 40",
                "release 11",
                "release 21",
                "release 41",
                "release 12",
                "release 22",
                "release 42",
                "release 4",
                "release 3",
                "release 2",
                "release 1"
            ]
        );
        assert_eq!(graphics.gpu, Gpu::EMPTY);
        assert_eq!(graphics.slots, [Slot::EMPTY; SLOTS]);
        assert_eq!(graphics.ring, Interop::new());
        assert!(!graphics.proved);
    }

    #[test]
    fn a_fault_keeps_every_reference() {
        let mut graphics = bound();
        submit(&mut graphics, TEXTURE);
        graphics.faulted = true;
        COMPLETED.set(1);
        assert!(!graphics.check_drained(&LEAVES));
        assert!(!release_checked(&mut graphics, &LEAVES));
        assert!(!prepare(&mut graphics, 8));
        assert!(calls().is_empty());
        assert_eq!(graphics.slots[0].source, handle(TEXTURE));
    }

    #[test]
    fn an_unbound_preview_proves_and_releases_nothing() {
        let mut graphics = fresh();
        assert!(graphics.check_drained(&LEAVES));
        assert!(release_checked(&mut graphics, &LEAVES));
        assert!(calls().is_empty());
    }
}
