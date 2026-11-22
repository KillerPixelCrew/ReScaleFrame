// SPDX-License-Identifier: MIT
use super::{Handle, Host, INIT_FAILED, OK, Setup};
use crate::sdk_module::FFX_CONTEXT_EXPORTS;
use crate::sys::rsf_vk_function;
use rsf_game_sys::backend_result::MISSING_ENTRY_POINT;
use std::ffi::{CStr, c_char};
use std::ptr;

// The SDK leaf's binders give these entries their function pointer types.
const SDK_ENTRY_COUNT: usize = 1 + FFX_CONTEXT_EXPORTS.len();
const DEVICE_NAMES: [&CStr; 3] = [
    c"vkCmdPipelineBarrier",
    c"vkCmdCopyImage",
    c"vkDeviceWaitIdle",
];
const WSI_NAMES: [&CStr; 5] = [
    c"vkQueuePresentKHR",
    c"vkAcquireNextImageKHR",
    c"vkGetSwapchainImagesKHR",
    c"vkCreateSwapchainKHR",
    c"vkDestroySwapchainKHR",
];

pub(super) type CoreEntries = [rsf_vk_function; SDK_ENTRY_COUNT + DEVICE_NAMES.len()];
pub(super) type WsiEntries = [rsf_vk_function; 1 + WSI_NAMES.len()];

/// Startup steps of the SDK leaf, in the order `open` runs them.
pub(super) struct Platform {
    pub(super) allocate: unsafe extern "C" fn(*const Setup, *const Host, u32, u32) -> Handle,
    pub(super) close: unsafe extern "C" fn(Handle),
    pub(super) symbol: unsafe extern "C" fn(Handle, *const c_char) -> rsf_vk_function,
    pub(super) device_proc:
        unsafe extern "C" fn(Handle, *const c_char, u32, &mut rsf_vk_function) -> i32,
    pub(super) instance_proc:
        unsafe extern "C" fn(Handle, *const c_char, &mut rsf_vk_function) -> i32,
    pub(super) bind_core: unsafe extern "C" fn(Handle, &CoreEntries),
    pub(super) read_memory: unsafe extern "C" fn(Handle, rsf_vk_function) -> i32,
    pub(super) load_resources: unsafe extern "C" fn(Handle) -> i32,
    pub(super) initialize_capture: unsafe extern "C" fn(Handle, &mut Handle) -> i32,
    pub(super) bind_wsi: unsafe extern "C" fn(Handle, &WsiEntries),
}

const _: () = {
    assert!(size_of::<CoreEntries>() == 8 * size_of::<Handle>());
    assert!(size_of::<WsiEntries>() == 6 * size_of::<Handle>());
};

fn checked(result: i32) -> Result<(), i32> {
    if result == OK { Ok(()) } else { Err(result) }
}

impl Platform {
    fn prepare(&self, native: Handle, module: Handle) -> Result<Handle, i32> {
        let mut core = [None; SDK_ENTRY_COUNT + DEVICE_NAMES.len()];
        core[0] = unsafe { (self.symbol)(module, c"ffxConfigure".as_ptr()) };
        for (entry, name) in core[1..SDK_ENTRY_COUNT].iter_mut().zip(FFX_CONTEXT_EXPORTS) {
            *entry = unsafe { (self.symbol)(module, name.as_ptr()) };
        }
        if core[..SDK_ENTRY_COUNT].iter().any(Option::is_none) {
            return Err(MISSING_ENTRY_POINT);
        }
        for (entry, name) in core[SDK_ENTRY_COUNT..].iter_mut().zip(DEVICE_NAMES) {
            checked(unsafe { (self.device_proc)(native, name.as_ptr(), 1, entry) })?;
            if entry.is_none() {
                return Err(MISSING_ENTRY_POINT);
            }
        }
        unsafe { (self.bind_core)(native, &core) };
        let mut memory = None;
        checked(unsafe {
            (self.instance_proc)(
                native,
                c"vkGetPhysicalDeviceMemoryProperties".as_ptr(),
                &mut memory,
            )
        })?;
        if memory.is_none() {
            return Err(MISSING_ENTRY_POINT);
        }
        checked(unsafe { (self.read_memory)(native, memory) })?;
        checked(unsafe { (self.load_resources)(native) })?;
        let mut resources = ptr::null_mut();
        checked(unsafe { (self.initialize_capture)(native, &mut resources) })?;

        let mut wsi = [None; 1 + WSI_NAMES.len()];
        wsi[0] = unsafe { (self.symbol)(module, c"rsfFfxSetVulkanWsiDispatch".as_ptr()) };
        if wsi[0].is_none() {
            return Err(MISSING_ENTRY_POINT);
        }
        // All five WSI names use the original device resolver, without alias promotion.
        for (entry, name) in wsi[1..].iter_mut().zip(WSI_NAMES) {
            checked(unsafe { (self.device_proc)(native, name.as_ptr(), 0, entry) })?;
        }
        if wsi[1..].iter().any(Option::is_none) {
            return Err(INIT_FAILED);
        }
        unsafe { (self.bind_wsi)(native, &wsi) };
        Ok(resources)
    }
}

