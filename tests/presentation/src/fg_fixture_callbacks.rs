// SPDX-License-Identifier: MIT
//! Real provider preparation, latency and retirement receipts for the shared FG fixture.
use crate::fg_bridge::*;
use crate::fixture_device_abi::rsf_fg_fixture_inputs;
use crate::present_fixture_loops_abi::*;
use crate::present_fixture_time::stamp;
use rsf_game_sys::{rsf_frame_record, rsf_game_render_pass};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

type Raw = *mut c_void;

const PHASE_UI_COMPLETE: u32 = 5;
const RENDER_SUBMIT_END: u32 = 3;
const PRESENT_START: u32 = 4;
const PRESENT_END: u32 = 5;

// Only the application/presentation thread enters these callbacks. The query worker
// only reads DXGI state and the synchronous window callback does not use this state.
pub(super) struct State {
    pub(super) frame: Cell<u64>,
    pub(super) accepted: Cell<bool>,
    pub(super) samples: Cell<u64>,
    pub(super) captured_frame: Cell<u64>,
    pub(super) readback: Cell<Raw>,
    service: bool,
    hot: bool,
    flip: bool,
    mfg: bool,
    inputs: RefCell<Option<Inputs>>,
}

struct Inputs {
    depth: ID3D12Resource,
    motion: ID3D12Resource,
    cpu_heap: ID3D12DescriptorHeap,
    gpu_heap: ID3D12DescriptorHeap,
    color_heap: Option<ID3D12DescriptorHeap>,
    stride: u32,
}

impl Drop for State {
    fn drop(&mut self) {
        let readback = self.readback.get();
        if !readback.is_null() {
            drop(unsafe { ID3D12Resource::from_raw(readback) });
        }
    }
}

