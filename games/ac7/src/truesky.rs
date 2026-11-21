// SPDX-License-Identifier: MIT
//! TrueSky plugin patches and call relays of the native renderer: the exact sites, expected
//! bytes and effect fingerprint, their admission, relay installation and rollback, the full cloud
//! grid and its refusal, and the retryable retirement.
//!
//! The sites belong to TrueSkyPluginRender_MT.dll (SHA-256 c6c58ea3...1004d0, 1,711,616 bytes)
//! with mixed_resolution.fxo (45,263 bytes, CRC-32 78881ef3). Every RVA, byte and log text was
//! moved unchanged from native_renderer.cpp. Research: docs/research/ue418-hook-map.md,
//! .agents/skills/truesky-render-integration/references/ac7-depth.md and
//! docs/research/rust-ac7-truesky-relays.md.
//!
//! The ownership record (`rsf_ac7_truesky_patches`) lives in the native renderer. Only this module
//! changes it, always under the renderer's cloud resolution lock. native_renderer.cpp keeps the
//! relay handlers with their D3D11 binding checks and one-to-one depth shader, the module lookup,
//! the relay memory and the protected code writes, and lends them through
//! `rsf_ac7_truesky_leaves`.
use crate::guards::rsf_ac7_guards_held;
use crate::platform_abi::rsf_ac7_native_renderer;
use crate::sites::{Code, NativeCode};
use crate::sites_abi::{
    RSF_AC7_RELAY_BYTES, rsf_ac7_call_relay, rsf_ac7_truesky_leaves, rsf_ac7_truesky_patches,
};
use std::ffi::c_void;
use std::io::Read;
use std::ptr::null_mut;

const RELAY: usize = RSF_AC7_RELAY_BYTES as usize;

const fn wide<const N: usize>(text: &[u8; N]) -> [u16; N] {
    let mut out = [0; N];
    let mut index = 0;
    while index < N {
        out[index] = text[index] as u16;
        index += 1;
    }
    out
}

/// The plugin module as GetModuleHandleW names it, terminated.
pub(crate) const MODULE: [u16; 27] = wide(b"TrueSkyPluginRender_MT.dll\0");

/// The shipped mixed-resolution effect beside the executable, by exact size and CRC-32 (IEEE).
/// Its x2/x3/x4 depth permutations are why the full grid needs the first-party x1 kernel.
pub(crate) const EFFECT_PATH: &str =
    r"Engine\Plugins\TrueSkyPlugin\shaderbin\Win64\mixed_resolution.fxo";
pub(crate) const EFFECT_BYTES: usize = 45263;
pub(crate) const EFFECT_CRC: u32 = 0x7888_1ef3;

/// RenderInterfaceViewFrame passes q.downscale into the native per-view allocator. Patching this
/// consumer also covers settings the actor or a sequence writes after startup: the load
/// `mov eax,[rdi+0x210]` becomes divisor 1, `mov eax,1; nop`.
pub(crate) const RESOLUTION_RVA: u32 = 0x87f6d;
pub(crate) const RESOLUTION_EXPECTED: [u8; 16] = [
    0x8b, 0x87, 0x10, 0x02, 0x00, 0x00, 0x89, 0x44, 0x24, 0x38, 0x48, 0x8b, 0x45, 0xd0, 0x0f, 0x29,
];
pub(crate) const NATIVE_DIVISOR: [u8; 6] = [0x8b, 0x87, 0x10, 0x02, 0x00, 0x00];
pub(crate) const FULL_DIVISOR: [u8; 6] = [0xb8, 0x01, 0x00, 0x00, 0x00, 0x90];

/// The depth bounds producer's Dispatch, `call [r10+0x148]`, relayed through its 7 bytes to the
/// handler that runs the first-party x1 kernel on verified full-grid bindings.
pub(crate) const DEPTH_CALL_RVA: u32 = 0xc003f;
pub(crate) const DEPTH_CALL_EXPECTED: [u8; 16] = [
    0x41, 0xff, 0x92, 0x48, 0x01, 0x00, 0x00, 0x48, 0x8b, 0x4b, 0x08, 0x4c, 0x8d, 0x83, 0xb0, 0x01,
];
pub(crate) const DEPTH_CALL_BYTES: usize = 7;

