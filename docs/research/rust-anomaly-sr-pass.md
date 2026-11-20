# Anomaly SR pass construction in Rust

The port starts from `eb3d579`. The Stage 5 seam now gives the Rust
temporal driver the camera and jitter facts already collected by the Rust
stage driver, paired with the native leaf's borrowed texture pointers. The
temporal driver selects the reset reason, builds `rsf_game_render_pass`, and
submits it to the same synchronous Runtime evaluation function. It also
validates a backend's planned render size with the existing Rust plan
function directly. `sr_pass.h` keeps the native fixture API as a facade over
the production Rust builder. The stored-motion decode reference remains
available to fixtures that compare the Runtime's GPU decode with CPU values.
[SR seam evidence](anomaly-sr-seam.md#12-stages-4-and-5-implementation-sr-inputs-and-temporal-sr-at-the-seam),
[SR stage audit](rust-anomaly-sr-stage.md),
[SR temporal audit](rust-anomaly-sr-temporal.md)

The guarded reads still take `Device.mView` and the unjittered
`Device.mProject` at the main-view H1 before raster jitter, for both verified
Anomaly 1.5.3 profiles. SR and FG now use one X-Ray camera implementation for
planes, pose and clip mapping. It preserves positive view Z, standard finite
depth, and row-major storage with row vectors. It derives near, far and
vertical FOV from `_33`, `_43` and `_22`, and computes
`inverse(V * P) * (previous_V * previous_P)` and its inverse in double
precision. SR maps a reset or unavailable previous camera to the current
camera. Current and previous applied jitter still travel in render pixels
with positive X right and positive Y down. The SR cut compares consecutive
main-view poses in metres and degrees with the configured thresholds. The
reset order remains first, device, settings, sequence, then cut. The Runtime
still handles its own size, rectangle, session and frame-continuity resets.
[FG camera audit](rust-anomaly-fg-stage.md)

The existing D3D11 owner keeps the resources live through evaluation, including
the depth twin and prepared motion. The native seam supplies an optional
exposure pointer and the motion-coverage fact separately. The SDK layout,
resource identities, refusal order, output reinsertion and bilinear recovery
use the same contracts. The prior shader and buffer inventory still applies
because the port uses the same producers, shaders and native resource leases.

## Verification

Worktree-local MSVC 19.52 x64 builds used the coordinator's pinned MinHook
`c3fcafdc10146beb5919319d0683e44e3c30d537` and vendor inputs. The first
configure failed the downloaded yyjson hash check. The existing pinned
copy then passed the repository's manifest checks. The installed Rust
MSVC toolchain was 1.100.0. All 271 locked workspace tests passed, including
the FG camera contract and an H1 test for the exact cut distance, a larger
move, and an invalid camera. `cargo fmt --check` and locked workspace
Clippy with all targets and warnings denied passed. Bindgen 0.72.1 reproduced
the private bindings byte for byte.

Debug and Release each passed 27 focused CPU contracts, including pass,
reconstruction, jitter, Runtime SR API, swap and the Standard and AVX hook
and input fixtures. The input-event contracts used the CPU-only dispatch
with a fake installer. The SR round-trip, jitter, temporal and FG device
modes were build-only. Live SR behaviour in both profiles still needs
verification in the game.

A worktree-local CPU comparison compiled the `eb3d579` header alongside
the Rust facade in Debug and Release. It compared 1,000 cases with changing
poses, FOV, far plane, jitter and reset, plus singular history and refusal
inputs. Pass decisions, populated SDK fields, matrix elements and cut
decisions matched. The checked-in native pass contract also verifies that
a refused pass clears its lease pointers and that a reset discards a
singular previous camera while retaining the previous applied jitter value.

`dumpbin` compared both adapter DLLs with the `8322179a36c7` staged DLL
(SHA-256 `b978d99e2506d4d4864d267a109cd354a31e834c8e420d27dba4d8e9031fdca5`).
Both builds match the staged DLL's `rsf_anomaly_observation` at ordinal 1,
`rsf_anomaly_start` at 2 and `rsf_get_game_plugin_api` at 3. They retain
the UTF-16 `ReScaleFrame.Runtime.dll` name, the loader lookup imports
and all 16 by-name Runtime lookups, including the SR table, colour transport
and input dump functions. The Runtime is borrowed dynamically. The staged
Release imports for CRT conversion and time are absent, consistent with the
earlier SR and FG stage ports (`3c4b0d0` and `f7a0f83`).
