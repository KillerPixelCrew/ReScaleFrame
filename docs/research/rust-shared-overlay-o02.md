# Shared overlay rendering and panel source port

O02 began with O01 input and hosting at `dd5b4375`. The branch merged C02
`bb08cfea` as `cd487337` and the combined D08 base `266b2082` as
`cd4a493e`. The latter contains D08 `d941e44c` and the Minecraft extension
`d47d77a4`.

D3D11 shader compilation, descriptors, atlas updates, indexed draw and
context restoration are in Rust. The shader source and shader model match
the previous implementation. The target, depth view and UAV bindings are
kept throughout the pass. C++ contains exceptions from foreign log sinks.

Rust owns the native D3D12 renderer references, frame slots, and failed
owner retirement. Retirement resubmits a missing signal to the retained
original queue and keeps the renderer until the fence, armed event and input
subclass can retire. The replacement starts immediately afterward. Retrying
the same failing owner waits one second.

D3D11-on-12 has one host admission decision before wrapping. The explicit
D08 backbuffer still uses PRESENT state and its independent HWND installs
input. The legacy chain path retains RENDER_TARGET state. Both release and
flush their wrappers before returning.

Panel ABI 19 appends fields and keeps the previous offsets. Deploy a matching
Runtime and panel DLL. The Runtime service table remains ABI 4. VRR is stored
as the reserved value `2147483647` in `FrameLimitUs`. Fixed values remain
microsecond intervals. The pacing owner samples the presenting display at
up to 500 ms intervals and uses active effective generation to resolve the
application interval. With no refresh information the policy stays selected
and the pacer receives no limit. Reflex's per-output interval uses the same
display sample. AC7 and the callback host persist through the shared profile
store.

## Port files and fixtures

| PORT path | Source disposition |
| --- | --- |
| `runtime/graphics/include/rescaleframe/overlay_renderer.h` | Retained the C renderer ABI. The five renderer entries are implemented in Rust. |
| `runtime/graphics/src/overlay_renderer.cpp` | Ported D3D11 pipeline creation, resources and draws. Retained the exception-contained log callback. |
| `runtime/graphics/src/overlay_renderer_platform.h` | Removed the superseded D3D11 renderer leaf table. |
| `runtime/orchestrator/include/rescaleframe/overlay_api.h` | Retained Runtime overlay API 4 and documented the required adapter edit callback. |
| `runtime/orchestrator/include/rescaleframe/overlay_d3d12.h` | Retained the native, legacy wrapped and D08 exact-backbuffer/explicit HWND interfaces. |
| `runtime/orchestrator/include/rescaleframe/overlay_fg_stats.h` | Rust fills FG stats and latency telemetry. The C wrapper supplies the native latency getter. |
| `runtime/orchestrator/src/overlay_d3d12.cpp` | Retained D3D11-on-12 SDK leaves. Rust admits wrapping from the D3D11 host's draw decision. |
| `runtime/orchestrator/src/overlay_native12.cpp` | Rust owns lifecycle, queue recovery and COM references. Retained D3D12 shader, descriptor and command leaves. |
| `runtime/orchestrator/src/overlay_native12_platform.h` | Retained the private SDK leaf ABI and added the size-checked Rust COM adoption record. |
| `runtime/orchestrator/src/overlay_panel.h` | Retained the panel loader contract. DLL lookup, admission and lifetime are in Rust. |
| `runtime/orchestrator/src/overlay_platform.cpp` | Retained Runtime DLL exports and native leaves. Service and display refresh behavior are in Rust. |
| `runtime/orchestrator/src/overlay_platform.h` | Retained the private native service table for generated Rust bindings. |
| `runtime/orchestrator/src/overlay_wrapped12_platform.h` | Replaced wrapper visibility polling with host admission and explicit admitted draw state. |
| `runtime/orchestrator/src/sharpening_overlay.h` | Replaced C++ field maps with declarations for the shared Rust mapping. |
| `ui/overlay/include/rescaleframe/overlay.h` | Appended panel ABI 19 quality-choice and change-receipt fields. |
| `tests/overlay_dll_api.cpp` | Rust owns the DLL contract. The C++ client imports the real Runtime and supplies the guard-page operation. |
| `tests/overlay_dll_api_device.cpp` | Rust owns the real-panel callback, frame, cursor, resize and reset scenario. C++ retains hidden-window D3D11 leaves and hardware/WARP skip. |
| `tests/overlay_dll_api_no_sr.cpp` | Rust owns no-SR and no-FG acceptance and persistence checks. C++ passes the actual DLL import. |
| `tests/overlay_native12.cpp` | Rust owns the WARP queue block, four submissions, drain and debug checks. C++ calls the production wrapper. |
| `tests/overlay_panel_fixture.cpp` | Replaced with a Rust DLL preserving the five exports, triangle and initial atlas update. |

