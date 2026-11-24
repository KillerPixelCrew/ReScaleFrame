// SPDX-License-Identifier: MIT
use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};

const MODULE: usize = 0x10000;
const IMAGE: u32 = MIN_IMAGE_BYTES as u32;
const RVA: u32 = 0x800;
const CODE: [u8; GUARD_BYTES] = [0x90; GUARD_BYTES];

#[derive(Default)]
struct Hook {
    enabled: bool,
    queued: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Initialize,
    Create(usize),
    Remove(usize),
    Queue(usize, bool),
    Apply,
}

struct Fake {
    dos: Option<IMAGE_DOS_HEADER>,
    pe: Option<IMAGE_NT_HEADERS64>,
    code_available: bool,
    initialize: bool,
    create_failure: Option<usize>,
    queue_failure: Option<(usize, bool)>,
    apply_failures: VecDeque<usize>,
    hooks: BTreeMap<usize, Hook>,
    calls: Vec<Operation>,
}

impl Default for Fake {
    fn default() -> Self {
        let mut pe = IMAGE_NT_HEADERS64 {
            Signature: PE_SIGNATURE,
            ..Default::default()
        };
        pe.FileHeader.Machine = crate::AMD64;
        pe.OptionalHeader.Magic = PE32_PLUS;
        pe.OptionalHeader.SizeOfImage = IMAGE;
        Self {
            dos: Some(IMAGE_DOS_HEADER {
                e_magic: DOS_SIGNATURE,
                e_lfanew: 64,
                ..Default::default()
            }),
            pe: Some(pe),
            code_available: true,
            initialize: true,
            create_failure: None,
            queue_failure: None,
            apply_failures: VecDeque::new(),
            hooks: BTreeMap::new(),
            calls: Vec::new(),
        }
    }
}

thread_local! {
    static FAKE: RefCell<Fake> = RefCell::new(Fake::default());
}

const OPS: HookOps = HookOps {
    initialize,
    create,
    remove_unqueued: remove,
    queue,
    apply,
};

// The headers sit at the module base and its NT offset; any executable read is site code.
unsafe fn read(
    module: Address,
    address: *const c_void,
    out: *mut c_void,
    size: usize,
    code: bool,
) -> bool {
    FAKE.with_borrow(|fake| unsafe {
        if code {
            assert_eq!(size, CODE.len());
            if fake.code_available {
                out.cast::<[u8; GUARD_BYTES]>().write(CODE);
            }
            return fake.code_available;
        }
        let nt = |dos: IMAGE_DOS_HEADER| module.addr().wrapping_add(dos.e_lfanew as usize);
        match (fake.dos, fake.pe) {
            (Some(dos), _) if size == size_of_val(&dos) && address == module.cast_const() => {
                out.cast::<IMAGE_DOS_HEADER>().write(dos);
                true
            }
            (Some(dos), Some(pe)) if size == size_of_val(&pe) && address.addr() == nt(dos) => {
                out.cast::<IMAGE_NT_HEADERS64>().write(pe);
                true
            }
            _ => false,
        }
    })
}

unsafe fn initialize() -> bool {
    FAKE.with_borrow_mut(|fake| {
        fake.calls.push(Operation::Initialize);
        fake.initialize
    })
}

unsafe fn create(address: Address, _: Address, out: *mut Address) -> bool {
    let address = address as usize;
    FAKE.with_borrow_mut(|fake| {
        fake.calls.push(Operation::Create(address));
        if fake.create_failure == Some(address) || fake.hooks.contains_key(&address) {
            return false;
        }
        fake.hooks.insert(address, Hook::default());
        unsafe { out.write((address + 0x100000) as Address) };
        true
    })
}

unsafe fn remove(address: Address) {
    FAKE.with_borrow_mut(|fake| {
        fake.calls.push(Operation::Remove(address as usize));
        let hook = fake.hooks.remove(&(address as usize)).unwrap();
        assert!(!hook.enabled && !hook.queued);
    });
}

unsafe fn queue(address: Address, enable: bool) -> bool {
    let address = address as usize;
    FAKE.with_borrow_mut(|fake| {
        fake.calls.push(Operation::Queue(address, enable));
        if fake.queue_failure == Some((address, enable)) {
            fake.queue_failure = None;
            return false;
        }
        let Some(hook) = fake.hooks.get_mut(&address) else {
            return false;
        };
        hook.queued = enable;
        true
    })
}

unsafe fn apply() -> bool {
    FAKE.with_borrow_mut(|fake| {
        fake.calls.push(Operation::Apply);
        let failure = fake.apply_failures.pop_front();
        for (i, hook) in fake
            .hooks
            .values_mut()
            .filter(|hook| hook.enabled != hook.queued)
            .enumerate()
        {
            if failure == Some(i) {
                return false;
            }
            hook.enabled = hook.queued;
        }
        true
    })
}

