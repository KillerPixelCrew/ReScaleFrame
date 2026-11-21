// SPDX-License-Identifier: MIT
//! Window admission and queue retirement for the Unity Editor preview.
use crate::bridge_interop::Interop;
use rsf_game_sys::rsf_game_render_pass;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

const R8G8B8A8_TYPELESS: u32 = 27;
const R8G8B8A8_UNORM: u32 = 28;
const R8G8B8A8_UNORM_SRGB: u32 = 29;

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

type Candidate = unsafe extern "C" fn(*mut c_void, *mut c_void, *const Rect);

#[repr(C)]
struct Leaves {
    enumerate: unsafe extern "C" fn(*mut c_void, Candidate),
    create_window: unsafe extern "C" fn(*mut c_void, i32, i32) -> *mut c_void,
    position: unsafe extern "C" fn(*mut c_void, *mut c_void, i32, i32, i32, i32) -> bool,
    show: unsafe extern "C" fn(*mut c_void, bool, bool),
    bind_gpu: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool,
    create_chain: unsafe extern "C" fn(*mut c_void, u32, u32) -> bool,
    chain_extent: unsafe extern "C" fn(*mut u32, *mut u32) -> bool,
    resize_chain: unsafe extern "C" fn(u32, u32) -> bool,
    describe: unsafe extern "C" fn(*mut c_void, *mut Texture),
    wait: unsafe extern "C" fn(u64) -> bool,
    record: unsafe extern "C" fn(u32, *mut c_void, bool) -> bool,
    signal: unsafe extern "C" fn(u64) -> bool,
    present: unsafe extern "C" fn(*const rsf_game_render_pass) -> bool,
}

const _: () = {
    assert!(size_of::<Rect>() == 16 && size_of::<Texture>() == 16);
    assert!(size_of::<Leaves>() == 104);
};

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
    device: usize,
    queue: usize,
    chain: bool,
    faulted: bool,
    ring: Interop,
}

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
            device: 0,
            queue: 0,
            chain: false,
            faulted: false,
            ring: Interop::new(),
        }
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
        if self.device == 0 {
            if !unsafe { (leaves.bind_gpu)(device, queue) } {
                return false;
            }
            self.device = device as usize;
            self.queue = queue as usize;
        }
        if self.device != device as usize || self.queue != queue as usize {
            return false;
        }
        if !self.chain {
            self.chain =
                unsafe { (leaves.create_chain)(window.handle as *mut c_void, width, height) };
            return self.chain;
        }
        let (mut old_width, mut old_height) = (0, 0);
        if !unsafe { (leaves.chain_extent)(&mut old_width, &mut old_height) } {
            return false;
        }
        (old_width, old_height) == (width, height)
            || unsafe {
                (leaves.wait)(self.ring.submitted()) && (leaves.resize_chain)(width, height)
            }
    }
}

/// Package retirement closes every bridge route to the preview before reading its fence.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_drained(completed: u64) -> bool {
    graphics().is_some_and(|graphics| !graphics.faulted && graphics.ring.drained(completed))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_unity_preview_reset() {
    *GRAPHICS.lock().unwrap_or_else(PoisonError::into_inner) = Graphics::new();
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
    if !region.visible || !graphics.chain || graphics.faulted || texture.is_null() {
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
    let (device, queue) = (
        graphics.device as *mut c_void,
        graphics.queue as *mut c_void,
    );
    if !graphics.prepare(region, device, queue, width, height, leaves) {
        return false;
    }
    let (slot, retired) = graphics.ring.slot();
    if !unsafe { (leaves.wait)(retired) && (leaves.record)(slot as u32, texture, common_state) } {
        return false;
    }
    let complete = graphics.ring.submit(slot).unwrap_or(0);
    if complete == 0 || !unsafe { (leaves.signal)(complete) } {
        // The queue already owns this recording. Keep all leases and refuse further reuse.
        graphics.faulted = true;
        return false;
    }
    let shown = unsafe { (leaves.present)(pass) };
    graphics.ring.advance(slot);
    if shown {
        BASE_FRAMES.fetch_add(1, Ordering::Relaxed);
        if window().visible {
            unsafe { (leaves.show)(region.handle as *mut c_void, true, true) };
        }
    }
    shown
}
