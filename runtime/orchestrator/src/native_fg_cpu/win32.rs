// SPDX-License-Identifier: MIT
use crate::owned_win32_handle;
use crate::win32_diagnostics::module_location;
use std::ffi::{c_char, c_void};
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows_sys::Win32::{
    Foundation::{DuplicateHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0},
    System::{
        Diagnostics::Debug::{CONTEXT, CONTEXT_CONTROL_AMD64, GetThreadContext, ReadProcessMemory},
        Memory::{
            MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_EXECUTE_READ,
            PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS,
            VirtualQuery,
        },
        Performance::QueryPerformanceFrequency,
        SystemInformation::{GetTickCount, GetTickCount64},
        Threading::{
            CreateEventW, GetCurrentProcess, GetCurrentProcessId, GetCurrentThread,
            GetCurrentThreadId, ResumeThread, SetEvent, SuspendThread, THREAD_GET_CONTEXT,
            THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME, WaitForSingleObject,
        },
    },
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{
            GetWindowThreadProcessId, MSG, MsgWaitForMultipleObjectsEx, PM_NOREMOVE,
            PM_QS_SENDMESSAGE, PeekMessageW, PostMessageW, QS_SENDMESSAGE,
        },
    },
};

fn completion_event() -> Option<HANDLE> {
    static EVENT: OnceLock<Option<OwnedHandle>> = OnceLock::new();
    EVENT
        .get_or_init(|| unsafe { owned_win32_handle(CreateEventW(null(), 0, 0, null())) })
        .as_ref()
        .map(AsRawHandle::as_raw_handle)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_signal_completion() {
    if let Some(event) = completion_event() {
        unsafe { SetEvent(event) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_wait_completion(timeout: u32, sent_messages: u32) -> i32 {
    let Some(event) = completion_event() else {
        return 0;
    };
    if sent_messages == 0 {
        return i32::from(unsafe { WaitForSingleObject(event, timeout) } != WAIT_FAILED);
    }
    let result = unsafe { MsgWaitForMultipleObjectsEx(1, &event, timeout, QS_SENDMESSAGE, 0) };
    if result == WAIT_OBJECT_0 + 1 {
        let mut message = MSG::default();
        unsafe {
            PeekMessageW(
                &mut message,
                null_mut(),
                0,
                0,
                PM_NOREMOVE | PM_QS_SENDMESSAGE,
            )
        };
    }
    i32::from(result != WAIT_FAILED)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_owned_window_thread(key: u64) -> u32 {
    let mut process = 0;
    let thread = unsafe { GetWindowThreadProcessId(key as usize as *mut c_void, &mut process) };
    if process == unsafe { GetCurrentProcessId() } {
        thread
    } else {
        0
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_post_ping(window: *mut c_void, message: u32) -> i32 {
    unsafe { PostMessageW(window, message, 0, 0) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_input_idle_ms() -> u32 {
    let mut input = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut input) } != 0 {
        unsafe { GetTickCount() }.wrapping_sub(input.dwTime)
    } else {
        u32::MAX
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_tick_millis() -> u64 {
    unsafe { GetTickCount64() }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_qpc_frequency() -> u64 {
    let mut frequency = 0;
    unsafe { QueryPerformanceFrequency(&mut frequency) };
    frequency as u64
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_nfg_duplicate_render_thread() -> HANDLE {
    let mut copy = null_mut();
    let process = unsafe { GetCurrentProcess() };
    let access = THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION;
    if unsafe {
        DuplicateHandle(
            process,
            GetCurrentThread(),
            process,
            &mut copy,
            access,
            0,
            0,
        )
    } != 0
    {
        copy
    } else {
        null_mut()
    }
}

// Windows expects the x64 CONTEXT buffer to be 16-byte aligned.
#[repr(C, align(16))]
struct AlignedContext(CONTEXT);

unsafe fn sample_suspended(thread: HANDLE, addresses: *mut u64, capacity: usize) -> usize {
    let mut context = AlignedContext(CONTEXT {
        ContextFlags: CONTEXT_CONTROL_AMD64,
        ..Default::default()
    });
    if unsafe { GetThreadContext(thread, &mut context.0) } == 0 {
        return 0;
    }
    unsafe { addresses.write(context.0.Rip) };
    let stack = context.0.Rsp as usize as *const c_void;
    let mut region = MEMORY_BASIC_INFORMATION::default();
    if unsafe { VirtualQuery(stack, &mut region, size_of::<MEMORY_BASIC_INFORMATION>()) } == 0
        || region.State != MEM_COMMIT
        || region.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0
    {
        return 1;
    }
    let Some(available) = stack
        .addr()
        .checked_sub(region.BaseAddress.addr())
        .and_then(|offset| region.RegionSize.checked_sub(offset))
    else {
        return 1;
    };
    let words = (capacity - 1).min(available / size_of::<u64>());
    let mut copied = 0;
    if words != 0
        && unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                stack,
                addresses.add(1).cast(),
                words * size_of::<u64>(),
                &mut copied,
            )
        } != 0
    {
        return 1 + copied / size_of::<u64>();
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_nfg_sample_render_thread(
    thread: HANDLE,
    addresses: *mut u64,
    capacity: usize,
) -> usize {
    if capacity == 0 || addresses.is_null() || unsafe { SuspendThread(thread) } == u32::MAX {
        return 0;
    }
    let count = unsafe { sample_suspended(thread, addresses, capacity) };
    // The sampling helper performs no logging or allocation while suspended.
    if unsafe { ResumeThread(thread) } == u32::MAX {
        0
    } else {
        count
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_nfg_describe_address(
    address: u64,
    output: *mut c_char,
    capacity: usize,
) -> usize {
    let address = address as usize;
    let mut region = MEMORY_BASIC_INFORMATION::default();
    let executable =
        PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    if unsafe {
        VirtualQuery(
            address as *const c_void,
            &mut region,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    } == 0
        || region.Protect & executable == 0
    {
        return 0;
    }
    let Some((name, offset)) = module_location(address) else {
        return 0;
    };
    let text = format!(" {name}+0x{offset:x}");
    if output.is_null() || text.len() >= capacity {
        return 0;
    }
    unsafe {
        text.as_ptr()
            .copy_to_nonoverlapping(output.cast(), text.len());
        output.add(text.len()).write(0);
    }
    text.len()
}
