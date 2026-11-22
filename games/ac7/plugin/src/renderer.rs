// SPDX-License-Identifier: MIT
//! The native renderer transaction behind the lifecycle's five renderer calls: prepare, start,
//! is_active, quiesce and stop.
//!
//! native_renderer/leaves.rs (formerly native_renderer.cpp) holds the native leaves: the renderer object and its fields, MinHook, module pinning, the
//! render scopes, the input hook, the controller facts and the TrueSky restore, which takes the
//! renderer's cloud resolution lock and calls truesky.rs. This module decides their order and
//! refusals. It reuses the existing owners and their state:
//! admission and the hook table are sites.rs, the engine patches patches.rs with the renderer's
//! `patches` record, the hook body counts guards.rs, and the installed pointer, flags and MinHook
//! records stay in the renderer.
//!
//! Two rules differ from the former C++ so that a partial activation and a retried stop are safe:
//! - A failed installation disables every hook it created, and removes them only once no hook
//!   body is running. While one is, the hooks stay created with their trampolines, so a body
//!   that has not yet called its original still reaches it, and the renderer, its options
//!   callbacks and the installed pointer stay reachable in the pinned module. The former rollback
//!   removed the hooks, freeing the trampolines, before it looked at the guards.
//! - Retirement disables and removes the hooks that are created, which a removal clears, instead
//!   of every bound target, so a retried stop or a stop after a partial activation removes
//!   exactly what is still owned.
use crate::guards::rsf_ac7_guards_held;
use crate::lifecycle::Renderer;
use crate::native_renderer::leaves;
use crate::patches;
use crate::sites::{self, Code, CodeView, HOOK_SITES, NativeCode};
use crate::sys::rsf_ac7_native_renderer_options;
use crate::sys::{RSF_AC7_HOOK_SITE_COUNT, rsf_ac7_code_patches};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use windows::Win32::System::WindowsProgramming::GetPrivateProfileIntW;
use windows::core::{PCWSTR, w};

const HOOKS: usize = RSF_AC7_HOOK_SITE_COUNT as usize;
/// Queued render-scope capacity of the native pass controller.
pub(crate) const PENDING_CAPACITY: u32 = 256;
const DIAGNOSTICS_VARIABLE: &str = "RSF_AC7_PIXEL_DIAGNOSTICS";

pub(crate) const PINNED: &str =
    "AC7 native preparation failed during a forwarding call; inactive module pinned until exit";
pub(crate) const DIAGNOSTICS: &str =
    "AC7 native renderer: pixel uniform binding diagnostics enabled";
pub(crate) const PREPARED: &str = "AC7 native renderer prepared: owned view sizing, pre-tonemap SR graph node, downstream engine descriptors, owned widget raster targets and RHI leases; activation pending";

/// The native side of one renderer call. `Code` compares and writes engine code and logs to the
/// renderer's host log.
pub(crate) trait Native: Code {
    fn base(&self) -> *mut u8;
    fn installed(&self) -> Renderer;
    /// Publishes `renderer` to every hook body; null clears it.
    fn install(&self, renderer: Renderer);
    /// A renderer owning a copy of `options`, or null.
    fn create(&self, options: &rsf_ac7_native_renderer_options) -> Renderer;
    fn release(&self, renderer: Renderer);
    /// The renderer's engine patch ownership record.
    fn patches(&self, renderer: Renderer) -> *mut rsf_ac7_code_patches;
    fn active(&self, renderer: Renderer) -> bool;
    fn set_active(&self, renderer: Renderer, active: bool);
    fn quiescing(&self, renderer: Renderer) -> bool;
    fn set_quiescing(&self, renderer: Renderer);
    fn scopes_create(&self, renderer: Renderer, capacity: u32) -> bool;
    fn scopes_quiesce(&self, renderer: Renderer);
    fn scopes_idle(&self, renderer: Renderer) -> bool;
    fn scopes_destroy(&self, renderer: Renderer) -> bool;
    fn hooks_initialize(&self) -> bool;
    fn hook_bind(&self, hook: u32, target: *mut u8);
    fn hook_create(&self, hook: u32) -> bool;
    fn hook_enable(&self, hook: u32) -> bool;
    fn hook_created(&self, hook: u32) -> bool;
    fn hook_disable(&self, hook: u32) -> bool;
    fn hook_remove(&self, hook: u32) -> bool;
    fn pin(&self);
    fn drain(&self, renderer: Renderer);
    fn outstanding(&self, renderer: Renderer) -> bool;
    fn retired(&self, renderer: Renderer) -> bool;
    fn input_unhook(&self, renderer: Renderer) -> bool;
    fn controller_disable(&self);
    fn controller_remove(&self) -> bool;
    fn truesky_restore(&self, renderer: Renderer) -> bool;
    /// Hook bodies running (guards.rs): entry guards, plus outer guards when `outer_too`.
    fn guards_held(&self, outer_too: bool) -> u32;
    /// Whether the environment asks for the pixel uniform binding diagnostic.
    fn pixel_diagnostics(&self) -> bool;
}

