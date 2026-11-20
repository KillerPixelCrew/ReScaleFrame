// SPDX-License-Identifier: MIT
use crate::native_cpu::read_frame;
use rsf_game_sys::{
    RSF_GAME_CPU_SIMULATION_END, RSF_GAME_RENDER_PRESENT, RSF_GAME_RENDER_SUBMISSION,
    rsf_game_render_pass,
};
use std::mem::{offset_of, size_of, take};
use std::sync::Mutex;

pub(crate) mod streamline;
use streamline::StreamlineFrames;

const INVALID: i32 = -1;
const NOT_READY: i32 = -8;
const EXECUTION_SLOTS: usize = 128;
const PASS_PREFIX: usize = offset_of!(rsf_game_render_pass, source_frame_id) + size_of::<u64>();

// Values consumed by the existing Vulkan SDK marker bridges.
#[derive(Clone, Copy)]
#[repr(i32)]
enum Marker {
    SubmitBegin = 2,
    SubmitEnd = 3,
    PresentBegin = 4,
    PresentEnd = 5,
}

impl Marker {
    fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Frame {
    session: u64,
    id: u64,
}

#[derive(Clone, Copy)]
struct Execution {
    frame: Frame,
    stages: u8,
}

impl Execution {
    fn advance(&mut self, marker: Marker) -> i32 {
        use Marker::*;
        let seen = |marker: Marker| self.stages & marker.bit() != 0;
        let allowed = match marker {
            SubmitBegin => !seen(PresentBegin),
            SubmitEnd => seen(SubmitBegin) && !seen(PresentBegin),
            PresentBegin => seen(SubmitEnd),
            PresentEnd => seen(PresentBegin),
        };
        if !allowed {
            return NOT_READY;
        }
        if seen(marker) {
            return 0;
        }
        self.stages |= marker.bit();
        marker as i32
    }
}

#[derive(Default)]
struct LegacyPresent {
    rendered: u64,
    presenting: u64,
}

impl LegacyPresent {
    fn present(&mut self, begin: bool) -> u64 {
        if !begin {
            return take(&mut self.presenting);
        }
        if self.presenting != 0 {
            return 0;
        }
        self.presenting = take(&mut self.rendered);
        self.presenting
    }
}

struct Frames {
    executions: [Option<Execution>; EXECUTION_SLOTS],
    legacy: LegacyPresent,
    streamline: StreamlineFrames,
}

impl Frames {
    const fn new() -> Self {
        Self {
            executions: [None; EXECUTION_SLOTS],
            legacy: LegacyPresent {
                rendered: 0,
                presenting: 0,
            },
            streamline: StreamlineFrames::new(),
        }
    }

    fn slot(&mut self, frame: Frame) -> &mut Option<Execution> {
        &mut self.executions[(frame.id % EXECUTION_SLOTS as u64) as usize]
    }

    fn boundary(&mut self, frame: Frame, skipped: bool) {
        if skipped {
            self.streamline.cancel(frame);
        }
        let slot = self.slot(frame);
        let matches = slot.is_some_and(|record| record.frame == frame);
        if skipped {
            if matches {
                *slot = None;
            }
        } else if !matches {
            *slot = Some(Execution { frame, stages: 0 });
        }
    }
}

// The native owner serializes SDK operations separately. No provider work
// or CPU ledger access runs while holding this state lock.
static FRAMES: Mutex<Frames> = Mutex::new(Frames::new());

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_frames_reset() {
    *FRAMES.lock().unwrap() = Frames::new();
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_frame_boundary(session: u64, frame: u64, skipped: u32) {
    FRAMES
        .lock()
        .unwrap()
        .boundary(Frame { session, id: frame }, skipped != 0);
}

// A positive result identifies one marker, zero is a duplicate and a
// negative result is the public error. Only the prefix through source_frame_id is read.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_latencyvk_execution(
    pass: *const rsf_game_render_pass,
    begin: u32,
) -> i32 {
    if pass.is_null() || (unsafe { (*pass).struct_size } as usize) < PASS_PREFIX {
        return INVALID;
    }
    let frame = Frame {
        session: unsafe { (*pass).session_id },
        id: unsafe { (*pass).source_frame_id },
    };
    if frame.session == 0 || frame.id == 0 {
        return INVALID;
    }
    let marker = match (unsafe { (*pass).role }, begin) {
        (RSF_GAME_RENDER_SUBMISSION, 1) => Marker::SubmitBegin,
        (RSF_GAME_RENDER_SUBMISSION, 0) => Marker::SubmitEnd,
        (RSF_GAME_RENDER_PRESENT, 1) => Marker::PresentBegin,
        (RSF_GAME_RENDER_PRESENT, 0) => Marker::PresentEnd,
        _ => return INVALID,
    };
    if !read_frame(frame.session, frame.id).is_some_and(|facts| {
        facts.failed == 0 && facts.stage_mask & (1 << RSF_GAME_CPU_SIMULATION_END) != 0
    }) {
        return NOT_READY;
    }
    let mut frames = FRAMES.lock().unwrap();
    match frames.slot(frame) {
        Some(record) if record.frame == frame => record.advance(marker),
        _ => NOT_READY,
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_legacy_render(frame: u64) {
    FRAMES.lock().unwrap().legacy.rendered = frame;
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_legacy_present(begin: u32) -> u64 {
    FRAMES.lock().unwrap().legacy.present(begin != 0)
}
