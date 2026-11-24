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
    dos: Option<DosFacts>,
    pe: Option<PeFacts>,
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
        Self {
            dos: Some(DosFacts {
                nt_offset: 64,
                signature: DOS_SIGNATURE,
            }),
            pe: Some(PeFacts {
                signature: PE_SIGNATURE,
                image_size: IMAGE,
                machine: AMD64,
                optional_magic: PE32_PLUS,
            }),
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

const PLATFORM: Platform = Platform {
    headers: ImageHeaders {
        dos: Some(dos),
        pe: Some(pe),
    },
    read: Some(read),
    hooks: HookOps {
        initialize: Some(initialize),
        create: Some(create),
        remove_unqueued: Some(remove),
        queue: Some(queue),
        apply: Some(apply),
    },
};

unsafe extern "C" fn dos(_: *const c_void, out: *mut DosFacts) -> bool {
    FAKE.with_borrow(|fake| {
        let Some(dos) = fake.dos else { return false };
        unsafe { out.write(dos) };
        true
    })
}

unsafe extern "C" fn pe(_: *const c_void, _: *const c_void, out: *mut PeFacts) -> bool {
    FAKE.with_borrow(|fake| {
        let Some(pe) = fake.pe else { return false };
        unsafe { out.write(pe) };
        true
    })
}

unsafe extern "C" fn read(
    _: *const c_void,
    _: *const c_void,
    out: Address,
    size: usize,
    executable: bool,
) -> bool {
    assert_eq!(size, CODE.len());
    assert!(executable);
    FAKE.with_borrow(|fake| {
        if fake.code_available {
            unsafe { out.cast::<[u8; GUARD_BYTES]>().write(CODE) };
        }
        fake.code_available
    })
}

unsafe extern "C" fn initialize() -> bool {
    FAKE.with_borrow_mut(|fake| {
        fake.calls.push(Operation::Initialize);
        fake.initialize
    })
}

unsafe extern "C" fn create(address: Address, _: Address, out: *mut Address) -> bool {
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

unsafe extern "C" fn remove(address: Address) {
    FAKE.with_borrow_mut(|fake| {
        fake.calls.push(Operation::Remove(address as usize));
        let hook = fake.hooks.remove(&(address as usize)).unwrap();
        assert!(!hook.enabled && !hook.queued);
    });
}

unsafe extern "C" fn queue(address: Address, enable: bool) -> bool {
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

unsafe extern "C" fn apply() -> bool {
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
struct Handle(Address);

impl Handle {
    fn install(&mut self, sites: &[Site]) -> bool {
        self.install_image(MODULE, IMAGE, sites)
    }

    fn install_image(&mut self, module: usize, size: u32, sites: &[Site]) -> bool {
        unsafe {
            rsf_anomaly_hook_set_install(
                &mut self.0,
                &PLATFORM,
                module as Address,
                size,
                sites.as_ptr(),
                sites.len(),
            )
        }
    }

    fn resolved(&mut self, targets: &[Target]) -> bool {
        unsafe {
            rsf_anomaly_hook_set_install_resolved(
                &mut self.0,
                &PLATFORM,
                targets.as_ptr(),
                targets.len(),
            )
        }
    }

    fn enabled(&mut self, enable: bool) -> bool {
        unsafe { rsf_anomaly_hook_set_set_enabled(&mut self.0, &PLATFORM, enable) }
    }

    fn created(&self) -> bool {
        unsafe { rsf_anomaly_hook_set_created(&self.0) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // The fake creates no executable trampolines.
        if !self.0.is_null() {
            drop(unsafe { Box::from_raw(self.0.cast::<Batch>()) });
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
    let mut batch = Handle::default();
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
    let mut batch = Handle::default();
    let failures: [fn(&mut Fake); 10] = [
        |f| f.dos = None,
        |f| f.dos.as_mut().unwrap().signature ^= 1,
        |f| f.dos.as_mut().unwrap().nt_offset = -1,
        |f| f.dos.as_mut().unwrap().nt_offset = (MIN_IMAGE_BYTES - PE_HEADER_BYTES + 1) as i32,
        |f| f.pe = None,
        |f| f.pe.as_mut().unwrap().signature ^= 1,
        |f| f.pe.as_mut().unwrap().machine ^= 1,
        |f| f.pe.as_mut().unwrap().optional_magic ^= 1,
        |f| f.pe.as_mut().unwrap().image_size += 1,
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
        fake.dos.as_mut().unwrap().nt_offset = (MIN_IMAGE_BYTES - PE_HEADER_BYTES) as i32;
    });
    assert!(!batch.install_image(usize::MAX - 1, IMAGE, &sites));
    FAKE.with_borrow_mut(|fake| fake.dos.as_mut().unwrap().nt_offset = 0);
    assert!(!batch.install_image(usize::MAX - 1, IMAGE, &sites));
    no_minhook_calls();
    FAKE.with_borrow_mut(|fake| {
        fake.dos.as_mut().unwrap().nt_offset = (MIN_IMAGE_BYTES - PE_HEADER_BYTES) as i32;
    });
    assert!(batch.install(&sites));
}

#[test]
fn creation_failure_removes_only_unqueued_members_and_allows_retry() {
    let first = MODULE + RVA as usize;
    let second = first + GUARD_BYTES;
    let mut originals = [ptr::null_mut(); 2];
    let sites = sites(&mut originals);
    let mut batch = Handle::default();

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
    let mut batch = Handle::default();
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
    let mut batch = Handle::default();
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
    let mut other = Handle::default();
    assert!(!other.resolved(&[draw, draw]));
    assert!(other.resolved(&[draw]));
    assert_eq!(detours(), [(false, false), (false, false), (true, true)]);
    assert_eq!(originals, published);
}

#[test]
fn failed_disable_apply_does_not_short_circuit_the_next_enable() {
    let mut originals = [ptr::null_mut(); 2];
    let sites = sites(&mut originals);
    let mut batch = Handle::default();
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
