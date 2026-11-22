// SPDX-License-Identifier: MIT
//! Fallout 4 AE 1.11.240 CPU event layout: hook and caller RVAs, the expected instruction bytes,
//! and the guarded input-state snapshot. Every value belongs to the exact recognized build; the
//! evidence is in games/fallout4/engine.json and docs/research/fallout4-*.
use crate::{Plain, image, read};
use std::ffi::c_void;

// Hooked functions, in RSF_FALLOUT4_CPU_HOOK_* order.
pub(super) const HOOK_ENTRIES: [(usize, &[u8]); 10] = [
    (
        0xc33250,
        &[
            0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x81, 0xec, 0x40, 0x01, 0x00, 0x00,
        ],
    ),
    (
        0x1675140,
        &[0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x6c, 0x24, 0x18],
    ),
    (0x16754d0, &[0x0f, 0xb7, 0x42, 0x1c, 0xa8, 0x01, 0x74, 0x08]),
    (
        0xc339d0,
        &[
            0x48, 0x8b, 0xc4, 0x48, 0x89, 0x58, 0x08, 0x48, 0x89, 0x70, 0x10,
        ],
    ),
    (0x1672540, &[0x48, 0x83, 0xec, 0x38, 0x80, 0x79, 0x35, 0x00]),
    (
        0x1673ae0,
        &[
            0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x89, 0x6c, 0x24, 0x30,
        ],
    ),
    (
        0xddbe50,
        &[0x48, 0x8b, 0xc4, 0x56, 0x57, 0x48, 0x83, 0xec, 0x68],
    ),
    (
        0xbd6850,
        &[0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x30],
    ),
    (0xc34eb0, &[0x48, 0x83, 0xec, 0x28, 0xf3, 0x0f, 0x10, 0x0d]),
    (
        0xbd51c0,
        &[
            0x48, 0x8b, 0x0d, 0x29, 0xc1, 0x6c, 0x02, 0x48, 0x85, 0xc9, 0x74, 0x20,
        ],
    ),
];

// Exact return addresses that select each callback.
pub(super) const SWAP_CALLER: usize = 0xc346b1;
pub(super) const KEYBOARD_CALLER: usize = 0xc3a992;
pub(super) const MOUSE_CALLER: usize = 0xc3a970;
pub(super) const IDLE_CALLER: usize = 0xc315c0;
pub(super) const MAIN_POLL_CALLER: usize = 0xc342ba;
pub(super) const WRAPPER_POLL_CALLER: usize = 0xc34ec8;
pub(super) const WORKER_POLL_CALLER: usize = 0xbd7653;
pub(super) const CONSUME_CALLER: usize = 0xc342de;
pub(super) const WORKER_CONSUME_CALLER: usize = 0xc34eec;
pub(super) const PLAYER_CONTROLS_CALLER: usize = 0x1673b74;
pub(super) const JOB_CALLER: usize = 0xbd76df;
pub(super) const WRAPPER_CALLER: usize = 0xbd6945;
pub(super) const JOIN_CALLER: usize = 0xc346c0;
pub(super) const PEEK_CALLERS: [usize; 4] = [0xc338ea, 0xc3391e, 0xc3398b, 0xc339c1];
// USER32 PeekMessageA import cell.
pub(super) const PEEK_SLOT: usize = 0x2440a98;

// Globals and their layouts.
pub(super) const MAIN: usize = 0x32dd130;
const MANAGER: usize = 0x3e75cb8;
pub(super) const QUEUE: usize = 0x3273ae0;
pub(super) const RECEIVER: usize = 0x31f0b80;
pub(super) const RECEIVER_VTABLE: usize = 0x2571a30;
pub(super) const WORKER: usize = 0x32a12f0;
pub(super) const WORKER_OBJECT: usize = 0x32a1320;
pub(super) const WORKER_VTABLE: usize = 0x255e3b8;
pub(super) const WORKER_DESTRUCTOR: usize = 0xbd72e0;
pub(super) const WORKER_ENTRY: usize = 0xbd7430;
pub(super) const NATIVE_FRAME: usize = 0x3d87238;
const MANAGER_POLLING: usize = 0x35;
const MAIN_FLAGS: usize = 0x24;
const MAIN_MENU: usize = 0x1d0;
pub(super) const MAIN_WINDOW: usize = 0x30;
pub(super) const WORKER_START: usize = 0x58;
pub(super) const WORKER_COMPLETE: usize = 0x60;
pub(super) const WORKER_MODE: usize = 0x68;
pub(super) const WORKER_EXITING: usize = 0x6c;
pub(super) const WORKER_RUNNING: usize = 0x6e;
// The queue's enqueue sequence and the receiver's consumed cursor.
pub(super) const QUEUE_SEQUENCE: usize = 0x1490;
pub(super) const RECEIVER_CURSOR: usize = 8;

