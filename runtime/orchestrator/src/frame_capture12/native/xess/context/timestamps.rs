// SPDX-License-Identifier: MIT
use super::{Handle, SLOTS, borrow, execute, readback_buffer};
use crate::allocate_handle;
use crate::xess_fg_bridge::timing::{Accumulator, GpuSample, rsf_core_xess_timing_gpu};
use std::ptr;
use windows::Win32::Graphics::Direct3D12::*;

const QUERIES_PER_SLOT: u32 = 3;
const QUERY_SIZE: usize = size_of::<u64>();

#[derive(Default)]
struct Slot {
    present_enter: i64,
    present_exit: i64,
    submitted: bool,
}

struct Timestamps {
    // Field order follows native retirement: list, allocators, readback, query heap.
    commands: ID3D12GraphicsCommandList,
    allocators: [ID3D12CommandAllocator; SLOTS],
    readback: ID3D12Resource,
    heap: ID3D12QueryHeap,
    frequency: u64,
    slots: [Slot; SLOTS],
}

impl Timestamps {
    fn open(device: &ID3D12Device, queue: &ID3D12CommandQueue) -> Option<Self> {
        let count = SLOTS as u32 * QUERIES_PER_SLOT;
        let desc = D3D12_QUERY_HEAP_DESC {
            Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
            Count: count,
            NodeMask: 0,
        };
        let mut heap = None::<ID3D12QueryHeap>;
        unsafe { device.CreateQueryHeap(&desc, &mut heap) }.ok()?;
        let heap = heap?;
        let readback = readback_buffer(device, u64::from(count) * QUERY_SIZE as u64)?;
        let mut allocators: [Option<ID3D12CommandAllocator>; SLOTS] = Default::default();
        for slot in &mut allocators {
            *slot = Some(
                unsafe {
                    device.CreateCommandAllocator::<ID3D12CommandAllocator>(
                        D3D12_COMMAND_LIST_TYPE_DIRECT,
                    )
                }
                .ok()?,
            );
        }
        let allocators = allocators.map(Option::unwrap);
        let commands: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocators[0],
                None::<&ID3D12PipelineState>,
            )
        }
        .ok()?;
        unsafe { commands.Close() }.ok()?;
        let frequency = unsafe { queue.GetTimestampFrequency() }.ok()?;
        if frequency == 0 {
            return None;
        }
        Some(Self {
            commands,
            allocators,
            readback,
            heap,
            frequency,
            slots: Default::default(),
        })
    }

    fn harvest(&mut self, queue: &ID3D12CommandQueue, slot: usize) -> Option<GpuSample> {
        let facts = &mut self.slots[slot];
        if !std::mem::take(&mut facts.submitted) {
            return None;
        }
        let index = slot * QUERIES_PER_SLOT as usize;
        let range = D3D12_RANGE {
            Begin: index * QUERY_SIZE,
            End: (index + QUERIES_PER_SLOT as usize) * QUERY_SIZE,
        };
        let mut mapped = ptr::null_mut();
        unsafe { self.readback.Map(0, Some(&range), Some(&mut mapped)) }.ok()?;
        let samples =
            (!mapped.is_null()).then(|| unsafe { mapped.cast::<[u64; 3]>().add(slot).read() });
        let written = D3D12_RANGE { Begin: 0, End: 0 };
        unsafe { self.readback.Unmap(0, Some(&written)) };
        let [start, copies_end, end] = samples?;
        if end <= start {
            return None;
        }
        let (mut calibration_gpu, mut calibration_cpu) = (0, 0);
        unsafe { queue.GetClockCalibration(&mut calibration_gpu, &mut calibration_cpu) }.ok()?;
        Some(GpuSample {
            start,
            copies_end,
            end,
            frequency: self.frequency,
            calibration_gpu,
            calibration_cpu,
            present_enter: facts.present_enter,
            present_exit: facts.present_exit,
        })
    }

    fn finish(
        &mut self,
        queue: &ID3D12CommandQueue,
        slot: usize,
        exit: i64,
    ) -> windows::core::Result<()> {
        self.slots[slot].present_exit = exit;
        let allocator = &self.allocators[slot];
        let query = slot as u32 * QUERIES_PER_SLOT;
        unsafe {
            allocator.Reset()?;
            self.commands
                .Reset(allocator, None::<&ID3D12PipelineState>)?;
            self.commands
                .EndQuery(&self.heap, D3D12_QUERY_TYPE_TIMESTAMP, query + 2);
            self.commands.ResolveQueryData(
                &self.heap,
                D3D12_QUERY_TYPE_TIMESTAMP,
                query,
                QUERIES_PER_SLOT,
                &self.readback,
                u64::from(query) * QUERY_SIZE as u64,
            );
            self.commands.Close()?;
            execute(queue, &self.commands);
        }
        self.slots[slot].submitted = true;
        Ok(())
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_timestamps_destroy(timing: *mut Timestamps) {
    if !timing.is_null() {
        drop(unsafe { Box::from_raw(timing) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_timestamps_begin(
    storage: &mut *mut Timestamps,
    device: Handle,
    queue: Handle,
    slot: u32,
    timing: &Accumulator,
    cpu_frequency: i64,
) -> u32 {
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    if (*storage).is_null() {
        let device = unsafe { borrow::<ID3D12Device>(&device) };
        let Some(created) = Timestamps::open(device, queue) else {
            return 0;
        };
        *storage = allocate_handle(created);
    }
    let Some(state) = (unsafe { (*storage).as_mut() }) else {
        return 0;
    };
    if let Some(sample) = state.harvest(queue, slot as usize) {
        rsf_core_xess_timing_gpu(timing, &sample, cpu_frequency);
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_timestamps_copy(
    timing: &Timestamps,
    command: Handle,
    slot: u32,
    index: u32,
) {
    let command = unsafe { borrow::<ID3D12GraphicsCommandList>(&command) };
    unsafe {
        command.EndQuery(
            &timing.heap,
            D3D12_QUERY_TYPE_TIMESTAMP,
            slot * QUERIES_PER_SLOT + index,
        )
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_dx12_timestamps_enter(timing: &mut Timestamps, slot: u32, enter: i64) {
    timing.slots[slot as usize].present_enter = enter;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_dx12_timestamps_finish(
    timing: &mut Timestamps,
    queue: Handle,
    slot: u32,
    exit: i64,
) {
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    let _ = timing.finish(queue, slot as usize, exit);
}
