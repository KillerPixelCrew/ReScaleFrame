# Fallout 4 capability and latency admission audit, 16 November 2026

Read-only source audit of the current adapter and generic owner, using the
95ffa1e/ea377c8 built checkpoint as context. Other owners are changing reduced
inputs/hooks; this audit neither enables capabilities nor validates those edits.
No build, device/game run, attachment or deployment was performed. The narrow
cleanup acknowledgement correction authorized later is recorded separately below.

## Capability decision

| Flag | Current producer / smallest remaining step | Acceptance boundary |
| --- | --- | --- |
| FG_INPUTS1 | Pre-SR depth/motion and copied domain facts, FINAL_SCENE, FRAME, SUBMISSION and WINDOW producers now exist. The missing declaration is not a missing new SR/FG framework. Retain these guarded same-source leases; once coherent owners freeze, a cold validation build can declare this implemented producer. | Still prove actual normalization/capture, HUDless/outer Present match, submission completion and tag eligibility on the game path, including SR Off. Declaration alone proves none of those. |
| INPUT_TIMELINE2 | Seven guarded hooks implement prospective poll ownership. Generic accepted BIND already promotes flags0 sources and derives the missing span. No adapter INPUT_SAMPLE or SIM hook is necessary. A coherent cold validation build can declare this source implementation, with runtime refusal still decisive. | Prove accepted RESERVE/SAMPLED/BIND and current source clocks, one pre-poll pacing operation, generation continuity and provider-specific tags/markers on actual gameplay. |
| RENDERED_SOURCE4 | The generic render-only alternative permits only FSR3/FSR4, latency Off and no pre-input pacing. The existing render scopes can supply that vocabulary, but it is not the intended replacement for verified native consumption. | Do not combine with INPUT_TIMELINE: host creation rejects that combination. Do not select it to keep FG running on excluded load/freecamera/unbound frames. |

`plugin.cpp` still declares capabilities0. The smallest intended declaration for
full testing is FG_INPUTS|INPUT_TIMELINE, value3, after the source owners freeze;
INPUT_TIMELINE alone can expose latency without FG resources. No flag was
changed here. This is source readiness for a bounded test, not shipping/game
acceptance. Native motion coverage and composed-depth quality remain separate
requirements from event admission.

## Actual CPU facts and generic derivation

The seven hooks are Swap, keyboard RAWINPUT, mouse RAWINPUT, OnIdle, shared Poll,
queue dispatch and actual PlayerControls entry. The installed image fingerprint
and native proof are retained in [input ownership](fallout4-input-ownership-20261116.md).
The selected Poll returns at c342ba inside the selected OnIdle. Every dispatcher
invocation advances the shared generation, including worker/foreign callers.
RESERVE precedes unchanged Poll; poll_begin_qpc is measured after RESERVE returns;
SAMPLED follows return with PRODUCED and input_kind0. That asserts a completed
sample buffer, not a new keyboard/mouse/controller event.

Only actual PlayerControls entry from1673b74 within the admitted queue dispatcher
opens the reserved FRAME_BEGIN and emits BIND. A dispatch that only adopts its
queue generation without invoking PlayerControls does not bind. Native pointers,
Main flags/menu/polling-enabled state, queue generation, raw revision and in-flight
poll/raw writers are checked before and after callbacks. Intervening, overlapping,
worker or raw mutation, changed consumer/state and quiescence withdraw the source.
No adapter sleep, vendor call or fake simulation fact is needed.

The generic ledger accepts BIND only for an adopted PRODUCED reservation and
matching live, nonfailed frame, ordered timestamps/frequency and valid state.
On acceptance it promotes the stored input timestamp to actual poll entry and
sets canonical INPUT_KEYED even when the adapter's original flags are0. It then
derives SIM_BEGIN at BIND, tagged DERIVED_BOUNDARY, after revalidating state after
the sink. Submission begins derive SIM_END only from the bound, submitted,
same-provider post-update owner. Existing native stages can replace derived
ones. These are honest core-derived spans, not proof of engine-wide simulation.
Missing native SIM callbacks are therefore not a blanket blocker.

