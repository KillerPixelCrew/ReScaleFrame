// SPDX-License-Identifier: MIT
// Fake leaf table and provider for the presentation bridge owner. No device, window or SDK.
use super::*;
use crate::com_ref::test::{
    Fake as FakeCom, add_ref as com_add_ref, fake as fake_com, raw as com_raw,
};
use crate::sys::{RSF_FG_FIXED, rsf_fg_frame};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::null;
use std::sync::MutexGuard;

static TEST_LOCK: Mutex<()> = Mutex::new(());

struct FakeBuffer {
    application: Box<FakeCom>,
    peer: Option<Box<FakeCom>>,
    slot: usize,
    handle: Raw,
}

// One slot pair leased to the owner. The fake keeps one reference; the owner holds the other.
struct FakeCommands {
    allocator: Box<FakeCom>,
    list: Box<FakeCom>,
}

struct FakePhysical {
    chain: Box<FakeCom>,
    queue: Raw,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SyncFailure {
    Queue,
    CompletionEvent,
    Fence,
    ReadyEvent,
}

struct Fake {
    calls: Vec<String>,
    logs: Vec<String>,
    slots: Vec<(u32, u64)>,
    presents: Vec<(u32, u32)>,
    native12: bool,
    physical: Vec<FakePhysical>,
    provider_queue: Raw,
    proxy_queue: Box<FakeCom>,
    native_queue: Box<FakeCom>,
    direct_queue: Box<FakeCom>,
    interop_queue: Box<FakeCom>,
    completion_fence: Box<FakeCom>,
    retirement_fence: Box<FakeCom>,
    reenter_retirement: bool,
    sync_failure: Option<SyncFailure>,
    sync_calls: Vec<&'static str>,
    query_chain_result: i32,
    reenter_physical: bool,
    width: u32,
    height: u32,
    format: u32,
    buffer_count: u32,
    flags: u32,
    windowed: u32,
    tearing: bool,
    fsr4: bool,
    fullscreen: i32,
    fullscreen_result: i32,
    join_result: i32,
    queue_missing: bool,
    host_available: bool,
    host_load_result: i32,
    host: usize,
    adapter_default: u32,
    present_result: i32,
    signal_result: i32,
    status_reads: u32,
    resize_result: i32,
    configuration_calls: usize,
    create_results: HashMap<u32, i32>,
    real_results: HashMap<u32, i32>,
    plain_result: i32,
    status: rsf_fg_status,
    drain_result: i32,
    retirement_result: i32,
    checked_provider: bool,
    refused_destroy: Raw,
    verify_result: i32,
    unpublish_alias_on_lookup: bool,
    client: (u32, u32),
    allocation_failure: Option<usize>,
    buffers: Vec<FakeBuffer>,
    buffer_slot: usize,
    buffer_usage: u32,
    target_view: Box<FakeCom>,
    backbuffer: Box<FakeCom>,
    backbuffer_result: i32,
    backbuffer_null: bool,
    unwrap_failure: bool,
    close_result: i32,
    commands: Vec<FakeCommands>,
    slot_failure: Option<usize>,
    // Pairs the owner still held at completion-event close and host release.
    held_at_release: Vec<usize>,
    sessions: usize,
    refs: i64,
}

impl Fake {
    fn new() -> Self {
        let mut status = EMPTY_STATUS;
        status.struct_size = size_of::<rsf_fg_status>() as u32;
        Self {
            calls: Vec::new(),
            logs: Vec::new(),
            slots: Vec::new(),
            presents: Vec::new(),
            native12: false,
            physical: Vec::new(),
            provider_queue: null_mut(),
            proxy_queue: fake_com(),
            native_queue: fake_com(),
            direct_queue: fake_com(),
            interop_queue: fake_com(),
            completion_fence: fake_com(),
            retirement_fence: fake_com(),
            reenter_retirement: false,
            sync_failure: None,
            sync_calls: Vec::new(),
            query_chain_result: S_OK,
            reenter_physical: false,
            width: 640,
            height: 480,
            format: 28,
            buffer_count: 2,
            flags: 0,
            windowed: 1,
            tearing: false,
            fsr4: false,
            fullscreen: 0,
            fullscreen_result: S_OK,
            join_result: S_OK,
            queue_missing: false,
            host_available: false,
            host_load_result: OK,
            host: 0,
            adapter_default: XESS,
            present_result: S_OK,
            signal_result: S_OK,
            status_reads: 0,
            resize_result: S_OK,
            configuration_calls: 0,
            create_results: HashMap::new(),
            real_results: HashMap::new(),
            plain_result: S_OK,
            status,
            drain_result: S_OK,
            retirement_result: OK,
            checked_provider: false,
            refused_destroy: null_mut(),
            verify_result: S_OK,
            unpublish_alias_on_lookup: false,
            client: (800, 600),
            allocation_failure: None,
            buffers: Vec::new(),
            buffer_slot: 0,
            buffer_usage: 0,
            target_view: fake_com(),
            backbuffer: fake_com(),
            backbuffer_result: S_OK,
            backbuffer_null: false,
            unwrap_failure: false,
            close_result: S_OK,
            commands: Vec::new(),
            slot_failure: None,
            held_at_release: Vec::new(),
            sessions: 0,
            refs: 0,
        }
    }

    fn interop(&self) -> &FakeCom {
        if self.native12 {
            &self.direct_queue
        } else {
            &self.interop_queue
        }
    }

    fn assert_completion(&self, fence: Raw) {
        assert_eq!(fence, com_raw(&self.completion_fence));
        assert_eq!(self.completion_fence.refs.get(), 2);
    }
}

thread_local! {
    static FAKE: RefCell<Fake> = RefCell::new(Fake::new());
}

fn with<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
    FAKE.with_borrow_mut(f)
}

fn call(name: impl Into<String>) {
    with(|fake| fake.calls.push(name.into()));
}

fn take_calls() -> Vec<String> {
    with(|fake| std::mem::take(&mut fake.calls))
}

fn raw(value: usize) -> Raw {
    value as Raw
}

// Provider ----------------------------------------------------------------------------------

unsafe extern "C" fn p_create(
    setup: *const rsf_generation_setup,
    session: *mut Raw,
    chain: *mut Raw,
) -> i32 {
    let setup = unsafe { &*setup };
    let backend = setup.feature_major;
    let probe = setup.chain.hwnd.is_null();
    call(format!(
        "{} {backend}",
        if probe { "probe" } else { "create" }
    ));
    let result = with(|fake| {
        let real = if probe {
            None
        } else {
            fake.real_results.get(&backend).copied()
        };
        real.or_else(|| fake.create_results.get(&backend).copied())
            .unwrap_or(OK)
    });
    if result == OK {
        let id = with(|fake| {
            fake.sessions += 1;
            if !probe {
                fake.provider_queue = setup.chain.d3d12_queue;
            }
            0x1000 + fake.sessions
        });
        unsafe {
            *session = raw(id);
            *chain = raw(0x9000 + backend as usize);
        }
    }
    result
}
unsafe extern "C" fn p_configure(_: Raw, _: *const rsf_fg_options) -> i32 {
    with(|fake| fake.configuration_calls += 1);
    OK
}
unsafe extern "C" fn p_begin(_: Raw, id: u64) -> i32 {
    call(format!("begin_frame {id}"));
    OK
}
unsafe extern "C" fn p_marker(_: Raw, marker: u32, id: u64, controller: u32) -> i32 {
    call(format!("marker {marker} {id} {controller}"));
    OK
}
unsafe extern "C" fn p_after(_: Raw) -> i32 {
    call("after");
    OK
}
unsafe extern "C" fn p_status(_: Raw, status: *mut rsf_fg_status) -> i32 {
    let snapshot = with(|fake| {
        fake.status_reads += 1;
        fake.status
    });
    unsafe { *status = snapshot };
    OK
}
unsafe extern "C" fn p_retirement(_: Raw, retirement: *mut rsf_fg_retirement) -> i32 {
    unsafe {
        (*retirement).fence = raw(0xF0);
        (*retirement).value = 7;
    }
    with(|fake| fake.retirement_result)
}
unsafe extern "C" fn p_destroy(session: Raw) {
    call(format!("destroy {:x}", session as usize));
}
unsafe extern "C" fn p_destroy_checked(session: Raw) -> i32 {
    if with(|fake| fake.refused_destroy == session) {
        call(format!("refused destroy {:x}", session as usize));
        return NOT_READY;
    }
    unsafe { p_destroy(session) };
    OK
}
unsafe extern "C" fn p_abort(_: Raw, id: u64) -> i32 {
    call(format!("abort_frame {id}"));
    OK
}
unsafe extern "C" fn p_prepare(_: Raw, _: Raw, _: *const rsf_fg_frame) -> i32 {
    OK
}

const PROVIDER_BASE: Provider = Provider {
    struct_size: size_of::<Provider>() as u32,
    create: Some(p_create),
    configure: Some(p_configure),
    begin_frame: Some(p_begin),
    marker: Some(p_marker),
    prepare: Some(p_prepare),
    after_present: Some(p_after),
    status: Some(p_status),
    retirement: Some(p_retirement),
    destroy: Some(p_destroy),
    abort_frame: Some(p_abort),
    destroy_checked: None,
};

static PROVIDER: Provider = PROVIDER_BASE;
static CHECKED_PROVIDER: Provider = Provider {
    destroy_checked: Some(p_destroy_checked),
    ..PROVIDER_BASE
};

// Leaves: d3d11_present.rs calls these as `leaf::*` under test --------------------------------

pub(super) use self::{
    ok_bridge as create_upload_fence, ok_bridge as native_device, ok_bridge as resolve_graphics,
};

struct FakeBridge {
    chain: *mut Chain,
}

