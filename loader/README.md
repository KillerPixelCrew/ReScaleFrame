# Loader and research proxy

`rsf_bootstrap` exports a version only. The product launcher, early-loading handshake, and plugin lifecycle are pending. Current AC7 experiments use `proxy/`, built as `dinput8.dll`, with helpers in `diagnostics/` and `runtime/`.

## Research setup

Place the proxy beside `Ace7Game.exe`. Under Proton, select it with `WINEDLLOVERRIDES="dinput8=n,b"`. It forwards AC7's imported `DirectInput8Create` to the system DLL.

Keep the separately obtained NVIDIA runtime in a directory visible to the game, for example `ReScaleFrame/streamline/` beside the executable. The tested set includes `sl.interposer.dll`, `sl.common.dll`, `sl.dlss.dll`, `sl.pcl.dll`, and `nvngx_dlss.dll`. See [dependencies](../docs/dependencies.md) for versions and terms.

Example Steam launch options, on one line, with both paths replaced by Windows paths visible inside Proton:

```text
PROTON_ENABLE_NVAPI=1 RSF_OBSERVE=1 RSF_ENABLE_JITTER=1 RSF_DUMP_DIR="Z:\path\to\captures" RSF_STREAMLINE_BIN="Z:\path\to\ReScaleFrame\streamline" WINEDLLOVERRIDES="dinput8=n,b" %command%
```

## Playing

DLSS and reinsertion start automatically after the executable is ready and the presenting device
exists. The first few rendered frames identify the scene and its composition path. Startup does
not require a capture directory or a developer hotkey. Missing backend support leaves DLSS inactive
and reports the reason.

**Insert** opens or closes the overlay. Its only controls are **Enable DLSS** and the five presets.
The old function-key actions are removed. Disable restores 100% scene resolution, stops reinsertion
and closes the forced jitter gate. Native selects DLAA with reinsertion at output resolution.

The selected preset determines screen percentage through DLSS's render-size query, both at startup
and when changed. The setter accepts the last successfully applied value or the game's reset to
100%; it no longer assumes every preset change starts at 100%. A refused game write rolls the
backend mode back. Changes reset temporal history and wait for a fresh evaluated frame before
rebuilding reinsertion. The backend remains loaded while disabled so another preset can be selected.

Choices persist in `%LOCALAPPDATA%\ReScaleFrame\AC7.ini`, independently of the installation's
advanced settings. Without saved choices, DLSS starts enabled with Performance selected.

The briefing layer targets 100% of output resolution at every scene quality, using the
existing expected-byte-checked allocation-scale patch. It uses an unjittered view in both VS and PS
and is recombined after scene SR, before tonemapping. This replaces the default secondary DLSS
integration. See [the implementation and validation limits](../docs/research/ac7-consumer-session.md).

## Settings

Settings live in `ReScaleFrame.ini` beside the proxy, one `NAME=value` per line, `#` or `;` starting
a comment, read once at attach. [`ReScaleFrame.ini.sample`](ReScaleFrame.ini.sample) is a starting
point. The names are the ones below, unchanged, because they were environment variables first and an
environment variable of the same name still wins over the file. That keeps an existing launch line
working and makes a one-off override a launch option rather than an edit.

Numbers accept hexadecimal with an `0x` prefix, and a setting present and zero is that value rather
than an absence, so `RSF_DECODE_MOTION=0` disables decoding and quality `0` selects Native. Both
were [review findings](../docs/review.md) against the old parser.

The vendor runtime is found at `ReScaleFrame\streamline` beside the proxy, and `renderdoc.dll`
beside the proxy, so neither path normally needs setting at all.

## Environment settings

