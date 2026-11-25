// SPDX-License-Identifier: MIT
//! Holds the game's D3D11CreateDeviceAndSwapChain until the plugin session has started.
//!
//! The session starts on a worker because the Runtime and the adapter cannot load under the loader
//! lock, and the game's main thread runs on meanwhile. An adapter scopes the game's one device
//! creation by hooking the import thunk at start, so a game that reached creation first created
//! its device unscoped. Process attach therefore points the executable's import slot at a gate;
//! that is memory writes only. The first creation call waits there for the session start, or
//! runs it when the worker has not begun, puts the original import back and re-enters the import
//! thunk with the game's own stack, so a hook on the thunk sees the game's call and caller.
use rsf_game_sys::signature::{EXECUTE, find_rip_reference, import_slot, mapped_sections, views};
use rsf_game_sys::win32::read_process;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{PAGE_PROTECTION_FLAGS, PAGE_READWRITE, VirtualProtect};

/// Address of the gated import slot; zero when nothing is gated.
static SLOT: AtomicUsize = AtomicUsize::new(0);
/// The import the gate displaced.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static ARMED: AtomicU8 = AtomicU8::new(NO_IMPORT);
/// The gate's continuation, stored by `arm`. The naked gate is kept in every binary that links this
/// archive, so it calls through this pointer: a direct reference would keep the plugin start, and
/// with it the executable hash, in carriers that never arm the gate.
static RESOLVE: AtomicUsize = AtomicUsize::new(0);

const NO_IMPORT: u8 = 0;
const HELD: u8 = 1;
const REFUSED: u8 = 2;

/// `jmp [rip+slot]`: the import thunk an adapter hooks.
const THUNK: [u8; 2] = [0xff, 0x25];

/// Points the slot at the gate and returns the displaced import; None for an unbound slot.
fn take(cell: &AtomicUsize, gate: usize) -> Option<usize> {
    let original = cell.load(Ordering::Acquire);
    (original != 0
        && original != gate
        && cell
            .compare_exchange(original, gate, Ordering::AcqRel, Ordering::Acquire)
            .is_ok())
    .then_some(original)
}