/// Allocates a leaf, binds its SDK, device and WSI entries, and closes the
/// unpublished leaf again when any step fails.
#[allow(clippy::too_many_arguments)]
pub(super) fn open(
    platform: &Platform,
    setup: *const Setup,
    host: *const Host,
    module: Handle,
    width: u32,
    height: u32,
    out: &mut Handle,
    resources: &mut Handle,
) -> i32 {
    let native = unsafe { (platform.allocate)(setup, host, width, height) };
    if native.is_null() {
        return INIT_FAILED;
    }
    match platform.prepare(native, module) {
        Ok(user) => {
            *out = native;
            *resources = user;
            OK
        }
        Err(result) => {
            unsafe { (platform.close)(native) };
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn record(call: impl Into<String>) {
        CALLS.with_borrow_mut(|calls| calls.push(call.into()));
    }

    unsafe extern "C" fn entry() {}

    unsafe extern "C" fn allocate(_: *const Setup, _: *const Host, _: u32, _: u32) -> Handle {
        record("allocate");
        ptr::without_provenance_mut(1)
    }

    unsafe extern "C" fn close(native: Handle) {
        assert_eq!(native.addr(), 1);
        record("close");
    }

    unsafe extern "C" fn symbol(_: Handle, name: *const c_char) -> rsf_vk_function {
        record(unsafe { CStr::from_ptr(name) }.to_string_lossy());
        Some(entry)
    }

    unsafe extern "C" fn device_proc(
        _: Handle,
        name: *const c_char,
        aliases: u32,
        out: &mut rsf_vk_function,
    ) -> i32 {
        let name = unsafe { CStr::from_ptr(name) };
        record(format!("{aliases}:{}", name.to_string_lossy()));
        *out = if name == c"vkQueuePresentKHR" {
            None
        } else {
            Some(entry)
        };
        OK
    }

    unsafe extern "C" fn instance_proc(
        _: Handle,
        name: *const c_char,
        out: &mut rsf_vk_function,
    ) -> i32 {
        record(unsafe { CStr::from_ptr(name) }.to_string_lossy());
        *out = Some(entry);
        OK
    }

    unsafe extern "C" fn bind_core(_: Handle, entries: &CoreEntries) {
        assert!(entries.iter().all(Option::is_some));
        record("bind core");
    }

    unsafe extern "C" fn read_memory(_: Handle, _: rsf_vk_function) -> i32 {
        record("read memory");
        OK
    }

    unsafe extern "C" fn load_resources(_: Handle) -> i32 {
        record("load resources");
        OK
    }

    unsafe extern "C" fn initialize_capture(_: Handle, out: &mut Handle) -> i32 {
        record("initialize capture");
        *out = ptr::without_provenance_mut(2);
        OK
    }

    unsafe extern "C" fn bind_wsi(_: Handle, _: &WsiEntries) {
        record("bind wsi");
    }

    #[test]
    fn a_missing_wsi_entry_retires_the_unpublished_capsule_after_the_full_lookup_group() {
        let platform = Platform {
            allocate,
            close,
            symbol,
            device_proc,
            instance_proc,
            bind_core,
            read_memory,
            load_resources,
            initialize_capture,
            bind_wsi,
        };
        let (mut native, mut resources) = (ptr::null_mut(), ptr::null_mut());
        assert_eq!(
            open(
                &platform,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                640,
                360,
                &mut native,
                &mut resources
            ),
            INIT_FAILED,
        );
        assert!(native.is_null() && resources.is_null());
        CALLS.with_borrow(|calls| {
            assert_eq!(
                calls.as_slice(),
                [
                    "allocate",
                    "ffxConfigure",
                    "ffxCreateContext",
                    "ffxDestroyContext",
                    "ffxQuery",
                    "ffxDispatch",
                    "1:vkCmdPipelineBarrier",
                    "1:vkCmdCopyImage",
                    "1:vkDeviceWaitIdle",
                    "bind core",
                    "vkGetPhysicalDeviceMemoryProperties",
                    "read memory",
                    "load resources",
                    "initialize capture",
                    "rsfFfxSetVulkanWsiDispatch",
                    "0:vkQueuePresentKHR",
                    "0:vkAcquireNextImageKHR",
                    "0:vkGetSwapchainImagesKHR",
                    "0:vkCreateSwapchainKHR",
                    "0:vkDestroySwapchainKHR",
                    "close",
                ],
            );
        });
    }
}
