# Anomaly frame-generation stage in Rust

The cut starts from the merged Anomaly adapter at `9d53556`. The staged
`8322179a36c7` adapter supplies the PE export baseline and the earlier
game evidence. [Engine fingerprints and source revision](../../games/anomaly/engine.json)

## Engine facts and resource ownership

| Boundary | Evidence carried through the port |
| --- | --- |
| Cold startup | Rust reads and admits the existing INI options and validates D3D11 FG API 3 and Present API 1. It looks up both queries on the carrier's borrowed Runtime. It installs the bridge once before device creation and publishes the overlay link only on success. Auto and the requested provider settings go to the Runtime for resolution. |
| Input and simulation | Verified `CInput::OnFrame` still reports the actual sample and simulation begin. Buffered receipts and delivered window messages use the open source ID, with source zero for window messages outside that frame. A new input update closes a preceding skipped render. The original render Begin reports the simulation end and reserves an identity when no input callback ran. The Runtime owns substitutes, deduplication, pacing and marker mapping. |
| Camera and SR | Rust latches the unjittered row-vector view/projection at render Begin and applies the existing 4 m / 45° cut rule. The FRAME, SUBMISSION and SR source scope carry the same session and source ID. Stage 5 supplies the normalized inputs and camera from its own SR pass. Stages 2 through 5 retain ownership of world depth and can leave FG inputs unavailable. |
| SR-off inputs | The native leaf reads the selected profile's `HW.pBaseZB` (Standard `0xceb8f0`, AVX `0xcd98f0`). Rust holds a local depth texture reference and admits a written main-view RTV2 at matching dimensions. The existing input pass copies standard D24S8 depth to R32_FLOAT and converts valid RTV2 vectors to previous-minus-current render pixels in R16G16_FLOAT. Its shader uses the same previous-clip transform and first-person depth rule. The pass keeps its output leases live through Runtime capture. |
| HUD-less colour | At entry to `CHUDManager::RenderUI`, the scene includes post-processing, world-space item UI and tracers at output resolution. The adapter holds the owned immediate-context and backbuffer references through the paired FINAL_SCENE calls and diagnostic dump. The shared overlay still draws in the bridge's before-Present callback. |
| End, Present and reset | End closes SUBMISSION and clears the SR association before opening WINDOW. The bridge forwards its actual Present and latency callbacks to the Runtime. End closes the accepted WINDOW and FRAME scopes before publishing the status and accepted camera history. The SR-off input owner drops before the original `CRender::reset_begin` runs. Provider retirement and resize remain at the existing Runtime and native boundaries. |

The existing Standard and AVX byte guards, the shader code and the engine
producer owners are shared with the baseline. The scope, menu, pause and
post-process paths keep their prior classification and refusal. This cut
used the recorded shader inventory and source inspection. Fresh active
bindings and motion coverage, including PDA and post-process UI paths,
require a game capture. [Anomaly FG evidence](anomaly-fg.md),
[AVX profile](anomaly-avx-profile.md), [C++ game runs](rust-anomaly-cpp-baseline.md)

## Corrected history admission

The old HUD-less handoff updated the policy with depth/motion readiness
before `GetBuffer` and ignored both FINAL_SCENE return values. A successful
Present could then count a source as fed and update its previous camera
even if the code never obtained a HUD-less surface. Rust now requires
both inputs and acceptance of the paired scene passes before the source
can advance adapter history. A rejected handoff takes the existing
`no_inputs` path. The fake-table regression refuses a missing backbuffer
despite ready SR inputs and a successful Present. The next accepted
source still carries RESET. The same test checks the exact pass order,
source and submission IDs, and buffer and context release order.

## Validation

MSVC 19.52 x64 Debug and Release built the adapter, the FG and Stage 5 SR
device fixtures, and their focused dependencies in `build/anomaly-fg`.
Each configuration passed 15 CPU contracts, including the Standard and
AVX SR and input hooks, input events through the Runtime table, lifecycle,
cold entry, FG policy and the SR and overlay API contracts. The device
fixtures were built for subsequent device validation.

Both adapter DLLs matched the staged `8322179a36c7` DLL by export name and
ordinal. The exports are `1 rsf_anomaly_observation`, `2 rsf_anomaly_start`
and `3 rsf_get_game_plugin_api`. PE imports and lookup strings confirmed
that the adapter borrows `ReScaleFrame.Runtime.dll` and resolves the FG
and presentation queries dynamically.

Bindgen 0.72.1 and libclang 18.1.1 regenerated the private binding byte
for byte. Formatting and workspace Clippy passed with Rust 1.100.0.
All 223 locked workspace Rust tests passed, including the new settings,
camera and HUD-less refusal regressions.

Device execution, real-world borderless placement, vendor recovery after
resize, moving-game output and measured latency remain unverified for this
revision. Scope hold, capture and development diagnostics use cold INI
settings. The panel continues to expose the bridge and shared frontend
settings.
