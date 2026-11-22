// SPDX-License-Identifier: MIT
use super::hooks::{bind_named, rsf_census_hook_original};
use super::json::Json;
use super::platform;
use super::rsf_census_call_state;
use super::session::{rsf_census_active, rsf_census_call_begin, rsf_census_call_finish};
use super::stats::{
    rsf_census_present_enter, rsf_census_present_leave, rsf_census_stats_queue_describe,
    rsf_census_stats_queue_submit,
};
use crate::sys::*;
use std::ffi::{CStr, c_char, c_void};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::null_mut;
use std::sync::OnceLock;

type Handle = *mut c_void;
type Info = *const c_void;
type VkResult = i32;
type Entry = rsf_census_vk_entry;
const COUNT: usize = RSF_CENSUS_VK_COUNT as usize;
const VULKAN_QUEUE: u8 = 2;

struct Services {
    entries: [u16; COUNT],
}
static SERVICES: OnceLock<Services> = OnceLock::new();

fn services() -> &'static Services {
    SERVICES
        .get()
        .expect("Vulkan wrapper registered without services")
}

fn id(entry: Entry) -> u16 {
    services().entries[entry as usize]
}

// Capture from the actual entry wrapper, before going through another Rust or native call frame.
struct Call<'a> {
    storage: *mut rsf_census_call_state,
    _borrow: PhantomData<&'a mut MaybeUninit<rsf_census_call_state>>,
}

impl<'a> Call<'a> {
    fn begin(
        storage: &'a mut MaybeUninit<rsf_census_call_state>,
        entry: Entry,
        caller: Handle,
    ) -> Self {
        let storage = storage.as_mut_ptr();
        unsafe { rsf_census_call_begin(storage, id(entry), caller) };
        Self {
            storage,
            _borrow: PhantomData,
        }
    }

    fn json(&mut self) -> Option<Json<'_>> {
        unsafe {
            (*self.storage)
                .full
                .then(|| Json(&mut (*self.storage).json))
        }
    }

    fn finish(self, result: u64) {
        unsafe { rsf_census_call_finish(self.storage, result) };
    }
}

trait VulkanJson {
    fn output(&mut self, key: &CStr, out: Info);
}

impl VulkanJson for Json<'_> {
    fn output(&mut self, key: &CStr, out: Info) {
        if let Some(bytes) = read::<8>(out, 0) {
            self.hex(Some(key), u64::from_le_bytes(bytes));
        } else {
            self.raw(key, c"null");
        }
    }
}

fn read<const N: usize>(address: Info, offset: usize) -> Option<[u8; N]> {
    let base = address as usize;
    if !(0x10000..0x0000_7fff_ffff_0000).contains(&base) {
        return None;
    }
    let address = base.checked_add(offset)? as Info;
    let mut bytes = [0; N];
    unsafe { platform().read.unwrap()(address, bytes.as_mut_ptr().cast(), N) }.then_some(bytes)
}

