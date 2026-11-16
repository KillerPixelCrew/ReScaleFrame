# Generic unframed input receipt routing proposal

Proposal only. Actual received/dequeued input and consumption of sampled engine
state are different facts. A source0 INPUT_EVENT can support notification
telemetry without promoting any prospective source to BOUND or measured input.
The existing prospective attribution refusal in `native_cpu.cpp` stays intact,
including its negative fixture expectations of no attributed inputs/pings.
No implementation, test execution or native acceptance is claimed here.

## Current contract and missing facts

The existing v1 event carries session, actual QPC/frequency, kind bits, message
ID and WINDOW_MESSAGE/INPUT_UNFRAMED flags. Source0 packets currently enter a
16-entry pending queue. Only a keyed FRAME_BEGIN drains that queue, and the
prospective INPUT_EVENT guard deliberately prevents those observations being
counted against a reservation. Flushing at BIND or removing that guard would
conflate receipt with consumption. Neither is the proposed change.

PCL registration is already generic: the Streamline host obtains
`statsWindowMessage` from PCL state. `rsf_streamline_host_input` uses an existing
source token, emits the registered ping or controller notification once per
token and does not emit deprecated keyboard/mouse marker6. It increments its
kind counters before marker success, so those counters are not proof of SDK
acceptance. The game must never choose the PCL message ID or vendor marker.

Private CpuOwner records are keyed under a mutex; provider calls have a shared
lifetime guard and generation check. CPU services accept any-thread facts.
WINDOW/Present match state is thread-local, so an arbitrary input worker must
not query that TLS and call it the current window. The host's cold-bound
chain/window registry can supply routing context, but it cannot prove which
window a null-filter message pump actually dequeued. The current event also has
no stable receipt ID: timestamp/kind equality is not a safe duplicate test for
two genuine events in one QPC tick.

## Smallest staged change

**Observation-only reporting needs no new game contract.** On validated actual
source0 callbacks, record bounded private receipt counters at callback time,
separate from frame input_mask, frame input timestamps, pings and reservation
state. Counters mean accepted callback receipts, not unique physical actions.
Do not replay them at a future BIND. Existing v1 behavior and prospective
negative fixtures remain unchanged. This alone does not provide safe
window-specific or duplicate-safe vendor notification routing.

**For the requested stronger route, append provenance to the existing event
interface.** Keep `rsf_game_cpu_event` and v2 source-event layout unchanged.
Define a size/version-checked extension of that same INPUT_EVENT callback,
source_frame_id0, with an event version reserved for receipt packets, containing:

- `receipt_id`: nonzero monotonic identity of one actual removal/delivery batch
  within the reporting session, shared by any duplicate report of that receipt.
  It is not a frame/token ID. One adapter counter suffices; no contents are kept.
- `window_key`: the actual dequeued message's registered window identity, or0
  when there is no proven target. For controller queue delivery,0 is truthful
  when no native window association exists. The core may derive session/window
  routing only when its host binding is uniquely owned and says so in status.

The receipt namespace needs one authoritative reporter per host/session, or
cooperating reporters must preserve the same receipt ID. Independently assigned
IDs at PeekMessage and WndProc cannot prove that two callbacks are one OS event.
The minimal route must not claim cross-observer uniqueness from QPC equality.

Use the existing stage and callback, not a second event channel or source
ledger. Old/short packets remain accepted only under their existing semantics;
an older runtime must reject the new version safely. Follow SDK ABI/version
policy when landing the optional extension. For Windows thread messages with
no HWND, an exact target window cannot be invented from the host's HWND.

## Immediate core route, with explicit derived association

1. At the actual callback, validate packet size/version, session/frequency,
   receipt ID monotonicity, allowed kind/flags, active current lifecycle and
   report QPC not in the future. A kindless message is a PCL candidate only if
   its ID equals the current registered PCL ID. Ordinary window messages remain
   observation-only. Keep the original source0 timestamp and payload-free facts.
2. Deduplicate the receipt in the existing host/session ownership context using
   a small monotonic high-water mark and bounded replay mask, not an input-state
   ledger. Multiple callback threads can deliver IDs out of order; reject only
   already-seen or outside-window IDs, not every ID below the high-water mark.
   Two genuine IDs count as two receipts even if QPC/kind/message match.
3. Derive an association to an **already acquired** token only. Candidate owner
   must have the same session/provider generation, completed its pre-poll pacing
   boundary (`ready_qpc <= actual receipt QPC`), remain neither failed/aborted/
   retired nor render_started/present_started, and belong to the validated host
   window. OPEN/SAMPLED/ADOPTED/BOUND reservation state is permitted for this
   notification association; permitting OPEN is not a consumption assertion.
   This chooses a current CPU-phase owner rather than a rendering predecessor.
4. Require exactly one eligible owner. No candidate, multiple eligible windows/
   owners or unproven routing yields an unassociated receipt counter and no SDK
   call. Do not choose the newest frame globally, wait for BIND, associate with
   a future not-yet-acquired token, or delay the SDK call until a later tick.
