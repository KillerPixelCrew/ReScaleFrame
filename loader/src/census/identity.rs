// SPDX-License-Identifier: MIT
use super::json::{
    rsf_census_json_close, rsf_census_json_open, rsf_census_json_string, rsf_census_json_u64,
};
use super::json_abi::rsf_census_json;
use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::io::Write;
use std::ptr::{null, null_mut};
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicU32, AtomicU64, Ordering::*},
};
use windows_sys::Win32::{
    Foundation::{LocalFree, MAX_PATH},
    Globalization::{CP_UTF8, WideCharToMultiByte},
    System::{
        Diagnostics::Debug::RtlPcToFileHeader,
        LibraryLoader::GetModuleFileNameW,
        Threading::{GetCurrentThread, GetThreadDescription},
    },
};

const MODULES: usize = 512;
const THREADS: usize = 256;
const MODULE_BYTES: usize = 72;
const THREAD_BYTES: usize = 40;
const RECACHE_US: u64 = 1_000_000;

struct Module {
    key: AtomicU64,
    name: OnceLock<[u8; MODULE_BYTES]>,
}
static MODULE: [Module; MODULES] = [const {
    Module {
        key: AtomicU64::new(0),
        name: OnceLock::new(),
    }
}; MODULES];

struct Thread {
    tid: AtomicU32,
    calls: AtomicU64,
    name: Mutex<[u8; THREAD_BYTES]>,
}
static THREAD: [Thread; THREADS] = [const {
    Thread {
        tid: AtomicU32::new(0),
        calls: AtomicU64::new(0),
        name: Mutex::new([0; THREAD_BYTES]),
    }
}; THREADS];

pub(super) extern "C" fn rsf_census_module_base(pc: *mut c_void) -> u64 {
    if pc.is_null() {
        return 0;
    }
    let mut base = null_mut();
    unsafe { RtlPcToFileHeader(pc, &mut base) };
    base as u64
}

pub(super) fn rsf_census_module_index(base: u64) -> u32 {
    // Module bases are page aligned. Use the spare bit so the unknown base (zero)
    // gets a stable entry and publication is a single atomic operation.
    let key = base | 1;
    for (index, module) in MODULE.iter().enumerate() {
        let current = module.key.load(Acquire);
        if current == key {
            return index as u32;
        }
        if current == 0 {
            match module.key.compare_exchange(0, key, AcqRel, Acquire) {
                Ok(_) => return index as u32,
                Err(old) if old == key => return index as u32,
                Err(_) => {}
            }
        }
    }
    0
}

pub(super) extern "C" fn rsf_census_module_name(index: u32) -> *const c_char {
    let Some(module) = MODULE.get(index as usize) else {
        return c"unknown".as_ptr();
    };
    module
        .name
        .get_or_init(|| {
            let mut result = [0u8; MODULE_BYTES];
            let base = module.key.load(Acquire) & !1;
            if base == 0 {
                result[..7].copy_from_slice(b"unknown");
                return result;
            }
            let mut path = [0u16; MAX_PATH as usize];
            let length = unsafe { GetModuleFileNameW(base as _, path.as_mut_ptr(), MAX_PATH) };
            if length == 0 || length >= MAX_PATH {
                let _ = write!(&mut result[..MODULE_BYTES - 1], "module@{base:#x}");
                return result;
            }
            let begin = path[..length as usize]
                .iter()
                .rposition(|&unit| unit == u16::from(b'\\'))
                .map_or(0, |i| i + 1);
            utf8(unsafe { path.as_ptr().add(begin) }, &mut result);
            result
        })
        .as_ptr()
        .cast()
}

pub(super) extern "C" fn rsf_census_module_name_from_base(base: u64) -> *const c_char {
    rsf_census_module_name(rsf_census_module_index(base))
}

fn thread_entry(tid: u32, create: bool) -> Option<&'static Thread> {
    let start = (tid.wrapping_mul(2_654_435_761) >> 8) as usize;
    for step in 0..THREADS {
        let thread = &THREAD[(start + step) & (THREADS - 1)];
        match thread.tid.load(Acquire) {
            id if id == tid => return Some(thread),
            0 if !create => return None,
            0 => match thread.tid.compare_exchange(0, tid, AcqRel, Acquire) {
                Ok(_) => return Some(thread),
                Err(id) if id == tid => return Some(thread),
                Err(_) => {}
            },
            _ => {}
        }
    }
    None
}

pub(super) fn rsf_census_thread_note(tid: u32) {
    if let Some(thread) = thread_entry(tid, true) {
        thread.calls.fetch_add(1, Relaxed);
    }
}

fn utf8(source: *const u16, output: &mut [u8]) {
    unsafe {
        WideCharToMultiByte(
            CP_UTF8,
            0,
            source,
            -1,
            output.as_mut_ptr(),
            output.len() as i32,
            null(),
            null_mut(),
        );
    }
    if let Some(last) = output.last_mut() {
        *last = 0;
    }
}

#[derive(Clone, Copy)]
struct CachedThread {
    tid: u32,
    stamp: u64,
    name: [u8; THREAD_BYTES],
}

thread_local! {
    static THREAD_NAME: Cell<CachedThread> = const { Cell::new(CachedThread {
        tid: 0, stamp: 0, name: [0; THREAD_BYTES],
    }) };
}

pub(super) fn rsf_census_thread_name(tid: u32, now: u64, out: &mut [u8; THREAD_BYTES]) {
    out.fill(0);
    let _ = THREAD_NAME.try_with(|cell| {
        let mut cached = cell.get();
        if cached.tid != tid || now.saturating_sub(cached.stamp) > RECACHE_US {
            cached = CachedThread {
                tid,
                stamp: now,
                name: [0; THREAD_BYTES],
            };
            let mut name = null_mut();
            if unsafe { GetThreadDescription(GetCurrentThread(), &mut name) } >= 0
                && !name.is_null()
            {
                utf8(name, &mut cached.name);
                unsafe { LocalFree(name.cast()) };
            }
            if let Some(thread) = thread_entry(tid, false)
                && let Ok(mut name) = thread.name.try_lock()
            {
                *name = cached.name;
            }
            cell.set(cached);
        }
        *out = cached.name;
    });
}

pub(super) unsafe extern "C" fn rsf_census_threads_summary(json: &mut rsf_census_json) {
    unsafe { rsf_census_json_open(json, c"threads".as_ptr(), b'[' as c_char) };
    for thread in &THREAD {
        let tid = thread.tid.load(Acquire);
        if tid == 0 {
            continue;
        }
        let name = thread
            .name
            .try_lock()
            .map_or([0; THREAD_BYTES], |name| *name);
        unsafe {
            rsf_census_json_open(json, null(), b'{' as c_char);
            rsf_census_json_u64(json, c"tid".as_ptr(), tid.into());
            rsf_census_json_string(json, c"name".as_ptr(), name.as_ptr().cast());
            rsf_census_json_u64(json, c"calls".as_ptr(), thread.calls.load(Relaxed));
            rsf_census_json_close(json, b'}' as c_char);
        }
    }
    rsf_census_json_close(json, b']' as c_char);
}
