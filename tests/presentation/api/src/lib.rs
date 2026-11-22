// SPDX-License-Identifier: MIT
#![cfg(all(windows, not(test)))]
#![allow(non_camel_case_types)]

mod abi;

use abi::*;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{self, null, null_mut};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW, GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
    VirtualAlloc, VirtualFree, VirtualProtect,
};
use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

type rsf_backend_log_fn = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;
type rsf_observer_present_fn = Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>;
type rsf_observer_present_event_fn = Option<unsafe extern "C" fn(*mut c_void, *const c_void)>;
type rsf_generation_provider = c_void;
type rsf_latency_marker = u32;
type rsf_result = i32;
type rsf_streamline_graphics = c_void;
type rsf_streamline_host = c_void;
type Query = unsafe extern "C" fn(*mut rsf_d3d11_present_api) -> i32;

const SIZE: usize = size_of::<rsf_d3d11_present_api>();
const VERSION: u32 = RSF_D3D11_PRESENT_API_VERSION;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;

// Refusal inputs contain arbitrary bytes in the function slots. Keep them raw.
#[repr(C, align(8))]
#[derive(Clone, Copy, PartialEq, Eq)]
struct RawTable {
    capacity: u32,
    version: u32,
    slots: [u8; SIZE - 8],
}

impl RawTable {
    fn new(capacity: u32, version: u32) -> Self {
        Self {
            capacity,
            version,
            slots: [0xa5; SIZE - 8],
        }
    }

    fn call(&mut self, query: Query) -> i32 {
        unsafe { query(ptr::from_mut(self).cast()) }
    }

    fn populated(&self) -> rsf_d3d11_present_api {
        // Only called after the Runtime has initialized every function slot.
        unsafe { ptr::from_ref(self).cast::<rsf_d3d11_present_api>().read() }
    }
}

const _: () = {
    assert!(size_of::<RawTable>() == SIZE);
    assert!(align_of::<RawTable>() == align_of::<rsf_d3d11_present_api>());
};

type Check = Result<(), &'static str>;

fn require(condition: bool, message: &'static str) -> Check {
    if condition { Ok(()) } else { Err(message) }
}

fn refusal_contract(query: Query) -> Check {
    require(
        unsafe { query(null_mut()) } == INVALID_ARGUMENT,
        "null query",
    )?;
    for size in [0, 1, 3, 4, 7, 8, SIZE as u32 - 1] {
        for version in [VERSION, u32::MAX] {
            refuses(query, size, version, INVALID_ARGUMENT)?;
        }
    }
    for version in [0, VERSION + 1, u32::MAX] {
        for size in [SIZE as u32, u32::MAX] {
            refuses(query, size, version, ABI_MISMATCH)?;
        }
    }
    Ok(())
}

fn refuses(query: Query, capacity: u32, version: u32, expected: i32) -> Check {
    let before = RawTable::new(capacity, version);
    let mut after = before;
    require(after.call(query) == expected, "query refusal HRESULT")?;
    require(before == after, "query refusal preserves caller bytes")
}

struct Pages(*mut c_void);

impl Drop for Pages {
    fn drop(&mut self) {
        unsafe { VirtualFree(self.0, 0, MEM_RELEASE) };
    }
}

fn guard_page_contract(query: Query) -> Check {
    let mut system = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut system) };
    let page = system.dwPageSize as usize;
    let memory =
        Pages(unsafe { VirtualAlloc(null(), page * 2, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) });
    if memory.0.is_null() {
        return Err("allocate refusal pages");
    }
    let first = memory.0.cast::<u8>();
    let guard = unsafe { first.add(page) };
    let prefix = unsafe { guard.sub(8).cast::<[u32; 2]>() };
    let mut previous = 0;
    require(
        unsafe { VirtualProtect(guard.cast(), page, PAGE_NOACCESS, &mut previous) } != 0,
        "protect inaccessible function slots",
    )?;
    for (size, expected) in [
        (SIZE as u32 - 1, INVALID_ARGUMENT),
        (SIZE as u32, ABI_MISMATCH),
    ] {
        let header = [size, u32::MAX];
        unsafe { prefix.write(header) };
        require(
            unsafe { VirtualProtect(memory.0, page, PAGE_READONLY, &mut previous) } != 0,
            "protect refusal header",
        )?;
        require(
            unsafe { query(prefix.cast()) } == expected,
            "query reads only accessible prefix",
        )?;
        require(
            unsafe { prefix.read() } == header,
            "read-only prefix is unchanged",
        )?;
        require(
            unsafe { VirtualProtect(memory.0, page, PAGE_READWRITE, &mut previous) } != 0,
            "restore writable refusal header",
        )?;
    }
    Ok(())
}

fn slot_in_module(address: *const u16, runtime: HMODULE) -> bool {
    let mut owner = null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    (unsafe { GetModuleHandleExW(flags, address, &mut owner) }) != 0 && owner == runtime
}

