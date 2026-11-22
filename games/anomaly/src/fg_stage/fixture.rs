// SPDX-License-Identifier: MIT
//! The original CTest scenarios, through the same Rust stage and Runtime tables as the game.
use super::options::Settings;
use super::*;
use std::cell::RefCell;
use std::slice;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::core::w;

use crate::sys::*;
#[path = "fixture_device.rs"]
mod device;

struct Checks(u32);

impl Checks {
    fn require(&mut self, condition: bool, label: &str) {
        if !condition {
            eprintln!("FAIL {label}");
            self.0 += 1;
        }
    }

    fn exit(&self) -> i32 {
        i32::from(self.0 != 0)
    }
}

unsafe extern "C" fn log_line(_: Raw, message: *const c_char) {
    println!(
        "  | {}",
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    );
}

const LOG: Log = Log {
    callback: Some(log_line),
    user: 0,
};

fn options(backend: Option<u32>, capture_dir: String, vendors: [Vec<u16>; 3]) -> Options {
    Options::admit(
        Settings {
            bridge: true,
            debug: false,
            development: false,
            scope_hold: 4,
            capture: if backend.is_some() {
                CaptureSettings::new(true, 0, 1, 1)
            } else {
                CaptureSettings::new(false, 600, 3, 300)
            },
            frontend: Frontend {
                enabled: u32::from(backend.is_some()),
                backend: backend.unwrap_or(RSF_FG_BACKEND_AUTO),
                generated: 1,
                latency: RSF_REFLEX_ON,
                frame_limit_us: 0,
            },
        },
        capture_dir,
        vendors.map(|path| crate::sr_stage::plain_directory(&path)),
    )
}

fn engine_bindings(state_scope: StateScope) -> EngineBindings {
    fn sr_owns_depth(_: u32) -> bool {
        // The fixture's Temporal driver is separate from the native SR scene owner.
        false
    }
    EngineBindings {
        state_scope,
        engine_depth_view: device::depth_view,
        sr_owns_depth,
        motion_target: device::motion_target,
        graphics: d3d::Graphics::default(),
    }
}

type Cpu = unsafe extern "C" fn(*const rsf_game_cpu_event) -> i32;
static CORE_CPU: OnceLock<Cpu> = OnceLock::new();
thread_local! {
    static INPUT_FACTS: RefCell<Vec<rsf_game_cpu_event>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C" fn observe_input(event: *const rsf_game_cpu_event) -> i32 {
    INPUT_FACTS.with_borrow_mut(|facts| facts.push(unsafe { *event }));
    unsafe { CORE_CPU.get().expect("CPU callback bound")(event) }
}

unsafe extern "C" fn cpu_only_install(_: *const rsf_d3d11_present_setup) -> i32 {
    1
}

