// SPDX-License-Identifier: MIT
use super::vendor_census_asi_abi::*;
use super::{named, wide};
use serde_json::Value;
use std::{
    ffi::{CStr, c_char},
    fs,
    mem::transmute,
    path::Path,
};
use windows_sys::Win32::{
    Foundation::FARPROC,
    System::{Environment::SetEnvironmentVariableA, LibraryLoader::LoadLibraryW},
};

type Install = unsafe extern "C" fn(*const rsf_census_options) -> i32;
type CensusOp = unsafe extern "C" fn() -> i32;
type Counters = unsafe extern "C" fn(*mut rsf_census_counters) -> i32;
type Jitter = unsafe extern "C" fn(i32) -> i32;
type DispatchTwice = unsafe extern "C" fn(*mut [i32; 2]);

#[derive(Default)]
struct Checks(u32);

impl Checks {
    fn check(&mut self, pass: bool, message: &str) {
        if !pass {
            eprintln!("FAIL {message}");
            self.0 += 1;
        }
    }

    fn read(&mut self, dir: &Path, leaf: &str) -> Vec<u8> {
        match fs::read(dir.join(leaf)) {
            Ok(data) => data,
            Err(error) => {
                eprintln!("FAIL reading {leaf}: {error}");
                self.0 += 1;
                Vec::new()
            }
        }
    }

    fn summary(&mut self, dir: &Path) -> Value {
        let data = self.read(dir, "census-summary.json");
        match serde_json::from_slice(&data) {
            Ok(summary) => summary,
            Err(error) => {
                eprintln!("FAIL parsing census-summary.json: {error}");
                self.0 += 1;
                Value::Null
            }
        }
    }

    fn records(&mut self, data: &[u8]) -> Vec<Value> {
        data.split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .enumerate()
            .filter_map(|(index, line)| match serde_json::from_slice(line) {
                Ok(row) => Some(row),
                Err(error) => {
                    eprintln!("FAIL parsing census.jsonl line {}: {error}", index + 1);
                    self.0 += 1;
                    None
                }
            })
            .collect()
    }

    fn counters(&mut self, get: Counters) -> rsf_census_counters {
        let mut out = rsf_census_counters {
            struct_size: size_of::<rsf_census_counters>() as u32,
            abi_version: RSF_CENSUS_ABI_VERSION,
            ..Default::default()
        };
        self.check(unsafe { get(&mut out) } == 0, "read census counters");
        out
    }
}

