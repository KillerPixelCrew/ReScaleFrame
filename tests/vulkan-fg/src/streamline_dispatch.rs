// SPDX-License-Identifier: MIT
use crate::streamline_abi::*;
use ash::vk::{self, Handle as _};
use std::{
    cell::Cell,
    ffi::{CStr, c_char},
};

thread_local! {
    static SEQUENCE: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "system" fn dependency(
    command: vk::CommandBuffer,
    source: vk::PipelineStageFlags,
    destination: vk::PipelineStageFlags,
    flags: vk::DependencyFlags,
    memory_count: u32,
    memory: *const vk::MemoryBarrier<'_>,
    buffer_count: u32,
    _: *const vk::BufferMemoryBarrier<'_>,
    image_count: u32,
    _: *const vk::ImageMemoryBarrier<'_>,
) {
    assert_eq!(
        (
            command.as_raw(),
            flags.as_raw(),
            memory_count,
            buffer_count,
            image_count
        ),
        (1, 0, 1, 0, 0)
    );
    let before = SEQUENCE.get().is_multiple_of(3);
    assert!(before || SEQUENCE.get() % 3 == 2);
    let all_memory = vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE;
    let transfer = vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::TRANSFER_WRITE;
    let all = vk::PipelineStageFlags::ALL_COMMANDS;
    let transfer_stage = vk::PipelineStageFlags::TRANSFER;
    let (src, dst, src_stage, dst_stage) = if before {
        (all_memory, transfer, all, transfer_stage)
    } else {
        (transfer, all_memory, transfer_stage, all)
    };
    let memory = unsafe { &*memory };
    assert_eq!(
        (
            memory.src_access_mask.as_raw(),
            memory.dst_access_mask.as_raw()
        ),
        (src.as_raw(), dst.as_raw())
    );
    assert_eq!(
        (source.as_raw(), destination.as_raw()),
        (src_stage.as_raw(), dst_stage.as_raw())
    );
    SEQUENCE.set(SEQUENCE.get() + 1);
}

unsafe extern "system" fn fill(
    command: vk::CommandBuffer,
    buffer: vk::Buffer,
    offset: u64,
    size: u64,
    data: u32,
) {
    assert_eq!(
        (
            SEQUENCE.get() % 3,
            command.as_raw(),
            buffer.as_raw(),
            offset,
            size,
            data
        ),
        (1, 1, 2, 16, 1024, 0x12345678)
    );
    SEQUENCE.set(SEQUENCE.get() + 1);
}

unsafe extern "system" fn resolve(_: vk::Device, name: *const c_char) -> vk::PFN_vkVoidFunction {
    let name = unsafe { CStr::from_ptr(name) };
    if name == c"vkCmdFillBuffer" {
        Some(unsafe {
            std::mem::transmute::<vk::PFN_vkCmdFillBuffer, unsafe extern "system" fn()>(fill)
        })
    } else if name == c"vkCmdPipelineBarrier" {
        Some(unsafe {
            std::mem::transmute::<vk::PFN_vkCmdPipelineBarrier, unsafe extern "system" fn()>(
                dependency,
            )
        })
    } else {
        None
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_sl_dispatch_run() -> i32 {
    unsafe {
        rsf_fixture_sl_dispatch_configure(3, resolve as *const () as usize);
        let name = c"vkCmdFillBuffer";
        let wrapped = rsf_fixture_sl_dispatch_get(3, name.as_ptr());
        assert_ne!(wrapped, 0);
        assert_eq!(wrapped, rsf_fixture_sl_dispatch_fill());
        assert_eq!(
            rsf_fixture_sl_dispatch_get(4, name.as_ptr()),
            fill as *const () as usize
        );
        let wrapped: vk::PFN_vkCmdFillBuffer = std::mem::transmute(wrapped);
        for _ in 0..3 {
            wrapped(
                vk::CommandBuffer::from_raw(1),
                vk::Buffer::from_raw(2),
                16,
                1024,
                0x12345678,
            );
        }
        assert_eq!(SEQUENCE.get(), 9);
    }
    println!("Streamline NGX fill-buffer dependency checks passed");
    0
}
