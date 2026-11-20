// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};
use crate::overlay_native12_abi::{
    RSF_OVERLAY12_TEXTURE_LIMIT, rsf_overlay12_log, rsf_overlay12_platform as Platform,
    rsf_overlay12_texture,
};
use crate::sr_session::has_capacity;
use std::ffi::{CStr, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

const SLOTS: usize = 3;
const TEXTURES: usize = RSF_OVERLAY12_TEXTURE_LIMIT as usize;
const TEXTURE_EDGE: u32 = 16384;
const ATLAS_BATCH: usize = 32;
const STARTUP_MS: u64 = 8000;
const STARTUP_FADE_MS: u64 = 1000;

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
        completed != u64::MAX && completed >= wait_for
    }

    fn submitted(&mut self, slot: usize, value: u64) {
        self.completions[slot] = value;
        self.cursor = (slot + 1) % SLOTS;
        self.serial = value;
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
        slot: usize,
        update: &rsf_overlay_texture_update,
    ) -> Option<()> {
        if update.pixels.is_null()
            || !(1..=TEXTURE_EDGE).contains(&update.width)
            || !(1..=TEXTURE_EDGE).contains(&update.height)
        {
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
        if update.x.checked_add(update.width)? > texture.width
            || update.y.checked_add(update.height)? > texture.height
        {
            return None;
        }
        let upload = unsafe {
            ComRef::from_owned((platform.upload_texture.unwrap())(
                native,
                slot as u32,
                texture.resource.as_ptr(),
                update,
            ))
        }?;
        self.retained[slot].push(upload);
        self.retained[slot].push(texture.resource.clone());
        if let Some(replacement) = replacement {
            match position {
                Some(index) => self.textures[index] = replacement,
                None => self.textures.push(replacement),
            }
        }
        Some(())
    }

    fn record(
        &mut self,
        platform: &Platform,
        native: *mut c_void,
        slot: usize,
        target: &ComRef,
        width: u32,
        height: u32,
    ) -> Option<()> {
        let mut batch = [rsf_overlay_texture_update::default(); ATLAS_BATCH];
        loop {
            let count = unsafe {
                (platform.updates.unwrap())(native, batch.as_mut_ptr(), ATLAS_BATCH as u32)
            };
            if count == 0 {
                break;
            }
            for update in batch.get(..count as usize)? {
                self.update(platform, native, slot, update)?;
            }
        }

        let mut bindings = [rsf_overlay12_texture {
            id: 0,
            resource: std::ptr::null_mut(),
        }; TEXTURES];
        for (binding, texture) in bindings.iter_mut().zip(&self.textures) {
            *binding = rsf_overlay12_texture {
                id: texture.id,
                resource: texture.resource.as_ptr(),
            };
            self.retained[slot].push(texture.resource.clone());
        }
        let recorded = unsafe {
            (platform.record.unwrap())(
                native,
                slot as u32,
                target.as_ptr(),
                width,
                height,
                bindings.as_ptr(),
                self.textures.len() as u32,
            )
        };
        if recorded == 0 {
            return None;
        }

        let mut ids = [0; ATLAS_BATCH];
        loop {
            let count =
                unsafe { (platform.frees.unwrap())(native, ids.as_mut_ptr(), ATLAS_BATCH as u32) };
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

struct Panel {
    native: usize,
    device: usize,
    queue: usize,
    failed: bool,
    ring: Ring,
    atlas: Atlas,
    started: u64,
    previous: u64,
    was_visible: bool,
    trace_left: u32,
    logger: rsf_overlay12_log,
    log_user: usize,
}

static READY: AtomicBool = AtomicBool::new(false);
static PANEL: Mutex<Panel> = Mutex::new(Panel {
    native: 0,
    device: 0,
    queue: 0,
    failed: false,
    ring: Ring {
        completions: [0; SLOTS],
        cursor: 0,
        serial: 0,
    },
    atlas: Atlas::new(),
    started: 0,
    previous: 0,
    was_visible: false,
    trace_left: 0,
    logger: None,
    log_user: 0,
});

impl Panel {
    fn log(&self, message: &CStr) {
        if let Some(call) = self.logger {
            unsafe { call(self.log_user as *mut c_void, message.as_ptr()) };
        }
    }

    fn fail(&mut self, platform: &Platform) -> i32 {
        self.failed = true;
        READY.store(false, Ordering::Release);
        unsafe { (platform.set_visible.unwrap())(0) };
        self.log(c"Native D3D12 overlay refused graphics work; see validation evidence.");
        0
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
    if READY.load(Ordering::Acquire) {
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
    if device.is_null() || queue.is_null() || chain.is_null() || !unsafe { has_capacity(stats) } {
        return 0;
    }
    let Ok(mut panel) = PANEL.try_lock() else {
        return 0;
    };
    if panel.failed {
        return 0;
    }
    if panel.native == 0 {
        panel.logger = logger;
        panel.log_user = user as usize;
        let native = unsafe { (platform.initialize.unwrap())(device, queue, chain, logger, user) };
        if native.is_null() {
            return panel.fail(platform);
        }
        panel.native = native as usize;
        panel.device = device as usize;
        panel.queue = queue as usize;
        panel.started = unsafe { GetTickCount64() };
        panel.previous = panel.started;
        READY.store(true, Ordering::Release);
        panel.log(
            c"RSF native D3D12 overlay ready; Insert opens settings. No D3D11-on-12 resources.",
        );
    }
    if panel.device != device as usize || panel.queue != queue as usize {
        return 0;
    }
    let now = unsafe { GetTickCount64() };
    let active_for = now.saturating_sub(panel.started);
    let visible = unsafe { (platform.visible.unwrap())() } != 0;
    if !visible && unsafe { (*stats).show_performance_hud } == 0 && active_for >= STARTUP_MS {
        return 0;
    }
    let native = panel.native as *mut c_void;
    let completed = unsafe { (platform.completed.unwrap())(native) };
    if completed == u64::MAX {
        return panel.fail(platform);
    }
    let (slot, wait_for) = panel.ring.select(completed);
    if wait_for != 0 {
        // Only already-submitted overlay work can block slot reuse.
        if unsafe { (platform.wait.unwrap())(native, wait_for) } == 0
            || !Ring::slot_ready(unsafe { (platform.completed.unwrap())(native) }, wait_for)
        {
            return panel.fail(platform);
        }
    }
    panel.atlas.retained[slot].clear();
    let mut width = 0;
    let mut height = 0;
    let slot_index = slot as u32;
    let target = unsafe {
        ComRef::from_owned((platform.begin.unwrap())(
            native,
            slot_index,
            chain,
            &mut width,
            &mut height,
        ))
    };
    let Some(target) = target else {
        return panel.fail(platform);
    };
    if visible && !panel.was_visible {
        panel.trace_left = 4;
    }
    panel.was_visible = visible;
    let trace = visible && panel.trace_left != 0;
    if trace {
        panel.trace_left -= 1;
        panel.log(c"Overlay12: input/layout begin");
    }
    let elapsed = now.saturating_sub(panel.previous) as f32 / 1000.0;
    let hint_alpha = STARTUP_MS.saturating_sub(active_for).min(STARTUP_FADE_MS) as f32 / 1000.0;
    let mut decided = rsf_overlay_intent {
        struct_size: size_of::<rsf_overlay_intent>() as u32,
        ..Default::default()
    };
    if unsafe {
        (platform.layout.unwrap())(
            native,
            slot_index,
            width,
            height,
            elapsed,
            hint_alpha,
            stats,
            &mut decided,
        )
    } == 0
    {
        return panel.fail(platform);
    }
    panel.previous = now;
    if decided.close_requested != 0 {
        unsafe { (platform.set_visible.unwrap())(0) };
    }
    if trace {
        panel.log(c"Overlay12: atlas uploads and draw recording");
    }
    if panel
        .atlas
        .record(platform, native, slot, &target, width, height)
        .is_none()
    {
        return panel.fail(platform);
    }
    let Some(value) = panel
        .ring
        .serial
        .checked_add(1)
        .filter(|value| *value != u64::MAX)
    else {
        return panel.fail(platform);
    };
    if trace {
        panel.log(c"Overlay12: queue submission begin");
    }
    if unsafe { (platform.submit.unwrap())(native, slot_index, value) } == 0 {
        // Execute may already have succeeded. Keep the whole native owner
        // alive and refuse future reuse because the retirement is unknown.
        return panel.fail(platform);
    }
    panel.ring.submitted(slot, value);
    if trace {
        panel.log(c"Overlay12: submission completed");
    }
    if !intent.is_null() {
        unsafe { intent.write(decided) };
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{Fake, add_ref, fake, raw};
    use std::ptr::{from_mut, null_mut};

    struct AtlasFixture {
        textures: [Box<Fake>; 2],
        upload: Box<Fake>,
        created: usize,
        uploaded: usize,
        pending: bool,
        free: bool,
        refuse_upload: bool,
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
        native: *mut c_void,
        out: *mut rsf_overlay_texture_update,
        _: u32,
    ) -> u32 {
        let fixture = unsafe { &mut *native.cast::<AtlasFixture>() };
        if std::mem::take(&mut fixture.pending) {
            unsafe { out.write(UPDATE) };
            1
        } else {
            0
        }
    }

    unsafe extern "C" fn frees(native: *mut c_void, out: *mut u64, _: u32) -> u32 {
        let fixture = unsafe { &mut *native.cast::<AtlasFixture>() };
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
        _: u32,
        _: *mut c_void,
        _: *const rsf_overlay_texture_update,
    ) -> *mut c_void {
        let fixture = unsafe { &mut *native.cast::<AtlasFixture>() };
        fixture.uploaded += 1;
        if fixture.refuse_upload {
            return null_mut();
        }
        let staging = raw(&fixture.upload);
        unsafe { add_ref(staging.cast()) };
        staging
    }

    unsafe extern "C" fn draw(
        native: *mut c_void,
        _: u32,
        _: *mut c_void,
        _: u32,
        _: u32,
        bindings: *const rsf_overlay12_texture,
        count: u32,
    ) -> u32 {
        let fixture = unsafe { &*native.cast::<AtlasFixture>() };
        let binding = unsafe { &*bindings };
        u32::from(
            count == 1
                && binding.id == UPDATE.id
                && binding.resource == raw(&fixture.textures[fixture.created - 1]),
        )
    }

    const PLATFORM: Platform = Platform {
        initialize: None,
        visible: None,
        set_visible: None,
        completed: None,
        wait: None,
        begin: None,
        layout: None,
        updates: Some(updates),
        frees: Some(frees),
        create_texture: Some(create),
        upload_texture: Some(upload),
        record: Some(draw),
        submit: None,
    };

    fn fixture() -> AtlasFixture {
        AtlasFixture {
            textures: [fake(), fake()],
            upload: fake(),
            created: 0,
            uploaded: 0,
            pending: true,
            free: false,
            refuse_upload: false,
        }
    }

    #[test]
    fn replacement_and_free_keep_each_slots_draw_and_upload_resources() {
        let mut fixture = fixture();
        let target_resource = fake();
        let target = unsafe { ComRef::from_owned(raw(&target_resource)).unwrap() };
        let mut atlas = Atlas::new();
        let native = from_mut(&mut fixture).cast();
        assert!(atlas.record(&PLATFORM, native, 0, &target, 4, 4).is_some());
        fixture.pending = true;
        fixture.free = true;
        let native = from_mut(&mut fixture).cast();
        assert!(atlas.record(&PLATFORM, native, 1, &target, 4, 4).is_some());
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
        assert!(atlas.update(&PLATFORM, native, 0, &UPDATE).is_some());
        let mut patch = UPDATE;
        patch.is_whole_texture = 0;
        patch.x = u32::MAX;
        assert!(atlas.update(&PLATFORM, native, 0, &patch).is_none());
        assert_eq!(fixture.uploaded, 1);
        patch.x = 1;
        patch.width = 3;
        let native = from_mut(&mut fixture).cast();
        assert!(atlas.update(&PLATFORM, native, 0, &patch).is_some());
        fixture.refuse_upload = true;
        let native = from_mut(&mut fixture).cast();
        assert!(atlas.update(&PLATFORM, native, 0, &UPDATE).is_none());
        assert_eq!(
            atlas.textures[0].resource.as_ptr(),
            raw(&fixture.textures[0])
        );
        assert_eq!(fixture.textures[1].refs.get(), 1);
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
}
