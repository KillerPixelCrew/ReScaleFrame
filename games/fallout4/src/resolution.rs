// SPDX-License-Identifier: MIT
//! Producer resolution and the synchronous full-output borrow for Fallout 4 AE 1.11.240.

use crate::build_layout::{
    RENDERER_CHAIN, RENDERER_CONTEXT, RENDERER_OBJECT, RENDERER_POINTER, RENDERER_WINDOW,
};
use crate::cpu_events::{current_identity, render_source_is_current};
use crate::depth_domains::{self, Snapshot};
pub(crate) use crate::render_records::NativeOutputOwner as OutputOwner;
use crate::render_records::NativeResolutionReceipt as Receipt;
use crate::screen::{WORLD_LEASE_INVALID, rsf_fallout4_ordinary_screen_refusals};
use crate::{Plain, image, read};
use rsf_game_sys::{
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_SR, rsf_game_host_services,
    rsf_game_render_config, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;
use windows::core::Interface;
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::System::{
    Diagnostics::Debug::WriteProcessMemory, Threading::GetCurrentProcess,
};

const STATE: usize = 0x3d871a0;
pub(crate) const TARGETS: usize = 0x2f4d750;
const RATIOS: usize = TARGETS + 0xfb8;
const VIEWPORT: usize = TARGETS + 0xfe4;
const ACTIVE: usize = TARGETS + 0xfe5;
const DISABLED: usize = TARGETS + 0xfe8;
const SHADOW: usize = 0x1b70;
const RASTER: usize = 0x90;
pub(crate) const MANAGER: usize = 0x3e5e4b0;
const SHADOW_VIEWPORT_DIRTY: u32 = 2;
const VIEWPORT_UNIT: usize = 0x2929458;

type Reserve = unsafe extern "C" fn(*const rsf_game_render_pass) -> bool;
type ReportFn = unsafe extern "C" fn(*const c_char);
type Report = Option<ReportFn>;
type Upscale = unsafe extern "C" fn(*mut c_void, *mut c_void, *const rsf_game_render_pass) -> i32;
type RenderConfig = unsafe extern "C" fn(*mut c_void, *mut rsf_game_render_config) -> i32;

unsafe extern "C" {
    fn rsf_fallout4_resolution_reconfigure(
        upscale: Upscale,
        user: *mut c_void,
        context: *mut c_void,
        planning: &rsf_game_render_pass,
    ) -> bool;
    fn rsf_fallout4_resolution_config(
        query: RenderConfig,
        user: *mut c_void,
        config: &mut rsf_game_render_config,
    ) -> i32;
    fn rsf_fallout4_resolution_report_receipt(
        report: ReportFn,
        pass: &rsf_game_render_pass,
        context: *mut c_void,
        receipt: &Receipt,
    );
}

impl OutputOwner {
    pub(crate) const EMPTY: Self = Self {
        context: 0,
        manager: 0,
        triangles: [0; 3],
    };
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<OutputOwner>() == 40 && offset_of!(OutputOwner, triangles) == 16);
    assert!(size_of::<Receipt>() == 128 && offset_of!(Receipt, input_ratio) == 32);
    assert!(offset_of!(Receipt, frame) == 72 && offset_of!(Receipt, evaluation) == 87);
    assert!(offset_of!(Receipt, actual_count) == 96);
};

#[derive(Clone, Copy, Default)]
struct FrameOwner {
    native_frame: u64,
    window: u64,
    view: u64,
    size: [u32; 2],
    device_context: usize,
    chain: usize,
}

impl FrameOwner {
    fn from_pass(
        pass: &rsf_game_render_pass,
        size: [u32; 2],
        device_context: usize,
        chain: usize,
    ) -> Self {
        Self {
            native_frame: pass.native_frame,
            window: pass.window_key,
            view: pass.view_key,
            size,
            device_context,
            chain,
        }
    }

    fn frame_size_current(self, base: usize) -> bool {
        read::<u32>(base + STATE + 0x98).map(u64::from) == Some(self.native_frame)
            && read::<u32>(base + STATE + 0x78) == Some(self.size[0])
            && read::<u32>(base + STATE + 0x7c) == Some(self.size[1])
    }

    fn renderer_current(self, base: usize) -> bool {
        let renderer = base + RENDERER_OBJECT;
        read::<usize>(base + RENDERER_POINTER) == Some(renderer)
            && read::<usize>(renderer + RENDERER_WINDOW).map(|value| value as u64)
                == Some(self.window)
            && read::<usize>(renderer + RENDERER_CONTEXT) == Some(self.device_context)
            && read::<usize>(renderer + RENDERER_CHAIN) == Some(self.chain)
            && read::<usize>(base + 0x32dd338)
                .filter(|&graph| graph != 0)
                .and_then(|graph| read::<usize>(graph.wrapping_add(0x140)))
                .map(|value| value as u64)
                == Some(self.view)
    }
}

#[derive(Clone, Copy, Default)]
struct Geometry {
    manager: usize,
    triangles: [usize; 3],
}

impl Geometry {
    fn capture(base: usize) -> Option<Self> {
        let manager = read::<usize>(base + MANAGER).filter(|&value| value != 0)?;
        let [a, b, c] = Self::triangles(manager)?;
        (a != 0 && b != 0 && c != 0).then_some(Self {
            manager,
            triangles: [a, b, c],
        })
    }

