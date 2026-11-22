// SPDX-License-Identifier: MIT
use super::{TestResult, check};
use crate::d3d11_sharpening::Frame;
use crate::d3d12_sharpening::{Sharpen12, transition};
use crate::owned_win32_handle;
use crate::render_abi::rsf_sharpening_options as Options;
use crate::sharpening_policy::defaults;
use std::ffi::{CStr, c_void};
use std::mem::ManuallyDrop;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory4};
use windows::core::Interface;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

const WIDTH: usize = 33;
const HEIGHT: usize = 17;
const COUNT: usize = WIDTH * HEIGHT;
const SLOTS: usize = 3;
const UAV: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
const COPY_DEST: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COPY_DEST;
const COPY_SOURCE: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COPY_SOURCE;
const SHADER_READ: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
type Image = Vec<f32>;

struct Readback {
    buffer: ID3D12Resource,
    footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
}

fn location(
    resource: &ID3D12Resource,
    footprint: Option<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>,
) -> D3D12_TEXTURE_COPY_LOCATION {
    let (kind, details) = match footprint {
        Some(footprint) => (
            D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: footprint,
            },
        ),
        None => (
            D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        ),
    };
    D3D12_TEXTURE_COPY_LOCATION {
        // The command recording retains the resource through the fixture's slot.
        pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource.as_raw()) })),
        Type: kind,
        Anonymous: details,
    }
}

struct Slot {
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    retained: Vec<ID3D12Resource>,
}

struct Fixture {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
    event: OwnedHandle,
    tick: u64,
    slots: Vec<Slot>,
    pass: Sharpen12,
}

