// SPDX-License-Identifier: MIT
//! Shared host callbacks and packet dispatch for the Mono and package DLLs.

use crate::bridge_d3d12;
use crate::bridge_state::*;
use crate::sys::rsf_unity_packet;
use crate::sys::{
    RSF_UNITY_D3D12_EVENT_FLUSH, RSF_UNITY_D3D12_EVENT_MODIFIES, RSF_UNITY_D3D12_EVENT_SYNC,
    rsf_unity_d3d12_engine,
};
use crate::sys::{RSF_UNITY_NATIVE_ABI_VERSION, rsf_unity_native_api};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::*;
use std::ffi::{c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU64, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        Threading::GetCurrentThreadId,
    },
    UI::WindowsAndMessaging::{
        CallNextHookEx, MSG, PM_REMOVE, SetWindowsHookExW, UnhookWindowsHookEx, WH_GETMESSAGE,
    },
};

#[cfg(not(feature = "mono-plugin"))]
pub(crate) mod package;

unsafe extern "C" {
    fn rsf_unity_d3d12_sdk_engine() -> *const rsf_unity_d3d12_engine;
    fn rsf_unity_d3d12_sdk_reserve_events() -> c_int;
    fn rsf_unity_d3d12_sdk_configure_event(
        id: c_int,
        allow_queue: bool,
        flags: u32,
        bind_texture: bool,
    );
    fn rsf_unity_d3d12_sdk_available() -> bool;
    fn rsf_unity_d3d12_sdk_lookup(address: *mut c_void) -> bool;
}

#[cfg_attr(feature = "mono-plugin", allow(dead_code))]
enum Route {
    None,
    D3d11,
    D3d12,
    Vulkan,
}

fn route() -> Route {
    #[cfg(feature = "mono-plugin")]
    {
        Route::D3d12
    }
    #[cfg(not(feature = "mono-plugin"))]
    {
        package::route()
    }
}

struct Services(rsf_game_host_services);
// The lifecycle keeps the host and user pointer live until callback admission and GPU work drain.
unsafe impl Send for Services {}

static HOST: Mutex<Option<Services>> = Mutex::new(None);
static D3D12_EVENT_ID: AtomicI32 = AtomicI32::new(0);
static INPUT_SOURCE: AtomicU64 = AtomicU64::new(0);
static MESSAGE_HOOK: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

fn host_lock() -> MutexGuard<'static, Option<Services>> {
    lock_or_recover(&HOST)
}

fn host() -> Option<rsf_game_host_services> {
    host_lock().as_ref().map(|service| service.0)
}

struct Callback;

impl Callback {
    fn enter() -> Option<Self> {
        rsf_unity_frames_callback_enter().then_some(Self)
    }
}

impl Drop for Callback {
    fn drop(&mut self) {
        rsf_unity_frames_callback_leave();
    }
}

struct Packet {
    packet: rsf_unity_packet,
    slot: u32,
}

impl Packet {
    fn take(address: *mut c_void) -> Option<Self> {
        let (slot, packet) = rsf_unity_frames_take_packet(address)?;
        Some(Self { packet, slot })
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        rsf_unity_frames_end_packet(self.slot);
    }
}

unsafe extern "C" fn log(message: *const c_char) {
    if let Some(_callback) = Callback::enter()
        && let Some(host) = host()
        && let Some(log) = host.log
    {
        unsafe { rsf_game_sys::rsf_game_log_guarded(Some(log), host.user, message) };
    }
}

unsafe extern "C" fn config(
    width: u32,
    height: u32,
    output: *mut crate::sys::rsf_unity_config,
) -> c_int {
    let Some(_callback) = Callback::enter() else {
        return 0;
    };
    let Some(host) = host() else { return 0 };
    unsafe { rsf_unity_frames_config(&host, width, height, output.cast()) }
}

unsafe extern "C" fn enqueue(packet: *const crate::sys::rsf_unity_packet) -> *mut c_void {
    let locked = host_lock();
    let session = locked.as_ref().map_or(0, |host| host.0.session_id);
    #[cfg(not(feature = "mono-plugin"))]
    if !matches!(route(), Route::D3d11 | Route::D3d12) {
        return unsafe { rsf_unity_frames_admit_packet(packet, session) };
    }
    unsafe { rsf_unity_frames_admit_leased_packet(packet, session) }
}

unsafe extern "system" fn render(id: c_int, address: *mut c_void) {
    let Some(_callback) = Callback::enter() else {
        return;
    };
    let base = rsf_unity_get_event_id();
    let offset = id.wrapping_sub(base) as u32;
    if offset >= 3 {
        return;
    }
    let Some(packet) = Packet::take(address) else {
        return;
    };
    let Some(host) = host() else { return };
    match route() {
        Route::D3d12 => {
            if let Some(engine) = unsafe { rsf_unity_d3d12_sdk_engine().as_ref() } {
                unsafe { bridge_d3d12::render_packet(engine, &host, offset, &packet.packet) };
            }
        }
        #[cfg(not(feature = "mono-plugin"))]
        Route::D3d11 | Route::Vulkan if rsf_unity_frames_active() => {
            package::render(id, offset, &host, &packet.packet)
        }
        _ => {}
    }
}

