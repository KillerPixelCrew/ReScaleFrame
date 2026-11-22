// SPDX-License-Identifier: MIT
use super::module_dump_abi::{
    RSF_MODULE_DUMP_ABI_VERSION, rsf_dump_options, rsf_dump_report, rsf_dump_result,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::fs;
use std::mem::{offset_of, transmute};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    Security::Cryptography::{
        BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash,
        BCryptFinishHash, BCryptHashData, BCryptOpenAlgorithmProvider,
    },
    System::{
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Threading::{CreateEventW, CreateThread, SetEvent, Sleep, WaitForSingleObject},
    },
};
use windows_sys::core::w;

type Dump = unsafe extern "C" fn(
    *const c_void,
    *const rsf_dump_options,
    *mut rsf_dump_report,
) -> rsf_dump_result;

#[derive(Default)]
struct Checks {
    failed: bool,
}

impl Checks {
    fn check(&mut self, condition: bool, message: &str) -> bool {
        if !condition {
            eprintln!("{message}");
            self.failed = true;
        }
        condition
    }

    fn includes(&mut self, text: &str, part: &str, message: &str) {
        self.check(text.contains(part), message);
    }
}

unsafe extern "system" fn worker(release: *mut c_void) -> u32 {
    unsafe { WaitForSingleObject(release, 30_000) };
    0
}

struct Worker {
    release: HANDLE,
    thread: HANDLE,
    can_name: bool,
}

impl Worker {
    fn start(checks: &mut Checks) -> Self {
        let release = unsafe { CreateEventW(null(), 1, 0, null()) };
        let thread = if release.is_null() {
            null_mut()
        } else {
            unsafe { CreateThread(null(), 0, Some(worker), release, 0, null_mut()) }
        };
        checks.check(!thread.is_null(), "The worker thread must start.");
        let kernel = unsafe { GetModuleHandleW(w!("kernel32.dll")) };
        let name = if kernel.is_null() {
            None
        } else {
            unsafe { GetProcAddress(kernel, c"SetThreadDescription".as_ptr().cast()) }
        };
        if let Some(name) = name.filter(|_| !thread.is_null()) {
            let set_name = unsafe {
                transmute::<_, unsafe extern "system" fn(HANDLE, *const u16) -> i32>(name)
            };
            unsafe { set_name(thread, w!("rsf-dump-probe-worker")) };
        }
        unsafe { Sleep(50) };
        Self {
            release,
            thread,
            can_name: name.is_some(),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        unsafe {
            if !self.release.is_null() {
                SetEvent(self.release);
            }
            let stopped = if self.thread.is_null() {
                true
            } else {
                let stopped = WaitForSingleObject(self.thread, 5000) == WAIT_OBJECT_0;
                CloseHandle(self.thread);
                stopped
            };
            if stopped && !self.release.is_null() {
                CloseHandle(self.release);
            }
        }
    }
}

fn json_structure_ok(text: &str) -> bool {
    let mut stack = Vec::new();
    let mut string = false;
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        if string {
            match byte {
                b'\\' => {
                    if bytes.next().is_none() {
                        return false;
                    }
                }
                b'"' => string = false,
                0..=0x1f => return false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => string = true,
                b'{' => stack.push(b'}'),
                b'[' => stack.push(b']'),
                b'}' | b']' => {
                    if stack.pop() != Some(byte) {
                        return false;
                    }
                }
                _ => {}
            }
        }
    }
    !string && stack.is_empty()
}

