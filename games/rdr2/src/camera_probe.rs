// SPDX-License-Identifier: MIT
//! Opt-in camera capture at the native upscaler boundary and its separate fiber reference.
use crate::asi::{self, AsiPlatform, DiagnosticHook, DiagnosticInstall, install_diagnostic_hooks};
use crate::layout::{self, FRAME_COUNTER, TLS_INDEX, block, camera_probe};
use crate::platform::lock;
use crate::script_input::ScriptHook;
use std::ffi::{OsString, c_void};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
use windows_sys::Win32::System::Performance::QueryPerformanceCounter;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};

const CAPTURE_FRAMES: u32 = 64;
const PATH_LIMIT: usize = 260 - 64;
const CAMERA_POSITION: u64 = 0x5352_e025_ec2b_416f;
const CAMERA_ROTATION: u64 = 0x6026_85bd_85dd_26ca;
const CAMERA_FOV: u64 = 0x04af_7797_1e50_8f6a;

struct Probe {
    directory: PathBuf,
    request: PathBuf,
    image: usize,
    installed: bool,
    next_poll: u64,
    remaining: u32,
    capture: u32,
    index: u32,
}

impl Probe {
    fn request(&mut self) {
        if self.installed && self.remaining == 0 {
            self.remaining = CAPTURE_FRAMES;
            self.capture = self.capture.wrapping_add(1);
            self.index = 0;
        }
    }
}

static PROBE: Mutex<Option<Probe>> = Mutex::new(None);
static REFERENCE: OnceLock<ScriptHook> = OnceLock::new();

// This is the existing RSFCAM1 version 2 binary record. All fields are plain scalars/arrays.
#[repr(C)]
struct Snapshot {
    magic: [u8; 8],
    version: u32,
    size: u32,
    capture: u32,
    index: u32,
    provider: u32,
    thread: u32,
    tick: u64,
    qpc: u64,
    image_base: u64,
    tls_block: u64,
    view: u64,
    context: u64,
    parameters: u64,
    return_rva: u64,
    frame_counter: u32,
    tls_index: u32,
    valid_view: u32,
    valid_parameters: u32,
    view_bytes: [u8; 0x800],
    parameter_bytes: [u8; 0x100],
    render_width: u32,
    render_height: u32,
    job: u64,
    draw_context: u64,
    job_bytes: [u8; 0x80],
}

const _: () = {
    assert!(size_of::<Snapshot>() == 2568);
    assert!(std::mem::offset_of!(Snapshot, tick) == 32);
    assert!(std::mem::offset_of!(Snapshot, view_bytes) == 112);
    assert!(std::mem::offset_of!(Snapshot, job_bytes) == 2440);
};

fn timestamp() -> u64 {
    let mut qpc = 0;
    unsafe { QueryPerformanceCounter(&mut qpc) };
    qpc as u64
}

fn read_bytes(platform: &AsiPlatform, address: usize, bytes: &mut [u8]) -> bool {
    unsafe { (platform.read)(address, bytes.as_mut_ptr().cast(), bytes.len()) }
}

impl Snapshot {
    fn read_worker(
        &mut self,
        platform: &AsiPlatform,
        image: usize,
        thread_slots: unsafe extern "C" fn() -> usize,
    ) {
        self.frame_counter = platform.read_value(image + FRAME_COUNTER).unwrap_or(0);
        self.tls_index = platform.read_value(image + TLS_INDEX).unwrap_or(0);
        let slots = unsafe { thread_slots() };
        if self.tls_index >= block::TLS_LIMIT {
            return;
        }
        let offset = self.tls_index as usize * size_of::<usize>();
        let Some(block) = platform
            .read_value::<usize>(slots.wrapping_add(offset))
            .filter(|&block| block != 0)
        else {
            return;
        };
        self.tls_block = block as u64;
        self.view = platform
            .read_value(block.wrapping_add(block::VIEW))
            .unwrap_or(0);
        self.draw_context = platform
            .read_value(block.wrapping_add(block::DRAW))
            .unwrap_or(0);
        self.job = platform
            .read_value(block.wrapping_add(camera_probe::JOB))
            .unwrap_or(0);
        read_bytes(platform, self.job as usize, &mut self.job_bytes);
        self.valid_view = u32::from(read_bytes(
            platform,
            self.view as usize,
            &mut self.view_bytes,
        ));
    }

