// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d12_fence::FenceState;
use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};
use crate::overlay_geometry::{borrowed_draw_slice, patch_fits, valid_indices, valid_texture};
use crate::overlay_host_abi::{
    RSF_OVERLAY_ABI_VERSION, RSF_OVERLAY_HOST_PANEL_LOADED, rsf_overlay,
    rsf_overlay_draw_data as DrawData, rsf_overlay_host_panel, rsf_overlay_input,
    rsf_overlay_vertex,
};
use crate::overlay_native12_abi::{
    RSF_OVERLAY12_TEXTURE_LIMIT, rsf_overlay12_draw as Draw, rsf_overlay12_draw_call as DrawCall,
    rsf_overlay12_frame as FrameTable, rsf_overlay12_log, rsf_overlay12_platform as Platform,
    rsf_overlay12_target as Target,
};
use crate::overlay_panel::{
    rsf_core_overlay_panel_open, rsf_core_overlay_panel_release, rsf_core_overlay_panel_retain,
};
use crate::owned_win32_handle;
use crate::sr_session::has_capacity;
use std::ffi::{CStr, c_void};
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr::{NonNull, null_mut};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::{
        SystemInformation::GetTickCount64,
        Threading::{CreateEventW, WaitForSingleObject},
    },
};

const SLOTS: usize = 3;
const TEXTURES: usize = RSF_OVERLAY12_TEXTURE_LIMIT as usize;
const ATLAS_BATCH: usize = 32;
const STARTUP_MS: u64 = 8000;
const STARTUP_FADE_MS: u64 = 1000;
const FENCE_WAIT_MS: u32 = 5000;
const RETRY_DELAY_MS: u64 = 1000;

struct Ui {
    api: rsf_overlay_host_panel,
    handle: NonNull<rsf_overlay>,
    module: *mut c_void,
}

// The panel lock serializes all calls into this instance.
unsafe impl Send for Ui {}

impl Ui {
    fn open() -> Option<Self> {
        let mut api = rsf_overlay_host_panel::default();
        let mut module = null_mut();
        if unsafe { rsf_core_overlay_panel_open(&mut api, null_mut(), &mut module) }
            != RSF_OVERLAY_HOST_PANEL_LOADED as u32
        {
            return None;
        }
        let handle = unsafe { (api.create.unwrap())(RSF_OVERLAY_ABI_VERSION) };
        let Some(handle) = NonNull::new(handle) else {
            unsafe { rsf_core_overlay_panel_release(module) };
            return None;
        };
        Some(Self {
            api,
            handle,
            module,
        })
    }

    fn publish(&mut self) {
        unsafe { rsf_core_overlay_panel_retain(std::mem::replace(&mut self.module, null_mut())) };
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        unsafe { (self.api.destroy.unwrap())(self.handle.as_ptr()) };
        if !self.module.is_null() {
            unsafe { rsf_core_overlay_panel_release(self.module) };
        }
    }
}

#[derive(Default)]
struct Ring {
    completions: [u64; SLOTS],
    cursor: usize,
    serial: u64,
}

impl Ring {
    fn select(&self, completed: u64) -> (usize, u64) {
        let mut oldest = 0;
        for index in (0..SLOTS).map(|i| (self.cursor + i) % SLOTS) {
            let completion = self.completions[index];
            if completion <= completed {
                return (index, 0);
            }
            if completion < self.completions[oldest] {
                oldest = index;
            }
        }
        (oldest, self.completions[oldest])
    }

    fn slot_ready(completed: u64, wait_for: u64) -> bool {
        FenceState::check(completed, wait_for) == FenceState::Complete
    }

    fn submitted(&mut self, slot: usize, value: u64) {
        self.completions[slot] = value;
        self.cursor = (slot + 1) % SLOTS;
        self.serial = value;
    }
}

struct Fence {
    reference: ComRef,
    event: Option<OwnedHandle>,
    armed: Option<u64>,
}

fn retire_fence_event(armed: &mut Option<u64>, signaled: bool, completed: u64) -> bool {
    let Some(value) = *armed else { return true };
    let state = FenceState::check(completed, value);
    if state == FenceState::DeviceRemoved || (signaled && state == FenceState::Complete) {
        *armed = None;
    }
    armed.is_none()
}

// D3D12 fences and Win32 event handles can be moved between threads. PANEL serializes waits and signals.
unsafe impl Send for Fence {}

impl Fence {
    fn completed(&self, platform: &Platform) -> u64 {
        unsafe { (platform.completed.unwrap())(self.reference.as_ptr()) }
    }

    fn wait_for_slot(&mut self, platform: &Platform, value: u64) -> bool {
        if self.armed.is_some() {
            return false;
        }
        if self.event.is_none() {
            let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
            self.event = unsafe { owned_win32_handle(event) };
        }
        let Some(event) = &self.event else {
            return false;
        };
        let event = event.as_raw_handle();
        if unsafe { (platform.arm.unwrap())(self.reference.as_ptr(), event, value) } == 0 {
            return false;
        }
        self.armed = Some(value);
        // Keep the event with its fence after a timeout. A later GPU signal may still reach it.
        self.finish_event(platform, FENCE_WAIT_MS)
            && Ring::slot_ready(self.completed(platform), value)
    }

    fn finish_event(&mut self, platform: &Platform, timeout: u32) -> bool {
        if self.armed.is_none() {
            return true;
        }
        let Some(event) = &self.event else {
            return false;
        };
        let event = event.as_raw_handle();
        let signaled = unsafe { WaitForSingleObject(event, timeout) } == WAIT_OBJECT_0;
        let completed = self.completed(platform);
        retire_fence_event(&mut self.armed, signaled, completed)
    }
}

