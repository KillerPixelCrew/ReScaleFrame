// SPDX-License-Identifier: MIT
use super::memory::{MemoryCache, read, read_exact, readable_extent};
use super::names::escape_unit;
use super::sections_abi::rsf_module_entry;
use std::ffi::{OsStr, c_void};
use std::fmt::{self, Write as _};
use std::fs::File;
use std::io::{self, BufWriter, Write as _};
use std::mem::transmute;
use std::ptr::null_mut;
use std::slice;
use windows_sys::Win32::System::{
    Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE,
        TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    },
    LibraryLoader::{GetModuleHandleW, GetProcAddress},
    Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    SystemInformation::GetTickCount64,
    Threading::{
        GetCurrentProcessId, GetCurrentThreadId, GetThreadPriority, OpenThread,
        THREAD_QUERY_INFORMATION, THREAD_QUERY_LIMITED_INFORMATION,
    },
};
use windows_sys::{
    Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    core::w,
};

pub(super) struct Stream {
    output: BufWriter<File>,
    failed: bool,
}

impl Stream {
    pub(super) fn open(path: &OsStr) -> io::Result<Self> {
        Ok(Self {
            output: BufWriter::new(File::create(path)?),
            failed: false,
        })
    }

    pub(super) fn emit(&mut self, text: &str) {
        if !self.failed {
            self.failed = self.output.write_all(text.as_bytes()).is_err();
        }
    }

    pub(super) fn finish(mut self) -> bool {
        !self.failed && self.output.flush().is_ok()
    }
}

impl fmt::Write for Stream {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.emit(text);
        Ok(())
    }
}

pub(super) fn rsf_dump_now_ms() -> f64 {
    let (mut freq, mut counter) = (0, 0);
    if unsafe { QueryPerformanceFrequency(&mut freq) } != 0
        && freq != 0
        && unsafe { QueryPerformanceCounter(&mut counter) } != 0
    {
        counter as f64 * 1000.0 / freq as f64
    } else {
        (unsafe { GetTickCount64() }) as f64
    }
}

fn elapsed(start: f64) -> u32 {
    (rsf_dump_now_ms() - start + 0.5) as u32
}

const MAX_THREADS: u32 = 4096;
const THREAD_WIN32_START: u32 = 9;
type Query = unsafe extern "system" fn(HANDLE, u32, *mut c_void, u32, *mut u32) -> i32;
type Describe = unsafe extern "system" fn(HANDLE, *mut *mut u16) -> i32;
type Proc = unsafe extern "system" fn() -> isize;

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub(super) fn collect_modules(entries: &mut [rsf_module_entry]) -> usize {
    if entries.is_empty() {
        return 0;
    }
    let mut count = 0;
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return 0;
    }
    let snapshot = Handle(snapshot);
    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    let mut has_entry = unsafe { Module32FirstW(snapshot.0, &mut entry) } != 0;
    while has_entry && count < entries.len() {
        let mut path = [0; super::sections_abi::RSF_DUMP_MODULE_PATH_UNITS as usize];
        let limit = path.len() - 1;
        let length = entry
            .szExePath
            .iter()
            .take(limit)
            .position(|&unit| unit == 0)
            .unwrap_or(limit);
        path[..length].copy_from_slice(&entry.szExePath[..length]);
        let module = rsf_module_entry {
            path,
            base: entry.modBaseAddr,
            size: entry.modBaseSize as usize,
        };
        entries[count] = module;
        count += 1;
        has_entry = unsafe { Module32NextW(snapshot.0, &mut entry) } != 0;
    }
    count
}

fn thread_module(start: u64, modules: &[rsf_module_entry]) -> Option<(&rsf_module_entry, u64)> {
    if start == 0 {
        return None;
    }
    modules.iter().find_map(|module| {
        let rva = start.checked_sub(module.base as u64)?;
        (rva < module.size as u64).then_some((module, rva))
    })
}

fn wide(out: &mut Stream, value: &[u16]) {
    for &unit in value.iter().take_while(|&&unit| unit != 0) {
        escape_unit(out, unit);
    }
}

fn describe_thread(out: &mut Stream, thread: &Handle, describe: Describe) {
    let mut description: *mut u16 = null_mut();
    if unsafe { describe(thread.0, &mut description) } < 0 || description.is_null() {
        return;
    }
    unsafe {
        if description.read() != 0 {
            out.emit(", \"description\": \"");
            let mut unit = description;
            while unit.read() != 0 {
                escape_unit(out, unit.read());
                unit = unit.add(1);
            }
            out.emit("\"");
        }
        LocalFree(description.cast());
    }
}

