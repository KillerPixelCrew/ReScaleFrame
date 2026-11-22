// SPDX-License-Identifier: MIT
//! Selects the outer Present detour and constructs its independent genuine forwarding route.
use super::hde_abi::{
    F_DISP32, F_ERROR, F_IMM8, F_IMM16, F_IMM32, F_IMM64, F_MODRM, F_PREFIX_67, F_RELATIVE,
    hde64_disasm, hde64s,
};
use super::{Handle, ROUTES, probe};
use crate::d3d11_observer_callbacks::snapshot;
use crate::d3d11_observer_module::ModuleFile;
use std::ffi::{CString, c_void};
use std::ptr::{self, null_mut, without_provenance_mut};
use windows_sys::Win32::{
    Foundation::{HMODULE, MAX_PATH},
    System::{
        Diagnostics::Debug::{FlushInstructionCache, ReadProcessMemory},
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW,
        },
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc, VirtualFree,
        },
        ProcessStatus::{K32GetModuleInformation, MODULEINFO},
        SystemInformation::{GetSystemInfo, SYSTEM_INFO},
        Threading::GetCurrentProcess,
    },
};

const BLOCK_SIZE: usize = 64;
const FILE_WINDOW: usize = 32;
const ABSOLUTE_JUMP_SIZE: usize = 14;
const CHAIN_LIMIT: usize = 8;
const MH_OK: i32 = 0;
const MH_ALREADY_INITIALIZED: i32 = 1;
const MH_NOT_CREATED: i32 = 4;
const MH_ENABLED: i32 = 5;
const MH_DISABLED: i32 = 6;

unsafe extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: Handle, detour: Handle, original: *mut Handle) -> i32;
    fn MH_EnableHook(target: Handle) -> i32;
    fn MH_DisableHook(target: Handle) -> i32;
    fn MH_RemoveHook(target: Handle) -> i32;
}

fn module_at(address: usize) -> Option<HMODULE> {
    let mut module = null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if unsafe { GetModuleHandleExW(flags, ptr::without_provenance(address), &mut module) } == 0 {
        return None;
    }
    (!module.is_null()).then_some(module)
}

fn module_path(module: HMODULE) -> Option<Vec<u16>> {
    let mut path = [0u16; MAX_PATH as usize];
    let length = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), MAX_PATH) } as usize;
    (length != 0 && length < path.len()).then(|| path[..length].to_vec())
}

fn module_leaf(module: HMODULE) -> String {
    let Some(path) = module_path(module) else {
        return "?".to_owned();
    };
    let name = path
        .rsplit(|&unit| unit == u16::from(b'\\'))
        .next()
        .unwrap_or(&path);
    String::from_utf16_lossy(name)
}

#[derive(Default)]
struct ModuleImage {
    module: HMODULE,
    base: usize,
    size: usize,
    path: String,
    file: Option<ModuleFile>,
}

impl ModuleImage {
    fn locate(&mut self, address: usize) -> Result<(), String> {
        let module =
            module_at(address).ok_or_else(|| format!("{address:#x} is not inside any module"))?;
        if self.module != module {
            let mut info = MODULEINFO::default();
            if unsafe {
                K32GetModuleInformation(
                    GetCurrentProcess(),
                    module,
                    &mut info,
                    size_of::<MODULEINFO>() as u32,
                )
            } == 0
                || info.lpBaseOfDll.is_null()
                || info.SizeOfImage == 0
            {
                return Err(format!("{address:#x} has no usable module extent"));
            }
            *self = Self {
                module,
                base: info.lpBaseOfDll.addr(),
                size: info.SizeOfImage as usize,
                ..Self::default()
            };
        }
        Ok(())
    }

    fn read(&mut self, address: usize) -> Result<(), String> {
        self.locate(address)?;
        if self.file.is_none() {
            let path = module_path(self.module).ok_or("the module's path could not be read")?;
            self.path = String::from_utf16_lossy(&path);
            self.file =
                Some(ModuleFile::open(&path).map_err(|error| format!("{} {error}", self.path))?);
        }
        Ok(())
    }