unsafe fn directory<'a>(text: *const c_char) -> Option<&'a Path> {
    unsafe { CStr::from_ptr(text) }.to_str().ok().map(Path::new)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_budget(
    dir: *const c_char,
    install: Install,
    flush: CensusOp,
    get: Counters,
    shutdown: CensusOp,
    jitter: Jitter,
) -> i32 {
    let Some(path) = (unsafe { directory(dir) }) else {
        return 2;
    };
    let mut checks = Checks::default();
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: dir,
        byte_budget: 40_000,
        full_record_cap: 1_000_000,
        sample_interval: 1,
        summary_period_ms: 50,
        flags: RSF_CENSUS_FLAG_NO_STATIC_SCAN | RSF_CENSUS_FLAG_NO_DXGI | RSF_CENSUS_FLAG_NO_VULKAN,
    };
    checks.check(
        unsafe { install(&options) } == 0,
        "install census for budget scenario",
    );
    for index in 0..5000 {
        checks.check(
            unsafe { jitter(index) } == 0,
            "direct imported jitter function succeeds",
        );
    }
    checks.check(unsafe { flush() } == 0, "flush census records");
    let counters = checks.counters(get);
    let data = checks.read(path, "census.jsonl");
    checks.check(data.len() <= 40_000, "JSONL respects the byte budget");
    checks.check(
        counters.bytes_written == data.len() as u64,
        "counter matches JSONL byte length",
    );
    checks.check(
        counters.bytes_written > 30_000,
        "the scenario actually uses the budget",
    );
    checks.check(counters.calls_seen >= 5000, "all jitter calls count");
    let dropped = counters.records_dropped_budget + counters.records_dropped_queue;
    checks.check(dropped > 0, "the scenario drops records");
    checks.check(
        counters.records_dropped_size == 0,
        "the scenario drops no oversized records",
    );
    checks.check(
        counters.records_written + counters.records_dropped_budget == counters.records_queued,
        "queued count matches written and budget-dropped records",
    );
    let _ = checks.records(&data);
    let summary = checks.summary(path);
    let counted: u64 = summary["sites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|site| site["function"] == "ffxFsr2GetJitterOffset")
        .filter_map(|site| site["count"].as_u64())
        .sum();
    checks.check(counted == 5000, "summary retains every jitter call");
    checks.check(
        summary["budget"]["exhausted"] == true,
        "summary reports exhaustion",
    );
    checks.check(
        summary["budget"]["bytes_written"]
            .as_u64()
            .is_some_and(|bytes| bytes <= 40_000),
        "summary respects the byte budget",
    );
    checks.check(unsafe { shutdown() } == 0, "shut down census");
    println!(
        "vendor_census budget: {} failure(s), {} bytes written, {dropped} records dropped",
        checks.0, counters.bytes_written
    );
    i32::from(checks.0 != 0)
}

struct Asi {
    result: CensusOp,
    flush: CensusOp,
    counters: Counters,
    shutdown: CensusOp,
}

fn load_asi() -> Option<Asi> {
    let path = std::env::current_exe()
        .ok()?
        .with_file_name("rsf_census_asi.dll");
    let path = wide(path);
    let module = unsafe { LoadLibraryW(path.as_ptr()) };
    if module.is_null() {
        return None;
    }
    type Entry = unsafe extern "system" fn() -> isize;
    let resolve: [FARPROC; 4] = [
        named(module, c"asi_install_result"),
        named(module, c"asi_flush"),
        named(module, c"asi_counters"),
        named(module, c"asi_shutdown"),
    ];
    Some(Asi {
        result: unsafe { transmute::<Entry, CensusOp>(resolve[0]?) },
        flush: unsafe { transmute::<Entry, CensusOp>(resolve[1]?) },
        counters: unsafe { transmute::<Entry, Counters>(resolve[2]?) },
        shutdown: unsafe { transmute::<Entry, CensusOp>(resolve[3]?) },
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_dllmain(dir: *const c_char, dispatch: DispatchTwice) -> i32 {
    let Some(path) = (unsafe { directory(dir) }) else {
        return 2;
    };
    let mut checks = Checks::default();
    checks.check(
        unsafe { SetEnvironmentVariableA(c"RSF_CENSUS_OUT".as_ptr().cast(), dir.cast()) } != 0,
        "configure the ASI fixture output",
    );
    let Some(asi) = load_asi() else {
        eprintln!("FAIL load the ASI fixture and its required exports");
        return 1;
    };
    checks.check(
        unsafe { (asi.result)() } == 0,
        "ASI installs under the loader lock",
    );
    let mut receipts = [0; 2];
    unsafe { dispatch(&mut receipts) };
    checks.check(
        receipts == [0x1234_abcd; 2],
        "both native FSR2 dispatch return values survive",
    );
    checks.check(unsafe { (asi.flush)() } == 0, "flush ASI census records");
    let counters = checks.counters(asi.counters);
    checks.check(
        counters.hooks_installed > 0 && counters.modules_hooked > 0,
        "hook modules already mapped",
    );
    let data = checks.read(path, "census.jsonl");
    let rows = checks.records(&data);
    let dispatches: Vec<_> = rows
        .iter()
        .filter(|row| row["fn"] == "ffxFsr2ContextDispatch")
        .collect();
    checks.check(
        dispatches.len() == 2,
        "both native FSR2 dispatches are recorded",
    );
    if let Some(first) = dispatches.first() {
        checks.check(
            first["args"]["layout_check"] == "ok",
            "first dispatch layout is accepted",
        );
        let module = std::env::current_exe().ok().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
        checks.check(
            module
                .as_deref()
                .is_some_and(|module| first["module"] == module),
            "first dispatch belongs to the fixture executable",
        );
    }
    let summary = checks.summary(path);
    checks.check(
        summary["modules_hooked"].as_array().is_some_and(|modules| {
            modules
                .iter()
                .any(|module| module == "ffx_fsr2_api_x64.dll")
        }),
        "summary identifies the already mapped FSR2 fixture",
    );
    checks.check(unsafe { (asi.shutdown)() } == 0, "shut down ASI census");
    println!("vendor_census dllmain: {} failure(s)", checks.0);
    i32::from(checks.0 != 0)
}
