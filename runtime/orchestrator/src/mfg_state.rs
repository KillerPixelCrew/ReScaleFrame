// SPDX-License-Identifier: MIT
use crate::sdk_module::SdkModule;
use crate::sync::lock;
use crate::sys::rsf_backend_log_fn as Log;
use std::ffi::{CStr, CString, c_void};
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::{
            Debug::{
                FlushInstructionCache, IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC,
                IMAGE_SCN_MEM_EXECUTE, IMAGE_SECTION_HEADER, ReadProcessMemory,
            },
            ToolHelp::{
                CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                TH32CS_SNAPMODULE,
            },
        },
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExW,
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
        },
        Memory::{PAGE_EXECUTE_READ, PAGE_EXECUTE_WRITECOPY, VirtualProtect},
        ProcessStatus::{K32GetModuleInformation, MODULEINFO},
        SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE},
        Threading::{GetCurrentProcess, GetCurrentProcessId},
    },
};

#[path = "mfg_gate.rs"]
mod gate;
#[path = "mfg_midpoint.rs"]
mod midpoint;
#[path = "mfg_provider.rs"]
mod provider;

type Handle = *mut c_void;
// The SDK host lends the NVIDIA signature check (`rsf_streamline_vk::admission`).
pub(crate) type VerifySignature = Option<unsafe fn(*const u16) -> bool>;

const NOT_APPLICABLE: u32 = 0;
const READY: u32 = 1;
const REFUSED: u32 = 2;
const REPORT_BYTES: usize = 512;
const WRAPPER_PATTERN_BYTES: usize = 10;
const WRAPPER_PATCH_OFFSET: usize = 7;

struct ProviderReceipt {
    temporal: bool,
    gate: bool,
    failure: u32,
}

fn emit(log: Log, user: Handle, text: &CStr) {
    if let Some(log) = log {
        unsafe { log(user, text.as_ptr()) };
    }
}

// The verified temporal program precedes the count/index gate, so the gate
// never opens a provider that would run the unpatched midpoint kernel.
fn prepare_provider(module: Handle, path: *const u16) -> ProviderReceipt {
    let temporal = !path.is_null() && midpoint::patch_provider(module, path);
    ProviderReceipt {
        temporal,
        gate: temporal && gate::patch(module),
        failure: midpoint::failure_code(),
    }
}

// A foreign NOP is not success. Runtime transitions arrive here with the
// presentation owner drained and no live FG allocation.
fn publish_wrapper(address: *mut u8) -> bool {
    const EXPECTED: [u8; 3] = [0x0f, 0x42, 0xd1];
    const REPLACEMENT: [u8; 3] = [0x90; 3];
    let site = address.cast::<[u8; 3]>();
    // The retained wrapper image covers these three bytes.
    let read = || unsafe { site.read_volatile() };
    let mut previous = 0;
    if !gate::protection_matches(address as usize, PAGE_EXECUTE_READ, 3)
        || read() != EXPECTED
        || unsafe { VirtualProtect(address.cast(), 3, PAGE_EXECUTE_WRITECOPY, &mut previous) } == 0
    {
        return false;
    }
    if read() == EXPECTED {
        unsafe { site.write_volatile(REPLACEMENT) };
    }
    unsafe { FlushInstructionCache(GetCurrentProcess(), address.cast(), 3) };
    let mut ignored = 0;
    let restored = unsafe { VirtualProtect(address.cast(), 3, previous, &mut ignored) };
    restored != 0 && read() == REPLACEMENT
}

struct ModuleImage {
    base: *mut u8,
    size: usize,
}

impl ModuleImage {
    fn new(module: &SdkModule) -> Option<Self> {
        let mut info = MODULEINFO::default();
        let result = unsafe {
            K32GetModuleInformation(
                GetCurrentProcess(),
                module.as_ptr(),
                &mut info,
                size_of::<MODULEINFO>() as u32,
            )
        };
        (result != 0 && info.lpBaseOfDll == module.as_ptr()).then_some(Self {
            base: info.lpBaseOfDll.cast(),
            size: info.SizeOfImage as usize,
        })
    }

    fn address(&self, offset: usize, size: usize) -> Option<*mut u8> {
        (offset.checked_add(size)? <= self.size).then(|| self.base.wrapping_add(offset))
    }

    fn read(&self, offset: usize, out: Handle, size: usize) -> Option<()> {
        let address = self.address(offset, size)?;
        let mut copied = 0;
        let result = unsafe {
            ReadProcessMemory(GetCurrentProcess(), address.cast(), out, size, &mut copied)
        };
        (result != 0 && copied == size).then_some(())
    }

    fn record<T: Default>(&self, offset: usize) -> Option<T> {
        let mut record = T::default();
        self.read(offset, (&raw mut record).cast(), size_of::<T>())?;
        Some(record)
    }

