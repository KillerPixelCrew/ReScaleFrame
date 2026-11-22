// SPDX-License-Identifier: MIT
//! Drives real D3D11 and D3D12 chains through the linked provider and overlay.
use crate::d3d11_device_checks::rsf_d3d11_fixture_flip_source;
use crate::fg_bridge::*;
use crate::fg_fixture_callbacks::{State, prepare_input, provider, provider_status};
use crate::fg_fixture_focus::*;
use crate::fixture_device_abi::rsf_fg_fixture_submission;
use crate::present_fixture_loops_abi::*;
use crate::present_fixture_time;
use rsf_game_sys::*;
use std::ffi::CStr;
use std::ptr::null_mut;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation::{HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11RenderTargetView};
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Resource};
use windows::Win32::Graphics::Dxgi::{DXGI_PRESENT, IDXGISwapChain};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};
use windows::core::Interface;

const NOT_SUPPORTED: i32 = -7;
const RSF_LATENCY_SIMULATION_START: u32 = 0;
const RSF_LATENCY_SIMULATION_END: u32 = 1;
const RSF_LATENCY_RENDER_SUBMIT_START: u32 = 2;

// The caller's COM lease outlives this worker on every exit, including an early refusal.
struct Queries {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<(u32, u32)>>,
}

impl Queries {
    fn start(chain: &IDXGISwapChain) -> Option<Self> {
        let receiver = chain.as_raw() as usize;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::Builder::new()
            .spawn(move || {
                let (mut count, mut failed, mut previous) = (0, 0, 0);
                while !stopping.load(Ordering::Acquire) {
                    failed +=
                        unsafe { rsf_fg_bridge_fixture_query(receiver as *mut _, &mut previous) };
                    count += 1;
                    thread::yield_now();
                }
                (count, failed)
            })
            .ok()?;
        Some(Self {
            stop,
            worker: Some(worker),
        })
    }

    fn finish(&mut self) -> Option<(u32, u32)> {
        self.stop.store(true, Ordering::Release);
        self.worker.take()?.join().ok()
    }
}

