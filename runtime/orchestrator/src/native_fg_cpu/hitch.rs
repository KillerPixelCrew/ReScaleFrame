// SPDX-License-Identifier: MIT
use super::{Source, lock, now_qpc, win32};
use crate::native_fg::sink;
use crate::owned_win32_handle;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

const SAMPLES_PER_SOURCE: u32 = 3;
const TOTAL_SAMPLES: u32 = 36;
const FIRST_SAMPLE_MS: f64 = 15.0;
const LOG_CAPACITY: usize = 1900;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Interval {
    source: Source,
    submission: u64,
    opened_qpc: u64,
}

struct RenderThread {
    id: u32,
    handle: Arc<OwnedHandle>,
    session: u64,
    interval: Option<Interval>,
}

#[derive(PartialEq, Eq)]
enum Worker {
    Idle,
    Watching,
}

struct Watch {
    thread: Option<RenderThread>,
    worker: Worker,
    samples: Samples,
}

static WATCH: Mutex<Watch> = Mutex::new(Watch {
    thread: None,
    worker: Worker::Idle,
    samples: Samples {
        source: Source { session: 0, id: 0 },
        taken: 0,
        total: 0,
    },
});

static CHANGED: Condvar = Condvar::new();

fn close_interval(current: &mut Option<Interval>, source: Source, submission: u64) {
    if current
        .is_some_and(|interval| interval.source == source && interval.submission == submission)
    {
        *current = None;
    }
}

pub(super) fn close(source: Source, submission: u64) {
    if let Some(thread) = lock(&WATCH).thread.as_mut() {
        close_interval(&mut thread.interval, source, submission);
        CHANGED.notify_one();
    }
}

pub(super) fn end_session(session: u64) {
    let mut state = lock(&WATCH);
    if state
        .thread
        .as_ref()
        .is_some_and(|thread| thread.session == session)
    {
        state.thread = None;
        CHANGED.notify_one();
    }
}

pub(super) fn open(source: Source, submission: u64, opened_qpc: u64) {
    let mut state = lock(&WATCH);
    if state.samples.total == TOTAL_SAMPLES || opened_qpc == 0 {
        return;
    }
    let id = win32::thread_id();
    if state
        .thread
        .as_ref()
        .is_none_or(|thread| thread.id != id || thread.session != source.session)
    {
        let handle = win32::duplicate_render_thread();
        // The callback transfers a real duplicated handle to this thread.
        let Some(handle) = (unsafe { owned_win32_handle(handle) }) else {
            state.thread = None;
            CHANGED.notify_one();
            return;
        };
        state.thread = Some(RenderThread {
            id,
            handle: Arc::new(handle),
            session: source.session,
            interval: None,
        });
    }
    let thread = state.thread.as_mut().expect("the render thread is open");
    thread.interval = Some(Interval {
        source,
        submission,
        opened_qpc,
    });
    CHANGED.notify_one();
    if state.worker == Worker::Idle {
        let frequency = win32::qpc_frequency();
        if frequency != 0
            && thread::Builder::new()
                .name("RSF render hitch".into())
                .spawn(move || watch_thread(frequency))
                .is_ok()
        {
            state.worker = Worker::Watching;
        }
    }
}

#[derive(Default)]
struct Samples {
    source: Source,
    taken: u32,
    total: u32,
}

impl Samples {
    fn take_if_due(&mut self, source: Source, elapsed_ms: f64) -> bool {
        if source != self.source {
            self.source = source;
            self.taken = 0;
        }
        if self.total == TOTAL_SAMPLES
            || self.taken == SAMPLES_PER_SOURCE
            || elapsed_ms < FIRST_SAMPLE_MS * f64::from(self.taken + 1)
        {
            return false;
        }
        self.taken += 1;
        self.total += 1;
        true
    }
}

