// SPDX-License-Identifier: MIT
//! Unity render-pass routing, window admission and device events.
use super::{
    LEAVES, Leaves, OPERATIONS, PACKAGE, Package, RECORDING, UNITY_GRAPHICS, UNITY_INTERFACES,
};
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
use super::{presentation, services};
use crate::sys::RSF_UNITY_PREVIEW_SOURCE_COMMON;
use crate::sys::{RSF_UNITY_DEVICE_EVENT_INITIALIZE, RSF_UNITY_DEVICE_EVENT_SHUTDOWN};
use rsf_game_sys::{RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_WINDOW, rsf_game_render_pass};
use std::ffi::{c_int, c_void};
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
use std::ptr::null_mut;
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::Ordering::SeqCst;

/// Editor preview refusals with a texture logged per process.
const PREVIEW_REPORTS: u32 = 4;
/// window_class's answers.
const CLASS_PLAYER: u32 = 1;
const CLASS_OTHER: u32 = 2;

/// Unity's device: waiting for its initialize event, ready with the bridge loaded, or retained
/// for the process after a refused retirement, when Unity may already have destroyed it.
pub(super) const DEVICE_WAITING: u8 = 0;
pub(super) const DEVICE_READY: u8 = 1;
pub(super) const DEVICE_RETAINED: u8 = 2;

const RETAINED: &str = "Unity device shutdown retained unproved bridge work; initialization requires a process restart.";

// SAFETY for every call below: package.c's leaves, borrowing their pointer arguments
// for the call only.
impl Leaves {
    fn fg_submitted(&self, session: u64, source_frame: u64) {
        unsafe { (self.fg_submitted)(session, source_frame) }
    }
    fn fg_hudless(&self, commands: *mut c_void, pass: &rsf_game_render_pass) {
        unsafe { (self.fg_hudless)(commands, pass) }
    }
    fn preview_present(&self, pass: &rsf_game_render_pass, common: bool) -> bool {
        unsafe { (self.preview_present)(pass.color_input, pass, u32::from(common)) != 0 }
    }
    /// ID3D12Resource::GetDesc's width, height and format.
    fn texture_desc(&self, resource: *mut c_void) -> (u64, u32, u32) {
        let (mut width, mut height, mut format) = (0u64, 0u32, 0u32);
        unsafe { (self.texture_desc)(resource, &mut width, &mut height, &mut format) };
        (width, height, format)
    }
    fn preview_prepare(&self, width: u32, height: u32) -> bool {
        unsafe { (self.preview_prepare)(width, height) != 0 }
    }
    fn preview_owns(&self, window: *mut c_void) -> bool {
        (self.preview_owns)(window)
    }
    fn renderer_vulkan(&self) -> bool {
        unsafe { (self.renderer_vulkan)(UNITY_GRAPHICS.load(SeqCst)) != 0 }
    }
    fn vulkan_owns(&self, window: *mut c_void) -> bool {
        (self.vulkan_owns)(window)
    }
    fn window_class(&self, window: *mut c_void) -> u32 {
        unsafe { (self.window_class)(window) }
    }
    fn bridge_load(&self) -> bool {
        unsafe { (self.bridge_load)(UNITY_INTERFACES.load(SeqCst)) != 0 }
    }
    fn bridge_unload(&self) {
        unsafe { (self.bridge_unload)() }
    }
}

impl Package {
    fn editor(&self) -> bool {
        self.services.editor.load(SeqCst)
    }

