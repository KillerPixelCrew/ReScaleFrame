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

## Hook registration, admission and retirement in Rust

This source port starts at `17904281`. It moves the render hook lifecycle out of
`render_hooks.cpp` without new research: every RVA, expected byte and call pair is the value the
C++ owner already checked, with the evidence in the [native callback analysis](fallout4-render-integration-20261116.md),
the [depth-domain notes](fallout4-depth-domains-20261116.md), the [reduced consumer notes](fallout4-reduced-consumers-20261116.md)
and the [Address Library cross-check](fallout4-address-library-20261116.md).

| Owner | Rust (`render_hooks.rs`) | C++ (`render_hooks.cpp`) |
| --- | --- | --- |
| Hook records | The eight RVAs and expected entry bytes in the original creation and enable order, and each record's created state. | Each typed detour and its typed MinHook trampoline, which the detour calls on its own C++ frame. `remove_hook` clears that trampoline only after a successful `MH_RemoveHook`. |
| Verify | Entry bytes, then the resolution, screen-query and depth-domain guards, TAA vtable `0x2915eb0` slot 8, nine producer calls, seven outer Present instructions and eight World/imagespace calls, in the original short-circuit order. Session and prepare add the render-input guards after them. | None. |
| Prepare | Refuses a live adapter, any busy scope, missing `process_upscale` or `render_pass`, or a guard mismatch. Resolves the five Runtime exports by name, binds them to the callbacks even when the negotiation then refuses, and requires both auxiliary exports and `render_config`. Creates missing hooks in order. A failure removes this call's hooks in reverse; a refused removal keeps its record and logs the original rollback text. Host services bind only after every hook exists. | The bound exports and services the callbacks read. |
| Start | Rechecks busy and verify, queues every created hook, sets live only if all queued, then applies. A missing record or refused queue or apply clears live, queues every created hook disabled and applies again. The enable receipt is unchanged. | `MH_QueueEnableHook`, `MH_QueueDisableHook`, `MH_ApplyQueued`. |
| Quiesce, stop | Quiesce clears live (release). Stop quiesces, refuses while busy, disables every created hook through the shared disable leaf (`MH_OK` or `MH_ERROR_DISABLED`) without short-circuiting, then requires not busy. Trampolines stay for a later start. | None. |
| Busy and live | Busy is the C++ scope counters, then the resolution owner. Live is a Rust atomic read with acquire by every callback through `rsf_fallout4_render_live`. | Detour-in-flight, WINDOW, FRAME, auxiliary and recovery counters, unchanged. |

The six `RSF_FALLOUT4_*_RENDER` subsystem operations and their `subsystem` leaf are retired from
`rsf_fallout4_platform`; `platform_abi.rs` was edited to match the bindgen layout for the shorter
table. The C++ subsystem wrapped these calls in a catch that answered `RSF_ERROR_NOT_READY` on a
throw. None of the moved calls can throw: MinHook, `GetProcAddress`, the guarded reads and the
noexcept leaves return codes, and the host log keeps its guarded leaf. The GameSDK ABI, the
Runtime export names, the refusal and receipt texts and the engine policy boundary are unchanged.
Recovery reservation, SEAL/FINISH/RETIRE, auxiliary retirement, FRAME/SUBMISSION/WINDOW scope and
every producer callback body stay in C++ for a separate slice.

A scripted comparison against `17904281` matched all eight hook records, seven instruction guards,
seventeen call pairs in order and every refusal and receipt text. Pinned Rust 1.94.1 formatting,
locked offline Cargo metadata and whitespace checks passed. Build, link, Clippy, tests, the shared
host fixture, deployment and game checks remain with the user; this is a source port, not a
runtime result.
