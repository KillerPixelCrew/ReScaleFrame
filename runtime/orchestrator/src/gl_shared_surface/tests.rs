// SPDX-License-Identifier: MIT
// Synthetic OpenGL sharing contracts: a recording leaf table and reference-counted fake COM
// objects. No GL context or D3D12 device is involved, so these establish admission, ordering,
// lifetime and recovery policy only. The fence completes only when the CPU waits for a value the
// queue or GL has been asked to signal, so a lease release proves an observed completion.
use super::*;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::c_char;

const E_FAIL: i32 = 0x8000_4005_u32 as i32;
const GL_CONTEXT: Handle = 0x77 as Handle;
const LUID: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

// Read only through the COM calls in com_ref.cpp.
#[allow(dead_code)]
#[repr(C)]
struct Vtable {
    query: usize,
    add_ref: unsafe extern "system" fn(*mut Fake) -> u32,
    release: unsafe extern "system" fn(*mut Fake) -> u32,
}

// IUnknown layout: the vtable pointer first.
#[repr(C)]
struct Fake {
    vtable: &'static Vtable,
    refs: Cell<u32>,
    name: String,
}

unsafe extern "system" fn add_ref(this: *mut Fake) -> u32 {
    let refs = unsafe { &(*this).refs };
    refs.set(refs.get() + 1);
    refs.get()
}

unsafe extern "system" fn release(this: *mut Fake) -> u32 {
    let fake = unsafe { &*this };
    assert!(fake.refs.get() > 0, "over-released {}", fake.name);
    fake.refs.set(fake.refs.get() - 1);
    if fake.refs.get() == 0 {
        event(format!("release {}", fake.name));
        world(|world| world.com.remove(&fake.name));
    }
    fake.refs.get()
}

static VTABLE: Vtable = Vtable {
    query: 0,
    add_ref,
    release,
};

fn com(kind: &str) -> Handle {
    let name = world(|world| {
        world.next += 1;
        let name = format!("{kind}{}", world.next);
        world.com.insert(name.clone());
        name
    });
    Box::into_raw(Box::new(Fake {
        vtable: &VTABLE,
        refs: Cell::new(1),
        name,
    }))
    .cast()
}

fn refs(object: Handle) -> u32 {
    unsafe { (*object.cast::<Fake>()).refs.get() }
}

fn com_name(object: Handle) -> String {
    unsafe { (*object.cast::<Fake>()).name.clone() }
}

#[derive(Default)]
struct World {
    events: Vec<String>,
    logs: Vec<String>,
    next: usize,
    gl_names: u32,
    com: HashSet<String>,
    handles: HashSet<usize>,
    context: usize,
    missing_extension: bool,
    gl_errors: Vec<u32>,
    device_luid: [u8; 8],
    queue_owner: usize,
    signalled: u64,
    completed: u64,
    stall: bool,
    fail_signal: u32,
    fail_queue_wait: bool,
    fail_close: bool,
    fail_semaphore: bool,
    fail_memory: bool,
    fail_storage: bool,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World::default());
}

fn world<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}

fn event(text: String) {
    world(|world| world.events.push(text));
}

fn events() -> Vec<String> {
    world(|world| std::mem::take(&mut world.events))
}

fn count(events: &[String], text: &str) -> usize {
    events.iter().filter(|event| event.as_str() == text).count()
}

fn gl_name() -> u32 {
    world(|world| {
        world.gl_names += 1;
        world.gl_names
    })
}

fn names(count: u32, names: *const u32) -> String {
    let names = unsafe { std::slice::from_raw_parts(names, count as usize) };
    let text: Vec<_> = names.iter().map(u32::to_string).collect();
    text.join(",")
}

