// SPDX-License-Identifier: MIT
//! `ReScaleFrame.RDR2.asi` admission under the loader lock: the process identity, the
//! `ReScaleFrame.ini` `[RDR2]` gates, the exact-build ForceD3D12 parameter write, and the order in
//! which the engine-settings owner and the ASI's native diagnostic owners are armed. The ASI loader
//! loads the module during CRT startup, after decryption and before the engine reads its settings.
#[cfg(windows)]
#[path = "asi/windows.rs"]
mod windows;
#[cfg(all(windows, feature = "capture-fixture", not(test)))]
pub(crate) use windows::{RSF_CENSUS_ABI_VERSION, rsf_census_options};
#[cfg(windows)]
pub(crate) use windows::{
    adapter_vendor, desktop_refresh, engine_find, engine_manager, engine_owner, social_club,
};
#[cfg(all(windows, feature = "research"))]
pub(crate) use windows::{
    camera_probe, camera_reference, census, census_report, d3d12_trace, dump, frame_probe, measure,
    module_list, native_capture,
};

use crate::engine_settings;
use crate::layout::{self, asi};
use crate::platform::{self, lock, read_value};
use crate::runtime_worker::log;
use rsf_game_sys::win32::module_path;
use rsf_minhook_sys::MH_StatusToString;
use std::ffi::{CStr, c_char};
use std::ptr::{self, null_mut};
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::WindowsProgramming::GetPrivateProfileIntW;
#[cfg(feature = "research")]
use windows_sys::Win32::System::WindowsProgramming::GetPrivateProfileStringW;

#[cfg(feature = "research")]
const MAX_PATH: u32 = 260;
const LONG_PATH: u32 = 32768;

#[cfg(feature = "research")]
#[repr(C)]
pub(crate) struct DiagnosticHook {
    detour: *const std::ffi::c_void,
    original: *mut *mut std::ffi::c_void,
}

#[cfg(all(windows, feature = "research"))]
impl DiagnosticHook {
    pub(crate) const fn new(
        detour: *const std::ffi::c_void,
        original: *mut *mut std::ffi::c_void,
    ) -> Self {
        Self { detour, original }
    }
}

#[cfg(feature = "research")]
pub(crate) enum DiagnosticInstall {
    Installed,
    Refused,
    Retained,
}

/// Shared startup transaction for the ASI's two-site diagnostic probes.
#[cfg(feature = "research")]
pub(crate) fn install_diagnostic_hooks(
    image: usize,
    sites: [layout::Site; 2],
    hooks: &[DiagnosticHook; 2],
) -> DiagnosticInstall {
    use rsf_minhook_sys::{MH_CreateHook, MH_OK};
    if image == 0
        || !sites
            .iter()
            .all(|site| platform::matches(image.wrapping_add(site.rva), &site.expected))
        || !unsafe { platform::hook_initialize() }
    {
        return DiagnosticInstall::Refused;
    }
    let targets = sites.map(|site| image + site.rva);
    for (index, (&target, hook)) in targets.iter().zip(hooks).enumerate() {
        if unsafe { MH_CreateHook(target as _, hook.detour.cast_mut(), hook.original) } != MH_OK {
            let removed = targets[..index]
                .iter()
                .all(|&created| platform::remove(created));
            return if removed {
                DiagnosticInstall::Refused
            } else {
                DiagnosticInstall::Retained
            };
        }
    }
    if targets.iter().all(|&target| platform::enable(target)) {
        return DiagnosticInstall::Installed;
    }
    // A partially enabled set can leave a callback on the stack. Disable both sites and keep
    // their trampolines until process exit.
    for target in targets {
        platform::disable(target);
    }
    DiagnosticInstall::Retained
}

