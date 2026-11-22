// SPDX-License-Identifier: MIT
// Shared D3D11/native D3D12 presentation owner and typed DXGI facade.
// This module decides admission and
// publication, provider switching, frame and slot sequencing, accepted Present,
// fallback, resize and retirement, status and callback order, and latency routing.
use crate::com_ref::ComRef;
use crate::d3d11_host_abi::{
    RSF_D3D11_DISPATCH_PRESENT, RSF_D3D11_DISPATCH_PRESENT1, RSF_D3D11_DISPATCH_RELEASE,
    RSF_D3D11_DISPATCH_RESIZE_BUFFERS, RSF_D3D11_DISPATCH_RESIZE_BUFFERS1,
    RSF_D3D11_DISPATCH_SET_COLOR_SPACE1, RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE,
    rsf_d3d11_chain_dispatch, rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_1,
};
use crate::d3d11_present_abi::*;
use crate::fg_choice::known;
use crate::fg_choice::rsf_fg_select_auto_backend;
use crate::fg12_abi::{rsf_d3d11_present_setup as Setup, rsf_streamline_host};
use crate::native_fg_cpu_abi::rsf_d3d11_present_status as PresentStatus;
use crate::render_abi::{
    RSF_BACKEND_ABI_VERSION, RSF_FG_ABI_VERSION, RSF_FG_BACKEND_ALL, RSF_FG_BACKEND_AUTO,
    RSF_FG_OFF, RSF_PACING_REFLEX, RSF_PACING_XELL, RSF_REFLEX_OFF, rsf_fg_options,
    rsf_fg_retirement, rsf_fg_status, rsf_generation_provider as Provider, rsf_generation_setup,
};
use crate::scene_abi::rsf_observer_present_event as Event;
use crate::shared_surface::{self, Surface, SurfaceSetup};
use crate::sr_session::{has_capacity, valid_unit_scale};
use std::cell::Cell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Write as _;
use std::ptr::{self, NonNull, null_mut};
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU8, AtomicU32, AtomicU64, Ordering,
};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, TryLockError};

#[path = "d3d11_present_device.rs"]
mod device;
#[path = "d3d11_present_dxgi.rs"]
mod dxgi;
#[path = "d3d11_present_facade.rs"]
mod facade;
#[path = "d3d11_present_factory.rs"]
mod factory;
#[path = "d3d11_present_gpu.rs"]
mod gpu;
#[path = "d3d11_present_native.rs"]
mod native;
#[path = "d3d11_present_streamline_abi.rs"]
mod streamline_abi;

type Platform = rsf_d3d11p_platform;
type Graphics = rsf_streamline_graphics;

pub(crate) const S_OK: i32 = 0;
pub(crate) const E_FAIL: i32 = 0x8000_4005_u32 as i32;
const E_POINTER: i32 = 0x8000_4003_u32 as i32;
pub(crate) const E_INVALIDARG: i32 = 0x8007_0057_u32 as i32;
pub(crate) const E_OUTOFMEMORY: i32 = 0x8007_000E_u32 as i32;
pub(crate) const E_UNEXPECTED: i32 = 0x8000_FFFF_u32 as i32;
pub(crate) const DXGI_ERROR_INVALID_CALL: i32 = 0x887A_0001_u32 as i32;
pub(crate) const DXGI_ERROR_UNSUPPORTED: i32 = 0x887A_0004_u32 as i32;
pub(crate) const DXGI_ERROR_DEVICE_REMOVED: i32 = 0x887A_0005_u32 as i32;
pub(crate) const DXGI_ERROR_WAS_STILL_DRAWING: i32 = 0x887A_000A_u32 as i32;
const DXGI_PRESENT_TEST: u32 = 0x1;
const DXGI_PRESENT_ALLOW_TEARING: u32 = 0x200;
const DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT: u32 = 64;
const DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING: u32 = 2048;
const DXGI_SWAP_EFFECT_FLIP_DISCARD: u32 = 4;
const DXGI_USAGE_RENDER_TARGET_OUTPUT: u32 = 0x20;
const DXGI_USAGE_UNORDERED_ACCESS: u32 = 0x400;
const FORMATS: [u32; 4] = [28, 87, 24, 10]; // R8G8B8A8, B8G8R8A8, R10G10B10A2 UNORM, R16G16B16A16 FLOAT

pub(crate) const OK: i32 = 0;
pub(crate) const INVALID_ARGUMENT: i32 = -1;
pub(crate) const ABI_MISMATCH: i32 = -2;
pub(crate) const INIT_FAILED: i32 = -6;
pub(crate) const NOT_SUPPORTED: i32 = -7;
pub(crate) const NOT_READY: i32 = -8;

pub(crate) const DLSS: u32 = 1;
pub(crate) const FSR3: u32 = 3;
pub(crate) const FSR4: u32 = 4;
pub(crate) const XESS: u32 = 5;
const AUTO: u32 = RSF_FG_BACKEND_AUTO;
const NO_REQUEST: u32 = u32::MAX;
const PACING_LAST: u32 = 3; // RSF_PACING_ANTILAG2
const LATENCY_INPUT_SAMPLE: u32 = 6;
const UI_MODE_NONE: u32 = 1;
const STATUS_VERSION: u32 = 1;
const MAX_BUFFERS: u32 = 8;
const SLOTS: usize = 3;
const APPLICATION_FENCES: usize = 16;
const STEPS: [&str; 3] = ["slot fence wait", "allocator reset", "command list reset"];

#[repr(u8)]
enum PhysicalCreation {
    Idle,
    Pending,
    Creating,
}

pub(super) const FG_OFF: rsf_fg_options = rsf_fg_options {
    struct_size: size_of::<rsf_fg_options>() as u32,
    abi_version: RSF_FG_ABI_VERSION,
    mode: RSF_FG_OFF,
    generated_frames: 1,
    dynamic_target_fps: 0.0,
    reflex_mode: RSF_REFLEX_OFF,
    frame_limit_us: 0,
};

fn failed(result: i32) -> bool {
    result < 0
}

fn acquire_resource(create: impl FnOnce(*mut Raw) -> i32) -> Result<ComRef, i32> {
    let mut resource = null_mut();
    let result = create(&mut resource);
    let owned = unsafe { ComRef::from_owned(resource) };
    if failed(result) {
        Err(result)
    } else {
        owned.ok_or(E_UNEXPECTED)
    }
}

fn bit(backend: u32) -> u32 {
    1u32.checked_shl(backend).unwrap_or(0)
}

