// SPDX-License-Identifier: MIT
//! CPU event sequencing around the typed native continuations.

use crate::cpu_hooks::{self, Services, active};
use crate::cpu_native::*;
use crate::cpu_policy::*;
use crate::cpu_sources::*;
use crate::platform;
use crate::render_native::{log_host, rsf_skyrim_call_render_pass, thread};
use crate::sys::*;
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use windows_sys::Win32::System::Performance::QueryPerformanceCounter;

const SUBMIT_RETURN: usize = 0x658b14;
const JOIN_RETURN: usize = 0x659334;
const NAMED_POLL_RETURN: usize = 0xebc248;

unsafe extern "C" {
    fn rsf_skyrim_guarded_read(address: usize, out: *mut std::ffi::c_void, size: usize) -> bool;
    fn rsf_skyrim_query_menus(
        open: usize,
        frozen: usize,
        ui: usize,
        loading: usize,
        main: usize,
        facts: &mut rsf_skyrim_menu_observation,
    ) -> bool;
    fn rsf_skyrim_cpu_event_guarded(host: &rsf_game_host_services, event: &rsf_game_cpu_event);
}

struct Frame {
    services: Services,
    source: u64,
    render: RenderState,
    input: FrameInput,
    jobs: JobTicket,
    main: usize,
    reservation: Reservation,
    entry_readable: bool,
    queued_move: bool,
    refusal: u32,
}

struct Poll {
    scope: PollScope,
    previous: Option<NonNull<PollScope>>,
    services: Option<Services>,
    reserved: u64,
    begin: u64,
}

enum Receipt {
    None,
    Frame(Frame),
    Poll(Poll),
    Render(Option<(NonNull<Frame>, RenderReceipt)>),
    World(bool),
    Dispatch {
        previous: usize,
        services: Option<Services>,
        event: rsf_game_cpu_event,
    },
    Controls,
    Jobs {
        before: ManagerState,
        ticket: JobTicket,
        owns_wait: bool,
    },
    NamedPoll(JobTicket),
    Consumer {
        stamp: Stamp,
        mapped: bool,
    },
}

const _: () = {
    assert!(size_of::<Receipt>() <= RSF_SKYRIM_CPU_STORAGE_WORDS as usize * size_of::<u64>());
    assert!(align_of::<Receipt>() <= align_of::<u64>());
};

// Pointers refer to receipts lent by an outer native detour on this thread. Changes to
// TLS and the source proof are paired in finish, including on native unwind.
thread_local! {
    static FRAME: Cell<Option<NonNull<Frame>>> = const { Cell::new(None) };
    static POLL: Cell<Option<NonNull<PollScope>>> = const { Cell::new(None) };
    static NAMED_POLL: Cell<JobTicket> = const { Cell::new(JobTicket::EMPTY) };
    static DISPATCHED_LIST: Cell<usize> = const { Cell::new(0) };
}

static POLL_REPORTS: AtomicU32 = AtomicU32::new(0);
static PROSPECTIVE_REPORTS: AtomicU32 = AtomicU32::new(0);
static RENDER_REPORTS: AtomicU32 = AtomicU32::new(0);

fn base() -> usize {
    platform::get().map_or(0, platform::Platform::main_module)
}

fn from_site(caller: usize, rva: usize) -> bool {
    let image = base();
    image != 0 && image.checked_add(rva) == Some(caller)
}

fn qpc() -> u64 {
    let mut time = 0;
    unsafe { QueryPerformanceCounter(&mut time) };
    time as u64
}

fn log(host: &rsf_game_host_services, message: String) {
    if host.log.is_some()
        && let Ok(message) = CString::new(message)
    {
        unsafe { log_host(host, message.as_ptr()) };
    }
}

