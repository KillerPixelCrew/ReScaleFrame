// SPDX-License-Identifier: MIT
//! A record of every Rust panic, left before the process can die of it.
//!
//! A panic that reaches an `extern "C"` boundary aborts through `__fastfail`, which skips vectored
//! exception handlers and, with Windows Error Reporting off, leaves nothing behind; the default
//! hook writes to a stderr a game does not have. Every binary links its own copy of std, so each
//! installs this hook from its own entry point.
//!
//! The record goes to `%LOCALAPPDATA%\ReScaleFrame\panic.log` (appended, next to each game's log
//! folder) and to OutputDebugString. It is formatted into a stack buffer and written with one
//! open, write, flush and close, so recording takes no lock and allocates nothing.
use std::fmt::{self, Write};
use std::panic::PanicHookInfo;
use std::ptr::{null, null_mut};
use std::sync::Once;
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE, INVALID_HANDLE_VALUE, SYSTEMTIME};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FlushFileBuffers, OPEN_ALWAYS, WriteFile,
};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows_sys::Win32::System::Environment::GetEnvironmentVariableW;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

/// Installs the hook once for the binary that links this copy, ahead of the hook it replaces,
/// which still runs.
pub fn install() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let record = Record::of(info);
            unsafe { OutputDebugStringA(record.terminated().as_ptr()) };
            let mut path = [0; PATH_CAPACITY];
            if let Some(path) = log_path(&mut path) {
                append(path, record.text());
            }
            previous(info);
        }));
    });
}

const RECORD_CAPACITY: usize = 2048;
const PATH_CAPACITY: usize = 1024;
const DIRECTORY: &[u8] = b"\\ReScaleFrame";
const FILE: &[u8] = b"\\panic.log";

/// One record line in a fixed buffer: text that does not fit is cut at a character boundary,
/// leaving room for CRLF and a NUL.
struct Record {
    bytes: [u8; RECORD_CAPACITY],
    length: usize,
}

impl Write for Record {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let room = RECORD_CAPACITY - 3 - self.length;
        let take = text.floor_char_boundary(room.min(text.len()));
        self.bytes[self.length..self.length + take].copy_from_slice(&text.as_bytes()[..take]);
        self.length += take;
        Ok(())
    }
}

impl Record {
    /// `2026-11-25 00:43:14.123 ReScaleFrame.Runtime.dll thread 88768 panicked at file:line:col: message`
    fn of(info: &PanicHookInfo<'_>) -> Self {
        let mut record = Self {
            bytes: [0; RECORD_CAPACITY],
            length: 0,
        };
        let mut time: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut time) };
        let _ = write!(
            record,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} ",
            time.wYear,
            time.wMonth,
            time.wDay,
            time.wHour,
            time.wMinute,
            time.wSecond,
            time.wMilliseconds
        );
        record.module_name();
        let _ = write!(record, " thread {} panicked", unsafe {
            GetCurrentThreadId()
        });
        if let Some(location) = info.location() {
            let _ = write!(record, " at {location}");
        }
        let message = info.payload_as_str().unwrap_or("Box<dyn Any>");
        let _ = write!(record, ": {message}");
        // Room for these was kept back.
        record.bytes[record.length..record.length + 3].copy_from_slice(b"\r\n\0");
        record.length += 2;
        record
    }

    /// The file name of the binary this copy of the hook is linked into.
    fn module_name(&mut self) {
        let mut module: HMODULE = null_mut();
        let flags =
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
        let address = install as fn() as *const u16;
        let mut path = [0; PATH_CAPACITY];
        let length = if unsafe { GetModuleHandleExW(flags, address, &mut module) } != 0 {
            (unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), PATH_CAPACITY as u32) })
                as usize
        } else {
            0
        };
        let path = &path[..length.min(PATH_CAPACITY)];
        let name = path
            .rsplit(|&unit| unit == u16::from(b'\\') || unit == u16::from(b'/'))
            .next()
            .unwrap_or(path);
        if name.is_empty() {
            let _ = self.write_str("unknown module");
        }
        for unit in char::decode_utf16(name.iter().copied()) {
            let _ = self.write_char(unit.unwrap_or(char::REPLACEMENT_CHARACTER));
        }
    }

    fn text(&self) -> &[u8] {
        &self.bytes[..self.length]
    }

    fn terminated(&self) -> &[u8] {
        &self.bytes[..=self.length]
    }
}