impl Drop for Queries {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[derive(Default)]
struct Activity {
    expected_tagged: u64,
    samples: u64,
    previous_total: u64,
    mfg: [u32; 6],
    fsr: u32,
    xess: u32,
}

fn capability_snapshot() -> rsf_native_fg_status {
    let mut status = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    unsafe { rsf_fg12_status(&mut status) };
    status
}

impl Activity {
    fn record(
        &mut self,
        plan: &FramePlan,
        id: u64,
        backend: u32,
        epoch: u64,
        mfg: bool,
        hot: bool,
    ) -> bool {
        let result = capability_snapshot();
        let session = unsafe { rsf_d3d11_present_session() };
        let mut accepted = true;
        if !session.is_null()
            && let Some(api) = provider()
            && let Some(capability) = unsafe { provider_status(api, session) }
        {
            let deferred = result.vendor.supported == 0
                && result.vendor.max_generated_frames == 0
                && epoch != unsafe { rsf_d3d11_present_generation() };
            let same = deferred
                || (result.vendor.max_generated_frames == capability.max_generated_frames
                    && result.vendor.supported == capability.supported);
            if deferred {
                println!(
                    "Capability report deferred after provider switch frame={id} backend={}",
                    unsafe { rsf_d3d11_present_backend() }
                );
            }
            if !same {
                println!(
                    "Capability mismatch frame={id} backend={} cached={}/{} current={}/{}",
                    unsafe { rsf_d3d11_present_backend() },
                    result.vendor.supported,
                    result.vendor.max_generated_frames,
                    capability.supported,
                    capability.max_generated_frames
                );
            }
            accepted &= same;
        }
        self.expected_tagged += u64::from(plan.expected_tag);
        self.samples += u64::from(result.vendor.active);
        accepted &= result.last_result == 0 && result.tagged_frames == self.expected_tagged;
        if mfg && backend == RSF_FG_BACKEND_DLSS {
            let vendor = &result.vendor;
            let delta = vendor.total_presented.saturating_sub(self.previous_total);
            if vendor.active != 0
                && delta > u64::from(vendor.effective_generated_frames)
                && let Some(count) = self.mfg.get_mut(vendor.effective_generated_frames as usize)
            {
                *count += 1;
            }
            self.previous_total = vendor.total_presented;
            accepted &= vendor.mfg_unlock_status == RSF_MFG_UNLOCK_READY
                && vendor.max_generated_frames >= 5;
            println!(
                "MFG frame={id} admission={} max={} configured={} active={} presented_delta={delta}",
                vendor.mfg_unlock_status,
                vendor.max_generated_frames,
                vendor.effective_generated_frames,
                vendor.active
            );
        }
        if plan.expect_refusal != 0 {
            accepted &= result.reason != 0;
        }
        if hot {
            match backend {
                RSF_FG_BACKEND_FSR3 => self.fsr += result.vendor.active,
                RSF_FG_BACKEND_XESS => self.xess += result.vendor.active,
                _ => {}
            }
        }
        accepted
    }
}

fn cpu(source: u64) {
    for stage in [
        RSF_GAME_CPU_FRAME_BEGIN,
        RSF_GAME_CPU_PACING,
        RSF_GAME_CPU_INPUT_SAMPLE,
        RSF_GAME_CPU_SIMULATION_BEGIN,
        RSF_GAME_CPU_SIMULATION_END,
    ] {
        let mut event: rsf_game_cpu_event = unsafe { std::mem::zeroed() };
        event.struct_size = size_of::<rsf_game_cpu_event>() as u32;
        event.stage = stage;
        event.session_id = 1;
        event.source_frame_id = source;
        event.timestamp_qpc = present_fixture_time::stamp();
        event.qpc_frequency = present_fixture_time::frequency();
        unsafe { rsf_fg12_cpu(&event) };
    }
}

unsafe fn legacy_begin(
    api: Option<&rsf_generation_provider>,
    session: *mut std::ffi::c_void,
    id: u64,
    legacy_hot: bool,
) -> bool {
    let mut accepted = if legacy_hot {
        let acquired = unsafe { rsf_d3d11_present_acquire(id) };
        let begun = unsafe { rsf_d3d11_present_begin(id, null_mut()) };
        acquired == 0 && begun == 0
    } else {
        api.and_then(|api| api.begin_frame)
            .is_some_and(|begin| unsafe { begin(session, id) } == 0)
    };
    for marker in [
        RSF_LATENCY_SIMULATION_START,
        RSF_LATENCY_SIMULATION_END,
        RSF_LATENCY_RENDER_SUBMIT_START,
    ] {
        let result = if legacy_hot {
            unsafe { rsf_d3d11_present_marker(rsf_d3d11_present_generation(), id, marker, 0) }
        } else {
            let Some(mark) = api.and_then(|api| api.marker) else {
                return false;
            };
            unsafe { mark(session, marker, id, 0) }
        };
        accepted &= result == 0;
    }
    accepted
}

fn legacy_switch(plan: &FramePlan, id: u64) -> bool {
    unsafe { rsf_d3d11_present_abort(id) };
    let requested = plan.legacy_request < 0
        || unsafe { rsf_d3d11_present_request(plan.legacy_request as u32) } == 0;
    if plan.legacy_expected >= 0 {
        let active = unsafe { rsf_d3d11_present_backend() };
        println!(
            "legacy runtime switch frame={id} backend={active} result={}",
            unsafe { rsf_d3d11_present_switch_result() }
        );
        return requested && active == plan.legacy_expected as u32;
    }
    requested
}

fn hot_switch(
    plan: &FramePlan,
    id: u64,
    vendor: u32,
    fsr4_supported: bool,
    streamline: bool,
) -> bool {
    let mut accepted = true;
    if id == 35 && vendor == 0x10de {
        accepted &= unsafe { rsf_d3d11_present_backend() } == RSF_FG_BACKEND_FSR3
            && unsafe { rsf_d3d11_present_switch_result() } == NOT_SUPPORTED;
    }
    if plan.hot_request >= 0 {
        let request = plan.hot_request as u32;
        let expected = if request == RSF_FG_BACKEND_FSR4 && !fsr4_supported {
            NOT_SUPPORTED
        } else {
            0
        };
        accepted &= unsafe { rsf_d3d11_present_request(request) } == expected;
    }
    if id == 36 && streamline {
        accepted &= unsafe { rsf_d3d11_present_backend() } == RSF_FG_BACKEND_DLSS;
    }
    accepted
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_fixture_run_loop(
    run: &rsf_fg_loop,
    focus_out: &mut *mut std::ffi::c_void,
) -> i32 {
    let state = unsafe { &*run.state.cast::<State>() };
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&run.chain) }.unwrap();
    let hot = run.hot != 0;
    let legacy_hot = run.legacy_hot != 0;
    let mfg = run.mfg != 0;
    let window = HWND(run.window);
    let gpu = unsafe { ID3D12Device::from_raw_borrowed(&run.gpu) }.unwrap();
    let fsr4_supported = run.fsr4_supported != 0;
    let mut accepted = (run.backend_choices & (1 << RSF_FG_BACKEND_FSR4) != 0) == fsr4_supported;
    let held = if hot {
        let Ok(resource) = (unsafe { chain.GetBuffer::<ID3D12Resource>(0) }) else {
            return -1;
        };
        Some(resource)
    } else {
        None
    };
    let mut queries = None;
    if hot {
        queries = Queries::start(chain);
        if queries.is_none() {
            return -1;
        }
    }
    if run.focus_test != 0 {
        let lease = rsf_fg_bridge_fixture_focus_start(window);
        if lease.is_null() {
            return -1;
        }
        *focus_out = lease;
    }
    let initial_api = provider();
    let mut session = unsafe { rsf_d3d11_present_session() };
    let mut activity = Activity::default();
    for id in 1..=if mfg { 130 } else { 40 } {
        state.frame.set(id);
        if !focus_out.is_null() && unsafe { rsf_fg_bridge_fixture_focus_valid(*focus_out) } == 0 {
            println!("FAIL: focus test ended at its time limit or after a human focus change");
            accepted = false;
            break;
        }
        if mfg {
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, Some(window), 0, 0, PM_REMOVE) }.as_bool() {
                unsafe { DispatchMessageW(&message) };
            }
            let count = rsf_fg_bridge_fixture_mfg_count(id);
            if count != 0 {
                let changed = rsf_native_fg_options {
                    struct_size: size_of::<rsf_native_fg_options>() as u32,
                    mode: if id == 90 { RSF_FG_OFF } else { RSF_FG_FIXED },
                    generated_frames: count,
                    reflex_mode: RSF_REFLEX_ON,
                    ..Default::default()
                };
                unsafe { rsf_fg12_options(&changed) };
            }
        }
        if !run.application_wait.is_null()
            && unsafe { WaitForSingleObject(HANDLE(run.application_wait), 10000) } != WAIT_OBJECT_0
        {
            println!("FAIL: retained application latency handle stalled at frame {id}");
            return -1;
        }
        let current = unsafe { rsf_d3d11_present_backend() };
        let epoch = unsafe { rsf_d3d11_present_generation() };
        let plan = frame_plan(
            id,
            run.hot,
            run.mfg,
            run.vendor,
            current,
            run.with_streamline,
        );
        session = unsafe { rsf_d3d11_present_session() };
        if run.service != 0 {
            let source = rsf_fg_bridge_fixture_source(id, run.hot);
            cpu(source);
            let submission = rsf_fg_fixture_submission {
                chain: run.chain,
                gpu: run.gpu,
                queue: run.queue,
                allocator: run.allocator,
                list: run.commands,
                finish_allocator: run.finish_allocator,
                finish_list: run.finish_commands,
                session,
                source,
                produce_input: plan.produce_input,
                submit: plan.submit,
                view: plan.view,
                overlay: run.overlay,
                overlay_visible: plan.overlay_visible,
                prepare_user: run.state.cast(),
                prepare: Some(prepare_input),
                log: run.log,
            };
            let mut drawn = u32::from(accepted);
            if unsafe { rsf_fg_bridge_fixture_submit(&submission, &mut drawn) } != 0 {
                return -1;
            }
            accepted &= drawn != 0;
        } else {
            accepted &= unsafe { legacy_begin(initial_api, session, id, legacy_hot) };
        }
        if run.flip11 != 0 {
            let context =
                unsafe { ID3D11DeviceContext::from_raw_borrowed(&run.context11) }.unwrap();
            let view =
                unsafe { ID3D11RenderTargetView::from_raw_borrowed(&run.cached_rtv) }.unwrap();
            unsafe { context.ClearRenderTargetView(view, &[(id % 2) as f32, 0.5, 0.25, 1.0]) };
        }
        let presented = unsafe { chain.Present(0, DXGI_PRESENT(0)) };
        if presented.is_err() {
            let reason = unsafe { (gpu.vtable().GetDeviceRemovedReason)(run.gpu) };
            println!(
                "Present failed frame={id} backend={} HRESULT={:08x} device={:08x}",
                unsafe { rsf_d3d11_present_backend() },
                presented.0 as u32,
                reason.0 as u32
            );
            return -1;
        }
        accepted &= state.accepted.get();
        if legacy_hot {
            accepted &= legacy_switch(&plan, id);
        }
        if run.service != 0 {
            accepted &= activity.record(&plan, id, current, epoch, mfg, hot);
            if let Some(held) = &held {
                accepted &= unsafe { chain.GetBuffer::<ID3D12Resource>(0) }
                    .is_ok_and(|buffer| buffer.as_raw() == held.as_raw());
                accepted &= hot_switch(
                    &plan,
                    id,
                    run.vendor,
                    fsr4_supported,
                    run.with_streamline != 0,
                );
            }
        }
        let drain_queue = if run.flip11 != 0 {
            unsafe { rsf_d3d11_present_queue() }
        } else {
            run.graphics_queue
        };
        if unsafe { rsf_fg_bridge_fixture_drain(drain_queue, run.fence, run.done, id) } != 0 {
            return -1;
        }
        if run.flip11 != 0 {
            let captured = state.captured_frame.get();
            let readback = state.readback.get();
            let matched = unsafe { rsf_fg_bridge_fixture_check_flip(readback, id) };
            if matched <= 0 {
                if matched == 0 {
                    unsafe {
                        rsf_d3d11_fixture_flip_source(
                            run.device11,
                            run.context11,
                            run.cached_rtv,
                            captured,
                        )
                    };
                }
                return -1;
            }
        }
    }
    if mfg {
        let [_, x2, x3, x4, _, x6] = activity.mfg;
        println!("MFG confirmed samples 2x={x2} 3x={x3} 4x={x4} 6x={x6}");
        accepted &= [x2, x3, x4, x6].into_iter().all(|count| count != 0);
    }
    if let Some(queries) = &mut queries {
        let Some((count, failures)) = queries.finish() else {
            return -1;
        };
        println!("concurrent engine queries={count} failures={failures}");
        accepted &= count != 0 && failures == 0;
    }
    let status = if hot {
        let status = capability_snapshot().vendor;
        println!(
            "runtime switches: FSR active={} XeSS active={} buffer identity preserved={}",
            activity.fsr,
            activity.xess,
            u32::from(accepted)
        );
        accepted &= activity.fsr != 0 && activity.xess != 0;
        status
    } else {
        if legacy_hot {
            session = unsafe { rsf_d3d11_present_session() };
        }
        let api = if legacy_hot { provider() } else { initial_api };
        api.and_then(|api| unsafe { provider_status(api, session) })
            .unwrap_or_default()
    };
    accepted &= state.accepted.get();
    let samples = activity.samples + state.samples.get();
    let version = CStr::from_bytes_until_nul(unsafe {
        std::slice::from_raw_parts(
            status.version_name.as_ptr().cast(),
            status.version_name.len(),
        )
    })
    .unwrap_or(c"")
    .to_string_lossy();
    println!(
        "backend={} API={} version={version} accepted={} active_samples={samples} callbacks={} total_presented={}",
        run.backend,
        if run.native12 != 0 { 12 } else { 11 },
        u32::from(accepted),
        status.generated_callbacks,
        status.total_presented
    );
    if accepted && (samples != 0 || legacy_hot) {
        0
    } else {
        1
    }
}
