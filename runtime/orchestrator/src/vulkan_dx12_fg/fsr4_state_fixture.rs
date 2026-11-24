// SPDX-License-Identifier: MIT
//! CPU doubles for the FSR4 FG WSI owner: the bridge, importer and shared graphics leaves that
//! vulkan_fsr4_fg calls in the fixture-leaves archive. Called by the native_fsr4_fg_state_contract
//! ctest.
use super::*;
use crate::fsr4_fg_bridge::state_fixture::rsf_fixture_fsr4fg_bridge_state;
use crate::sys::{
    RSF_SRVK_ABI_VERSION, rsf_fsr4fg_create as BridgeCreate, rsf_fsr4fg_frame as BridgeFrame,
    rsf_fsr4fg_info as BridgeInfo, rsf_fsr4fg_present_status as BridgeStatus, rsf_vk_function,
};
use crate::vulkan_fsr4_fg::{
    Fsr4, Options, create as fsr4_create, rsf_fsr4gvk_acquire, rsf_fsr4gvk_destroy,
    rsf_fsr4gvk_drain, rsf_fsr4gvk_enable_options, rsf_fsr4gvk_hudless, rsf_fsr4gvk_prepare,
    rsf_fsr4gvk_present_source, rsf_fsr4gvk_status, rsf_fsr4gvk_submitted,
};
use ash::vk::Handle as _;
use rsf_game_sys::rsf_frame_record;
use std::{
    cell::{Cell, RefCell},
    ffi::CStr,
};

const TIMELINE: u64 = 40;
const ADAPTER: u64 = 41;
const INJECTED: i32 = -93;
const ALL: u32 = vk::PipelineStageFlags::ALL_COMMANDS.as_raw();
const NOT_READY: i32 = vk::Result::NOT_READY.as_raw();

type Gpu = Handle;
type Bridge = Handle;
type Fsr4Owner = *mut Owner<Fsr4>;

struct Capture {
    commands: Handle,
    frame: u64,
    group: u64,
    image: u64,
    role: Vec<u8>,
}

#[derive(Default)]
struct Calls {
    next_handle: u64,
    provider_luid: u64,
    last_done: u64,
    presented: u64,
    depth: Handle,
    starts: u32,
    images_destroyed: u32,
    pools_destroyed: u32,
    releases: u32,
    idles: u32,
    cleared: u32,
    enables: u32,
    pending_initialized: u32,
    guide_releases: u32,
    fail_marker: bool,
    fail_start: bool,
    fail_importer_destroy: bool,
    waits: Vec<u64>,
    markers: Vec<u64>,
    marked_complete: Vec<u64>,
    discarded: Vec<u64>,
    initialized_copies: Vec<u32>,
    captures: Vec<Capture>,
}

thread_local! {
    static CALLS: RefCell<Calls> = RefCell::new(Calls::default());
    static FAILURES: Cell<i32> = const { Cell::new(0) };
    static SWAPCHAIN: vk::SwapchainCreateInfoKHR<'static> = vk::SwapchainCreateInfoKHR::default()
        .image_extent(vk::Extent2D { width: 640, height: 360 })
        .image_format(vk::Format::R8G8B8A8_UNORM)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
        .image_array_layers(1)
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .min_image_count(3)
        .present_mode(vk::PresentModeKHR::FIFO);
}

macro_rules! check {
    ($test:expr) => {
        if !$test {
            eprintln!("line {}: {}", line!(), stringify!($test));
            FAILURES.set(FAILURES.get() + 1);
        }
    };
}

// Define a typed C ABI fixture callback in place, using the table field to check its signature.
macro_rules! callback {
    (($($arg:tt : $ty:ty),*) $(-> $result:ty)? $body:block) => {{
        unsafe extern "C" fn call($($arg: $ty),*) $(-> $result)? $body
        call
    }};
}

fn calls<R>(f: impl FnOnce(&mut Calls) -> R) -> R {
    CALLS.with(|state| f(&mut state.borrow_mut()))
}

