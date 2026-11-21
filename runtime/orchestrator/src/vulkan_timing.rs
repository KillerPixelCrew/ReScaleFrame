// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use std::ffi::{CStr, OsStr, c_char};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
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
    let mut units = path.encode_wide();
    let first = units.next();
    let second = units.next();
    let third = units.next();
    let backslash = u16::from(b'\\');
    let drive = second == Some(u16::from(b':'))
        && (third == Some(backslash) || third == Some(u16::from(b'/')));
    let unc = first == Some(backslash) && second == Some(backslash);
    (drive || unc) && units.take(32765).count() < 32765
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

#[unsafe(no_mangle)]
extern "C" fn rsf_core_vk_timing_open() -> *mut Timing {
    std::env::var_os("RSF_VULKAN_TIMING_FILE")
        .and_then(|path| Timing::open(&path))
        .map_or(ptr::null_mut(), allocate_handle)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vk_timing_close(timing: *mut Timing) {
    if !timing.is_null() {
        drop(unsafe { Box::from_raw(timing) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vk_timing_begin(timing: *const Timing) -> i64 {
    let Some(timing) = (unsafe { timing.as_ref() }) else {
        return 0;
    };
    if timing.records.load(Ordering::Relaxed) < MAX_RECORDS {
        counter()
    } else {
        0
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vk_timing_end(
    timing: *const Timing,
    stage: *const c_char,
    frame: u64,
    start: i64,
) {
    if start <= 0 || stage.is_null() {
        return;
    }
    let end = counter();
    let Some(timing) = (unsafe { timing.as_ref() }) else {
        return;
    };
    if end >= start
        && let Ok(stage) = unsafe { CStr::from_ptr(stage) }.to_str()
    {
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
        assert_eq!(unsafe { rsf_core_vk_timing_begin(&timing) }, 0);
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