    fn disk(&self, address: usize, count: usize) -> Option<&[u8]> {
        let rva = address.checked_sub(self.base)?;
        if rva >= self.size || count > self.size - rva {
            return None;
        }
        self.file.as_ref()?.span(rva, count)
    }

    fn genuine(&mut self, route: &probe::Route) -> Result<usize, String> {
        let table = route.table.addr();
        let slot = table
            .checked_add(route.index * size_of::<Handle>())
            .ok_or("invalid table slot")?;
        self.locate(table)?;
        let current = usize::from_le_bytes(
            read::<{ size_of::<Handle>() }>(slot).ok_or("the Present slot could not be read")?,
        );
        if current
            .checked_sub(self.base)
            .is_some_and(|rva| rva < self.size)
        {
            return Ok(current);
        }
        self.read(table)?;
        let stored = self
            .disk(slot, size_of::<u64>())
            .ok_or_else(|| format!("no section of {} covers the table", self.path))?;
        let address = u64::from_le_bytes(stored.try_into().expect("checked slot span"));
        let base = self
            .file
            .as_ref()
            .expect("loaded module file")
            .preferred_base();
        let rva = address
            .checked_sub(base)
            .filter(|&rva| rva < self.size as u64)
            .ok_or_else(|| {
                format!(
                    "{} holds {address:#x} in that slot, which is outside its image",
                    self.path
                )
            })?;
        Ok(self.base + rva as usize)
    }
}

fn read<const N: usize>(address: usize) -> Option<[u8; N]> {
    let mut bytes = [0u8; N];
    let mut read = 0;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            ptr::without_provenance(address),
            bytes.as_mut_ptr().cast(),
            N,
            &mut read,
        )
    };
    (ok != 0 && read == N).then_some(bytes)
}

fn relative(address: usize, length: usize, displacement: [u8; 4]) -> Option<usize> {
    address
        .checked_add(length)?
        .checked_add_signed(i32::from_le_bytes(displacement) as isize)
}

fn decode_jump(address: usize) -> Option<usize> {
    match read::<2>(address)? {
        [0xe9, _] => relative(address, 5, read::<4>(address.checked_add(1)?)?),
        [0xff, 0x25] => {
            let indirect = relative(address, 6, read::<4>(address.checked_add(2)?)?)?;
            Some(usize::from_le_bytes(read(indirect)?))
        }
        [0x48, 0xb8] if read::<2>(address.checked_add(10)?)? == [0xff, 0xe0] => {
            Some(usize::from_le_bytes(read(address.checked_add(2)?)?))
        }
        _ => None,
    }
    .filter(|&target| target != 0)
}

// Failed installs free their allocation. A published genuine route is kept
// process-lifetime, including after successful retirement.
struct Trampoline(*mut u8);

impl Drop for Trampoline {
    fn drop(&mut self) {
        unsafe { VirtualFree(self.0.cast(), 0, MEM_RELEASE) };
    }
}

impl Trampoline {
    fn near(entry: usize) -> Option<Self> {
        let mut info = SYSTEM_INFO::default();
        unsafe { GetSystemInfo(&mut info) };
        let granularity = match info.dwAllocationGranularity {
            0 => 0x10000,
            value => value as usize,
        };
        let origin = entry & !(granularity - 1);
        let lowest = if origin > 0x7000_0000 {
            origin - 0x7000_0000
        } else {
            granularity
        };
        let mut candidate = origin.checked_sub(granularity)?;
        while candidate > lowest {
            let block = unsafe {
                VirtualAlloc(
                    ptr::without_provenance(candidate),
                    BLOCK_SIZE,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_EXECUTE_READWRITE,
                )
            };
            if !block.is_null() {
                return Some(Self(block.cast()));
            }
            candidate = candidate.checked_sub(granularity)?;
        }
        None
    }

