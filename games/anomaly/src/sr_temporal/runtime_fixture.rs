// SPDX-License-Identifier: MIT
//! Query the real Runtime DLL import from a consumer executable with no device.
use super::*;
use rsf_game_sys::{RSF_GAME_RENDER_SR, rsf_game_render_pass, rsf_result};
use std::mem::MaybeUninit;
use std::ptr;
use std::slice;

mod lifecycle;

use rsf_game_sys::{
    RSF_ERROR_ABI_MISMATCH as ABI_MISMATCH, RSF_ERROR_INVALID_ARGUMENT as INVALID_ARGUMENT,
};
const NOT_RUNNING: i32 = -4;
const TABLE_SIZE: usize = size_of::<rsf_d3d11_sr_api>();
type Query = unsafe extern "C" fn(*mut rsf_d3d11_sr_api) -> rsf_result;

enum ApiStatus {
    Bound,
    Missing,
    Refused,
}

unsafe fn rsf_anomaly_sr_query(
    query: rsf_get_d3d11_sr_api_fn,
    out: &mut rsf_d3d11_sr_api,
    reason: *mut *const c_char,
) -> ApiStatus {
    match query_api(query) {
        Ok((_, table)) => {
            *out = table;
            ApiStatus::Bound
        }
        Err(text) => {
            if !reason.is_null() {
                unsafe { reason.write(text.as_ptr()) };
            }
            if query.is_none() {
                ApiStatus::Missing
            } else {
                ApiStatus::Refused
            }
        }
    }
}

unsafe fn rsf_anomaly_sr_bind_table(
    driver: &Driver,
    table: &rsf_d3d11_sr_api,
    session: u64,
) -> bool {
    // The synthetic Runtime admits the requested provider (DLSS for Auto) and starts it in the fake.
    unsafe extern "C" fn admit(request: *mut Admission) -> i32 {
        let request = unsafe { &mut *request };
        request.offered_backends = request.backend_choices;
        request.selected_backend = if request.requested_backend == AUTO {
            DLSS
        } else {
            request.requested_backend
        };
        OK
    }
    let render = unsafe { &mut *driver.render.get() };
    let startup = Startup {
        start: lifecycle::start_backend,
        admit,
    };
    let result = validate_api(*table).map(|api| (api, startup));
    let bound = driver.bind(render, result, session);
    if let Err(reason) = result {
        render.say(&format!("Anomaly SR stage 5: {}", reason.to_string_lossy()));
    }
    bound
}

fn runtime() -> Raw {
    let module = crate::runtime_module::rsf_anomaly_runtime_module();
    assert!(!module.is_null());
    module
}

fn header() -> rsf_d3d11_sr_api {
    rsf_d3d11_sr_api {
        struct_size: TABLE_SIZE as u32,
        abi_version: RSF_D3D11_SR_API_VERSION,
        ..Default::default()
    }
}

