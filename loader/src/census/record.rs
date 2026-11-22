// SPDX-License-Identifier: MIT
use super::identity::{rsf_census_module_base, rsf_census_module_name_from_base};
use super::json::{
    rsf_census_json_close, rsf_census_json_hex, rsf_census_json_open, rsf_census_json_string,
    rsf_census_json_u64,
};
use super::platform;
use super::session::{append_line, drop_line};
use crate::sys::rsf_census_json;
use crate::sys::{
    RSF_CENSUS_RECORD_BODY_MAX, RSF_CENSUS_RECORD_STACK_FRAMES, rsf_census_queued_record,
};
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char};
use std::io::Write;
use std::mem::MaybeUninit;
use std::ptr::{null, null_mut};
use std::slice;
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, AtomicUsize, Ordering::*},
};

const SLOTS: usize = 1024;
const LINE_BYTES: usize =
    RSF_CENSUS_RECORD_BODY_MAX as usize + 768 + RSF_CENSUS_RECORD_STACK_FRAMES as usize * 192;

struct Slot {
    stamp: AtomicUsize,
    record: UnsafeCell<MaybeUninit<rsf_census_queued_record>>,
}

// Each slot is accessed by one reserved producer or the sole consumer. The stamp is the
// release/acquire handoff in both directions. Raw code addresses are only copied or reported.
unsafe impl Send for Slot {}
unsafe impl Sync for Slot {}

#[repr(align(64))]
struct QueuePosition(AtomicUsize);

struct LineScratch(UnsafeCell<[u8; LINE_BYTES]>);
// The writer lock permits only one drain at a time, including the fatal-error path.
unsafe impl Sync for LineScratch {}

struct Ring {
    write: QueuePosition,
    read: QueuePosition,
    sequence: AtomicU64,
    slots: Vec<Slot>,
    line: LineScratch,
}

static RING: OnceLock<Ring> = OnceLock::new();

pub(super) fn rsf_census_record_init() -> bool {
    let mut slots = Vec::new();
    if slots.try_reserve_exact(SLOTS).is_err() {
        return false;
    }
    for stamp in 0..SLOTS {
        slots.push(Slot {
            stamp: AtomicUsize::new(stamp),
            record: UnsafeCell::new(MaybeUninit::zeroed()),
        });
    }
    RING.set(Ring {
        write: QueuePosition(AtomicUsize::new(0)),
        read: QueuePosition(AtomicUsize::new(0)),
        sequence: AtomicU64::new(0),
        slots,
        line: LineScratch(UnsafeCell::new([0; LINE_BYTES])),
    })
    .is_ok()
}

pub(super) fn rsf_census_record_acquire(position: &mut usize) -> *mut rsf_census_queued_record {
    let Some(ring) = RING.get() else {
        return null_mut();
    };
    let mut ticket = ring.write.0.load(Relaxed);
    loop {
        let slot = &ring.slots[ticket & (SLOTS - 1)];
        let stamp = slot.stamp.load(Acquire);
        match (stamp.wrapping_sub(ticket) as isize).cmp(&0) {
            std::cmp::Ordering::Equal => {
                match ring.write.0.compare_exchange_weak(
                    ticket,
                    ticket.wrapping_add(1),
                    Relaxed,
                    Relaxed,
                ) {
                    Ok(_) => {
                        *position = ticket;
                        return slot.record.get().cast();
                    }
                    Err(current) => ticket = current,
                }
            }
            std::cmp::Ordering::Less => return null_mut(),
            std::cmp::Ordering::Greater => ticket = ring.write.0.load(Relaxed),
        }
    }
}

pub(super) fn rsf_census_record_publish(position: usize) {
    let ring = RING.get().expect("a published ticket has a live ring");
    ring.slots[position & (SLOTS - 1)]
        .stamp
        .store(position.wrapping_add(1), Release);
}