pub(super) unsafe extern "C" fn provider_table(family: u32) -> *const Provider {
    if family == 0 {
        null()
    } else if with(|fake| fake.checked_provider) {
        &CHECKED_PROVIDER
    } else {
        &PROVIDER
    }
}
pub(super) unsafe extern "C" fn observer_present(event: *mut Event) {
    call(if unsafe { (*event).completed } != 0 {
        "observed"
    } else {
        "observer"
    });
}
pub(super) unsafe extern "C" fn window_valid(window: Raw) -> u32 {
    u32::from(!window.is_null())
}
pub(super) unsafe extern "C" fn client_size(_: Raw, width: *mut u32, height: *mut u32) -> u32 {
    let (w, h) = with(|fake| fake.client);
    unsafe {
        *width = w;
        *height = h;
    }
    1
}
pub(super) unsafe extern "C" fn canonical_identity(receiver: Raw) -> Raw {
    if receiver != OUTER_ALIAS.load(Ordering::Acquire) {
        return null_mut();
    }
    let identity = ACTIVE_BRIDGE.load(Ordering::Acquire);
    if with(|fake| fake.unpublish_alias_on_lookup) {
        OUTER_ALIAS.store(null_mut(), Ordering::Release);
    }
    identity
}
pub(super) unsafe extern "C" fn add_ref(_: Raw) {
    with(|fake| fake.refs += 1);
}
pub(super) unsafe extern "C" fn release(_: Raw) {
    with(|fake| fake.refs -= 1);
}
pub(super) unsafe extern "C" fn host_sleep(_: Raw, id: u64) -> i32 {
    call(format!("host_sleep {id}"));
    OK
}
pub(super) unsafe extern "C" fn host_marker(_: Raw, id: u64, marker: u32, _: u32) -> i32 {
    call(format!("host_marker {marker} {id}"));
    OK
}
pub(super) unsafe extern "C" fn host_acquire(_: Raw, id: u64) -> i32 {
    call(format!("host_acquire {id}"));
    OK
}
pub(super) unsafe extern "C" fn host_input(_: Raw, id: u64, kinds: u32, _: u32) -> i32 {
    call(format!("host_input {id} {kinds}"));
    OK
}
pub(super) unsafe extern "C" fn host_abort(_: Raw, id: u64) -> i32 {
    call(format!("host_abort {id}"));
    OK
}
pub(super) unsafe extern "C" fn host_reflex_off(_: Raw) -> i32 {
    call("host_reflex_off");
    OK
}
pub(super) unsafe extern "C" fn host_generation_load(_: Raw, enabled: u32) -> i32 {
    call(format!("host_load {enabled}"));
    with(|fake| fake.host_load_result)
}
pub(super) unsafe extern "C" fn host_present_common(
    _: Raw,
    chain: Raw,
    _: u32,
    _: u32,
    begin: u32,
) -> i32 {
    assert_chain(chain);
    call(format!("host_common {begin}"));
    OK
}
pub(super) unsafe extern "C" fn bridge_new(chain: *mut Chain) -> Raw {
    Box::into_raw(Box::new(FakeBridge { chain })).cast()
}
pub(super) unsafe extern "C" fn bridge_release_owned(bridge: Raw) {
    let bridge = unsafe { Box::from_raw(bridge.cast::<FakeBridge>()) };
    unsafe { rsf_core_d3d11p_chain_destroy(bridge.chain) };
}
pub(super) unsafe extern "C" fn bridge_chain(bridge: Raw) -> *mut Chain {
    unsafe { (*bridge.cast::<FakeBridge>()).chain }
}
pub(super) unsafe extern "C" fn facts(_: Raw, out: *mut rsf_d3d11p_facts) {
    let value = with(|fake| rsf_d3d11p_facts {
        window: raw(0x10),
        width: fake.width,
        height: fake.height,
        format: fake.format,
        buffer_count: fake.buffer_count,
        flags: fake.flags,
        windowed: fake.windowed,
        native12: u32::from(fake.native12),
        tearing: u32::from(fake.tearing),
        graphics: Graphics {
            struct_size: 48,
            device: raw(0x40),
            queue: com_raw(&fake.proxy_queue),
            factory: null_mut(),
            native_device: raw(0x42),
            native_queue: com_raw(&fake.native_queue),
        },
        direct_queue: if fake.native12 {
            com_raw(&fake.direct_queue)
        } else {
            null_mut()
        },
        device11: raw(0x44),
        buffer_usage: fake.buffer_usage,
    });
    unsafe { *out = value };
}
pub(super) unsafe extern "C" fn init_source(_: Raw, _: Raw, _: Raw, _: *const c_void) -> u32 {
    u32::from(with(|fake| fake.native12))
}
pub(super) unsafe extern "C" fn owning_adapter(
    _: Raw,
    _: Raw,
    adapter: *mut rsf_d3d11p_adapter,
) -> i32 {
    let backend = with(|fake| fake.adapter_default);
    unsafe {
        *adapter = rsf_d3d11p_adapter {
            vendor: 0x8086,
            device: 1,
            default_backend: backend,
            reserved: 0,
        }
    };
    S_OK
}
pub(super) unsafe extern "C" fn ok_bridge(_: Raw) -> i32 {
    S_OK
}
pub(super) unsafe extern "C" fn adopt_host(bridge: Raw, _: u32, result: *mut i32) -> u32 {
    if with(|fake| fake.host_available) {
        unsafe { *result = create_host(bridge) };
        1
    } else {
        0
    }
}
pub(super) unsafe extern "C" fn d3d11_device(_: Raw, _: Raw) -> i32 {
    S_OK
}
pub(super) unsafe extern "C" fn create_host(_: Raw) -> i32 {
    with(|fake| {
        if fake.host_available {
            fake.host = 0xAA;
            OK
        } else {
            NOT_SUPPORTED
        }
    })
}
pub(super) unsafe extern "C" fn host(_: Raw) -> Raw {
    raw(with(|fake| fake.host))
}
pub(super) unsafe extern "C" fn tearing_supported(_: Raw) -> u32 {
    u32::from(with(|fake| fake.tearing))
}
pub(super) unsafe extern "C" fn fsr4_supported(_: Raw, _: *const c_char) -> u32 {
    u32::from(with(|fake| fake.fsr4))
}
pub(super) unsafe extern "C" fn create_interop_queue(device: Raw, out: *mut Raw) -> i32 {
    assert_eq!(device, raw(0x40));
    with(|fake| {
        assert!(!fake.native12);
        fake.sync_calls.push("create interop queue");
        if fake.sync_failure == Some(SyncFailure::Queue) {
            return E_FAIL;
        }
        unsafe { give_ref(com_raw(&fake.interop_queue), out) };
        S_OK
    })
}
pub(super) unsafe extern "C" fn create_completion_fence(device: Raw, out: *mut Raw) -> i32 {
    assert_eq!(device, raw(0x40));
    with(|fake| {
        assert_eq!(fake.interop().refs.get(), 2);
        fake.sync_calls.push("create completion fence");
        if fake.sync_failure == Some(SyncFailure::Fence) {
            return E_FAIL;
        }
        unsafe { give_ref(com_raw(&fake.completion_fence), out) };
        S_OK
    })
}
const COMPLETION_EVENT: usize = 0x61;
const READY_EVENT: usize = 0x62;
pub(super) unsafe extern "C" fn create_event(initially_signaled: u32) -> Raw {
    with(|fake| {
        assert_eq!(fake.interop().refs.get(), 2);
        let (operation, failure, event) = if initially_signaled == 0 {
            (
                "create completion event",
                SyncFailure::CompletionEvent,
                COMPLETION_EVENT,
            )
        } else {
            assert!(fake.native12);
            fake.assert_completion(com_raw(&fake.completion_fence));
            ("create ready event", SyncFailure::ReadyEvent, READY_EVENT)
        };
        fake.sync_calls.push(operation);
        if fake.sync_failure == Some(failure) {
            null_mut()
        } else {
            raw(event)
        }
    })
}
pub(super) unsafe extern "C" fn duplicate_event(event: Raw) -> Raw {
    assert_eq!(event, raw(READY_EVENT));
    raw(0x63)
}
pub(super) unsafe extern "C" fn close_event(event: Raw) {
    if event == raw(COMPLETION_EVENT) {
        call("close completion event");
        with(|fake| {
            let held = held_slot_pairs(fake);
            fake.held_at_release.push(held);
            fake.sync_calls.push("close completion event");
        });
    } else {
        assert_eq!(event, raw(READY_EVENT));
        with(|fake| fake.sync_calls.push("close ready event"));
    }
}
fn allocate_buffer(shared: bool) -> (Raw, i32) {
    with(|fake| {
        let failure = fake.allocation_failure == Some(fake.buffers.len());
        let slot = fake.buffer_slot;
        fake.buffer_slot += 1;
        let buffer = FakeBuffer {
            application: fake_com(),
            peer: shared.then(fake_com),
            slot,
            handle: null_mut(),
        };
        let resource = com_raw(&buffer.application);
        fake.buffers.push(buffer);
        (resource, if failure { E_FAIL } else { S_OK })
    })
}

unsafe fn give_ref(resource: Raw, out: *mut Raw) {
    unsafe {
        com_add_ref(resource.cast());
        out.write(resource);
    }
}

pub(super) unsafe extern "C" fn create_render_buffer(
    _: Raw,
    width: u32,
    height: u32,
    format: u32,
    out: *mut Raw,
) -> i32 {
    call(format!("make_render {width}x{height} {format}"));
    let (resource, result) = allocate_buffer(false);
    unsafe { give_ref(resource, out) };
    result
}

unsafe extern "C" fn shared_texture(_: Raw, setup: &SurfaceSetup, out: *mut Raw) -> i32 {
    assert_eq!(setup.render_target, 1);
    assert_eq!(
        setup.unordered_access,
        with(|fake| u32::from(fake.buffer_usage & DXGI_USAGE_UNORDERED_ACCESS != 0))
    );
    let (resource, result) = allocate_buffer(true);
    unsafe { give_ref(resource, out) };
    result
}

unsafe extern "C" fn shared_target(_: Raw, _: Raw, out: *mut Raw) -> i32 {
    let view = with(|fake| com_raw(&fake.target_view));
    unsafe { give_ref(view, out) };
    S_OK
}

unsafe extern "C" fn shared_resource(texture11: Raw, out: *mut Raw) -> i32 {
    unsafe { give_ref(texture11, out) };
    S_OK
}

unsafe extern "C" fn shared_handle(_: Raw, out: *mut Raw) -> i32 {
    let handle =
        unsafe { windows_sys::Win32::System::Threading::CreateEventW(null(), 0, 0, null()) };
    assert!(!handle.is_null());
    with(|fake| fake.buffers.last_mut().unwrap().handle = handle);
    unsafe { out.write(handle) };
    S_OK
}

unsafe extern "C" fn shared_peer(_: Raw, _: Raw, out: *mut Raw) -> i32 {
    let peer = with(|fake| com_raw(fake.buffers.last().unwrap().peer.as_ref().unwrap()));
    unsafe { give_ref(peer, out) };
    S_OK
}

unsafe extern "C" fn no_output(_: Raw, _: *mut Raw) -> i32 {
    E_UNEXPECTED
}

pub(super) unsafe extern "C" fn shared_surface_platform() -> *const shared_surface::Platform {
    static SHARED: shared_surface::Platform = shared_surface::Platform {
        texture: shared_texture,
        target: shared_target,
        query_resource: shared_resource,
        resource_handle: shared_handle,
        open_resource: shared_peer,
        query_device5: no_output,
        fence: no_output,
        fence_handle: no_output,
        open_fence: shared_peer,
    };
    &SHARED
}
fn stage_physical(fake: &mut Fake, queue: Raw) -> Raw {
    assert!(
        fake.physical.iter().all(|old| old.chain.refs.get() == 1),
        "one physical chain at a time"
    );
    let chain = fake_com();
    let raw = com_raw(&chain);
    fake.physical.push(FakePhysical { chain, queue });
    raw
}

fn assert_chain(chain: Raw) {
    with(|fake| {
        let physical = fake.physical.last().unwrap();
        assert_eq!(chain, com_raw(&physical.chain));
        assert_eq!(physical.chain.refs.get(), 2);
    });
}

fn assert_queue(queue: Raw) {
    with(|fake| {
        assert_eq!(queue, fake.physical.last().unwrap().queue);
        let interop = queue == com_raw(fake.interop());
        assert_eq!(
            unsafe { &*queue.cast::<FakeCom>() }.refs.get(),
            2 + u32::from(interop)
        );
    });
}

pub(super) unsafe extern "C" fn create_plain(
    _: Raw,
    plan: &rsf_d3d11p_plain_plan,
    physical: *mut Raw,
    queue: *mut Raw,
) -> i32 {
    call("plain");
    with(|fake| {
        let (owner_queue, count, usage) = if fake.native12 {
            (com_raw(&fake.direct_queue), 0, 0)
        } else {
            (
                com_raw(&fake.native_queue),
                3,
                DXGI_USAGE_RENDER_TARGET_OUTPUT,
            )
        };
        assert_eq!(plan.queue, owner_queue);
        assert_eq!(plan.swap_effect, DXGI_SWAP_EFFECT_FLIP_DISCARD);
        assert_eq!(
            plan.flags,
            if fake.tearing {
                DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING
            } else {
                0
            }
        );
        assert_eq!((plan.buffer_count, plan.buffer_usage), (count, usage));
        if fake.plain_result >= 0 {
            let chain = stage_physical(fake, owner_queue);
            unsafe {
                give_ref(chain, physical);
                *queue = if fake.queue_missing {
                    null_mut()
                } else {
                    owner_queue
                };
            }
        }
        fake.plain_result
    })
}
pub(super) unsafe extern "C" fn query_chain4(_: Raw, out: *mut Raw) -> i32 {
    call("adopt");
    with(|fake| {
        let chain = stage_physical(fake, fake.provider_queue);
        unsafe { give_ref(chain, out) };
        fake.query_chain_result
    })
}