/// The mixed-resolution pass selector `cmp r12d,2; jne`, compared from its `cmp`. `jne` (75)
/// becomes `ja` (77): divisors 1 and 2 both enter the native x2 binding, Apply and cleanup, which
/// divisor 1 otherwise skipped before its unbalanced Dispatch and Unapply; x3/x4 keep their paths.
pub(crate) const DEPTH_BRANCH_RVA: u32 = 0xbff0b;
pub(crate) const DEPTH_BRANCH_LEAD: usize = 4;
pub(crate) const DEPTH_BRANCH_EXPECTED: [u8; 16] = [
    0x41, 0x83, 0xfc, 0x02, 0x75, 0x1d, 0x48, 0x8b, 0x4b, 0x08, 0x4c, 0x8d, 0x43, 0x18, 0x48, 0x8b,
];
pub(crate) const NATIVE_BRANCH: [u8; 1] = [0x75];
pub(crate) const SUPPORTED_BRANCH: [u8; 1] = [0x77];

/// The normalized scene-depth bounds format request. Only its immediate changes, TrueSky format
/// 22 (R16G16_UNORM) to 5 (R32G32_FLOAT); the native ensure2D owner then rebuilds the texture and
/// both views together. It belongs to depth bounds production, not cloud colour or scene Z.
pub(crate) const DEPTH_FORMAT_RVA: u32 = 0xbfc05;
pub(crate) const DEPTH_FORMAT_IMMEDIATE: usize = 4;
pub(crate) const DEPTH_FORMAT_EXPECTED: [u8; 16] = [
    0xc7, 0x44, 0x24, 0x20, 0x16, 0x00, 0x00, 0x00, 0xff, 0x50, 0x58, 0x8b, 0x87, 0x94, 0x00, 0x00,
];
pub(crate) const NATIVE_DEPTH_FORMAT: [u8; 4] = [0x16, 0x00, 0x00, 0x00];
pub(crate) const PRECISE_DEPTH_FORMAT: [u8; 4] = [0x05, 0x00, 0x00, 0x00];

/// composite_tile reaches the render platform with `call [rax+0x130]` (platform, deviceContext&,
/// count), relayed through its 6 bytes and compared over the 16 bytes before and from it.
pub(crate) const MOTION_CALL_RVA: u32 = 0xabc6b;
pub(crate) const MOTION_BEFORE: [u8; 16] = [
    0x49, 0x8b, 0x8e, 0x28, 0x02, 0x00, 0x00, 0x45, 0x8b, 0xc4, 0x49, 0x8b, 0xd7, 0x48, 0x8b, 0x01,
];
pub(crate) const MOTION_CALL_EXPECTED: [u8; 16] = [
    0xff, 0x90, 0x30, 0x01, 0x00, 0x00, 0x49, 0x8b, 0x9e, 0x60, 0x02, 0x00, 0x00, 0x48, 0x8d, 0x0d,
];
pub(crate) const MOTION_CALL_BYTES: usize = 6;

pub(crate) const MOTION_REFUSED: &str =
    "native TrueSky cloud motion refused: composite_tile draw bytes differ at DLL RVA 0xabc6b";
pub(crate) const MOTION_INSTALLED: &str =
    "native TrueSky cloud motion: composite_tile draw relay installed at DLL RVA 0xabc6b";
pub(crate) const DEPTH_FORMAT_REFUSED: &str = "native TrueSky precise depth allocation refused at DLL RVA 0xbfc09; full cloud grid remains disabled";
pub(crate) const DEPTH_FORMAT: &str = "native TrueSky: normalized scene-depth bounds use R32G32_FLOAT at DLL RVA 0xbfc09; native owner rebuilds matching texture/views";
pub(crate) const DEPTH_INSTALLED: &str = "native TrueSky depth: guarded one-to-one pass route installed at DLL RVAs 0xbff0b/0xc003f; waiting for GPU shader preparation";
pub(crate) const DEPTH_REFUSED: &str = "native TrueSky full-resolution patch refused: depth producer code/effect fingerprint or relay allocation differs";
pub(crate) const RESOLUTION_REFUSED: &str = "native TrueSky cloud-resolution patch refused: expected bytes differ or code protection failed at DLL RVA 0x87f6d";
pub(crate) const RESOLUTION: &str =
    "native TrueSky: cloud/depth producer uses scene resolution, divisor 1 at DLL RVA 0x87f6d";
pub(crate) const FULL_GRID_RETIRED: &str = "native TrueSky: full-grid depth bindings changed; unsafe dispatch refused, supported cloud grid restored for next native frame";
pub(crate) const FULL_GRID_RETAINED: &str = "native TrueSky: full-grid depth bindings changed and divisor restore failed; unsafe dispatch refused, renderer retained";
pub(crate) const RESOLUTION_KEPT: &str =
    "native TrueSky cloud-resolution patch could not restore; renderer retained";