fn reset() {
    calls(|state| {
        *state = Calls {
            next_handle: 100,
            provider_luid: ADAPTER,
            ..Calls::default()
        }
    });
}

fn next() -> u64 {
    calls(|state| {
        let handle = state.next_handle;
        state.next_handle += 1;
        handle
    })
}

fn fake<T>(id: u64) -> *mut T {
    ptr::without_provenance_mut(id as usize)
}

const IMAGES: ImageOps = ImageOps {
    local_memory_types: Some(callback!((_: Handle) -> u32 { 1 })),
    create_image: Some(
        callback!((_: Handle, _: u32, _: u32, _: u32, usage: u32, out: *mut u64) -> i32 {
            let expected = vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED;
            check!(vk::ImageUsageFlags::from_raw(usage).contains(expected));
            unsafe { out.write(next()) };
            0
        }),
    ),
    requirements: Some(
        callback!((_: Handle, _: u64, bytes: *mut u64, types: *mut u32) {
            unsafe { bytes.write(1); types.write(1); }
        }),
    ),
    allocate: Some(
        callback!((_: Handle, _: u64, _: u32, out: *mut u64) -> i32 {
            unsafe { out.write(next()) };
            0
        }),
    ),
    bind: Some(callback!((_: Handle, image: u64, memory: u64) -> i32 {
        check!(image != 0 && memory != 0);
        0
    })),
    create_view: None,
    destroy_view: None,
    destroy_image: Some(callback!((_: Handle, image: u64) {
        check!(image != 0);
        calls(|state| state.images_destroyed += 1);
    })),
    free_memory: Some(callback!((_: Handle, memory: u64) { check!(memory != 0); })),
    create_fence: None,
    destroy_fence: None,
    wait_fence: None,
    reset_fence: None,
    submit_fence: None,
};

pub(crate) unsafe extern "C" fn probe(_: u64, _: *const u16, _: *mut BridgeInfo) -> i32 {
    0
}

pub(crate) unsafe extern "C" fn bridge_create(
    create: *const BridgeCreate,
    out: *mut Bridge,
) -> i32 {
    let create = unsafe { &*create };
    check!(create.adapter_luid == ADAPTER && create.generated_count == 1);
    unsafe { out.write(fake(1)) };
    0
}

pub(crate) unsafe extern "C" fn bridge_info(_: Bridge, info: *mut BridgeInfo) -> i32 {
    let info = unsafe { &mut *info };
    info.adapter_luid = calls(|state| state.provider_luid);
    info.provider_version = 700;
    info.max_generated = 1;
    0
}

pub(crate) unsafe extern "C" fn bridge_start(_: Bridge, _: Handle) -> i32 {
    calls(|state| {
        state.starts += 1;
        if state.fail_start { INJECTED } else { 0 }
    })
}

pub(crate) unsafe extern "C" fn bridge_drain(_: Bridge) -> i32 {
    0
}

pub(crate) unsafe extern "C" fn bridge_prepare_destroy(_: Bridge) -> i32 {
    0
}

pub(crate) unsafe extern "C" fn bridge_destroy(_: Bridge) -> i32 {
    calls(|state| state.releases += 1);
    0
}

pub(crate) unsafe extern "C" fn interop_create(
    _: *const Graphics,
    _: Bridge,
    luid: u64,
    enabled: u32,
    out: *mut Handle,
) -> i32 {
    check!(luid == ADAPTER && enabled == 1);
    unsafe { out.write(fake(2)) };
    0
}

pub(crate) fn importer() -> &'static InteropOps {
    &IMPORTER
}

pub(crate) fn graphics() -> &'static GraphicsOps {
    &GRAPHICS
}

