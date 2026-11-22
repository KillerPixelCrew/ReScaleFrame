// SPDX-License-Identifier: MIT
use super::json::{Json, rsf_census_json_string};
use super::rsf_census_json;
use super::rsf_census_sample;
use crate::sys::rsf_census_hook_desc;
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::io::Write;
use std::sync::{
    OnceLock,
    atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering::*},
};

const SITE_CAP: usize = 8192;
const SITE_PROBES: usize = 64;
const SITE_THREADS: usize = 6;
const FUNCTION_CAP: usize = 1024;
const QUEUE_CAP: usize = 128;
const HIST_CAP: usize = 66;
const D3D12_QUEUE: u8 = 1;

struct Settings {
    count: usize,
    cap: u64,
    interval: u64,
}
static SETTINGS: OnceLock<Settings> = OnceLock::new();
static CALLS: AtomicU64 = AtomicU64::new(0);
static PRESENT: AtomicU64 = AtomicU64::new(0);
static SITE_COUNT: AtomicU32 = AtomicU32::new(0);
static SITE_OVERFLOW: AtomicU32 = AtomicU32::new(0);

struct Range {
    first: AtomicU64,
    last: AtomicU64,
}
impl Range {
    const fn new() -> Self {
        Self {
            first: AtomicU64::new(0),
            last: AtomicU64::new(0),
        }
    }
    fn note(&self, present: u64) {
        let mark = present.wrapping_add(1);
        let _ = self.first.compare_exchange(0, mark, Relaxed, Relaxed);
        self.last.store(mark, Relaxed);
    }
    fn summary(&self, json: &mut Json<'_>) {
        json.number(
            Some(c"first_present"),
            self.first.load(Relaxed).saturating_sub(1),
        );
        json.number(
            Some(c"last_present"),
            self.last.load(Relaxed).saturating_sub(1),
        );
    }
}

struct Histogram {
    current: AtomicU32,
    buckets: [AtomicU64; HIST_CAP],
}
impl Histogram {
    const fn new() -> Self {
        Self {
            current: AtomicU32::new(0),
            buckets: [const { AtomicU64::new(0) }; HIST_CAP],
        }
    }
    fn note(&self) {
        self.current.fetch_add(1, Relaxed);
    }
    fn roll(&self) {
        let count = self.current.swap(0, Relaxed) as usize;
        if count != 0 {
            self.buckets[count.min(HIST_CAP - 1)].fetch_add(1, Relaxed);
        }
    }
    fn summary(&self, json: &mut Json<'_>) {
        json.open(Some(c"calls_per_present"), b'{');
        for (bucket, count) in self.buckets.iter().enumerate().skip(1) {
            let count = count.load(Relaxed);
            if count == 0 {
                continue;
            }
            if bucket == HIST_CAP - 1 {
                json.number(Some(c"65+"), count);
            } else {
                let mut text = [0u8; 8];
                let _ = write!(&mut text[..7], "{bucket}");
                json.number(CStr::from_bytes_until_nul(&text).ok(), count);
            }
        }
        json.close(b'}');
    }
}

struct Site {
    key: AtomicU64,
    calls: AtomicU64,
    range: Range,
    threads: [AtomicU32; SITE_THREADS],
    thread_count: AtomicU32,
}
static SITES: [Site; SITE_CAP] = [const {
    Site {
        key: AtomicU64::new(0),
        calls: AtomicU64::new(0),
        range: Range::new(),
        threads: [const { AtomicU32::new(0) }; SITE_THREADS],
        thread_count: AtomicU32::new(0),
    }
}; SITE_CAP];

struct Function {
    calls: AtomicU64,
    histogram: Histogram,
}
static FUNCTIONS: [Function; FUNCTION_CAP] = [const {
    Function {
        calls: AtomicU64::new(0),
        histogram: Histogram::new(),
    }
}; FUNCTION_CAP];

struct Queue {
    address: AtomicU64,
    kind: AtomicU8,
    descriptors: [AtomicU32; 3],
    calls: AtomicU64,
    lists: AtomicU64,
    range: Range,
    histogram: Histogram,
}
static QUEUES: [Queue; QUEUE_CAP] = [const {
    Queue {
        address: AtomicU64::new(0),
        kind: AtomicU8::new(0),
        descriptors: [const { AtomicU32::new(0) }; 3],
        calls: AtomicU64::new(0),
        lists: AtomicU64::new(0),
        range: Range::new(),
        histogram: Histogram::new(),
    }
}; QUEUE_CAP];

