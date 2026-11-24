// SPDX-License-Identifier: MIT
//! A per-output D3D12 presenter. The caller owns window selection and leases the source.
#[cfg(windows)]
// Production calls the D3D12 and DXGI leaves in `windows`; unit tests call same-named fakes.
#[cfg(not(test))]
mod windows;
use crate::sync::lock;
use rsf_game_sys::rsf_game_render_pass;
use rsf_runtime_submission::SubmissionRing;
#[cfg(windows)]
use std::sync::{Mutex, TryLockError};
use std::{ffi::c_void, ptr};
#[cfg(test)]
use tests as leaf;
#[cfg(not(test))]
use windows as leaf;

type Observer = Option<unsafe extern "C" fn(*const rsf_game_render_pass) -> i32>;

const SLOTS: usize = 3;
const RECORD_REFUSED: u32 = 0;
const RECORD_BROKEN: u32 = 1;
const RECORD_CLOSED: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Target {
    struct_size: u32,
    version: u32,
    handle: *mut c_void,
    generation: u64,
}
const _: () = {
    assert!(size_of::<Target>() == 24);
    assert!(std::mem::offset_of!(Target, generation) == 16);
};

impl Target {
    const EMPTY: Self = Self {
        struct_size: size_of::<Self>() as u32,
        version: 1,
        handle: ptr::null_mut(),
        generation: 0,
    };
}

struct Texture {
    width: u64,
    height: u32,
    compatible: bool,
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

    /// Drops the slot: allocator, list, source.
    fn clear(self) {
        unref(self.allocator);
        unref(self.list);
        unref(self.source);
    }

    /// Drops a refused fresh slot: list, then allocator.
    fn discard(self) {
        unref(self.list);
        unref(self.allocator);
    }
}

/// Drops one transferred reference. Null holds none.
fn unref(object: *mut c_void) {
    if !object.is_null() {
        unsafe { leaf::release(object) };
    }
}

struct Graphics {
    gpu: Gpu,
    slots: [Slot; SLOTS],
    /// A refused signal left a recording with the queue. Keep every reference and refuse reuse.
    faulted: bool,
    /// The last drained check found every submission complete, and nothing was executed since.
    proved: bool,
    ring: SubmissionRing,
    target: Target,
    replace_chain: bool,
}

// The references are used only under the graphics mutex: render-thread callers try to take it,
// and the package lifecycle takes it for the drained check and the release.
unsafe impl Send for Graphics {}

impl Graphics {
    const fn new() -> Self {
        Self {
            gpu: Gpu::EMPTY,
            slots: [Slot::EMPTY; SLOTS],
            faulted: false,
            proved: false,
            ring: SubmissionRing::new(SLOTS),
            target: Target::EMPTY,
            replace_chain: false,
        }
    }

    /// Takes one reference each to the lent device and queue and creates the fence. A refused or
    /// incomplete transfer is dropped at once.
    fn bind(&mut self, device: *mut c_void, queue: *mut c_void) -> bool {
        let mut bound = Gpu::EMPTY;
        let accepted = unsafe { leaf::bind_gpu(device, queue, &mut bound) };
        if !accepted || bound.device.is_null() || bound.queue.is_null() || bound.fence.is_null() {
            unref(bound.fence);
            unref(bound.queue);
            unref(bound.device);
            return false;
        }
        // The leaf never writes the swapchain, which create_chain adds next.
        self.gpu = bound;
        true
    }

    fn create_chain(&mut self, window: Target, width: u32, height: u32) -> bool {
        let mut chain = ptr::null_mut();
        let created =
            unsafe { leaf::create_chain(&self.gpu, window.handle, width, height, &mut chain) };
        if !created || chain.is_null() {
            unref(chain);
            return false;
        }
        self.gpu.chain = chain;
        true
    }