fn physical_lease(chain: *mut Chain, initialize: u32) -> (i32, *const Physical, Raw) {
    let mut lease = null();
    let mut physical = null_mut();
    let result =
        unsafe { rsf_core_d3d11p_acquire_physical(chain, initialize, &mut lease, &mut physical) };
    (result, lease, physical)
}
fn reenter_physical(expected: Raw) {
    if with(|fake| fake.reenter_physical) {
        let bridge = rsf_core_d3d11p_active_bridge();
        let (result, lease, chain) = physical_lease(chain_of(bridge), 1);
        assert_eq!(chain, expected);
        assert_eq!(
            result,
            if expected.is_null() {
                DXGI_ERROR_WAS_STILL_DRAWING
            } else {
                S_OK
            }
        );
        unsafe { rsf_core_d3d11p_release_physical(lease) };
    }
}
pub(super) unsafe extern "C" fn get_fullscreen(chain: Raw, fullscreen: *mut i32) -> i32 {
    assert_chain(chain);
    unsafe { *fullscreen = with(|fake| fake.fullscreen) };
    S_OK
}
pub(super) unsafe extern "C" fn set_fullscreen(chain: Raw, fullscreen: i32, _: Raw) -> i32 {
    assert_chain(chain);
    call(format!("fullscreen {fullscreen}"));
    reenter_physical(if fullscreen == 0 { null_mut() } else { chain });
    with(|fake| std::mem::replace(&mut fake.fullscreen_result, S_OK))
}
pub(super) unsafe extern "C" fn physical_present(chain: Raw, sync: u32, flags: u32) -> i32 {
    assert_chain(chain);
    reenter_physical(chain);
    if flags & DXGI_PRESENT_TEST != 0 {
        call("present_test");
        return S_OK;
    }
    call("present");
    with(|fake| {
        fake.presents.push((sync, flags));
        fake.present_result
    })
}
pub(super) unsafe extern "C" fn resize_physical(
    _: Raw,
    chain: Raw,
    count: u32,
    width: u32,
    height: u32,
    _: u32,
) -> i32 {
    assert_chain(chain);
    call(format!("resize {count} {width}x{height}"));
    with(|fake| {
        if !failed(fake.resize_result) {
            fake.buffer_slot = 0;
        }
        fake.resize_result
    })
}
pub(super) unsafe extern "C" fn join(signaller: Raw, waiter: Raw, fence: Raw, value: u64) -> i32 {
    let source = with(|fake| {
        fake.assert_completion(fence);
        assert!(fake.native12);
        com_raw(fake.interop())
    });
    let direction = if signaller == source {
        assert_queue(waiter);
        0
    } else {
        assert_eq!(waiter, source);
        assert_queue(signaller);
        1
    };
    assert_ne!(signaller, waiter);
    call(format!("join {direction} {value}"));
    with(|fake| fake.join_result)
}
pub(super) unsafe extern "C" fn create_slot(_: Raw, allocator: *mut Raw, list: *mut Raw) -> i32 {
    unsafe {
        allocator.write(null_mut());
        list.write(null_mut());
    }
    with(|fake| {
        if fake.slot_failure == Some(fake.commands.len()) {
            return E_FAIL;
        }
        let pair = FakeCommands {
            allocator: fake_com(),
            list: fake_com(),
        };
        unsafe {
            give_ref(com_raw(&pair.allocator), allocator);
            give_ref(com_raw(&pair.list), list);
        }
        fake.commands.push(pair);
        S_OK
    })
}
// The slot whose leased list, and allocator when given, the owner passed.
fn slot_of(list: Raw, allocator: Option<Raw>) -> usize {
    with(|fake| {
        let slot = fake
            .commands
            .iter()
            .rposition(|pair| {
                com_raw(&pair.list) == list
                    && allocator.is_none_or(|value| com_raw(&pair.allocator) == value)
            })
            .expect("a leased slot pair");
        let pair = &fake.commands[slot];
        assert_eq!((pair.list.refs.get(), pair.allocator.refs.get()), (2, 2));
        slot % SLOTS
    })
}
fn held_slot_pairs(fake: &Fake) -> usize {
    fake.commands
        .iter()
        .filter(|pair| pair.list.refs.get() > 1 || pair.allocator.refs.get() > 1)
        .count()
}
pub(super) unsafe extern "C" fn slot_reset(
    completion: Raw,
    event: Raw,
    allocator: Raw,
    list: Raw,
    fence: u64,
    step: *mut u32,
) -> i32 {
    with(|fake| fake.assert_completion(completion));
    assert_eq!(event, raw(COMPLETION_EVENT));
    unsafe { *step = 0 };
    let slot = slot_of(list, Some(allocator)) as u32;
    with(|fake| fake.slots.push((slot, fence)));
    S_OK
}
pub(super) unsafe extern "C" fn upload_wait(_: Raw, queue: Raw, value: u64) -> i32 {
    assert_queue(queue);
    call(format!("upload {value}"));
    S_OK
}
pub(super) unsafe extern "C" fn acquire_backbuffer(chain: Raw, out: *mut Raw) -> i32 {
    assert_chain(chain);
    let (resource, result, null) = with(|fake| {
        (
            com_raw(&fake.backbuffer),
            fake.backbuffer_result,
            fake.backbuffer_null,
        )
    });
    if !null {
        unsafe { give_ref(resource, out) };
    }
    result
}
pub(super) unsafe extern "C" fn record_copy(_: Raw, list: Raw, destination: Raw, source: Raw) {
    let slot = slot_of(list, None);
    let render_index = with(|fake| {
        assert_eq!(destination, com_raw(&fake.backbuffer));
        fake.buffers
            .iter()
            .find(|buffer| com_raw(buffer.peer.as_ref().unwrap_or(&buffer.application)) == source)
            .unwrap()
            .slot
    });
    call(format!("copy {slot} {render_index}"));
}
pub(super) unsafe extern "C" fn context11(_: Raw) -> Raw {
    raw(0xD0)
}
pub(super) unsafe extern "C" fn close_execute(queue: Raw, list: Raw, command: Raw) -> i32 {
    assert_queue(queue);
    slot_of(list, None);
    // The fake queue takes the slot list itself.
    assert_eq!(command, list);
    with(|fake| fake.close_result)
}
pub(super) unsafe extern "C" fn upload_return(_: Raw, queue: Raw, value: u64) -> i32 {
    assert_queue(queue);
    call(format!("return {value}"));
    S_OK
}
pub(super) unsafe extern "C" fn signal_slot(queue: Raw, fence: Raw, value: u64) -> i32 {
    assert_queue(queue);
    with(|fake| {
        fake.assert_completion(fence);
        let held = fake.native12 && !failed(fake.backbuffer_result) && !fake.backbuffer_null;
        assert_eq!(fake.backbuffer.refs.get(), 1 + u32::from(held));
    });
    call(format!("slot_signal {value}"));
    with(|fake| fake.signal_result)
}
pub(super) unsafe extern "C" fn application_ready(event: Raw, fence: Raw, value: u64) -> i32 {
    assert_eq!(event, raw(READY_EVENT));
    if !fence.is_null() {
        with(|fake| fake.assert_completion(fence));
    }
    let on_completion = u32::from(!fence.is_null());
    call(format!("app_ready {on_completion} {value}"));
    S_OK
}
pub(super) unsafe extern "C" fn report_device_failure(_: Raw, _: i32, _: u32) {
    call("device_failure");
}
pub(super) unsafe extern "C" fn log_present_failure(_: Raw, step: *const c_char, _: i32, _: u32) {
    call(format!(
        "present_failure {}",
        unsafe { CStr::from_ptr(step) }.to_string_lossy()
    ));
}
pub(super) unsafe extern "C" fn signal_wait(queue: Raw, fence: Raw, event: Raw, value: u64) -> i32 {
    assert_eq!(event, raw(COMPLETION_EVENT));
    let interop = with(|fake| {
        fake.assert_completion(fence);
        let interop = queue == com_raw(fake.interop());
        assert!(interop || queue == com_raw(&fake.proxy_queue));
        u32::from(interop)
    });
    call(format!("signal_wait {interop} {value}"));
    with(|fake| fake.drain_result)
}
pub(super) unsafe extern "C" fn wait_fence(fence: Raw, event: Raw, value: u64) -> i32 {
    assert_eq!(fence, raw(0xF0));
    assert_eq!(event, raw(COMPLETION_EVENT));
    call(format!("wait_fence {value}"));
    S_OK
}
pub(super) unsafe extern "C" fn query_buffer_interface(
    resource: Raw,
    _: *const c_void,
    out: *mut Raw,
) -> i32 {
    unsafe { give_ref(resource, out) };
    S_OK
}
pub(super) unsafe extern "C" fn get_physical_buffer(
    chain: Raw,
    index: u32,
    _: *const c_void,
    out: *mut Raw,
) -> i32 {
    assert_chain(chain);
    call(format!("physical GetBuffer {index}"));
    let resource = with(|fake| com_raw(&fake.backbuffer));
    unsafe { give_ref(resource, out) };
    S_OK
}
pub(super) unsafe extern "C" fn log_buffer_refusal(
    _: Raw,
    _: u32,
    _: *const c_void,
    _: i32,
    _: u32,
) {
    call("GetBuffer refused");
}
pub(super) unsafe extern "C" fn buffer_referenced(resource: Raw) -> u32 {
    let refs = unsafe { &*resource.cast::<FakeCom>() }.refs.get();
    u32::from(refs > 2)
}
pub(super) unsafe extern "C" fn set_description(
    _: Raw,
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
) {
    with(|fake| {
        fake.buffer_count = count;
        fake.width = width;
        fake.height = height;
        fake.format = format;
        fake.flags = flags;
    });
}
pub(super) unsafe extern "C" fn create_retirement_fence(_: Raw, out: *mut Raw) -> i32 {
    with(|fake| {
        fake.sync_calls.push("create retirement fence");
        unsafe { give_ref(com_raw(&fake.retirement_fence), out) };
    });
    S_OK
}
pub(super) unsafe extern "C" fn retire_submit(
    _: Raw,
    queue: Raw,
    retirement_fence: Raw,
    _: Raw,
    _: u64,
    _: Raw,
    value: u64,
) -> i32 {
    assert_queue(queue);
    with(|fake| assert_eq!(retirement_fence, com_raw(&fake.retirement_fence)));
    if with(|fake| std::mem::take(&mut fake.reenter_retirement)) {
        let (mut fence, mut serial) = (retirement_fence, value);
        assert_eq!(
            unsafe {
                rsf_d3d11_present_retire(
                    rsf_d3d11_present_session(),
                    null_mut(),
                    &mut fence,
                    &mut serial,
                )
            },
            NOT_READY
        );
        assert!(fence.is_null() && serial == 0);
    }
    call(format!("retire_submit {value}"));
    OK
}
pub(super) unsafe extern "C" fn retain_all(_: Raw) {
    call("retain");
}
pub(super) unsafe extern "C" fn release_presentation(_: Raw) {
    call("release_presentation");
}
pub(super) unsafe extern "C" fn release_direct_graphics(_: Raw) {
    with(|fake| {
        assert_eq!(fake.completion_fence.refs.get(), 1);
        assert_eq!(fake.retirement_fence.refs.get(), 1);
        fake.sync_calls.push("release direct graphics");
    });
}
pub(super) unsafe extern "C" fn release_host(_: Raw) {
    call("release_host");
    with(|fake| {
        assert_eq!(fake.interop().refs.get(), 1);
        assert_eq!(fake.completion_fence.refs.get(), 1);
        assert_eq!(fake.retirement_fence.refs.get(), 1);
        let held = held_slot_pairs(fake);
        fake.held_at_release.push(held);
    });
}
pub(super) unsafe extern "C" fn source_queue(_: Raw, queue: Raw) -> Raw {
    with(|fake| assert_eq!(queue, com_raw(fake.interop())));
    unsafe { com_add_ref(queue.cast()) };
    queue
}
pub(super) unsafe extern "C" fn command_list(_: Raw, queue: Raw, list: Raw) -> Raw {
    if !queue.is_null() {
        assert_queue(queue);
    }
    if with(|fake| fake.unwrap_failure) {
        null_mut()
    } else {
        list
    }
}
pub(super) unsafe extern "C" fn verify_outer(_: Raw, _: Raw, _: Raw, _: Raw, _: Raw) -> i32 {
    with(|fake| fake.verify_result)
}

// Settings callbacks ------------------------------------------------------------------------

unsafe extern "C" fn fake_log(_: Raw, message: *const c_char) {
    let text = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    with(|fake| fake.logs.push(text));
}
unsafe extern "C" fn accept(_: Raw, _: Raw) -> i32 {
    1
}
unsafe extern "C" fn before(_: Raw, _: Raw) {
    call("before");
}
unsafe extern "C" fn present_event(_: Raw, event: *const Event) {
    call(if unsafe { (*event).completed } != 0 {
        "presented"
    } else {
        "present_event"
    });
}
unsafe extern "C" fn latency_event(_: Raw, event: *const Event) {
    call(if unsafe { (*event).completed } != 0 {
        "latency_done"
    } else {
        "latency_begin"
    });
}
unsafe extern "C" fn retire_callback(_: Raw, session: Raw) {
    assert_eq!(with(|fake| fake.backbuffer.refs.get()), 2);
    call("retire");
    if with(|fake| fake.reenter_physical) && !session.is_null() {
        assert_queue(unsafe { rsf_d3d11_present_queue() });
        let list = raw(0xCA);
        assert_eq!(unsafe { rsf_d3d11_present_command_list(list) }, list);
        let mut fence = null_mut();
        let mut value = 0;
        assert_eq!(
            unsafe { rsf_d3d11_present_retire(session, list, &mut fence, &mut value) },
            OK
        );
        assert!(!fence.is_null() && value != 0);
    }
}
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn prepare_callback(
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    backbuffer: Raw,
    _: *mut rsf_streamline_host,
    _: Raw,
    _: u32,
) {
    with(|fake| {
        assert_eq!(backbuffer, com_raw(&fake.backbuffer));
        assert_eq!(fake.backbuffer.refs.get(), 2);
    });
    call("prepare");
}

fn setup(backend: u32, switching: u32) -> Setup {
    let mut value: Setup = unsafe { std::mem::zeroed() };
    value.struct_size = size_of::<Setup>() as u32;
    value.runtime_directory_utf8 = c"runtime".as_ptr();
    value.log = Some(fake_log);
    value.accept_window = Some(accept);
    value.before_present = Some(before);
    value.present_event = Some(present_event);
    value.latency_event = Some(latency_event);
    value.retire = Some(retire_callback);
    value.prepare = Some(prepare_callback);
    value.backend = backend;
    value.runtime_switching = switching;
    value.units_to_meters = 1.0;
    value.engine_version_utf8 = c"1.0".as_ptr();
    value.project_id_utf8 = c"project".as_ptr();
    value
}