    fn bytes(&self) -> &[u8] {
        // Every field is initialized and the asserted layout has no padding. The supported
        // Windows x64 adapter writes the same little-endian record as the former native struct.
        unsafe { std::slice::from_raw_parts(std::ptr::from_ref(self).cast(), size_of::<Self>()) }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_camera_install(
    directory: *const u16,
    hooks: *const [DiagnosticHook; 2],
    enable: unsafe extern "C" fn(usize) -> i32,
) -> bool {
    let (Some(platform), Some(hooks)) = (asi::platform(), unsafe { hooks.as_ref() }) else {
        return false;
    };
    if directory.is_null() {
        return false;
    }
    let Some(length) = (0..=PATH_LIMIT).find(|&index| unsafe { directory.add(index).read() } == 0)
    else {
        return false;
    };
    let mut probe = lock(&PROBE);
    if length == 0 || probe.is_some() {
        return false;
    }
    let image = unsafe { (platform.main_module)() };
    let Some((_, signature, timestamp, size)) = asi::pe_header(platform, image) else {
        return false;
    };
    if signature != asi::PE_SIGNATURE
        || timestamp != layout::IMAGE_TIMESTAMP
        || size != layout::IMAGE_SIZE
    {
        return false;
    }
    let installed = match install_diagnostic_hooks(
        platform,
        image,
        [camera_probe::FSR, camera_probe::DLSS],
        hooks,
        enable,
    ) {
        DiagnosticInstall::Installed => true,
        DiagnosticInstall::Refused => return false,
        DiagnosticInstall::Retained => false,
    };
    let directory = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(directory, length)
    }));
    *probe = Some(Probe {
        request: directory.join("camera.request"),
        directory,
        image,
        installed,
        next_poll: 0,
        remaining: 0,
        capture: 0,
        index: 0,
    });
    installed
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_camera_request() {
    if let Some(probe) = lock(&PROBE).as_mut() {
        probe.request();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_camera_capture(
    provider: u32,
    context: *const c_void,
    parameters: *const c_void,
    caller: *const c_void,
    thread_slots: unsafe extern "C" fn() -> usize,
) {
    let Some(platform) = asi::platform() else {
        return;
    };
    let mut probe = lock(&PROBE);
    let Some(probe) = probe.as_mut().filter(|probe| probe.installed) else {
        return;
    };
    let now = unsafe { GetTickCount64() };
    if probe.remaining == 0 && now >= probe.next_poll {
        probe.next_poll = now.wrapping_add(500);
        if fs::remove_file(&probe.request).is_ok() {
            probe.request();
        }
    }
    if probe.remaining == 0 {
        return;
    }
    // The lock orders binary files across both hooks. No live game pointer survives this call.
    let mut record: Snapshot = unsafe { std::mem::zeroed() };
    record.magic = *b"RSFCAM1\0";
    record.version = 2;
    record.size = size_of::<Snapshot>() as u32;
    record.capture = probe.capture;
    record.index = probe.index;
    record.provider = provider;
    record.thread = unsafe { GetCurrentThreadId() };
    record.tick = now;
    record.qpc = timestamp();
    record.image_base = probe.image as u64;
    record.context = context as u64;
    record.parameters = parameters as u64;
    record.return_rva = (caller as u64).wrapping_sub(record.image_base);
    record.read_worker(platform, probe.image, thread_slots);
    record.valid_parameters = u32::from(read_bytes(
        platform,
        parameters as usize,
        &mut record.parameter_bytes,
    ));
    if let Some(colour) = platform
        .read_value::<usize>(parameters as usize)
        .filter(|&colour| colour != 0)
        && let Some([width, height]) =
            platform.read_value::<[u16; 2]>(colour.wrapping_add(camera_probe::COLOUR_EXTENT))
    {
        record.render_width = u32::from(width);
        record.render_height = u32::from(height);
    }
    let process = unsafe { GetCurrentProcessId() };
    let output = probe.directory.join(format!(
        "camera-{process}-{:03}-{:03}.bin",
        record.capture, record.index
    ));
    let _ = fs::write(output, record.bytes());
    probe.index += 1;
    probe.remaining -= 1;
}

fn vector(script: &ScriptHook, hash: u64, arguments: &[u64]) -> Option<[f32; 3]> {
    let slots = unsafe { script.native(hash, arguments) };
    if slots.is_null() {
        return None;
    }
    // The ScriptHook vector ABI uses one 8-byte slot for each float.
    let values =
        std::array::from_fn(|index| f32::from_bits(unsafe { slots.add(index).read() } as u32));
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

fn reference_sample(script: &ScriptHook) -> Option<([f32; 3], [f32; 3], f32)> {
    let position = vector(script, CAMERA_POSITION, &[])?;
    let rotation = vector(script, CAMERA_ROTATION, &[2])?;
    let value = unsafe { script.native(CAMERA_FOV, &[]).as_ref() }?;
    let fov = f32::from_bits(*value as u32);
    fov.is_finite().then_some((position, rotation, fov))
}

extern "C" fn script_reference() {
    let (Some(script), Some(platform)) = (REFERENCE.get(), asi::platform()) else {
        return;
    };
    loop {
        let capture = lock(&PROBE)
            .as_ref()
            .filter(|probe| probe.remaining != 0)
            .map(|probe| (probe.directory.clone(), probe.image, probe.capture));
        if let Some((directory, image, capture)) = capture
            && let Some(([px, py, pz], [rx, ry, rz], fov)) = reference_sample(script)
        {
            let qpc = timestamp();
            let frame = platform
                .read_value::<u32>(image + FRAME_COUNTER)
                .unwrap_or(0);
            let thread = unsafe { GetCurrentThreadId() };
            let process = unsafe { GetCurrentProcessId() };
            let path = directory.join(format!("script-{process}-{capture:03}.jsonl"));
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(
                    file,
                    "{{\"frame\":{frame},\"qpc\":{qpc},\"thread\":{thread},\"position\":[{px},{py},{pz}],\"rotation_order_2\":[{rx},{ry},{rz}],\"fov_degrees\":{fov}}}"
                );
            }
        }
        unsafe { script.wait_frame() };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_camera_reference(instance: HMODULE, library: *const u16) -> bool {
    if instance.is_null()
        || library.is_null()
        || !lock(&PROBE).as_ref().is_some_and(|probe| probe.installed)
    {
        return false;
    }
    let module = unsafe { LoadLibraryW(library) };
    if module.is_null() {
        return false;
    }
    let Some(script) = ScriptHook::resolve(module) else {
        return false;
    };
    if REFERENCE.set(script).is_err() {
        return false;
    }
    unsafe { script.register(instance, script_reference) };
    true
}