    /// The render callback in the C++ order. An end reports every pass but the window's as
    /// submitted. The window pass asks for the generation provider, then presents the Editor
    /// preview or hands the pass to generation; the final scene is generation's HUD-less image.
    /// Any other pass is SR: the Editor preview is prepared first and true runs the SR pass on
    /// the original under the package operation lock.
    pub(super) fn route(
        &self,
        leaves: &Leaves,
        commands: *mut c_void,
        original: &rsf_game_render_pass,
        begin: bool,
    ) -> bool {
        let pass = *original;
        if !begin {
            if pass.role != RSF_GAME_RENDER_WINDOW {
                leaves.fg_submitted(pass.session_id, pass.source_frame_id);
            }
            return false;
        }
        if pass.role == RSF_GAME_RENDER_WINDOW {
            self.request_generation(leaves);
            if self.editor() {
                let shown = leaves
                    .preview_present(&pass, pass.flags & RSF_UNITY_PREVIEW_SOURCE_COMMON != 0);
                self.preview_refused(leaves, shown, &pass);
            } else {
                unsafe { (leaves.fg_window)(&pass) };
            }
            return false;
        }
        if pass.role == RSF_GAME_RENDER_FINAL_SCENE {
            leaves.fg_hudless(commands, &pass);
            return false;
        }
        !self.editor()
            || leaves.preview_prepare(pass.camera.output_width, pass.camera.output_height)
    }

    /// Every refusal counts toward the budget, as the C++ static did; one with a texture within
    /// the first four is logged with the texture's and camera's extents.
    fn preview_refused(&self, leaves: &Leaves, shown: bool, pass: &rsf_game_render_pass) {
        if shown
            || self.preview_reports.fetch_add(1, SeqCst) >= PREVIEW_REPORTS
            || pass.color_input.is_null()
        {
            return;
        }
        let (width, height, format) = leaves.texture_desc(pass.color_input);
        self.log(
            leaves,
            &format!(
                "Editor presentation pending: texture={width}x{height} format={format} expected={}x{}",
                pass.camera.output_width, pass.camera.output_height
            ),
        );
    }

    /// Generation's window guard: the Editor accepts only the preview's window and the Vulkan
    /// player only its surface. Otherwise this process's UnityWndClass window is accepted, and
    /// another window of this process when it is the Vulkan surface.
    pub(super) fn accept_window(&self, leaves: &Leaves, window: *mut c_void) -> bool {
        if self.editor() {
            return leaves.preview_owns(window);
        }
        if leaves.renderer_vulkan() {
            return leaves.vulkan_owns(window);
        }
        match leaves.window_class(window) {
            CLASS_PLAYER => true,
            CLASS_OTHER => leaves.vulkan_owns(window),
            _ => false,
        }
    }

    /// Unity's device events, under the package operation lock. A waiting device loads the bridge once
    /// Unity's interfaces exist. A ready one retires the session, then unloads the bridge; a
    /// refused retirement retains the device, since Unity's void event can go on to destroy it
    /// and a later callback cannot assume borrowed Vulkan handles still name a live device.
    pub(super) fn device_event(&self, leaves: &Leaves, initialize: bool) {
        let device = self.device.load(SeqCst);
        if initialize {
            if device == DEVICE_WAITING && leaves.bridge_load() {
                self.device.store(DEVICE_READY, SeqCst);
            }
            return;
        }
        if device != DEVICE_READY {
            return;
        }
        if !self.retire(leaves) {
            self.device.store(DEVICE_RETAINED, SeqCst);
            self.log(leaves, RETAINED);
            return;
        }
        leaves.bridge_unload();
        self.presentation.device_retired();
        self.device.store(DEVICE_WAITING, SeqCst);
    }

    /// rsfu_shutdown after the preview is hidden: nothing to retire while waiting, a retained
    /// device refuses, a ready one reports its session's retirement.
    #[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
    pub(super) fn shutdown(&self, leaves: &Leaves) -> bool {
        let retired = match self.device.load(SeqCst) {
            DEVICE_READY => self.retire(leaves),
            device => device == DEVICE_WAITING,
        };
        if retired {
            self.stop_window_input(leaves);
        }
        retired
    }
}

/// The bridge holds its callback and packet leases until this returns.
pub(super) unsafe extern "C" fn render_callback(
    _: *mut c_void,
    commands: *mut c_void,
    pass: *const rsf_game_render_pass,
    begin: u32,
) {
    let Some(leaves) = LEAVES.get() else {
        return;
    };
    if commands.is_null() || pass.is_null() {
        return;
    }
    let pass = unsafe { &*pass };
    if PACKAGE.route(leaves, commands, pass, begin != 0) {
        let _operation = OPERATIONS.lock().unwrap_or_else(PoisonError::into_inner);
        PACKAGE.render(leaves, commands, pass);
    } else {
        RECORDING.set(None);
    }
}