fn reset() -> MutexGuard<'static, ()> {
    let guard = lock(&TEST_LOCK);
    for value in [&ACTIVE_HOST, &ACTIVE_BRIDGE, &ACTIVE_SESSION, &OUTER_ALIAS] {
        value.store(null_mut(), Ordering::SeqCst);
    }
    ACTIVE_CHAIN.store(null_mut(), Ordering::SeqCst);
    lock(&ACTIVE_GRAPHICS).0 = NO_GRAPHICS;
    ACTIVE_BACKEND.store(DLSS, Ordering::SeqCst);
    REQUESTED_BACKEND.store(NO_REQUEST, Ordering::SeqCst);
    BACKEND_CHOICES.store((RSF_FG_BACKEND_ALL | 1) & !(1 << FSR4), Ordering::SeqCst);
    SWITCH_RESULT.store(0, Ordering::SeqCst);
    CHAIN_GENERATION.store(1, Ordering::SeqCst);
    RETIREMENT_PENDING.store(false, Ordering::Release);
    HOST_PACED.store(false, Ordering::SeqCst);
    SCOPED.store(false, Ordering::SeqCst);
    SCOPED_ADMITTED.store(false, Ordering::SeqCst);
    ACTIVE_CREATION_TOKEN.store(0, Ordering::SeqCst);
    *lock(&STATUS) = Snapshot::new();
    SETTINGS.store(null_mut(), Ordering::SeqCst);
    CREATING.set(false);
    SCOPE_TOKEN.set(0);
    SCOPE_WINDOW.set(null_mut());
    drop(SCOPE_CAPTURED.replace(Captured {
        object: null_mut(),
        release: None,
    }));
    with(|fake| *fake = Fake::new());
    guard
}

fn prepare(value: &Setup) -> i32 {
    unsafe { rsf_core_d3d11p_prepare(value, 0) }
}

fn request(flags: u32) -> rsf_d3d11p_request {
    rsf_d3d11p_request {
        window: raw(0x10),
        width: 640,
        height: 480,
        format: 28,
        sample_count: 1,
        buffer_count: 2,
        windowed: 1,
        flags,
        swap_effect: 0,
        refresh_numerator: 60,
        refresh_denominator: 1,
    }
}

fn create_with(value: &rsf_d3d11p_request) -> (i32, Raw) {
    let mut out = null_mut();
    let result = unsafe { rsf_core_d3d11p_create(raw(0x20), raw(0x30), value, null(), &mut out) };
    (result, out)
}

fn create() -> Raw {
    let (result, bridge) = create_with(&request(0));
    assert_eq!(result, S_OK);
    bridge
}

fn chain_of(bridge: Raw) -> *mut Chain {
    unsafe { bridge_chain(bridge) }
}

unsafe fn faulted(chain: *mut Chain) -> u32 {
    unsafe { owner(chain) }.map_or(1, |o| u32::from(o.faulted.load(Ordering::Acquire)))
}

fn present(bridge: Raw, sync: u32, flags: u32) -> i32 {
    unsafe { rsf_core_d3d11p_present(chain_of(bridge), sync, flags) }
}

fn get_buffer(chain: *mut Chain, index: u32) -> (i32, Raw) {
    let mut out = raw(0xDEAD);
    let result = unsafe { rsf_core_d3d11p_get_buffer(chain, index, null(), &mut out) };
    (result, out)
}

fn status() -> (i32, PresentStatus) {
    let mut value: PresentStatus = unsafe { std::mem::zeroed() };
    value.struct_size = size_of::<PresentStatus>() as u32;
    value.version = STATUS_VERSION;
    let result = unsafe { rsf_d3d11_present_status_get(&mut value) };
    (result, value)
}

// Tests -------------------------------------------------------------------------------------

// Ported from the copied-status part of tests/d3d11_present_bridge.cpp.
#[test]
fn copied_status_contract() {
    let read = |cache: &Snapshot| {
        let mut value: PresentStatus = unsafe { std::mem::zeroed() };
        value.struct_size = size_of::<PresentStatus>() as u32;
        value.version = STATUS_VERSION;
        let result = unsafe { cache.read(&mut value) };
        (result, value)
    };
    let mut cache = Snapshot::new();
    let (result, cold) = read(&cache);
    assert!(result == NOT_READY && cold.valid == 0);
    let mut short: PresentStatus = unsafe { std::mem::zeroed() };
    short.struct_size = size_of::<PresentStatus>() as u32 - 1;
    short.version = STATUS_VERSION;
    assert_eq!(unsafe { cache.read(&mut short) }, INVALID_ARGUMENT);
    short.struct_size += 1;
    short.version += 1;
    assert_eq!(unsafe { cache.read(&mut short) }, ABI_MISMATCH);
    assert_eq!(unsafe { cache.read(null_mut()) }, INVALID_ARGUMENT);
    let mut owner = EMPTY_STATUS;
    owner.struct_size = size_of::<rsf_fg_status>() as u32;
    owner.version_id = 7;
    owner.total_presented = 7001;
    cache.activate(2, 7);
    assert!(cache.publish(2, 7, &owner));
    let (result, held) = read(&cache);
    assert_eq!(result, OK);
    cache.invalidate();
    assert!(!cache.publish(2, 7, &owner));
    let (result, value) = read(&cache);
    assert!(result == NOT_READY && value.valid == 0 && value.state.total_presented == 0);
    // The copied receipt remains usable after invalidation.
    assert!(held.generation == 7 && held.state.total_presented == 7001);
    cache.activate(3, 8);
    let mut current = EMPTY_STATUS;
    current.struct_size = size_of::<rsf_fg_status>() as u32;
    current.version_id = 8;
    assert!(!cache.publish(2, 7, &current) && !cache.publish(2, 8, &current));
    assert!(cache.publish(3, 8, &current));
    current.struct_size -= 1;
    assert!(!cache.publish(3, 8, &current));
    current.struct_size += 1;
    cache.clear();
    assert_eq!(read(&cache).0, NOT_READY);
    assert!(cache.publish(3, 8, &current));

    let shared = Mutex::new(Snapshot::new());
    let done = AtomicBool::new(false);
    let failed = AtomicBool::new(false);
    let observed = AtomicU32::new(0);
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                while !done.load(Ordering::SeqCst) {
                    let mut copy: PresentStatus = unsafe { std::mem::zeroed() };
                    copy.struct_size = size_of::<PresentStatus>() as u32;
                    copy.version = STATUS_VERSION;
                    let result = unsafe { lock(&shared).read(&mut copy) };
                    if result == OK {
                        if copy.valid == 0
                            || copy.state.version_id != copy.generation
                            || copy.backend != (copy.generation % 3 + 1) as u32
                            || copy.state.total_presented / 1000 != copy.generation
                        {
                            failed.store(true, Ordering::SeqCst);
                        }
                        observed.fetch_add(1, Ordering::SeqCst);
                    } else if result != NOT_READY
                        || copy.valid != 0
                        || copy.state.total_presented != 0
                    {
                        failed.store(true, Ordering::SeqCst);
                    }
                }
            });
        }
        for generation in 9..4096_u64 {
            let mut cache = lock(&shared);
            cache.invalidate();
            let backend = (generation % 3 + 1) as u32;
            cache.activate(backend, generation);
            let mut status = EMPTY_STATUS;
            status.struct_size = size_of::<rsf_fg_status>() as u32;
            status.version_id = generation;
            status.total_presented = generation * 1000 + 17;
            if !cache.publish(backend, generation, &status) {
                failed.store(true, Ordering::SeqCst);
            }
        }
        lock(&shared).invalidate();
        done.store(true, Ordering::SeqCst);
    });
    assert!(!failed.load(Ordering::SeqCst));
    assert_eq!(read(&lock(&shared)).0, NOT_READY);
}

#[test]
fn prepare_refuses_invalid_setup_and_selects_cold_backend() {
    let _guard = reset();
    let mut value = setup(0, 0);
    assert_eq!(unsafe { rsf_core_d3d11p_prepare(null(), 0) }, 0);
    assert_eq!(
        unsafe { rsf_core_d3d11p_prepare(&value, 1) },
        0,
        "installed hooks refuse"
    );
    value.struct_size -= 1;
    assert_eq!(prepare(&value), 0);
    value.struct_size += 1;
    for units in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        value.units_to_meters = units;
        assert_eq!(prepare(&value), 0);
    }
    assert!(with(|fake| fake.logs.iter().all(
        |line| line.contains("units_to_meters must be finite and positive")
    )));
    value.units_to_meters = 1.0;
    value.backend = 2;
    assert_eq!(prepare(&value), 0, "unknown backend");
    value.backend = AUTO;
    assert_eq!(prepare(&value), 0, "Auto needs runtime switching");
    value.runtime_switching = 1;
    assert_eq!(prepare(&value), 1);
    value.allowed_backend_choices = 1 << 6;
    assert_eq!(prepare(&value), 0, "choices outside the provider set");
    value.allowed_backend_choices = 1 << FSR3;
    value.backend = XESS;
    assert_eq!(prepare(&value), 0, "cold backend outside the choices");
    value.allowed_backend_choices = 0;
    value.backend = 0;
    value.runtime_switching = 0;
    assert_eq!(prepare(&value), 1);
    assert_eq!(
        rsf_d3d11_present_backend(),
        DLSS,
        "zero keeps the DLSS default"
    );
    value.runtime_switching = 1;
    assert_eq!(prepare(&value), 1);
    assert_eq!(rsf_d3d11_present_backend(), 0, "switching starts Off");
    let copy = settings();
    let directory = |text: *const c_char| unsafe { CStr::from_ptr(text) }.to_str().unwrap();
    assert_eq!(
        directory(copy.fsr4_directory_utf8),
        "runtime",
        "SDK folders fall back to the runtime folder"
    );
    assert_eq!(directory(copy.streamline_directory_utf8), "runtime");
    assert!(prepared().is_some_and(|value| value.identity && !value.streamline_explicit));
}

#[test]
fn creation_admission_publication_and_retirement() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    let mut refused = request(0);
    refused.sample_count = 4;
    assert_eq!(create_with(&refused).0, DXGI_ERROR_UNSUPPORTED);
    refused = request(0);
    refused.format = 2;
    assert_eq!(create_with(&refused).0, DXGI_ERROR_UNSUPPORTED);
    refused = request(0);
    refused.buffer_count = 9;
    assert_eq!(create_with(&refused).0, DXGI_ERROR_UNSUPPORTED);
    refused = request(0);
    refused.window = null_mut();
    assert_eq!(create_with(&refused).0, DXGI_ERROR_UNSUPPORTED);
    assert!(take_calls().is_empty());

    let bridge = create();
    assert_eq!(take_calls(), ["create 3", "adopt"]);
    assert_eq!(rsf_core_d3d11p_active_bridge(), bridge);
    assert_eq!(unsafe { rsf_d3d11_present_is_owner(bridge) }, 1);
    assert_eq!(rsf_d3d11_present_has_owner(), 1);
    assert_eq!(dispatch_admit(bridge), 1);
    assert_eq!(dispatch_admit(raw(0x1234)), 0);
    assert_eq!(
        rsf_d3d11_present_backend_choices() & (1 << FSR4),
        0,
        "FSR4 needs SDK support"
    );
    assert_eq!(rsf_d3d11_present_session(), raw(0x1001));
    let (result, value) = status();
    assert!(result == OK && value.backend == FSR3 && value.generation == 1);
    assert_eq!(
        create_with(&request(0)).0,
        DXGI_ERROR_UNSUPPORTED,
        "one active owner"
    );
    let mut short = size_of::<u32>() as u32;
    assert_eq!(
        unsafe { rsf_d3d11_present_graphics(ptr::from_mut(&mut short).cast()) },
        0
    );
    assert_eq!(short, size_of::<u32>() as u32);
    let mut graphics = NO_GRAPHICS;
    graphics.struct_size = 48;
    assert_eq!(unsafe { rsf_d3d11_present_graphics(&mut graphics) }, 1);
    let interop = with(|fake| com_raw(fake.interop()));
    assert!(
        graphics.queue == interop
            && graphics.native_queue.is_null()
            && graphics.device == raw(0x40)
    );
    let source_queue = unsafe { rsf_d3d11_present_source_queue() };
    assert_eq!(source_queue, interop);
    assert_eq!(with(|fake| fake.interop().refs.get()), 3);
    drop(unsafe { com_ref::owned(source_queue) });
    assert!(unsafe { rsf_core_d3d11p_duplicate_application_ready(chain_of(bridge)) }.is_null());

    unsafe { bridge_release_owned(bridge) };
    assert_eq!(
        take_calls(),
        [
            "signal_wait 1 1",
            "signal_wait 0 2",
            "wait_fence 7",
            "release_presentation",
            "destroy 1001",
            "close completion event"
        ]
    );
    // Without a private host the pairs outlive event closure, as the facade members did.
    with(|fake| {
        assert_eq!(fake.held_at_release, [SLOTS]);
        assert_eq!(fake.commands.len(), SLOTS);
        assert_eq!(held_slot_pairs(fake), 0);
    });
    assert!(rsf_core_d3d11p_active_bridge().is_null() && rsf_d3d11_present_session().is_null());
    assert_eq!(generation(), 2);
    assert!(!RETIREMENT_PENDING.load(Ordering::Acquire));
    assert_eq!(rsf_d3d11_present_has_owner(), 0);
    assert_eq!(status().0, NOT_READY);
    assert_eq!(unsafe { rsf_d3d11_present_graphics(&mut graphics) }, 0);
}