    fn build(entry: usize, file: &[u8]) -> Result<Self, String> {
        let block = Self::near(entry)
            .ok_or_else(|| format!("no executable memory near {entry:#x} for a trampoline"))?;
        let bytes = build_trampoline(entry, block.0.addr(), file)?;
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), block.0, bytes.len());
            FlushInstructionCache(GetCurrentProcess(), block.0.cast(), BLOCK_SIZE);
        }
        Ok(block)
    }
}

fn build_trampoline(entry: usize, base: usize, file: &[u8]) -> Result<[u8; BLOCK_SIZE], String> {
    let mut trampoline = [0; BLOCK_SIZE];
    let mut copied = 0;
    while copied < ABSOLUTE_JUMP_SIZE {
        // HDE has no input-length argument. Pad its private read window and reject a
        // decoded length that reaches beyond the file-backed input.
        let tail = &file[copied..];
        let mut padded = [0u8; FILE_WINDOW];
        let count = tail.len().min(padded.len());
        padded[..count].copy_from_slice(&tail[..count]);
        let mut instruction = hde64s::default();
        let length = unsafe { hde64_disasm(padded.as_ptr().cast(), &mut instruction) } as usize;
        let flags = instruction.flags;
        let end = copied + length;
        if flags & F_ERROR != 0
            || length == 0
            || length > count
            || end > BLOCK_SIZE - ABSOLUTE_JUMP_SIZE
        {
            return Err(format!(
                "Present's prologue could not be decoded at byte {copied}"
            ));
        }
        if flags & F_RELATIVE != 0
            || matches!(
                instruction.opcode,
                0xe8 | 0xe9 | 0xeb | 0x70..=0x7f | 0xc2 | 0xc3
            )
            || instruction.opcode == 0x0f && matches!(instruction.opcode2, 0x80..=0x8f)
        {
            return Err(format!(
                "Present's prologue has a relative jump or return at byte {copied}"
            ));
        }
        trampoline[copied..end].copy_from_slice(&file[copied..end]);
        if flags & F_MODRM != 0 && instruction.modrm & 0xc7 == 0x05 {
            // HDE's address-size override decoder does not establish the disp32 layout
            // that the 64-bit RIP-relative relocation relies on.
            if flags & F_PREFIX_67 != 0 || flags & F_DISP32 == 0 {
                return Err(format!(
                    "Present's prologue has an unsupported address size at byte {copied}"
                ));
            }
            let immediate = ((flags & (F_IMM8 | F_IMM16 | F_IMM32 | F_IMM64)) >> 2) as usize;
            let at = end
                .checked_sub(immediate + 4)
                .filter(|&at| at >= copied)
                .ok_or("invalid RIP-relative instruction extent")?;
            let displacement = trampoline[at..at + 4]
                .try_into()
                .expect("checked displacement extent");
            let target = relative(entry, end, displacement).ok_or("invalid RIP-relative target")?;
            let moved = i32::try_from(target as i128 - (base as i128 + end as i128))
                .map_err(|_| "Present's prologue addresses memory too far from the trampoline")?;
            trampoline[at..at + 4].copy_from_slice(&moved.to_le_bytes());
        }
        copied = end;
    }
    trampoline[copied..copied + 6].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
    let resume = entry
        .checked_add(copied)
        .ok_or("invalid Present resume address")?;
    trampoline[copied + 6..copied + ABSOLUTE_JUMP_SIZE].copy_from_slice(&resume.to_le_bytes());
    Ok(trampoline)
}

fn say(text: String) {
    if let Some(callbacks) = snapshot()
        && let Ok(message) = CString::new(text)
    {
        callbacks.say(&message);
    }
}

fn select_target(entry: usize, hook: Handle) -> Option<(usize, String, usize)> {
    let mut at = entry;
    for hop in 1..=CHAIN_LIMIT {
        let target = decode_jump(at)?;
        if let Some(module) = module_at(target) {
            return (target != hook.addr()).then(|| (target, module_leaf(module), hop));
        }
        at = target;
    }
    None
}