    fn prepare(
        &mut self,
        window: Target,
        device: *mut c_void,
        queue: *mut c_void,
        width: u32,
        height: u32,
    ) -> bool {
        if self.faulted
            || window.handle.is_null()
            || window.generation < self.target.generation
            || (window.generation == self.target.generation && window.handle != self.target.handle)
            || device.is_null()
            || queue.is_null()
            || width == 0
            || height == 0
        {
            return false;
        }
        if self.gpu.device.is_null() && !self.bind(device, queue) {
            return false;
        }
        if self.gpu.device != device || self.gpu.queue != queue {
            return false;
        }
        if self.target != window {
            // Refuse presentations to the previous target immediately. Keep its native
            // chain until the queue proves the copies complete, then retry the new target.
            self.target = window;
            self.replace_chain = true;
        }
        if self.replace_chain {
            if !self.check_drained() {
                return false;
            }
            unref(std::mem::replace(&mut self.gpu.chain, ptr::null_mut()));
            self.replace_chain = false;
        }
        if self.gpu.chain.is_null() {
            return self.create_chain(window, width, height);
        }
        let Some(extent) = (unsafe { leaf::chain_extent(self.gpu.chain) }) else {
            return false;
        };
        extent == (width, height)
            || unsafe {
                leaf::wait(self.gpu.fence, self.ring.submitted())
                    && leaf::resize_chain(self.gpu.chain, width, height)
            }
    }

    /// The per-slot proof: the next ring slot, once its last submission completed. Only then may
    /// the slot drop its source reference and reset its allocator.
    fn retired_slot(&self) -> Option<usize> {
        let (slot, retired) = self.ring.slot();
        unsafe { leaf::wait(self.gpu.fence, retired) }.then_some(slot)
    }

    /// Copies `texture` to the current back buffer through slot `index` and executes it.
    fn record(&mut self, index: usize, texture: *mut c_void, common: bool) -> bool {
        // Resolve the destination before touching the slot, so a refusal leaves no open list.
        let mut backbuffer = ptr::null_mut();
        if !unsafe { leaf::acquire(self.gpu.chain, &mut backbuffer) } || backbuffer.is_null() {
            unref(backbuffer);
            return false;
        }
        let recorded = self.record_slot(index, backbuffer, texture, common);
        unref(backbuffer);
        recorded
    }

    fn record_slot(
        &mut self,
        index: usize,
        backbuffer: *mut c_void,
        texture: *mut c_void,
        common: bool,
    ) -> bool {
        let Some(slot) = self.slots.get_mut(index) else {
            return false;
        };
        unref(std::mem::replace(&mut slot.source, ptr::null_mut()));
        if slot.list.is_null() {
            let mut fresh = Slot::EMPTY;
            let created = unsafe { leaf::create_slot(self.gpu.device, &mut fresh) };
            if !created || fresh.allocator.is_null() || fresh.list.is_null() {
                fresh.discard();
                return false;
            }
            *slot = fresh;
        }
        match unsafe { leaf::record(&*slot, backbuffer, texture, common) } {
            RECORD_CLOSED => {}
            RECORD_BROKEN => {
                std::mem::replace(slot, Slot::EMPTY).clear();
                return false;
            }
            RECORD_REFUSED => return false,
            // Not a record result: refuse as a failed reset does.
            _ => return false,
        }
        // The slot holds its source until the slot's next submission proves this one complete.
        unsafe { leaf::retain(texture) };
        slot.source = texture;
        self.proved = false;
        unsafe { leaf::execute(self.gpu.queue, slot.list) };
        true
    }

    fn submit(&mut self, texture: *mut c_void, common: bool) -> bool {
        if self.faulted || self.ring.next_completion().is_none() {
            return false;
        }
        let Some(slot) = self.retired_slot() else {
            return false;
        };
        if !self.record(slot, texture, common) {
            return false;
        }
        // The GPU already owns the list and source. A missing or refused signal
        // leaves this slot current and bars all further use or retirement.
        if !self.ring.submit(slot).is_some_and(|complete| unsafe {
            leaf::signal(self.gpu.queue, self.gpu.fence, complete)
        }) {
            self.faulted = true;
            return false;
        }
        self.ring.advance(slot);
        true
    }

    fn present(
        &mut self,
        target: Target,
        texture: *mut c_void,
        pass: &rsf_game_render_pass,
        common: bool,
        observer: Observer,
    ) -> bool {
        if self.faulted || self.replace_chain || self.gpu.chain.is_null() || self.target != target {
            return false;
        }
        let facts = unsafe { leaf::describe(texture) };
        let width = pass.camera.output_width;
        let height = pass.camera.output_height;
        if facts.width != u64::from(width) || facts.height != height || !facts.compatible {
            return false;
        }
        self.prepare(target, self.gpu.device, self.gpu.queue, width, height)
            && self.submit(texture, common)
            && unsafe { leaf::present(self.gpu.chain, pass, observer) }
    }

