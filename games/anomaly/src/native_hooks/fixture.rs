// SPDX-License-Identifier: MIT
use super::*;
use crate::engine_guards::{NativeSite, Read};
use std::ffi::CStr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    LibraryLoader::GetModuleHandleW,
    Memory::{
        MEM_COMMIT, MEM_DECOMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE,
        PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY,
        PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect, VirtualQuery,
    },
    SystemInformation::{GetSystemInfo, SYSTEM_INFO},
    Threading::GetCurrentProcess,
};

type Function = unsafe extern "C" fn(i32) -> i32;

struct Flight {
    entered: SyncSender<()>,
    resume: Receiver<()>,
}

static FLIGHT: Mutex<Option<Flight>> = Mutex::new(None);
static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

extern "C" fn replacement(value: i32) -> i32 {
    let flight = FLIGHT.lock().unwrap().take();
    if let Some(flight) = flight {
        let _ = flight.entered.send(());
        let _ = flight.resume.recv();
    }
    unsafe { call(ORIGINAL.load(Ordering::Acquire), value) + 3 }
}

unsafe fn call(address: Address, value: i32) -> i32 {
    let function: Function = unsafe { std::mem::transmute(address) };
    unsafe { function(value) }
}

struct Image(Address);

impl Image {
    fn new(size: u32, protection: u32) -> Self {
        let base = unsafe {
            VirtualAlloc(
                ptr::null(),
                size as usize,
                MEM_RESERVE | MEM_COMMIT,
                protection,
            )
        };
        assert!(!base.is_null(), "allocate synthetic image");
        Self(base)
    }

    fn at(&self, rva: u32) -> Address {
        self.0.wrapping_byte_add(rva as usize)
    }

    fn write_headers(&self, size: u32) -> *mut IMAGE_DOS_HEADER {
        let dos = self.0.cast::<IMAGE_DOS_HEADER>();
        let mut pe = IMAGE_NT_HEADERS64 {
            Signature: PE_SIGNATURE,
            ..Default::default()
        };
        pe.FileHeader.Machine = AMD64;
        pe.OptionalHeader.Magic = PE32_PLUS;
        pe.OptionalHeader.SizeOfImage = size;
        unsafe {
            dos.write(IMAGE_DOS_HEADER {
                e_magic: DOS_SIGNATURE,
                e_lfanew: 64,
                ..Default::default()
            });
            self.at(64).cast::<IMAGE_NT_HEADERS64>().write(pe);
        }
        dos
    }

    fn protect(&self, rva: u32, size: u32, protection: u32) {
        let mut old = 0;
        assert_ne!(
            unsafe { VirtualProtect(self.at(rva), size as usize, protection, &mut old) },
            0
        );
    }

    fn flush(&self, size: u32) {
        assert_ne!(
            unsafe { FlushInstructionCache(GetCurrentProcess(), self.0, size as usize) },
            0
        );
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { VirtualFree(self.0, 0, MEM_RELEASE) };
    }
}

fn page_size() -> u32 {
    let mut info = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut info) };
    info.dwPageSize
}

fn bytes(address: Address) -> [u8; GUARD_BYTES] {
    unsafe { address.cast::<[u8; GUARD_BYTES]>().read() }
}

fn copy_contract(copy: Read, page: u32) {
    let inaccessible = Image::new(page, PAGE_NOACCESS);
    let original = 0x73u8;
    let mut copied = 0u8;
    let out = ptr::from_mut(&mut copied).cast();
    let from = ptr::from_ref(&original).cast();
    assert!(unsafe { copy(out, from, 1) });
    assert_eq!(copied, original);
    unsafe { SetLastError(0x525346) };
    let input_refused = !unsafe { copy(out, inaccessible.0, 1) };
    let output_refused = !unsafe { copy(inaccessible.0, from, 1) };
    let last_error = unsafe { GetLastError() };
    assert!(input_refused && output_refused);
    if cfg!(target_env = "gnu") {
        assert_eq!(last_error, 0x525346);
    }
}

fn fixture_site(module: Address, function: Function, original: *mut Address) -> Site {
    let address = function as Address;
    Site {
        _name: c"test executable fixture".as_ptr(),
        rva: (address.addr() - module.addr()).try_into().unwrap(),
        expected: bytes(address),
        replacement: replacement as Address,
        original,
    }
}

fn install(
    batch: &mut Address,
    platform: &Platform,
    base: Address,
    size: u32,
    sites: &[Site],
) -> bool {
    unsafe {
        rsf_anomaly_hook_set_install(batch, platform, base, size, sites.as_ptr(), sites.len())
    }
}

