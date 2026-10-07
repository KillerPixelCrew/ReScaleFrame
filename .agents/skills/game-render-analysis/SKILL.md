---
name: game-render-analysis
description: Investigate a shipped game's render passes, temporal inputs, camera data and engine hook sites for ReScaleFrame. Use for executable analysis, bounded frame captures, upscaler defects and graphics interoperability. Route Unreal 4 and TrueSky questions to their repository skills.
license: GPL-3.0-only
---

# Game render analysis

Read [AGENTS.md](../../../AGENTS.md) for ownership, ABI, patch and delivery rules. Commands
run from the repository root. This skill supplies a research workflow, not permission to
launch a game, modify an unrelated installation or publish a release. Follow the current
user's scope; AC7 testing in this session is user-run, without autonomous launch or termination.

## Resume from evidence

Before proposing an experiment, read the current game evidence, implementation tracker and
relevant research's latest correction. Review the conversation or a targeted transcript excerpt
when a previous attempt or acceptance is unclear. Preserve failed approaches so context loss
does not restart disproven experiments.

- [Session lessons](references/session-lessons.md): accepted AC7 mechanisms, failures and limits
  from 30 September to 2 October 2026. Read for AC7 regressions or backend refusals.
- [Methodology](../../../docs/research/methodology.md): build identification and evidence standards.
- [Tooling](../../../docs/tooling.md): Ghidra MCP, installed tooling and authorized source.
- [Research index](../../../docs/research/README.md): select the smallest relevant evidence set.
- [Implementation tracker](../../../docs/implementation.md) and `games/<id>/engine.json`:
  current status and fingerprints. Early pending entries may be historical.

For shared UE4 graph/view/UI/RHI concepts, use [unreal4-render-integration](../unreal4-render-integration/SKILL.md).
For the matched 4.18 branch and AC7 private implementation, use
[ue418-render-integration](../ue418-render-integration/SKILL.md). For cloud/depth/history work,
use [truesky-render-integration](../truesky-render-integration/SKILL.md).

## Work backward to the producer

Use a draw, dispatch, shader or final image to find the affected data. Walk callers, dependencies
and resource ownership back to the subsystem that creates the state. Determine where resolution,
precision, coverage or lifetime information is lost before choosing a hook.

Establish the engine owner, allocation descriptor and actual producer/consumers; frame/view/
renderer/history identity; allocation versus active rectangles; format, units and later writes;
and every downstream branch affected, including exposure, bloom, reflections, transparency and UI.
Ask whether the proposed change repairs production or only processes already damaged data.

A convenient D3D bind is an observation point. Native allocation, per-view setup and graph nodes
are preferable when they expose the owner's intent. Creation order, capture resource IDs, a 50%
size floor and a fixed SRV slot are not general identifiers. An AddRef preserves allocation,
not frame contents. Keep game conventions in `games/<id>` and vendor/GPU services in `runtime/`.

## Match the build and source

Fingerprint the executable before trusting offsets:

```powershell
python tools/inspect-game.py game.exe .local/fingerprint.json
```

Create the output directory first. This tool reads PE metadata, strings and hashes, not entropy.
Confirm a decrypted dump through readable instructions and import/call references. Record load
base, RVA mapping and dump hash. Keep binaries and licensed data untracked.

Search existing `references/`, vendor checkouts and research first. When a gap remains, search
source hosting for matching engine branches, middleware integrations, public projects and
existing decompilations; obey explicit requests for online research. Record origin and revision.
Use Ghidra for native code, ILSpy for managed code and shader disassembly as appropriate.
Reference source is a lead, not code to copy into this repository.

Confirm the selected Ghidra program/module before using MCP. Match control flow, calling
convention and layouts to instructions; nearby source, strings and Function ID alone do not
prove a hook. Name/save matched functions. See [Ghidra tools](../../../tools/ghidra/README.md).

When engine behavior must stop, follow the expected-byte code-patch approach after decryption.
Do not repeat console/config overrides the game can overwrite. Preflight sites for the matching
module, announce success/refusal and document restoration/lifetime. Never bypass a failing guard.

## Capture the active path

Choose existing bounded instrumentation when an external debugger cannot capture the active
upscaler. AC7 F9 already records raw colour/depth/motion, constants, shaders/bindings and native
metadata. Check enabled settings, captured images and completeness before asking the user to
map scenes. Use [the F9 procedure](../../../loader/README.md) and
`tools/analyze-ac7-motion-capture.py`. Previews can conceal precision loss; inspect raw values.

RenderDoc remains useful for supported runs and shader topology. The observed DLSS capture
restriction and Steam/Nsight attachment failures concern that setup; they are not reasons to
stop producer analysis or impose a new debugger workflow on the user.

Capture at actual use after inherited bindings and implicit unbinding. Inspect all relevant
shader stages, SRV slots, deferred/RHI work, UAVs and constants. The XML parser is a candidate
timeline, not a complete state model. Separate sessions and report drops, resets and effective settings.

## Preserve temporal, colour and execution contracts

Record motion bias/scale, direction, units, camera inclusion, jitter and unwritten encoding.
Test validity before decoding; decoded zero can be valid motion. Depth/camera reprojection
cannot invent independent object motion or geometry absent from a velocity pass. Keep diagnostic
colours out of temporal inputs.

Distinguish linear scene radiance from display HDR. SDR AC7 still uses float pre-tonemap colour.
A higher-precision copy cannot restore precision lost at production. Preserve native bloom,
exposure and tonemapping. A backend flag/preset is not proof of cause; users can override DLSS
models at driver level.

Queue copied frame/view metadata with owned resource leases. CPU graph completion is not RHI
execution, and recording completion is not GPU completion. Retire pool references on their owner.
Restore touched graphics state, including high inherited SRV slots and UAV counters. Avoid locks
across reentrant graphics calls.

For DX11/DX12 SR, use same-adapter shared resources and ordered GPU fences. CPU waits belong at
busy command-slot reuse or resource/context retirement. Failed dispatch still needs ordering
before the next upload. Check effective provider and output rather than inferring execution
from FPS. See [interop evidence](../../../docs/research/sr-interop-performance-20261002.md).

## Validate and record

State a hypothesis, its rejection condition and a bounded experiment. Update the model when
evidence disagrees. Unsupported by a default path is not impossible; FSR4's guarded INT8 route
proved the distinction. Do not relabel an internal FSR3 fallback as FSR4.

Compare settled matched scenes with SR off and multiple scales. Check static jitter, pans,
roll/FOV, cuts, independent motion, cloud/aircraft overlap, pause/resume, backend/quality switches,
resize and teardown. A clean aircraft with missing clouds is a regression.

Update research, engine evidence and tracker with question, rationale, discovery, revisions,
units, identities, lifetimes, failed attempts, runtime use and uncertainty. Separate source review,
build, synthetic/device fixtures, captured game data and user-run visual acceptance. Numeric
fixtures do not prove moving-game quality, FPS gains or latency. FG/Reflex acceptance and missing-object motion coverage require their own evidence. Current
AC7 DLSS-G and final Unity FG corrections have recorded acceptance; higher MFG, new AC7 vendor FG,
Claw and measured latency claims remain separate gaps. See [current status](../../../docs/current-status.md).

Use focused checks for documentation. Native/SDK changes require `eng/verify.ps1`; Rust behavior
also requires `cargo test --workspace --locked`. Verification must not alter a game installation.