unsafe extern "C" fn gl_current() -> Handle {
    world(|world| world.context) as Handle
}
unsafe extern "C" fn gl_load(_: *mut GlProcs) -> c_int {
    c_int::from(!world(|world| world.missing_extension))
}
unsafe extern "C" fn gl_error() -> u32 {
    world(|world| world.gl_errors.pop()).unwrap_or(GL_NO_ERROR)
}
unsafe extern "C" fn gl_luid(_: *const GlProcs, out: *mut u8) {
    unsafe { out.copy_from_nonoverlapping(LUID.as_ptr(), 8) };
}
unsafe extern "C" fn semaphore_import(_: *const GlProcs, handle: Handle, out: *mut u32) -> u32 {
    assert!(world(|world| world.handles.contains(&(handle as usize))));
    let name = gl_name();
    unsafe { out.write(name) };
    event(format!("import semaphore {name}"));
    if world(|world| world.fail_semaphore) {
        0x502
    } else {
        GL_NO_ERROR
    }
}
unsafe extern "C" fn semaphore_delete(_: *const GlProcs, name: u32) {
    event(format!("delete semaphore {name}"));
}
unsafe extern "C" fn memory_import(_: *const GlProcs, handle: Handle, out: *mut u32) -> u32 {
    assert!(world(|world| world.handles.contains(&(handle as usize))));
    let name = gl_name();
    unsafe { out.write(name) };
    if world(|world| world.fail_memory) {
        0x502
    } else {
        GL_NO_ERROR
    }
}
unsafe extern "C" fn memory_delete(_: *const GlProcs, name: u32) {
    event(format!("delete memory {name}"));
}
unsafe extern "C" fn texture_store(
    _: *const GlProcs,
    memory: u32,
    internal: u32,
    width: u32,
    height: u32,
    out: *mut u32,
) -> u32 {
    let name = gl_name();
    unsafe { out.write(name) };
    event(format!(
        "store {name} memory {memory} 0x{internal:x} {width}x{height}"
    ));
    if world(|world| world.fail_storage) {
        0x505
    } else {
        GL_NO_ERROR
    }
}
unsafe extern "C" fn texture_delete(name: u32) {
    event(format!("delete texture {name}"));
}
unsafe extern "C" fn gl_release(
    _: *const GlProcs,
    _: u32,
    value: u64,
    count: u32,
    textures: *const u32,
    layouts: *const u32,
) {
    let layouts = unsafe { std::slice::from_raw_parts(layouts, count as usize) };
    assert!(layouts.iter().all(|layout| *layout == LAYOUT_GENERAL));
    event(format!("release {value} [{}]", names(count, textures)));
    world(|world| world.signalled = world.signalled.max(value));
}
unsafe extern "C" fn gl_acquire(
    _: *const GlProcs,
    _: u32,
    value: u64,
    count: u32,
    textures: *const u32,
    _: *const u32,
) {
    event(format!("acquire {value} [{}]", names(count, textures)));
}
unsafe extern "C" fn finish() {
    event("finish".into());
}
unsafe extern "C" fn device_create(luid: *const u8, out: *mut Handle) -> i32 {
    assert_eq!(unsafe { std::slice::from_raw_parts(luid, 8) }, LUID);
    unsafe { out.write(com("device")) };
    OK
}
unsafe extern "C" fn queue_create(_: Handle, out: *mut Handle) -> i32 {
    unsafe { out.write(com("queue")) };
    OK
}
unsafe extern "C" fn device_luid(_: Handle, out: *mut u8) {
    let luid = world(|world| world.device_luid);
    unsafe { out.copy_from_nonoverlapping(luid.as_ptr(), 8) };
}
unsafe extern "C" fn queue_device(_: Handle, out: *mut Handle) -> i32 {
    let owner = world(|world| world.queue_owner) as Handle;
    unsafe {
        add_ref(owner.cast());
        out.write(owner);
    }
    OK
}
unsafe extern "C" fn fence_create(_: Handle, out: *mut Handle) -> i32 {
    unsafe { out.write(com("fence")) };
    OK
}
fn nt_handle() -> Handle {
    world(|world| {
        world.next += 1;
        let handle = 0x1_0000 + world.next;
        world.handles.insert(handle);
        handle as Handle
    })
}
unsafe extern "C" fn shared_handle(_: Handle, _: Handle, out: *mut Handle) -> i32 {
    unsafe { out.write(nt_handle()) };
    OK
}
unsafe extern "C" fn event_create() -> Handle {
    nt_handle()
}
unsafe extern "C" fn handle_close(handle: Handle) {
    assert!(world(|world| world.handles.remove(&(handle as usize))));
}
unsafe extern "C" fn slot_create(_: Handle, allocator: *mut Handle, list: *mut Handle) -> i32 {
    unsafe {
        allocator.write(com("allocator"));
        list.write(com("list"));
    }
    OK
}
unsafe extern "C" fn slot_reset(_: Handle, list: Handle) -> i32 {
    event(format!("reset {}", com_name(list)));
    OK
}
unsafe extern "C" fn list_close(list: Handle) -> i32 {
    event(format!("close {}", com_name(list)));
    if world(|world| std::mem::take(&mut world.fail_close)) {
        E_FAIL
    } else {
        OK
    }
}
unsafe extern "C" fn fence_completed(_: Handle) -> u64 {
    world(|world| world.completed)
}
unsafe extern "C" fn fence_wait(_: Handle, value: u64, _: Handle, timeout: u32) -> c_int {
    assert_eq!(timeout, CPU_WAIT_MS);
    event(format!("wait {value}"));
    world(|world| {
        if world.stall || value > world.signalled {
            return 0;
        }
        world.completed = world.completed.max(value);
        1
    })
}
unsafe extern "C" fn queue_wait(_: Handle, _: Handle, value: u64) -> i32 {
    event(format!("queue wait {value}"));
    if world(|world| std::mem::take(&mut world.fail_queue_wait)) {
        E_FAIL
    } else {
        OK
    }
}
unsafe extern "C" fn queue_execute(_: Handle, list: Handle) {
    event(format!("execute {}", com_name(list)));
}
unsafe extern "C" fn queue_signal(_: Handle, _: Handle, value: u64) -> i32 {
    world(|world| {
        if world.fail_signal > 0 {
            world.fail_signal -= 1;
            world.events.push(format!("signal {value} refused"));
            return E_FAIL;
        }
        world.events.push(format!("signal {value}"));
        world.signalled = world.signalled.max(value);
        OK
    })
}
unsafe extern "C" fn resource_create(
    _: Handle,
    _: u32,
    _: u32,
    format: u32,
    out: *mut Handle,
) -> i32 {
    unsafe { out.write(com(&format!("resource{format}_"))) };
    OK
}

