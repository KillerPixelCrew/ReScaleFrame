// SPDX-License-Identifier: MIT
//! DLSS-G state for the legacy VulkanStorySl bridge. The orchestrator's
//! rsf_dlssgvk owner has its own proxy, frame inputs and retirement protocol.

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

pub(crate) type SetOptions = unsafe extern "C" fn(u32, &Options) -> i32;
pub(crate) type FreeResources = unsafe extern "C" fn() -> i32;

// Checked against sl::Result in the native leaf.
const OUT_OF_VRAM: i32 = 39;

pub(crate) struct FrameGeneration {
    configured: bool,
    resources_may_exist: bool,
    enabled: bool,
    last_options: Options,
    swapchain: u64,
}

impl FrameGeneration {
    pub(crate) const fn new() -> Self {
        Self {
            configured: false,
            resources_may_exist: false,
            enabled: false,
            last_options: Options {
                count: 0,
                width: 0,
                height: 0,
                format: 0,
                back_buffers: 0,
            },
            swapchain: 0,
        }
    }

    pub(crate) fn swapchain_created(&mut self, swapchain: u64) {
        self.swapchain = swapchain;
        self.configured = false;
    }

    pub(crate) fn swapchain_destroyed(&mut self, swapchain: u64) {
        // Deferred retirement of the old chain preserves the successor's cache.
        if self.swapchain == swapchain {
            self.swapchain = 0;
            self.configured = false;
        }
    }

    pub(crate) unsafe fn set(
        &mut self,
        apply: Option<SetOptions>,
        enabled: bool,
        requested: Options,
    ) -> i32 {
        // Off is the SDK default. Repeating Off can also interfere with a
        // provider switch when the SDK sees both requests before Present.
        if !enabled && !self.enabled {
            return 0;
        }
        let Some(apply) = apply else {
            return -1;
        };
        if enabled && requested.count == 0 {
            return -2;
        }
        if self.configured && self.enabled == enabled && self.last_options == requested {
            return 0;
        }

        let sdk_options = Options {
            count: if enabled { requested.count } else { 1 },
            ..requested
        };
        let result = unsafe { apply(u32::from(enabled), &sdk_options) };
        if result == 0 {
            self.configured = true;
            self.enabled = enabled;
            self.resources_may_exist |= enabled;
            self.last_options = requested;
        } else if !enabled && result == OUT_OF_VRAM {
            // Leave resource ownership pending so the host can drain and free.
            self.enabled = false;
            self.configured = false;
        }
        result
    }

    pub(crate) unsafe fn disable(&mut self, apply: Option<SetOptions>) -> i32 {
        if !self.resources_may_exist || !self.enabled {
            return 0;
        }
        unsafe { self.set(apply, false, self.last_options) }
    }

    pub(crate) unsafe fn free(
        &mut self,
        options_bound: bool,
        release: Option<FreeResources>,
    ) -> i32 {
        if !options_bound || !self.resources_may_exist {
            return 0;
        }
        let (false, Some(release)) = (self.enabled, release) else {
            return -1;
        };
        let result = unsafe { release() };
        if result == 0 {
            self.configured = false;
            self.resources_may_exist = false;
        }
        result
    }
}