#[derive(Default)]
struct Handle(HookSet);

impl Handle {
    fn install(&self, sites: &[Site]) -> bool {
        self.install_image(MODULE, IMAGE, sites)
    }

    fn install_image(&self, module: usize, size: u32, sites: &[Site]) -> bool {
        install_with(&self.0, &OPS, read, module as Address, size, sites)
    }

    fn resolved(&self, targets: &[Target]) -> bool {
        install_resolved_with(&self.0, &OPS, targets)
    }

    fn enabled(&self, enable: bool) -> bool {
        set_enabled_with(&self.0, &OPS, enable)
    }

    fn created(&self) -> bool {
        created(&self.0)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // The fake creates no executable trampolines.
        let batch = *self.0.get_mut();
        if !batch.is_null() {
            drop(unsafe { Box::from_raw(batch.cast::<Batch>()) });
        }
    }
}

fn sites(originals: &mut [Address]) -> Vec<Site> {
    originals
        .iter_mut()
        .enumerate()
        .map(|(i, original)| Site {
            name: ptr::null(),
            rva: RVA + (i * GUARD_BYTES) as u32,
            expected: CODE,
            replacement: ptr::dangling_mut(),
            original,
        })
        .collect()
}

fn no_minhook_calls() {
    FAKE.with_borrow(|fake| assert!(fake.calls.is_empty()));
}

fn detours() -> Vec<(bool, bool)> {
    FAKE.with_borrow(|fake| {
        fake.hooks
            .values()
            .map(|hook| (hook.enabled, hook.queued))
            .collect()
    })
}

#[test]
fn exact_preflight_limits_and_late_guard_refusals() {
    let mut originals = [ptr::null_mut(); MAX_SITES + 1];
    let mut sites = sites(&mut originals);
    let batch = Handle::default();
    assert!(!batch.enabled(true));
    assert!(batch.enabled(false));
    assert!(!batch.install(&[]));
    assert!(!batch.install(&sites));
    assert!(!batch.install_image(0, IMAGE, &sites[..1]));
    assert!(!batch.install_image(MODULE, IMAGE - 1, &sites[..1]));

    let last = MAX_SITES - 1;
    let valid_rva = IMAGE - GUARD_BYTES as u32;
    sites[last].rva = valid_rva + 1;
    assert!(!batch.install(&sites[..MAX_SITES]));
    sites[last].rva = sites[0].rva;
    assert!(!batch.install(&sites[..MAX_SITES]));
    sites[last].rva = valid_rva;
    for byte in 0..GUARD_BYTES {
        sites[last].expected[byte] ^= 1;
        assert!(!batch.install(&sites[..MAX_SITES]));
        sites[last].expected[byte] ^= 1;
    }
    let original = sites[last].original;
    sites[last].original = ptr::null_mut();
    assert!(!batch.install(&sites[..MAX_SITES]));
    sites[last].original = original;
    sites[last].replacement = ptr::null_mut();
    assert!(!batch.install(&sites[..MAX_SITES]));
    sites[last].replacement = ptr::dangling_mut();
    FAKE.with_borrow_mut(|fake| fake.code_available = false);
    assert!(!batch.install(&sites[..MAX_SITES]));
    FAKE.with_borrow_mut(|fake| fake.code_available = true);
    no_minhook_calls();
    assert!(!batch.created());
    assert!(originals.iter().all(|p| p.is_null()));

    assert!(batch.install(&sites[..MAX_SITES]));
    assert!(batch.created());
    assert_eq!(detours(), vec![(true, true); MAX_SITES]);
    assert!(!batch.install(&sites[..1]));
    assert!(batch.enabled(false));
}

#[test]
fn dos_and_pe_headers_refuse_before_minhook() {
    let mut originals = [ptr::null_mut()];
    let sites = sites(&mut originals);
    let batch = Handle::default();
    let failures: [fn(&mut Fake); 10] = [
        |f| f.dos = None,
        |f| f.dos.as_mut().unwrap().e_magic ^= 1,
        |f| f.dos.as_mut().unwrap().e_lfanew = -1,
        |f| f.dos.as_mut().unwrap().e_lfanew = (MIN_IMAGE_BYTES - PE_HEADER_BYTES + 1) as i32,
        |f| f.pe = None,
        |f| f.pe.as_mut().unwrap().Signature ^= 1,
        |f| f.pe.as_mut().unwrap().FileHeader.Machine ^= 1,
        |f| f.pe.as_mut().unwrap().OptionalHeader.Magic ^= 1,
        |f| f.pe.as_mut().unwrap().OptionalHeader.SizeOfImage += 1,
        |f| f.code_available = false,
    ];
    for fail in failures {
        FAKE.with_borrow_mut(|fake| {
            *fake = Fake::default();
            fail(fake);
        });
        assert!(!batch.install(&sites));
        no_minhook_calls();
    }
    FAKE.with_borrow_mut(|fake| {
        *fake = Fake::default();
        fake.dos.as_mut().unwrap().e_lfanew = (MIN_IMAGE_BYTES - PE_HEADER_BYTES) as i32;
    });
    assert!(!batch.install_image(usize::MAX - 1, IMAGE, &sites));
    FAKE.with_borrow_mut(|fake| fake.dos.as_mut().unwrap().e_lfanew = 0);
    assert!(!batch.install_image(usize::MAX - 1, IMAGE, &sites));
    no_minhook_calls();
    FAKE.with_borrow_mut(|fake| {
        fake.dos.as_mut().unwrap().e_lfanew = (MIN_IMAGE_BYTES - PE_HEADER_BYTES) as i32;
    });
    assert!(batch.install(&sites));
}

