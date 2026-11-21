# Fallout 4 World and Present admission in Rust

This source port starts at `1a87cbca` and uses the existing Fallout 4 AE 1.11.240.0 evidence for
executable SHA-256 `fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The [native callback analysis](fallout4-render-integration-20261116.md) identifies the ordinary
WorldBegin parent and outer Present owner. The [reduced recovery analysis](fallout4-native-reduced-recovery-20261116.md)
explains why pre-Present diagnostic capture can proceed without temporal or FG admission.

| Boundary | Admission and ownership |
| --- | --- |
| Before original WorldBegin | Rust requires return `0xc382a5`, a live adapter, the current host session's nonzero source without a submission, and no nested World guard. Open FRAME, auxiliaries, a recovery token or the resolution owner's pending work block admission before the CPU owner may bind a submission. C++ keeps the ordinary World guard on the hook stack and opens FRAME then SUBMISSION only after its double native owner and COM checks. |
| Before original outer Present | Rust requires return `0xc335ec`, renderer `0x3a25d00` relative to the image, and an open SUBMISSION. The C++ reader verifies the active renderer-window cell at `0x38e13b0` against `RendererData+0x58` before reading its chain at `+0x18`. Rust matches the chain from that cell and the frame, view, window, context and chain from the native owner against the FRAME lease. |
| Matched Present | C++ can capture reduced recovery output while the typed native framebuffer lease is live. Rust then requires the current render source and asks the existing screen facts path to revalidate any leased input. A sticky refusal withholds SUBMISSION completion. Success closes SUBMISSION before beginning the CPU Present phase and calling the original. The phase end uses the identity copied at phase begin on the C++ stack. |
| After Swap | The existing cleanup balances any refused SUBMISSION, ends WINDOW and FRAME, retires recovery and auxiliaries, and restores resolution state. A pending obligation retains its owner and blocks the next World admission. |

The boundary carries copied scalar identities and returns admission decisions. C++ keeps the typed
native trampolines, `LastError` restoration, COM references, callbacks and capture. The GameSDK ABI
and native guard bytes are preserved. Prepass and depth capture, FG input publication before SR,
reduced FINISH/RETIRE, logical framebuffer handoff and native TAA remain in their existing order.

Source comparison checked the original hook calls, exact caller and receiver constants, owner
predicates, capture-before-admission order and retained cleanup against `1a87cbca`. Pinned Rust
1.94.1 formatting, locked offline Cargo metadata and whitespace checks passed. Build, link,
Clippy, tests, deployment and device/game checks remain with the user. Matched capture through
Present, moving shader/buffer coverage, reduced recovery and TAA history across quality changes,
and stable FG/latency still require runtime evidence. See the [shader and buffer audit](fallout4-full-shader-buffer-audit-20261116.md).
