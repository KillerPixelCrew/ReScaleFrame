// SPDX-License-Identifier: MIT
// Exercise the exported cold-start entry in foreign Windows processes.
use std::ffi::{OsStr, c_char, c_void};
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::ptr;
use std::time::{Duration, Instant};

type Entry = unsafe extern "system" fn(*const u16) -> u32;
type Export = unsafe extern "system" fn() -> isize;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> Option<Export>;
    fn GetModuleHandleW(path: *const u16) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x100;
const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x1000;

struct Module(*mut c_void);

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}

fn refused_observe(dll: &OsStr, directory: &Path, check_invalid_ini: bool) -> Result<(), String> {
    let dll = wide(dll);
    let module = unsafe {
        LoadLibraryExW(
            dll.as_ptr(),
            ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    if module.is_null() {
        return Err(format!(
            "Adapter load failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let module = Module(module);
    let entry = unsafe { GetProcAddress(module.0, c"rsf_anomaly_start".as_ptr()) }
        .ok_or("Anomaly cold-start entry missing")?;
    let start = unsafe { std::mem::transmute::<Export, Entry>(entry) };

    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    if check_invalid_ini {
        let empty = [0u16];
        let missing = directory.join(format!("missing-{}.ini", std::process::id()));
        let missing = wide(missing.as_os_str());
        for path in [ptr::null(), empty.as_ptr(), missing.as_ptr()] {
            if unsafe { start(path) } != 1 {
                return Err("Invalid INI did not return configuration refusal".into());
            }
        }
    }
    let ini = directory.join("anomaly-test.ini");
    fs::write(&ini, b"[Anomaly]\nMode=Observe\n").map_err(|error| error.to_string())?;
    let ini = wide(ini.as_os_str());
    let runtime = wide(OsStr::new("ReScaleFrame.Runtime.dll"));
    let runtime_loaded = || !unsafe { GetModuleHandleW(runtime.as_ptr()) }.is_null();
    if runtime_loaded() {
        return Err("Runtime was already loaded".into());
    }
    let result = unsafe { start(ini.as_ptr()) };
    if result != 2 || runtime_loaded() {
        return Err(format!(
            "Unprofiled process must refuse before Runtime loads, result={result}"
        ));
    }
    Ok(())
}

fn named_copy(dll: &OsStr, directory: &Path, name: &str) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let folder = directory.join("named");
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let copy = folder.join(name);
    fs::copy(current, &copy).map_err(|error| error.to_string())?;
    let run = || -> std::io::Result<bool> {
        let mut child = Command::new(fs::canonicalize(&copy)?)
            .arg("--as-named-build")
            .arg(dll)
            .arg(folder.join("child"))
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status.success());
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                child.wait()?;
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    let result = run();
    let _ = fs::remove_file(copy);
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "{name} with a foreign hash did not complete its refusal check"
        )),
        Err(error) => Err(format!("{name} child failed: {error}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match args.as_slice() {
        [dll, directory] => refused_observe(dll, Path::new(directory), true).and_then(|()| {
            for name in ["AnomalyDX11.exe", "AnomalyDX11AVX.exe"] {
                named_copy(dll, Path::new(directory), name)?;
            }
            println!("Anomaly bootstrap: PASS (foreign host and profiled names with foreign hashes refuse)");
            Ok(())
        }),
        [mode, dll, directory] if mode == "--as-named-build" => {
            refused_observe(dll, Path::new(directory), false)
        }
        _ => return ExitCode::from(2),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