#[test]
fn failed_initialization_releases_unpublished_facade() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    with(|fake| {
        fake.create_results.insert(FSR3, INIT_FAILED);
    });
    assert_eq!(create_with(&request(0)).0, E_FAIL);
    assert!(rsf_core_d3d11p_active_bridge().is_null());
    assert!(!RETIREMENT_PENDING.load(Ordering::Acquire));
    let calls = take_calls();
    assert_eq!(calls[0], "create 3");
    assert!(calls.contains(&"close completion event".to_owned()));
    assert!(with(|fake| fake
        .logs
        .iter()
        .any(|line| line.contains("create refused (-6)"))));
    // Strict DLSS without an engine identity keeps the application's own presentation.
    let mut strict = setup(DLSS, 0);
    strict.project_id_utf8 = null();
    assert_eq!(prepare(&strict), 1);
    assert_eq!(create_with(&request(0)).0, DXGI_ERROR_UNSUPPORTED);
}

#[test]
fn partial_sync_initialization_retires_acquired_resources() {
    use SyncFailure::{CompletionEvent, Fence, Queue, ReadyEvent};
    for (failure, acquired, drained) in [
        (Queue, &[][..], true),
        (CompletionEvent, &["create interop queue"][..], true),
        (
            Fence,
            &["create interop queue", "create completion event"][..],
            true,
        ),
        (
            ReadyEvent,
            &["create completion event", "create completion fence"][..],
            true,
        ),
        (
            ReadyEvent,
            &["create completion event", "create completion fence"][..],
            false,
        ),
    ] {
        let _guard = reset();
        assert_eq!(prepare(&setup(FSR3, 1)), 1);
        with(|fake| {
            fake.native12 = failure == ReadyEvent;
            fake.sync_failure = Some(failure);
            fake.drain_result = if drained { S_OK } else { E_FAIL };
        });
        let flags = DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT;
        assert_eq!(create_with(&request(flags)), (E_FAIL, null_mut()));
        let mut sequence = acquired.to_vec();
        sequence.push(match failure {
            Queue => "create interop queue",
            CompletionEvent => "create completion event",
            Fence => "create completion fence",
            ReadyEvent => "create ready event",
        });
        if drained {
            if matches!(failure, Fence | ReadyEvent) {
                sequence.push("close completion event");
            }
            sequence.push("release direct graphics");
        }
        let calls = take_calls();
        if failure == ReadyEvent {
            assert_eq!(calls[0], "signal_wait 1 1");
            assert_eq!(calls[1], if drained { "signal_wait 0 2" } else { "retain" });
        } else {
            assert!(!calls.iter().any(|call| call.starts_with("signal_wait")));
        }
        with(|fake| {
            assert_eq!(fake.sync_calls, sequence);
            let refs = if drained { 1 } else { 2 };
            assert_eq!(fake.interop().refs.get(), refs);
            assert_eq!(fake.completion_fence.refs.get(), refs);
        });
    }
}

// A refused third pair leaves the first two with the unpublished facade. A private host's
// interop queue goes first, then the pairs, then the host. Without one the pairs end after
// the events close. A failed drain retains them.
#[test]
fn partial_slot_creation_releases_or_retains_created_pairs() {
    for (private, drained) in [(false, true), (true, true), (true, false)] {
        let _guard = reset();
        assert_eq!(prepare(&setup(FSR3, 1)), 1);
        with(|fake| {
            fake.slot_failure = Some(2);
            fake.host_available = private;
            if !drained {
                fake.drain_result = E_FAIL;
            }
        });
        assert_eq!(create_with(&request(0)).0, E_FAIL);
        assert!(rsf_core_d3d11p_active_bridge().is_null());
        let calls = take_calls();
        let (tail, held_at_release, held) = match (private, drained) {
            (_, false) => (vec!["signal_wait 1 1", "retain"], vec![], 2),
            (false, true) => (
                vec!["release_presentation", "close completion event"],
                vec![2],
                0,
            ),
            (true, true) => (
                vec![
                    "release_presentation",
                    "close completion event",
                    "release_host",
                ],
                vec![2, 0],
                0,
            ),
        };
        assert_eq!(calls[calls.len() - tail.len()..], tail[..]);
        with(|fake| {
            assert_eq!(fake.commands.len(), 2);
            assert_eq!(fake.held_at_release, held_at_release);
            assert_eq!(held_slot_pairs(fake), held);
        });
    }
}

#[test]
fn unpublished_provider_with_failed_drain_blocks_replacement() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    let mut fullscreen = request(0);
    fullscreen.windowed = 0;
    with(|fake| {
        fake.fullscreen_result = DXGI_ERROR_UNSUPPORTED;
        fake.drain_result = E_FAIL;
    });
    assert_eq!(create_with(&fullscreen).0, E_FAIL);
    assert!(rsf_core_d3d11p_active_bridge().is_null());
    assert_eq!(generation(), 1);
    let calls = take_calls();
    assert!(calls.iter().any(|call| call == "create 3"));
    assert!(calls.iter().any(|call| call == "retain"));

    assert_eq!(create_with(&request(0)).0, DXGI_ERROR_UNSUPPORTED);
    assert!(take_calls().is_empty());
}

#[test]
fn d3d11_present_orders_callbacks_and_rotates_slots() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    let bridge = create();
    take_calls();
    for _ in 0..4 {
        assert_eq!(present(bridge, 1, 0), S_OK);
    }
    let calls = take_calls();
    assert_eq!(
        calls[..15],
        [
            "observer",
            "present_event",
            "before",
            "upload 1",
            "copy 0 0",
            "prepare",
            "return 2",
            "latency_begin",
            "present",
            "latency_done",
            "after",
            "retire",
            "slot_signal 3",
            "observed",
            "presented"
        ]
    );
    // Three slots, each reused only after the fence of its previous frame.
    assert_eq!(
        with(|fake| fake.slots.clone()),
        [(0, 0), (1, 0), (2, 0), (0, 3)]
    );
    assert!(calls.contains(&"slot_signal 12".to_owned()));
    // The application's tearing request is dropped without the capability.
    assert_eq!(present(bridge, 0, DXGI_PRESENT_ALLOW_TEARING), S_OK);
    assert_eq!(with(|fake| *fake.presents.last().unwrap()), (0, 0));
    assert_eq!(present(bridge, 0, DXGI_PRESENT_TEST), S_OK);
    assert_eq!(take_calls().last().unwrap(), "present_test");
    // A failed physical Present faults the facade without touching native work again.
    with(|fake| fake.present_result = DXGI_ERROR_DEVICE_REMOVED);
    assert_eq!(present(bridge, 1, 0), DXGI_ERROR_DEVICE_REMOVED);
    take_calls();
    assert_eq!(present(bridge, 1, 0), DXGI_ERROR_DEVICE_REMOVED);
    assert!(take_calls().is_empty());
    assert_eq!(unsafe { faulted(chain_of(bridge)) }, 1);
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn present_interval_and_tearing_policy() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    with(|fake| fake.tearing = true);
    let bridge = create();
    assert_eq!(present(bridge, 0, 0), S_OK);
    assert_eq!(
        with(|fake| fake.presents[0]),
        (0, DXGI_PRESENT_ALLOW_TEARING)
    );
    with(|fake| fake.fullscreen = 1);
    assert_eq!(present(bridge, 0, 0), S_OK);
    assert_eq!(
        with(|fake| fake.presents[1]),
        (0, 0),
        "exclusive fullscreen never tears"
    );
    assert_eq!(present(bridge, 3, 0), S_OK);
    assert_eq!(
        with(|fake| fake.presents[2]),
        (3, 0),
        "inactive FG keeps the interval"
    );
    with(|fake| fake.status.effective_mode = RSF_FG_FIXED);
    assert_eq!(present(bridge, 3, 0), S_OK);
    assert_eq!(present(bridge, 3, 0), S_OK);
    assert_eq!(with(|fake| fake.presents[3..].to_vec()), [(1, 0), (1, 0)]);
    let normalized = with(|fake| {
        fake.logs
            .iter()
            .filter(|line| line.contains("interval normalized"))
            .count()
    });
    assert_eq!(normalized, 1, "logged once per requested interval");
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn native12_switching_sequences_frames_and_providers() {
    let _guard = reset();
    let mut value = setup(AUTO, 1);
    value.allowed_backend_choices = 0;
    assert_eq!(prepare(&value), 1);
    with(|fake| {
        fake.native12 = true;
        fake.adapter_default = XESS;
    });
    let (result, bridge) =
        create_with(&request(DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT));
    assert_eq!(result, S_OK);
    let chain = chain_of(bridge);
    assert_eq!(
        unsafe { rsf_core_d3d11p_duplicate_application_ready(chain) },
        raw(0x63)
    );
    assert_eq!(
        take_calls(),
        [
            "make_render 640x480 28",
            "make_render 640x480 28",
            "app_ready 0 0"
        ]
    );
    assert_eq!(
        rsf_d3d11_present_backend(),
        XESS,
        "Auto resolved from the owning adapter"
    );
    assert_eq!(unsafe { rsf_core_d3d11p_physical_pending(chain) }, 1);
    assert!(rsf_d3d11_present_session().is_null());

    assert_eq!(present(bridge, 1, 0), S_OK);
    let calls = take_calls();
    assert_eq!(
        calls[..2],
        ["create 5", "adopt"],
        "lazy physical creation at the first Present"
    );
    assert!(calls.contains(&"copy 0 0".to_owned()) && calls.contains(&"app_ready 1 1".to_owned()));
    assert_eq!(unsafe { rsf_core_d3d11p_render_index(chain) }, 1);
    assert_eq!(present(bridge, 1, 0), S_OK);
    assert!(take_calls().contains(&"copy 1 1".to_owned()));
    assert_eq!(unsafe { rsf_core_d3d11p_render_index(chain) }, 0);
    assert_eq!(unsafe { rsf_core_d3d11p_application_presents(chain) }, 2);
    assert_eq!(
        unsafe { rsf_core_d3d11p_set_max_latency(chain, 0) },
        DXGI_ERROR_INVALID_CALL
    );
    assert_eq!(unsafe { rsf_core_d3d11p_set_max_latency(chain, 5) }, S_OK);
    let mut latency = 0;
    assert_eq!(
        unsafe { rsf_core_d3d11p_max_latency(chain, &mut latency) },
        S_OK
    );
    assert_eq!(latency, 2, "bounded by the application buffer count");

    assert_eq!(rsf_d3d11_present_request(FSR4), NOT_SUPPORTED);
    assert_eq!(rsf_d3d11_present_switch_result(), NOT_SUPPORTED);
    assert_eq!(rsf_d3d11_present_request(FSR3), OK);
    assert_eq!(present(bridge, 1, 0), S_OK);
    let calls = take_calls();
    let switch = calls.iter().position(|call| call == "probe 3").unwrap();
    assert_eq!(
        calls[switch - 3..switch + 4],
        [
            "signal_wait 1 4",
            "signal_wait 0 5",
            "wait_fence 7",
            "probe 3",
            "destroy 1002",
            "destroy 1001",
            "create 3"
        ]
    );
    assert_eq!(rsf_d3d11_present_backend(), FSR3);
    assert_eq!(rsf_d3d11_present_generation(), 2);
    assert_eq!(rsf_d3d11_present_switch_result(), OK);
    let (result, value) = status();
    assert!(result == OK && value.backend == FSR3 && value.generation == 2);

    // A provider refused after its probe restores the previous one under a new generation.
    with(|fake| {
        fake.real_results.insert(XESS, INIT_FAILED);
    });
    assert_eq!(rsf_d3d11_present_request(XESS), OK);
    assert_eq!(present(bridge, 1, 0), S_OK);
    assert_eq!(rsf_d3d11_present_backend(), FSR3);
    assert_eq!(rsf_d3d11_present_switch_result(), INIT_FAILED);
    assert_eq!(rsf_d3d11_present_generation(), 3);
    with(|fake| fake.real_results.clear());
    assert_eq!(rsf_d3d11_present_request(XESS), OK);
    take_calls();
    // Leased providers postpone the switch to a later Present without losing the request.
    {
        let _lease = PROVIDER_GUARD.read().unwrap();
        assert_eq!(present(bridge, 1, 0), S_OK);
        assert_eq!(rsf_d3d11_present_backend(), FSR3);
        assert_eq!(REQUESTED_BACKEND.load(Ordering::SeqCst), XESS);
    }
    assert_eq!(present(bridge, 1, 0), S_OK);
    assert_eq!(rsf_d3d11_present_backend(), XESS);
    unsafe { bridge_release_owned(bridge) };
    assert!(with(|fake| fake.sync_calls.ends_with(&[
        "close completion event",
        "close ready event",
        "release direct graphics",
    ])));
}