fn cpu_contract(
    mut tables: Tables,
    label: &str,
    verified_input: bool,
    platform: &rsf_anomaly_fg_fixture_platform,
    state_scope: StateScope,
) -> i32 {
    let (Some(release), Some(ping_message), Some(read_cpu), Some(cpu_counters)) = (
        platform.release_cpu,
        platform.ping_message,
        platform.read_cpu,
        platform.cpu_counters,
    ) else {
        return 1;
    };
    unsafe { release() };
    let _ = CORE_CPU.set(tables.fg.cpu);
    tables.fg.cpu = observe_input;
    tables.present.install = cpu_only_install;
    if !Stage::install(
        options(None, String::new(), Default::default()),
        Log::default(),
        tables,
        engine_bindings(state_scope),
        0,
    ) {
        eprintln!("FAIL CPU-only stage install");
        return 1;
    }
    let mut checks = Checks(0);
    checks.require(
        verified_input,
        "selected profile has both input hook locations",
    );
    rsf_anomaly_fg_stage_input_begin(100);
    let Some(first) = INPUT_FACTS.with_borrow(|facts| {
        checks.require(
            facts.iter().map(|event| event.stage).eq([
                RSF_GAME_CPU_FRAME_BEGIN,
                RSF_GAME_CPU_INPUT_SAMPLE,
                RSF_GAME_CPU_SIMULATION_BEGIN,
            ]),
            "input callback reports begin, sample and simulation begin in order",
        );
        facts.first().copied()
    }) else {
        return 1;
    };
    let session = first.session_id;
    for kind in [
        RSF_GAME_INPUT_KEYBOARD,
        RSF_GAME_INPUT_KEYBOARD,
        RSF_GAME_INPUT_MOUSE,
        RSF_GAME_INPUT_CONTROLLER,
    ] {
        rsf_anomaly_fg_stage_input_received(kind, 0);
    }
    rsf_anomaly_fg_stage_render_begin(100, device::WIDTH, device::HEIGHT);
    rsf_anomaly_fg_stage_scope();
    close_frame();
    let read = |source| {
        let mut frame = rsf_native_cpu_frame {
            struct_size: size_of::<rsf_native_cpu_frame>() as u32,
            ..Default::default()
        };
        (unsafe { read_cpu(session, source, &mut frame) } != 0).then_some(frame)
    };
    checks.require(
        read(first.source_frame_id).is_some_and(|frame| {
            frame.ended != 0 && frame.input_mask == 7 && frame.input_events == [1; 3]
        }),
        "ended source records one event per device kind",
    );
    unsafe { ping_message(0xc123) };
    rsf_anomaly_fg_stage_input_received(0, 0xc123);
    let last = || INPUT_FACTS.with_borrow(|facts| facts.last().copied().unwrap_or_default());
    let message = last();
    checks.require(
        message.source_frame_id == 0 && message.flags & RSF_GAME_CPU_WINDOW_MESSAGE != 0,
        "unframed delivered message",
    );
    rsf_anomaly_fg_stage_input_begin(101);
    checks.require(
        read(last().source_frame_id).is_some_and(|frame| frame.pings == 1 && frame.input_mask == 0),
        "ping attached to the following frame",
    );
    rsf_anomaly_fg_stage_render_begin(101, device::WIDTH, device::HEIGHT);
    close_frame();
    rsf_anomaly_fg_stage_input_begin(102);
    let skipped = last().source_frame_id;
    rsf_anomaly_fg_stage_input_begin(103);
    INPUT_FACTS.with_borrow(|facts| {
        let events: Vec<_> = facts
            .iter()
            .filter(|event| event.source_frame_id == skipped)
            .collect();
        checks.require(
            events
                .iter()
                .all(|event| event.stage != RSF_GAME_CPU_SIMULATION_END),
            "no inferred simulation end",
        );
        checks.require(
            events.iter().any(|event| {
                event.stage == RSF_GAME_CPU_FRAME_END
                    && event.flags & RSF_GAME_CPU_RENDER_SKIPPED != 0
            }),
            "skipped source ended",
        );
    });
    checks.require(
        read(skipped).is_some_and(|frame| {
            frame.ended != 0 && frame.timestamps_qpc[RSF_GAME_CPU_SIMULATION_END as usize] == 0
        }),
        "skipped source has no simulation-end timestamp",
    );
    rsf_anomaly_fg_stage_render_begin(103, device::WIDTH, device::HEIGHT);
    close_frame();
    rsf_anomaly_fg_stage_render_begin(104, device::WIDTH, device::HEIGHT);
    checks.require(
        read(last().source_frame_id)
            .is_some_and(|frame| frame.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE != 0),
        "paint-only frame substitute",
    );
    close_frame();
    checks.require(
        INPUT_FACTS
            .with_borrow(|facts| facts.iter().all(|event| event.stage != RSF_GAME_CPU_PACING)),
        "integration never paces",
    );
    let mut totals = rsf_native_cpu_counters {
        struct_size: size_of::<rsf_native_cpu_counters>() as u32,
        ..Default::default()
    };
    checks.require(
        unsafe { cpu_counters(&mut totals) } != 0,
        "CPU counters readable",
    );
    checks.require(
        totals.order_violations == 0 && totals.substitutes == 1 && totals.pings == 1,
        "CPU totals",
    );
    println!("{label} input facts: {} failures", checks.0);
    checks.exit()
}

fn close_frame() {
    rsf_anomaly_fg_stage_end_before();
    rsf_anomaly_fg_stage_end_after();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_anomaly_fg(
    argc: i32,
    argv: *const *const c_char,
    platform: *const rsf_anomaly_fg_fixture_platform,
) -> i32 {
    if argc < 1 || argv.is_null() {
        return 77;
    }
    let Some(platform) = (unsafe { full_table(platform) }) else {
        return 1;
    };
    let Some(state_scope) = StateScope::bind(platform.state_scope) else {
        return 1;
    };
    let args: Vec<_> = unsafe { slice::from_raw_parts(argv, argc as usize) }[1..]
        .iter()
        .map(|&arg| unsafe { CStr::from_ptr(arg) })
        .collect();
    if let [command, profile] = args.as_slice()
        && *command == c"--input-contract"
    {
        let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
        let Some(tables) = Tables::query(runtime, LOG) else {
            return 1;
        };
        return if *profile == c"avx" {
            cpu_contract(
                tables,
                "avx",
                crate::input::has_fixture_profile(1),
                platform,
                state_scope,
            )
        } else {
            cpu_contract(
                tables,
                "standard",
                crate::input::has_fixture_profile(0),
                platform,
                state_scope,
            )
        };
    }
    let [sr, backend, root, tail @ ..] = args.as_slice() else {
        println!("SKIP: <sr off|5> <backend 1|3|5> <vendor root> [--switch]");
        return 77;
    };
    let backend = parse_backend(backend.to_bytes());
    device::run(
        *sr == c"5",
        backend,
        root,
        tail.first() == Some(&c"--switch"),
        state_scope,
    )
}

fn parse_backend(mut text: &[u8]) -> u32 {
    while text.first().is_some_and(u8::is_ascii_whitespace) {
        text = &text[1..];
    }
    let negative = text.first() == Some(&b'-');
    if negative || text.first() == Some(&b'+') {
        text = &text[1..];
    }
    let (digits, radix) = if let Some(digits) = text
        .strip_prefix(b"0x")
        .or_else(|| text.strip_prefix(b"0X"))
    {
        (digits, 16)
    } else if text.first() == Some(&b'0') {
        (text, 8)
    } else {
        (text, 10)
    };
    let mut value = 0_u32;
    for &byte in digits {
        let Some(digit) = char::from(byte).to_digit(radix) else {
            break;
        };
        let Some(next) = value
            .checked_mul(radix)
            .and_then(|value| value.checked_add(digit))
        else {
            return u32::MAX;
        };
        value = next;
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}
