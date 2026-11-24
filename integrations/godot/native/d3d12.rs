// SPDX-License-Identifier: MIT
//! The Godot D3D12 render path on Godot's render thread: the engine's device, queue and
//! completion fence, the SR context (requested provider, Auto resolved by the Runtime, spatial
//! mapped to the normalising context, the fallback to backend 0, the planned input extent and the
//! creation result) with its retirement rules, the three command slots and their fence values, the
//! pending submissions `rsfg_submitted` signals, the FG provider switch request, screen gating, the
//! refusal diagnostics, the current render frame, the pass identity Present reuses and the frame
//! capture owner.
//!
//! The ledger lock is never held across a Runtime call; fence creation and signalling, which
//! call no owner, run under it and keep fence values in submission order.
use crate::control::{self, Settings};
use crate::sys::*;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{
    RSF_FG_BACKEND_AUTO, RSF_FRAME_FLAG_RESET, RSF_GAME_FRAME_ABI_VERSION,
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, RSF_GAME_RENDER_WINDOW,
    RSF_SCREEN_FLIGHT, RSF_SCREEN_MENU, RSF_SCREEN_UNKNOWN, RSF_SR_AUTO, RSF_SR_NONE,
    RSF_SR_SPATIAL, backend_result, rsf_frame_record, rsf_game_render_pass,
};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, MutexGuard};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FENCE_FLAG_NONE, D3D12_RESOURCE_STATE_RENDER_TARGET, ID3D12CommandQueue, ID3D12Device,
    ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory4};
use windows::core::Interface;
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

type Handle = *mut c_void;

/// `rsf_sr12_evaluate` command slots 0..2.
const SLOTS: u32 = 3;
/// Refused passes whose inputs are logged.
const DIAGNOSTIC_LIMIT: u32 = 4;

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

/// The Runtime capture owner. It serializes its state internally; the Arc keeps it alive through
/// a capture call, including when another boot replaces it.
struct Capture(Handle);

unsafe impl Send for Capture {}
unsafe impl Sync for Capture {}

impl Drop for Capture {
    fn drop(&mut self) {
        unsafe { rsf_core_capture12_destroy(self.0) };
    }
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

/// The engine's first device and queue with the completion fence made for them.
#[derive(Clone)]
struct Binding {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
}

impl Binding {
    /// The fence reached `value`, or there is nothing to wait for. False after 15 s.
    fn wait(&self, value: u64) -> bool {
        if value == 0 {
            return true;
        }
        unsafe {
            if self.fence.GetCompletedValue() >= value {
                return true;
            }
            let event = CreateEventW(null(), 0, 0, null());
            if event.is_null() {
                return false;
            }
            let okay = self
                .fence
                .SetEventOnCompletion(value, HANDLE(event))
                .is_ok()
                && WaitForSingleObject(event, 15_000) == WAIT_OBJECT_0;
            CloseHandle(event);
            okay
        }
    }

