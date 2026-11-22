// SPDX-License-Identifier: MIT
use super::*;

const _: () = assert!(
    size_of::<vk::PresentInfoKHR<'static>>() == 64
        && std::mem::offset_of!(vk::PresentInfoKHR<'static>, p_swapchains) == 40
);

struct Present<'a> {
    queue: u64,
    info: &'a vk::PresentInfoKHR<'a>,
}

impl Present<'_> {
    fn forward(&self, info: &vk::PresentInfoKHR<'_>) -> Result<i32, &'static CStr> {
        let mut result = vk::Result::ERROR_DEVICE_LOST.as_raw();
        if unsafe { rsf_uvk_foreign_present(self.queue, ptr::from_ref(info).cast(), &mut result) } {
            Ok(result)
        } else {
            Err(PRESENT_EXCEPTION)
        }
    }

    fn native(&self, window: *mut c_void) -> Result<i32, &'static CStr> {
        unsafe { ShowWindowAsync(window, SW_HIDE) };
        self.forward(self.info)
    }

    fn admitted(&self) -> Result<i32, &'static CStr> {
        let window = rsf_unity_vulkan_surface_window();
        if window.is_null()
            || !unsafe { rsf_uvk_foreign_available() }
            || self.info.swapchain_count != 1
            || self.info.p_swapchains.is_null()
            || self.info.p_image_indices.is_null()
        {
            return self.forward(self.info);
        }
        let Some(instance) = instance() else {
            return Err(PRESENT_EXCEPTION);
        };
        if instance.queue != self.queue {
            return self.forward(self.info);
        }

        let mut source = PresentSource::default();
        let mut frame = MaybeUninit::uninit();
        let target = unsafe {
            rsf_unity_vulkan_present_begin(
                instance.instance,
                instance.device as *mut c_void,
                self.queue,
                (*self.info.p_swapchains).as_raw(),
                *self.info.p_image_indices,
                GetParent(window),
                &mut source,
                frame.as_mut_ptr(),
            )
        };
        // present_begin always takes the lease, even without a matching target.
        let _lease = Lease;
        if target == 0 {
            return self.forward(self.info);
        }
        if target != 1 {
            return self.native(window);
        }
        let frame = unsafe { frame.assume_init() };
        let mut graphics = DeviceHandles::EMPTY;
        if frame.services.render_pass.is_none()
            || initialize(&frame.services, &mut graphics) != DEVICE_READY
            || graphics.vk_device != instance.device
            || graphics.vk_queue != self.queue
        {
            return self.native(window);
        }
        let chain = configure_surface(source.width, source.height);
        if chain.is_null() {
            return self.native(window);
        }
        let Some(image) = prepare_rgba(5, source.width, source.height) else {
            return self.native(window);
        };
        // Acquire before resetting the slot or consuming Unity's Present waits.
        let buffer = unsafe { rsf_uvk_gpu_chain_buffer(chain) };
        if buffer.is_null() {
            stop(c"Unity Vulkan swap chain buffer was unavailable; the bridge stopped.");
            return self.native(window);
        }
        // This owned reference stays alive through both DXGI and forwarded Vulkan Present.
        let buffer = unsafe { ID3D12Resource::from_raw(buffer) };
        let Some((ring, slot)) = begin(&graphics) else {
            stop(SLOT_FAILED);
            return self.native(window);
        };
        unsafe {
            rsf_uvk_gpu_image_transition(&graphics, slot.vk[0], image.image, 5, false, true);
            rsf_uvk_gpu_blit_present(
                &graphics,
                slot.vk[0],
                source.image,
                image.image,
                source.width,
                source.height,
            );
            rsf_uvk_gpu_image_transition(&graphics, slot.vk[0], image.image, 0, false, false);
        }
        if !unsafe {
            rsf_uvk_gpu_upload(
                &graphics,
                &slot,
                self.info.p_wait_semaphores,
                self.info.wait_semaphore_count,
            )
        } {
            stop(c"Unity Vulkan present upload failed; the bridge stopped.");
            return Ok(vk::Result::ERROR_DEVICE_LOST.as_raw());
        }
        barrier(
            &slot,
            image.resource,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        let destination = buffer.as_raw();
        barrier(
            &slot,
            destination,
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        unsafe { rsf_uvk_gpu_dx_copy(slot.commands, destination, image.resource) };
        barrier(
            &slot,
            destination,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_PRESENT,
        );
        barrier(
            &slot,
            image.resource,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_COMMON,
        );

        let mut pass = pass(&frame);
        pass.role = RSF_GAME_RENDER_WINDOW;
        pass.swapchain = chain;
        if !render(&frame, graphics.queue, &pass, 1) {
            return Err(PRESENT_EXCEPTION);
        }
        if !unsafe { rsf_uvk_gpu_submit(&graphics, &slot, ring, source.present_ready) } {
            stop(c"Unity Vulkan present submission failed; the bridge stopped.");
            return Ok(vk::Result::ERROR_DEVICE_LOST.as_raw());
        }
        if unsafe { rsf_uvk_gpu_chain_present(chain) } {
            unsafe { ShowWindowAsync(window, SW_SHOWNOACTIVATE) };
        }
        let mut forwarded = *self.info;
        let ready = vk::Semaphore::from_raw(source.present_ready);
        forwarded.wait_semaphore_count = 1;
        forwarded.p_wait_semaphores = &ready;
        VULKAN.lock().advance(ring as usize);
        self.forward(&forwarded)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_bridge_present(
    queue: u64,
    info: *const vk::PresentInfoKHR<'_>,
) -> i32 {
    let Some(info) = (unsafe { info.as_ref() }) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    };
    let present = Present { queue, info };
    let Some(_entry) = Entry::begin() else {
        return present
            .forward(info)
            .unwrap_or(vk::Result::ERROR_DEVICE_LOST.as_raw());
    };
    present.admitted().unwrap_or_else(|message| {
        stop(message);
        vk::Result::ERROR_DEVICE_LOST.as_raw()
    })
}
