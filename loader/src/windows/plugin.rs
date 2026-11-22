// SPDX-License-Identifier: MIT
use super::{load_library, module_path, null_mut, spawn_startup, wide};
use rsf_game_sys::rsf_game_cpu_event;
use rsf_game_sys::rsf_plugin_session_options;
use rsf_game_sys::session::carrier_log::CarrierLog;
use rsf_game_sys::session::{Probe, host_services, with_session_options};
use std::ffi::{CStr, CString, c_char, c_void};
use std::fs::create_dir;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::core::BOOL;

type CpuSink = unsafe extern "C" fn(*const rsf_game_cpu_event) -> i32;
type Prepare = unsafe extern "C" fn(*const rsf_plugin_session_options, *mut *mut c_void) -> i32;
type Start = unsafe extern "C" fn(*mut c_void) -> i32;

static LOG: CarrierLog = CarrierLog::new();
static CPU_SINK: OnceLock<CpuSink> = OnceLock::new();
static CPU_SAMPLES: AtomicU32 = AtomicU32::new(0);

fn note(message: &[u8]) {
    LOG.write(unsafe { GetTickCount64() }, message);
}

unsafe extern "C" fn log(_: *mut c_void, message: *const c_char) {
    unsafe { LOG.write_c_message(GetTickCount64(), message) };
}

unsafe extern "C" fn cpu_event(_: *mut c_void, event: *const rsf_game_cpu_event) {
    let accepted = CPU_SINK.get().map_or(0, |sink| unsafe { sink(event) });
    if let Some(event) = unsafe { event.as_ref() }
        && CPU_SAMPLES.fetch_add(1, Ordering::Relaxed) < 32
    {
        note(
            format!(
                "Runtime CPU fact stage={} source={} thread={} accepted={} flags=0x{:x}",
                event.stage,
                event.source_frame_id,
                unsafe { GetCurrentThreadId() },
                accepted,
                event.flags,
            )
            .as_bytes(),
        );
    }
}
fn configure(module: HMODULE) -> Option<u32> {
    let path = module_path(module)?;
    let directory = path.parent()?.join("ReScaleFrame");
    let _ = create_dir(&directory);
    LOG.set_path(&wide(directory.join("loader.log")));
    let runtime_path = wide(directory.join("ReScaleFrame.Runtime.dll"));
    let plugin_path = wide(directory.join("ReScaleFrame.Entry.dll"));
    let runtime = load_library(
        runtime_path.as_ptr(),
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    );
    if runtime.is_null() {
        note(b"Runtime module load refused.");
        return None;
    }
    let resolve = |name: &CStr| unsafe { GetProcAddress(runtime, name.as_ptr().cast()) };
    let (Some(prepare), Some(start)) = (
        resolve(c"rsf_plugin_session_prepare"),
        resolve(c"rsf_plugin_session_start"),
    ) else {
        note(b"Runtime session ABI unavailable.");
        return None;
    };
    let prepare =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Prepare>(prepare) };
    let start =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Start>(start) };
    if let Some(sink) = resolve(c"rsf_native_cpu_event") {
        let _ = CPU_SINK.set(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, CpuSink>(sink)
        });
    }
    let executable = module_path(null_mut())?;
    let name = CString::new(executable.file_name()?.to_string_lossy().as_bytes()).ok()?;
    if name.as_bytes_with_nul().len() > 1024 {
        return None;
    }
    let hash = rsf_windows::sha256_file_hex(&executable)?;
    let hash = CStr::from_bytes_with_nul(&hash).ok()?;
    let probe = Probe {
        machine: 0x8664,
        executable: &name,
        sha256: hash,
    };
    let mut services = host_services(unsafe { GetTickCount64() }, Some(log));
    services.cpu_event = CPU_SINK.get().map(|_| cpu_event as _);
    let mut session = null_mut();
    let prepared = with_session_options(&plugin_path, probe, &services, |options| unsafe {
        prepare(options, &mut session)
    }) as u32;
    note(format!("Plugin prepare result={prepared}").as_bytes());
    if prepared != 0 {
        return Some(prepared);
    }
    let started = unsafe { start(session) } as u32;
    note(format!("Plugin start result={started}").as_bytes());
    // The runtime and carrier callbacks outlive the worker.
    Some(started)
}

unsafe extern "system" fn startup(module: *mut c_void) -> u32 {
    std::panic::catch_unwind(|| configure(module))
        .ok()
        .flatten()
        .unwrap_or(1)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_plugin_carrier_attach(instance: HMODULE, event: u32) -> BOOL {
    if event == DLL_PROCESS_ATTACH {
        spawn_startup(instance, Some(startup));
    }
    1
}