impl State {
    fn check(&self, accepted: bool) {
        self.accepted.set(self.accepted.get() && accepted);
    }
    fn source(&self) -> u64 {
        rsf_fg_bridge_fixture_source(self.frame.get(), u32::from(self.hot))
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_fg_fixture_state_create(
    service: u32,
    hot: u32,
    flip: u32,
    mfg: u32,
) -> Raw {
    Box::into_raw(Box::new(State {
        frame: Cell::new(0),
        accepted: Cell::new(true),
        samples: Cell::new(0),
        captured_frame: Cell::new(0),
        readback: Cell::new(null_mut()),
        service: service != 0,
        hot: hot != 0,
        flip: flip != 0,
        mfg: mfg != 0,
        inputs: RefCell::new(None),
    }))
    .cast()
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_fixture_state_destroy(state: Raw) {
    if !state.is_null() {
        drop(unsafe { Box::from_raw(state.cast::<State>()) });
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_fixture_state_inputs(
    state: Raw,
    inputs: &rsf_fg_fixture_inputs,
) {
    let state = unsafe { &*state.cast::<State>() };
    *state.inputs.borrow_mut() = Some(unsafe {
        Inputs {
            depth: ID3D12Resource::from_raw(inputs.depth),
            motion: ID3D12Resource::from_raw(inputs.motion),
            cpu_heap: ID3D12DescriptorHeap::from_raw(inputs.cpu_heap),
            gpu_heap: ID3D12DescriptorHeap::from_raw(inputs.gpu_heap),
            color_heap: (!inputs.color_heap.is_null())
                .then(|| ID3D12DescriptorHeap::from_raw(inputs.color_heap)),
            stride: inputs.stride,
        }
    });
}

pub(super) fn provider() -> Option<&'static rsf_generation_provider> {
    // Provider tables are immutable for the lifetime of the linked vendors.
    unsafe { rsf_d3d11_present_provider().as_ref() }
}

pub(super) unsafe fn provider_status(
    api: &rsf_generation_provider,
    session: Raw,
) -> Option<rsf_fg_status> {
    let mut status = rsf_fg_status {
        struct_size: size_of::<rsf_fg_status>() as u32,
        ..Default::default()
    };
    let result = unsafe { (api.status?)(session, &mut status) };
    (result == 0).then_some(status)
}

fn resource(
    raw: Raw,
    width: u32,
    height: u32,
    state: D3D12_RESOURCE_STATES,
) -> rsf_backend_resource {
    rsf_backend_resource {
        struct_size: size_of::<rsf_backend_resource>() as u32,
        resource: raw,
        generation: 1,
        width,
        height,
        state: state.0 as u32,
        ..Default::default()
    }
}

unsafe fn transition(
    commands: Raw,
    texture: Raw,
    from: D3D12_RESOURCE_STATES,
    to: D3D12_RESOURCE_STATES,
) {
    unsafe { rsf_fg_bridge_fixture_barrier(commands, texture, from.0 as u32, to.0 as u32) };
}

pub(super) unsafe extern "C" fn prepare_input(user: Raw, commands: Raw, back: Raw, session: Raw) {
    let state = unsafe { &*user.cast::<State>() };
    if state.flip {
        state.captured_frame.set(state.frame.get());
        let mut readback = state.readback.get();
        let captured = unsafe { rsf_fg_bridge_fixture_capture_flip(commands, back, &mut readback) };
        state.readback.set(readback);
        if captured == 0 {
            state.check(false);
            return;
        }
    }
    let api = provider();
    if !state.service && (api.is_none() || session.is_null()) {
        return;
    }
    let inputs = state.inputs.borrow();
    let Some(inputs) = inputs.as_ref() else {
        state.check(false);
        return;
    };
    if !state.service {
        let options = rsf_fg_options {
            struct_size: size_of::<rsf_fg_options>() as u32,
            abi_version: RSF_FG_ABI_VERSION,
            mode: RSF_FG_FIXED,
            generated_frames: 1,
            reflex_mode: RSF_REFLEX_OFF,
            ..Default::default()
        };
        state.check(
            api.and_then(|api| api.configure)
                .is_some_and(|configure| unsafe { configure(session, &options) } == 0),
        );
    }
    let depth = inputs.depth.as_raw();
    let motion = inputs.motion.as_raw();
    unsafe {
        rsf_fg_bridge_fixture_inputs(
            commands,
            depth,
            motion,
            inputs.cpu_heap.as_raw(),
            inputs.gpu_heap.as_raw(),
            inputs.stride,
        )
    };
    let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
    unsafe {
        rsf_fg_bridge_fixture_frame(
            back,
            depth,
            state.source(),
            stamp(),
            PHASE_UI_COMPLETE,
            &mut record,
        )
    };
    let frame = rsf_fg_frame {
        struct_size: size_of::<rsf_fg_frame>() as u32,
        record: &record,
        interpolate: 1,
        motion_scale_x: 1.0,
        motion_scale_y: 1.0,
        backbuffer: resource(
            back,
            record.output_width,
            record.output_height,
            D3D12_RESOURCE_STATE_PRESENT,
        ),
        depth: resource(
            depth,
            record.render_width,
            record.render_height,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        motion: resource(
            motion,
            record.render_width,
            record.render_height,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        ..Default::default()
    };
    if state.service {
        unsafe { rsf_fg12_capture(commands, &record, depth, motion) };
    }
    let ready = if state.hot {
        let heap = inputs.color_heap.as_ref().unwrap();
        let ready = unsafe {
            rsf_fg_bridge_fixture_hudless(
                commands,
                back,
                heap.as_raw(),
                state.frame.get(),
                u32::from(state.mfg),
                record.output_width,
                record.output_height,
            )
        } != 0;
        if ready {
            let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
            pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
            pass.session_id = 1;
            pass.source_frame_id = record.frame_id;
            pass.view_key = 1;
            pass.color_input = back;
            unsafe {
                rsf_fg12_hudless(commands, &pass);
                transition(
                    commands,
                    back,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PRESENT,
                );
            }
        }
        ready
    } else {
        true
    };
    if !ready {
        state.check(false);
    }
    if ready && !state.service {
        let result = api
            .and_then(|api| api.prepare)
            .map_or(-1, |prepare| unsafe { prepare(session, commands, &frame) });
        if result != 0 {
            println!("prepare frame={} refused={result}", state.frame.get());
        }
        state.check(result == 0);
    }
    for texture in [depth, motion] {
        unsafe {
            transition(
                commands,
                texture,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                D3D12_RESOURCE_STATE_COMMON,
            )
        };
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub(super) unsafe extern "C" fn rsf_fg_fixture_prepare(
    user: Raw,
    _: Raw,
    _: Raw,
    commands: Raw,
    back: Raw,
    _: *mut rsf_streamline_host,
    session: Raw,
    _: u32,
) {
    unsafe { prepare_input(user, commands, back, session) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_fixture_latency(
    user: Raw,
    event: *const rsf_observer_present_event,
) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    let state = unsafe { &*user.cast::<State>() };
    let session = unsafe { rsf_d3d11_present_session() };
    let Some(mark) = provider().and_then(|api| api.marker) else {
        return;
    };
    if session.is_null() {
        return;
    }
    if event.completed == 0 {
        state.check(unsafe { mark(session, RENDER_SUBMIT_END, state.source(), 0) } == 0);
        state.check(unsafe { mark(session, PRESENT_START, state.source(), 0) } == 0);
    } else {
        state.check(unsafe { mark(session, PRESENT_END, state.source(), 0) } == 0);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_fixture_retire(user: Raw, session: Raw) {
    let state = unsafe { &*user.cast::<State>() };
    let Some(api) = provider() else {
        return;
    };
    if session.is_null() {
        return;
    }
    let mut status = rsf_fg_status {
        struct_size: size_of::<rsf_fg_status>() as u32,
        ..Default::default()
    };
    let result = api.status.map_or(-1, |status_call| unsafe {
        status_call(session, &mut status)
    });
    state.check(result == 0);
    state
        .samples
        .set(state.samples.get() + u64::from(status.active));
    let mut fence = null_mut();
    let mut value = 0;
    let result = unsafe { rsf_d3d11_present_retire(session, null_mut(), &mut fence, &mut value) };
    state.check(result == 0 && !fence.is_null() && value != 0);
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_fg_fixture_expect_auto(preferred: u32) -> i32 {
    if unsafe { rsf_d3d11_present_backend() } != preferred
        || (preferred != 0 && unsafe { rsf_d3d11_present_session() }.is_null())
    {
        println!("FAIL: Auto must create the resolved provider");
        return 0;
    }
    println!("PASS: Auto instantiated provider {preferred} before the first Present");
    1
}