/// The name of a MinHook status.
pub(crate) fn status_text(status: i32) -> String {
    let text = unsafe { MH_StatusToString(status) };
    if text.is_null() {
        return format!("MH_STATUS {status}");
    }
    // MinHook status names are static strings.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

/// Startup results in the order the ASI produced them, announced by the worker outside the
/// loader lock.
static RESULTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(message: impl Into<String>) {
    lock(&RESULTS).push(message.into());
}

pub(crate) use rsf_game_sys::ffi_util::wide;
#[cfg(feature = "research")]
fn terminated(text: &[u16]) -> Vec<u16> {
    rsf_game_sys::ffi_util::wide_path(text, "")
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

struct Configuration {
    section: Vec<u16>,
    path: Vec<u16>,
}

impl Configuration {
    fn int(&self, key: &str, fallback: i32) -> u32 {
        let key = wide(key);
        let value = unsafe {
            GetPrivateProfileIntW(
                self.section.as_ptr(),
                key.as_ptr(),
                fallback,
                self.path.as_ptr(),
            )
        };
        value as u32
    }

    #[cfg(feature = "research")]
    fn enabled(&self, key: &str) -> bool {
        self.int(key, 0) != 0
    }

    /// The value without its terminator, read into a buffer of `capacity` units.
    #[cfg(feature = "research")]
    fn string(&self, key: &str, capacity: u32) -> Vec<u16> {
        let key = wide(key);
        let read_capacity = rsf_game_sys::profile_text::read_capacity(capacity as usize);
        let mut value = vec![0; read_capacity];
        let length = unsafe {
            GetPrivateProfileStringW(
                self.section.as_ptr(),
                key.as_ptr(),
                windows_sys::core::w!(""),
                value.as_mut_ptr(),
                read_capacity as u32,
                self.path.as_ptr(),
            )
        };
        value.truncate((length as usize).min(read_capacity.saturating_sub(1)));
        let mut value = rsf_game_sys::profile_text::decode(value);
        value.truncate(capacity.saturating_sub(1) as usize);
        value
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Force {
    Applied,
    Refused(String),
}

/// Writes "d3d12" to the engine's API parameter when the image, the parameter's section and every
/// read site match the supported build and nothing else has set it.
fn force_d3d12() -> Force {
    let base = unsafe { platform::main_module() };
    let Some((nt, facts)) = platform::image_header(base) else {
        return Force::Refused("ForceD3D12 refused: no main image.".into());
    };
    if facts.timestamp != layout::IMAGE_TIMESTAMP || facts.image_size != layout::IMAGE_SIZE {
        return Force::Refused(format!(
            "ForceD3D12 refused: build differs (timestamp 0x{:x}, image 0x{:x}).",
            facts.timestamp, facts.image_size
        ));
    }
    // IMAGE_FIRST_SECTION: NumberOfSections at +6, SizeOfOptionalHeader at +0x14.
    let sections = read_value::<u16>(nt + 6).unwrap_or(0);
    let optional = read_value::<u16>(nt + 0x14).unwrap_or(0);
    let first = nt + 0x18 + usize::from(optional);
    let parameter = u64::from(asi::API_OVERRIDE);
    let in_data = (0..usize::from(sections)).any(|index| {
        let header = first + index * 40;
        let name = read_value::<[u8; 8]>(header);
        let size = read_value::<u32>(header + 8).map(u64::from);
        let start = read_value::<u32>(header + 12).map(u64::from);
        matches!((name, size, start), (Some(name), Some(size), Some(start))
            if name == *b".data\0\0\0" && parameter >= start
                && parameter + size_of::<usize>() as u64 <= start + size)
    });
    if !in_data {
        return Force::Refused("ForceD3D12 refused: parameter outside .data.".into());
    }
    for (rva, opcode) in asi::API_OVERRIDE_READS {
        let bytes = read_value::<[u8; 7]>(base + rva as usize);
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

#[cfg(feature = "research")]
fn trace_d3d12(applied: bool, path: &[u16]) -> String {
    // ForceD3D12 already validated this exact image and the decrypted read sites.
    if !applied {
        return "D3D12 result trace refused: build not validated.".into();
    }
    let target = unsafe { platform::main_module() } + asi::RESULT_CHECK.rva;
    if !platform::matches(target, &asi::RESULT_CHECK.expected) {
        return "D3D12 result trace refused: unexpected helper bytes.".into();
    }
    let status = unsafe { d3d12_trace(target, layout::IMAGE_SIZE, path.as_ptr()) };
    format!(
        "D3D12 result trace at RVA 0x{:x}: {}",
        asi::RESULT_CHECK.rva,
        status_text(status)
    )
}

/// DllMain process attach of `ReScaleFrame.RDR2.asi`.
fn attach(module: HMODULE) {
    let Some(executable) = (unsafe { module_path(null_mut()) }) else {
        return;
    };
    if !is_game(executable.units()) {
        return;
    }
    if let Some((root, path)) =
        unsafe { module_path(module) }.and_then(|module| deployment(module.units()))
    {
        let config = Configuration {
            section: wide("RDR2"),
            path,
        };
        if config.int("Enable", 1) != 0 {
            arm(&config, &root);
        }
    }
    unsafe { windows::start_worker() };
}

/// The research owners (developer builds only), in their startup order. Returns whether the D3D12
/// trace is enabled, which the Social Club dispatcher also reports.
#[cfg(feature = "research")]
fn arm_research(config: &Configuration, root: &[u16], applied: bool) -> bool {
    if applied && config.enabled("NativeCapture") {
        let directory = config.string("CameraProbeDirectory", MAX_PATH);
        if !directory.is_empty() {
            unsafe { native_capture(terminated(&directory).as_ptr()) };
        }
    }
    if let Some(frame_probe) = frame_probe()
        && config.enabled("FrameProbe")
    {
        let library = config.string("FrameProbeLibrary", MAX_PATH);
        let prefix = config.string("FrameProbePrefix", MAX_PATH);
        if !library.is_empty() && !prefix.is_empty() && prefix.len() < MAX_PATH as usize - 16 {
            let result =
                unsafe { frame_probe(terminated(&library).as_ptr(), terminated(&prefix).as_ptr()) };
            record(format!("Frame probe result={result}"));
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
        let result = unsafe { census(terminated(&directory).as_ptr(), flags) };
        record(format!(
            "Vendor census install result={result} flags=0x{flags:x} output={}",
            String::from_utf16_lossy(&directory)
        ));
    }
    // The census initializes this ASI's MinHook instance; the optional game trace reuses it.
    let trace = config.enabled("TraceD3D12");
    if trace {
        let mut path = root.to_vec();
        path.extend("ReScaleFrame\\rdr2-d3d12-errors.log".encode_utf16());
        path.push(0);
        record(trace_d3d12(applied, &path));
    }
    if applied && config.enabled("CameraProbe") {
        let directory = config.string("CameraProbeDirectory", MAX_PATH);
        let armed =
            !directory.is_empty() && unsafe { camera_probe(terminated(&directory).as_ptr()) };
        let mut message = format!("Camera probe: {}", if armed { "armed" } else { "refused" });
        if armed && config.enabled("CameraReference") {
            let library = config.string("CameraReferenceLibrary", MAX_PATH);
            let reference =
                !library.is_empty() && unsafe { camera_reference(terminated(&library).as_ptr()) };
            message = format!(
                "Camera probe: armed; ScriptHook reference: {}",
                if reference {
                    "registered"
                } else {
                    "unavailable"
                }
            );
        }
        record(message);
    }
    trace
}

/// Arms the enabled startup owners in their required order.
fn arm(
    config: &Configuration,
    #[cfg_attr(not(feature = "research"), expect(unused_variables))] root: &[u16],
) {
    let mut applied = false;
    if config.int("ForceD3D12", 1) != 0 {
        let force = force_d3d12();
        applied = force == Force::Applied;
        record(match force {
            Force::Applied => format!(
                "ForceD3D12 applied: API parameter at RVA 0x{:x} set to \"d3d12\".",
                asi::API_OVERRIDE
            ),
            Force::Refused(reason) => reason,
        });
        if applied {
            engine_settings::install(config.int("SRQuality", 2));
        }
    }
    #[cfg(feature = "research")]
    let trace = arm_research(config, root, applied);
    #[cfg(not(feature = "research"))]
    let trace = false;
    if applied {
        let installed = unsafe { social_club(trace) };
        record(format!(
            "Social Club backbuffer dispatcher at RVA 0x{:x}: {}",
            asi::SOCIAL_CLUB_DISPATCHER,
            if installed { "armed" } else { "refused" }
        ));
    }
}

/// Logs the startup results from the runtime session worker.
pub(crate) fn report_startup() {
    for message in lock(&RESULTS).iter() {
        log(message);
    }
}

#[cfg(test)]
mod tests;
