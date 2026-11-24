// SPDX-License-Identifier: MIT
//! The guarded MinHook batches (native_hooks.cpp's HookSet). The C detour leaves keep a batch in their
//! `rsf_anomaly_hook_set` and call the `rsf_anomaly_hooks_*` entries below. Keep trampolines alive for the process:
//! disabling a detour does not retire a callback or a caller of its original trampoline.
use crate::engine_guards::guarded_copy;
use std::ffi::c_void;
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};
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
const PE32_PLUS: u16 = 0x20b;

type Address = *mut c_void;

// The records native_hooks.h declares, bound in crate::sys.
pub(crate) use crate::sys::{rsf_anomaly_hook_site as Site, rsf_anomaly_hook_target as Target};

/// MinHook's batch operations: `MINHOOK` in production, recording fakes in the rollback tests.
pub(crate) struct HookOps {
    pub(crate) initialize: unsafe fn() -> bool,
    pub(crate) create: unsafe fn(Address, Address, *mut Address) -> bool,
    // A never enabled or queued entry has no disable step.
    pub(crate) remove_unqueued: unsafe fn(Address),
    pub(crate) queue: unsafe fn(Address, bool) -> bool,
    pub(crate) apply: unsafe fn() -> bool,
}

#[cfg(windows)]
pub(crate) const MINHOOK: HookOps = {
    use rsf_minhook_sys::*;
    HookOps {
        initialize: || {
            matches!(
                unsafe { MH_Initialize() },
                MH_OK | MH_ERROR_ALREADY_INITIALIZED
            )
        },
        create:
            |target, detour, original| unsafe { MH_CreateHook(target, detour, original) } == MH_OK,
        remove_unqueued: |target| {
            unsafe { MH_RemoveHook(target) };
        },
        queue: |target, enable| {
            let status = unsafe {
                if enable {
                    MH_QueueEnableHook(target)
                } else {
                    MH_QueueDisableHook(target)
                }
            };
            status == MH_OK
        },
        apply: || unsafe { MH_ApplyQueued() } == MH_OK,
    }
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

/// Reads `size` bytes at `address` of the image at `owner`; `executable` asks for code.
pub(crate) type ReadImage = unsafe fn(Address, *const c_void, *mut c_void, usize, bool) -> bool;

/// Checks that a read is wholly inside one readable region of the intended image, then copies it through the
/// guarded copy, which still contains a fault if the protection changes.
#[cfg(windows)]
unsafe fn read_mapped(
    owner: Address,
    address: *const c_void,
    out: *mut c_void,
    size: usize,
    executable: bool,
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
        || region.AllocationBase != owner
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
    contained && readable && unsafe { guarded_copy(out, address, size) }
}

/// The guarded copy without the region check (observer preparation).
pub(crate) unsafe fn read_guarded(
    _: Address,
    address: *const c_void,
    out: *mut c_void,
    size: usize,
    _: bool,
) -> bool {
    unsafe { guarded_copy(out, address, size) }
}

// The caller selects a PE header record whose fields accept every bit pattern.
#[cfg(windows)]
unsafe fn read_header<T: Default>(read: ReadImage, module: Address, address: Address) -> Option<T> {
    let mut header = T::default();
    let out = ptr::from_mut(&mut header).cast();
    unsafe { read(module, address, out, size_of::<T>(), false) }.then_some(header)
}

/// An AMD64 PE32+ image of `image_size` bytes whose NT headers start at most `max_nt_offset` bytes in.
#[cfg(windows)]
pub(crate) fn image_matches(
    read: ReadImage,
    module: Address,
    image_size: u32,
    max_nt_offset: usize,
) -> bool {
    const { assert!(size_of::<IMAGE_NT_HEADERS64>() == PE_HEADER_BYTES) };
    if module.is_null() {
        return false;
    }
    let Some(dos) = (unsafe { read_header::<IMAGE_DOS_HEADER>(read, module, module) }) else {
        return false;
    };
    let Ok(offset) = usize::try_from(dos.e_lfanew) else {
        return false;
    };
    if dos.e_magic != DOS_SIGNATURE
        || offset > max_nt_offset
        || module.addr().checked_add(offset).is_none()
    {
        return false;
    }
    unsafe { read_header::<IMAGE_NT_HEADERS64>(read, module, module.wrapping_byte_add(offset)) }
        .is_some_and(|pe| {
            pe.Signature == PE_SIGNATURE
                && pe.FileHeader.Machine == crate::AMD64
                && pe.OptionalHeader.Magic == PE32_PLUS
                && pe.OptionalHeader.SizeOfImage == image_size
        })
}

#[cfg(windows)]
fn preflight(
    read: ReadImage,
    module: Address,
    image_size: u32,
    sites: &[Site],
    targets: &mut [Target],
) -> bool {
    if (image_size as usize) < MIN_IMAGE_BYTES
        || !image_matches(
            read,
            module,
            image_size,
            image_size as usize - PE_HEADER_BYTES,
        )
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
            read(
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
    fn create(ops: &HookOps, sites: &[Target]) -> Option<Box<Self>> {
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
        if !unsafe { (ops.initialize)() } {
            return None;
        }
        let mut batch = Box::new(Self {
            targets: [ptr::null_mut(); MAX_SITES],
            count: sites.len(),
            enabled: false,
        });
        if !unsafe { ops.create_unqueued(sites) } {
            return None;
        }
        for (index, site) in sites.iter().enumerate() {
            batch.targets[index] = site.address;
        }
        Some(batch)
    }

    fn enable(&mut self, ops: &HookOps) -> bool {
        if !self.enabled {
            self.enabled = enable_batch(&self.targets[..self.count], ops, || {});
        }
        self.enabled
    }

    fn disable(&mut self, ops: &HookOps) -> bool {
        self.enabled = false;
        disable_batch(&self.targets[..self.count], ops)
    }
}

// The caller holds the MinHook transaction through activation and rollback.
pub(super) fn enable_batch(
    targets: &[Address],
    ops: &HookOps,
    before_rollback: impl FnOnce(),
) -> bool {
    let queued = targets
        .iter()
        .all(|&target| unsafe { (ops.queue)(target, true) });
    if queued && unsafe { (ops.apply)() } {
        return true;
    }
    before_rollback();
    // Apply may have redirected a suspended thread into a trampoline even when
    // patching failed. Cancel and undo the batch without removing its originals.
    disable_batch(targets, ops);
    false
}

fn disable_batch(targets: &[Address], ops: &HookOps) -> bool {
    let mut queued_all = true;
    for &target in targets {
        // Reset the rest of the queue even if one target has no entry.
        queued_all &= unsafe { (ops.queue)(target, false) };
    }
    let applied = unsafe { (ops.apply)() };
    queued_all && applied
}

// MH_ApplyQueued drains the MinHook instance's entire queue. Hold this lock
// from the first queue operation through apply and its rollback, across all sets.
static TRANSACTION: Mutex<()> = Mutex::new(());

pub(super) fn transaction() -> MutexGuard<'static, ()> {
    crate::lock(&TRANSACTION)
}

/// One batch: the `rsf_anomaly_hook_set` of a C leaf, or a Rust owner's static. Null until created.
pub(crate) type HookSet = AtomicPtr<c_void>;

fn install_targets(set: &HookSet, ops: &HookOps, sites: &[Target]) -> bool {
    let Some(created) = Batch::create(ops, sites) else {
        return false;
    };
    // The adapter pins itself before installation. The batch and the original
    // slots are process-lifetime once the batch can become reachable.
    let batch = Box::leak(created);
    set.store(ptr::from_mut(batch).cast(), Ordering::Release);
    batch.enable(ops)
}

#[cfg(windows)]
fn install_with(
    set: &HookSet,
    ops: &HookOps,
    read: ReadImage,
    module: Address,
    image_size: u32,
    sites: &[Site],
) -> bool {
    let _guard = transaction();
    if created(set) || !(1..=MAX_SITES).contains(&sites.len()) {
        return false;
    }
    let mut targets = [Target::default(); MAX_SITES];
    let targets = &mut targets[..sites.len()];
    preflight(read, module, image_size, sites, targets) && install_targets(set, ops, targets)
}

fn install_resolved_with(set: &HookSet, ops: &HookOps, sites: &[Target]) -> bool {
    let _guard = transaction();
    !created(set) && (1..=MAX_SITES).contains(&sites.len()) && install_targets(set, ops, sites)
}

fn set_enabled_with(set: &HookSet, ops: &HookOps, enable: bool) -> bool {
    let _guard = transaction();
    // Only this module creates batches, and only under the transaction.
    match (
        unsafe { set.load(Ordering::Acquire).cast::<Batch>().as_mut() },
        enable,
    ) {
        (Some(batch), true) => batch.enable(ops),
        (Some(batch), false) => batch.disable(ops),
        (None, _) => !enable,
    }
}

pub(crate) fn created(set: &HookSet) -> bool {
    !set.load(Ordering::Acquire).is_null()
}

/// Verifies the image headers and every site's mapped code before the first detour is written.
#[cfg(windows)]
pub(crate) fn install(set: &HookSet, module: Address, image_size: u32, sites: &[Site]) -> bool {
    install_with(set, &MINHOOK, read_mapped, module, image_size, sites)
}

#[cfg(windows)]
pub(crate) fn set_enabled(set: &HookSet, enable: bool) -> bool {
    set_enabled_with(set, &MINHOOK, enable)
}

// The C entries. `set` is the caller's zero-initialized rsf_anomaly_hook_set, whose one pointer is the batch.
#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hooks_install(
    set: &HookSet,
    module: Address,
    image_size: u32,
    sites: *const Site,
    count: usize,
) -> bool {
    // The caller lends `count` records for this call.
    (1..=MAX_SITES).contains(&count)
        && !sites.is_null()
        && install(set, module, image_size, unsafe {
            slice::from_raw_parts(sites, count)
        })
}

/// Only for immediate-context D3D11 methods resolved through their COM vtable.
#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_hooks_install_resolved(
    set: &HookSet,
    sites: *const Target,
    count: usize,
) -> bool {
    (1..=MAX_SITES).contains(&count)
        && !sites.is_null()
        && install_resolved_with(set, &MINHOOK, unsafe {
            slice::from_raw_parts(sites, count)
        })
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_hooks_enable(set: &HookSet) -> bool {
    set_enabled(set, true)
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_hooks_disable(set: &HookSet) -> bool {
    set_enabled(set, false)
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_hooks_created(set: &HookSet) -> bool {
    created(set)
}

#[cfg(test)]
mod tests;