    fn vendor_id(&self) -> u32 {
        unsafe {
            CreateDXGIFactory1::<IDXGIFactory4>()
                .and_then(|factory| {
                    factory.EnumAdapterByLuid::<IDXGIAdapter1>(self.device.GetAdapterLuid())
                })
                .and_then(|adapter| adapter.GetDesc1())
                .map_or(0, |desc| desc.VendorId)
        }
    }
}

/// The pass identity Present reuses. The engine's borrowed texture handles are cleared.
#[derive(Clone, Copy)]
struct Rendered(rsf_game_render_pass);

// SAFETY: the texture handles are cleared before the copy is stored. The remaining pointer
// fields are identity values copied from the engine's pass; this crate never dereferences them
// and hands the record only to the Runtime's Present.
unsafe impl Send for Rendered {}

impl Rendered {
    fn of(pass: &rsf_game_render_pass) -> Self {
        let mut copy = *pass;
        for handle in [
            &mut copy.color_input,
            &mut copy.color_output,
            &mut copy.depth,
            &mut copy.motion,
            &mut copy.exposure,
            &mut copy.color_output_readable,
            &mut copy.ui_input,
            &mut copy.swapchain,
            &mut copy.scene_surface,
            &mut copy.sampled_texture,
        ] {
            *handle = null_mut();
        }
        Self(copy)
    }
}

/// A live `rsf_sr12` context and the request it was created for.
#[derive(Clone, Copy)]
struct Context {
    /// The Runtime handle, kept as an address.
    handle: usize,
    /// The provider that was created; zero normalises inputs only.
    backend: u32,
    generation: u64,
    output: (u32, u32),
}

impl Context {
    /// A settings generation change, a new output extent, or, for the normalising context, an
    /// input extent other than its plan drains and recreates the context.
    fn stale(
        &self,
        generation: u64,
        output: (u32, u32),
        render: (u32, u32),
        plan: (u32, u32),
    ) -> bool {
        self.generation != generation
            || self.output != output
            || (self.backend == RSF_SR_NONE && render != plan)
    }
}

/// A command list recorded for one slot, waiting for its submission.
#[derive(Clone, Copy)]
struct Pending {
    commands: usize,
    slot: u32,
    session: u64,
    frame: u64,
    /// The HUD-less colour record; its submission is reported to FG.
    hudless: bool,
    /// The engine reported the list on the bound queue, even if the signal refused.
    submitted: bool,
}

struct Ledger {
    /// Published only after the fence exists, so a refused fence is retried on the next pass.
    binding: Option<Binding>,
    capture: Option<Arc<Capture>>,
    context: Option<Context>,
    /// `rsf_sr12_plan` of the last context made; kept after retirement.
    plan: (u32, u32),
    /// The first creation result of the last context request, before any fallback.
    context_result: i32,
    /// The last fence value signalled.
    submitted: u64,
    /// The fence value each slot's last submission signalled.
    slots: [u64; SLOTS as usize],
    next_slot: u32,
    pending: Vec<Pending>,
    current_frame: u64,
    /// Committed only by a recorded pass, so Present reports a refused one as an SR miss.
    rendered: Option<Rendered>,
    attempted_sr: Option<(u64, u64)>,
    /// The slot the source's SR pass acquired, which its HUD-less record may share.
    sr_slot: Option<((u64, u64), u32)>,
    diagnostics: u32,
}

static LEDGER: Mutex<Ledger> = Mutex::new(Ledger {
    binding: None,
    capture: None,
    context: None,
    plan: (0, 0),
    context_result: backend_result::OK,
    submitted: 0,
    slots: [0; SLOTS as usize],
    next_slot: 0,
    pending: Vec::new(),
    current_frame: 0,
    rendered: None,
    attempted_sr: None,
    sr_slot: None,
    diagnostics: 0,
});

fn ledger() -> MutexGuard<'static, Ledger> {
    lock_or_recover(&LEDGER)
}

fn binding() -> Option<Binding> {
    ledger().binding.clone()
}

fn capture() -> Option<Arc<Capture>> {
    ledger().capture.clone()
}

/// A new Runtime capture owner for a boot. A disabled or null owner does not stop rendering.
pub(crate) fn prepare_capture() {
    let capture = Arc::new(Capture(unsafe { rsf_core_capture12_create(1) }));
    let retired = ledger().capture.replace(capture);
    drop(retired);
}

// The DLL's CRT exit hook invokes this after the host is quiesced. Avoid waiting on a thread that
// may have been terminated during process detach.
#[unsafe(no_mangle)]
extern "C" fn rsf_godot_host_unload() {
    let retired = LEDGER
        .try_lock()
        .ok()
        .map(|mut ledger| (ledger.capture.take(), ledger.binding.take()));
    drop(retired);
}

