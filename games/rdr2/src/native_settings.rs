// SPDX-License-Identifier: MIT
//! Opt-in, bounded settings evidence collection. The detours never change native data.
use crate::asi::{DiagnosticHook, DiagnosticInstall, install_diagnostic_hooks};
use crate::layout::settings_probe::{BIND, GETTER_SLOT, KEYS, METADATA_KEY, READ};
use crate::platform::{self, lock, read_bytes, read_value};
use crate::runtime_worker::log;
use std::ffi::{OsString, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

struct Probe {
    output: PathBuf,
    image: usize,
    image_size: usize,
    rows: u8,
    values: u8,
}

static PROBE: Mutex<Option<Probe>> = Mutex::new(None);
static ARMED: AtomicBool = AtomicBool::new(false);

type Bind = unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void);
type Read = unsafe extern "C" fn(*const c_void, *mut c_void, *const c_void, *mut c_void, i32);
static mut BIND_ORIGINAL: *mut c_void = std::ptr::null_mut();
static mut READ_ORIGINAL: *mut c_void = std::ptr::null_mut();

unsafe extern "C" fn bind(
    destination: *mut c_void,
    context: *const c_void,
    metadata: *const c_void,
) {
    row(destination, context, metadata);
    let original: Bind = unsafe { std::mem::transmute(BIND_ORIGINAL) };
    unsafe { original(destination, context, metadata) };
}

unsafe extern "C" fn read(
    path: *const c_void,
    source: *mut c_void,
    kind: *const c_void,
    destination: *mut c_void,
    flags: i32,
) {
    let original: Read = unsafe { std::mem::transmute(READ_ORIGINAL) };
    unsafe { original(path, source, kind, destination, flags) };
    value(path, source, kind, destination, flags);
}

pub(crate) fn install(path: &[u16]) -> bool {
    let path = path.strip_suffix(&[0]).unwrap_or(path);
    let hooks = [
        DiagnosticHook::new(bind as *const c_void, &raw mut BIND_ORIGINAL),
        DiagnosticHook::new(read as *const c_void, &raw mut READ_ORIGINAL),
    ];
    let mut probe = lock(&PROBE);
    if path.is_empty() || probe.is_some() {
        return false;
    }
    let image = unsafe { platform::main_module() };
    let Some(facts) = platform::image_facts(image) else {
        return false;
    };
    let installed = match install_diagnostic_hooks(image, [BIND, READ], &hooks) {
        DiagnosticInstall::Installed => true,
        DiagnosticInstall::Refused => return false,
        DiagnosticInstall::Retained => false,
    };
    *probe = Some(Probe {
        output: PathBuf::from(OsString::from_wide(path)),
        image,
        image_size: facts.image_size as usize,
        rows: 0,
        values: 0,
    });
    drop(probe);
    if installed {
        ARMED.store(true, Ordering::Release);
        log(
            "RDR2 native settings read-only probe installed: max16 rows /32 values; no menu mutation.",
        );
    }
    installed
}

impl Probe {
    // Keep successful external addresses for diagnosis. Null means the read failed or
    // the pointer was null; only an address inside this image has an RVA.
    fn address_fields(&self, address: Option<usize>) -> (String, String) {
        match address.filter(|&address| address != 0) {
            Some(address) => {
                let rva = address
                    .checked_sub(self.image)
                    .filter(|&offset| offset < self.image_size);
                let address = format!("\"{address:016x}\"");
                (
                    rva.map_or_else(|| "null".into(), |rva| format!("\"{rva:x}\"")),
                    address,
                )
            }
            None => ("null".into(), "null".into()),
        }
    }
}

fn hex_bytes(address: usize, size: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut source = [0u8; 256];
    if size > source.len() || !read_bytes(address, &mut source[..size]) {
        return String::from("null");
    }
    let mut text = String::with_capacity(size * 2 + 2);
    text.push('"');
    for &byte in &source[..size] {
        text.push(char::from(HEX[(byte >> 4) as usize]));
        text.push(char::from(HEX[(byte & 15) as usize]));
    }
    text.push('"');
    text
}

fn row(destination: *const c_void, context: *const c_void, metadata: *const c_void) {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let destination = destination as usize;
    let metadata = metadata as usize;
    let key = read_value::<u32>(metadata.wrapping_add(METADATA_KEY)).unwrap_or(0);
    if !KEYS.contains(&key) {
        return;
    }
    let mut probe = lock(&PROBE);
    let Some(probe) = probe.as_mut().filter(|probe| probe.rows < 16) else {
        return;
    };
    probe.rows += 1;
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&probe.output)
    else {
        return;
    };
    let table = read_value::<usize>(destination);
    let thread = unsafe { GetCurrentThreadId() };
    let (rva, table_address) = probe.address_fields(table);
    let destination_bytes = hex_bytes(destination, 128);
    let context = hex_bytes(context as usize, 128);
    let metadata = hex_bytes(metadata, 128);
    let table = table.map_or_else(|| "null".into(), |table| hex_bytes(table, 256));
    let _ = writeln!(
        file,
        "{{\"event\":\"row\",\"thread\":{thread},\"key\":\"{key:08x}\",\"destination\":\"{destination:016X}\",\"vtable_rva\":{rva},\"vtable_address\":{table_address},\"destination_bytes\":{destination_bytes},\"context\":{context},\"metadata\":{metadata},\"destination_vtable\":{table}}}"
    );
}

fn value(
    path: *const c_void,
    source: *const c_void,
    kind: *const c_void,
    destination: *const c_void,
    flags: i32,
) {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let mut probe = lock(&PROBE);
    let Some(probe) = probe.as_mut().filter(|probe| probe.values < 32) else {
        return;
    };
    probe.values += 1;
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&probe.output)
    else {
        return;
    };
    let source = source as usize;
    let table = read_value::<usize>(source);
    let getter = table
        .filter(|&address| address != 0)
        .and_then(|address| address.checked_add(GETTER_SLOT))
        .and_then(read_value::<usize>);
    let thread = unsafe { GetCurrentThreadId() };
    let (rva, table_address) = probe.address_fields(table);
    let (getter_rva, getter_address) = probe.address_fields(getter);
    let path = hex_bytes(path as usize, 128);
    let source_bytes = hex_bytes(source, 128);
    let kind = hex_bytes(kind as usize, 32);
    let value = hex_bytes(destination as usize, 32);
    let _ = writeln!(
        file,
        "{{\"event\":\"value\",\"thread\":{thread},\"source\":\"{source:016X}\",\"vtable_rva\":{rva},\"vtable_address\":{table_address},\"getter_rva\":{getter_rva},\"getter_address\":{getter_address},\"flags\":{flags},\"path\":{path},\"source_bytes\":{source_bytes},\"type\":{kind},\"value\":{value}}}"
    );
}