static LEAVES: Leaves = Leaves {
    gl_current,
    gl_load,
    gl_error,
    gl_luid,
    semaphore_import,
    semaphore_delete,
    memory_import,
    memory_delete,
    texture_store,
    texture_delete,
    release: gl_release,
    acquire: gl_acquire,
    finish,
    device_create,
    queue_create,
    device_luid,
    queue_device,
    fence_create,
    shared_handle,
    event_create,
    handle_close,
    slot_create,
    slot_reset,
    list_close,
    fence_completed,
    fence_wait,
    queue_wait,
    queue_execute,
    queue_signal,
    resource_create,
};

unsafe extern "C" fn log(_: Handle, text: *const c_char) {
    let text = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    world(|world| world.logs.push(text));
}

fn setup() -> rsf_gl_setup {
    rsf_gl_setup {
        struct_size: size_of::<rsf_gl_setup>() as u32,
        abi_version: 1,
        log: Some(log),
        ..Default::default()
    }
}

fn reset() {
    world(|world| {
        *world = World::default();
        world.context = GL_CONTEXT as usize;
        world.device_luid = LUID;
    });
}

fn open_with(setup: &rsf_gl_setup) -> (i32, *mut Context) {
    let mut context = ptr::null_mut();
    let result = unsafe { rsf_core_gl_open(setup, &mut context, &LEAVES) };
    (result, context)
}

fn open_context() -> *mut Context {
    reset();
    let (result, context) = open_with(&setup());
    assert_eq!(result, OK);
    events();
    world(|world| world.logs.clear());
    context
}

fn texture(context: *mut Context, format: u32) -> *mut Texture {
    let mut texture = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_gl_texture_create(context, 64, 32, format, &mut texture) },
        OK
    );
    texture
}

fn info(texture: *mut Texture) -> rsf_gl_texture_info {
    let mut info = rsf_gl_texture_info {
        struct_size: size_of::<rsf_gl_texture_info>() as u32,
        ..Default::default()
    };
    assert_eq!(unsafe { rsf_gl_texture_get(texture, &mut info) }, OK);
    info
}

