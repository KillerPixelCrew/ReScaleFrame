# Repository review: 7 September 2026

Reviewed code at `a9f3d5d` (the user's README-instructions commit; runtime code is unchanged from `a65e69a`). Scope: loader/proxy, diagnostics, AC7 plugin and view reader, graphics helpers, DLSS adapter/orchestrator, Rust input model and egui/FFI, analysis tools, native tests, build/CI, and documentation.

The diagnostic DLSS path has recorded in-game results. It is not yet a complete plugin-driven integration: reinsertion, UI wiring, lifecycle, and target-device validation remain open. This change rewrites documentation and the shared agent skill; it does not fix runtime code.

P1 means a correctness/lifecycle issue to address before expanding integration. P2 means a functional defect or a material verification gap. Findings below are source-confirmed unless a reproduction is stated; crashes and game-specific visual consequences were not reproduced in this environment.

## Findings

### 1. P1: F8 startup uses the game's immediate context from a worker

[`observe_worker`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/loader/proxy/src/dinput8_proxy.c#L522-L525) calls `start_dlss`, which reaches `rsf_dlss_pipeline_start`. Startup obtains the game's immediate context and calls [`ClearRenderTargetView`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/runtime/orchestrator/src/dlss_pipeline.cpp#L348-L355) while the game can still be rendering. No synchronization with the game's context owner is established. F7 also publishes plain bridge state across the worker/render boundary.

Concurrent context use can corrupt command submission or cause intermittent failures. D3D11 device object creation is a different case; [Microsoft requires synchronized access to a shared context](https://learn.microsoft.com/en-us/windows/win32/direct3d11/overviews-direct3d-11-render-multi-thread-intro).

Move start/toggle/stop intents to a bounded queue consumed at a render-thread boundary. Verify repeated startup/toggles while rendering and during mission transitions, with thread identity logged around context operations.

### 2. P1: Invalid matrices pass the AC7 view reader

[`invert`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/games/ac7/src/view_uniforms.cpp#L56-L116) tests pivot magnitude without checking finiteness. NaN comparisons bypass the singularity test, inversion returns success, and [`rsf_ac7_view_read`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/games/ac7/src/view_uniforms.cpp#L207-L212) accepts the result. Other matrix/jitter comparisons have similar gaps; frame assembly does not provide a complete numeric validation boundary.

Reproduced locally: build a valid synthetic view using the existing test fixture, replace the first float at `0x6E0` with `quiet_NaN()`, and call the reader. It returns `RSF_AC7_VIEW_OK` with a non-finite inverse. The existing view tests still pass. The Python layout helper also returns no failures for an all-NaN input, although its CLI's perspective filter is a separate step.

Reject non-finite inputs and outputs before inversion/submission, validate jitter/rectangles, and add poisoned-input cases. The failure should be an explicit invalid-camera refusal and a history reset when continuity is lost.

### 3. P1: Observer rollback and teardown leave unsafe hook states

If CreateTexture2D and CreateBuffer hooks install but the Present patch fails, [`rsf_observer_install`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/runtime/graphics/src/d3d11_observer.cpp#L538-L553) restores only CreateTexture2D. CreateBuffer remains hooked while `installed` stays false; uninstall then refuses to clean it up. Retrying can record the hook itself as the original function, leading to recursion.

Separately, uninstall clears `original_create_buffer` without first draining callbacks, while the hook calls that pointer before acquiring the observer mutex. An in-flight callback can therefore observe a cleared forwarding pointer.

Make installation transactional and rollback every successful patch in reverse order. Stop admission and drain callbacks before clearing forwarding pointers or resources. Inject failure at each patch step and exercise retry/uninstall with concurrent buffer creation.

### 4. P2: The frame tap excludes its RGBA16F colour candidates

[`resource_roles.cpp`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/runtime/graphics/src/resource_roles.cpp#L149-L154) classifies render-size RGBA16F SRV/RTV textures as `RSF_ROLE_SCENE_COLOR`. The tap first puts one into the history candidate, then its [colour-selection loop](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/runtime/graphics/src/frame_tap.cpp#L319-L332) skips every role except `UNKNOWN`. That makes the advertised RGBA16F colour path unreachable for a normally classified target. R11G11B10 survives because the classifier leaves it unknown.

Select colour/history through verified pass semantics, and allow the supported colour role in the interim. Test both formats together with multiple RGBA16F/history candidates; the classifier-only tests do not exercise this interaction.

### 5. P2: Numeric settings reject valid zero values

> Status 7 September 2026: the parser now reads with `strtoul` base 0 and honours a present-and-zero
> value; the `config_parse` test that closes this lands with the representation plan's M5.

[`read_number`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/loader/proxy/src/dinput8_proxy.c#L125-L133) uses decimal `strtol` and accepts only values greater than zero. Consequently, `RSF_DLSS_QUALITY=0` falls back to Performance (`3`), and `RSF_DECODE_MOTION=0` falls back to enabled (`1`). Hexadecimal RVA overrides also fall back instead of selecting the requested address.

Use typed parsing with explicit ranges and full-string/overflow checks. Zero should be valid for native quality and booleans. Keep desired quality and the engine's effective render size consistent; F8 currently applies the independent screen-percentage setting before backend planning.

### 6. P2: F7 can display stale output after a missed/refused frame

[`on_present`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/loader/proxy/src/dlss_bridge.c#L295-L308) checks whether any evaluation has ever succeeded, not whether this frame succeeded. If no main view qualifies, or evaluation fails, the prior output can still cover the game's current back buffer. This can hide a menu/loading image or show a frozen flight frame.

Track success and output generation per frame; skip the blit when current output is invalid. Reset temporal history after gaps. Verify a successful frame followed by no match, refusal, and resource replacement.

### 7. P2: The egui input path loses clicks between frames

[`record_button`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/runtime/graphics/src/overlay_input.cpp#L147-L159) stores only the final button mask. Collection copies that mask, and Rust infers edges by comparing frame samples. A down/up pair between two collections leaves no edge for egui. This is especially relevant during slow frames or synchronous captures.

Queue bounded button events with position/order, or extend the ABI with preserved transitions. Test press and release between collections, plus focus/visibility changes and overflow. Existing Rust tests feed frame-separated input and do not cover the Win32 handoff.

### 8. P2: CI skips Rust execution and the live SDK implementation

> Status 7 September 2026: open. Planned as a two-leg vendor-header matrix plus `cargo test` in the
> representation plan's build section.

[`eng/verify.ps1`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/eng/verify.ps1#L22-L26) runs CTest, formatting, and Clippy; it never runs `cargo test`. The [workflow](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/.github/workflows/verify.yml) also does not provide Streamline headers. A clean CI checkout therefore compiles the unavailable stub, leaving the SDK-enabled C++ branch unchecked.

Add Rust test execution and a compile job with pinned Streamline headers. Keep actual GPU evaluation as a separately reported gate. Add integration coverage for the frame tap, startup/teardown, present blit, and C++→Rust overlay boundary as those paths are wired.

### 9. P2: Capture read lists omit inherited SRV bindings

[`build_timeline`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/tools/parse-capture.py#L89-L110) starts a fresh read set at each output-binding change and only records SRV changes while that pass is current. A texture bound before the output change remains available to the draw but disappears from the reported reads. The tool can therefore misidentify which pass consumes motion/history; compute-only and deferred-context work are also incompletely represented.

Reproduced with a small XML stream: create SRV #7 for texture #42, bind it, change the render target, then draw. The parser reports an empty read set. Track state per context/stage and sample it at draw/dispatch, or report the analysis limits explicitly. The rewritten guides now do the latter.

### 10. P2: Discovery rejects render sizes below half output

The tap leaves render dimensions unknown and [`at_render_resolution`](https://github.com/KillerPixelCrew/ReScaleFrame/blob/a9f3d5d/runtime/graphics/src/resource_roles.cpp#L79-L82) rejects inputs smaller than half the output on either axis. A roughly one-third-size Ultra Performance input therefore cannot be discovered even if the engine and backend accept it. Selecting an exposed quality level does not remove this floor.

Pass verified render dimensions or a validated backend range into discovery. Test actual SDK-planned extents, alignment rounding, and secondary-view rejection. Do not broaden the heuristic blindly.

## Integration questions still requiring evidence

These are investigations, not additional confirmed defects:

- Reconcile live motion scales/signs with the Rust model and the loaded Streamline contract. Verify stationary-camera jitter and written-vector camera inclusion before changing factors.
- Check graphics state before/after the complete SDK call, decode, blit, and overlay sequence. ReScaleFrame has no enclosing SDK state guard, but that alone does not prove which D3D11 state the loaded vendor path changes. [Streamline's guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuide.md) assigns host restoration responsibility; validate the exact version/API path.
- Replace global binding heuristics with explicit pass/view/frame identity. Verify secondary/deferred contexts, implicit unbinding, and input contents retained until Present.
- Verify texture-dump context ownership and multisample staging behaviour. Those paths are not covered by the portable checks here.
- Complete reinsertion, exposure convention, camera-cut/reset propagation, and output-resolution HUD composition before FG expansion.

## Checks performed

- Built and ran the existing entropy test with Linux GCC and the existing AC7 view test with Linux G++, both with warnings as errors. Both passed. These are portable logic tests, not a Windows build.
- Ran the focused C++ NaN reproduction and the Python inherited-SRV reproduction described above.
- Parsed all ten Python tools successfully.
- Checked documentation links, todo preservation, skill metadata/links, whitespace, and the diff before committing.

The environment lacks Rust, CMake, PowerShell, MinGW/Wine, vendor runtimes, game captures, and a Windows graphics test environment. The full build, Rust tests, GPU evaluation, and game/Claw validation were not run. Historical game results in the docs remain attributed to the recorded sessions.

## Documentation changes

Corrected contradictory game metadata about velocity consumers and evidence strength. Replaced stale scaffold status with the current diagnostic DLSS status; corrected settings defaults/limitations; consolidated capture corrections and matrix evidence; shortened component guides, architecture, and validation prose. Preserved all existing unchecked motion, engine, and egui tasks, then added the ten review follow-ups.

Merged the user's README instructions from `a9f3d5d`, completed the research/tool guidance, linked Ghidra MCP and Function ID databases, and moved the rendering skill to the shared Agent Skills layout. Its workflow now distinguishes creation from use, candidate pass matching from proof, and entropy measurement from PE inspection. These changes follow the implementation and evidence reviewed above; no new game experiment is claimed.