#[test]
fn switch_probe_refusal_keeps_and_double_refusal_presents_plain() {
    let _guard = reset();
    assert_eq!(prepare(&setup(AUTO, 1)), 1);
    with(|fake| {
        fake.native12 = true;
        fake.adapter_default = XESS;
    });
    let bridge = create();
    assert_eq!(present(bridge, 1, 0), S_OK);
    // A refused probe changes nothing: same provider, same generation.
    with(|fake| {
        fake.create_results.insert(FSR3, INIT_FAILED);
    });
    assert_eq!(rsf_d3d11_present_request(FSR3), OK);
    take_calls();
    assert_eq!(present(bridge, 1, 0), S_OK);
    assert!(!take_calls().iter().any(|call| call.starts_with("destroy")));
    assert_eq!(with(|fake| fake.physical[0].chain.refs.get()), 2);
    assert_eq!(rsf_d3d11_present_switch_result(), INIT_FAILED);
    assert_eq!(rsf_d3d11_present_backend(), XESS);
    assert_eq!(rsf_d3d11_present_generation(), 1);
    // Probe succeeds, then both the new and the restored provider are refused.
    with(|fake| {
        fake.create_results.clear();
        fake.real_results.insert(FSR3, INIT_FAILED);
        fake.real_results.insert(XESS, INIT_FAILED);
    });
    assert_eq!(rsf_d3d11_present_request(FSR3), OK);
    assert_eq!(present(bridge, 1, 0), S_OK);
    let calls = take_calls();
    let probe = calls.iter().position(|call| call == "probe 3").unwrap();
    assert_eq!(
        calls[probe..probe + 6],
        [
            "probe 3",
            "destroy 1002",
            "destroy 1001",
            "create 3",
            "create 5",
            "plain"
        ]
    );
    assert_eq!(rsf_d3d11_present_switch_result(), INIT_FAILED);
    assert_eq!(rsf_d3d11_present_backend(), 0);
    assert_eq!(rsf_d3d11_present_generation(), 2);
    assert!(rsf_d3d11_present_session().is_null());
    assert_eq!(present(bridge, 1, 0), S_OK, "plain presentation continues");
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn provider_teardown_refusal_defers_replacement_until_retry_succeeds() {
    let _guard = reset();
    assert_eq!(prepare(&setup(XESS, 1)), 1);
    with(|fake| {
        fake.native12 = true;
        fake.checked_provider = true;
    });
    let bridge = create();
    assert_eq!(present(bridge, 1, 0), S_OK);
    let outgoing = rsf_d3d11_present_session();
    assert!(!outgoing.is_null());
    take_calls();

    with(|fake| fake.refused_destroy = outgoing);
    assert_eq!(rsf_d3d11_present_request(FSR3), OK);
    assert_eq!(present(bridge, 1, 0), S_OK);
    assert_eq!(rsf_d3d11_present_switch_result(), NOT_READY);
    assert_eq!(rsf_d3d11_present_generation(), 1);
    assert!(rsf_d3d11_present_session().is_null());
    let refused = format!("refused destroy {:x}", outgoing as usize);
    let calls = take_calls();
    assert!(calls.contains(&refused));
    assert!(!calls.iter().any(|call| call == "create 3"));

    with(|fake| fake.refused_destroy = null_mut());
    assert_eq!(present(bridge, 1, 0), S_OK);
    assert_eq!(rsf_d3d11_present_switch_result(), OK);
    assert_eq!(rsf_d3d11_present_backend(), FSR3);
    assert_eq!(rsf_d3d11_present_generation(), 2);
    let calls = take_calls();
    let destroyed = format!("destroy {:x}", outgoing as usize);
    let retired = calls.iter().position(|call| call == &destroyed).unwrap();
    let created = calls.iter().position(|call| call == "create 3").unwrap();
    assert!(retired < created);
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn ensure_failure_falls_back_to_plain_presentation() {
    let _guard = reset();
    assert_eq!(prepare(&setup(XESS, 1)), 1);
    with(|fake| {
        fake.create_results.insert(XESS, INIT_FAILED);
    });
    let bridge = create();
    take_calls();
    assert_eq!(present(bridge, 1, 0), S_OK);
    assert_eq!(take_calls()[..2], ["create 5", "plain"]);
    assert_eq!(rsf_d3d11_present_backend(), 0);
    assert_eq!(rsf_d3d11_present_switch_result(), INIT_FAILED);
    assert!(rsf_d3d11_present_session().is_null());
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn physical_lease_defers_switch_and_keeps_callbacks_on_the_creation_queue() {
    for native12 in [false, true] {
        let _guard = reset();
        let mut value = setup(FSR3, 1);
        value.streamline_directory_utf8 = c"streamline".as_ptr();
        assert_eq!(prepare(&value), 1);
        with(|fake| {
            fake.native12 = native12;
            fake.host_available = true;
            fake.windowed = 0;
            fake.fullscreen = 1;
            fake.reenter_physical = true;
        });
        let bridge = create();
        let chain = chain_of(bridge);
        assert_eq!(
            physical_lease(chain, 0),
            (DXGI_ERROR_WAS_STILL_DRAWING, null(), null_mut())
        );
        assert_eq!(
            unsafe { rsf_d3d11_present_command_list(raw(0xC1)) },
            raw(0xC1)
        );
        assert_eq!(present(bridge, 0, DXGI_PRESENT_TEST), S_OK);
        let (result, old_lease, old_chain) = physical_lease(chain, 1);
        assert_eq!(result, S_OK);
        let old_queue = with(|fake| {
            if native12 {
                com_raw(&fake.direct_queue)
            } else {
                com_raw(&fake.native_queue)
            }
        });
        assert_eq!(unsafe { rsf_d3d11_present_queue() }, old_queue);
        assert_eq!(rsf_d3d11_present_request(DLSS), OK);
        assert_eq!(present(bridge, 1, 0), S_OK);
        assert_eq!(rsf_d3d11_present_backend(), FSR3);
        assert_eq!(rsf_d3d11_present_generation(), 1);
        assert_eq!(REQUESTED_BACKEND.load(Ordering::SeqCst), DLSS);
        assert_chain(old_chain);
        assert_queue(old_queue);
        unsafe { rsf_core_d3d11p_release_physical(old_lease) };

        assert_eq!(present(bridge, 1, 0), S_OK);
        assert_eq!(rsf_d3d11_present_backend(), DLSS);
        assert_eq!(rsf_d3d11_present_generation(), 2);
        let (result, new_lease, new_chain) = physical_lease(chain, 0);
        assert_eq!(result, S_OK);
        assert_ne!(new_chain, old_chain);
        let new_queue = with(|fake| {
            assert_eq!(fake.physical[0].chain.refs.get(), 1);
            assert_eq!(
                unsafe { &*old_queue.cast::<FakeCom>() }.refs.get(),
                1 + u32::from(native12)
            );
            com_raw(&fake.proxy_queue)
        });
        assert_eq!(unsafe { rsf_d3d11_present_queue() }, new_queue);
        unsafe { rsf_core_d3d11p_release_physical(new_lease) };
        assert_eq!(present(bridge, 1, 0), S_OK);
        unsafe { bridge_release_owned(bridge) };
        assert!(unsafe { rsf_d3d11_present_queue() }.is_null());
        with(|fake| {
            assert_eq!(fake.physical[1].chain.refs.get(), 1);
            assert_eq!(fake.proxy_queue.refs.get(), 1);
            assert_eq!(fake.interop().refs.get(), 2, "shared host keeps the queue");
        });
    }
}

#[test]
fn failed_creation_releases_partial_pairs_before_fallback() {
    for queue_missing in [false, true] {
        let _guard = reset();
        assert_eq!(prepare(&setup(XESS, 1)), 1);
        with(|fake| {
            fake.query_chain_result = E_FAIL;
            fake.queue_missing = queue_missing;
        });
        let bridge = create();
        let chain = chain_of(bridge);
        let (result, lease, physical) = physical_lease(chain, 1);
        assert_eq!(rsf_d3d11_present_backend(), 0);
        assert_eq!(rsf_d3d11_present_switch_result(), INIT_FAILED);
        assert!(rsf_d3d11_present_session().is_null());
        let calls = take_calls();
        assert!(calls.ends_with(&[
            "create 5".into(),
            "adopt".into(),
            "destroy 1001".into(),
            "plain".into()
        ]));
        with(|fake| assert_eq!(fake.physical[0].chain.refs.get(), 1));
        if queue_missing {
            assert_eq!((result, lease, physical), (E_FAIL, null(), null_mut()));
            assert_eq!(unsafe { faulted(chain) }, 1);
            assert!(unsafe { rsf_d3d11_present_queue() }.is_null());
            with(|fake| {
                assert_eq!(fake.physical[1].chain.refs.get(), 1);
                assert_eq!(fake.native_queue.refs.get(), 1);
            });
        } else {
            assert_eq!(result, S_OK);
            assert_chain(physical);
            assert_queue(unsafe { rsf_d3d11_present_queue() });
            unsafe { rsf_core_d3d11p_release_physical(lease) };
        }
        unsafe { bridge_release_owned(bridge) };
    }
}

#[test]
fn resize_normalizes_and_preserves_shared_surfaces_on_allocation_failure() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    let bridge = create();
    let chain = chain_of(bridge);
    take_calls();
    assert_eq!(
        unsafe { rsf_core_d3d11p_resize(chain, 9, 1, 1, 0, 0) },
        DXGI_ERROR_INVALID_CALL
    );
    assert_eq!(
        unsafe { rsf_core_d3d11p_resize(chain, 0, 0, 0, 0, 2) },
        S_OK
    );
    let calls = take_calls();
    assert!(calls.contains(&"resize 3 800x600".to_owned()));
    assert_eq!(
        with(|fake| (
            fake.width,
            fake.height,
            fake.format,
            fake.flags,
            fake.buffer_count
        )),
        (800, 600, 28, 2, 2)
    );
    let (result, pointer) = get_buffer(chain, 1);
    assert_eq!(result, S_OK);
    let retained = unsafe { com_ref::owned(pointer) }.unwrap();
    assert_eq!(
        unsafe { rsf_core_d3d11p_resize(chain, 2, 320, 200, 0, 0) },
        DXGI_ERROR_INVALID_CALL
    );
    drop(retained);
    take_calls();
    with(|fake| {
        fake.allocation_failure = Some(fake.buffers.len() + 1);
    });
    assert_eq!(
        unsafe { rsf_core_d3d11p_resize(chain, 2, 320, 200, 0, 0) },
        E_FAIL
    );
    assert!(!take_calls().contains(&"resize 3 320x200".to_owned()));
    assert_eq!(unsafe { faulted(chain) }, 0);
    let (result, still_old) = get_buffer(chain, 1);
    assert_eq!((result, still_old), (S_OK, pointer));
    drop(unsafe { com_ref::owned(still_old) });
    with(|fake| {
        assert_eq!(fake.buffers.len(), 6);
        assert_eq!(fake.target_view.refs.get(), 3);
        assert!(
            fake.buffers[4..]
                .iter()
                .all(|buffer| buffer.application.refs.get() == 1
                    && buffer.peer.as_ref().unwrap().refs.get() == 1)
        );
    });
    assert_eq!(present(bridge, 1, 0), S_OK);
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn partial_render_buffer_creation_rolls_back_without_publication() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 1)), 1);
    with(|fake| {
        fake.native12 = true;
        fake.allocation_failure = Some(1);
    });
    assert_eq!(create_with(&request(0)), (E_FAIL, null_mut()));
    assert!(rsf_core_d3d11p_active_bridge().is_null());
    with(|fake| {
        assert_eq!(fake.buffers.len(), 2);
        assert!(fake.buffers.iter().all(|b| b.application.refs.get() == 1));
    });
}