    fn wrapper_patch(&self) -> Option<(*mut u8, u32)> {
        let dos: IMAGE_DOS_HEADER = self.record(0)?;
        if dos.e_magic != IMAGE_DOS_SIGNATURE || dos.e_lfanew <= 0 {
            return None;
        }
        let nt = dos.e_lfanew as usize;
        let headers: IMAGE_NT_HEADERS64 = self.record(nt)?;
        if headers.Signature != IMAGE_NT_SIGNATURE
            || headers.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC
            || usize::from(headers.FileHeader.SizeOfOptionalHeader)
                < size_of_val(&headers.OptionalHeader)
            || headers.OptionalHeader.SizeOfImage as usize > self.size
        {
            return None;
        }
        let header_size = std::mem::offset_of!(IMAGE_NT_HEADERS64, OptionalHeader);
        let table = nt + header_size + usize::from(headers.FileHeader.SizeOfOptionalHeader);
        let mut found = None;
        let mut section_bytes = Vec::new();
        for index in 0..usize::from(headers.FileHeader.NumberOfSections) {
            let section: IMAGE_SECTION_HEADER =
                self.record(table + index * size_of::<IMAGE_SECTION_HEADER>())?;
            let rva = section.VirtualAddress as usize;
            let image_size = headers.OptionalHeader.SizeOfImage as usize;
            if section.Characteristics & IMAGE_SCN_MEM_EXECUTE == 0 || rva >= image_size {
                continue;
            }
            let size = (image_size - rva).min(
                section
                    .SizeOfRawData
                    .max(unsafe { section.Misc.VirtualSize }) as usize,
            );
            if size < WRAPPER_PATTERN_BYTES {
                continue;
            }
            section_bytes.clear();
            section_bytes.try_reserve_exact(size).ok()?;
            section_bytes.resize(size, 0);
            self.read(rva, section_bytes.as_mut_ptr().cast(), size)?;
            for (offset, bytes) in section_bytes.windows(WRAPPER_PATTERN_BYTES).enumerate() {
                let Some(capacity) = wrapper_capacity(bytes) else {
                    continue;
                };
                if found.is_some() {
                    return None;
                }
                found = Some((
                    self.address(rva + offset + WRAPPER_PATCH_OFFSET, 3)?,
                    capacity,
                ));
            }
        }
        found
    }
}

fn wrapper_capacity(bytes: &[u8]) -> Option<u32> {
    // MFG-Unlocker 2b52823e universal_wrapper_profile.h.
    let &[0xba, a, b, c, d, 0x3b, 0xca, x, y, z] = bytes else {
        return None;
    };
    let maximum = u32::from_le_bytes([a, b, c, d]);
    (matches!(maximum, 1 | 3 | 5) && matches!([x, y, z], [0x0f, 0x42, 0xd1] | [0x90, 0x90, 0x90]))
        .then_some(maximum)
}

fn prepare_wrapper(module: &SdkModule) -> u32 {
    let Some((address, capacity)) =
        ModuleImage::new(module).and_then(|image| image.wrapper_patch())
    else {
        return 1;
    };
    if capacity > 1 && publish_wrapper(address) {
        capacity
    } else {
        1
    }
}

struct ModuleSnapshot(HANDLE);

impl Drop for ModuleSnapshot {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn retain_module(address: Handle) -> Option<SdkModule> {
    let mut module = null_mut();
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            address.cast(),
            &mut module,
        )
    } == 0
    {
        return None;
    }
    SdkModule::adopt_system(module)
}

fn find_provider() -> Option<SdkModule> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, GetCurrentProcessId()) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let snapshot = ModuleSnapshot(snapshot);
    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = None;
    let mut more = unsafe { Module32FirstW(snapshot.0, &mut entry) };
    while more != 0 {
        if let Some(module) = retain_module(entry.modBaseAddr.cast())
            && module.as_ptr() == entry.modBaseAddr.cast()
            && provider::implementation(module.as_ptr())
        {
            if found.is_some() {
                return None;
            }
            found = Some(module);
        }
        more = unsafe { Module32NextW(snapshot.0, &mut entry) };
    }
    found
}

struct State {
    wrapper: Option<SdkModule>,
    maximum: u32,
    status: u32,
}

impl State {
    const fn new() -> Self {
        Self {
            wrapper: None,
            maximum: 1,
            status: NOT_APPLICABLE,
        }
    }
}

pub(crate) struct Owner(Mutex<State>);

impl Owner {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.0)
    }

    fn release(&self) {
        let _update = update();
        let previous = std::mem::replace(&mut *self.state(), State::new());
        drop(previous);
    }
}

// The native compatibility calls operate on process-wide SDK modules.
static UPDATE: Mutex<()> = Mutex::new(());

fn update() -> MutexGuard<'static, ()> {
    lock(&UPDATE)
}

fn try_update() -> Option<MutexGuard<'static, ()>> {
    crate::sync::try_lock(&UPDATE)
}

