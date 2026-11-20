// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats};
use crate::overlay_native12_abi::{rsf_overlay12_log, rsf_overlay12_platform as Platform};
use crate::sr_session::has_capacity;
use std::ffi::{CStr, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

const SLOTS: usize = 3;
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

struct Panel {
    native: usize,
    device: usize,
    queue: usize,
    failed: bool,
    ring: Ring,
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
    if unsafe { (platform.record.unwrap())(native, slot_index, target.as_ptr(), width, height) }
        == 0
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
