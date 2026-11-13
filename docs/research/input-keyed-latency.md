# Input-keyed latency event contract

13 November 2026. This is the w37 implementation contract, against repository baseline 43619d9. Latency belongs to the
core orchestrator. Game and engine adapters report facts through the size/version-checked Game API CPU/render/submit/
present callbacks. They neither call vendor SDKs nor select pacing owners or marker meanings. Implementation and game
acceptance are tracked separately below; the contract alone is not evidence that a host has migrated.

| Integration reports | Core orchestrator decides |
| --- | --- |
| Session and monotonic source-frame identity, frame begin/end | Frame/token reservation, bounded ownership and retirement |
| The engine is about to sample input, immediately before the native read | The single pre-input pacing operation and simulation-start marker, with the same source identity |
| Received keyboard/mouse/controller data, as kind bits without key/button values | Per-frame deduplication, vendor input markers and kind counters |
| A dequeued or delivered window message ID | Registered PCL ping recognition/routing and ping counter; no adapter hardcodes SDK message names |
| Simulation begin/end, recording/submission and real Present begin/end | Marker mapping, deduplication, order validation and legal missing-event substitutes |
| Graphics device/API, provider readiness and current settings, via Runtime-owned hosts | Reflex/PCL, XeLL, Anti-Lag 2 or none; one owner; driver failure fallback; status/overlay reporting |
| Which source image is real and which vendor outputs are generated | Frame-generation frame typing and driver metadata, on Runtime/vendor presentation paths |

The existing rsf_game_cpu_event carries source identity, event kind and optional message ID. Its INPUT_SAMPLE fact means
"about to poll native input now", not a guessed event at frame open. INPUT_EVENT means a real received kind or message
candidate. SIMULATION_BEGIN/END remain engine facts. PACING is a legacy pre-input boundary fact; new adapters must not
invent it at frame open. Core policy makes the pacing call at the real input boundary and refuses duplicate calls for the
same frame. Game ABI 21 appends event_version and flags to rsf_game_cpu_event; event version 1 opts into INPUT_KEYED facts.
The D3D11 FG access table is version 2 so an older Runtime cannot silently ignore this pacing contract. WINDOW_MESSAGE
marks a delivered message and permits source 0 when no engine frame owns it. The core matches a registered PCL
candidate at the next frame reservation only within 250 ms, or expires it. It does not retain ordinary keyboard/mouse
kind events across frames. INPUT_SUBSTITUTE is core-generated for a missing input callback, skips a late sleep and
refuses FG for that source. RENDER_SKIPPED reports an update that did not render; the core retires its token rather
than making the next input boundary wait for a Present that cannot occur.

Required source-frame order: previous Present completion (or the Runtime's documented threaded join), reserve frame,
core pacing, native input sample and simulation start, simulation end, render submission begin/end, actual Present
begin/end. The engine may still poll input in menus, scopes and pause: these are valid input frames, independent of FG
eligibility. A frame without an input callback must be labelled as a substitute, never presented as measured real input.
No queued kind or ping may leak into a later source identity after a skipped render, load, reset or pause.

The migration retains the existing legacy threaded marker dependencies: simulation can overlap rendering, and
EndFrame can follow Present. Their marker ledger requires simulation start before simulation end/render start,
render start before render end/Present start, and Present start before Present end. Input-keyed version-1 sources
use the strict canonical sequence above. This compatibility path is not evidence that a legacy host reports a real poll.

Anti-Lag 2 is one core mechanism using AMD's pinned MIT SDK. Its update belongs at core pacing, gated by AMD device
identity and SDK success; non-AMD or failed initialization must leave it unavailable. DX12 FSR uses the documented
end-render marker and swapchain private data so FSR can distinguish real/generated presents. XeLL and Reflex owners
must not receive a second independent pacing sleep. No AMD hardware acceptance is available on this machine.

Migration order: Anomaly and shared mechanisms first, then AC7, RDR2, Unity Mono/URP, Godot and Minecraft. Project
Wingman currently observes only; its unavailable rendering/input lifecycle must be stated explicitly. The final evidence
matrix will identify each fact's file/line, fixture coverage, missing substitutions and game-run acceptance limits.