const IMPORTER: InteropOps = InteropOps {
    image: callback!((_: Handle, format: u32, width: u32, height: u32, out: *mut SharedImage) -> i32 {
        let id = next();
        unsafe { out.write(SharedImage { image: id, dx12_resource: fake(id), format, width, height }) };
        0
    }),
    timeline: callback!((_: Handle) -> u64 { TIMELINE }),
    destroy: callback!((_: Handle, drained: u32) -> i32 {
        check!(drained == 1);
        if calls(|state| state.fail_importer_destroy) { INJECTED } else { 0 }
    }),
    release_guides: callback!((_: Handle, depth: u64, motion: u64) -> i32 {
        check!(depth != 0 || motion != 0);
        calls(|state| {
            check!(state.waits.len() == 2 && state.marked_complete.len() == 2);
            state.guide_releases += 1;
        });
        0
    }),
};

const GRAPHICS: GraphicsOps = GraphicsOps {
    adapter_luid: callback!((_: *const Graphics, out: *mut u64) -> i32 {
        unsafe { out.write(ADAPTER) };
        0
    }),
    platform_allocate: callback!((setup: *const Setup) -> Gpu {
        SWAPCHAIN.with(|info| check!(!setup.is_null()
            && unsafe { (*setup).create_info } == ptr::from_ref(info).cast()));
        fake(3)
    }),
    platform_destroy: callback!((_: Gpu) {}),
    platform_resolve: callback!((_: Gpu, graphics: *const Graphics) -> i32 {
        let graphics = unsafe { &*graphics };
        check!(graphics.device == fake(5) && graphics.instance_proc.is_some() && graphics.device_proc.is_some());
        0
    }),
    format_features: callback!((_: Gpu, format: u32) -> u32 {
        let format = vk::Format::from_raw(format as i32);
        check!(format == vk::Format::R8G8B8A8_UNORM || format == vk::Format::B8G8R8A8_UNORM);
        (vk::FormatFeatureFlags::BLIT_SRC | vk::FormatFeatureFlags::BLIT_DST).as_raw()
    }),
    initialize_capture: callback!((_: Gpu, graphics: *const Graphics) -> i32 {
        check!(!graphics.is_null() && unsafe { (*graphics).device } == fake(5));
        0
    }),
    create_pool: callback!((_: Gpu, out: *mut u64) -> i32 { unsafe { out.write(next()) }; 0 }),
    destroy_pool: callback!((_: Gpu, _: u64) { calls(|state| state.pools_destroyed += 1); }),
    images: &IMAGES,
    allocate_commands: callback!((_: Gpu, _: u64, out: *mut Handle) -> i32 {
        unsafe { out.write(fake(next())) };
        0
    }),
    begin: callback!((_: Gpu, _: Handle) -> i32 { 0 }),
    end: callback!((_: Gpu, _: Handle) -> i32 { 0 }),
    barrier: callback!((_: Gpu, list: Handle, plan: *const Barrier) {
        let plan = unsafe { &*plan };
        check!(!list.is_null() && plan.image != 0);
        check!(plan.source_stages == ALL && plan.target_stages == ALL);
        if plan.new_layout == vk::ImageLayout::TRANSFER_DST_OPTIMAL.as_raw() as u32 {
            calls(|state| state.pending_initialized = u32::from(plan.old_layout == vk::ImageLayout::GENERAL.as_raw() as u32));
        }
    }),
    copy: transfer,
    blit: transfer,
    clear: callback!((_: Gpu, list: Handle, image: u64) {
        check!(!list.is_null() && image != 0);
        calls(|state| state.cleared += 1);
    }),
    acquire_submit: callback!((_: Gpu, _: u64, _: u64) -> i32 { 0 }),
    present_submit,
    wait_timeline: callback!((_: Gpu, timeline: u64, value: u64, timeout: u64) -> i32 {
        check!(timeline == TIMELINE && timeout != 0 && value <= calls(|state| state.last_done));
        0
    }),
    create_fence: callback!((_: Gpu, out: *mut u64) -> i32 { unsafe { out.write(next()) }; 0 }),
    mark_submit: callback!((_: Gpu, fence: u64) -> i32 {
        calls(|state| {
            state.markers.push(fence);
            if state.fail_marker { INJECTED } else { 0 }
        })
    }),
    wait_fence: callback!((_: Gpu, fence: u64, _: u64) -> i32 {
        calls(|state| state.waits.push(fence));
        0
    }),
    destroy_fence: callback!((_: Gpu, _: u64) {}),
    idle: callback!((_: Gpu) -> i32 { calls(|state| state.idles += 1); 0 }),
    capture_provider: callback!((_: Gpu, provider: u64, backend: u32) {
        check!(provider == 700 && backend == 4);
    }),
    capture_record,
    capture_complete: callback!((_: Gpu, group: u64) { calls(|state| state.marked_complete.push(group)); }),
    capture_discard: callback!((_: Gpu, group: u64) { calls(|state| state.discarded.push(group)); }),
    capture_flush: callback!((_: Gpu) -> i32 { 0 }),
    describe,
};

