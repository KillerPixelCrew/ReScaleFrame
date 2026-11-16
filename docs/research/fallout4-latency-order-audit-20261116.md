# Fallout 4 CPU and marker ordering audit, 16 November 2026

This audit reads source and retained receipts only. It does not query or interact
with a game process. Trial `native-raster-20261116-132159-236`, PID6188,
used the reduced batch after the coordinator's Windows Release221/15 gate and
scoped55/41/39/34/34 fixture. Capture timed out: native depth admission stopped
at producer color selector26. The native facts owner owns that separate repair.
No SR, reduced-quality, input pacing or FG acceptance follows from this run.

The controller's `world-observation.json` describes a visible first-person
shower interior entered from ExitSave. That is a retained visual observation,
not a native cell name or FormID. The renderer/CPU logs do not identify a more
precise location. Do not infer a particular named cell from the interior.

## What the order counter measures

At the controller's public latency sample, frames21688 and order_violations9904
coexist with owner/mode0, marker_calls0, derived_boundaries0, substitutes0,
duplicates0 and unframed_inputs0. This is not9904 proven physical input or SDK
ordering failures. `native_cpu.cpp` increments that same order counter both on
invalid CPU stage order and on rejected `rsf_native_cpu_marker` prerequisites.
The latter validation runs before `native_fg.cpp::frame_marker` can call the
vendor presentation marker, even while requested latency is Off.

The retained loader log has180 completed120-frame counter windows. Windows1
through139 report0 violations; window140 reports128; the next40 report240
each, sum9728. The public sample has88 more frames than21600, and its remaining
176 violations exactly continue that two-per-frame slope. First world source
16737/submission1 appears with native frame17204. Numerically,
`2*(21688-16737+1)=9904`. The equality and zero-to-two transition match the
start of ordinary world FRAME/SUBMISSION emission, not a native CPU timestamp
reversal. Source IDs are not generally equivalent to frame counts, so this
arithmetic is supporting evidence, not an event-by-event reservation trace.

## Actual producer and generic consumer paths

`cpu_events.cpp` reports a fallback FRAME_BEGIN when admitted Swap has no bound
identity, then FRAME_END after Swap. `emit` uses current event version with
flags0 at begin. It reports RENDER_SKIPPED only when no ordinary world was
associated. It does not invent INPUT_SAMPLE or SIMULATION boundaries. The
generic stage checker accepts a mask1 partial FRAME_END, including this actual
rendered fallback; it remains incomplete input/simulation evidence.

The distinct prospective path is RESERVE before selected native Poll, SAMPLED
after an unchanged actual poll, then FRAME_BEGIN and BIND at the real guarded
PlayerControls consumer. Runtime BIND promotes the earlier poll timestamp and
INPUT_KEYED flag and derives its simulation-span begin centrally. Its failure
or a later poll/raw/native-state change aborts that source. No adapter fake SIM
event or late guessed input sample is justified by these marker errors.

The generic render path has a concrete invalid-marker attempt:

1. Ordinary SUBMISSION begin calls `start_render_marker`. Its strict bound/input
   admission checks currently apply to prospective owners, but a known unkeyed
   fallback can still reach RENDER_SUBMIT_START with marker mask0.
2. `native_cpu_marker` rejects marker2: the retained legacy prerequisite table
   requires simulation-start bit0 first. This increments order_violations.
3. FRAME end calls `end_render_marker` even if the start marker was rejected.
   Marker3 requires submit-start bit2, still absent, and increments it again.

This reproduces the two-error fallback path directly from source and matches
the observed windows. In this run failed producer admission also prevents a
WINDOW/FG-input lease, so later final receipts have source0 rather than an
eligible source for Present. It does not establish that every counted error
has been individually traced or why prospective candidates were absent/refused.

The smallest repair belongs to the shared native_fg marker admission/body,
with native_cpu's strict validator retained: do not begin a marker interval
for an unbound/partial input source, and do not emit an interval end for a start
that was never validly accepted. Preserve valid legacy and actual prospective
paths. An eligibility predicate must use real stage/reservation facts, not just
turn off the counter or change the adapter to fabricate missing boundaries.
The shared owner is assigned this repair; this worker did not edit it.

## Logging and the next missing native fact

Host::cpu logs the integer dispatch return, but only for the first24 incoming
events. Both retained loader copies show only initial skipped frames1..12,
all accepted. That budget is exhausted before the first ordinary world source
and cannot identify later RESERVE/SAMPLED/BIND/ABORT or marker rejection. The
adapter's cpu_event callback is void; it cannot honestly label its own emitted
fact accepted by the runtime. Any adapter receipt must say emitted, or native
candidate accepted/refused, and keep runtime acknowledgement separate.

The exact next missing fact is the selected-main-poll predicate and native
reservation/consumer sequence after world entry: guarded caller, poll/raw
generation, native state, sampled queue generation, consumer admission and
abort reason, paired with bounded per-stage runtime return status. The first
24 startup receipts cannot substitute for that evidence. Unframed input count0
alone proves neither absent device activity nor a bad poll producer: current
RAWINPUT receipts cover only their exact native call sites, and the native MSG
and delivered-controller receipts described in the input note were not yet
implemented in this artifact. Their source0 delivery facts do not themselves
grant prospective ownership or latency availability.

## Final-state correction and limits

The public modeOff sample is a point observation, not final settings. The root
later copied `loader-post-controller.log` separately; its real FG prepare rows
show selected_source0, screen0, inputs0, submission_ok0, hudless0, owner0 and
result-8. Thus FG was actually attempted/refused later; do not claim final FG
requestedOff. The root subsequently reported no Fallout4/game/launcher process.
No exit handle/result was supplied, so normal exit0 is not established. Original
controller results and later user changes remain preserved; recovery is not
executed by this audit. No build, deployment, UI, focus, input, attach or process
action was performed by this worker.


The subsequent authorized source-only notification change is documented in the
[input ownership followup](fallout4-input-ownership-20261116.md#native-notification-implementation-followup-source-frozen).
It does not change the artifact or acceptance limits audited above. The shared
owner separately repairs marker start/end eligibility and adds bounded late
real dispatch acknowledgement; neither repair is claimed runtime-validated here.