fn query_contract(query: Query) {
    #[repr(C)]
    struct Storage {
        table: MaybeUninit<rsf_d3d11_sr_api>,
        tail: [u8; 16],
    }
    let mut storage = MaybeUninit::<Storage>::uninit();
    let buffer = storage.as_mut_ptr().cast::<u8>();
    let table = buffer.cast::<rsf_d3d11_sr_api>();
    unsafe {
        assert_eq!(query(null_mut()), INVALID_ARGUMENT);
        ptr::write_bytes(buffer, 0xcd, size_of::<Storage>());
        (&raw mut (*table).struct_size).write(TABLE_SIZE as u32 - 1);
        assert_eq!(query(table), INVALID_ARGUMENT);
        assert!(
            slice::from_raw_parts(buffer.add(4), size_of::<Storage>() - 4)
                .iter()
                .all(|&b| b == 0xcd)
        );

        (&raw mut (*table).struct_size).write(TABLE_SIZE as u32);
        (&raw mut (*table).abi_version).write(RSF_D3D11_SR_API_VERSION + 1);
        assert_eq!(query(table), ABI_MISMATCH);
        assert!(
            slice::from_raw_parts(buffer.add(8), size_of::<Storage>() - 8)
                .iter()
                .all(|&b| b == 0xcd)
        );
        (&raw mut (*table).abi_version).write(3);
        assert_eq!(query(table), ABI_MISMATCH);
        assert_eq!(
            (
                (&raw const (*table).abi_version).read(),
                (&raw const (*table).struct_size).read()
            ),
            (3, TABLE_SIZE as u32)
        );

        (&raw mut (*table).struct_size).write(size_of::<Storage>() as u32);
        (&raw mut (*table).abi_version).write(RSF_D3D11_SR_API_VERSION);
        assert_eq!(query(table), OK);
        let table = table.read();
        assert_eq!(table.struct_size as usize, TABLE_SIZE);
        assert_eq!(table.abi_version, RSF_D3D11_SR_API_VERSION);
        assert_eq!(table.pipeline_abi_version, RSF_DLSS_PIPELINE_ABI_VERSION);
        assert_eq!(RSF_DLSS_PIPELINE_ABI_VERSION, 5);
        assert!(
            slice::from_raw_parts(buffer.add(TABLE_SIZE), 16)
                .iter()
                .all(|&b| b == 0xcd)
        );
        assert!(validate_api(table).is_ok());
        let resolved: rsf_get_d3d11_sr_api_fn =
            crate::runtime_module::export(runtime(), c"rsf_get_d3d11_sr_api".to_bytes_with_nul());
        assert!(resolved.is_some());
    }
}

fn idle_contract(query: Query) {
    let mut api = header();
    unsafe {
        assert_eq!(query(&mut api), OK);
        let mut status = fresh_pipeline_status();
        status.struct_size -= 4;
        let read_status = api.status.unwrap();
        assert_eq!(read_status(&mut status), INVALID_ARGUMENT);
        status = fresh_pipeline_status();
        assert_eq!(read_status(&mut status), OK);
        assert_eq!((status.running, status.frames_evaluated), (0, 0));
        assert_eq!(api.stop.unwrap()(), NOT_RUNNING);
        assert!(api.output_texture.unwrap()().is_null());
        assert_eq!(api.reset.unwrap()(), OK);
        assert_eq!(api.request_dump.unwrap()(ptr::null()), INVALID_ARGUMENT);
        assert_eq!(api.request_dump.unwrap()(c"x".as_ptr()), NOT_RUNNING);
        assert_eq!(
            api.select_backend.unwrap()(XESS, null_mut(), null_mut()),
            NOT_RUNNING
        );
        assert_eq!(
            api.resize_output.unwrap()(1920, 1080, null_mut(), null_mut()),
            NOT_RUNNING
        );
        assert_eq!(
            api.resize_output.unwrap()(0, 1080, null_mut(), null_mut()),
            INVALID_ARGUMENT
        );
        assert_eq!(
            api.set_quality.unwrap()(5, null_mut(), null_mut()),
            INVALID_ARGUMENT
        );
        assert_eq!(api.set_preset.unwrap()(0), OK);

        let mut setup = rsf_dlss_pipeline_setup {
            struct_size: size_of::<rsf_dlss_pipeline_setup>() as u32,
            abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
            streamline_directory_utf8: c"Z:\\does-not-exist".as_ptr(),
            output_width: 64,
            output_height: 64,
            motion: motion_decode_params(),
            ..Default::default()
        };
        let mut fake = 0_i32;
        let fake = ptr::from_mut(&mut fake).cast();
        let start = api.start.unwrap();
        assert_eq!(start(null_mut(), &setup), INVALID_ARGUMENT);
        setup.struct_size -= 4;
        assert_eq!(start(fake, &setup), INVALID_ARGUMENT);
        setup.struct_size += 4;
        setup.abi_version = 4;
        assert_eq!(start(fake, &setup), ABI_MISMATCH);
        setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
        setup.motion.scale_x = 0.0;
        assert_eq!(start(fake, &setup), INVALID_ARGUMENT);
        status = fresh_pipeline_status();
        assert_eq!(read_status(&mut status), OK);
        assert_eq!(status.running, 0);

        let evaluate = api.evaluate.unwrap();
        // This public C POD uses integers, floats and nullable pointers. A zeroed
        // pass has no resources and is intentionally refused by the Runtime.
        let mut pass: rsf_game_render_pass = MaybeUninit::zeroed().assume_init();
        pass.struct_size = size_of::<rsf_game_render_pass>() as u32 - 8;
        assert_eq!(evaluate(null_mut(), &pass), INVALID_ARGUMENT);
        assert_eq!(evaluate(fake, ptr::null()), INVALID_ARGUMENT);
        assert_eq!(evaluate(fake, &pass), INVALID_ARGUMENT);
        pass.struct_size += 8;
        pass.role = RSF_GAME_RENDER_SR;
        assert_eq!(evaluate(fake, &pass), INVALID_ARGUMENT);
        assert_eq!(api.stop.unwrap()(), NOT_RUNNING);
    }
}