/// Loads `address` into `slot`, leaving it unchanged when the read fails.
pub(super) fn load<T: Plain>(slot: &mut T, address: usize) -> bool {
    match read::<T>(address) {
        Some(value) => {
            *slot = value;
            true
        }
        None => false,
    }
}

/// `expected` matches the image bytes at `rva`.
pub(super) fn bytes(rva: usize, expected: &[u8]) -> bool {
    crate::matches_bytes::<16>(image() + rva, expected)
}

/// The eligible Main/input owner snapshot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct InputState {
    pub main: usize,
    pub manager: usize,
    pub queue: usize,
    pub receiver: usize,
    pub flags: [u8; 8],
    pub menu: u8,
    pub polling_enabled: u8,
}

impl InputState {
    pub const ZERO: Self = Self {
        main: 0,
        manager: 0,
        queue: 0,
        receiver: 0,
        flags: [0; 8],
        menu: 0,
        polling_enabled: 0,
    };
}

/// How far the snapshot read reached; `Eligible` is the only admitting stage.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum StateRead {
    NotCalled,
    IdleMain,
    MainRead,
    MainMatch,
    ManagerRead,
    ManagerNull,
    PollingRead,
    PollingDisabled,
    QueueRead,
    QueueNull,
    ReceiverRead,
    ReceiverNull,
    VtableRead,
    VtableMatch,
    FlagsRead,
    MenuRead,
    MainFlags,
    Menu,
    Eligible,
}

pub(super) const STATE_NAMES: [&str; 19] = [
    "not-called",
    "idle-main-missing",
    "main-read",
    "main-association",
    "manager-read",
    "manager-null",
    "polling-read",
    "polling-disabled",
    "queue-read",
    "queue-null",
    "receiver-read",
    "receiver-null",
    "vtable-read",
    "vtable-association",
    "flags-read",
    "menu-read",
    "main-eligibility",
    "menu",
    "eligible",
];

/// Reads the snapshot for `main` (nonzero) in the original read order and short circuit.
pub(super) fn state(value: &mut InputState, main: usize) -> StateRead {
    if main == 0 {
        return StateRead::IdleMain;
    }
    let base = image();
    if !load(&mut value.main, base + MAIN) {
        return StateRead::MainRead;
    }
    if value.main != main {
        return StateRead::MainMatch;
    }
    if !load(&mut value.manager, base + MANAGER) {
        return StateRead::ManagerRead;
    }
    if value.manager == 0 {
        return StateRead::ManagerNull;
    }
    if !load(&mut value.polling_enabled, value.manager + MANAGER_POLLING) {
        return StateRead::PollingRead;
    }
    if value.polling_enabled == 0 {
        return StateRead::PollingDisabled;
    }
    if !load(&mut value.queue, base + QUEUE) {
        return StateRead::QueueRead;
    }
    if value.queue == 0 {
        return StateRead::QueueNull;
    }
    if !load(&mut value.receiver, base + RECEIVER) {
        return StateRead::ReceiverRead;
    }
    if value.receiver == 0 {
        return StateRead::ReceiverNull;
    }
    let mut vtable = 0usize;
    if !load(&mut vtable, value.receiver) {
        return StateRead::VtableRead;
    }
    if vtable != base + RECEIVER_VTABLE {
        return StateRead::VtableMatch;
    }
    if !load(&mut value.flags, value.main + MAIN_FLAGS) {
        return StateRead::FlagsRead;
    }
    if !load(&mut value.menu, value.main + MAIN_MENU) {
        return StateRead::MenuRead;
    }
    let flags = value.flags;
    if flags[0] != 0
        || flags[1] != 0
        || flags[2] != 0
        || flags[3] == 0
        || flags[5] != 0
        || flags[6] != 0
        || flags[7] != 0
    {
        return StateRead::MainFlags;
    }
    if value.menu != 0 {
        return StateRead::Menu;
    }
    StateRead::Eligible
}

/// The queue and receiver globals still name this delivery.
pub(super) fn delivery_current(enabled: bool, queue: usize, receiver: usize) -> bool {
    let base = image();
    enabled
        && read::<usize>(base + QUEUE).is_some_and(|current| current != 0 && current == queue)
        && read::<usize>(base + RECEIVER).is_some_and(|current| current != 0 && current == receiver)
        && read::<usize>(receiver).is_some_and(|vtable| vtable == base + RECEIVER_VTABLE)
}