/// Puts the original back only where the gate still sits: false when another call already did,
/// or a later hook chained over the gate and reached it as its original.
fn reopen(cell: &AtomicUsize, gate: usize, original: usize) -> bool {
    cell.compare_exchange(gate, original, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// Where a call that reached the gate continues, and whether it will reach the gate again. A
/// held call re-enters the thunk. When the slot could not be reopened (a later hook chained over
/// the gate) that re-entry returns here through the chain and continues at the displaced import;
/// so does a game without a unique thunk.
fn release(
    returning: bool,
    reopened: bool,
    thunk: Option<usize>,
    original: usize,
) -> (usize, bool) {
    match thunk {
        Some(thunk) if !returning => (thunk, !reopened),
        _ => (original, false),
    }
}

/// Runs `write` on the slot made writable, restoring its protection after.
fn writable<T>(slot: usize, write: impl FnOnce(&AtomicUsize) -> T) -> Option<T> {
    let mut previous: PAGE_PROTECTION_FLAGS = 0;
    let address = slot as *const c_void;
    if unsafe { VirtualProtect(address, size_of::<usize>(), PAGE_READWRITE, &mut previous) } == 0 {
        return None;
    }
    // SAFETY: an import address table cell is an aligned, now writable pointer slot.
    let result = write(unsafe { &*(slot as *const AtomicUsize) });
    let mut ignored = 0;
    unsafe { VirtualProtect(address, size_of::<usize>(), previous, &mut ignored) };
    Some(result)
}

fn gate_address() -> usize {
    gate as *const () as usize
}

fn executable() -> usize {
    unsafe { GetModuleHandleW(null()) as usize }
}

/// The executable's D3D11CreateDeviceAndSwapChain import slot RVA.
fn slot_rva(base: usize) -> Option<usize> {
    import_slot(
        |rva, out| unsafe { read_process(base + rva, out.as_mut_ptr().cast(), out.len()) },
        "d3d11.dll",
        "D3D11CreateDeviceAndSwapChain",
    )
}

/// Process attach, under the loader lock: the executable's imports are bound and its code has not
/// run yet. Nothing here loads a module.
pub(super) fn arm() {
    let base = executable();
    let Some(rva) = (base != 0).then(|| slot_rva(base)).flatten() else {
        return;
    };
    let slot = base + rva;
    RESOLVE.store(resolve as usize, Ordering::Release);
    let held = writable(slot, |cell| take(cell, gate_address())).flatten();
    if let Some(original) = held {
        ORIGINAL.store(original, Ordering::Release);
        SLOT.store(slot, Ordering::Release);
    }
    ARMED.store(
        if held.is_some() { HELD } else { REFUSED },
        Ordering::Release,
    );
}

/// The arming outcome for the carrier log.
pub(super) fn report() -> Option<&'static str> {
    match ARMED.load(Ordering::Acquire) {
        HELD => Some(
            "creation gate: D3D11CreateDeviceAndSwapChain held until the plugin session has started",
        ),
        REFUSED => Some(
            "creation gate refused: import slot unbound or not writable; device creation is not held",
        ),
        _ => None,
    }
}

/// The unique import thunk through the slot, found in the code once it runs (decrypted).
fn thunk(base: usize, slot: usize) -> Option<usize> {
    let code = mapped_sections(base, read_process, |_, flags| flags & EXECUTE != 0)?;
    find_rip_reference(&views(&code), &THUNK, slot - base)
        .ok()
        .map(|rva| base + rva)
}

thread_local! {
    /// The next gate entry on this thread is a released call returning through a chained hook.
    static RETURNING: Cell<bool> = const { Cell::new(false) };
}

/// The gate's continuation once the session start has completed.
extern "C" fn resolve() -> usize {
    let original = ORIGINAL.load(Ordering::Acquire);
    std::panic::catch_unwind(|| {
        let returning = RETURNING.replace(false);
        if !returning {
            super::plugin::start();
        }
        let slot = SLOT.load(Ordering::Acquire);
        let reopened = !returning
            && writable(slot, |cell| reopen(cell, gate_address(), original)).unwrap_or(false);
        static THUNK: OnceLock<Option<usize>> = OnceLock::new();
        let thunk = *THUNK.get_or_init(|| {
            let thunk = thunk(executable(), slot);
            super::plugin::note(if thunk.is_some() {
                b"creation gate released: the game's creation re-enters its import thunk"
            } else {
                b"creation gate released: no unique import thunk; the game's creation continues natively"
            });
            thunk
        });
        let (target, returns) = release(returning, reopened, thunk, original);
        RETURNING.set(returns);
        target
    })
    .unwrap_or(original)
}

/// Bound in the import slot. The game's register arguments are saved around `resolve` and its
/// stack arguments and return address stay untouched, so the jump continues the game's own call.
#[unsafe(naked)]
unsafe extern "system" fn gate() {
    std::arch::naked_asm!(
        "sub rsp, 0x88",
        "mov [rsp + 0x20], rcx",
        "mov [rsp + 0x28], rdx",
        "mov [rsp + 0x30], r8",
        "mov [rsp + 0x38], r9",
        "movdqu xmmword ptr [rsp + 0x40], xmm0",
        "movdqu xmmword ptr [rsp + 0x50], xmm1",
        "movdqu xmmword ptr [rsp + 0x60], xmm2",
        "movdqu xmmword ptr [rsp + 0x70], xmm3",
        "call qword ptr [rip + {resolve}]",
        "mov rcx, [rsp + 0x20]",
        "mov rdx, [rsp + 0x28]",
        "mov r8, [rsp + 0x30]",
        "mov r9, [rsp + 0x38]",
        "movdqu xmm0, xmmword ptr [rsp + 0x40]",
        "movdqu xmm1, xmmword ptr [rsp + 0x50]",
        "movdqu xmm2, xmmword ptr [rsp + 0x60]",
        "movdqu xmm3, xmmword ptr [rsp + 0x70]",
        "add rsp, 0x88",
        "jmp rax",
        resolve = sym RESOLVE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const GATE: usize = 0x1000;
    const IMPORT: usize = 0x2000;
    const THUNK_AT: usize = 0x3000;

    #[test]
    fn the_held_call_reenters_the_thunk_through_the_reopened_slot() {
        let cell = AtomicUsize::new(IMPORT);
        assert_eq!(take(&cell, GATE), Some(IMPORT));
        assert_eq!(cell.load(Ordering::Relaxed), GATE);
        let reopened = reopen(&cell, GATE, IMPORT);
        assert!(reopened);
        assert_eq!(cell.load(Ordering::Relaxed), IMPORT);
        // The thunk now jumps through the original import and never returns to the gate.
        assert_eq!(
            release(false, reopened, Some(THUNK_AT), IMPORT),
            (THUNK_AT, false)
        );
    }

    #[test]
    fn a_call_through_a_chained_hook_reenters_the_thunk_once() {
        // A later hook chained over the gate keeps its slot.
        let chained = AtomicUsize::new(0x4000);
        assert!(!reopen(&chained, GATE, IMPORT));
        assert_eq!(chained.load(Ordering::Relaxed), 0x4000);
        // The thunk is re-entered and its chain brings the call back, which then continues.
        assert_eq!(
            release(false, false, Some(THUNK_AT), IMPORT),
            (THUNK_AT, true)
        );
        assert_eq!(
            release(true, false, Some(THUNK_AT), IMPORT),
            (IMPORT, false)
        );
        // No unique thunk: the original import.
        assert_eq!(release(false, true, None, IMPORT), (IMPORT, false));
    }

    #[test]
    fn an_unbound_slot_is_not_held() {
        assert_eq!(take(&AtomicUsize::new(0), GATE), None);
        assert_eq!(take(&AtomicUsize::new(GATE), GATE), None);
    }
}