## Rust findings

| Exact PORT title | Source evidence |
| --- | --- |
| Skip empty overlay draw calls before resolving textures on both renderers. | Both draw plans call `overlay_geometry::draw_clip` before atlas lookup. The mixed D3D12 fixture names absent textures in two empty calls and expects the valid call. |
| Allow the native D3D12 panel to recover after its failed owner retires. | `Panel::retire_failed` runs before comparing owners. The source fixture signals through the original renderer, waits for its serial and then admits a replacement. |
| Normalize saved FG backends with the same rule used for live settings. | `Config::accepts_fg_backend` handles validation and saved-value recovery. The DLL fixture loads a stale backend with no FG choices, expects Auto and checks the next save. |
| Preserve Auto as the requested FG setting when building panel statistics. | Service and render host report requested Auto apart from the effective provider. The panel fixture uses the enabled control with Auto and a resolved provider. |
| Require an adapter to acknowledge an editable overlay option. | Setup rejects nonempty options without a change callback, and the cache waits for a positive return. The DLL contract exercises the missing callback. |
| Share the generated overlay wire types with the overlay frontend. | The Runtime and panel use the same generated stats, host options and intent records. Independent C and panel Rust assertions pin the layout. |
| Define the selectable SR quality levels from the same capability list the runtime accepts. | The service sends its admission mask. The callback host shares its mask function with admission, and the Vulkan route uses the same standard and extended masks. The panel fixture offers level 5 only when advertised. |
| Define sharpening defaults once for the runtime, panel and recording fixtures. | The C header supplies generated defaults for the Runtime, panel and Godot fake. The Rust field mapping fixture uses distinct values. |
| Reconcile refused quality and enable requests with an explicit apply result. | Panel edit IDs are returned by the AC7, service, render and callback hosts. Panel fixtures exercise quality and enable retries and ignore a stale receipt. |
| Keep every offered integration tab reachable at supported panel scales. | Independent rail scrolling makes Game and Display reachable at 720p and 1.5× in the source fixture. |
| Let the pacing owner resolve the VRR frame limit from the effective output mode. | The panel sends a VRR policy. The provider and CPU pacing paths resolve it from the display and effective generation. Source fixtures cover active, suspended and changed or missing refresh inputs. |
| Share the FG deferral-reason catalog across the runtime and overlay. | `native_fg.h` generates the reason IDs. One catalog serves the panel and Runtime status, including invalid input and unknown codes. |
| Leave presented FPS unknown when generation is active without an aggregate counter. | Rendered and presented rates have independent baselines. The fixture exercises active aggregate, unavailable aggregate and restored pass-through. |
| overlay_native12 Panel keeps its started state as three parallel Options and unwraps them on every Present | `Panel` uses a single `Option<Started>` containing UI, renderer and fence. Fence event retirement handles a missing event. |
| Use one elapsed-frame policy for the shared overlay panel. | D3D11 QPC and D3D12 millisecond ticks feed the same normalization function. Source assertions cover first frames, backwards clocks, normal intervals and gaps. |
| Use one draw-admission decision for the wrapped overlay. | The host supplies visibility and hint state before wrapping. The fixture checks one admission and no target acquisition or flush after refusal. |
| Document that a zero SR choice mask disables the upscaling controls. | The generated wire type uses the public header contract. The panel continues to disable the SR controls for a zero backend mask. |
| Make the reported quality a single value in the overlay model. | FFI keeps one raw quality value and the model decodes it on demand. The test retains the unknown ID. |
| Use the input event position to draw the overlay cursor. | The point returned by raw input is shared by event dispatch and cursor painting. |
| Draw available tabs directly from the fixed tab list. | A fixed availability array and indexed tab order replace allocations and repeated position lookup. |
| Limit the segmented control to the single-row layout its callers use. | One row sized by the item count preserves the taller newline labels. |
| Replace the AC7 scene wording for the generic FG scene-unsupported reason. | The shared catalog says “Waiting for an eligible scene”. The cursor comment describes panel ownership. |
| Check detailed HUD inputs before formatting the text. | The cache compares backend, quality and interval before constructing strings. Compact and detailed modes remain distinct. |

## Verification boundary

Source checks compared shader text, descriptor and context call order,
ABI prefixes and size assertions, and ownership on draw, refusal and cleanup.
The affected bindings were reproduced with bindgen 0.72.1 and pinned Rust
1.94.1 formatting. Locked offline Cargo metadata and the changed-source
whitespace checks passed.

Builds, lint, fixture execution and device validation are user-owned.
The checkout has no observed visual or gameplay acceptance. Device checks
still cover WARP slot pressure, the real-panel Runtime DLL, output and
generation changes under VRR, and Minecraft's exact backbuffer and input HWND.
