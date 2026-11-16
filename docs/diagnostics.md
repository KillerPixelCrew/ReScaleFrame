# Runtime capture diagnostics

The shared D3D11 host exposes **Capture next SR inputs** and **Capture SR failures
(max 3 per session)** through the shared overlay's options panel. Automatic
failure capture is off by default. Its control applies immediately and saves
`CaptureSRFailures` in the host settings section.

Automatic capture records resources at the failed evaluation or spatial-recovery
boundary, before their caller-owned lease ends. These are failure-time snapshots,
not guaranteed pristine pre-evaluation copies. The existing bundle records the
source/session/view, render/output dimensions, available textures, SR result and
matching final Present where available. Missing images remain unavailable and
incomplete frame joins remain partial. Spatial-recovery success does not replace
the failed temporal result.

At most three automatic bundles are requested per host session. Existing manual
or active captures take precedence; toggling the option does not reset the cap.
Planning-only reconfiguration does not trigger a failure capture. Captures use
the existing configured captures directory and can cause readback stalls.

This initial mechanism covers the shared D3D11 host, including its SR backends.
It does not automatically diagnose image-quality defects after successful SR,
capture missing adapter inputs before a host callback, or prove other graphics
API paths. End-to-end failure-content fixtures remain work.

**Capture 4 ready SR frames** is a one-shot overlay action. It waits for valid
primary gameplay sources with an unchanged ready configuration. Planning and
missing-camera frames cannot consume the sequence. It selects four distinct
sources, waiting for each active/manual capture to finish. These are samples,
not guaranteed consecutive native frames; readbacks can affect timing.

Inspect captured evidence without a running game:

```powershell
python tools/analyze-sr-captures.py <bundle-or-capture-directory> --output report.json
```

The analyzer distinguishes evaluated-and-presented evidence, spatial recovery,
partial/failed bundles and invalid identity/artifact records. A zero SR result
alone does not prove evaluation. It requires a captured backend output and valid
camera data, and checks final-image/Present evidence separately. It never labels
image quality accepted. Real retained temporal and spatial bundles were checked;
GPU automation-content validation is still pending.

## Programmatic harness

`tools/runtime-action.py` is the maintained bounded runtime helper. Callers supply
an owned PID, loaded Runtime base, matching Runtime file and SHA256. It validates
the x64 PE, loaded module identity and allowed public exports before allocating
bounded API packets in that process. It never searches for or launches a game.
Actions include `capture`, `capture-sequence`, `capture-failures-on` and
`capture-failures-off`; the latter two use the same saved control as the overlay.

```powershell
python tools/runtime-action.py <pid> <decimal-base> <Runtime.dll> capture-sequence <sha256>
python tools/capture-sr-sequence.py --pid <pid> --module-base <base> --runtime <Runtime.dll> --sha256 <sha256> --captures <captures/website> --report <report.json>
```

The sequence runner waits for three stable foreground status observations, arms
one sequence, and selects only new bundles tagged as sequence captures for that
PID. It retains partial/error classifications and always writes its report after
the operation starts. It never changes SR/FG settings or terminates the game.
Exit0 means four evaluated-and-presented bundles,1 means four bundles including
failures, and2 means an operation error/timeout. None establish visual acceptance.

The16-byte `rsf_d3d11_capture_control` contract is independently versioned and
size-checked. Commands use shared overlay actions rather than another capture
implementation. API invalid-packet/absent-owner tests and fake-runtime sequence
completion/timeout/report/cleanup tests pass. Real GPU content and native-game
validation remain separate.

The result writer accepts explicit `sequence_capture`, `automatic_sr_failure`
and `spatial_recovery` records in addition to `sr` and `fg`. Only `sr` changes
the bundle's temporal result. A no-device C++ test writes real capture metadata,
checks all marker files and verifies that spatial success cannot erase SR-3.

Fallout4 offers `recovery_native_output` before its native Present for an active
capture after committed reduced rendering. It verifies native frame, view,
window, context, chain and retained raster owner, then calls the generic runtime
texture-capture service. This is downstream failure evidence only; it does not
publish WINDOW/FG admission or turn an unmatched Present into a complete bundle.
Build verified; native capture-content verification remains pending.

For an FG validation sequence, pass `--require-fg`. It waits for actual active
FG, an effective provider and valid output statistics as well as ready SR.
FG metadata is joined by exact session/source/native/view pass identity; its
separate resource-pool generation is recorded rather than compared with the SR
pool generation. The metadata fixture verifies a distinct generation succeeds
and a different view cannot overwrite the matched FG record.

## Controlled camera pans

With explicit user authorization, `tools/pan-game-camera.py` sends bounded
relative mouse packets to a caller-selected foreground PID and samples the
Runtime status throughout. It never activates another window, stops before
sending further packets if foreground ownership changes, and sends no buttons.
Provide the same PID/base/Runtime/hash inputs as the sequence runner, plus
`--dx`, `--dy`, `--steps` and `--report`. Deltas are bounded to10 per packet
and240 packets. The report records input delivery only; camera movement must
be independently observed, and sampled status can miss single-frame failures.
The bounded loop and foreground-loss stop path have fake-sender unit tests.

`tools/restore-capture-trial.ps1` resumes a controller's deferred file restoration
only after the caller-specified game/launcher processes have exited. Supply
`-Report`, `-GameRoot` and `-BlockedProcessNames`. It preflights the full manifest,
checks backup/current hashes and path containment, rejects reparse paths, seals
mutable logs/settings, restores only known entries, and verifies each result.
It never terminates a process or deletes directories. Preserve its separate
receipt rather than rewriting an earlier controller timeout as success.

Automatic GPU capture is opt-in and must remain off for ordinary loading and
latency runs. The current D3D11 path performs synchronous GPU readback and image
encoding, which can stall the game; the session cap limits volume, not hitch
length. Use lightweight logs/status for those runs. The local native trial
controller explicitly starts CaptureSRFailures=0 and only requests its scripted
scene capture when CaptureScene is explicitly supplied. Do not enable capture
sequences as part of latency validation without a separate explicit request.

`runtime-action.py ... receipts-status ...` queries observed input-receipt counts.
They count accepted native callbacks, not unique physical actions or sampled
frame inputs. They are separate from latency markers, and do not establish PCL
notification delivery or vendor latency acceptance.

`runtime-action.py ... input-routes ...` reports actual receipt routing separately
from frame input. Explicit-window and thread-bound matches are distinct. Routed
counts mean successful calls on existing tokens; SDK coalescing can produce fewer
markers. No candidate or ambiguity remains unrouted and never creates a source.

The opt-in tools/watch-runtime-stall.py observes an existing hash-checked runtime for at most 600 seconds. If requested FG stops advancing application Presents for the configured threshold, it runs the explicitly supplied cdb noninvasively once to retain CPU stacks and a minidump. It does not enable GPU captures or terminate the game. A stall observation is diagnostic evidence, not proof of its cause.