    fn triangles(manager: usize) -> Option<[usize; 3]> {
        Some([
            read(manager.wrapping_add(0x28))?,
            read(manager.wrapping_add(0x38))?,
            read(manager.wrapping_add(0x40))?,
        ])
    }

    fn current(self, base: usize) -> bool {
        read::<usize>(base + MANAGER) == Some(self.manager)
            && Self::triangles(self.manager) == Some(self.triangles)
    }
}

#[derive(Clone, Copy, Default)]
struct TargetValues {
    ratios: [f32; 2],
    active: u8,
}

impl TargetValues {
    const FULL: Self = Self {
        ratios: [1.0; 2],
        active: 0,
    };

    fn capture(base: usize) -> Option<Self> {
        Some(Self {
            ratios: read(base + RATIOS)?,
            active: read(base + ACTIVE)?,
        })
    }

    fn valid_ratios(self) -> bool {
        self.ratios.iter().all(|&ratio| ratio > 0.0 && ratio <= 1.0)
    }
}

#[derive(Clone, Copy, Default)]
struct TargetWrites {
    ratios: [bool; 2],
    active: bool,
}

impl TargetWrites {
    fn write_ratios(&mut self, base: usize, values: TargetValues) -> bool {
        for (index, ratio) in values.ratios.iter().enumerate() {
            self.ratios[index] = true;
            if !write(base + RATIOS + size_of::<f32>() * index, ratio) {
                return false;
            }
        }
        true
    }

    fn restore(
        &mut self,
        base: usize,
        saved: TargetValues,
        owned: TargetValues,
        changed: &mut bool,
        same_owner: bool,
    ) -> bool {
        let mut okay = true;
        for (index, pending) in self.ratios.iter_mut().enumerate() {
            okay &= restore(
                base + RATIOS + size_of::<f32>() * index,
                saved.ratios[index],
                owned.ratios[index],
                pending,
                changed,
                same_owner,
            );
        }
        okay & restore(
            base + ACTIVE,
            saved.active,
            owned.active,
            &mut self.active,
            changed,
            same_owner,
        )
    }
}

#[derive(Clone, Copy, Default)]
struct FullOutput {
    targets: TargetValues,
    target_writes: TargetWrites,
    viewport: u8,
    jitter: [f32; 2],
    geometry: Geometry,
    raster: [f32; 6],
    owned_raster: [f32; 6],
    dirty: u32,
    owned_dirty: Option<u32>,
    viewport_owned: bool,
    jitter_owned: bool,
    raster_owned: bool,
    dirty_owned: bool,
    native_full_viewport: bool,
}

#[derive(Clone, Copy, Default)]
struct Scope {
    frame: FrameOwner,
    context: usize,
    original_targets: TargetValues,
    input_targets: TargetValues,
    input_writes: TargetWrites,
    full: FullOutput,
    input: bool,
    output: bool,
    changed: bool,
}

impl Scope {
    fn current(&self, base: usize, shadow: bool) -> bool {
        (!shadow || native_context(base) == Some(self.context))
            && self.frame.frame_size_current(base)
            && self.frame.renderer_current(base)
    }

    fn shadow(&self) -> usize {
        self.context.wrapping_add(SHADOW)
    }
}

#[derive(Clone, Copy)]
struct ReceiptHistory {
    session: u64,
    source: u64,
    sources: u32,
    stages: u32,
    ordinary: bool,
}

thread_local! {
    static SCOPE: Cell<Scope> = Cell::new(Scope::default());
    static RECEIPT: Cell<ReceiptHistory> = const { Cell::new(ReceiptHistory {
        session: 0, source: 0, sources: 0, stages: 0, ordinary: false,
    }) };
}

static OWNERS: AtomicU32 = AtomicU32::new(0);
static ORDINARY_ROWS: AtomicU32 = AtomicU32::new(0);
static SAMPLED_ROWS: AtomicU32 = AtomicU32::new(0);

fn write<T: Plain>(address: usize, value: &T) -> bool {
    let mut written = 0;
    let okay = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            std::ptr::from_ref(value).cast(),
            size_of::<T>(),
            &mut written,
        )
    };
    okay != 0 && written == size_of::<T>()
}

trait Field: Plain {
    type Bits: Eq;
    fn bits(self) -> Self::Bits;
}

impl Field for u8 {
    type Bits = Self;
    fn bits(self) -> Self::Bits {
        self
    }
}

impl Field for u32 {
    type Bits = Self;
    fn bits(self) -> Self::Bits {
        self
    }
}

impl Field for f32 {
    type Bits = u32;
    fn bits(self) -> Self::Bits {
        self.to_bits()
    }
}

impl<const N: usize> Field for [f32; N] {
    type Bits = [u32; N];
    fn bits(self) -> Self::Bits {
        self.map(f32::to_bits)
    }
}

