// SPDX-License-Identifier: MIT
use super::hooks::{rsf_census_hook_counts, rsf_census_hooks_remove, rsf_census_hooks_summary};
use super::identity::*;
use super::json::{
    rsf_census_json_close, rsf_census_json_hex, rsf_census_json_open, rsf_census_json_raw,
    rsf_census_json_string, rsf_census_json_u64,
};
use super::json_abi::rsf_census_json;
use super::output::*;
use super::record::*;
use super::record_abi::{
    RSF_CENSUS_RECORD_BODY_MAX, RSF_CENSUS_RECORD_STACK_FRAMES, rsf_census_record_writer,
};
use super::session_abi::*;
use super::static_scan::rsf_census_static_scan;
use super::stats::*;
use super::stats_abi::{rsf_census_sample, rsf_census_stats_names};
use std::ffi::{CStr, c_char, c_void};
use std::mem::MaybeUninit;
use std::ptr::{self, null, null_mut};
use std::sync::{
    Mutex, MutexGuard, OnceLock,
    atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering::*},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, MAX_PATH, SetLastError, WAIT_OBJECT_0},
    Globalization::{CP_UTF8, MultiByteToWideChar, WideCharToMultiByte},
    Storage::FileSystem::CreateDirectoryW,
    System::{
        Diagnostics::Debug::{
            AddVectoredExceptionHandler, EXCEPTION_POINTERS, RtlCaptureStackBackTrace,
        },
        LibraryLoader::GetModuleFileNameW,
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        Threading::{
            CreateEventW, CreateThread, GetCurrentProcessId, GetCurrentThreadId, SetEvent, Sleep,
            WaitForSingleObject,
        },
    },
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const ALREADY_INSTALLED: i32 = -3;
const NOT_INSTALLED: i32 = -4;
const IO_ERROR: i32 = -5;
const HOOK_ERROR: i32 = -6;
const PATH_CAP: usize = MAX_PATH as usize;
const SUMMARY_BYTES: usize = 4 << 20;
const FINDING_CAP: usize = 192;
const STACK_FRAMES: usize = RSF_CENSUS_RECORD_STACK_FRAMES as usize;

struct Writer {
    summary: Vec<u8>,
    last_summary_us: u64,
}

struct Core {
    platform: rsf_census_core_platform,
    flags: u32,
    budget: u64,
    cap: u32,
    interval: u32,
    period_us: u64,
    dir: [u16; PATH_CAP],
    exe: [u8; PATH_CAP * 3],
    frequency: i64,
    start: i64,
    wake: AtomicPtr<c_void>,
    writer_handle: AtomicPtr<c_void>,
    scan_handle: AtomicPtr<c_void>,
    writer_tid: AtomicU32,
    writer: Mutex<Writer>,
}

// The native table and callback addresses live for the full process. Worker and event
// handles are transferred atomically, and the writer lock owns the file and scratch buffers.
unsafe impl Send for Core {}
unsafe impl Sync for Core {}
static CORE: OnceLock<Core> = OnceLock::new();
static EVER_INSTALLED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static INITIALIZING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static QUEUED: AtomicU64 = AtomicU64::new(0);
static DROPPED_QUEUE: AtomicU64 = AtomicU64::new(0);
static DROPPED_SIZE: AtomicU64 = AtomicU64::new(0);
static FATAL_REPORTS: AtomicU32 = AtomicU32::new(0);

struct CrashCall(MaybeUninit<rsf_census_call_state>);
// The guarded slot stays in process memory and records only borrowed code addresses.
unsafe impl Send for CrashCall {}
static CRASH_CALL: Mutex<CrashCall> = Mutex::new(CrashCall(MaybeUninit::uninit()));

#[derive(Clone, Copy)]
struct Finding {
    vendor: [u8; 24],
    what: [u8; 72],
    rva: u64,
    function: u64,
}

static FINDING_COUNT: AtomicU32 = AtomicU32::new(0);
static FINDINGS: Mutex<[Finding; FINDING_CAP]> = Mutex::new(
    [Finding {
        vendor: [0; 24],
        what: [0; 72],
        rva: 0,
        function: 0,
    }; FINDING_CAP],
);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn configured(value: u32, default: u32) -> u32 {
    if value == 0 { default } else { value }
}

