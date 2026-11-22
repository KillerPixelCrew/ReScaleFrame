// SPDX-License-Identifier: MIT
use crate::sdk_module::SdkModule;
use std::ffi::{c_char, c_void};
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::{
            Debug::{
                IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC, IMAGE_SCN_MEM_EXECUTE,
                IMAGE_SECTION_HEADER, ReadProcessMemory,
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
        ProcessStatus::{K32GetModuleInformation, MODULEINFO},
        SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE},
        Threading::{GetCurrentProcess, GetCurrentProcessId},
    },
};

type Handle = *mut c_void;

const NOT_APPLICABLE: u32 = 0;
const READY: u32 = 1;
const REFUSED: u32 = 2;
const REPORT_BYTES: usize = 512;
const WRAPPER_PATTERN_BYTES: usize = 10;
const WRAPPER_PATCH_OFFSET: usize = 7;

#[repr(C)]
struct Platform {
    supported: unsafe extern "C" fn(Handle) -> u32,
    verify_signature: unsafe extern "C" fn(*const u16) -> u32,
    wrapper_match: unsafe extern "C" fn(*const u8) -> u32,
    publish_wrapper: unsafe extern "C" fn(Handle) -> u32,
    is_provider: unsafe extern "C" fn(Handle) -> u32,
    prepare_provider: unsafe extern "C" fn(Handle, u32, &mut [c_char; REPORT_BYTES]) -> u32,
}

const _: () = assert!(size_of::<Platform>() == 6 * size_of::<Handle>());

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

    fn wrapper_patch(&self, platform: &Platform) -> Option<(Handle, u32)> {
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
                // The pinned wrapper profile starts with MOV EDX, imm32.
                if bytes[0] != 0xba {
                    continue;
                }
                let capacity = unsafe { (platform.wrapper_match)(bytes.as_ptr()) };
                if capacity == 0 {
                    continue;
                }
                if found.is_some() {
                    return None;
                }
                found = Some((
                    self.address(rva + offset + WRAPPER_PATCH_OFFSET, 3)?.cast(),
                    capacity,
                ));
            }
        }
        found
    }
}

fn prepare_wrapper(module: &SdkModule, platform: &Platform) -> u32 {
    let Some((address, capacity)) =
        ModuleImage::new(module).and_then(|image| image.wrapper_patch(platform))
    else {
        return 1;
    };
    if capacity > 1 && unsafe { (platform.publish_wrapper)(address) } != 0 {
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

fn find_provider(platform: &Platform) -> Option<SdkModule> {
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
            && unsafe { (platform.is_provider)(module.as_ptr()) } != 0
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

struct Owner(Mutex<State>);

impl Owner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
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
    UPDATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn try_update() -> Option<MutexGuard<'static, ()>> {
    match UPDATE.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_mfg_create() -> *mut Owner {
    crate::allocate_handle(Owner(Mutex::new(State::new())))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_prime(
    owner: &Owner,
    platform: &Platform,
    device: Handle,
    directory: *const u16,
    require_signature: u32,
) -> u32 {
    let Some(_update) = try_update() else {
        return 0;
    };
    if device.is_null() || unsafe { (platform.supported)(device) } == 0 {
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
    if require_signature != 0 && unsafe { (platform.verify_signature)(path.as_ptr()) } == 0 {
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
    let maximum = prepare_wrapper(&wrapper, platform);
    let mut state = owner.state();
    state.maximum = maximum;
    state.wrapper = Some(wrapper);
    // Zero means no report. Any loaded wrapper logs its effective capacity.
    state.maximum
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_prepare(
    owner: &Owner,
    platform: &Platform,
    device: Handle,
    get_state: Handle,
    report: &mut [c_char; REPORT_BYTES],
) -> u32 {
    let Some(_update) = try_update() else {
        return 0;
    };
    let (wrapper, maximum) = {
        let state = owner.state();
        if state.maximum <= 1 || device.is_null() || get_state.is_null() {
            return 0;
        }
        let Some(wrapper) = state.wrapper.as_ref() else {
            return 0;
        };
        // The update guard prevents releasing the module during the call.
        (wrapper.as_ptr(), state.maximum)
    };
    let prepared = retain_module(get_state).is_some_and(|module| {
        if module.as_ptr() != wrapper {
            return false;
        }
        find_provider(platform).is_some_and(|provider| {
            (unsafe { (platform.prepare_provider)(provider.as_ptr(), maximum, report) }) != 0
        })
    });
    if report[0] == 0 {
        for (dst, &byte) in report.iter_mut().zip(
            c"RTX 40 MFG unlock refused: wrapper or provider ownership mismatch"
                .to_bytes_with_nul(),
        ) {
            *dst = byte as c_char;
        }
    }
    owner.state().status = if prepared { READY } else { REFUSED };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_status(owner: *const Owner, status: &mut u32, maximum: &mut u32) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        let state = owner.state();
        *status = state.status;
        *maximum = state.maximum;
    } else {
        *status = NOT_APPLICABLE;
        *maximum = 1;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_release(owner: *const Owner) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        owner.release();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_destroy(owner: *mut Owner) {
    if !owner.is_null() {
        let owner = unsafe { Box::from_raw(owner) };
        owner.release();
    }
}