fn begin(context: *mut Context, textures: &[*mut Texture]) -> (i32, Handle, u32) {
    let (mut list, mut slot) = (ptr::null_mut(), u32::MAX);
    let result = unsafe {
        rsf_gl_begin(
            context,
            textures.len() as u32,
            textures.as_ptr(),
            &mut list,
            &mut slot,
        )
    };
    (result, list, slot)
}

fn submit(context: *mut Context, textures: &[*mut Texture]) -> i32 {
    unsafe { rsf_gl_submit(context, textures.len() as u32, textures.as_ptr()) }
}

fn transaction(context: *mut Context, textures: &[*mut Texture]) {
    assert_eq!(begin(context, textures).0, OK);
    assert_eq!(submit(context, textures), OK);
}

fn close_all(context: *mut Context, textures: &[*mut Texture]) {
    assert_eq!(unsafe { rsf_gl_drain(context) }, OK);
    for &texture in textures {
        unsafe { rsf_gl_texture_destroy(texture) };
    }
    assert_eq!(unsafe { rsf_gl_close(context) }, OK);
    world(|world| {
        assert!(world.com.is_empty(), "live COM objects: {:?}", world.com);
        assert!(
            world.handles.is_empty(),
            "open handles: {:?}",
            world.handles
        );
    });
}

// Fake allocation order on open: device1, queue2, fence3, fence handle 4, event 5, then allocator
// and list pairs 6-11, so the slots record on list7, list9 and list11. GL semaphore 1. Each
// texture then takes a resource and a handle, and a GL memory object and a texture name.

#[test]
fn normal_transactions_keep_the_native_order() {
    let context = open_context();
    let scene = texture(context, RSF_GL_RGBA16F);
    let depth = texture(context, RSF_GL_R32F);
    let motion = texture(context, RSF_GL_RG16F);
    let output = texture(context, RSF_GL_RGBA8);
    assert_eq!(
        events(),
        [
            "store 3 memory 2 0x881a 64x32",
            "store 5 memory 4 0x822e 64x32",
            "store 7 memory 6 0x822f 64x32",
            "store 9 memory 8 0x8058 64x32",
        ]
    );
    let formats: Vec<_> = [scene, depth, motion, output]
        .map(info)
        .iter()
        .map(|info| (info.texture, info.format, com_name(info.resource)))
        .collect();
    assert_eq!(
        formats,
        [
            (3, RSF_GL_RGBA16F, "resource10_12".into()),
            (5, RSF_GL_R32F, "resource41_14".into()),
            (7, RSF_GL_RG16F, "resource34_16".into()),
            (9, RSF_GL_RGBA8, "resource28_18".into()),
        ]
    );

    let (result, list, slot) = begin(context, &[scene, depth]);
    assert_eq!((result, com_name(list), slot), (OK, "list7".into(), 0));
    assert_eq!(submit(context, &[scene, depth]), OK);
    assert_eq!(unsafe { rsf_gl_drain(context) }, OK);
    assert_eq!(
        events(),
        [
            "reset list7",
            "release 1 [3,5]",
            "queue wait 1",
            "close list7",
            "execute list7",
            "signal 2",
            "acquire 2 [3,5]",
            "finish",
            "signal 3",
            "wait 3",
        ]
    );

    // Slots rotate. Slot 1 completes at 5, which no CPU wait has observed yet.
    for _ in 0..SLOTS {
        transaction(context, &[output]);
    }
    events();
    let (result, list, slot) = begin(context, &[output]);
    assert_eq!((result, com_name(list), slot), (OK, "list9".into(), 1));
    assert_eq!(
        events(),
        ["wait 5", "reset list9", "release 10 [9]", "queue wait 10"]
    );
    assert_eq!(submit(context, &[output]), OK);
    close_all(context, &[scene, depth, motion, output]);
}

