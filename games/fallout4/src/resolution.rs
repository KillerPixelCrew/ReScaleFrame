// SPDX-License-Identifier: MIT
//! Producer resolution and the synchronous full-output borrow for Fallout 4 AE 1.11.240.

use crate::cpu_abi::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::cpu_events::{
    rsf_fallout4_cpu_current_identity, rsf_fallout4_cpu_render_source_is_current,
};
use crate::depth_domains::{self, Leaves as DepthLeaves, Snapshot};
use crate::{Plain, image, read, screen::WORLD_LEASE_INVALID};
use rsf_game_sys::{
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_SR, rsf_game_host_services,
    rsf_game_render_config, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::System::{
    Diagnostics::Debug::WriteProcessMemory, Threading::GetCurrentProcess,
};

const STATE: usize = 0x3d871a0;
const TARGETS: usize = 0x2f4d750;
const RATIOS: usize = TARGETS + 0xfb8;
const VIEWPORT: usize = TARGETS + 0xfe4;
const ACTIVE: usize = TARGETS + 0xfe5;
const DISABLED: usize = TARGETS + 0xfe8;
const SHADOW: usize = 0x1b70;
const RASTER: usize = 0x90;
const MANAGER: usize = 0x3e5e4b0;
const SHADOW_VIEWPORT_DIRTY: u32 = 2;

type Reserve = Option<unsafe extern "C" fn(*const rsf_game_render_pass) -> bool>;
type Report = Option<unsafe extern "C" fn(*const c_char)>;

#[repr(C)]
struct Leaves {
    tls_slots: unsafe extern "C" fn() -> usize,
    screen_refusals: unsafe extern "C" fn(&CpuIdentity) -> u32,
    reconfigure: unsafe extern "C" fn(
        *const rsf_game_host_services,
        *mut c_void,
        &rsf_game_render_pass,
    ) -> bool,
    render_config:
        unsafe extern "C" fn(*const rsf_game_host_services, &mut rsf_game_render_config) -> bool,
    reserve: unsafe extern "C" fn(Reserve, &rsf_game_render_pass) -> bool,
    build_viewport: unsafe extern "C" fn(usize, usize, usize, u32, u32),
    report: unsafe extern "C" fn(Report, &rsf_game_render_pass, *mut c_void, &Receipt),
}

#[repr(C)]
struct Receipt {
    stage: *const c_char,
    geometry: [usize; 3],
    input_ratio: [f32; 2],
    ratio: [f32; 2],
    raster: [f32; 6],
    frame: u32,
    dirty: u32,
    result: i32,
    viewport: u8,
    active: u8,
    partial: u8,
    evaluation: bool,
    published: bool,
    input: bool,
    output: bool,
    changed: bool,
    scalars: bool,
    owner: bool,
    shadow: bool,
}

#[repr(C)]
#[derive(Default)]
struct OutputOwner {
    context: u64,
    manager: u64,
    triangles: [u64; 3],
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<OutputOwner>() == 40 && offset_of!(OutputOwner, triangles) == 16);
    assert!(size_of::<Receipt>() == 96 && offset_of!(Receipt, input_ratio) == 32);
    assert!(offset_of!(Receipt, frame) == 72 && offset_of!(Receipt, evaluation) == 87);
    assert!(size_of::<Leaves>() == 7 * size_of::<usize>());
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
        let renderer = base + 0x3a25d10;
        read::<usize>(base + 0x38e13a0) == Some(renderer)
            && read::<usize>(renderer + 0x58).map(|value| value as u64) == Some(self.window)
            && read::<usize>(renderer + 0x50) == Some(self.device_context)
            && read::<usize>(renderer + 0x70) == Some(self.chain)
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
    ) -> bool {
        let mut okay = true;
        for (index, pending) in self.ratios.iter_mut().enumerate() {
            okay &= restore(
                base + RATIOS + size_of::<f32>() * index,
                saved.ratios[index],
                owned.ratios[index],
                pending,
                changed,
            );
        }
        okay & restore(
            base + ACTIVE,
            saved.active,
            owned.active,
            &mut self.active,
            changed,
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
    fn current(&self, leaves: &Leaves, base: usize, shadow: bool) -> bool {
        (!shadow || native_context(leaves, base) == Some(self.context))
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

fn restore<T: Field>(
    address: usize,
    saved: T,
    owned: T,
    pending: &mut bool,
    changed: &mut bool,
) -> bool {
    if !*pending {
        return true;
    }
    let Some(current) = read::<T>(address) else {
        return false;
    };
    if current.bits() != saved.bits() {
        if current.bits() != owned.bits() {
            *changed = true;
        } else if !write(address, &saved) {
            return false;
        }
    }
    *pending = false;
    true
}

fn native_context(leaves: &Leaves, base: usize) -> Option<usize> {
    let index = read::<u32>(base + 0x3e73018).filter(|&index| index <= 4096)?;
    let slots = unsafe { (leaves.tls_slots)() };
    if slots == 0 {
        return None;
    }
    let block = read::<usize>(slots.wrapping_add(index as usize * size_of::<usize>()))
        .filter(|&value| value != 0)?;
    match read::<usize>(block.wrapping_add(0xb20))? {
        0 => read::<usize>(base + 0x38e1398).filter(|&value| value != 0),
        context => Some(context),
    }
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
    leaves: &Leaves,
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
    if !unsafe { (leaves.reconfigure)(host, context, &planning) } {
        return None;
    }
    let mut config = rsf_game_render_config {
        struct_size: size_of::<rsf_game_render_config>() as u32,
        enabled: 0,
        output_width: 0,
        output_height: 0,
        render_width: 0,
        render_height: 0,
    };
    if !unsafe { (leaves.render_config)(host, &mut config) } {
        return None;
    }
    let render = [config.render_width, config.render_height];
    (config.enabled != 0
        && [config.output_width, config.output_height] == size
        && render[0] != 0
        && render[1] != 0
        && render[0] <= size[0]
        && render[1] <= size[1])
        .then_some(render)
}

fn preflight(
    leaves: &Leaves,
    base: usize,
    pass: &rsf_game_render_pass,
    context: *mut c_void,
) -> Option<OutputOwner> {
    if context.is_null() {
        return None;
    }
    let engine = native_context(leaves, base)?;
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
    leaves: &Leaves,
    host: *const rsf_game_host_services,
    frame: *const rsf_game_render_pass,
    context: *mut c_void,
    reserve: Reserve,
) -> Option<()> {
    let current = SCOPE.get();
    if current.input
        || current.output
        || context.is_null()
        || unsafe { (*host).render_config.is_none() || (*host).process_upscale.is_none() }
    {
        return None;
    }
    let mut cpu = CpuIdentity {
        session: 0,
        source: 0,
        submission: 0,
        began_qpc: 0,
        frequency: 0,
    };
    unsafe { rsf_fallout4_cpu_current_identity(&mut cpu) };
    let pass = unsafe { frame.read() };
    if (cpu.session, cpu.source, cpu.submission)
        != (pass.session_id, pass.source_frame_id, pass.submission_id)
        || !unsafe { rsf_fallout4_cpu_render_source_is_current(&cpu) }
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
    let render = configured_targets(leaves, host, &pass, context, size)?;
    if !unsafe { rsf_fallout4_cpu_render_source_is_current(&cpu) }
        || unsafe { (leaves.screen_refusals)(&cpu) } != WORLD_LEASE_INVALID
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
    scope.context = native_context(leaves, base)?;
    SCOPE.set(scope);
    if !scope.current(leaves, base, false) {
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
        if reserve.is_none()
            || preflight(leaves, base, &planning, context).is_none()
            || !unsafe { (leaves.reserve)(reserve, &planning) }
        {
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
        if !finish_source(leaves, base) {
            let mut pending = SCOPE.get();
            pending.changed = true;
            SCOPE.set(pending);
        }
        return None;
    }
    Some(())
}

fn begin_output(
    leaves: &Leaves,
    depth: &DepthLeaves,
    snapshot: &Snapshot,
    pass: &rsf_game_render_pass,
) -> Option<()> {
    if SCOPE.get().output {
        return None;
    }
    let (context, device_context, chain) = depth_domains::resolution_owner(depth, snapshot)?;
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
    if !scope.current(leaves, base, true) || read::<u32>(base + DISABLED) != Some(0) {
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
        end_output(leaves, base);
        return None;
    }
    full.raster_owned = true;
    full.dirty_owned = true;
    SCOPE.set(scope);

    // Supply explicit output dimensions through the native builder, independent of the last target.
    let [width, height] = scope.frame.size;
    unsafe { (leaves.build_viewport)(base + 0x1817ef0, base + 0x3a25d00, shadow, width, height) };
    scope = SCOPE.get();
    let owned_raster = read::<[f32; 6]>(shadow.wrapping_add(RASTER));
    if let Some(raster) = owned_raster {
        scope.full.owned_raster = raster;
        SCOPE.set(scope);
    }
    if owned_raster.is_some_and(|raster| {
        let [width, height] = scope.frame.size;
        scope.full.geometry.current(base) && raster[..4] == [0.0, 0.0, width as f32, height as f32]
    }) {
        return Some(());
    }
    end_output(leaves, base);
    None
}

fn native_dr(leaves: &Leaves, base: usize, scope: &mut Scope) -> Option<()> {
    if !scope.input && !scope.output {
        return Some(());
    }
    if !scope.current(leaves, base, scope.output) {
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

fn end_output(leaves: &Leaves, base: usize) -> bool {
    let mut scope = SCOPE.get();
    if !scope.output {
        return !scope.changed;
    }
    if !scope.current(leaves, base, true) {
        return false;
    }
    let shadow = scope.shadow();
    let full = &mut scope.full;
    if !full.geometry.current(base) {
        scope.changed = true;
    }
    let mut okay =
        full.target_writes
            .restore(base, full.targets, TargetValues::FULL, &mut scope.changed);
    okay &= restore(
        base + VIEWPORT,
        full.viewport,
        0,
        &mut full.viewport_owned,
        &mut scope.changed,
    );
    okay &= restore(
        base + STATE + 4,
        full.jitter,
        [0.0; 2],
        &mut full.jitter_owned,
        &mut scope.changed,
    );
    if full.native_full_viewport {
        full.raster_owned = false;
        full.dirty_owned = false;
    }
    okay &= restore(
        shadow.wrapping_add(RASTER),
        full.raster,
        full.owned_raster,
        &mut full.raster_owned,
        &mut scope.changed,
    );
    if full.dirty_owned {
        if read::<u32>(shadow).is_some_and(|dirty| {
            write(
                shadow,
                &((dirty & !SHADOW_VIEWPORT_DIRTY) | (full.dirty & SHADOW_VIEWPORT_DIRTY)),
            )
        }) {
            full.dirty_owned = false;
        } else {
            okay = false;
        }
    }
    if okay {
        scope.output = false;
        OWNERS.fetch_sub(1, Ordering::AcqRel);
    }
    SCOPE.set(scope);
    okay && !scope.changed
}

fn finish_source(leaves: &Leaves, base: usize) -> bool {
    let valid = end_output(leaves, base);
    let mut scope = SCOPE.get();
    if scope.output {
        return false;
    }
    if !scope.input {
        SCOPE.set(Scope::default());
        return valid;
    }
    if !scope.current(leaves, base, false) {
        return false;
    }
    let okay = scope.input_writes.restore(
        base,
        scope.original_targets,
        scope.input_targets,
        &mut scope.changed,
    );
    let accepted = okay && valid && !scope.changed;
    if okay {
        OWNERS.fetch_sub(1, Ordering::AcqRel);
        scope = Scope::default();
    }
    SCOPE.set(scope);
    accepted
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
extern "C" fn rsf_fallout4_resolution_verify() -> bool {
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
        && guard(base, 0x1817ef0, b"\x40\x53\x48\x83\xec\x60")
        && guard(base, 0x1817f1f, b"\x45\x85\xc0\x74\x10\x41\x8b\xc0")
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_inputs(pass: &rsf_game_render_pass) -> bool {
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
    leaves: &Leaves,
    host: *const rsf_game_host_services,
    frame: *const rsf_game_render_pass,
    context: *mut c_void,
    reserve: Reserve,
) -> bool {
    unsafe { configure(leaves, host, frame, context, reserve) }.is_some()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_resolution_preflight(
    leaves: &Leaves,
    pass: &rsf_game_render_pass,
    context: *mut c_void,
    output: *mut OutputOwner,
) -> bool {
    if let Some(output) = unsafe { output.as_mut() } {
        *output = OutputOwner::default();
    }
    let Some(owner) = preflight(leaves, image(), pass, context) else {
        return false;
    };
    if let Some(output) = unsafe { output.as_mut() } {
        *output = owner;
    }
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_begin_output(
    leaves: &Leaves,
    depth: &DepthLeaves,
    snapshot: &Snapshot,
    pass: &rsf_game_render_pass,
) -> bool {
    begin_output(leaves, depth, snapshot, pass).is_some()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_native_dr(leaves: &Leaves) -> bool {
    let mut scope = SCOPE.get();
    let okay = native_dr(leaves, image(), &mut scope).is_some();
    SCOPE.set(scope);
    okay
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_native_viewport(enabled: bool) -> bool {
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
extern "C" fn rsf_fallout4_resolution_end_output(leaves: &Leaves) -> bool {
    end_output(leaves, image())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_finish_source(leaves: &Leaves) -> bool {
    finish_source(leaves, image())
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fallout4_resolution_busy() -> bool {
    OWNERS.load(Ordering::Acquire) != 0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_output_busy() -> bool {
    SCOPE.get().output
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_resolution_refused() -> bool {
    SCOPE.get().changed
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_resolution_report(
    leaves: &Leaves,
    stage: u32,
    pass: *const rsf_game_render_pass,
    context: *mut c_void,
    published: bool,
    result: i32,
    report: Report,
) {
    const STAGES: [&std::ffi::CStr; 8] = [
        c"pre-sr",
        c"output-ready",
        c"evaluated",
        c"effects-return",
        c"hudless",
        c"ui-return",
        c"ui-restored",
        c"source-retired",
    ];
    let Some(name) = STAGES.get(stage as usize) else {
        return;
    };
    if report.is_none() {
        return;
    }
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
        evaluation: stage == 2,
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
    };
    receipt.scalars = collect_scalars(base, &mut receipt).is_some();
    receipt.owner = scope.current(leaves, base, true)
        && !context.is_null()
        && context as usize == scope.frame.device_context;
    receipt.shadow = receipt.owner && collect_shadow(&scope, &mut receipt).is_some();
    unsafe { (leaves.report)(report, &pass, context, &receipt) };
}