fn frame_event(frame: NonNull<Frame>, stage: u32) {
    let (host, event) = {
        let frame = unsafe { frame.as_ref() };
        let host = frame.services.host;
        if host.cpu_event.is_none() {
            return;
        }
        let mut event = unsafe { std::mem::zeroed::<rsf_game_cpu_event>() };
        rsf_skyrim_cpu_frame_event(
            &mut event,
            &frame.render,
            stage,
            host.session_id,
            frame.source,
            qpc(),
            frame.services.frequency,
        );
        (host, event)
    };
    unsafe { rsf_skyrim_cpu_event_guarded(&host, &event) };
}

#[allow(clippy::too_many_arguments)]
fn source_event(
    services: Services,
    stage: u32,
    source: u64,
    predecessor: u64,
    generation: u64,
    flags: u32,
    kinds: u32,
    reason: u32,
    poll_begin: u64,
) {
    let host = services.host;
    if host.cpu_event.is_none() || source == 0 {
        return;
    }
    let mut event = unsafe { std::mem::zeroed::<rsf_game_cpu_source_event>() };
    rsf_skyrim_cpu_source_event(
        &mut event,
        stage,
        host.session_id,
        source,
        qpc(),
        services.frequency,
        predecessor,
        generation,
        flags,
        kinds,
        reason,
        poll_begin,
    );
    unsafe { rsf_skyrim_cpu_event_guarded(&host, &event.header) };
}

fn abort(services: Services, source: u64, reason: u32) {
    source_event(
        services,
        RSF_GAME_CPU_SOURCE_ABORT,
        source,
        0,
        0,
        0,
        0,
        reason,
        0,
    );
}

fn release(source: u64) -> Reservation {
    let mut released = Reservation::EMPTY;
    rsf_skyrim_sources_release(&mut released, source);
    released
}

fn job() -> JobTicket {
    let mut ticket = JobTicket::EMPTY;
    rsf_skyrim_jobs_snapshot(&mut ticket);
    ticket
}

