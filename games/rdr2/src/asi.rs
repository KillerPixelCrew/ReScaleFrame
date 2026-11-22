// SPDX-License-Identifier: MIT
//! `ReScaleFrame.RDR2.asi` admission under the loader lock: the process identity, the
//! `ReScaleFrame.ini` `[RDR2]` gates, the exact-build ForceD3D12 parameter write, and the order in
//! which the engine-settings owner and the ASI's native diagnostic owners are armed. The ASI loader
//! loads the module during CRT startup, after decryption and before the engine reads its settings.
//!
//! Platform callbacks also allow startup policy to be exercised in synthetic fixtures.
#[cfg(all(windows, not(test)))]
#[path = "asi/windows.rs"]
mod windows;
#[cfg(all(windows, feature = "capture-fixture", not(test)))]
pub(crate) use windows::{RSF_CENSUS_ABI_VERSION, rsf_census_options};

use crate::engine_settings;
use crate::layout::{self, asi};
use std::borrow::Cow;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

pub(crate) const ASI_PLATFORM_VERSION: u32 = 1;
const MAX_PATH: u32 = 260;
const LONG_PATH: u32 = 32768;
pub(crate) const MH_OK: i32 = 0;
pub(crate) const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;

#[repr(C)]
pub(crate) struct DiagnosticHook {
    detour: *const c_void,
    original: *mut *mut c_void,
}

#[cfg(windows)]
impl DiagnosticHook {
    pub(crate) const fn new(detour: *const c_void, original: *mut *mut c_void) -> Self {
        Self { detour, original }
    }
}

const _: () = assert!(size_of::<DiagnosticHook>() == 2 * size_of::<usize>());

pub(crate) enum DiagnosticInstall {
    Installed,
    Refused,
    Retained,
}

/// Shared startup transaction for the ASI's two-site diagnostic probes.
pub(crate) fn install_diagnostic_hooks(
    platform: &AsiPlatform,
    image: usize,
    sites: [layout::Site; 2],
    hooks: &[DiagnosticHook; 2],
    enable: unsafe extern "C" fn(usize) -> i32,
) -> DiagnosticInstall {
    if image == 0
        || !sites.iter().all(|site| {
            platform.read_value::<[u8; 16]>(image.wrapping_add(site.rva)) == Some(site.expected)
        })
    {
        return DiagnosticInstall::Refused;
    }
    let initialized = unsafe { (platform.hook_initialize)() };
    if initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED {
        return DiagnosticInstall::Refused;
    }
    let targets = sites.map(|site| image + site.rva);
    for (index, (&target, hook)) in targets.iter().zip(hooks).enumerate() {
        if unsafe { (platform.hook_create)(target, hook.detour, hook.original) } != MH_OK {
            let removed = targets[..index]
                .iter()
                .all(|&created| unsafe { (platform.hook_remove)(created) } == MH_OK);
            return if removed {
                DiagnosticInstall::Refused
            } else {
                DiagnosticInstall::Retained
            };
        }
    }
    if targets
        .iter()
        .all(|&target| unsafe { enable(target) } == MH_OK)
    {
        return DiagnosticInstall::Installed;
    }
    // A partially enabled set can leave a callback on the stack. Disable both sites and keep
    // their trampolines until process exit.
    for target in targets {
        unsafe { (platform.hook_disable)(target) };
    }
    DiagnosticInstall::Retained
}