/// FG runs only in requested gameplay on the flight screen; every other pass is a menu pass.
fn screen(settings: &Settings, screen: u32) -> u32 {
    if settings.gameplay != 0 && screen == RSF_SCREEN_FLIGHT {
        RSF_SCREEN_FLIGHT
    } else {
        RSF_SCREEN_MENU
    }
}

/// Keeps the engine's device and queue with a new fence; `None` when the fence failed.
fn bind(device: Handle, queue: Handle) -> Option<Binding> {
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }?;
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }?;
    let fence = unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }.ok()?;
    Some(Binding {
        device: device.clone(),
        queue: queue.clone(),
        fence,
    })
}

/// # Safety
/// `pass` follows the checked record contract; the device, queue and commands are the engine's
/// live objects for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_render(
    device: Handle,
    queue: Handle,
    commands: Handle,
    pass: *const rsf_game_render_pass,
) -> c_int {
    if !control::booted() || device.is_null() || queue.is_null() || commands.is_null() {
        return 0;
    }
    let Some(pass) = (unsafe { rsf_game_sys::ffi_util::checked(pass) }) else {
        return 0;
    };
    if rsf_game_sys::depth_domains_requested(&pass.depth_domains) {
        return 0;
    }
    render(device, queue, commands, pass)
}

fn render(device: Handle, queue: Handle, commands: Handle, pass: &rsf_game_render_pass) -> c_int {
    {
        let mut ledger = ledger();
        if pass.role == RSF_GAME_RENDER_SR {
            ledger.attempted_sr = Some((pass.session_id, pass.source_frame_id));
            ledger.sr_slot = None;
        }
        match &ledger.binding {
            None => match bind(device, queue) {
                Some(binding) => ledger.binding = Some(binding),
                None => return 0,
            },
            Some(bound) if bound.device.as_raw() == device && bound.queue.as_raw() == queue => {}
            Some(_) => return 0,
        }
    }
    if pass.role == RSF_GAME_RENDER_SR
        && let Some(capture) = capture()
    {
        capture.event(pass.session_id, pass.source_frame_id, c"native-sr-begin", 0);
    }
    if pass.role == RSF_GAME_RENDER_FINAL_SCENE {
        return final_scene(commands, pass);
    }
    let Some(binding) = binding() else {
        return 0;
    };
    let (settings, generation) = control::snapshot();
    let fg = match settings.fg_backend {
        RSF_FG_BACKEND_AUTO => unsafe { rsf_fg_default_backend(binding.vendor_id(), 0) },
        backend => backend,
    };
    if unsafe { rsf_d3d11_present_has_owner() } != 0 && fg != unsafe { rsf_d3d11_present_backend() }
    {
        unsafe { rsf_d3d11_present_request(fg) };
    }
    let camera = &pass.camera;
    let output = (camera.output_width, camera.output_height);
    let render = (camera.render_width, camera.render_height);
    let retire = {
        let ledger = ledger();
        let stale = ledger
            .context
            .filter(|context| context.stale(generation, output, render, ledger.plan));
        if stale.is_some() && !ledger.pending.is_empty() {
            return 0;
        }
        stale.map(|context| (context.handle, ledger.submitted))
    };
    if let Some((handle, submitted)) = retire {
        // All submitted work drains before the context is destroyed.
        if !binding.wait(submitted) {
            return 0;
        }
        unsafe { rsf_sr12_destroy(handle as *mut rsf_sr12) };
        ledger().context = None;
    }
    let existing = ledger().context;
    let Some(context) =
        existing.or_else(|| create(&binding, &settings, generation, output, render))
    else {
        return 0;
    };
    let Some(slot) = acquire_slot(&binding) else {
        return 0;
    };
    let mut frame = *pass;
    frame.screen = screen(&settings, pass.screen);
    let sharpening = control::sharpening(&settings);
    let evaluated = unsafe {
        let context = context.handle as *mut rsf_sr12;
        rsf_sr12_set_sharpening(context, &sharpening);
        rsf_sr12_evaluate(context, commands, &frame, slot)
    };
    let (plan, diagnose) = {
        let mut ledger = ledger();
        let diagnose =
            evaluated == backend_result::INVALID_ARGUMENT && ledger.diagnostics < DIAGNOSTIC_LIMIT;
        if diagnose {
            ledger.diagnostics += 1;
        }
        (ledger.plan, diagnose)
    };
    if diagnose {
        refusal_log(&binding, pass, plan);
    }
    // The driver submission callback signals this pending slot after recording is submitted.
    let context_result = {
        let mut ledger = ledger();
        ledger.pending.push(Pending {
            commands: commands as usize,
            slot,
            session: pass.session_id,
            frame: pass.source_frame_id,
            hudless: false,
            submitted: false,
        });
        if evaluated == backend_result::OK {
            ledger.rendered = Some(Rendered::of(pass));
            if pass.role == RSF_GAME_RENDER_SR {
                ledger.sr_slot = Some(((pass.session_id, pass.source_frame_id), slot));
            }
        }
        ledger.context_result
    };
    control::sr_frame(
        settings.sr_backend,
        context.backend,
        context_result,
        evaluated,
        plan,
        context.output,
    );
    c_int::from(evaluated == backend_result::OK)
}

