// SPDX-License-Identifier: MIT
use crate::sr_session::has_capacity;
use crate::sys::{RSF_RENDER_LINK_ABI_VERSION, rsf_render_link as Link};
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, INIT_FAILED, INVALID_ARGUMENT, NOT_READY, STALE_RESOURCES,
};
use std::{ptr, sync::Mutex};

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
    if !unsafe { has_capacity(link) } {
        return Err(INVALID_ARGUMENT);
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
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    if session == 0 || !(1..=64).contains(&capacity) {
        return INVALID_ARGUMENT;
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
        return INVALID_ARGUMENT;
    };
    if key == 0 || frame == 0 || generation == 0 {
        return INVALID_ARGUMENT;
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
        return INVALID_ARGUMENT;
    };
    if key == 0 || generation == 0 {
        return INVALID_ARGUMENT;
    }
    let table = handle.lock().unwrap();
    let Some(entry) = table.entries.iter().find(|e| e.producer_key == key) else {
        return NOT_READY;
    };
    if entry.resource_generation != generation {
        return STALE_RESOURCES;
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
        return INVALID_ARGUMENT;
    };
    let ticket = unsafe { &*link };
    if ticket.producer_key == 0 || ticket.submission_id == 0 {
        return INVALID_ARGUMENT;
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
        return STALE_RESOURCES;
    }
    table.entries.swap_remove(index);
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_render_links_destroy(raw: *mut Handle) -> i32 {
    let Some(handle) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !handle.lock().unwrap().entries.is_empty() {
        return NOT_READY;
    }
    // The ABI requires the owner to stop producers and consumers before destruction.
    drop(unsafe { Box::from_raw(raw) });
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_leased_source_across_threads_and_rejects_stale_release() {
        // Native entry points validate their size/version and own this fixture's table.
        unsafe {
            let mut handle = ptr::null_mut();
            assert_eq!(rsf_render_links_create(0, 2, &mut handle), INVALID_ARGUMENT);
            assert_eq!(
                rsf_render_links_create(42, 65, &mut handle),
                INVALID_ARGUMENT
            );
            assert!(handle.is_null());
            assert_eq!(rsf_render_links_create(42, 2, &mut handle), 0);
            let table = &*handle;
            let source = u64::from(u32::MAX) + 2;
            let mut old = EMPTY;
            let mut next = EMPTY;
            assert_eq!(rsf_render_links_bind(table, 100, source, 7, &mut old), 0);
            assert_eq!(
                rsf_render_links_bind(table, 200, source + 1, 7, &mut next),
                0
            );
            let consumed = std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let mut out = EMPTY;
                        assert_eq!(rsf_render_links_lookup(table, 100, 7, &mut out), 0);
                        out
                    })
                    .join()
                    .unwrap()
            });
            assert_eq!(
                (consumed.session_id, consumed.source_frame_id),
                (42, source)
            );
            assert!(next.submission_id > consumed.submission_id);
            let mut refused = consumed;
            assert_eq!(
                rsf_render_links_bind(table, 100, source + 2, 7, &mut refused),
                NOT_READY
            );
            assert_eq!((refused.producer_key, refused.source_frame_id), (0, 0));
            assert_eq!(
                rsf_render_links_bind(table, 300, source + 2, 7, &mut refused),
                NOT_READY
            );
            assert_eq!(rsf_render_links_destroy(handle), NOT_READY);
            assert_eq!(
                rsf_render_links_lookup(table, 100, 8, &mut refused),
                STALE_RESOURCES
            );
            assert_eq!(refused.source_frame_id, 0);
            let wrong = Link {
                session_id: 43,
                ..old
            };
            assert_eq!(rsf_render_links_release(table, &wrong), STALE_RESOURCES);
            assert_eq!(rsf_render_links_release(table, &old), 0);
            let mut reused = EMPTY;
            assert_eq!(
                rsf_render_links_bind(table, 100, source + 3, 8, &mut reused),
                0
            );
            assert!(reused.submission_id > next.submission_id);
            assert_eq!(rsf_render_links_release(table, &old), STALE_RESOURCES);
            assert_eq!(rsf_render_links_lookup(table, 100, 8, &mut refused), 0);
            assert_eq!(refused.source_frame_id, source + 3);
            assert_eq!(rsf_render_links_release(table, &reused), 0);
            assert_eq!(rsf_render_links_release(table, &next), 0);
            assert_eq!(rsf_render_links_destroy(handle), 0);
        }
    }

    #[test]
    fn serializes_same_key_admission_and_keeps_distinct_views() {
        unsafe {
            let mut handle = ptr::null_mut();
            assert_eq!(rsf_render_links_create(42, 2, &mut handle), 0);
            let table = &*handle;
            let mut first = EMPTY;
            let mut second = EMPTY;
            assert_eq!(rsf_render_links_bind(table, 100, 50, 8, &mut first), 0);
            assert_eq!(rsf_render_links_bind(table, 200, 50, 8, &mut second), 0);
            assert_eq!(first.source_frame_id, second.source_frame_id);
            assert_ne!(first.submission_id, second.submission_id);
            assert_eq!(rsf_render_links_release(table, &second), 0);
            assert_eq!(rsf_render_links_release(table, &first), 0);
            let results = std::thread::scope(|scope| {
                let try_bind = |frame| {
                    let mut out = EMPTY;
                    let result = rsf_render_links_bind(table, 500, frame, 8, &mut out);
                    (result, out)
                };
                let a = scope.spawn(move || try_bind(51));
                let b = scope.spawn(move || try_bind(52));
                [a.join().unwrap(), b.join().unwrap()]
            });
            let mut accepted = 0;
            for (result, link) in results {
                if result == 0 {
                    accepted += 1;
                    assert_eq!(rsf_render_links_release(table, &link), 0);
                } else {
                    assert_eq!(result, NOT_READY);
                    assert_eq!(link.producer_key, 0);
                }
            }
            assert_eq!(accepted, 1);
            assert_eq!(rsf_render_links_destroy(handle), 0);
        }
    }
}