fn section_hash(data: &[u8]) -> Option<String> {
    let mut provider = null_mut();
    if unsafe { BCryptOpenAlgorithmProvider(&mut provider, BCRYPT_SHA256_ALGORITHM, null(), 0) } < 0
    {
        return None;
    }
    let mut hash = null_mut();
    let mut digest = [0u8; 32];
    let ok = unsafe { BCryptCreateHash(provider, &mut hash, null_mut(), 0, null(), 0, 0) } >= 0
        && data.chunks(1 << 28).all(
            |chunk| unsafe { BCryptHashData(hash, chunk.as_ptr(), chunk.len() as u32, 0) } >= 0,
        )
        && unsafe { BCryptFinishHash(hash, digest.as_mut_ptr(), digest.len() as u32, 0) } >= 0;
    unsafe {
        if !hash.is_null() {
            BCryptDestroyHash(hash);
        }
        BCryptCloseAlgorithmProvider(provider, 0);
    }
    ok.then(|| digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn pe_sections(dump: &[u8]) -> Option<std::slice::ChunksExact<'_, u8>> {
    let nt = u32::from_le_bytes(dump.get(60..64)?.try_into().ok()?) as usize;
    let coff = dump.get(nt..)?.get(..24)?;
    if &dump[..2] != b"MZ" || &coff[..4] != b"PE\0\0" {
        return None;
    }
    let count = u16::from_le_bytes(coff[6..8].try_into().ok()?) as usize;
    let optional = u16::from_le_bytes(coff[20..22].try_into().ok()?) as usize;
    let start = nt.checked_add(24 + optional)?;
    Some(dump.get(start..)?.get(..count * 40)?.chunks_exact(40))
}

fn check_sections(checks: &mut Checks, text: &str, dump: &[u8]) {
    let Some(sections) = pe_sections(dump) else {
        checks.check(false, "The dump must have a complete PE section table.");
        return;
    };
    let mut entries = text
        .split_once("\"sections\": [")
        .map_or("", |(_, entries)| entries);
    checks.includes(
        entries,
        "\"entropy\": ",
        "Sections must carry sampled entropy.",
    );
    let mut hashed = 0;
    for section in sections {
        let size = u32::from_le_bytes(section[8..12].try_into().unwrap()) as usize;
        if size == 0 {
            continue;
        }
        let rva = u32::from_le_bytes(section[12..16].try_into().unwrap()) as usize;
        let Some((_, rest)) = entries.split_once("\"sha256\": \"") else {
            checks.check(false, "Every non-empty section must carry a hash.");
            break;
        };
        let Some(bytes) = dump.get(rva..).and_then(|bytes| bytes.get(..size)) else {
            checks.check(false, "The dump must cover the section.");
            break;
        };
        let actual = section_hash(bytes);
        checks.check(
            actual.is_some() && actual.as_deref() == rest.get(..64),
            "The recorded section hash must match the section bytes in the dump.",
        );
        hashed += 1;
        entries = rest.get(64..).unwrap_or_default();
    }
    checks.check(hashed > 0, "At least one section hash must be verified.");
}

fn check_threads(checks: &mut Checks, text: &str, named: bool, worker_rva: u64) {
    if named {
        let at = text.find("\"description\": \"rsf-dump-probe-worker\"");
        if checks.check(at.is_some(), "The named worker thread must be listed.") {
            let at = at.unwrap();
            let entry = text[..at]
                .rfind("{\"tid\"")
                .and_then(|begin| text[at..].find('}').map(|end| &text[begin..at + end]));
            if checks.check(entry.is_some(), "The thread object is malformed.") {
                let entry = entry.unwrap();
                checks.includes(
                    entry,
                    &format!("\"start_rva\": \"{worker_rva:#x}\""),
                    "The worker thread's start address must be its entry point's RVA.",
                );
                checks.includes(
                    entry,
                    "\"dumping\": false",
                    "The worker is not the dumping thread.",
                );
                checks.includes(entry, "\"priority\"", "Priority must be listed.");
            }
        }
    }
    checks.check(
        text.matches("\"dumping\": true").count() == 1,
        "Exactly the dumping thread must be marked.",
    );
}

fn check_rtti(checks: &mut Checks, text: &str, vtable_rva: u64, report: &rsf_dump_report) {
    checks.includes(
        text,
        "\"rtti\": {\"classes\": [",
        "The RTTI section must exist.",
    );
    checks.includes(
        text,
        "\"budget_ms\"",
        "The RTTI section must report its budget.",
    );
    checks.includes(text, "\"elapsed_ms\"", "Elapsed time must be reported.");
    if cfg!(target_env = "msvc") {
        let at = text.find("\"name\": \"RsfDumpProbeDerived\"");
        if checks.check(
            at.is_some(),
            "The polymorphic class must be found by its RTTI.",
        ) {
            let at = at.unwrap();
            let entry = text[..at].rfind("{\"vtable_rva\"").and_then(|begin| {
                text[at..]
                    .find("\"declared_bases\"")
                    .map(|end| &text[begin..at + end])
            });
            if checks.check(entry.is_some(), "The class entry is malformed.") {
                let entry = entry.unwrap();
                checks.check(
                    entry.starts_with(&format!("{{\"vtable_rva\": \"{vtable_rva:#x}\"")),
                    "The class must carry the vtable RVA read from the live object.",
                );
                checks.check(
                    entry.contains("\"decorated\": \".?AURsfDumpProbeDerived@")
                        || entry.contains("\"decorated\": \".?AVRsfDumpProbeDerived@"),
                    "The decorated name must be recorded.",
                );
                checks.includes(
                    entry,
                    "\"offset\": 0, \"cd_offset\": 0",
                    "A single-inheritance class has zero offsets.",
                );
                checks.includes(
                    entry,
                    "\"name\": \"RsfDumpProbeBase\"",
                    "The base class must come from the hierarchy descriptor.",
                );
            }
        }
        checks.check(
            report.rtti_classes > 0,
            "The report must count recovered classes.",
        );
    }
}

fn check_sidecar(
    checks: &mut Checks,
    text: &str,
    report: &rsf_dump_report,
    can_name: bool,
    worker_rva: u64,
    vtable_rva: u64,
) {
    checks.check(
        json_structure_ok(text),
        "The sidecar must be structurally valid JSON.",
    );
    for field in [
        "\"schema_version\": 2",
        "\"abi_version\": 1",
        "\"code_entropy\"",
        "\"resolves_into\"",
        "\"timing_ms\"",
    ] {
        checks.includes(text, field, &format!("The sidecar is missing {field}."));
    }
    check_threads(checks, text, can_name, worker_rva);
    check_rtti(checks, text, vtable_rva, report);
    let functions = text.split_once("\"functions\": {\"entries\": [");
    if checks.check(functions.is_some(), "The functions section must exist.") {
        let (_, functions) = functions.unwrap();
        let count = functions
            .split_once("\"count\": ")
            .and_then(|(_, text)| text.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|value| value.parse::<u32>().ok());
        checks.check(
            count.is_some_and(|count| count != 0),
            "An x64 executable must list function starts.",
        );
        checks.includes(functions, "\n    [", "Function entries must be listed.");
    }
    checks.check(
        report.function_starts > 0,
        "The report must count function starts.",
    );
    checks.includes(
        text,
        &format!("\"rva\": \"{worker_rva:#x}\", \"name\": \"Probe_Worker\""),
        "The hex-addressed name must be merged.",
    );
    checks.includes(
        text,
        r#""name": "Probe_Decimal \"quoted\"""#,
        "The decimal-addressed name must be merged and escaped.",
    );
    checks.includes(
        text,
        "\"skipped_lines\": 1",
        "The malformed line must be counted.",
    );
}

fn check_v1(checks: &mut Checks, dump: Dump, mut options: rsf_dump_options) {
    options.struct_size = offset_of!(rsf_dump_options, name_list_path_utf8) as u32;
    options.name_list_path_utf8 = null();
    options.output_name_utf8 = c"v1caller".as_ptr();
    let v1_size = offset_of!(rsf_dump_report, rtti_classes) as u32;
    let mut report = rsf_dump_report {
        struct_size: v1_size,
        rtti_classes: 0x1234_5678,
        ..Default::default()
    };
    checks.check(
        unsafe { dump(null(), &options, &mut report) } == 0,
        "A schema 1 sized options and report structure must be accepted.",
    );
    checks.check(
        report.struct_size == v1_size
            && report.sections_written > 0
            && report.rtti_classes == 0x1234_5678,
        "A schema 1 report must be filled without overrunning it.",
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_module_dump_v2(
    argc: i32,
    argv: *const *const c_char,
    vtable_rva: u64,
    dump: Dump,
) -> i32 {
    if argc != 2 {
        eprintln!("usage: module_dump_v2 <output directory>");
        return 2;
    }
    let path = unsafe { CStr::from_ptr(*argv.add(1)) };
    let Ok(directory) = path.to_str() else {
        eprintln!("The output directory must be valid UTF-8.");
        return 2;
    };
    let directory = Path::new(directory);
    let mut checks = Checks::default();
    let module = unsafe { GetModuleHandleW(null()) } as usize;
    let worker_rva = (worker as *const () as usize).wrapping_sub(module) as u64;
    let worker = Worker::start(&mut checks);
    let names_path = directory.join("v2-names.tsv");
    let names = format!(
        "# engine names\n{worker_rva:#x}\tProbe_Worker\n{vtable_rva}\tProbe_Decimal \"quoted\"\nnot-a-line\n"
    );
    checks.check(
        fs::write(&names_path, names).is_ok(),
        "The name list must be writable.",
    );
    let names_path = CString::new(names_path.to_string_lossy().as_bytes())
        .expect("directory came from a C string");
    let options = rsf_dump_options {
        struct_size: size_of::<rsf_dump_options>() as u32,
        abi_version: RSF_MODULE_DUMP_ABI_VERSION,
        output_directory_utf8: path.as_ptr(),
        output_name_utf8: c"v2".as_ptr(),
        entropy_threshold: 7.0,
        name_list_path_utf8: names_path.as_ptr(),
        rtti_budget_ms: 20_000,
        ..Default::default()
    };
    let mut report = rsf_dump_report {
        struct_size: size_of::<rsf_dump_report>() as u32,
        ..Default::default()
    };
    let result = unsafe { dump(null(), &options, &mut report) };
    checks.check(result == 0, "The dump must succeed.");
    check_v1(&mut checks, dump, options);
    let can_name = worker.can_name;
    drop(worker);
    if result != 0 {
        return 1;
    }

    let text = fs::read_to_string(directory.join("v2.dump.json"));
    let bytes = fs::read(directory.join("v2.dump"));
    checks.check(text.is_ok(), "The sidecar must exist.");
    checks.check(bytes.is_ok(), "The dump must exist.");
    if let (Ok(text), Ok(bytes)) = (text, bytes) {
        check_sidecar(
            &mut checks,
            &text,
            &report,
            can_name,
            worker_rva,
            vtable_rva,
        );
        check_sections(&mut checks, &text, &bytes);
    }
    i32::from(checks.failed)
}