struct ThreadView<'a> {
    modules: &'a [rsf_module_entry],
    query: Option<Query>,
    describe: Option<Describe>,
    dumping: u32,
}

impl ThreadView<'_> {
    fn emit(&self, out: &mut Stream, entry: &THREADENTRY32, comma: &str) {
        let tid = entry.th32ThreadID;
        let mut thread = unsafe {
            OpenThread(
                THREAD_QUERY_INFORMATION | THREAD_QUERY_LIMITED_INFORMATION,
                0,
                tid,
            )
        };
        if thread.is_null() {
            thread = unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, tid) };
        }
        let thread = (!thread.is_null()).then(|| Handle(thread));
        let mut start = 0;
        if let (Some(thread), Some(query)) = (&thread, self.query) {
            let mut address: *mut c_void = null_mut();
            let result = unsafe {
                query(
                    thread.0,
                    THREAD_WIN32_START,
                    (&raw mut address).cast(),
                    size_of::<*mut c_void>() as u32,
                    null_mut(),
                )
            };
            if result >= 0 {
                start = address as u64;
            }
        }
        let dumping = tid == self.dumping;
        let _ = write!(
            out,
            "{comma}\n    {{\"tid\": {tid}, \"dumping\": {dumping}, \"start_address\": \"{start:#x}\""
        );
        if let Some((owner, rva)) = thread_module(start, self.modules) {
            let path = &owner.path[..owner
                .path
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(owner.path.len())];
            let leaf = path
                .iter()
                .rposition(|&unit| unit == u16::from(b'\\'))
                .map_or(path, |i| &path[i + 1..]);
            out.emit(", \"start_module\": \"");
            wide(out, leaf);
            let _ = write!(out, "\", \"start_rva\": \"{rva:#x}\"");
        }
        if let Some(thread) = &thread {
            if let Some(describe) = self.describe {
                describe_thread(out, thread, describe);
            }
            let priority = unsafe { GetThreadPriority(thread.0) };
            if priority != i32::MAX {
                let _ = write!(out, ", \"priority\": {priority}");
            }
        }
        let _ = write!(out, ", \"base_priority\": {}}}", entry.tpBasePri);
    }
}

pub(super) unsafe fn write_threads(
    out: &mut Stream,
    modules: *const rsf_module_entry,
    count: usize,
) -> u32 {
    let started = rsf_dump_now_ms();
    let modules = if modules.is_null() || count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(modules, count) }
    };
    let ntdll = unsafe { GetModuleHandleW(w!("ntdll.dll")) };
    let kernel = unsafe { GetModuleHandleW(w!("kernel32.dll")) };
    let query = if ntdll.is_null() {
        None
    } else {
        unsafe { GetProcAddress(ntdll, c"NtQueryInformationThread".as_ptr().cast()) }
            .map(|proc| unsafe { transmute::<Proc, Query>(proc) })
    };
    let describe = if kernel.is_null() {
        None
    } else {
        unsafe { GetProcAddress(kernel, c"GetThreadDescription".as_ptr().cast()) }
            .map(|proc| unsafe { transmute::<Proc, Describe>(proc) })
    };
    let view = ThreadView {
        modules,
        query,
        describe,
        dumping: unsafe { GetCurrentThreadId() },
    };
    let process = unsafe { GetCurrentProcessId() };
    out.emit("  \"threads\": {\"list\": [");
    let mut listed = 0;
    let mut truncated = false;
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot != INVALID_HANDLE_VALUE {
        let snapshot = Handle(snapshot);
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut has_entry = unsafe { Thread32First(snapshot.0, &mut entry) } != 0;
        while has_entry {
            if entry.th32OwnerProcessID == process {
                if listed >= MAX_THREADS {
                    truncated = true;
                    break;
                }
                view.emit(out, &entry, if listed == 0 { "" } else { "," });
                listed += 1;
            }
            has_entry = unsafe { Thread32Next(snapshot.0, &mut entry) } != 0;
        }
    }
    let source = if query.is_some() {
        "NtQueryInformationThread(ThreadQuerySetWin32StartAddress)"
    } else {
        "unavailable"
    };
    let _ = writeln!(
        out,
        "\n  ], \"count\": {listed}, \"truncated\": {truncated}, \"start_address_source\": \"{source}\", \"elapsed_ms\": {}}},",
        elapsed(started)
    );
    listed
}

const ENTRY_BYTES: usize = 12;
const MAX_PDATA_BYTES: u32 = 96 * 1024 * 1024;
const CHUNK_BYTES: usize = ENTRY_BYTES * 1024;
const UNW_FLAG_CHAININFO: u8 = 4;

