// SPDX-License-Identifier: MIT
use super::json::{
    rsf_census_json_close, rsf_census_json_i64, rsf_census_json_open, rsf_census_json_raw,
    rsf_census_json_string, rsf_census_json_u64,
};
use super::rsf_census_json;
use super::session::rsf_census_active;
use super::vendor::register_vendors;
use crate::sys::*;
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::mem::transmute;
use std::ptr::{null, null_mut};
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering::*},
};
use windows_sys::Win32::{
    Foundation::{FARPROC, GetLastError, HANDLE, HMODULE, MAX_PATH, SetLastError},
    System::LibraryLoader::{
        GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_AS_DATAFILE,
        LOAD_LIBRARY_AS_DATAFILE_EXCLUSIVE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
    },
};
use windows_sys::core::w;

type Ptr = *mut c_void;
type Resolve = unsafe extern "system" fn(HMODULE, *const u8) -> FARPROC;
const ENTRIES: usize = 1024;
const UNKNOWN: u16 = u16::MAX;
const SEEN_CAP: usize = 1024;
const RECORD_CAP: usize = 384;
const MODULE_CAP: usize = 24;
const NAME_CAP: usize = 40;
const HOOK_OK: i32 = 0;

struct Config {
    platform: rsf_census_platform,
    flags: u32,
    names: [u16; ENTRIES],
    wrappers: [usize; ENTRIES],
}

// The native descriptor array is immutable for the process lifetime. Callback addresses
// remain valid with the census. The wrapper array is owned by this configuration.
unsafe impl Send for Config {}
unsafe impl Sync for Config {}
static CONFIG: OnceLock<Config> = OnceLock::new();

static ORIGINALS: [AtomicPtr<c_void>; ENTRIES] = [const { AtomicPtr::new(null_mut()) }; ENTRIES];
static TARGETS: [AtomicPtr<c_void>; ENTRIES] = [const { AtomicPtr::new(null_mut()) }; ENTRIES];
static SEEN: [AtomicUsize; SEEN_CAP] = [const { AtomicUsize::new(0) }; SEEN_CAP];
static INSTALLED: AtomicU32 = AtomicU32::new(0);
static CONFLICTS: AtomicU32 = AtomicU32::new(0);
static MODULES: AtomicU32 = AtomicU32::new(0);

static GET_PROC: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static GET_PROC_CALLER: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static LOAD_W: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static LOAD_A: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static LOAD_EX_W: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static LOAD_EX_A: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

thread_local! { static BUSY: Cell<bool> = const { Cell::new(false) }; }

struct Busy;
impl Busy {
    fn enter() -> Option<Self> {
        BUSY.try_with(|busy| {
            if busy.get() {
                None
            } else {
                busy.set(true);
                Some(Self)
            }
        })
        .ok()
        .flatten()
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        let _ = BUSY.try_with(|busy| busy.set(false));
    }
}

#[derive(Clone, Copy)]
struct Hook {
    entry: u16,
    detour: bool,
    status: i32,
    target: usize,
    patched: Option<[u8; 5]>,
    module: [u8; NAME_CAP],
}

struct Book {
    records: [Hook; RECORD_CAP],
    count: usize,
    modules: [[u8; NAME_CAP]; MODULE_CAP],
}

static BOOK: Mutex<Book> = Mutex::new(Book {
    records: [Hook {
        entry: 0,
        detour: false,
        status: 0,
        target: 0,
        patched: None,
        module: [0; NAME_CAP],
    }; RECORD_CAP],
    count: 0,
    modules: [[0; NAME_CAP]; MODULE_CAP],
});

fn book() -> std::sync::MutexGuard<'static, Book> {
    BOOK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn platform() -> &'static rsf_census_platform {
    &CONFIG
        .get()
        .expect("hook platform must be published before detours")
        .platform
}

fn descriptor(cfg: &Config, entry: u16) -> Option<&rsf_census_hook_desc> {
    (entry < cfg.platform.count).then(|| unsafe { &*cfg.platform.entries.add(entry as usize) })
}