fn restore_pending<T: Field>(
    pending: &mut bool,
    changed: &mut bool,
    saved: T,
    owned: T,
    same_owner: bool,
    read_current: impl FnOnce() -> Option<T>,
    write_saved: impl FnOnce(T) -> bool,
) -> bool {
    if !*pending {
        return true;
    }
    let Some(current) = read_current() else {
        return false;
    };
    if current.bits() == saved.bits() {
        *pending = false;
    } else if current.bits() != owned.bits() {
        *changed = true;
        *pending = false;
    } else if same_owner && write_saved(saved) {
        *pending = false;
    }
    !*pending
}

fn restore<T: Field>(
    address: usize,
    saved: T,
    owned: T,
    pending: &mut bool,
    changed: &mut bool,
    same_owner: bool,
) -> bool {
    restore_pending(
        pending,
        changed,
        saved,
        owned,
        same_owner,
        || read(address),
        |value| write(address, &value),
    )
}

#[derive(Clone, Copy)]
struct CleanupOwners {
    renderer: bool,
    shadow: bool,
    advanced: bool,
}

/// Revalidate live owners without treating a later frame as an automatic write refusal.
fn cleanup_owners(base: usize, scope: &Scope, shadow: bool) -> Option<CleanupOwners> {
    let frame = u64::from(read::<u32>(base + STATE + 0x98)?);
    let advanced = frame != scope.frame.native_frame;
    let renderer = scope.frame.renderer_current(base)
        && read::<u32>(base + STATE + 0x78) == Some(scope.frame.size[0])
        && read::<u32>(base + STATE + 0x7c) == Some(scope.frame.size[1]);
    let context = if shadow {
        native_context(base)?
    } else {
        scope.context
    };
    let shadow_current = context == scope.context;
    if !advanced && (!renderer || !shadow_current) {
        return None;
    }
    Some(CleanupOwners {
        renderer,
        shadow: shadow_current,
        advanced,
    })
}

fn native_context(base: usize) -> Option<usize> {
    crate::build_layout::render_context(base, false)
        .map(|(context, _)| context)
        .filter(|&context| context != 0)
}

fn input_enabled(base: usize) -> bool {
    read::<u32>(base + DISABLED) == Some(0)
        && read::<u8>(base + VIEWPORT) == Some(1)
        && read::<u32>(base + STATE + 0xac) == Some(1)
        && read::<u32>(base + STATE + 0xb0) == Some(0)
}

fn output_size(pass: &rsf_game_render_pass) -> [u32; 2] {
    [pass.camera.output_width, pass.camera.output_height]
}

fn configured_targets(
    host: *const rsf_game_host_services,
    frame: &rsf_game_render_pass,
    context: *mut c_void,
    size: [u32; 2],
) -> Option<[u32; 2]> {
    let mut planning = *frame;
    planning.role = RSF_GAME_RENDER_SR;
    planning.flags = RSF_GAME_RENDER_RECONFIGURE | RSF_GAME_RENDER_NO_FG;
    [planning.camera.output_width, planning.camera.output_height] = size;
    // Reconfigure prepares the plan. Admission comes from the sized config that follows.
    let upscale = unsafe { (*host).process_upscale }?;
    if !unsafe { rsf_fallout4_resolution_reconfigure(upscale, (*host).user, context, &planning) } {
        return None;
    }
    let mut config = rsf_game_render_config::request();
    // Reconfigure may reenter. Sample the config callback and user after it.
    let query = unsafe { (*host).render_config }?;
    if unsafe { rsf_fallout4_resolution_config(query, (*host).user, &mut config) } != 1 {
        return None;
    }
    config.checked_render_extent(size)
}

fn preflight(
    base: usize,
    pass: &rsf_game_render_pass,
    context: *mut c_void,
) -> Option<OutputOwner> {
    if context.is_null() {
        return None;
    }
    let engine = native_context(base)?;
    let owner = FrameOwner::from_pass(
        pass,
        output_size(pass),
        context as usize,
        pass.swapchain as usize,
    );
    if !owner.renderer_current(base)
        || !owner.frame_size_current(base)
        || read::<u32>(base + DISABLED) != Some(0)
    {
        return None;
    }
    let shadow = engine.wrapping_add(SHADOW);
    let raster = read::<[f32; 6]>(shadow.wrapping_add(RASTER))?;
    read::<u32>(shadow)?;
    let jitter = read::<[f32; 2]>(base + STATE + 4)?;
    let geometry = Geometry::capture(base)?;
    if !raster.into_iter().all(f32::is_finite) || !jitter.into_iter().all(f32::is_finite) {
        return None;
    }
    Some(OutputOwner {
        context: engine as u64,
        manager: geometry.manager as u64,
        triangles: geometry.triangles.map(|value| value as u64),
    })
}

