# AC7 F9 renderer-root registration in Rust

This port moves root admission, registration and retirement into the Rust F9 record owner
introduced by the earlier [capture ports](rust-ac7-motion-artifacts.md). The source baseline
is `301ad289d88552521dbb2b48e30f1e41340ae73d`, which includes
`4a488cc2036638c93a2c19e4ea42a87c81e886af`. The hook identities and executable fingerprints
come from the [renderer-root audit](ac7-renderer-roots-20260930.md) and
[hook map](ue418-hook-map.md#native-renderer-and-widget-roots-located-on-30-september).
This is a source comparison of existing sites.

Rust checks the seven bytes at the RHI current-command write `0x120e67c` first, using the
existing native guarded copy. It then checks the selected entries in their original order:
post-process, widget prepare, widget queue, main temporal construction, main temporal
execution and recursive graph execution. Native renderer ownership omits the post-process
and widget queue slots from both the guards and the hook batch. A single ownership snapshot
selects the route for the attempt, and each attempt starts with empty target slots. This
keeps the guarded and installed entries paired and prevents a reinstall from reusing a
target omitted on the new route.

Once the selected guards pass, the native batch creates hooks in slot order, then enables
them in that order. A create failure rolls back the preceding slots. An enable failure rolls
back every created slot. Rollback and retirement still disable and remove each selected
target in ascending slot order through the same native helper. Rust records the exact
successful batch and publishes the current-command address at `0x3c78358` together with
its availability. Shutdown stops the capture session and removes the velocity observations
before retiring the root batch. The typed detours, trampolines, live engine and D3D11 reads,
and artifact writers continue to use the existing paths. A root refusal still allows the
independent velocity hook attempt.

A mechanical parent comparison matched all six entry RVAs, byte spans and lengths, the RHI
guard and its computed global RVA, both owner routes, detour/original table order, the native
MinHook and rollback helper, shutdown order and log text. The queued command/execute reads
and session hook-status field still use root availability. Pinned Rust formatting, locked
offline Windows Cargo metadata and Git whitespace checks passed. Compile and link, Clippy,
tests, fresh F9 capture, device and game verification, deployment and push remain user-owned.
