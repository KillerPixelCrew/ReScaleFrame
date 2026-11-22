// SPDX-License-Identifier: MIT
use crate::native_geometry_abi::{Coverage, PASSES, Stats, TYPES, VERDICTS};
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

const PASS_NAMES: [&CStr; PASSES] = [c"main", c"shadow", c"other"];
const TYPE_NAMES: [&CStr; TYPES] = [
    c"static",
    c"progressive",
    c"skeleton_st",
    c"skeleton_pm",
    c"tree_st",
    c"tree_pm",
];
const VERDICT_NAMES: [&CStr; VERDICTS] = [
    c"identified",
    c"ignored_shadow",
    c"ignored_static_world",
    c"unidentified_dynamic",
    c"unidentified_deformed",
    c"unidentified_ambiguous",
    c"unidentified_unreadable",
    c"unidentified_nocapture",
    c"identified_foliage",
];

pub(super) struct Counter(AtomicU64);

impl Counter {
    const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub(super) fn increment(&self) {
        self.0.fetch_add(1, Relaxed);
    }

    fn read(&self) -> u64 {
        self.0.load(Relaxed)
    }
}

pub(super) struct Counters {
    pub models_registered: Counter,
    pub models_retired: Counter,
    pub draws_identified: Counter,
    pub draws_unidentified: Counter,
    pub capture_faults: Counter,
    pub draws: [[[Counter; VERDICTS]; TYPES]; PASSES],
    pub flush_total: [Counter; PASSES],
    pub flush_in_visual: [Counter; PASSES],
    pub captured: Counter,
    pub capture_stale: Counter,
    pub capture_no_world: Counter,
    pub capture_no_palette: Counter,
    pub capture_no_context: Counter,
    pub sunk: Counter,
    pub detail_renders: [Counter; PASSES],
    pub flush_in_detail: [Counter; PASSES],
    pub views_main: Counter,
    pub views_scope: Counter,
    pub hud_scopes: Counter,
    pub near_windows: Counter,
}

pub(super) static COUNTERS: Counters = Counters {
    models_registered: Counter::new(),
    models_retired: Counter::new(),
    draws_identified: Counter::new(),
    draws_unidentified: Counter::new(),
    capture_faults: Counter::new(),
    draws: [const { [const { [const { Counter::new() }; VERDICTS] }; TYPES] }; PASSES],
    flush_total: [const { Counter::new() }; PASSES],
    flush_in_visual: [const { Counter::new() }; PASSES],
    captured: Counter::new(),
    capture_stale: Counter::new(),
    capture_no_world: Counter::new(),
    capture_no_palette: Counter::new(),
    capture_no_context: Counter::new(),
    sunk: Counter::new(),
    detail_renders: [const { Counter::new() }; PASSES],
    flush_in_detail: [const { Counter::new() }; PASSES],
    views_main: Counter::new(),
    views_scope: Counter::new(),
    hud_scopes: Counter::new(),
    near_windows: Counter::new(),
};

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_geometry_stats_read(out: *mut Stats) {
    let c = &COUNTERS;
    let stats = Stats {
        models_registered: c.models_registered.read(),
        models_retired: c.models_retired.read(),
        draws_identified: c.draws_identified.read(),
        draws_unidentified: c.draws_unidentified.read(),
        capture_faults: c.capture_faults.read(),
    };
    unsafe { out.write(stats) };
}

fn counts<const N: usize>(counters: &[Counter; N]) -> [u64; N] {
    std::array::from_fn(|i| counters[i].read())
}

