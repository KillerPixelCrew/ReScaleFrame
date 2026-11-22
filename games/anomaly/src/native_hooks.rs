// SPDX-License-Identifier: MIT
use std::ffi::{c_char, c_void};
use std::ptr;
use std::slice;
use std::sync::{Mutex, MutexGuard, PoisonError};
#[cfg(windows)]
use windows_sys::Win32::System::{
    Diagnostics::Debug::IMAGE_NT_HEADERS64, SystemServices::IMAGE_DOS_HEADER,
};

#[cfg(all(windows, feature = "native-hook-fixture"))]
mod fixture;

const MAX_SITES: usize = 24;
const GUARD_BYTES: usize = 16;
const MIN_IMAGE_BYTES: usize = 4096;
const PE_HEADER_BYTES: usize = 264;
const DOS_SIGNATURE: u16 = 0x5a4d;
const PE_SIGNATURE: u32 = 0x4550;
const AMD64: u16 = 0x8664;
const PE32_PLUS: u16 = 0x20b;

type Address = *mut c_void;
type Queue = unsafe extern "C" fn(Address, bool) -> bool;
type Apply = unsafe extern "C" fn() -> bool;

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct Site {
    pub(super) _name: *const c_char,
    pub(super) rva: u32,
    pub(super) expected: [u8; GUARD_BYTES],
    pub(super) replacement: Address,
    pub(super) original: *mut Address,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(super) struct Target {
    pub(super) address: Address,
    pub(super) replacement: Address,
    pub(super) original: *mut Address,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct DosFacts {
    nt_offset: i32,
    signature: u16,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct PeFacts {
    signature: u32,
    image_size: u32,
    machine: u16,
    optional_magic: u16,
}

#[repr(C)]
pub(super) struct ImageHeaders {
    dos: unsafe extern "C" fn(Address, *mut DosFacts) -> bool,
    pe: unsafe extern "C" fn(Address, Address, *mut PeFacts) -> bool,
}

#[repr(C)]
struct Platform {
    headers: ImageHeaders,
    read: unsafe extern "C" fn(Address, Address, Address, usize, bool) -> bool,
    hooks: HookOps,
}

#[repr(C)]
pub(super) struct HookOps {
    pub(super) initialize: unsafe extern "C" fn() -> bool,
    pub(super) create: unsafe extern "C" fn(Address, Address, *mut Address) -> bool,
    pub(super) remove_unqueued: unsafe extern "C" fn(Address),
    pub(super) queue: Queue,
    pub(super) apply: Apply,
}

const _: () = {
    assert!(size_of::<DosFacts>() == 8 && size_of::<PeFacts>() == 12);
    assert!(size_of::<ImageHeaders>() == 16 && std::mem::offset_of!(ImageHeaders, pe) == 8);
    assert!(size_of::<HookOps>() == 40);
    assert!(size_of::<Platform>() == 64 && std::mem::offset_of!(Platform, hooks) == 24);
};

impl HookOps {
    // The caller holds the shared transaction and has validated the targets.
    pub(super) unsafe fn create_unqueued(&self, sites: &[Target]) -> bool {
        for (index, site) in sites.iter().enumerate() {
            if !unsafe { (self.create)(site.address, site.replacement, site.original) } {
                for prior in &sites[..index] {
                    unsafe {
                        (self.remove_unqueued)(prior.address);
                        prior.original.write(ptr::null_mut());
                    }
                }
                return false;
            }
        }
        true
    }
}

impl ImageHeaders {
    pub(super) fn matches(&self, module: Address, image_size: u32, max_nt_offset: usize) -> bool {
        if module.is_null() {
            return false;
        }
        let mut dos = DosFacts::default();
        if !unsafe { (self.dos)(module, &mut dos) } || dos.signature != DOS_SIGNATURE {
            return false;
        }
        let Ok(offset) = usize::try_from(dos.nt_offset) else {
            return false;
        };
        if offset > max_nt_offset || module.addr().checked_add(offset).is_none() {
            return false;
        }
        let mut pe = PeFacts::default();
        let read = unsafe { (self.pe)(module, module.wrapping_byte_add(offset), &mut pe) };
        read && pe.signature == PE_SIGNATURE
            && pe.machine == AMD64
            && pe.optional_magic == PE32_PLUS
            && pe.image_size == image_size
    }
}

fn preflight(
    platform: &Platform,
    module: Address,
    image_size: u32,
    sites: &[Site],
    targets: &mut [Target],
) -> bool {
    if (image_size as usize) < MIN_IMAGE_BYTES
        || !platform
            .headers
            .matches(module, image_size, image_size as usize - PE_HEADER_BYTES)
    {
        return false;
    }
    for (site, target) in sites.iter().zip(targets.iter_mut()) {
        let rva = site.rva as usize;
        if rva > image_size as usize - GUARD_BYTES || (module as usize).checked_add(rva).is_none() {
            return false;
        }
        *target = Target {
            address: module.wrapping_byte_add(rva),
            replacement: site.replacement,
            original: site.original,
        };
        let mut bytes = [0; GUARD_BYTES];
        if !unsafe {
            (platform.read)(
                module,
                target.address,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                true,
            )
        } || bytes != site.expected
        {
            return false;
        }
    }
    true
}

struct Batch {
    targets: [Address; MAX_SITES],
    count: usize,
    enabled: bool,
}

impl Batch {
    fn create(platform: &Platform, sites: &[Target]) -> Option<Box<Self>> {
        for (index, site) in sites.iter().enumerate() {
            if site.address.is_null()
                || site.replacement.is_null()
                || site.original.is_null()
                || sites[..index]
                    .iter()
                    .any(|other| other.address == site.address)
            {
                return None;
            }
        }
        if !unsafe { (platform.hooks.initialize)() } {
            return None;
        }
        let mut batch = Box::new(Self {
            targets: [ptr::null_mut(); MAX_SITES],
            count: sites.len(),
            enabled: false,
        });
        if !unsafe { platform.hooks.create_unqueued(sites) } {
            return None;
        }
        for (index, site) in sites.iter().enumerate() {
            batch.targets[index] = site.address;
        }
        Some(batch)
    }

    fn enable(&mut self, platform: &Platform) -> bool {
        if self.enabled {
            return true;
        }
        self.enabled = enable_batch(
            &self.targets[..self.count],
            platform.hooks.queue,
            platform.hooks.apply,
            || {},
        );
        self.enabled
    }

    fn disable(&mut self, platform: &Platform) -> bool {
        self.enabled = false;
        disable_batch(
            &self.targets[..self.count],
            platform.hooks.queue,
            platform.hooks.apply,
        )
    }
}

// The caller holds the MinHook transaction through activation and rollback.
pub(super) fn enable_batch(
    targets: &[Address],
    queue: Queue,
    apply: Apply,
    before_rollback: impl FnOnce(),
) -> bool {
    let queued = targets.iter().all(|&target| unsafe { queue(target, true) });
    if queued && unsafe { apply() } {
        return true;
    }
    before_rollback();
    // Apply may have redirected a suspended thread into a trampoline even when
    // patching failed. Cancel and undo the batch without removing its originals.
    disable_batch(targets, queue, apply);
    false
}

fn disable_batch(targets: &[Address], queue: Queue, apply: Apply) -> bool {
    let mut queued_all = true;
    for &target in targets {
        // Reset the rest of the queue even if one target has no entry.
        queued_all &= unsafe { queue(target, false) };
    }
    let applied = unsafe { apply() };
    queued_all && applied
}

// MH_ApplyQueued drains the MinHook instance's entire queue. Hold this lock
// from the first queue operation through apply and its rollback, across all sets.
static TRANSACTION: Mutex<()> = Mutex::new(());

pub(super) fn transaction() -> MutexGuard<'static, ()> {
    TRANSACTION.lock().unwrap_or_else(PoisonError::into_inner)
}

fn install(batch: &mut *mut c_void, platform: &Platform, sites: &[Target]) -> bool {
    let Some(created) = Batch::create(platform, sites) else {
        return false;
    };
    // The adapter pins itself before installation. The opaque handle and the
    // original slots are process-lifetime once the batch can become reachable.
    let retained = Box::into_raw(created);
    *batch = retained.cast();
    unsafe { (*retained).enable(platform) }
}

fn valid_count(count: usize) -> bool {
    (1..=MAX_SITES).contains(&count)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hook_set_install(
    batch: *mut *mut c_void,
    platform: &Platform,
    module: Address,
    image_size: u32,
    sites: *const Site,
    count: usize,
) -> bool {
    let _guard = transaction();
    // The native rsf_anomaly_hook_set owns this handle and borrows the caller's span for this call.
    let batch = unsafe { &mut *batch };
    if !batch.is_null() || !valid_count(count) || sites.is_null() {
        return false;
    }
    let sites = unsafe { slice::from_raw_parts(sites, count) };
    let mut targets = [Target::default(); MAX_SITES];
    let targets = &mut targets[..count];
    preflight(platform, module, image_size, sites, targets) && install(batch, platform, targets)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hook_set_install_resolved(
    batch: *mut *mut c_void,
    platform: &Platform,
    sites: *const Target,
    count: usize,
) -> bool {
    let _guard = transaction();
    let batch = unsafe { &mut *batch };
    if !batch.is_null() || !valid_count(count) || sites.is_null() {
        return false;
    }
    install(batch, platform, unsafe {
        slice::from_raw_parts(sites, count)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hook_set_set_enabled(
    batch: *mut *mut c_void,
    platform: &Platform,
    enable: bool,
) -> bool {
    let _guard = transaction();
    let batch = unsafe { (*batch).cast::<Batch>().as_mut() };
    match (batch, enable) {
        (Some(batch), true) => batch.enable(platform),
        (Some(batch), false) => batch.disable(platform),
        (None, _) => !enable,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hook_set_created(batch: *const *mut c_void) -> bool {
    let _guard = transaction();
    !unsafe { *batch }.is_null()
}

/// Check that a read is wholly inside one readable region of the intended
/// image. The platform copy still contains faults if protection changes.
#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hook_read_mapped(
    owner: *const c_void,
    address: *const c_void,
    out: *mut c_void,
    size: usize,
    executable: bool,
    copy: crate::engine_guards::Read,
) -> bool {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
        PAGE_WRITECOPY, VirtualQuery,
    };

    if address.is_null() || size == 0 {
        return false;
    }
    let mut region = MEMORY_BASIC_INFORMATION::default();
    if unsafe { VirtualQuery(address, &mut region, size_of::<MEMORY_BASIC_INFORMATION>()) } == 0
        || region.State != MEM_COMMIT
        || region.AllocationBase.cast_const() != owner
        || region.Protect & (PAGE_GUARD | PAGE_NOACCESS) != 0
    {
        return false;
    }
    let contained = address
        .addr()
        .checked_sub(region.BaseAddress.addr())
        .and_then(|offset| offset.checked_add(size))
        .is_some_and(|end| end <= region.RegionSize);
    let readable = match region.Protect & 0xff {
        PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY => true,
        PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY => !executable,
        _ => false,
    };
    contained && readable && unsafe { copy(out, address, size) }
}

#[cfg(windows)]
// The caller selects a PE header record whose fields accept every bit pattern.
unsafe fn read_header<T: Default>(
    owner: *const c_void,
    address: *const c_void,
    mapped: bool,
    copy: crate::engine_guards::Read,
) -> Option<T> {
    let mut header = T::default();
    let out = ptr::from_mut(&mut header).cast();
    let complete = if mapped {
        unsafe { rsf_anomaly_hook_read_mapped(owner, address, out, size_of::<T>(), false, copy) }
    } else {
        unsafe { copy(out, address, size_of::<T>()) }
    };
    complete.then_some(header)
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hook_read_dos(
    module: *const c_void,
    out: &mut DosFacts,
    mapped: bool,
    copy: crate::engine_guards::Read,
) -> bool {
    let Some(header): Option<IMAGE_DOS_HEADER> =
        (unsafe { read_header(module, module, mapped, copy) })
    else {
        return false;
    };
    *out = DosFacts {
        nt_offset: header.e_lfanew,
        signature: header.e_magic,
    };
    true
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hook_read_pe(
    module: *const c_void,
    address: *const c_void,
    out: &mut PeFacts,
    mapped: bool,
    copy: crate::engine_guards::Read,
) -> bool {
    const { assert!(size_of::<IMAGE_NT_HEADERS64>() == PE_HEADER_BYTES) };
    let Some(header): Option<IMAGE_NT_HEADERS64> =
        (unsafe { read_header(module, address, mapped, copy) })
    else {
        return false;
    };
    *out = PeFacts {
        signature: header.Signature,
        image_size: header.OptionalHeader.SizeOfImage,
        machine: header.FileHeader.Machine,
        optional_magic: header.OptionalHeader.Magic,
    };
    true
}

#[cfg(test)]
mod tests;