/// Process-lived Win32, native engine and diagnostic callbacks for ASI admission.
#[repr(C)]
pub(crate) struct AsiPlatform {
    _struct_size: u32,
    _version: u32,
    // Process, image and memory. `read` is a non-faulting ReadProcessMemory copy.
    pub(crate) main_module: unsafe extern "C" fn() -> usize,
    pub(crate) read: unsafe extern "C" fn(address: usize, out: *mut c_void, size: usize) -> bool,
    /// GetModuleFileNameW of the executable and of this ASI.
    pub(crate) executable_path: unsafe extern "C" fn(out: *mut u16, capacity: u32) -> u32,
    pub(crate) module_path: unsafe extern "C" fn(out: *mut u16, capacity: u32) -> u32,
    pub(crate) current_thread: unsafe extern "C" fn() -> u32,
    // GetPrivateProfileIntW and GetPrivateProfileStringW.
    pub(crate) profile_int: unsafe extern "C" fn(
        path: *const u16,
        section: *const u16,
        key: *const u16,
        fallback: i32,
    ) -> u32,
    pub(crate) profile_string: unsafe extern "C" fn(
        path: *const u16,
        section: *const u16,
        key: *const u16,
        out: *mut u16,
        capacity: u32,
    ) -> u32,
    // This module's MinHook instance, shared with the census and the diagnostic owners. Raw
    // MH_STATUS values.
    pub(crate) hook_initialize: unsafe extern "C" fn() -> i32,
    pub(crate) hook_create: unsafe extern "C" fn(
        target: usize,
        detour: *const c_void,
        original: *mut *mut c_void,
    ) -> i32,
    pub(crate) hook_queue_enable: unsafe extern "C" fn(target: usize) -> i32,
    pub(crate) hook_apply_queued: unsafe extern "C" fn() -> i32,
    pub(crate) hook_disable: unsafe extern "C" fn(target: usize) -> i32,
    pub(crate) hook_remove: unsafe extern "C" fn(target: usize) -> i32,
    pub(crate) hook_status: unsafe extern "C" fn(status: i32) -> *const c_char,
    // DXGI adapter vendor by enumeration index (0 when unavailable) and the desktop refresh rate.
    pub(crate) adapter_vendor: unsafe extern "C" fn(index: u32) -> u32,
    pub(crate) desktop_refresh: unsafe extern "C" fn() -> u32,
    // Calls into engine functions whose bytes this crate verified.
    pub(crate) engine_manager: unsafe extern "C" fn(function: usize) -> *mut c_void,
    pub(crate) engine_owner:
        unsafe extern "C" fn(function: usize, manager: *mut c_void) -> *mut c_void,
    pub(crate) engine_find: unsafe extern "C" fn(
        function: usize,
        manager: *mut c_void,
        ids: *const u32,
        count: u64,
        scratch: *mut c_void,
    ) -> *mut c_void,
    // Native diagnostic owners of the ASI. `frame_probe` is null without RenderDoc support.
    pub(crate) d3d12_trace:
        unsafe extern "C" fn(target: usize, image_size: u32, path: *const u16) -> i32,
    pub(crate) native_capture: unsafe extern "C" fn(directory: *const u16) -> bool,
    pub(crate) frame_probe:
        Option<unsafe extern "C" fn(library: *const u16, prefix: *const u16) -> i32>,
    pub(crate) census: unsafe extern "C" fn(directory: *const u16, flags: u32) -> i32,
    pub(crate) camera_probe: unsafe extern "C" fn(directory: *const u16) -> bool,
    pub(crate) camera_reference: unsafe extern "C" fn(library: *const u16) -> bool,
    pub(crate) social_club: unsafe extern "C" fn(trace: bool) -> bool,
    /// Starts the runtime session worker thread.
    pub(crate) start_worker: unsafe extern "C" fn(),
}

const _: () = {
    assert!(size_of::<AsiPlatform>() == 8 + 27 * size_of::<usize>());
    assert!(std::mem::offset_of!(AsiPlatform, hook_initialize) == 8 + 7 * 8);
    assert!(std::mem::offset_of!(AsiPlatform, adapter_vendor) == 8 + 14 * 8);
    assert!(std::mem::offset_of!(AsiPlatform, d3d12_trace) == 8 + 19 * 8);
    assert!(std::mem::offset_of!(AsiPlatform, start_worker) == 8 + 26 * 8);
};