struct Initializing;
impl Drop for Initializing {
    fn drop(&mut self) {
        INITIALIZING.store(false, Release);
    }
}

fn path_with_suffix(dir: &[u16; PATH_CAP], suffix: &str) -> [u16; PATH_CAP + 32] {
    let length = dir.iter().position(|&unit| unit == 0).unwrap_or(PATH_CAP);
    let mut path = [0u16; PATH_CAP + 32];
    path[..length].copy_from_slice(&dir[..length]);
    for (slot, unit) in path[length..].iter_mut().zip(suffix.encode_utf16()) {
        *slot = unit;
    }
    path
}

fn make_directories(dir: &mut [u16; PATH_CAP]) {
    let length = dir.iter().position(|&unit| unit == 0).unwrap_or(PATH_CAP);
    for index in 1..length {
        let unit = dir[index];
        if matches!(unit, 0x5c | 0x2f) {
            dir[index] = 0;
            unsafe { CreateDirectoryW(dir.as_ptr(), null()) };
            dir[index] = unit;
        }
    }
    unsafe { CreateDirectoryW(dir.as_ptr(), null()) };
}

fn output_dir(options: &rsf_census_options) -> Option<[u16; PATH_CAP]> {
    let mut dir = [0u16; PATH_CAP];
    if options.output_dir_utf8.is_null() || unsafe { *options.output_dir_utf8 } == 0 {
        dir[0] = u16::from(b'.');
        return Some(dir);
    }
    let count = unsafe {
        MultiByteToWideChar(
            CP_UTF8,
            0,
            options.output_dir_utf8.cast(),
            -1,
            dir.as_mut_ptr(),
            dir.len() as i32,
        )
    };
    let mut end = usize::try_from(count).ok()?.checked_sub(1)?;
    while end > 1 && matches!(dir[end - 1], 0x5c | 0x2f) {
        end -= 1;
        dir[end] = 0;
    }
    Some(dir)
}

fn executable_name() -> [u8; PATH_CAP * 3] {
    let mut path = [0u16; PATH_CAP];
    let mut output = [0u8; PATH_CAP * 3];
    let count = unsafe { GetModuleFileNameW(null_mut(), path.as_mut_ptr(), PATH_CAP as u32) };
    if count != 0 && count < PATH_CAP as u32 {
        unsafe {
            WideCharToMultiByte(
                CP_UTF8,
                0,
                path.as_ptr(),
                -1,
                output.as_mut_ptr(),
                output.len() as i32,
                null(),
                null_mut(),
            );
        }
    }
    output
}

impl Core {
    fn now(&self) -> u64 {
        let mut time = 0;
        unsafe { QueryPerformanceCounter(&mut time) };
        let elapsed = (i128::from(time) - i128::from(self.start)).max(0);
        (elapsed * 1_000_000 / i128::from(self.frequency)) as u64
    }

    fn drain(&self) {
        let writer = rsf_census_record_writer {
            entries: self.platform.functions,
            entry_count: self.platform.count.into(),
            module_name: Some(rsf_census_module_name_from_base),
            frame_base: Some(rsf_census_module_base),
            append: Some(append_line),
            drop_size: Some(drop_line),
        };
        unsafe { rsf_census_record_drain(&writer) };
    }

    fn flush(&self, writer: &mut Writer) {
        self.drain();
        rsf_census_output_flush(self.now());
        writer.write_summary(self);
    }
}

unsafe extern "C" fn append_line(data: *const c_char, size: usize) {
    if let Some(core) = CORE.get() {
        unsafe { rsf_census_output_append(data, size, core.now()) };
    }
}
extern "C" fn drop_line() {
    DROPPED_SIZE.fetch_add(1, Relaxed);
}

fn json_buffer(output: &mut [u8], reserve: usize) -> rsf_census_json {
    rsf_census_json {
        buffer: output.as_mut_ptr().cast(),
        capacity: output.len() - reserve,
        length: 0,
        comma: 0,
        depth: 0,
        overflow: false,
    }
}

