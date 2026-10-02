# Analysing a game's renderer

Start with a question and record the evidence needed to answer it. AC7's history includes useful failures; the procedure below keeps those lessons without treating one game's behaviour as a universal rule.

Before a new experiment, check the latest implementation status and relevant transcript corrections.
The accepted AC7 native path now owns view/target allocation, SR graph insertion and queued
lifetimes. Earlier texture matching and Present-time evaluation describe discovery/compatibility
paths. See [the session update](skills-session-update-20261002.md) for the maintained skills.

## 1. Establish the build and readable data

Fingerprint the executable before using offsets:

```bash
python3 tools/inspect-game.py game.exe .local/fingerprint.json
```

This script records PE metadata, imports, strings, and SHA-256. **It does not measure entropy.** Use the loader's entropy helpers or a separate section-entropy analysis for that check.

High entropy is a packing/encryption clue, not proof by itself. Inspect section layout, entry point, imports, and plausible references. AC7's shipped `.text` was near 8 bits/byte with no useful graphics-import references; a decrypted runtime image was needed. Plaintext data still supplied strings and interface IDs.

For a runtime dump, record the actual load base and map file offsets to RVAs. Verify readable code through disassembly and import references as well as entropy. AC7's dump contained 19,888 import-call/jump references where the shipped image had none. Keep dumps untracked.

## 2. Turn source leads into verified locations

Search existing local references first, then source hosting for engine branches, middleware/plugin
integrations, public game projects and existing decompilations. Useful source often lives in a
larger project's checkout. Record its origin, revision and engine version, and compare it with
the installed binary before trusting layouts or behaviour. Keep reference material untracked.

Use decompilers as ordinary research tools alongside source: ILSpy for managed assemblies,
Ghidra for native code, and shader disassembly/decompilation for graphics programs. Decompiled
types and names are hypotheses; verify calling conventions, fields and producer ownership against
instructions and runtime evidence. Source availability is a lead, not a substitute for matching.

Use embedded source paths and console-variable references to narrow the search. `map-source-files.py` and `find-string-refs.py` produce candidates; byte scans and inlined assertions do not prove function boundaries or source ownership.

Compare several distinctive call sequences against an authorized engine checkout. Validate the actual shipped build before using any address. Source from a nearby engine version explains behaviour but does not establish game offsets.

## 3. Capture allocations, bindings, and contents

Use RenderDoc where it can capture the active path. The recorded Proton workflow loads the
Windows capture DLL and converts its stream with Windows `renderdoccmd` under Wine; pixel replay
used Windows. The later Windows DLSS run could not use that capture route. Bounded F9 native
captures provide raw colour/depth/motion, shader/binding and view/graph evidence without requiring
a frame debugger. Check the existing images/metadata before asking the user to identify scenes.
See [capture setup](../../loader/README.md).

Formats and sizes narrow resource candidates. At each relevant draw, inspect complete bindings, shader identity, valid rectangles, and pixel contents. The current XML parser does not fully model inherited SRVs, compute work, or deferred contexts, so its pass/read lists require confirmation.

Creation hooks describe allocations; binding/draw hooks describe their use. Both matter. Capture IDs are local identifiers, not runtime signatures. AC7's half-resolution mask was misidentified as motion from format/binding alone.

## 4. Associate engine data with the rendered view

Read the view constants at the pass that uses them, or capture matching CPU data when the engine builds/uploads them. Creation order and allocation size alone do not select the main camera.

Use a size histogram during discovery: AC7's view data occupies a 4096-byte buffer bucket. Check perspective versus orthographic views, viewport rectangles, render-frame/view identity, and resource lifetime.

Validate candidate offsets through relationships across multiple scenes:

| Field | Check |
| --- | --- |
| Projection / inverse | Their product is identity; aspect/FOV match the view |
| Camera basis | Unit length, orthogonality, agreement with view-to-world |
| Previous-frame transform | Agreement with current/previous transforms and origin shift |
| Sizes | Actual dimensions and reciprocal fields |
| Jitter | Projection agreement, pixel scale, current/previous sequence |