/// Reuse descriptors only after the slot's last submission completed.
fn acquire_slot(binding: &Binding) -> Option<u32> {
    let (slot, fence) = {
        let ledger = ledger();
        let slot = ledger.next_slot % SLOTS;
        if ledger.pending.iter().any(|item| item.slot == slot)
            || ledger.sr_slot.is_some_and(|(_, acquired)| acquired == slot)
        {
            return None;
        }
        (slot, ledger.slots[slot as usize])
    };
    if !binding.wait(fence) {
        return None;
    }
    ledger().next_slot = (slot + 1) % SLOTS;
    Some(slot)
}

/// The HUD-less record can share this source's admitted SR slot while its lists are pending on
/// the same queue. After submission it must wait for that slot's completion fence.
fn final_scene(commands: Handle, pass: &rsf_game_render_pass) -> c_int {
    let Some(binding) = binding() else {
        return 0;
    };
    let source = (pass.session_id, pass.source_frame_id);
    let (attempted, admitted) = {
        let mut ledger = ledger();
        let attempted = ledger.attempted_sr == Some(source);
        let admitted = ledger.sr_slot.take().filter(|(key, _)| *key == source);
        (attempted, admitted)
    };
    let slot = if let Some((_, slot)) = admitted {
        let fence = {
            let ledger = ledger();
            let mut pending = ledger.pending.iter().filter(|item| item.slot == slot);
            if let Some(item) = pending.next() {
                if item.submitted
                    || (item.session, item.frame) != source
                    || pending.next().is_some()
                {
                    return 0;
                }
                None
            } else {
                Some(ledger.slots[slot as usize])
            }
        };
        if fence.is_some_and(|fence| !binding.wait(fence)) {
            return 0;
        }
        slot
    } else {
        if attempted {
            return 0;
        }
        let Some(slot) = acquire_slot(&binding) else {
            return 0;
        };
        slot
    };
    if let Some(capture) = capture() {
        capture.flush(slot);
        capture.record_final_scene(commands, pass, slot);
    }
    unsafe { rsf_fg12_hudless(commands, pass) };
    let mut ledger = ledger();
    ledger.pending.push(Pending {
        commands: commands as usize,
        slot,
        session: pass.session_id,
        frame: pass.source_frame_id,
        hudless: true,
        submitted: false,
    });
    if !attempted {
        ledger.rendered = Some(Rendered::of(pass));
    }
    1
}