/// RSF_AC7_PIXEL_DIAGNOSTICS as the former GetEnvironmentVariableA read it into eight bytes: a
/// value of one to seven characters that does not start with '0'.
pub(crate) fn environment_flag(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| {
        let text = value.as_encoded_bytes();
        text.len() < 8 && text.first().is_some_and(|first| *first != b'0')
    })
}

/// The engine patch record of `renderer`.
///
/// # Safety
/// The record lives in the renderer, and only this owner changes it. The GameSDK lifecycle runs
/// one transition at a time, so start and stop never hold it together.
unsafe fn patch_record<'a>(
    native: &impl Native,
    renderer: Renderer,
) -> Option<&'a mut rsf_ac7_code_patches> {
    unsafe { native.patches(renderer).as_mut() }
}

fn disable_created(native: &impl Native) -> bool {
    let mut disabled = true;
    for hook in 0..RSF_AC7_HOOK_SITE_COUNT {
        if native.hook_created(hook) {
            disabled &= native.hook_disable(hook);
        }
    }
    disabled
}

fn remove_created(native: &impl Native) -> bool {
    let mut removed = true;
    for hook in 0..RSF_AC7_HOOK_SITE_COUNT {
        if native.hook_created(hook) {
            removed &= native.hook_remove(hook);
        }
    }
    removed
}

fn bound_hooks(bound: &[bool; HOOKS]) -> impl Iterator<Item = u32> + '_ {
    (0..RSF_AC7_HOOK_SITE_COUNT).filter(move |hook| bound[*hook as usize])
}

/// All or nothing, as ac7_hooks.h: creates every bound hook in Hook order, then enables them in
/// the same order. Stops at the first failure and leaves the rollback to the caller.
fn install_hooks(native: &impl Native, bound: &[bool; HOOKS]) -> bool {
    bound_hooks(bound).all(|hook| native.hook_create(hook))
        && bound_hooks(bound).all(|hook| native.hook_enable(hook))
}

/// Releases a renderer that was never published. Scope destruction refuses only queued markers,
/// which an inactive renderer has none of; the former code ignored its result as well.
fn discard(native: &impl Native, renderer: Renderer) {
    native.scopes_quiesce(renderer);
    native.scopes_destroy(renderer);
    native.release(renderer);
}

/// Prepare after the installed check. The renderer and whether preparation
/// succeeded: a renderer with `false` is a pinned inactive renderer that stop retires later.
pub(crate) fn prepare(
    native: &impl Native,
    options: &rsf_ac7_native_renderer_options,
) -> (Renderer, bool) {
    let refused = (null_mut(), false);
    let base = native.base();
    // Exact-build admission before anything is allocated or hooked.
    if !sites::admit(base, native) {
        return refused;
    }
    let renderer = native.create(options);
    if renderer.is_null() {
        return refused;
    }
    if !native.scopes_create(renderer, PENDING_CAPACITY) {
        native.release(renderer);
        return refused;
    }
    if !native.hooks_initialize() {
        discard(native, renderer);
        return refused;
    }
    // The evidence hooks run for every pixel uniform bind. Their sites were verified above, but
    // they are hooked only when the diagnostic is asked for (sites::wanted).
    let diagnostics = native.pixel_diagnostics();
    let bound: [bool; HOOKS] = std::array::from_fn(|hook| sites::wanted(hook as u32, diagnostics));
    for (hook, site) in HOOK_SITES.iter().enumerate() {
        let target = if bound[hook] {
            base.wrapping_add(site.rva as usize)
        } else {
            null_mut()
        };
        native.hook_bind(hook as u32, target);
    }
    native.install(renderer);
    if !install_hooks(native, &bound) {
        let disabled = disable_created(native);
        native.set_active(renderer, false);
        if !disabled || native.guards_held(true) != 0 || !remove_created(native) {
            // Keep the renderer and any remaining hooks until stop proves retirement.
            native.pin();
            native.set_quiescing(renderer);
            native.scopes_quiesce(renderer);
            native.log(PINNED);
            return (renderer, false);
        }
        native.install(null_mut());
        discard(native, renderer);
        return refused;
    }
    if diagnostics {
        native.log(DIAGNOSTICS);
    }
    native.log(PREPARED);
    (renderer, true)
}

/// Scene precision, then temporal velocity (patches.rs). A refusal keeps what was applied owned
/// until stop restores it.
pub(crate) fn start(native: &impl Native, renderer: Renderer) -> bool {
    if renderer.is_null() || native.installed() != renderer || native.quiescing(renderer) {
        return false;
    }
    let Some(record) = (unsafe { patch_record(native, renderer) }) else {
        return false;
    };
    if !patches::apply(record, native.base(), native) {
        return false;
    }
    native.set_active(renderer, true);
    true
}

