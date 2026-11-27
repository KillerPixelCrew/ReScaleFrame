# Steam overlay site discovery

The Skyrim SE and Fallout 4 adapters forward the swapchain methods Steam's overlay intercepts
through `integrations/steam`. It used to accept only one `gameoverlayrenderer64.dll` build (file
SHA-256 plus fixed RVAs), so every Steam client update refused it. Since 2026-11-24
`integrations/steam/src/discovery.rs` finds the overlay code it patches in the loaded module, and
the overlay's SHA-256 is written to the log only (AGENTS.md Rule 4).

## What is patched and how it is found

Steam keeps each original swapchain method it hooked in a saved slot and loads it with a
`mov rax,[rip+slot]` or `mov r10,[rip+slot]` before calling or tail-jumping through it. The
forwarding replaces each complete 7-byte load with a jump to a relay stub that loads the
adapter's handler instead, and hooks the Present and Present1 handler entries so a facade enters
the Runtime before Steam takes its global Present lock.

- **Ten loads and two entries** are matched by patterns unique in the overlay's executable
  sections, with rel32 branches and RIP displacements as wildcards. Each load pattern covers the
  load and the call or tail jump after it, which the old guard checked as 16 fixed following
  bytes. The patterns were derived from the installed overlay with
  `tools/derive-site-patterns.py` and `docs/research/evidence/steam-overlay-sites-11.23.65.88.spec`.
- **Saved slots** (one per method: Release, Present, SetFullscreenState, ResizeBuffers,
  Present1, SetColorSpace1, ResizeBuffers1) are the RIP operands of their loads; the two loads of
  Release, Present and Present1 must name the same slot.
- A site that is missing, ambiguous or disagrees refuses the forwarding with
  `Steam swapchain forwarding refused: site 'X' was not found in gameoverlayrenderer64.dll
  SHA-256 <hash>.`; every pattern is matched again in live memory just before the hooks are
  written (`site 'X' changed since discovery`). The relay stub copies the load's own REX, opcode
  and ModRM from the live site, which its pattern fixes.

## Verification

`discovery::tests::every_site_resolves_in_the_installed_overlay` reads the overlay installed with
Steam (`RSF_STEAM_OVERLAY` names another copy), resolves every site and slot and, for the recorded
build, compares them with its RVAs. On 2026-11-24 the installed overlay 11.23.65.88 (13 November,
SHA-256 `21c0c1dd...d5a6e52f`, PE timestamp `0x6af66f0a`) resolved to:

| Site | RVA |
| --- | --- |
| Release loads | `0x9c32f`, `0x9c375` |
| Present loads | `0x9c154`, `0x9c23e` |
| SetFullscreenState load | `0x9c6cf` |
| ResizeBuffers load | `0x9c5bc` |
| Present1 loads | `0x9bf72`, `0x9c05d` |
| SetColorSpace1 load | `0x9c630` |
| ResizeBuffers1 load | `0x9c4df` |
| Present / Present1 entries | `0x9c130`, `0x9bf50` |
| Saved slots, by method | `0x1834b0`, `0x1834b8`, `0x1834c8`, `0x1834c0`, `0x1834d0`, `0x1834d8`, `0x1834e0` |

Discovery in a running game and against a later overlay build have not been observed yet. A
Steam update that changes the patched code itself still refuses, naming the site, rather than
being followed.