fn timestamp() -> (u64, u64) {
    let (mut now, mut frequency) = (0, 0);
    unsafe {
        QueryPerformanceCounter(&mut now);
        QueryPerformanceFrequency(&mut frequency);
    }
    (now as u64, frequency as u64)
}

unsafe extern "system" fn received_message(
    code: c_int,
    removed: WPARAM,
    argument: LPARAM,
) -> LRESULT {
    if let Some(_callback) = Callback::enter()
        && code >= 0
        && removed == PM_REMOVE as WPARAM
        && rsf_unity_frames_active()
        && let Some(host) = host()
        && let Some(deliver) = host.cpu_event
    {
        let message = unsafe { &*(argument as *const MSG) };
        let kind = rsf_windows::rsf_win32_input_kind(message.message, message.lParam);
        let source = INPUT_SOURCE.load(SeqCst);
        let mut flags = RSF_GAME_CPU_INPUT_KEYED;
        if message.message != 0 {
            flags |= RSF_GAME_CPU_WINDOW_MESSAGE;
        }
        if source == 0 && kind != 0 {
            flags |= RSF_GAME_CPU_INPUT_UNFRAMED;
        }
        let fact = build_cpu_event(
            host.session_id,
            RSF_GAME_CPU_INPUT_EVENT,
            source,
            timestamp(),
            kind,
            message.message,
            flags,
        );
        unsafe { deliver(host.user, &fact) };
    }
    unsafe { CallNextHookEx(null_mut(), code, removed, argument) }
}

unsafe extern "C" fn cpu_fact(fact: *const rsf_game_cpu_event) {
    let Some(_callback) = Callback::enter() else {
        return;
    };
    if !rsf_unity_frames_active() {
        return;
    }
    let Some(host) = host() else { return };
    let Some(fact) = (unsafe { ffi_util::checked(fact) }) else {
        return;
    };
    if fact.session_id != host.session_id {
        return;
    }
    if let Some(deliver) = host.cpu_event {
        if matches!(route(), Route::D3d12) && fact.stage == RSF_GAME_CPU_INPUT_EVENT {
            let mut fact = *fact;
            fact.source_frame_id = INPUT_SOURCE.load(SeqCst);
            if fact.source_frame_id == 0 {
                fact.flags |= RSF_GAME_CPU_INPUT_UNFRAMED;
            }
            unsafe { deliver(host.user, &fact) };
        } else {
            unsafe { deliver(host.user, fact) };
        }
    }
}

fn open_message_hook() {
    if !MESSAGE_HOOK.load(SeqCst).is_null() {
        return;
    }
    let hook = unsafe {
        SetWindowsHookExW(
            WH_GETMESSAGE,
            Some(received_message),
            null_mut(),
            GetCurrentThreadId(),
        )
    };
    if !hook.is_null()
        && MESSAGE_HOOK
            .compare_exchange(null_mut(), hook, SeqCst, SeqCst)
            .is_err()
    {
        unsafe { UnhookWindowsHookEx(hook) };
    }
}

unsafe extern "C" fn cpu_event(stage: u32, frame_id: u64) {
    let Some(_callback) = Callback::enter() else {
        return;
    };
    let Some(host) = host() else { return };
    if !rsf_unity_frames_active() || frame_id == 0 || host.cpu_event.is_none() {
        return;
    }
    let now = timestamp();
    let d3d12 = matches!(route(), Route::D3d12);
    if d3d12 && stage == RSF_GAME_CPU_FRAME_BEGIN {
        INPUT_SOURCE.store(frame_id, SeqCst);
        open_message_hook();
    }
    rsf_unity_frames_cpu_stage(&host, stage, frame_id, now);
    if d3d12 && stage == RSF_GAME_CPU_FRAME_END {
        INPUT_SOURCE.store(0, SeqCst);
    }
}

unsafe fn prepare(host: *const rsf_game_host_services, api: *mut rsf_unity_native_api) -> u32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    if api.is_null()
        || host.render_pass.is_none()
        || host.render_config.is_none()
        || host.log.is_none()
    {
        return 0;
    }
    let mut locked = host_lock();
    *locked = Some(Services(*host));
    #[cfg(feature = "mono-plugin")]
    rsf_unity_frames_open_callbacks();
    unsafe {
        api.write(rsf_unity_native_api {
            struct_size: size_of::<rsf_unity_native_api>() as u32,
            abi_version: RSF_UNITY_NATIVE_ABI_VERSION,
            session_id: host.session_id,
            log: Some(log),
            config: Some(config),
            enqueue: Some(enqueue),
            render_event: render as *const () as *mut c_void,
            managed_state: Some(rsf_unity_frames_report_managed),
            cpu_event: Some(cpu_event),
            cpu_fact: Some(cpu_fact),
        })
    };
    1
}