struct Upload {
    resource: ComRef,
    capacity: u64,
}

fn upload(
    buffer: &mut Option<Upload>,
    platform: &Platform,
    native: *mut c_void,
    bytes: u32,
) -> Option<*mut c_void> {
    let bytes = u64::from(bytes);
    if let Some(buffer) = buffer.as_ref().filter(|buffer| buffer.capacity >= bytes) {
        return Some(buffer.resource.as_ptr());
    }
    let mut capacity = 0;
    let resource = unsafe {
        ComRef::from_owned((platform.create_upload.unwrap())(
            native,
            bytes,
            &mut capacity,
        ))
    }?;
    if capacity < bytes {
        return None;
    }
    let handle = resource.as_ptr();
    *buffer = Some(Upload { resource, capacity });
    Some(handle)
}

struct DrawSlot {
    pipeline: Option<ComRef>,
    vertices: Option<Upload>,
    indices: Option<Upload>,
}

impl DrawSlot {
    const fn new() -> Self {
        Self {
            pipeline: None,
            vertices: None,
            indices: None,
        }
    }

    fn bind_uploads(
        &mut self,
        platform: &Platform,
        native: *mut c_void,
        data: &DrawData,
        binding: &mut Draw,
    ) -> Option<()> {
        if data.vertex_count == 0 || data.index_count == 0 {
            return Some(());
        }
        if data.vertices.is_null() || data.indices.is_null() {
            return None;
        }
        binding.vertex_bytes = data
            .vertex_count
            .checked_mul(size_of::<rsf_overlay_vertex>() as u32)?;
        binding.index_bytes = data.index_count.checked_mul(size_of::<u32>() as u32)?;
        binding.vertices = upload(&mut self.vertices, platform, native, binding.vertex_bytes)?;
        binding.indices = upload(&mut self.indices, platform, native, binding.index_bytes)?;
        Some(())
    }
}

struct Graphics {
    pipeline: Option<(u32, ComRef)>,
    slots: [DrawSlot; SLOTS],
}

// The pipeline and uploads are free-threaded D3D12 references under the panel lock.
unsafe impl Send for Graphics {}

impl Graphics {
    const fn new() -> Self {
        Self {
            pipeline: None,
            slots: [const { DrawSlot::new() }; SLOTS],
        }
    }

    fn bind_pipeline(
        &mut self,
        platform: &Platform,
        native: *mut c_void,
        slot: usize,
        format: u32,
    ) -> Option<*mut c_void> {
        if self
            .pipeline
            .as_ref()
            .is_none_or(|(active, _)| *active != format)
        {
            let pipeline =
                unsafe { ComRef::from_owned((platform.create_pipeline.unwrap())(native, format)) }?;
            self.pipeline = Some((format, pipeline));
        }
        let (_, pipeline) = self.pipeline.as_ref()?;
        self.slots[slot].pipeline = Some(pipeline.clone());
        Some(pipeline.as_ptr())
    }
}

struct Texture {
    id: u64,
    width: u32,
    height: u32,
    resource: ComRef,
}

struct Atlas {
    textures: Vec<Texture>,
    retained: [Vec<ComRef>; SLOTS],
}

// Only thread-safe D3D12 resource references enter the atlas. All changes,
// including release of a completed slot, are serialized by PANEL.
unsafe impl Send for Atlas {}

impl Atlas {
    const fn new() -> Self {
        Self {
            textures: Vec::new(),
            retained: [const { Vec::new() }; SLOTS],
        }
    }

    fn position(&self, id: u64) -> Option<usize> {
        self.textures.iter().position(|entry| entry.id == id)
    }

    fn update(
        &mut self,
        platform: &Platform,
        native: *mut c_void,
        lease: &Lease,
        update: &rsf_overlay_texture_update,
    ) -> Option<()> {
        if !valid_texture(update) {
            return None;
        }
        let position = self.position(update.id);
        let replacement = if update.is_whole_texture != 0 {
            if update.x != 0
                || update.y != 0
                || (position.is_none() && self.textures.len() == TEXTURES)
            {
                return None;
            }
            let resource = unsafe {
                ComRef::from_owned((platform.create_texture.unwrap())(
                    native,
                    update.width,
                    update.height,
                ))
            }?;
            Some(Texture {
                id: update.id,
                width: update.width,
                height: update.height,
                resource,
            })
        } else {
            None
        };
        let texture = match replacement.as_ref() {
            Some(texture) => texture,
            None => &self.textures[position?],
        };
        if !patch_fits(update, texture.width, texture.height) {
            return None;
        }
        let upload = unsafe {
            ComRef::from_owned((platform.upload_texture.unwrap())(
                native,
                &lease.frame,
                texture.resource.as_ptr(),
                update,
            ))
        }?;
        self.retained[lease.slot].push(upload);
        self.retained[lease.slot].push(texture.resource.clone());
        if let Some(replacement) = replacement {
            match position {
                Some(index) => self.textures[index] = replacement,
                None => self.textures.push(replacement),
            }
        }
        Some(())
    }

    unsafe fn draw_calls(&self, data: &DrawData, binding: &Draw) -> Option<Vec<DrawCall>> {
        if binding.vertex_bytes == 0 || binding.index_bytes == 0 {
            return Some(Vec::new());
        }
        let indices = unsafe { borrowed_draw_slice(data.indices, data.index_count) }?;
        let calls = unsafe { borrowed_draw_slice(data.calls, data.call_count) }?;
        let mut planned = Vec::new();
        planned.try_reserve_exact(calls.len()).ok()?;
        let edge = |origin: u32, offset: u32, limit: u32| {
            i32::try_from(origin.saturating_add(offset).min(limit)).ok()
        };
        for call in calls {
            let descriptor = self.position(call.texture_id)? as u32;
            if !valid_indices(call, indices, data.vertex_count) {
                return None;
            }
            planned.push(DrawCall {
                descriptor,
                first_index: call.index_offset,
                index_count: call.index_count,
                vertex_offset: i32::try_from(call.vertex_offset).ok()?,
                clip: [
                    edge(call.clip_x, 0, binding.width)?,
                    edge(call.clip_y, 0, binding.height)?,
                    edge(call.clip_x, call.clip_width, binding.width)?,
                    edge(call.clip_y, call.clip_height, binding.height)?,
                ],
            });
        }
        Some(planned)
    }