pub(super) extern "C" fn accept_window(_: *mut c_void, window: *mut c_void) -> c_int {
    LEAVES.get().map_or(0, |leaves| {
        c_int::from(PACKAGE.accept_window(leaves, window))
    })
}

// The preview presenter consumes the Runtime observation receipt before presenting.
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
pub(super) unsafe extern "C" fn observe_preview(pass: *const rsf_game_render_pass) -> c_int {
    let Some(leaves) = LEAVES.get() else {
        return 0;
    };
    // The presenter lends the pass with the preview swap chain already attached. The Runtime
    // entry cannot unwind, so its return is the observation.
    unsafe { (leaves.fg_window)(pass) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_device_event(sdk_event: u32) {
    let initialize = match sdk_event {
        RSF_UNITY_DEVICE_EVENT_INITIALIZE => true,
        RSF_UNITY_DEVICE_EVENT_SHUTDOWN => false,
        _ => return,
    };
    let _operation = OPERATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(leaves) = LEAVES.get() {
        PACKAGE.device_event(leaves, initialize);
    }
}

#[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
pub(super) fn shutdown() -> bool {
    let _operation = OPERATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    match LEAVES.get() {
        Some(leaves) => PACKAGE.shutdown(leaves),
        None => PACKAGE.device.load(SeqCst) == DEVICE_WAITING,
    }
}

// Only the load/unload entries take this lock. Registration can call the device callback,
// so the package operation lock must be free and the registry already published.
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
static REGISTERED: Mutex<bool> = Mutex::new(false);

// package.c and vulkan_bridge.c: the Unity SDK calls.
#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
unsafe extern "C" {
    fn rsf_unity_package_graphics_interface(interfaces: *mut c_void) -> *mut c_void;
    fn rsf_unity_package_device_callback(graphics: *mut c_void, subscribe: u32);
    fn rsf_uvk_preload(interfaces: *mut c_void);
}

#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_plugin_load(interfaces: *mut c_void) {
    let mut registered = REGISTERED.lock().unwrap_or_else(PoisonError::into_inner);
    if PACKAGE.device.load(SeqCst) == DEVICE_RETAINED || !UNITY_INTERFACES.load(SeqCst).is_null() {
        return;
    }
    UNITY_INTERFACES.store(interfaces, SeqCst);
    let graphics = unsafe { rsf_unity_package_graphics_interface(interfaces) };
    UNITY_GRAPHICS.store(graphics, SeqCst);
    if !interfaces.is_null() {
        unsafe { rsf_uvk_preload(interfaces) };
    }
    presentation::load();
    if !graphics.is_null() {
        *registered = true;
        unsafe { rsf_unity_package_device_callback(graphics, 1) };
        rsf_unity_package_device_event(RSF_UNITY_DEVICE_EVENT_INITIALIZE);
    }
}

#[cfg(all(windows, not(feature = "mono-plugin"), not(test)))]
#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_plugin_unload() {
    let mut registered = REGISTERED.lock().unwrap_or_else(PoisonError::into_inner);
    if std::mem::take(&mut *registered) {
        unsafe { rsf_unity_package_device_callback(UNITY_GRAPHICS.load(SeqCst), 0) };
    }
    rsf_unity_package_device_event(RSF_UNITY_DEVICE_EVENT_SHUTDOWN);
    services::unload();
    UNITY_INTERFACES.store(null_mut(), SeqCst);
    UNITY_GRAPHICS.store(null_mut(), SeqCst);
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_without_a_loaded_table() {
        let pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        unsafe {
            render_callback(
                std::ptr::null_mut(),
                std::ptr::NonNull::dangling().as_ptr(),
                &pass,
                1,
            );
        }
        // The static package's device is still waiting: shutdown has nothing to retire.
        assert_eq!(accept_window(std::ptr::null_mut(), std::ptr::null_mut()), 0);
        rsf_unity_package_device_event(RSF_UNITY_DEVICE_EVENT_INITIALIZE);
        assert_eq!(PACKAGE.device.load(SeqCst), DEVICE_WAITING);
        assert!(shutdown());
    }
}