pub(crate) fn rsf_core_mfg_create() -> *mut Owner {
    crate::allocate_handle(Owner(Mutex::new(State::new())))
}

unsafe fn prime(
    owner: &Owner,
    device: Handle,
    directory: *const u16,
    verify_signature: VerifySignature,
) -> u32 {
    let Some(_update) = try_update() else {
        return 0;
    };
    if device.is_null() || !unsafe { midpoint::observe_d3d12_device(device) } {
        owner.state().status = NOT_APPLICABLE;
        return 0;
    }
    {
        let mut state = owner.state();
        if state.wrapper.is_some() {
            return 0;
        }
        state.status = REFUSED;
    }
    // The host owns a validated, NUL-terminated directory of at most 1024 units.
    let path: Vec<_> = (0..)
        .map(|index| unsafe { *directory.add(index) })
        .take_while(|&unit| unit != 0)
        .chain("\\sl.dlss_g.dll".encode_utf16())
        .chain([0])
        .collect();
    if let Some(verify) = verify_signature
        && !unsafe { verify(path.as_ptr()) }
    {
        return 0;
    }
    let Some(wrapper) = SdkModule::adopt_system(unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    }) else {
        return 0;
    };
    let maximum = prepare_wrapper(&wrapper);
    let mut state = owner.state();
    state.maximum = maximum;
    state.wrapper = Some(wrapper);
    maximum
}

// A null verifier means the host does not require a signed wrapper.
pub(crate) unsafe fn rsf_core_mfg_prime(
    owner: &Owner,
    device: Handle,
    directory: *const u16,
    verify_signature: VerifySignature,
    log: Log,
    user: Handle,
) {
    let maximum = unsafe { prime(owner, device, directory, verify_signature) };
    if maximum != 0 && log.is_some() {
        let prepared = u32::from(maximum > 1);
        let text = CString::new(format!(
            "RTX 40 MFG wrapper prepared={prepared} capacity={maximum} before plugin initialization"
        ));
        if let Ok(text) = text {
            emit(log, user, &text);
        }
    }
}

pub(crate) unsafe fn rsf_core_mfg_prepare(
    owner: &Owner,
    device: Handle,
    get_state: Handle,
    log: Log,
    user: Handle,
) {
    let Some(update) = try_update() else {
        return;
    };
    let (wrapper, maximum) = {
        let state = owner.state();
        if state.maximum <= 1 || device.is_null() || get_state.is_null() {
            return;
        }
        let Some(wrapper) = state.wrapper.as_ref() else {
            return;
        };
        // The update guard prevents releasing the module during the call.
        (wrapper.as_ptr(), state.maximum)
    };
    let mut path = [0u16; 32768];
    let receipt = retain_module(get_state).and_then(|module| {
        if module.as_ptr() != wrapper {
            return None;
        }
        let provider = find_provider()?;
        let length = provider::module_path(provider.as_ptr(), &mut path).unwrap_or(0);
        let eligible = length != 0
            && unsafe { provider::supported_provider(provider.as_ptr(), path.as_ptr()) != 0 };
        // A null path requests only the current failure receipt.
        let path_ptr = if eligible {
            path.as_ptr()
        } else {
            std::ptr::null()
        };
        Some((length, prepare_provider(provider.as_ptr(), path_ptr)))
    });
    let prepared = receipt.as_ref().is_some_and(|(_, receipt)| receipt.gate);
    owner.state().status = if prepared { READY } else { REFUSED };
    drop(update);
    if log.is_none() {
        return;
    }
    if let Some((
        length,
        ProviderReceipt {
            temporal,
            gate,
            failure,
        },
    )) = receipt
    {
        let status = if prepared { "ready" } else { "refused" };
        let (temporal, gate) = (u32::from(temporal), u32::from(gate));
        let path = String::from_utf16_lossy(&path[..length]);
        let mut text = format!(
            "RTX 40 MFG unlock {status}: wrapper_capacity={maximum} temporal={temporal} gate={gate} failure={failure} provider={path}"
        );
        text.truncate(text.floor_char_boundary(REPORT_BYTES - 1));
        if let Ok(text) = CString::new(text) {
            emit(log, user, &text);
        }
    } else {
        emit(
            log,
            user,
            c"RTX 40 MFG unlock refused: wrapper or provider ownership mismatch",
        );
    }
}

pub(crate) unsafe fn rsf_core_mfg_status(
    owner: *const Owner,
    status: &mut u32,
    maximum: &mut u32,
) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        let state = owner.state();
        *status = state.status;
        *maximum = state.maximum;
    } else {
        *status = NOT_APPLICABLE;
        *maximum = 1;
    }
}

pub(crate) unsafe fn rsf_core_mfg_release(owner: *const Owner) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        owner.release();
    }
}

pub(crate) unsafe fn rsf_core_mfg_destroy(owner: *mut Owner) {
    if !owner.is_null() {
        let owner = unsafe { Box::from_raw(owner) };
        owner.release();
    }
}
