# Anomaly input and overlay port

The N02 port moves DirectInput and DXGI callbacks, COM probes and backbuffer
access into the Anomaly Rust static library. It uses the same system DirectInput
module and WARP probe as the prior C++ implementation. The overlay keeps the
carrier-provided Runtime handle. The port uses the existing MinHook transaction,
the Anomaly window filter and the shared overlay API.

## Closed review findings

- **Report consumed input from the gate operation that identified it.**
  `games/anomaly/src/input/gate.rs` takes the device kind and hardware count from the
  originating buffer read. It releases the gate lock before querying and calling
  the observer. Peeks, flushes, captured hardware and synthetic releases do not
  generate receipts. `games/anomaly/src/overlay_hooks.rs` uses this path directly.
- **Give the Anomaly overlay diagnostics INI keys adapter option rows.**
  `games/anomaly/src/adapter_options.rs` registers restart-marked controls for
  `OverlayKeyPoll`, `OverlayCapture`, `OverlayCaptureFrames` (1–16),
  `OverlayCaptureStride` (1–600) and `OverlayCaptureCycles` (1–8). The numeric
  and toggle options use 37 of the shared panel's 40 slots when the integrated
  N06 SR rows are registered. `OverlayCaptureDir` stays a path in the INI.
- **Return the actual adapter-option apply and save result.**
  `games/anomaly/src/overlay.rs` validates the submitted range and forwards the
  private setter result through the shared callback. The Anomaly, motion and SR
  setters propagate persistence failures. N06 scale, camera cut, capture
  schedule and signature options commit their state only after persisting.
  Capture refusals preserve schedule progress and the Stage 3 burst revision.
  Saving an SR mip offset while disabled records it as
  `SRMipBias=off <offset>`. The parser still accepts the previous `off` and
  numeric forms.
- **Reuse the local timestamp formatter for Anomaly capture directories.**
  `games/anomaly/src/capture_clock.rs` supplies the common local
  `YYYYMMDD-HHMMSS` directory component. The overlay and FG still each choose
  their own root and custom path.

## Port paths and remaining native boundaries

- `games/anomaly/src/overlay_hooks.cpp` and `overlay_hooks.h` are the C++ bridge
  that provides the existing MinHook operations to `overlay_hooks.rs`.
  `overlay_service.cpp` and `overlay_service.h` bridge the existing carrier,
  FG sink and native fixtures to `overlay.rs` and `overlay_native.rs`.
  `overlay_input_gate.h` keeps the opaque C++ handle and checks the Windows SDK
  constants. `overlay_platform.h` supplies the private ABI for bindgen.
- `games/anomaly/src/input_hooks.cpp` and `input_hooks.h` retain the native
  guarded copy and slot exchange and call the original through its native thunk.
  Rust already owns slot admission, polling scope, in-flight counts, and event
  routing. `input_profile.h` supplies the version-pinned guard data to the native
  build profile and existing executable evidence scripts. The port
  keeps the verified addresses, bytes and engine callback contract from the
  existing bootstrap.
- `tests/anomaly_overlay_input_gate.cpp` and `tests/anomaly_overlay_api.cpp` are
  replaced by Rust fixtures in `games/anomaly/src/input/gate/tests.rs` and
  `games/anomaly/src/overlay/tests.rs`.
  `tests/anomaly_input_hooks.cpp`, `tests/anomaly_overlay_hooks.cpp` and
  `tests/anomaly_overlay_visibility.cpp` remain as native fixtures for the
  guarded executable slot, real COM detours and the Runtime DLL/window/renderer
  boundary.

## Verification scope

Source comparison against Windows SDK 10.0.26100.0 checked the ANSI and Unicode
DirectInput, IDXGISwapChain/IDXGISwapChain1 and ID3D11Device vtable slots used by
the port. The integration preserves N06's shared vendor paths, mip divisor,
recovery and timing source. At full registration, 37 options exceed the old
32-option cap. The generic API limit is now 40 across the adapter, Runtime and
panel, with the same API version and record layouts. Anomaly should ship with
the integrated Runtime so the full setup is accepted. The native overlay API
fixture covers acceptance at the limit and refusal above it.

The Anomaly and Runtime overlay bindings, SR stage binding and panel limit
were regenerated with bindgen-cli 0.72.1. The edited Rust files were formatted
with Rust 1.94.1 and the patch passed the whitespace check. Build, lint, fixture
and game execution remain with the user. Runtime validation is pending.

## Residual port from 3665aef0

The existing N02 port on this base is `0c029e08971744c5f0865a7961940951f4d05044`.
The residual port removes the C++ input, overlay service, hook and gate
facades. The Rust gate is process-owned, and the existing MinHook operation
table still supplies installation and rollback. DXGI targets are still
probed from the system D3D11 WARP chain, with Present1 discovered via its
interface. The overlay still admits the same-process Anomaly engine window
and rejects test Presents and foreign FG chains. Resize notification now
requires the chain that owns the panel. The overlay retains the original
trampolines and their module references across retirement.

`input/profile.rs` carries the Standard and AVX OnFrame, KeyUpdate and
MouseUpdate guards, primary vtable slot RVAs and complete OnFrame bodies.
Bootstrap passes the build index only after its name and hash admission.
The native detour keeps C++ unwinding around the original OnFrame call
so the Rust in-flight ticket is retired on both return and unwind. The
narrow `input_hooks.h` declarations keep that boundary and expose input
slot enable, disable and idle to the observer. `overlay_platform.h`
retains the generated private startup and option layouts. The three
remaining fixture executables keep small native entry points for the
guarded Win32 primitives, MinHook and SDK mouse format, and the imported
Runtime query respectively. Their assertions and device scenarios run in
Rust when the fixtures are executed.

The original option descriptors, restart flags and apply/save results
continue through overlay service v4. Startup also registers the overlay
toggle and capture directory through O02's existing saved-options ABI 1.
Both rows require a restart. The Runtime copies the descriptors and
persists the values without borrowing an adapter callback. Empty capture
text selects the existing timestamped directory. The cold reader decodes
saved UTF-16 and accepts legacy INI text. The visibility fixture supplies an
encoded directory with a supplementary Unicode character and retains real
Runtime, WARP/hardware, PNG pixel, input and settings checks.

Pinned Rust 1.94.1 formatting, bindgen-cli 0.72.1, locked offline Cargo
metadata and whitespace checks passed. Source checks matched the six site
guards, both complete bodies, both slot RVAs and both device field offsets
against the base, and checked fixture registration and O02 ABI equality.
The N06 SR source and Runtime API remain outside this replay. Windows
builds, fixtures, lint, device and gameplay execution remain pending.

For the N06 fixture replay, retain its `rsf_anomaly_sr_fixture_rust` link
for `rsf_anomaly_sr_hooks_test` and remove that fixture's reference to
`input_hooks.cpp`. The observer now calls the Rust slot owner through
the inline declarations. Both N02 and N06 implementation entries belong
in the tracker.
