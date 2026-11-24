// SPDX-License-Identifier: MIT
//! The D3D12, DXGI and Win32 side of `Platform`, calling the Runtime owners.

use crate::platform::*;
use crate::sys::*;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_FRAME_ABI_VERSION, RSF_GAME_RENDER_RESET, rsf_frame_record,
    rsf_game_render_pass,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::{
    Direct3D12::{
        D3D12_FENCE_FLAG_NONE, D3D12_RESOURCE_STATE_RENDER_TARGET, ID3D12CommandQueue,
        ID3D12Device, ID3D12Fence, ID3D12Resource,
    },
    Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory4},
};
use windows::core::Interface;
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::{
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        SystemInformation::GetTickCount64,
        Threading::{CreateEventW, GetCurrentProcessId, WaitForSingleObject},
    },
    UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, GetClassNameW, GetForegroundWindow,
        GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible, SetWindowLongPtrW, WNDPROC,
    },
};

type Handle = *mut c_void;

unsafe extern "C" {
    // Runtime capture owner, as exported by runtime/orchestrator/src/frame_capture12.rs.
    fn rsf_core_capture12_create(environment: u32) -> Handle;
    fn rsf_core_capture12_destroy(owner: Handle);
    fn rsf_core_capture12_wants(owner: Handle, frame: u64) -> i32;
    fn rsf_core_capture12_event(
        owner: Handle,
        identity: *const rsf_frame_record,
        name: *const c_char,
        sequence: u64,
        reset: u32,
        result: i32,
    ) -> i32;
    fn rsf_core_capture12_record(
        owner: Handle,
        list: Handle,
        source: Handle,
        state: u32,
        identity: *const rsf_frame_record,
        role: *const c_char,
        slot: u32,
        ordinal: u32,
        reset: u32,
        preview: u32,
        preview_scale: f32,
    ) -> i32;
    fn rsf_core_capture12_flush(owner: Handle, slot: u32, ready_only: u32) -> i32;
}

struct Binding {
    fence: ID3D12Fence,
    queue: ID3D12CommandQueue,
    device: ID3D12Device,
}

static BINDING: Mutex<Option<Arc<Binding>>> = Mutex::new(None);
static CAPTURE: Mutex<Option<Arc<Capture>>> = Mutex::new(None);

fn binding() -> Option<Arc<Binding>> {
    lock_or_recover(&BINDING).clone()
}

struct Capture(Handle);

// The Runtime capture owner serializes its state internally. Arc keeps the owner alive
// through the capture call, including when another boot replaces it.
unsafe impl Send for Capture {}
unsafe impl Sync for Capture {}

impl Drop for Capture {
    fn drop(&mut self) {
        unsafe { rsf_core_capture12_destroy(self.0) };
    }
}

fn capture() -> Option<Arc<Capture>> {
    lock_or_recover(&CAPTURE).clone()
}

// The DLL's C++ static lifetime invokes this after the host is quiesced. Avoid
// waiting on a thread that may have been terminated during process detach.
#[unsafe(no_mangle)]
extern "C" fn rsf_godot_host_unload() {
    let capture = CAPTURE.try_lock().ok().and_then(|mut value| value.take());
    drop(capture);
    let binding = BINDING.try_lock().ok().and_then(|mut value| value.take());
    drop(binding);
}

impl Capture {
    fn event(&self, session: u64, frame: u64, name: &CStr, result: i32) {
        unsafe {
            if rsf_core_capture12_wants(self.0, frame) != 0 {
                let identity = rsf_frame_record {
                    session_id: session,
                    frame_id: frame,
                    ..Default::default()
                };
                rsf_core_capture12_event(self.0, &identity, name.as_ptr(), frame, 0, result);
            }
        }
    }

    fn flush(&self, slot: u32) {
        unsafe { rsf_core_capture12_flush(self.0, slot, 0) };
    }

