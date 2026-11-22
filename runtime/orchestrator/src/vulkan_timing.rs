// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use rsf_game_sys::windows_path;
use std::ffi::{CStr, OsStr, c_char};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, Once, PoisonError};
use windows_sys::Win32::System::{
    Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    Threading::GetCurrentThreadId,
};

const MAX_RECORDS: u32 = 2048;
const HEADER: &[u8] = b"pid,tid,stage,frame,start_qpc,end_qpc,frequency,duration_ms\n";
const FILE_APPEND_DATA: u32 = 4;
const FILE_SHARE_READ_WRITE: u32 = 3;

struct Timing {
    file: Mutex<File>,
    frequency: i64,
    records: AtomicU32,
}

fn absolute_path(path: &OsStr) -> bool {
    path.encode_wide().take(32768).count() < 32768
        && windows_path::absolute_path(path.encode_wide())
}

fn counter() -> i64 {
    let mut value = 0;
    if unsafe { QueryPerformanceCounter(&mut value) } == 0 {
        return 0;
    }
    value
}

impl Timing {
    fn open(path: &OsStr) -> Option<Self> {
        if !absolute_path(path) {
            return None;
        }
        let mut frequency = 0;
        if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
            return None;
        }
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .access_mode(FILE_APPEND_DATA)
            .share_mode(FILE_SHARE_READ_WRITE)
            .open(path)
            .ok()?;
        if file.metadata().is_ok_and(|metadata| metadata.len() == 0) {
            let _ = file.write_all(HEADER);
        }
        Some(Self {
            file: Mutex::new(file),
            frequency,
            records: AtomicU32::new(0),
        })
    }

    fn write(&self, stage: &str, frame: u64, thread: u32, start: i64, end: i64) {
        if self
            .records
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < MAX_RECORDS).then_some(count + 1)
            })
            .is_err()
        {
            return;
        }
        let elapsed = (end - start) as f64 * 1000.0 / self.frequency as f64;
        let line = format!(
            "{},{thread},{stage},{frame},{start},{end},{},{elapsed:.6}\n",
            std::process::id(),
            self.frequency,
        );
        if line.len() < 512 {
            let _ = self
                .file
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .write_all(line.as_bytes());
        }
    }
}

// The final image's CRT runs this callback on normal DLL detach or executable exit.
unsafe extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
}

static INITIALIZE: Once = Once::new();
static TIMING: AtomicPtr<Timing> = AtomicPtr::new(ptr::null_mut());

extern "C" fn close_timing() {
    let timing = TIMING.swap(ptr::null_mut(), Ordering::AcqRel);
    if !timing.is_null() {
        // The host drains rendering before unloading the module.
        unsafe { drop(Box::from_raw(timing)) };
    }
}

fn begin(timing: &Timing) -> i64 {
    if timing.records.load(Ordering::Relaxed) < MAX_RECORDS {
        counter()
    } else {
        0
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_vk_timing_begin() -> i64 {
    timing_begin()
}

pub(crate) fn timing_begin() -> i64 {
    INITIALIZE.call_once(|| {
        let Some(timing) = crate::diagnostic_preferences::value("RSF_VULKAN_TIMING_FILE")
            .and_then(|path| Timing::open(&path))
        else {
            return;
        };
        let timing = allocate_handle(timing);
        if !timing.is_null() {
            if unsafe { atexit(close_timing) } == 0 {
                TIMING.store(timing, Ordering::Release);
            } else {
                unsafe { drop(Box::from_raw(timing)) };
            }
        }
    });
    unsafe { TIMING.load(Ordering::Acquire).as_ref() }.map_or(0, begin)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vk_timing_end(stage: *const c_char, frame: u64, start: i64) {
    if start > 0
        && !stage.is_null()
        && let Ok(stage) = unsafe { CStr::from_ptr(stage) }.to_str()
    {
        timing_end(stage, frame, start);
    }
}

pub(crate) fn timing_end(stage: &str, frame: u64, start: i64) {
    if start <= 0 {
        return;
    }
    let end = counter();
    let Some(timing) = (unsafe { TIMING.load(Ordering::Acquire).as_ref() }) else {
        return;
    };
    if end >= start {
        timing.write(stage, frame, unsafe { GetCurrentThreadId() }, start, end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_bounded_csv_and_does_not_repeat_the_header() {
        assert!(!absolute_path(OsStr::new(r"relative\timing.csv")));
        assert!(!absolute_path(OsStr::new(r"D:timing.csv")));
        assert!(absolute_path(OsStr::new(r"\\server\share\timing.csv")));
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("rsf-vk-timing-{}-{suffix}.csv", std::process::id()));
        let timing = Timing::open(path.as_os_str()).unwrap();
        let frequency = timing.frequency;
        for frame in 1..=u64::from(MAX_RECORDS) + 2 {
            timing.write("submit", frame, 17, 1, 1 + frequency);
        }
        assert_eq!(begin(&timing), 0);
        drop(timing);
        drop(Timing::open(path.as_os_str()).unwrap());

        let text = fs::read_to_string(&path).unwrap();
        fs::remove_file(path).unwrap();
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap().as_bytes(),
            &HEADER[..HEADER.len() - 1]
        );
        assert_eq!(
            lines.next().unwrap(),
            format!(
                "{},17,submit,1,1,{},{frequency},1000.000000",
                std::process::id(),
                1 + frequency
            )
        );
        assert_eq!(lines.count(), MAX_RECORDS as usize - 1);
    }
}
