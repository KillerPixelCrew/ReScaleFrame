# Native marker admission, 16 November 2026

Question: why can a source with only accepted, unkeyed FRAME_BEGIN/FRAME_END facts produce render-marker order violations? The adapter audit reported two violations per such world source. Shared source inspection confirmed that `start_render_marker` set `CpuOwner::render_started` before checking CPU facts or provider acceptance. It then attempted RenderSubmitStart without a simulation marker. `end_render_marker` independently attempted RenderSubmitEnd even when that start failed. The analogous Present path also attempted markers without an accepted interval. These are generic admission errors, not evidence that the adapter should invent input or simulation facts.

The repair stays in `native_fg.cpp`. Submission admission now requires a known, nonfailed CPU record with input-stage and simulation-start marker prerequisites. Keyed sources retain the existing derived simulation-end dispatch; prospective sources additionally retain accepted BIND, actual poll-entry timestamp, post-update submission identity and current provider generation checks. Unkeyed frame-only fallback returns before that derivation and before any marker call. Rendered-source mode continues to report no physical-input latency markers.

Attempt-in-progress flags prevent synchronous reentry from opening another interval. Successful interval state is published only after the CPU marker is newly accepted and the existing provider call succeeds. A duplicate CPU-marker result is not a new provider start. Provider errors remain reported and mark that owner failed. Render and Present ends require an accepted start and cannot repeat. Present admission retains canonical keyed dependencies and the existing legacy threaded ordering, including Present before late simulation/render end. Actual Present completion still releases the frame join even when latency-marker admission was refused. No CPU-order diagnostics, counters, CPU provenance, pacing/provider selection, pre-poll sleep/token mechanism, resources or public ABI were changed.

The existing `Host::cpu` receipt now retains the actual return from its single `rsf_native_cpu_dispatch` call. It keeps up to 24 initial rows plus at most 64 late rows: 53 first-stage/one-in-128-source samples and an independently reserved first refusal for each of the eleven stages. Each row includes session, source, stage, accepted, flags, packet version and size. Qualified source packets additionally expose native generation, poll-entry timestamp when present, and abort reason. Base and extension size/version guards precede field reads; extension copies happen only after receipt-budget admission. There is no extra dispatch, CPU snapshot or adapter callback acknowledgement claim.

## Device-tested regression coverage

The existing `tests/d3d11_present_bridge.cpp` plain hidden-device fixture now exercises the actual CPU ledger, FG owner and provider:

| Case | Required outcome |
| --- | --- |
| Unkeyed frame-only fallback, including unmatched/repeated ends | No input timestamp, substitute, pacing timestamp, marker or order-violation increment |
| Legacy threaded input | Existing late SimulationEnd/RenderSubmitEnd order still completes all six markers |
| Keyed input and actual SimulationBegin | Existing submission-time end derivation, synchronous logger reentry and duplicate begins/ends complete once |
| Provider token withdrawn before submission start | Failed start is retained, with no matching render end or Present markers |
| Actual prospective poll followed by accepted BIND | Real poll timestamp, canonical six-marker order and the retained single pre-poll token; no second submission sleep |

The regression passed these checks, including unchanged duplicate-marker and order-violation counters across these cases. Existing negative-order tests in `latency_event_contract.cpp` and `native_cpu_reservation.cpp` remain unchanged, including genuine invalid-marker calls and refused binding cases.

The coordinator's background Release build completed with exit 0 in
`.local/fallout4-research/build-scene-input-marker-20261116.log`. The safe CTest
subset completed with exit 0: 218 passed and 14 skipped out of 232, in 82.22
seconds (`ctest-scene-input-marker-background-20261116.log`). Four foreground FG
tests were explicitly excluded so the user could keep working. Cargo fmt and
Clippy passed. This is a partial gate; full `eng/verify.ps1` remains deferred.
The existing negative CPU-order tests passed in that subset.

The separate exact two-argument hidden run completed with exit 0 in
`.local/fallout4-research/hidden-marker-scene-input-20261116.log`, including the
new marker-admission regression PASS for the fallback, legacy, keyed, bound-poll,
failed-start, synchronous reentry and repeated-end checks. Its invocation was:

```powershell
& './build/windows-x64/fixtures/fg/Release/rsf_d3d11_present_bridge_test.exe' 'D:/Coding/ReScaleFrame/.local/fg-provider-sdk-comparison-20261115-202429/streamline' '0x10de'
```

Supply exactly those two arguments. The no-argument CTest entry skips with 77, while optional visible/borderless modes are separate existing paths and are not needed here. The hidden fixture requires the matching SDK root and supported adapter.
The separate `scene-input-scoped-fixture-20261116.log` run also exited 0 with 55
sources, 41 captures, 39 SR evaluations, 34 tags and 34 generated callbacks. Its
rendered-source latency owner, pacing and marker counts remained zero. The
matching Runtime SHA256 is
`90E86B69D96B29D5D74F42E1A5DF61289FC0C4B3CD72EC4B1EC9C793F9F13178`.
These device checks do not prove physical scanout, lower latency, game provider
activity or game acceptance. Actual late RESERVE/SAMPLED/BIND/refusal receipts and bounded foreground gameplay/provider validation remain outstanding.