#[test]
fn signal_refused_after_execute_retains_the_transaction() {
    let context = open_context();
    let scene = texture(context, RSF_GL_RGBA16F);
    let output = texture(context, RSF_GL_RGBA8);
    let scene_resource = info(scene).resource;
    let scene_resource_name = com_name(scene_resource);
    events();
    assert_eq!(begin(context, &[scene, output]).0, OK);
    events();
    world(|world| world.fail_signal = 3);
    assert_eq!(submit(context, &[scene, output]), FEATURE_FAILED);
    assert_eq!(
        events(),
        ["close list7", "execute list7", "signal 2 refused"],
        "GL must not reacquire before the queue signals"
    );

    // The executed list is neither closed nor submitted again. Drain and close keep refusing,
    // and the slot is never waited on or reset with an unknown completion.
    assert_eq!(submit(context, &[scene, output]), INVALID);
    assert_eq!(unsafe { rsf_gl_drain(context) }, FEATURE_FAILED);
    assert_eq!(unsafe { rsf_gl_close(context) }, FEATURE_FAILED);
    assert_eq!(events(), ["signal 3 refused", "signal 4 refused"]);

    // The caller may destroy a texture meanwhile: its GL names go, its D3D12 memory stays leased.
    unsafe { rsf_gl_texture_destroy(scene) };
    assert_eq!(events(), ["delete texture 3", "delete memory 2"]);
    assert_eq!(refs(scene_resource), 1, "the slot lease keeps the resource");

    // The next accepted signal follows the executed list. GL reacquires only what still exists,
    // and the lease ends once a CPU wait proves completion.
    assert_eq!(unsafe { rsf_gl_drain(context) }, OK);
    assert_eq!(
        events(),
        [
            "signal 5".to_string(),
            "acquire 5 [5]".into(),
            "finish".into(),
            "signal 6".into(),
            "wait 6".into(),
            format!("release {scene_resource_name}"),
        ]
    );

    // Recording resumes on the following slot; the failed slot is reset once, after completion.
    let (result, list, slot) = begin(context, &[output]);
    assert_eq!((result, com_name(list), slot), (OK, "list9".into(), 1));
    assert_eq!(submit(context, &[output]), OK);
    transaction(context, &[output]);
    transaction(context, &[output]);
    let sequence = events();
    assert_eq!(count(&sequence, "reset list7"), 1, "{sequence:?}");
    assert_eq!(count(&sequence, "execute list7"), 1, "{sequence:?}");
    close_all(context, &[output]);
}

#[test]
fn signal_refused_after_execute_settles_on_the_next_begin() {
    let context = open_context();
    let output = texture(context, RSF_GL_RGBA8);
    events();
    assert_eq!(begin(context, &[output]).0, OK);
    events();
    world(|world| world.fail_signal = 1);
    assert_eq!(submit(context, &[output]), FEATURE_FAILED);
    let (result, list, slot) = begin(context, &[output]);
    assert_eq!((result, com_name(list), slot), (OK, "list9".into(), 1));
    assert_eq!(
        events(),
        [
            "close list7",
            "execute list7",
            "signal 2 refused",
            "signal 3",
            "acquire 3 [3]",
            "reset list9",
            "release 4 [3]",
            "queue wait 4",
        ]
    );
    assert_eq!(
        world(|world| world.logs.last().cloned()),
        Some("OpenGL sharing: D3D12 completion signal failed (0x80004005)".into())
    );
    assert_eq!(submit(context, &[output]), OK);
    close_all(context, &[output]);
}

#[test]
fn refused_queue_wait_returns_the_textures_to_gl() {
    let context = open_context();
    let output = texture(context, RSF_GL_RGBA8);
    let resource = info(output).resource;
    events();
    world(|world| world.fail_queue_wait = true);
    let (result, list, _) = begin(context, &[output]);
    assert_eq!((result, list), (FEATURE_FAILED, ptr::null_mut()));
    assert_eq!(
        events(),
        [
            "reset list7",
            "release 1 [3]",
            "queue wait 1",
            "close list7",
            "acquire 1 [3]",
        ]
    );
    assert_eq!(refs(resource), 1, "a refused begin takes no lease");
    assert_eq!(submit(context, &[output]), INVALID);
    let (result, _, slot) = begin(context, &[output]);
    assert_eq!((result, slot), (OK, 0));
    assert_eq!(
        refs(resource),
        2,
        "the open transaction leases its resource"
    );
    assert_eq!(submit(context, &[output]), OK);
    close_all(context, &[output]);
}