pub(crate) unsafe extern "C" fn bridge_present(
    _: Bridge,
    frame: *const BridgeFrame,
    status: *mut BridgeStatus,
) -> i32 {
    let frame = unsafe { &*frame };
    calls(|state| {
        check!(frame.done_value == state.last_done + 2 && frame.ready_value == state.last_done + 1);
        check!(!frame.color.is_null());
        state.last_done = frame.done_value;
        state.presented = if frame.record.is_null() {
            0
        } else {
            unsafe { (&raw const (*frame.record).frame_id).read() }
        };
        state.enables += frame.enabled;
        state.depth = frame.depth;
    });
    // The FIFO swapchain maps to vsync in the Rust admission.
    check!(frame.vsync == 1);
    if frame.enabled != 0 {
        check!(
            !frame.record.is_null()
                && frame.motion_scale_x == 320.0
                && frame.motion_scale_y == 180.0
        );
        check!(
            !frame.depth.is_null()
                && !frame.motion.is_null()
                && !frame.hudless.is_null()
                && frame.hudless != frame.color
        );
    } else {
        check!(frame.hudless == frame.color && frame.motion_scale_x == 1.0);
    }
    let status = unsafe { &mut *status };
    status.frames_presented = 1;
    status.fg_enabled = frame.enabled;
    status.generated_callbacks = 0;
    0
}

unsafe extern "C" fn transfer(_: Gpu, list: Handle, plan: *const Transfer) {
    let plan = unsafe { &*plan };
    check!(!list.is_null() && plan.source != 0 && plan.target != 0);
    calls(|state| state.initialized_copies.push(state.pending_initialized));
}

unsafe extern "C" fn present_submit(_: Gpu, list: Handle, plan: &PresentSubmit) -> i32 {
    let last = calls(|state| state.last_done);
    check!(!list.is_null() && plan.signal == TIMELINE && plan.ready == last + 1);
    check!(plan.waits[0] == 88 && plan.values[0] == 0 && plan.stages[0] == ALL);
    if last != 0 {
        check!(plan.wait_count == 2 && plan.waits[1] == TIMELINE);
        check!(plan.values[1] == last && plan.stages[1] == ALL);
    } else {
        check!(plan.wait_count == 1);
    }
    0
}

unsafe extern "C" fn capture_record(
    _: Gpu,
    commands: Handle,
    image: *const Image,
    source: *const Record,
    role: *const c_char,
    group: u64,
) {
    check!(group != 0 && !image.is_null() && !source.is_null() && !role.is_null());
    if !image.is_null() && !source.is_null() && !role.is_null() {
        calls(|state| {
            state.captures.push(Capture {
                commands,
                group,
                frame: unsafe { (*source).frame_id },
                image: unsafe { (*image).image },
                role: unsafe { CStr::from_ptr(role) }.to_bytes().to_vec(),
            })
        });
    }
}