#[test]
fn failed_render_buffer_resize_exposes_no_partial_batch() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 1)), 1);
    with(|fake| fake.native12 = true);
    let bridge = create();
    let chain = chain_of(bridge);
    with(|fake| fake.allocation_failure = Some(3));
    assert_eq!(
        unsafe { rsf_core_d3d11p_resize(chain, 3, 320, 200, 24, 0) },
        E_FAIL
    );
    assert!(!take_calls().contains(&"resize 0 320x200".to_owned()));
    assert_eq!(unsafe { faulted(chain) }, 0);
    let (result, old) = get_buffer(chain, 0);
    assert_eq!(
        (result, old),
        (S_OK, with(|fake| com_raw(&fake.buffers[0].application)))
    );
    drop(unsafe { com_ref::owned(old) });
    with(|fake| {
        assert_eq!(fake.buffers.len(), 4);
        assert!(
            fake.buffers[2..]
                .iter()
                .all(|b| b.application.refs.get() == 1)
        );
    });
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn native_resize_publishes_new_bounds_only_after_physical_success() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 1)), 1);
    with(|fake| fake.native12 = true);
    let bridge = create();
    let chain = chain_of(bridge);
    assert_eq!(present(bridge, 1, 0), S_OK);
    assert_eq!(unsafe { rsf_core_d3d11p_render_index(chain) }, 1);

    let (result, old) = get_buffer(chain, 1);
    assert_eq!(result, S_OK);
    let held = unsafe { com_ref::owned(old) };
    let resize = || unsafe { rsf_core_d3d11p_resize(chain, 3, 320, 200, 24, 0) };
    let configuration_calls = with(|fake| fake.configuration_calls);
    assert_eq!(resize(), DXGI_ERROR_INVALID_CALL);
    assert_eq!(with(|fake| fake.configuration_calls), configuration_calls);
    drop(held);
    with(|fake| fake.resize_result = E_FAIL);
    assert_eq!(resize(), E_FAIL);
    assert_eq!(unsafe { rsf_core_d3d11p_render_index(chain) }, 1);
    let (result, still_old) = get_buffer(chain, 1);
    assert_eq!((result, still_old), (S_OK, old));
    drop(unsafe { com_ref::owned(still_old) });
    assert_eq!(get_buffer(chain, 2), (DXGI_ERROR_INVALID_CALL, null_mut()));
    with(|fake| {
        assert_eq!(fake.buffers.len(), 5);
        assert!(
            fake.buffers[2..]
                .iter()
                .all(|b| b.application.refs.get() == 1)
        );
    });

    with(|fake| fake.resize_result = S_OK);
    assert_eq!(resize(), S_OK);
    assert_eq!(unsafe { rsf_core_d3d11p_render_index(chain) }, 0);
    with(|fake| {
        assert_eq!(fake.buffer_count, 3);
        assert_eq!(fake.buffers.len(), 8);
        assert!(
            fake.buffers[..5]
                .iter()
                .all(|b| b.application.refs.get() == 1)
        );
    });
    let (result, new) = get_buffer(chain, 2);
    assert_eq!(
        (result, new),
        (S_OK, with(|fake| com_raw(&fake.buffers[7].application)))
    );
    drop(unsafe { com_ref::owned(new) });
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn application_buffers_are_queryable_before_physical_creation() {
    for native12 in [false, true] {
        let _guard = reset();
        assert_eq!(prepare(&setup(FSR3, 1)), 1);
        with(|fake| {
            fake.native12 = native12;
            fake.buffer_usage = DXGI_USAGE_UNORDERED_ACCESS;
        });
        let bridge = create();
        let chain = chain_of(bridge);
        assert_eq!(
            unsafe { rsf_core_d3d11p_resize(chain, MAX_BUFFERS + 1, 320, 200, 0, 0) },
            DXGI_ERROR_INVALID_CALL
        );
        assert_eq!(
            get_buffer(chain, MAX_BUFFERS),
            (DXGI_ERROR_INVALID_CALL, null_mut())
        );
        assert_eq!(get_buffer(chain, 2), (DXGI_ERROR_INVALID_CALL, null_mut()));
        assert_eq!(
            unsafe { rsf_core_d3d11p_get_buffer(chain, 0, null(), null_mut()) },
            E_POINTER
        );
        let (result, pointer) = get_buffer(chain, 1);
        assert_eq!(result, S_OK);
        assert_eq!(unsafe { rsf_core_d3d11p_physical_pending(chain) }, 1);
        with(|fake| {
            assert!(fake.physical.is_empty());
            let held = &fake.buffers[1].application;
            assert_eq!(pointer, com_raw(held));
            assert_eq!(held.refs.get(), 3);
        });
        unsafe { bridge_release_owned(bridge) };
        assert_eq!(with(|fake| fake.buffers[1].application.refs.get()), 2);
        drop(unsafe { com_ref::owned(pointer) });
        assert_eq!(with(|fake| fake.buffers[1].application.refs.get()), 1);
    }
}

#[test]
fn strict_native_buffer_queries_use_dxgi_and_clear_refused_output() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    with(|fake| fake.native12 = true);
    let bridge = create();
    let chain = chain_of(bridge);
    assert!(with(|fake| fake.buffers.is_empty()));
    let (result, pointer) = get_buffer(chain, 2);
    assert_eq!(result, S_OK);
    assert_eq!(pointer, with(|fake| com_raw(&fake.backbuffer)));
    assert!(take_calls().contains(&"physical GetBuffer 2".to_owned()));
    drop(unsafe { com_ref::owned(pointer) });
    assert_eq!(
        unsafe { rsf_core_d3d11p_resize(chain, 4, 320, 200, 28, 0) },
        S_OK
    );
    assert!(take_calls().contains(&"resize 4 320x200".to_owned()));
    assert_eq!(with(|fake| fake.buffer_count), 4);
    drop(unsafe { owner(chain) }.unwrap().take_physical());
    for _ in 0..5 {
        assert_eq!(
            get_buffer(chain, 2),
            (DXGI_ERROR_WAS_STILL_DRAWING, null_mut())
        );
    }
    assert_eq!(take_calls(), ["GetBuffer refused"; 4]);
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn present_releases_backbuffer_on_success_and_refusal() {
    for native12 in [false, true] {
        for (acquire, no_output, close, expected) in [
            (S_OK, false, S_OK, S_OK),
            (S_OK, false, E_FAIL, E_FAIL),
            (E_FAIL, false, S_OK, E_FAIL),
            (S_OK, true, S_OK, E_FAIL),
        ] {
            let _guard = reset();
            assert_eq!(prepare(&setup(FSR3, 1)), 1);
            with(|fake| {
                fake.native12 = native12;
                fake.backbuffer_result = acquire;
                fake.backbuffer_null = no_output;
                fake.close_result = close;
            });
            let bridge = create();
            assert_eq!(present(bridge, 1, 0), expected);
            assert_eq!(with(|fake| fake.backbuffer.refs.get()), 1);
            let calls = take_calls();
            assert_eq!(
                calls.iter().any(|call| call == "prepare"),
                acquire == S_OK && !no_output
            );
            assert_eq!(calls.iter().any(|call| call == "present"), expected == S_OK);
            unsafe { bridge_release_owned(bridge) };
        }
    }
}

#[test]
fn retire_lease_matches_session_and_serializes_fences() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    let bridge = create();
    let session = rsf_d3d11_present_session();
    let (mut fence, mut value) = (null_mut(), 0_u64);
    assert_eq!(
        unsafe { rsf_d3d11_present_retire(session, null_mut(), null_mut(), &mut value) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_d3d11_present_retire(raw(0x55), null_mut(), &mut fence, &mut value) },
        NOT_READY
    );
    with(|fake| fake.reenter_retirement = true);
    let (expected_fence, completion_fence) = with(|fake| {
        (
            com_raw(&fake.retirement_fence),
            com_raw(&fake.completion_fence),
        )
    });
    assert_ne!(expected_fence, completion_fence);
    for expected in 1..=2 {
        assert_eq!(
            unsafe { rsf_d3d11_present_retire(session, raw(0xC1), &mut fence, &mut value) },
            OK
        );
        assert!(fence == expected_fence && value == expected);
    }
    assert_eq!(
        with(|fake| fake
            .sync_calls
            .iter()
            .filter(|&&call| call == "create retirement fence")
            .count()),
        1
    );
    with(|fake| fake.retirement_result = NOT_READY);
    assert_eq!(
        unsafe { rsf_d3d11_present_retire(session, null_mut(), &mut fence, &mut value) },
        NOT_READY
    );
    assert!(fence.is_null() && value == 0);
    with(|fake| fake.retirement_result = OK);
    let list = raw(0xC1);
    assert_queue(unsafe { rsf_d3d11_present_queue() });
    assert_eq!(unsafe { rsf_d3d11_present_command_list(list) }, list);
    {
        let _replacement = PROVIDER_GUARD.write().unwrap();
        assert!(unsafe { rsf_d3d11_present_queue() }.is_null());
        assert!(unsafe { rsf_d3d11_present_command_list(list) }.is_null());
        assert_eq!(
            unsafe { rsf_d3d11_present_retire(session, null_mut(), &mut fence, &mut value) },
            NOT_READY
        );
    }
    unsafe { bridge_release_owned(bridge) };
    assert_eq!(with(|fake| fake.retirement_fence.refs.get()), 1);
}

#[test]
fn latency_routes_to_host_or_provider_once() {
    let _guard = reset();
    ACTIVE_BACKEND.store(XESS, Ordering::SeqCst);
    ACTIVE_SESSION.store(raw(0x1234), Ordering::SeqCst);
    ACTIVE_HOST.store(raw(0xAA), Ordering::SeqCst);
    let mut generation = 0;
    assert_eq!(unsafe { rsf_d3d11_present_begin(5, &mut generation) }, OK);
    assert_eq!(generation, 1);
    assert_eq!(unsafe { rsf_d3d11_present_marker(1, 5, 2, 0) }, OK);
    assert_eq!(unsafe { rsf_d3d11_present_input(5, 4, 9) }, OK);
    assert_eq!(unsafe { rsf_d3d11_present_input(5, 0, 9) }, OK);
    assert_eq!(unsafe { rsf_d3d11_present_abort(5) }, OK);
    assert_eq!(
        take_calls(),
        [
            "begin_frame 5",
            "host_marker 2 5",
            "marker 2 5 0",
            "host_input 5 4",
            "marker 6 5 1",
            "host_input 5 0",
            "host_abort 5",
            "abort_frame 5"
        ]
    );
    HOST_PACED.store(true, Ordering::SeqCst);
    assert_eq!(unsafe { rsf_d3d11_present_begin(6, null_mut()) }, OK);
    assert_eq!(unsafe { rsf_d3d11_present_marker(1, 6, 2, 0) }, OK);
    assert_eq!(unsafe { rsf_d3d11_present_abort(6) }, OK);
    assert_eq!(
        take_calls(),
        ["host_sleep 6", "host_marker 2 6", "host_abort 6"]
    );
    // The core names the pacer; a stale generation, null frame or unknown owner is refused.
    let stale = 9;
    assert_eq!(
        unsafe { rsf_core_d3d11p_pace(7, RSF_PACING_REFLEX, &stale, null_mut()) },
        NOT_READY
    );
    assert_eq!(
        unsafe { rsf_core_d3d11p_pace(0, RSF_PACING_REFLEX, null(), null_mut()) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_core_d3d11p_pace(7, 4, null(), null_mut()) },
        INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_core_d3d11p_pace(7, RSF_PACING_REFLEX, null(), &mut generation) },
        OK
    );
    assert_eq!(
        unsafe { rsf_core_d3d11p_pace(8, RSF_PACING_XELL, null(), null_mut()) },
        OK
    );
    assert_eq!(
        unsafe { rsf_core_d3d11p_pace(9, PACING_LAST, null(), null_mut()) },
        OK
    );
    assert_eq!(take_calls(), ["host_sleep 7", "begin_frame 8"]);
    assert_eq!(unsafe { rsf_d3d11_present_receipt(2, 9, 1, 3) }, NOT_READY);
    assert_eq!(unsafe { rsf_d3d11_present_receipt(1, 9, 1, 3) }, OK);
    assert_eq!(unsafe { rsf_d3d11_present_acquire(9) }, OK);
    assert_eq!(take_calls(), ["host_input 9 1", "host_acquire 9"]);
    ACTIVE_HOST.store(null_mut(), Ordering::SeqCst);
    assert_eq!(
        unsafe { rsf_core_d3d11p_pace(10, RSF_PACING_REFLEX, null(), null_mut()) },
        NOT_READY
    );
    assert_eq!(unsafe { rsf_d3d11_present_receipt(1, 9, 1, 3) }, NOT_READY);
    // A caller arriving during provider replacement is turned away, never blocked.
    let _replacement = PROVIDER_GUARD.write().unwrap();
    assert_eq!(
        unsafe { rsf_d3d11_present_begin(11, null_mut()) },
        NOT_READY
    );
    assert!(take_calls().is_empty());
}

