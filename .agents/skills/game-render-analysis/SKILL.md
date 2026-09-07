---
name: game-render-analysis
description: Investigate a shipped game's render passes, temporal inputs, camera data, and hook sites for upscaling or frame generation. Use for executable analysis, RenderDoc captures, view-buffer mapping, and engine integration research in ReScaleFrame.
license: GPL-3.0-only
---

# Game render analysis

Read [AGENTS.md](../../../AGENTS.md) for repository rules. This skill supplies rendering research guidance; it does not authorize installing mods, launching games, patching processes, or changing an unrelated installation.

Commands below run from the repository root. Read only the references needed for the question:

- [Methodology](../../../docs/research/methodology.md): build identification, capture, and evidence checks.
- [AC7 captures](../../../docs/research/ac7-frame-capture.md): measured resources, view offsets, motion, jitter, and corrections.
- [Tool setup](../../../docs/tooling.md): Ghidra MCP, Function ID databases, RenderDoc, build tools, and authorized engine source.
- [Proxy setup](../../../loader/README.md): current environment settings, hotkeys, and parser limitations.
- [Ghidra scripts](../../../tools/ghidra/README.md): type generation and COM-call analysis.
- [Validation plan](../../../docs/research/validation-plan.md): synthetic and game acceptance criteria.

## Establish the build and question

State what needs to be known and which observation would establish it. Record the executable fingerprint, renderer, and relevant reference revisions before trusting addresses.

```bash
python3 tools/inspect-game.py game.exe .local/fingerprint.json
```

Create the output directory first. This script reads PE metadata, imports, and string anchors; it does not measure entropy. Use the loader entropy helpers or a separate section analysis when needed. High entropy is a clue to compression/encryption; confirm it with section layout, references, and readable disassembly.

AC7 needed a decrypted runtime image. Its import references and decoded instructions confirmed the dump more strongly than an entropy threshold alone. Keep licensed images and machine paths untracked.

## Use source and signatures as leads

Compare distinctive source call sequences with the shipped build. `map-source-files.py` and `find-string-refs.py` narrow the search, but byte-scan hits and inlined assertions do not establish function boundaries.

Ghidra MCP can inspect the selected program. Confirm its identity before calling analysis or mutation tools. Function ID databases match compiled functions; `.gdt` archives describe types. Record the database/compiler/architecture and inspect ambiguous matches. Neither kind of data validates an AC7 runtime hook by itself.

## Find resources at their actual use

Use creation hooks for allocation facts and binding/draw inspection for resource use. Select from the complete binding state at a draw, not while the engine is updating one slot. Track all relevant slots and contexts, including inherited bindings, implicit unbinding, and compute work.

`parse-capture.py` provides a candidate timeline from RenderDoc XML. It does not yet model all those state changes, so verify its read lists against the capture. Formats and sizes identify candidates; inspect shaders, contents, rectangles, and frame/view identity before assigning a role.

Do not use capture resource IDs as runtime identifiers. Do not make a fixed 50% size floor or a slot-zero colour assumption part of a general matcher. AC7 binds normals in slot zero; its half-resolution two-channel mask was initially mistaken for velocity.

## Capture matching view data

During discovery, use a buffer-size range and histogram. AC7's view allocation is 4096 bytes, and the same allocation can serve perspective and orthographic views. Creation order does not select the camera.

Read constants at the relevant use, or capture matching CPU data when the engine constructs/uploads them. Carry render-frame/view identity with the resources.

Use stock source for layout candidates and validate relationships across multiple scenes:

- Projection/inverse products, aspect/FOV, and finite values.
- Camera basis length/orthogonality and agreement with view-to-world.
- Current/previous transforms including translated-world origin changes.
- View rectangles, buffer dimensions, and reciprocals.
- Jitter agreement with projection entries and the current/previous sequence.

`ue4-view-layout.py`, `analyze-view-buffers.py`, and `verify-view-layout.py` support these checks. Their checks are not infallible; non-finite-input gaps are recorded in the review. Separate capture sessions before differencing them. AC7's reset capture indices once combined old and new runs and hid the jitter change.

## Establish temporal conventions

Read producer and consumer shaders. Record storage bias/scale, direction, units, sentinel, camera coverage, jitter inclusion, and dilation. Source constants need capture validation; small observed movement may not distinguish a correct scale from a plausible wrong one.

AC7's sparse velocity needs decoding before any backend scale is applied. Test raw unwritten values before decoding, then preserve a separate invalid sentinel: decoded zero can be valid motion. Keep diagnostic marker colours separate from motion channels.

Use the engine's camera transform. Depth converts it into per-pixel camera displacement; it cannot recover independent object motion. Determine whether written vectors replace camera motion or add an object contribution before combining them. Inspect engine consumers for a reusable full-motion output, and check whether the loaded backend already performs the needed resolve.

Prefer enabling a suitable engine path over duplicating it. AC7's jitter gate allows the engine to update projection and derived state together. Require the expected build and patch bytes; verify history/reset effects after the change. This is not a guarantee that every engine gate is safe to bypass.

## Choose the consumption and insertion boundaries

Trace each input's producer, later writes, and last use. Retaining a resource preserves its allocation, not its contents. Copy before reuse when required and keep constants/resources on the same frame.

AC7's initial resource match precedes later sky/cloud draws. Present-time evaluation includes them for the debug path, but proper reinsertion needs an earlier verified boundary. Do not generalize Present as the right evaluation point for every game.

Capture full and reduced render scale. Equal dimensions concealed AC7's reduced-resolution HUD composite. Check downstream allocations, grading, exposure, spatial scaling, cloud history, refraction, and UI. Verify effective scale after mission changes; the current timer is interim glue.

## Diagnose and validate

Split counters by failure stage: hook calls, resource matches, view reads, secondary views, missing jitter, evaluations, and refusals. Log near-miss bindings when counters cannot explain a failure. Report after frames have run, keep repeated logs bounded, and trigger per-frame work by frame identity rather than texture identity alone.

Keep immediate-context work on its owning thread and avoid locks across reentrant graphics calls. Test rollback and quiescent teardown. Clear newly allocated debug output as needed, but treat unwritten regions as an extent/evaluation problem to investigate.

Use matched still images for detail and live sequences for temporal defects. A static world with an orbiting camera mainly exercises camera reprojection. Add independent object motion, stationary-camera jitter, pans, roll/FOV changes, cuts, transparency, and resource replacement. Label warm-up, capture gaps, refused frames, and tooling overhead.

Use the existing egui interface for inspection/capture as it becomes available. Keep diagnostic rendering out of scene inputs and temporal history.

## Publish the result

Record why the approach was chosen, how the result was found, supporting evidence, where and when the code uses it, failed approaches, and remaining uncertainty. Update research notes, game evidence, and the todo. Each commit/PR includes the research or a link; documentation-only work records its rationale and checks.

Distinguish source-inspected, built, synthetic-tested, capture-validated, game-tested, and target-device-tested results. A Wine run is not an MSVC/Windows pass. A higher presentation count is not measured latency or handheld performance.
