// SPDX-License-Identifier: MIT
use crate::settings_store::{read_saved_number, read_saved_text, wide, write_saved_text};
use crate::sys::{
    RSF_OVERLAY_SAVED_OPTIONS_ABI_VERSION, RSF_OVERLAY_SAVED_TEXT_LIMIT, rsf_overlay_saved_apply,
    rsf_overlay_saved_setup,
};
use crate::sys::{rsf_overlay_intent as Intent, rsf_overlay_stats as Stats, *};
use rsf_game_sys::profile_text;
use std::ffi::{CStr, CString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::ptr::null_mut;
use std::sync::{Arc, Mutex, Once};
use windows_sys::Win32::System::{
    DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
    Memory::{GlobalLock, GlobalSize, GlobalUnlock},
};

const OK: i32 = 0;
const INVALID: i32 = -1;
const VERSION: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;
const CF_UNICODETEXT: u32 = 13;
const LIMIT: usize = RSF_OVERLAY_SAVED_ROW_LIMIT as usize;
const TEXT_LIMIT: usize = RSF_OVERLAY_SAVED_TEXT_LIMIT as usize;
const _: () = assert!(TEXT_LIMIT == profile_text::MAX_UNITS);

struct OptionSpec {
    label: CString,
    key: Vec<u16>,
    kind: u32,
    restart: u32,
    max_text: usize,
    minimum: i32,
    maximum: i32,
    step: i32,
    divisor: u32,
}

impl OptionSpec {
    fn valid_text(&self, units: &[u16]) -> bool {
        units.len() <= self.max_text
            && !units.iter().any(|&unit| matches!(unit, 0 | 10 | 13))
            && char::decode_utf16(units.iter().copied()).all(|unit| unit.is_ok())
    }

    fn valid_number(&self, value: i32) -> bool {
        (self.minimum..=self.maximum).contains(&value)
    }
}

struct Value {
    units: Vec<u16>,
    number: i32,
    last_result: i32,
}

struct Values {
    active: bool,
    rows: Vec<Value>,
}

struct Bank {
    path: Vec<u16>,
    section: Vec<u16>,
    specs: Vec<OptionSpec>,
    values: Mutex<Values>,
    apply: rsf_overlay_saved_apply,
    user: usize,
    create_parent: bool,
}

type BankKey = (usize, bool);
static BANKS: Mutex<Vec<(BankKey, Arc<Bank>)>> = Mutex::new(Vec::new());
static RUNTIME_DIAGNOSTICS: Once = Once::new();

unsafe fn wide_path(ptr: *const u16) -> Option<Vec<u16>> {
    if ptr.is_null() {
        return None;
    }
    let len = (0..32_768).find(|&index| unsafe { ptr.add(index).read() } == 0)?;
    (len != 0).then(|| unsafe { std::slice::from_raw_parts(ptr, len + 1) }.to_vec())
}

impl Bank {
    unsafe fn read(setup: &rsf_overlay_saved_setup) -> Option<Self> {
        if setup.options.is_null() || !(1..=LIMIT).contains(&(setup.option_count as usize)) {
            return None;
        }
        let path = unsafe { wide_path(setup.configuration_path) }?;
        let section = unsafe { wide_path(setup.configuration_section) }?;
        let mut specs: Vec<OptionSpec> = Vec::new();
        let mut rows = Vec::new();
        for option in
            unsafe { std::slice::from_raw_parts(setup.options, setup.option_count as usize) }
        {
            if option.label_utf8.is_null()
                || option.key_utf8.is_null()
                || option.kind > RSF_OVERLAY_SAVED_TEXT
                || option.restart > 1
                || (option.restart == 0 && setup.apply.is_none())
            {
                return None;
            }
            let label = unsafe { CStr::from_ptr(option.label_utf8) };
            let key = unsafe { CStr::from_ptr(option.key_utf8) }.to_str().ok()?;
            if label.to_str().ok()?.is_empty()
                || key.is_empty()
                || !key.is_ascii()
                || key.contains(['=', '\r', '\n'])
            {
                return None;
            }
            let key = wide(&key.to_ascii_lowercase());
            if specs.iter().any(|spec| spec.key == key) {
                return None;
            }
            let toggle = option.kind == RSF_OVERLAY_SAVED_TOGGLE;
            let spec = OptionSpec {
                label: label.to_owned(),
                key,
                kind: option.kind,
                restart: option.restart,
                max_text: option.max_text_length as usize,
                minimum: if toggle { 0 } else { option.minimum },
                maximum: if toggle { 1 } else { option.maximum },
                step: option.step.max(1),
                divisor: option.divisor.max(1),
            };
            let default_length = option.default_text_length as usize;
            let value = if spec.kind == RSF_OVERLAY_SAVED_TEXT {
                if !(1..=TEXT_LIMIT).contains(&spec.max_text)
                    || default_length > spec.max_text
                    || (default_length != 0 && option.default_text.is_null())
                {
                    return None;
                }
                let fallback = if default_length == 0 {
                    &[]
                } else {
                    unsafe { std::slice::from_raw_parts(option.default_text, default_length) }
                };
                if !spec.valid_text(fallback) {
                    return None;
                }
                Value {
                    units: read_saved_text(&path, &section, &spec.key)
                        .map(profile_text::decode)
                        .filter(|value| !value.contains(&0) && String::from_utf16(value).is_ok())
                        .unwrap_or_else(|| fallback.to_vec()),
                    number: 0,
                    last_result: OK,
                }
            } else {
                if !spec.valid_number(option.default_value) {
                    return None;
                }
                let stored = read_saved_number(&path, &section, &spec.key, option.default_value);
                let number = if toggle {
                    i32::from(stored != 0)
                } else if spec.valid_number(stored) {
                    stored
                } else {
                    option.default_value
                };
                Value {
                    units: Vec::new(),
                    number,
                    last_result: OK,
                }
            };
            specs.push(spec);
            rows.push(value);
        }
        Some(Self {
            path,
            section,
            specs,
            values: Mutex::new(Values { active: true, rows }),
            apply: setup.apply,
            user: setup.user as usize,
            create_parent: false,
        })
    }

    fn apply(&self, index: usize, action: u32, number: i32) {
        let Ok(mut state) = self.values.try_lock() else {
            return;
        };
        if !state.active {
            return;
        }
        let spec = &self.specs[index];
        let value = &mut state.rows[index];
        let units = match (spec.kind, action) {
            (RSF_OVERLAY_SAVED_TEXT, RSF_OVERLAY_SAVED_PASTE) => {
                clipboard(spec.max_text).filter(|units| spec.valid_text(units))
            }
            (RSF_OVERLAY_SAVED_TEXT, RSF_OVERLAY_SAVED_CLEAR) => Some(Vec::new()),
            (RSF_OVERLAY_SAVED_TOGGLE | RSF_OVERLAY_SAVED_NUMBER, RSF_OVERLAY_SAVED_SET)
                if spec.valid_number(number) =>
            {
                Some(number.to_string().encode_utf16().collect())
            }
            _ => None,
        };
        let Some(units) = units else {
            value.last_result = INVALID;
            return;
        };
        if self.create_parent {
            let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(
                &self.path[..self.path.len() - 1],
            ));
            if path
                .parent()
                .is_none_or(|parent| std::fs::create_dir_all(parent).is_err())
            {
                value.last_result = NOT_READY;
                return;
            }
        }
        let prior = read_saved_text(&self.path, &self.section, &spec.key);
        let stored = if spec.kind == RSF_OVERLAY_SAVED_TEXT {
            profile_text::encode(&units)
        } else {
            units.clone()
        };
        value.last_result = write_saved_text(&self.path, &self.section, &spec.key, Some(&stored));
        if value.last_result != OK {
            return;
        }
        if spec.restart == 0
            && let Some(apply) = self.apply
        {
            let mut terminated = units.clone();
            terminated.push(0);
            let result = unsafe {
                apply(
                    self.user as *mut c_void,
                    index as u32,
                    terminated.as_ptr(),
                    units.len() as u32,
                )
            };
            if result != OK {
                let restored =
                    write_saved_text(&self.path, &self.section, &spec.key, prior.as_deref());
                value.last_result = if restored == OK { result } else { NOT_READY };
                return;
            }
        }
        if spec.kind == RSF_OVERLAY_SAVED_TEXT {
            value.units = units;
        } else {
            value.number = number;
        }
    }
}