#[test]
fn scoped_creation_admits_only_the_verified_outer_chain() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    rsf_core_d3d11p_set_scoped(1);
    let window = raw(0x10);
    let mut token = 0;
    assert_eq!(
        unsafe { rsf_core_d3d11p_creation_begin(window, &mut token, 0) },
        DXGI_ERROR_INVALID_CALL
    );
    assert_eq!(
        rsf_core_d3d11p_capture(window),
        0,
        "no transaction, no capture"
    );
    assert_eq!(
        unsafe { rsf_core_d3d11p_creation_begin(window, &mut token, 1) },
        S_OK
    );
    assert_ne!(token, 0);
    let mut second = 0;
    assert_eq!(
        unsafe { rsf_core_d3d11p_creation_begin(window, &mut second, 1) },
        DXGI_ERROR_INVALID_CALL
    );
    assert_eq!(rsf_core_d3d11p_capture(raw(0x11)), 0);
    assert_eq!(rsf_core_d3d11p_capture(window), 1);
    let bridge = create();
    unsafe { rsf_core_d3d11p_captured(bridge) };
    assert_eq!(
        rsf_core_d3d11p_capture(window),
        0,
        "one capture per transaction"
    );
    assert_eq!(
        physical_lease(chain_of(bridge), 1),
        (DXGI_ERROR_WAS_STILL_DRAWING, null(), null_mut())
    );
    assert_eq!(unsafe { rsf_d3d11_present_is_owner(bridge) }, 0);
    assert_eq!(rsf_d3d11_present_has_owner(), 0);
    assert_eq!(dispatch_admit(bridge), 2);
    let outer = raw(0x500);
    assert_eq!(
        unsafe { rsf_d3d11_present_creation_end(window, token + 1, raw(1), raw(2), outer) },
        E_INVALIDARG
    );
    assert_eq!(
        unsafe { rsf_d3d11_present_creation_end(window, token, raw(1), raw(2), outer) },
        S_OK
    );
    assert_eq!(
        with(|fake| fake.refs),
        0,
        "the captured reference is released"
    );
    assert_eq!(unsafe { rsf_d3d11_present_is_owner(bridge) }, 1);
    assert_eq!(unsafe { rsf_d3d11_present_is_owner(outer) }, 1);
    with(|fake| fake.unpublish_alias_on_lookup = true);
    assert_eq!(
        unsafe { rsf_d3d11_present_is_owner(outer) },
        0,
        "the alias was unpublished while its identity query ran"
    );
    assert_eq!(dispatch_admit(bridge), 1);
    unsafe { bridge_release_owned(bridge) };

    // A refused transaction faults its captured facade and keeps it unadmitted.
    with(|fake| fake.verify_result = E_INVALIDARG);
    token = 0;
    assert_eq!(
        unsafe { rsf_core_d3d11p_creation_begin(window, &mut token, 1) },
        S_OK
    );
    let bridge = create();
    unsafe { rsf_core_d3d11p_captured(bridge) };
    assert_eq!(
        unsafe { rsf_d3d11_present_creation_end(window, token, raw(1), raw(2), outer) },
        E_INVALIDARG
    );
    assert_eq!(unsafe { faulted(chain_of(bridge)) }, 1);
    assert_eq!(dispatch_admit(bridge), 2);
    assert_eq!(present(bridge, 1, 0), DXGI_ERROR_DEVICE_REMOVED);
    unsafe { bridge_release_owned(bridge) };
    rsf_core_d3d11p_set_scoped(0);
}

#[test]
fn failed_retirement_retains_provider_and_inputs() {
    for native12 in [false, true] {
        let _guard = reset();
        assert_eq!(prepare(&setup(FSR3, u32::from(native12))), 1);
        with(|fake| fake.native12 = native12);
        let (result, bridge) =
            create_with(&request(DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT));
        assert_eq!(result, S_OK);
        assert_eq!(present(bridge, 1, DXGI_PRESENT_TEST), S_OK);
        take_calls();
        with(|fake| fake.drain_result = E_FAIL);
        unsafe { bridge_release_owned(bridge) };
        assert_eq!(take_calls(), ["signal_wait 1 1", "retain"]);
        assert!(rsf_core_d3d11p_active_bridge().is_null());
        assert_eq!(generation(), 1);
        assert_eq!(create_with(&request(0)).0, DXGI_ERROR_UNSUPPORTED);
        assert!(take_calls().is_empty());
        with(|fake| {
            assert_eq!(
                held_slot_pairs(fake),
                SLOTS,
                "slot pairs stay GPU-reachable"
            );
            assert!(fake.held_at_release.is_empty());
            let physical = fake.physical.last().unwrap();
            assert_eq!(physical.chain.refs.get(), 2);
            assert_eq!(
                unsafe { &*physical.queue.cast::<FakeCom>() }.refs.get(),
                2 + u32::from(native12)
            );
            assert_eq!(fake.completion_fence.refs.get(), 2);
            assert!(!fake.sync_calls.contains(&"close completion event"));
            assert!(!fake.sync_calls.contains(&"close ready event"));
            assert_eq!(fake.sync_calls.contains(&"create ready event"), native12);
            for buffer in &fake.buffers {
                assert_eq!(buffer.application.refs.get(), 2);
                if let Some(peer) = &buffer.peer {
                    assert_eq!(peer.refs.get(), 2);
                    // Under the fake drain refusal, the Win32 handle is still open.
                    assert_ne!(
                        unsafe { windows_sys::Win32::Foundation::CloseHandle(buffer.handle) },
                        0
                    );
                }
            }
        });
    }
}

#[test]
fn provider_family_is_the_single_backend_mapping() {
    assert_eq!(family(DLSS), Some(FAMILY_DLSS));
    assert_eq!(family(FSR3), Some(FAMILY_FIDELITYFX));
    assert_eq!(family(FSR4), Some(FAMILY_FIDELITYFX));
    assert_eq!(family(XESS), Some(FAMILY_XESS));
    for backend in [0, 2, 6, AUTO, 32, u32::MAX] {
        assert_eq!(family(backend), None);
        assert!(!known(backend));
    }
}

// Defect in the C++ owner: a refused fullscreen entry right after lazy physical creation
// returned before the session and status were published. The chain then presented through
// an unpublished provider: no session for FG or latency, and NOT_READY status until a switch.
#[test]
fn lazy_creation_publishes_before_fullscreen_entry() {
    let _guard = reset();
    assert_eq!(prepare(&setup(XESS, 1)), 1);
    let mut value = request(0);
    value.windowed = 0;
    with(|fake| fake.windowed = 0);
    let (result, bridge) = create_with(&value);
    assert_eq!(result, S_OK);
    with(|fake| fake.fullscreen_result = DXGI_ERROR_UNSUPPORTED);
    assert_eq!(present(bridge, 1, 0), E_FAIL);
    assert_eq!(
        present(bridge, 1, 0),
        S_OK,
        "the created chain keeps presenting"
    );
    assert!(
        !rsf_d3d11_present_session().is_null(),
        "provider session published"
    );
    let (result, value) = status();
    assert!(result == OK && value.backend == XESS);
    unsafe { bridge_release_owned(bridge) };
}

// Defect in the C++ owner: early Present failures after the application notification
// returned without its completion and left the facade accepting further Presents.
#[test]
fn early_present_failures_complete_the_notification_and_fault() {
    let _guard = reset();
    let mut value = setup(DLSS, 1);
    value.streamline_directory_utf8 = c"streamline".as_ptr();
    assert_eq!(prepare(&value), 1);
    with(|fake| {
        fake.native12 = true;
        fake.host_available = true;
    });
    let bridge = create();
    assert_eq!(present(bridge, 1, 0), S_OK);
    take_calls();
    with(|fake| fake.join_result = E_FAIL);
    assert_eq!(present(bridge, 1, 0), DXGI_ERROR_DEVICE_REMOVED);
    let calls = take_calls();
    assert_eq!(calls[..2], ["observer", "present_event"]);
    assert!(calls.contains(&"observed".to_owned()) && calls.contains(&"presented".to_owned()));
    assert_eq!(unsafe { faulted(chain_of(bridge)) }, 1);
    unsafe { bridge_release_owned(bridge) };

    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    with(|fake| {
        fake.native12 = false;
        fake.host_available = false;
        fake.host = 0;
        fake.join_result = S_OK;
    });
    let bridge = create();
    take_calls();
    with(|fake| fake.unwrap_failure = true);
    assert_eq!(present(bridge, 1, 0), E_FAIL);
    let calls = take_calls();
    assert!(calls.contains(&"observed".to_owned()) && calls.contains(&"presented".to_owned()));
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call.as_str(), "prepare" | "present"))
    );
    assert!(calls.iter().any(|call| call.starts_with("slot_signal")));
    assert_eq!(unsafe { faulted(chain_of(bridge)) }, 1);
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn lazy_creation_retries_after_host_load_refusal() {
    let _guard = reset();
    let mut value = setup(FSR3, 1);
    value.streamline_directory_utf8 = c"streamline".as_ptr();
    assert_eq!(prepare(&value), 1);
    with(|fake| {
        fake.host_available = true;
        fake.host_load_result = NOT_READY;
        fake.status.pacing_owner = RSF_PACING_REFLEX;
    });
    let bridge = create();
    let chain = chain_of(bridge);
    assert_eq!(present(bridge, 0, DXGI_PRESENT_TEST), E_FAIL);
    assert_eq!(unsafe { rsf_core_d3d11p_physical_pending(chain) }, 1);
    assert!(with(|fake| fake.physical.is_empty()));
    assert!(rsf_d3d11_present_session().is_null());
    with(|fake| fake.host_load_result = OK);
    assert_eq!(present(bridge, 0, DXGI_PRESENT_TEST), S_OK);
    assert_eq!(unsafe { rsf_core_d3d11p_physical_pending(chain) }, 0);
    assert!(HOST_PACED.load(Ordering::SeqCst));
    assert_eq!(with(|fake| fake.status_reads), 1);
    assert_eq!(status().0, OK);
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn eager_creation_publishes_and_logs_one_status_snapshot() {
    let _guard = reset();
    assert_eq!(prepare(&setup(FSR3, 0)), 1);
    let bridge = create();
    assert_eq!(with(|fake| fake.status_reads), 1);
    assert_eq!(status().0, OK);
    assert!(with(|fake| fake
        .logs
        .iter()
        .any(|line| line.starts_with("Generation support="))));
    unsafe { bridge_release_owned(bridge) };
}

#[test]
fn a_nonblocking_present_is_retryable_only_when_submission_is_fenced() {
    use windows::Win32::Graphics::Dxgi::DXGI_PRESENT_DO_NOT_WAIT;

    for native12 in [false, true] {
        let _guard = reset();
        assert_eq!(prepare(&setup(FSR3, 1)), 1);
        with(|fake| {
            fake.native12 = native12;
            fake.present_result = DXGI_ERROR_WAS_STILL_DRAWING;
        });
        let bridge = create();
        let chain = chain_of(bridge);
        let flags = DXGI_PRESENT_DO_NOT_WAIT.0;
        assert_eq!(present(bridge, 1, flags), DXGI_ERROR_WAS_STILL_DRAWING);
        assert_eq!(with(|fake| fake.presents.last().copied()), Some((1, flags)));
        assert_eq!(unsafe { faulted(chain) }, 0);
        assert_eq!(unsafe { rsf_core_d3d11p_render_index(chain) }, 0);
        assert!(
            take_calls()
                .iter()
                .any(|call| call.starts_with("slot_signal"))
        );
        with(|fake| fake.present_result = S_OK);
        assert_eq!(present(bridge, 1, flags), S_OK);
        assert_eq!(
            unsafe { rsf_core_d3d11p_render_index(chain) },
            u32::from(native12)
        );

        with(|fake| {
            fake.present_result = DXGI_ERROR_WAS_STILL_DRAWING;
            fake.signal_result = E_FAIL;
        });
        assert_eq!(present(bridge, 1, flags), DXGI_ERROR_DEVICE_REMOVED);
        assert_eq!(unsafe { faulted(chain) }, 1);
        assert_eq!(present(bridge, 1, flags), DXGI_ERROR_DEVICE_REMOVED);
        unsafe { bridge_release_owned(bridge) };
    }
}

#[test]
fn linked_chain_dispatch_pins_calls_and_consumes_only_the_release_reference() {
    use crate::d3d11_host_abi::{RSF_D3D11_DISPATCH_PRESENT, RSF_D3D11_HOST_ABI_VERSION};

    unsafe extern "C" fn release(receiver: Raw) -> u32 {
        unsafe { crate::com_ref::test::release(receiver.cast()) }
    }
    unsafe extern "C" fn invoke(request: *mut rsf_d3d11_chain_dispatch) -> i32 {
        let receiver = unsafe { &*((*request).receiver.cast::<FakeCom>()) };
        assert_eq!(receiver.refs.get(), 2);
        S_OK
    }

    let _guard = reset();
    let receiver = fake_com();
    let raw_receiver = com_raw(&receiver);
    ACTIVE_BRIDGE.store(raw_receiver, Ordering::Release);
    let mut request: rsf_d3d11_chain_dispatch = unsafe { std::mem::zeroed() };
    request.struct_size = size_of::<rsf_d3d11_chain_dispatch>() as u32;
    request.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    request.receiver = raw_receiver;
    request.method = RSF_D3D11_DISPATCH_PRESENT;
    unsafe { rsf_core_d3d11p_dispatch_owned(&mut request, release, invoke) };
    let result = unsafe { dispatch_result_slot(&mut request).read() };
    assert_eq!((request.handled, result, receiver.refs.get()), (1, S_OK, 1));

    SCOPED.store(true, Ordering::Release);
    SCOPED_ADMITTED.store(false, Ordering::Release);
    unsafe { rsf_core_d3d11p_dispatch_owned(&mut request, release, invoke) };
    assert_eq!(
        unsafe { dispatch_result_slot(&mut request).read() },
        DXGI_ERROR_INVALID_CALL
    );

    request.method = RSF_D3D11_DISPATCH_RELEASE;
    unsafe { rsf_core_d3d11p_dispatch_owned(&mut request, release, invoke) };
    let result = unsafe { dispatch_result_slot(&mut request).read() };
    assert_eq!((request.handled, result, receiver.refs.get()), (1, 0, 0));
    ACTIVE_BRIDGE.store(null_mut(), Ordering::Release);
}