fn install(
    image: &mut ModuleImage,
    route: &probe::Route,
    method: usize,
    label: &str,
) -> Result<(), String> {
    let genuine = image.genuine(route)?;
    image.read(genuine)?;
    let stored = image
        .disk(genuine, FILE_WINDOW)
        .ok_or_else(|| format!("no section of {} covers Present", image.path))?;
    let loaded = read::<FILE_WINDOW>(genuine).ok_or("the Present entry could not be read")?;
    let patched = loaded.as_slice() != stored;
    let trampoline = Trampoline::build(genuine, stored)?;
    let (target, owner, hops) = if patched {
        select_target(genuine, route.hook).ok_or_else(|| {
            format!(
                "Present's entry is patched ({:02x?}) and leads to no function inside a module",
                &loaded[..5]
            )
        })?
    } else {
        (genuine, module_leaf(image.module), 0)
    };
    let init = unsafe { MH_Initialize() };
    if !matches!(init, MH_OK | MH_ALREADY_INITIALIZED) {
        return Err(format!("MinHook did not initialise, status {init}"));
    }
    let target = without_provenance_mut::<c_void>(target);
    let mut chain = null_mut();
    let created = unsafe { MH_CreateHook(target, route.hook, &mut chain) };
    if created != MH_OK {
        return Err(format!(
            "the detour on {owner} could not be created, status {created}"
        ));
    }
    let genuine_route = trampoline.0.cast();
    ROUTES[method].publish(target, chain, genuine_route);
    let enabled = unsafe { MH_EnableHook(target) };
    if !matches!(enabled, MH_OK | MH_ENABLED) {
        let removed = unsafe { MH_RemoveHook(target) };
        if matches!(removed, MH_OK | MH_NOT_CREATED) {
            ROUTES[method].publish(null_mut(), null_mut(), null_mut());
        } else {
            std::mem::forget(trampoline);
        }
        return Err(format!(
            "the detour on {owner} could not be enabled, status {enabled}"
        ));
    }
    std::mem::forget(trampoline);
    if patched {
        say(format!(
            "observer: {label} entry at {genuine:#x} is hooked by others; detoured the outermost hook's function at {target:p} in {owner}, {hops} hop(s) down the chain. Genuine body at {genuine_route:p}"
        ));
    } else {
        say(format!(
            "observer: {label} detoured at {genuine:#x} in {owner}, entry unpatched"
        ));
    }
    Ok(())
}

pub(super) struct Setup {
    pub(super) routes: [probe::Route; 2],
    image: ModuleImage,
}

impl Setup {
    pub(super) fn new(routes: [probe::Route; 2]) -> Self {
        Self {
            routes,
            image: ModuleImage::default(),
        }
    }
}

pub(super) unsafe extern "C" fn detour(user: Handle, method: u32) -> i32 {
    let setup = unsafe { &mut *user.cast::<Setup>() };
    let method = method as usize;
    let label = ["Present", "Present1"][method];
    match install(&mut setup.image, &setup.routes[method], method, label) {
        Ok(()) => 1,
        Err(reason) => {
            if method == 0 {
                say(format!(
                    "observer: Present detour unavailable because {reason}. The table fallback refuses re-entry when the genuine route is unavailable"
                ));
            } else {
                say(format!("observer: Present1 hook unavailable: {reason}"));
            }
            0
        }
    }
}

pub(super) unsafe extern "C" fn remove(target: Handle) -> i32 {
    match unsafe { MH_DisableHook(target) } {
        MH_NOT_CREATED => 1,
        MH_OK | MH_DISABLED => i32::from(matches!(
            unsafe { MH_RemoveHook(target) },
            MH_OK | MH_NOT_CREATED
        )),
        _ => 0,
    }
}
