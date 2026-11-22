// SPDX-License-Identifier: MIT
use super::{load_library, module_path, null_mut, spawn_startup, wide};
use rsf_game_sys::plugin_session_abi::{
    RSF_PLUGIN_SESSION_ABI_VERSION, rsf_plugin_session_options,
};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_game_cpu_event, rsf_game_host_services, rsf_game_probe,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::fs::{OpenOptions, create_dir};
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider,
    BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData,
    BCryptOpenAlgorithmProvider,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::core::BOOL;

type CpuSink = unsafe extern "C" fn(*const rsf_game_cpu_event) -> i32;
type Prepare = unsafe extern "C" fn(*const rsf_plugin_session_options, *mut *mut c_void) -> i32;
type Start = unsafe extern "C" fn(*mut c_void) -> i32;

static LOG: OnceLock<Mutex<PathBuf>> = OnceLock::new();
static CPU_SINK: OnceLock<CpuSink> = OnceLock::new();
static CPU_SAMPLES: AtomicU32 = AtomicU32::new(0);

fn note(message: &[u8]) {
    let Some(path) = LOG.get().and_then(|path| path.lock().ok()) else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_path())
    {
        let _ = file.write_all(message);
        let _ = file.write_all(b"\n");
    }
}

unsafe extern "C" fn log(_: *mut c_void, message: *const c_char) {
    let message = if message.is_null() {
        b""
    } else {
        unsafe { CStr::from_ptr(message) }.to_bytes()
    };
    note(message);
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

struct Algorithm(BCRYPT_ALG_HANDLE);
impl Drop for Algorithm {
    fn drop(&mut self) {
        unsafe { BCryptCloseAlgorithmProvider(self.0, 0) };
    }
}

struct Hash(BCRYPT_HASH_HANDLE);
impl Drop for Hash {
    fn drop(&mut self) {
        unsafe { BCryptDestroyHash(self.0) };
    }
}

fn digest(path: &Path) -> Option<[u8; 65]> {
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
        .ok()?;
    let mut algorithm = null_mut();
    if unsafe {
        BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, std::ptr::null(), 0)
    } < 0
    {
        return None;
    }
    let algorithm = Algorithm(algorithm);
    let mut hash = null_mut();
    if unsafe {
        BCryptCreateHash(
            algorithm.0,
            &mut hash,
            null_mut(),
            0,
            std::ptr::null(),
            0,
            0,
        )
    } < 0
    {
        return None;
    }
    let hash = Hash(hash);
    let mut buffer = vec![0; 65536];
    loop {
        match file.read(&mut buffer).ok()? {
            0 => break,
            count if unsafe { BCryptHashData(hash.0, buffer.as_ptr(), count as u32, 0) } >= 0 => (),
            _ => return None,
        }
    }
    let mut bytes = [0; 32];
    if unsafe { BCryptFinishHash(hash.0, bytes.as_mut_ptr(), bytes.len() as u32, 0) } < 0 {
        return None;
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = [0; 65];
    for (digits, byte) in result.as_chunks_mut::<2>().0.iter_mut().zip(bytes) {
        *digits = [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]];
    }
    Some(result)
}

fn configure(module: HMODULE) -> Option<u32> {
    let path = module_path(module)?;
    let directory = path.parent()?.join("ReScaleFrame");
    let _ = create_dir(&directory);
    let _ = LOG.set(Mutex::new(directory.join("loader.log")));
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
    let hash = digest(&executable)?;
    let probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: 0x8664,
        reserved: 0,
        executable_name_utf8: name.as_ptr(),
        sha256_hex: hash.as_ptr().cast(),
    };
    let options = rsf_plugin_session_options {
        struct_size: size_of::<rsf_plugin_session_options>() as u32,
        abi_version: RSF_PLUGIN_SESSION_ABI_VERSION,
        plugin_path: plugin_path.as_ptr(),
        probe: &probe,
        services: rsf_game_host_services {
            struct_size: size_of::<rsf_game_host_services>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            session_id: 1,
            log: Some(log),
            render_pass: None,
            user: null_mut(),
            render_config: None,
            cpu_event: CPU_SINK.get().map(|_| cpu_event as _),
            scope_state: None,
            process_upscale: None,
            submit: None,
            present: None,
            process_latency: None,
            render_feedback: None,
        },
    };
    let mut session = null_mut();
    let prepared = unsafe { prepare(&options, &mut session) } as u32;
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