thread_local! {
    static TAMPERED: std::cell::Cell<rsf_d3d11_sr_api> = std::cell::Cell::new(header());
}

unsafe extern "C" fn mismatch(_: *mut rsf_d3d11_sr_api) -> rsf_result {
    ABI_MISMATCH
}

unsafe extern "C" fn tampered(out: *mut rsf_d3d11_sr_api) -> rsf_result {
    unsafe { out.write(TAMPERED.get()) };
    OK
}

fn driver() -> Box<Driver> {
    // The Rust fixture owns the same handle that the C++ driver facade owns.
    let driver = unsafe { Box::from_raw(rsf_anomaly_sr_create()) };
    let options = rsf_anomaly_sr_driver_config {
        stage: 5,
        initial: DEFAULT_SETTINGS,
        ..Default::default()
    };
    unsafe { rsf_anomaly_sr_configure(&driver, &options, None, null_mut()) };
    driver
}

fn adapter_binding(query: Query) {
    let mut real = header();
    assert_eq!(unsafe { query(&mut real) }, OK);
    let mut out = rsf_d3d11_sr_api::default();
    let mut reason = ptr::null();
    assert!(matches!(
        unsafe { rsf_anomaly_sr_query(None, &mut out, &mut reason) },
        ApiStatus::Missing
    ));
    assert!(!reason.is_null());
    assert!(matches!(
        unsafe { rsf_anomaly_sr_query(Some(query), &mut out, &mut reason) },
        ApiStatus::Bound
    ));
    assert_eq!(out.start.unwrap() as usize, real.start.unwrap() as usize);
    assert_eq!(
        out.evaluate.unwrap() as usize,
        real.evaluate.unwrap() as usize
    );

    assert!(matches!(
        unsafe { rsf_anomaly_sr_query(Some(mismatch), &mut out, &mut reason) },
        ApiStatus::Refused
    ));
    assert!(
        unsafe { CStr::from_ptr(reason) }
            .to_string_lossy()
            .contains("version")
    );
    let replacements = [
        rsf_d3d11_sr_api {
            struct_size: TABLE_SIZE as u32 - 8,
            ..real
        },
        rsf_d3d11_sr_api {
            abi_version: 99,
            ..real
        },
        rsf_d3d11_sr_api {
            pipeline_abi_version: 4,
            ..real
        },
        rsf_d3d11_sr_api {
            start: None,
            ..real
        },
        rsf_d3d11_sr_api {
            evaluate: None,
            ..real
        },
        rsf_d3d11_sr_api {
            reset: None,
            ..real
        },
        rsf_d3d11_sr_api { stop: None, ..real },
        rsf_d3d11_sr_api {
            status: None,
            ..real
        },
        rsf_d3d11_sr_api {
            select_backend: None,
            ..real
        },
        rsf_d3d11_sr_api {
            set_quality: None,
            ..real
        },
        rsf_d3d11_sr_api {
            set_preset: None,
            ..real
        },
        rsf_d3d11_sr_api {
            resize_output: None,
            ..real
        },
        rsf_d3d11_sr_api {
            output_texture: None,
            ..real
        },
        rsf_d3d11_sr_api {
            request_dump: None,
            ..real
        },
        rsf_d3d11_sr_api {
            set_sharpening: None,
            ..real
        },
    ];
    for table in replacements {
        TAMPERED.set(table);
        let mut untouched = rsf_d3d11_sr_api {
            struct_size: 77,
            ..Default::default()
        };
        assert!(matches!(
            unsafe { rsf_anomaly_sr_query(Some(tampered), &mut untouched, &mut reason) },
            ApiStatus::Refused
        ));
        assert_eq!(untouched.struct_size, 77);
        assert!(untouched.start.is_none());
    }

    let live = driver();
    assert!(unsafe { rsf_anomaly_sr_bind_runtime(&live, runtime()) });
    assert!(!rsf_anomaly_sr_running(&live));
    let mut report = rsf_overlay_sr_report::default();
    rsf_anomaly_sr_overlay_report(&live, &mut report);
    assert_eq!(report.effective_backend, 0);
    assert_eq!(
        report.struct_size as usize,
        size_of::<rsf_overlay_sr_report>()
    );
    assert_ne!(report.reason[0], 0);
    let mut input = PassInputs::default();
    assert!(!unsafe {
        rsf_anomaly_sr_evaluate(
            &live,
            null_mut(),
            &mut input,
            &ResetFacts::default(),
            null_mut(),
            null_mut(),
        )
    });
    assert_eq!(
        live.published().counters.refused[Refusal::NotRunning as usize],
        1
    );
    assert!(rsf_anomaly_sr_plan(&live, 1920, 1080).is_none());
    // A value that is no provider id, and a quality past Ultra Quality (5, offered since every
    // engine that renders reduced renders it), keep the current choice (Auto at the default quality).
    rsf_anomaly_sr_request(
        &live,
        &Settings {
            enabled: false,
            backend: 15,
            quality: 6,
            preset: 0,
        },
    );
    assert!(!rsf_anomaly_sr_enabled(&live));
    assert_eq!(live.published().status.requested.backend, AUTO);
    assert_eq!(live.published().status.requested.quality, 1);
    // FSR2 is one of the providers every game offers.
    rsf_anomaly_sr_request(
        &live,
        &Settings {
            enabled: false,
            backend: 2,
            quality: 1,
            preset: 0,
        },
    );
    assert_eq!(live.published().status.requested.backend, 2);

    let refused = driver();
    assert!(!unsafe {
        rsf_anomaly_sr_bind_table(&refused, &rsf_d3d11_sr_api { stop: None, ..real }, 1)
    });
    let state = refused.published();
    assert!(!state.status.bound && !state.status.reason_text().is_empty());
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_runtime_fixture(selection: *const c_char, query: Query) -> i32 {
    type Case = (&'static str, fn(Query));
    let cases: [Case; 6] = [
        ("query", query_contract),
        ("idle", idle_contract),
        ("binding", adapter_binding),
        ("lifecycle", |_| lifecycle::lifecycle()),
        ("start-refused", |_| lifecycle::refused_start()),
        ("announce", |_| lifecycle::announce()),
    ];
    let mut ran = 0;
    let mut failures = 0;
    for (name, run) in cases {
        if !selection.is_null()
            && unsafe { CStr::from_ptr(selection) }.to_bytes() != name.as_bytes()
        {
            continue;
        }
        failures += i32::from(std::panic::catch_unwind(|| run(query)).is_err());
        ran += 1;
    }
    if ran == 0 {
        println!("unknown case");
        return 2;
    }
    println!("{ran} case(s), {failures} failure(s)");
    i32::from(failures != 0)
}