// The single backend to provider family mapping. FidelityFX serves FSR3 and FSR4.
pub(crate) fn family(backend: u32) -> Option<u32> {
    match backend {
        DLSS => Some(FAMILY_DLSS),
        FSR3 | FSR4 => Some(FAMILY_FIDELITYFX),
        XESS => Some(FAMILY_XESS),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_fg_provider_family(backend: u32) -> u32 {
    family(backend).unwrap_or(0)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn hex(value: *const c_void) -> String {
    format!("{:016X}", value as usize)
}

// Settings ----------------------------------------------------------------------------------

struct Prepared {
    setup: Setup,
    _strings: [CString; 7],
    streamline_explicit: bool,
    streamline_empty: bool,
    identity: bool,
}

// The copies hold only pointers to their own strings and the caller's callbacks.
unsafe impl Send for Prepared {}
unsafe impl Sync for Prepared {}

struct Empty(Setup);
unsafe impl Sync for Empty {}
static EMPTY: Empty = Empty(unsafe { std::mem::zeroed() });
static SETTINGS: AtomicPtr<Prepared> = AtomicPtr::new(null_mut());
// Every prepared copy stays allocated: callbacks may hold its strings across a re-prepare.
// Boxed so each copy keeps its address when the vector grows.
#[allow(clippy::vec_box)]
static RETAINED: Mutex<Vec<Box<Prepared>>> = Mutex::new(Vec::new());

fn prepared() -> Option<&'static Prepared> {
    unsafe { SETTINGS.load(Ordering::Acquire).as_ref() }
}

fn settings() -> &'static Setup {
    prepared().map_or(&EMPTY.0, |value| &value.setup)
}

fn switching() -> bool {
    settings().runtime_switching != 0
}

fn log(text: &str) {
    let setup = settings();
    if let Some(log) = setup.log {
        let text = CString::new(text.replace('\0', " ")).unwrap_or_default();
        unsafe { log(setup.user, text.as_ptr()) };
    }
}

unsafe fn copy_string(text: *const c_char, fallback: &CStr) -> CString {
    if text.is_null() {
        fallback.to_owned()
    } else {
        unsafe { CStr::from_ptr(text) }.to_owned()
    }
}

fn sdk_directory(backend: u32) -> *const c_char {
    let setup = settings();
    match backend {
        DLSS => setup.streamline_directory_utf8,
        FSR4 => setup.fsr4_directory_utf8,
        XESS => setup.xess_directory_utf8,
        _ => setup.fsr3_directory_utf8,
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_settings() -> *const Setup {
    settings()
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_prepare(
    setup: *const Setup,
    installed: u32,
) -> i32 {
    let Some(input) = (unsafe { setup.as_ref() }) else {
        return 0;
    };
    if (input.struct_size as usize) < size_of::<Setup>()
        || input.runtime_directory_utf8.is_null()
        || input.accept_window.is_none()
        || installed != 0
    {
        return 0;
    }
    // Depth units are the plugin's to state; the bridge never guesses them from the API.
    if !valid_unit_scale(input.units_to_meters) {
        if let Some(log) = input.log {
            let text =
                c"presentation bridge: units_to_meters must be finite and positive; interception refused";
            unsafe { log(input.user, text.as_ptr()) };
        }
        return 0;
    }
    if input.backend != 0
        && !known(input.backend)
        && !(input.backend == AUTO && input.runtime_switching != 0)
    {
        return 0;
    }
    let allowed = input.allowed_backend_choices;
    if allowed != 0 && allowed & !RSF_FG_BACKEND_ALL != 0 {
        return 0;
    }
    if allowed != 0
        && input.backend != 0
        && input.backend != AUTO
        && allowed & bit(input.backend) == 0
    {
        return 0;
    }
    let runtime = unsafe { CStr::from_ptr(input.runtime_directory_utf8) }.to_owned();
    let empty = CString::default();
    let strings = unsafe {
        [
            copy_string(input.fsr3_directory_utf8, &runtime),
            copy_string(input.fsr4_directory_utf8, &runtime),
            copy_string(input.xess_directory_utf8, &runtime),
            copy_string(input.streamline_directory_utf8, &runtime),
            copy_string(input.engine_version_utf8, &empty),
            copy_string(input.project_id_utf8, &empty),
            runtime,
        ]
    };
    let mut copy: Setup = unsafe { ptr::read(setup) };
    copy.struct_size = size_of::<Setup>() as u32;
    copy.fsr3_directory_utf8 = strings[0].as_ptr();
    copy.fsr4_directory_utf8 = strings[1].as_ptr();
    copy.xess_directory_utf8 = strings[2].as_ptr();
    copy.streamline_directory_utf8 = strings[3].as_ptr();
    copy.engine_version_utf8 = strings[4].as_ptr();
    copy.project_id_utf8 = strings[5].as_ptr();
    copy.runtime_directory_utf8 = strings[6].as_ptr();
    let value = Box::new(Prepared {
        setup: copy,
        streamline_explicit: !input.streamline_directory_utf8.is_null(),
        streamline_empty: strings[3].is_empty(),
        identity: !strings[4].is_empty() && !strings[5].is_empty(),
        _strings: strings,
    });
    let pointer = ptr::from_ref(value.as_ref()).cast_mut();
    lock(&RETAINED).push(value);
    SETTINGS.store(pointer, Ordering::Release);
    let backend = match input.backend {
        0 if input.runtime_switching != 0 => 0,
        0 => DLSS,
        value => value,
    };
    ACTIVE_BACKEND.store(backend, Ordering::SeqCst);
    1
}

// Process owner -----------------------------------------------------------------------------

static ACTIVE_BACKEND: AtomicU32 = AtomicU32::new(DLSS);
static REQUESTED_BACKEND: AtomicU32 = AtomicU32::new(NO_REQUEST);
static BACKEND_CHOICES: AtomicU32 = AtomicU32::new((RSF_FG_BACKEND_ALL | 1) & !(1 << FSR4));
static SWITCH_RESULT: AtomicI32 = AtomicI32::new(0);
static CHAIN_GENERATION: AtomicU64 = AtomicU64::new(1);
static ACTIVE_HOST: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static ACTIVE_CHAIN: AtomicPtr<Owner> = AtomicPtr::new(null_mut());
static ACTIVE_BRIDGE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static ACTIVE_SESSION: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
// An unpublished owner still owns its provider until its queues have drained.
static RETIREMENT_PENDING: AtomicBool = AtomicBool::new(false);
// The active provider paces through the shared Streamline host (Reflex), so latency calls go
// to the host once rather than to both. Changes only with ACTIVE_SESSION, under PROVIDER_GUARD.
static HOST_PACED: AtomicBool = AtomicBool::new(false);
static ACTIVE_GRAPHICS: Mutex<SendGraphics> = Mutex::new(SendGraphics(Graphics::EMPTY));
// Main-thread latency calls share the provider lifetime; only replacement and retirement
// write-lock it, so a caller arriving mid-switch is turned away instead of blocking.
static PROVIDER_GUARD: RwLock<()> = RwLock::new(());
static CREATION_GUARD: Mutex<()> = Mutex::new(());
static STATUS: Mutex<Snapshot> = Mutex::new(Snapshot::new());
static SCOPED: AtomicBool = AtomicBool::new(false);
static CREATION_TOKENS: AtomicU64 = AtomicU64::new(0);
static ACTIVE_CREATION_TOKEN: AtomicU64 = AtomicU64::new(0);
static OUTER_ALIAS: AtomicPtr<c_void> = AtomicPtr::new(null_mut()); // Retained after admission.
static SCOPED_ADMITTED: AtomicBool = AtomicBool::new(false);

struct SendGraphics(Graphics);
unsafe impl Send for SendGraphics {}

struct Captured {
    object: Raw,
    release: Option<unsafe extern "C" fn(Raw)>,
}

impl Drop for Captured {
    fn drop(&mut self) {
        if let (false, Some(release)) = (self.object.is_null(), self.release) {
            unsafe { release(self.object) };
        }
    }
}

thread_local! {
    // A nested native creation from a provider or plain chain must forward unchanged.
    static CREATING: Cell<bool> = const { Cell::new(false) };
    static SCOPE_TOKEN: Cell<u64> = const { Cell::new(0) };
    static SCOPE_WINDOW: Cell<Raw> = const { Cell::new(null_mut()) };
    static SCOPE_CAPTURED: std::cell::RefCell<Captured> =
        const { std::cell::RefCell::new(Captured { object: null_mut(), release: None }) };
}

fn generation() -> u64 {
    CHAIN_GENERATION.load(Ordering::SeqCst)
}

fn provider_api(platform: &Platform, backend: u32) -> Option<&'static Provider> {
    let family = family(backend)?;
    let table = unsafe { (platform.provider_table)(family) };
    if table.is_null() || unsafe { table.cast::<u32>().read() } < size_of::<Provider>() as u32 {
        return None;
    }
    unsafe { table.as_ref() }
}

fn destroy_provider(api: Option<&Provider>, session: Raw) -> i32 {
    if session.is_null() {
        return OK;
    }
    let Some(api) = api else {
        return NOT_SUPPORTED;
    };
    if let Some(destroy) = api.destroy_checked {
        return unsafe { destroy(session) };
    }
    let Some(destroy) = api.destroy else {
        return NOT_SUPPORTED;
    };
    unsafe { destroy(session) };
    OK
}

fn read_guard() -> Option<std::sync::RwLockReadGuard<'static, ()>> {
    match PROVIDER_GUARD.try_read() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(guard)) => Some(guard.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

// Copied provider status -------------------------------------------------------------------

// Owns copies only. No provider or COM lifetime and no vendor call crosses its lock.
pub(crate) struct Snapshot {
    valid: u32,
    backend: u32,
    generation: u64,
    state: rsf_fg_status,
    accepting: bool,
}

const EMPTY_STATUS: rsf_fg_status = unsafe { std::mem::zeroed() };

impl Snapshot {
    pub(crate) const fn new() -> Self {
        Self {
            valid: 0,
            backend: 0,
            generation: 0,
            state: EMPTY_STATUS,
            accepting: false,
        }
    }

    pub(crate) fn activate(&mut self, backend: u32, generation: u64) {
        *self = Self {
            backend,
            generation,
            accepting: true,
            ..Self::new()
        };
    }

    pub(crate) fn invalidate(&mut self) {
        self.valid = 0;
        self.state = EMPTY_STATUS;
        self.accepting = false;
    }

    pub(crate) fn clear(&mut self) {
        self.valid = 0;
        self.state = EMPTY_STATUS;
    }

    pub(crate) fn publish(&mut self, backend: u32, generation: u64, state: &rsf_fg_status) -> bool {
        if !self.accepting
            || generation == 0
            || backend != self.backend
            || generation != self.generation
            || (state.struct_size as usize) < size_of::<rsf_fg_status>()
        {
            return false;
        }
        self.state = *state;
        self.valid = 1;
        true
    }

    pub(crate) unsafe fn read(&self, output: *mut PresentStatus) -> i32 {
        let Some(output) = (unsafe { output.as_mut() }) else {
            return INVALID_ARGUMENT;
        };
        if (output.struct_size as usize) < size_of::<PresentStatus>() {
            return INVALID_ARGUMENT;
        }
        if output.version != STATUS_VERSION {
            return ABI_MISMATCH;
        }
        output.struct_size = size_of::<PresentStatus>() as u32;
        output.version = STATUS_VERSION;
        output.valid = self.valid;
        output.backend = self.backend;
        output.generation = self.generation;
        output.state = self.state;
        if self.valid != 0 { OK } else { NOT_READY }
    }
}

// Facade sequencing ------------------------------------------------------------------------

#[derive(Default)]
struct Timing {
    tick_ms: f64,
    epoch: u64,
    previous: u64,
    frames: u64,
    sdk_start: u64,
    interval_min: f64,
    interval_max: f64,
    sum: [f64; 5],
    max: [f64; 5],
    slots: u32,
}

impl Timing {
    // Clears one reporting window. A zero epoch also restarts the clock on the next frame.
    fn reset(&mut self) {
        self.frames = 0;
        self.sum = [0.0; 5];
        self.max = [0.0; 5];
        self.interval_min = 0.0;
        self.interval_max = 0.0;
        self.slots = 0;
    }
}

enum ApplicationBuffer {
    Shared(Surface),
    Render(ComRef),
}

impl ApplicationBuffer {
    fn application(&self) -> &ComRef {
        match self {
            Self::Shared(surface) => &surface.texture11,
            Self::Render(resource) => resource,
        }
    }

    fn copy_source(&self) -> Option<&ComRef> {
        match self {
            Self::Shared(surface) => surface.texture12.as_ref(),
            Self::Render(resource) => Some(resource),
        }
    }
}

type ApplicationBuffers = [Option<ApplicationBuffer>; MAX_BUFFERS as usize];

pub(crate) struct Physical {
    // Release the chain before its creation queue.
    chain: ComRef,
    queue: ComRef,
}

// The DXGI chain and D3D12 queue can be referenced from the render and window threads.
// A lease pins their lifetime. DXGI and the caller still govern operation ordering.
unsafe impl Send for Physical {}
unsafe impl Sync for Physical {}

// One direct allocator and closed list on the graphics device (Streamline's proxy device when a
// host exists). The list is declared first so it releases before its allocator.
struct Commands {
    list: ComRef,
    allocator: ComRef,
}

// The slot pairs in slot order, fewer than SLOTS only after a refused initialization.
struct CommandSlots(Vec<Commands>);

// Created before publication, recorded and submitted under Present's pin, and released or
// retained by the facade's final Release, when no pin remains.
unsafe impl Send for CommandSlots {}
unsafe impl Sync for CommandSlots {}

// A Win32 event from the platform leaf. The other handle returned to the application
// by DuplicateHandle has its own lifetime.
struct EventLease {
    handle: NonNull<c_void>,
    close: unsafe extern "C" fn(Raw),
}

impl EventLease {
    fn create(platform: &Platform, initially_signaled: bool) -> Option<Self> {
        let handle = unsafe { (platform.create_event)(u32::from(initially_signaled)) };
        NonNull::new(handle).map(|handle| Self {
            handle,
            close: platform.close_event,
        })
    }

    fn as_ptr(&self) -> Raw {
        self.handle.as_ptr()
    }
}

impl Drop for EventLease {
    fn drop(&mut self) {
        unsafe { (self.close)(self.as_ptr()) };
    }
}

#[derive(Default)]
struct PresentationSync {
    // Borrowed for drain. The facade or its host owns this queue.
    graphics_queue: Option<NonNull<c_void>>,
    interop_queue: Option<ComRef>,
    completion_event: Option<EventLease>,
    completion_fence: Option<ComRef>,
    application_ready: Option<EventLease>,
}

impl PresentationSync {
    // Store each acquisition before checking the result so an unpublished facade retires
    // the same partial state as a published one.
    fn initialize(&mut self, p: &Platform, facts: &rsf_d3d11p_facts, ready: bool) -> i32 {
        self.graphics_queue = NonNull::new(facts.graphics.queue);
        if facts.native12 != 0 {
            self.interop_queue = Some(unsafe { ComRef::retain(facts.direct_queue) });
        } else {
            let mut queue = null_mut();
            let result = unsafe { (p.create_interop_queue)(facts.graphics.device, &mut queue) };
            self.interop_queue = unsafe { ComRef::from_owned(queue) };
            if failed(result) {
                return E_FAIL;
            }
        }
        self.completion_event = EventLease::create(p, false);
        if self.completion_event.is_none() {
            return E_FAIL;
        }
        let mut fence = null_mut();
        let result = unsafe { (p.create_completion_fence)(facts.graphics.device, &mut fence) };
        self.completion_fence = unsafe { ComRef::from_owned(fence) };
        if failed(result) {
            return E_FAIL;
        }
        if ready {
            self.application_ready = EventLease::create(p, true);
            if self.application_ready.is_none() {
                return E_FAIL;
            }
        }
        S_OK
    }

    fn queue(&self) -> Raw {
        self.interop_queue
            .as_ref()
            .map_or(null_mut(), ComRef::as_ptr)
    }

    fn completion(&self) -> (Raw, Raw) {
        (
            self.completion_fence
                .as_ref()
                .map_or(null_mut(), ComRef::as_ptr),
            self.completion_event
                .as_ref()
                .map_or(null_mut(), EventLease::as_ptr),
        )
    }

    fn ready_event(&self) -> Raw {
        self.application_ready
            .as_ref()
            .map_or(null_mut(), EventLease::as_ptr)
    }
}

#[derive(Default)]
struct ProviderRetirement {
    fence: Option<ComRef>,
    serial: u64,
}

// DXGI serializes the application's frame calls. Provider callbacks and the retire
// lease can reenter on the same thread.
pub(crate) struct Owner {
    platform: *const Platform,
    bridge: AtomicPtr<c_void>,
    native12: AtomicBool,
    tearing: AtomicBool,
    host: AtomicPtr<c_void>,
    provider: AtomicPtr<c_void>,
    provider_backend: AtomicU32,
    physical: Mutex<Option<Arc<Physical>>>,
    physical_creation: AtomicU8,
    faulted: AtomicBool,
    serial: AtomicU64,
    next_slot: AtomicU32,
    slot_fences: [AtomicU64; SLOTS],
    sync: Mutex<PresentationSync>,
    commands: Mutex<Option<Arc<CommandSlots>>>,
    buffers: Mutex<ApplicationBuffers>,
    render_index: AtomicU32,
    application_serial: AtomicU64,
    application_fences: [AtomicU64; APPLICATION_FENCES],
    application_latency: AtomicU32,
    application_presents: AtomicU32,
    provider_retirement: Mutex<ProviderRetirement>,
    switch_fullscreen: Mutex<Option<i32>>,
    unpublished_provider: Mutex<Option<(u32, usize)>>,
    failed_buffer_reports: AtomicU32,
    post_switch_frames: AtomicU32,
    last_normalized_sync: AtomicU32,
    timing: Mutex<Timing>,
}

unsafe impl Send for Owner {}
unsafe impl Sync for Owner {}

impl Owner {
    fn new(platform: *const Platform) -> Self {
        Self {
            platform,
            bridge: AtomicPtr::new(null_mut()),
            native12: AtomicBool::new(false),
            tearing: AtomicBool::new(false),
            host: AtomicPtr::new(null_mut()),
            provider: AtomicPtr::new(null_mut()),
            provider_backend: AtomicU32::new(0),
            physical: Mutex::default(),
            physical_creation: AtomicU8::new(PhysicalCreation::Idle as u8),
            faulted: AtomicBool::new(false),
            serial: AtomicU64::new(0),
            next_slot: AtomicU32::new(0),
            slot_fences: Default::default(),
            sync: Mutex::default(),
            commands: Mutex::default(),
            buffers: Mutex::default(),
            render_index: AtomicU32::new(0),
            application_serial: AtomicU64::new(0),
            application_fences: Default::default(),
            application_latency: AtomicU32::new(1),
            application_presents: AtomicU32::new(0),
            provider_retirement: Mutex::default(),
            switch_fullscreen: Mutex::default(),
            unpublished_provider: Mutex::default(),
            failed_buffer_reports: AtomicU32::new(0),
            post_switch_frames: AtomicU32::new(0),
            last_normalized_sync: AtomicU32::new(0),
            timing: Mutex::new(Timing::default()),
        }
    }

    fn p(&self) -> &Platform {
        unsafe { &*self.platform }
    }

    fn b(&self) -> Raw {
        self.bridge.load(Ordering::Acquire)
    }

    fn provider(&self) -> Raw {
        self.provider.load(Ordering::Acquire)
    }

    fn host(&self) -> Raw {
        self.host.load(Ordering::Acquire)
    }

    fn native12(&self) -> bool {
        self.native12.load(Ordering::Relaxed)
    }

    fn next_serial(&self) -> u64 {
        self.serial.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn facts(&self) -> rsf_d3d11p_facts {
        let mut facts = rsf_d3d11p_facts {
            window: null_mut(),
            width: 0,
            height: 0,
            format: 0,
            buffer_count: 0,
            flags: 0,
            windowed: 0,
            native12: 0,
            tearing: 0,
            graphics: Graphics::EMPTY,
            direct_queue: null_mut(),
            device11: null_mut(),
            buffer_usage: 0,
        };
        unsafe { (self.p().facts)(self.b(), &mut facts) };
        facts
    }

    fn api(&self) -> Option<&'static Provider> {
        provider_api(self.p(), self.provider_backend.load(Ordering::Relaxed))
    }

    // Every current provider status is a cached-only copy; SDK polling stays in the owner callbacks.
    fn provider_status(&self) -> Option<rsf_fg_status> {
        let provider = self.provider();
        let status = self.api()?.status?;
        if provider.is_null() {
            return None;
        }
        let mut state = EMPTY_STATUS;
        state.struct_size = size_of::<rsf_fg_status>() as u32;
        (unsafe { status(provider, &mut state) } == OK).then_some(state)
    }

    fn publish_status(&self) {
        let snapshot = self.provider_status();
        let mut status = lock(&STATUS);
        self.write_status(&mut status, snapshot.as_ref());
    }

    fn write_status(&self, status: &mut Snapshot, snapshot: Option<&rsf_fg_status>) {
        let backend = self.provider_backend.load(Ordering::Relaxed);
        if let Some(snapshot) = snapshot {
            let _ = status.publish(backend, generation(), snapshot);
        } else {
            status.clear();
        }
    }

    fn publish_owner_status(&self) -> Option<rsf_fg_status> {
        let snapshot = self.provider_status();
        // A Reflex-paced provider with a shared host leaves pacing with that host.
        let host_paced = !self.host().is_null()
            && snapshot
                .as_ref()
                .is_some_and(|state| state.pacing_owner == RSF_PACING_REFLEX);
        HOST_PACED.store(host_paced, Ordering::SeqCst);
        let mut status = lock(&STATUS);
        status.activate(ACTIVE_BACKEND.load(Ordering::SeqCst), generation());
        self.write_status(&mut status, snapshot.as_ref());
        snapshot
    }

    fn receipt(&self, stage: &str, result: i32) {
        if self.post_switch_frames.load(Ordering::Relaxed) == 0 {
            return;
        }
        let thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        log(&format!(
            "Bridge post-switch stage={stage} backend={} generation={} HRESULT=0x{:08x} thread={thread}",
            ACTIVE_BACKEND.load(Ordering::SeqCst),
            generation(),
            result as u32
        ));
    }

    fn generation_setup(&self, backend: u32) -> rsf_generation_setup {
        let setup = settings();
        let facts = self.facts();
        let native12 = self.native12();
        let mut value: rsf_generation_setup = unsafe { std::mem::zeroed() };
        value.struct_size = size_of::<rsf_generation_setup>() as u32;
        value.abi_version = RSF_FG_ABI_VERSION;
        value.feature_major = backend;
        value.streamline_host = self.host();
        value.runtime_directory_utf8 = sdk_directory(backend);
        value.depth_inverted = setup.depth_inverted;
        value.depth_infinite = setup.depth_infinite;
        value.view_space_to_meters = setup.units_to_meters;
        let chain = &mut value.chain;
        chain.struct_size = size_of_val(chain) as u32;
        chain.abi_version = RSF_BACKEND_ABI_VERSION;
        chain.d3d12_device = facts.graphics.device;
        chain.d3d12_queue = facts.graphics.queue;
        if backend != DLSS {
            chain.d3d12_device = facts.graphics.native_device;
            chain.d3d12_queue = if native12 {
                facts.direct_queue
            } else {
                facts.graphics.native_queue
            };
        }
        chain.hwnd = facts.window;
        chain.width = facts.width;
        chain.height = facts.height;
        chain.format = facts.format;
        chain.buffer_count = if native12 { facts.buffer_count } else { 3 };
        chain.max_generated_frames = match setup.max_generated_frames {
            0 => 3,
            value => value,
        };
        chain.ui_mode = match setup.ui_mode {
            0 => UI_MODE_NONE,
            value => value,
        };
        chain.allow_tearing = u32::from(self.tearing.load(Ordering::Relaxed));
        chain.log = setup.log;
        chain.log_user = setup.user;
        value
    }

    fn physical(&self) -> Option<Arc<Physical>> {
        // Copy only the lease under the mutex. COM calls and releases run outside it.
        lock(&self.physical).clone()
    }

    fn publish_physical(&self, chain: Result<ComRef, i32>, queue: Raw) -> i32 {
        let Ok(chain) = chain else { return INIT_FAILED };
        if queue.is_null() {
            return INIT_FAILED;
        }
        let physical = Arc::new(Physical {
            chain,
            queue: unsafe { ComRef::retain(queue) },
        });
        let mut current = lock(&self.physical);
        if current.is_some() {
            return INIT_FAILED;
        }
        *current = Some(physical);
        OK
    }

    fn take_physical(&self) -> Option<Arc<Physical>> {
        lock(&self.physical).take()
    }

    fn retire_unpublished(&self) -> i32 {
        let mut pending = lock(&self.unpublished_provider);
        let Some((backend, session)) = *pending else {
            return OK;
        };
        let result = destroy_provider(provider_api(self.p(), backend), session as Raw);
        if result == OK {
            *pending = None;
        }
        result
    }

    fn discard_unpublished(&self, backend: u32, session: Raw) -> i32 {
        if session.is_null() {
            return OK;
        }
        *lock(&self.unpublished_provider) = Some((backend, session as usize));
        self.retire_unpublished()
    }

    fn create_physical(&self, backend: u32) -> i32 {
        let retired = self.retire_unpublished();
        if retired != OK {
            return retired;
        }
        let p = self.p();
        if backend == 0 {
            let facts = self.facts();
            // D3D11 application bindings belong to the virtual buffers. The plain physical
            // D3D12 chain only presents copies. Native D3D12 keeps its application buffer shape.
            let (creation_queue, buffer_count, buffer_usage) = if facts.native12 != 0 {
                (facts.direct_queue, 0, 0)
            } else {
                (
                    facts.graphics.native_queue,
                    3,
                    DXGI_USAGE_RENDER_TARGET_OUTPUT,
                )
            };
            let plan = rsf_d3d11p_plain_plan {
                queue: creation_queue,
                swap_effect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                flags: if facts.tearing != 0 {
                    DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING
                } else {
                    0
                },
                buffer_count,
                buffer_usage,
            };
            let mut queue = null_mut();
            let chain = acquire_resource(|out| unsafe {
                (p.create_plain)(self.b(), &plan, out, &mut queue)
            });
            return self.publish_physical(chain, queue);
        }
        let setup = self.generation_setup(backend);
        let Some(api) = provider_api(p, backend) else {
            return NOT_SUPPORTED;
        };
        let (Some(create), Some(configure)) = (api.create, api.configure) else {
            return NOT_SUPPORTED;
        };
        let mut provider = null_mut();
        let mut chain = null_mut();
        let mut result = unsafe { create(&setup, &mut provider, &mut chain) };
        if result != OK {
            let runtime = if setup.runtime_directory_utf8.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(setup.runtime_directory_utf8) }
                    .to_string_lossy()
                    .into_owned()
            };
            log(&format!(
                "Generation provider create failed backend={backend} result={result} allowed=0x{:x} size={}x{} runtime={runtime}",
                settings().allowed_backend_choices,
                setup.chain.width,
                setup.chain.height
            ));
        }
        if result == OK && chain.is_null() {
            result = INIT_FAILED;
        }
        if result == OK {
            result = unsafe { configure(provider, &FG_OFF) };
        }
        if result == OK {
            let chain4 = acquire_resource(|out| unsafe { (p.query_chain4)(chain, out) });
            result = self.publish_physical(chain4, setup.chain.d3d12_queue);
        }
        if result == OK {
            self.provider_backend.store(backend, Ordering::Relaxed);
            self.provider.store(provider, Ordering::Release);
        } else {
            let retired = self.discard_unpublished(backend, provider);
            if retired != OK {
                return retired;
            }
        }
        result
    }

    // Joins the shared queues and the provider's retirement fence.
    fn drain(&self) -> i32 {
        let p = self.p();
        let (interop, graphics, (fence, event)) = {
            let sync = lock(&self.sync);
            (sync.queue(), sync.graphics_queue, sync.completion())
        };
        let Some(graphics) = graphics else {
            return S_OK;
        };
        if fence.is_null() {
            return S_OK;
        }
        if !interop.is_null()
            && failed(unsafe { (p.signal_wait)(interop, fence, event, self.next_serial()) })
        {
            return E_FAIL;
        }
        if failed(unsafe { (p.signal_wait)(graphics.as_ptr(), fence, event, self.next_serial()) }) {
            return E_FAIL;
        }
        let provider = self.provider();
        if !provider.is_null() {
            let mut retirement = rsf_fg_retirement {
                struct_size: size_of::<rsf_fg_retirement>() as u32,
                fence: null_mut(),
                value: 0,
            };
            let retired = self
                .api()
                .and_then(|api| api.retirement)
                .map_or(NOT_SUPPORTED, |call| unsafe {
                    call(provider, &mut retirement)
                });
            if retired != OK
                || failed(unsafe { (p.wait_fence)(retirement.fence, event, retirement.value) })
            {
                return E_FAIL;
            }
        }
        S_OK
    }

    fn creation_pending(&self) -> bool {
        self.physical_creation.load(Ordering::Acquire) != PhysicalCreation::Idle as u8
    }

    fn ensure_physical(&self) -> Result<Arc<Physical>, i32> {
        let p = self.p();
        // Never create the HWND owner from an outer wrapper's constructor query.
        if SCOPED.load(Ordering::Acquire) && SCOPE_TOKEN.get() != 0 {
            return Err(DXGI_ERROR_WAS_STILL_DRAWING);
        }
        if self.faulted.load(Ordering::Acquire) {
            return Err(DXGI_ERROR_DEVICE_REMOVED);
        }
        match self.physical_creation.compare_exchange(
            PhysicalCreation::Pending as u8,
            PhysicalCreation::Creating as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {}
            Err(state) if state == PhysicalCreation::Creating as u8 => {
                return Err(DXGI_ERROR_WAS_STILL_DRAWING);
            }
            Err(_) => return self.physical().ok_or(DXGI_ERROR_WAS_STILL_DRAWING),
        }
        // Overlay creation callbacks retain per-call device and queue state. A provider's
        // nested native creation happens after the engine's factory call returns.
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                CREATING.set(self.0);
            }
        }
        let _restore = Restore(CREATING.replace(true));
        let host = self.host();
        if !host.is_null()
            && ACTIVE_BACKEND.load(Ordering::SeqCst) != DLSS
            && unsafe { (p.host_generation_load)(host, 0) } != OK
        {
            self.physical_creation
                .store(PhysicalCreation::Pending as u8, Ordering::Release);
            return Err(E_FAIL);
        }
        let result = self.create_physical(ACTIVE_BACKEND.load(Ordering::SeqCst));
        if result != OK {
            SWITCH_RESULT.store(result, Ordering::SeqCst);
            ACTIVE_BACKEND.store(0, Ordering::SeqCst);
            if self.create_physical(0) != OK {
                if lock(&self.unpublished_provider).is_some() {
                    self.physical_creation
                        .store(PhysicalCreation::Pending as u8, Ordering::Release);
                    return Err(DXGI_ERROR_WAS_STILL_DRAWING);
                }
                self.faulted.store(true, Ordering::Release);
                self.physical_creation
                    .store(PhysicalCreation::Idle as u8, Ordering::Release);
                return Err(E_FAIL);
            }
        }
        let Some(physical) = self.physical() else {
            self.faulted.store(true, Ordering::Release);
            self.physical_creation
                .store(PhysicalCreation::Idle as u8, Ordering::Release);
            return Err(E_FAIL);
        };
        // Publish the created chain before fullscreen entry. A refused entry fails this call,
        // but the chain stays and later calls present through it, so its session must be visible.
        ACTIVE_SESSION.store(self.provider(), Ordering::Release);
        let _ = self.publish_owner_status();
        self.physical_creation
            .store(PhysicalCreation::Idle as u8, Ordering::Release);
        if self.facts().windowed == 0
            && failed(unsafe { (p.set_fullscreen)(physical.chain.as_ptr(), 1, null_mut()) })
        {
            return Err(E_FAIL);
        }
        Ok(physical)
    }

    fn allocate_application_buffers(
        &self,
        facts: &rsf_d3d11p_facts,
    ) -> Result<ApplicationBuffers, i32> {
        let p = self.p();
        let mut batch = ApplicationBuffers::default();
        let slots = batch
            .get_mut(..facts.buffer_count as usize)
            .filter(|slots| !slots.is_empty())
            .ok_or(E_INVALIDARG)?;
        if self.native12() {
            for slot in slots {
                let resource = acquire_resource(|out| unsafe {
                    (p.create_render_buffer)(self.b(), facts.width, facts.height, facts.format, out)
                })?;
                *slot = Some(ApplicationBuffer::Render(resource));
            }
        } else {
            let setup = SurfaceSetup {
                struct_size: size_of::<SurfaceSetup>() as u32,
                abi_version: shared_surface::ABI_VERSION,
                width: facts.width,
                height: facts.height,
                format: facts.format,
                render_target: 1,
                log: settings().log,
                log_user: settings().user,
                unordered_access: u32::from(facts.buffer_usage & DXGI_USAGE_UNORDERED_ACCESS != 0),
            };
            let device12 = facts.graphics.native_device;
            if device12.is_null() || !shared_surface::valid_surface(facts.device11, &setup) {
                return Err(E_FAIL);
            }
            let platform = unsafe { &*(p.shared_surface_platform)() };
            for slot in slots {
                let surface = unsafe {
                    shared_surface::create_surface(platform, facts.device11, device12, &setup)
                }
                .map_err(|_| E_FAIL)?;
                *slot = Some(ApplicationBuffer::Shared(surface));
            }
        }
        Ok(batch)
    }

    fn create_application_buffers(&self, facts: &rsf_d3d11p_facts) -> Result<(), i32> {
        let batch = self.allocate_application_buffers(facts)?;
        *lock(&self.buffers) = batch;
        Ok(())
    }

    fn take_application_buffers(&self) -> ApplicationBuffers {
        std::mem::take(&mut *lock(&self.buffers))
    }

    fn buffers_referenced(&self) -> bool {
        lock(&self.buffers).iter().flatten().any(|buffer| unsafe {
            (self.p().buffer_referenced)(buffer.application().as_ptr()) != 0
        })
    }

    unsafe fn get_buffer(&self, index: u32, iid: *const c_void, out: *mut Raw) -> i32 {
        let p = self.p();
        // Wrappers can query virtual buffers before any physical chain is created.
        if !self.native12() || switching() {
            let buffer = lock(&self.buffers)
                .get(index as usize)
                .and_then(Option::as_ref)
                .map(|buffer| buffer.application().clone());
            return buffer.map_or(DXGI_ERROR_INVALID_CALL, |buffer| unsafe {
                (p.query_buffer_interface)(buffer.as_ptr(), iid, out)
            });
        }
        let physical = match self.ensure_physical() {
            Ok(physical) => physical,
            Err(error) => {
                if self.failed_buffer_reports.fetch_add(1, Ordering::Relaxed) < 4 {
                    let pending = u32::from(self.creation_pending());
                    unsafe { (p.log_buffer_refusal)(self.b(), index, iid, error, pending) };
                }
                return error;
            }
        };
        unsafe { (p.get_physical_buffer)(physical.chain.as_ptr(), index, iid, out) }
    }

    fn acquire_backbuffer(&self, physical: &Physical) -> Result<ComRef, i32> {
        acquire_resource(|out| unsafe {
            (self.p().acquire_backbuffer)(physical.chain.as_ptr(), out)
        })
    }

    fn copy_source(&self, index: u32) -> Option<ComRef> {
        lock(&self.buffers)
            .get(index as usize)
            .and_then(Option::as_ref)
            .and_then(ApplicationBuffer::copy_source)
            .cloned()
    }

    // Creates the slot pairs in slot order. A refusal keeps the earlier pairs, which teardown
    // releases or retains with the rest of the unpublished facade.
    fn create_command_slots(&self) -> i32 {
        let p = self.p();
        let mut pairs = Vec::with_capacity(SLOTS);
        let mut result = S_OK;
        while pairs.len() < SLOTS {
            let (mut allocator, mut list) = (null_mut(), null_mut());
            let created = unsafe { (p.create_slot)(self.b(), &mut allocator, &mut list) };
            let list = unsafe { ComRef::from_owned(list) };
            let allocator = unsafe { ComRef::from_owned(allocator) };
            match (list, allocator) {
                (Some(list), Some(allocator)) if !failed(created) => {
                    pairs.push(Commands { list, allocator });
                }
                _ => {
                    result = E_FAIL;
                    break;
                }
            }
        }
        *lock(&self.commands) = Some(Arc::new(CommandSlots(pairs)));
        result
    }

    // Pins the slot pairs for one Present. Publication requires all three.
    fn command_slots(&self) -> Option<Arc<CommandSlots>> {
        lock(&self.commands)
            .clone()
            .filter(|slots| slots.0.len() == SLOTS)
    }

    fn take_command_slots(&self) -> Option<Arc<CommandSlots>> {
        lock(&self.commands).take()
    }

    // The facade's last reference is gone, so Present holds no pin. Each list releases before
    // its allocator, in slot order or last slot first.
    fn release_command_slots(&self, last_first: bool) {
        if let Some(slots) = self.take_command_slots()
            && let Ok(CommandSlots(mut pairs)) = Arc::try_unwrap(slots)
        {
            if last_first {
                pairs.reverse();
            }
            drop(pairs);
        }
    }

    fn reset_application_frames(&self) {
        self.render_index.store(0, Ordering::Relaxed);
        self.application_serial.store(0, Ordering::Relaxed);
        for fence in &self.application_fences {
            fence.store(0, Ordering::Relaxed);
        }
        let ready = lock(&self.sync).ready_event();
        if !ready.is_null() {
            let _ = unsafe { (self.p().application_ready)(ready, null_mut(), 0) };
        }
    }

    fn initialize(
        &self,
        factory: Raw,
        source: Raw,
        request: &rsf_d3d11p_request,
        desc: *const c_void,
    ) -> i32 {
        let p = self.p();
        let b = self.b();
        let setup = settings();
        let identity = prepared().is_some_and(|value| value.identity);
        let mut frequency = 0_i64;
        unsafe {
            windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency)
        };
        lock(&self.timing).tick_ms = if frequency > 0 {
            1000.0 / frequency as f64
        } else {
            0.0
        };
        let native12 = unsafe { (p.init_source)(b, factory, source, desc) } != 0;
        self.native12.store(native12, Ordering::Relaxed);
        if setup.backend == AUTO {
            let mut adapter = rsf_d3d11p_adapter::default();
            if failed(unsafe { (p.owning_adapter)(b, source, &mut adapter) }) {
                return E_FAIL;
            }
            let allowed = setup.allowed_backend_choices;
            // An unrestricted setup keeps the adapter default, including Off.
            let backend = if allowed == 0 {
                adapter.default_backend
            } else {
                rsf_fg_select_auto_backend(adapter.default_backend, allowed)
            };
            ACTIVE_BACKEND.store(backend, Ordering::SeqCst);
            log(&format!(
                "Auto FG selected backend {backend} from owning adapter vendor=0x{:x} device=0x{:x}",
                adapter.vendor, adapter.device
            ));
        }
        let explicit =
            prepared().is_some_and(|value| value.streamline_explicit && !value.streamline_empty);
        if native12 {
            // Native D3D12 plugins use the engine's device and queue.
            if setup.backend == DLSS && setup.runtime_switching == 0 {
                return DXGI_ERROR_UNSUPPORTED;
            }
            let result = unsafe { (p.native_device)(b) };
            if failed(result) {
                return result;
            }
            if setup.runtime_switching != 0 && explicit {
                let mut adopted = NOT_SUPPORTED;
                if unsafe { (p.adopt_host)(b, u32::from(identity), &mut adopted) } != 0 {
                    // Only INIT_FAILED can follow slInit, slUpgradeInterface or slSetD3DDevice on the
                    // engine device, which may then be half registered. Every other refusal happens
                    // before the device is touched and only leaves DLSS-G unavailable.
                    if adopted == INIT_FAILED {
                        log("Native D3D12 shared Streamline registration refused");
                        return DXGI_ERROR_UNSUPPORTED;
                    }
                    if adopted != OK {
                        log(&format!(
                            "Native D3D12 shared Streamline host unavailable ({adopted}). DLSS-G disabled"
                        ));
                    }
                }
            }
        } else {
            let result = unsafe { (p.d3d11_device)(b, source) };
            if failed(result) {
                return result;
            }
        }
        // The host decides which adapters Streamline serves. Only a strict DLSS request (no runtime
        // switching) needs it; otherwise a refusal leaves FidelityFX, XeSS and Off available.
        let dlss_required =
            setup.runtime_switching == 0 && (setup.backend == 0 || setup.backend == DLSS);
        if !native12 && (dlss_required || setup.runtime_switching != 0) {
            let result = if identity {
                unsafe { (p.create_host)(b) }
            } else {
                NOT_SUPPORTED
            };
            if result != OK {
                log(&format!(
                    "Streamline FG host refused ({result}); {}",
                    if dlss_required {
                        "retaining D3D11 presentation"
                    } else {
                        "DLSS-G unavailable, other generation backends remain"
                    }
                ));
                if dlss_required {
                    return DXGI_ERROR_UNSUPPORTED;
                }
            }
        }
        self.host.store(unsafe { (p.host)(b) }, Ordering::Release);
        let result = unsafe { (p.resolve_graphics)(b) };
        if failed(result) {
            return result;
        }
        self.tearing
            .store(unsafe { (p.tearing_supported)(b) } != 0, Ordering::Relaxed);
        let mut choices = RSF_FG_BACKEND_ALL | 1;
        if unsafe { (p.fsr4_supported)(b, setup.fsr4_directory_utf8) } == 0 {
            choices &= !bit(FSR4);
        }
        if setup.allowed_backend_choices != 0 {
            choices &= setup.allowed_backend_choices | 1;
        }
        BACKEND_CHOICES.store(choices, Ordering::SeqCst);
        let ready = native12
            && setup.runtime_switching != 0
            && request.flags & DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT != 0;
        let mut sync = PresentationSync::default();
        let result = sync.initialize(p, &self.facts(), ready);
        *lock(&self.sync) = sync;
        if failed(result) {
            return E_FAIL;
        }
        if !native12 {
            if failed(unsafe { (p.create_upload_fence)(b) })
                || self.create_application_buffers(&self.facts()).is_err()
            {
                return E_FAIL;
            }
            self.render_index.store(0, Ordering::Relaxed);
        }
        if failed(self.create_command_slots()) {
            return E_FAIL;
        }
        if setup.runtime_switching != 0 {
            if native12 {
                if self.create_application_buffers(&self.facts()).is_err() {
                    return E_FAIL;
                }
                self.reset_application_frames();
            }
            self.physical_creation
                .store(PhysicalCreation::Pending as u8, Ordering::Release);
        } else {
            let backend = ACTIVE_BACKEND.load(Ordering::SeqCst);
            let result = self.create_physical(backend);
            if result != OK {
                log(&format!(
                    "Generation backend={backend} create refused ({result}); retaining engine presentation"
                ));
                return E_FAIL;
            }
        }
        if request.windowed == 0
            && let Some(physical) = self.physical()
            && failed(unsafe { (p.set_fullscreen)(physical.chain.as_ptr(), 1, null_mut()) })
        {
            return E_FAIL;
        }
        log(if self.tearing.load(Ordering::Relaxed) {
            "presentation bridge: windowed flip chain supports immediate tearing presentation"
        } else {
            "presentation bridge: DXGI tearing unavailable; using supported flip presentation"
        });
        let facts = self.facts();
        ACTIVE_HOST.store(self.host(), Ordering::Release);
        ACTIVE_SESSION.store(self.provider(), Ordering::Release);
        let interop_queue = lock(&self.sync).queue();
        lock(&ACTIVE_GRAPHICS).0 = Graphics {
            queue: interop_queue,
            native_queue: null_mut(),
            ..facts.graphics
        };
        ACTIVE_BRIDGE.store(b, Ordering::Release);
        ACTIVE_CHAIN.store(ptr::from_ref(self).cast_mut(), Ordering::Release);
        let snapshot = self.publish_owner_status();
        if setup.log.is_some()
            && let Some(status) = snapshot
        {
            let version = unsafe { CStr::from_ptr(status.version_name.as_ptr()) };
            log(&format!(
                "Generation support={} max generated={} dynamic={} VSync={} minimum={} latency available={} version={}",
                status.supported,
                status.max_generated_frames,
                status.dynamic_supported,
                status.vsync_supported,
                status.min_dimension,
                status.low_latency_available,
                version.to_string_lossy()
            ));
        }
        log(if native12 {
            "presentation bridge: native D3D12 engine queue, one vendor generation chain"
        } else {
            "presentation bridge: D3D11 facade, one vendor D3D12 generation chain, separate interop queue"
        });
        S_OK
    }

    // A requested provider change happens at a drained Present boundary.
    fn switch_if_requested(&self) {
        let selected = REQUESTED_BACKEND.swap(NO_REQUEST, Ordering::SeqCst);
        if selected != NO_REQUEST {
            self.switch_provider(selected);
        }
    }

    fn defer_switch(backend: u32) {
        // Retry at another completed Present without displacing a newer request.
        let _ = REQUESTED_BACKEND.compare_exchange(
            NO_REQUEST,
            backend,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    fn switch_provider(&self, backend: u32) {
        let p = self.p();
        let b = self.b();
        let setup = settings();
        let allowed = setup.allowed_backend_choices;
        if backend != 0 && allowed != 0 && allowed & bit(backend) == 0 {
            SWITCH_RESULT.store(NOT_SUPPORTED, Ordering::SeqCst);
            return;
        }
        if setup.runtime_switching == 0 || (backend == DLSS && self.host().is_null()) {
            SWITCH_RESULT.store(NOT_SUPPORTED, Ordering::SeqCst);
            return;
        }
        if backend == ACTIVE_BACKEND.load(Ordering::SeqCst)
            && lock(&self.switch_fullscreen).is_none()
        {
            SWITCH_RESULT.store(0, Ordering::SeqCst);
            return;
        }
        let _guard = match PROVIDER_GUARD.try_write() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(guard)) => guard.into_inner(),
            Err(TryLockError::WouldBlock) => {
                // A pacer may still need this Present thread to finish the older frame.
                Self::defer_switch(backend);
                return;
            }
        };
        let previous = ACTIVE_BACKEND.load(Ordering::SeqCst);
        let mut result = if failed(self.drain()) {
            NOT_READY
        } else {
            self.retire_unpublished()
        };
        if result == OK
            && lock(&self.switch_fullscreen).is_none()
            && backend != 0
            && backend != DLSS
        {
            let mut probe_setup = self.generation_setup(backend);
            probe_setup.chain.hwnd = null_mut();
            result = match provider_api(p, backend).and_then(|api| api.create) {
                Some(create) => {
                    let mut probe = null_mut();
                    let mut probe_chain = null_mut();
                    let created = unsafe { create(&probe_setup, &mut probe, &mut probe_chain) };
                    let retired = self.discard_unpublished(backend, probe);
                    if retired == OK { created } else { retired }
                }
                _ => NOT_SUPPORTED,
            };
        }
        if result != OK {
            SWITCH_RESULT.store(result, Ordering::SeqCst);
            if lock(&self.switch_fullscreen).is_some() || lock(&self.unpublished_provider).is_some()
            {
                Self::defer_switch(backend);
            }
            return;
        }
        let outgoing = {
            let mut current = lock(&self.physical);
            // DXGI needs the old HWND chain released before its replacement. A query or
            // window operation may hold it on a thread that needs this Present to return.
            if current
                .as_ref()
                .is_some_and(|pair| Arc::strong_count(pair) != 1)
            {
                Self::defer_switch(backend);
                return;
            }
            current.take()
        };
        let mut fullscreen = lock(&self.switch_fullscreen).unwrap_or(0);
        let old_chain = outgoing
            .as_ref()
            .map_or(null_mut(), |pair| pair.chain.as_ptr());
        if !old_chain.is_null() {
            let _ = unsafe { (p.get_fullscreen)(old_chain, &mut fullscreen) };
            if fullscreen != 0 {
                let _ = unsafe { (p.set_fullscreen)(old_chain, 0, null_mut()) };
            }
        }
        log(&format!(
            "Generation switch identity facade={} old_physical={} old_provider={} requested={backend}",
            hex(b),
            hex(old_chain),
            hex(self.provider())
        ));
        lock(&STATUS).invalidate();
        drop(outgoing);
        ACTIVE_SESSION.store(null_mut(), Ordering::Release);
        HOST_PACED.store(false, Ordering::SeqCst);
        let retired = destroy_provider(self.api(), self.provider());
        if retired != OK {
            *lock(&self.switch_fullscreen) = Some(fullscreen);
            SWITCH_RESULT.store(retired, Ordering::SeqCst);
            Self::defer_switch(backend);
            return;
        }
        *lock(&self.switch_fullscreen) = None;
        self.provider.store(null_mut(), Ordering::Release);
        self.provider_backend.store(0, Ordering::Relaxed);
        CREATING.set(true);
        let host = self.host();
        if !host.is_null() {
            let _ = unsafe { (p.host_reflex_off)(host) };
            result = unsafe { (p.host_generation_load)(host, u32::from(backend == DLSS)) };
        }
        if result == OK {
            result = self.create_physical(backend);
        }
        let mut effective = backend;
        if result != OK {
            effective = previous;
            if !host.is_null() {
                let _ = unsafe { (p.host_generation_load)(host, u32::from(previous == DLSS)) };
            }
            if self.create_physical(previous) != OK {
                effective = 0;
                if self.create_physical(0) != OK {
                    if lock(&self.unpublished_provider).is_some() {
                        CREATING.set(false);
                        *lock(&self.switch_fullscreen) = Some(fullscreen);
                        SWITCH_RESULT.store(NOT_READY, Ordering::SeqCst);
                        Self::defer_switch(backend);
                        return;
                    }
                    self.faulted.store(true, Ordering::Release);
                }
            }
        }
        CREATING.set(false);
        let physical = self.physical();
        if fullscreen != 0
            && let Some(physical) = &physical
        {
            let _ = unsafe { (p.set_fullscreen)(physical.chain.as_ptr(), 1, null_mut()) };
        }
        ACTIVE_BACKEND.store(effective, Ordering::SeqCst);
        ACTIVE_SESSION.store(self.provider(), Ordering::Release);
        CHAIN_GENERATION.fetch_add(1, Ordering::SeqCst);
        let _ = self.publish_owner_status();
        {
            let mut timing = lock(&self.timing);
            timing.epoch = 0;
            timing.reset();
        }
        self.post_switch_frames.store(4, Ordering::Relaxed);
        SWITCH_RESULT.store(result, Ordering::SeqCst);
        log(&format!(
            "Generation runtime switch requested={backend} effective={effective} result={result} generation={}",
            generation()
        ));
        log(&format!(
            "Generation switch identity facade={} new_physical={} new_provider={} effective={effective}",
            hex(b),
            hex(physical
                .as_ref()
                .map_or(null_mut(), |pair| pair.chain.as_ptr())),
            hex(self.provider())
        ));
    }

    fn application_frame_complete(&self, value: u64) -> i32 {
        let p = self.p();
        let (ready, fence) = {
            let sync = lock(&self.sync);
            (sync.ready_event(), sync.completion().0)
        };
        if ready.is_null() {
            return S_OK;
        }
        let frame = self.application_serial.fetch_add(1, Ordering::Relaxed) + 1;
        self.application_fences[(frame % APPLICATION_FENCES as u64) as usize]
            .store(value, Ordering::Relaxed);
        let latency = u64::from(self.application_latency.load(Ordering::Relaxed));
        if frame < latency {
            return unsafe { (p.application_ready)(ready, null_mut(), 0) };
        }
        let available = self.application_fences
            [((frame - latency + 1) % APPLICATION_FENCES as u64) as usize]
            .load(Ordering::Relaxed);
        unsafe { (p.application_ready)(ready, fence, available) }
    }

    fn notify_presented(&self, event: &mut Event, result: i32) {
        event.completed = 1;
        event.result = result;
        unsafe { (self.p().observer_present)(event) };
        if let Some(callback) = settings().present_event {
            unsafe { callback(settings().user, event) };
        }
    }

    // Physical Present and the provider callbacks around it, after the frame's list was submitted.
    fn present_physical(
        &self,
        chain: Raw,
        event: &mut Event,
        mut sync: u32,
        flags: u32,
        ticks: &mut [u64; 6],
    ) -> i32 {
        let p = self.p();
        let setup = settings();
        let debug = setup.debug_timing != 0;
        self.receipt("physical-enter", S_OK);
        // Multi-refresh intervals throttle generated output as well as real frames. XeLL requires
        // interval 1 when VSync is enabled; DLSS-G also clamps it. Preserve the application's
        // VSync choice while normalizing its cadence at the owned physical chain.
        if sync > 1
            && let Some(state) = self.provider_status()
            && (state.effective_mode != RSF_FG_OFF
                || (state.pacing_owner == RSF_PACING_XELL
                    && state.effective_reflex != RSF_REFLEX_OFF))
        {
            if self.last_normalized_sync.swap(sync, Ordering::Relaxed) != sync {
                log(&format!(
                    "Physical Present interval normalized: requested={sync} effective=1 backend={} for FG/latency pacing",
                    self.provider_backend.load(Ordering::Relaxed)
                ));
            }
            sync = 1;
        }
        let host = self.host();
        let common = !host.is_null() && ACTIVE_BACKEND.load(Ordering::SeqCst) != DLSS;
        if common && unsafe { (p.host_present_common)(host, chain, sync, flags, 1) } != OK {
            log("presentation bridge: shared Streamline common Present begin refused");
            return E_FAIL;
        }
        if let Some(callback) = setup.latency_event {
            unsafe { callback(setup.user, event) };
        }
        if debug {
            ticks[3] = tick();
        }
        self.receipt("vendor-present-enter", S_OK);
        let result = unsafe { (p.physical_present)(chain, sync, flags) };
        self.receipt("vendor-present-done", result);
        if common && unsafe { (p.host_present_common)(host, chain, sync, flags, 0) } != OK {
            log("presentation bridge: shared Streamline common Present end refused");
        }
        if debug {
            ticks[4] = tick();
        }
        event.completed = 1;
        event.result = result;
        if let Some(callback) = setup.latency_event {
            unsafe { callback(setup.user, event) };
        }
        let provider = self.provider();
        if !provider.is_null()
            && let Some(after) = self.api().and_then(|api| api.after_present)
        {
            unsafe { after(provider) };
        }
        self.publish_status();
        self.receipt("retire-enter", S_OK);
        if let Some(retire) = setup.retire {
            unsafe { retire(setup.user, provider) };
        }
        self.receipt("retire-done", S_OK);
        result
    }

    fn prepare_callback(&self, context: Raw, command: Raw, backbuffer: &ComRef, sync: u32) {
        let setup = settings();
        if let Some(prepare) = setup.prepare {
            unsafe {
                prepare(
                    setup.user,
                    self.b(),
                    context,
                    command,
                    backbuffer.as_ptr(),
                    self.host().cast::<rsf_streamline_host>(),
                    self.provider(),
                    sync,
                )
            };
        }
    }

    fn present(&self, sync: u32, flags: u32) -> i32 {
        let p = self.p();
        let b = self.b();
        let setup = settings();
        let debug = setup.debug_timing != 0;
        let switching = setup.runtime_switching != 0;
        if self.faulted.load(Ordering::Acquire) {
            return DXGI_ERROR_DEVICE_REMOVED;
        }
        if flags & DXGI_PRESENT_TEST != 0 {
            return self.ensure_physical().map_or_else(
                |error| error,
                |physical| unsafe { (p.physical_present)(physical.chain.as_ptr(), sync, flags) },
            );
        }
        // An incomplete provider teardown has no physical chain to present on.
        if lock(&self.switch_fullscreen).is_some() {
            self.switch_if_requested();
        }
        self.receipt("application-enter", S_OK);
        let physical = match self.ensure_physical() {
            Ok(physical) => physical,
            Err(error) => return error,
        };
        // A published facade has all three pairs. Its final Release comes after this pin ends.
        let Some(commands) = self.command_slots() else {
            return E_UNEXPECTED;
        };
        let chain = physical.chain.as_ptr();
        let queue = physical.queue.as_ptr();
        // Legacy discard chains cannot request the flip-model tearing creation flag. Translate
        // VSync Off to the supported windowed immediate-present contract.
        let mut physical_flags = flags & !DXGI_PRESENT_ALLOW_TEARING;
        let mut fullscreen = 0;
        if sync == 0
            && self.tearing.load(Ordering::Relaxed)
            && !failed(unsafe { (p.get_fullscreen)(chain, &mut fullscreen) })
            && fullscreen == 0
        {
            physical_flags |= DXGI_PRESENT_ALLOW_TEARING;
        }
        let mut ticks = [0_u64; 6];
        if debug {
            ticks[0] = tick();
        }
        let mut event = Event {
            struct_size: size_of::<Event>() as u32,
            completed: 0,
            swapchain: b,
            sync_interval: sync,
            flags,
            result: 0,
            method: 0,
        };
        unsafe { (p.observer_present)(&mut event) };
        if let Some(callback) = setup.present_event {
            unsafe { callback(setup.user, &event) };
        }
        if let Some(callback) = setup.before_present {
            unsafe { callback(setup.user, b) };
        }
        self.receipt("application-callbacks-done", S_OK);
        if debug {
            ticks[1] = tick();
        }
        let mut result = E_FAIL;
        let mut submission_fenced = false;
        // The fence serial advances three times per frame, so it cannot also select the slot.
        let index = self.next_slot.load(Ordering::Relaxed);
        self.next_slot
            .store((index + 1) % SLOTS as u32, Ordering::Relaxed);
        let slot = &self.slot_fences[index as usize];
        let pair = &commands.0[index as usize];
        let (allocator, list) = (pair.allocator.as_ptr(), pair.list.as_ptr());
        // The facade pins these borrowed references through Present, just like the slot set.
        let (source_queue, (completion_fence, completion_event)) = {
            let sync = lock(&self.sync);
            (sync.queue(), sync.completion())
        };
        if self.native12() {
            // Streamline requires CreateCommandQueue through its upgraded device, so its queue is
            // not the engine queue. Join the engine's source work into it, and the copy back out.
            let joined = queue != source_queue;
            // Each step before the physical Present keeps its own HRESULT for the failure report.
            // Each attempt completes the application notification, including nonblocking refusal.
            let mut step = "source queue join";
            let mut stage = DXGI_ERROR_DEVICE_REMOVED;
            let source_joined = !joined
                || !failed(unsafe {
                    (p.join)(source_queue, queue, completion_fence, self.next_serial())
                });
            if source_joined {
                let mut step_index = 0;
                stage = unsafe {
                    (p.slot_reset)(
                        completion_fence,
                        completion_event,
                        allocator,
                        list,
                        slot.load(Ordering::Relaxed),
                        &mut step_index,
                    )
                };
                step = STEPS[(step_index as usize).min(STEPS.len() - 1)];
            } else {
                result = stage;
            }
            if source_joined && !failed(stage) {
                let render_index = self.render_index.load(Ordering::Relaxed);
                step = "physical GetBuffer";
                let backbuffer = self.acquire_backbuffer(&physical);
                match &backbuffer {
                    Err(error) => stage = *error,
                    Ok(backbuffer) => {
                        let source = switching.then(|| self.copy_source(render_index)).flatten();
                        if switching && source.is_none() {
                            step = "render buffer missing";
                            stage = E_UNEXPECTED;
                        } else {
                            if let Some(source) = &source {
                                unsafe {
                                    (p.record_copy)(b, list, backbuffer.as_ptr(), source.as_ptr())
                                };
                            }
                            let command = unsafe { (p.command_list)(b, queue, list) };
                            if command.is_null() {
                                step = "command list unwrap";
                                stage = E_FAIL;
                            } else {
                                self.prepare_callback(null_mut(), command, backbuffer, sync);
                                self.publish_status();
                                step = "command list Close";
                                stage = unsafe { (p.close_execute)(queue, list, command) };
                            }
                            if !failed(stage) {
                                if joined
                                    && failed(unsafe {
                                        (p.join)(
                                            queue,
                                            source_queue,
                                            completion_fence,
                                            self.next_serial(),
                                        )
                                    })
                                {
                                    // The submitted list stays covered by the slot fence below.
                                    step = "present queue join";
                                    stage = DXGI_ERROR_DEVICE_REMOVED;
                                    result = stage;
                                } else {
                                    step = "physical Present";
                                    result = self.present_physical(
                                        chain,
                                        &mut event,
                                        sync,
                                        physical_flags,
                                        &mut ticks,
                                    );
                                    stage = result;
                                }
                            }
                        }
                    }
                }
                let fence = self.next_serial();
                slot.store(fence, Ordering::Relaxed);
                submission_fenced =
                    !failed(unsafe { (p.signal_slot)(queue, completion_fence, fence) });
                if !submission_fenced {
                    step = "slot fence signal";
                    stage = DXGI_ERROR_DEVICE_REMOVED;
                    result = stage;
                }
                drop(backbuffer);
            }
            if failed(result) {
                let step = CString::new(step).unwrap_or_default();
                unsafe {
                    (p.log_present_failure)(
                        b,
                        step.as_ptr(),
                        stage,
                        self.render_index.load(Ordering::Relaxed),
                    )
                };
            }
            self.notify_presented(&mut event, result);
            drop(physical);
            if !failed(result) && switching {
                let available = self.application_frame_complete(slot.load(Ordering::Relaxed));
                if failed(available) {
                    return available;
                }
                self.application_presents.fetch_add(1, Ordering::Relaxed);
                let count = self.facts().buffer_count.max(1);
                let next = (self.render_index.load(Ordering::Relaxed) + 1) % count;
                self.render_index.store(next, Ordering::Relaxed);
                self.switch_if_requested();
            }
            // Only physical Present supplies this result. A failed slot signal replaces it
            // with DEVICE_REMOVED because the submitted list cannot be reused safely.
            if failed(result) && !(submission_fenced && result == DXGI_ERROR_WAS_STILL_DRAWING) {
                unsafe {
                    (p.report_device_failure)(b, result, ACTIVE_BACKEND.load(Ordering::SeqCst))
                };
                self.faulted.store(true, Ordering::Release);
            }
            return result;
        }
        self.receipt("d3d11-slot-enter", S_OK);
        let mut step_index = 0;
        if !failed(unsafe {
            (p.slot_reset)(
                completion_fence,
                completion_event,
                allocator,
                list,
                slot.load(Ordering::Relaxed),
                &mut step_index,
            )
        }) {
            if debug {
                ticks[2] = tick();
            }
            // Real DXGI backbuffers (the Off path) must be written on their creation queue.
            // The separate interop queue stays with SR; the final upload uses the present queue.
            // Submit the D3D11 signal with its preceding work before D3D12 waits on it.
            if !failed(unsafe { (p.upload_wait)(b, queue, self.next_serial()) })
                && let Ok(backbuffer) = self.acquire_backbuffer(&physical)
                && let Some(source) = self.copy_source(0)
            {
                // The application writes slot zero. Generated frames use the physical backbuffer.
                unsafe { (p.record_copy)(b, list, backbuffer.as_ptr(), source.as_ptr()) };
                let command = unsafe { (p.command_list)(b, queue, list) };
                if !command.is_null() {
                    self.prepare_callback(unsafe { (p.context11)(b) }, command, &backbuffer, sync);
                    self.publish_status();
                    if !failed(unsafe { (p.close_execute)(queue, list, command) })
                        && !failed(unsafe { (p.upload_return)(b, queue, self.next_serial()) })
                    {
                        result = self.present_physical(
                            chain,
                            &mut event,
                            sync,
                            physical_flags,
                            &mut ticks,
                        );
                    }
                }
            }
            let fence = self.next_serial();
            slot.store(fence, Ordering::Relaxed);
            submission_fenced = !failed(unsafe { (p.signal_slot)(queue, completion_fence, fence) });
            if !submission_fenced {
                result = DXGI_ERROR_DEVICE_REMOVED;
            }
        }
        self.receipt("application-complete", result);
        self.notify_presented(&mut event, result);
        drop(physical);
        let frames = self.post_switch_frames.load(Ordering::Relaxed);
        if frames != 0 {
            self.post_switch_frames.store(frames - 1, Ordering::Relaxed);
        }
        if debug && !failed(result) {
            ticks[5] = tick();
            self.trace_pacing(&ticks, index, sync, physical_flags);
        }
        if !failed(result) && switching {
            self.switch_if_requested();
        }
        if failed(result) && !(submission_fenced && result == DXGI_ERROR_WAS_STILL_DRAWING) {
            self.faulted.store(true, Ordering::Release);
        }
        result
    }

    fn trace_pacing(&self, ticks: &[u64; 6], index: u32, sync: u32, physical_flags: u32) {
        let setup = settings();
        if setup.debug_timing == 0 || setup.log.is_none() || self.provider().is_null() {
            return;
        }
        let Some(state) = self.provider_status() else {
            return;
        };
        let mut timing = lock(&self.timing);
        if timing.tick_ms == 0.0 {
            return;
        }
        let tick_ms = timing.tick_ms;
        let end = ticks[5];
        if timing.epoch == 0 {
            timing.epoch = end;
            timing.sdk_start = state.total_presented;
            timing.previous = ticks[0];
            return;
        }
        let interval = ticks[0].wrapping_sub(timing.previous) as f64 * tick_ms;
        timing.previous = ticks[0];
        timing.interval_min = if timing.frames != 0 {
            timing.interval_min.min(interval)
        } else {
            interval
        };
        timing.interval_max = timing.interval_max.max(interval);
        timing.frames += 1;
        timing.slots |= 1 << index;
        for i in 0..5 {
            let elapsed = ticks[i + 1].wrapping_sub(ticks[i]) as f64 * tick_ms;
            timing.sum[i] += elapsed;
            timing.max[i] = timing.max[i].max(elapsed);
        }
        let elapsed = end.wrapping_sub(timing.epoch) as f64 * tick_ms;
        if elapsed < 1000.0 {
            return;
        }
        let frames = timing.frames as f64;
        let mut text = format!(
            "FG pacing frames={} rendered={:.1} presented={:.1} interval_ms={:.2}..{:.2}",
            timing.frames,
            frames * 1000.0 / elapsed,
            state.total_presented.wrapping_sub(timing.sdk_start) as f64 * 1000.0 / elapsed,
            timing.interval_min,
            timing.interval_max
        );
        for (i, name) in ["callbacks", "slot_wait", "upload", "present", "sdk"]
            .iter()
            .enumerate()
        {
            let _ = write!(
                text,
                " {name}_ms={:.2}/{:.2}",
                timing.sum[i] / frames,
                timing.max[i]
            );
        }
        let _ = write!(
            text,
            " slots=0x{:x} sync={sync} active={} physical_flags=0x{physical_flags:x}",
            timing.slots, state.active
        );
        timing.epoch = end;
        timing.sdk_start = state.total_presented;
        timing.reset();
        drop(timing);
        log(&text);
    }

    fn set_fullscreen(&self, fullscreen: i32, target: Raw) -> i32 {
        let physical = match self.ensure_physical() {
            Ok(physical) => physical,
            Err(error) => return error,
        };
        if failed(self.drain()) {
            return E_FAIL;
        }
        unsafe { (self.p().set_fullscreen)(physical.chain.as_ptr(), fullscreen, target) }
    }

    fn resize(
        &self,
        count: u32,
        mut width: u32,
        mut height: u32,
        mut format: u32,
        flags: u32,
    ) -> i32 {
        let p = self.p();
        let b = self.b();
        if count > MAX_BUFFERS || self.faulted.load(Ordering::Acquire) {
            return DXGI_ERROR_INVALID_CALL;
        }
        let physical = match self.ensure_physical() {
            Ok(physical) => physical,
            Err(error) => return error,
        };
        let mut facts = self.facts();
        if (width == 0 || height == 0)
            && unsafe { (p.client_size)(facts.window, &mut width, &mut height) } == 0
        {
            return E_FAIL;
        }
        if width == 0 || height == 0 {
            return DXGI_ERROR_INVALID_CALL;
        }
        if format == 0 {
            format = facts.format;
        }
        if failed(self.drain()) {
            return E_FAIL;
        }
        let native12 = self.native12();
        let switching = switching();
        if (!native12 || switching) && self.buffers_referenced() {
            return DXGI_ERROR_INVALID_CALL;
        }
        if count != 0 {
            facts.buffer_count = count;
        }
        facts.width = width;
        facts.height = height;
        facts.format = format;
        // The application images are independent allocations, not references to
        // DXGI's physical backbuffers. Keep both batches until DXGI accepts the resize.
        let replacement = if !native12 || switching {
            match self.allocate_application_buffers(&facts) {
                Ok(batch) => Some(batch),
                Err(error) => return error,
            }
        } else {
            None
        };
        let provider = self.provider();
        if !provider.is_null() {
            let configured = self
                .api()
                .and_then(|api| api.configure)
                .map_or(NOT_SUPPORTED, |configure| unsafe {
                    configure(provider, &FG_OFF)
                });
            if configured != OK {
                return E_FAIL;
            }
        }
        self.publish_status();
        // With runtime switching the application renders into its own buffers, never the physical
        // chain, so the physical count stays as created (0 keeps it). Changing it under an FG
        // provider's proxy chain broke its back-buffer index after an application count change.
        let physical_count = match (native12, switching) {
            (false, _) => 3,
            (true, true) => 0,
            (true, false) => count,
        };
        let result = unsafe {
            (p.resize_physical)(
                b,
                physical.chain.as_ptr(),
                physical_count,
                width,
                height,
                format,
            )
        };
        if failed(result) {
            return result;
        }
        if let Some(batch) = replacement {
            *lock(&self.buffers) = batch;
        }
        unsafe { (p.set_description)(b, facts.buffer_count, width, height, format, flags) };
        if native12 && switching {
            self.reset_application_frames();
        } else if !native12 {
            self.render_index.store(0, Ordering::Relaxed);
        }
        S_OK
    }

    // Joins vendor retirement on the provider queue, optionally runs a closed list there, and
    // signals one private fence that covers both. Present thread, between Presents.
    fn retire(&self, session: Raw, list: Raw, fence: &mut Raw, value: &mut u64) -> i32 {
        let p = self.p();
        let b = self.b();
        let provider = self.provider();
        let Some(retirement) = self.api().and_then(|api| api.retirement) else {
            return NOT_READY;
        };
        if session.is_null() || session != provider {
            return NOT_READY;
        }
        let Some(physical) = self.physical() else {
            return NOT_READY;
        };
        // The provider guard pins the session. This lock also protects the fence and serial
        // against overlapping retire calls, including a callback that reenters on this thread.
        let mut state = match self.provider_retirement.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return NOT_READY,
        };
        if state.fence.is_none() {
            let mut acquired = null_mut();
            let result = unsafe { (p.create_retirement_fence)(b, &mut acquired) };
            let acquired = unsafe { ComRef::from_owned(acquired) };
            if failed(result) || acquired.is_none() {
                return INIT_FAILED;
            }
            state.fence = acquired;
        }
        let retired = state.fence.as_ref().map_or(null_mut(), ComRef::as_ptr);
        let Some(next) = state
            .serial
            .checked_add(1)
            .filter(|&value| value != u64::MAX)
        else {
            return NOT_READY;
        };
        let mut vendor = rsf_fg_retirement {
            struct_size: size_of::<rsf_fg_retirement>() as u32,
            fence: null_mut(),
            value: 0,
        };
        let result = unsafe { retirement(session, &mut vendor) };
        if result != OK {
            return result;
        }
        let result = unsafe {
            (p.retire_submit)(
                b,
                physical.queue.as_ptr(),
                retired,
                vendor.fence,
                vendor.value,
                list,
                next,
            )
        };
        if result != OK {
            return result;
        }
        state.serial = next;
        *fence = retired;
        *value = next;
        OK
    }

    // The facade's last reference is gone. Unpublish, drain and release or retain.
    fn retire_owner(&self) {
        let p = self.p();
        let b = self.b();
        let published = {
            // Drain existing shared provider leases before unpublishing. Release this lock
            // before the GPU drain; new leases cannot obtain this session.
            let _guard = PROVIDER_GUARD
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            let published = ACTIVE_CHAIN.load(Ordering::Acquire) == ptr::from_ref(self).cast_mut();
            if published {
                RETIREMENT_PENDING.store(true, Ordering::Release);
                lock(&STATUS).invalidate();
                OUTER_ALIAS.store(null_mut(), Ordering::Release);
                lock(&ACTIVE_GRAPHICS).0 = Graphics::EMPTY;
                ACTIVE_CHAIN.store(null_mut(), Ordering::Release);
                ACTIVE_BRIDGE.store(null_mut(), Ordering::Release);
                ACTIVE_SESSION.store(null_mut(), Ordering::Release);
                HOST_PACED.store(false, Ordering::SeqCst);
            }
            published
        };
        // On device failure retain GPU-reachable storage for the process rather than free it.
        if failed(self.drain()) || self.retire_unpublished() != OK {
            // Initializing owners can fail after creating a provider but before publication.
            RETIREMENT_PENDING.store(true, Ordering::Release);
            log("presentation bridge: retirement failed; retaining provider and shared inputs");
            std::mem::forget(self.take_application_buffers());
            std::mem::forget(self.take_physical());
            // Submitted lists and their allocators can still be GPU-reachable.
            std::mem::forget(self.take_command_slots());
            // A refused wait can leave the completion and application events armed.
            std::mem::forget(std::mem::take(&mut *lock(&self.sync)));
            std::mem::forget(lock(&self.provider_retirement).fence.take());
            unsafe { (p.retain_all)(b) };
            return;
        }
        let checked = self.api().is_some_and(|api| api.destroy_checked.is_some());
        // Keep native buffers on the facade until teardown for a checked provider.
        let buffers = self.take_application_buffers();
        let retained_buffers = if self.native12() || checked {
            Some(buffers)
        } else {
            drop(buffers);
            None
        };
        let physical = self.take_physical();
        if !checked {
            unsafe { (p.release_presentation)(b) };
        }
        drop(physical);
        if destroy_provider(self.api(), self.provider()) != OK {
            RETIREMENT_PENDING.store(true, Ordering::Release);
            log("presentation bridge: provider teardown refused; retaining shared inputs");
            std::mem::forget(retained_buffers);
            std::mem::forget(self.take_command_slots());
            std::mem::forget(std::mem::take(&mut *lock(&self.sync)));
            std::mem::forget(lock(&self.provider_retirement).fence.take());
            unsafe { (p.retain_all)(b) };
            return;
        }
        self.provider.store(null_mut(), Ordering::Release);
        let direct_buffers = if self.native12() {
            retained_buffers
        } else {
            drop(retained_buffers);
            None
        };
        if checked {
            unsafe { (p.release_presentation)(b) };
        }
        // The host also serves SR and CPU callbacks and stays loaded while it is the published one.
        let host = self.host();
        let private = !host.is_null() && ACTIVE_HOST.load(Ordering::Acquire) != host;
        let mut sync = std::mem::take(&mut *lock(&self.sync));
        let retired = lock(&self.provider_retirement).fence.take();
        drop(sync.completion_event.take());
        drop(sync.application_ready.take());
        if private {
            // The slot lists belong to the private host's device and end before the host,
            // after its interop queue and in slot order.
            drop(sync.interop_queue.take());
            self.release_command_slots(false);
            drop(sync.completion_fence.take());
            drop(retired);
            unsafe { (p.release_host)(b) };
        } else if !host.is_null() {
            // Shared SR callbacks keep using the process-owned interop queue.
            std::mem::forget(sync.interop_queue.take());
        }
        drop(direct_buffers);
        if published {
            CHAIN_GENERATION.fetch_add(1, Ordering::SeqCst);
            RETIREMENT_PENDING.store(false, Ordering::Release);
        }
        if !private {
            // Preserve the former reverse member release order after the events close.
            self.release_command_slots(true);
            drop(retired);
            drop(sync.completion_fence.take());
            unsafe { (p.release_direct_graphics)(b) };
            drop(sync.interop_queue.take());
        }
    }

    fn refuse(&self) {
        self.faulted.store(true, Ordering::Release);
        SCOPED_ADMITTED.store(false, Ordering::Release);
        OUTER_ALIAS.store(null_mut(), Ordering::Release);
    }
}