/// A new context for the current request. A provider that cannot be created falls back to the
/// normalising context; the first result is the one reported.
fn create(
    binding: &Binding,
    settings: &Settings,
    generation: u64,
    output: (u32, u32),
    render: (u32, u32),
) -> Option<Context> {
    let paths = control::paths()?;
    let mut backend = settings.sr_backend;
    if backend == RSF_SR_AUTO {
        backend = unsafe { rsf_gpu_default_sr_backend(binding.vendor_id(), 0) };
    }
    // Godot evaluates FSR1 before the callback; the context only normalises its inputs.
    if backend == RSF_SR_SPATIAL {
        backend = RSF_SR_NONE;
    }
    let vendors = &paths.vendors;
    let mut setup = rsf_sr12_setup {
        struct_size: size_of::<rsf_sr12_setup>() as u32,
        abi_version: 1,
        device: binding.device.as_raw(),
        backend,
        quality: settings.quality,
        output_width: output.0,
        output_height: output.1,
        render_width: render.0,
        render_height: render.1,
        inverted_depth: 1,
        streamline_host: unsafe { rsf_d3d11_present_host() }.cast(),
        fsr2_directory_utf8: vendors.fidelityfx.as_ptr(),
        fsr3_directory_utf8: vendors.fidelityfx.as_ptr(),
        fsr4_directory_utf8: vendors.fidelityfx.as_ptr(),
        xess_directory_utf8: vendors.xess.as_ptr(),
        log: Some(control::log),
        dlss: rsf_dlss_setup {
            struct_size: size_of::<rsf_dlss_setup>() as u32,
            abi_version: RSF_DLSS_ABI_VERSION,
            interposer_path_utf8: vendors.interposer.as_ptr(),
            plugin_directory_utf8: vendors.streamline.as_ptr(),
            log_directory_utf8: paths.runtime.as_ptr(),
            application_id: 0,
            engine: 0, // RSF_DLSS_ENGINE_CUSTOM
            engine_version_utf8: paths.version.as_ptr(),
            project_id_utf8: paths.project.as_ptr(),
            require_signature: 1,
            log: Some(control::log),
            log_user: null_mut(),
            preset: settings.dlss_preset,
        },
        ..Default::default()
    };
    let mut context = null_mut();
    let result = unsafe { rsf_sr12_create(&setup, &mut context) };
    ledger().context_result = result;
    if context.is_null() && backend != RSF_SR_NONE {
        setup.backend = RSF_SR_NONE;
        let _ = unsafe { rsf_sr12_create(&setup, &mut context) };
        backend = RSF_SR_NONE;
    }
    if context.is_null() {
        return None;
    }
    let mut plan = ledger().plan;
    unsafe { rsf_sr12_plan(context, &mut plan.0, &mut plan.1) };
    let context = Context {
        handle: context as usize,
        backend,
        generation,
        output,
    };
    let mut ledger = ledger();
    ledger.plan = plan;
    ledger.context = Some(context);
    Some(context)
}