    // data borrows the live panel layout while this slot is recorded.
    unsafe fn record(
        &mut self,
        platform: &Platform,
        native: *mut c_void,
        ui: &Ui,
        lease: &Lease,
        binding: &Draw,
        data: &DrawData,
    ) -> Option<()> {
        let mut batch = [rsf_overlay_texture_update::default(); ATLAS_BATCH];
        loop {
            let count = unsafe {
                (ui.api.texture_updates.unwrap())(
                    ui.handle.as_ptr(),
                    batch.as_mut_ptr(),
                    ATLAS_BATCH as u32,
                )
            };
            if count == 0 {
                break;
            }
            for update in batch.get(..count as usize)? {
                self.update(platform, native, lease, update)?;
            }
        }

        let calls = unsafe { self.draw_calls(data, binding) }?;
        let mut bindings = [std::ptr::null_mut(); TEXTURES];
        for (binding, texture) in bindings.iter_mut().zip(&self.textures) {
            *binding = texture.resource.as_ptr();
            self.retained[lease.slot].push(texture.resource.clone());
        }
        let recorded = unsafe {
            (platform.record.unwrap())(
                native,
                &lease.frame,
                binding,
                bindings.as_ptr(),
                self.textures.len() as u32,
                calls.as_ptr(),
                calls.len() as u32,
            )
        };
        if recorded == 0 {
            return None;
        }

        let mut ids = [0; ATLAS_BATCH];
        loop {
            let count = unsafe {
                (ui.api.textures_to_free.unwrap())(
                    ui.handle.as_ptr(),
                    ids.as_mut_ptr(),
                    ATLAS_BATCH as u32,
                )
            };
            if count == 0 {
                return Some(());
            }
            for &id in ids.get(..count as usize)? {
                if let Some(index) = self.position(id) {
                    self.textures.swap_remove(index);
                }
            }
        }
    }
}

// One Present slot on the overlay device: direct allocator, its closed list, the shader-visible
// SRV heap and the RTV heap. Fields release in declaration order, the reverse of creation, as the
// former native members did.
#[derive(Default)]
struct Frame {
    rtv: Option<ComRef>,
    srv: Option<ComRef>,
    commands: Option<ComRef>,
    allocator: Option<ComRef>,
}

impl Frame {
    // The leaf transfers every reference it acquired, including on refusal.
    unsafe fn adopt(table: FrameTable) -> Self {
        Self {
            rtv: unsafe { ComRef::from_owned(table.rtv) },
            srv: unsafe { ComRef::from_owned(table.srv) },
            commands: unsafe { ComRef::from_owned(table.commands) },
            allocator: unsafe { ComRef::from_owned(table.allocator) },
        }
    }

    fn table(&self) -> Option<FrameTable> {
        Some(FrameTable {
            allocator: self.allocator.as_ref()?.as_ptr(),
            commands: self.commands.as_ref()?.as_ptr(),
            srv: self.srv.as_ref()?.as_ptr(),
            rtv: self.rtv.as_ref()?.as_ptr(),
        })
    }
}

// The ring slot chosen for one Present and the borrowed table of its frame.
struct Lease {
    slot: usize,
    frame: FrameTable,
}

struct Renderer {
    handle: std::ptr::NonNull<c_void>,
    destroy: unsafe extern "C" fn(*mut c_void),
    frames: [Frame; SLOTS],
}

// The native leaves read this view while Rust owns its individual references.
#[repr(C)]
struct NativeObjects {
    pixel_shader: Option<ComRef>,
    vertex_shader: Option<ComRef>,
    root: Option<ComRef>,
    queue: Option<ComRef>,
    device: Option<ComRef>,
    srv_stride: u32,
}