#[test]
fn refused_close_signals_past_the_gl_wait_without_executing() {
    let context = open_context();
    let output = texture(context, RSF_GL_RGBA8);
    events();
    assert_eq!(begin(context, &[output]).0, OK);
    events();
    world(|world| world.fail_close = true);
    assert_eq!(submit(context, &[output]), FEATURE_FAILED);
    assert_eq!(events(), ["close list7", "signal 2", "acquire 2 [3]"]);
    let (result, _, slot) = begin(context, &[output]);
    assert_eq!((result, slot), (OK, 1));
    assert_eq!(submit(context, &[output]), OK);
    close_all(context, &[output]);
}

#[test]
fn a_slot_is_not_reused_before_its_completion() {
    let context = open_context();
    let output = texture(context, RSF_GL_RGBA8);
    let resource = info(output).resource;
    for _ in 0..SLOTS {
        transaction(context, &[output]);
    }
    events();
    assert_eq!(
        refs(resource),
        4,
        "three unretired slots lease the resource"
    );
    world(|world| world.stall = true);
    assert_eq!(begin(context, &[output]).0, FEATURE_FAILED);
    assert_eq!(events(), ["wait 2"]);
    assert_eq!(unsafe { rsf_gl_drain(context) }, FEATURE_FAILED);
    assert_eq!(events(), ["finish", "signal 7", "wait 7"]);
    assert_eq!(refs(resource), 4);
    world(|world| world.stall = false);
    assert_eq!(begin(context, &[output]).0, OK);
    assert_eq!(&events()[..2], ["wait 2", "reset list7"]);
    assert_eq!(refs(resource), 4, "the reused slot exchanged its lease");
    assert_eq!(submit(context, &[output]), OK);
    assert_eq!(unsafe { rsf_gl_drain(context) }, OK);
    assert_eq!(refs(resource), 1, "a drain proves every slot complete");
    close_all(context, &[output]);
}

#[test]
fn owner_admission_and_call_order() {
    let context = open_context();
    let output = texture(context, RSF_GL_RGBA8);
    // A second owner on the same thread and GL context.
    let (result, other) = open_with(&setup());
    assert_eq!(result, OK);
    let foreign = texture(other, RSF_GL_RGBA8);
    let (mut device, mut queue) = (ptr::null_mut(), ptr::null_mut());
    assert_eq!(
        unsafe { rsf_gl_graphics(context, &mut device, &mut queue) },
        OK
    );
    assert_eq!(
        (com_name(device), com_name(queue)),
        ("device1".into(), "queue2".into())
    );
    events();

    world(|world| world.context = 0x99);
    assert_eq!(begin(context, &[output]).0, INVALID);
    assert_eq!(unsafe { rsf_gl_drain(context) }, INVALID);
    assert_eq!(unsafe { rsf_gl_close(context) }, INVALID);
    assert_eq!(
        unsafe { rsf_gl_graphics(context, &mut device, &mut queue) },
        INVALID
    );
    let mut created = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_gl_texture_create(context, 4, 4, RSF_GL_RGBA8, &mut created) },
        INVALID
    );
    unsafe { rsf_gl_texture_destroy(output) };
    assert!(events().is_empty(), "another WGL context does nothing");
    world(|world| world.context = GL_CONTEXT as usize);

    let mut list = ptr::null_mut();
    assert_eq!(begin(context, &[foreign]).0, INVALID);
    assert_eq!(begin(context, &[output; BATCH + 1]).0, INVALID);
    assert_eq!(
        unsafe { rsf_gl_begin(context, 1, ptr::null(), &mut list, &mut 0) },
        INVALID
    );
    assert_eq!(submit(context, &[output]), INVALID);
    assert_eq!(begin(context, &[]).0, OK);
    assert_eq!(begin(context, &[output]).0, INVALID);
    assert_eq!(unsafe { rsf_gl_drain(context) }, INVALID);
    assert_eq!(submit(context, &[foreign]), INVALID);
    assert_eq!(submit(context, &[]), OK);
    assert_eq!(
        events(),
        [
            "reset list7",
            "release 1 []",
            "queue wait 1",
            "close list7",
            "execute list7",
            "signal 2",
            "acquire 2 []",
        ]
    );

    // A sticky error from the game's own GL work is logged and not attributed to the import.
    world(|world| {
        world.logs.clear();
        world.gl_errors = vec![0x500, 0x502];
    });
    assert_eq!(
        unsafe { rsf_gl_texture_create(context, 4, 4, 99, &mut created) },
        INVALID
    );
    assert_eq!(
        unsafe { rsf_gl_texture_create(context, 0, 4, RSF_GL_RGBA8, &mut created) },
        INVALID
    );
    assert_eq!(
        world(|world| std::mem::take(&mut world.logs)),
        [
            "OpenGL sharing: pre-existing GL operation before interop failed (0x00000502)",
            "OpenGL sharing: pre-existing GL operation before interop failed (0x00000500)",
        ]
    );
    unsafe { rsf_gl_texture_destroy(foreign) };
    assert_eq!(unsafe { rsf_gl_close(other) }, OK);
    close_all(context, &[output]);
}

