// SPDX-License-Identifier: MIT
//! Opt-in, bounded settings evidence collection. The detours never change native data.
use crate::asi::{self, AsiPlatform, MH_ERROR_ALREADY_INITIALIZED, MH_OK};
use crate::layout::settings_probe::{BIND, GETTER_SLOT, KEYS, METADATA_KEY, READ};
use crate::platform::lock;
use crate::runtime_worker::log;
use std::ffi::{OsString, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[repr(C)]
struct Hook {
    detour: *const c_void,
    original: *mut *mut c_void,
}

struct Probe {
    output: PathBuf,
    image: usize,
    rows: u8,
    values: u8,
    installed: bool,
}

static PROBE: Mutex<Option<Probe>> = Mutex::new(None);
static ARMED: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_settings_install(
    path: *const u16,
    hooks: *const [Hook; 2],
    enable: unsafe extern "C" fn(usize) -> i32,
) -> bool {
    let (Some(platform), Some(hooks)) = (asi::platform(), unsafe { hooks.as_ref() }) else {
        return false;
    };
    if path.is_null() {
        return false;
    }
    let Some(length) = (0..32768).find(|&index| unsafe { path.add(index).read() } == 0) else {
        return false;
    };
    let mut probe = lock(&PROBE);
    if length == 0 || probe.is_some() {
        return false;
    }
    let image = unsafe { (platform.main_module)() };
    if image == 0 {
        return false;
    }
    let sites = [BIND, READ];
    if !sites.iter().all(|site| {
        platform.read_value::<[u8; 16]>(image.wrapping_add(site.rva)) == Some(site.expected)
    }) {
        return false;
    }
    let initialized = unsafe { (platform.hook_initialize)() };
    if initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED {
        return false;
    }
    let targets = sites.map(|site| image + site.rva);
    let options = || Probe {
        output: PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(path, length)
        })),
        image,
        rows: 0,
        values: 0,
        installed: false,
    };
    for (index, (&target, hook)) in targets.iter().zip(hooks).enumerate() {
        if unsafe { (platform.hook_create)(target, hook.detour, hook.original) } != MH_OK {
            for &created in &targets[..index] {
                if unsafe { (platform.hook_remove)(created) } != MH_OK {
                    // Retain a refused owner if an unpublished hook could not be removed.
                    *probe = Some(options());
                }
            }
            return false;
        }
    }
    if !targets
        .iter()
        .all(|&target| unsafe { enable(target) } == MH_OK)
    {
        // Partial enable can leave a callback on the stack. Disable both sites and retain their
        // trampolines for the ASI's process lifetime. The refused owner cannot be armed.
        for target in targets {
            unsafe { (platform.hook_disable)(target) };
        }
        *probe = Some(options());
        return false;
    }
    *probe = Some(Probe {
        installed: true,
        ..options()
    });
    drop(probe);
    log("RDR2 native settings read-only probe installed: max16 rows /32 values; no menu mutation.");
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_native_settings_probe_arm() {
    if lock(&PROBE).as_ref().is_some_and(|probe| probe.installed) {
        ARMED.store(true, Ordering::Release);
    }
}

fn hex_bytes(platform: &AsiPlatform, address: usize, size: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut source = [0u8; 256];
    if size > source.len() || !unsafe { (platform.read)(address, source.as_mut_ptr().cast(), size) }
    {
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

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_settings_row(
    destination: *const c_void,
    context: *const c_void,
    metadata: *const c_void,
) {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let Some(platform) = asi::platform() else {
        return;
    };
    let destination = destination as usize;
    let metadata = metadata as usize;
    let key = platform
        .read_value::<u32>(metadata.wrapping_add(METADATA_KEY))
        .unwrap_or(0);
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
    let table = platform.read_value::<usize>(destination).unwrap_or(0);
    let thread = unsafe { (platform.current_thread)() };
    let rva = table.wrapping_sub(probe.image);
    let destination_bytes = hex_bytes(platform, destination, 128);
    let context = hex_bytes(platform, context as usize, 128);
    let metadata = hex_bytes(platform, metadata, 128);
    let table = hex_bytes(platform, table, 256);
    let _ = writeln!(
        file,
        "{{\"event\":\"row\",\"thread\":{thread},\"key\":\"{key:08x}\",\"destination\":\"{destination:016X}\",\"vtable_rva\":\"{rva:x}\",\"destination_bytes\":{destination_bytes},\"context\":{context},\"metadata\":{metadata},\"destination_vtable\":{table}}}"
    );
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_settings_value(
    path: *const c_void,
    source: *const c_void,
    kind: *const c_void,
    destination: *const c_void,
    flags: i32,
) {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let Some(platform) = asi::platform() else {
        return;
    };
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
    let table = platform.read_value::<usize>(source).unwrap_or(0);
    let getter = platform
        .read_value::<usize>(table.wrapping_add(GETTER_SLOT))
        .unwrap_or(0);
    let thread = unsafe { (platform.current_thread)() };
    let rva = table.wrapping_sub(probe.image);
    let getter_rva = getter.wrapping_sub(probe.image);
    let path = hex_bytes(platform, path as usize, 128);
    let source_bytes = hex_bytes(platform, source, 128);
    let kind = hex_bytes(platform, kind as usize, 32);
    let value = hex_bytes(platform, destination as usize, 32);
    let _ = writeln!(
        file,
        "{{\"event\":\"value\",\"thread\":{thread},\"source\":\"{source:016X}\",\"vtable_rva\":\"{rva:x}\",\"getter_rva\":\"{getter_rva:x}\",\"flags\":{flags},\"path\":{path},\"source_bytes\":{source_bytes},\"type\":{kind},\"value\":{value}}}"
    );
}