impl Fixture {
    fn start() -> TestResult<Self> {
        let mut debug: Option<ID3D12Debug> = None;
        if unsafe { D3D12GetDebugInterface(&mut debug) }.is_ok()
            && let Some(debug) = debug
        {
            unsafe { debug.EnableDebugLayer() };
        }
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory1() }?;
        let warp: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }?;
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&warp, D3D_FEATURE_LEVEL_11_0, &mut device) }?;
        let device = device.ok_or("null WARP device")?;
        let queue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default()) }?;
        let fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }?;
        let event = unsafe { owned_win32_handle(CreateEventW(ptr::null(), 0, 0, ptr::null())) }
            .ok_or("create fence completion event")?;
        let mut slots = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            let allocator: ID3D12CommandAllocator =
                unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }?;
            let list: ID3D12GraphicsCommandList = unsafe {
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
            }?;
            unsafe { list.Close() }?;
            slots.push(Slot {
                allocator,
                list,
                retained: Vec::new(),
            });
        }
        Ok(Self {
            device,
            queue,
            fence,
            event,
            tick: 0,
            slots,
            pass: Sharpen12::default(),
        })
    }

    fn submit(&mut self, slots: usize) -> TestResult {
        let mut commands = Vec::with_capacity(slots);
        for slot in &self.slots[..slots] {
            unsafe { slot.list.Close() }?;
            commands.push(Some(slot.list.cast::<ID3D12CommandList>()?));
        }
        self.tick += 1;
        let event = self.event.as_raw_handle();
        unsafe {
            self.queue.ExecuteCommandLists(&commands);
            self.queue.Signal(&self.fence, self.tick)?;
            self.fence.SetEventOnCompletion(self.tick, HANDLE(event))?;
        }
        check(
            unsafe { WaitForSingleObject(event, 10_000) } == WAIT_OBJECT_0
                && (self.tick..u64::MAX).contains(&unsafe { self.fence.GetCompletedValue() }),
            "D3D12 fixture submission failed to retire",
        )
    }

    fn resource(
        &self,
        heap: D3D12_HEAP_TYPE,
        desc: &D3D12_RESOURCE_DESC,
        state: D3D12_RESOURCE_STATES,
    ) -> TestResult<ID3D12Resource> {
        let mut result = None;
        let properties = D3D12_HEAP_PROPERTIES {
            Type: heap,
            ..Default::default()
        };
        unsafe {
            self.device.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                desc,
                state,
                None,
                &mut result,
            )
        }?;
        result.ok_or_else(|| "null committed resource".into())
    }

    fn buffer(&self, bytes: u64, heap: D3D12_HEAP_TYPE) -> TestResult<ID3D12Resource> {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: bytes,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let state = if heap == D3D12_HEAP_TYPE_READBACK {
            COPY_DEST
        } else {
            D3D12_RESOURCE_STATE_GENERIC_READ
        };
        self.resource(heap, &desc, state)
    }

    fn footprint(&self, desc: &D3D12_RESOURCE_DESC) -> (D3D12_PLACED_SUBRESOURCE_FOOTPRINT, u64) {
        let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
        let mut bytes = 0;
        unsafe {
            self.device.GetCopyableFootprints(
                desc,
                0,
                1,
                0,
                Some(&mut footprint),
                None,
                None,
                Some(&mut bytes),
            )
        };
        (footprint, bytes)
    }

    fn texture(
        &mut self,
        slot: usize,
        size: [usize; 2],
        format: DXGI_FORMAT,
        pixels: &[f32],
        state: D3D12_RESOURCE_STATES,
    ) -> TestResult<ID3D12Resource> {
        let [width, height] = size;
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width as u64,
            Height: height as u32,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        let texture = self.resource(D3D12_HEAP_TYPE_DEFAULT, &desc, UAV)?;
        let (footprint, bytes) = self.footprint(&desc);
        let upload = self.buffer(bytes, D3D12_HEAP_TYPE_UPLOAD)?;
        let row = size_of_val(pixels) / height;
        let mut mapped: *mut c_void = ptr::null_mut();
        unsafe { upload.Map(0, Some(&D3D12_RANGE::default()), Some(&mut mapped)) }?;
        check(!mapped.is_null(), "upload map returned null")?;
        let list = &self.slots[slot].list;
        unsafe {
            for y in 0..height {
                ptr::copy_nonoverlapping(
                    pixels.as_ptr().cast::<u8>().add(y * row),
                    mapped
                        .cast::<u8>()
                        .add(y * footprint.Footprint.RowPitch as usize),
                    row,
                );
            }
            upload.Unmap(0, None);
            list.ResourceBarrier(&[transition(&texture, UAV, COPY_DEST)]);
            list.CopyTextureRegion(
                &location(&texture, None),
                0,
                0,
                0,
                &location(&upload, Some(footprint)),
                None,
            );
            list.ResourceBarrier(&[transition(&texture, COPY_DEST, state)]);
        }
        self.slots[slot].retained.extend([upload, texture.clone()]);
        Ok(texture)
    }

    fn capture(&self, slot: usize, texture: &ID3D12Resource) -> TestResult<Readback> {
        let (footprint, bytes) = self.footprint(&unsafe { texture.GetDesc() });
        let buffer = self.buffer(bytes, D3D12_HEAP_TYPE_READBACK)?;
        let list = &self.slots[slot].list;
        unsafe {
            list.ResourceBarrier(&[transition(texture, UAV, COPY_SOURCE)]);
            list.CopyTextureRegion(
                &location(&buffer, Some(footprint)),
                0,
                0,
                0,
                &location(texture, None),
                None,
            );
            list.ResourceBarrier(&[transition(texture, COPY_SOURCE, UAV)]);
        }
        Ok(Readback { buffer, footprint })
    }

    fn record(
        &mut self,
        slot: usize,
        image: &[f32],
        options: &Options,
        depth: bool,
        hdr: bool,
    ) -> TestResult<Readback> {
        let recording = &mut self.slots[slot];
        recording.retained.clear();
        unsafe {
            recording.allocator.Reset()?;
            recording
                .list
                .Reset(&recording.allocator, None::<&ID3D12PipelineState>)?;
        }
        let size = [WIDTH, HEIGHT];
        let output = self.texture(slot, size, DXGI_FORMAT_R32G32B32A32_FLOAT, image, UAV)?;
        let motion = self.texture(
            slot,
            size,
            DXGI_FORMAT_R32G32_FLOAT,
            &[0.0; COUNT * 2],
            SHADER_READ,
        )?;
        let depth_image = self.texture(
            slot,
            size,
            DXGI_FORMAT_R32_FLOAT,
            &[0.5; COUNT],
            SHADER_READ,
        )?;
        let exposure = self.texture(slot, [1, 1], DXGI_FORMAT_R32_FLOAT, &[2.0], SHADER_READ)?;
        let frame = Frame {
            depth_linear: 0,
            depth_inverted: 1,
            hdr: u32::from(hdr),
            near_plane: 0.0,
            far_plane: 0.0,
            exposure: 1.0,
            motion_x: 1.0,
            motion_y: 1.0,
        };
        let depth_raw = if depth {
            depth_image.as_raw()
        } else {
            ptr::null_mut()
        };
        check(
            self.pass.run(
                self.slots[slot].list.as_raw(),
                [
                    output.as_raw(),
                    motion.as_raw(),
                    depth_raw,
                    exposure.as_raw(),
                ],
                options,
                &frame,
                slot as u32,
            ),
            "sharpening recording refused",
        )?;
        self.capture(slot, &output)
    }

    fn run(
        &mut self,
        image: &[f32],
        options: &Options,
        depth: bool,
        hdr: bool,
    ) -> TestResult<Image> {
        let captured = self.record(0, image, options, depth, hdr)?;
        self.submit(1)?;
        read(&captured)
    }

    fn validate(&self) -> TestResult {
        let Ok(messages) = self.device.cast::<ID3D12InfoQueue>() else {
            return Ok(());
        };
        let count = unsafe { messages.GetNumStoredMessagesAllowedByRetrievalFilter() };
        for index in 0..count {
            let mut bytes = 0;
            unsafe { messages.GetMessage(index, None, &mut bytes) }?;
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
            unsafe { messages.GetMessage(index, Some(message), &mut bytes) }?;
            let message = unsafe { &*message };
            if message.Severity.0 <= D3D12_MESSAGE_SEVERITY_WARNING.0 {
                let description = unsafe { CStr::from_ptr(message.pDescription.cast()) };
                return Err(format!("D3D12 validation: {}", description.to_string_lossy()).into());
            }
        }
        Ok(())
    }
}