// The SDK and recovery calls can reenter the native hooks. Do not keep a Rust reference to
// their World pass or a borrow of the thread-local scope across either call.
unsafe fn configure(
    host: *const rsf_game_host_services,
    frame: *const rsf_game_render_pass,
    context: *mut c_void,
    reserve: Option<Reserve>,
) -> Option<()> {
    let current = SCOPE.get();
    if current.input
        || current.output
        || context.is_null()
        || unsafe { (*host).render_config.is_none() || (*host).process_upscale.is_none() }
    {
        return None;
    }
    let cpu = current_identity();
    let pass = unsafe { frame.read() };
    if (cpu.session, cpu.source, cpu.submission)
        != (pass.session_id, pass.source_frame_id, pass.submission_id)
        || !render_source_is_current(&cpu)
    {
        return None;
    }
    let base = image();
    let size = [
        read::<u32>(base + STATE + 0x78)?,
        read::<u32>(base + STATE + 0x7c)?,
    ];
    if !size.iter().all(|value| (1..=16384).contains(value)) || !input_enabled(base) {
        return None;
    }
    let render = configured_targets(host, &pass, context, size)?;
    if !render_source_is_current(&cpu)
        || rsf_fallout4_ordinary_screen_refusals(&cpu) != WORLD_LEASE_INVALID
        || !input_enabled(base)
    {
        return None;
    }

    let pass = unsafe { frame.read() };
    let mut scope = Scope {
        frame: FrameOwner::from_pass(&pass, size, context as usize, pass.swapchain as usize),
        ..Scope::default()
    };
    SCOPE.set(scope);
    scope.context = native_context(base)?;
    SCOPE.set(scope);
    if !scope.current(base, false) {
        return None;
    }
    scope.original_targets = TargetValues::capture(base)?;
    if !scope.original_targets.valid_ratios() {
        return None;
    }
    scope.input_targets = TargetValues {
        ratios: [
            render[0] as f32 / size[0] as f32,
            render[1] as f32 / size[1] as f32,
        ],
        active: u8::from(render != size),
    };
    SCOPE.set(scope);
    if scope.input_targets.active != 0 {
        let mut planning = pass;
        planning.role = RSF_GAME_RENDER_SR;
        [planning.camera.output_width, planning.camera.output_height] = size;
        [planning.camera.render_width, planning.camera.render_height] = render;
        planning.render_rect = [0, 0, render[0] as i32, render[1] as i32];
        planning.output_rect = [0, 0, size[0] as i32, size[1] as i32];
        if !reserve.is_some_and(|reserve| {
            preflight(base, &planning, context).is_some() && unsafe { reserve(&planning) }
        }) {
            SCOPE.set(Scope::default());
            return None;
        }
        scope = SCOPE.get();
    }
    scope.input = true;
    SCOPE.set(scope);
    OWNERS.fetch_add(1, Ordering::AcqRel);
    let okay = scope.input_writes.write_ratios(base, scope.input_targets) && {
        scope.input_writes.active = true;
        write(base + ACTIVE, &scope.input_targets.active)
    };
    SCOPE.set(scope);
    if !okay {
        if !finish_source(base) {
            let mut pending = SCOPE.get();
            pending.changed = true;
            SCOPE.set(pending);
        }
        return None;
    }
    Some(())
}

/// The nonzero-width branch of the verified native builder at 0x1817ef0.
fn replay_output_viewport(base: usize, scope: &Scope) -> Option<()> {
    let [width, height] = scope.frame.size;
    if width == 0 || height == 0 {
        return None;
    }
    let width = width as f32;
    let height = height as f32;
    let shadow = scope.shadow();
    let unit = read::<f32>(base + VIEWPORT_UNIT)?;
    if native_context(base)? != scope.context {
        return None;
    }
    let left = read::<f32>(shadow.checked_add(0x6d0)?)?;
    let x = width * left;
    if !write(shadow + RASTER, &x) {
        return None;
    }
    let top = read::<f32>(shadow.checked_add(0x6d8)?)?;
    let y = (unit - top) * height;
    if !write(shadow + RASTER + 4, &y) {
        return None;
    }
    let right = read::<f32>(shadow.checked_add(0x6d4)?)?;
    let left = read::<f32>(shadow.checked_add(0x6d0)?)?;
    let extent_x = (right - left) * width * unit;
    if !write(shadow + RASTER + 8, &extent_x) {
        return None;
    }
    let top = read::<f32>(shadow.checked_add(0x6d8)?)?;
    let bottom = read::<f32>(shadow.checked_add(0x6dc)?)?;
    let extent_y = (top - bottom) * height * unit;
    if !write(shadow + RASTER + 12, &extent_y) {
        return None;
    }
    // Depth endpoints are bit copies. The dirty OR precedes the last depth store.
    let near = read::<u32>(shadow.checked_add(0x6e0)?)?;
    if !write(shadow + RASTER + 16, &near) {
        return None;
    }
    let far = read::<u32>(shadow.checked_add(0x6e4)?)?;
    let dirty = read::<u32>(shadow)?;
    (write(shadow, &(dirty | SHADOW_VIEWPORT_DIRTY)) && write(shadow + RASTER + 20, &far))
        .then_some(())
}

