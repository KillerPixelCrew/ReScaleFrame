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

## Reverse engineering and agentic debugging: walk back to the roots

Do not confuse the first observable point with the correct intervention point.

This rule applies during **initial reverse engineering and hook-site selection**, not only after a defect has already been reproduced. RenderDoc, API traces, logs, and graphics hooks often reveal a convenient pass, layer, render target, draw, dispatch, or composition stage. Treat that as an observation point and a lead. It may be a leaf or branch even when nothing is obviously broken yet.

For every candidate tap point, walk backwards through ownership and production until you understand where the state is created. Ask:

- What engine subsystem owns this resource, value, or draw?
- Which function or object creates the state that reaches this point?
- What upstream inputs determine its resolution, lifetime, timing, and contents?
- Is this pass merely consuming or composing information created earlier?
- Has useful information already been rasterized, quantized, downscaled, merged, or discarded by this point?
- Can an earlier engine-level function expose the same intent more directly and with fewer heuristics?
- Is the proposed hook stable because it represents an engine concept, or merely convenient because it is visible at a graphics/API boundary?

The general rule is:

> Find the leaf with RenderDoc. Walk the tree back to the roots. Tap the roots, not the leaves or branches.

RenderDoc is excellent at locating **where things land** and reconstructing the render graph. Use it to identify consumers and composition order, then use Ghidra, source, Function ID/signature libraries, runtime instrumentation, and call tracing to walk producer chains back toward the engine logic that created those resources or decisions.

Do not stop reverse engineering merely because a workable graphics-level hook exists. Prefer the deepest practical engine-level point that still represents the semantic intent you need. The lower you tap into the engine's own logic, the more freedom you usually retain: resolution, composition order, resource creation, transforms, timing, lifetime, and data representation may still be variables instead of already-fixed constraints.

Late hooks are narrower by construction. By the time data reaches a final graphics/API boundary, the engine may already have rasterized, merged, clipped, downscaled, transformed, or discarded information. At that point you are adapting to decisions that have already been made. A deeper tap can let ReScaleFrame influence those decisions before they become irreversible.

This does **not** mean “always hook the earliest function possible.” The target is the earliest stable ownership boundary that still exposes the concept cleanly and can be validated. A later hook may still be the correct place for copying, synchronization, final composition, or API interop. What matters is choosing the tap because it owns the desired behavior, not because it was the first place tooling made it visible.

Examples:

- A low-resolution UI is not fundamentally a final-composition problem. If UI elements are rasterized low and only then composed, tapping the final texture just magnifies missing detail. Trace back to the UI element render/composition path, preserve element state and positions, render or retain them at output resolution, upscale the scene separately, then composite the UI afterward.
- A briefing map may appear as one blurry final layer, while the actual root is a secondary scene/render target configured to an internal percentage before global render scale is applied. The useful tap is where that scene's resolution is chosen, not where its texture is later blended.
- A motion-vector texture visible in RenderDoc may only be the consumer-facing representation. Trace back to the engine camera/object-motion logic and the shader or resolve path that produces it before deciding where ReScaleFrame should intervene.
- A temporal artifact visible at Present may originate several passes earlier in camera state, history reset, transparency composition, or resource reuse. Present is an observation point, not proof that Present is the right hook.

If the downstream output no longer contains the information needed for a clean implementation, moving farther downstream cannot recover it. Move upstream until the information still exists and the engine's intent is still represented explicitly.

## Agentic debugging: hypothesis before implementation

Hard reverse-engineering work is a hypothesis loop, not a prompt-and-pray loop.

1. Describe the failure or unknown precisely.
2. Propose a causal explanation or architecture hypothesis.
3. State what observation would prove or disprove it.
4. Gather that evidence with the appropriate tool.
5. Update the model of the engine/render path.
6. Select or revise the tap point from that model.
7. Change the implementation only after the model improves.
8. Repeat until the mechanism is explained rather than merely hidden.

Do not accept `unsupported`, `not documented`, or `could not find` as synonyms for `impossible`. Distinguish:

- not yet understood,
- not exposed by current tooling,
- not supported by the obvious API/path,
- blocked by a verified technical constraint,
- actually impossible.

Only the last two justify stopping, and both require evidence.

When a familiar approach stalls, change the problem framing rather than repeating the same search harder:

- static analysis fails -> inspect or dump the decrypted runtime image,
- string matching is weak -> build Function ID/signature libraries from matching engine/SDK source,
- allocation-time identity is ambiguous -> inspect actual resource use in RenderDoc,
- a graphics-level tap is convenient but semantically late -> trace calls and ownership back to the engine function that creates the state,
- final composition is already damaged -> intercept before composition/rasterization,
- reconstruction is fragile -> look for the engine-native source value or path,
- repeated manual analysis dominates -> build instrumentation or tooling that makes the state observable.

Agents tend to reach for common solution patterns first. Use domain knowledge to propose less obvious but testable hypotheses, then use Ghidra, RenderDoc, runtime captures, signatures, source builds, instrumentation, and synthetic harnesses to prove or reject them.

Record failed hypotheses and why they failed. Do not preserve only the final answer; long reverse-engineering tasks need the reasoning trail so later agents do not repeat disproven paths after context loss or compaction.

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

When a render/API observation identifies a candidate function or resource, use references, callers, owners, source analogues, and matched functions to walk upward through the producer chain until the responsible engine concept is understood. Do not treat the first matching D3D call, shader, resource bind, or composition pass as the end of reverse engineering.

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