    /// Reads the fence without waiting. A device-removal read of `u64::MAX` refuses, as does a
    /// fault. Without a fence the read is 0.
    fn check_drained(&mut self) -> bool {
        let completed = if self.gpu.fence.is_null() {
            0
        } else {
            unsafe { leaf::completed(self.gpu.fence) }
        };
        self.proved = !self.faulted && self.ring.drained(completed);
        self.proved
    }

    /// Drops every reference: each slot's allocator, list and source,
    /// then the swapchain, fence, queue and device.
    fn destroy(self) {
        for slot in self.slots {
            slot.clear();
        }
        unref(self.gpu.chain);
        unref(self.gpu.fence);
        unref(self.gpu.queue);
        unref(self.gpu.device);
    }
}

/// Destroys the record only with a drained proof taken since its last submission, then leaves a
/// fresh one. A refusal destroys nothing.
fn release_checked(graphics: &mut Graphics) -> bool {
    if !graphics.proved {
        return false;
    }
    std::mem::replace(graphics, Graphics::new()).destroy();
    true
}

// These symbols are linked into the package's existing Runtime core. Entry points require
// caller admission to remain closed between the final proof and destruction.
#[cfg(windows)]
struct Output(Mutex<Graphics>, Observer);

#[cfg(windows)]
fn read_target(target: *const Target) -> Option<Target> {
    if target.is_null() || unsafe { target.cast::<u32>().read() } < size_of::<Target>() as u32 {
        return None;
    }
    let mut target = unsafe { target.read() };
    target.struct_size = size_of::<Target>() as u32;
    (target.version == 1 && target.generation != 0 && !target.handle.is_null()).then_some(target)
}