5. Call the existing generic provider input operation with that owner/token while
   its provider generation/lifetime is pinned. This is core policy. It causes
   no acquire, sleep, INPUT_SAMPLE, SIM boundary, BIND, frame input_mask or
   marker0..5 update. The public event remains source0. The private association
   receipt records token/source ID and `derived-notification-association`.
6. Retain once-per-token ping/controller coalescing in the provider. Keyboard
   and mouse notifications remain counts, not deprecated ordinary input sample
   markers. A controller notification reflects actual delivered event/batch,
   not an unchanged successful controller poll or hardware timestamp.

A kindless WINDOW_MESSAGE ping requires a nonzero proven target window key
matching the bound host; unique-host inference cannot turn a thread message
with HWND0 into a proven window-targeted ping. A controller-kind receipt with
window0 may use uniquely owned host/session input routing, labelled derived,
without claiming that the device's event physically targeted an HWND. Ambiguous
or missing host input routing stays unassociated.

The candidate predicate above is deliberately narrower than all live tokens.
It may leave notifications unassociated during presentation or sleep. That is
an honest limit, not permission to mint a token or widen source ownership.
If ordinary callback timing leaves no candidate, measure that fact before
proposing a different generic association policy.

## Races, lifetime and status

Choose/claim an attempt under private receipt/owner locking, then invoke SDK
work outside the CPU ledger/owner lock. Recheck generation and token identity
under the existing provider lifetime guard; do not use a copied raw token after
retirement. Existing host input emission holds marker_guard across its SDK
operation, protecting token/abort interaction. A per-host callback reentry guard
must prevent a recursive receipt from entering that SDK operation again. Count
it as observed/reentry-refused or coalesced, never replay it later as if it were
fresh. Do not solve reentry by making the CPU ledger lock recursive.

Private per-token in-flight/coalesced state must be claimed before SDK emission;
duplicates/reentry cannot make a second call for that receipt. Existing provider
coalescing is once per successful ping/controller notification for a token;
a distinct genuine receipt after a failed SDK attempt is a distinct boundary,
not a deferred retry of the old receipt. On SDK failure,
record the attempted/failed receipt and do not retry that same receipt after
its actual boundary. Abort/session reset/provider replacement invalidates any
uncommitted association. A callback already admitted to a pinned provider is
serialized with existing marker/token retirement. Stop first quiesces new
receipt reporting and drains entered callbacks before releasing that ownership.
No render-state/graphics-context work is allowed on an input worker.

Initially bounded private diagnostics suffice to validate this route. Existing
frame input/ping counters and low-latency availability must not silently change
meaning. Exposing it in API/overlay status needs a separate append-only,
size-compatible receipt-status tail: observed, duplicate/reentry, unassociated,
associated-derived, SDK-attempted/succeeded/failed, kind/ping totals and last
association provenance/session/generation. It must distinguish SDK notification
success from sampled-state ownership, pacing availability and measured latency.
`DERIVED_BOUNDARY` and `INPUT_SUBSTITUTE` are unsuitable receipt provenance flags:
neither a lifecycle boundary nor a substituted poll is being reported.

## Required negative cases before implementation acceptance

| Case | Required result |
| --- | --- |
| No host/window/token, suspended lifecycle or source0 old packet entering the proposed new route | Observation/refusal only; no new token, SDK notification or pacing. Legacy routing semantics stay unchanged. |
| Window mismatch, null window with ambiguous binding, wrong registered message | No PCL emission; retain honest unassociated receipt/reason. |
| Session/frequency/generation changes, invalid/stale/future QPC | Refuse association; never replay into a replacement session/token. |
| Same receipt twice, concurrent duplicate, recursive callback | At most one attempt; no double receipt acceptance or SDK emission. |
| Several real pings in one token | Count unique receipts separately; existing vendor coalescing permits one successful ping marker for that token. |
| Real controller delivery versus unchanged native poll | Delivered batch may notify; poll success alone may not. |
| Candidate aborts before claim/SDK call, provider changes | Unassociated/failed attempt; no fallback to another token after the boundary. |
| BIND occurs before Present | No notification replay at BIND and no change to reservation/source ownership. Prior association remains labelled derived notification only. |
| Receipt after render begins or token retires | No eligible CPU-phase token; do not attach to an old presented image. |
| Prospective packet submitted to existing frame INPUT_EVENT path | Existing guard still refuses input/PCL attribution, as current negative fixtures require. |

This proposal does not block factual source-owner/FG/pacing validation. It needs
generic review and focused synthetic race/lifecycle tests before code, followed
by vendor/device acceptance. No game-specific branch, option or adapter pacing
policy is proposed. This task changed this document and an ignored source-anchor
JSON only; no production/test/CMake edits, builds, device/game/process/UI actions,
attachment or deployment occurred.