#[test]
fn open_validates_setup_and_releases_partial_state() {
    reset();
    let mut short = setup();
    short.struct_size -= 1;
    assert_eq!(open_with(&short), (INVALID, ptr::null_mut()));
    let mut version = setup();
    version.abi_version = 2;
    assert_eq!(open_with(&version).0, ABI_MISMATCH);
    let mut half = setup();
    half.device = com("device");
    assert_eq!(open_with(&half).0, INVALID);
    unsafe { release(half.device.cast()) };
    world(|world| world.context = 0);
    assert_eq!(open_with(&setup()).0, INVALID);
    assert_eq!(
        unsafe { rsf_core_gl_open(&setup(), ptr::null_mut(), &LEAVES) },
        INVALID
    );

    reset();
    world(|world| world.missing_extension = true);
    assert_eq!(open_with(&setup()).0, NOT_SUPPORTED);
    assert_eq!(
        world(|world| world.logs.clone()),
        ["OpenGL sharing requires EXT_memory_object_win32 and EXT_semaphore_win32."]
    );

    reset();
    world(|world| world.fail_semaphore = true);
    assert_eq!(open_with(&setup()).0, FEATURE_FAILED);
    assert_eq!(&events()[..2], ["import semaphore 1", "delete semaphore 1"]);
    world(|world| {
        assert!(world.com.is_empty(), "{:?}", world.com);
        assert!(world.handles.is_empty());
    });

    // A supplied pair must share the context's adapter; a refusal drops only added references.
    reset();
    let (device, queue) = (com("device"), com("queue"));
    world(|world| {
        world.queue_owner = device as usize;
        world.device_luid = [9; 8];
    });
    let mut supplied = setup();
    supplied.device = device;
    supplied.queue = queue;
    assert_eq!(open_with(&supplied).0, INVALID);
    assert_eq!((refs(device), refs(queue)), (1, 1));
    world(|world| world.device_luid = LUID);
    let (result, context) = open_with(&supplied);
    assert_eq!(result, OK);
    assert_eq!((refs(device), refs(queue)), (2, 2));
    assert_eq!(unsafe { rsf_gl_close(context) }, OK);
    assert_eq!((refs(device), refs(queue)), (1, 1));
    unsafe {
        release(device.cast());
        release(queue.cast());
    }
}

#[test]
fn texture_import_failures_release_what_they_created() {
    let context = open_context();
    world(|world| world.fail_memory = true);
    let mut created = ptr::null_mut();
    assert_eq!(
        unsafe { rsf_gl_texture_create(context, 8, 8, RSF_GL_R32F, &mut created) },
        FEATURE_FAILED
    );
    assert!(created.is_null());
    assert_eq!(events(), ["delete memory 2", "release resource41_12"]);
    world(|world| {
        world.fail_memory = false;
        world.fail_storage = true;
    });
    assert_eq!(
        unsafe { rsf_gl_texture_create(context, 8, 8, RSF_GL_R32F, &mut created) },
        FEATURE_FAILED
    );
    assert_eq!(
        events(),
        [
            "store 4 memory 3 0x822e 8x8",
            "delete texture 4",
            "delete memory 3",
            "release resource41_14",
        ]
    );
    assert_eq!(
        world(|world| std::mem::take(&mut world.logs)),
        [
            "OpenGL sharing: D3D12 memory import failed (0x00000502)",
            "OpenGL sharing: shared texture storage failed (0x00000505)",
        ]
    );
    world(|world| world.fail_storage = false);
    let output = texture(context, RSF_GL_RGBA8);
    close_all(context, &[output]);
}