fn find_site(key: u64) -> Option<&'static Site> {
    let start = (key.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40) as usize;
    for step in 0..SITE_PROBES {
        let site = &SITES[(start + step) & (SITE_CAP - 1)];
        match site.key.load(Acquire) {
            current if current == key => return Some(site),
            0 => match site.key.compare_exchange(0, key, AcqRel, Acquire) {
                Ok(_) => {
                    SITE_COUNT.fetch_add(1, Relaxed);
                    return Some(site);
                }
                Err(current) if current == key => return Some(site),
                Err(_) => {}
            },
            _ => {}
        }
    }
    None
}

fn queue(kind: u8, address: u64) -> Option<&'static Queue> {
    if address == 0 {
        return None;
    }
    let start = (((address >> 4).wrapping_mul(2_654_435_761) >> 8) as usize) & (QUEUE_CAP - 1);
    for step in 0..QUEUE_CAP {
        let queue = &QUEUES[(start + step) & (QUEUE_CAP - 1)];
        let current = queue.address.load(Acquire);
        if current == address && queue.kind.load(Acquire) == kind {
            return Some(queue);
        }
        if current == 0
            && queue
                .address
                .compare_exchange(0, address, AcqRel, Acquire)
                .is_ok()
        {
            queue.kind.store(kind, Release);
            return Some(queue);
        }
    }
    None
}

pub(super) fn rsf_census_stats_init(entries: u16, cap: u32, interval: u32) -> bool {
    let count = usize::from(entries);
    count <= FUNCTION_CAP
        && SETTINGS
            .set(Settings {
                count,
                cap: cap.into(),
                interval: interval.into(),
            })
            .is_ok()
}

pub(super) fn rsf_census_stats_sample(
    entry: u16,
    module: u32,
    rva: u32,
    tid: u32,
    out: &mut rsf_census_sample,
) -> bool {
    let Some(cfg) = SETTINGS.get() else {
        return false;
    };
    if usize::from(entry) >= cfg.count {
        return false;
    }
    CALLS.fetch_add(1, Relaxed);
    let function = &FUNCTIONS[usize::from(entry)];
    out.ordinal = function.calls.fetch_add(1, Relaxed).wrapping_add(1);
    function.histogram.note();
    let key = (u64::from(entry) << 48) | (u64::from(module) << 32) | u64::from(rva);
    let Some(site) = find_site(if key == 0 { 1 } else { key }) else {
        SITE_OVERFLOW.fetch_add(1, Relaxed);
        return false;
    };
    let ordinal = site.calls.fetch_add(1, Relaxed);
    out.site_ordinal = ordinal.wrapping_add(1);
    site.range.note(PRESENT.load(Relaxed));
    let seen = site
        .threads
        .iter()
        .take(site.thread_count.load(Relaxed) as usize)
        .any(|previous| previous.load(Relaxed) == tid);
    if !seen {
        let slot = site.thread_count.fetch_add(1, Relaxed) as usize;
        if let Some(previous) = site.threads.get(slot) {
            previous.store(tid, Relaxed);
        }
    }
    ordinal < cfg.cap || (cfg.interval != 0 && (ordinal - cfg.cap).is_multiple_of(cfg.interval))
}

pub(super) fn rsf_census_stats_counts(
    calls: &mut u64,
    present: &mut u64,
    sites: &mut u32,
    overflow: &mut u32,
) {
    *calls = CALLS.load(Relaxed);
    *present = PRESENT.load(Relaxed);
    *sites = SITE_COUNT.load(Relaxed);
    *overflow = SITE_OVERFLOW.load(Relaxed);
}

fn roll_present() {
    let Some(cfg) = SETTINGS.get() else { return };
    for function in &FUNCTIONS[..cfg.count] {
        function.histogram.roll();
    }
    for queue in &QUEUES {
        if queue.address.load(Acquire) != 0 {
            queue.histogram.roll();
        }
    }
    PRESENT.fetch_add(1, Relaxed);
}

pub(super) fn rsf_census_stats_present_index() -> u64 {
    PRESENT.load(Relaxed)
}

thread_local! { static PRESENT_DEPTH: Cell<u32> = const { Cell::new(0) }; }

pub(super) fn rsf_census_present_enter(test: bool) -> bool {
    let outer = PRESENT_DEPTH
        .try_with(|depth| {
            let old = depth.get();
            depth.set(old.wrapping_add(1));
            old == 0
        })
        .unwrap_or(false);
    if outer && !test && super::session::rsf_census_active() {
        roll_present();
    }
    outer
}

pub(super) fn rsf_census_present_leave() {
    let _ = PRESENT_DEPTH.try_with(|depth| depth.set(depth.get().wrapping_sub(1)));
}

