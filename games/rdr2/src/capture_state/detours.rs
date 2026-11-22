// SPDX-License-Identifier: MIT
//! Typed D3D12/DXGI observations. Originals retain ordering and return ownership.
use super::d3d12::*;
use super::gpu_leases::{self, ReadbackLeases, rsf_rdr2_capture_gpu_accepts_target};
use super::hooks;
use super::install::owner;
use super::metadata::*;
use super::pipeline::{rsf_rdr2_capture_graphics_facts, rsf_rdr2_capture_stream_facts};
use super::recordings::*;
use super::rsf_rdr2_capture_active;
use super::shots::{
    self, Image, rsf_rdr2_capture_after_submit, rsf_rdr2_capture_before_submit,
    rsf_rdr2_capture_draw_shot, rsf_rdr2_capture_pending_count,
};
use crate::{
    camera_probe::rsf_rdr2_camera_request,
    layout,
    platform::{lock, minhook},
};
use std::{
    arch::naked_asm,
    cell::Cell,
    ffi::{CStr, c_void},
    ptr::{null, null_mut},
    sync::atomic::Ordering,
};
use windows::Win32::Graphics::{
    Direct3D12::*,
    Dxgi::{DXGI_HDR_METADATA_TYPE, IDXGISwapChain, IDXGISwapChain4, IDXGISwapChain4_Vtbl},
};
use windows::core::{BOOL, GUID, HRESULT, IUnknown, Interface, PCWSTR};
use windows_sys::Win32::System::{
    Diagnostics::Debug::RtlCaptureStackBackTrace, SystemInformation::GetTickCount64,
};

#[repr(u32)]
#[derive(Clone, Copy)]
enum Role {
    CreatePso,
    CreateList,
    CreateRtv,
    CreateLibrary,
    LoadPso,
    CreateStreamPso,
    LoadStreamPso,
    Reset,
    SetPso,
    SetRtv,
    Draw,
    DrawIndexed,
    Barrier,
    CreateCommitted,
    CreatePlaced,
    SetHdr,
}
const _: () = assert!(Role::SetHdr as u32 == 15);

// Pin both the trampoline signature and slot to the generated Windows SDK table.
macro_rules! sdk_slots {
    ($table:ty, $($role:ident => $method:ident : $thunk:ident),+ $(,)?) => {
        const _: fn(&mut $table) = |table| { $(table.$method = $thunk;)+ };
        const _: () = {
            $(assert!(std::mem::offset_of!($table, $method)
                == hooks::SLOTS[Role::$role as usize] as usize * size_of::<usize>());)+
        };
    };
}
sdk_slots!(ID3D12Device_Vtbl,
    CreatePso => CreateGraphicsPipelineState: create_pso,
    CreateList => CreateCommandList: create_list,
    CreateRtv => CreateRenderTargetView: create_rtv,
    CreateCommitted => CreateCommittedResource: create_committed,
    CreatePlaced => CreatePlacedResource: create_placed,
);
sdk_slots!(ID3D12Device1_Vtbl, CreateLibrary => CreatePipelineLibrary: create_library);
sdk_slots!(ID3D12Device2_Vtbl, CreateStreamPso => CreatePipelineState: create_stream);
sdk_slots!(ID3D12PipelineLibrary_Vtbl, LoadPso => LoadGraphicsPipeline: load_pso);
sdk_slots!(ID3D12PipelineLibrary1_Vtbl, LoadStreamPso => LoadPipeline: load_stream);
sdk_slots!(ID3D12GraphicsCommandList_Vtbl,
    Reset => Reset: reset,
    SetPso => SetPipelineState: set_pso,
    SetRtv => OMSetRenderTargets: set_rtv,
    Draw => DrawInstanced: draw,
    DrawIndexed => DrawIndexedInstanced: draw_indexed,
    Barrier => ResourceBarrier: barriers,
);
sdk_slots!(IDXGISwapChain4_Vtbl, SetHdr => SetHDRMetaData: set_hdr);

type Raw = *mut c_void;
type Out = *mut Raw;
type Iid = *const GUID;

