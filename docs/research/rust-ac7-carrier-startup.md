# AC7 carrier startup policy in Rust

Source base `9260a14e10be8546c79245571a4d4b4a80434383`. The sites come from the existing Steam
build 9855922 evidence for `Ace7Game.exe`, AMD64, SHA-256
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`.
This change traces the shipping carrier source and reuses the recorded executable and UE4.18.3
evidence. See the [hook map](ue418-hook-map.md), [briefing and consumer notes](ac7-consumer-session.md)
and [native renderer sites](rust-ac7-native-sites.md).

## Entry and preparation

- The MSVC combined version shim still compares the main executable leaf with `_wcsicmp` during
  process attach. It takes the name from the same `rsf-ac7` constant used by GameSDK detection.
  Unity crash helpers still take the forwarding-only branch and other processes still get the
  configured startup worker. The dedicated DirectInput carrier and the non-MSVC combined
  carrier still enter the AC7 attach handler directly. That handler registers crash reporting,
  enables optional early capture support and starts the worker. The separate thin carrier has its
  own entry path. DirectInput forwarding still resolves the system library by absolute path and
  returns its HRESULT, or `E_FAIL` on the existing refusal paths.
- The worker starts the observer and cold presentation interception before waiting for decrypted
  code. It calls `rsf_ac7_carrier_install_decrypted` after the existing entropy wait or optional
  dump succeeds. Rust applies the jitter gate, optional translucent velocity gate, depth and view
  gates, separate-translucency scale patch and then calls native plugin preparation. At this stage
  the native worker installed optional motion capture and published `startup_ready` after
  preparation returned, including on a native preparation refusal.
- The cold presentation callback still checks that the window belongs to this process and compares
  its class to `UnrealWindow` before attempting preparation. The adapter supplies the class and DLL
  basename. Native code still resolves the sibling `ReScaleFrame.Game.AC7.dll` and hashes the main
  executable via BCrypt on both preparation paths. Rust fills the existing `rsf_game_probe` from
  the shared executable name and AMD64 constant and borrows the digest for the session call.
  The bridge keeps the same SRW lock, host callbacks, result handling and motion-capture ownership
  notification. The Runtime session queries the GameSDK export, checks executable detection and
  calls prepare. The native renderer still checks the patched depth and view gates before hook
  creation. A refused cold window or preparation follows the existing original-chain path.

## Guarded sites

| Purpose | RVA | Policy |
| --- | --- | --- |
| Temporal jitter | `0x112b1f3` | Compare the stock six-byte JNE and open via six-byte NOP or `E9 rel32 90`. Reversing the gate expects the exact open bytes. Only the default site can use the existing main-view relay. |
| Translucent velocity | `0x11823de` | Remove the optional dynamic-mesh JA blend-mode gate with the six-byte NOP. The stock RVA requires the expected bytes. |
| Enlarged separate-translucency depth and view | `0x1168f6f`, `0x1097a0c`, `0x109d03a`, `0x109d061`, `0x11583a6`, `0x1025b12`, `0x10294d2`, `0x102ada2`, `0x102c672`, `0x102e792`, `0x1031902` | Preflight all 11 via identical guarded writes, apply in order, and attempt reverse rollback of the applied prefix on refusal. Native admission uses the same table's patched bytes. Depth conformance is published before scale registration. |
| Separate-translucency scaling | `0x10be329` | Replace the 15-byte window with the existing scale instruction sequence. The stock RVA requires the expected bytes. The native leaf registers the immediate at RVA `+7` when its address is aligned for a live 32-bit write. |

The translucent velocity and scale settings retain their diagnostic custom-RVA behavior, which
passes an empty expected window to `rsf_patch_code`. The jitter override still checks expected
bytes. Native leaves retain `rsf_patch_code`, the jitter relay allocation and relative address
calculation, the live gate state and the aligned scale write. The carriers use their existing
`rsf-ac7` archive dependency. The carrier callback table checks its own size and ABI version and
uses the existing GameSDK probe and logging types.

## Verification and remaining work

Source comparison against `9260a14` matched the 14 startup RVAs and byte windows, the 11
post-patch admission windows, the install and reverse rollback order, the worker's decryption
gate, the attach handler, the native relay and DirectInput forwarding. It also checked the
carrier export definitions, CMake link definitions, Runtime and SDK paths, and AC7 motion and
velocity workers for changes. Pinned Rust formatting, locked offline Cargo metadata and
whitespace checks cover the changed source. Compile, link, fixture, device and in-game behavior
remain unverified for this revision.

The carrier still owns the Win32 worker, settings reader, diagnostics, console and jitter relay
mechanics, presentation setup and the compatibility bridge. The cold path can call preparation
independently of the worker, as in the base revision. Existing start timing and refusal behavior
are part of the pending game validation.

## Worker readiness cutover from `1eda132`

The worker calls `start_observer` then `start_generation` before its native decryption wait.
`RSF_OBSERVE=0` skips observer and overlay registration while the worker still requests cold
presentation interception. The optional module dump and the measure-only path use the same 250 ms
interval, 180 s timeout and two consecutive samples below 7.0. Failure returns from the worker
before the Rust call. The non-dump diagnostic and the dump report and module sampling follow that
call as before.

The cold swapchain path invokes `accept_ac7_window` before allocating a presentation bridge. The
native callback checks current process ownership and `UnrealWindow`, then calls the same native
preparation leaf as the worker. Preparation hashes the executable and supplies the sibling DLL
path and GameSDK probe to the bridge. Under its SRW lock, the bridge reuses the result when a
session exists. The Runtime drops a refused session after successful cleanup so a later attempt
can retry, and retains a partial session if cleanup fails. Window or preparation refusal falls
through to the original DXGI create call. Cold preparation still runs independently of worker
readiness and SR enablement.

Rust now reads `RSF_MOTION_CAPTURE` with the existing native settings reader after the preparation
log. A nonzero value calls the native capture installer and discards its return value. Rust then
publishes `startup_ready` with a sequentially consistent atomic store. The overlay action reads it
with the same ordering, replacing the Win32 interlocked flag. Preparation refusal or capture
refusal still allows publication. Capture configuration in the observer and compile-time
RenderDoc gating at attach are unchanged. The private carrier callback table appends the capture
installer and advances to version 2 with matching x64 layout assertions. Both carriers use the
same table and existing Rust archive. GameSDK ABI 24, Runtime interfaces and carrier export
definitions are unchanged.

Automatic SR startup remains on Present. It needs enablement, worker readiness, an unstarted
backend, no previous automatic attempt and a valid presenting swapchain extent. The bridge
records the attempt before calling `start_dlss`, which reads the observer's presented size and
starts on its acquired D3D11 device. Re-enabling clears the attempt flag. A manual start request
still calls the native action directly. The bridge activates the native session only after the
pipeline starts and selects the compatibility frame-tap callbacks when the native session is
absent or activation refuses.

Source comparison with `1eda132` covered the worker failure and success paths, cold-window and
device ordering, optional capture and SR enablement, prepare/refusal and fallback paths, the
private table and both carriers' link and export definitions. Rust 1.94.1 formatting, locked
offline Windows Cargo metadata and whitespace checks passed. The native thread, wait, window and
device calls, preparation bridge and compatibility activation still reside in native code.
Compile, link, Clippy, tests and device and in-game behavior remain unverified for this cutover.

## Automatic SR admission cutover from `3856821`

Both carriers register the Rust readiness getter and the native `start_dlss` action. The
D3D11 observer updates the extent from the swap chain before calling the bridge's Present hook.
The D3D11 presentation facade also updates the observer before invoking the same hook. After
shutdown handling, queued actions and the capture tick, the bridge calls the Rust admission
owner with the current enable request and backend state. Rust checks readiness and the startup
action before querying the observer. A successful query with nonzero width allows an atomic
claim of the attempt before the native action runs. An unavailable extent leaves the attempt
open. A refused start spends the attempt. The queued manual start calls the same action
directly and bypasses automatic admission.

The bridge uses the attempt flag for the existing waiting and refusal text. Enabling clears it
before any quality reapply. Disabling first restores 100% render scale on the running
compatibility path and aborts on a refused restore. A successful disable clears reinsertion,
display and jitter before publishing and saving the setting. Completed shutdown marks the
attempt spent after retiring the session and pipeline. The carrier's private callback table
and the public GameSDK ABI retain their existing versions and layouts.

The start action resolves the Streamline path before asking the observer for the extent again.
Both queries use the same native status helper and the width-only availability check. Rust
applies `RSF_DLSS_OUTPUT_WIDTH` then `RSF_DLSS_OUTPUT_HEIGHT` through the existing settings reader.
Dimensions are pixels and the fallback for each is its freshly observed value. The same
Streamline override and sibling fallback, shared FidelityFX directory and per-SDK overrides,
XeSS resolution and sibling game DLL path still reach their existing native callers. The
startup overrides do not apply to extraction or live native output resize.

The bridge acquires the game's D3D11 device and starts the pipeline before asking Runtime to
start a prepared native session. Preparation wires the render pass, CPU event, scope, render
config and log services. Successful activation publishes native ownership and leaves the
geometry callback active without the compatibility pass, target, input and candidate callbacks.
An absent or refused native session takes the compatibility callback path when no native owner
has been published. The compatibility path still waits for the discovered tail and a fresh
evaluation before automatic reinsertion. Tap or quality refusal still follows the existing
native cleanup sequence. Vendor selection, pacing and GPU resource handling remain in their
existing owners.

Source comparison against `3856821` covered enablement and retry, extent availability and
override order, both Present entry points, native activation and fallback, the SDK paths, and
the two carrier link and export definitions. Rust 1.94.1 formatting, locked offline Windows
Cargo metadata and whitespace checks passed. Native session activation, tap installation and
compatibility rendering remain in the bridge. Compile, link, Clippy, tests, device and game
verification remain pending.
