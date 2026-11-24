// SPDX-License-Identifier: MIT
//! Fallout 4 AE CPU event layout: the discovered hook, caller and global sites (discovery.rs), the
//! structure offsets the discovered patterns fix, and the guarded input-state snapshot. The
//! evidence is in games/fallout4/engine.json and docs/research/fallout4-*.
use crate::discovery::{Site, rva, verify_all};
use crate::{image, load, read};

// Hooked functions, in RSF_FALLOUT4_CPU_HOOK_* order.
pub(super) const HOOKS: [Site; 10] = [
    Site::Swap,
    Site::Keyboard,
    Site::Mouse,
    Site::Idle,
    Site::InputPoll,
    Site::Consume,
    Site::PlayerControls,
    Site::JobEnd,
    Site::Wrapper,
    Site::WorkerJoin,
];

// The return addresses that select each callback are the call sites' discovered sites.
pub(super) const PEEK_CALLERS: [Site; 4] = [Site::Peek1, Site::Peek2, Site::Peek3, Site::Peek4];

// Structure offsets. The manager's polling flag is fixed by the poll entry pattern, the worker's
// mode (+68) and completion event (+60) by the join pattern; the rest are documented in
// games/fallout4/engine.json.
const MANAGER_POLLING: usize = 0x35;
const MAIN_FLAGS: usize = 0x24;
const MAIN_MENU: usize = 0x1d0;
pub(super) const MAIN_WINDOW: usize = 0x30;
pub(super) const WORKER_START: usize = 0x58;
pub(super) const WORKER_COMPLETE: usize = 0x60;
pub(super) const WORKER_MODE: usize = 0x68;
pub(super) const WORKER_EXITING: usize = 0x6c;
pub(super) const WORKER_RUNNING: usize = 0x6e;
// The queue's enqueue sequence and the receiver's consumed cursor, fixed by the consume and
// enqueue patterns.
pub(super) const QUEUE_SEQUENCE: usize = 0x1490;
pub(super) const RECEIVER_CURSOR: usize = 8;
/// The native frame counter in the graphics state, fixed by the native frame read.
pub(super) const NATIVE_FRAME: usize = 0x98;
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
    if !load(base + rva(Site::Main), &mut value.main) {
        return StateRead::MainRead;
    }
    if value.main != main {
        return StateRead::MainMatch;
    }
    if !load(base + rva(Site::InputManager), &mut value.manager) {
        return StateRead::ManagerRead;
    }
    if value.manager == 0 {
        return StateRead::ManagerNull;
    }
    if !load(value.manager + MANAGER_POLLING, &mut value.polling_enabled) {
        return StateRead::PollingRead;
    }
    if value.polling_enabled == 0 {
        return StateRead::PollingDisabled;
    }
    if !load(base + rva(Site::EventQueue), &mut value.queue) {
        return StateRead::QueueRead;
    }
    if value.queue == 0 {
        return StateRead::QueueNull;
    }
    if !load(base + rva(Site::Controls), &mut value.receiver) {
        return StateRead::ReceiverRead;
    }
    if value.receiver == 0 {
        return StateRead::ReceiverNull;
    }
    let mut vtable = 0usize;
    if !load(value.receiver, &mut vtable) {
        return StateRead::VtableRead;
    }
    if vtable != base + rva(Site::ControlsVtable) {
        return StateRead::VtableMatch;
    }
    if !load(value.main + MAIN_FLAGS, &mut value.flags) {
        return StateRead::FlagsRead;
    }
    if !load(value.main + MAIN_MENU, &mut value.menu) {
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
        && read::<usize>(base + rva(Site::EventQueue))
            .is_some_and(|current| current != 0 && current == queue)
        && read::<usize>(base + rva(Site::Controls))
            .is_some_and(|current| current != 0 && current == receiver)
        && read::<usize>(receiver).is_some_and(|vtable| vtable == base + rva(Site::ControlsVtable))
}

/// The delivered event list holds a gamepad button or thumbstick event. Walks at most 256 nodes
/// and stops at the first unreadable or unknown node.
pub(super) fn controller_delivered(events: usize) -> bool {
    let mut node = events;
    let mut count = 0u32;
    while node != 0 && count < 256 {
        let (mut device, mut kind, mut next) = (0i32, 0i32, 0usize);
        if !load(node + 8, &mut device)
            || !load(node + 0x10, &mut kind)
            || !load(node + 0x18, &mut next)
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

/// The worker's job, class and entries, every hooked entry and every selecting call site.
pub(super) fn guards() -> bool {
    verify_all(&HOOKS)
        && verify_all(&[
            Site::JobName,
            Site::WorkerDestructor,
            Site::WorkerEntry,
            Site::JobCallback,
            Site::WrapperCall,
            Site::WorkerConsumeCall,
            Site::JoinCall,
            Site::SwapCall,
            Site::KeyboardCall,
            Site::MouseCall,
            Site::MainPollCall,
            Site::ConsumeCall,
            Site::IdleCall,
            Site::WrapperPollCall,
            Site::WorkerPollCall,
        ])
}

/// The PeekMessageA call sites and their import, and the PlayerControls and queue dispatch
/// instructions the receipts depend on. Both callers first check guards(), including the
/// PlayerControls hook entry and the consume body.
pub(super) fn notification_guards() -> bool {
    verify_all(&PEEK_CALLERS)
        && verify_all(&[
            Site::PeekImport,
            Site::PlayerControlsCall,
            Site::ControlsEvents,
            Site::ControlsKind,
            Site::EnqueueSequence,
            Site::DeviceEnqueue1,
            Site::DeviceEnqueue2,
            Site::QueueDevice,
            Site::DeviceEnqueue3,
            Site::DeviceEnqueue4,
            Site::QueueKind,
            Site::QueueAppend,
            Site::GamepadState,
        ])
}
