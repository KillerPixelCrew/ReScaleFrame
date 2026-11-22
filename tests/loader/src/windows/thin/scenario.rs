// SPDX-License-Identifier: MIT
use super::super::{address, fail, load, null, null_mut, require, wide};
use super::{Status, create_object, create_proc, release};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use windows_sys::Win32::Foundation::{
    CloseHandle, FreeLibrary, GENERIC_WRITE, HWND, INVALID_HANDLE_VALUE, LPARAM,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, OPEN_EXISTING,
};
use windows_sys::Win32::System::Diagnostics::Debug::{GetErrorMode, GetThreadErrorMode};
use windows_sys::Win32::System::Environment::SetEnvironmentVariableW;
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;
use windows_sys::Win32::System::Threading::{
    ExitProcess, GetCurrentProcessId, GetCurrentThreadId, Sleep,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
};
use windows_sys::core::{BOOL, w};

const RUNTIME: &str = "ReScaleFrame/RuntimeOne.dll";
const DEFAULT_RUNTIME: &str = "ReScaleFrame/ReScaleFrame.Runtime.dll";
const ADAPTER: &str = "ReScaleFrame/AdapterOne.dll";
const ENTRY: &str = "ThinFixtureStart";

fn io<T>(result: io::Result<T>, context: &str) -> T {
    result.unwrap_or_else(|error| fail(&format!("{context}: {error}")))
}

fn junction(link: &Path, target: &Path) {
    io(fs::create_dir(link), "create junction directory");
    let substitute = OsStr::new(r"\??\")
        .encode_wide()
        .chain(target.as_os_str().encode_wide().map(|unit| {
            if unit == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                unit
            }
        }))
        .collect::<Vec<_>>();
    let length =
        u16::try_from(substitute.len() * 2).unwrap_or_else(|_| fail("junction target length"));
    require(
        length <= u16::MAX - 12,
        "junction lengths fit the reparse record",
    );
    let mut reparse = Vec::new();
    reparse.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    for field in [8 + length + 4, 0, 0, length, length + 2, 0] {
        reparse.extend(field.to_le_bytes());
    }
    reparse.extend(
        substitute
            .into_iter()
            .chain([0, 0])
            .flat_map(u16::to_le_bytes),
    );
    unsafe {
        let directory = CreateFileW(
            wide(link).as_ptr(),
            GENERIC_WRITE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        );
        require(directory != INVALID_HANDLE_VALUE, "open junction");
        let mut returned = 0;
        let ok = DeviceIoControl(
            directory,
            FSCTL_SET_REPARSE_POINT,
            reparse.as_ptr().cast(),
            reparse.len() as u32,
            null_mut(),
            0,
            &mut returned,
            null_mut(),
        );
        require(ok != 0, "create native unprivileged junction fixture");
        CloseHandle(directory);
    }
}

unsafe extern "system" fn window(window: HWND, _: LPARAM) -> BOOL {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, &mut pid);
        if pid == GetCurrentProcessId() && IsWindowVisible(window) != 0 {
            eprintln!("FAIL: loader displayed a visible window in its test subprocess");
            ExitProcess(2);
        }
    }
    1
}

struct Watch<'scope> {
    stop: &'scope AtomicBool,
    worker: Option<thread::ScopedJoinHandle<'scope, ()>>,
}

impl Drop for Watch<'_> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            require(worker.join().is_ok(), "join loader window watcher");
        }
    }
}

fn watch<'scope, 'env>(
    scope: &'scope thread::Scope<'scope, 'env>,
    stop: &'scope AtomicBool,
) -> Watch<'scope> {
    let worker = scope.spawn(|| unsafe {
        let deadline = GetTickCount64() + 4000;
        while !stop.load(Ordering::Relaxed) {
            EnumWindows(Some(window), 0);
            if GetTickCount64() >= deadline {
                eprintln!("FAIL: native loader exceeded four-second no-UI deadline");
                ExitProcess(2);
            }
            Sleep(2);
        }
    });
    Watch {
        stop,
        worker: Some(worker),
    }
}

fn configured(runtime: &str, adapter: &str, entry: &str) -> String {
    format!(
        "[Loader]\nAutoStart=1\nRuntimeModule={runtime}\nEntryModule={adapter}\nEntryPoint={entry}\n"
    )
}