fn clipboard(max: usize) -> Option<Vec<u16>> {
    if unsafe { OpenClipboard(null_mut()) } == 0 {
        return None;
    }
    struct Open;
    impl Drop for Open {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }
    let _open = Open;
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if handle.is_null() {
        return None;
    }
    let count = (unsafe { GlobalSize(handle) } / size_of::<u16>()).min(max + 1);
    let units = unsafe { GlobalLock(handle) }.cast::<u16>();
    if units.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(units, count) };
    let result = slice
        .iter()
        .position(|&unit| unit == 0)
        .map(|end| slice[..end].to_vec());
    unsafe { GlobalUnlock(handle) };
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_saved_options_register(
    scope: *mut c_void,
    setup: *const rsf_overlay_saved_setup,
) -> i32 {
    if !unsafe { crate::sr_session::has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != RSF_OVERLAY_SAVED_OPTIONS_ABI_VERSION {
        return VERSION;
    }
    RUNTIME_DIAGNOSTICS.call_once(register_runtime_diagnostics);
    unsafe { register((scope as usize, false), setup) }
}

pub(super) unsafe fn register_builtin(scope: *mut c_void, setup: &rsf_overlay_saved_setup) -> i32 {
    unsafe { register((scope as usize, true), setup) }
}

unsafe fn register(key: BankKey, setup: &rsf_overlay_saved_setup) -> i32 {
    let Some(mut bank) = (unsafe { Bank::read(setup) }) else {
        return INVALID;
    };
    bank.create_parent = key == (0, true);
    let mut banks = BANKS.lock().unwrap();
    if banks.iter().any(|(existing, _)| *existing == key) {
        return BUSY;
    }
    let fits = |scope| {
        banks
            .iter()
            .filter(|((owner, _), _)| *owner == 0 || *owner == scope)
            .map(|(_, bank)| bank.specs.len())
            .sum::<usize>()
            + bank.specs.len()
            <= LIMIT
    };
    if !fits(key.0) || (key.0 == 0 && banks.iter().any(|((owner, _), _)| !fits(*owner))) {
        return INVALID;
    }
    banks.push((key, Arc::new(bank)));
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_saved_options_unregister(scope: *mut c_void) -> i32 {
    let mut banks = BANKS.lock().unwrap();
    let Some(index) = banks
        .iter()
        .position(|(key, _)| *key == (scope as usize, false))
    else {
        return OK;
    };
    {
        let Ok(mut values) = banks[index].1.values.try_lock() else {
            return BUSY;
        };
        values.active = false;
    }
    banks.remove(index);
    OK
}

#[derive(Default)]
pub(super) struct Frame {
    rows: Vec<rsf_overlay_saved_row>,
    text: Vec<Vec<u16>>,
    routes: Vec<(Arc<Bank>, usize)>,
}

impl Frame {
    pub(super) fn new(scope: *mut c_void) -> Self {
        RUNTIME_DIAGNOSTICS.call_once(register_runtime_diagnostics);
        let mut frame = Self::default();
        let banks: Vec<_> = BANKS
            .lock()
            .unwrap()
            .iter()
            .filter(|((owner, _), _)| *owner == 0 || *owner == scope as usize)
            .map(|(_, bank)| Arc::clone(bank))
            .collect();
        for bank in banks {
            let Ok(values) = bank.values.try_lock() else {
                continue;
            };
            if !values.active {
                continue;
            }
            for (index, (spec, value)) in bank.specs.iter().zip(&values.rows).enumerate() {
                if frame.rows.len() == LIMIT {
                    break;
                }
                frame.text.push(value.units.clone());
                let text = frame.text.last().unwrap();
                frame.rows.push(rsf_overlay_saved_row {
                    label: spec.label.as_ptr(),
                    text: text.as_ptr(),
                    text_length: text.len() as u32,
                    kind: spec.kind,
                    restart: spec.restart,
                    value: value.number,
                    minimum: spec.minimum,
                    maximum: spec.maximum,
                    step: spec.step,
                    divisor: spec.divisor,
                    last_result: value.last_result,
                });
                frame.routes.push((Arc::clone(&bank), index));
            }
        }
        frame
    }

    pub(super) fn attach(&self, stats: &mut Stats) {
        stats.saved_row_count = self.rows.len() as u32;
        stats.saved_rows = self.rows.as_ptr();
    }

    pub(super) fn apply(&self, intent: &Intent) {
        if intent.saved_action != 0
            && let Some((bank, index)) = self.routes.get(intent.saved_index as usize)
        {
            bank.apply(*index, intent.saved_action, intent.saved_value);
        }
    }
}

fn register_runtime_diagnostics() {
    use crate::sys::rsf_overlay_saved_option as Descriptor;
    let Some(path) = crate::diagnostic_preferences::profile() else {
        return;
    };
    let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    let section = wide(crate::diagnostic_preferences::SECTION);
    let text = |key: &CStr, label: &CStr| Descriptor {
        label_utf8: label.as_ptr(),
        key_utf8: key.as_ptr(),
        kind: RSF_OVERLAY_SAVED_TEXT,
        restart: 1,
        max_text_length: RSF_OVERLAY_SAVED_TEXT_LIMIT,
        ..Default::default()
    };
    let number = |key: &CStr, label: &CStr, default_value, maximum| Descriptor {
        label_utf8: label.as_ptr(),
        key_utf8: key.as_ptr(),
        kind: RSF_OVERLAY_SAVED_NUMBER,
        restart: 1,
        default_value,
        maximum,
        step: 1,
        divisor: 1,
        ..Default::default()
    };
    let toggle = |key: &CStr, label: &CStr| Descriptor {
        label_utf8: label.as_ptr(),
        key_utf8: key.as_ptr(),
        kind: RSF_OVERLAY_SAVED_TOGGLE,
        restart: 1,
        ..Default::default()
    };
    let mut rows = [
        text(
            c"RSF_VULKAN_FRAME_CAPTURE_DIR",
            c"Vulkan frame capture directory",
        ),
        text(c"RSF_FRAME_CAPTURE_DIR", c"D3D12 frame capture directory"),
        number(
            c"RSF_FRAME_CAPTURE_FIRST",
            c"First frame to capture",
            30,
            i32::MAX,
        ),
        number(c"RSF_FRAME_CAPTURE_COUNT", c"Frame capture count", 4, 16),
        toggle(
            c"RSF_XESS_DIAGNOSTIC_PROBE",
            c"XeSS Vulkan diagnostic probe",
        ),
        toggle(c"RSF_FRAME_CAPTURE_RESET_SR", c"Reset SR on D3D12 capture"),
        text(c"RSF_VULKAN_TIMING_FILE", c"Vulkan timing output file"),
    ];
    let mut labels = Vec::new();
    for row in &mut rows {
        let key = unsafe { CStr::from_ptr(row.key_utf8) }
            .to_str()
            .expect("diagnostic key");
        if std::env::var_os(key).is_some() {
            let label = unsafe { CStr::from_ptr(row.label_utf8) }.to_string_lossy();
            labels.push(CString::new(format!("{label} (environment override)")).unwrap());
            row.label_utf8 = labels.last().unwrap().as_ptr();
        }
    }
    let setup = rsf_overlay_saved_setup {
        struct_size: size_of::<rsf_overlay_saved_setup>() as u32,
        abi_version: RSF_OVERLAY_SAVED_OPTIONS_ABI_VERSION,
        configuration_path: path.as_ptr(),
        configuration_section: section.as_ptr(),
        options: rows.as_ptr(),
        option_count: rows.len() as u32,
        ..Default::default()
    };
    let _ = unsafe { register_builtin(null_mut(), &setup) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::rsf_overlay_saved_option;
    use crate::temporary_ini::TemporaryIni;

    unsafe extern "C" fn reject(_: *mut c_void, _: u32, _: *const u16, _: u32) -> i32 {
        INVALID
    }

    #[test]
    fn a_refused_live_edit_restores_the_profile_and_the_displayed_value() {
        let ini = TemporaryIni::new("saved-option", "refused");
        let section = wide("Test");
        let key = wide("Toggle");
        assert_eq!(
            write_saved_text(ini.wide(), &section, &key, Some(&[u16::from(b'1')])),
            OK
        );
        let option = rsf_overlay_saved_option {
            label_utf8: c"Toggle".as_ptr(),
            key_utf8: c"Toggle".as_ptr(),
            kind: RSF_OVERLAY_SAVED_TOGGLE,
            ..Default::default()
        };
        let setup = rsf_overlay_saved_setup {
            configuration_path: ini.wide().as_ptr(),
            configuration_section: section.as_ptr(),
            options: &option,
            option_count: 1,
            apply: Some(reject),
            ..Default::default()
        };
        let bank = unsafe { Bank::read(&setup) }.expect("valid option");
        bank.apply(0, RSF_OVERLAY_SAVED_SET, 0);
        let state = bank.values.lock().unwrap();
        assert_eq!(
            (state.rows[0].number, state.rows[0].last_result),
            (1, INVALID)
        );
        assert_eq!(read_saved_number(ini.wide(), &section, &key, 0), 1);
    }
}