pub(crate) const MOTION_KEPT: &str =
    "native TrueSky cloud motion relay could not restore; renderer retained";
pub(crate) const DEPTH_CALL_KEPT: &str =
    "native TrueSky depth dispatch could not restore; renderer retained";
pub(crate) const DEPTH_FORMAT_KEPT: &str =
    "native TrueSky depth format could not restore; renderer retained";
pub(crate) const DEPTH_BRANCH_KEPT: &str =
    "native TrueSky depth pass selector could not restore; renderer retained";
pub(crate) const RELAY_RUNNING: &str = "native TrueSky relays restored while a relay call was still running; relay memory and cloud resources retained for the next stop";

/// The native handler a relay reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handler {
    DepthDispatch,
    CompositeDraw,
}

/// The native side of one TrueSky call. `Code` compares and writes plugin code and logs to the
/// renderer's host log.
pub(crate) trait Plugin: Code {
    /// Base of TrueSkyPluginRender_MT.dll, or null while it is not loaded.
    fn module(&self) -> *mut u8;
    /// Whether mixed_resolution.fxo has the fingerprinted size and CRC.
    fn effect(&self) -> bool;
    /// Relay memory within rel32 search range of `call` holding `handler`, or null.
    fn relay_allocate(&self, call: *mut u8, handler: Handler) -> *mut c_void;
    fn relay_release(&self, slot: *mut c_void);
    /// Whether the depth relay verified its one-to-one shader and bindings on the device.
    fn depth_ready(&self) -> bool;
    fn motion_release(&self);
    fn depth_release(&self);
    /// Hook and relay bodies running, entry and outer guards (guards.rs).
    fn guards_held(&self) -> u32;
}

pub(crate) const EMPTY_RELAY: rsf_ac7_call_relay = rsf_ac7_call_relay {
    call: null_mut(),
    slot: null_mut(),
    native: [0; RELAY],
    relayed: [0; RELAY],
    size: 0,
};

/// CRC-32 (IEEE, reflected 0xedb88320), as the former bytewise loop computed it.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |crc, byte| {
        (0..8).fold(crc ^ u32::from(*byte), |crc, _| {
            (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1))
        })
    })
}

pub(crate) fn effect_matches(bytes: &[u8]) -> bool {
    bytes.len() == EFFECT_BYTES && crc32(bytes) == EFFECT_CRC
}

