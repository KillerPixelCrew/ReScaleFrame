// SPDX-License-Identifier: MIT
use std::ffi::{CStr, OsString, c_char, c_void};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::{
    Foundation::MAX_PATH,
    Globalization::{CP_UTF8, MultiByteToWideChar},
};

const MAX_NAME_BYTES: usize = 1024;
const LINE_BYTES: usize = MAX_NAME_BYTES + 63;
const MAX_LINES: u32 = 2_000_000;
type Write = unsafe extern "C" fn(*mut c_void, *const u8, usize);

fn parse_rva(bytes: &[u8]) -> Option<u32> {
    let (digits, radix) = if let Some(digits) = bytes
        .strip_prefix(b"0x")
        .or_else(|| bytes.strip_prefix(b"0X"))
    {
        (digits, 16)
    } else {
        (bytes, 10)
    };
    let start = digits
        .iter()
        .position(|&b| !matches!(b, b'\t'..=b'\r' | b' '))?;
    let end = digits.iter().rposition(|b| *b != b' ')? + 1;
    let digits = std::str::from_utf8(digits.get(start..end)?).ok()?;
    let value = match digits.strip_prefix('-') {
        Some(value) if !value.starts_with('+') => {
            u64::from_str_radix(value, radix).ok()?.wrapping_neg()
        }
        Some(_) => return None,
        None => u64::from_str_radix(digits, radix).ok()?,
    };
    u32::try_from(value).ok()
}

fn escape_unit(out: &mut String, value: u16) {
    match value {
        0x22 => out.push_str("\\\""),
        0x5c => out.push_str("\\\\"),
        0x20..=0x7e => out.push(char::from(value as u8)),
        _ => {
            let _ = write!(out, "\\u{value:04x}");
        }
    }
}

fn json_string(out: &mut String, bytes: &[u8]) {
    out.push('"');
    if let Ok(text) = std::str::from_utf8(bytes)
        && text.encode_utf16().take(MAX_NAME_BYTES + 1).count() <= MAX_NAME_BYTES
    {
        for unit in text.encode_utf16() {
            escape_unit(out, unit);
        }
    } else {
        for &byte in bytes.iter().take(MAX_NAME_BYTES) {
            escape_unit(out, byte.into());
        }
    }
    out.push('"');
}

fn open_names(path: &CStr) -> Result<File, &'static str> {
    let mut buffer = [0u16; MAX_PATH as usize * 2];
    let count = unsafe {
        MultiByteToWideChar(
            CP_UTF8,
            0,
            path.as_ptr().cast(),
            -1,
            buffer.as_mut_ptr(),
            buffer.len() as i32,
        )
    };
    if count == 0 {
        return Err("the name list path is not valid UTF-8");
    }
    let path = OsString::from_wide(&buffer[..count as usize - 1]);
    File::open(path).map_err(|_| "the name list could not be opened")
}

fn name_entry(line: &[u8]) -> Option<(u32, &[u8])> {
    let tab = line.iter().position(|&byte| byte == b'\t')?;
    let name = line.get(tab + 1..)?;
    if name.is_empty() {
        return None;
    }
    Some((parse_rva(&line[..tab])?, name))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dump_names_impl(file: *mut c_void, path: *const c_char, write: Write) {
    let emit = |text: &str| unsafe { write(file, text.as_ptr(), text.len()) };
    let path = if path.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(path) })
    };
    emit("  \"names\": {\"entries\": [");
    let mut accepted = 0;
    let mut skipped = 0;
    let mut reason = None;
    let mut output = String::new();
    if let Some(path) = path {
        match open_names(path) {
            Ok(file) => {
                let mut input = BufReader::new(file);
                let mut line = Vec::with_capacity(LINE_BYTES);
                for index in 0..=MAX_LINES {
                    line.clear();
                    let mut chunk = (&mut input).take(LINE_BYTES as u64);
                    if !matches!(chunk.read_until(b'\n', &mut line), Ok(count) if count != 0) {
                        break;
                    }
                    if index == MAX_LINES {
                        reason = Some("the name list exceeds the line bound");
                        break;
                    }
                    if let Some(nul) = line.iter().position(|&byte| byte == 0) {
                        line.truncate(nul);
                    }
                    while matches!(line.last(), Some(b'\r' | b'\n')) {
                        line.pop();
                    }
                    let line = if index == 0 {
                        line.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&line)
                    } else {
                        &line
                    };
                    if line.is_empty() || line[0] == b'#' {
                        continue;
                    }
                    let Some((rva, name)) = name_entry(line) else {
                        skipped += 1;
                        continue;
                    };
                    output.clear();
                    if accepted != 0 {
                        output.push(',');
                    }
                    let _ = write!(output, "\n    {{\"rva\": \"{rva:#x}\", \"name\": ");
                    json_string(&mut output, name);
                    output.push('}');
                    emit(&output);
                    accepted += 1;
                }
            }
            Err(error) => reason = Some(error),
        }
    }
    output.clear();
    output.push_str("\n  ], \"source\": ");
    if let Some(path) = path {
        json_string(&mut output, path.to_bytes());
    } else {
        output.push_str("null");
    }
    let _ = write!(
        output,
        ", \"count\": {accepted}, \"skipped_lines\": {skipped}"
    );
    if let Some(reason) = reason {
        output.push_str(", \"reason\": ");
        json_string(&mut output, reason.as_bytes());
    }
    output.push_str("},\n");
    emit(&output);
}