RESERVE establishes the generic token and may pace once, before native Poll.
FRAME_BEGIN adopts that prospective owner; BIND derives a marker, not another
token or sleep. INPUT_SAMPLE is not dispatched a second time. Generation and
actual-pacer checks remain: Reflex/XeLL/Anti-Lag behavior belongs to the core.
FSR3/4 with latency/limiter Off has the existing no-pacer-required exception;
other requested paths must satisfy actual pacing/provider requirements.

Startup callbacks are dropped before d3d11_host.bridge_bound. The adapter's
pending.bound means it emitted the native BIND fact through a void callback,
not that the runtime accepted it. Only ledger state BOUND/CLOSED and same-source
snapshot/clocks/counters can prove acceptance. A rejected source never gains
input ownership from that local boolean. The Swap fallback opens only a rendered
FRAME_BEGIN/END identity; in input-timeline mode it has no promoted input or SIM
span, fails measured FG admission, cannot pace through missing poll ownership,
and marker dependency checks refuse missing-stage calls. Do not promote it into
input timing or switch per-frame provider policy to hide that refusal.

## Render, screen and depth association

Ordinary WorldBegin obtains exactly one CPU submission identity, validates native
owner/context/device/window/outer chain/view/frame, then opens FRAME and native
SUBMISSION before work. AFTER_SIMULATION is a constructor ordering fact, not an
adapter SIM_END. Pre-imagespace inputs require current-source camera/resources
and the same-source prepass domain snapshot. Mapped depth stays a scalar composed
input with explicit approximation limits, not whole-scene physical validity.

Screen queries recheck Main state, freeze counts, menu state, camera identity and
world lease. Loading/main/pause/Pipboy/map/VATS and VATS/freecamera states refuse;
only camera indices0/4/8 with all other facts stable qualify as gameplay. Refusal
is sticky through FG inputs, HUDless and pre-Present checks. A local eligible
screen still cannot override a core-unbound input source.

FG_INPUTS is reported before SR can mutate leased resources. Its normalized
contents are retained by the host; the adapter keeps only copied scalar facts.
HUDless uses actual logical framebuffer0 after imagespace, rechecking full output
extent/frame/context/chain. WINDOW stays open while the original outer Present
performs its own work. The guarded native Present routine entry from c335ec
closes SUBMISSION before outer COM Present, with unchanged original call/retry
handling. After Swap, finish_render_scope balances any refused submission, then
WINDOW and FRAME, before CPU FRAME_END. No retry mints a source and no FRAME end
may abort the token ahead of Present. The existing input-timeline prepare
conjunction still requires all of submission, bound reservation, CPU clocks,
provider generation, matching scene/HUDless/window, supported screen, continuity
and provider/vsync state.

## Coverage and lifecycle gaps, without fabricated events

Keyboard/mouse RAWINPUT observations are unframed receipts. Controller is sampled
inside the native five-device Poll, but no controller received-event hook/counter
exists. A successful or unchanged XInputGetState is not delivered user input.
The native message loop's PeekMessageA PM_REMOVE sites are proven but not hooked;
WndProc/overlay observation or PeekMessage without removal is not a dequeued fact.
Do not claim complete controller event notification or PCL dequeued-message
coverage from the current seven hooks.

Prospective-source policy deliberately does not promote arbitrary unframed/raw
or WINDOW_MESSAGE receipts to that source; native_cpu_reservation tests explicitly
keep input_mask/pings0. The original flags0 FRAME_BEGIN also does not use the
legacy keyed-FRAME_BEGIN pending-message drain. Do not bypass this by tagging
FRAME_BEGIN as measured input. For complete event notification, the smallest
separate work is verified typed native events at the already-hooked actual
consumer, reported as source-associated INPUT_EVENT kinds only when observed.
Actual dequeued MSG IDs require the native message-pump lifetime/ABI guard; any
PCL association/routing for prospective sources belongs in the generic owner,
not a vendor call or input-sample invention in the adapter. These notification
coverage gaps do not invalidate the existing actual poll/BIND sampling boundary.

