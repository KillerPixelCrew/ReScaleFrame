# Fallout 4 producer resolution in Rust

The port moves the decisions and receipts in `resolution.cpp` at `31a06a94` into
`games/fallout4/src/resolution.rs`. It targets Fallout 4 AE 1.11.240.0, executable SHA-256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`. The matched executable
evidence for the resolution writer, viewport builder, full-output scope and fallback remains in
the [output scope](fallout4-native-reduced-output-scope-20261116.md) and
[reduced recovery](fallout4-native-reduced-recovery-20261116.md) notes. This migration used
the pinned C++ and the present Rust screen and depth owners as its source of truth.

| Boundary | Resolution behavior |
| --- | --- |
| After original UpdateDR | Match the borrowed CPU session, source and submission. Check the output dimensions, FE4 and FE8, and TAA state. Ask the host to reconfigure with SR and NO_FG, then read its sized render config. Recheck the same CPU source, the screen owner's pre-world refusal, native fields and frame/view/window/context/chain. Preflight and reserve recovery before writing any reduced ratios. Ratios are floating-point render pixels divided by output pixels. |
| After the selected prepass, before original effects | The depth owner checks the raster-only recovery lease or the temporal snapshot. Resolution borrows its native Context and verifies the frame/view/window/context/chain before saving FE4, FE5, ratios, jitter, shadow viewport, dirty word and triangle identities. It writes unit ratios and invokes the native builder with explicit output pixel dimensions. The resulting shadow XYWH and borrowed geometry must match. |
| Native DR and viewport callbacks inside a scope | A completed DR writer supersedes input ratio and activation writes, even when equal. An output borrow saves the new native values and reapplies unit ratios. The native viewport callback transfers FE4 intent even when it writes the same byte. Native full viewport intent releases the corresponding shadow and dirty obligations. |
| RenderUI return and source retirement at Swap | Restore each pending field only if it still contains the exact bytes written by the scope. Preserve superseding values, flag a changed triangle association, and restore only the viewport bit in the dirty word. Failed reads, writes or owner checks retain outstanding obligations for the existing cleanup retry. |
| Reduced-stage receipts | Rust admits and deduplicates the same eight stages per session and source, with the existing first-four-source and 128-frame sampling rules and two process budgets. It gathers scalar and shadow facts. C++ queries the typed D3D11 viewport only with the matched callback Context and emits the original format under the LastError guard. |

`resolution.cpp` retains the typed viewport builder, the screen query bridge, guarded SDK and
recovery calls, and native viewport reporting. A private, layout-checked C ABI passes values
and short-lived arguments. The existing GameSDK pass and recovery ABI are used as before.
The hook bodies and World fallback stay in `render_hooks.cpp`. No additional callback or
resource lifetime is introduced. Preflight shares the frame/renderer guards with restoration,
so it reads the window before the device context and chain. A guarded read contributes a
complete value to the scope or receipt. These changes do not alter the native callback
sequence or the criteria for committing a reduced frame.

Source comparison against `31a06a94` matched all eleven guarded RVAs and byte sequences,
the native addresses, the eight receipt stages and receipt format, and the public C++
resolution signatures and render hook bodies. Pinned Rust 1.94.1 formatting, locked offline
Cargo metadata and whitespace checks passed. Compile, link, Clippy, tests, deployment,
device checks and game checks remain with the user. Moving and all-quality fallback,
native recovery output and downstream shader/consumer coverage still require game evidence.
