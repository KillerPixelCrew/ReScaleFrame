// SPDX-License-Identifier: MIT
use crate::streamline_abi::*;
use ash::vk::{self, Handle as _};
use std::{
    cell::Cell,
    ffi::{CStr, c_char},
};

type Barrier = vk::ImageMemoryBarrier<'static>;

/// The barriers the corrected dispatch records for one requested barrier.
fn initialized(requested: &Barrier) -> Vec<Barrier> {
    let mut output = [Barrier::default(); 8];
    let count = unsafe {
        rsf_fixture_sl_dispatch_barriers(
            std::ptr::from_ref(requested).cast(),
            output.as_mut_ptr().cast(),
            output.len() as u32,
        )
    };
    output[..count as usize].to_vec()
}

/// SDK-created images enter their first layout once per mip, layer and
/// aspect, keep combined depth and stencil aspects together, and keep the
/// contents of initialized subresources. Formerly sdk-fixes/dispatch-tests.cpp.
fn first_use_barriers() {
    let both = vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL;
    let tracked = || unsafe { rsf_fixture_sl_dispatch_tracked(1) } != 0;
    unsafe { rsf_fixture_sl_dispatch_fresh(1, 2, 2, both.as_raw()) };
    let mut requested = Barrier {
        image: vk::Image::from_raw(1),
        old_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        new_layout: vk::ImageLayout::GENERAL,
        subresource_range: vk::ImageSubresourceRange {
            aspect_mask: both,
            base_mip_level: 0,
            level_count: vk::REMAINING_MIP_LEVELS,
            base_array_layer: 0,
            layer_count: vk::REMAINING_ARRAY_LAYERS,
        },
        ..Default::default()
    };
    let first = initialized(&requested);
    assert_eq!(first.len(), 4);
    for barrier in &first {
        assert!(barrier.old_layout == vk::ImageLayout::UNDEFINED);
        // Combined aspects stay combined without separateDepthStencilLayouts.
        assert!(barrier.subresource_range.aspect_mask == both);
        let range = barrier.subresource_range;
        assert_eq!((range.level_count, range.layer_count), (1, 1));
    }
    assert!(!tracked());
    requested.old_layout = requested.new_layout;
    let reused = initialized(&requested);
    assert!(reused.len() == 1 && reused[0].old_layout == vk::ImageLayout::GENERAL);
    let color = vk::ImageAspectFlags::COLOR;
    unsafe { rsf_fixture_sl_dispatch_fresh(1, 2, 1, color.as_raw()) };
    requested.subresource_range = vk::ImageSubresourceRange {
        aspect_mask: color,
        base_mip_level: 1,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    let partial = initialized(&requested);
    assert!(partial.len() == 1 && partial[0].old_layout == vk::ImageLayout::UNDEFINED);
    assert!(tracked());
    let same_mip = initialized(&requested);
    assert!(same_mip[0].old_layout == requested.old_layout);
    requested.subresource_range.base_mip_level = 0;
    let remaining = initialized(&requested);
    assert!(remaining[0].old_layout == vk::ImageLayout::UNDEFINED);
    assert!(!tracked());
}

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
    first_use_barriers();
    println!("Streamline NGX fill-buffer dependency and first-use layout checks passed");
    0
}