// The native table is a constant with static storage in the ASI.
unsafe impl Sync for AsiPlatform {}

static PLATFORM: AtomicPtr<AsiPlatform> = AtomicPtr::new(ptr::null_mut());

fn register(platform: *const AsiPlatform) -> bool {
    if platform.is_null() {
        return false;
    }
    // Only the leading header is readable from a short or foreign table.
    let header = platform.cast::<u32>();
    let size = unsafe { header.read() };
    let accepted = size as usize == size_of::<AsiPlatform>()
        && unsafe { header.add(1).read() } == ASI_PLATFORM_VERSION;
    if accepted {
        PLATFORM.store(platform.cast_mut(), Ordering::Release);
    }
    accepted
}

pub(crate) fn platform() -> Option<&'static AsiPlatform> {
    // A registered table lives in the ASI's constant data; the ASI is never unloaded.
    unsafe { PLATFORM.load(Ordering::Acquire).as_ref() }
}

impl AsiPlatform {
    /// Copies a plain-data value. `T` must be valid for every bit pattern.
    pub(crate) fn read_value<T: Copy + Default>(&self, address: usize) -> Option<T> {
        let mut value = T::default();
        let read = address != 0
            && unsafe { (self.read)(address, (&raw mut value).cast(), size_of::<T>()) };
        read.then_some(value)
    }

    pub(crate) fn matches(&self, address: usize, expected: &[u8]) -> bool {
        let mut bytes = vec![0; expected.len()];
        address != 0
            && unsafe { (self.read)(address, bytes.as_mut_ptr().cast(), bytes.len()) }
            && bytes == expected
    }

    pub(crate) fn status_text(&self, status: i32) -> Cow<'static, str> {
        let text = unsafe { (self.hook_status)(status) };
        if text.is_null() {
            return Cow::Owned(format!("MH_STATUS {status}"));
        }
        // MinHook status names are static strings.
        Cow::Owned(
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// Startup results, announced by the worker outside the loader lock in this order.
struct Results {
    force: Cow<'static, str>,
    trace: Cow<'static, str>,
    social_club: Cow<'static, str>,
    camera: Cow<'static, str>,
    frame: Cow<'static, str>,
    census: Cow<'static, str>,
}

static RESULTS: Mutex<Results> = Mutex::new(Results {
    force: Cow::Borrowed("ForceD3D12 disabled."),
    trace: Cow::Borrowed("D3D12 result trace disabled."),
    social_club: Cow::Borrowed("Social Club backbuffer fix disabled."),
    camera: Cow::Borrowed("Camera probe disabled."),
    frame: Cow::Borrowed("Frame probe disabled."),
    census: Cow::Borrowed("Vendor census disabled."),
});

fn record(update: impl FnOnce(&mut Results)) {
    update(&mut crate::platform::lock(&RESULTS));
}

/// NUL-terminated UTF-16 for the leaves.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn terminated(text: &[u16]) -> Vec<u16> {
    text.iter().copied().chain(std::iter::once(0)).collect()
}

/// True when the executable path names the supported game, ignoring ASCII case.
pub(crate) fn is_game(executable: &[u16]) -> bool {
    let start = executable
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\'))
        .map_or(0, |separator| separator + 1);
    String::from_utf16(&executable[start..])
        .is_ok_and(|name| name.as_bytes().eq_ignore_ascii_case(layout::EXECUTABLE))
}

/// The deployment root (the ASI's directory, with its trailing separator) and the
/// `ReScaleFrame.ini` beside it.
pub(crate) fn deployment(module: &[u16]) -> Option<(Vec<u16>, Vec<u16>)> {
    let separator = module.iter().rposition(|&unit| unit == u16::from(b'\\'))?;
    let root = module[..=separator].to_vec();
    let mut configuration = root.clone();
    configuration.extend("ReScaleFrame.ini".encode_utf16());
    configuration.push(0);
    (configuration.len() <= LONG_PATH as usize).then_some((root, configuration))
}

/// `site_rva + 7 + disp32` of a RIP-relative `mov r64, [rip + disp32]` names the parameter.
pub(crate) fn reads_parameter(site_rva: u32, opcode: [u8; 3], bytes: [u8; 7]) -> bool {
    let displacement = u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]);
    bytes[..3] == opcode && site_rva.wrapping_add(7).wrapping_add(displacement) == asi::API_OVERRIDE
}