#[cfg(windows)]
fn with_output(output: &Output, call: impl FnOnce(&mut Graphics) -> bool) -> i32 {
    let mut graphics = match output.0.try_lock() {
        Ok(graphics) => graphics,
        Err(TryLockError::Poisoned(error)) => error.into_inner(),
        Err(TryLockError::WouldBlock) => return 0,
    };
    i32::from(call(&mut graphics))
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_d3d12_window_create(observer: Observer) -> *mut c_void {
    Box::into_raw(Box::new(Output(Mutex::new(Graphics::new()), observer))).cast()
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d12_window_prepare(
    output: *mut c_void,
    target: *const Target,
    device: *mut c_void,
    queue: *mut c_void,
    width: u32,
    height: u32,
) -> i32 {
    let (Some(output), Some(target)) = (
        unsafe { output.cast::<Output>().as_ref() },
        read_target(target),
    ) else {
        return 0;
    };
    with_output(output, |graphics| {
        graphics.prepare(target, device, queue, width, height)
    })
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d12_window_present(
    output: *mut c_void,
    target: *const Target,
    texture: *mut c_void,
    pass: *const rsf_game_render_pass,
    common: u32,
) -> i32 {
    let (Some(output), Some(target)) = (
        unsafe { output.cast::<Output>().as_ref() },
        read_target(target),
    ) else {
        return 0;
    };
    if texture.is_null()
        || pass.is_null()
        || unsafe { pass.cast::<u32>().read() } < size_of::<rsf_game_render_pass>() as u32
    {
        return 0;
    }
    let pass = unsafe { &*pass };
    with_output(output, |graphics| {
        graphics.present(target, texture, pass, common != 0, output.1)
    })
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d12_window_drained(output: *mut c_void) -> i32 {
    unsafe { output.cast::<Output>().as_ref() }.map_or(1, |output| {
        with_output(output, |graphics| graphics.check_drained())
    })
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d12_window_destroy(output: *mut c_void) -> i32 {
    let output = output.cast::<Output>();
    let Some(output_ref) = (unsafe { output.as_ref() }) else {
        return 1;
    };
    if !release_checked(&mut lock(&output_ref.0)) {
        return 0;
    }
    drop(unsafe { Box::from_raw(output) });
    1
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

    pub(super) unsafe extern "C" fn bind_gpu(
        device: *mut c_void,
        queue: *mut c_void,
        out: *mut Gpu,
    ) -> bool {
        if !called("bind_gpu") {
            return false;
        }
        let out = unsafe { &mut *out };
        out.device = device;
        out.queue = queue;
        out.fence = handle(FENCE);
        true
    }

    pub(super) unsafe extern "C" fn create_chain(
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

    pub(super) unsafe fn chain_extent(_: *mut c_void) -> Option<(u32, u32)> {
        called("chain_extent").then_some((8, 8))
    }

    pub(super) unsafe extern "C" fn resize_chain(_: *mut c_void, width: u32, height: u32) -> bool {
        called(&format!("resize {width}x{height}"))
    }

    pub(super) unsafe fn describe(_: *mut c_void) -> Texture {
        Texture {
            width: 8,
            height: 8,
            compatible: true,
        }
    }

    pub(super) unsafe extern "C" fn completed(_: *mut c_void) -> u64 {
        COMPLETED.get()
    }

    pub(super) unsafe extern "C" fn wait(_: *mut c_void, value: u64) -> bool {
        called(&format!("wait {value}"))
    }

    pub(super) unsafe extern "C" fn acquire(_: *mut c_void, out: *mut *mut c_void) -> bool {
        if !called("acquire") {
            return false;
        }
        unsafe { out.write(handle(BACKBUFFER)) };
        true
    }

    /// A refusal transfers the allocator only, as the native leaf does on a refused list.
    pub(super) unsafe extern "C" fn create_slot(_: *mut c_void, out: *mut Slot) -> bool {
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

    pub(super) unsafe extern "C" fn record(
        _: *const Slot,
        _: *mut c_void,
        _: *mut c_void,
        _: bool,
    ) -> u32 {
        if !called("record") {
            RECORD_REFUSED
        } else if REFUSE.get() == "close" {
            RECORD_BROKEN
        } else {
            RECORD_CLOSED
        }
    }

    pub(super) unsafe extern "C" fn execute(_: *mut c_void, list: *mut c_void) {
        called(&format!("execute {}", list.addr()));
    }

    pub(super) unsafe extern "C" fn signal(_: *mut c_void, _: *mut c_void, value: u64) -> bool {
        called(&format!("signal {value}"))
    }

    pub(super) unsafe extern "C" fn present(
        _: *mut c_void,
        _: *const rsf_game_render_pass,
        _: Observer,
    ) -> bool {
        called("present")
    }

    pub(super) unsafe extern "C" fn retain(object: *mut c_void) {
        called(&format!("retain {}", object.addr()));
    }

    pub(super) unsafe extern "C" fn release(object: *mut c_void) {
        called(&format!("release {}", object.addr()));
    }

    fn visible() -> Target {
        Target {
            struct_size: size_of::<Target>() as u32,
            version: 1,
            handle: handle(7),
            generation: 1,
        }
    }

    fn prepare(graphics: &mut Graphics, width: u32) -> bool {
        graphics.prepare(visible(), handle(DEVICE), handle(QUEUE), width, 8)
    }

    /// A record with its device, queue, fence and swapchain and no calls recorded.
    fn bound() -> Graphics {
        let mut graphics = fresh();
        assert!(prepare(&mut graphics, 8));
        calls();
        graphics
    }

    fn pass() -> rsf_game_render_pass {
        // The C ABI pass contains scalars and nullable resource pointers.
        let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        pass.camera.output_width = 8;
        pass.camera.output_height = 8;
        pass
    }

    fn submit(graphics: &mut Graphics, texture: usize) {
        assert!(graphics.submit(handle(texture), false));
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
        assert!(!graphics.prepare(visible(), handle(9), handle(QUEUE), 8, 8));
        assert!(calls().is_empty());
        // A new extent waits for every submission before resizing.
        assert!(prepare(&mut graphics, 16));
        assert_eq!(calls(), ["chain_extent", "wait 0", "resize 16x8"]);
    }

    #[test]
    fn a_slot_is_created_on_first_use_and_holds_its_source_until_reuse() {
        let mut graphics = bound();
        assert_eq!(graphics.retired_slot(), Some(0));
        assert!(graphics.record(0, handle(TEXTURE), false));
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
        assert!(graphics.record(0, handle(TEXTURE + 1), true));
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
        assert!(!graphics.record(SLOTS, handle(TEXTURE), false));
        assert_eq!(calls(), ["acquire", "release 30"]);
    }

    #[test]
    fn a_slot_is_reused_only_after_its_completion() {
        let mut graphics = bound();
        for texture in 0..SLOTS {
            submit(&mut graphics, TEXTURE + texture);
        }
        REFUSE.set("wait 1");
        assert_eq!(graphics.retired_slot(), None);
        assert_eq!(calls(), ["wait 1"]);
        // The refused proof left slot 0 and its source untouched.
        assert_eq!(graphics.slots[0].source, handle(TEXTURE));
    }

    #[test]
    fn a_refused_destination_keeps_the_slot() {
        let mut graphics = bound();
        submit(&mut graphics, TEXTURE);
        REFUSE.set("acquire");
        assert!(!graphics.record(0, handle(TEXTURE + 1), false));
        assert_eq!(calls(), ["acquire"]);
        assert_eq!(graphics.slots[0].source, handle(TEXTURE));
    }

    #[test]
    fn a_partial_slot_is_dropped_at_once() {
        let mut graphics = bound();
        REFUSE.set("create_slot");
        assert!(!graphics.record(0, handle(TEXTURE), false));
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
        assert!(!graphics.record(0, handle(TEXTURE), false));
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
        assert!(!graphics.record(0, handle(TEXTURE), false));
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
        assert!(!release_checked(&mut graphics));
        COMPLETED.set(1);
        assert!(!graphics.check_drained());
        COMPLETED.set(u64::MAX);
        assert!(!graphics.check_drained());
        COMPLETED.set(2);
        assert!(graphics.check_drained());
        // A later submission voids the proof.
        submit(&mut graphics, TEXTURE + 2);
        assert!(!release_checked(&mut graphics));
        assert!(calls().is_empty());
        COMPLETED.set(3);
        assert!(graphics.check_drained());
        assert!(release_checked(&mut graphics));
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
        assert_eq!(graphics.ring, SubmissionRing::new(SLOTS));
        assert!(!graphics.proved);
    }

    #[test]
    fn a_fault_keeps_every_reference() {
        let mut graphics = bound();
        REFUSE.set("signal 1");
        assert!(!graphics.present(visible(), handle(TEXTURE), &pass(), false, None));
        assert_eq!(graphics.ring.slot(), (0, 1));
        assert_eq!(graphics.ring.submitted(), 1);
        assert!(!graphics.proved);
        assert_eq!(
            calls(),
            [
                "chain_extent",
                "wait 0",
                "acquire",
                "create_slot",
                "record",
                "retain 40",
                "execute 20",
                "release 30",
                "signal 1"
            ]
        );
        COMPLETED.set(1);
        assert!(!graphics.check_drained());
        assert!(!release_checked(&mut graphics));
        assert!(!prepare(&mut graphics, 8));
        assert!(!graphics.present(visible(), handle(TEXTURE), &pass(), false, None));
        assert!(calls().is_empty());
        assert_eq!(graphics.slots[0].source, handle(TEXTURE));
    }

    #[test]
    fn a_recycled_window_waits_for_the_old_chain_and_refuses_stale_presents() {
        let mut graphics = bound();
        let pass = pass();
        assert!(graphics.present(visible(), handle(TEXTURE), &pass, false, None));
        calls();
        let new = Target {
            generation: 2,
            ..visible()
        };
        let prepare_new =
            |graphics: &mut Graphics| graphics.prepare(new, handle(DEVICE), handle(QUEUE), 8, 8);
        assert!(!prepare_new(&mut graphics));
        assert!(!graphics.present(visible(), handle(TEXTURE), &pass, false, None));
        assert!(!graphics.present(new, handle(TEXTURE), &pass, false, None));
        COMPLETED.set(u64::MAX);
        assert!(!prepare_new(&mut graphics));
        assert!(!release_checked(&mut graphics));
        assert_eq!(graphics.gpu.chain, handle(CHAIN));
        assert!(calls().is_empty());
        COMPLETED.set(1);
        assert!(prepare_new(&mut graphics));
        assert_eq!(calls(), ["release 4", "create_chain 8x8"]);
        assert!(!prepare(&mut graphics, 8));
        assert!(graphics.present(new, handle(TEXTURE), &pass, false, None));
    }

    #[test]
    fn an_unbound_preview_proves_and_releases_nothing() {
        let mut graphics = fresh();
        assert!(graphics.check_drained());
        assert!(release_checked(&mut graphics));
        assert!(calls().is_empty());
    }
}