const _: () = {
    assert!(size_of::<NativeObjects>() == 48);
    assert!(std::mem::offset_of!(NativeObjects, srv_stride) == 40);
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay12_adopt(objects: *mut NativeObjects) -> *mut c_void {
    // The leaf hands over an owned set and never reads or drops that record afterward.
    let objects = unsafe { objects.read() };
    crate::allocate_handle(objects).cast()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay12_destroy(renderer: *mut c_void) {
    // Successful adoption yields the sole ownership handle.
    drop(unsafe { Box::from_raw(renderer.cast::<NativeObjects>()) });
}

// Only the locked panel uses or releases the native renderer and its frames.
unsafe impl Send for Renderer {}

impl Renderer {
    // A refusal keeps the complete and partial frames until teardown, after the startup fence.
    fn create_frames(&mut self, platform: &Platform) -> Option<()> {
        for frame in &mut self.frames {
            let mut table = FrameTable {
                allocator: null_mut(),
                commands: null_mut(),
                srv: null_mut(),
                rtv: null_mut(),
            };
            let created =
                unsafe { (platform.create_frame.unwrap())(self.handle.as_ptr(), &mut table) };
            *frame = unsafe { Frame::adopt(table) };
            if created == 0 || frame.table().is_none() {
                return None;
            }
        }
        Some(())
    }

    fn frame(&self, slot: usize) -> Option<FrameTable> {
        self.frames.get(slot).and_then(Frame::table)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // The frames end last slot first and before the device objects, where the native
        // renderer's frame members were destroyed.
        for frame in self.frames.iter_mut().rev() {
            *frame = Frame::default();
        }
        unsafe { (self.destroy)(self.handle.as_ptr()) };
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    device: usize,
    queue: usize,
}

struct Retry {
    owner: Owner,
    after: u64,
}

impl Retry {
    fn allows(&self, owner: Owner, now: u64) -> bool {
        owner != self.owner || now >= self.after
    }
}

struct Started {
    // Release leased GPU resources before their renderer, and the UI before the fence.
    atlas: Atlas,
    graphics: Graphics,
    ui: Ui,
    renderer: Renderer,
    fence: Fence,
    owner: Owner,
    failed: bool,
    unsignalled: Option<u64>,
    ring: Ring,
    started: u64,
    previous: u64,
    was_visible: bool,
    trace_left: u32,
    logger: rsf_overlay12_log,
    log_user: usize,
}

struct Panel {
    started: Option<Started>,
    retry: Option<Retry>,
}

static READY: AtomicBool = AtomicBool::new(false);
static PANEL: Mutex<Panel> = Mutex::new(Panel {
    started: None,
    retry: None,
});

impl Started {
    fn initialize(
        platform: &Platform,
        device: *mut c_void,
        queue: *mut c_void,
        chain: *mut c_void,
        logger: rsf_overlay12_log,
        user: *mut c_void,
    ) -> Option<Self> {
        let window = unsafe { (platform.window.unwrap())(chain) };
        if window.is_null() {
            return None;
        }
        let mut ui = Ui::open()?;
        let mut fence = null_mut();
        let native = unsafe { (platform.initialize.unwrap())(device, queue, &mut fence) };
        let fence = unsafe { ComRef::from_owned(fence) };
        let mut renderer = Renderer {
            handle: NonNull::new(native)?,
            destroy: platform.destroy.unwrap(),
            frames: std::array::from_fn(|_| Frame::default()),
        };
        let reference = fence?;
        // A refusal releases the fence, then the created frames and the renderer, as the native
        // initialization failure did. Nothing has been submitted yet.
        renderer.create_frames(platform)?;
        if unsafe { (platform.install_input.unwrap())(window, logger, user) } == 0 {
            return None;
        }
        ui.publish();
        let started = unsafe { GetTickCount64() };
        Some(Self {
            atlas: Atlas::new(),
            graphics: Graphics::new(),
            ui,
            renderer,
            fence: Fence {
                reference,
                event: None,
                armed: None,
            },
            owner: Owner {
                device: device as usize,
                queue: queue as usize,
            },
            failed: false,
            unsignalled: None,
            ring: Ring::default(),
            started,
            previous: started,
            was_visible: false,
            trace_left: 0,
            logger,
            log_user: user as usize,
        })
    }

    fn log(&self, message: &CStr) {
        if let Some(call) = self.logger {
            unsafe { call(self.log_user as *mut c_void, message.as_ptr()) };
        }
    }

    fn fail(&mut self, platform: &Platform) {
        self.failed = true;
        READY.store(false, Ordering::Release);
        unsafe { (platform.set_visible.unwrap())(0) };
        self.log(c"Native D3D12 overlay refused graphics work; see validation evidence.");
    }

    fn retired(&mut self, platform: &Platform) -> bool {
        let completed = self.fence.completed(platform);
        if completed != u64::MAX {
            if let Some(value) = self.unsignalled {
                // Signal on the retained original queue behind the already executed list.
                if unsafe {
                    (platform.signal.unwrap())(
                        self.renderer.handle.as_ptr(),
                        self.fence.reference.as_ptr(),
                        value,
                    )
                } != 0
                {
                    self.ring.serial = value;
                    self.unsignalled = None;
                }
                return false;
            }
            if completed < self.ring.serial {
                return false;
            }
        }
        self.fence.finish_event(platform, 0) && unsafe { (platform.stop_input.unwrap())() != 0 }
    }
}

impl Panel {
    fn defer_retry(&mut self, owner: Owner, now: u64) {
        self.retry = Some(Retry {
            owner,
            after: now.saturating_add(RETRY_DELAY_MS),
        });
    }

    fn retire_failed(&mut self, platform: &Platform, now: u64) {
        if let Some(started) = &mut self.started
            && started.failed
            && started.retired(platform)
        {
            let owner = started.owner;
            self.started = None;
            self.defer_retry(owner, now);
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay12_visible(platform: &Platform) -> u32 {
    if READY.load(Ordering::Acquire) {
        unsafe { (platform.visible.unwrap())() }
    } else {
        0
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay12_toggle(platform: &Platform) {
    if let Ok(_panel) = PANEL.try_lock()
        && READY.load(Ordering::Acquire)
    {
        let visible = unsafe { (platform.visible.unwrap())() };
        unsafe { (platform.set_visible.unwrap())(u32::from(visible == 0)) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay12_present(
    platform: &Platform,
    device: *mut c_void,
    queue: *mut c_void,
    chain: *mut c_void,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: rsf_overlay12_log,
    user: *mut c_void,
) -> i32 {
    if device.is_null()
        || queue.is_null()
        || chain.is_null()
        || !unsafe { has_capacity(stats) }
        || (!intent.is_null() && !unsafe { has_capacity(intent) })
    {
        return 0;
    }
    let Ok(mut panel) = PANEL.try_lock() else {
        return 0;
    };
    let owner = Owner {
        device: device as usize,
        queue: queue as usize,
    };
    let now = unsafe { GetTickCount64() };
    panel.retire_failed(platform, now);
    if panel.started.is_none() {
        if panel
            .retry
            .as_ref()
            .is_some_and(|retry| !retry.allows(owner, now))
        {
            return 0;
        }
        let Some(started) = Started::initialize(platform, device, queue, chain, logger, user)
        else {
            panel.defer_retry(owner, now);
            if let Some(log) = logger {
                unsafe {
                    log(
                        user,
                        c"Native D3D12 overlay initialization failed.".as_ptr(),
                    )
                };
            }
            return 0;
        };
        READY.store(true, Ordering::Release);
        started.log(
            c"RSF native D3D12 overlay ready; Insert opens settings. No D3D11-on-12 resources.",
        );
        panel.started = Some(started);
        panel.retry = None;
    }
    let Some(started) = &mut panel.started else {
        return 0;
    };
    if started.failed || started.owner != owner {
        return 0;
    }
    if let Some(decided) = unsafe { started.present(platform, chain, stats) } {
        if !intent.is_null() {
            unsafe { intent.write(decided) };
        }
        return 1;
    }
    if started.failed {
        panel.retire_failed(platform, now);
    }
    0
}

impl Started {
    unsafe fn present(
        &mut self,
        platform: &Platform,
        chain: *mut c_void,
        stats: *const rsf_overlay_stats,
    ) -> Option<rsf_overlay_intent> {
        let now = unsafe { GetTickCount64() };
        let active_for = now.saturating_sub(self.started);
        let visible = unsafe { (platform.visible.unwrap())() } != 0;
        if !visible && unsafe { (*stats).show_performance_hud } == 0 && active_for >= STARTUP_MS {
            return None;
        }
        let native = self.renderer.handle.as_ptr();
        let completed = self.fence.completed(platform);
        if completed == u64::MAX {
            self.fail(platform);
            return None;
        }
        let (slot, wait_for) = self.ring.select(completed);
        // Only already-submitted overlay work can block slot reuse.
        if wait_for != 0 && !self.fence.wait_for_slot(platform, wait_for) {
            self.fail(platform);
            return None;
        }
        // The slot's frame stays owned by the renderer. The fence proof above admits its reuse.
        let Some(table) = self.renderer.frame(slot) else {
            self.fail(platform);
            return None;
        };
        let lease = Lease { slot, frame: table };
        self.atlas.retained[slot].clear();
        self.graphics.slots[slot].pipeline = None;
        let mut frame = Target {
            width: 0,
            height: 0,
            format: 0,
        };
        let target = unsafe {
            ComRef::from_owned((platform.begin.unwrap())(&lease.frame, chain, &mut frame))
        };
        let Some(target) = target else {
            self.fail(platform);
            return None;
        };
        let Some(pipeline) = self
            .graphics
            .bind_pipeline(platform, native, slot, frame.format)
        else {
            self.fail(platform);
            return None;
        };
        if visible && !self.was_visible {
            self.trace_left = 4;
        }
        self.was_visible = visible;
        let trace = visible && self.trace_left != 0;
        if trace {
            self.trace_left -= 1;
            self.log(c"Overlay12: input/layout begin");
        }
        let elapsed = now.saturating_sub(self.previous) as f32 / 1000.0;
        let hint_alpha = STARTUP_MS.saturating_sub(active_for).min(STARTUP_FADE_MS) as f32 / 1000.0;
        let mut decided = rsf_overlay_intent {
            struct_size: size_of::<rsf_overlay_intent>() as u32,
            ..Default::default()
        };
        let mut data = DrawData {
            struct_size: size_of::<DrawData>() as u32,
            ..Default::default()
        };
        let mut input = rsf_overlay_input {
            struct_size: size_of::<rsf_overlay_input>() as u32,
            ..Default::default()
        };
        if unsafe {
            (platform.collect_input.unwrap())(&mut input, frame.width, frame.height, elapsed)
        } == 0
        {
            self.fail(platform);
            return None;
        }
        input.startup_hint_alpha = hint_alpha;
        let ui = &self.ui;
        if unsafe {
            (ui.api.frame.unwrap())(ui.handle.as_ptr(), &input, stats, &mut data, &mut decided)
        } != 0
        {
            self.fail(platform);
            return None;
        }
        self.previous = now;
        if decided.close_requested != 0 {
            unsafe { (platform.set_visible.unwrap())(0) };
        }
        if trace {
            self.log(c"Overlay12: atlas uploads and draw recording");
        }
        let mut binding = Draw {
            target: target.as_ptr(),
            pipeline,
            vertices: std::ptr::null_mut(),
            indices: std::ptr::null_mut(),
            vertex_data: data.vertices.cast(),
            index_data: data.indices.cast(),
            width: frame.width,
            height: frame.height,
            vertex_bytes: 0,
            index_bytes: 0,
        };
        if self.graphics.slots[slot]
            .bind_uploads(platform, native, &data, &mut binding)
            .is_none()
        {
            self.fail(platform);
            return None;
        }
        if unsafe {
            self.atlas
                .record(platform, native, &self.ui, &lease, &binding, &data)
        }
        .is_none()
        {
            self.fail(platform);
            return None;
        }
        let Some(value) = self
            .ring
            .serial
            .checked_add(1)
            .filter(|value| *value != u64::MAX)
        else {
            self.fail(platform);
            return None;
        };
        if trace {
            self.log(c"Overlay12: queue submission begin");
        }
        let fence = self.fence.reference.as_ptr();
        unsafe { (platform.execute.unwrap())(native, &lease.frame) };
        if unsafe { (platform.signal.unwrap())(native, fence, value) } == 0 {
            // Execute already happened. The failed owner stays alive until a signal proves retirement.
            self.unsignalled = Some(value);
            self.fail(platform);
            return None;
        }
        self.ring.submitted(slot, value);
        if trace {
            self.log(c"Overlay12: submission completed");
        }
        Some(decided)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{Fake, add_ref, fake, raw};
    use std::ptr::{from_mut, null_mut, without_provenance_mut};

    struct AtlasFixture {
        textures: [Box<Fake>; 2],
        upload: Box<Fake>,
        created: usize,
        uploaded: usize,
        pending: bool,
        free: bool,
        refuse_upload: bool,
        // The list marker of the frame each upload and record leaf received.
        lists: Vec<usize>,
    }

    // A slot lease whose frame table carries slot + 1 as its list marker.
    fn lease(slot: usize) -> Lease {
        Lease {
            slot,
            frame: FrameTable {
                allocator: null_mut(),
                commands: without_provenance_mut(slot + 1),
                srv: null_mut(),
                rtv: null_mut(),
            },
        }
    }

    static PIXELS: [u8; 64] = [255; 64];
    const UPDATE: rsf_overlay_texture_update = rsf_overlay_texture_update {
        id: 5,
        x: 0,
        y: 0,
        width: 4,
        height: 4,
        pixels: PIXELS.as_ptr(),
        is_whole_texture: 1,
    };

    unsafe extern "C" fn updates(
        ui: *mut rsf_overlay,
        out: *mut rsf_overlay_texture_update,
        _: u32,
    ) -> u32 {
        let fixture = unsafe { &mut *ui.cast::<AtlasFixture>() };
        if std::mem::take(&mut fixture.pending) {
            unsafe { out.write(UPDATE) };
            1
        } else {
            0
        }
    }

    unsafe extern "C" fn frees(ui: *mut rsf_overlay, out: *mut u64, _: u32) -> u32 {
        let fixture = unsafe { &mut *ui.cast::<AtlasFixture>() };
        if std::mem::take(&mut fixture.free) {
            unsafe { out.write(UPDATE.id) };
            1
        } else {
            0
        }
    }

    unsafe extern "C" fn create(native: *mut c_void, _: u32, _: u32) -> *mut c_void {
        let fixture = unsafe { &mut *native.cast::<AtlasFixture>() };
        let texture = raw(&fixture.textures[fixture.created]);
        fixture.created += 1;
        unsafe { add_ref(texture.cast()) };
        texture
    }

    unsafe extern "C" fn upload(
        native: *mut c_void,
        frame: *const FrameTable,
        _: *mut c_void,
        _: *const rsf_overlay_texture_update,
    ) -> *mut c_void {
        let fixture = unsafe { &mut *native.cast::<AtlasFixture>() };
        fixture.uploaded += 1;
        fixture.lists.push(unsafe { (*frame).commands }.addr());
        if fixture.refuse_upload {
            return null_mut();
        }
        let staging = raw(&fixture.upload);
        unsafe { add_ref(staging.cast()) };
        staging
    }

    unsafe extern "C" fn draw(
        native: *mut c_void,
        frame: *const FrameTable,
        _: *const Draw,
        bindings: *const *mut c_void,
        count: u32,
        calls: *const DrawCall,
        call_count: u32,
    ) -> u32 {
        let fixture = unsafe { &mut *native.cast::<AtlasFixture>() };
        fixture.lists.push(unsafe { (*frame).commands }.addr());
        u32::from(
            count == 1
                && call_count == 1
                && unsafe { (*calls).descriptor == 0 }
                && unsafe { *bindings == raw(&fixture.textures[fixture.created - 1]) },
        )
    }

    const PLATFORM: Platform = Platform {
        window: None,
        initialize: None,
        create_frame: None,
        install_input: None,
        stop_input: None,
        destroy: None,
        visible: None,
        set_visible: None,
        completed: None,
        arm: None,
        begin: None,
        create_pipeline: None,
        create_upload: None,
        collect_input: None,
        create_texture: Some(create),
        upload_texture: Some(upload),
        record: Some(draw),
        execute: None,
        signal: None,
    };

    unsafe extern "C" fn destroy_ui(_: *mut rsf_overlay) {}

    fn fake_ui(native: *mut c_void) -> Ui {
        Ui {
            api: rsf_overlay_host_panel {
                destroy: Some(destroy_ui),
                texture_updates: Some(updates),
                textures_to_free: Some(frees),
                ..Default::default()
            },
            handle: NonNull::new(native.cast()).unwrap(),
            module: null_mut(),
        }
    }

    fn fixture() -> AtlasFixture {
        AtlasFixture {
            textures: [fake(), fake()],
            upload: fake(),
            created: 0,
            uploaded: 0,
            pending: true,
            free: false,
            refuse_upload: false,
            lists: Vec::new(),
        }
    }

    #[test]
    fn replacement_and_free_keep_each_slots_draw_and_upload_resources() {
        let mut fixture = fixture();
        let target_resource = fake();
        let target = unsafe { ComRef::from_owned(raw(&target_resource)).unwrap() };
        let vertices = [rsf_overlay_vertex::default()];
        let indices = [0u32; 3];
        let calls = [crate::overlay_host_abi::rsf_overlay_draw_call {
            index_count: 3,
            clip_width: 4,
            clip_height: 4,
            texture_id: UPDATE.id,
            ..Default::default()
        }];
        let data = DrawData {
            struct_size: size_of::<DrawData>() as u32,
            vertices: vertices.as_ptr(),
            vertex_count: 1,
            indices: indices.as_ptr(),
            index_count: 3,
            calls: calls.as_ptr(),
            call_count: 1,
        };
        let binding = Draw {
            target: target.as_ptr(),
            pipeline: null_mut(),
            vertices: null_mut(),
            indices: null_mut(),
            vertex_data: data.vertices.cast(),
            index_data: data.indices.cast(),
            width: 4,
            height: 4,
            vertex_bytes: size_of_val(&vertices) as u32,
            index_bytes: size_of_val(&indices) as u32,
        };
        let mut atlas = Atlas::new();
        let native = from_mut(&mut fixture).cast();
        assert!(
            unsafe {
                atlas.record(
                    &PLATFORM,
                    native,
                    &fake_ui(native),
                    &lease(0),
                    &binding,
                    &data,
                )
            }
            .is_some()
        );
        fixture.pending = true;
        fixture.free = true;
        let native = from_mut(&mut fixture).cast();
        assert!(
            unsafe {
                atlas.record(
                    &PLATFORM,
                    native,
                    &fake_ui(native),
                    &lease(1),
                    &binding,
                    &data,
                )
            }
            .is_some()
        );
        // Each slot's upload and draw recorded on that slot's own list.
        assert_eq!(fixture.lists, [1, 1, 2, 2]);
        assert!(atlas.textures.is_empty());
        assert_eq!(fixture.textures[0].refs.get(), 3);
        assert_eq!(fixture.textures[1].refs.get(), 3);
        assert_eq!(fixture.upload.refs.get(), 3);

        atlas.retained[0].clear();
        assert_eq!(fixture.textures[0].refs.get(), 1);
        assert_eq!(fixture.textures[1].refs.get(), 3);
        assert_eq!(fixture.upload.refs.get(), 2);
        atlas.retained[1].clear();
        assert_eq!(fixture.textures[1].refs.get(), 1);
        assert_eq!(fixture.upload.refs.get(), 1);
    }

    #[test]
    fn invalid_patch_and_failed_replacement_leave_the_installed_atlas_intact() {
        let mut fixture = fixture();
        let native = from_mut(&mut fixture).cast();
        let mut atlas = Atlas::new();
        let slot = lease(0);
        assert!(atlas.update(&PLATFORM, native, &slot, &UPDATE).is_some());
        let mut patch = UPDATE;
        patch.is_whole_texture = 0;
        patch.x = u32::MAX;
        assert!(atlas.update(&PLATFORM, native, &slot, &patch).is_none());
        assert_eq!(fixture.uploaded, 1);
        patch.x = 1;
        patch.width = 3;
        let native = from_mut(&mut fixture).cast();
        assert!(atlas.update(&PLATFORM, native, &slot, &patch).is_some());
        fixture.refuse_upload = true;
        let native = from_mut(&mut fixture).cast();
        assert!(atlas.update(&PLATFORM, native, &slot, &UPDATE).is_none());
        assert_eq!(
            atlas.textures[0].resource.as_ptr(),
            raw(&fixture.textures[0])
        );
        assert_eq!(fixture.textures[1].refs.get(), 1);
    }

    // Hands out the fakes in creation order, four per frame, and stops at refuse_at.
    struct FrameFixture {
        objects: [Box<Fake>; 4 * SLOTS],
        created: usize,
        refuse_at: usize,
        refs_at_destroy: Option<Vec<u32>>,
        signalled: Option<u64>,
    }

    impl FrameFixture {
        fn refs(&self) -> Vec<u32> {
            self.objects
                .iter()
                .map(|object| object.refs.get())
                .collect()
        }
    }

    unsafe extern "C" fn create_frame(native: *mut c_void, out: *mut FrameTable) -> u32 {
        let fixture = unsafe { &mut *native.cast::<FrameFixture>() };
        let mut parts = [null_mut(); 4];
        for part in &mut parts {
            if fixture.created == fixture.refuse_at {
                break;
            }
            let object = raw(&fixture.objects[fixture.created]);
            unsafe { add_ref(object.cast()) };
            *part = object;
            fixture.created += 1;
        }
        let [allocator, commands, srv, rtv] = parts;
        unsafe {
            out.write(FrameTable {
                allocator,
                commands,
                srv,
                rtv,
            })
        };
        u32::from(parts.iter().all(|part| !part.is_null()))
    }

    unsafe extern "C" fn destroy_renderer(native: *mut c_void) {
        let fixture = unsafe { &mut *native.cast::<FrameFixture>() };
        fixture.refs_at_destroy = Some(fixture.refs());
    }

    fn frame_renderer(fixture: &mut FrameFixture) -> Renderer {
        Renderer {
            handle: NonNull::from(fixture).cast(),
            destroy: destroy_renderer,
            frames: std::array::from_fn(|_| Frame::default()),
        }
    }

    const FRAME_PLATFORM: Platform = Platform {
        create_frame: Some(create_frame),
        ..PLATFORM
    };

    #[test]
    fn frames_are_created_in_slot_order_and_released_before_the_renderer() {
        let mut fixture = FrameFixture {
            objects: std::array::from_fn(|_| fake()),
            created: 0,
            refuse_at: usize::MAX,
            refs_at_destroy: None,
            signalled: None,
        };
        let mut renderer = frame_renderer(&mut fixture);
        assert!(renderer.create_frames(&FRAME_PLATFORM).is_some());
        assert!(renderer.frames.iter().all(|frame| frame.table().is_some()));
        let table = renderer.frame(1).unwrap();
        assert_eq!(table.allocator, raw(&fixture.objects[4]));
        assert_eq!(table.commands, raw(&fixture.objects[5]));
        assert_eq!(table.srv, raw(&fixture.objects[6]));
        assert_eq!(table.rtv, raw(&fixture.objects[7]));
        assert!(renderer.frame(SLOTS).is_none());
        assert_eq!(fixture.refs(), [2; 4 * SLOTS]);
        drop(renderer);
        assert_eq!(
            fixture.refs_at_destroy.as_deref(),
            Some(&[1; 4 * SLOTS][..])
        );
    }

    #[test]
    fn adopted_native_renderer_owns_one_reference_per_object() {
        let objects: [Box<Fake>; 5] = std::array::from_fn(|_| fake());
        let [pixel_shader, vertex_shader, root, queue, device] = objects.each_ref().map(|object| {
            let pointer = raw(object);
            unsafe {
                add_ref(pointer.cast());
                ComRef::from_owned(pointer)
            }
        });
        let mut source = std::mem::ManuallyDrop::new(NativeObjects {
            pixel_shader,
            vertex_shader,
            root,
            queue,
            device,
            srv_stride: 16,
        });
        let renderer = unsafe { rsf_core_overlay12_adopt(&mut *source) };
        assert!(!renderer.is_null());
        assert_eq!(objects.each_ref().map(|object| object.refs.get()), [2; 5]);
        unsafe { rsf_core_overlay12_destroy(renderer) };
        assert_eq!(objects.each_ref().map(|object| object.refs.get()), [1; 5]);
    }

    #[test]
    fn refused_frame_keeps_its_objects_and_earlier_frames_for_teardown() {
        let mut fixture = FrameFixture {
            objects: std::array::from_fn(|_| fake()),
            created: 0,
            // The third frame's SRV heap is refused after its allocator and list exist.
            refuse_at: 10,
            refs_at_destroy: None,
            signalled: None,
        };
        let mut renderer = frame_renderer(&mut fixture);
        assert!(renderer.create_frames(&FRAME_PLATFORM).is_none());
        assert!(renderer.frame(1).is_some());
        assert!(renderer.frame(2).is_none());
        assert_eq!(fixture.refs(), [2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1]);
        drop(renderer);
        assert_eq!(
            fixture.refs_at_destroy.as_deref(),
            Some(&[1; 4 * SLOTS][..])
        );
    }

    #[test]
    fn failed_owner_retires_on_its_original_queue_before_a_replacement_can_start() {
        unsafe extern "C" fn pending(_: *mut c_void) -> u64 {
            0
        }
        unsafe extern "C" fn completed(_: *mut c_void) -> u64 {
            9
        }
        unsafe extern "C" fn signal(native: *mut c_void, _: *mut c_void, value: u64) -> u32 {
            // The retained renderer handle identifies the fixture for the original queue.
            unsafe { &mut *native.cast::<FrameFixture>() }.signalled = Some(value);
            1
        }
        unsafe extern "C" fn stop_input() -> u32 {
            1
        }

        let mut fixture = FrameFixture {
            objects: std::array::from_fn(|_| fake()),
            created: 0,
            refuse_at: usize::MAX,
            refs_at_destroy: None,
            signalled: None,
        };
        let renderer = frame_renderer(&mut fixture);
        let fence = fake();
        let reference = unsafe {
            add_ref(raw(&fence).cast());
            ComRef::from_owned(raw(&fence)).unwrap()
        };
        let owner = Owner {
            device: 1,
            queue: 2,
        };
        let replacement = Owner {
            device: 3,
            queue: 4,
        };
        let mut panel = Panel {
            started: Some(Started {
                atlas: Atlas::new(),
                graphics: Graphics::new(),
                ui: fake_ui(renderer.handle.as_ptr()),
                renderer,
                fence: Fence {
                    reference,
                    event: None,
                    armed: None,
                },
                owner,
                failed: true,
                unsignalled: Some(9),
                ring: Ring::default(),
                started: 0,
                previous: 0,
                was_visible: false,
                trace_left: 0,
                logger: None,
                log_user: 0,
            }),
            retry: None,
        };
        let platform = Platform {
            completed: Some(pending),
            signal: Some(signal),
            stop_input: Some(stop_input),
            ..FRAME_PLATFORM
        };
        panel.retire_failed(&platform, 42);
        assert_eq!(fixture.signalled, Some(9));
        assert!(panel.started.is_some());
        panel.retire_failed(&platform, 43);
        assert!(panel.started.is_some());
        panel.retire_failed(
            &Platform {
                completed: Some(completed),
                ..platform
            },
            44,
        );
        assert!(panel.started.is_none());
        assert!(fixture.refs_at_destroy.is_some());
        assert_eq!(fence.refs.get(), 1);
        let retry = panel.retry.unwrap();
        assert!(retry.allows(replacement, 44));
        assert!(!retry.allows(owner, 44));
        assert!(retry.allows(owner, 44 + RETRY_DELAY_MS));
    }

    #[test]
    fn full_ring_waits_its_oldest_submitted_fence() {
        let mut ring = Ring::default();
        for index in 0..SLOTS {
            assert_eq!(ring.select(0), (index, 0));
            ring.submitted(index, index as u64 + 1);
        }
        assert_eq!(ring.select(0), (0, 1));
        assert!(!Ring::slot_ready(0, 1));
        assert!(!Ring::slot_ready(u64::MAX, 1));
        assert!(Ring::slot_ready(1, 1));
        assert_eq!(ring.select(1), (0, 0));
        ring.submitted(0, 4);
        assert_eq!(ring.select(1), (1, 2));
    }

    #[test]
    fn event_retirement_requires_the_armed_value_or_device_removal() {
        let mut armed = Some(7);
        assert!(!retire_fence_event(&mut armed, true, 3));
        assert!(!retire_fence_event(&mut armed, false, 7));
        assert_eq!(armed, Some(7));
        assert!(retire_fence_event(&mut armed, true, 7));
        assert_eq!(armed, None);

        armed = Some(9);
        assert!(retire_fence_event(&mut armed, false, u64::MAX));
        assert_eq!(armed, None);
    }
}