fn scenario_config(mode: &str, valid: &str, nonce: &str) -> (Vec<u8>, &'static [u8]) {
    let invalid = |text: String| (text.into_bytes(), b"".as_slice());
    match mode {
        "malformed" => invalid(valid.replace("AutoStart=1", "AutoStart=1trailing")),
        "duplicate" => invalid(format!("{valid}aUtOsTaRt=0\n")),
        "escape" => invalid(configured(
            &format!("../OutsideRuntime-{nonce}.dll"),
            ADAPTER,
            ENTRY,
        )),
        "junction" => invalid(configured("Linked/RuntimeOne.dll", ADAPTER, ENTRY)),
        "empty" => invalid(String::new()),
        "missing_section" => invalid("[Other]\nAutoStart=1\n".into()),
        "missing_auto" => invalid(format!(
            "[Loader]\nEntryModule={ADAPTER}\nEntryPoint={ENTRY}\n"
        )),
        "missing_entry" => invalid(format!("[Loader]\nAutoStart=1\nRuntimeModule={RUNTIME}\n")),
        "empty_runtime" => invalid(configured("", ADAPTER, ENTRY)),
        "empty_entry" => invalid(configured(RUNTIME, "", ENTRY)),
        "invalid_symbol" => invalid(configured(RUNTIME, ADAPTER, "Bad Export")),
        "unknown_key" => invalid(format!("{valid}RunAnotherRuntime=yes\n")),
        "duplicate_section" => invalid(format!("{valid}[lOaDeR]\nAutoStart=1\n")),
        "syntax" => invalid(format!("{valid}not a key value pair\n")),
        "nul" => invalid(format!("{valid}\0ignored")),
        "bad_utf8" => ([valid.as_bytes(), b"\xff"].concat(), b""),
        "oversized" => invalid(format!("{valid}{}", ";".repeat(65537))),
        "absolute" => invalid(configured(
            "C:/Windows/System32/dinput8.dll",
            ADAPTER,
            ENTRY,
        )),
        "rooted" => invalid(configured("/Windows/System32/dinput8.dll", ADAPTER, ENTRY)),
        "drive_relative" => invalid(configured("C:RuntimeOne.dll", ADAPTER, ENTRY)),
        "unc" => invalid(configured(
            r"\\localhost\share\RuntimeOne.dll",
            ADAPTER,
            ENTRY,
        )),
        "ads" => invalid(configured(
            "ReScaleFrame/RuntimeOne.dll:stream",
            ADAPTER,
            ENTRY,
        )),
        "trailing_dot" => invalid(configured("ReScaleFrame/RuntimeOne.dll.", ADAPTER, ENTRY)),
        "empty_component" => invalid(configured("ReScaleFrame//RuntimeOne.dll", ADAPTER, ENTRY)),
        "parent_component" => invalid(configured(
            "ReScaleFrame/../ReScaleFrame/RuntimeOne.dll",
            ADAPTER,
            ENTRY,
        )),
        "adapter_escape" => invalid(configured(RUNTIME, "../OutsideAdapter.dll", ENTRY)),
        "same_module" => invalid(configured(RUNTIME, RUNTIME, ENTRY)),
        "self_module" => invalid(configured("dinput8.dll", ADAPTER, ENTRY)),
        "disabled" => invalid("[Loader]\nAutoStart=false\n".into()),
        "default" => (
            format!("[Loader]\nAutoStart=true\nEntryModule={ADAPTER}\nEntryPoint={ENTRY}\n")
                .into_bytes(),
            b"RAE",
        ),
        "utf8_bom" => ([b"\xef\xbb\xbf", valid.as_bytes()].concat(), b"RAE"),
        "utf16" => (
            std::iter::once(0xfeff)
                .chain(valid.encode_utf16())
                .flat_map(u16::to_le_bytes)
                .collect(),
            b"RAE",
        ),
        "shared_config" => (
            format!("; comment\n[Other]\nUnrelated=ok\n{valid}").into_bytes(),
            b"RAE",
        ),
        "missing_export" => (
            configured(RUNTIME, ADAPTER, "MissingExport").into_bytes(),
            b"RA",
        ),
        "missing_config" | "missing_runtime" | "missing_adapter" | "bad_runtime_image"
        | "hardlink_alias" => invalid(valid.into()),
        "bad_adapter_image" => (valid.as_bytes().to_vec(), b"R"),
        "startup" | "concurrent" | "scoped_modes" | "refuse" | "reentrant" => {
            (valid.as_bytes().to_vec(), b"RAE")
        }
        _ => fail("unknown startup scenario"),
    }
}