fn read(out: &Readback) -> TestResult<Image> {
    let mut image = vec![0.0; COUNT * 4];
    let mut mapped: *mut c_void = ptr::null_mut();
    unsafe { out.buffer.Map(0, None, Some(&mut mapped)) }?;
    check(!mapped.is_null(), "readback map returned null")?;
    unsafe {
        for y in 0..HEIGHT {
            ptr::copy_nonoverlapping(
                mapped
                    .cast::<u8>()
                    .add(y * out.footprint.Footprint.RowPitch as usize),
                image.as_mut_ptr().cast::<u8>().add(y * WIDTH * 16),
                WIDTH * 16,
            );
        }
        out.buffer.Unmap(0, Some(&D3D12_RANGE::default()));
    }
    Ok(image)
}

fn pixels(make: impl Fn(usize) -> f32) -> Image {
    (0..COUNT)
        .flat_map(|i| {
            [
                make(i % WIDTH),
                make(i % WIDTH),
                make(i % WIDTH),
                i as f32 / COUNT as f32,
            ]
        })
        .collect()
}

fn verify(fixture: &mut Fixture) -> TestResult {
    let source = pixels(|x| {
        if x < WIDTH / 2 {
            10.0 + 0.2 * x as f32
        } else {
            80.0 + 0.2 * x as f32
        }
    });
    let before = fixture.pass.dispatches();
    for (mode, strength) in [(0, 0.4), (4, 1.0), (1, 0.0)] {
        let options = Options {
            mode,
            sharpness: strength,
            ..defaults()
        };
        let actual = fixture.run(&source, &options, true, true)?;
        check(
            actual
                .iter()
                .map(|x| x.to_bits())
                .eq(source.iter().map(|x| x.to_bits()))
                && fixture.pass.dispatches() == before,
            "inactive filter keeps output and dispatch count",
        )?;
    }
    for mode in [1, 2, 3] {
        let options = Options {
            mode,
            sharpness: 1.0,
            ..defaults()
        };
        let flat = pixels(|_| 10.0);
        check(
            fixture.run(&flat, &options, true, true)? == flat,
            "flat HDR and alpha",
        )?;
        let actual = fixture.run(&source, &options, true, true)?;
        let mut changed = false;
        for (i, (got, original)) in actual
            .chunks_exact(4)
            .zip(source.chunks_exact(4))
            .enumerate()
        {
            check(got[3] == original[3], "HDR edge retains alpha")?;
            let (x, y) = ((i % WIDTH) as i32, (i / WIDTH) as i32);
            for channel in 0..3 {
                let (mut low, mut high) = (original[channel], original[channel]);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let sx = (x + dx).clamp(0, WIDTH as i32 - 1) as usize;
                        let sy = (y + dy).clamp(0, HEIGHT as i32 - 1) as usize;
                        let sample = source[(sy * WIDTH + sx) * 4 + channel];
                        low = low.min(sample);
                        high = high.max(sample);
                    }
                }
                check(
                    got[channel].is_finite() && (low - 1e-3..=high + 1e-3).contains(&got[channel]),
                    "HDR neighborhood bounds",
                )?;
                changed |= (got[channel] - original[channel]).abs() > 1e-3;
            }
        }
        check(changed, "HDR edge sharpened")?;
        let sdr = pixels(|x| {
            if x < WIDTH / 2 {
                0.1 + 0.002 * x as f32
            } else {
                0.8 + 0.002 * x as f32
            }
        });
        check(
            fixture.run(&sdr, &options, true, false)? != sdr,
            "SDR edge sharpened",
        )?;
        let white = pixels(|_| 1.0);
        check(
            fixture.run(&white, &options, true, false)? == white,
            "SDR white stays flat",
        )?;
        let missing = fixture.run(&source, &options, false, true)?;
        check(
            missing == fixture.run(&source, &Options { mode: 1, ..options }, false, true)?,
            "missing depth uses RCAS",
        )?;
    }
    let mut records = Vec::with_capacity(SLOTS);
    for slot in 0..SLOTS {
        let options = Options {
            mode: 1,
            sharpness: 0.2 + 0.4 * slot as f32,
            ..defaults()
        };
        records.push(fixture.record(slot, &source, &options, true, true)?);
    }
    fixture.submit(SLOTS)?;
    let mut deltas = Vec::with_capacity(SLOTS);
    for record in &records {
        let actual = read(record)?;
        deltas.push(
            actual
                .iter()
                .zip(&source)
                .enumerate()
                .filter(|(i, _)| i % 4 != 3)
                .map(|(_, (actual, expected))| f64::from((actual - expected).abs()))
                .sum::<f64>(),
        );
    }
    check(
        deltas[0] > 0.0 && deltas[1] > deltas[0] && deltas[2] > deltas[1],
        "per-slot constants and monotonic response",
    )?;
    fixture.validate()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_d3d12_fixture() -> i32 {
    let mut disabled = Sharpen12::default();
    let blank = Frame {
        depth_linear: 0,
        depth_inverted: 0,
        hdr: 1,
        near_plane: 0.0,
        far_plane: 0.0,
        exposure: 1.0,
        motion_x: 1.0,
        motion_y: 1.0,
    };
    if !disabled.run(
        ptr::null_mut(),
        [ptr::null_mut(); 4],
        &defaults(),
        &blank,
        0,
    ) || disabled.dispatches() != 0
    {
        return 1;
    }
    let mut fixture = match Fixture::start() {
        Ok(fixture) => fixture,
        Err(error) => {
            eprintln!("sharpening_d3d12 WARP unavailable: {error}");
            return 77;
        }
    };
    if let Err(error) = verify(&mut fixture) {
        eprintln!("sharpening_d3d12: {error}");
        return 1;
    }
    println!("sharpening_d3d12: {} dispatches", fixture.pass.dispatches());
    0
}