Quiesce disables publication first and keeps modules/trampolines alive. Entered
callbacks and render/frame/window/output owners hold busy until their own normal
closure, and CPU hook removal waits for render/creation drain. This is the safe
intended ownership order, not a tested live lifecycle result. The required
lifecycle is successful cold start, quiesce with BUSY while owners drain, then
stop. `plugin_session.cpp:rsf_plugin_session_start` explicitly returns NOT_READY
when self->quiesced is set; session quiesce->start is not a supported hot-resume
operation. The earlier wording suggesting a hot-restart correction was required
was too broad and is superseded by this session-contract check.

At the direct hook level, quiesce leaves patched entries while start rechecks
original bytes/requires MH_EnableHook MH_OK, and creation prepare/start requires
a cold native owner. These are limitations outside a supported session-resume
contract, not a new integration defect or a reason to invent a resume framework.
Do not clear busy or remove in-flight trampolines to simulate restart. The
idempotent auxiliary cleanup acknowledgement remains a separate drain fix;
it does not change generic lifecycle policy.

## Required negative and bounded acceptance checks

- Verify startup-before-binding BIND is not reported as accepted; compare later
  ledger BOUND/CLOSED, actual poll clock and provider generation for the same ID.
- Retain flags0 promotion, refused/withdrawn/unbound BIND, reentrant withdrawal,
  stale generation, duplicate and marker-order tests; no second INPUT_SAMPLE,
  sleep, token or counter promotion on unchanged polls.
- Exercise worker/intervening/raw-overlap changes, missing consumer, receiver
  generation adoption without callback, skipped redraw and multiple WorldBegin;
  each must refuse safely rather than reuse another source.
- Check menu/load/freecamera/pause transitions and SR On/Off with the same-source
  FRAME/SUBMISSION/FG/HUDless/WINDOW/actual Present/end ordering and normal closure.
- For each available provider, test FG/latency Off and requested modes, actual
  pacing-before-poll, reset/switch generation, callbacks versus completed activity,
  and received-event/PCL coverage separately. Tags or callback counts are not
  scanout or latency measurements; foreground must be verified at OS level.
- Quiesce during poll/render/auxiliary ownership must return busy and retain all
  leases; retry drain and stop must complete without an orphan or falsely cleared
  owner. A session start after quiesce must retain the existing NOT_READY refusal,
  not be treated as a failed supported resume.

Current generic reservation and scoped rendered-source fixtures establish
synthetic contracts only. The latter intentionally uses RENDERED_SOURCE with
zero physical input/pacing and cannot certify the actual native input-timeline
path. A bounded gameplay trial with source3 capability is still required; no
capability was enabled and no game acceptance was added by this audit.

## Narrow auxiliary cleanup acknowledgement follow-up

Coordinator subsequently authorized idempotent restore acknowledgement: after
valid host/user, size/version, current session and graphics-owner-thread checks,
no pending transaction returns OK, including a failure already rolled back.
A pending transaction still requires its exact frame/view/session and successful
restoration; mismatches never drop ownership. The function now checks configured graphics ownership/session before its empty
OK, and still calls exact matches plus restore for pending work. Suspension does
not prevent draining. No ABI or option is added.

Added graphics-free host checks for wrong user, short size, wrong ABI and unknown
graphics owner. The existing scoped rendered-FG fixture adds verified-owner empty
acknowledgement, failure-before-prepare acknowledgement, wrong session/thread
refusal in both empty and pending states, mismatched pending frame/view refusal,
a subsequent BUSY check proving the original lease survived, successful repeat
OK, and repeat OK after a second destination fails partial preparation and rolls
back. Its one auxiliary transaction runs inside an existing warmed source rather
than adding another test target or game run. These checks are written but not
built/executed by this worker; the coordinator owns the gate and device fixture.

## Later bounded trial limit

The coordinator's borderless checkpoint95 run PID37768 visibly entered a world
and established input/window fit, but recorded only two SR publications before
a target-math refusal and no completed F10 capture. Native Quit UI then ended
with C0000005 without a localized fault. All20 staged entries were restored after
a transient exited-process-name race. Geometry/math owners are triaging that
separate refusal; this trial adds no FG/latency capability acceptance and does
not justify changing session lifecycle policy.