fn begin_output(snapshot: &Snapshot, pass: &rsf_game_render_pass) -> Option<()> {
    if SCOPE.get().output {
        return None;
    }
    let (context, device_context, chain) = depth_domains::resolution_owner(snapshot)?;
    let mut scope = SCOPE.get();
    if !scope.input {
        scope = Scope {
            frame: FrameOwner::from_pass(pass, output_size(pass), device_context, chain),
            ..Scope::default()
        };
    }
    // Input owns globals only. The verified producer lends its Context for the output shadow.
    scope.context = context;
    SCOPE.set(scope);
    let base = image();
    if !scope.current(base, true) || read::<u32>(base + DISABLED) != Some(0) {
        return None;
    }
    let shadow = scope.shadow();
    scope.full.targets = TargetValues::capture(base)?;
    scope.full.viewport = read(base + VIEWPORT)?;
    scope.full.jitter = read(base + STATE + 4)?;
    scope.full.geometry = Geometry::capture(base)?;
    scope.full.raster = read(shadow.wrapping_add(RASTER))?;
    scope.full.dirty = read(shadow)?;
    if !scope.full.raster.into_iter().all(f32::is_finite)
        || !scope.full.jitter.into_iter().all(f32::is_finite)
    {
        return None;
    }
    scope.output = true;
    scope.full.native_full_viewport = false;
    SCOPE.set(scope);
    OWNERS.fetch_add(1, Ordering::AcqRel);
    let full = &mut scope.full;
    let okay = full.target_writes.write_ratios(base, TargetValues::FULL) && {
        full.target_writes.active = true;
        full.viewport_owned = true;
        full.jitter_owned = true;
        write(base + ACTIVE, &0u8)
            && write(base + VIEWPORT, &0u8)
            && write(base + STATE + 4, &[0.0f32; 2])
    };
    if !okay {
        SCOPE.set(scope);
        end_output(base);
        return None;
    }
    full.raster_owned = true;
    full.dirty_owned = true;
    SCOPE.set(scope);

    // The native writer itself did not call Win32. Its checked replay must
    // leave the incoming LastError for the following observation.
    let error = unsafe { GetLastError() };
    let replayed = replay_output_viewport(base, &scope).is_some();
    unsafe { SetLastError(error) };
    scope = SCOPE.get();
    let owned_raster = read::<[f32; 6]>(shadow.wrapping_add(RASTER));
    if let Some(raster) = owned_raster {
        scope.full.owned_raster = raster;
    }
    scope.full.owned_dirty = read::<u32>(shadow).map(|dirty| dirty & SHADOW_VIEWPORT_DIRTY);
    SCOPE.set(scope);
    if replayed
        && owned_raster.is_some_and(|raster| {
            let [width, height] = scope.frame.size;
            scope.full.geometry.current(base)
                && raster[..4] == [0.0, 0.0, width as f32, height as f32]
        })
    {
        return Some(());
    }
    end_output(base);
    None
}

fn native_dr(base: usize, scope: &mut Scope) -> Option<()> {
    if !scope.input && !scope.output {
        return Some(());
    }
    if !scope.current(base, scope.output) {
        return None;
    }
    let targets = TargetValues::capture(base)?;
    if !targets.valid_ratios() {
        return None;
    }
    // A completed native writer supersedes input ownership even when its values are equal.
    // The input scope still pins its source until Swap retires it.
    scope.input_writes = TargetWrites::default();
    if scope.output {
        scope.full.targets = targets;
        if !scope
            .full
            .target_writes
            .write_ratios(base, TargetValues::FULL)
        {
            return None;
        }
        scope.full.target_writes.active = true;
        if !write(base + ACTIVE, &0u8) {
            return None;
        }
    }
    Some(())
}

fn end_output(base: usize) -> bool {
    let mut scope = SCOPE.get();
    if !scope.output {
        return !scope.changed;
    }
    let Some(owners) = cleanup_owners(base, &scope, true) else {
        return false;
    };
    let shadow = scope.shadow();
    let full = &mut scope.full;
    if owners.advanced || !full.geometry.current(base) {
        scope.changed = true;
    }
    let mut okay = full.target_writes.restore(
        base,
        full.targets,
        TargetValues::FULL,
        &mut scope.changed,
        owners.renderer,
    );
    okay &= restore(
        base + VIEWPORT,
        full.viewport,
        0,
        &mut full.viewport_owned,
        &mut scope.changed,
        owners.renderer,
    );
    okay &= restore(
        base + STATE + 4,
        full.jitter,
        [0.0; 2],
        &mut full.jitter_owned,
        &mut scope.changed,
        owners.renderer,
    );
    // A new live Context supersedes the shadow association. Never chase the old
    // pointer across this boundary. In the original Context compare each write.
    if full.native_full_viewport || !owners.shadow {
        full.raster_owned = false;
        full.dirty_owned = false;
    }
    okay &= restore(
        shadow.wrapping_add(RASTER),
        full.raster,
        full.owned_raster,
        &mut full.raster_owned,
        &mut scope.changed,
        owners.renderer && owners.shadow,
    );
    if full.dirty_owned {
        okay &= if let Some(dirty) = read::<u32>(shadow) {
            let saved = full.dirty & SHADOW_VIEWPORT_DIRTY;
            let current = dirty & SHADOW_VIEWPORT_DIRTY;
            if let Some(owned) = full.owned_dirty {
                restore_pending(
                    &mut full.dirty_owned,
                    &mut scope.changed,
                    saved,
                    owned,
                    owners.renderer,
                    || Some(current),
                    |value| write(shadow, &((dirty & !SHADOW_VIEWPORT_DIRTY) | value)),
                )
            } else {
                full.dirty_owned = current != saved;
                !full.dirty_owned
            }
        } else {
            false
        };
    }
    if okay {
        scope.output = false;
        OWNERS.fetch_sub(1, Ordering::AcqRel);
    }
    SCOPE.set(scope);
    okay && !scope.changed
}

