// SPDX-License-Identifier: MIT
use std::ffi::CString;

const BAD_UTF16: &str = "invalid UTF-16LE configuration";
const BAD_UTF8: &str = "invalid UTF-8 configuration";
pub(super) const BAD_SIZE: &str = "configuration is empty, unreadable, or exceeds 64 KiB";
pub(super) const MAX_SIZE: u64 = 65536;

pub(super) struct Config {
    pub auto_start: bool,
    pub runtime_module: String,
    pub entry_module: String,
    pub entry_point: CString,
}

fn decode(bytes: &[u8]) -> Result<String, &'static str> {
    let text = if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (pairs, remainder) = bytes.as_chunks::<2>();
        if pairs.is_empty() || !remainder.is_empty() {
            return Err(BAD_UTF16);
        }
        let units: Vec<_> = pairs.iter().copied().map(u16::from_le_bytes).collect();
        String::from_utf16(&units).map_err(|_| BAD_UTF16)?
    } else {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        if bytes.is_empty() {
            return Err(BAD_UTF8);
        }
        std::str::from_utf8(bytes).map_err(|_| BAD_UTF8)?.to_owned()
    };
    if text
        .bytes()
        .any(|c| (c < 32 && !matches!(c, b'\t' | b'\r' | b'\n')) || c == 127)
    {
        return Err("configuration contains control characters");
    }
    Ok(text)
}

fn trim(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\r'])
}

pub(super) fn parse(bytes: &[u8]) -> Result<Config, &'static str> {
    let text = decode(bytes)?;
    let mut section = "";
    let mut loader_seen = false;
    let mut auto_start = None;
    let mut runtime_module = None;
    let mut entry_module = None;
    let mut entry_point = None;

    for line in text.split('\n').map(trim) {
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let name = header
                .strip_suffix(']')
                .filter(|name| !name.contains(']'))
                .ok_or("malformed INI section")?;
            section = trim(name);
            if section.is_empty() {
                return Err("empty INI section");
            }
            if section.eq_ignore_ascii_case("loader") {
                if loader_seen {
                    return Err("duplicate Loader section");
                }
                loader_seen = true;
            }
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("malformed INI key/value")?;
        let key = trim(key);
        if section.is_empty() || key.is_empty() {
            return Err("malformed INI key/value");
        }
        if !section.eq_ignore_ascii_case("loader") {
            continue;
        }
        let slot = match key.to_ascii_lowercase().as_str() {
            "autostart" => &mut auto_start,
            "runtimemodule" => &mut runtime_module,
            "entrymodule" => &mut entry_module,
            "entrypoint" => &mut entry_point,
            _ => return Err("unknown Loader key"),
        };
        if slot.replace(trim(value)).is_some() {
            return Err("duplicate Loader key");
        }
    }

    let auto_start = match auto_start
        .ok_or("missing Loader AutoStart")?
        .to_ascii_lowercase()
        .as_str()
    {
        "1" | "true" => true,
        "0" | "false" => false,
        _ => return Err("invalid AutoStart boolean"),
    };
    if let Some(name) = entry_point
        && (name.is_empty()
            || name.len() > 255
            || !name.bytes().enumerate().all(|(i, c)| {
                c.is_ascii_alphabetic() || c == b'_' || (i != 0 && c.is_ascii_digit())
            }))
    {
        return Err("invalid entry point name");
    }
    let runtime_module = runtime_module.unwrap_or("ReScaleFrame/ReScaleFrame.Runtime.dll");
    let entry_module = entry_module.unwrap_or_default();
    let entry_point = entry_point.unwrap_or_default();
    if auto_start
        && (runtime_module.is_empty() || entry_module.is_empty() || entry_point.is_empty())
    {
        return Err("missing configured module or entry point");
    }
    Ok(Config {
        auto_start,
        runtime_module: runtime_module.to_owned(),
        entry_module: entry_module.to_owned(),
        entry_point: CString::new(entry_point).map_err(|_| "invalid entry point name")?,
    })
}
