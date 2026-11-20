# Generic D3D12 render host Rust cutover

The baseline is `2b23cfe` on `rust/orchestrator-anomaly`. The question was whether the generic D3D12 render host behind `render_host.h` and `render_frontend.h` could move to Rust without changing its callers, the exported entry points or the SR12 and FG12 owners it drives. This was a source port with synthetic contracts and CPU fixtures. No device, window or game ran.

## Ownership after the move

| Concern | Owner and evidence |
| --- | --- |
| Admission | `render_host.rs` reads the setup and services prefixes before any field and keeps the refusal order: short or incomplete setup, short services or a zero session, then setup and game ABI versions, then an already published host. `render_host_platform.cpp` asserts the setup and observer layouts the Rust mirrors name. |
| Host and control state | One `Mutex<State>` holds the SR context, the NGX registration, the device, fence and queues (as `ComRef`), the three recording slots, the 64 scene records, the submission snapshots and the status counters. The poison flag, Auto resolution, observed lists, graphics readiness and panel layout word stay atomics, as before. The published host lives for the process. |
| Settings | The poll, `rsf_render_settings_get`, the status snapshot and the overlay all read the section through `settings_store.rs` with the frontend defaults and sanitize the post-pass values through the public `sharpening_settings.h` helper. A commit validates the whole request first, then writes the owned keys in the store's single section write and applies them at once. Provider admission for FG uses `fg_choice::allowed` against `RSF_FG_BACKEND_ALL`; SR Auto uses `rsf_gpu_select_sr_backend`. |
| Graphics lifecycle | Device and fence adoption on the first pass, foreign device refusal, ratio-based quality, the context replacement conditions, drain before replacement, NGX registration retention, slot reservation before evaluation, the submitted-slot join, the 15 s fence wait and every poisoning path are unchanged in order. |
| Submission and scenes | Begin snapshots pending slots by recording serial for the submitted lists, End signals one fence value on the single DIRECT queue, retires matching slots and reports the matching scene to FG12. A second DIRECT queue poisons. Scene capture, HUD-less handoff and Present matching by frame, surface and submission are unchanged. |
| Services and routing | Rust fills `process_upscale`, `submit`, `present`, `render_pass`, `cpu_event`, `process_latency` and `render_feedback`, publishes the host and replaces the log callback with its own forwarder. The engine-presentation path still creates the D3D12 observer; its submissions are forwarded only for tracked recordings. |
| Status and overlay | The frontend status, the FG rate counters and reasons, the overlay statistics, the probe and the translation of panel intent into one settings commit are in Rust. `rsf_overlay_fill_fg_stats` and the panel draw remain native leaves. |
| Platform leaves | Each leaf in `render_host_platform.cpp` performs one D3D12, DXGI, SR12, DLSS registration, FG12, presentation-bridge, overlay, observer, clock, cursor or header-helper call. The eleven public entries stay native forwarders so the Runtime DLL and embedded consumers keep their exported names and ordinals. |

## Changed behavior

- The FG mode passed to `rsf_fg12_options` is the store's normalized `FG` value (0 or 1). A hand-edited `FG=2` used to reach FG12 as mode 2. The overlay and frontend have only ever written 0 or 1.
- The status reads its requested FG, backend, generated frames and latency through the settings store instead of four separate profile reads, with the same defaults.
- The overlay takes one FG status sample and one locked settings read per Present and derives both the status and the panel settings from it. The C++ host sampled FG twice and took the lock twice, so a busy owner could produce a fresh status with stale settings or the reverse.
- When the engine-presentation observer refuses creation, the host allocation is retained instead of freed, because diagnostics and partially installed hooks may already name it as their callback user.
- An NGX registration is recorded only when its create or borrow succeeds.
- Log lines are no longer truncated to fixed buffers. The probe's closing line reads "survived 120".
- A failed allocation inside a callback aborts instead of being caught and reported as not ready. The host allocation itself is still fallible.

## Verification

Commands ran from the worktree with Visual Studio 2026. The build directory was configured with `cmake -S . -B build/render-host -G "Visual Studio 18 2026" -A x64 -DBUILD_TESTING=ON` and the vendor and reference paths of the coordinator's `build/rust-migration` cache.

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed. `rsf-orchestrator` ran 68 tests, 12 of them new render host contracts with a recording platform, reference-counted fake COM objects and a temporary INI section. They cover admission refusals, one-shot requests and FG or observer installation, Auto resolution and context configuration, slot reservation and the busy paths, submission snapshots and the single DIRECT queue, fence poisoning, registration Present routing, settings validation and the transactional commit, status reasons and rates, overlay intent routing and the probe.
- Debug CTest `render_frontend_contract`, the new `render_frontend_embedded` (the same fixture linked against `ReScaleFrame::OrchestratorCore`), `orchestrator_domain_contract`, `sr12_abi`, `sr12_abi_embedded`, `fg12_abi` and `fg12_abi_embedded` passed 7 of 7.
- `dumpbin /exports` on Release `ReScaleFrame.Runtime.dll` (381), the AC7 `dinput8.dll` (256), the shared `dinput8.dll` loader (273) and `ReScaleFrame.Minecraft.dll` (267) matched the coordinator's `build/rust-migration/baseline-exports` in every ordinal and name.
- Built only, not run: `rsf_unity_sr_d3d12_test`, which creates a D3D12 device and drives this host.

Unverified: real D3D12 submission, fence and observer behavior, vendor evaluation through this host, the shared overlay on a device, the RDR2 adapter and any game. The full `eng/verify.ps1` suite was not run.