fn tick() -> u64 {
    let mut now = 0_i64;
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now) };
    now as u64
}

// The caller holds the provider read lease across any use of the facade.
fn active() -> Option<&'static Owner> {
    unsafe { ACTIVE_CHAIN.load(Ordering::Acquire).as_ref() }
}

unsafe fn owner<'a>(chain: *mut Chain) -> Option<&'a Owner> {
    unsafe { chain.cast::<Owner>().as_ref() }
}

// Creation and admission --------------------------------------------------------------------

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_set_scoped(scoped: u32) {
    SCOPED.store(scoped != 0, Ordering::Release);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_capture(window: Raw) -> u32 {
    let captured = SCOPE_CAPTURED.with_borrow(|value| !value.object.is_null());
    u32::from(
        !SCOPED.load(Ordering::Acquire)
            || (SCOPE_TOKEN.get() != 0 && SCOPE_WINDOW.get() == window && !captured),
    )
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_captured(platform: *const Platform, bridge: Raw) {
    let Some(p) = (unsafe { platform.as_ref() }) else {
        return;
    };
    if !SCOPED.load(Ordering::Acquire) || bridge.is_null() {
        return;
    }
    unsafe { (p.add_ref)(bridge) };
    let previous = SCOPE_CAPTURED.replace(Captured {
        object: bridge,
        release: Some(p.release),
    });
    drop(previous);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_active_bridge() -> Raw {
    ACTIVE_BRIDGE.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_create(
    platform: *const Platform,
    factory: Raw,
    source: Raw,
    request: *const rsf_d3d11p_request,
    desc: *const c_void,
    out: *mut Raw,
) -> i32 {
    let (Some(p), Some(request)) = (unsafe { platform.as_ref() }, unsafe { request.as_ref() })
    else {
        return DXGI_ERROR_UNSUPPORTED;
    };
    let setup = settings();
    let Some(accept) = setup.accept_window else {
        return DXGI_ERROR_UNSUPPORTED;
    };
    if unsafe { accept(setup.user, request.window) } == 0 {
        return DXGI_ERROR_UNSUPPORTED;
    }
    if CREATING.get()
        || !ACTIVE_CHAIN.load(Ordering::Acquire).is_null()
        || out.is_null()
        || request.sample_count != 1
        || request.width == 0
        || request.height == 0
        || unsafe { (p.window_valid)(request.window) } == 0
        || !FORMATS.contains(&request.format)
    {
        return DXGI_ERROR_UNSUPPORTED;
    }
    if source.is_null() || request.buffer_count == 0 || request.buffer_count > MAX_BUFFERS {
        return DXGI_ERROR_UNSUPPORTED;
    }
    let _creation = lock(&CREATION_GUARD);
    if !ACTIVE_CHAIN.load(Ordering::Acquire).is_null() || RETIREMENT_PENDING.load(Ordering::Acquire)
    {
        return DXGI_ERROR_UNSUPPORTED;
    }
    CREATING.set(true);
    let owner = crate::allocate_handle(Owner::new(platform));
    let result = if owner.is_null() {
        E_OUTOFMEMORY
    } else {
        let bridge = unsafe { (p.bridge_new)(owner.cast()) };
        if bridge.is_null() {
            drop(unsafe { Box::from_raw(owner) });
            E_OUTOFMEMORY
        } else {
            let value = unsafe { &*owner };
            value.bridge.store(bridge, Ordering::Release);
            let result = value.initialize(factory, source, request, desc);
            if failed(result) {
                // Not published; never enter an external Release hook.
                unsafe { (p.bridge_release_owned)(bridge) };
            } else {
                log(&format!(
                    "presentation bridge: application chain {}x{} refresh={}/{} windowed={} buffers={} flags=0x{:x} effect={}",
                    request.width,
                    request.height,
                    request.refresh_numerator,
                    request.refresh_denominator,
                    request.windowed as i32,
                    request.buffer_count,
                    request.flags,
                    request.swap_effect
                ));
                unsafe { *out = bridge };
            }
            result
        }
    };
    CREATING.set(false);
    result
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_creation_begin(
    platform: *const Platform,
    window: Raw,
    token: *mut u64,
    installed: u32,
) -> i32 {
    let (Some(p), Some(token)) = (unsafe { platform.as_ref() }, unsafe { token.as_mut() }) else {
        return E_INVALIDARG;
    };
    if *token != 0 || window.is_null() || unsafe { (p.window_valid)(window) } == 0 {
        return E_INVALIDARG;
    }
    if !SCOPED.load(Ordering::Acquire)
        || installed == 0
        || SCOPE_TOKEN.get() != 0
        || CREATING.get()
        || !ACTIVE_CHAIN.load(Ordering::Acquire).is_null()
    {
        return DXGI_ERROR_INVALID_CALL;
    }
    let selected = CREATION_TOKENS.fetch_add(1, Ordering::SeqCst) + 1;
    if ACTIVE_CREATION_TOKEN
        .compare_exchange(0, selected, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return DXGI_ERROR_INVALID_CALL;
    }
    SCOPE_WINDOW.set(window);
    SCOPE_TOKEN.set(selected);
    *token = selected;
    S_OK
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_creation_end(
    platform: *const Platform,
    window: Raw,
    token: u64,
    device: Raw,
    context: Raw,
    outer: Raw,
) -> i32 {
    let Some(p) = (unsafe { platform.as_ref() }) else {
        return E_INVALIDARG;
    };
    if token == 0 || token != SCOPE_TOKEN.get() || window != SCOPE_WINDOW.get() {
        return E_INVALIDARG;
    }
    // Close even when native creation failed or its outputs refuse.
    let captured = SCOPE_CAPTURED.replace(Captured {
        object: null_mut(),
        release: None,
    });
    SCOPE_TOKEN.set(0);
    SCOPE_WINDOW.set(null_mut());
    ACTIVE_CREATION_TOKEN.store(0, Ordering::SeqCst);
    let inner = captured.object;
    let result = if inner.is_null() || device.is_null() || context.is_null() || outer.is_null() {
        DXGI_ERROR_UNSUPPORTED
    } else if inner != ACTIVE_BRIDGE.load(Ordering::Acquire)
        || failed(unsafe { (p.verify_outer)(inner, window, device, context, outer) })
    {
        E_INVALIDARG
    } else {
        OUTER_ALIAS.store(outer, Ordering::Release);
        SCOPED_ADMITTED.store(true, Ordering::Release);
        log(
            "presentation bridge: scoped inner owner and final outer chain canonical identity verified; outer Present retained",
        );
        S_OK
    };
    if failed(result)
        && !inner.is_null()
        && let Some(owner) = unsafe { owner((p.bridge_chain)(inner)) }
    {
        owner.refuse();
    }
    drop(captured);
    result
}

// Facade methods ----------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_chain_destroy(chain: *mut Chain) {
    let owner = chain.cast::<Owner>();
    if owner.is_null() {
        return;
    }
    unsafe { (*owner).retire_owner() };
    drop(unsafe { Box::from_raw(owner) });
}

macro_rules! facade {
    ($(#[$attr:meta])* $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty, $missing:expr, |$owner:ident| $body:expr) => {
        $(#[$attr])*
        #[unsafe(no_mangle)]
        pub(crate) unsafe extern "C" fn $name(chain: *mut Chain $(, $arg: $ty)*) -> $ret {
            match unsafe { owner(chain) } {
                Some($owner) => $body,
                None => $missing,
            }
        }
    };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_acquire_physical(
    chain: *mut Chain,
    initialize: u32,
    lease: *mut *const Physical,
    physical: *mut Raw,
) -> i32 {
    let (Some(lease), Some(physical)) = (unsafe { lease.as_mut() }, unsafe { physical.as_mut() })
    else {
        return E_POINTER;
    };
    *lease = ptr::null();
    *physical = null_mut();
    let Some(owner) = (unsafe { owner(chain) }) else {
        return E_FAIL;
    };
    let pair = if initialize == 0 {
        owner.physical().ok_or(DXGI_ERROR_WAS_STILL_DRAWING)
    } else {
        owner.ensure_physical()
    };
    match pair {
        Ok(pair) => {
            *physical = pair.chain.as_ptr();
            *lease = Arc::into_raw(pair);
            S_OK
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_release_physical(lease: *const Physical) {
    if !lease.is_null() {
        drop(unsafe { Arc::from_raw(lease) });
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_get_buffer(
    chain: *mut Chain,
    index: u32,
    iid: *const c_void,
    out: *mut Raw,
) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    // Failed admission or deferred physical creation must clear the caller's output.
    unsafe { out.write(null_mut()) };
    let Some(owner) = (unsafe { owner(chain) }) else {
        return E_FAIL;
    };
    unsafe { owner.get_buffer(index, iid, out) }
}
facade!(
    #[inline(never)]
    rsf_core_d3d11p_present(sync: u32, flags: u32) -> i32,
    E_FAIL,
    |o| o.present(sync, flags)
);
facade!(
    #[inline(never)]
    rsf_core_d3d11p_set_fullscreen(fullscreen: i32, target: Raw) -> i32,
    E_FAIL,
    |o| o.set_fullscreen(fullscreen, target)
);
facade!(
    #[inline(never)]
    rsf_core_d3d11p_resize(count: u32, width: u32, height: u32, format: u32, flags: u32) -> i32,
    E_FAIL,
    |o| o.resize(count, width, height, format, flags)
);
facade!(rsf_core_d3d11p_faulted() -> u32, 1, |o| u32::from(o.faulted.load(Ordering::Acquire)));
facade!(rsf_core_d3d11p_physical_pending() -> u32, 0,
    |o| u32::from(o.creation_pending()));
facade!(rsf_core_d3d11p_render_index() -> u32, 0, |o| o.render_index.load(Ordering::Relaxed));
facade!(rsf_core_d3d11p_application_presents() -> u32, 0,
    |o| o.application_presents.load(Ordering::Relaxed));
facade!(
    rsf_core_d3d11p_set_max_latency(value: u32) -> i32,
    DXGI_ERROR_INVALID_CALL,
    |o| {
        if lock(&o.sync).ready_event().is_null()
            || value == 0
            || value as usize > APPLICATION_FENCES
        {
            DXGI_ERROR_INVALID_CALL
        } else {
            o.application_latency
                .store(value.min(o.facts().buffer_count), Ordering::Relaxed);
            S_OK
        }
    }
);
facade!(
    rsf_core_d3d11p_max_latency(value: *mut u32) -> i32,
    DXGI_ERROR_INVALID_CALL,
    |o| {
        if lock(&o.sync).ready_event().is_null() || value.is_null() {
            DXGI_ERROR_INVALID_CALL
        } else {
            unsafe { *value = o.application_latency.load(Ordering::Relaxed) };
            S_OK
        }
    }
);
facade!(
    rsf_core_d3d11p_duplicate_application_ready() -> Raw,
    null_mut(),
    |o| {
        let ready = lock(&o.sync).ready_event();
        if ready.is_null() {
            null_mut()
        } else {
            unsafe { (o.p().duplicate_event)(ready) }
        }
    }
);

// Public bridge exports ---------------------------------------------------------------------

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_host() -> Raw {
    ACTIVE_HOST.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_graphics(graphics: *mut Graphics) -> i32 {
    if !unsafe { has_capacity(graphics) } {
        return 0;
    }
    let snapshot = lock(&ACTIVE_GRAPHICS).0;
    if snapshot.queue.is_null() {
        return 0;
    }
    unsafe { graphics.write(snapshot) };
    1
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_queue(platform: *const Platform) -> Raw {
    let Some(_guard) = read_guard() else {
        return null_mut();
    };
    if platform.is_null() {
        return null_mut();
    }
    active()
        .and_then(Owner::physical)
        .map_or(null_mut(), |physical| physical.queue.as_ptr())
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_source_queue(platform: *const Platform) -> Raw {
    // The same lock fences owner unpublication. A transition must not block the producer.
    let Some(_guard) = read_guard() else {
        return null_mut();
    };
    match (unsafe { platform.as_ref() }, active()) {
        (Some(p), Some(owner)) => {
            let queue = lock(&owner.sync).queue();
            unsafe { (p.source_queue)(owner.b(), queue) }
        }
        _ => null_mut(),
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_command_list(
    platform: *const Platform,
    list: Raw,
) -> Raw {
    let Some(_guard) = read_guard() else {
        return null_mut();
    };
    if list.is_null() {
        return null_mut();
    }
    let (Some(p), Some(owner)) = (unsafe { platform.as_ref() }, active()) else {
        return null_mut();
    };
    let physical = owner.physical();
    let queue = physical
        .as_ref()
        .map_or(null_mut(), |pair| pair.queue.as_ptr());
    // Pending creation used the native-list path when a host was already available.
    unsafe { (p.command_list)(owner.b(), queue, list) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_is_owner(
    platform: *const Platform,
    swapchain: Raw,
) -> i32 {
    let active = ACTIVE_BRIDGE.load(Ordering::Acquire);
    let Some(p) = (unsafe { platform.as_ref() }) else {
        return 0;
    };
    if swapchain.is_null()
        || active.is_null()
        || (SCOPED.load(Ordering::Acquire) && !SCOPED_ADMITTED.load(Ordering::Acquire))
    {
        return 0;
    }
    if active == swapchain {
        return 1;
    }
    if OUTER_ALIAS.load(Ordering::Acquire) != swapchain {
        return 0;
    }
    // The facade publishes its canonical COM interface address. Query only the
    // caller's live receiver, since the cached address can retire during the call.
    let identity = unsafe { (p.canonical_identity)(swapchain) };
    i32::from(
        identity == active
            && ACTIVE_BRIDGE.load(Ordering::Acquire) == active
            && OUTER_ALIAS.load(Ordering::Acquire) == swapchain
            && rsf_core_d3d11p_has_owner() != 0,
    )
}

fn dispatch_admit(receiver: Raw) -> u32 {
    let active = ACTIVE_BRIDGE.load(Ordering::Acquire);
    if receiver.is_null() || receiver != active {
        0
    } else if SCOPED.load(Ordering::Acquire) && !SCOPED_ADMITTED.load(Ordering::Acquire) {
        2
    } else {
        1
    }
}

unsafe fn dispatch_result_slot(request: *mut rsf_d3d11_chain_dispatch) -> *mut i32 {
    // The C union's HRESULT and reference count have the same 32-bit storage.
    unsafe { (&raw mut (*request).result).cast::<i32>() }
}

unsafe extern "C" fn dispatch_invoke_owned(request: *mut rsf_d3d11_chain_dispatch) -> i32 {
    let request = unsafe { &*request };
    let chain = unsafe { facade::rsf_d3d11p_facade_chain(request.receiver) };
    // Call private continuations because the published COM methods may be detoured.
    unsafe {
        match request.method {
            RSF_D3D11_DISPATCH_PRESENT => {
                let args = request.args.present;
                rsf_core_d3d11p_present(chain, args.sync_interval, args.flags)
            }
            RSF_D3D11_DISPATCH_PRESENT1 => {
                let args = request.args.present1;
                dxgi::rsf_d3d11p_facade_present1(
                    chain,
                    args.sync_interval,
                    args.flags,
                    args.parameters.cast(),
                )
            }
            RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE => {
                let args = request.args.fullscreen;
                rsf_core_d3d11p_set_fullscreen(chain, args.fullscreen as i32, args.target)
            }
            RSF_D3D11_DISPATCH_RESIZE_BUFFERS => {
                let args = request.args.resize_buffers;
                rsf_core_d3d11p_resize(
                    chain,
                    args.buffer_count,
                    args.width,
                    args.height,
                    args.format,
                    args.flags,
                )
            }
            RSF_D3D11_DISPATCH_SET_COLOR_SPACE1 => facade::rsf_d3d11p_facade_color_space(
                request.receiver,
                request.args.color_space.color_space,
            ),
            RSF_D3D11_DISPATCH_RESIZE_BUFFERS1 => {
                let args = request.args.resize_buffers1;
                dxgi::rsf_d3d11p_facade_resize1(
                    chain,
                    args.buffer_count,
                    args.width,
                    args.height,
                    args.format,
                    args.flags,
                    args.node_masks,
                    args.present_queues,
                )
            }
            _ => E_FAIL,
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_dispatch_chain(request: *mut rsf_d3d11_chain_dispatch) {
    unsafe {
        rsf_core_d3d11p_dispatch_owned(
            request,
            facade::rsf_d3d11p_facade_release,
            dispatch_invoke_owned,
        )
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_dispatch_present(
    receiver: Raw,
    sync_interval: u32,
    flags: u32,
    result: *mut i32,
) -> i32 {
    if result.is_null() {
        return 0;
    }
    // The legacy call has no dispatch header; the core takes an exact receiver.
    let mut request: rsf_d3d11_chain_dispatch = unsafe { std::mem::zeroed() };
    request.receiver = receiver;
    request.method = RSF_D3D11_DISPATCH_PRESENT;
    request.args.present = rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_1 {
        sync_interval,
        flags,
    };
    unsafe { rsf_d3d11p_dispatch_chain(&mut request) };
    if request.handled != 0 {
        unsafe { result.write(request.result.hresult) };
    }
    request.handled as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d11p_dispatch_owned(
    request: *mut rsf_d3d11_chain_dispatch,
    release_owned: unsafe extern "C" fn(Raw) -> u32,
    invoke_owned: unsafe extern "C" fn(*mut rsf_d3d11_chain_dispatch) -> i32,
) {
    if request.is_null() {
        return;
    }
    let receiver = unsafe { (*request).receiver };
    let admission = dispatch_admit(receiver);
    if admission == 0 {
        return;
    }
    unsafe { (*request).handled = 1 };
    let result = unsafe { dispatch_result_slot(request) };
    if unsafe { (*request).method } == RSF_D3D11_DISPATCH_RELEASE {
        // Release consumes the caller's reference, including during scope refusal.
        unsafe { result.write(release_owned(receiver) as i32) };
    } else if admission == 2 {
        unsafe { result.write(DXGI_ERROR_INVALID_CALL) };
    } else {
        // The caller holds a live COM reference. No host lock spans the owned call.
        let _lease = unsafe { ComRef::retain(receiver) };
        unsafe { result.write(invoke_owned(request)) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d11p_get_api(out: *mut u8, table: *const u8) -> i32 {
    // The native export thunk supplies the typed function table. The Rust owner
    // treats the client storage as raw bytes until size and version are accepted.
    const TABLE_SIZE: usize = 184;
    if out.is_null() || (unsafe { out.cast::<u32>().read() } as usize) < TABLE_SIZE {
        return INVALID_ARGUMENT;
    }
    if unsafe { out.cast::<u32>().add(1).read() } != 1 {
        return ABI_MISMATCH;
    }
    unsafe { ptr::copy_nonoverlapping(table, out, TABLE_SIZE) };
    OK
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_has_owner() -> i32 {
    i32::from(
        !ACTIVE_BRIDGE.load(Ordering::Acquire).is_null()
            && (!SCOPED.load(Ordering::Acquire) || SCOPED_ADMITTED.load(Ordering::Acquire)),
    )
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_provider(
    platform: *const Platform,
) -> *const Provider {
    unsafe { platform.as_ref() }
        .and_then(|p| provider_api(p, ACTIVE_BACKEND.load(Ordering::SeqCst)))
        .map_or(ptr::null(), ptr::from_ref)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_status_get(status: *mut PresentStatus) -> i32 {
    unsafe { lock(&STATUS).read(status) }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_session() -> Raw {
    ACTIVE_SESSION.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_backend() -> u32 {
    ACTIVE_BACKEND.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_backend_choices() -> u32 {
    BACKEND_CHOICES.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_request(backend: u32) -> i32 {
    if !switching() || ACTIVE_CHAIN.load(Ordering::Acquire).is_null() {
        return NOT_READY;
    }
    if backend != 0 && !known(backend) {
        return NOT_SUPPORTED;
    }
    if BACKEND_CHOICES.load(Ordering::SeqCst) & bit(backend) == 0 {
        SWITCH_RESULT.store(NOT_SUPPORTED, Ordering::SeqCst);
        return NOT_SUPPORTED;
    }
    SWITCH_RESULT.store(0, Ordering::SeqCst);
    REQUESTED_BACKEND.store(backend, Ordering::SeqCst);
    OK
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_switch_result() -> i32 {
    SWITCH_RESULT.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_d3d11p_generation() -> u64 {
    generation()
}

// Latency routing ---------------------------------------------------------------------------

struct Lease<'a> {
    p: &'a Platform,
    api: Option<&'static Provider>,
    session: Raw,
    host: Raw,
}

impl Lease<'_> {
    fn context(&self) -> Option<(&'static Provider, Raw)> {
        self.api
            .filter(|_| !self.session.is_null())
            .map(|api| (api, self.session))
    }
}

// Shares the provider lifetime; refuses NOT_READY while replacement or retirement holds it.
unsafe fn with_provider(platform: *const Platform, call: impl FnOnce(&Lease) -> i32) -> i32 {
    let Some(p) = (unsafe { platform.as_ref() }) else {
        return NOT_READY;
    };
    let Some(_guard) = read_guard() else {
        return NOT_READY;
    };
    call(&Lease {
        p,
        api: provider_api(p, ACTIVE_BACKEND.load(Ordering::SeqCst)),
        session: ACTIVE_SESSION.load(Ordering::Acquire),
        host: ACTIVE_HOST.load(Ordering::Acquire),
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_retire(
    platform: *const Platform,
    session: Raw,
    list: Raw,
    fence: *mut Raw,
    value: *mut u64,
) -> i32 {
    let (Some(fence), Some(value)) = (unsafe { fence.as_mut() }, unsafe { value.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    *fence = null_mut();
    *value = 0;
    unsafe {
        with_provider(platform, |_| {
            active().map_or(NOT_READY, |owner| owner.retire(session, list, fence, value))
        })
    }
}

// A provider that paces through the host is the host: it gets the host calls once. Any other
// provider owns its own pacing, and the host only records markers for latency reporting.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_begin(
    platform: *const Platform,
    id: u64,
    out: *mut u64,
) -> i32 {
    unsafe {
        with_provider(platform, |lease| {
            if let Some(out) = out.as_mut() {
                *out = generation();
            }
            if HOST_PACED.load(Ordering::SeqCst) {
                return (lease.p.host_sleep)(lease.host, id);
            }
            match lease
                .context()
                .and_then(|(api, session)| Some((api.begin_frame?, session)))
            {
                Some((begin, session)) => begin(session, id),
                None => OK,
            }
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_pace(
    platform: *const Platform,
    id: u64,
    owner: u32,
    expected: *const u64,
    out: *mut u64,
) -> i32 {
    unsafe {
        with_provider(platform, |lease| {
            let current = generation();
            if let Some(out) = out.as_mut() {
                *out = current;
            }
            if expected
                .as_ref()
                .is_some_and(|expected| *expected != current)
            {
                return NOT_READY;
            }
            if id == 0 || owner > PACING_LAST {
                return INVALID_ARGUMENT;
            }
            match owner {
                RSF_PACING_REFLEX if !lease.host.is_null() => (lease.p.host_sleep)(lease.host, id),
                RSF_PACING_REFLEX => NOT_READY,
                RSF_PACING_XELL => lease
                    .context()
                    .and_then(|(api, session)| Some((api.begin_frame?, session)))
                    .map_or(NOT_READY, |(begin, session)| begin(session, id)),
                // Anti-Lag updates live in the core; none has no sleeper.
                _ => OK,
            }
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_marker(
    platform: *const Platform,
    expected: u64,
    id: u64,
    marker: u32,
    controller: u32,
) -> i32 {
    unsafe {
        with_provider(platform, |lease| {
            if expected != generation() {
                return NOT_READY;
            }
            let host = if lease.host.is_null() {
                OK
            } else {
                (lease.p.host_marker)(lease.host, id, marker, controller)
            };
            if host != OK || HOST_PACED.load(Ordering::SeqCst) {
                return host;
            }
            match lease
                .context()
                .and_then(|(api, session)| Some((api.marker?, session)))
            {
                Some((call, session)) => call(session, marker, id, controller),
                None => host,
            }
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_acquire(platform: *const Platform, id: u64) -> i32 {
    unsafe {
        with_provider(platform, |lease| {
            if lease.host.is_null() {
                OK
            } else {
                (lease.p.host_acquire)(lease.host, id)
            }
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_receipt(
    platform: *const Platform,
    expected: u64,
    id: u64,
    kinds: u32,
    message: u32,
) -> i32 {
    unsafe {
        with_provider(platform, |lease| {
            if expected != generation() || lease.host.is_null() {
                NOT_READY
            } else {
                (lease.p.host_input)(lease.host, id, kinds, message)
            }
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_input(
    platform: *const Platform,
    id: u64,
    kinds: u32,
    message: u32,
) -> i32 {
    unsafe {
        with_provider(platform, |lease| {
            let result = if lease.host.is_null() {
                OK
            } else {
                (lease.p.host_input)(lease.host, id, kinds, message)
            };
            if result != OK || HOST_PACED.load(Ordering::SeqCst) || kinds == 0 {
                return result;
            }
            match lease
                .context()
                .and_then(|(api, session)| Some((api.marker?, session)))
            {
                Some((call, session)) => {
                    call(session, LATENCY_INPUT_SAMPLE, id, u32::from(kinds & 4 != 0))
                }
                None => result,
            }
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_d3d11p_abort(platform: *const Platform, id: u64) -> i32 {
    unsafe {
        with_provider(platform, |lease| {
            let host = if lease.host.is_null() {
                OK
            } else {
                (lease.p.host_abort)(lease.host, id)
            };
            if HOST_PACED.load(Ordering::SeqCst) {
                return host;
            }
            match lease
                .context()
                .and_then(|(api, session)| Some((api.abort_frame?, session)))
            {
                Some((call, session)) => call(session, id),
                None => host,
            }
        })
    }
}

#[cfg(test)]
#[path = "d3d11_present_tests.rs"]
mod tests;