fn stamp() -> Stamp {
    let mut stamp = Stamp::default();
    rsf_skyrim_epochs_snapshot(&mut stamp);
    stamp
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_tick_admissible() -> bool {
    active() && FRAME.get().is_none()
}

fn begin_frame(main: usize) -> Receipt {
    if !active() || FRAME.get().is_some() {
        return Receipt::None;
    }
    let Some(services) = cpu_hooks::services().filter(|_| active()) else {
        return Receipt::None;
    };
    // Only a sampled late poll whose native job joined normally can be adopted.
    let mut reservation = Reservation::EMPTY;
    rsf_skyrim_sources_adopt(
        &mut reservation,
        services.host.session_id,
        rsf_skyrim_jobs_retired(),
    );
    let mut queued_move = false;
    let (source, entry_readable) = if reservation.source != 0 {
        (
            reservation.source,
            rsf_skyrim_cpu_queued_move(&mut queued_move),
        )
    } else {
        abort(
            services,
            rsf_skyrim_sources_withdraw_pending(0),
            RSF_GAME_CPU_ABORT_STATE_CHANGED,
        );
        (rsf_skyrim_cpu_next_source(), false)
    };
    Receipt::Frame(Frame {
        services,
        source,
        render: RenderState::default(),
        input: FrameInput::default(),
        jobs: JobTicket::EMPTY,
        main,
        reservation,
        entry_readable,
        queued_move,
        refusal: 0,
    })
}

fn finish_frame(frame: NonNull<Frame>, returned: bool) {
    let (source, services, facts) = unsafe {
        let frame = frame.as_ref();
        (frame.source, frame.services, frame.reservation)
    };
    if facts.source != 0 {
        let owned = release(source);
        if owned.phase == Phase::Bound {
            rsf_skyrim_worker_release();
        }
        if !returned {
            abort(services, owned.source, RSF_GAME_CPU_ABORT_NOT_CONSUMED);
        } else if services.host.log.is_some() && PROSPECTIVE_REPORTS.fetch_add(1, Relaxed) < 16 {
            let mut worker = WorkerCounts::default();
            rsf_skyrim_worker_counts(&mut worker);
            let message = {
                let frame = unsafe { frame.as_ref() };
                format!(
                    "Skyrim prospective source={source} predecessor={} poll_generation={} job_generation={} bound={} refusal={} kinds=0x{:x} main_player_update={} main_camera={} main_player_camera={} main_movement={} main_generation_match={} main_ambiguous={} worker_movement={} worker_player_update={} worker_camera={} mismatched={}; input timeline ownership diagnostics",
                    facts.predecessor,
                    facts.poll_generation,
                    facts.job_generation,
                    u8::from(owned.phase == Phase::Bound),
                    frame.refusal,
                    facts.kinds,
                    frame.input.player_update_calls,
                    frame.input.camera_updates,
                    frame.input.player_camera_updates,
                    frame.input.movement_calls,
                    u8::from(frame.input.consumed.generation == facts.poll_generation),
                    u8::from(frame.input.ambiguous),
                    worker.movement,
                    worker.player_update,
                    worker.camera,
                    worker.mismatched,
                )
            };
            log(&services.host, message);
        }
    }
    if returned {
        frame_event(frame, RSF_GAME_CPU_FRAME_END);
    }
    FRAME.set(None);
}

fn begin_submit(caller: usize) -> Receipt {
    let mut before = ManagerState::default();
    let mut ticket = JobTicket::EMPTY;
    if active() {
        if let Some(mut frame) = FRAME.get().filter(|frame| {
            from_site(caller, SUBMIT_RETURN)
                && rsf_skyrim_cpu_manager_state(&mut before)
                && before.main == unsafe { frame.as_ref().main }
                && before.requested == 0
                && before.running == 0
        }) {
            let (services, source, adopted) = unsafe {
                let frame = frame.as_ref();
                (frame.services, frame.source, frame.reservation.source != 0)
            };
            if adopted {
                let mut free_camera = false;
                let camera_readable = rsf_skyrim_cpu_free_camera(&mut free_camera);
                // The normal-simulation branch has passed the pause/freeze decision.
                // Refuse a bind if a consumer ran earlier or a queued-move/free-camera
                // path can bypass the verified PlayerCharacter update envelope.
                let refusal = {
                    let frame = unsafe { frame.as_mut() };
                    let input = &frame.input;
                    frame.refusal = rsf_skyrim_bind_refusal(
                        frame.entry_readable && camera_readable,
                        frame.queued_move,
                        input.player_update_calls != 0
                            || input.movement_calls != 0
                            || input.camera_updates != 0
                            || input.player_camera_updates != 0,
                        free_camera,
                    );
                    frame.refusal
                };
                if refusal != 0 {
                    abort(
                        services,
                        release(source).source,
                        RSF_GAME_CPU_ABORT_NOT_CONSUMED,
                    );
                } else if rsf_skyrim_sources_bind(source, stamp().generation) {
                    rsf_skyrim_worker_bind(source);
                    source_event(services, RSF_GAME_CPU_SOURCE_BIND, source, 0, 0, 0, 0, 0, 0);
                } else {
                    abort(
                        services,
                        release(source).source,
                        RSF_GAME_CPU_ABORT_STATE_CHANGED,
                    );
                }
            }
            frame_event(frame, RSF_GAME_CPU_SIMULATION_BEGIN);
            // The original wakes the worker after setting both mode1 fields.
            rsf_skyrim_jobs_publish(
                &mut ticket,
                before.manager,
                source,
                services.host.session_id,
                true,
            );
            unsafe { frame.as_mut().jobs = ticket };
        } else if FRAME.get().is_some() || job().generation != 0 {
            rsf_skyrim_jobs_invalidate();
        }
    }
    Receipt::Jobs {
        before,
        ticket,
        owns_wait: false,
    }
}

fn begin_join(caller: usize) -> Receipt {
    let mut before = ManagerState::default();
    let mut ticket = JobTicket::EMPTY;
    let owns_wait = FRAME.get().is_some_and(|frame| {
        let frame = unsafe { frame.as_ref() };
        ticket = frame.jobs;
        from_site(caller, JOIN_RETURN)
            && rsf_skyrim_cpu_manager_state(&mut before)
            && before.main == frame.main
            && before.manager == ticket.manager
            && before.requested <= 1
            && before.running == 1
            && rsf_skyrim_jobs_matches(&ticket)
    });
    Receipt::Jobs {
        before,
        ticket,
        owns_wait,
    }
}

fn finish_jobs(
    index: rsf_skyrim_cpu_hook,
    before: &ManagerState,
    ticket: &JobTicket,
    owns_wait: bool,
    result: Option<usize>,
) {
    let mut after = ManagerState::default();
    if index == RSF_SKYRIM_CPU_SUBMIT {
        if ticket.generation != 0
            && (result.is_none_or(|r| r == 0)
                || !rsf_skyrim_cpu_manager_state(&mut after)
                || after.manager != before.manager
                || after.main != before.main
                || after.requested != 1
                || after.running != 1)
        {
            rsf_skyrim_jobs_invalidate();
        }
    } else if owns_wait {
        // WAIT_OBJECT_0
        if result == Some(0)
            && rsf_skyrim_cpu_manager_state(&mut after)
            && after.manager == before.manager
            && after.main == before.main
            && after.requested == 0
            && after.running == 0
        {
            rsf_skyrim_jobs_retire(ticket);
        } else {
            rsf_skyrim_jobs_invalidate();
        }
    } else if active() && job().generation != 0 {
        rsf_skyrim_jobs_invalidate();
    }
}

fn begin_named_poll(caller: usize) -> Receipt {
    let previous = NAMED_POLL.replace(JobTicket::EMPTY);
    let mut native = ManagerState::default();
    let ticket = job();
    if active()
        && from_site(caller, NAMED_POLL_RETURN)
        && previous.generation == 0
        && rsf_skyrim_cpu_manager_state(&mut native)
        && native.manager == ticket.manager
        && native.requested <= 1
        && native.running == 1
    {
        NAMED_POLL.set(ticket);
    }
    Receipt::NamedPoll(previous)
}

fn prepare_poll(mut poll: NonNull<Poll>) {
    let (services, provenance, scheduled) = {
        let poll = unsafe { poll.as_mut() };
        POLL.set(Some(NonNull::from(&mut poll.scope)));
        let mut provenance = PollProvenance::default();
        let running = active();
        let scheduled = running
            && rsf_skyrim_poll_provenance(&mut provenance, &poll.scope, &NAMED_POLL.get())
            && active();
        poll.services = running.then(cpu_hooks::services).flatten();
        (poll.services, provenance, scheduled)
    };
    let mut reserved = 0;
    if let Some(services) = services {
        if POLL_REPORTS.fetch_add(1, Relaxed) < 16 && services.host.log.is_some() {
            log(
                &services.host,
                format!(
                    "Skyrim native poll thread={} scoped_source={} native_job_generation={} submitting_source={} poll_generation={}; submitting source is not input consumption",
                    thread(),
                    render_source(),
                    provenance.job_generation,
                    provenance.submitting_source,
                    provenance.poll_generation,
                ),
            );
        }
        if scheduled {
            let source = rsf_skyrim_cpu_next_source();
            let mut receipt = Reserve {
                reserved: Reservation::EMPTY,
                withdrawn: 0,
                overlap: false,
            };
            rsf_skyrim_sources_reserve(
                &mut receipt,
                source,
                provenance.submitting_source,
                provenance.session,
                provenance.job_generation,
                provenance.poll_generation,
            );
            abort(
                services,
                receipt.withdrawn,
                if receipt.overlap {
                    RSF_GAME_CPU_ABORT_OVERLAP
                } else {
                    RSF_GAME_CPU_ABORT_INTERVENING_POLL
                },
            );
            if receipt.reserved.source != 0 {
                source_event(
                    services,
                    RSF_GAME_CPU_SOURCE_RESERVE,
                    source,
                    provenance.submitting_source,
                    provenance.poll_generation,
                    RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR,
                    0,
                    0,
                    0,
                );
                // The synchronous callback may overlap withdrawal. Reissue the
                // terminal fact if withdrawal won before reservation publication.
                if rsf_skyrim_sources_still_reserved(source) {
                    reserved = source;
                } else {
                    abort(services, source, RSF_GAME_CPU_ABORT_STATE_CHANGED);
                }
            }
        } else {
            abort(
                services,
                rsf_skyrim_sources_withdraw_pending(0),
                RSF_GAME_CPU_ABORT_INTERVENING_POLL,
            );
            rsf_skyrim_worker_intervening_poll();
        }
    }
    if let Some(mut frame) = FRAME.get()
        && !scheduled
        && rsf_skyrim_cpu_sample_tick(&mut unsafe { frame.as_mut() }.render)
    {
        frame_event(frame, RSF_GAME_CPU_INPUT_SAMPLE);
    }
    // Sample after the synchronous reserve callback and immediately before the original.
    unsafe {
        poll.as_mut().reserved = reserved;
        if reserved != 0 {
            poll.as_mut().begin = qpc();
        }
    }
}

fn finish_poll(poll: &mut Poll, returned: bool) {
    POLL.set(poll.previous);
    rsf_skyrim_poll_end(&poll.scope, !returned || poll.previous.is_some());
    let Some(services) = poll.services.filter(|_| poll.reserved != 0) else {
        return;
    };
    let source = poll.reserved;
    match rsf_skyrim_poll_sample_status(&poll.scope, source) {
        PollSample::NotProduced => abort(services, source, RSF_GAME_CPU_ABORT_NOT_PRODUCED),
        PollSample::Ready if returned => {
            source_event(
                services,
                RSF_GAME_CPU_SOURCE_SAMPLED,
                source,
                0,
                poll.scope.ticket.generation,
                RSF_GAME_CPU_SOURCE_PRODUCED,
                poll.scope.kinds,
                0,
                poll.begin,
            );
            if !rsf_skyrim_sources_sample(source, true, poll.scope.kinds) {
                abort(
                    services,
                    rsf_skyrim_sources_withdraw_pending(source),
                    RSF_GAME_CPU_ABORT_STATE_CHANGED,
                );
            }
        }
        PollSample::None | PollSample::Ready => {}
    }
}

fn begin_dispatch(manager: usize, list: usize) -> Receipt {
    let mut event = unsafe { std::mem::zeroed::<rsf_game_cpu_event>() };
    let services = if active() && rsf_skyrim_cpu_is_input_manager(manager) {
        let kinds = rsf_skyrim_cpu_input_kinds(list, rsf_skyrim_guarded_read);
        cpu_hooks::services()
            .filter(|_| kinds != 0)
            .inspect(|services| {
                rsf_skyrim_cpu_input_event(
                    &mut event,
                    services.host.session_id,
                    kinds,
                    qpc(),
                    services.frequency,
                );
            })
    } else {
        None
    };
    let selected = POLL
        .get()
        .filter(|scope| unsafe { scope.as_ref().manager } == manager);
    let previous = DISPATCHED_LIST.replace(selected.map_or(0, |_| list));
    if let Some(mut scope) = selected.filter(|_| list != 0) {
        rsf_skyrim_poll_record_kinds(unsafe { scope.as_mut() }, event.input_kind);
    }
    Receipt::Dispatch {
        previous,
        services,
        event,
    }
}

fn matching_poll(list: usize) -> Option<NonNull<PollScope>> {
    POLL.get()
        .filter(|_| list != 0 && list == DISPATCHED_LIST.get())
}

fn finish_consumer(owner: usize, stamp: &Stamp, mapped: bool, consumer: Consumer) {
    if !active() {
        return;
    }
    let mapped = match consumer {
        Consumer::Movement | Consumer::PlayerUpdate => {
            if !mapped || !rsf_skyrim_cpu_is_player(owner) {
                return;
            }
            true
        }
        Consumer::PlayerCamera if !mapped => return,
        _ => mapped,
    };
    if let Some(mut frame) = FRAME.get() {
        rsf_skyrim_cpu_record_consumer(
            &mut unsafe { frame.as_mut() }.input,
            stamp,
            consumer,
            mapped,
        );
    } else if mapped {
        rsf_skyrim_worker_record_consumer(stamp, consumer);
    }
}

pub(crate) fn render_source() -> u64 {
    FRAME
        .get()
        .map_or(0, |frame| unsafe { frame.as_ref().source })
}

pub(crate) fn render_identity(pass: &mut rsf_game_render_pass) -> bool {
    FRAME.get().is_some_and(|frame| {
        let frame = unsafe { frame.as_ref() };
        rsf_skyrim_cpu_render_identity(&frame.render, pass, frame.source, active())
    })
}

pub(crate) fn render_eligible() -> bool {
    FRAME.get().is_some_and(|frame| {
        let render = &unsafe { frame.as_ref() }.render;
        rsf_skyrim_rendered_source_eligible(
            active() && render.scope != 0,
            render.primary_world_entries,
            render.screen,
        )
    })
}

pub(crate) fn render_diagnostic(stage: &CStr, pass: &rsf_game_render_pass) {
    let Some(frame) = FRAME.get() else {
        return;
    };
    let host = unsafe { frame.as_ref().services.host };
    let source = render_source();
    if host.log.is_none()
        || (source > 8 && !source.is_multiple_of(128))
        || RENDER_REPORTS.fetch_add(1, Relaxed) >= 64
    {
        return;
    }
    let mut owned = Reservation::EMPTY;
    rsf_skyrim_sources_owned(&mut owned);
    let message = {
        let frame = unsafe { frame.as_ref() };
        let render = &frame.render;
        format!(
            "Skyrim render producer stage={} source={} cpu_source={source} native={} scope={} cpu_scope={} flags=0x{:x} screen={} cpu_screen={} active={} world_entries={} eligible={} render_entered={} simulation_ended={} primary_count={} reservation={} owned_source={} owned_phase={} bound={} refusal={} poll={} job={} entry_readable={} queued_move={} input_ambiguous={}; diagnostic only",
            stage.to_string_lossy(),
            pass.source_frame_id,
            pass.native_frame,
            pass.scope_id,
            render.scope,
            pass.flags,
            pass.screen,
            render.screen,
            u8::from(active()),
            render.primary_world_entries,
            u8::from(render_eligible()),
            u8::from(render.render_entered),
            u8::from(render.simulation_ended),
            render.primary_count,
            frame.reservation.source,
            owned.source,
            owned.phase as u32,
            u8::from(owned.source == source && owned.phase == Phase::Bound),
            frame.refusal,
            frame.reservation.poll_generation,
            frame.reservation.job_generation,
            u8::from(frame.entry_readable),
            u8::from(frame.queued_move),
            u8::from(frame.input.ambiguous),
        )
    };
    log(&host, message);
}

fn render_fact(pass: &mut rsf_game_render_pass, role: u32) {
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.role = role;
    if let Some(frame) = FRAME.get() {
        let host = unsafe { frame.as_ref().services.host };
        if host.render_pass.is_some() && render_identity(pass) {
            rsf_skyrim_cpu_render_fact(pass, host.session_id);
        }
    }
}

fn report_render(pass: &rsf_game_render_pass, begin: u32) {
    if let Some(frame) = FRAME.get().filter(|_| pass.source_frame_id != 0) {
        let host = unsafe { frame.as_ref().services.host };
        unsafe { rsf_skyrim_call_render_pass(&host, ptr::null_mut(), pass, begin) };
    }
}

fn begin_render(main: usize, pass: &mut rsf_game_render_pass) -> Receipt {
    let restore = FRAME.get().map(|mut frame| {
        let screen = rsf_skyrim_cpu_screen(main, false, rsf_skyrim_query_menus);
        let mut previous = RenderReceipt::default();
        if rsf_skyrim_cpu_begin_render(&mut unsafe { frame.as_mut() }.render, screen, &mut previous)
        {
            frame_event(frame, RSF_GAME_CPU_SIMULATION_END);
        }
        (frame, previous)
    });
    render_fact(pass, RSF_GAME_RENDER_FRAME);
    report_render(pass, 1);
    Receipt::Render(restore)
}

fn begin_world(
    main: usize,
    caller: usize,
    output: u32,
    pass: &mut rsf_game_render_pass,
) -> Receipt {
    render_fact(pass, RSF_GAME_RENDER_SUBMISSION);
    let primary = FRAME.get().is_some_and(|mut frame| {
        let render = &mut unsafe { frame.as_mut() }.render;
        if !rsf_skyrim_cpu_begin_world(render, pass, base(), caller, output) {
            return false;
        }
        let screen = rsf_skyrim_cpu_screen(main, true, rsf_skyrim_query_menus);
        rsf_skyrim_cpu_world_screen(&mut unsafe { frame.as_mut() }.render, screen);
        render_identity(pass);
        report_render(pass, 1);
        true
    });
    Receipt::World(primary)
}

fn receipt(call: *mut rsf_skyrim_cpu_call) -> *mut Receipt {
    // The storage remains at a stable address until the native scope exits.
    unsafe { (&raw mut (*call).storage).cast() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cpu_before(
    index: rsf_skyrim_cpu_hook,
    call: *mut rsf_skyrim_cpu_call,
    pass: *mut rsf_game_render_pass,
) {
    let (owner, argument, caller) = unsafe { ((*call).owner, (*call).argument, (*call).caller) };
    let state = match index {
        RSF_SKYRIM_CPU_TICK => begin_frame(owner),
        RSF_SKYRIM_CPU_POLL => {
            let mut scope = PollScope::default();
            rsf_skyrim_poll_begin(&mut scope, owner);
            Receipt::Poll(Poll {
                scope,
                previous: POLL.get(),
                services: None,
                reserved: 0,
                begin: 0,
            })
        }
        RSF_SKYRIM_CPU_RENDER => begin_render(owner, unsafe { &mut *pass }),
        RSF_SKYRIM_CPU_WORLD => begin_world(owner, caller, argument as u32, unsafe { &mut *pass }),
        RSF_SKYRIM_CPU_DISPATCH => begin_dispatch(owner, argument),
        RSF_SKYRIM_CPU_CONTROLS => {
            rsf_skyrim_epochs_begin_write();
            if matching_poll(argument).is_none() {
                rsf_skyrim_epochs_invalidate();
            }
            Receipt::Controls
        }
        RSF_SKYRIM_CPU_SUBMIT => begin_submit(caller),
        RSF_SKYRIM_CPU_JOIN => begin_join(caller),
        RSF_SKYRIM_CPU_NAMED_POLL => begin_named_poll(caller),
        RSF_SKYRIM_CPU_MOVEMENT
        | RSF_SKYRIM_CPU_PLAYER_UPDATE
        | RSF_SKYRIM_CPU_CAMERA
        | RSF_SKYRIM_CPU_PLAYER_CAMERA => {
            let stamp = stamp();
            let mapped = match index {
                RSF_SKYRIM_CPU_CAMERA => rsf_skyrim_cpu_camera_consumes(owner),
                RSF_SKYRIM_CPU_PLAYER_CAMERA => rsf_skyrim_cpu_is_player_camera(owner),
                _ => rsf_skyrim_cpu_is_player(owner),
            };
            Receipt::Consumer { stamp, mapped }
        }
        _ => Receipt::None,
    };
    let receipt = receipt(call);
    unsafe { receipt.write(state) };
    match unsafe { &mut *receipt } {
        Receipt::Frame(frame) => {
            let frame = NonNull::from(frame);
            FRAME.set(Some(frame));
            frame_event(frame, RSF_GAME_CPU_FRAME_BEGIN);
        }
        Receipt::Poll(poll) => prepare_poll(NonNull::from(poll)),
        _ => {}
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cpu_after(
    index: rsf_skyrim_cpu_hook,
    call: *mut rsf_skyrim_cpu_call,
    pass: *mut rsf_game_render_pass,
    result: usize,
    returned: bool,
) {
    let (owner, argument) = unsafe { ((*call).owner, (*call).argument) };
    let receipt = receipt(call);
    match unsafe { &mut *receipt } {
        Receipt::Frame(frame) => {
            let frame = NonNull::from(frame);
            FRAME.set(Some(frame));
            finish_frame(frame, returned);
        }
        Receipt::Poll(poll) => finish_poll(poll, returned),
        Receipt::Render(restore) => {
            if returned {
                let pass = unsafe { &mut *pass };
                render_identity(pass);
                report_render(pass, 0);
            }
            if let Some((mut frame, previous)) = restore.take() {
                rsf_skyrim_cpu_end_render(&mut unsafe { frame.as_mut() }.render, &previous);
            }
        }
        Receipt::World(true) if returned => {
            if let Some(mut frame) = FRAME.get() {
                rsf_skyrim_cpu_end_world(&mut unsafe { frame.as_mut() }.render);
                let pass = unsafe { &mut *pass };
                render_identity(pass);
                report_render(pass, 0);
            }
        }
        Receipt::Dispatch {
            previous,
            services,
            event,
        } => {
            DISPATCHED_LIST.set(*previous);
            if returned
                && let Some(services) = services
                && services.host.cpu_event.is_some()
            {
                unsafe { rsf_skyrim_cpu_event_guarded(&services.host, event) };
            }
        }
        Receipt::Controls => {
            if !returned {
                // The native writer may have changed controls before unwinding.
                rsf_skyrim_epochs_invalidate();
            } else if active()
                && let Some(mut scope) = matching_poll(argument)
                && rsf_skyrim_cpu_is_player_controls(owner)
            {
                rsf_skyrim_poll_sink_produced(unsafe { scope.as_mut() });
            }
            rsf_skyrim_epochs_end_write();
        }
        Receipt::Jobs {
            before,
            ticket,
            owns_wait,
        } => {
            finish_jobs(
                index,
                before,
                ticket,
                *owns_wait,
                returned.then_some(result),
            );
        }
        Receipt::NamedPoll(previous) => NAMED_POLL.set(*previous),
        Receipt::Consumer { stamp, mapped } if returned => {
            let consumer = match index {
                RSF_SKYRIM_CPU_MOVEMENT => Consumer::Movement,
                RSF_SKYRIM_CPU_PLAYER_UPDATE => Consumer::PlayerUpdate,
                RSF_SKYRIM_CPU_CAMERA => Consumer::Camera,
                _ => Consumer::PlayerCamera,
            };
            finish_consumer(owner, stamp, *mapped, consumer);
        }
        _ => {}
    }
    unsafe { ptr::drop_in_place(receipt) };
}

pub(crate) fn start() -> bool {
    match cpu_hooks::start() {
        Ok(()) => true,
        Err(refusal) => {
            if let Some(services) = cpu_hooks::services() {
                let stage = unsafe { CStr::from_ptr(refusal.stage) };
                log(
                    &services.host,
                    format!(
                        "Skyrim CPU start refused: {} RVA={:X} status={}",
                        stage.to_string_lossy(),
                        refusal.rva,
                        refusal.status
                    ),
                );
            }
            false
        }
    }
}
