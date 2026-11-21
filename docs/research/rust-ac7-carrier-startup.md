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
  gates, separate-translucency scale patch and then calls native plugin preparation. The worker
  still installs optional motion capture and publishes `startup_ready` after preparation returns,
  including on a native preparation refusal.
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