    fn record_final_scene(&self, commands: Handle, pass: &rsf_game_render_pass, slot: u32) {
        let record = rsf_frame_record {
            struct_size: size_of::<rsf_frame_record>() as u32,
            abi_version: RSF_GAME_FRAME_ABI_VERSION,
            frame_id: pass.source_frame_id,
            session_id: pass.session_id,
            view_id: pass.view_key as u32,
            camera: pass.camera,
            resource_generation: pass.resource_generation,
            flags: if pass.flags & RSF_GAME_RENDER_RESET != 0 {
                RSF_FRAME_FLAG_RESET
            } else {
                0
            },
            render_width: pass.camera.render_width,
            render_height: pass.camera.render_height,
            frame_time_ms: pass.camera.frame_time_seconds * 1000.0,
            output_width: pass.camera.output_width,
            output_height: pass.camera.output_height,
            ..Default::default()
        };
        unsafe {
            rsf_core_capture12_record(
                self.0,
                commands,
                pass.color_input,
                D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32,
                &record,
                c"fg-hudless-color".as_ptr(),
                slot,
                0,
                u32::MAX,
                u32::MAX,
                0.0,
            )
        };
    }
}

extern "C" fn log(_: Handle, message: *const c_char) {
    crate::CONTROL.log(message);
}

unsafe extern "C" fn accept(_: Handle, window: Handle) -> i32 {
    crate::CONTROL.window.accept(window).into()
}

fn native_fg_options(options: &FgOptions) -> rsf_native_fg_options {
    rsf_native_fg_options {
        struct_size: size_of::<rsf_native_fg_options>() as u32,
        mode: options.mode,
        generated_frames: options.generated_frames,
        reflex_mode: options.reflex_mode,
        frame_limit_us: options.frame_limit_us,
        debug: options.debug,
    }
}

fn input_facts(inputs: &rsf_native_cpu_counters, boundaries: u64) -> InputFacts {
    InputFacts {
        keyboard: inputs.input_events[0],
        mouse: inputs.input_events[1],
        controller: inputs.input_events[2],
        pings: inputs.pings,
        substitutes: inputs.substitutes,
        order_violations: inputs.order_violations,
        derived_boundaries: boundaries,
    }
}

fn vendor_id() -> u32 {
    let Some(binding) = binding() else {
        return 0;
    };
    unsafe {
        CreateDXGIFactory1::<IDXGIFactory4>()
            .and_then(|factory| {
                factory.EnumAdapterByLuid::<IDXGIAdapter1>(binding.device.GetAdapterLuid())
            })
            .and_then(|adapter| adapter.GetDesc1())
            .map_or(0, |desc| desc.VendorId)
    }
}

fn log_text(message: String) {
    if let Ok(message) = CString::new(message) {
        crate::CONTROL.log(message.as_ptr());
    }
}

unsafe extern "C" {
    // The Runtime's private choice (runtime/orchestrator/src/fg_auto_choice.h).
    fn rsf_fg_select_auto_backend(preferred: u32, choices: u32) -> u32;
    // native_cpu_service.h, returning the Rust view of rsf_cpu_session_service.
    fn rsf_fg12_cpu_service() -> *const CpuService;
    fn rsf_latencyvk_cpu_service() -> *const CpuService;
}

// The D3D12, DXGI and Win32 side of the Platform. Device, queue, command list and context
// pointers are the engine's or the Runtime's live objects for the call; this crate only compares
// and passes them. The Vulkan side is in `vulkan/native.rs`.
impl Platform {
    pub(crate) fn tick_count(&self) -> u64 {
        unsafe { GetTickCount64() }
    }

    /// A disabled or null Runtime capture owner does not stop rendering.
    pub(crate) fn prepare_capture(&self) {
        let capture = unsafe { rsf_core_capture12_create(1) };
        let retired = lock_or_recover(&CAPTURE).replace(Arc::new(Capture(capture)));
        drop(retired);
    }

