// SPDX-License-Identifier: MIT
//! Real provider preparation, latency and retirement receipts for the shared FG fixture.
use crate::fg_bridge::{Inputs, capture_flip, frame, hudless, record_inputs, source_id};
use crate::present_fixture_startup::Raw;
use crate::present_fixture_time::stamp;
use crate::sys::*;
use rsf_game_sys::rsf_game_render_pass;
use rsf_test_support::borrow;
use rsf_test_support::d3d12::transition;
use std::cell::{Cell, RefCell};
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

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
    pub(super) readback: RefCell<Option<ID3D12Resource>>,
    service: bool,
    hot: bool,
    flip: bool,
    mfg: bool,
    inputs: RefCell<Option<Inputs>>,
}

impl State {
    pub(super) fn new(service: bool, hot: bool, flip: bool, mfg: bool) -> Self {
        Self {
            frame: Cell::new(0),
            accepted: Cell::new(true),
            samples: Cell::new(0),
            captured_frame: Cell::new(0),
            readback: RefCell::new(None),
            service,
            hot,
            flip,
            mfg,
            inputs: RefCell::new(None),
        }
    }

    pub(super) fn set_inputs(&self, inputs: Inputs) {
        *self.inputs.borrow_mut() = Some(inputs);
    }

    fn check(&self, accepted: bool) {
        self.accepted.set(self.accepted.get() && accepted);
    }

    fn source(&self) -> u64 {
        source_id(self.frame.get(), self.hot)
    }
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
    resource: &ID3D12Resource,
    width: u32,
    height: u32,
    state: D3D12_RESOURCE_STATES,
) -> rsf_backend_resource {
    rsf_backend_resource {
        struct_size: size_of::<rsf_backend_resource>() as u32,
        resource: resource.as_raw(),
        generation: 1,
        width,
        height,
        state: state.0 as u32,
        ..Default::default()
    }
}

/// Records the frame's inputs on `commands` and hands them to the provider, or captures them for
/// the service loop.
pub(super) fn prepare_input(
    state: &State,
    commands: &ID3D12GraphicsCommandList,
    back: &ID3D12Resource,
    session: Raw,
) {
    if state.flip {
        state.captured_frame.set(state.frame.get());
        if !capture_flip(commands, back, &mut state.readback.borrow_mut()) {
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
    record_inputs(commands, inputs);
    let record = frame(
        back,
        &inputs.depth,
        state.source(),
        stamp(),
        PHASE_UI_COMPLETE,
    );
    let provider_frame = rsf_fg_frame {
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
            &inputs.depth,
            record.render_width,
            record.render_height,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        motion: resource(
            &inputs.motion,
            record.render_width,
            record.render_height,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        ..Default::default()
    };
    if state.service {
        unsafe {
            rsf_fg12_capture(
                commands.as_raw(),
                &record,
                inputs.depth.as_raw(),
                inputs.motion.as_raw(),
            )
        };
    }
    let ready = !state.hot || {
        let heap = inputs.color_heap.as_ref().unwrap();
        let ready = hudless(
            commands,
            back,
            heap,
            state.frame.get(),
            state.mfg,
            record.output_width,
            record.output_height,
        );
        if ready {
            let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
            pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
            pass.session_id = 1;
            pass.source_frame_id = record.frame_id;
            pass.view_key = 1;
            pass.color_input = back.as_raw();
            unsafe { rsf_fg12_hudless(commands.as_raw(), &pass) };
            transition(
                commands,
                back,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PRESENT,
            );
        }
        ready
    };
    if !ready {
        state.check(false);
    }
    if ready && !state.service {
        let result = api
            .and_then(|api| api.prepare)
            .map_or(-1, |prepare| unsafe {
                prepare(session, commands.as_raw(), &provider_frame)
            });
        if result != 0 {
            println!("prepare frame={} refused={result}", state.frame.get());
        }
        state.check(result == 0);
    }
    for texture in [&inputs.depth, &inputs.motion] {
        transition(
            commands,
            texture,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            D3D12_RESOURCE_STATE_COMMON,
        );
    }
}

/// The presentation setup's prepare callback; `user` is the fixture [`State`].
#[allow(clippy::too_many_arguments)]
pub(super) unsafe extern "C" fn prepare_callback(
    user: Raw,
    _: Raw,
    _: Raw,
    commands: Raw,
    back: Raw,
    _: *mut rsf_streamline_host,
    session: Raw,
    _: u32,
) {
    unsafe {
        prepare_input(
            &*user.cast::<State>(),
            borrow(&commands),
            borrow(&back),
            session,
        )
    };
}

pub(super) unsafe extern "C" fn latency_callback(
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

pub(super) unsafe extern "C" fn retire_callback(user: Raw, session: Raw) {
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

/// Whether Auto created the provider it resolved to before the first Present.
pub(super) fn expect_auto(preferred: u32) -> bool {
    if unsafe { rsf_d3d11_present_backend() } != preferred
        || (preferred != 0 && unsafe { rsf_d3d11_present_session() }.is_null())
    {
        println!("FAIL: Auto must create the resolved provider");
        return false;
    }
    println!("PASS: Auto instantiated provider {preferred} before the first Present");
    true
}
