// SPDX-License-Identifier: MIT
use super::{Lookup, NOT_SUPPORTED, fallback_admit, rsf_srvk_fallback, rsf_vk_get_proc};
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::FEATURE_FAILED;
use std::ffi::{CStr, c_void};
use std::{mem, ptr};

const LINEAR_BLIT: vk::FormatFeatureFlags = vk::FormatFeatureFlags::from_raw(
    vk::FormatFeatureFlags::BLIT_SRC.as_raw()
        | vk::FormatFeatureFlags::BLIT_DST.as_raw()
        | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR.as_raw(),
);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_srvk_spatial_fallback(
    lookup: Option<Lookup>,
    call: *const rsf_srvk_fallback,
) -> i32 {
    let status = unsafe { fallback_admit(call) };
    if status != 0 {
        return status;
    }
    let Some(lookup) = lookup else {
        return NOT_SUPPORTED;
    };
    let call = unsafe { &*call };
    let resolve = |proc: rsf_vk_get_proc, object: *mut c_void, name: &CStr| {
        let mut address = None;
        let ok = unsafe { lookup(proc.unwrap(), object, name.as_ptr(), &mut address) };
        (ok != 0).then_some(address).ok_or(FEATURE_FAILED)
    };
    let load = || {
        Ok::<_, i32>((
            resolve(
                call.instance_proc,
                call.instance,
                c"vkGetPhysicalDeviceFormatProperties",
            )?,
            resolve(call.device_proc, call.device, c"vkCmdPipelineBarrier")?,
            resolve(call.device_proc, call.device, c"vkCmdBlitImage")?,
        ))
    };
    let (get_properties, barrier, blit) = match load() {
        Ok((Some(get), Some(barrier), Some(blit))) => (get, barrier, blit),
        Ok(_) => return NOT_SUPPORTED,
        Err(error) => return error,
    };
    // Match the loader symbol to its Vulkan system-ABI function type.
    let get_properties = unsafe {
        mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkGetPhysicalDeviceFormatProperties>(
            get_properties,
        )
    };
    let barrier =
        unsafe { mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkCmdPipelineBarrier>(barrier) };
    let blit = unsafe { mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkCmdBlitImage>(blit) };
    let mut properties = vk::FormatProperties::default();
    unsafe {
        get_properties(
            vk::PhysicalDevice::from_raw(call.physical_device as u64),
            vk::Format::from_raw(call.color.format as i32),
            &mut properties,
        );
    }
    if !properties.optimal_tiling_features.contains(LINEAR_BLIT) {
        return NOT_SUPPORTED;
    }

    let source = &call.color;
    let target = &call.output;
    let commands = vk::CommandBuffer::from_raw(call.command_buffer as u64);
    let layers = vk::ImageSubresourceLayers::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .layer_count(1);
    let transition = |image, before, after, src_access, dst_access| {
        vk::ImageMemoryBarrier::default()
            .image(vk::Image::from_raw(image))
            .old_layout(before)
            .new_layout(after)
            .src_access_mask(src_access)
            .dst_access_mask(dst_access)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .level_count(1)
                    .layer_count(1),
            )
    };
    let record = |before, after, change: vk::ImageMemoryBarrier<'_>| unsafe {
        barrier(
            commands,
            before,
            after,
            vk::DependencyFlags::empty(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            1,
            &change,
        );
    };
    let all = vk::PipelineStageFlags::ALL_COMMANDS;
    let transfer = vk::PipelineStageFlags::TRANSFER;
    let read = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
    let copy_src = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
    let copy_dst = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
    record(
        all,
        transfer,
        transition(
            source.image,
            read,
            copy_src,
            vk::AccessFlags::SHADER_READ,
            vk::AccessFlags::TRANSFER_READ,
        ),
    );
    record(
        all,
        transfer,
        transition(
            target.image,
            copy_dst,
            copy_dst,
            vk::AccessFlags::MEMORY_WRITE,
            vk::AccessFlags::TRANSFER_WRITE,
        ),
    );

    let offsets = |width, height| {
        [
            vk::Offset3D::default(),
            vk::Offset3D {
                x: width,
                y: height,
                z: 1,
            },
        ]
    };
    let region = vk::ImageBlit::default()
        .src_subresource(layers)
        .dst_subresource(layers)
        .src_offsets(offsets(source.width as i32, source.height as i32))
        .dst_offsets(offsets(target.width as i32, target.height as i32));
    unsafe {
        blit(
            commands,
            vk::Image::from_raw(source.image),
            copy_src,
            vk::Image::from_raw(target.image),
            copy_dst,
            1,
            &region,
            vk::Filter::LINEAR,
        );
    }
    record(
        transfer,
        all,
        transition(
            source.image,
            copy_src,
            read,
            vk::AccessFlags::TRANSFER_READ,
            vk::AccessFlags::SHADER_READ,
        ),
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::{RSF_SRVK_ABI_VERSION, rsf_srvk_image, rsf_vk_function};
    use crate::vulkan_sr::INVALID_ARGUMENT;
    use std::ffi::c_char;

    #[derive(Debug, PartialEq)]
    enum Operation {
        Transition(u64, i32, i32, u32, u32),
        Blit([i32; 2], [i32; 2]),
    }

    #[derive(Default)]
    struct Fake {
        features: vk::FormatFeatureFlags,
        commands: Vec<Operation>,
    }

    unsafe extern "system" fn properties(
        physical: vk::PhysicalDevice,
        format: vk::Format,
        out: *mut vk::FormatProperties,
    ) {
        assert!(format == vk::Format::R16G16B16A16_SFLOAT);
        let device = unsafe { &*(physical.as_raw() as *const Fake) };
        unsafe { (*out).optimal_tiling_features = device.features };
    }

    unsafe extern "system" fn barrier(
        commands: vk::CommandBuffer,
        source: vk::PipelineStageFlags,
        target: vk::PipelineStageFlags,
        _: vk::DependencyFlags,
        _: u32,
        _: *const vk::MemoryBarrier<'_>,
        _: u32,
        _: *const vk::BufferMemoryBarrier<'_>,
        count: u32,
        changes: *const vk::ImageMemoryBarrier<'_>,
    ) {
        assert_eq!(count, 1);
        let device = unsafe { &mut *(commands.as_raw() as *mut Fake) };
        let change = unsafe { &*changes };
        device.commands.push(Operation::Transition(
            change.image.as_raw(),
            change.old_layout.as_raw(),
            change.new_layout.as_raw(),
            source.as_raw(),
            target.as_raw(),
        ));
    }

    unsafe extern "system" fn blit(
        commands: vk::CommandBuffer,
        source: vk::Image,
        _: vk::ImageLayout,
        target: vk::Image,
        _: vk::ImageLayout,
        count: u32,
        region: *const vk::ImageBlit,
        filter: vk::Filter,
    ) {
        assert_eq!(
            (source.as_raw(), target.as_raw(), count, filter.as_raw()),
            (11, 12, 1, 1)
        );
        let device = unsafe { &mut *(commands.as_raw() as *mut Fake) };
        let region = unsafe { &*region };
        device.commands.push(Operation::Blit(
            [region.src_offsets[1].x, region.src_offsets[1].y],
            [region.dst_offsets[1].x, region.dst_offsets[1].y],
        ));
    }

    unsafe extern "C" fn proc(_: *mut c_void, name: *const c_char) -> rsf_vk_function {
        let name = unsafe { CStr::from_ptr(name) };
        let entry = match name.to_bytes() {
            b"vkGetPhysicalDeviceFormatProperties" => {
                properties as vk::PFN_vkGetPhysicalDeviceFormatProperties as *const ()
            }
            b"vkCmdPipelineBarrier" => barrier as vk::PFN_vkCmdPipelineBarrier as *const (),
            b"vkCmdBlitImage" => blit as vk::PFN_vkCmdBlitImage as *const (),
            _ => return None,
        };
        Some(unsafe { mem::transmute::<*const (), unsafe extern "C" fn()>(entry) })
    }

    unsafe extern "C" fn lookup(
        proc: unsafe extern "C" fn(*mut c_void, *const c_char) -> rsf_vk_function,
        object: *mut c_void,
        name: *const c_char,
        out: *mut rsf_vk_function,
    ) -> i32 {
        unsafe { out.write(proc(object, name)) };
        1
    }

    #[test]
    fn validates_before_recording_the_linear_blit() {
        let mut fake = Fake::default();
        let device = ptr::from_mut(&mut fake).cast();
        let color = rsf_srvk_image {
            device,
            image: 11,
            format: vk::Format::R16G16B16A16_SFLOAT.as_raw() as u32,
            layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32,
            width: 64,
            height: 36,
            copy_source: 1,
        };
        let mut input = rsf_srvk_fallback {
            struct_size: size_of::<rsf_srvk_fallback>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: device,
            physical_device: device,
            device,
            command_buffer: device,
            instance_proc: Some(proc),
            device_proc: Some(proc),
            color,
            output: rsf_srvk_image {
                image: 12,
                width: 128,
                height: 72,
                layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL.as_raw() as u32,
                ..color
            },
        };
        assert_eq!(
            unsafe { rsf_core_srvk_spatial_fallback(None, &input) },
            NOT_SUPPORTED
        );
        assert_eq!(
            unsafe { rsf_core_srvk_spatial_fallback(Some(lookup), &input) },
            NOT_SUPPORTED
        );
        assert!(fake.commands.is_empty());
        fake.features = LINEAR_BLIT;
        assert_eq!(
            unsafe { rsf_core_srvk_spatial_fallback(Some(lookup), &input) },
            0
        );
        let all = vk::PipelineStageFlags::ALL_COMMANDS.as_raw();
        let transfer = vk::PipelineStageFlags::TRANSFER.as_raw();
        assert_eq!(
            fake.commands,
            [
                Operation::Transition(11, 5, 6, all, transfer),
                Operation::Transition(12, 7, 7, all, transfer),
                Operation::Blit([64, 36], [128, 72]),
                Operation::Transition(11, 6, 5, transfer, all),
            ]
        );
        fake.commands.clear();
        input.output.image = input.color.image;
        assert_eq!(
            unsafe { rsf_core_srvk_spatial_fallback(Some(lookup), &input) },
            INVALID_ARGUMENT
        );
        assert!(fake.commands.is_empty());
    }
}