#[test]
fn creation_failure_removes_only_unqueued_members_and_allows_retry() {
    let first = MODULE + RVA as usize;
    let second = first + GUARD_BYTES;
    let mut originals = [ptr::null_mut(); 2];
    let sites = sites(&mut originals);
    let batch = Handle::default();

    FAKE.with_borrow_mut(|fake| fake.initialize = false);
    assert!(!batch.install(&sites));
    FAKE.with_borrow_mut(|fake| {
        assert_eq!(fake.calls, [Operation::Initialize]);
        fake.calls.clear();
        fake.initialize = true;
        fake.create_failure = Some(second);
    });
    assert!(!batch.install(&sites));
    assert!(!batch.created());
    assert_eq!(originals, [ptr::null_mut(); 2]);
    FAKE.with_borrow_mut(|fake| {
        assert!(fake.hooks.is_empty());
        assert_eq!(
            fake.calls,
            [
                Operation::Initialize,
                Operation::Create(first),
                Operation::Create(second),
                Operation::Remove(first),
            ]
        );
        fake.create_failure = None;
    });
    assert!(batch.install(&sites));
}

#[test]
fn failed_enable_queue_cancels_prior_members_without_releasing_trampolines() {
    let mut originals = [ptr::null_mut(); 2];
    let sites = sites(&mut originals);
    let batch = Handle::default();
    FAKE.with_borrow_mut(|fake| {
        fake.queue_failure = Some((MODULE + RVA as usize + GUARD_BYTES, true));
    });
    assert!(!batch.install(&sites));
    assert!(batch.created());
    assert_eq!(detours(), [(false, false), (false, false)]);
    let published = originals;
    assert!(published.iter().all(|p| !p.is_null()));
    assert!(batch.enabled(true));
    assert!(batch.enabled(false));
    assert_eq!(originals, published);
}

#[test]
fn partial_apply_retains_originals_and_another_batch_cannot_enable_the_failed_set() {
    let mut originals = [ptr::null_mut(); 2];
    let sites = sites(&mut originals);
    let batch = Handle::default();
    FAKE.with_borrow_mut(|fake| fake.apply_failures.extend([1, 0]));
    assert!(!batch.install(&sites));
    assert!(batch.created());
    assert_eq!(detours(), [(true, false), (false, false)]);
    let published = originals;
    assert!(published.iter().all(|p| !p.is_null()));
    let mut draw_original = ptr::null_mut();
    let draw = Target {
        address: (MODULE + IMAGE as usize) as Address,
        replacement: ptr::dangling_mut(),
        original: &mut draw_original,
    };
    let other = Handle::default();
    assert!(!other.resolved(&[draw, draw]));
    assert!(other.resolved(&[draw]));
    assert_eq!(detours(), [(false, false), (false, false), (true, true)]);
    assert_eq!(originals, published);
}

#[test]
fn failed_disable_apply_does_not_short_circuit_the_next_enable() {
    let mut originals = [ptr::null_mut(); 2];
    let sites = sites(&mut originals);
    let batch = Handle::default();
    assert!(batch.install(&sites));
    let published = originals;
    FAKE.with_borrow_mut(|fake| fake.apply_failures.push_back(1));
    assert!(!batch.enabled(false));
    assert_eq!(detours(), [(false, false), (true, false)]);
    assert!(batch.enabled(true));
    assert_eq!(detours(), [(true, true), (true, true)]);
    assert_eq!(originals, published);
    assert!(batch.enabled(false));
    assert_eq!(detours(), [(false, false), (false, false)]);

    FAKE.with_borrow_mut(|fake| fake.apply_failures.push_back(1));
    assert!(!batch.enabled(true));
    assert_eq!(detours(), [(false, false), (false, false)]);
    assert_eq!(originals, published);
}