/// The delivered event list holds a gamepad button or thumbstick event. Walks at most 256 nodes
/// and stops at the first unreadable or unknown node.
pub(super) fn controller_delivered(events: usize) -> bool {
    let mut node = events;
    let mut count = 0u32;
    while node != 0 && count < 256 {
        let (mut device, mut kind, mut next) = (0i32, 0i32, 0usize);
        if !load(&mut device, node + 8)
            || !load(&mut kind, node + 0x10)
            || !load(&mut next, node + 0x18)
            || !(0..=2).contains(&device)
            || !(0..=6).contains(&kind)
        {
            return false;
        }
        if device == 2 && (kind == 0 || kind == 4) {
            return true;
        }
        node = next;
        count += 1;
    }
    false
}

/// The worker class, every hooked entry and every selecting call site.
pub(super) fn guards() -> bool {
    const CHECKS: [(usize, &[u8]); 27] = [
        HOOK_ENTRIES[7], // worker job end
        HOOK_ENTRIES[8], // worker poll/consume wrapper
        HOOK_ENTRIES[9], // post-Swap worker join
        (
            0xbd76d2,
            &[
                0x48, 0x8b, 0x47, 0x10, 0x48, 0x85, 0xc0, 0x74, 0x04, 0x33, 0xc9, 0xff, 0x10,
            ],
        ), // callback
        (0xbd6940, &[0xe8, 0x6b, 0xe5, 0x05, 0x00]), // wrapper call
        (0xc34ee7, &[0xe8, 0xf4, 0xeb, 0xa3, 0x00]), // worker consume
        (0xc346bb, &[0xe8, 0x00, 0x0b, 0xfa, 0xff]), // join call
        (
            0xbd51cc,
            &[
                0x83, 0x79, 0x68, 0x01, 0x75, 0x1a, 0xc7, 0x41, 0x68, 0x00, 0x00, 0x00, 0x00,
            ],
        ), // join branch
        (
            0xbd51d9,
            &[
                0x45, 0x33, 0xc0, 0x48, 0x8b, 0x49, 0x60, 0xba, 0xff, 0xff, 0xff, 0xff,
            ],
        ), // join arguments
        (0xbd51e5, &[0x48, 0xff, 0x25, 0x2c, 0xb1, 0x86, 0x01]), // join wait
        HOOK_ENTRIES[0], // Swap
        (0xc346ac, &[0xe8, 0x9f, 0xeb, 0xff, 0xff]), // Swap call
        HOOK_ENTRIES[1], // keyboard RAWINPUT
        HOOK_ENTRIES[2], // mouse RAWINPUT
        (0xc3a98d, &[0xe8, 0xae, 0xa7, 0xa3, 0x00]), // keyboard call
        (0xc3a96b, &[0xe8, 0x60, 0xab, 0xa3, 0x00]), // mouse call
        HOOK_ENTRIES[3], // Main idle
        HOOK_ENTRIES[4], // BSInputDeviceManager poll
        HOOK_ENTRIES[5], // input event queue consume
        (0xc342b5, &[0xe8, 0x86, 0xe2, 0xa3, 0x00]), // poll call
        (0xc342d9, &[0xe8, 0x02, 0xf8, 0xa3, 0x00]), // consume call
        (0xc315bb, &[0xe8, 0x10, 0x24, 0x00, 0x00]), // idle call
        (0xc315b4, &[0x48, 0x8b, 0x0d, 0x75, 0xbb, 0x6a, 0x02]), // idle argument
        (0xc342ae, &[0x48, 0x8b, 0x0d, 0x03, 0x1a, 0x24, 0x03]), // main poll argument
        (0xc34ec3, &[0xe8, 0x78, 0xd6, 0xa3, 0x00]), // wrapper poll
        (0xbd764e, &[0xe8, 0xed, 0xae, 0xa9, 0x00]), // worker poll
        HOOK_ENTRIES[crate::cpu_abi::RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS as usize],
    ];
    // The worker job descriptor and name, then the worker class locator, destructor and entry.
    let base = image();
    read::<usize>(base + 0x2f30fc0) == Some(base + HOOK_ENTRIES[7].0)
        && read::<usize>(base + 0x2f30fc8) == Some(base + 0x255dd10)
        && read::<usize>(base + 0x255e3b0) == Some(base + 0x29c88d8)
        && read::<usize>(base + WORKER_VTABLE) == Some(base + WORKER_DESTRUCTOR)
        && read::<usize>(base + 0x255e3c0) == Some(base + WORKER_ENTRY)
        && CHECKS.iter().all(|&(rva, expected)| bytes(rva, expected))
}