fn watch_thread(frequency: u64) {
    let mut addresses = [0; 513]; // The instruction pointer followed by up to 512 stack words.
    loop {
        let mut state = lock(&WATCH);
        let (interval, handle, elapsed_ms) = loop {
            if state.thread.is_none() || state.samples.total == TOTAL_SAMPLES {
                state.thread = None;
                state.worker = Worker::Idle;
                return;
            }
            let target = state
                .thread
                .as_ref()
                .and_then(|thread| Some((thread.interval?, Arc::clone(&thread.handle))));
            if let Some((interval, handle)) = target {
                let elapsed_ms = now_qpc().saturating_sub(interval.opened_qpc) as f64 * 1000.0
                    / frequency as f64;
                if state.samples.take_if_due(interval.source, elapsed_ms) {
                    break (interval, handle, elapsed_ms);
                }
                if state.samples.taken < SAMPLES_PER_SOURCE {
                    let due_ms = FIRST_SAMPLE_MS * f64::from(state.samples.taken + 1);
                    let delay = Duration::from_secs_f64((due_ms - elapsed_ms).max(1.0) / 1000.0);
                    state = CHANGED.wait_timeout(state, delay).unwrap().0;
                    continue;
                }
            }
            state = CHANGED.wait(state).unwrap();
        };
        drop(state);
        // The native leaf resumes the thread before returning any copied addresses.
        let count = unsafe {
            win32::sample_render_thread(
                handle.as_raw_handle(),
                addresses.as_mut_ptr(),
                addresses.len(),
            )
        };
        let still_current = lock(&WATCH)
            .thread
            .as_ref()
            .is_some_and(|thread| thread.interval == Some(interval));
        if count != 0 && still_current {
            report(
                interval.source.id,
                elapsed_ms,
                &addresses[..count.min(addresses.len())],
            );
        }
    }
}

fn report(source: u64, elapsed_ms: f64, addresses: &[u64]) {
    let mut message = format!("Render hitch source={source} open {elapsed_ms:.1} ms; executing");
    let mut bytes = [0; 384];
    let mut append = |message: &mut String, address| {
        let remaining = LOG_CAPACITY - message.len() - 1;
        let capacity = remaining.min(bytes.len());
        let size = unsafe { win32::describe_address(address, bytes.as_mut_ptr().cast(), capacity) };
        if size == 0 || size >= capacity {
            return false;
        }
        let description = String::from_utf8_lossy(&bytes[..size]);
        if description.len() >= remaining {
            return false;
        }
        message.push_str(&description);
        true
    };
    if let Some((&instruction, stack)) = addresses.split_first() {
        append(&mut message, instruction);
        message.push_str("; stack code addresses:");
        let mut listed = 0;
        for &address in stack {
            if listed == 40 || message.len() + 96 >= LOG_CAPACITY {
                break;
            }
            listed += u32::from(append(&mut message, address));
        }
    }
    sink(false).say(&message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_close_keeps_the_current_submission() {
        let source = Source { session: 2, id: 8 };
        let mut current = Some(Interval {
            source,
            submission: 5,
            opened_qpc: 99,
        });
        close_interval(
            &mut current,
            Source {
                session: 1,
                ..source
            },
            5,
        );
        close_interval(&mut current, source, 4);
        assert!(current.is_some());
        close_interval(&mut current, source, 5);
        assert!(current.is_none());
    }

    #[test]
    fn samples_are_bounded_per_source_and_across_the_process() {
        let mut samples = Samples::default();
        for session in 1..=12 {
            let source = Source { session, id: 8 };
            assert!(!samples.take_if_due(source, 14.9));
            assert!(samples.take_if_due(source, 15.0));
            assert!(!samples.take_if_due(source, 29.9));
            assert!(samples.take_if_due(source, 30.0));
            assert!(samples.take_if_due(source, 45.0));
            assert!(!samples.take_if_due(source, 1000.0));
        }
        assert_eq!(samples.total, 36);
        assert!(!samples.take_if_due(Source { session: 13, id: 8 }, 1000.0));
    }
}
