// SPDX-License-Identifier: MIT
use super::{Handle, Image, READ, TestResult, check, raw};
use crate::frame_capture_vk::*;
use crate::sys::rsf_vk_function;
use ash::vk::{self, Handle as _};
use rsf_game_sys::rsf_frame_record;
use std::{
    cell::Cell,
    ffi::{CStr, c_char, c_void},
    fs, ptr,
};
use windows_sys::Win32::System::Environment::SetEnvironmentVariableW;

#[derive(Default)]
struct Device {
    pixels: [u8; 4],
    mapped: Cell<u32>,
    released: Cell<u32>,
}

#[derive(Default)]
struct Commands(Cell<*mut Cell<bool>>);

unsafe extern "C" fn capture_proc(_: Handle, name: *const c_char) -> rsf_vk_function {
    macro_rules! leaf {
        ($ty:ident, ($($name:ident: $param:ty),*) $(-> $ret:ty)? $body:block) => {{
            unsafe extern "system" fn call($($name: $param),*) $(-> $ret)? $body
            rsf_test_support::vk_entry::<vk::$ty>(call)
        }};
    }
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkCreateBuffer" => {
            leaf!(PFN_vkCreateBuffer, (_device: vk::Device, _info: *const vk::BufferCreateInfo<'_>,
                _allocator: *const vk::AllocationCallbacks<'_>, out: *mut vk::Buffer) -> vk::Result {
                unsafe { out.write(vk::Buffer::from_raw(1)) };
                vk::Result::SUCCESS
            })
        }
        b"vkDestroyBuffer" => {
            leaf!(PFN_vkDestroyBuffer, (_device: vk::Device, _buffer: vk::Buffer, _allocator: *const vk::AllocationCallbacks<'_>) {})
        }
        b"vkGetBufferMemoryRequirements" => {
            leaf!(PFN_vkGetBufferMemoryRequirements, (_device: vk::Device, _buffer: vk::Buffer, out: *mut vk::MemoryRequirements) {
                unsafe { out.write(vk::MemoryRequirements { size: 4, alignment: 1, memory_type_bits: 1 }) };
            })
        }
        b"vkAllocateMemory" => {
            leaf!(PFN_vkAllocateMemory, (_device: vk::Device, _info: *const vk::MemoryAllocateInfo<'_>,
                _allocator: *const vk::AllocationCallbacks<'_>, out: *mut vk::DeviceMemory) -> vk::Result {
                unsafe { out.write(vk::DeviceMemory::from_raw(1)) };
                vk::Result::SUCCESS
            })
        }
        b"vkFreeMemory" => {
            leaf!(PFN_vkFreeMemory, (_device: vk::Device, _memory: vk::DeviceMemory, _allocator: *const vk::AllocationCallbacks<'_>) {})
        }
        b"vkBindBufferMemory" => {
            leaf!(PFN_vkBindBufferMemory, (_device: vk::Device, _buffer: vk::Buffer,
            _memory: vk::DeviceMemory, _offset: u64) -> vk::Result { vk::Result::SUCCESS })
        }
        b"vkMapMemory" => leaf!(PFN_vkMapMemory, (device: vk::Device, _memory: vk::DeviceMemory,
            _offset: u64, _size: u64, _flags: vk::MemoryMapFlags, out: *mut *mut c_void) -> vk::Result {
            let device = unsafe { &*raw(device).cast::<Device>() };
            device.mapped.set(device.mapped.get() + 1);
            unsafe { out.write(device.pixels.as_ptr().cast_mut().cast()) };
            vk::Result::SUCCESS
        }),
        b"vkUnmapMemory" => {
            leaf!(PFN_vkUnmapMemory, (_device: vk::Device, _memory: vk::DeviceMemory) {})
        }
        b"vkCmdPipelineBarrier" => leaf!(PFN_vkCmdPipelineBarrier, (_commands: vk::CommandBuffer,
            _source: vk::PipelineStageFlags, _target: vk::PipelineStageFlags, _flags: vk::DependencyFlags,
            _mem_count: u32, _mem: *const vk::MemoryBarrier<'_>, _buf_count: u32, _buffers: *const vk::BufferMemoryBarrier<'_>,
            _image_count: u32, _images: *const vk::ImageMemoryBarrier<'_>) {}),
        b"vkCmdCopyImageToBuffer" => {
            leaf!(PFN_vkCmdCopyImageToBuffer, (_commands: vk::CommandBuffer, _image: vk::Image,
            _layout: vk::ImageLayout, _buffer: vk::Buffer, _count: u32, _regions: *const vk::BufferImageCopy) {})
        }
        b"vkCreateEvent" => {
            leaf!(PFN_vkCreateEvent, (_device: vk::Device, _info: *const vk::EventCreateInfo<'_>,
                _allocator: *const vk::AllocationCallbacks<'_>, out: *mut vk::Event) -> vk::Result {
                unsafe { out.write(vk::Event::from_raw(Box::into_raw(Box::new(Cell::new(false))) as u64)) };
                vk::Result::SUCCESS
            })
        }
        b"vkCmdSetEvent" => {
            leaf!(PFN_vkCmdSetEvent, (commands: vk::CommandBuffer, event: vk::Event, _stages: vk::PipelineStageFlags) {
                unsafe { &*raw(commands).cast::<Commands>() }.0.set(raw(event).cast());
            })
        }
        b"vkGetEventStatus" => {
            leaf!(PFN_vkGetEventStatus, (_device: vk::Device, event: vk::Event) -> vk::Result {
                if unsafe { &*raw(event).cast::<Cell<bool>>() }.get() { vk::Result::EVENT_SET } else { vk::Result::EVENT_RESET }
            })
        }
        b"vkDestroyEvent" => {
            leaf!(PFN_vkDestroyEvent, (device: vk::Device, event: vk::Event, _allocator: *const vk::AllocationCallbacks<'_>) {
                let device = unsafe { &*raw(device).cast::<Device>() };
                device.released.set(device.released.get() + 1);
                drop(unsafe { Box::from_raw(raw(event).cast::<Cell<bool>>()) });
            })
        }
        _ => None,
    }
}

struct Capture(*mut Owner);
impl Drop for Capture {
    fn drop(&mut self) {
        unsafe { rsf_core_capture_vk_destroy(self.0) };
    }
}

pub(super) fn run() -> TestResult {
    let directory = std::env::temp_dir().join(format!("rsf-srvk-contract-{}", std::process::id()));
    let wide = rsf_test_support::wide_os(&directory);
    unsafe {
        SetEnvironmentVariableW(
            windows::core::w!("RSF_VULKAN_FRAME_CAPTURE_DIR").as_ptr(),
            wide.as_ptr(),
        );
        SetEnvironmentVariableW(
            windows::core::w!("RSF_FRAME_CAPTURE_FIRST").as_ptr(),
            windows::core::w!("1").as_ptr(),
        );
        SetEnvironmentVariableW(
            windows::core::w!("RSF_FRAME_CAPTURE_COUNT").as_ptr(),
            windows::core::w!("1").as_ptr(),
        );
    }
    let device = Device::default();
    let commands = [
        Commands::default(),
        Commands::default(),
        Commands::default(),
    ];
    let owner = Capture(rsf_core_capture_vk_create(1));
    let capture = owner.0;
    let mut memory = vk::PhysicalDeviceMemoryProperties {
        memory_type_count: 1,
        ..Default::default()
    };
    memory.memory_types[0].property_flags =
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let handle = ptr::from_ref(&device).cast_mut().cast();
    check(
        unsafe { rsf_core_capture_vk_initialize(capture, handle, Some(capture_proc), &memory) }
            != 0,
        "tracked capture initialized",
    )?;
    let frame = rsf_frame_record {
        session_id: 1,
        frame_id: 1,
        ..unsafe { std::mem::zeroed() }
    };
    let source = Image {
        device: handle,
        image: 1,
        format: vk::Format::R8_UNORM.as_raw() as u32,
        width: 2,
        height: 2,
        layout: READ.as_raw() as u32,
        copy_source: 1,
    };
    let command = |index: usize| ptr::from_ref(&commands[index]).cast_mut().cast();
    for (i, role) in [c"submitted", c"abandoned", c"drained"]
        .into_iter()
        .enumerate()
    {
        check(
            unsafe {
                rsf_core_capture_vk_record(
                    capture,
                    command(i),
                    &source,
                    &frame,
                    role.as_ptr(),
                    0,
                    0,
                    ptr::null_mut(),
                    0,
                )
            } != 0,
            "tracked readbacks recorded",
        )?;
    }
    for index in [0, 2] {
        let event = commands[index].0.get();
        check(!event.is_null(), "copy records its completion event")?;
        unsafe { &*event }.set(true);
    }
    let counts = || {
        (device.mapped.get(), device.released.get(), unsafe {
            rsf_core_capture_vk_pending(capture)
        })
    };
    check(
        unsafe { rsf_core_capture_vk_wants(capture, frame.frame_id) } != 0
            && device.mapped.get() == 0,
        "completed copy still needs retirement",
    )?;
    unsafe { rsf_core_capture_vk_retire(capture, command(0)) };
    check(
        counts() == (1, 1, 2),
        "retirement selects the exact command buffer",
    )?;
    unsafe { rsf_core_capture_vk_retire(capture, command(1)) };
    check(
        counts() == (1, 2, 1),
        "abandoned recording is discarded without mapping",
    )?;
    check(
        unsafe { rsf_core_capture_vk_flush(capture, 0, ptr::null_mut()) } != 0
            && counts() == (2, 3, 0),
        "full drain retires remaining executed readbacks",
    )?;
    check(
        fs::metadata(directory.join("1-1-v0-g0-submitted-0.raw"))?.len() == 4
            && !directory.join("1-1-v0-g0-abandoned-0.raw").exists(),
        "capture bytes only come from executed copies",
    )?;
    for filename in [
        "1-1-v0-g0-submitted-0.raw",
        "1-1-v0-g0-submitted-0.json",
        "1-1-v0-g0-drained-0.raw",
        "1-1-v0-g0-drained-0.json",
        "capture-errors.log",
    ] {
        let _ = fs::remove_file(directory.join(filename));
    }
    fs::remove_dir(directory)?;
    Ok(())
}