fn snapshot() -> Coverage {
    let c = &COUNTERS;
    Coverage {
        draws: std::array::from_fn(|pass| std::array::from_fn(|ty| counts(&c.draws[pass][ty]))),
        flush_total: counts(&c.flush_total),
        flush_in_visual: counts(&c.flush_in_visual),
        captured: c.captured.read(),
        capture_stale: c.capture_stale.read(),
        capture_no_world: c.capture_no_world.read(),
        capture_no_palette: c.capture_no_palette.read(),
        capture_no_context: c.capture_no_context.read(),
        sunk: c.sunk.read(),
        detail_renders: counts(&c.detail_renders),
        flush_in_detail: counts(&c.flush_in_detail),
        views_main: c.views_main.read(),
        views_scope: c.views_scope.read(),
        hud_scopes: c.hud_scopes.read(),
        near_windows: c.near_windows.read(),
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_coverage_read(out: *mut Coverage) {
    unsafe { out.write(snapshot()) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_pass_name(pass: u8) -> *const c_char {
    PASS_NAMES.get(pass as usize).unwrap_or(&c"other").as_ptr()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_type_name(ty: u8) -> *const c_char {
    TYPE_NAMES.get(ty as usize).unwrap_or(&c"unknown").as_ptr()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_verdict_name(verdict: u8) -> *const c_char {
    VERDICT_NAMES
        .get(verdict as usize)
        .unwrap_or(&c"unknown")
        .as_ptr()
}

type Emit = unsafe extern "C" fn(*mut c_void, *const c_char);

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_geometry_report(emit: Option<Emit>, user: *mut c_void) {
    let Some(emit) = emit else { return };
    let line = |message: String| {
        if let Ok(message) = CString::new(message) {
            unsafe { emit(user, message.as_ptr()) };
        }
    };
    let c = snapshot();
    let mut verdicts = [0u64; VERDICTS];
    let mut passes = [0u64; PASSES];
    for (pass, rows) in c.draws.iter().enumerate() {
        for row in rows {
            for (verdict, &count) in row.iter().enumerate() {
                verdicts[verdict] = verdicts[verdict].wrapping_add(count);
                passes[pass] = passes[pass].wrapping_add(count);
            }
        }
    }
    let [main, shadow, other] = passes;
    let [
        identified,
        ignored_shadow,
        ignored_static,
        dynamic,
        deformed,
        ambiguous,
        unreadable,
        nocapture,
        foliage,
    ] = verdicts;
    line(format!(
        "Anomaly geometry coverage render-entries main={main} shadow={shadow} other={other} | identified={} (foliage={foliage}) ignored_shadow={ignored_shadow} \
         ignored_static_world={ignored_static} | unidentified dynamic={dynamic} deformed={deformed} ambiguous={ambiguous} unreadable={unreadable} nocapture={nocapture}",
        identified.wrapping_add(foliage)
    ));
    for (pass, rows) in PASS_NAMES.iter().zip(c.draws) {
        for (ty, row) in TYPE_NAMES.iter().zip(rows) {
            if row.iter().all(|&count| count == 0) {
                continue;
            }
            let [
                identified,
                shadow,
                static_world,
                dynamic,
                deformed,
                ambiguous,
                unreadable,
                nocapture,
                foliage,
            ] = row;
            let other = ambiguous.wrapping_add(unreadable).wrapping_add(nocapture);
            line(format!(
                "Anomaly geometry coverage pass={} type={} identified={identified} ignored_shadow={shadow} ignored_static_world={static_world} \
                 unidentified_dynamic={dynamic} unidentified_deformed={deformed} unidentified_other={other} identified_foliage={foliage}",
                pass.to_string_lossy(),
                ty.to_string_lossy(),
            ));
        }
    }
    line(format!(
        "Anomaly geometry draw-time flush main={}(in_visual={}) shadow={}(in_visual={}) other={}(in_visual={}) \
         | captured={} sunk={} stale={} no_world={} no_palette={} no_context={} \
         | views main={} scope={} hud_scopes={} near_windows={} \
         | detail renders main={} shadow={} other={} flush main={} shadow={} other={}",
        c.flush_total[0],
        c.flush_in_visual[0],
        c.flush_total[1],
        c.flush_in_visual[1],
        c.flush_total[2],
        c.flush_in_visual[2],
        c.captured,
        c.sunk,
        c.capture_stale,
        c.capture_no_world,
        c.capture_no_palette,
        c.capture_no_context,
        c.views_main,
        c.views_scope,
        c.hud_scopes,
        c.near_windows,
        c.detail_renders[0],
        c.detail_renders[1],
        c.detail_renders[2],
        c.flush_in_detail[0],
        c.flush_in_detail[1],
        c.flush_in_detail[2],
    ));
}