// Supply native swapchain facts to the admission owner using the real Vulkan layout.
unsafe extern "C" fn describe(info: *const c_void, out: *mut Swapchain) {
    SWAPCHAIN.with(|expected| check!(info == ptr::from_ref(expected).cast()));
    let info = unsafe { &*info.cast::<vk::SwapchainCreateInfoKHR<'_>>() };
    check!(info.p_next.is_null());
    unsafe {
        out.write(Swapchain {
            width: info.image_extent.width,
            height: info.image_extent.height,
            format: info.image_format.as_raw() as u32,
            usage: info.image_usage.as_raw(),
            color_space: info.image_color_space.as_raw() as u32,
            layers: info.image_array_layers,
            sharing_mode: info.image_sharing_mode.as_raw() as u32,
            min_images: info.min_image_count,
            flags: info.flags.as_raw(),
            present_mode: info.present_mode.as_raw() as u32,
            extension_count: 0,
            extension_type: 0,
            old_chain: info.old_swapchain.as_raw(),
        })
    };
}

fn create() -> (i32, Fsr4Owner, u64) {
    unsafe extern "C" fn lookup(_: Handle, _: *const c_char) -> rsf_vk_function {
        None
    }
    unsafe extern "C" fn submit(_: Handle, _: u32, _: *const c_void, _: u64) -> i32 {
        0
    }
    unsafe extern "C" fn lock(_: Handle) {}
    let options = Options {
        struct_size: size_of::<Options>() as u32,
        abi_version: 1,
        window: fake(7),
        timeline_enabled: 1,
        generated_count: 1,
    };
    let mut graphics = Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        graphics: Graphics {
            struct_size: size_of::<Graphics>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: fake(5),
            physical_device: fake(5),
            device: fake(5),
            instance_proc: Some(lookup),
            device_proc: Some(lookup),
            runtime_directory_utf8: c"fixture".as_ptr(),
            ..Default::default()
        },
        graphics_submit: Some(submit),
        lock_queues: Some(lock),
        unlock_queues: Some(lock),
        ..Default::default()
    };
    graphics.queues[0] = fake(6);
    let mut owner = ptr::null_mut();
    let mut chain = 0;
    let code = SWAPCHAIN.with(|info| {
        graphics.create_info = ptr::from_ref(info).cast();
        unsafe { fsr4_create(&graphics, &options, &mut owner, &mut chain) }
    });
    (code, owner, chain)
}

fn source(frame: u64) -> rsf_frame_record {
    let mut record = rsf_frame_record {
        resource_generation: 13,
        ..rsf_test_support::frame_record(11, frame, 12, [320, 180], [640, 360])
    };
    record.camera.struct_size = size_of_val(&record.camera) as u32;
    record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    record
}

fn inputs(record: &rsf_frame_record) -> Frame {
    let srv = |image, format: vk::Format, width, height| Image {
        device: fake(5),
        image,
        format: format.as_raw() as u32,
        width,
        height,
        layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32,
        copy_source: 1,
    };
    Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        record,
        depth: srv(
            21,
            vk::Format::R32_SFLOAT,
            record.render_width,
            record.render_height,
        ),
        motion: srv(
            22,
            vk::Format::R16G16_SFLOAT,
            record.render_width,
            record.render_height,
        ),
        hudless: srv(23, vk::Format::R8G8B8A8_UNORM, 640, 360),
        motion_scale_x: 320.0,
        motion_scale_y: 180.0,
    }
}

fn status() -> Status {
    Status {
        struct_size: size_of::<Status>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        ..Default::default()
    }
}