fn bytes<'a>(address: *const c_char) -> Option<&'a CStr> {
    (!address.is_null()).then(|| unsafe { CStr::from_ptr(address) })
}

fn wrapper(cfg: &Config, entry: u16) -> Ptr {
    if entry < cfg.platform.count {
        cfg.wrappers[entry as usize] as Ptr
    } else {
        null_mut()
    }
}

fn hash_name(name: &CStr) -> usize {
    name.to_bytes()
        .iter()
        .fold(2_166_136_261u32, |hash, &byte| {
            (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
        }) as usize
        & (ENTRIES - 1)
}

fn lookup(cfg: &Config, name: &CStr) -> u16 {
    let start = hash_name(name);
    for step in 0..ENTRIES {
        let entry = cfg.names[(start + step) & (ENTRIES - 1)];
        let Some(desc) = descriptor(cfg, entry) else {
            break;
        };
        if bytes(desc.name) == Some(name) {
            return entry;
        }
    }
    UNKNOWN
}

pub(super) fn bind_named(api: &CStr, name: &CStr, target: Ptr) -> Option<Ptr> {
    let cfg = CONFIG.get()?;
    let entry = lookup(cfg, name);
    let desc = descriptor(cfg, entry)?;
    if bytes(desc.api) != Some(api) {
        return None;
    }
    Some(rsf_census_hook_bind_thunk(entry, target))
}

fn enabled(cfg: &Config, entry: u16, desc: &rsf_census_hook_desc) -> bool {
    let api = bytes(desc.api);
    (cfg.flags & RSF_CENSUS_FLAG_D3D12_DEBUG_ONLY == 0 || entry == cfg.platform.graphics_roots[3])
        && (cfg.flags & RSF_CENSUS_FLAG_NO_DXGI == 0
            || !matches!(api.map(CStr::to_bytes), Some(b"d3d12" | b"dxgi")))
        && (cfg.flags & RSF_CENSUS_FLAG_NO_VULKAN == 0 || api != Some(c"vulkan"))
}

fn short_name(name: &[u8]) -> [u8; NAME_CAP] {
    let mut copy = [0; NAME_CAP];
    let len = name.len().min(copy.len() - 1);
    copy[..len].copy_from_slice(&name[..len]);
    copy
}

fn note_hook(entry: u16, detour: bool, status: i32, target: Ptr, module: &[u8]) {
    let mut book = book();
    let index = book.count;
    if let Some(record) = book.records.get_mut(index) {
        *record = Hook {
            entry,
            detour,
            status,
            target: target as usize,
            patched: None,
            module: short_name(module),
        };
        book.count += 1;
    }
}

pub(super) fn rsf_census_hook_original(entry: u16) -> Ptr {
    ORIGINALS
        .get(entry as usize)
        .map_or(null_mut(), |original| original.load(Acquire))
}

pub(super) fn rsf_census_hook_bind_thunk(entry: u16, real: Ptr) -> Ptr {
    let Some(cfg) = CONFIG.get() else { return real };
    let replacement = wrapper(cfg, entry);
    if replacement.is_null() || TARGETS[entry as usize].load(Acquire) == real {
        return real;
    }
    match ORIGINALS[entry as usize].compare_exchange(null_mut(), real, AcqRel, Acquire) {
        Ok(_) => {
            INSTALLED.fetch_add(1, Relaxed);
            note_hook(entry, false, HOOK_OK, null_mut(), b"");
            replacement
        }
        Err(original) if original == real => replacement,
        Err(_) => {
            CONFLICTS.fetch_add(1, Relaxed);
            real
        }
    }
}

fn bind_inline(cfg: &Config, entry: u16, target: Ptr, module: &[u8]) -> bool {
    let replacement = wrapper(cfg, entry);
    let Some(desc) = descriptor(cfg, entry) else {
        return false;
    };
    if replacement.is_null() || target.is_null() || !enabled(cfg, entry, desc) {
        return false;
    }
    let original = &ORIGINALS[entry as usize];
    if !original.load(Acquire).is_null() {
        if TARGETS[entry as usize].load(Acquire) != target {
            CONFLICTS.fetch_add(1, Relaxed);
        }
        return false;
    }
    let mut trampoline = null_mut();
    let status = unsafe { cfg.platform.create.unwrap()(target, replacement, &mut trampoline) };
    if status != HOOK_OK {
        note_hook(entry, true, status, null_mut(), module);
        return false;
    }
    if original
        .compare_exchange(null_mut(), trampoline, AcqRel, Acquire)
        .is_err()
    {
        unsafe { cfg.platform.remove.unwrap()(target) };
        return false;
    }
    TARGETS[entry as usize].store(target, Release);
    let status = unsafe { cfg.platform.enable.unwrap()(target) };
    note_hook(entry, true, status, target, module);
    if status == HOOK_OK {
        INSTALLED.fetch_add(1, Relaxed);
        true
    } else {
        false
    }
}

pub(super) fn rsf_census_hook_bind_inline(entry: u16, target: Ptr, module: *const c_char) -> bool {
    CONFIG.get().is_some_and(|cfg| {
        bind_inline(
            cfg,
            entry,
            target,
            bytes(module).map_or(b"", CStr::to_bytes),
        )
    })
}

fn seen_before(base: usize) -> bool {
    let start = ((base >> 12).wrapping_mul(2_654_435_761) >> 8) & (SEEN_CAP - 1);
    for step in 0..SEEN_CAP {
        let entry = &SEEN[(start + step) & (SEEN_CAP - 1)];
        let found = entry.load(Acquire);
        if found == base {
            return true;
        }
        if found == 0 {
            match entry.compare_exchange(0, base, AcqRel, Acquire) {
                Ok(_) => return false,
                Err(old) if old == base => return true,
                Err(_) => {}
            }
        }
    }
    true
}

fn basename(path: &[u16], buffer: &mut [u8; 48]) -> usize {
    let start = path
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\'))
        .map_or(0, |i| i + 1);
    let count = (path.len() - start).min(buffer.len() - 1);
    for (unit, out) in path[start..][..count].iter().zip(buffer.iter_mut()) {
        *out = if *unit < 128 {
            (*unit as u8).to_ascii_lowercase()
        } else {
            b'?'
        };
    }
    count
}

fn hook_module(cfg: &Config, module: HMODULE, path: &[u16]) {
    let mut name = [0; 48];
    let count = basename(path, &mut name);
    let name = &name[..count];
    let resolve = unsafe { transmute::<Ptr, Resolve>(GET_PROC.load(Acquire)) };
    let mut any = false;
    for entry in 0..cfg.platform.count {
        let desc = descriptor(cfg, entry).unwrap();
        let Some(modules) = bytes(desc.modules) else {
            continue;
        };
        if desc.name.is_null()
            || modules.to_bytes().is_empty()
            || !modules
                .to_bytes()
                .split(|&byte| byte == b'|')
                .any(|part| part.eq_ignore_ascii_case(name))
            || !enabled(cfg, entry, desc)
        {
            continue;
        }
        if let Some(real) = unsafe { resolve(module, desc.name.cast()) } {
            any |= bind_inline(cfg, entry, real as Ptr, name);
        }
    }
    if any {
        rsf_census_hooks_apply();
        let mut book = book();
        let index = MODULES.fetch_add(1, Relaxed) as usize;
        if let Some(module) = book.modules.get_mut(index) {
            *module = short_name(name);
        }
    }
}

fn loader_field<const N: usize>(cfg: &Config, base: usize, offset: usize) -> Option<[u8; N]> {
    if base == 0 {
        return None;
    }
    let address = base.checked_add(offset)?;
    let mut field = [0; N];
    unsafe { cfg.platform.read.unwrap()(address as _, field.as_mut_ptr().cast(), N) }
        .then_some(field)
}

fn loader_word(cfg: &Config, base: usize, offset: usize) -> Option<usize> {
    loader_field::<8>(cfg, base, offset).map(usize::from_le_bytes)
}

fn scan_modules(cfg: &Config) {
    // Windows x64 TEB->PEB->Ldr->InLoadOrderModuleList. This walk uses the same
    // loader list as the native DllMain implementation and never waits for its lock.
    let teb = unsafe { cfg.platform.current_teb.unwrap()() } as usize;
    let Some(ldr) = loader_word(cfg, teb, 0x60).and_then(|peb| loader_word(cfg, peb, 0x18)) else {
        return;
    };
    if ldr == 0 {
        return;
    }
    let Some(head) = ldr.checked_add(0x10) else {
        return;
    };
    let Some(mut entry) = loader_word(cfg, head, 0) else {
        return;
    };
    for _ in 0..16_384 {
        if entry == head {
            break;
        }
        let Some(row) = loader_field::<0x58>(cfg, entry, 0) else {
            break;
        };
        entry = usize::from_le_bytes(row[..8].try_into().unwrap());
        let module = usize::from_le_bytes(row[0x30..0x38].try_into().unwrap());
        let length = u16::from_le_bytes(row[0x48..0x4a].try_into().unwrap()) as usize;
        let source = usize::from_le_bytes(row[0x50..0x58].try_into().unwrap());
        let mut path = [0u16; MAX_PATH as usize];
        let units = length / 2;
        if module == 0 || source == 0 || !length.is_multiple_of(2) || units >= path.len() {
            continue;
        }
        let copied =
            unsafe { cfg.platform.read.unwrap()(source as _, path.as_mut_ptr().cast(), length) };
        if copied {
            seen_before(module);
            hook_module(cfg, module as HMODULE, &path[..units]);
        }
    }
}

fn post_load(module: HMODULE, flags: u32) {
    let data_flags = LOAD_LIBRARY_AS_DATAFILE
        | LOAD_LIBRARY_AS_IMAGE_RESOURCE
        | LOAD_LIBRARY_AS_DATAFILE_EXCLUSIVE;
    if module.is_null() || module as usize & 3 != 0 || flags & data_flags != 0 {
        return;
    }
    let Some(_busy) = Busy::enter() else { return };
    let cfg = CONFIG.get().unwrap();
    if !rsf_census_active() || seen_before(module as usize) {
        return;
    }
    let last_error = unsafe { GetLastError() };
    let mut path = [0u16; MAX_PATH as usize];
    if let Some(name) = unsafe { rsf_game_sys::win32::module_path_in(module, &mut path) } {
        hook_module(cfg, module, name);
    }
    unsafe { SetLastError(last_error) };
}

fn intercept(real: FARPROC, name: *const u8) -> FARPROC {
    let Some(real_fn) = real else { return real };
    if name as usize <= 0xffff {
        return real;
    }
    let Some(_busy) = Busy::enter() else {
        return real;
    };
    let cfg = CONFIG.get().unwrap();
    if !rsf_census_active() {
        return real;
    }
    let last_error = unsafe { GetLastError() };
    let mut buffer = [0u8; 256];
    let valid = unsafe {
        cfg.platform.read_string.unwrap()(name.cast(), buffer.as_mut_ptr().cast(), buffer.len())
    };
    let mut result = real;
    if valid
        && let Ok(name) = CStr::from_bytes_until_nul(&buffer)
        && name.to_bytes().len() < buffer.len() - 1
    {
        let entry = lookup(cfg, name);
        // Graphics creation exports are detoured at the module entry. Resolver thunks
        // here can create a cycle with an overlay that resolves its own chain root.
        if let Some(desc) = descriptor(cfg, entry)
            && enabled(cfg, entry, desc)
            && !cfg.platform.graphics_roots.contains(&entry)
        {
            let bound = rsf_census_hook_bind_thunk(entry, real_fn as Ptr);
            result = Some(unsafe { transmute::<Ptr, unsafe extern "system" fn() -> isize>(bound) });
        }
    }
    unsafe { SetLastError(last_error) };
    result
}

unsafe extern "system" fn get_proc(module: HMODULE, name: *const u8) -> FARPROC {
    let original = unsafe { transmute::<Ptr, Resolve>(GET_PROC.load(Acquire)) };
    intercept(unsafe { original(module, name) }, name)
}

unsafe extern "system" fn get_proc_caller(
    module: HMODULE,
    name: *const u8,
    caller: Ptr,
) -> FARPROC {
    type Original = unsafe extern "system" fn(HMODULE, *const u8, Ptr) -> FARPROC;
    let original = unsafe { transmute::<Ptr, Original>(GET_PROC_CALLER.load(Acquire)) };
    intercept(unsafe { original(module, name, caller) }, name)
}

macro_rules! loader_hook {
    ($name:ident, $encoding:ty, $slot:ident) => {
        unsafe extern "system" fn $name(path: *const $encoding) -> HMODULE {
            type Original = unsafe extern "system" fn(*const $encoding) -> HMODULE;
            let original = unsafe { transmute::<Ptr, Original>($slot.load(Acquire)) };
            let module = unsafe { original(path) };
            post_load(module, 0);
            module
        }
    };
    ($name:ident, $encoding:ty, $slot:ident, extended) => {
        unsafe extern "system" fn $name(
            path: *const $encoding,
            file: HANDLE,
            flags: u32,
        ) -> HMODULE {
            type Original = unsafe extern "system" fn(*const $encoding, HANDLE, u32) -> HMODULE;
            let original = unsafe { transmute::<Ptr, Original>($slot.load(Acquire)) };
            let module = unsafe { original(path, file, flags) };
            post_load(module, flags);
            module
        }
    };
}
loader_hook!(load_w, u16, LOAD_W);
loader_hook!(load_a, u8, LOAD_A);
loader_hook!(load_ex_w, u16, LOAD_EX_W, extended);
loader_hook!(load_ex_a, u8, LOAD_EX_A, extended);

fn install_api(module: *const u16, name: &CStr, detour: Ptr, slot: &AtomicPtr<c_void>) -> bool {
    let p = platform();
    let mut original = null_mut();
    if unsafe { p.create_api.unwrap()(module, name.as_ptr(), detour, &mut original) } != HOOK_OK {
        return false;
    }
    slot.store(original, Release);
    unsafe { p.enable.unwrap()(null_mut()) };
    true
}

pub(super) unsafe fn rsf_census_hooks_install(value: &rsf_census_platform, flags: u32) -> bool {
    if usize::from(value.count) >= ENTRIES || unsafe { value.initialize.unwrap()() } != HOOK_OK {
        return false;
    }
    let mut wrappers = [null_mut(); ENTRIES];
    if !unsafe { register_vendors(value, &mut wrappers[..usize::from(value.count)]) } {
        return false;
    }
    let mut cfg = Config {
        platform: *value,
        flags,
        names: [UNKNOWN; ENTRIES],
        wrappers: wrappers.map(|wrapper| wrapper as usize),
    };
    for entry in 0..cfg.platform.count {
        if let Some(name) = descriptor(&cfg, entry).and_then(|desc| bytes(desc.name)) {
            let mut index = hash_name(name);
            while cfg.names[index] != UNKNOWN {
                index = (index + 1) & (ENTRIES - 1);
            }
            cfg.names[index] = entry;
        }
    }
    GET_PROC.store(GetProcAddress as Ptr, Release);
    if CONFIG.set(cfg).is_err() {
        return false;
    }
    let module = if unsafe { GetModuleHandleW(w!("kernelbase.dll")) }.is_null() {
        w!("kernel32.dll")
    } else {
        w!("kernelbase.dll")
    };
    if flags & RSF_CENSUS_FLAG_NO_GETPROCADDRESS_HOOK == 0 {
        if !install_api(module, c"GetProcAddress", get_proc as Ptr, &GET_PROC) {
            return false;
        }
        install_api(
            module,
            c"GetProcAddressForCaller",
            get_proc_caller as Ptr,
            &GET_PROC_CALLER,
        );
    }
    if flags & RSF_CENSUS_FLAG_NO_LOADLIBRARY_HOOK == 0 {
        for (name, hook, slot) in [
            (c"LoadLibraryW", load_w as Ptr, &LOAD_W),
            (c"LoadLibraryA", load_a as Ptr, &LOAD_A),
            (c"LoadLibraryExW", load_ex_w as Ptr, &LOAD_EX_W),
            (c"LoadLibraryExA", load_ex_a as Ptr, &LOAD_EX_A),
        ] {
            install_api(module, name, hook, slot);
        }
    }
    unsafe { platform().apply.unwrap()() };
    scan_modules(CONFIG.get().unwrap());
    true
}

pub(super) fn rsf_census_hooks_remove() {
    if let Some(cfg) = CONFIG.get() {
        // A caller may still hold a trampoline in flight.
        unsafe { cfg.platform.disable.unwrap()() };
    }
}

pub(super) fn rsf_census_hooks_apply() {
    let Some(cfg) = CONFIG.get() else { return };
    unsafe { cfg.platform.apply.unwrap()() };
    let mut book = book();
    let count = book.count;
    for hook in &mut book.records[..count] {
        if hook.detour && hook.status == HOOK_OK && hook.patched.is_none() && hook.target != 0 {
            let mut patch = [0; 5];
            if unsafe {
                cfg.platform.read.unwrap()(hook.target as _, patch.as_mut_ptr().cast(), patch.len())
            } {
                hook.patched = Some(patch);
            }
        }
    }
}

pub(super) fn rsf_census_hook_counts(installed: &mut u32, conflicts: &mut u32, modules: &mut u32) {
    *installed = INSTALLED.load(Relaxed);
    *conflicts = CONFLICTS.load(Relaxed);
    *modules = MODULES.load(Relaxed);
}

pub(super) unsafe fn rsf_census_hooks_summary(json: &mut rsf_census_json) {
    // The fatal-error writer may reach this while another thread is publishing a hook.
    let book = BOOK.try_lock().ok();
    unsafe {
        rsf_census_json_open(json, c"hooks".as_ptr(), b'[' as c_char);
        if let Some((book, cfg)) = book.as_ref().zip(CONFIG.get()) {
            for hook in &book.records[..book.count] {
                let desc = descriptor(cfg, hook.entry).unwrap();
                rsf_census_json_open(json, null(), b'{' as c_char);
                let name = if desc.name.is_null() {
                    desc.id
                } else {
                    desc.name
                };
                rsf_census_json_string(json, c"function".as_ptr(), name);
                rsf_census_json_string(json, c"api".as_ptr(), desc.api);
                let binding = if hook.detour {
                    c"detour"
                } else {
                    c"getprocaddress_thunk"
                };
                rsf_census_json_string(json, c"binding".as_ptr(), binding.as_ptr());
                if hook.module[0] != 0 {
                    rsf_census_json_string(json, c"module".as_ptr(), hook.module.as_ptr().cast());
                }
                if hook.detour {
                    rsf_census_json_i64(json, c"minhook_status".as_ptr(), hook.status.into());
                }
                if let Some(patched) = hook.patched {
                    let mut now = [0u8; 5];
                    let readable = cfg.platform.read.unwrap()(
                        hook.target as _,
                        now.as_mut_ptr().cast(),
                        now.len(),
                    );
                    let modified = if !readable || now != patched {
                        c"true"
                    } else {
                        c"false"
                    };
                    rsf_census_json_raw(json, c"entry_modified".as_ptr(), modified.as_ptr());
                }
                rsf_census_json_close(json, b'}' as c_char);
            }
        }
        rsf_census_json_close(json, b']' as c_char);
        rsf_census_json_u64(
            json,
            c"hook_conflicts".as_ptr(),
            CONFLICTS.load(Relaxed).into(),
        );
        rsf_census_json_open(json, c"modules_hooked".as_ptr(), b'[' as c_char);
        if let Some(book) = book.as_ref() {
            for module in book.modules.iter().take(MODULES.load(Relaxed) as usize) {
                rsf_census_json_string(json, null(), module.as_ptr().cast());
            }
        }
        rsf_census_json_close(json, b']' as c_char);
    }
}
