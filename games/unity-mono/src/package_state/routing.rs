// SPDX-License-Identifier: MIT
//! The Unity package's routing behind package.cpp: the host render callback's window,
//! final-scene, SR and submitted passes with the Editor preview's admission and diagnostic
//! budget, the generation window guard, and Unity's device lifecycle. package.cpp keeps the
//! Unity, Win32, D3D12, Vulkan, preview and Runtime calls as leaves and runs the SR pass itself
//! under its mutex when the route asks for it.
//!
//! No lock here is held across a leaf. The route runs outside package.cpp's mutex, as the C++
//! callback did; the device lifecycle runs under it, so its state is only stored here.
use super::{Leaves, PACKAGE, Package};
use crate::package_abi::rsf_unity_package_platform;
use rsf_game_sys::{RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_WINDOW, rsf_game_render_pass};
use std::ffi::{c_int, c_void};
use std::sync::atomic::Ordering::SeqCst;

/// RSF_UNITY_PREVIEW_SOURCE_COMMON, package.h's flag; package.cpp asserts it.
const PREVIEW_SOURCE_COMMON: u32 = 0x4000_0000;
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

// SAFETY for every call below: package.cpp's noexcept leaves, borrowing their pointer arguments
// for the call only.
impl Leaves {
    fn fg_submitted(&self, session: u64, source_frame: u64) {
        unsafe { (self.fg_submitted)(session, source_frame) }
    }
    fn fg_window(&self, pass: &rsf_game_render_pass) {
        unsafe { (self.fg_window)(pass) }
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
        unsafe { (self.preview_owns)(window) != 0 }
    }
    fn renderer_vulkan(&self) -> bool {
        unsafe { (self.renderer_vulkan)() != 0 }
    }
    fn vulkan_owns(&self, window: *mut c_void) -> bool {
        unsafe { (self.vulkan_owns)(window) != 0 }
    }
    fn window_class(&self, window: *mut c_void) -> u32 {
        unsafe { (self.window_class)(window) }
    }
    fn bridge_load(&self) -> bool {
        unsafe { (self.bridge_load)() != 0 }
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
    /// the original under package.cpp's mutex.
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
                let shown = leaves.preview_present(&pass, pass.flags & PREVIEW_SOURCE_COMMON != 0);
                self.preview_refused(leaves, shown, &pass);
            } else {
                leaves.fg_window(&pass);
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

    /// Unity's device events, under package.cpp's mutex. A waiting device loads the bridge once
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
        self.device.store(DEVICE_WAITING, SeqCst);
    }

    /// rsfu_shutdown after the preview is hidden: nothing to retire while waiting, a retained
    /// device refuses, a ready one reports its session's retirement.
    pub(super) fn shutdown(&self, leaves: &Leaves) -> bool {
        match self.device.load(SeqCst) {
            DEVICE_READY => self.retire(leaves),
            device => device == DEVICE_WAITING,
        }
    }
}

/// package.cpp's render callback body. A null list or pass does nothing, as before.
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_unity_package_route(
    commands: *mut c_void,
    pass: *const rsf_game_render_pass,
    begin: u32,
    platform: *const rsf_unity_package_platform,
) -> u32 {
    let Some(leaves) = (unsafe { Leaves::adopt(platform) }) else {
        return 0;
    };
    if commands.is_null() || pass.is_null() {
        return 0;
    }
    u32::from(PACKAGE.route(&leaves, commands, unsafe { &*pass }, begin != 0))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_accept_window(
    window: *mut c_void,
    platform: *const rsf_unity_package_platform,
) -> c_int {
    unsafe { Leaves::adopt(platform) }.map_or(0, |leaves| {
        c_int::from(PACKAGE.accept_window(&leaves, window))
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_device_event(
    initialize: u32,
    platform: *const rsf_unity_package_platform,
) {
    if let Some(leaves) = unsafe { Leaves::adopt(platform) } {
        PACKAGE.device_event(&leaves, initialize != 0);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_device_retained() -> u32 {
    u32::from(PACKAGE.device.load(SeqCst) == DEVICE_RETAINED)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_shutdown(
    platform: *const rsf_unity_package_platform,
) -> u32 {
    u32::from(match unsafe { Leaves::adopt(platform) } {
        Some(leaves) => PACKAGE.shutdown(&leaves),
        None => PACKAGE.device.load(SeqCst) == DEVICE_WAITING,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_refuse_an_incomplete_table() {
        let pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        unsafe {
            assert_eq!(
                rsf_unity_package_route(
                    std::ptr::NonNull::dangling().as_ptr(),
                    &pass,
                    1,
                    std::ptr::null()
                ),
                0
            );
            assert_eq!(
                rsf_unity_package_accept_window(std::ptr::null_mut(), std::ptr::null()),
                0
            );
            rsf_unity_package_device_event(1, std::ptr::null());
        }
        // The static package's device is still waiting: shutdown has nothing to retire.
        assert_eq!(rsf_unity_package_device_retained(), 0);
        assert_eq!(unsafe { rsf_unity_package_shutdown(std::ptr::null()) }, 1);
    }
}