struct Configuration<'a> {
    platform: &'a AsiPlatform,
    section: Vec<u16>,
    path: Vec<u16>,
}

impl Configuration<'_> {
    fn int(&self, key: &str, fallback: i32) -> u32 {
        let key = wide(key);
        unsafe {
            (self.platform.profile_int)(
                self.path.as_ptr(),
                self.section.as_ptr(),
                key.as_ptr(),
                fallback,
            )
        }
    }

    fn enabled(&self, key: &str) -> bool {
        self.int(key, 0) != 0
    }

    /// The value without its terminator, read into a buffer of `capacity` units.
    fn string(&self, key: &str, capacity: u32) -> Vec<u16> {
        let key = wide(key);
        let read_capacity = rsf_game_sys::profile_text::read_capacity(capacity as usize);
        let mut value = vec![0; read_capacity];
        let length = unsafe {
            (self.platform.profile_string)(
                self.path.as_ptr(),
                self.section.as_ptr(),
                key.as_ptr(),
                value.as_mut_ptr(),
                read_capacity as u32,
            )
        };
        value.truncate((length as usize).min(read_capacity.saturating_sub(1)));
        let mut value = rsf_game_sys::profile_text::decode(value);
        value.truncate(capacity.saturating_sub(1) as usize);
        value
    }
}

