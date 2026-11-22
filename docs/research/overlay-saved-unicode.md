# Saved Unicode overlay options

This source follow-up starts at `d8415c47` on
`NightHammer1000/overlay-unicode`. Earlier O02 branches remain available.

## Contract and native boundaries

`cba43eee` adds `overlay_saved_options.h`, an independent size-checked
registration contract with ABI 1. Overlay service v4 and its existing numeric
and toggle producer records keep their layout and entry points. The new
registration supports a session-wide bank and banks scoped to a native
render host's `services.user`. The caller owns the meaning, INI section and
keys. The Runtime copies descriptors, owns saved values and keeps callbacks
borrowed until unregister succeeds. Restart rows save without applying a live
callback. A live callback sees the persisted value and can refuse without
mutating live state. On refusal, the store restores the earlier value or
removes a newly created key. A failed restoration is reported in the row.

Panel ABI 20 appends the row pointer, row count and saved-option intent.
Existing panel record member offsets are preserved. Deploy its Runtime and
panel DLL together. The panel offers **Paste and save** and **Clear and save**
for text using the Windows Unicode clipboard. The existing input capture
and routing do not change. A new saved text edit accepts up to 4096 UTF-16
units of single-line text. Valid supplementary characters stay intact.
The shared profile codec encodes saved text as ASCII UTF-16 units so Windows
can write it through an existing ANSI INI. Existing unencoded values remain
readable. The numeric and toggle values keep their ordinary INI representation.

Runtime exports are added through the existing native export declarations.
There is no replacement renderer DLL or new C++ policy. D08's exact-backbuffer
and independent input-HWND files, GPU submissions, provider selection and
pacing policy are untouched. The saved callback is dispatched from the owner
after a successful overlay draw, using the same frame's row-to-owner snapshot.

## Finding and integration handoffs

| Finding | Source handoff and disposition |
| --- | --- |
| C02, **Give the render host's INI-only diagnostic keys live overlay controls.** | `cba43eee` keeps the existing editable probe and restart-labelled GraphicsDiagnostics controls. It replaces the capture toggle with a live Unicode `SRCaptureDirectory` row in the render host's supplied section. The callback re-arms SR capture and updates the installed FG capture owner. Clearing disables both paths; a refused live update keeps the previous state. The native render host setup remains version 1. |
| R01, **Give the RDR2 startup and diagnostic INI keys saved overlay controls.** | `38c1cf4a` registers 18 descriptors after `rsf_render_host_create` succeeds and before plugin preparation. The native host resolves that exact `services.user` bank on its normal pre-Present overlay path. RDR2 owns the `[RDR2]` keys, bounds and restart labels. Cold ASI and explicit attach readers decode saved text. A cold `Enable=0` still disables the renderer, so re-enabling that configuration requires editing the INI. |
| U01, **Expose the Unity Mono graphics diagnostics in the shared overlay.** | `515e4344` registers the `[UnitySR]` rows after callback-host start. GraphicsDebug is restart-only. FGCaptureDirectory applies live through `rsf_fg12_capture_outputs` after the existing Unity path resolver handles absolute and relative paths. The callback is unregistered before stop can unload it. The callback host's setup ABI is unchanged. |
| V06, **Give environment-only diagnostic options overlay controls.** | `cba43eee` adds seven Runtime-owned restart rows in `%LOCALAPPDATA%\ReScaleFrame\Diagnostics.ini`, section `[Diagnostics]`. The shared Vulkan and D3D12 capture readers, the Runtime and standalone XeSS-FG capture owner, and Vulkan timing read those keys at their existing initialization points. An explicit environment variable still takes precedence, and its row identifies the override. The first-frame widget accepts 0 through `INT32_MAX`; environment parsing retains the existing wider range. The standalone bridge reads this profile and has no panel of its own. `a00408e2` adds AC7's pixel diagnostics to its existing restart option catalog and reads `[Startup]` in the adapter's own `AC7.ini`, with its existing environment parser as fallback. |

## Source verification

The six affected generated files were reproduced with bindgen 0.72.1 and
pinned Rust 1.94.1 formatting. Layout assertions cover the v4 contract and the
panel ABI 20 appended fields. Source fixtures cover a supplementary-character
path in an existing ASCII profile and rollback after a refused live edit.
Locked offline Cargo metadata and changed-source whitespace checks passed.

Builds, tests, lint, device execution and gameplay checks were outside this
dispatch. The WARP and device fixtures, native RDR2 panel reachability,
Unity's live FG path and a standalone XeSS-FG restart still need execution
before they can be called verified. `PORT.md` remains coordinator-owned.
