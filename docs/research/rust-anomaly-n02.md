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