struct Entry {
    begin: u32,
    end: u32,
    unwind: u32,
    chained: bool,
}

#[derive(Default)]
struct Functions {
    entries: Vec<Entry>,
    raw: u32,
    invalid: u32,
    reason: Option<&'static str>,
}

impl Functions {
    fn read(base: usize, image_size: u32, directory: u32, size: u32) -> Self {
        let mut table = Self::default();
        let location = base.checked_add(directory as usize);
        if directory == 0 || size < ENTRY_BYTES as u32 {
            table.reason = Some("the module has no exception directory");
        } else if size > MAX_PDATA_BYTES
            || directory
                .checked_add(size)
                .is_none_or(|end| end > image_size)
            || base.checked_add(image_size as usize).is_none()
        {
            table.reason =
                Some("the exception directory lies outside the image or exceeds the size bound");
        } else if let Some(location) = location {
            table.read_entries(base, image_size, location, size as usize);
        }
        table
    }

    fn read_entries(&mut self, base: usize, image_size: u32, location: usize, size: usize) {
        let wanted = size / ENTRY_BYTES;
        let mut cache = MemoryCache::default();
        let readable = readable_extent(&mut cache, location, wanted * ENTRY_BYTES) / ENTRY_BYTES;
        if self.entries.try_reserve_exact(readable).is_err() {
            self.reason = Some("allocation failed");
            return;
        }
        if readable < wanted {
            self.reason = Some("part of the exception directory is not readable");
        }
        let length = readable * ENTRY_BYTES;
        let mut chunk = [0u8; CHUNK_BYTES];
        for offset in (0..length).step_by(CHUNK_BYTES) {
            let bytes = (length - offset).min(CHUNK_BYTES);
            // A page can lose readability after VirtualQuery. Decode only complete copied entries.
            let (ok, copied) = unsafe { read(location + offset, chunk.as_mut_ptr(), bytes) };
            for entry in chunk[..copied].chunks_exact(ENTRY_BYTES) {
                self.read_entry(base, image_size, &mut cache, entry);
            }
            if !ok || copied != bytes {
                self.reason = Some("part of the exception directory is not readable");
                break;
            }
        }
        self.entries
            .sort_unstable_by_key(|entry| (entry.begin, entry.chained, entry.end));
        self.entries.dedup_by_key(|entry| entry.begin);
    }

    fn read_entry(&mut self, base: usize, image_size: u32, cache: &mut MemoryCache, entry: &[u8]) {
        self.raw += 1;
        let begin = u32::from_le_bytes(entry[0..4].try_into().unwrap());
        let end = u32::from_le_bytes(entry[4..8].try_into().unwrap());
        let unwind = u32::from_le_bytes(entry[8..12].try_into().unwrap());
        if begin == 0 || end <= begin || end > image_size {
            self.invalid += 1;
            return;
        }
        let chained = unwind & 1 == 0
            && unwind < image_size
            && read_exact::<1>(cache, base + unwind as usize)
                .is_some_and(|[flags]| (flags >> 3) & UNW_FLAG_CHAININFO != 0);
        self.entries.push(Entry {
            begin,
            end,
            unwind,
            chained,
        });
    }
}

pub(super) fn write_functions(
    out: &mut Stream,
    base: *const u8,
    image_size: u32,
    directory: u32,
    size: u32,
) -> u32 {
    let started = rsf_dump_now_ms();
    let table = Functions::read(base as usize, image_size, directory, size);
    let count = table.entries.len();
    let mut chained = 0;
    out.emit("  \"functions\": {\"entries\": [");
    for (index, row) in table.entries.iter().enumerate() {
        let comma = if index == 0 { "" } else { "," };
        let chain = u32::from(row.chained);
        let _ = write!(
            out,
            "{comma}\n    [{},{},{},{chain}]",
            row.begin, row.end, row.unwind
        );
        chained += chain;
    }
    let _ = write!(
        out,
        "\n  ], \"format\": \"[begin_rva, end_rva, unwind_rva, chained] as decimal integers\", \
         \"source\": \"exception directory (.pdata RUNTIME_FUNCTION)\", \
         \"raw_entries\": {}, \"invalid\": {}, \"count\": {count}, \"chained\": {chained}, ",
        table.raw, table.invalid
    );
    if let Some(reason) = table.reason {
        let _ = write!(out, "\"reason\": \"{reason}\", ");
    }
    let _ = writeln!(out, "\"elapsed_ms\": {}}},", elapsed(started));
    count as u32
}