fn enabled(batch: &mut Address, platform: &Platform, enable: bool) -> bool {
    unsafe { rsf_anomaly_hook_set_set_enabled(batch, platform, enable) }
}

fn executable_contract(platform: &Platform, target: Function, untouched: Function) {
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let mut dos = DosFacts::default();
    let mut pe = PeFacts::default();
    assert!(unsafe { (platform.headers.dos)(module, &mut dos) });
    assert!(unsafe {
        (platform.headers.pe)(
            module,
            module.wrapping_byte_add(dos.nt_offset as usize),
            &mut pe,
        )
    });
    let mut original = ptr::null_mut();
    let mut site = fixture_site(module, target, &raw mut original);
    let entry = target as Address;
    let expected = site.expected;
    let mut batch = ptr::null_mut();
    site.expected[GUARD_BYTES - 1] ^= 1;
    assert!(!install(
        &mut batch,
        platform,
        module,
        pe.image_size,
        slice::from_ref(&site)
    ));
    assert!(batch.is_null() && original.is_null());
    assert_eq!(unsafe { target(2) }, 9);
    assert_eq!(bytes(entry), expected);
    site.expected = expected;
    assert!(!install(
        &mut batch,
        platform,
        module,
        pe.image_size,
        &[site, site]
    ));
    assert!(!install(
        &mut batch,
        platform,
        module,
        pe.image_size,
        &[
            site,
            Site {
                rva: pe.image_size,
                ..site
            }
        ],
    ));
    assert_eq!(unsafe { target(2) }, 9);
    assert_eq!(bytes(entry), expected);
    assert!(install(
        &mut batch,
        platform,
        module,
        pe.image_size,
        &[site]
    ));
    assert!(!batch.is_null() && !original.is_null());
    ORIGINAL.store(original, Ordering::Release);
    assert_eq!(unsafe { target(2) }, 12);
    assert_eq!(unsafe { call(original, 2) }, 9);
    let trampoline = original;

    let (entered, wait_enter) = sync_channel(0);
    let (resume_detour, resume) = sync_channel(0);
    *FLIGHT.lock().unwrap() = Some(Flight { entered, resume });
    let thread = std::thread::spawn(move || unsafe { target(2) });
    wait_enter.recv().unwrap();
    let restored = enabled(&mut batch, platform, false);
    resume_detour.send(()).unwrap();
    assert_eq!(thread.join().unwrap(), 12);
    assert!(restored);
    assert_eq!(original, trampoline);
    assert_eq!(unsafe { target(2) }, 9);
    assert_eq!(bytes(entry), expected);

    let mut fresh = ptr::null_mut();
    let mut conflict = ptr::null_mut();
    let mut other = ptr::null_mut();
    let new = fixture_site(module, untouched, &raw mut fresh);
    let already_registered = fixture_site(module, target, &raw mut conflict);
    assert!(!install(
        &mut other,
        platform,
        module,
        pe.image_size,
        &[new, already_registered]
    ));
    assert!(other.is_null() && fresh.is_null() && conflict.is_null());
    assert_eq!(original, trampoline);
    assert_eq!(bytes(untouched as Address), new.expected);
    let resolved = Target {
        address: untouched as Address,
        replacement: new.replacement,
        original: new.original,
    };
    assert!(unsafe { rsf_anomaly_hook_set_install_resolved(&mut other, platform, &resolved, 1) });
    assert!(!fresh.is_null() && enabled(&mut other, platform, false));
    assert_eq!(unsafe { call(fresh, 2) }, 21);
    assert!(enabled(&mut batch, platform, true));
    assert_eq!(original, trampoline);
    assert_eq!(unsafe { target(2) }, 12);
    assert!(enabled(&mut batch, platform, false));
    assert_eq!(unsafe { target(2) }, 9);
}