fn present(owner: Fsr4Owner, chain: u64, source: *const Record, expected: u64, enabled: u32) {
    let mut index = 0;
    check!(unsafe { rsf_fsr4gvk_acquire(owner, 100, 1, 0, &mut index) } == 0);
    let mut results = [vk::Result::INCOMPLETE];
    let waits = [vk::Semaphore::from_raw(88)];
    let chains = [vk::SwapchainKHR::from_raw(chain)];
    let indices = [index];
    // Exercise the same Vulkan descriptor reader used by both providers.
    let info = vk::PresentInfoKHR::default()
        .wait_semaphores(&waits)
        .swapchains(&chains)
        .image_indices(&indices)
        .results(&mut results);
    let code = unsafe { rsf_fsr4gvk_present_source(owner, ptr::from_ref(&info).cast(), source) };
    check!(code == 0 && results[0] == vk::Result::SUCCESS);
    check!(calls(|state| state.presented) == expected);
    let mut status = status();
    check!(unsafe { rsf_fsr4gvk_status(owner, &mut status) } == 0 && status.enabled == enabled);
    check!(status.source_frame == if enabled != 0 { expected } else { 0 });
    check!(status.last_present_count == 1 && status.generated_callbacks == 0);
}

#[repr(C, align(8))]
struct IdentityPrefix {
    size: u32,
    abi: u32,
    frame: u64,
    session: u64,
    view: u32,
    generation: u32,
}

const _: () = {
    assert!(align_of::<IdentityPrefix>() == align_of::<rsf_frame_record>());
    assert!(size_of::<IdentityPrefix>() == std::mem::offset_of!(rsf_frame_record, phase));
};

fn prove_identity_and_completion() {
    reset();
    let (code, owner, chain) = create();
    check!(code == 0 && !owner.is_null() && chain != 0);
    if owner.is_null() {
        return;
    }
    check!(unsafe { rsf_fsr4gvk_enable_options(owner, 1, 1) } == 0);
    let first = source(1);
    let invalid = rsf_frame_record {
        resource_generation: first.resource_generation + 1,
        ..first
    };
    let guides = fake(90);
    let hudless = fake(91);
    let abandoned = fake(92);
    let initial = inputs(&first);
    let mismatch = inputs(&invalid);
    check!(unsafe { rsf_fsr4gvk_prepare(owner, guides, &initial) } == 0);
    check!(unsafe { rsf_fsr4gvk_hudless(owner, hudless, &mismatch) } == INVALID);
    check!(unsafe { rsf_fsr4gvk_hudless(owner, hudless, &initial) } == 0);
    let groups = calls(|state| {
        check!(state.captures.len() == 3);
        if let [d, m, h] = state.captures.as_slice() {
            check!(
                d.commands == guides
                    && d.frame == 1
                    && d.image == 21
                    && d.role.as_slice() == b"fg-depth"
            );
            check!(
                m.commands == guides
                    && m.frame == 1
                    && m.image == 22
                    && m.role.as_slice() == b"fg-motion-uv"
            );
            check!(
                h.commands == hudless
                    && h.frame == 1
                    && h.image == 23
                    && h.role.as_slice() == b"fg-hudless-color"
            );
            check!(d.group == m.group && d.group != h.group);
            [d.group, h.group]
        } else {
            [0, 0]
        }
    });
    unsafe { rsf_fsr4gvk_submitted(owner, &guides, 1) };
    check!(unsafe { rsf_fsr4gvk_drain(owner) } == NOT_READY && calls(|state| state.idles) == 0);
    present(owner, chain, &invalid, 0, 0);
    let off_depth = calls(|state| {
        check!(state.marked_complete.is_empty() && state.markers.len() == 1);
        state.depth
    });
    unsafe {
        rsf_fsr4gvk_submitted(owner, &hudless, 1);
        rsf_fsr4gvk_submitted(owner, &hudless, 1);
    }
    calls(|state| {
        check!(
            matches!(state.markers.as_slice(), [first, second] if first != second)
                && state.waits.is_empty()
                && state.marked_complete.is_empty()
        );
    });

    // A newer recording does not supply resources for an exact older Present.
    let old = source(2);
    let newer = source(3);
    check!(unsafe { rsf_fsr4gvk_prepare(owner, abandoned, &inputs(&old)) } == 0);
    check!(unsafe { rsf_fsr4gvk_prepare(owner, abandoned, &inputs(&newer)) } == 0);
    unsafe { rsf_fsr4gvk_submitted(owner, &abandoned, 1) };
    calls(|state| check!(state.discarded.len() == 1 && state.markers.len() == 3));
    let prefix = IdentityPrefix {
        size: size_of::<IdentityPrefix>() as u32,
        abi: RSF_GAME_FRAME_ABI_VERSION,
        frame: first.frame_id,
        session: first.session_id,
        view: first.view_id,
        generation: first.resource_generation,
    };
    let short_identity = ptr::from_ref(&prefix).cast();
    present(owner, chain, short_identity, 1, 1);
    calls(|state| check!(state.enables == 1 && state.depth != off_depth && state.cleared == 2));
    present(owner, chain, short_identity, 1, 0);
    calls(|state| check!(state.depth == off_depth && state.marked_complete.is_empty()));

    let reused = rsf_frame_record {
        render_width: 288,
        render_height: 162,
        ..source(7)
    };
    check!(unsafe { rsf_fsr4gvk_prepare(owner, guides, &inputs(&reused)) } == 0);
    calls(|state| {
        check!(state.waits.len() == 2 && state.marked_complete == groups);
        check!(state.guide_releases == 1);
        check!(state.initialized_copies.ends_with(&[0, 0]));
    });

    // A source with only the submitted guide stage may present its real frame.
    unsafe { rsf_fsr4gvk_submitted(owner, &guides, 1) };
    present(owner, chain, &reused, 7, 0);
    check!(
        unsafe { rsf_fsr4gvk_drain(owner) } == 0 && calls(|state| state.marked_complete.len()) == 4
    );
    check!(unsafe { rsf_fsr4gvk_destroy(owner) } == 0 && calls(|state| state.releases) == 1);
}