fn refusal_log(binding: &Binding, pass: &rsf_game_render_pass, plan: (u32, u32)) {
    let camera = pass.camera;
    control::log_text(format!(
        "Godot SR input refusal: actual={}x{} output={}x{} plan={}x{} device={:p} frame={} history={}",
        camera.render_width,
        camera.render_height,
        camera.output_width,
        camera.output_height,
        plan.0,
        plan.1,
        binding.device.as_raw(),
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
        control::log_text(format!(
            "Godot SR resource {index}: {}x{} format={} samples={} layers={} device={owner:p}",
            desc.Width, desc.Height, desc.Format.0, desc.SampleDesc.Count, desc.DepthOrArraySize,
        ));
    }
}

/// One engine submission on `queue`. When it carries pending command lists, the fence is
/// signalled once for all of them and each slot takes that value.
///
/// # Safety
/// `lists` is null or readable for `count` entries.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_submitted(queue: Handle, lists: *const Handle, count: u32) {
    if queue.is_null() || lists.is_null() || count == 0 {
        return;
    }
    // SAFETY: `count` readable command list pointers, compared by address only.
    let lists = unsafe { std::slice::from_raw_parts(lists.cast::<usize>(), count as usize) };
    let hudless = {
        let mut ledger = ledger();
        let Some(binding) = ledger
            .binding
            .clone()
            .filter(|bound| bound.queue.as_raw() == queue)
        else {
            return;
        };
        for item in &mut ledger.pending {
            item.submitted |= lists.contains(&item.commands);
        }
        if !ledger.pending.iter().any(|item| item.submitted) {
            return;
        }
        // A later signal on this queue also covers previously reported lists.
        let value = ledger.submitted.wrapping_add(1);
        if unsafe { binding.queue.Signal(&binding.fence, value) }.is_err() {
            return;
        }
        ledger.submitted = value;
        let Ledger { pending, slots, .. } = &mut *ledger;
        let mut hudless = Vec::new();
        pending.retain(|item| {
            if !item.submitted {
                return true;
            }
            if let Some(fence) = slots.get_mut(item.slot as usize) {
                *fence = value;
            }
            // HUD-less colour depends on the SR output in the engine graph. Its actual
            // submission establishes both captures; recording completion establishes neither.
            if item.hudless {
                hudless.push((item.session, item.frame));
            }
            false
        });
        hudless
    };
    for (session, frame) in hudless {
        unsafe { rsf_fg12_submitted(session, frame) };
    }
}

/// `rsfg_render_begin` on a booted session.
pub(crate) fn render_begin(frame: u64) {
    {
        let mut ledger = ledger();
        ledger.current_frame = frame;
        ledger.rendered = None;
        ledger.attempted_sr = None;
        ledger.sr_slot = None;
    }
    if frame != 0 {
        let session = control::session_id();
        let result = unsafe { rsf_fg12_render_begin(session, frame) };
        if let Some(capture) = capture() {
            capture.event(session, frame, c"render-submit-begin", result);
        }
    }
}

/// The current frame's rendered identity, or a bare identity and a missed SR report when no pass
/// rendered it.
#[unsafe(no_mangle)]
extern "C" fn rsfg_present(chain: Handle) {
    if !control::booted() || chain.is_null() {
        return;
    }
    let (current, rendered) = {
        let ledger = ledger();
        (ledger.current_frame, ledger.rendered)
    };
    if current == 0 {
        return;
    }
    let session = control::session_id();
    let mut pass = if let Some(Rendered(pass)) = rendered
        && pass.source_frame_id == current
        && pass.session_id == session
    {
        pass
    } else {
        control::sr_missed();
        rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            session_id: session,
            source_frame_id: current,
            native_frame: current,
            screen: RSF_SCREEN_UNKNOWN,
            ..Default::default()
        }
    };
    pass.role = RSF_GAME_RENDER_WINDOW;
    pass.swapchain = chain;
    unsafe { rsf_fg12_window(&pass) };
}

/// The D3D12 branch of `rsfg_shutdown`: drain every submission, end the FG session, then flush
/// capture, destroy the context and cancel the session's frame dump. A refusal at any step keeps
/// the context.
pub(crate) fn shutdown() -> c_int {
    let session = control::session_id();
    let (submitted, handle, binding) = {
        let ledger = ledger();
        if !ledger.pending.is_empty() {
            return 0;
        }
        (
            ledger.submitted,
            ledger.context.map_or(0, |context| context.handle),
            ledger.binding.clone(),
        )
    };
    let drained = submitted == 0 || binding.is_some_and(|binding| binding.wait(submitted));
    if !drained || unsafe { rsf_fg12_end_session(session) } == 0 {
        return 0;
    }
    if let Some(capture) = capture() {
        capture.flush(u32::MAX);
    }
    unsafe {
        rsf_sr12_destroy(handle as *mut rsf_sr12);
        rsf_input_dump_cancel(session);
    }
    ledger().context = None;
    1
}
