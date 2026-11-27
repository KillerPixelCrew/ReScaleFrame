# Generic D3D12 render host Rust cutover

The baseline is `2b23cfe` on `rust/orchestrator-anomaly`. The question was whether the generic D3D12 render host behind `render_host.h` and `render_frontend.h` could move to Rust without changing its callers, the exported entry points or the SR12 and FG12 owners it drives. This was a source port with synthetic contracts and CPU fixtures. No device, window or game ran.

## Ownership after the move

| Concern | Owner and evidence |
| --- | --- |
| Admission | `render_host.rs` reads the setup and services prefixes before any field and keeps the refusal order: short or incomplete setup, short services or a zero session, then setup and game ABI versions, then an already published host. `render_host_platform.c` no longer asserts the setup and observer layouts the Rust mirrors name. |
| Host and control state | One `Mutex<State>` holds the SR context, the NGX registration, the device, fence and queues (as `ComRef`), the three recording slots, the 64 scene records, the submission snapshots and the status counters. The poison flag, Auto resolution, observed lists, graphics readiness and panel layout word stay atomics, as before. The published host lives for the process. |
| Settings | The poll, `rsf_render_settings_get`, the status snapshot and the overlay all read the section through `settings_store.rs` with the frontend defaults and sanitize the post-pass values through the public `sharpening_settings.h` helper. A commit validates the whole request first, then writes the owned keys in the store's single section write and applies them at once. Provider admission for FG uses `fg_choice::allowed` against `RSF_FG_BACKEND_ALL`; SR Auto uses `rsf_gpu_select_sr_backend`. |
| Graphics lifecycle | Device and fence adoption on the first pass, foreign device refusal, ratio-based quality, the context replacement conditions, drain before replacement, NGX registration retention, slot reservation before evaluation, the submitted-slot join, the 15 s fence wait and every poisoning path are unchanged in order. |
| Submission and scenes | Begin snapshots pending slots by recording serial for the submitted lists, End signals one fence value on the single DIRECT queue, retires matching slots and reports the matching scene to FG12. A second DIRECT queue poisons. Scene capture, HUD-less handoff and Present matching by frame, surface and submission are unchanged. |
| Services and routing | Rust fills `process_upscale`, `submit`, `present`, `render_pass`, `cpu_event`, `process_latency` and `render_feedback`, publishes the host and replaces the log callback with its own forwarder. The engine-presentation path still creates the D3D12 observer; its submissions are forwarded only for tracked recordings. |
| Status and overlay | The frontend status, the FG rate counters and reasons, the overlay statistics, the probe and the translation of panel intent into one settings commit are in Rust. `rsf_overlay_fill_fg_stats` and the panel draw remain native leaves. |
| Platform leaves | Each leaf formerly in `render_host_platform.cpp` performed one D3D12, DXGI, SR12, DLSS registration, FG12, presentation-bridge, overlay, observer, clock, cursor or header-helper call. The eleven public entries stay native forwarders so the Runtime DLL and embedded consumers keep their exported names and ordinals. |

## FG mode compatibility

The C++ host passed the raw `FG` value of its section to `rsf_fg12_options`, where 2 and 3 are `RSF_FG_AUTO` and `RSF_FG_DYNAMIC`. The first cut at `960cf8e` passed the store's normalized `fg_enabled` instead, so a saved 2 or 3 reached FG12 as Fixed. Review found the regression. `settings_store.rs` now returns the stored `FG` value beside the settings from the same profile read, the poll passes that value as the FG12 mode, and `rsf_render_settings::fg_enabled`, the status and the overlay keep the boolean. `polling_passes_the_stored_fg_mode_and_keeps_the_public_boolean` polls `FG` 2, 3, 1, 0 and a missing key; `fg_mode_keeps_the_stored_value_beside_the_boolean` checks the store.

## Changed behavior

- The status reads its requested FG, backend, generated frames and latency through the settings store instead of four separate profile reads, with the same defaults.
- The overlay takes one FG status sample and one locked settings read per Present and derives both the status and the panel settings from it. The C++ host sampled FG twice and took the lock twice, so a busy owner could produce a fresh status with stale settings or the reverse.
- When the engine-presentation observer refuses creation, the host allocation is retained instead of freed, because diagnostics and partially installed hooks may already name it as their callback user.
- An NGX registration is recorded only when its create or borrow succeeds.
- Log lines are no longer truncated to fixed buffers. The probe's closing line reads "survived 120".
- A failed allocation inside a callback aborts instead of being caught and reported as not ready. The host allocation itself is still fallible.

## Verification