/// The PeekMessageA call sites and their import, and the PlayerControls and queue dispatch
/// instructions the receipts depend on. Both callers first check guards(), including the
/// PlayerControls hook entry.
pub(super) fn notification_guards() -> bool {
    const CHECKS: [(usize, &[u8]); 34] = [
        (0xc338e4, &[0xff, 0x15, 0xae, 0xd1, 0x80, 0x01]),
        (0xc33918, &[0xff, 0x15, 0x7a, 0xd1, 0x80, 0x01]),
        (0xc33985, &[0xff, 0x15, 0x0d, 0xd1, 0x80, 0x01]),
        (0xc339bb, &[0xff, 0x15, 0xd7, 0xd0, 0x80, 0x01]),
        (0xc338d3, &[0xc7, 0x44, 0x24, 0x20, 0x01, 0x00, 0x00, 0x00]),
        (0xc33907, &[0xc7, 0x44, 0x24, 0x20, 0x01, 0x00, 0x00, 0x00]),
        (0xc33959, &[0xc7, 0x44, 0x24, 0x20, 0x01, 0x00, 0x00, 0x00]),
        (0xc339a9, &[0xc7, 0x44, 0x24, 0x20, 0x01, 0x00, 0x00, 0x00]),
        (0xc338ea, &[0x85, 0xc0, 0x74, 0x34]),
        (0xc3391e, &[0x85, 0xc0, 0x75, 0xce]),
        (0xc3398b, &[0x85, 0xc0, 0x74, 0x36]),
        (0xc339c1, &[0x85, 0xc0, 0x75, 0xcb]),
        (0xddbffb, &[0x48, 0x8b, 0x5b, 0x18]),
        (0xddc44c, &[0x48, 0x63, 0x45, 0x10]),
        (0x1673aeb, &[0x48, 0x8d, 0x99, 0x78, 0x14, 0x00, 0x00]),
        (0x1673b00, &[0xe8, 0xfb, 0x58, 0xfe, 0xff]),
        (0x1673b30, &[0xe8, 0xcb, 0x58, 0xfe, 0xff]),
        (0x1673b64, &[0xe8, 0x67, 0x5b, 0xfe, 0xff]),
        (0x1673b90, &[0xe9, 0x3b, 0x5b, 0xfe, 0xff]),
        (0x1673b35, &[0x48, 0x8b, 0xbd, 0x80, 0x14, 0x00, 0x00]),
        (0x1673b48, &[0x39, 0x47, 0x20]),
        (
            0x1673b53,
            &[0x8b, 0x85, 0x90, 0x14, 0x00, 0x00, 0x89, 0x46, 0x08],
        ),
        (
            0x1673b69,
            &[
                0x48, 0x8b, 0x06, 0x48, 0x8b, 0xd7, 0x48, 0x8b, 0xce, 0xff, 0x10,
            ],
        ),
        (
            0x167465c,
            &[
                0x8b, 0x86, 0x90, 0x14, 0x00, 0x00, 0x89, 0x47, 0x20, 0xff, 0x86, 0x90, 0x14, 0x00,
                0x00,
            ],
        ),
        (0x167466b, &[0x48, 0xc7, 0x47, 0x18, 0x00, 0x00, 0x00, 0x00]),
        (0x16778a2, &[0xe8, 0x39, 0xe7, 0xff, 0xff]),
        (0x16760df, &[0xe8, 0x1c, 0xdb, 0xff, 0xff]),
        (0x1673d1c, &[0x44, 0x89, 0x7f, 0x08]),
        (0x1677a5f, &[0xe8, 0x0c, 0xc7, 0xff, 0xff]),
        (0x1677b02, &[0xe8, 0x69, 0xc6, 0xff, 0xff]),
        (0x1674205, &[0x83, 0x78, 0x10, 0x04]),
        (0x16742a4, &[0xc7, 0x47, 0x08, 0x02, 0x00, 0x00, 0x00]),
        (0x16742d4, &[0xe8, 0x37, 0x03, 0x00, 0x00]),
        (0x1677bb7, &[0xff, 0x25, 0x53, 0x93, 0xdc, 0x00]),
    ];
    if !CHECKS.iter().all(|&(rva, expected)| bytes(rva, expected)) {
        return false;
    }
    let base = image();
    read::<u32>(base + 0x2ed1098) == Some(0x2ed1d60)
        && read::<u32>(base + 0x2ed10a4) == Some(0x2ed3736)
        && read::<u32>(base + 0x2ed10a8) == Some(0x2440a70)
        && read::<u64>(base + 0x2ed1d88) == Some(0x2ed35fe)
        && bytes(0x2ed3736, b"USER32.dll\0")
        && bytes(0x2ed3600, b"PeekMessageA\0")
}