fn number(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn word(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn read_name(source: Info, output: &mut [u8; 96]) -> bool {
    unsafe { platform().read_string.unwrap()(source, output.as_mut_ptr().cast(), output.len()) }
}

fn name_list(json: &mut Json<'_>, key: &CStr, names: Info, count: u32) {
    json.open(Some(key), b'[');
    for index in 0..count.min(96) as usize {
        let Some(entry) = read::<8>(names, index * 8) else {
            break;
        };
        let mut name = [0; 96];
        if !read_name(u64::from_le_bytes(entry) as Info, &mut name) {
            break;
        }
        if let Ok(name) = CStr::from_bytes_until_nul(&name) {
            json.text(None, name);
        }
    }
    json.close(b']');
}

fn proc_addr(entry: Entry, caller: Handle, handle: Handle, name: *const c_char) -> Handle {
    let original = original!(
        entry,
        unsafe extern "system" fn(Handle, *const c_char) -> Handle
    );
    let mut storage = MaybeUninit::uninit();
    let mut record = Call::begin(&mut storage, entry, caller);
    let mut copied_name = [0; 96];
    let have_name = read_name(name.cast(), &mut copied_name);
    let result = original.map_or(null_mut(), |call| unsafe { call(handle, name) });
    if let Some(mut json) = record.json() {
        json.hex(Some(c"handle"), handle as u64);
        if let Ok(name) = CStr::from_bytes_until_nul(&copied_name) {
            json.text(Some(c"name"), name);
        }
        json.hex(Some(c"result"), result as u64);
    }
    record.finish(result as u64);
    if !result.is_null() && have_name {
        CStr::from_bytes_until_nul(&copied_name)
            .ok()
            .and_then(|name| bind_named(c"vulkan", name, result))
            .unwrap_or(result)
    } else {
        result
    }
}

#[inline(never)]
extern "system" fn get_instance_proc(instance: Handle, name: *const c_char) -> Handle {
    proc_addr(RSF_CENSUS_VkGetInstanceProcAddr, caller!(), instance, name)
}

#[inline(never)]
extern "system" fn get_device_proc(device: Handle, name: *const c_char) -> Handle {
    proc_addr(RSF_CENSUS_VkGetDeviceProcAddr, caller!(), device, name)
}

fn instance_info(json: &mut Json<'_>, info: Info) {
    let Some(info) = read::<64>(info, 0) else {
        json.raw(c"create_info_unreadable", c"true");
        return;
    };
    if let Some(app) = read::<48>(word(&info, 24) as Info, 0) {
        for (key, offset) in [(c"application", 16), (c"engine", 32)] {
            let mut name = [0; 96];
            if read_name(word(&app, offset) as Info, &mut name)
                && let Ok(name) = CStr::from_bytes_until_nul(&name)
            {
                json.text(Some(key), name);
            }
        }
        json.hex(Some(c"application_version"), number(&app, 24).into());
        json.hex(Some(c"engine_version"), number(&app, 40).into());
        let api = number(&app, 44);
        json.number(Some(c"api_major"), (api >> 22) & 0x7f);
        json.number(Some(c"api_minor"), (api >> 12) & 0x3ff);
    }
    name_list(json, c"layers", word(&info, 40) as Info, number(&info, 32));
    name_list(
        json,
        c"extensions",
        word(&info, 56) as Info,
        number(&info, 48),
    );
}

#[inline(never)]
extern "system" fn create_instance(info: Info, allocator: Info, out: *mut Handle) -> VkResult {
    let entry = RSF_CENSUS_VkCreateInstance;
    let caller = caller!();
    let original = original!(
        entry,
        unsafe extern "system" fn(Info, Info, *mut Handle) -> VkResult
    );
    let mut storage = MaybeUninit::uninit();
    let mut record = Call::begin(&mut storage, entry, caller);
    if let Some(mut json) = record.json() {
        instance_info(&mut json, info);
    }
    let result = original.map_or(0, |call| unsafe { call(info, allocator, out) });
    if let Some(mut json) = record.json() {
        json.output(c"instance", out.cast());
    }
    record.finish(u64::from(result as u32));
    result
}

fn device_info(json: &mut Json<'_>, physical: Handle, info: Info) {
    json.hex(Some(c"physical_device"), physical as u64);
    let Some(info) = read::<72>(info, 0) else {
        json.raw(c"create_info_unreadable", c"true");
        return;
    };
    json.open(Some(c"queue_families"), b'[');
    for index in 0..number(&info, 20).min(16) as usize {
        let Some(queue) = read::<40>(word(&info, 24) as Info, index * 40) else {
            break;
        };
        json.open(None, b'{');
        json.number(Some(c"family"), number(&queue, 20));
        json.number(Some(c"count"), number(&queue, 24));
        json.close(b'}');
    }
    json.close(b']');
    name_list(json, c"layers", word(&info, 40) as Info, number(&info, 32));
    name_list(
        json,
        c"extensions",
        word(&info, 56) as Info,
        number(&info, 48),
    );
}

#[inline(never)]
extern "system" fn create_device(
    physical: Handle,
    info: Info,
    allocator: Info,
    out: *mut Handle,
) -> VkResult {
    let entry = RSF_CENSUS_VkCreateDevice;
    let caller = caller!();
    let original = original!(
        entry,
        unsafe extern "system" fn(Handle, Info, Info, *mut Handle) -> VkResult
    );
    let mut storage = MaybeUninit::uninit();
    let mut record = Call::begin(&mut storage, entry, caller);
    if let Some(mut json) = record.json() {
        device_info(&mut json, physical, info);
    }
    let result = original.map_or(0, |call| unsafe { call(physical, info, allocator, out) });
    if let Some(mut json) = record.json() {
        json.output(c"device", out.cast());
    }
    record.finish(u64::from(result as u32));
    result
}

#[inline(never)]
extern "system" fn get_queue(device: Handle, family: u32, index: u32, out: *mut Handle) {
    let caller = caller!();
    let entry = RSF_CENSUS_VkGetDeviceQueue;
    let original = original!(
        entry,
        unsafe extern "system" fn(Handle, u32, u32, *mut Handle)
    );
    let mut storage = MaybeUninit::uninit();
    let mut record = Call::begin(&mut storage, entry, caller);
    if let Some(mut json) = record.json() {
        json.hex(Some(c"device"), device as u64);
        json.number(Some(c"family"), family);
        json.number(Some(c"index"), index);
    }
    if let Some(call) = original {
        unsafe { call(device, family, index, out) };
    }
    if let Some(mut json) = record.json() {
        json.output(c"queue", out.cast());
    }
    if let Some(queue) = get_queue_handle(out) {
        describe_queue(queue, family, index);
    }
    record.finish(0);
}

fn get_queue_handle(out: *mut Handle) -> Option<Handle> {
    read::<8>(out.cast(), 0)
        .map(u64::from_le_bytes)
        .filter(|&queue| queue != 0)
        .map(|queue| queue as Handle)
}

fn describe_queue(queue: Handle, family: u32, index: u32) {
    if rsf_census_active() {
        rsf_census_stats_queue_describe(VULKAN_QUEUE, queue, family, index, 0);
    }
}

#[inline(never)]
extern "system" fn get_queue2(device: Handle, info: Info, out: *mut Handle) {
    let entry = RSF_CENSUS_VkGetDeviceQueue2;
    let caller = caller!();
    let original = original!(entry, unsafe extern "system" fn(Handle, Info, *mut Handle));
    let mut storage = MaybeUninit::uninit();
    let mut record = Call::begin(&mut storage, entry, caller);
    if let Some(mut json) = record.json() {
        json.hex(Some(c"device"), device as u64);
        if let Some(info) = read::<32>(info, 0) {
            json.number(Some(c"family"), number(&info, 20));
            json.number(Some(c"index"), number(&info, 24));
        }
    }
    if let Some(call) = original {
        unsafe { call(device, info, out) };
    }
    if let Some(mut json) = record.json() {
        json.output(c"queue", out.cast());
    }
    if let Some(queue) = get_queue_handle(out)
        && let Some(info) = read::<32>(info, 0)
    {
        describe_queue(queue, number(&info, 20), number(&info, 24));
    }
    record.finish(0);
}

fn sum_submit<const STRIDE: usize>(info: Info, count: u32, offsets: [usize; 3]) -> [u32; 3] {
    let mut totals = [0u32; 3];
    for index in 0..count.min(16) as usize {
        let Some(batch) = read::<STRIDE>(info, index * STRIDE) else {
            break;
        };
        for (total, offset) in totals.iter_mut().zip(offsets) {
            *total = total.wrapping_add(number(&batch, offset));
        }
    }
    totals
}

fn submit(
    entry: Entry,
    caller: Handle,
    queue: Handle,
    count: u32,
    info: Info,
    fence: u64,
) -> VkResult {
    let original = original!(
        entry,
        unsafe extern "system" fn(Handle, u32, Info, u64) -> VkResult
    );
    let mut storage = MaybeUninit::uninit();
    let mut record = Call::begin(&mut storage, entry, caller);
    if let Some(mut json) = record.json() {
        json.hex(Some(c"queue"), queue as u64);
        json.number(Some(c"batches"), count);
        json.hex(Some(c"fence"), fence);
        let [commands, waits, signals] = if entry == RSF_CENSUS_VkQueueSubmit {
            sum_submit::<72>(info, count, [40, 16, 56])
        } else {
            sum_submit::<64>(info, count, [32, 20, 48])
        };
        json.number(Some(c"command_buffers_in_first_16"), commands);
        json.number(Some(c"wait_semaphores"), waits);
        json.number(Some(c"signal_semaphores"), signals);
    }
    let result = original.map_or(0, |call| unsafe { call(queue, count, info, fence) });
    if rsf_census_active() {
        rsf_census_stats_queue_submit(VULKAN_QUEUE, queue, count);
    }
    record.finish(u64::from(result as u32));
    result
}

#[inline(never)]
extern "system" fn queue_submit(queue: Handle, count: u32, info: Info, fence: u64) -> VkResult {
    submit(
        RSF_CENSUS_VkQueueSubmit,
        caller!(),
        queue,
        count,
        info,
        fence,
    )
}

#[inline(never)]
extern "system" fn queue_submit2(queue: Handle, count: u32, info: Info, fence: u64) -> VkResult {
    submit(
        RSF_CENSUS_VkQueueSubmit2,
        caller!(),
        queue,
        count,
        info,
        fence,
    )
}

#[inline(never)]
extern "system" fn queue_submit2_khr(
    queue: Handle,
    count: u32,
    info: Info,
    fence: u64,
) -> VkResult {
    submit(
        RSF_CENSUS_VkQueueSubmit2KHR,
        caller!(),
        queue,
        count,
        info,
        fence,
    )
}

fn present_info(json: &mut Json<'_>, queue: Handle, info: Info) {
    json.hex(Some(c"queue"), queue as u64);
    if let Some(info) = read::<64>(info, 0) {
        let count = number(&info, 32);
        json.number(Some(c"wait_semaphores"), number(&info, 16));
        json.number(Some(c"swapchain_count"), count);
        json.open(Some(c"swapchains"), b'[');
        for index in 0..count.min(4) as usize {
            let Some(chain) = read::<8>(word(&info, 40) as Info, index * 8) else {
                break;
            };
            json.hex(None, u64::from_le_bytes(chain));
        }
        json.close(b']');
        json.open(Some(c"image_indices"), b'[');
        for index in 0..count.min(4) as usize {
            let Some(image) = read::<4>(word(&info, 48) as Info, index * 4) else {
                break;
            };
            json.number(None, u32::from_le_bytes(image));
        }
        json.close(b']');
    }
}

#[inline(never)]
extern "system" fn queue_present(queue: Handle, info: Info) -> VkResult {
    let entry = RSF_CENSUS_VkQueuePresentKHR;
    let caller = caller!();
    let original = original!(entry, unsafe extern "system" fn(Handle, Info) -> VkResult);
    rsf_census_present_enter(false);
    let mut storage = MaybeUninit::uninit();
    let mut record = Call::begin(&mut storage, entry, caller);
    if let Some(mut json) = record.json() {
        present_info(&mut json, queue, info);
    }
    let result = original.map_or(0, |call| unsafe { call(queue, info) });
    record.finish(u64::from(result as u32));
    rsf_census_present_leave();
    result
}

fn swapchain_info(json: &mut Json<'_>, device: Handle, info: Info) {
    json.hex(Some(c"device"), device as u64);
    let Some(info) = read::<104>(info, 0) else {
        json.raw(c"create_info_unreadable", c"true");
        return;
    };
    json.hex(Some(c"surface"), word(&info, 24));
    for (name, offset) in [
        (c"min_image_count", 32),
        (c"image_format", 36),
        (c"image_color_space", 40),
        (c"width", 44),
        (c"height", 48),
        (c"array_layers", 52),
    ] {
        json.number(Some(name), number(&info, offset));
    }
    json.hex(Some(c"image_usage"), number(&info, 56).into());
    json.number(Some(c"sharing_mode"), number(&info, 60));
    json.hex(Some(c"pre_transform"), number(&info, 80).into());
    json.hex(Some(c"composite_alpha"), number(&info, 84).into());
    let mode = number(&info, 88);
    const MODES: [&CStr; 4] = [c"IMMEDIATE", c"MAILBOX", c"FIFO", c"FIFO_RELAXED"];
    json.number(Some(c"present_mode"), mode);
    json.text(
        Some(c"present_mode_name"),
        MODES.get(mode as usize).copied().unwrap_or(c"other"),
    );
    json.raw(
        c"clipped",
        if number(&info, 92) != 0 {
            c"true"
        } else {
            c"false"
        },
    );
    json.hex(Some(c"old_swapchain"), word(&info, 96));
}

#[inline(never)]
extern "system" fn create_swapchain(
    device: Handle,
    info: Info,
    allocator: Info,
    out: *mut u64,
) -> VkResult {
    let entry = RSF_CENSUS_VkCreateSwapchainKHR;
    let caller = caller!();
    let original = original!(
        entry,
        unsafe extern "system" fn(Handle, Info, Info, *mut u64) -> VkResult
    );
    let mut storage = MaybeUninit::uninit();
    let mut record = Call::begin(&mut storage, entry, caller);
    if let Some(mut json) = record.json() {
        swapchain_info(&mut json, device, info);
    }
    let result = original.map_or(0, |call| unsafe { call(device, info, allocator, out) });
    if let Some(mut json) = record.json()
        && let Some(chain) = read::<8>(out.cast(), 0)
    {
        json.hex(Some(c"swapchain"), u64::from_le_bytes(chain));
    }
    record.finish(u64::from(result as u32));
    result
}

pub(super) unsafe fn rsf_census_vk_bind(ids: &[u16; COUNT], wrappers: &mut [Handle; COUNT]) {
    let _ = SERVICES.set(Services { entries: *ids });
    let entries: [(Entry, Handle); COUNT] = [
        (
            RSF_CENSUS_VkGetInstanceProcAddr,
            get_instance_proc as Handle,
        ),
        (RSF_CENSUS_VkGetDeviceProcAddr, get_device_proc as Handle),
        (RSF_CENSUS_VkCreateInstance, create_instance as Handle),
        (RSF_CENSUS_VkCreateDevice, create_device as Handle),
        (RSF_CENSUS_VkGetDeviceQueue, get_queue as Handle),
        (RSF_CENSUS_VkGetDeviceQueue2, get_queue2 as Handle),
        (RSF_CENSUS_VkQueueSubmit, queue_submit as Handle),
        (RSF_CENSUS_VkQueueSubmit2, queue_submit2 as Handle),
        (RSF_CENSUS_VkQueueSubmit2KHR, queue_submit2_khr as Handle),
        (RSF_CENSUS_VkQueuePresentKHR, queue_present as Handle),
        (RSF_CENSUS_VkCreateSwapchainKHR, create_swapchain as Handle),
    ];
    for (index, wrapper) in entries {
        wrappers[index as usize] = wrapper;
    }
}