fn slot_contract(api: &rsf_d3d11_present_api, runtime: HMODULE) -> Check {
    macro_rules! check {
        ($($slot:ident),* $(,)?) => {
            $(
                let pointer = api.$slot.ok_or(concat!(stringify!($slot), " slot missing"))?;
                require(slot_in_module(pointer as *const u16, runtime),
                    concat!(stringify!($slot), " slot must belong to the imported Runtime"))?;
            )*
        };
    }
    check!(
        install,
        prepare,
        create_for_hwnd,
        host,
        graphics,
        queue,
        command_list,
        retire,
        is_owner,
        has_owner,
        provider,
        session,
        backend,
        backend_choices,
        request,
        switch_result,
        generation,
        begin,
        marker,
        acquire,
        input,
        abort
    );
    Ok(())
}

fn snapshot(api: &rsf_d3d11_present_api) -> (i32, u32, u32, i32, u64) {
    // Population and module checks establish all five pointers before use.
    unsafe {
        (
            api.has_owner.unwrap()(),
            api.backend.unwrap()(),
            api.backend_choices.unwrap()(),
            api.switch_result.unwrap()(),
            api.generation.unwrap()(),
        )
    }
}

fn extension_contract(query: Query, original: RawTable) -> Check {
    #[repr(C)]
    struct Extended {
        before: u64,
        table: RawTable,
        after: [u8; 64],
    }

    let mut extended = Extended {
        before: 0x5a5a5a5a5a5a5a5a,
        table: RawTable::new((SIZE + 64) as u32, VERSION),
        after: [0x5a; 64],
    };
    let allocation = ptr::from_mut(&mut extended);
    let table = unsafe { &raw mut (*allocation).table };
    require(
        unsafe { query(table.cast()) } == 0,
        "larger caller capacity is accepted",
    )?;
    require(
        extended.before == 0x5a5a5a5a5a5a5a5a,
        "leading canary preserved",
    )?;
    require(extended.after == [0x5a; 64], "extension bytes preserved")?;
    require(extended.table == original, "table bytes are stable")
}

fn parallel_contract(query: Query, original: RawTable) -> Check {
    let equal = std::thread::scope(|scope| {
        let handles = (0..8)
            .map(|_| {
                scope.spawn(move || {
                    (0..1000).all(|_| {
                        let mut copy = RawTable::new(SIZE as u32, VERSION);
                        copy.call(query) == 0 && copy == original
                    })
                })
            })
            .collect::<Vec<_>>();
        handles.into_iter().fold(true, |all_equal, handle| {
            handle.join().unwrap_or(false) && all_equal
        })
    });
    require(equal, "parallel queries return a stable table")
}

unsafe extern "C" fn log_scale(user: *mut c_void, text: *const c_char) {
    let text = unsafe { CStr::from_ptr(text) }.to_bytes();
    let field = b"units_to_meters";
    unsafe {
        user.cast::<bool>()
            .write(text.windows(field.len()).any(|part| part == field))
    };
}

unsafe extern "C" fn accept_window(_: *mut c_void, _: *mut c_void) -> i32 {
    1
}

fn setup_contract(api: &rsf_d3d11_present_api) -> Check {
    let mut reported_units = false;
    let mut setup: rsf_d3d11_present_setup = unsafe { std::mem::zeroed() };
    setup.struct_size = size_of::<rsf_d3d11_present_setup>() as u32;
    setup.runtime_directory_utf8 = c".".as_ptr();
    setup.user = ptr::from_mut(&mut reported_units).cast();
    setup.log = Some(log_scale);
    setup.accept_window = Some(accept_window);
    setup.units_to_meters = f32::INFINITY;
    let result = unsafe { api.prepare.unwrap()(&setup) };
    require(
        result == 0 && reported_units,
        "public setup refuses positive infinity",
    )
}

fn run(query: Query) -> Check {
    refusal_contract(query)?;
    guard_page_contract(query)?;
    let name = "ReScaleFrame.Runtime.dll"
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let runtime = unsafe { GetModuleHandleW(name.as_ptr()) };
    require(!runtime.is_null(), "real Runtime DLL is imported")?;
    require(
        unsafe { GetProcAddress(runtime, RSF_D3D11_PRESENT_API_ENTRY_POINT.as_ptr()) }.is_some(),
        "query has an undecorated C export",
    )?;
    let mut table = RawTable::new(SIZE as u32, VERSION);
    require(table.call(query) == 0, "compatible query succeeds")?;
    require(
        table.capacity == SIZE as u32 && table.version == VERSION,
        "supported size and version",
    )?;
    let api = table.populated();
    slot_contract(&api, runtime)?;
    let before = snapshot(&api);
    require(before.0 == 0, "query does not create a presentation owner")?;
    refusal_contract(query)?;
    require(
        snapshot(&api) == before,
        "refusals preserve owner observations",
    )?;
    extension_contract(query, table)?;
    parallel_contract(query, table)?;
    setup_contract(&api)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11_present_api_fixture(query: rsf_get_d3d11_present_api_fn) -> i32 {
    let result = query.ok_or("missing DLL import").and_then(run);
    if let Err(error) = result {
        eprintln!("FAIL: {error}");
        1
    } else {
        println!("PASS: Runtime DLL import, table, ABI refusal, canaries and concurrent queries");
        0
    }
}