    pub(crate) fn fg_install(&self, install: &FgInstall) -> i32 {
        let setup = rsf_fg12_setup {
            struct_size: size_of::<rsf_fg12_setup>() as u32,
            backend: install.backend,
            max_generated_frames: install.max_generated_frames,
            runtime_switching: install.runtime_switching,
            ui_mode: install.ui_mode,
            options: native_fg_options(&install.options),
            depth_inverted: install.depth_inverted,
            depth_infinite: install.depth_infinite,
            units_to_meters: install.units_to_meters,
            accept_window: Some(accept),
            log: Some(log),
            runtime_directory_utf8: install.fidelityfx_directory,
            fsr3_directory_utf8: install.fidelityfx_directory,
            fsr4_directory_utf8: install.fidelityfx_directory,
            xess_directory_utf8: install.xess_directory,
            streamline_directory_utf8: install.streamline_directory,
            engine_type: install.engine_type,
            engine_version_utf8: install.engine_version,
            project_id_utf8: install.project_id,
            user: null_mut(),
        };
        unsafe { rsf_fg12_install_explicit(&setup) }
    }

    pub(crate) fn fg_options(&self, options: &FgOptions) {
        unsafe { rsf_fg12_options(&native_fg_options(options)) };
    }

    pub(crate) fn dlss_preset_valid(&self, preset: u32) -> bool {
        preset <= 13 && (RSF_DLSS_PRESET_MASK & (1 << preset)) != 0
    }

    pub(crate) fn sharpening_valid(&self, options: &Sharpening) -> bool {
        options.struct_size as usize >= size_of::<Sharpening>()
            && unsafe { rsf_sharpening_core_valid(*options) } != 0
    }

    pub(crate) fn sharpening_defaults(&self) -> Sharpening {
        unsafe { rsf_sharpening_core_defaults() }
    }

    pub(crate) fn fg_backend_known(&self, backend: u32) -> bool {
        unsafe { rsf_fg_backend_known(backend) != 0 }
    }

    pub(crate) fn cpu_counters(&self) -> InputFacts {
        let mut inputs = rsf_native_cpu_counters {
            struct_size: size_of::<rsf_native_cpu_counters>() as u32,
            ..Default::default()
        };
        unsafe { rsf_native_cpu_counters_get(&mut inputs) };
        input_facts(&inputs, inputs.derived_frame_ends)
    }