unsafe extern "C" fn target(object: Raw, slot: u32) -> Raw {
    unsafe { object.cast::<*const Raw>().read().add(slot as usize).read() }
}
unsafe extern "C" fn create(target: Raw, replacement: *const c_void, original: Out) -> i32 {
    unsafe { minhook::MH_CreateHook(target, replacement.cast_mut(), original) }
}
unsafe extern "C" fn enable(target: Raw) -> i32 {
    unsafe { minhook::MH_EnableHook(target) }
}
unsafe extern "C" fn hook_report(slot: u32, target: Raw, result: i32) {
    let status = if result == hooks::DIFFERENT_TARGET {
        c"unobserved vtable target"
    } else {
        let raw = unsafe { minhook::MH_StatusToString(result) };
        if raw.is_null() {
            c"unknown"
        } else {
            unsafe { CStr::from_ptr(raw) }
        }
    };
    owner().log(format_args!(
        "hook slot={slot} target={target:p} result={}",
        status.to_string_lossy()
    ));
}
static HOOK_OPS: hooks::Ops = hooks::Ops {
    target,
    create,
    enable,
    report: hook_report,
};

fn bind(role: Role, object: Raw, detour: *const c_void) {
    hooks::bind(&HOOK_OPS, role as u32, object, detour);
}
fn original<F: Copy>(role: Role) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let address = hooks::original(role as u32);
    assert!(
        !address.is_null(),
        "capture trampoline published before enable"
    );
    unsafe { std::mem::transmute_copy(&address) }
}

unsafe fn output(hr: HRESULT, out: Out) -> Option<Raw> {
    if hr.is_err() || out.is_null() {
        return None;
    }
    let pointer = unsafe { out.read() };
    (!pointer.is_null()).then_some(pointer)
}

unsafe fn query<T: Interface>(object: Raw) -> Option<T> {
    unsafe { IUnknown::from_raw_borrowed(&object) }?.cast().ok()
}

thread_local! {
    static COPYING: Cell<bool> = const { Cell::new(false) };
    static DRAWING: Cell<bool> = const { Cell::new(false) };
}