/// The effect beside the process executable (GetModuleFileNameW of the process). A longer file
/// reads one byte past the size and differs.
fn effect_file() -> bool {
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let Some(directory) = executable.parent() else {
        return false;
    };
    let Ok(file) = std::fs::File::open(directory.join(EFFECT_PATH)) else {
        return false;
    };
    let mut bytes = Vec::with_capacity(EFFECT_BYTES + 1);
    file.take(EFFECT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_ok()
        && effect_matches(&bytes)
}

/// The native and relayed bytes of an owned relay.
fn relay_bytes(relay: &rsf_ac7_call_relay) -> (&[u8], &[u8]) {
    let size = (relay.size as usize).min(RELAY);
    (&relay.native[..size], &relay.relayed[..size])
}

/// The relay for the `native` call (6 or 7 bytes) at `call`: a slot holding the handler and the
/// `call [rip+rel32]` that reaches it, padded with `nop`. Writes nothing to the code yet.
fn prepare_relay(
    plugin: &impl Plugin,
    call: *mut u8,
    native: &[u8],
    handler: Handler,
) -> Option<rsf_ac7_call_relay> {
    if !(6..=RELAY).contains(&native.len()) {
        return None;
    }
    let slot = plugin.relay_allocate(call, handler);
    if slot.is_null() {
        return None;
    }
    // rel32 counts from the end of the 6-byte `ff 15` form.
    let distance = slot.addr() as i128 - (call.addr() as i128 + 6);
    let Ok(relative) = i32::try_from(distance) else {
        plugin.relay_release(slot);
        return None;
    };
    let mut relay = rsf_ac7_call_relay {
        call,
        slot,
        size: native.len() as u32,
        ..EMPTY_RELAY
    };
    relay.native[..native.len()].copy_from_slice(native);
    relay.relayed[..2].copy_from_slice(&[0xff, 0x15]);
    relay.relayed[2..6].copy_from_slice(&relative.to_le_bytes());
    relay.relayed[6] = 0x90;
    Some(relay)
}

/// Redirects the call. A refused write releases the slot.
fn patch_relay(plugin: &impl Plugin, relay: &rsf_ac7_call_relay) -> bool {
    let (native, relayed) = relay_bytes(relay);
    if plugin.write(relay.call, native, relayed) {
        return true;
    }
    plugin.relay_release(relay.slot);
    false
}

/// Puts the native call back and forgets it. The slot stays until no relay call can still be
/// running (`restore`).
fn restore_call(plugin: &impl Plugin, relay: &mut rsf_ac7_call_relay) -> bool {
    if relay.call.is_null() {
        return true;
    }
    let (native, relayed) = relay_bytes(relay);
    if !plugin.write(relay.call, relayed, native) {
        return false;
    }
    relay.call = null_mut();
    true
}

/// Frees an owned slot and forgets the relay; false when there was none.
fn release_slot(plugin: &impl Plugin, relay: &mut rsf_ac7_call_relay) -> bool {
    if relay.slot.is_null() {
        return false;
    }
    plugin.relay_release(relay.slot);
    *relay = EMPTY_RELAY;
    true
}

/// The composite_tile draw relay that supplies cloud motion depth. A refusal is logged and tried
/// again on the next sky render, as before.
fn install_motion(state: &mut rsf_ac7_truesky_patches, module: *mut u8, plugin: &impl Plugin) {
    if !state.motion_call.slot.is_null() {
        return;
    }
    let call = module.wrapping_add(MOTION_CALL_RVA as usize);
    if !plugin.matches(call.wrapping_sub(MOTION_BEFORE.len()), &MOTION_BEFORE)
        || !plugin.matches(call, &MOTION_CALL_EXPECTED)
    {
        plugin.log(MOTION_REFUSED);
        return;
    }
    let Some(relay) = prepare_relay(
        plugin,
        call,
        &MOTION_CALL_EXPECTED[..MOTION_CALL_BYTES],
        Handler::CompositeDraw,
    ) else {
        return;
    };
    if !patch_relay(plugin, &relay) {
        return;
    }
    state.motion_call = relay;
    plugin.log(MOTION_INSTALLED);
}

/// The guarded one-to-one depth route: code and effect fingerprint, then the pass selector, the
/// dispatch relay and the precise format. True once all three are owned.
fn install_depth(
    state: &mut rsf_ac7_truesky_patches,
    module: *mut u8,
    plugin: &impl Plugin,
) -> bool {
    if !state.depth_call.slot.is_null() {
        return !state.depth_format_site.is_null();
    }
    let call = module.wrapping_add(DEPTH_CALL_RVA as usize);
    let branch = module.wrapping_add(DEPTH_BRANCH_RVA as usize);
    let format = module.wrapping_add(DEPTH_FORMAT_RVA as usize);
    if !plugin.matches(call, &DEPTH_CALL_EXPECTED)
        || !plugin.matches(
            branch.wrapping_sub(DEPTH_BRANCH_LEAD),
            &DEPTH_BRANCH_EXPECTED,
        )
        || !plugin.matches(format, &DEPTH_FORMAT_EXPECTED)
        || !plugin.effect()
    {
        return false;
    }
    let Some(relay) = prepare_relay(
        plugin,
        call,
        &DEPTH_CALL_EXPECTED[..DEPTH_CALL_BYTES],
        Handler::DepthDispatch,
    ) else {
        return false;
    };
    if !plugin.write(branch, &NATIVE_BRANCH, &SUPPORTED_BRANCH) {
        plugin.relay_release(relay.slot);
        return false;
    }
    if !patch_relay(plugin, &relay) {
        // The former rollback ignored a refused selector restore and left it patched and unowned.
        // Keep it owned so stop restores it.
        if !plugin.write(branch, &SUPPORTED_BRANCH, &NATIVE_BRANCH) {
            state.depth_branch = branch;
        }
        return false;
    }
    state.depth_call = relay;
    state.depth_branch = branch;
    let site = format.wrapping_add(DEPTH_FORMAT_IMMEDIATE);
    if !plugin.write(site, &NATIVE_DEPTH_FORMAT, &PRECISE_DEPTH_FORMAT) {
        plugin.log(DEPTH_FORMAT_REFUSED);
        return false;
    }
    state.depth_format_site = site;
    plugin.log(DEPTH_FORMAT);
    plugin.log(DEPTH_INSTALLED);
    true
}

/// Per sky render while active. The first depth pass runs at its supported native divisor; the
/// full grid is enabled only after the replacement kernel has compiled and its bindings were
/// verified on the actual device. A refusal keeps stock cloud resolution for this renderer.
pub(crate) fn apply(state: &mut rsf_ac7_truesky_patches, plugin: &impl Plugin) {
    if !state.resolution_site.is_null() || state.resolution_refused {
        return;
    }
    let module = plugin.module();
    if module.is_null() {
        return;
    }
    install_motion(state, module, plugin);
    if !install_depth(state, module, plugin) {
        state.resolution_refused = true;
        plugin.log(DEPTH_REFUSED);
        return;
    }
    if !plugin.depth_ready() {
        return;
    }
    let site = module.wrapping_add(RESOLUTION_RVA as usize);
    if !plugin.matches(site, &RESOLUTION_EXPECTED)
        || !plugin.write(site, &NATIVE_DIVISOR, &FULL_DIVISOR)
    {
        state.resolution_refused = true;
        plugin.log(RESOLUTION_REFUSED);
        return;
    }
    state.resolution_site = site;
    plugin.log(RESOLUTION);
}

/// Readiness of an earlier frame does not authorize x2 on this frame's x1 targets. Full-grid mode
/// retires for the next native allocation; the caller rejects the unverified dispatch and the
/// enclosing native function still performs its balanced Unapply.
pub(crate) fn refuse_full_grid(state: &mut rsf_ac7_truesky_patches, plugin: &impl Plugin) {
    state.resolution_refused = true;
    if state.resolution_site.is_null() {
        return;
    }
    if plugin.write(state.resolution_site, &FULL_DIVISOR, &NATIVE_DIVISOR) {
        state.resolution_site = null_mut();
        plugin.log(FULL_GRID_RETIRED);
    } else {
        plugin.log(FULL_GRID_RETAINED);
    }
}

/// Retirement in the former order: divisor, motion relay, depth relay, format, selector. Each
/// step forgets only what it restored, so a refused stop continues on the next one. The relay
/// memory and the cloud resources go last, once no relay call can still be using them.
pub(crate) fn restore(state: &mut rsf_ac7_truesky_patches, plugin: &impl Plugin) -> bool {
    if !state.resolution_site.is_null() {
        if !plugin.write(state.resolution_site, &FULL_DIVISOR, &NATIVE_DIVISOR) {
            plugin.log(RESOLUTION_KEPT);
            return false;
        }
        state.resolution_site = null_mut();
    }
    if !restore_call(plugin, &mut state.motion_call) {
        plugin.log(MOTION_KEPT);
        return false;
    }
    if !restore_call(plugin, &mut state.depth_call) {
        plugin.log(DEPTH_CALL_KEPT);
        return false;
    }
    if !state.depth_format_site.is_null() {
        if !plugin.write(
            state.depth_format_site,
            &PRECISE_DEPTH_FORMAT,
            &NATIVE_DEPTH_FORMAT,
        ) {
            plugin.log(DEPTH_FORMAT_KEPT);
            return false;
        }
        state.depth_format_site = null_mut();
    }
    if !state.depth_branch.is_null() {
        if !plugin.write(state.depth_branch, &SUPPORTED_BRANCH, &NATIVE_BRANCH) {
            plugin.log(DEPTH_BRANCH_KEPT);
            return false;
        }
        state.depth_branch = null_mut();
    }
    // The relays are not MinHook hooks, so stop's disable did not stop them, and the depth
    // handler does not check activity. A handler that took its relay before the call was
    // restored may still read its slot, the depth shader or the motion resources.
    if plugin.guards_held() != 0 {
        plugin.log(RELAY_RUNNING);
        return false;
    }
    if release_slot(plugin, &mut state.motion_call) {
        plugin.motion_release();
    }
    release_slot(plugin, &mut state.depth_call);
    plugin.depth_release();
    true
}

/// The validated leaf table of one call.
struct NativePlugin<'a> {
    code: NativeCode<'a>,
    renderer: *mut rsf_ac7_native_renderer,
    module: unsafe extern "C" fn(*const u16) -> *mut u8,
    relay_allocate: unsafe extern "C" fn(*mut u8, *mut c_void) -> *mut c_void,
    relay_release: unsafe extern "C" fn(*mut c_void),
    depth_dispatch: *mut c_void,
    composite_draw: *mut c_void,
    depth_ready: unsafe extern "C" fn(*mut rsf_ac7_native_renderer) -> bool,
    motion_release: unsafe extern "C" fn(*mut rsf_ac7_native_renderer),
    depth_release: unsafe extern "C" fn(*mut rsf_ac7_native_renderer),
}