fn finish_source(base: usize) -> bool {
    let valid = end_output(base);
    let mut scope = SCOPE.get();
    if scope.output {
        return false;
    }
    if !scope.input {
        SCOPE.set(Scope::default());
        return valid;
    }
    let Some(owners) = cleanup_owners(base, &scope, false) else {
        return false;
    };
    scope.changed |= owners.advanced;
    let okay = scope.input_writes.restore(
        base,
        scope.original_targets,
        scope.input_targets,
        &mut scope.changed,
        owners.renderer,
    );
    let accepted = okay && valid && !scope.changed;
    if okay {
        OWNERS.fetch_sub(1, Ordering::AcqRel);
        scope = Scope::default();
    }
    SCOPE.set(scope);
    accepted
}

/// The verified ordinary World boundary may retire an older scope on its owning thread.
pub(crate) fn retry_retired(base: usize) {
    let scope = SCOPE.get();
    if (scope.input || scope.output)
        && read::<u32>(base + STATE + 0x98)
            .is_some_and(|frame| u64::from(frame) != scope.frame.native_frame)
    {
        let _ = finish_source(base);
    }
}

fn in_receipt_budget(index: u32, pass: &rsf_game_render_pass, scope: &Scope) -> bool {
    let mut receipt = RECEIPT.get();
    let same_source = (receipt.session, receipt.source) == (pass.session_id, pass.source_frame_id);
    let camera = &pass.camera;
    let reduced = |render: u32, output: u32| render != 0 && render < output;
    if !(same_source
        || scope.input
            && scope
                .input_targets
                .ratios
                .into_iter()
                .any(|ratio| ratio < 1.0)
        || reduced(camera.render_width, camera.output_width)
        || reduced(camera.render_height, camera.output_height))
    {
        return false;
    }
    if !same_source {
        receipt.session = pass.session_id;
        receipt.source = pass.source_frame_id;
        receipt.stages = 0;
        receipt.ordinary = receipt.sources < 4;
        receipt.sources = receipt.sources.wrapping_add(1);
        RECEIPT.set(receipt);
    }
    let bit = 1 << index;
    if (!receipt.ordinary && !pass.native_frame.is_multiple_of(128)) || receipt.stages & bit != 0 {
        return false;
    }
    let budget = if receipt.ordinary {
        &ORDINARY_ROWS
    } else {
        &SAMPLED_ROWS
    };
    if budget.fetch_add(1, Ordering::Relaxed) >= 32 {
        return false;
    }
    receipt.stages |= bit;
    RECEIPT.set(receipt);
    true
}

fn collect_scalars(base: usize, receipt: &mut Receipt) -> Option<()> {
    receipt.ratio = read(base + RATIOS)?;
    receipt.viewport = read(base + VIEWPORT)?;
    receipt.active = read(base + ACTIVE)?;
    receipt.frame = read(base + STATE + 0x98)?;
    let manager = read::<usize>(base + MANAGER).filter(|&value| value != 0)?;
    receipt.partial = read(manager.wrapping_add(0x48))?;
    for (index, offset) in [0x28, 0x38, 0x40].into_iter().enumerate() {
        receipt.geometry[index] = read(manager.wrapping_add(offset))?;
    }
    Some(())
}

