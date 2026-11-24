// SPDX-License-Identifier: MIT
use crate::sys::*;
use crate::sys::{RSF_FG_FIXED, RSF_FG_OFF, RSF_REFLEX_ON};
use std::ffi::c_void;
use std::mem::{align_of, size_of};
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

const SIZE: usize = size_of::<rsf_d3d11_fg_api>();
const VERSION: u32 = RSF_D3D11_FG_API_VERSION;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
type Query = unsafe extern "C" fn(*mut rsf_d3d11_fg_api) -> i32;

#[repr(C, align(8))]
#[derive(Clone, Copy, PartialEq, Eq)]
struct RawTable {
    capacity: u32,
    version: u32,
    slots: [u8; SIZE - 8],
}

impl RawTable {
    fn new(capacity: u32, version: u32, fill: u8) -> Self {
        Self {
            capacity,
            version,
            slots: [fill; SIZE - 8],
        }
    }

    fn as_raw(&mut self) -> *mut rsf_d3d11_fg_api {
        std::ptr::from_mut(self).cast()
    }
}

#[repr(C)]
struct Larger {
    table: RawTable,
    tail: [u8; 32],
}

const _: () = {
    assert!(size_of::<RawTable>() == SIZE);
    assert!(align_of::<RawTable>() == align_of::<rsf_d3d11_fg_api>());
};

#[derive(Default)]
struct Check {
    failures: u32,
}

impl Check {
    fn that(&mut self, condition: bool, message: &str) {
        if !condition {
            self.failures += 1;
            eprintln!("FAIL: {message}");
        }
    }

    fn refusal(&mut self, query: Query, size: u32, version: u32, expected: i32) {
        let mut table = RawTable::new(size, version, 0xa5);
        let before = table;
        self.that(
            unsafe { query(table.as_raw()) } == expected,
            "deterministic ABI refusal result",
        );
        self.that(table == before, "refusal preserves every caller byte");
    }
}

fn now() -> u64 {
    let mut ticks = 0;
    unsafe { QueryPerformanceCounter(&mut ticks) };
    ticks as u64
}

fn qpc_frequency() -> u64 {
    let mut frequency = 0;
    unsafe { QueryPerformanceFrequency(&mut frequency) };
    frequency as u64
}

