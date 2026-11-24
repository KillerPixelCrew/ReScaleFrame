// SPDX-License-Identifier: MIT
//! DLSS-G state for the standalone Streamline bridge. The orchestrator's
//! rsf_dlssgvk owner has its own proxy, frame inputs and retirement protocol.

use crate::sdk_fg_options;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Options {
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    back_buffers: u32,
}

impl Options {
    pub(crate) const fn new(
        count: u32,
        width: u32,
        height: u32,
        format: u32,
        back_buffers: u32,
    ) -> Self {
        Self {
            count,
            width,
            height,
            format,
            back_buffers,
        }
    }
}

// The SDK option and release leaves in the Windows bridge.
use crate::sdk_api::OUT_OF_VRAM;
use crate::win32::{fg_apply, fg_options_bound, fg_release};

pub(crate) struct Swapchain {
    current: u64,
    pub revision: u64,
}

impl Swapchain {
    pub(crate) const fn new() -> Self {
        Self {
            current: 0,
            revision: 0,
        }
    }

    pub(crate) fn created(&mut self, swapchain: u64) {
        self.current = swapchain;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn destroyed(&mut self, swapchain: u64) {
        // Deferred retirement of the old chain preserves the successor's cache.
        if self.current == swapchain {
            self.current = 0;
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

pub(crate) struct FrameGeneration {
    configured_at: Option<u64>,
    resources_may_exist: bool,
    enabled: bool,
    last_options: Options,
}

impl FrameGeneration {
    pub(crate) const fn new() -> Self {
        Self {
            configured_at: None,
            resources_may_exist: false,
            enabled: false,
            last_options: Options::new(0, 0, 0, 0, 0),
        }
    }

    pub(crate) unsafe fn set(&mut self, revision: u64, enabled: bool, requested: Options) -> i32 {
        // Off is the SDK default. Repeating Off can also interfere with a
        // provider switch when the SDK sees both requests before Present.
        if !enabled && !self.enabled {
            return 0;
        }
        if !fg_options_bound() {
            return -1;
        }
        if enabled && requested.count == 0 {
            return -2;
        }
        if self.configured_at == Some(revision)
            && self.enabled == enabled
            && self.last_options == requested
        {
            return 0;
        }

        let mut sdk_options = sdk_fg_options::pack(
            enabled.then_some(requested.count),
            requested.width,
            requested.height,
            requested.format,
            requested.back_buffers,
        );
        sdk_options.ui_recomposition = 1;
        let result = unsafe { fg_apply(&sdk_options) };
        if result == 0 {
            // Keep the pre-call revision. A WSI callback during SetOptions
            // invalidates this result for reuse while ownership still updates.
            self.configured_at = Some(revision);
            self.enabled = enabled;
            self.resources_may_exist |= enabled;
            self.last_options = requested;
        } else if !enabled && result == OUT_OF_VRAM {
            // Leave resource ownership pending so the host can drain and free.
            self.enabled = false;
            self.configured_at = None;
        }
        result
    }

    pub(crate) unsafe fn disable(&mut self, revision: u64) -> i32 {
        if !self.resources_may_exist || !self.enabled {
            return 0;
        }
        unsafe { self.set(revision, false, self.last_options) }
    }

    pub(crate) unsafe fn free(&mut self) -> i32 {
        if !self.resources_may_exist || !fg_options_bound() {
            return 0;
        }
        if self.enabled {
            return -1;
        }
        let result = unsafe { fg_release() };
        if result == 0 {
            self.configured_at = None;
            self.resources_may_exist = false;
        }
        result
    }
}
