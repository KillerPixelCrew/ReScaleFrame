// SPDX-License-Identifier: MIT
use super::*;

pub(super) fn execute(
    frame: Frame,
    kind: u32,
    reconfigure: bool,
    textures: &[rsf_uvk_engine_image; 4],
) -> Result<(), &'static CStr> {
    if frame.services.render_pass.is_none() {
        return Ok(());
    }
    let graphics = match initialize(&frame.services) {
        Device::Ready(graphics) => graphics,
        Device::Busy => return Ok(()),
        Device::Failed => {
            fault(INIT_FAILED, c"Unity Vulkan shared device/fence initialization failed; native output remains in place.");
            return Ok(());
        }
    };
    let (ring, slot) = begin(&graphics).ok_or(SLOT_FAILED)?;
    if reconfigure && !unsafe { rsf_uvk_gpu_drain(&graphics) } {
        return Err(c"Unity Vulkan GPU drain before reconfiguration failed; the bridge stopped.");
    }
    let camera = frame.packet.camera;
    let output = [camera.output_width, camera.output_height];
    let probe = frame.packet.flags & RSF_UNITY_PACKET_PROBE != 0;
    let selected = |index| !probe || !matches!(index, 1 | 2);
    let mut images = [ImageHandles::EMPTY; 4];
    let target = match kind {
        EVENT_WINDOW | EVENT_HUDLESS => {
            let index = if kind == EVENT_WINDOW { 5 } else { 4 };
            let view = prepare_rgba(index, output[0], output[1]).ok_or(IMAGE_FAILED)?;
            record_input(&graphics, slot.vk[0], index, view, textures[0], output);
            Some(view)
        }
        _ => {
            let formats = [
                (
                    vk::Format::R16G16B16A16_SFLOAT,
                    DXGI_FORMAT_R16G16B16A16_FLOAT,
                ),
                (vk::Format::R32_SFLOAT, DXGI_FORMAT_R32_FLOAT),
                (vk::Format::R16G16_SFLOAT, DXGI_FORMAT_R16G16_FLOAT),
                (
                    vk::Format::R16G16B16A16_SFLOAT,
                    DXGI_FORMAT_R16G16B16A16_FLOAT,
                ),
            ];
            for (index, ((view, texture), (format, dx_format))) in
                images.iter_mut().zip(textures).zip(formats).enumerate()
            {
                if selected(index) {
                    let [width, height] = if index == 3 {
                        output
                    } else {
                        [camera.render_width, camera.render_height]
                    };
                    *view = prepare_image(index as u32, width, height, format, dx_format)
                        .ok_or(IMAGE_FAILED)?;
                    record_input(
                        &graphics,
                        slot.vk[0],
                        index as u32,
                        *view,
                        *texture,
                        [width, height],
                    );
                }
            }
            None
        }
    };
    if !unsafe { rsf_uvk_gpu_upload(&graphics, &slot, ptr::null(), 0) } {
        return Err(c"Unity Vulkan upload submission failed; the bridge stopped.");
    }
    let mut pass = base_pass(&frame.packet);
    if reconfigure {
        pass.flags |= RSF_GAME_RENDER_RECONFIGURE;
        pass.camera_valid = 0;
    }
    match target {
        Some(view) if kind == EVENT_WINDOW => {
            pass.role = RSF_GAME_RENDER_WINDOW;
            pass.color_input = view.resource;
            pass.flags |= RSF_UNITY_PREVIEW_SOURCE_COMMON;
            if !render(&frame, graphics.queue, &pass, 1) {
                return Err(QUEUE_EXCEPTION);
            }
        }
        Some(view) => {
            pass.role = RSF_GAME_RENDER_FINAL_SCENE;
            pass.color_input = view.resource;
            barrier(
                &slot,
                view.resource,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            );
            if !render(&frame, slot.commands, &pass, 1) {
                return Err(QUEUE_EXCEPTION);
            }
            barrier(
                &slot,
                view.resource,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_COMMON,
            );
        }
        None => {
            let state = |index| {
                if index == 3 {
                    D3D12_RESOURCE_STATE_COPY_DEST
                } else {
                    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
                }
            };
            for (index, (view, input)) in images
                .into_iter()
                .zip([
                    &mut pass.color_input,
                    &mut pass.depth,
                    &mut pass.motion,
                    &mut pass.color_output,
                ])
                .enumerate()
            {
                if selected(index) {
                    *input = view.resource;
                    barrier(
                        &slot,
                        view.resource,
                        D3D12_RESOURCE_STATE_COMMON,
                        state(index),
                    );
                }
            }
            if !render(&frame, slot.commands, &pass, 1) {
                return Err(QUEUE_EXCEPTION);
            }
            for (index, view) in images.into_iter().enumerate() {
                if selected(index) {
                    barrier(
                        &slot,
                        view.resource,
                        state(index),
                        D3D12_RESOURCE_STATE_COMMON,
                    );
                }
            }
            if !probe && !reconfigure {
                unsafe {
                    rsf_uvk_gpu_image_transition(
                        &graphics,
                        slot.vk[1],
                        images[3].image,
                        3,
                        true,
                        true,
                    );
                    rsf_uvk_gpu_copy(
                        &graphics,
                        slot.vk[1],
                        images[3].image,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL.as_raw(),
                        vk::ImageAspectFlags::COLOR.as_raw(),
                        textures[3].image,
                        textures[3].layout,
                        output[0],
                        output[1],
                    );
                    rsf_uvk_gpu_image_transition(
                        &graphics,
                        slot.vk[1],
                        images[3].image,
                        0,
                        true,
                        false,
                    );
                }
            }
        }
    }
    if !unsafe { rsf_uvk_gpu_submit(&graphics, &slot, ring, 0) } {
        return Err(c"Unity Vulkan render submission failed; the bridge stopped.");
    }
    if !render(&frame, slot.commands, &pass, 0) {
        return Err(QUEUE_EXCEPTION);
    }
    VULKAN.lock().advance(ring as usize);
    Ok(())
}
