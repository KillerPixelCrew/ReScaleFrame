// SPDX-License-Identifier: MIT
use super::leaves;
use super::{Platform, Refused};
use crate::lease::Lease;
use crate::sys::rsf_anomaly_sr_timer as Stats;
use std::ffi::c_void;

type Handle = *mut c_void;
const SLOTS: usize = 4;
// ID3D11DeviceContext::GetData HRESULTs.
const READY: i32 = 0;
const PENDING: i32 = 1;

enum Work {
    Free,
    Recording(Option<bool>),
    Pending(Option<bool>),
}

struct Slot {
    clock: Lease,
    begin: Lease,
    seam: Lease,
    end: Lease,
    work: Work,
}

struct Ring {
    slots: Vec<Slot>,
    device: Lease,
    cursor: usize,
}

impl Ring {
    fn new(platform: &Platform, device: Handle) -> Option<Self> {
        if !platform.bound {
            return None;
        }
        let device = Lease::new(
            leaf!(platform, timer_retain_device(device)),
            leaves::release,
        )?;
        let create = |clock| {
            let query = leaf!(platform, timer_create_query(device.as_ptr(), clock));
            Lease::new(query, leaves::release)
        };
        let mut slots = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            slots.push(Slot {
                clock: create(true)?,
                begin: create(false)?,
                seam: create(false)?,
                end: create(false)?,
                work: Work::Free,
            });
        }
        Some(Self {
            device,
            slots,
            cursor: 0,
        })
    }

    fn advance(&mut self) {
        self.cursor = (self.cursor + 1) % SLOTS;
    }

    fn collect(&mut self, platform: &Platform, context: Handle, stats: &mut Stats) {
        for slot in &mut self.slots {
            let Work::Pending(swapped) = slot.work else {
                continue;
            };
            let mut frequency = 0;
            let mut disjoint = false;
            let clock = if platform.bound {
                leaf!(
                    platform,
                    timer_clock(context, slot.clock.as_ptr(), &mut frequency, &mut disjoint)
                )
            } else {
                -1
            };
            if clock == PENDING {
                continue;
            }
            if clock != READY {
                slot.work = Work::Free;
                stats.skipped += u64::from(swapped.is_some());
                continue;
            }
            let Some(swapped) = swapped else {
                // An abandoned interval has no seam or end timestamp to poll.
                slot.work = Work::Free;
                continue;
            };
            let mut ticks = [0; 3];
            let status = [&slot.begin, &slot.seam, &slot.end].map(|query| query.as_ptr());
            let status = std::array::from_fn::<_, 3, _>(|i| {
                if platform.bound {
                    leaf!(platform, timer_timestamp(context, status[i], &mut ticks[i]))
                } else {
                    -1
                }
            });
            if status
                .iter()
                .any(|result| !matches!(*result, READY | PENDING))
            {
                slot.work = Work::Free;
                stats.skipped += 1;
                continue;
            }
            if status.contains(&PENDING) {
                continue;
            }
            slot.work = Work::Free;
            let [begin, seam, end] = ticks;
            if disjoint || frequency == 0 || seam < begin || end < seam {
                stats.skipped += 1;
                continue;
            }
            let scale = 1000.0 / frequency as f64;
            let index = usize::from(swapped);
            stats.world_ms[index] += (seam - begin) as f64 * scale;
            stats.samples[index] += 1;
            if swapped {
                stats.seam_ms += (end - seam) as f64 * scale;
                stats.seam_samples += 1;
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Timer {
    ring: Option<Ring>,
    stats: Stats,
    pub(super) generation: u64,
}

// The stage lock serializes access. D3D11 device and query references can be
// released on another thread. Recording uses only the current engine context.
unsafe impl Send for Timer {}

impl Timer {
    pub(super) fn reset(&mut self) {
        self.ring = None;
        self.stats = Stats::default();
        self.generation = self.generation.wrapping_add(1);
    }

    pub(super) fn begin(&mut self, platform: &Platform, device: Handle, context: Handle) {
        if context.is_null() {
            return;
        }
        if self
            .ring
            .as_ref()
            .is_none_or(|ring| ring.device.as_ptr() != device)
        {
            self.reset();
            if !device.is_null() {
                self.ring = Ring::new(platform, device);
                self.stats.device = self.ring.is_some();
            }
        }
        let Some(ring) = &mut self.ring else { return };
        ring.collect(platform, context, &mut self.stats);
        let slot = &mut ring.slots[ring.cursor];
        if matches!(slot.work, Work::Recording(_)) {
            leaf!(platform, timer_issue(context, slot.clock.as_ptr(), false));
            slot.work = Work::Pending(None);
            self.stats.skipped += 1;
            ring.advance();
        }
        let slot = &mut ring.slots[ring.cursor];
        if !matches!(slot.work, Work::Free) {
            self.stats.skipped += 1;
            return;
        }
        leaf!(platform, timer_issue(context, slot.clock.as_ptr(), true));
        leaf!(platform, timer_issue(context, slot.begin.as_ptr(), false));
        slot.work = Work::Recording(None);
    }

    fn current(&mut self, device: Handle, context: Handle) -> Option<&mut Ring> {
        self.ring
            .as_mut()
            .filter(|ring| !context.is_null() && ring.device.as_ptr() == device)
    }

    pub(super) fn seam(
        &mut self,
        platform: &Platform,
        device: Handle,
        context: Handle,
        swapped: bool,
    ) {
        let Some(ring) = self.current(device, context) else {
            return;
        };
        let slot = &mut ring.slots[ring.cursor];
        if matches!(slot.work, Work::Recording(None)) {
            leaf!(platform, timer_issue(context, slot.seam.as_ptr(), false));
            slot.work = Work::Recording(Some(swapped));
        }
    }

    pub(super) fn finish(&mut self, platform: &Platform, device: Handle, context: Handle) {
        let Some(ring) = self.current(device, context) else {
            return;
        };
        let slot = &mut ring.slots[ring.cursor];
        if let Work::Recording(Some(swapped)) = slot.work {
            leaf!(platform, timer_issue(context, slot.end.as_ptr(), false));
            leaf!(platform, timer_issue(context, slot.clock.as_ptr(), false));
            slot.work = Work::Pending(Some(swapped));
            ring.advance();
        }
    }

    pub(super) fn sample(&mut self, platform: &Platform, device: Handle, context: Handle) -> Stats {
        if let Some(ring) = &mut self.ring
            && !context.is_null()
            && ring.device.as_ptr() == device
        {
            ring.collect(platform, context, &mut self.stats);
        }
        self.stats
    }
}
