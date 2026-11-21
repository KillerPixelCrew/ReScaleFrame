// SPDX-License-Identifier: MIT
use super::*;
use crate::frame_dump::DumpApi;
use rsf_game_sys::{RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record};

const SESSION: u64 = 0x0000_4d4f_5449_4f4e;
const RENDER_SUBMIT: u32 = 2;
const RAW: u32 = 0;
const DENSE_MOTION: u32 = 7;

#[derive(Clone, Copy)]
pub(super) struct Schedule {
    pub enabled: bool,
    pub start: u32,
    pub count: u32,
    pub interval: u32,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            enabled: false,
            start: 600,
            count: 3,
            interval: 300,
        }
    }
}

pub(super) struct Capture {
    api: DumpApi,
    interval: u64,
    frames: u64,
    remaining: u32,
    next: u64,
    last_main: Option<(u32, u64)>,
    active: Option<Active>,
}

struct Active {
    record: rsf_frame_record,
    generation: u64,
    context: usize,
    position: usize,
    texture: usize,
}

impl Capture {
    pub fn bind(p: &Platform, settings: Schedule, hash: *const c_char, log: Log) -> Option<Self> {
        if unsafe { (p.sr_capture)() } {
            log.text(c"Anomaly motion capture refused: SRStageCapture owns the frame dump service this run.");
            return None;
        }
        let Some(api) = DumpApi::bind() else {
            log.text(c"Anomaly motion capture refused: Runtime frame dump exports missing.");
            return None;
        };
        let path = std::env::var_os("RSF_DUMP_DIR")
            .filter(|path| !path.is_empty())
            .or_else(|| {
                std::env::var_os("LOCALAPPDATA")
                    .filter(|path| !path.is_empty())
                    .map(|root| {
                        Path::new(&root)
                            .join("ReScaleFrame")
                            .join("Anomaly")
                            .into_os_string()
                    })
            });
        let Some(path) = path.and_then(|path| CString::new(path.to_string_lossy().as_bytes()).ok())
        else {
            log.text(c"Anomaly motion capture refused: LOCALAPPDATA unavailable.");
            return None;
        };
        unsafe {
            (api.configure)(
                path.as_ptr(),
                c"Anomaly".as_ptr(),
                hash,
                log.callback,
                log.user as Raw,
            )
        };
        Some(Self {
            api,
            interval: settings.interval.into(),
            frames: 0,
            remaining: settings.count,
            next: settings.start.into(),
            last_main: None,
            active: None,
        })
    }

    fn abandon(&mut self, counters: &mut Counters) {
        if let Some(active) = self.active.take() {
            unsafe { (self.api.complete)(&active.record, 0x80004005_u32 as i32) };
            counters.capture_failed += 1;
        }
    }

    pub fn frame(
        &mut self,
        facts: &WindowFacts,
        view: &DeviceView,
        written: bool,
        counters: &mut Counters,
    ) {
        if facts.view != MAIN {
            return;
        }
        let main = (facts.frame, facts.generation);
        if self.last_main != Some(main) {
            self.frames += 1;
            self.last_main = Some(main);
        }
        self.abandon(counters);
        if !written {
            return;
        }
        unsafe { (self.api.poll)() };
        let scheduled = self.remaining != 0 && self.frames >= self.next;
        if scheduled {
            unsafe { (self.api.request)() };
        }
        if unsafe { (self.api.requested)() } == 0 {
            return;
        }
        // Every field in this FFI record (including its numeric C enum aliases) admits zero.
        let record = rsf_frame_record {
            struct_size: size_of::<rsf_frame_record>() as u32,
            abi_version: RSF_GAME_FRAME_ABI_VERSION,
            frame_id: u64::from(facts.frame) + 1,
            session_id: SESSION,
            phase: RENDER_SUBMIT,
            render_width: view.width,
            render_height: view.height,
            output_width: view.width,
            output_height: view.height,
            ..unsafe { std::mem::zeroed() }
        };
        if unsafe { (self.api.begin_record)(&record, 0) } != 0 {
            self.active = Some(Active {
                record,
                generation: facts.generation,
                context: facts.context as usize,
                position: facts.position_surface as usize,
                texture: view.texture as usize,
            });
            counters.captures += 1;
            if scheduled {
                self.remaining -= 1;
                self.next = self.frames.saturating_add(self.interval);
            }
        }
    }

    pub fn finish(&mut self, window: &window::Window, texture: Raw, counters: &mut Counters) {
        let Some(active) = self.active.take() else {
            return;
        };
        if window.view != MAIN
            || active.record.frame_id != u64::from(window.frame) + 1
            || active.generation != window.generation
            || active.context != window.context as usize
            || active.position != window.position_surface as usize
            || active.texture != texture as usize
            || !window.target_ok
        {
            self.active = Some(active);
            self.abandon(counters);
            return;
        }
        let record = active.record;
        for (name, description, resource, mode, scale) in [
            (c"anomaly_motion", c"Motion RTV2 after the G-buffer pass, RGBA16F (current-minus-previous UV x, UV y down, validity, 0)",
                texture, RAW, 1.0),
            (c"anomaly_motion_view", c"Motion RTV2 as dense motion (UV x20)", texture, DENSE_MOTION, 20.0),
            (c"anomaly_gbuffer_position", c"rt_Position after the G-buffer pass (packed normal, view Z, material)",
                window.position_surface, RAW, 1.0),
        ] {
            if resource.is_null() || window.context.is_null() {
                continue;
            }
            if unsafe { (self.api.texture_record)(window.context, &record, name.as_ptr(), description.as_ptr(),
                resource, mode, scale) } != 0 {
                counters.images += 1;
            } else {
                counters.capture_failed += 1;
            }
        }
        unsafe { (self.api.complete)(&record, 0) };
    }