fn release_d3d12() {
    let mut locked = host_lock();
    let hook = MESSAGE_HOOK.swap(null_mut(), SeqCst);
    if !hook.is_null() {
        unsafe { UnhookWindowsHookEx(hook) };
    }
    INPUT_SOURCE.store(0, SeqCst);
    bridge_d3d12::release_graphics();
    *locked = None;
    rsf_unity_frames_release();
}

/// The game plugin's session start: stores the host and writes the managed API.
///
/// # Safety
/// host and pi are null or valid for the call; the host outlives the session.
#[cfg(feature = "mono-plugin")]
pub unsafe fn mono_prepare(
    host: *const rsf_game_host_services,
    api: *mut rsf_unity_native_api,
) -> bool {
    unsafe { prepare(host, api) != 0 }
}

#[cfg(feature = "mono-plugin")]
pub fn mono_try_close() -> bool {
    let _locked = host_lock();
    rsf_unity_frames_try_close_completed(bridge_d3d12::completed())
}

#[cfg(feature = "mono-plugin")]
pub fn mono_release() {
    release_d3d12();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_d3d12_configure_events(package: bool) {
    let id = unsafe { rsf_unity_d3d12_sdk_reserve_events() };
    D3D12_EVENT_ID.store(id, SeqCst);
    let flush = RSF_UNITY_D3D12_EVENT_SYNC | RSF_UNITY_D3D12_EVENT_FLUSH;
    let modify = RSF_UNITY_D3D12_EVENT_SYNC | RSF_UNITY_D3D12_EVENT_MODIFIES;
    unsafe {
        rsf_unity_d3d12_sdk_configure_event(id, false, flush, false);
        rsf_unity_d3d12_sdk_configure_event(id + 1, true, flush, false);
        // Capture pre-UI on Unity's recording list before the synchronized window queue flush.
        rsf_unity_d3d12_sdk_configure_event(id + 2, false, modify, true);
        if package {
            rsf_unity_d3d12_sdk_configure_event(id + 1, true, flush, true);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_unity_set_engine_spatial(spatial: u32, quality: u32) -> c_int {
    c_int::from(rsf_unity_frames_set_policy(spatial, quality))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_get_event_id() -> c_int {
    #[cfg(not(feature = "mono-plugin"))]
    if !matches!(route(), Route::D3d12) {
        return package::event_id();
    }
    D3D12_EVENT_ID.load(SeqCst)
}

// The legacy player registry is opt-in by exact code bytes and file digest.
fn lookup_player_registry() -> bool {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

    const RVA: usize = 0x74fc80;
    const EXPECTED: [u8; 24] = [
        0x48, 0x83, 0xec, 0x38, 0x48, 0x89, 0x4c, 0x24, 0x20, 0x48, 0x8d, 0x4c, 0x24, 0x20, 0x48,
        0x89, 0x54, 0x24, 0x28, 0xe8, 0x78, 0xff, 0xff, 0xff,
    ];
    const SHA256: [u8; 32] = [
        0xbb, 0xdf, 0x3e, 0x73, 0x52, 0x22, 0x23, 0xea, 0x45, 0xc3, 0xa5, 0x40, 0x49, 0xb7, 0x16,
        0xf9, 0xda, 0x3e, 0x63, 0x29, 0x94, 0x63, 0x67, 0x34, 0xc5, 0xd5, 0x33, 0x26, 0xaa, 0x2c,
        0x36, 0xd0,
    ];
    let player = unsafe { GetModuleHandleW(windows_sys::w!("UnityPlayer.dll")) };
    if player.is_null() {
        return false;
    }
    let mut buffer = [0u16; 1024];
    let Some(path) = (unsafe { win32::module_path_in(player, &mut buffer) }) else {
        return false;
    };
    let path = OsString::from_wide(path);
    if rsf_windows::sha256_file(Path::new(&path)) != Some(SHA256) {
        return false;
    }
    // This RVA and byte span belong only to the matched on-disk image.
    let site = unsafe { player.cast::<u8>().add(RVA) };
    if unsafe { site.cast::<[u8; EXPECTED.len()]>().read() } != EXPECTED {
        return false;
    }
    if !unsafe { rsf_unity_d3d12_sdk_lookup(site.cast()) } {
        return false;
    }
    rsf_unity_d3d12_configure_events(false);
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_get_render_event() -> *mut c_void {
    #[cfg(feature = "mono-plugin")]
    let Some(_callback) = Callback::enter() else {
        return null_mut();
    };
    match route() {
        Route::D3d12 => {
            if unsafe { rsf_unity_d3d12_sdk_engine() }.is_null() {
                if !unsafe { rsf_unity_d3d12_sdk_available() } || !lookup_player_registry() {
                    return null_mut();
                }
                if let Some(host) = host()
                    && let Some(log) = host.log
                {
                    unsafe {
                        rsf_game_sys::rsf_game_log_guarded(Some(log), host.user, c"Unity native registry matched through player PDB, SHA256 and expected-byte guards.".as_ptr())
                    };
                }
            }
        }
        Route::None => return null_mut(),
        Route::D3d11 | Route::Vulkan => {}
    }
    render as *const () as *mut c_void
}
