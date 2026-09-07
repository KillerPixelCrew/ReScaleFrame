# Analysing a game's renderer

Start with a question and record the evidence needed to answer it. AC7's history includes useful failures; the procedure below keeps those lessons without treating one game's behaviour as a universal rule.

## 1. Establish the build and readable data

Fingerprint the executable before using offsets:

```bash
python3 tools/inspect-game.py game.exe .local/fingerprint.json
```

This script records PE metadata, imports, strings, and SHA-256. **It does not measure entropy.** Use the loader's entropy helpers or a separate section-entropy analysis for that check.

High entropy is a packing/encryption clue, not proof by itself. Inspect section layout, entry point, imports, and plausible references. AC7's shipped `.text` was near 8 bits/byte with no useful graphics-import references; a decrypted runtime image was needed. Plaintext data still supplied strings and interface IDs.

For a runtime dump, record the actual load base and map file offsets to RVAs. Verify readable code through disassembly and import references as well as entropy. AC7's dump contained 19,888 import-call/jump references where the shipped image had none. Keep dumps untracked.

## 2. Turn source leads into verified locations

Use embedded source paths and console-variable references to narrow the search. `map-source-files.py` and `find-string-refs.py` produce candidates; byte scans and inlined assertions do not prove function boundaries or source ownership.

Compare several distinctive call sequences against an authorized engine checkout. Validate the actual shipped build before using any address. Source from a nearby engine version explains behaviour but does not establish game offsets.

## 3. Capture allocations, bindings, and contents

Use RenderDoc to inspect the frame. The recorded Proton workflow loads the Windows capture DLL and converts its structured stream with Windows `renderdoccmd` under Wine. Pixel replay used Windows. See [capture setup](../../loader/README.md).

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

Use engine camera transforms directly. Depth turns that transform into per-pixel displacement; it cannot recover independent object movement. Determine whether written velocity replaces or adds to camera motion before combining them. Check what the loaded backend already resolves before adding another pass.

## 6. Choose when to consume each resource

Identify producers, later writes, and last use. An AddRef keeps a texture allocated but does not preserve its pixels. Copy before reuse when evaluation cannot consume immediately.

AC7's first qualifying binding precedes later sky draws. Evaluating there omitted the sky; Present-time evaluation fixed the diagnostic image. Proper SR reinsertion still needs an earlier, verified scene boundary and compatible downstream allocations.

Capture both full and reduced render scale. Equal dimensions concealed that AC7 also reduced its HUD composite. Check grading, transparency, refraction, spatial scaling, and UI separately.

## 7. Validate and publish

Test stationary-camera jitter, camera pans, independent object motion, combined motion, cuts, and resource changes. Compare matched input/output dimensions and settled temporal histories. Use live footage for temporal defects and numeric captures for convention checks.

Keep context work on its owning thread. Avoid locks across reentrant graphics calls. Bound logs, GPU readbacks, memory, and file output. Report refused frames, capture gaps, reset reasons, and actual effective settings.

For each result, record why the approach was used, how it was found, what disproved earlier assumptions, and where/when the runtime uses it. Update game evidence and the tracker. Distinguish source-inspected, built, synthetic-tested, capture-validated, game-tested, and target-device-tested work.