pub(crate) fn is_active(native: &impl Native, renderer: Renderer) -> bool {
    !renderer.is_null() && native.installed() == renderer && native.active(renderer)
}

pub(crate) fn quiesce(native: &impl Native, renderer: Renderer) {
    if renderer.is_null() {
        return;
    }
    native.set_quiescing(renderer);
    native.set_active(renderer, false);
    native.controller_disable();
    native.scopes_quiesce(renderer);
}

/// Retirement. True when the renderer is gone. Every refusal leaves a state the next call can
/// continue from: each step forgets only what it retired.
pub(crate) fn stop(native: &impl Native, renderer: Renderer) -> bool {
    if renderer.is_null() {
        return true;
    }
    if native.active(renderer) {
        return false;
    }
    // Admission. Inactive CPU observers stay installed until their render-thread-only pool refs
    // retire: disabling first would remove the only owner that can drain an asynchronous RHI
    // release. Nothing changes before every check passes.
    native.drain(renderer);
    if native.guards_held(false) != 0
        || native.outstanding(renderer)
        || !native.scopes_idle(renderer)
    {
        return false;
    }
    if !native.input_unhook(renderer) {
        return false;
    }
    if !disable_created(native) || native.guards_held(true) != 0 || !native.controller_remove() {
        return false;
    }
    // Temporal velocity, then scene precision; a refusal is logged there.
    let Some(record) = (unsafe { patch_record(native, renderer) }) else {
        return false;
    };
    if !patches::restore(record, native) {
        return false;
    }
    if !native.truesky_restore(renderer) || !native.scopes_destroy(renderer) {
        return false;
    }
    native.drain(renderer);
    if native.retired(renderer) {
        return false;
    }
    if !remove_created(native) {
        return false;
    }
    native.install(null_mut());
    native.release(renderer);
    true
}

/// The native renderer leaves (native_renderer/leaves.rs) for one call, with the loaded image base.
/// It logs nowhere until `log_to`.
struct NativeLeaves {
    code: NativeCode,
    base: *mut u8,
}

impl NativeLeaves {
    fn new() -> Option<Self> {
        let mut code = unsafe { NativeCode::new(&leaves::code_leaves(None, null_mut())) }?;
        code.view.log = None;
        code.view.user = null_mut();
        Some(Self {
            code,
            base: leaves::base(),
        })
    }

    /// Logs to the host log of these renderer options.
    fn log_to(mut self, options: &rsf_ac7_native_renderer_options) -> Self {
        self.code.view.log = options.log;
        self.code.view.user = options.user;
        self
    }

    /// Logs to the host log of a live, nonnull renderer.
    fn log_to_renderer(self, renderer: Renderer) -> Self {
        if renderer.is_null() {
            return self;
        }
        // SAFETY: a nonnull renderer handed back by the lifecycle is live until stop deletes it,
        // and its options live inside it.
        match unsafe { leaves::options(renderer).as_ref() } {
            Some(options) => self.log_to(options),
            None => self,
        }
    }
}

impl CodeView for NativeLeaves {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        self.code.matches(address, expected)
    }

    fn log(&self, message: &str) {
        self.code.log(message);
    }
}

impl Code for NativeLeaves {
    fn write(&self, site: *mut u8, expected: &[u8], replacement: &[u8]) -> bool {
        self.code.write(site, expected, replacement)
    }
}