    pub fn cancel(&mut self, counters: &mut Counters) {
        self.abandon(counters);
        unsafe { (self.api.cancel)(SESSION) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Dump {
        requested: bool,
        requests: u32,
        admit: bool,
        begun: Vec<u64>,
        completed: Vec<(u64, i32)>,
        copies: u32,
    }
    thread_local! { static DUMP: RefCell<Dump> = RefCell::default(); }

    unsafe extern "C" fn configure(
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: rsf_game_log_fn,
        _: Raw,
    ) {
    }
    unsafe extern "C" fn request() {
        DUMP.with_borrow_mut(|d| {
            d.requested = true;
            d.requests += 1;
        });
    }
    unsafe extern "C" fn poll() {}
    unsafe extern "C" fn requested() -> i32 {
        DUMP.with_borrow(|d| d.requested.into())
    }
    unsafe extern "C" fn begin(record: &rsf_frame_record, _: u32) -> i32 {
        DUMP.with_borrow_mut(|d| {
            if !d.admit {
                return 0;
            }
            d.requested = false;
            d.begun.push(record.frame_id);
            1
        })
    }
    unsafe extern "C" fn texture(
        _: Raw,
        _: &rsf_frame_record,
        _: *const c_char,
        _: *const c_char,
        _: Raw,
        _: u32,
        _: f32,
    ) -> i32 {
        DUMP.with_borrow_mut(|d| d.copies += 1);
        1
    }
    unsafe extern "C" fn complete(record: &rsf_frame_record, result: i32) {
        DUMP.with_borrow_mut(|d| d.completed.push((record.frame_id, result)));
    }
    unsafe extern "C" fn cancel(_: u64) {}

    #[test]
    fn schedule_advances_on_main_identity_and_only_spends_an_accepted_capture() {
        let api = DumpApi {
            configure,
            request,
            poll,
            requested,
            begin_record: begin,
            texture_record: texture,
            complete,
            cancel,
            result: None,
        };
        let mut capture = Capture {
            api,
            interval: 2,
            frames: 0,
            remaining: 2,
            next: 2,
            last_main: None,
            active: None,
        };
        let mut counters = Counters::default();
        let texture = ptr::dangling_mut::<u8>().cast();
        let view = DeviceView {
            width: 640,
            height: 480,
            texture,
            ..Default::default()
        };
        let frame = |view_id, frame| WindowFacts {
            device: ptr::null_mut(),
            context: texture,
            backend: ptr::null_mut(),
            position_rtv: texture,
            position_surface: texture,
            generation: 3,
            frame,
            view: view_id,
        };
        capture.frame(&frame(MAIN, 10), &view, true, &mut counters);
        capture.frame(&frame(1, 11), &view, true, &mut counters);
        capture.frame(&frame(MAIN, 10), &view, true, &mut counters);
        DUMP.with_borrow(|d| assert_eq!(d.requests, 0));
        capture.frame(&frame(MAIN, 12), &view, true, &mut counters);
        assert_eq!(capture.remaining, 2);

        DUMP.with_borrow_mut(|d| d.admit = true);
        let accepted = frame(MAIN, 13);
        capture.frame(&accepted, &view, true, &mut counters);
        assert_eq!(capture.remaining, 1);
        let mut window = window::Window::default();
        window::begin(
            &mut window,
            texture,
            texture,
            texture,
            accepted.frame,
            MAIN,
            accepted.generation,
        );
        window::open(&mut window, true);
        capture.finish(&window, texture, &mut counters);
        assert_eq!(counters.images, 3);

        capture.frame(&frame(1, 14), &view, true, &mut counters);
        capture.frame(&frame(MAIN, 15), &view, true, &mut counters);
        assert!(capture.active.is_none());
        capture.frame(&frame(MAIN, 16), &view, true, &mut counters);
        assert_eq!(capture.remaining, 0);

        // A close from a different source, even on the same immediate context
        // and allocation, must never copy into the accepted record.
        window::begin(
            &mut window,
            texture,
            texture,
            texture,
            16,
            MAIN,
            accepted.generation + 1,
        );
        window::open(&mut window, true);
        capture.finish(&window, texture, &mut counters);
        assert_eq!(counters.capture_failed, 1);
        DUMP.with_borrow(|d| {
            assert_eq!(d.begun, [14, 17]);
            assert_eq!(d.copies, 3);
            assert_eq!(d.completed, [(14, 0), (17, 0x80004005_u32 as i32)]);
        });
    }
}