fn collect_shadow(scope: &Scope, receipt: &mut Receipt) -> Option<()> {
    let shadow = scope.shadow();
    receipt.raster = read(shadow.wrapping_add(RASTER))?;
    receipt.dirty = read(shadow)?;
    Some(())
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fallout4_resolution_verify() -> bool {
    let base = image();
    fn guard<const N: usize>(base: usize, rva: usize, expected: &[u8; N]) -> bool {
        read::<[u8; N]>(base + rva).as_ref() == Some(expected)
    }
    guard(base, 0x182d586, b"\x83\xb9\xac\x00\x00\x00\x00")
        && guard(base, 0x182d647, b"\xf3\x0f\x11\x7f\x04\xf3\x0f\x11\x5f\x08")
        && guard(
            base,
            0x21856f5,
            b"\x80\x3d\x38\x90\xdc\x00\x00\xba\x38\x00\x00\x00\x75\x05\xba\x28\x00\x00\x00",
        )
        && guard(
            base,
            0x21856df,
            b"\x80\x7d\x48\x00\x74\x07\xba\x40\x00\x00\x00",
        )
        && guard(base, 0x1839ec0, b"\x88\x91\xe4\x0f\x00\x00\xc3")
        && guard(base, 0x183a2d4, b"\xf0\xff\x81\xe8\x0f\x00\x00")
        && guard(base, 0x183a278, b"\x41\xc6\x82\xe5\x0f\x00\x00\x01")
        && guard(base, 0x181801e, b"\xf3\x0f\x11\x83\x98\x00\x00\x00")
        && guard(base, 0x1839e00, b"\x48\x83\xec\x38\x8b\x15\x0e\x92\x63\x02")
        // Guard the explicit-size entry and the common shadow writer through RET.
        && guard(base, 0x1817ef0, b"\x40\x53\x48\x83\xec\x60\x0f\x29\x74\x24\x50\x48\x8b\xda\xf3\x0f\x10\x35\
            \x52\x15\x11\x01\x0f\x29\x7c\x24\x40\x44\x0f\x29\x44\x24\x30\x44\x0f\x28\
            \xc6\x44\x0f\x29\x4c\x24\x20\x44\x0f\x28\xce\x45\x85\xc0\x74\x10\x41\x8b\
            \xc0\x0f\x57\xff\xf3\x48\x0f\x2a\xf8\x41\x8b\xc1\xeb\x63")
        && guard(base, 0x1817f97, b"\x8b\x0d\x7b\xb0\x65\x02\x0f\x57\xd2\xf3\x48\x0f\x2a\xd0\x65\x48\x8b\x04\
            \x25\x58\x00\x00\x00\xba\x20\x0b\x00\x00\x48\x8b\x04\xc8\x48\x8b\x0c\x02\
            \x48\x85\xc9\x75\x07\x48\x8b\x0d\xd1\x93\x0c\x02\x48\x81\xc1\x70\x1b\x00\
            \x00\x0f\x28\xc7\xf3\x0f\x59\x81\xd0\x06\x00\x00\xf3\x0f\x11\x83\x90\x00\
            \x00\x00\xf3\x0f\x5c\xb1\xd8\x06\x00\x00\xf3\x0f\x59\xf2\xf3\x0f\x11\xb3\
            \x94\x00\x00\x00\xf3\x0f\x10\x81\xd4\x06\x00\x00\xf3\x0f\x5c\x81\xd0\x06\
            \x00\x00\x0f\x28\x74\x24\x50\xf3\x0f\x59\xc7\x0f\x28\x7c\x24\x40\xf3\x41\
            \x0f\x59\xc0\x44\x0f\x28\x44\x24\x30\xf3\x0f\x11\x83\x98\x00\x00\x00\xf3\
            \x0f\x10\x89\xd8\x06\x00\x00\xf3\x0f\x5c\x89\xdc\x06\x00\x00\xf3\x0f\x59\
            \xca\xf3\x41\x0f\x59\xc9\x44\x0f\x28\x4c\x24\x20\xf3\x0f\x11\x8b\x9c\x00\
            \x00\x00\x8b\x81\xe0\x06\x00\x00\x89\x83\xa0\x00\x00\x00\x8b\x81\xe4\x06\
            \x00\x00\x83\x0b\x02\x89\x83\xa4\x00\x00\x00\x48\x83\xc4\x60\x5b\xc3")
        && guard(base, VIEWPORT_UNIT, &1.0_f32.to_le_bytes())
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fallout4_resolution_inputs(pass: &rsf_game_render_pass) -> bool {
    let [left, top, right, bottom] = pass.render_rect;
    let [output_left, output_top, output_right, output_bottom] = pass.output_rect;
    pass.camera_valid != 0
        && left >= 0
        && top >= 0
        && right > left
        && bottom > top
        && right <= output_right
        && bottom <= output_bottom
        && (right - left) as u32 == pass.camera.render_width
        && (bottom - top) as u32 == pass.camera.render_height
        && output_left == 0
        && output_top == 0
        && [output_right as u32, output_bottom as u32] == output_size(pass)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_resolution_configure(
    host: *const rsf_game_host_services,
    frame: *const rsf_game_render_pass,
    context: *mut c_void,
    reserve: Option<Reserve>,
) -> bool {
    unsafe { configure(host, frame, context, reserve) }.is_some()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_resolution_preflight(
    pass: &rsf_game_render_pass,
    context: *mut c_void,
    output: *mut OutputOwner,
) -> bool {
    if let Some(output) = unsafe { output.as_mut() } {
        *output = OutputOwner::default();
    }
    let Some(owner) = preflight(image(), pass, context) else {
        return false;
    };
    if let Some(output) = unsafe { output.as_mut() } {
        *output = owner;
    }
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_begin_output(
    snapshot: &Snapshot,
    pass: &rsf_game_render_pass,
) -> bool {
    begin_output(snapshot, pass).is_some()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_native_dr() -> bool {
    let mut scope = SCOPE.get();
    let okay = native_dr(image(), &mut scope).is_some();
    SCOPE.set(scope);
    okay
}

pub(crate) fn native_viewport(enabled: bool) -> bool {
    let mut scope = SCOPE.get();
    if !scope.output {
        return true;
    }
    // The original callback supplies FE4 intent even when it writes our same value.
    scope.full.viewport = u8::from(enabled);
    if !enabled {
        scope.full.native_full_viewport = true;
    }
    SCOPE.set(scope);
    !enabled || write(image() + VIEWPORT, &0u8)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_end_output() -> bool {
    end_output(image())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_finish_source() -> bool {
    finish_source(image())
}

pub(crate) fn is_busy() -> bool {
    OWNERS.load(Ordering::Acquire) != 0
}

pub(crate) fn owns_full_output() -> bool {
    SCOPE.get().output
}

pub(crate) fn refused() -> bool {
    SCOPE.get().changed
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_resolution_report(
    stage: u32,
    pass: *const rsf_game_render_pass,
    context: *mut c_void,
    published: bool,
    result: i32,
    report: Report,
) {
    let error = unsafe { GetLastError() };
    // The guarded C++ receipt leaf contains foreign exceptions. Restore the
    // caller's error after all receipt work, including early-refusal paths.
    unsafe { report_state(stage, pass, context, published, result, report) };
    unsafe { SetLastError(error) };
}

unsafe fn report_state(
    stage: u32,
    pass: *const rsf_game_render_pass,
    context: *mut c_void,
    published: bool,
    result: i32,
    report: Report,
) {
    use crate::resolution_stages::*;
    let name = match stage {
        RSF_FALLOUT4_STAGE_PRE_SR => c"pre-sr",
        RSF_FALLOUT4_STAGE_OUTPUT_READY => c"output-ready",
        RSF_FALLOUT4_STAGE_EVALUATED => c"evaluated",
        RSF_FALLOUT4_STAGE_EFFECTS_RETURNED => c"effects-return",
        RSF_FALLOUT4_STAGE_HUDLESS => c"hudless",
        RSF_FALLOUT4_STAGE_UI_RETURN => c"ui-return",
        RSF_FALLOUT4_STAGE_UI_RESTORED => c"ui-restored",
        RSF_FALLOUT4_STAGE_SOURCE_RETIRED => c"source-retired",
        _ => return,
    };
    let Some(report) = report else {
        return;
    };
    let pass = unsafe { pass.read() };
    if pass.source_frame_id == 0 {
        return;
    }
    let scope = SCOPE.get();
    if !in_receipt_budget(stage, &pass, &scope) {
        return;
    }
    let base = image();
    let mut receipt = Receipt {
        stage: name.as_ptr(),
        result,
        evaluation: stage == RSF_FALLOUT4_STAGE_EVALUATED,
        published,
        input: scope.input,
        output: scope.output,
        changed: scope.changed,
        input_ratio: scope.input_targets.ratios,
        ratio: [0.0; 2],
        raster: [0.0; 6],
        geometry: [0; 3],
        frame: 0,
        dirty: 0,
        viewport: 0,
        active: 0,
        partial: 0,
        scalars: false,
        owner: false,
        shadow: false,
        actual_count: 0,
        actual_read: 0,
        actual_raster: [0.0; 6],
    };
    receipt.scalars = collect_scalars(base, &mut receipt).is_some();
    receipt.owner = scope.current(base, true)
        && !context.is_null()
        && context as usize == scope.frame.device_context;
    receipt.shadow = receipt.owner && collect_shadow(&scope, &mut receipt).is_some();
    if receipt.owner {
        // SAFETY: the receipt above verified the calling frame and its borrowed D3D11 context.
        if let Some(context) = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) } {
            let (count, copied, raster) = crate::d3d11::viewport(context);
            receipt.actual_count = count;
            receipt.actual_read = u32::from(copied == 1);
            receipt.actual_raster = raster;
        }
    }
    unsafe { rsf_fallout4_resolution_report_receipt(report, &pass, context, &receipt) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_retry_waits_for_frame_advance_and_retires_the_input_scope() {
        let mut native = vec![0_u8; STATE + 0x9c];
        let base = native.as_mut_ptr() as usize;
        let native_frame = base + STATE + 0x98;
        assert!(write(native_frame, &7_u32));
        assert!(write(base + RATIOS, &0.5_f32));
        let count = OWNERS.fetch_add(1, Ordering::AcqRel);
        SCOPE.set(Scope {
            frame: FrameOwner {
                native_frame: 7,
                ..FrameOwner::default()
            },
            original_targets: TargetValues {
                ratios: [1.0; 2],
                active: 0,
            },
            input_targets: TargetValues {
                ratios: [0.5; 2],
                active: 1,
            },
            input_writes: TargetWrites {
                ratios: [true, false],
                active: false,
            },
            input: true,
            ..Scope::default()
        });
        retry_retired(base);
        assert!(SCOPE.get().input && SCOPE.get().input_writes.ratios[0]);

        assert!(write(native_frame, &8_u32));
        assert!(write(base + RATIOS, &0.75_f32));
        retry_retired(base);
        assert!(!SCOPE.get().input);
        assert_eq!(OWNERS.load(Ordering::Acquire), count);
        assert_eq!(read::<f32>(base + RATIOS), Some(0.75));
    }

    #[test]
    fn failed_restore_can_retire_a_superseded_value() {
        let mut pending = true;
        let mut changed = false;
        assert!(!restore_pending(
            &mut pending,
            &mut changed,
            1.0f32,
            0.5,
            true,
            || Some(0.5),
            |_| false
        ));
        assert!(pending && !changed);
        // Identical owned bits with a different renderer cannot authorize a stale write.
        assert!(!restore_pending(
            &mut pending,
            &mut changed,
            1.0f32,
            0.5,
            false,
            || Some(0.5),
            |_| { panic!("old renderer does not authorize write") }
        ));
        // A new native frame has since supplied different ratios. Retire without a write.
        assert!(restore_pending(
            &mut pending,
            &mut changed,
            1.0f32,
            0.5,
            false,
            || Some(0.75),
            |_| { panic!("superseding value must survive") }
        ));
        assert!(!pending && changed);
    }
}