// SAFETY, for every call below: the leaves keep the contracts of native_renderer/leaves.rs, and a
// renderer argument is one the native side created and has not released.
impl Native for NativeLeaves {
    fn base(&self) -> *mut u8 {
        self.base
    }
    fn installed(&self) -> Renderer {
        unsafe { leaves::installed() }
    }
    fn install(&self, renderer: Renderer) {
        unsafe { leaves::install(renderer) }
    }
    fn create(&self, options: &rsf_ac7_native_renderer_options) -> Renderer {
        unsafe { leaves::create(options) }
    }
    fn release(&self, renderer: Renderer) {
        unsafe { leaves::release(renderer) }
    }
    fn patches(&self, renderer: Renderer) -> *mut rsf_ac7_code_patches {
        unsafe { leaves::patches(renderer) }
    }
    fn active(&self, renderer: Renderer) -> bool {
        unsafe { leaves::active(renderer) }
    }
    fn set_active(&self, renderer: Renderer, active: bool) {
        unsafe { leaves::set_active(renderer, active) }
    }
    fn quiescing(&self, renderer: Renderer) -> bool {
        unsafe { leaves::quiescing(renderer) }
    }
    fn set_quiescing(&self, renderer: Renderer) {
        unsafe { leaves::set_quiescing(renderer) }
    }
    fn scopes_create(&self, renderer: Renderer, capacity: u32) -> bool {
        unsafe { leaves::scopes_create(renderer, capacity) }
    }
    fn scopes_quiesce(&self, renderer: Renderer) {
        unsafe { leaves::scopes_quiesce(renderer) }
    }
    fn scopes_idle(&self, renderer: Renderer) -> bool {
        unsafe { leaves::scopes_idle(renderer) }
    }
    fn scopes_destroy(&self, renderer: Renderer) -> bool {
        unsafe { leaves::scopes_destroy(renderer) }
    }
    fn hooks_initialize(&self) -> bool {
        unsafe { leaves::hooks_initialize() }
    }
    fn hook_bind(&self, hook: u32, target: *mut u8) {
        unsafe { leaves::hook_bind(hook, target) }
    }
    fn hook_create(&self, hook: u32) -> bool {
        unsafe { leaves::hook_create(hook) }
    }
    fn hook_enable(&self, hook: u32) -> bool {
        unsafe { leaves::hook_enable(hook) }
    }
    fn hook_created(&self, hook: u32) -> bool {
        unsafe { leaves::hook_created(hook) }
    }
    fn hook_disable(&self, hook: u32) -> bool {
        unsafe { leaves::hook_disable(hook) }
    }
    fn hook_remove(&self, hook: u32) -> bool {
        unsafe { leaves::hook_remove(hook) }
    }
    fn pin(&self) {
        unsafe { leaves::pin() }
    }
    fn drain(&self, renderer: Renderer) {
        unsafe { leaves::drain(renderer) }
    }
    fn outstanding(&self, renderer: Renderer) -> bool {
        unsafe { leaves::outstanding(renderer) }
    }
    fn retired(&self, renderer: Renderer) -> bool {
        unsafe { leaves::retired(renderer) }
    }
    fn input_unhook(&self, renderer: Renderer) -> bool {
        unsafe { leaves::input_unhook(renderer) }
    }
    fn controller_disable(&self) {
        unsafe { leaves::controller_disable() }
    }
    fn controller_remove(&self) -> bool {
        unsafe { leaves::controller_remove() }
    }
    fn truesky_restore(&self, renderer: Renderer) -> bool {
        unsafe { leaves::truesky_restore(renderer) }
    }
    fn guards_held(&self, outer_too: bool) -> u32 {
        rsf_ac7_guards_held(outer_too)
    }
    fn pixel_diagnostics(&self) -> bool {
        let fallback = environment_flag(std::env::var_os(DIAGNOSTICS_VARIABLE).as_deref());
        let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|path| !path.is_empty()) else {
            return fallback;
        };
        let path = std::path::PathBuf::from(local)
            .join("ReScaleFrame")
            .join("AC7.ini");
        let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        unsafe {
            GetPrivateProfileIntW(
                w!("Startup"),
                w!("RSF_AC7_PIXEL_DIAGNOSTICS"),
                i32::from(fallback),
                PCWSTR(path.as_ptr()),
            ) != 0
        }
    }
}

/// Prepares a renderer for the host options lifecycle.rs built from validated GameSDK services.
/// Refused while another renderer is installed.
pub(crate) fn rsf_ac7_renderer_prepare(
    options: &rsf_ac7_native_renderer_options,
) -> (Renderer, bool) {
    let refused = (null_mut(), false);
    let Some(native) = NativeLeaves::new() else {
        return refused;
    };
    let native = native.log_to(options);
    if !native.installed().is_null() {
        return refused;
    }
    prepare(&native, options)
}

/// # Safety
/// `renderer` is null or live.
pub(crate) unsafe fn rsf_ac7_renderer_start(renderer: Renderer) -> bool {
    !renderer.is_null()
        && NativeLeaves::new()
            .is_some_and(|native| start(&native.log_to_renderer(renderer), renderer))
}

/// # Safety
/// As `rsf_ac7_renderer_start`.
pub(crate) unsafe fn rsf_ac7_renderer_is_active(renderer: Renderer) -> bool {
    NativeLeaves::new().is_some_and(|native| is_active(&native, renderer))
}

/// # Safety
/// As `rsf_ac7_renderer_start`.
pub(crate) unsafe fn rsf_ac7_renderer_quiesce(renderer: Renderer) {
    if let Some(native) = NativeLeaves::new() {
        quiesce(&native, renderer);
    }
}

/// # Safety
/// As `rsf_ac7_renderer_start`.
pub(crate) unsafe fn rsf_ac7_renderer_stop(renderer: Renderer) -> bool {
    renderer.is_null()
        || NativeLeaves::new()
            .is_some_and(|native| stop(&native.log_to_renderer(renderer), renderer))
}

#[cfg(test)]
mod tests;