| Variable | Default / purpose |
| --- | --- |
| `RSF_DUMP_DIR` | Logs/captures; defaults to `%LOCALAPPDATA%\ReScaleFrame\AC7` |
| `RSF_DUMP_MODULE` | `0`; opt in to executable dumps and module sampling |
| `RSF_DLSS_ENABLE` | `1`; initial enable choice before saved preferences |
| `RSF_OBSERVE` | `1`; install the D3D11 observer |
| `RSF_OBSERVE_FORMAT` | `35`, `R16G16_UNORM` |
| `RSF_OBSERVE_MIN_WIDTH` | `512` |
| `RSF_OBSERVE_CAPACITY` | `8` retained textures |
| `RSF_VIEW_CB_MIN`, `RSF_VIEW_CB_MAX` | `1024`, `8192` bytes |
| `RSF_RENDERDOC_DLL` | Explicit Windows RenderDoc DLL path |
| `RSF_CAPTURE_PREFIX` | RenderDoc output prefix |
| `RSF_UI_CLASSIFY` | `1`; name pipeline objects as the game creates them and classify the draws made from them. Reports through the `ui:` lines and changes nothing |
| `RSF_UI_SHADER_FORCE`, `RSF_UI_SHADER_SKIP` | hex hash lists naming shaders the rules got wrong, in either direction. The hashes are printed by the `ui draw:` trace lines |
| `RSF_UI_ENCODE` | `1`; which transfer function the composite applies to the extracted interface. `0` none, `1` sRGB, `2` gamma 2.2 |
| `RSF_ENABLE_JITTER` | Default `1`; `0` off; `1` patches the AA gate but opens it only while a reconstruction runs; `2` opens it from decryption to exit, which is what every result before this used |
| `RSF_JITTER_RVA` | Default address `0x112b1f3` |
| `RSF_TRANSLUCENT_VELOCITY` | `0`; set `1` to let translucent draws reach the velocity pass |
| `RSF_TRANSLUCENT_VELOCITY_RVA` | Default address `0x11823de` |
| `RSF_SCREEN_PERCENTAGE` | Legacy diagnostic setting; consumer startup uses the selected DLSS preset |
| `RSF_CONSOLE_SINGLETON_RVA` | Default address `0x3a8b290` |
| `RSF_CONSOLE_FIND_SLOT` | Default byte offset `0x90` |
| `RSF_STREAMLINE_BIN` | Directory containing the vendor runtime |
| `RSF_DLSS_QUALITY` | `3`: Performance; backend enum is Native=0, Quality=1, Balanced=2, Performance=3, Ultra Performance=4 |
| `RSF_DLSS_OUTPUT_WIDTH`, `RSF_DLSS_OUTPUT_HEIGHT` | Observer's presented dimensions |
| `RSF_DECODE_MOTION` | `1`; controls decoded diagnostic dumps |
| `RSF_OVERLAY_DLL` | Explicit path to `rescaleframe_overlay.dll`; otherwise looked for beside the proxy |
| `RSF_RENDERDOC` | `0`; set `1` to load RenderDoc at attach for a capture session |
| `RSF_FULL_TRANSLUCENCY` | `1`; patches the separate translucency halving out and carries the scale as a rewritable immediate |
| `RSF_FULL_TRANSLUCENCY_RVA` | Default address `0x10be329` |
| `RSF_TRANSLUCENCY_TARGET` | `100`; percent of output, independent of the scene preset. `0` explicitly matches the scene |
| `RSF_TRANSLUCENCY_UNJITTER` | `1`; unjitter VS/PS and bypass layer DLSS. `0` retains the experimental 1:1 temporal route |
| `RSF_TRANSLUCENCY_SCALE` | `0`; a direct multiplier in percent overriding the target |
| `RSF_REINSERT_DEPTH` | `0`; what reinsertion does when a promoted target meets the game's render-resolution depth: drop, keep, refuse |

The keys the [representation plan](../docs/representation-plan.md) introduces (`RSF_UI_*`, `RSF_POLICY_*`, `RSF_PRESENTATION`, `RSF_FG*`, `RSF_SR_VENDOR`, vendor runtime directories) are documented here as each milestone lands, not before.

Quality selection does not choose `RSF_SCREEN_PERCENTAGE` automatically; the [representation plan](../docs/representation-plan.md) derives the render scale from the vendor's plan per quality level.

## Startup and diagnostics

The worker waits for decrypted code before applying expected-byte-checked patches. The Present
callback starts the backend once the device and dimensions are known; it does not repeatedly load
a failing vendor runtime. Use Enable DLSS to retry after a failure. Renderer maintenance runs on
the render thread. Module dumps are opt-in; vendor informational spam is not forwarded to the log.
Warnings, errors and state changes remain available in `rsf-dump.log`.

The proxy and overlay are still the research carrier, not the completed plugin lifecycle or product
installer. Synthetic Windows checks do not establish briefing image quality or flight regression.

## Development SR backend switching

The current development overlay offers DLSS, FSR2, FSR3, FSR4 and XeSS after startup. Selection is
applied on the render thread; a refused backend keeps the active one. Existing startup is DLSS-first.
The published 0.1.0 package contains DLSS only and is not automatically updated by these changes.
New FSR/XeSS paths are built and synthetic device-tested, not yet game-tested in AC7.

`RSF_FFX_BIN` selects the absolute directory containing `amd_fidelityfx_upscaler_dx12.dll`.
It defaults to `ReScaleFrame\fidelityfx` beside the proxy. SDK 2.3.0 can supply all three FSR
families. `RSF_FSR2_BIN`, `RSF_FSR3_BIN` and `RSF_FSR4_BIN` optionally select separate SDK runtime
directories. `RSF_XESS_BIN` selects the directory containing `libxess.dll`, defaulting to
`ReScaleFrame\xess`. Settings use the existing INI/environment precedence. No new vendor binaries
are installed by building or testing; preserve their distribution terms when preparing a package.

[Architecture, SDK revisions, device checks and remaining validation](../docs/research/orchestrator-sr-switching.md).