Use `ue4-view-layout.py` for stock-layout candidates, `analyze-view-buffers.py` for variation, and `verify-view-layout.py` for AC7 relationships. Reject non-finite values explicitly. Keep separate capture sessions separate; overwritten filenames previously hid the jitter change.

## 5. Verify temporal conventions

Read the producer and consumer shaders. Record storage bias/scale, direction, units, sentinel, camera coverage, jitter, and dilation. Small captured values could not distinguish AC7's correct velocity scale from an earlier wrong one.

Prefer enabling an existing engine path when it supplies the missing data. AC7's AA-gate patch lets the engine generate jitter before deriving matrices. Require a recognized build and expected patch bytes, and verify reset behaviour afterward.

Follow the expected-byte code-patch approach when the game's native policy must change. Console
or configuration values that the game resets are not stable producer ownership. Preserve a
logged refusal and native fallback when a fingerprint/site differs; never weaken the guard.

Check raw velocity validity before decoding. A valid decoded zero is not an unwritten vector.
Shader substitution cannot create geometry excluded from the pass or supply missing previous bones.

Use engine camera transforms directly. Depth turns that transform into per-pixel displacement; it cannot recover independent object movement. Determine whether written velocity replaces or adds to camera motion before combining them. Check what the loaded backend already resolves before adding another pass.

## 6. Choose when to consume each resource

Identify producers, later writes, and last use. An AddRef keeps a texture allocated but does not preserve its pixels. Copy before reuse when evaluation cannot consume immediately.

AC7's early qualifying binding preceded later sky draws. Present-time evaluation repaired the
diagnostic image, but the accepted integration inserts an owned SR/fallback graph node before
native tonemapping, with matched bloom/exposure and reviewed downstream dimensions. Do not
repeat Present rediscovery as the production architecture. See [the native refactor](ac7-native-renderer-refactor-20261001.md).

Join native CPU recorders before shared view changes. Saving a uniform by moving it cleared a
shared slot and caused the hangar worker crash. Copy/AddRef and publish from a local source;
retain generated buffers/pool leases through queued RHI use and retire on the native owner.

Capture both full and reduced render scale. Equal dimensions concealed that AC7 also reduced its HUD composite. Check grading, transparency, refraction, spatial scaling, and UI separately.

Fix resolution and precision at their producer. UI keeps logical geometry while physical raster
targets follow output size. TrueSky cloud/depth/history dimensions and effect permutations must
agree; x1 allocation arithmetic alone did not provide an x1 depth shader. UNORM depth bounds
lost aircraft precision before masking, and an array-UAV guard wrongly rejected the actual 2D
view. Verify real bindings/dispatch, not shader compilation. A clean plane with absent clouds fails.

SDR display output still consumes linear floating-point radiance before tone mapping. A higher
precision copy cannot restore quantized scene values. Keep vendor colour/exposure observations
separate from a claim about proprietary internals. For FSR/XeSS transfers, GPU fence ordering
avoids current-frame CPU stalls; fixtures and FPS differences are different evidence. Default
FSR4 refusal was superseded by guarded, device-scoped INT8 execution, proven separately from FSR3.

## 7. Validate and publish

Test stationary-camera jitter, camera pans, independent object motion, combined motion, cuts, and resource changes. Compare matched input/output dimensions and settled temporal histories. Use live footage for temporal defects and numeric captures for convention checks.

Keep context work on its owning thread. Avoid locks across reentrant graphics calls. Bound logs, GPU readbacks, memory, and file output. Report refused frames, capture gaps, reset reasons, and actual effective settings.

For each result, record why the approach was used, how it was found, what disproved earlier assumptions, and where/when the runtime uses it. Update game evidence and the tracker. Distinguish source-inspected, built, synthetic-tested, capture-validated, game-tested, and target-device-tested work.
