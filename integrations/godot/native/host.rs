// SPDX-License-Identifier: MIT
//! D3D12 and Win32 operations behind the Godot session's platform table.

use crate::host_abi::*;
use crate::platform::*;
use crate::rsf_godot_log;
use crate::vulkan::native::*;
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_FRAME_ABI_VERSION, RSF_GAME_RENDER_RESET, rsf_frame_record,
    rsf_game_render_pass,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, PoisonError};
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
    // Runtime capture owner, as declared in runtime/backends/common/frame_capture12.h.
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
    BINDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
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
    CAPTURE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
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

fn empty_frame_record() -> rsf_frame_record {
    // The public C record contains only scalar fields and arrays of scalar fields.
    unsafe { std::mem::zeroed() }
}

impl Capture {
    fn event(&self, session: u64, frame: u64, name: &CStr, result: i32) {
        unsafe {
            if rsf_core_capture12_wants(self.0, frame) != 0 {
                let identity = rsf_frame_record {
                    session_id: session,
                    frame_id: frame,
                    ..empty_frame_record()
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
            ..empty_frame_record()
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
    rsf_godot_log(message);
}

unsafe extern "C" fn accept(_: Handle, window: Handle) -> i32 {
    unsafe { crate::rsf_godot_accept_window(native_platform(), window) }
}

extern "C" fn tick_count() -> u64 {
    unsafe { GetTickCount64() }
}

extern "C" fn prepare_capture() -> u32 {
    let capture = unsafe { rsf_core_capture12_create(1) };
    let retired = CAPTURE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(Arc::new(Capture(capture)));
    drop(retired);
    // A disabled/null Runtime capture owner does not stop rendering.
    1
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

unsafe extern "C" fn fg_install(install: *const FgInstall) -> i32 {
    let install = unsafe { &*install };
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

unsafe extern "C" fn fg_options(value: *const FgOptions) {
    unsafe { rsf_fg12_options(&native_fg_options(&*value)) };
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

unsafe extern "C" fn cpu_counters(facts: *mut InputFacts) {
    let mut inputs = rsf_native_cpu_counters {
        struct_size: size_of::<rsf_native_cpu_counters>() as u32,
        ..Default::default()
    };
    unsafe {
        rsf_native_cpu_counters_get(&mut inputs);
        facts.write(input_facts(&inputs, inputs.derived_frame_ends));
    }
}

unsafe extern "C" fn window_facts(window: Handle, facts: *mut WindowFacts) {
    let mut foreground = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground);
        facts.write(WindowFacts {
            foreground_process_id: foreground,
            current_process_id: GetCurrentProcessId(),
            input_window_visible: u32::from(IsWindowVisible(window) != 0),
            ..Default::default()
        });
    }
}

unsafe extern "C" fn window_identity(window: Handle, identity: *mut WindowIdentity) {
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
        identity.write(value);
    }
}

extern "C" fn window_procedure_get(window: Handle) -> isize {
    unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) }
}

extern "C" fn window_procedure_set(window: Handle, procedure: isize) -> isize {
    unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, procedure) }
}

extern "C" fn window_forward(
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

unsafe extern "C" fn query_counter(counter: *mut u64, frequency: *mut u64) {
    let (mut now, mut rate) = (0, 0);
    unsafe {
        QueryPerformanceCounter(&mut now);
        QueryPerformanceFrequency(&mut rate);
        counter.write(now as u64);
        frequency.write(rate as u64);
    }
}

unsafe extern "C" fn fg_facts(facts: *mut FgFacts) {
    let mut fg = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    unsafe {
        rsf_fg12_status(&mut fg);
        facts.write(FgFacts {
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
            ..Default::default()
        });
    }
}

unsafe extern "C" fn latency_facts(facts: *mut LatencyFacts) -> u32 {
    let mut latency = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..Default::default()
    };
    if unsafe { rsf_native_latency_status_get(&mut latency) } == 0 {
        return 0;
    }
    unsafe {
        facts.write(LatencyFacts {
            inputs: input_facts(&latency.events, latency.derived_boundaries),
            paced_frames: latency.paced_frames,
            marker_calls: latency.marker_calls,
            pcl_available: latency.pcl_available,
            report_available: latency.report_available,
            owner: latency.owner,
            available: latency.available,
            effective_mode: latency.effective_mode,
            ..Default::default()
        });
    }
    1
}