fn mapping_contract(platform: &Platform, page: u32) {
    let size = page * 3;
    let image = Image::new(size, PAGE_READWRITE);
    let dos = image.write_headers(size);
    let mut original = ptr::null_mut();
    let mut expected = [0x90; GUARD_BYTES];
    expected[..6].copy_from_slice(&[0x8b, 0xc1, 0x83, 0xc0, 0x07, 0xc3]);
    let valid = page + 64;
    let split = 2 * page - 8;
    for rva in [valid, split] {
        unsafe { image.at(rva).cast::<[u8; GUARD_BYTES]>().write(expected) };
    }
    image.flush(size);
    let site = Site {
        _name: c"mapped code fixture".as_ptr(),
        rva: valid,
        expected,
        replacement: replacement as Address,
        original: &raw mut original,
    };
    let mut batch = ptr::null_mut();
    assert!(!install(&mut batch, platform, image.0, size, &[site]));
    for protection in [PAGE_EXECUTE, PAGE_EXECUTE_READ | PAGE_GUARD, PAGE_NOACCESS] {
        image.protect(page, page, protection);
        assert!(!install(&mut batch, platform, image.0, size, &[site]));
        if protection & PAGE_GUARD != 0 {
            let mut info = MEMORY_BASIC_INFORMATION::default();
            assert_ne!(
                unsafe { VirtualQuery(image.at(page), &mut info, size_of_val(&info)) },
                0
            );
            assert_ne!(info.Protect & PAGE_GUARD, 0);
        }
    }
    image.protect(page, page, PAGE_EXECUTE_READ);
    unsafe { (*dos).e_lfanew = (page - 1) as i32 };
    assert!(!install(&mut batch, platform, image.0, size, &[site]));
    unsafe { (*dos).e_lfanew = 64 };
    image.protect(2 * page, page, PAGE_EXECUTE_READWRITE);
    assert!(!install(
        &mut batch,
        platform,
        image.0,
        size,
        &[Site { rva: split, ..site }]
    ));
    assert_ne!(
        unsafe { VirtualFree(image.at(2 * page), page as usize, MEM_DECOMMIT) },
        0
    );
    assert!(!install(
        &mut batch,
        platform,
        image.0,
        size,
        &[Site {
            rva: 2 * page,
            ..site
        }]
    ));
    assert!(!install(&mut batch, platform, image.at(1), size, &[site]));
    assert!(batch.is_null() && original.is_null());
    image.protect(0, page, PAGE_READONLY);
    assert!(install(&mut batch, platform, image.0, size, &[site]));
    assert!(!original.is_null() && enabled(&mut batch, platform, false));
    assert_eq!(unsafe { call(original, 2) }, 9);
    assert_eq!(bytes(image.at(valid)), expected);
}

#[repr(C)]
struct FixtureProfile {
    label: *const c_char,
    image_size: u32,
    sites: *const NativeSite,
    count: usize,
}

const _: () =
    assert!(size_of::<FixtureProfile>() == 32 && std::mem::offset_of!(FixtureProfile, sites) == 16);

fn profile_contract(platform: &Platform, profile: &FixtureProfile) {
    println!(
        "Anomaly HookSet profile: {}",
        unsafe { CStr::from_ptr(profile.label) }.to_string_lossy()
    );
    let size = profile.image_size;
    let image = Image::new(size, PAGE_EXECUTE_READWRITE);
    image.write_headers(size);
    let mut originals = vec![ptr::null_mut(); profile.count];
    let native_sites = unsafe { slice::from_raw_parts(profile.sites, profile.count) };
    let mut sites: Vec<_> = native_sites
        .iter()
        .zip(&mut originals)
        .map(|(site, original)| {
            unsafe {
                image
                    .at(site.rva)
                    .cast::<[u8; GUARD_BYTES]>()
                    .write(site.expected)
            };
            Site {
                _name: site.name,
                rva: site.rva,
                expected: site.expected,
                replacement: replacement as Address,
                original,
            }
        })
        .collect();
    image.flush(size);
    let mut batch = ptr::null_mut();
    for index in 0..sites.len() {
        sites[index].expected[GUARD_BYTES - 1] ^= 1;
        assert!(!install(&mut batch, platform, image.0, size, &sites) && batch.is_null());
        sites[index].expected[GUARD_BYTES - 1] ^= 1;
    }
    assert!(originals.iter().all(|address| address.is_null()));
    assert!(install(&mut batch, platform, image.0, size, &sites));
    for (site, original) in sites.iter().zip(&originals) {
        assert!(!original.is_null());
        assert_ne!(bytes(image.at(site.rva)), site.expected);
    }
    assert!(enabled(&mut batch, platform, false));
    for site in sites {
        assert_eq!(bytes(image.at(site.rva)), site.expected);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_anomaly_native_hooks(
    platform: &Platform,
    copy: Read,
    target: Function,
    untouched: Function,
    profiles: *const FixtureProfile,
    count: usize,
) -> i32 {
    std::panic::catch_unwind(|| {
        let page = page_size();
        copy_contract(copy, page);
        executable_contract(platform, target, untouched);
        mapping_contract(platform, page);
        for profile in unsafe { slice::from_raw_parts(profiles, count) } {
            profile_contract(platform, profile);
        }
        println!("Anomaly guarded native hooks: PASS");
        0
    })
    .unwrap_or(1)
}