    /// Foreground process and the visibility of `window`, the admitted input window or null.
    pub(crate) fn window_facts(&self, window: Handle) -> WindowFacts {
        let mut foreground = 0;
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground);
            WindowFacts {
                foreground_process_id: foreground,
                current_process_id: GetCurrentProcessId(),
                input_window_visible: u32::from(IsWindowVisible(window) != 0),
            }
        }
    }

    pub(crate) fn window_identity(&self, window: Handle) -> WindowIdentity {
        let mut value = WindowIdentity::default();
        unsafe {
            GetWindowThreadProcessId(window, &mut value.process_id);
            value.current_process_id = GetCurrentProcessId();
            let length = GetClassNameW(
                window,
                value.class_name.as_mut_ptr(),
                value.class_name.len() as i32,
            );
            value.class_length = length.max(0) as u32;
        }
        value
    }

    /// `GetWindowLongPtrW(GWLP_WNDPROC)`; zero on failure.
    pub(crate) fn window_procedure_get(&self, window: Handle) -> isize {
        unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) }
    }

    /// `SetWindowLongPtrW(GWLP_WNDPROC)`: the replaced procedure, zero on failure.
    pub(crate) fn window_procedure_set(&self, window: Handle, procedure: isize) -> isize {
        unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, procedure) }
    }

    /// `CallWindowProcW` of `previous`, or `DefWindowProcW` when it is zero.
    pub(crate) fn window_forward(
        &self,
        previous: isize,
        window: Handle,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        unsafe {
            if previous == 0 {
                DefWindowProcW(window, message, wparam, lparam)
            } else {
                // CallWindowProcW interprets the previous procedure token returned by Win32.
                let previous = std::mem::transmute::<isize, WNDPROC>(previous);
                CallWindowProcW(previous, window, message, wparam, lparam)
            }
        }
    }

    /// `QueryPerformanceCounter` and `QueryPerformanceFrequency`.
    pub(crate) fn query_counter(&self) -> (u64, u64) {
        let (mut now, mut rate) = (0, 0);
        unsafe {
            QueryPerformanceCounter(&mut now);
            QueryPerformanceFrequency(&mut rate);
        }
        (now as u64, rate as u64)
    }

    pub(crate) fn fg_facts(&self) -> FgFacts {
        let mut fg = rsf_native_fg_status {
            struct_size: size_of::<rsf_native_fg_status>() as u32,
            ..Default::default()
        };
        unsafe {
            rsf_fg12_status(&mut fg);
            FgFacts {
                backend: rsf_d3d11_present_backend(),
                available: fg.available,
                active: fg.vendor.active,
                switch_result: rsf_d3d11_present_switch_result(),
                last_result: fg.last_result,
                reason: fg.reason,
                max_generated_frames: fg.vendor.max_generated_frames,
                generated_frames: fg.vendor.effective_generated_frames,
                tagged_frames: fg.tagged_frames,
                total_presented: fg.vendor.total_presented,
                generated_presented: fg.vendor.generated_presented,
                statistics_flags: fg.vendor.valid_statistics,
                source_frame_id: fg.source_frame_id,
                provider_generation: rsf_d3d11_present_generation(),
                generated_callbacks: fg.vendor.generated_callbacks,
            }
        }
    }

    pub(crate) fn latency_facts(&self) -> Option<LatencyFacts> {
        let mut latency = rsf_native_latency_status {
            struct_size: size_of::<rsf_native_latency_status>() as u32,
            version: 1,
            ..Default::default()
        };
        if unsafe { rsf_native_latency_status_get(&mut latency) } == 0 {
            return None;
        }
        Some(LatencyFacts {
            inputs: input_facts(&latency.events, latency.derived_boundaries),
            paced_frames: latency.paced_frames,
            marker_calls: latency.marker_calls,
            pcl_available: latency.pcl_available,
            report_available: latency.report_available,
            owner: latency.owner,
            available: latency.available,
            effective_mode: latency.effective_mode,
        })
    }

    /// Keeps the engine's device and queue and creates the fence; false when the fence failed.
    pub(crate) fn d3d12_bind(&self, device: Handle, queue: Handle) -> bool {
        let (Some(device), Some(queue)) = (
            unsafe { ID3D12Device::from_raw_borrowed(&device) },
            unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) },
        ) else {
            return false;
        };
        let Ok(fence) = (unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) })
        else {
            return false;
        };
        let retired = lock_or_recover(&BINDING).replace(Arc::new(Binding {
            fence,
            queue: queue.clone(),
            device: device.clone(),
        }));
        drop(retired);
        true
    }

    /// The fence reached `value`, or there is nothing to wait for. False after 15 s.
    pub(crate) fn d3d12_wait(&self, value: u64) -> bool {
        if value == 0 {
            return true;
        }
        let Some(binding) = binding() else {
            return false;
        };
        unsafe {
            if binding.fence.GetCompletedValue() >= value {
                return true;
            }
            let event = CreateEventW(null(), 0, 0, null());
            if event.is_null() {
                return false;
            }
            let okay = binding
                .fence
                .SetEventOnCompletion(value, HANDLE(event))
                .is_ok()
                && WaitForSingleObject(event, 15_000) == WAIT_OBJECT_0;
            CloseHandle(event);
            okay
        }
    }

    pub(crate) fn d3d12_signal(&self, value: u64) -> bool {
        binding()
            .is_some_and(|binding| unsafe { binding.queue.Signal(&binding.fence, value).is_ok() })
    }

    /// `rsf_gpu_default_sr_backend` for the bound adapter.
    pub(crate) fn sr_default_backend(&self) -> u32 {
        unsafe { rsf_gpu_default_sr_backend(vendor_id(), 0) }
    }

    /// `rsf_fg_default_backend` for the bound adapter.
    pub(crate) fn fg_default_backend(&self) -> u32 {
        unsafe { rsf_fg_default_backend(vendor_id(), 0) }
    }

    /// The presentation provider's backend while it has an owner.
    pub(crate) fn present_owner(&self) -> Option<u32> {
        unsafe { (rsf_d3d11_present_has_owner() != 0).then(|| rsf_d3d11_present_backend()) }
    }

    pub(crate) fn present_request(&self, backend: u32) {
        unsafe { rsf_d3d11_present_request(backend) };
    }

    /// `desired` is the previous plan, which stays when no context is made.
    pub(crate) fn sr_create(&self, request: &SrCreate, desired: (u32, u32)) -> Option<SrContext> {
        let binding = binding()?;
        let paths = request.paths;
        let setup = rsf_sr12_setup {
            struct_size: size_of::<rsf_sr12_setup>() as u32,
            abi_version: 1,
            device: binding.device.as_raw(),
            backend: request.backend,
            quality: request.quality,
            output_width: request.output_width,
            output_height: request.output_height,
            render_width: request.render_width,
            render_height: request.render_height,
            inverted_depth: 1,
            streamline_host: unsafe { rsf_d3d11_present_host() }.cast(),
            fsr2_directory_utf8: paths.fidelityfx_directory,
            fsr3_directory_utf8: paths.fidelityfx_directory,
            fsr4_directory_utf8: paths.fidelityfx_directory,
            xess_directory_utf8: paths.xess_directory,
            log: Some(log),
            dlss: rsf_dlss_setup {
                struct_size: size_of::<rsf_dlss_setup>() as u32,
                abi_version: RSF_DLSS_ABI_VERSION,
                interposer_path_utf8: paths.streamline_interposer,
                plugin_directory_utf8: paths.streamline_directory,
                log_directory_utf8: paths.runtime_directory,
                application_id: 0,
                engine: 0, // RSF_DLSS_ENGINE_CUSTOM
                engine_version_utf8: paths.engine_version,
                project_id_utf8: paths.project_id,
                require_signature: 1,
                log: Some(log),
                log_user: null_mut(),
                preset: request.dlss_preset,
            },
            ..Default::default()
        };
        let mut context = null_mut();
        let mut created = SrContext {
            handle: null_mut(),
            result: 0,
            desired_width: desired.0,
            desired_height: desired.1,
        };
        unsafe {
            created.result = rsf_sr12_create(&setup, &mut context);
            created.handle = context.cast();
            if !context.is_null() {
                rsf_sr12_plan(
                    context,
                    &mut created.desired_width,
                    &mut created.desired_height,
                );
            }
        }
        Some(created)
    }

    pub(crate) fn sr_destroy(&self, context: Handle) {
        unsafe { rsf_sr12_destroy(context.cast()) };
    }

    pub(crate) fn sr_evaluate(
        &self,
        context: Handle,
        commands: Handle,
        pass: &rsf_game_render_pass,
        request: &SrEvaluate,
    ) -> i32 {
        let mut frame = *pass;
        frame.screen = request.screen;
        unsafe {
            rsf_sr12_set_sharpening(context.cast(), &request.sharpening);
            rsf_sr12_evaluate(context.cast(), commands, &frame, request.slot)
        }
    }

    pub(crate) fn sr_refusal_log(&self, pass: &rsf_game_render_pass, desired: (u32, u32)) {
        let camera = pass.camera;
        let device = binding().map_or(null_mut(), |binding| binding.device.as_raw());
        log_text(format!(
            "Godot SR input refusal: actual={}x{} output={}x{} plan={}x{} device={:p} frame={} history={}",
            camera.render_width,
            camera.render_height,
            camera.output_width,
            camera.output_height,
            desired.0,
            desired.1,
            device,
            pass.native_frame,
            pass.history_key,
        ));
        for (index, resource) in [pass.color_input, pass.depth, pass.motion, pass.color_output]
            .iter()
            .enumerate()
        {
            let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(resource) }) else {
                continue;
            };
            let desc = unsafe { resource.GetDesc() };
            let mut owner: Option<ID3D12Device> = None;
            let _ = unsafe { resource.GetDevice(&mut owner) };
            let owner = owner.as_ref().map_or(null_mut(), Interface::as_raw);
            log_text(format!(
                "Godot SR resource {index}: {}x{} format={} samples={} layers={} device={owner:p}",
                desc.Width,
                desc.Height,
                desc.Format.0,
                desc.SampleDesc.Count,
                desc.DepthOrArraySize,
            ));
        }
    }

    pub(crate) fn capture_sr_begin(&self, pass: &rsf_game_render_pass) {
        if let Some(capture) = capture() {
            capture.event(pass.session_id, pass.source_frame_id, c"native-sr-begin", 0);
        }
    }

    pub(crate) fn final_scene(&self, commands: Handle, pass: &rsf_game_render_pass, slot: u32) {
        if let Some(capture) = capture() {
            capture.flush(slot);
            capture.record_final_scene(commands, pass, slot);
        }
        unsafe { rsf_fg12_hudless(commands, pass) };
    }

    pub(crate) fn fg_submitted(&self, session_id: u64, frame_id: u64) {
        unsafe { rsf_fg12_submitted(session_id, frame_id) };
    }

    pub(crate) fn render_begin(&self, session_id: u64, frame_id: u64) {
        let result = unsafe { rsf_fg12_render_begin(session_id, frame_id) };
        if let Some(capture) = capture() {
            capture.event(session_id, frame_id, c"render-submit-begin", result);
        }
    }

    pub(crate) fn present_window(&self, pass: &rsf_game_render_pass) {
        unsafe { rsf_fg12_window(pass) };
    }

    pub(crate) fn fg_end_session(&self, session_id: u64) -> bool {
        unsafe { rsf_fg12_end_session(session_id) != 0 }
    }

    /// Capture flush, context destroy (null accepted) and the session's frame dump cancel.
    pub(crate) fn d3d12_release(&self, context: Handle, session_id: u64) {
        if let Some(capture) = capture() {
            capture.flush(u32::MAX);
        }
        unsafe {
            rsf_sr12_destroy(context.cast());
            rsf_input_dump_cancel(session_id);
        }
    }

    /// The session's Runtime-owned endpoint table. It is immutable and lives as long as the DLL.
    pub(crate) fn cpu_service(&self, vulkan: bool) -> Option<&'static CpuService> {
        let entry = if vulkan {
            rsf_latencyvk_cpu_service
        } else {
            rsf_fg12_cpu_service
        };
        // SAFETY: the Runtime returns an immutable process-lived record or null.
        unsafe { crate::checked(entry(), CPU_SERVICE_VERSION) }
    }

    /// `rsf_latencyvk_present`.
    pub(crate) fn vulkan_present_marker(&self, begin: u32) {
        unsafe { rsf_latencyvk_present(begin) };
    }

    /// `rsf_gpu_select_sr_backend`: the Runtime's SR Auto choice among `choices`.
    pub(crate) fn sr_select_backend(&self, vendor: u32, software: u32, choices: u32) -> u32 {
        unsafe { rsf_gpu_select_sr_backend(vendor, software, choices) }
    }

    /// `rsf_fg_select_auto_backend` of `rsf_fg_default_backend`: the Runtime's FG Auto choice
    /// among `choices`.
    pub(crate) fn fg_select_backend(&self, vendor: u32, software: u32, choices: u32) -> u32 {
        unsafe { rsf_fg_select_auto_backend(rsf_fg_default_backend(vendor, software), choices) }
    }
}