Commands ran from the worktree with Visual Studio 2026. The build directory was configured with `cmake -S . -B build/render-host -G "Visual Studio 18 2026" -A x64 -DBUILD_TESTING=ON` and the vendor and reference paths of the coordinator's `build/rust-migration` cache.

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed. `rsf-orchestrator` ran 70 tests after the FG mode fix (68 at `960cf8e`), 13 of them new render host contracts and one new settings store test, with a recording platform, reference-counted fake COM objects and a temporary INI section. They cover admission refusals, one-shot requests and FG or observer installation, Auto resolution and context configuration, slot reservation and the busy paths, submission snapshots and the single DIRECT queue, fence poisoning, registration Present routing, settings validation and the transactional commit, status reasons and rates, overlay intent routing and the probe.
- Debug CTest `render_frontend_contract`, the new `render_frontend_embedded` (the same fixture linked against `ReScaleFrame::OrchestratorCore`), `orchestrator_domain_contract`, `sr12_abi`, `sr12_abi_embedded`, `fg12_abi` and `fg12_abi_embedded` passed 7 of 7 at `960cf8e`. After the FG mode fix the Debug runtime was rebuilt and `render_frontend_contract`, `render_frontend_embedded` and `orchestrator_domain_contract` passed 3 of 3. The fix changes no export, so the Release comparison below was not repeated.
- `dumpbin /exports` on Release `ReScaleFrame.Runtime.dll` (381), the AC7 `dinput8.dll` (256), the shared `dinput8.dll` loader (273) and `ReScaleFrame.Minecraft.dll` (267) matched the coordinator's `build/rust-migration/baseline-exports` in every ordinal and name.
- Built only, not run: `rsf_unity_sr_d3d12_test`, which creates a D3D12 device and drives this host.

Unverified: real D3D12 submission, fence and observer behavior, vendor evaluation through this host, the shared overlay on a device, the RDR2 adapter and any game. The full `eng/verify.ps1` suite was not run.

## C02 follow-up on the saved Unicode base

At `3665aef0`, both D3D12 hosts owned their state in Rust but routed SR12,
FG12 and their Win32 and COM operations through host-specific C++ tables.
They now call the sibling Rust SR12 and FG12 owners directly. SR12 creation
uses the same static provider platform as the native API. Each public host
entry binds the existing FG12 platform before publishing any callbacks.
The Rust Win32 and COM leaves preserve the returned device references,
DIRECT queue test, auto-reset fence event, shared fence wait, and DXGI
Present count. The smaller C++ tables carry the remaining native service
and SDK edges, including Streamline registration and native object lookup,
the observer and overlay entry points, plugin sessions, and C header helpers.

The native host, callback host, frontend, window and link C headers still
define the public wire contracts. The native host setup now has a generated
Rust binding shared by the runtime and the fixture. The callback host's v1
and v2 setup, generation callbacks, retirement ordering and exports retain
their existing contracts. Native-window scene lookup was already direct on
this base.

The four old C++ fixture bodies now run from Rust via the existing CTest
entries and small C mains. The callback and frontend fixtures retain both
DLL and embedded linkage. The native window case keeps its fake canonical
IUnknown, reference accounting, thread-local scope, exact scene matching,
and real QPC checks. The render links case retains the DLL record checks
alongside the existing Rust concurrency and generation fixtures. The
render-host unit fixture checks that an FG capture refusal preserves an
active supplementary-character path and that an accepted clear rearms SR
capture. The shared saved-row transaction continues to own profile rollback.

Pinned Rust 1.94.1 formatting, bindgen 0.72.1 reproduction of five C02
binding outputs, locked offline Cargo metadata with all features, and
changed-source whitespace checks passed. These are source checks. Runtime
linking, all fixture execution and device behavior remain pending.

## C02 direct service calls and the foreign boundary

At `6c935471`, the two host platform translation units still built separate
C++ tables for Runtime services. The hosts now call the existing Rust depth
and preset policy, presentation owner, overlay owners, plugin-session owner,
D3D12 observer and diagnostics owner, and DLSS registration owner directly.
Only Rust fixture builds construct host callback tables. The public exports,
setup records, saved Unicode transaction, FG source receipts and GPU
retirement order carry forward from the base.

The native host resolves `sl.interposer.dll` and `slGetNativeInterface` in Rust.
It borrows the already-loaded module for DLSS registration. A successful
native-object lookup returns the SDK's owned reference. A missing entry,
failed call or C++ exception returns one retained reference to the original
object. `render_host_platform.c` is a C11 leaf that contains only the exception guard for this
foreign call: an SEH `__try` whose filter accepts only the MSVC C++ exception code
`0xE06D7363` and lets other faults propagate, so it stops what the former `catch (...)` stopped. The declaration matches `PFun_slGetNativeInterface` from the
local Streamline checkout at `e8aaa6eaac968711fb62473d4ae8256dde20919b`.
Plugin lifecycle and observer calls still pass through their existing foreign
guards. Capture cancellation still enters `input_dump.c` to establish its
services before reaching the Rust capture owner.

Pinned Rust 1.94.1 formatting, targeted bindgen 0.72.1 reproduction, locked
offline all-feature metadata and source checks passed. MSVC syntax-only
checks covered the remaining guard, its exact comparison with the Streamline
header, and the existing C fixture launcher. Linking, fixture execution and
device verification remain pending.