fn same<F: Copy>(actual: Option<F>, expected: Option<F>, equal: impl FnOnce(F, F) -> bool) -> bool {
    actual.zip(expected).is_some_and(|(a, b)| equal(a, b))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_d3d11_fg_api(
    imports: *const rsf_fg_fixture_imports,
    native: *const rsf_d3d11_fg_api,
) -> i32 {
    let (Some(imports), Some(native)) = (unsafe { (imports.as_ref(), native.as_ref()) }) else {
        return 1;
    };
    let (Some(query), Some(query_present), Some(read_cpu), Some(render_latency)) = (
        imports.fg,
        imports.present,
        imports.read_cpu,
        imports.render_latency,
    ) else {
        return 1;
    };

    let mut check = Check::default();
    check.that(
        unsafe { query(null_mut()) } == INVALID,
        "null query is INVALID_ARGUMENT",
    );
    for size in [0, 4, 8, (SIZE - 1) as u32] {
        check.refusal(query, size, VERSION, INVALID);
        check.refusal(query, size, u32::MAX, INVALID);
    }
    for version in [0, 2, 3, VERSION + 1, u32::MAX] {
        check.refusal(query, SIZE as u32, version, ABI_MISMATCH);
    }
    let mut large = Larger {
        table: RawTable::new(size_of::<Larger>() as u32, VERSION, 0x5a),
        tail: [0x5a; 32],
    };
    check.that(
        unsafe { query(large.table.as_raw()) } == 0,
        "larger capacity accepted",
    );
    check.that(
        large.table.capacity == SIZE as u32,
        "struct_size reports the runtime table",
    );
    check.that(large.tail == [0x5a; 32], "bytes beyond the table untouched");

    // Zeroed pointers are valid nullable callback slots.
    let mut api: rsf_d3d11_fg_api = unsafe { std::mem::zeroed() };
    api.struct_size = SIZE as u32;
    api.abi_version = VERSION;
    if unsafe { query(&mut api) } != 0 {
        eprintln!("FAIL: query current FG table");
        return 1;
    }
    let (
        Some(options),
        Some(status),
        Some(cpu),
        Some(pass),
        Some(bind),
        Some(capture),
        Some(ready),
        Some(dump),
        Some(_),
    ) = (
        api.options,
        api.status,
        api.cpu,
        api.pass,
        api.bind_source,
        api.capture,
        api.inputs_ready,
        api.dump_inputs,
        api.prepare,
    )
    else {
        eprintln!("FAIL: required FG entry missing");
        return 1;
    };
    check.that(
        api.set_log.is_some()
            && api.reflex_ordering.is_some()
            && api.retire.is_some()
            && api.present_event.is_some()
            && api.latency_event.is_some(),
        "every entry populated",
    );
    check.that(
        same(api.prepare, native.prepare, std::ptr::fn_addr_eq)
            && same(api.retire, native.retire, std::ptr::fn_addr_eq)
            && same(api.options, native.options, std::ptr::fn_addr_eq)
            && same(api.status, native.status, std::ptr::fn_addr_eq)
            && same(api.bind_source, native.bind_source, std::ptr::fn_addr_eq),
        "entries are the Runtime's native FG owner",
    );

    let settings = rsf_native_fg_options {
        struct_size: size_of::<rsf_native_fg_options>() as u32,
        mode: RSF_FG_FIXED,
        generated_frames: 2,
        reflex_mode: RSF_REFLEX_ON,
        frame_limit_us: 8_333,
        debug: 0,
    };
    let mut current = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { options(&settings) };
    check.that(
        unsafe { status(&mut current) } == 1
            && current.available == 0
            && current.vendor.effective_mode == RSF_FG_OFF
            && current.requested.generated_frames == 2
            && current.requested.frame_limit_us == 8_333,
        "status before a bridge owner",
    );
    let mut short_status = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32 - 1,
        ..unsafe { std::mem::zeroed() }
    };
    check.that(
        unsafe { status(&mut short_status) } == 0,
        "short status refused",
    );

    let session = 77;
    let frequency = qpc_frequency();
    let cpu_header = |stage, id| rsf_test_support::cpu_event(session, id, stage, now(), frequency);
    let event = |stage, id| unsafe { cpu(&cpu_header(stage, id)) };
    check.that(
        event(RSF_GAME_CPU_FRAME_BEGIN, 1) == 1,
        "frame begin accepted",
    );
    check.that(
        event(RSF_GAME_CPU_SIMULATION_END, 1) == 0,
        "simulation end before input refused",
    );
    event(RSF_GAME_CPU_FRAME_END, 1);
    for stage in [
        RSF_GAME_CPU_FRAME_BEGIN,
        RSF_GAME_CPU_PACING,
        RSF_GAME_CPU_INPUT_SAMPLE,
        RSF_GAME_CPU_SIMULATION_BEGIN,
        RSF_GAME_CPU_SIMULATION_END,
        RSF_GAME_CPU_FRAME_END,
    ] {
        check.that(event(stage, 2) == 1, "ordered frame accepted");
    }
    check.that(
        event(RSF_GAME_CPU_FRAME_BEGIN, 2) == 0,
        "repeated frame id refused",
    );
    let short = rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32 - 1,
        ..cpu_header(RSF_GAME_CPU_FRAME_BEGIN, 3)
    };
    check.that(unsafe { cpu(&short) } == 0, "short CPU event refused");

    let mut reserved = rsf_game_cpu_source_event {
        header: rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_source_event>() as u32,
            render_expected: 0,
            event_version: RSF_GAME_CPU_EVENT_VERSION_SOURCE,
            ..cpu_header(RSF_GAME_CPU_SOURCE_RESERVE, 3)
        },
        native_generation: 1,
        ..unsafe { std::mem::zeroed() }
    };
    check.that(
        unsafe { cpu((&raw const reserved).cast()) } == 1,
        "prospective reservation accepted without graphics",
    );
    reserved.header.stage = RSF_GAME_CPU_SOURCE_SAMPLED;
    reserved.poll_begin_qpc = now();
    reserved.header.timestamp_qpc = now();
    reserved.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
    check.that(
        unsafe { cpu((&raw const reserved).cast()) } == 1,
        "poll completion accepted",
    );
    let mut begin = rsf_game_cpu_event {
        render_expected: 0,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        ..cpu_header(RSF_GAME_CPU_FRAME_BEGIN, 3)
    };
    check.that(
        unsafe { cpu(&begin) } == 1,
        "flags0 prospective frame adopted",
    );
    reserved.header.stage = RSF_GAME_CPU_SOURCE_BIND;
    reserved.header.timestamp_qpc = now();
    reserved.header.flags = 0;
    reserved.poll_begin_qpc = 0;
    check.that(
        unsafe { cpu((&raw const reserved).cast()) } == 1,
        "consumer binds real poll",
    );
    let mut observed = rsf_native_cpu_frame {
        struct_size: size_of::<rsf_native_cpu_frame>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    check.that(
        unsafe { read_cpu(session, 3, &mut observed) } != 0
            && observed.stage_mask == 7
            && observed.flags & RSF_GAME_CPU_DERIVED_BOUNDARY != 0
            && observed.pacing_qpc == 0
            && observed.marker_mask == 0,
        "derived CPU begin does not invent vendor pacing or markers",
    );
    let render_pass = |role, source_frame_id| rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        role,
        session_id: session,
        source_frame_id,
        ..unsafe { std::mem::zeroed() }
    };
    let unowned_submission = rsf_game_render_pass {
        submission_id: 3,
        view_key: 3,
        flags: RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION,
        ..render_pass(RSF_GAME_RENDER_SUBMISSION, 3)
    };
    unsafe {
        pass(null_mut(), &unowned_submission, 1);
        render_latency(3, 1);
    }
    check.that(
        unsafe { read_cpu(session, 3, &mut observed) } != 0
            && observed.stage_mask == 7
            && observed.marker_mask == 0,
        "unowned submission cannot derive vendor interval end",
    );
    begin.stage = RSF_GAME_CPU_FRAME_END;
    begin.timestamp_qpc = now();
    begin.flags = RSF_GAME_CPU_RENDER_SKIPPED;
    check.that(
        unsafe { cpu(&begin) } == 1,
        "explicit skipped partial prospective frame closes",
    );

    let mut short_pass = render_pass(RSF_GAME_RENDER_WINDOW, 0);
    short_pass.struct_size -= 1;
    check.that(
        unsafe { pass(null_mut(), &short_pass, 1) == 0 && pass(null_mut(), null(), 1) == 0 },
        "short or null pass refused",
    );
    check.that(
        unsafe { pass(null_mut(), &render_pass(RSF_GAME_RENDER_WINDOW, 2), 1) } == 0,
        "WINDOW without scope/window/viewport/swapchain refused",
    );
    check.that(
        unsafe { pass(null_mut(), &render_pass(RSF_GAME_RENDER_FINAL_SCENE, 2), 1) } == 0,
        "FINAL_SCENE without primary/after-simulation identity refused",
    );
    let frame_pass = rsf_game_render_pass {
        session_id: 0,
        ..render_pass(RSF_GAME_RENDER_FRAME, 0)
    };
    check.that(
        unsafe { pass(null_mut(), &frame_pass, 1) == 1 && pass(null_mut(), &frame_pass, 0) == 1 },
        "FRAME without a bridge is a no-op",
    );

    let identity = rsf_game_render_pass {
        session_id: 0,
        ..render_pass(0, 5)
    };
    let mut short_identity = identity;
    short_identity.struct_size -= 1;
    unsafe {
        bind(&identity);
        bind(null());
        bind(&short_identity);
        bind(null());
    }
    check.that(
        unsafe { ready(0) == 0 && ready(5) == 0 },
        "no inputs held without a capture",
    );
    check.that(
        unsafe { dump(null_mut(), 5, c"x".as_ptr(), c"y".as_ptr()) } == 0,
        "dump without inputs writes nothing",
    );
    unsafe { capture(null_mut(), &identity, null()) };
    let mut malformed = rsf_d3d11_fg_inputs {
        struct_size: size_of::<rsf_d3d11_fg_inputs>() as u32,
        abi_version: RSF_D3D11_FG_INPUTS_VERSION,
        depth: null_mut(),
        motion: null_mut(),
    };
    // All invalid forms must be refused before the deliberately invalid context is used.
    let sentinel = (&raw mut malformed).cast::<c_void>();
    unsafe { capture(sentinel, &identity, &malformed) };
    malformed.abi_version = 0;
    unsafe { capture(sentinel, &identity, &malformed) };
    malformed.struct_size = size_of::<u32>() as u32;
    unsafe { capture(sentinel, &identity, &malformed) };
    check.that(
        unsafe { ready(5) } == 0,
        "refused capture does not hold inputs",
    );

    let mut present = rsf_d3d11_present_api {
        struct_size: size_of::<rsf_d3d11_present_api>() as u32,
        abi_version: RSF_D3D11_PRESENT_API_VERSION,
        ..unsafe { std::mem::zeroed() }
    };
    check.that(
        unsafe { query_present(&mut present) } == 0
            && present
                .has_owner
                .is_some_and(|has_owner| unsafe { has_owner() } == 0),
        "present API intact, no owner",
    );
    if check.failures != 0 {
        eprintln!("{} failure(s)", check.failures);
        1
    } else {
        println!("PASS: rsf_get_d3d11_fg_api contract through the real Runtime DLL");
        0
    }
}