unsafe extern "C" fn d3d12_bind(device: Handle, queue: Handle) -> u32 {
    let (Some(device), Some(queue)) = (
        unsafe { ID3D12Device::from_raw_borrowed(&device) },
        unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) },
    ) else {
        return 0;
    };
    let Ok(fence) = (unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }) else {
        return 0;
    };
    let retired = BINDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(Arc::new(Binding {
            fence,
            queue: queue.clone(),
            device: device.clone(),
        }));
    drop(retired);
    1
}

extern "C" fn d3d12_wait(value: u64) -> u32 {
    if value == 0 {
        return 1;
    }
    let Some(binding) = binding() else {
        return 0;
    };
    unsafe {
        if binding.fence.GetCompletedValue() >= value {
            return 1;
        }
        let event = CreateEventW(null(), 0, 0, null());
        if event.is_null() {
            return 0;
        }
        let okay = binding
            .fence
            .SetEventOnCompletion(value, HANDLE(event))
            .is_ok()
            && WaitForSingleObject(event, 15_000) == WAIT_OBJECT_0;
        CloseHandle(event);
        u32::from(okay)
    }
}

extern "C" fn d3d12_signal(value: u64) -> u32 {
    u32::from(
        binding()
            .is_some_and(|binding| unsafe { binding.queue.Signal(&binding.fence, value).is_ok() }),
    )
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

extern "C" fn sr_default_backend() -> u32 {
    unsafe { rsf_gpu_default_sr_backend(vendor_id(), 0) }
}

extern "C" fn fg_default_backend() -> u32 {
    unsafe { rsf_fg_default_backend(vendor_id(), 0) }
}

unsafe extern "C" fn present_owner(backend: *mut u32) -> u32 {
    unsafe {
        if rsf_d3d11_present_has_owner() == 0 {
            return 0;
        }
        backend.write(rsf_d3d11_present_backend());
    }
    1
}

extern "C" fn present_request(backend: u32) {
    unsafe { rsf_d3d11_present_request(backend) };
}

unsafe extern "C" fn sr_create(request: *const SrCreate, created: *mut SrContext) -> u32 {
    let Some(binding) = binding() else {
        return 0;
    };
    let request = unsafe { &*request };
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
    unsafe {
        let created = &mut *created;
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
    1
}

unsafe extern "C" fn sr_destroy(context: Handle) {
    unsafe { rsf_sr12_destroy(context.cast()) };
}

unsafe extern "C" fn sr_evaluate(
    context: Handle,
    commands: Handle,
    pass: *const rsf_game_render_pass,
    request: *const SrEvaluate,
    result: *mut i32,
) -> u32 {
    unsafe {
        let request = &*request;
        let mut frame = *pass;
        frame.screen = request.screen;
        rsf_sr12_set_sharpening(context.cast(), &request.sharpening);
        result.write(rsf_sr12_evaluate(
            context.cast(),
            commands,
            &frame,
            request.slot,
        ));
    }
    1
}

fn log_text(message: String) {
    if let Ok(message) = CString::new(message) {
        rsf_godot_log(message.as_ptr());
    }
}

unsafe extern "C" fn sr_refusal_log(
    pass: *const rsf_game_render_pass,
    desired_width: u32,
    desired_height: u32,
) {
    let pass = unsafe { &*pass };
    let camera = pass.camera;
    let device = binding().map_or(null_mut(), |binding| binding.device.as_raw());
    log_text(format!(
        "Godot SR input refusal: actual={}x{} output={}x{} plan={}x{} device={:p} frame={} history={}",
        camera.render_width,
        camera.render_height,
        camera.output_width,
        camera.output_height,
        desired_width,
        desired_height,
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
            desc.Width, desc.Height, desc.Format.0, desc.SampleDesc.Count, desc.DepthOrArraySize,
        ));
    }
}

unsafe extern "C" fn capture_sr_begin(pass: *const rsf_game_render_pass) -> u32 {
    if let Some(capture) = capture() {
        let pass = unsafe { &*pass };
        capture.event(pass.session_id, pass.source_frame_id, c"native-sr-begin", 0);
    }
    1
}

unsafe extern "C" fn final_scene(
    commands: Handle,
    pass: *const rsf_game_render_pass,
    slot: u32,
) -> u32 {
    if let Some(capture) = capture() {
        capture.flush(slot);
        capture.record_final_scene(commands, unsafe { &*pass }, slot);
    }
    unsafe { rsf_fg12_hudless(commands, pass) };
    1
}

extern "C" fn render_begin(session: u64, frame: u64) -> u32 {
    let result = unsafe { rsf_fg12_render_begin(session, frame) };
    if let Some(capture) = capture() {
        capture.event(session, frame, c"render-submit-begin", result);
    }
    1
}

extern "C" fn fg_end_session(session: u64) -> u32 {
    u32::from(unsafe { rsf_fg12_end_session(session) } != 0)
}

unsafe extern "C" fn d3d12_release(context: Handle, session: u64) -> u32 {
    if let Some(capture) = capture() {
        capture.flush(u32::MAX);
    }
    unsafe {
        rsf_sr12_destroy(context.cast());
        rsf_input_dump_cancel(session);
    }
    1
}

unsafe extern "C" fn leaf_dlss_preset_valid(preset: u32) -> u32 {
    u32::from(preset <= 13 && (RSF_DLSS_PRESET_MASK & (1 << preset)) != 0)
}

unsafe extern "C" fn leaf_sharpening_valid(options: *const Sharpening) -> u32 {
    let size = unsafe { options.cast::<u32>().read() };
    if (size as usize) < size_of::<Sharpening>() {
        return 0;
    }
    unsafe { rsf_sharpening_core_valid(options.read()) }
}

unsafe extern "C" fn leaf_sharpening_defaults(options: *mut Sharpening) {
    unsafe { options.write(rsf_sharpening_core_defaults()) };
}

unsafe extern "C" fn leaf_fg_backend_known(backend: u32) -> u32 {
    u32::from(unsafe { rsf_fg_backend_known(backend) } != 0)
}

unsafe extern "C" fn leaf_fg_select_backend(vendor: u32, software: u32, choices: u32) -> u32 {
    unsafe { rsf_fg_select_auto_backend(rsf_fg_default_backend(vendor, software), choices) }
}

static PLATFORM: Platform = Platform {
    struct_size: size_of::<Platform>() as u32,
    version: PLATFORM_VERSION,
    tick_count,
    prepare_capture,
    fg_install,
    fg_options,
    dlss_preset_valid: leaf_dlss_preset_valid,
    sharpening_valid: leaf_sharpening_valid,
    sharpening_defaults: leaf_sharpening_defaults,
    fg_backend_known: leaf_fg_backend_known,
    vulkan_supported: rsf_godot_vk_leaf_supported,
    vulkan_latency_facts: rsf_godot_vk_leaf_latency_facts,
    vulkan_locked: rsf_godot_vk_leaf_locked,
    vulkan_start: rsf_godot_vk_leaf_start,
    vulkan_stop: rsf_godot_vk_leaf_stop,
    vulkan_release: rsf_godot_vk_leaf_release,
    vulkan_facts: rsf_godot_vk_leaf_facts,
    vulkan_presenter_max: rsf_godot_vk_leaf_presenter_max,
    vulkan_fsr4_fg_probe: rsf_godot_vk_leaf_fsr4_fg_probe,
    vulkan_latency_options: rsf_godot_vk_leaf_latency_options,
    cpu_counters,
    window_facts,
    fg_facts,
    latency_facts,
    d3d12_bind,
    d3d12_wait,
    d3d12_signal,
    sr_default_backend,
    fg_default_backend,
    present_owner,
    present_request,
    sr_create,
    sr_destroy,
    sr_evaluate,
    sr_refusal_log,
    capture_sr_begin,
    final_scene,
    fg_submitted: rsf_fg12_submitted,
    render_begin,
    present_window: rsf_fg12_window,
    fg_end_session,
    d3d12_release,
    vulkan_presenter_drain: rsf_godot_vk_leaf_presenter_drain,
    vulkan_device_idle: rsf_godot_vk_leaf_device_idle,
    vulkan_sr_destroy: rsf_godot_vk_leaf_sr_destroy,
    vulkan_sr_create: rsf_godot_vk_leaf_sr_create,
    vulkan_sr_plan: rsf_godot_vk_leaf_sr_plan,
    vulkan_sr_record: rsf_godot_vk_leaf_sr_record,
    vulkan_sr_log: rsf_godot_vk_leaf_sr_log,
    vulkan_sr_dump_result: rsf_godot_vk_leaf_sr_dump_result,
    window_identity,
    window_procedure_get,
    window_procedure_set,
    window_forward,
    query_counter,
    d3d12_cpu_service: rsf_fg12_cpu_service,
    vulkan_cpu_service: rsf_latencyvk_cpu_service,
    vulkan_present_marker: rsf_latencyvk_present,
    sr_select_backend: rsf_gpu_select_sr_backend,
    fg_select_backend: leaf_fg_select_backend,
};

pub(crate) fn native_platform() -> &'static Platform {
    &PLATFORM
}