impl NativePlugin<'_> {
    /// The table when every leaf, both handlers and the renderer are present.
    ///
    /// # Safety
    /// `leaves` is null or valid for the call, and its leaves keep the contracts of ac7_sites.h.
    unsafe fn new(leaves: *const rsf_ac7_truesky_leaves) -> Option<Self> {
        let leaves = unsafe { leaves.as_ref() }?;
        if leaves.renderer.is_null()
            || leaves.depth_dispatch.is_null()
            || leaves.composite_draw.is_null()
        {
            return None;
        }
        Some(Self {
            code: unsafe { NativeCode::new(&leaves.code) }?,
            renderer: leaves.renderer,
            module: leaves.module?,
            relay_allocate: leaves.relay_allocate?,
            relay_release: leaves.relay_release?,
            depth_dispatch: leaves.depth_dispatch,
            composite_draw: leaves.composite_draw,
            depth_ready: leaves.depth_ready?,
            motion_release: leaves.motion_release?,
            depth_release: leaves.depth_release?,
        })
    }
}

impl Code for NativePlugin<'_> {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        self.code.matches(address, expected)
    }
    fn write(&self, site: *mut u8, expected: &[u8], replacement: &[u8]) -> bool {
        self.code.write(site, expected, replacement)
    }
    fn log(&self, message: &str) {
        self.code.log(message);
    }
}

