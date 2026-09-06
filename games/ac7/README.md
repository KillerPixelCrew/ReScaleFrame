# Ace Combat 7

First game target: Windows x64, D3D11, a vendor branch of UE4.18, Steam application 502500, build
9855922. The plugin recognizes that build by executable name, PE machine and SHA-256.

[`engine.json`](engine.json) records what was established about this game and how each claim was
established, since no public database records an engine's minor version. That file is the summary;
this one is the reasoning.

`rsf_game_info.rendering_ready` is still `0`, and the contract test asserts it. Nothing here renders
anything yet. What follows is what the plugin knows, what has been established about the game, and
what is still guessed at.

## Static analysis does not work on this executable

`.text` is encrypted on disk. Entropy is 7.997 across all 38.9 MB, the PE entry point sits in a
trailing `.bind` section belonging to the Steam DRM wrapper, and nothing in the file references the
`d3d11.dll` or `dxgi.dll` import slots. Only `.rdata` is plaintext, which is why string and
interface-ID research worked and nothing else did.

Everything below therefore comes from the module as it exists in memory after the wrapper has
decrypted it, or from the running game. See
[ghidra-tooling.md](../../docs/research/ghidra-tooling.md) for the dump route.

## What the plugin contains

`rsf_ac7_view` reads the view uniform buffer, which is where the camera data a reconstruction
backend cannot do without lives: projection, camera basis, position, `ClipToPrevClip`, the jitter,
and the view and buffer sizes. A frame capture cannot supply any of it.

It refuses rather than guesses. At runtime nothing labels a constant buffer, so the reader checks
relationships that hold in a view buffer and essentially nowhere else, and reports which view a
buffer describes. Over the captured set it recognizes all fifty as view buffers, accepts ten as
perspective views, refuses the rest as the orthographic views the interface renders through, and
marks the 1016x1016 and 128x93 viewports as secondary rather than the view the player looks
through. Handing a backend the camera of one of those would produce a plausible, wrong image.

The layout is stock 4.18 with one difference: `ViewToClipNoAA` does not exist in 4.18, so every
field after `ViewToClip` sits `0x40` earlier than a later engine puts it. That single shift is why
reading the stock layout stops working partway through. Offsets, and the identities that establish
them, are in [ac7-frame-capture.md](../../docs/research/ac7-frame-capture.md), and
`tools/verify-view-layout.py` re-checks them against captured buffers.

## What is established about the game

- **Temporal AA runs, without jitter.** The menu offers only FXAA and none, but the second pass of
  the frame binds colour, history, velocity, depth and a 1x1 exposure target together, which is
  `FRCPassPostProcessTemporalAA` and nothing else. Every super resolution input a backend needs is
  already bound at one place, and that place exists without anything being built.
- **The jitter can be revived.** `PreVisibilityFrameSetup` computes an offset only behind
  `View.AntiAliasingMethod == AAM_TemporalAA`. Stepping over one conditional jump at RVA
  `0x112b1f3`, six bytes, runs it whatever the menu says, and the engine then applies the offset to
  the projection itself so every derived matrix and the velocity buffer stay consistent. Measured
  after the patch: -0.147, +0.065 and -0.430 pixels, all inside half a pixel.
- **Render scale works** through `r.ScreenPercentage`, confirmed visually. In 4.18 that one variable
  is the whole mechanism. Reaching it needs the console manager singleton at RVA `0x3a8b290` and
  `FindConsoleVariable` at vtable offset `0x90`.
- **Velocity is object motion only**, biased into a sixteen bit target as
  `In * (0.499 * 0.5) + 32767/65535`, with a raw zero reserved to mean nothing wrote the pixel.
  Confirmed from pixels: 83.6% of a camera-panning frame is exactly zero.
- **The interface composites at render resolution** and is upscaled with the scene, so the
  insertion point is before that composite, not at the final draw.

## What is not established

- No hooks. Every RVA above is used by the research proxy in `loader/proxy`, which is a diagnostic
  carrier and not this plugin. They belong here once the architecture split happens, and until then
  this plugin holds no addresses.
- The velocity sign and axis convention a backend wants. The decode pass takes them as parameters
  for that reason, and only a rendered result settles which.
- Whether the identified targets keep their identities across missions, weather and menus. One
  session's captures are not a guarantee.

## Ownership

Game detection, hook sites, engine structure offsets and the velocity encoding belong here. Vendor
SDKs, GPU resources and presentation belong in `runtime/`. The AC7 addresses currently living in the
research proxy are on the wrong side of that line and are there because the proxy is what can be
loaded into the game today, not because it is where they go.

See [the source research](../../docs/research/ue418-hook-map.md) for the UE4.18 hook map and
[ac7-frame-capture.md](../../docs/research/ac7-frame-capture.md) for the frame analysis.
