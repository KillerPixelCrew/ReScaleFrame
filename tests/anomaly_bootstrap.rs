// SPDX-License-Identifier: MIT
// Exercise the exported cold-start entry in foreign Windows processes.
use rsf_test_support::{Library, wide_os as wide};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::ptr;
use std::time::{Duration, Instant};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};
use windows::core::PCWSTR;

type Entry = unsafe extern "system" fn(*const u16) -> u32;

fn refused_observe(dll: &OsStr, directory: &Path, check_invalid_ini: bool) -> Result<(), String> {
    let module = Library::open(
        dll,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    )
    .map_err(|error| format!("Adapter load failed: {error}"))?;
    let entry = module
        .symbol(c"rsf_anomaly_start")
        .ok_or("Anomaly cold-start entry missing")?;
    let start =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Entry>(entry) };

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
    let runtime_loaded = || unsafe { GetModuleHandleW(PCWSTR(runtime.as_ptr())) }.is_ok();
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