/// `%LOCALAPPDATA%\ReScaleFrame\panic.log`, NUL-terminated, creating the folder if needed.
fn log_path(buffer: &mut [u16; PATH_CAPACITY]) -> Option<&[u16]> {
    const VARIABLE: [u16; 13] = ascii(b"LOCALAPPDATA\0");
    let room = (PATH_CAPACITY - DIRECTORY.len() - FILE.len() - 1) as u32;
    let length = unsafe { GetEnvironmentVariableW(VARIABLE.as_ptr(), buffer.as_mut_ptr(), room) };
    if length == 0 || length >= room {
        return None;
    }
    let mut end = length as usize;
    for &byte in DIRECTORY {
        buffer[end] = u16::from(byte);
        end += 1;
    }
    buffer[end] = 0;
    // An existing folder is the usual answer.
    unsafe { CreateDirectoryW(buffer.as_ptr(), null()) };
    for &byte in FILE {
        buffer[end] = u16::from(byte);
        end += 1;
    }
    buffer[end] = 0;
    Some(&buffer[..=end])
}

const fn ascii<const N: usize>(text: &[u8; N]) -> [u16; N] {
    let mut wide = [0; N];
    let mut index = 0;
    while index < N {
        wide[index] = text[index] as u16;
        index += 1;
    }
    wide
}

/// Appends `text` to the NUL-terminated `path` and flushes it to disk before closing.
fn append(path: &[u16], text: &[u8]) -> bool {
    let file = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_APPEND_DATA,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return false;
    }
    let mut written = 0;
    let complete = unsafe {
        WriteFile(
            file,
            text.as_ptr(),
            text.len() as u32,
            &mut written,
            null_mut(),
        ) != 0
            && FlushFileBuffers(file) != 0
    };
    unsafe { CloseHandle(file) };
    complete && written as usize == text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStrExt;

    #[test]
    fn a_caught_panic_leaves_its_record() {
        let directory = std::env::temp_dir().join(format!("rsf-panic-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("panic.log");
        let _ = std::fs::remove_file(&file);
        let path: Vec<u16> = file.as_os_str().encode_wide().chain([0]).collect();
        let thread = unsafe { GetCurrentThreadId() };
        // Only this test's thread records; panics on other test threads pass through.
        let previous = std::sync::Arc::new(std::panic::take_hook());
        let chained = previous.clone();
        std::panic::set_hook(Box::new(move |info| {
            if unsafe { GetCurrentThreadId() } == thread {
                append(&path, Record::of(info).text());
            } else {
                chained(info);
            }
        }));
        let line = line!() + 1;
        let caught = std::panic::catch_unwind(|| panic!("FSR4 warm-up {}", 4));
        drop(std::panic::take_hook());
        let previous = std::sync::Arc::into_inner(previous).unwrap();
        std::panic::set_hook(previous);
        assert!(caught.is_err());

        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        let executable = std::env::current_exe().unwrap();
        let executable = executable.file_name().unwrap().to_str().unwrap();
        let expected = format!(
            " {executable} thread {thread} panicked at {}:{line}:",
            file!()
        );
        assert!(text.contains(&expected), "{text}");
        assert!(text.ends_with(": FSR4 warm-up 4\r\n"), "{text}");
        // The timestamp leads: yyyy-mm-dd hh:mm:ss.mmm.
        let stamp = text.as_bytes();
        assert_eq!(
            (stamp[4], stamp[7], stamp[10], stamp[13], stamp[19]),
            (b'-', b'-', b' ', b':', b'.')
        );
    }

    #[test]
    fn a_long_message_is_cut_on_a_character_boundary() {
        let mut record = Record {
            bytes: [0; RECORD_CAPACITY],
            length: 0,
        };
        let _ = record.write_str(&"é".repeat(RECORD_CAPACITY));
        assert_eq!(record.length, RECORD_CAPACITY - 4);
        assert!(std::str::from_utf8(record.text()).is_ok());
    }
}