struct Json<'a>(&'a mut rsf_census_json);
impl Json<'_> {
    fn open(&mut self, key: Option<&CStr>, delimiter: u8) {
        unsafe {
            rsf_census_json_open(
                self.0,
                key.map_or(null(), CStr::as_ptr),
                delimiter as c_char,
            )
        };
    }
    fn close(&mut self, delimiter: u8) {
        rsf_census_json_close(self.0, delimiter as c_char);
    }
    fn number(&mut self, key: &CStr, value: u64) {
        unsafe { rsf_census_json_u64(self.0, key.as_ptr(), value) };
    }
    fn hex(&mut self, key: &CStr, value: u64) {
        unsafe { rsf_census_json_hex(self.0, key.as_ptr(), value) };
    }
    fn text(&mut self, key: &CStr, value: &CStr) {
        unsafe { rsf_census_json_string(self.0, key.as_ptr(), value.as_ptr()) };
    }
    fn raw(&mut self, key: &CStr, value: &CStr) {
        unsafe { rsf_census_json_raw(self.0, key.as_ptr(), value.as_ptr()) };
    }
}

impl Writer {
    fn write_summary(&mut self, core: &Core) {
        let mut state = json_buffer(&mut self.summary, 16);
        let mut json = Json(&mut state);
        json.open(None, b'{');
        json.number(c"census_abi", RSF_CENSUS_ABI_VERSION.into());
        let exe = CStr::from_bytes_until_nul(&core.exe).unwrap_or(c"");
        let units = exe.to_bytes_with_nul();
        let leaf = units
            .iter()
            .rposition(|&byte| byte == b'\\')
            .map_or(0, |index| index + 1);
        json.text(
            c"process",
            CStr::from_bytes_with_nul(&units[leaf..]).unwrap_or(c""),
        );
        json.number(c"pid", unsafe { GetCurrentProcessId() }.into());
        json.number(c"elapsed_ms", core.now() / 1000);
        let (mut calls, mut present, mut sites, mut overflow) = (0, 0, 0, 0);
        rsf_census_stats_counts(&mut calls, &mut present, &mut sites, &mut overflow);
        json.number(c"present_index", present);
        let (mut written, mut dropped, mut bytes, mut exhausted) = (0, 0, 0, false);
        rsf_census_output_counts(&mut written, &mut dropped, &mut bytes, &mut exhausted);
        json.open(Some(c"budget"), b'{');
        json.number(c"byte_budget", core.budget);
        json.number(c"bytes_written", bytes);
        json.raw(c"exhausted", if exhausted { c"true" } else { c"false" });
        json.number(c"full_record_cap", core.cap.into());
        json.number(c"sample_interval", core.interval.into());
        json.close(b'}');
        json.open(Some(c"counters"), b'{');
        json.number(c"calls_seen", calls);
        json.number(c"records_queued", QUEUED.load(Relaxed));
        json.number(c"records_written", written);
        json.number(c"dropped_queue", DROPPED_QUEUE.load(Relaxed));
        json.number(c"dropped_budget", dropped);
        json.number(c"dropped_size", DROPPED_SIZE.load(Relaxed));
        json.number(c"sites", sites.into());
        json.number(c"site_overflow", overflow.into());
        json.close(b'}');
        let names = rsf_census_stats_names {
            entries: core.platform.functions,
            module_name: Some(rsf_census_module_name),
        };
        unsafe {
            rsf_census_stats_sites(json.0, &names);
            rsf_census_stats_histograms(json.0, core.platform.functions);
            rsf_census_threads_summary(json.0);
            rsf_census_stats_queues(json.0);
            rsf_census_hooks_summary(json.0);
        }
        json.open(Some(c"static_findings"), b'[');
        if let Ok(findings) = FINDINGS.try_lock() {
            let count = (FINDING_COUNT.load(Relaxed) as usize).min(findings.len());
            for finding in &findings[..count] {
                json.open(None, b'{');
                json.text(
                    c"vendor",
                    CStr::from_bytes_until_nul(&finding.vendor).unwrap_or(c""),
                );
                json.text(
                    c"what",
                    CStr::from_bytes_until_nul(&finding.what).unwrap_or(c""),
                );
                json.hex(c"rva", finding.rva);
                json.hex(c"function_rva", finding.function);
                json.close(b'}');
            }
        }
        json.close(b']');
        json.close(b'}');
        if state.overflow {
            return;
        }
        let temp = path_with_suffix(&core.dir, "\\census-summary.json.tmp");
        let final_path = path_with_suffix(&core.dir, "\\census-summary.json");
        if rsf_census_output_summary(
            temp.as_ptr(),
            final_path.as_ptr(),
            state.buffer,
            state.length,
        ) {
            self.last_summary_us = core.now();
        }
    }
}

