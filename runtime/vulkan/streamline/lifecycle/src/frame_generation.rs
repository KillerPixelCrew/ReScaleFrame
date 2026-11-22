// SPDX-License-Identifier: MIT
//! DLSS-G state for the standalone Streamline bridge. The orchestrator's
//! rsf_dlssgvk owner has its own proxy, frame inputs and retirement protocol.

use crate::sdk_fg_options::{self, Packed};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Options {
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    back_buffers: u32,
}

const _: () = assert!(size_of::<Options>() == 5 * size_of::<u32>());

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

#[repr(C)]
pub(crate) struct Platform {
    pub options_bound: unsafe extern "C" fn() -> u32,
    pub apply: unsafe extern "C" fn(&Packed) -> i32,
    pub release: unsafe extern "C" fn() -> i32,
}

const _: () = assert!(size_of::<Platform>() == 3 * size_of::<*mut std::ffi::c_void>());

// sl::Result::eWarnOutOfVRAM, checked in sdk_records_checks.h.
const OUT_OF_VRAM: i32 = 39;

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
            last_options: Options {
                count: 0,
                width: 0,
                height: 0,
                format: 0,
                back_buffers: 0,
            },
        }
    }

    pub(crate) unsafe fn set(
        &mut self,
        platform: &Platform,
        revision: u64,
        enabled: bool,
        requested: Options,
    ) -> i32 {
        // Off is the SDK default. Repeating Off can also interfere with a
        // provider switch when the SDK sees both requests before Present.
        if !enabled && !self.enabled {
            return 0;
        }
        if unsafe { (platform.options_bound)() } == 0 {
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
        let result = unsafe { (platform.apply)(&sdk_options) };
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

    pub(crate) unsafe fn disable(&mut self, platform: &Platform, revision: u64) -> i32 {
        if !self.resources_may_exist || !self.enabled {
            return 0;
        }
        unsafe { self.set(platform, revision, false, self.last_options) }
    }

    pub(crate) unsafe fn free(&mut self, platform: &Platform) -> i32 {
        if !self.resources_may_exist || unsafe { (platform.options_bound)() } == 0 {
            return 0;
        }
        if self.enabled {
            return -1;
        }
        let result = unsafe { (platform.release)() };
        if result == 0 {
            self.configured_at = None;
            self.resources_may_exist = false;
        }
        result
    }
}
