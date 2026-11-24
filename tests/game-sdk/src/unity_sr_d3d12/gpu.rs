// SPDX-License-Identifier: MIT
use super::{HEIGHT, Result, WIDTH, check, enabled};
use rsf_game_sys::rsf_game_host_services;
use rsf_test_support::d3d12::{buffer, committed, copy_location, footprint, transition};
use std::{
    ffi::{CStr, c_void},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows::{
    Win32::{
        Foundation::{HANDLE, HWND, WAIT_OBJECT_0},
        Graphics::{
            Direct3D::D3D_FEATURE_LEVEL_12_0,
            Direct3D12::*,
            Dxgi::{Common::*, *},
        },
        System::Threading::{CreateEventW, WaitForSingleObject},
    },
    core::{IUnknown, Interface, PCWSTR},
};

pub(super) const COPY_DEST: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COPY_DEST;
pub(super) const COPY_SOURCE: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COPY_SOURCE;
pub(super) const SHADER_READ: D3D12_RESOURCE_STATES =
    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;

pub(super) struct Gpu {
    pub factory: IDXGIFactory4,
    pub adapter: Option<IDXGIAdapter1>,
    pub device: ID3D12Device,
    pub queue: ID3D12CommandQueue,
    pub list: ID3D12GraphicsCommandList,
    allocator: ID3D12CommandAllocator,
    fence: ID3D12Fence,
}

impl Gpu {
    pub fn open(wanted_vendor: Option<u32>) -> Result<Option<Self>> {
        if enabled("RSF_UNITY_GPU_DEBUG") {
            let mut debug: Option<ID3D12Debug> = None;
            if unsafe { D3D12GetDebugInterface(&mut debug) }.is_err() || debug.is_none() {
                eprintln!("D3D12 debug interface unavailable.");
                return Ok(None);
            }
            unsafe { debug.expect("debug interface present").EnableDebugLayer() };
        }
        let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory4>() }) else {
            return Ok(None);
        };
        let selected = if let Some(vendor) = wanted_vendor {
            let mut found = None;
            for index in 0.. {
                let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
                    break;
                };
                if let Ok(desc) = unsafe { adapter.GetDesc1() }
                    && desc.VendorId == vendor
                    && desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0
                {
                    found = Some(adapter.cast::<IUnknown>()?);
                    break;
                }
            }
            if found.is_none() {
                return Ok(None);
            }
            found
        } else {
            None
        };
        let mut device = None;
        if unsafe { D3D12CreateDevice(selected.as_ref(), D3D_FEATURE_LEVEL_12_0, &mut device) }
            .is_err()
        {
            return Ok(None);
        }
        let Some(device): Option<ID3D12Device> = device else {
            return Ok(None);
        };
        let adapter: Option<IDXGIAdapter1> =
            unsafe { factory.EnumAdapterByLuid(device.GetAdapterLuid()) }.ok();
        if let Some(adapter) = &adapter
            && let Ok(desc) = unsafe { adapter.GetDesc1() }
        {
            let name = String::from_utf16_lossy(&desc.Description);
            eprintln!(
                "D3D12 fixture adapter {} vendor=0x{:x} device=0x{:x}",
                name.trim_end_matches('\0'),
                desc.VendorId,
                desc.DeviceId
            );
        }
        let queue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default()) }?;
        let allocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }?;
        let list = unsafe {
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
        }?;
        let fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }?;
        Ok(Some(Self {
            factory,
            adapter,
            device,
            queue,
            list,
            allocator,
            fence,
        }))
    }

    pub fn reset(&self) -> Result<()> {
        unsafe {
            self.allocator.Reset()?;
            self.list.Reset(&self.allocator, None)?;
        }
        Ok(())
    }

    pub fn submit(&self, host: &rsf_game_host_services, source: u64) -> Result<()> {
        let list = self.list.cast::<ID3D12CommandList>()?;
        let commands = [self.list.as_raw()];
        unsafe {
            self.list.Close()?;
            if let Some(submit) = host.submit {
                submit(
                    host.user,
                    self.queue.as_raw(),
                    source,
                    1,
                    commands.as_ptr(),
                    1,
                );
            }
            self.queue.ExecuteCommandLists(&[Some(list)]);
            if let Some(submit) = host.submit {
                submit(
                    host.user,
                    self.queue.as_raw(),
                    source,
                    1,
                    commands.as_ptr(),
                    0,
                );
            }
        }
        Ok(())
    }

    pub fn wait(&self, value: u64) -> Result<()> {
        wait(&self.queue, &self.fence, value)
    }

    pub fn texture(
        &self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        flags: D3D12_RESOURCE_FLAGS,
    ) -> Result<ID3D12Resource> {
        let format = if enabled("RSF_SR_TYPELESS_INPUTS") {
            match format {
                DXGI_FORMAT_R16G16B16A16_FLOAT => DXGI_FORMAT_R16G16B16A16_TYPELESS,
                DXGI_FORMAT_R16G16_FLOAT => DXGI_FORMAT_R16G16_TYPELESS,
                _ => format,
            }
        } else {
            format
        };
        Ok(committed(
            &self.device,
            D3D12_HEAP_TYPE_DEFAULT,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: width.into(),
                Height: height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Flags: flags,
                ..Default::default()
            },
            COPY_DEST,
        )?)
    }

    pub fn upload(&self, image: &ID3D12Resource, pixel: &[u8]) -> Result<ID3D12Resource> {
        let (layout, bytes) = footprint(&self.device, &unsafe { image.GetDesc() });
        let buffer = buffer(&self.device, bytes, D3D12_HEAP_TYPE_UPLOAD)?;
        let by = bytes as usize;
        let pitch = layout.Footprint.RowPitch as usize;
        let row_bytes = layout.Footprint.Width as usize * pixel.len();
        let offset = layout.Offset as usize;
        let height = layout.Footprint.Height as usize;
        check(
            height != 0 && row_bytes <= pitch && offset + (height - 1) * pitch + row_bytes <= by,
            "invalid texture upload footprint",
        )?;
        let mut address = ptr::null_mut();
        unsafe { buffer.Map(0, None, Some(&mut address)) }?;
        check(!address.is_null(), "null texture upload map")?;
        let upload = unsafe { std::slice::from_raw_parts_mut(address.cast::<u8>(), by) };
        upload.fill(0);
        for y in 0..height {
            let begin = offset + y * pitch;
            for destination in upload[begin..begin + row_bytes].chunks_exact_mut(pixel.len()) {
                destination.copy_from_slice(pixel);
            }
        }
        unsafe { buffer.Unmap(0, None) };
        self.copy(image, None, &buffer, Some(layout));
        transition(&self.list, image, COPY_DEST, SHADER_READ);
        Ok(buffer)
    }

    pub fn readback(&self, image: &ID3D12Resource) -> Result<Readback> {
        let (layout, bytes) = footprint(&self.device, &unsafe { image.GetDesc() });
        Ok(Readback {
            resource: buffer(&self.device, bytes, D3D12_HEAP_TYPE_READBACK)?,
            layout,
        })
    }

    pub fn copy_to_readback(&self, image: &ID3D12Resource, target: &Readback) {
        transition(&self.list, image, COPY_DEST, COPY_SOURCE);
        self.copy(&target.resource, Some(target.layout), image, None);
    }

    fn copy(
        &self,
        target: &ID3D12Resource,
        target_layout: Option<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>,
        source: &ID3D12Resource,
        source_layout: Option<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>,
    ) {
        unsafe {
            self.list.CopyTextureRegion(
                &copy_location(target, target_layout),
                0,
                0,
                0,
                &copy_location(source, source_layout),
                None,
            );
        }
    }

    pub fn swapchain(&self, window: HWND, buffers: u32) -> Result<IDXGISwapChain1> {
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: WIDTH,
            Height: HEIGHT,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: buffers,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            ..Default::default()
        };
        Ok(unsafe {
            self.factory.CreateSwapChainForHwnd(
                &self.queue,
                window,
                &desc,
                None,
                None::<&IDXGIOutput>,
            )
        }?)
    }

    pub fn validate(&self) -> Result<()> {
        if !enabled("RSF_UNITY_GPU_DEBUG") {
            return Ok(());
        }
        let info: ID3D12InfoQueue = self.device.cast()?;
        let mut ok = true;
        for index in 0..unsafe { info.GetNumStoredMessagesAllowedByRetrievalFilter() } {
            let mut bytes = 0;
            unsafe { info.GetMessage(index, None, &mut bytes) }?;
            check(
                bytes >= size_of::<D3D12_MESSAGE>(),
                "short D3D12 debug message",
            )?;
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
            unsafe { info.GetMessage(index, Some(message), &mut bytes) }?;
            let message = unsafe { &*message };
            if message.Severity.0 <= D3D12_MESSAGE_SEVERITY_ERROR.0 {
                let description = unsafe { CStr::from_ptr(message.pDescription.cast()) };
                eprintln!("D3D12 validation: {}", description.to_string_lossy());
                ok = false;
            }
        }
        eprintln!(
            "D3D12 debug validation {}",
            if ok { "passed" } else { "failed" }
        );
        check(ok, "D3D12 validation reported an error")
    }
}