fn prove_admission_and_retirement() {
    reset();
    calls(|state| state.provider_luid += 1);
    let (code, owner, chain) = create();
    check!(
        code == vk::Result::ERROR_INVALID_EXTERNAL_HANDLE.as_raw() && owner.is_null() && chain == 0
    );
    calls(|state| check!(state.starts == 0 && state.releases == 1));

    reset();
    calls(|state| {
        state.fail_start = true;
        state.fail_importer_destroy = true;
    });
    let (code, owner, chain) = create();
    check!(code == INJECTED && !owner.is_null() && chain == 0);
    calls(|state| {
        check!(state.pools_destroyed == 1 && state.images_destroyed == 3 && state.releases == 0);
        state.fail_importer_destroy = false;
    });
    check!(unsafe { rsf_fsr4gvk_destroy(owner) } == 0);
    calls(|state| {
        check!(state.pools_destroyed == 1 && state.images_destroyed == 3 && state.releases == 1)
    });

    reset();
    let (code, owner, _) = create();
    check!(code == 0 && !owner.is_null());
    if owner.is_null() {
        return;
    }
    let record = source(21);
    let list = fake(90);
    check!(unsafe { rsf_fsr4gvk_prepare(owner, list, &inputs(&record)) } == 0);
    calls(|state| state.fail_marker = true);
    unsafe { rsf_fsr4gvk_submitted(owner, &list, 1) };
    let mut status = status();
    check!(unsafe { rsf_fsr4gvk_status(owner, &mut status) } == 0 && status.result == INJECTED);
    check!(
        unsafe { rsf_fsr4gvk_drain(owner) } == NOT_READY
            && unsafe { rsf_fsr4gvk_destroy(owner) } == NOT_READY
    );
    // The terminal synthetic owner is retained through process exit.
    calls(|state| {
        check!(
            state.releases == 0 && state.images_destroyed == 0 && state.marked_complete.is_empty()
        )
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_fsr4fg_wsi_state() -> i32 {
    FAILURES.set(rsf_fixture_fsr4fg_bridge_state());
    prove_identity_and_completion();
    prove_admission_and_retirement();
    if FAILURES.get() == 0 {
        println!(
            "FSR4 Vulkan source identity, submission, timeline and retirement passed (CPU doubles)."
        );
    }
    i32::from(FAILURES.get() != 0)
}