/// A module path, or `None` when it is unavailable or, unless `truncated` is accepted, cut short.
fn read_path(
    read: unsafe extern "C" fn(*mut u16, u32) -> u32,
    truncated: bool,
) -> Option<Vec<u16>> {
    let mut path = vec![0; LONG_PATH as usize];
    let length = unsafe { read(path.as_mut_ptr(), LONG_PATH) } as usize;
    // GetModuleFileNameW returns the capacity and a terminated, truncated path when it is short.
    (length != 0 && (truncated || length < LONG_PATH as usize)).then(|| {
        path.truncate(length.min(LONG_PATH as usize - 1));
        path
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Force {
    Applied,
    Refused(String),
}

pub(crate) fn pe_header(platform: &AsiPlatform, base: usize) -> Option<(usize, u32, u32, u32)> {
    let (nt, facts) = crate::platform::image_header(platform.read, base)?;
    Some((nt, PE_SIGNATURE, facts.timestamp, facts.image_size))
}

pub(crate) use crate::platform::PE_SIGNATURE;

/// Writes "d3d12" to the engine's API parameter when the image, the parameter's section and every
/// read site match the supported build and nothing else has set it.
fn force_d3d12(platform: &AsiPlatform) -> Force {
    let base = unsafe { (platform.main_module)() };
    let Some((nt, signature, timestamp, image_size)) = pe_header(platform, base) else {
        return Force::Refused("ForceD3D12 refused: no main image.".into());
    };
    if signature != PE_SIGNATURE
        || timestamp != layout::IMAGE_TIMESTAMP
        || image_size != layout::IMAGE_SIZE
    {
        return Force::Refused(format!(
            "ForceD3D12 refused: build differs (timestamp 0x{timestamp:x}, image 0x{image_size:x})."
        ));
    }
    // IMAGE_FIRST_SECTION: NumberOfSections at +6, SizeOfOptionalHeader at +0x14.
    let sections = platform.read_value::<u16>(nt + 6).unwrap_or(0);
    let optional = platform.read_value::<u16>(nt + 0x14).unwrap_or(0);
    let first = nt + 0x18 + usize::from(optional);
    let parameter = u64::from(asi::API_OVERRIDE);
    let in_data = (0..usize::from(sections)).any(|index| {
        let header = first + index * 40;
        let name = platform.read_value::<[u8; 8]>(header);
        let size = platform.read_value::<u32>(header + 8).map(u64::from);
        let start = platform.read_value::<u32>(header + 12).map(u64::from);
        matches!((name, size, start), (Some(name), Some(size), Some(start))
            if name == *b".data\0\0\0" && parameter >= start
                && parameter + size_of::<usize>() as u64 <= start + size)
    });
    if !in_data {
        return Force::Refused("ForceD3D12 refused: parameter outside .data.".into());
    }
    for (rva, opcode) in asi::API_OVERRIDE_READS {
        let bytes = platform.read_value::<[u8; 7]>(base + rva as usize);
        if !bytes.is_some_and(|bytes| reads_parameter(rva, opcode, bytes)) {
            return Force::Refused(format!(
                "ForceD3D12 refused: unexpected bytes at RVA 0x{rva:x}."
            ));
        }
    }
    static D3D12: &CStr = c"d3d12";
    // The parameter is a pointer in the verified .data section; the string has static storage
    // in this never-unloaded module.
    let address = (base + asi::API_OVERRIDE as usize) as *mut *mut c_char;
    let slot = unsafe { AtomicPtr::from_ptr(address) };
    let previous = slot.compare_exchange(
        ptr::null_mut(),
        D3D12.as_ptr().cast_mut(),
        Ordering::SeqCst,
        Ordering::SeqCst,
    );
    match previous {
        Ok(_) => Force::Applied,
        Err(_) => {
            Force::Refused("ForceD3D12 refused: parameter already set by another component.".into())
        }
    }
}

fn trace_d3d12(platform: &AsiPlatform, applied: bool, path: &[u16]) -> Cow<'static, str> {
    // ForceD3D12 already validated this exact image and the decrypted read sites.
    if !applied {
        return "D3D12 result trace refused: build not validated.".into();
    }
    let target = unsafe { (platform.main_module)() } + asi::RESULT_CHECK.rva;
    if !platform.matches(target, &asi::RESULT_CHECK.expected) {
        return "D3D12 result trace refused: unexpected helper bytes.".into();
    }
    let status = unsafe { (platform.d3d12_trace)(target, layout::IMAGE_SIZE, path.as_ptr()) };
    format!(
        "D3D12 result trace at RVA 0x{:x}: {}",
        asi::RESULT_CHECK.rva,
        platform.status_text(status)
    )
    .into()
}

fn attach(platform: &AsiPlatform) {
    let Some(executable) = read_path(platform.executable_path, true) else {
        return;
    };
    if !is_game(&executable) {
        return;
    }
    if let Some((root, path)) =
        read_path(platform.module_path, false).and_then(|module| deployment(&module))
    {
        let config = Configuration {
            platform,
            section: wide("RDR2"),
            path,
        };
        if config.int("Enable", 1) != 0 {
            arm(platform, &config, &root);
        }
    }
    unsafe { (platform.start_worker)() };
}

/// Arms the enabled startup owners in their required order.
fn arm(platform: &AsiPlatform, config: &Configuration, root: &[u16]) {
    let mut applied = false;
    if config.int("ForceD3D12", 1) != 0 {
        let force = force_d3d12(platform);
        applied = force == Force::Applied;
        record(|results| {
            results.force = match force {
                Force::Applied => format!(
                    "ForceD3D12 applied: API parameter at RVA 0x{:x} set to \"d3d12\".",
                    asi::API_OVERRIDE
                )
                .into(),
                Force::Refused(reason) => reason.into(),
            }
        });
        if applied {
            engine_settings::install(platform, config.int("SRQuality", 2));
        }
    }
    if applied && config.enabled("NativeCapture") {
        let directory = config.string("CameraProbeDirectory", MAX_PATH);
        if !directory.is_empty() {
            unsafe { (platform.native_capture)(terminated(&directory).as_ptr()) };
        }
    }
    if let Some(frame_probe) = platform.frame_probe
        && config.enabled("FrameProbe")
    {
        let library = config.string("FrameProbeLibrary", MAX_PATH);
        let prefix = config.string("FrameProbePrefix", MAX_PATH);
        if !library.is_empty() && !prefix.is_empty() && prefix.len() < MAX_PATH as usize - 16 {
            let result =
                unsafe { frame_probe(terminated(&library).as_ptr(), terminated(&prefix).as_ptr()) };
            record(|results| results.frame = format!("Frame probe result={result}").into());
        }
    }
    // The engine resolves D3D12, DXGI, NGX and NVAPI entry points after CRT startup, so the
    // census must be bound here rather than on the worker thread.
    if config.enabled("Census") {
        let mut directory = config.string("CensusDirectory", MAX_PATH);
        if directory.is_empty() {
            directory = root.to_vec();
            directory.extend("ReScaleFrame\\census".encode_utf16());
        }
        // RSF_CENSUS_FLAG_* bits, for isolating a hook family during a failing run.
        let flags = config.int("CensusFlags", 0);
        let result = unsafe { (platform.census)(terminated(&directory).as_ptr(), flags) };
        record(|results| {
            results.census = format!(
                "Vendor census install result={result} flags=0x{flags:x} output={}",
                String::from_utf16_lossy(&directory)
            )
            .into();
        });
    }
    // The census initializes this ASI's MinHook instance; the optional game trace reuses it.
    let trace = config.enabled("TraceD3D12");
    if trace {
        let mut path = root.to_vec();
        path.extend("ReScaleFrame\\rdr2-d3d12-errors.log".encode_utf16());
        path.push(0);
        let message = trace_d3d12(platform, applied, &path);
        record(|results| results.trace = message);
    }
    if applied {
        if config.enabled("CameraProbe") {
            let directory = config.string("CameraProbeDirectory", MAX_PATH);
            let armed = !directory.is_empty()
                && unsafe { (platform.camera_probe)(terminated(&directory).as_ptr()) };
            let mut message = format!("Camera probe: {}", if armed { "armed" } else { "refused" });
            if armed && config.enabled("CameraReference") {
                let library = config.string("CameraReferenceLibrary", MAX_PATH);
                let reference = !library.is_empty()
                    && unsafe { (platform.camera_reference)(terminated(&library).as_ptr()) };
                message = format!(
                    "Camera probe: armed; ScriptHook reference: {}",
                    if reference {
                        "registered"
                    } else {
                        "unavailable"
                    }
                );
            }
            record(|results| results.camera = message.into());
        }
        let installed = unsafe { (platform.social_club)(trace) };
        record(|results| {
            results.social_club = format!(
                "Social Club backbuffer dispatcher at RVA 0x{:x}: {}",
                asi::SOCIAL_CLUB_DISPATCHER,
                if installed { "armed" } else { "refused" }
            )
            .into();
        });
    }
}

/// DllMain process attach of `ReScaleFrame.RDR2.asi`. A short or foreign table is refused.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_asi_attach(table: *const AsiPlatform) {
    if register(table)
        && let Some(platform) = platform()
    {
        attach(platform);
    }
}

/// Logs the startup results from the runtime session worker.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_rdr2_asi_report_startup(
    log: rsf_game_sys::rsf_game_log_fn,
    user: *mut c_void,
) {
    let Some(log) = log else { return };
    let results = crate::platform::lock(&RESULTS);
    for message in [
        &results.force,
        &results.trace,
        &results.social_club,
        &results.camera,
        &results.frame,
        &results.census,
    ] {
        if let Ok(text) = std::ffi::CString::new(message.as_bytes()) {
            unsafe { log(user, text.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests;