pub(super) fn wait(queue: &ID3D12CommandQueue, fence: &ID3D12Fence, value: u64) -> Result<()> {
    let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }?;
    let event = unsafe { OwnedHandle::from_raw_handle(event.0) };
    let raw = HANDLE(event.as_raw_handle());
    unsafe {
        queue.Signal(fence, value)?;
        fence.SetEventOnCompletion(value, raw)?;
        check(
            WaitForSingleObject(raw, 30_000) == WAIT_OBJECT_0,
            "D3D12 fence wait failed",
        )
    }
}
pub(super) struct Readback {
    resource: ID3D12Resource,
    layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
}

impl Readback {
    pub fn center_rgb(&self) -> Result<[u16; 3]> {
        let mut address: *mut c_void = ptr::null_mut();
        unsafe { self.resource.Map(0, None, Some(&mut address)) }?;
        check(!address.is_null(), "null readback map")?;
        let offset = self.layout.Offset as usize
            + self.layout.Footprint.RowPitch as usize * (HEIGHT / 2) as usize
            + (WIDTH / 2) as usize * 8;
        let center = unsafe { address.cast::<u8>().add(offset).cast::<u16>() };
        let rgb = unsafe {
            [
                center.read_unaligned(),
                center.add(1).read_unaligned(),
                center.add(2).read_unaligned(),
            ]
        };
        unsafe { self.resource.Unmap(0, None) };
        Ok(rgb)
    }
}