fn environment(name: *const u16, value: Option<&OsStr>) {
    let value = value.map(wide);
    require(
        unsafe {
            SetEnvironmentVariableW(name, value.as_ref().map_or(null(), |units| units.as_ptr()))
        } != 0
            || value.is_none(),
        "configure fixture environment",
    );
}

pub(super) fn startup_test(mode: &str, source: &Path, runtime: &Path, adapter: &Path) {
    let nonce = format!("{}-{}", unsafe { GetCurrentProcessId() }, unsafe {
        GetTickCount64()
    });
    let parent = source
        .parent()
        .unwrap_or_else(|| fail("carrier parent directory"));
    let root = parent.join(format!("fixture-{mode}-{nonce}-é"));
    io(
        fs::create_dir_all(root.join("ReScaleFrame")),
        "create isolated Unicode fixture root",
    );
    io(
        fs::copy(source, root.join("dinput8.dll")),
        "copy thin carrier",
    );
    for name in [RUNTIME, DEFAULT_RUNTIME] {
        io(
            fs::copy(runtime, root.join(name)),
            "copy configured and decoy runtimes",
        );
    }
    io(
        fs::copy(adapter, root.join(ADAPTER)),
        "copy configured adapter",
    );
    if mode == "junction" {
        let outside = parent.join(format!("outside-{nonce}"));
        io(fs::create_dir(&outside), "create outside junction target");
        io(
            fs::copy(runtime, outside.join("RuntimeOne.dll")),
            "copy runtime to junction target",
        );
        junction(&root.join("Linked"), &outside);
        require(
            root.join("Linked/RuntimeOne.dll").exists(),
            "junction actually resolves outside carrier directory",
        );
    }
    if mode == "escape" {
        io(
            fs::copy(runtime, parent.join(format!("OutsideRuntime-{nonce}.dll"))),
            "copy outside runtime",
        );
    }
    let config = root.join("ReScaleFrame.ini");
    let valid = configured(RUNTIME, ADAPTER, ENTRY);
    let (config_bytes, expected) = scenario_config(mode, &valid, &nonce);
    io(
        fs::write(&config, config_bytes),
        "write scenario configuration",
    );
    let final_config = io(fs::canonicalize(&config), "canonical fixture config path");
    match mode {
        "missing_config" => io(fs::remove_file(&config), "remove config"),
        "missing_runtime" => io(fs::remove_file(root.join(RUNTIME)), "remove runtime"),
        "missing_adapter" | "hardlink_alias" => {
            io(fs::remove_file(root.join(ADAPTER)), "remove adapter")
        }
        "bad_runtime_image" => io(
            fs::write(root.join(RUNTIME), b"not a PE image"),
            "invalidate runtime",
        ),
        "bad_adapter_image" => io(
            fs::write(root.join(ADAPTER), b"not a PE image"),
            "invalidate adapter",
        ),
        _ => (),
    }
    if mode == "hardlink_alias" {
        io(
            fs::hard_link(root.join(RUNTIME), root.join(ADAPTER)),
            "create hardlink alias",
        );
    }

    let events = root.join("events.txt");
    let observed = || fs::read(&events).unwrap_or_default();
    let read_diagnostic = || {
        io(
            fs::read_to_string(root.join("ReScaleFrame.Loader.log")),
            "startup diagnostic persisted",
        )
    };
    let thread_mode = unsafe { GetThreadErrorMode() };
    let process_mode = unsafe { GetErrorMode() };
    let mut output = null_mut();
    let pointer = (&raw mut output).expose_provenance();
    environment(w!("RSF_THIN_FIXTURE_EVENTS"), Some(events.as_os_str()));
    environment(
        w!("RSF_THIN_FIXTURE_CONFIG"),
        Some(final_config.as_os_str()),
    );
    environment(
        w!("RSF_THIN_FIXTURE_OUTPUT"),
        Some(OsStr::new(&format!("{pointer:x}"))),
    );
    environment(
        w!("RSF_THIN_FIXTURE_CHECK_ERROR_MODE"),
        (mode == "scoped_modes").then_some(OsStr::new("1")),
    );
    environment(
        w!("RSF_THIN_FIXTURE_REFUSE"),
        (mode == "refuse").then_some(OsStr::new("1")),
    );
    let thread_id = unsafe { GetCurrentThreadId() }.to_string();
    environment(
        w!("RSF_THIN_FIXTURE_THREAD"),
        (mode != "concurrent").then_some(OsStr::new(&thread_id)),
    );
    environment(
        w!("RSF_THIN_FIXTURE_THREAD_MODE"),
        Some(OsStr::new(&thread_mode.to_string())),
    );
    environment(
        w!("RSF_THIN_FIXTURE_PROCESS_MODE"),
        Some(OsStr::new(&process_mode.to_string())),
    );

    let carrier = load(root.join("dinput8.dll"), 0);
    require(!carrier.is_null(), "load copied carrier");
    unsafe { Sleep(120) };
    require(
        observed().is_empty(),
        "DllMain never starts configured modules or a background worker",
    );
    let can_unload =
        unsafe { std::mem::transmute::<_, Status>(address(carrier, c"DllCanUnloadNow")) };
    unsafe { can_unload() };
    require(
        observed().is_empty(),
        "non-create export never starts modules",
    );
    let create = create_proc(carrier);
    let create_address = format!("{:x}", create as usize);
    environment(
        w!("RSF_THIN_FIXTURE_REENTER"),
        (mode == "reentrant").then_some(OsStr::new(&create_address)),
    );

    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        let _watch = watch(scope, &stop);
        if mode == "concurrent" {
            let gate = Barrier::new(17);
            thread::scope(|scope| {
                for _ in 0..16 {
                    scope.spawn(|| {
                        gate.wait();
                        let mut object = null_mut();
                        unsafe { create_object(create, &raw mut object) };
                        require(
                            observed() == b"RAE",
                            "every concurrent return waits for adapter completion",
                        );
                        unsafe { release(object) };
                    });
                }
                gate.wait();
            });
        }
        unsafe { create_object(create, &raw mut output) };
        require(
            observed() == expected,
            "configured runtime and entry complete before the real create",
        );
        require(
            unsafe { GetThreadErrorMode() == thread_mode && GetErrorMode() == process_mode },
            "startup restores caller thread error mode and preserves process error mode",
        );
        unsafe { release(output) };
        if expected.is_empty() {
            io(fs::write(&config, &valid), "repair rejected config");
        }
        output = null_mut();
        unsafe { create_object(create, &raw mut output) };
        unsafe { release(output) };
        require(
            observed() == expected,
            "startup success or refusal is final exactly once",
        );

        let (selected, decoy) = if mode == "default" {
            (w!("ReScaleFrame.Runtime.dll"), w!("RuntimeOne.dll"))
        } else {
            (w!("RuntimeOne.dll"), w!("ReScaleFrame.Runtime.dll"))
        };
        let check_retention = || unsafe {
            require(
                GetModuleHandleW(selected).is_null() != expected.contains(&b'R')
                    && GetModuleHandleW(w!("AdapterOne.dll")).is_null() != expected.contains(&b'A'),
                "loaded configured modules stay mapped after refusal and carrier release",
            );
            require(
                GetModuleHandleW(decoy).is_null(),
                "no fallback runtime or double load",
            );
        };
        check_retention();
        let diagnostic = read_diagnostic();
        require(!diagnostic.is_empty(), "startup diagnostic persisted");
        let expected_diagnostic = match mode {
            "refuse" => Some("entry refused status=7"),
            "missing_export" => Some("entry export missing"),
            "bad_runtime_image" | "bad_adapter_image" => Some("load failed win32=193"),
            "reentrant" => Some("entry ready status=0"),
            _ => None,
        };
        if let Some(message) = expected_diagnostic {
            require(diagnostic.contains(message), message);
        }
        if expected.is_empty() {
            require(
                !diagnostic.contains("entry ready"),
                "refusal is never logged as successful startup",
            );
        }
        unsafe { FreeLibrary(carrier) };
        check_retention();
    });
}