struct Scope(&'static std::thread::LocalKey<Cell<bool>>, bool);
impl Scope {
    fn enter(flag: &'static std::thread::LocalKey<Cell<bool>>) -> Self {
        Self(flag, flag.with(|cell| cell.replace(true)))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}

pub(super) unsafe extern "C" fn copy_resource(
    list: Raw,
    resource: Raw,
    before: u32,
    leases: &mut ReadbackLeases,
    image: &mut Image,
) -> bool {
    let _scope = Scope::enter(&COPYING);
    let state = owner();
    let mut allocation = 0;
    if !unsafe {
        rsf_rdr2_capture_d3d12_copy(list, resource, before, leases, image, &mut allocation)
    } {
        if allocation != 0 {
            state.log(format_args!(
                "readback allocation failed hr={:08x}",
                allocation as u32
            ));
        }
        return false;
    }
    image.tick = unsafe { GetTickCount64() };
    image.frame = state
        .read(state.base + layout::FRAME_COUNTER)
        .unwrap_or_default();
    true
}

fn pipeline(pso: Raw, facts: Option<&Pipeline>) {
    let state = owner();
    let _guard = lock(&state.observations);
    unsafe {
        rsf_rdr2_capture_record_pipeline(
            &state.capture,
            pso,
            facts.map_or(null(), std::ptr::from_ref),
        )
    };
}

// The vtable methods use SDK descriptor and HRESULT types with raw COM pointers.
macro_rules! pso_detour {
    ($name:ident, $role:ident, $desc:ty, $facts:ident $(, $name_arg:ident : $name_ty:ty)?) => {
        unsafe extern "system" fn $name(this: Raw, $($name_arg: $name_ty,)? desc: *const $desc, iid: Iid, out: Out) -> HRESULT {
            type Original = unsafe extern "system" fn(Raw, $($name_ty,)? *const $desc, Iid, Out) -> HRESULT;
            let hr = unsafe { original::<Original>(Role::$role)(this, $($name_arg,)? desc, iid, out) };
            if let Some(pso) = unsafe { output(hr, out) } {
                let mut facts = Pipeline::default();
                let known = unsafe { $facts(desc, &mut facts) };
                pipeline(pso, known.then_some(&facts));
            }
            hr
        }
    }
}
pso_detour!(
    create_pso,
    CreatePso,
    D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    rsf_rdr2_capture_graphics_facts
);
pso_detour!(load_pso, LoadPso, D3D12_GRAPHICS_PIPELINE_STATE_DESC, rsf_rdr2_capture_graphics_facts, name: PCWSTR);
pso_detour!(
    create_stream,
    CreateStreamPso,
    D3D12_PIPELINE_STATE_STREAM_DESC,
    rsf_rdr2_capture_stream_facts
);
pso_detour!(load_stream, LoadStreamPso, D3D12_PIPELINE_STATE_STREAM_DESC, rsf_rdr2_capture_stream_facts, name: PCWSTR);

unsafe extern "system" fn create_library(
    this: Raw,
    data: *const c_void,
    size: usize,
    iid: Iid,
    out: Out,
) -> HRESULT {
    type Original = unsafe extern "system" fn(Raw, *const c_void, usize, Iid, Out) -> HRESULT;
    let hr = unsafe { original::<Original>(Role::CreateLibrary)(this, data, size, iid, out) };
    if let Some(library) = unsafe { output(hr, out) } {
        bind(Role::LoadPso, library, load_pso as _);
        if let Some(library1) = unsafe { query::<ID3D12PipelineLibrary1>(library) } {
            bind(Role::LoadStreamPso, library1.as_raw(), load_stream as _);
        }
    }
    hr
}

unsafe extern "system" fn create_rtv(
    this: Raw,
    resource: Raw,
    desc: *const D3D12_RENDER_TARGET_VIEW_DESC,
    handle: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    type Original = unsafe extern "system" fn(
        Raw,
        Raw,
        *const D3D12_RENDER_TARGET_VIEW_DESC,
        D3D12_CPU_DESCRIPTOR_HANDLE,
    );
    unsafe { original::<Original>(Role::CreateRtv)(this, resource, desc, handle) };
    let state = owner();
    let _guard = lock(&state.observations);
    unsafe { rsf_rdr2_capture_record_rtv(&state.capture, handle.ptr, resource) };
}

fn initial(hr: HRESULT, out: Out, before: D3D12_RESOURCE_STATES) {
    if !COPYING.get()
        && let Some(resource) = unsafe { output(hr, out) }
    {
        let state = owner();
        let _guard = lock(&state.observations);
        unsafe { rsf_rdr2_capture_initial_state(&state.capture, resource, before.0 as u32) };
    }
}

unsafe extern "system" fn create_committed(
    this: Raw,
    heap: *const D3D12_HEAP_PROPERTIES,
    flags: D3D12_HEAP_FLAGS,
    desc: *const D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
    clear: *const D3D12_CLEAR_VALUE,
    iid: Iid,
    out: Out,
) -> HRESULT {
    type Original = unsafe extern "system" fn(
        Raw,
        *const D3D12_HEAP_PROPERTIES,
        D3D12_HEAP_FLAGS,
        *const D3D12_RESOURCE_DESC,
        D3D12_RESOURCE_STATES,
        *const D3D12_CLEAR_VALUE,
        Iid,
        Out,
    ) -> HRESULT;
    let hr = unsafe {
        original::<Original>(Role::CreateCommitted)(this, heap, flags, desc, state, clear, iid, out)
    };
    initial(hr, out, state);
    hr
}

unsafe extern "system" fn create_placed(
    this: Raw,
    heap: Raw,
    offset: u64,
    desc: *const D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
    clear: *const D3D12_CLEAR_VALUE,
    iid: Iid,
    out: Out,
) -> HRESULT {
    type Original = unsafe extern "system" fn(
        Raw,
        Raw,
        u64,
        *const D3D12_RESOURCE_DESC,
        D3D12_RESOURCE_STATES,
        *const D3D12_CLEAR_VALUE,
        Iid,
        Out,
    ) -> HRESULT;
    let hr = unsafe {
        original::<Original>(Role::CreatePlaced)(this, heap, offset, desc, state, clear, iid, out)
    };
    initial(hr, out, state);
    hr
}

unsafe extern "system" fn barriers(list: Raw, count: u32, items: *const D3D12_RESOURCE_BARRIER) {
    type Original = unsafe extern "system" fn(Raw, u32, *const D3D12_RESOURCE_BARRIER);
    unsafe { original::<Original>(Role::Barrier)(list, count, items) };
    if COPYING.get() || count == 0 || items.is_null() {
        return;
    }
    let state = owner();
    let _guard = lock(&state.observations);
    for barrier in unsafe { std::slice::from_raw_parts(items, count as usize) } {
        match barrier.Type {
            D3D12_RESOURCE_BARRIER_TYPE_TRANSITION => unsafe {
                let t = &barrier.Anonymous.Transition;
                let resource = t.pResource.as_ref().map_or(null_mut(), Interface::as_raw);
                rsf_rdr2_capture_transition(
                    &state.capture,
                    list,
                    resource,
                    t.Subresource,
                    t.StateAfter.0 as u32,
                    barrier.Flags == D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY,
                );
            },
            D3D12_RESOURCE_BARRIER_TYPE_ALIASING => unsafe {
                let resource = barrier
                    .Anonymous
                    .Aliasing
                    .pResourceAfter
                    .as_ref()
                    .map_or(null_mut(), Interface::as_raw);
                rsf_rdr2_capture_alias(&state.capture, list, resource);
            },
            _ => {}
        }
    }
}

unsafe extern "system" fn reset(list: Raw, allocator: Raw, pso: Raw) -> HRESULT {
    type Original = unsafe extern "system" fn(Raw, Raw, Raw) -> HRESULT;
    let hr = unsafe { original::<Original>(Role::Reset)(list, allocator, pso) };
    if hr.is_ok() {
        let state = owner();
        let _guard = lock(&state.observations);
        unsafe { rsf_rdr2_capture_reset_list(&state.capture, list, pso) };
    }
    hr
}

unsafe extern "system" fn set_pso(list: Raw, pso: Raw) {
    type Original = unsafe extern "system" fn(Raw, Raw);
    unsafe { original::<Original>(Role::SetPso)(list, pso) };
    let state = owner();
    let _guard = lock(&state.observations);
    unsafe { rsf_rdr2_capture_bind_pipeline(&state.capture, list, pso) };
}

unsafe extern "system" fn set_rtv(
    list: Raw,
    count: u32,
    handles: *const D3D12_CPU_DESCRIPTOR_HANDLE,
    contiguous: BOOL,
    depth: *const D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    type Original = unsafe extern "system" fn(
        Raw,
        u32,
        *const D3D12_CPU_DESCRIPTOR_HANDLE,
        BOOL,
        *const D3D12_CPU_DESCRIPTOR_HANDLE,
    );
    unsafe { original::<Original>(Role::SetRtv)(list, count, handles, contiguous, depth) };
    let first = (count != 0)
        .then(|| unsafe { handles.as_ref() })
        .flatten()
        .map_or(null(), |h| std::ptr::from_ref(&h.ptr));
    let state = owner();
    let _guard = lock(&state.observations);
    unsafe { rsf_rdr2_capture_bind_rtv(&state.capture, list, first) };
}

// Pack the two counts into rdx, using r8 for the original caller. Preserve r9
// and the argument stack, including BaseVertexLocation and StartInstanceLocation.
#[unsafe(naked)]
unsafe extern "system" fn draw(_: Raw, _: u32, _: u32, _: u32, _: u32) {
    naked_asm!("mov edx, edx", "shl r8, 32", "or rdx, r8", "mov r8, [rsp]", "jmp {}", sym draw_bridge);
}
#[unsafe(naked)]
unsafe extern "system" fn draw_indexed(_: Raw, _: u32, _: u32, _: u32, _: i32, _: u32) {
    naked_asm!("mov edx, edx", "shl r8, 32", "or rdx, r8", "mov r8, [rsp]", "jmp {}", sym draw_indexed_bridge);
}

unsafe extern "system" fn draw_bridge(
    list: Raw,
    counts: u64,
    caller: u64,
    first: u32,
    instance: u32,
) {
    type Original = unsafe extern "system" fn(Raw, u32, u32, u32, u32);
    let vertices = counts as u32;
    let instances = (counts >> 32) as u32;
    let scope = Scope::enter(&DRAWING);
    unsafe { original::<Original>(Role::Draw)(list, vertices, instances, first, instance) };
    if !scope.1 {
        snapshot(list, vertices, instances, caller);
    }
}

unsafe extern "system" fn draw_indexed_bridge(
    list: Raw,
    counts: u64,
    caller: u64,
    first: u32,
    base: i32,
    instance: u32,
) {
    type Original = unsafe extern "system" fn(Raw, u32, u32, u32, i32, u32);
    let indices = counts as u32;
    let instances = (counts >> 32) as u32;
    let scope = Scope::enter(&DRAWING);
    unsafe {
        original::<Original>(Role::DrawIndexed)(list, indices, instances, first, base, instance)
    };
    if !scope.1 {
        snapshot(list, indices, instances, caller);
    }
}

#[inline(never)]
fn snapshot(list: Raw, vertices: u32, instances: u32, caller: u64) {
    let state = owner();
    if !rsf_rdr2_capture_active(&state.capture) {
        return;
    }
    let guard = lock(&state.observations);
    let mut binding = DrawBinding::default();
    if !rsf_rdr2_capture_active(&state.capture)
        || !unsafe { rsf_rdr2_capture_get_draw_binding(&state.capture, list, &mut binding) }
        || !unsafe { rsf_rdr2_capture_gpu_accepts_target(&state.capture, binding.target) }
    {
        return;
    }
    let mut draw = Draw {
        vertices,
        instances,
        caller,
        pso: binding.pso,
        pipeline: binding.pipeline,
        list: list.addr() as u64,
        pipeline_known: binding.pipeline_known,
        ..Draw::default()
    };
    let failed = match unsafe {
        rsf_rdr2_capture_draw_shot(
            &state.capture,
            list,
            binding.target,
            D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32,
            copy_resource,
            rsf_rdr2_capture_d3d12_completed,
            &mut draw,
        )
    } {
        shots::NO_SLOT => true,
        shots::SAVE_EVENT => {
            draw.stack_size = u32::from(unsafe {
                RtlCaptureStackBackTrace(
                    1,
                    draw.stack.len() as u32,
                    draw.stack.as_mut_ptr(),
                    null_mut(),
                )
            });
            !unsafe { rsf_rdr2_capture_record_draw(&state.capture, &draw) }
        }
        _ => false,
    };
    if failed {
        drop(guard);
        state.log(format_args!("snapshot failed"));
    }
}

unsafe extern "system" fn create_list(
    this: Raw,
    node: u32,
    kind: D3D12_COMMAND_LIST_TYPE,
    allocator: Raw,
    pso: Raw,
    iid: Iid,
    out: Out,
) -> HRESULT {
    type Original =
        unsafe extern "system" fn(Raw, u32, D3D12_COMMAND_LIST_TYPE, Raw, Raw, Iid, Out) -> HRESULT;
    let hr = unsafe {
        original::<Original>(Role::CreateList)(this, node, kind, allocator, pso, iid, out)
    };
    if kind == D3D12_COMMAND_LIST_TYPE_DIRECT
        && let Some(list) = unsafe { output(hr, out) }
    {
        bind(Role::Reset, list, reset as _);
        bind(Role::Draw, list, draw as _);
        bind(Role::DrawIndexed, list, draw_indexed as _);
        bind(Role::SetPso, list, set_pso as _);
        bind(Role::Barrier, list, barriers as _);
        bind(Role::SetRtv, list, set_rtv as _);
        let state = owner();
        let _guard = lock(&state.observations);
        unsafe { rsf_rdr2_capture_reset_list(&state.capture, list, pso) };
    }
    hr
}

pub(super) unsafe extern "C" fn device_created(device: Raw) {
    bind(Role::CreatePso, device, create_pso as _);
    bind(Role::CreateList, device, create_list as _);
    bind(Role::CreateRtv, device, create_rtv as _);
    bind(Role::CreateCommitted, device, create_committed as _);
    bind(Role::CreatePlaced, device, create_placed as _);
    if let Some(device) = unsafe { query::<ID3D12Device1>(device) } {
        bind(Role::CreateLibrary, device.as_raw(), create_library as _);
    }
    if let Some(device) = unsafe { query::<ID3D12Device2>(device) } {
        bind(Role::CreateStreamPso, device.as_raw(), create_stream as _);
    }
}

unsafe fn identity(object: Raw) -> Raw {
    unsafe { query::<IUnknown>(object) }
        .as_ref()
        .map_or(object, Interface::as_raw)
}

pub(super) unsafe extern "C" fn color_space_set(chain: Raw, space: u32) {
    let key = unsafe { identity(chain) };
    let state = owner();
    let _guard = lock(&state.observations);
    unsafe { rsf_rdr2_capture_record_color_space(&state.capture, key, space) };
}

unsafe extern "system" fn set_hdr(
    chain: Raw,
    kind: DXGI_HDR_METADATA_TYPE,
    size: u32,
    bytes: *const c_void,
) -> HRESULT {
    type Original =
        unsafe extern "system" fn(Raw, DXGI_HDR_METADATA_TYPE, u32, *const c_void) -> HRESULT;
    let hr = unsafe { original::<Original>(Role::SetHdr)(chain, kind, size, bytes) };
    if hr.is_ok() {
        let key = unsafe { identity(chain) };
        let state = owner();
        let _guard = lock(&state.observations);
        unsafe {
            rsf_rdr2_capture_record_hdr(
                &state.capture,
                key,
                kind.0 as u32,
                bytes.cast(),
                size as usize,
            )
        };
    }
    hr
}

pub(super) unsafe extern "C" fn swapchain_created(chain: Raw) {
    if let Some(chain) = unsafe { query::<IDXGISwapChain4>(chain) } {
        bind(Role::SetHdr, chain.as_raw(), set_hdr as _);
    }
    if unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .is_some_and(|chain| unsafe { chain.GetDesc() }.is_ok())
    {
        let key = unsafe { identity(chain) };
        let state = owner();
        let _guard = lock(&state.observations);
        unsafe { rsf_rdr2_capture_reset_presentation(&state.capture, key) };
    }
}

pub(super) unsafe extern "C" fn before_submit(queue: Raw, count: u32, lists: *const Raw) {
    let state = owner();
    let _guard = lock(&state.observations);
    unsafe { rsf_rdr2_capture_before_submit(&state.capture, queue, count, lists) };
}

pub(super) unsafe extern "C" fn after_submit(queue: Raw, _: u32, _: *const Raw) {
    let state = owner();
    let result = {
        let _guard = lock(&state.observations);
        unsafe {
            rsf_rdr2_capture_after_submit(&state.capture, queue, rsf_rdr2_capture_d3d12_ops())
        }
    };
    let message = match result {
        gpu_leases::NO_SIGNAL_NEEDED | gpu_leases::SIGNAL_ACCEPTED => return,
        gpu_leases::CREATE_REFUSED => "cannot create readback fence; retaining submitted readbacks",
        gpu_leases::SIGNAL_REFUSED => "readback fence Signal failed",
        shots::UNPAIRED_SUBMISSION => "submission pairing failed",
        _ => "readback fence owner unavailable; retaining submitted readbacks",
    };
    state.log(format_args!("{message}"));
}

pub(super) unsafe extern "C" fn before_present(chain: Raw) {
    let state = owner();
    let (armed, completed, progress, pending) = {
        let _guard = lock(&state.observations);
        let game_chain = if state.fixture {
            state.selected_chain.load(Ordering::Acquire)
        } else {
            state
                .read(state.base + layout::SWAPCHAIN)
                .unwrap_or(null_mut())
        };
        if chain != game_chain {
            return;
        }
        let mut completed = null_mut();
        let armed =
            unsafe { rsf_rdr2_capture_d3d12_present(&state.capture, chain, &mut completed) };
        if armed && !state.fixture {
            rsf_rdr2_camera_request();
        }
        let pending = unsafe { rsf_rdr2_capture_pending_count(&state.capture) };
        (armed, completed, state.progress(), pending)
    };
    if !completed.is_null() {
        state.log(format_args!(
            "capture {} recording complete: {} draws, {pending} readbacks resource={completed:p}",
            progress.sequence, progress.draws
        ));
    }
    if armed {
        state.log(format_args!("capture {} armed", progress.sequence));
    }
}