pub(super) fn rsf_census_stats_queue_describe(
    kind: u8,
    address: *const c_void,
    a: u32,
    b: u32,
    c: u32,
) {
    if let Some(queue) = queue(kind, address as u64) {
        for (field, value) in queue.descriptors.iter().zip([a, b, c]) {
            field.store(value, Relaxed);
        }
    }
}

pub(super) fn rsf_census_stats_queue_submit(kind: u8, address: *const c_void, lists: u32) -> bool {
    let Some(queue) = queue(kind, address as u64) else {
        return false;
    };
    let present = PRESENT.load(Relaxed);
    let first = queue.calls.fetch_add(1, Relaxed) == 0;
    queue.lists.fetch_add(lists.into(), Relaxed);
    queue.histogram.note();
    queue.range.note(present);
    first
}

trait StatsJson {
    unsafe fn raw_text(&mut self, key: &CStr, text: *const c_char);
}

impl StatsJson for Json<'_> {
    unsafe fn raw_text(&mut self, key: &CStr, text: *const c_char) {
        unsafe { rsf_census_json_string(self.0, key.as_ptr(), text) };
    }
}

fn function_name(desc: &rsf_census_hook_desc) -> *const c_char {
    if desc.name.is_null() {
        desc.id
    } else {
        desc.name
    }
}

pub(super) unsafe fn rsf_census_stats_sites(output: &mut rsf_census_json) {
    let mut json = Json(output);
    json.open(Some(c"sites"), b'[');
    for site in &SITES {
        let key = site.key.load(Acquire);
        if key == 0 {
            continue;
        }
        let entry = (key >> 48) as usize;
        let module = (key >> 32) as u16;
        let desc = unsafe { &*super::platform().entries.add(entry) };
        json.open(None, b'{');
        unsafe {
            json.raw_text(c"api", desc.api);
            json.raw_text(c"function", function_name(desc));
            json.raw_text(
                c"module",
                super::identity::rsf_census_module_name(module.into()),
            );
        }
        json.hex(c"rva", u64::from(key as u32));
        json.open(Some(c"threads"), b'[');
        let count = site.thread_count.load(Relaxed) as usize;
        for tid in site.threads.iter().take(count) {
            json.number(None, tid.load(Relaxed));
        }
        json.close(b']');
        if count > SITE_THREADS {
            json.number(
                Some(c"threads_beyond_listed"),
                (count - SITE_THREADS) as u64,
            );
        }
        json.number(Some(c"count"), site.calls.load(Relaxed));
        site.range.summary(&mut json);
        json.close(b'}');
    }
    json.close(b']');
}

pub(super) unsafe fn rsf_census_stats_histograms(
    output: &mut rsf_census_json,
    entries: *const rsf_census_hook_desc,
) {
    let mut json = Json(output);
    json.open(Some(c"histograms"), b'[');
    if let Some(cfg) = SETTINGS.get() {
        for (id, function) in FUNCTIONS[..cfg.count].iter().enumerate() {
            let calls = function.calls.load(Relaxed);
            if calls == 0 {
                continue;
            }
            let desc = unsafe { &*entries.add(id) };
            json.open(None, b'{');
            unsafe { json.raw_text(c"function", function_name(desc)) };
            json.number(Some(c"calls"), calls);
            function.histogram.summary(&mut json);
            json.close(b'}');
        }
    }
    json.close(b']');
}

pub(super) fn rsf_census_stats_queues(output: &mut rsf_census_json) {
    let mut json = Json(output);
    json.open(Some(c"queues"), b'[');
    for queue in &QUEUES {
        let address = queue.address.load(Acquire);
        let kind = queue.kind.load(Acquire);
        if address == 0 || kind == 0 {
            continue;
        }
        let d3d12 = kind == D3D12_QUEUE;
        json.open(None, b'{');
        unsafe { json.raw_text(c"kind", if d3d12 { c"d3d12" } else { c"vulkan" }.as_ptr()) };
        json.hex(c"queue", address);
        let [a, b, c] = queue
            .descriptors
            .each_ref()
            .map(|value| value.load(Relaxed));
        if d3d12 {
            json.number(Some(c"type"), a);
            json.signed(c"priority", b as i32);
            json.hex(c"flags", c.into());
        } else {
            json.number(Some(c"family"), a);
            json.number(Some(c"index"), b);
        }
        json.number(Some(c"submit_calls"), queue.calls.load(Relaxed));
        json.number(Some(c"lists_or_batches"), queue.lists.load(Relaxed));
        queue.range.summary(&mut json);
        queue.histogram.summary(&mut json);
        json.close(b'}');
    }
    json.close(b']');
}