fn write_start(core: &Core) {
    let mut line = [0u8; 1024];
    let mut state = json_buffer(&mut line, 2);
    let mut json = Json(&mut state);
    json.open(None, b'{');
    json.text(c"type", c"census_start");
    json.number(c"census_abi", RSF_CENSUS_ABI_VERSION.into());
    json.number(c"pid", unsafe { GetCurrentProcessId() }.into());
    json.text(c"exe", CStr::from_bytes_until_nul(&core.exe).unwrap_or(c""));
    json.number(c"qpc_frequency", core.frequency as u64);
    json.number(c"byte_budget", core.budget);
    json.number(c"full_record_cap", core.cap.into());
    json.number(c"sample_interval", core.interval.into());
    json.close(b'}');
    if !state.overflow {
        line[state.length] = b'\n';
        unsafe { append_line(line.as_ptr().cast(), state.length + 1) };
        rsf_census_output_reset_records();
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_census_active() -> bool {
    ACTIVE.load(Acquire)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_census_flags() -> u32 {
    CORE.get().map_or(0, |core| core.flags)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_census_call_begin(
    state: *mut rsf_census_call_state,
    entry: u16,
    caller: *mut c_void,
) {
    unsafe {
        (*state).full = false;
        (*state).fn_ = entry;
    }
    let Some(core) = CORE.get().filter(|_| rsf_census_active()) else {
        return;
    };
    let base = rsf_census_module_base(caller);
    let rva = (caller as u64).wrapping_sub(base) as u32;
    let tid = unsafe { GetCurrentThreadId() };
    let mut sample = rsf_census_sample {
        ordinal: 0,
        site_ordinal: 0,
    };
    let full = rsf_census_stats_sample(entry, rsf_census_module_index(base), rva, tid, &mut sample);
    if sample.site_ordinal != 0 {
        rsf_census_thread_note(tid);
    }
    if !full {
        return;
    }
    let now = core.now();
    let mut frames = [null_mut(); STACK_FRAMES + 6];
    let count = unsafe {
        RtlCaptureStackBackTrace(0, frames.len() as u32, frames.as_mut_ptr(), null_mut())
    } as usize;
    let frames = &frames[..count.min(frames.len())];
    let first = frames
        .iter()
        .position(|&frame| frame == caller)
        .unwrap_or(0);
    let copied = (frames.len() - first).min(STACK_FRAMES);
    unsafe {
        (*state).full = true;
        (*state).base = base;
        (*state).rva = rva;
        (*state).tid = tid;
        (*state).ord = sample.ordinal;
        (*state).site_ord = sample.site_ordinal;
        (*state).t_us = now;
        (*state).stack_n = copied as u32;
        ptr::copy_nonoverlapping(
            frames.as_ptr().add(first),
            (&raw mut (*state).stack).cast(),
            copied,
        );
        (*state).json = rsf_census_json {
            buffer: (&raw mut (*state).body).cast(),
            capacity: RSF_CENSUS_RECORD_BODY_MAX as usize,
            length: 0,
            comma: 0,
            depth: 0,
            overflow: false,
        };
        Json(&mut (*state).json).open(None, b'{');
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_census_call_finish(
    state: *mut rsf_census_call_state,
    result: u64,
) {
    if !unsafe { (*state).full } || !rsf_census_active() {
        return;
    }
    let last = unsafe { GetLastError() };
    unsafe { finish_record(state, result) };
    unsafe { SetLastError(last) };
}

unsafe fn finish_record(state: *mut rsf_census_call_state, result: u64) {
    unsafe {
        Json(&mut (*state).json).close(b'}');
        if (*state).json.overflow {
            drop_line();
            return;
        }
        let mut ticket = 0;
        let record = rsf_census_record_acquire(&mut ticket);
        if record.is_null() {
            DROPPED_QUEUE.fetch_add(1, Relaxed);
            return;
        }
        (*record).len = (*state).json.length as u32;
        (*record).fn_ = (*state).fn_;
        (*record).t_us = (*state).t_us;
        (*record).present = rsf_census_stats_present_index();
        (*record).ord = (*state).ord;
        (*record).site_ord = (*state).site_ord;
        (*record).base = (*state).base;
        (*record).rva = (*state).rva;
        (*record).tid = (*state).tid;
        (*record).ret = result;
        (*record).stack_n = (*state).stack_n;
        ptr::copy_nonoverlapping(
            (&raw const (*state).stack).cast::<*mut c_void>(),
            (&raw mut (*record).stack).cast(),
            (*state).stack_n as usize,
        );
        rsf_census_thread_name(
            (*state).tid,
            (*state).t_us,
            &mut *(&raw mut (*record).thread).cast(),
        );
        ptr::copy_nonoverlapping(
            (&raw const (*state).body).cast::<c_char>(),
            (&raw mut (*record).body).cast(),
            (*record).len as usize,
        );
        rsf_census_record_publish(ticket);
        QUEUED.fetch_add(1, Relaxed);
    }
}

fn truncated<const N: usize>(text: *const c_char) -> [u8; N] {
    let mut output = [0; N];
    if !text.is_null() {
        let text = unsafe { CStr::from_ptr(text) }.to_bytes();
        let count = text.len().min(N - 1);
        output[..count].copy_from_slice(&text[..count]);
    }
    output
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_census_add_finding(
    vendor: *const c_char,
    what: *const c_char,
    rva: u64,
    function: u64,
) {
    let mut findings = lock(&FINDINGS);
    let index = FINDING_COUNT.fetch_add(1, Relaxed) as usize;
    if let Some(result) = findings.get_mut(index) {
        *result = Finding {
            vendor: truncated(vendor),
            what: truncated(what),
            rva,
            function,
        };
    }
}

fn event(
    core: &Core,
    pc: *mut c_void,
    name: &CStr,
    result: u64,
    fields: impl FnOnce(&mut Json<'_>),
) {
    let mut call = MaybeUninit::<rsf_census_call_state>::uninit();
    record_event(call.as_mut_ptr(), core, pc, name, result, fields);
}

fn record_event(
    raw: *mut rsf_census_call_state,
    core: &Core,
    pc: *mut c_void,
    name: &CStr,
    result: u64,
    fields: impl FnOnce(&mut Json<'_>),
) {
    unsafe {
        rsf_census_call_begin(raw, core.platform.event_entry, pc);
        if (*raw).full {
            let mut json = Json(&mut (*raw).json);
            json.text(c"event", name);
            fields(&mut json);
        }
        rsf_census_call_finish(raw, result);
    }
}

unsafe extern "system" fn static_scan(_: *mut c_void) -> u32 {
    let Some(core) = CORE.get() else { return 0 };
    let started = core.now();
    rsf_census_static_scan(Some(rsf_census_add_finding));
    event(
        core,
        static_scan as *mut c_void,
        c"static_scan_done",
        0,
        |json| {
            json.number(c"elapsed_ms", core.now().saturating_sub(started) / 1000);
            json.number(c"findings", FINDING_COUNT.load(Relaxed).into());
        },
    );
    0
}

unsafe extern "system" fn writer_main(_: *mut c_void) -> u32 {
    let Some(core) = CORE.get() else { return 0 };
    while !STOP.load(Acquire) {
        unsafe { WaitForSingleObject(core.wake.load(Acquire), 200) };
        if STOP.load(Acquire) {
            break;
        }
        let mut writer = lock(&core.writer);
        core.drain();
        let now = core.now();
        rsf_census_output_flush_due(now);
        if now.saturating_sub(writer.last_summary_us) >= core.period_us {
            core.flush(&mut writer);
        }
    }
    0
}

unsafe extern "system" fn fatal_exception(info: *mut EXCEPTION_POINTERS) -> i32 {
    const CONTINUE_SEARCH: i32 = 0;
    let Some(info) = (unsafe { info.as_ref() }) else {
        return CONTINUE_SEARCH;
    };
    let Some(exception) = (unsafe { info.ExceptionRecord.as_ref() }) else {
        return CONTINUE_SEARCH;
    };
    let code = exception.ExceptionCode as u32;
    if !matches!(code, 0x8000_0003 | 0x0517_a7ed | 0xc000_0409 | 0xc000_00fd)
        || FATAL_REPORTS.fetch_add(1, Relaxed) >= 8
        || !rsf_census_active()
    {
        return CONTINUE_SEARCH;
    }
    let Some(core) = CORE.get() else {
        return CONTINUE_SEARCH;
    };
    if core.writer_tid.load(Acquire) == unsafe { GetCurrentThreadId() } {
        return CONTINUE_SEARCH;
    }
    let address = exception.ExceptionAddress;
    if let Ok(mut slot) = CRASH_CALL.try_lock() {
        record_event(
            slot.0.as_mut_ptr(),
            core,
            address,
            c"fatal_exception",
            code.into(),
            |json| {
                json.hex(c"code", code.into());
                json.hex(c"address", address as u64);
                json.number(c"present", rsf_census_stats_present_index());
            },
        );
    }
    for _ in 0..200 {
        if let Ok(mut writer) = core.writer.try_lock() {
            core.flush(&mut writer);
            break;
        }
        unsafe { Sleep(10) };
    }
    CONTINUE_SEARCH
}

fn configure(
    options: &rsf_census_options,
    platform: &rsf_census_core_platform,
    mut dir: [u16; PATH_CAP],
) -> Result<Core, i32> {
    make_directories(&mut dir);
    let mut frequency = 0;
    let mut start = 0;
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0
        || frequency <= 0
        || unsafe { QueryPerformanceCounter(&mut start) } == 0
    {
        return Err(IO_ERROR);
    }
    let cap = configured(options.full_record_cap, 32);
    let interval = configured(options.sample_interval, 600);
    if !rsf_census_record_init() || !rsf_census_stats_init(platform.count, cap, interval) {
        return Err(IO_ERROR);
    }
    let mut summary = Vec::new();
    summary
        .try_reserve_exact(SUMMARY_BYTES)
        .map_err(|_| IO_ERROR)?;
    summary.resize(SUMMARY_BYTES, 0);
    let budget = if options.byte_budget == 0 {
        64 << 20
    } else {
        options.byte_budget
    };
    let path = path_with_suffix(&dir, "\\census.jsonl");
    if !rsf_census_output_open(path.as_ptr(), budget) {
        return Err(IO_ERROR);
    }
    let wake = unsafe { CreateEventW(null(), 0, 0, null()) };
    if wake.is_null() {
        rsf_census_output_close();
        return Err(IO_ERROR);
    }
    Ok(Core {
        platform: *platform,
        flags: options.flags,
        budget,
        cap,
        interval,
        period_us: u64::from(configured(options.summary_period_ms, 2000)) * 1000,
        dir,
        exe: executable_name(),
        frequency,
        start,
        wake: AtomicPtr::new(wake),
        writer_handle: AtomicPtr::new(null_mut()),
        scan_handle: AtomicPtr::new(null_mut()),
        writer_tid: AtomicU32::new(0),
        writer: Mutex::new(Writer {
            summary,
            last_summary_us: 0,
        }),
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_census_install_impl(
    options: *const rsf_census_options,
    platform: &rsf_census_core_platform,
) -> rsf_census_result {
    if options.is_null()
        || unsafe { options.cast::<u32>().read() } < size_of::<rsf_census_options>() as u32
    {
        return INVALID_ARGUMENT;
    }
    let options = unsafe { &*options };
    if options.abi_version != RSF_CENSUS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if EVER_INSTALLED
        .compare_exchange(false, true, AcqRel, Acquire)
        .is_err()
    {
        return ALREADY_INSTALLED;
    }
    INITIALIZING.store(true, Release);
    let _initializing = Initializing;
    let Some(dir) = output_dir(options) else {
        drop(_initializing);
        EVER_INSTALLED.store(false, Release);
        return INVALID_ARGUMENT;
    };
    let core = match configure(options, platform, dir) {
        Ok(core) => core,
        Err(error) => return error,
    };
    if CORE.set(core).is_err() {
        return ALREADY_INSTALLED;
    }
    let core = CORE.get().unwrap();
    {
        let _writer = lock(&core.writer);
        write_start(core);
    }
    ACTIVE.store(true, Release);
    // CreateThread does not wait for the worker. Under DllMain it runs after loader-lock release.
    let mut tid = 0;
    let worker = unsafe { CreateThread(null(), 0, Some(writer_main), null(), 0, &mut tid) };
    if worker.is_null() {
        return IO_ERROR;
    }
    core.writer_tid.store(tid, Release);
    core.writer_handle.store(worker, Release);
    unsafe { AddVectoredExceptionHandler(1, Some(fatal_exception)) };
    if !unsafe { core.platform.install_hooks.unwrap()() } {
        return HOOK_ERROR;
    }
    if core.flags & RSF_CENSUS_FLAG_NO_STATIC_SCAN == 0 {
        let scanner = unsafe { CreateThread(null(), 0, Some(static_scan), null(), 0, null_mut()) };
        core.scan_handle.store(scanner, Release);
    }
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_census_flush() -> rsf_census_result {
    if !EVER_INSTALLED.load(Acquire) || INITIALIZING.load(Acquire) {
        return NOT_INSTALLED;
    }
    if let Some(core) = CORE.get() {
        core.flush(&mut lock(&core.writer));
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_census_get_counters(out: *mut rsf_census_counters) -> rsf_census_result {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    let struct_size = unsafe { (&raw const (*out).struct_size).read() };
    if struct_size < size_of::<rsf_census_counters>() as u32 {
        return INVALID_ARGUMENT;
    }
    let abi_version = unsafe { (&raw const (*out).abi_version).read() };
    if abi_version != RSF_CENSUS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let (mut calls_seen, mut present_index, mut sites, mut site_overflow) = (0, 0, 0, 0);
    rsf_census_stats_counts(
        &mut calls_seen,
        &mut present_index,
        &mut sites,
        &mut site_overflow,
    );
    let (mut records_written, mut records_dropped_budget, mut bytes_written) = (0, 0, 0);
    let mut exhausted = false;
    rsf_census_output_counts(
        &mut records_written,
        &mut records_dropped_budget,
        &mut bytes_written,
        &mut exhausted,
    );
    let (mut hooks_installed, mut hook_conflicts, mut modules_hooked) = (0, 0, 0);
    rsf_census_hook_counts(
        &mut hooks_installed,
        &mut hook_conflicts,
        &mut modules_hooked,
    );
    unsafe {
        out.write(rsf_census_counters {
            struct_size,
            abi_version,
            calls_seen,
            present_index,
            sites,
            site_overflow,
            records_written,
            records_dropped_budget,
            bytes_written,
            records_queued: QUEUED.load(Relaxed),
            records_dropped_queue: DROPPED_QUEUE.load(Relaxed),
            records_dropped_size: DROPPED_SIZE.load(Relaxed),
            hooks_installed,
            hook_conflicts,
            modules_hooked,
            static_findings: FINDING_COUNT.load(Relaxed).min(FINDING_CAP as u32),
        })
    };
    OK
}

fn join(handle: &AtomicPtr<c_void>, timeout: u32) -> bool {
    let thread = handle.load(Acquire);
    if thread.is_null() {
        return true;
    }
    if unsafe { WaitForSingleObject(thread, timeout) } != WAIT_OBJECT_0 {
        return false;
    }
    unsafe { CloseHandle(thread) };
    handle.store(null_mut(), Release);
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_census_shutdown() -> rsf_census_result {
    static SHUTDOWN: Mutex<()> = Mutex::new(());
    let _shutdown = lock(&SHUTDOWN);
    let Some(core) = CORE.get().filter(|_| rsf_census_active()) else {
        return NOT_INSTALLED;
    };
    if INITIALIZING.load(Acquire) {
        return IO_ERROR;
    }
    STOP.store(true, Release);
    unsafe { SetEvent(core.wake.load(Acquire)) };
    if !join(&core.writer_handle, 10_000) || !join(&core.scan_handle, 60_000) {
        return IO_ERROR;
    }
    let mut writer = lock(&core.writer);
    core.flush(&mut writer);
    ACTIVE.store(false, Release);
    drop(writer);
    rsf_census_hooks_remove();
    let _writer = lock(&core.writer);
    rsf_census_output_close();
    let wake = core.wake.swap(null_mut(), AcqRel);
    if !wake.is_null() {
        unsafe { CloseHandle(wake) };
    }
    OK
}
