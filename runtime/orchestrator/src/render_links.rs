// SPDX-License-Identifier: MIT
use crate::render_abi::{RSF_RENDER_LINK_ABI_VERSION, rsf_render_link as Link};
use std::{ptr, sync::Mutex};

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const STALE: i32 = -10;

const EMPTY: Link = Link {
    struct_size: size_of::<Link>() as u32,
    abi_version: RSF_RENDER_LINK_ABI_VERSION,
    session_id: 0,
    source_frame_id: 0,
    submission_id: 0,
    resource_generation: 0,
    producer_key: 0,
};

struct Table {
    session: u64,
    last_submission: u64,
    capacity: usize,
    entries: Vec<Link>,
}
type Handle = Mutex<Table>;

unsafe fn validate(link: *const Link) -> Result<(), i32> {
    if link.is_null() || unsafe { link.cast::<u32>().read() } < EMPTY.struct_size {
        return Err(INVALID);
    }
    if unsafe { (*link).abi_version } != EMPTY.abi_version {
        return Err(ABI_MISMATCH);
    }
    Ok(())
}

unsafe fn clear_output(out: *mut Link) -> Result<(), i32> {
    unsafe {
        validate(out)?;
        out.write(EMPTY);
    }
    Ok(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_render_links_create(
    session: u64,
    capacity: u32,
    out: *mut *mut Handle,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    if session == 0 || !(1..=64).contains(&capacity) {
        return INVALID;
    }
    let mut entries = Vec::new();
    let capacity = capacity as usize;
    if entries.try_reserve_exact(capacity).is_err() {
        return INIT_FAILED;
    }
    let handle = crate::allocate_handle(Mutex::new(Table {
        session,
        last_submission: 0,
        capacity,
        entries,
    }));
    if handle.is_null() {
        return INIT_FAILED;
    }
    unsafe { out.write(handle) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_render_links_bind(
    handle: *const Handle,
    key: u64,
    frame: u64,
    generation: u64,
    out: *mut Link,
) -> i32 {
    if let Err(code) = unsafe { clear_output(out) } {
        return code;
    }
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID;
    };
    if key == 0 || frame == 0 || generation == 0 {
        return INVALID;
    }
    let mut table = handle.lock().unwrap();
    if table.entries.len() == table.capacity || table.entries.iter().any(|e| e.producer_key == key)
    {
        return NOT_READY;
    }
    let Some(id) = table.last_submission.checked_add(1) else {
        return NOT_READY;
    };
    let entry = Link {
        session_id: table.session,
        source_frame_id: frame,
        submission_id: id,
        resource_generation: generation,
        producer_key: key,
        ..EMPTY
    };
    table.last_submission = id;
    table.entries.push(entry);
    unsafe { out.write(entry) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_render_links_lookup(
    handle: *const Handle,
    key: u64,
    generation: u64,
    out: *mut Link,
) -> i32 {
    if let Err(code) = unsafe { clear_output(out) } {
        return code;
    }
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID;
    };
    if key == 0 || generation == 0 {
        return INVALID;
    }
    let table = handle.lock().unwrap();
    let Some(entry) = table.entries.iter().find(|e| e.producer_key == key) else {
        return NOT_READY;
    };
    if entry.resource_generation != generation {
        return STALE;
    }
    unsafe { out.write(*entry) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_render_links_release(handle: *const Handle, link: *const Link) -> i32 {
    if let Err(code) = unsafe { validate(link) } {
        return code;
    }
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return INVALID;
    };
    let ticket = unsafe { &*link };
    if ticket.producer_key == 0 || ticket.submission_id == 0 {
        return INVALID;
    }
    let mut table = handle.lock().unwrap();
    let Some(index) = table
        .entries
        .iter()
        .position(|e| e.producer_key == ticket.producer_key)
    else {
        return NOT_READY;
    };
    let entry = &table.entries[index];
    if entry.session_id != ticket.session_id
        || entry.source_frame_id != ticket.source_frame_id
        || entry.submission_id != ticket.submission_id
        || entry.resource_generation != ticket.resource_generation
    {
        return STALE;
    }
    table.entries.swap_remove(index);
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_render_links_destroy(raw: *mut Handle) -> i32 {
    let Some(handle) = (unsafe { raw.as_ref() }) else {
        return INVALID;
    };
    if !handle.lock().unwrap().entries.is_empty() {
        return NOT_READY;
    }
    // The ABI requires the owner to stop producers and consumers before destruction.
    drop(unsafe { Box::from_raw(raw) });
    0
}