// SAFETY, for every call below: the leaves were lent for this call with the contracts of
// ac7_sites.h, and the renderer is the live one whose record is being changed.
impl Plugin for NativePlugin<'_> {
    fn module(&self) -> *mut u8 {
        unsafe { (self.module)(MODULE.as_ptr()) }
    }
    fn effect(&self) -> bool {
        effect_file()
    }
    fn relay_allocate(&self, call: *mut u8, handler: Handler) -> *mut c_void {
        let handler = match handler {
            Handler::DepthDispatch => self.depth_dispatch,
            Handler::CompositeDraw => self.composite_draw,
        };
        unsafe { (self.relay_allocate)(call, handler) }
    }
    fn relay_release(&self, slot: *mut c_void) {
        unsafe { (self.relay_release)(slot) }
    }
    fn depth_ready(&self) -> bool {
        unsafe { (self.depth_ready)(self.renderer) }
    }
    fn motion_release(&self) {
        unsafe { (self.motion_release)(self.renderer) }
    }
    fn depth_release(&self) {
        unsafe { (self.depth_release)(self.renderer) }
    }
    fn guards_held(&self) -> u32 {
        rsf_ac7_guards_held(true)
    }
}

/// The record and leaves of one native call.
///
/// # Safety
/// `patches` and `leaves` are null or valid for the call, and the caller holds the renderer's
/// cloud resolution lock, so nothing else uses `patches` during it.
unsafe fn native<'a>(
    patches: *mut rsf_ac7_truesky_patches,
    leaves: *const rsf_ac7_truesky_leaves,
) -> Option<(&'a mut rsf_ac7_truesky_patches, NativePlugin<'a>)> {
    let patches = unsafe { patches.as_mut() }?;
    let plugin = unsafe { NativePlugin::new(leaves) }?;
    Some((patches, plugin))
}

/// # Safety
/// See `native`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_truesky_apply(
    patches: *mut rsf_ac7_truesky_patches,
    leaves: *const rsf_ac7_truesky_leaves,
) {
    if let Some((patches, plugin)) = unsafe { native(patches, leaves) } {
        apply(patches, &plugin);
    }
}

/// # Safety
/// See `native`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_truesky_refuse_full_grid(
    patches: *mut rsf_ac7_truesky_patches,
    leaves: *const rsf_ac7_truesky_leaves,
) {
    if let Some((patches, plugin)) = unsafe { native(patches, leaves) } {
        refuse_full_grid(patches, &plugin);
    }
}

/// # Safety
/// See `native`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_truesky_restore(
    patches: *mut rsf_ac7_truesky_patches,
    leaves: *const rsf_ac7_truesky_leaves,
) -> bool {
    unsafe { native(patches, leaves) }.is_some_and(|(patches, plugin)| restore(patches, &plugin))
}

#[cfg(test)]
mod tests;
