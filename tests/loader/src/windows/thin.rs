// SPDX-License-Identifier: MIT
mod scenario;

use super::{address, fail, load, named, null_mut, ordinal, require, system_path};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_void};
use std::path::Path;
use std::ptr::fn_addr_eq;
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE, S_OK};
use windows_sys::Win32::System::Diagnostics::Debug::{
    IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT, IMAGE_DIRECTORY_ENTRY_EXPORT, IMAGE_DIRECTORY_ENTRY_IMPORT,
    IMAGE_NT_HEADERS64, IMAGE_SECTION_HEADER,
};
use windows_sys::Win32::System::LibraryLoader::{
    DONT_RESOLVE_DLL_REFERENCES, GetModuleHandleW, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_AMD64;
use windows_sys::Win32::System::SystemServices::{
    IMAGE_DOS_HEADER, IMAGE_EXPORT_DIRECTORY, IMAGE_IMPORT_DESCRIPTOR,
};
use windows_sys::core::{GUID, HRESULT};

const IID_DIRECT_INPUT_8W: GUID = GUID::from_u128(0xbf798031_483a_4da2_aa99_5d64ed369700);
const IID_NULL: GUID = GUID::from_u128(0);
type Create =
    unsafe extern "system" fn(HMODULE, u32, *const GUID, *mut *mut c_void, *mut c_void) -> HRESULT;
type Status = unsafe extern "system" fn() -> HRESULT;

fn create_proc(carrier: HMODULE) -> Create {
    unsafe { std::mem::transmute::<_, Create>(address(carrier, c"DirectInput8Create")) }
}

unsafe fn create_object(create: Create, output: *mut *mut c_void) {
    require(
        unsafe {
            create(
                GetModuleHandleW(null_mut()),
                0x0800,
                &IID_DIRECT_INPUT_8W,
                output,
                null_mut(),
            )
        } == S_OK
            && !unsafe { output.read() }.is_null(),
        "genuine DirectInput8 COM object created through carrier",
    );
}

unsafe fn release(object: *mut c_void) {
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    let table = unsafe { object.cast::<*const usize>().read() };
    // IUnknown is the prefix of IDirectInput8W. Release is its third entry.
    let release = unsafe { std::mem::transmute::<usize, Release>(table.add(2).read()) };
    unsafe { release(object) };
}

fn system(module: &str, flags: u32) -> HMODULE {
    let path = system_path(module);
    unsafe { LoadLibraryExW(path.as_ptr(), null_mut(), flags) }
}

struct Pe {
    base: *const u8,
    headers: IMAGE_NT_HEADERS64,
    sections: *const IMAGE_SECTION_HEADER,
}

impl Pe {
    fn mapped(module: HMODULE) -> Self {
        require(!module.is_null(), "map Windows DLL");
        let base = module.cast::<u8>();
        let dos = unsafe { base.cast::<IMAGE_DOS_HEADER>().read_unaligned() };
        let nt = unsafe { base.add(dos.e_lfanew as usize).cast::<IMAGE_NT_HEADERS64>() };
        let headers = unsafe { nt.read_unaligned() };
        let sections = unsafe {
            nt.cast::<u8>()
                .add(
                    std::mem::offset_of!(IMAGE_NT_HEADERS64, OptionalHeader)
                        + usize::from(headers.FileHeader.SizeOfOptionalHeader),
                )
                .cast()
        };
        Self {
            base,
            headers,
            sections,
        }
    }

    fn rva(&self, directory: u16) -> u32 {
        self.headers.OptionalHeader.DataDirectory[directory as usize].VirtualAddress
    }

    fn exports(&self) -> BTreeMap<CString, u16> {
        let rva = self.rva(IMAGE_DIRECTORY_ENTRY_EXPORT);
        require(rva != 0, "PE has export directory");
        let table = unsafe {
            self.base
                .add(rva as usize)
                .cast::<IMAGE_EXPORT_DIRECTORY>()
                .read_unaligned()
        };
        require(
            table.NumberOfNames == table.NumberOfFunctions,
            "no unnamed or extra exports",
        );
        let names = unsafe { self.base.add(table.AddressOfNames as usize).cast::<u32>() };
        let ordinals = unsafe {
            self.base
                .add(table.AddressOfNameOrdinals as usize)
                .cast::<u16>()
        };
        (0..table.NumberOfNames as usize)
            .map(|i| unsafe {
                let name =
                    CStr::from_ptr(self.base.add(names.add(i).read_unaligned() as usize).cast())
                        .to_owned();
                let ordinal =
                    u16::try_from(table.Base + u32::from(ordinals.add(i).read_unaligned()))
                        .unwrap_or_else(|_| fail("export ordinal in PE range"));
                (name, ordinal)
            })
            .collect()
    }

    fn check_imports(&self) {
        let rva = self.rva(IMAGE_DIRECTORY_ENTRY_IMPORT);
        require(rva != 0, "imports exist");
        let mut imports = unsafe {
            self.base
                .add(rva as usize)
                .cast::<IMAGE_IMPORT_DESCRIPTOR>()
        };
        loop {
            let import = unsafe { imports.read_unaligned() };
            if import.Name == 0 {
                break;
            }
            let name = unsafe { CStr::from_ptr(self.base.add(import.Name as usize).cast()) }
                .to_bytes()
                .to_ascii_lowercase();
            let allowed = matches!(
                name.as_slice(),
                b"kernel32.dll"
                    | b"ntdll.dll"
                    | b"api-ms-win-core-synch-l1-2-0.dll"
                    | b"vcruntime140.dll"
                    | b"vcruntime140d.dll"
                    | b"vcruntime140_1.dll"
                    | b"vcruntime140_1d.dll"
                    | b"msvcp140.dll"
                    | b"msvcp140d.dll"
                    | b"ucrtbased.dll"
                    | b"msvcrt.dll"
            ) || name.starts_with(b"api-ms-win-crt-");
            if !allowed {
                fail(&format!(
                    "unexpected import {}",
                    String::from_utf8_lossy(&name)
                ));
            }
            imports = unsafe { imports.add(1) };
        }
        require(
            self.rva(IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT) == 0,
            "no hidden delay imports",
        );
    }

    fn check_identity(&self, file: &[u8]) {
        // Read the initialized image bytes from disk. Mapping as an image can leave
        // inaccessible gaps between sections and excludes the on-disk COFF symbols.
        for index in 0..usize::from(self.headers.FileHeader.NumberOfSections) {
            let section = unsafe { self.sections.add(index).read_unaligned() };
            let start = section.PointerToRawData as usize;
            let end = start + section.SizeOfRawData as usize;
            let bytes = file
                .get(start..end)
                .unwrap_or_else(|| fail("PE section in file"));
            let bytes = bytes.to_ascii_lowercase();
            for name in [b"anomaly".as_slice(), b"orchestrator", b"ac7"] {
                require(
                    !bytes.windows(name.len()).any(|slice| slice == name),
                    "no game identity or orchestrator code/debug path",
                );
            }
        }
    }
}

fn pe_test(path: &Path) {
    let carrier = load(path, DONT_RESOLVE_DLL_REFERENCES);
    let genuine = system("dinput8.dll", DONT_RESOLVE_DLL_REFERENCES);
    let pe = Pe::mapped(carrier);
    require(
        pe.exports() == Pe::mapped(genuine).exports(),
        "export names and ordinals match System32 DirectInput",
    );
    require(
        pe.headers.FileHeader.Machine == IMAGE_FILE_MACHINE_AMD64,
        "native x64 image",
    );
    pe.check_imports();

    pe.check_identity(&std::fs::read(path).unwrap_or_else(|_| fail("read carrier from disk")));
    unsafe {
        FreeLibrary(genuine);
        FreeLibrary(carrier);
    }
}

fn forward_test(path: &Path) {
    let carrier = load(path, 0);
    let genuine = system("dinput8.dll", LOAD_LIBRARY_SEARCH_SYSTEM32);
    require(
        !carrier.is_null() && !genuine.is_null() && carrier != genuine,
        "load distinct carrier and genuine System32 DLL",
    );
    for (name, number) in Pe::mapped(genuine).exports() {
        require(
            named(carrier, &name)
                .zip(ordinal(carrier, number))
                .is_some_and(|(a, b)| fn_addr_eq(a, b)),
            "ordinal aliases same export",
        );
    }
    let create = create_proc(carrier);
    let real_create = create_proc(genuine);
    let mut output = null_mut();
    let mut real_output = null_mut();
    unsafe {
        let instance = GetModuleHandleW(null_mut());
        require(
            create(instance, 0, &IID_DIRECT_INPUT_8W, &mut output, null_mut())
                == real_create(
                    instance,
                    0,
                    &IID_DIRECT_INPUT_8W,
                    &mut real_output,
                    null_mut(),
                ),
            "invalid DirectInput version HRESULT forwarded exactly",
        );
    }
    unsafe {
        create_object(create, &raw mut output);
        release(output);
    }
    type ClassObject =
        unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    type Format = unsafe extern "system" fn() -> *const c_void;
    unsafe {
        let status = std::mem::transmute::<_, Status>(address(carrier, c"DllCanUnloadNow"));
        let real_status = std::mem::transmute::<_, Status>(address(genuine, c"DllCanUnloadNow"));
        require(status() == real_status(), "DllCanUnloadNow system HRESULT");
        let class = std::mem::transmute::<_, ClassObject>(address(carrier, c"DllGetClassObject"));
        let real_class =
            std::mem::transmute::<_, ClassObject>(address(genuine, c"DllGetClassObject"));
        require(
            class(&IID_NULL, &IID_NULL, &mut output)
                == real_class(&IID_NULL, &IID_NULL, &mut real_output),
            "DllGetClassObject system HRESULT",
        );
        let format = std::mem::transmute::<_, Format>(address(carrier, c"GetdfDIJoystick"));
        let real_format = std::mem::transmute::<_, Format>(address(genuine, c"GetdfDIJoystick"));
        require(
            format() == real_format(),
            "joystick data format pointer owned by genuine DLL",
        );
        FreeLibrary(carrier);
        FreeLibrary(genuine);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_thin_loader() -> i32 {
    let args: Vec<_> = std::env::args_os().collect();
    match args.as_slice() {
        [_, mode, carrier] if mode == "pe" => pe_test(Path::new(carrier)),
        [_, mode, carrier] if mode == "forward" => forward_test(Path::new(carrier)),
        [_, mode, carrier, runtime, adapter] => scenario::startup_test(
            mode.to_str()
                .unwrap_or_else(|| fail("scenario name is ASCII")),
            Path::new(carrier),
            Path::new(runtime),
            Path::new(adapter),
        ),
        _ => fail("usage: thin_loader_test <mode> <carrier> [runtime adapter]"),
    }
    println!("PASS thin loader contract");
    0
}