unsafe fn write_line(record: &rsf_census_queued_record, seq: u64, line: &mut [u8; LINE_BYTES]) {
    let fi = usize::from(record.fn_);
    let body = record.len as usize;
    if fi >= usize::from(platform().count) || body > record.body.len() {
        drop_line();
        return;
    }
    let mut json = rsf_census_json {
        buffer: line.as_mut_ptr().cast(),
        capacity: line.len() - 16,
        length: 0,
        comma: 0,
        depth: 0,
        overflow: false,
    };
    unsafe {
        let desc = &*platform().entries.add(fi);
        let module = rsf_census_module_name_from_base(record.base);
        rsf_census_json_open(&mut json, null(), b'{' as c_char);
        rsf_census_json_u64(&mut json, c"seq".as_ptr(), seq);
        rsf_census_json_u64(&mut json, c"t_us".as_ptr(), record.t_us);
        rsf_census_json_u64(&mut json, c"present".as_ptr(), record.present);
        rsf_census_json_string(&mut json, c"api".as_ptr(), desc.api);
        let name = if desc.name.is_null() {
            desc.id
        } else {
            desc.name
        };
        rsf_census_json_string(&mut json, c"fn".as_ptr(), name);
        rsf_census_json_u64(&mut json, c"ord".as_ptr(), record.ord);
        rsf_census_json_u64(&mut json, c"site_ord".as_ptr(), record.site_ord);
        rsf_census_json_string(&mut json, c"module".as_ptr(), module);
        rsf_census_json_hex(&mut json, c"rva".as_ptr(), record.rva.into());
        rsf_census_json_u64(&mut json, c"tid".as_ptr(), record.tid.into());
        rsf_census_json_string(&mut json, c"thread".as_ptr(), record.thread.as_ptr());
        rsf_census_json_hex(&mut json, c"ret".as_ptr(), record.ret);
        if record.stack_n != 0 {
            rsf_census_json_open(&mut json, c"stack".as_ptr(), b'[' as c_char);
            for &frame in record.stack.iter().take(record.stack_n as usize) {
                let base = rsf_census_module_base(frame);
                let mut label = [0u8; 96];
                let mut output = &mut label[..95];
                if base != 0 {
                    let module = CStr::from_ptr(rsf_census_module_name_from_base(base));
                    let _ = output.write_all(module.to_bytes());
                    let _ = write!(output, "+{:#x}", (frame as u64).wrapping_sub(base));
                } else {
                    let _ = write!(output, "{:#x}", frame as usize);
                }
                rsf_census_json_string(&mut json, null(), label.as_ptr().cast());
            }
            rsf_census_json_close(&mut json, b']' as c_char);
        }
    }
    if json.overflow || json.length + body + 12 > line.len() {
        drop_line();
        return;
    }
    let record_body = unsafe { slice::from_raw_parts(record.body.as_ptr().cast::<u8>(), body) };
    let mut offset = json.length;
    for part in [b",\"args\":".as_slice(), record_body, b"}\n"] {
        line[offset..offset + part.len()].copy_from_slice(part);
        offset += part.len();
    }
    unsafe { append_line(line.as_ptr().cast(), offset) };
}

pub(super) unsafe fn rsf_census_record_drain() {
    let Some(ring) = RING.get() else { return };
    let mut ticket = ring.read.0.load(Relaxed);
    loop {
        let slot = &ring.slots[ticket & (SLOTS - 1)];
        let next = ticket.wrapping_add(1);
        if slot.stamp.load(Acquire) != next {
            return;
        }
        ring.read.0.store(next, Relaxed);
        let record = unsafe { (&*slot.record.get()).assume_init_ref() };
        unsafe {
            write_line(
                record,
                ring.sequence.fetch_add(1, Relaxed),
                &mut *ring.line.0.get(),
            )
        };
        slot.stamp.store(ticket.wrapping_add(SLOTS), Release);
        ticket = next;
    }
}
