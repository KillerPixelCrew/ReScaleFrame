# Loader and research proxy

`rsf_bootstrap` exports a version only. The product launcher, early-loading handshake, and plugin lifecycle are pending. Current AC7 experiments use `proxy/`, built as `dinput8.dll`, with helpers in `diagnostics/` and `runtime/`.

## Research setup

Place the proxy beside `Ace7Game.exe`. Under Proton, select it with `WINEDLLOVERRIDES="dinput8=n,b"`. It forwards AC7's imported `DirectInput8Create` to the system DLL.

Keep the separately obtained NVIDIA runtime in a directory visible to the game, for example `ReScaleFrame/streamline/` beside the executable. The tested set includes `sl.interposer.dll`, `sl.common.dll`, `sl.dlss.dll`, `sl.pcl.dll`, and `nvngx_dlss.dll`. See [dependencies](../docs/dependencies.md) for versions and terms.

Example Steam launch options, on one line, with both paths replaced by Windows paths visible inside Proton:

```text
PROTON_ENABLE_NVAPI=1 RSF_OBSERVE=1 RSF_ENABLE_JITTER=1 RSF_DUMP_DIR="Z:\path\to\captures" RSF_STREAMLINE_BIN="Z:\path\to\ReScaleFrame\streamline" WINEDLLOVERRIDES="dinput8=n,b" %command%
```

The proxy starts a hotkey worker on attach. Omitting `RSF_DUMP_DIR` disables the module-dump worker; it does **not** make the whole proxy inert. Remove the proxy and its launch override to stop loading it. Memory patches do not edit the executable on disk.

## Hotkeys

| Key | Action |
| --- | --- |
| F4 | Open or close the engine's temporal jitter gate, live |
| F5 | Open or close the egui overlay |
| F6 | Reinsert the reconstruction into the game's own frame, or stop |
| F8 | Start DLSS after the game has a device; later presses report counters |
| F7 | Toggle the reconstructed debug image over the back buffer |
| F9 | Apply render scale and the corresponding jitter sequence length |
| F10 | Dump retained velocity/view data and, while DLSS runs, matched colour/output |
| F11 | Trigger RenderDoc when capture support is available |

F4 exists because a shimmering front end has two possible causes that no counter separates: an
offset nothing resolves, or a resolve that fails on elements with no motion vectors. Hold still on
the main menu and press it. If the shimmer stops, the jitter is the cause; if it does not, the
resolve is, and the jitter is only what makes it visible.

Start with F8, wait a few frames, then press F8 again to inspect evaluation/refusal counts. F7 shows motion; F10 provides still comparisons. F7 uses a rough tonemap and replaces the visible game frame, so it hides the HUD and does not preserve game grading. It is not output reinsertion.

F5 opens the egui overlay, and the panel now drives the session rather than reporting on it: start
the backend, set the render scale, toggle the debug view and reinsertion, dump a frame, take a
capture. It draws its own mouse cursor, because AC7 is played with a pad and hides the system one, so
a panel that answers a mouse would otherwise be unusable. Requests are applied on the render thread
at the point in the frame the panel was drawn from, which is the boundary the
[review](../docs/review.md) asks for and something a hotkey worker cannot offer.

It comes up as soon as the game has a device, before and independently of the backend, because the
state it is most useful in is the one where nothing is running and the panel can say why. It draws
over the finished frame after everything else and is never one of the reconstruction's inputs. The
panel DLL is found through `RSF_OVERLAY_DLL`, or beside the proxy.

The function keys below still work and are the fallback while the panel is unproven in game. They
come out once it is confirmed working there; removing the only control path before its replacement
has ever run would leave nothing to fall back to.

F6 is that reinsertion, and it is off until asked for because a wrong substitution corrupts the frame. It needs the frame's tail identified first, which the loader learns from the draw into the back buffer over the first few frames after F8, so an immediate press reports what is still missing. It refuses when the game renders at the presented size, which is also what a mission load looks like from inside the frame. With F6 on, the reconstruction runs before the game's tonemap rather than at Present, the game grades it and draws its own interface over it at output resolution, and the last draw into the back buffer becomes a copy. The result has not been looked at yet.

`RSF_TRANSLUCENT_VELOCITY=1` removes the blend-mode rejection in the velocity pass, so translucent
geometry can write motion vectors. Stock 4.18 excludes it, which is why AC7's mission map relief and
the vehicle symbols in replay have none, and why that layer also writes no depth to fall back on.
Only the rejection is removed; the material-domain check, the movable test and `SupportsVelocity`
still apply, so a material with no usable velocity permutation refuses rather than drawing wrongly.
It patches after decryption and checks the expected bytes first, refusing on an unrecognised build.

Whether anything is gained is a question for the velocity target, not the log line. Compare an F10
dump with the patch off and on in the same scene: `captureNN_0` is the raw velocity target and its
JSON records the unwritten fraction. The briefing screen cannot answer it, because nothing there
writes velocity at all. Mission replay is the case this exists for.

F10 writes `captureNN_*` TGA, JSON, and buffer files. The index restarts with the process and can overwrite earlier captures; use a new directory per run. Velocity previews show unwritten pixels in blue and zero motion in grey. The decoded dump should match the reference decode's range and unwritten fraction.

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
| `RSF_DUMP_DIR` | Module/capture output directory; created if missing |
| `RSF_OBSERVE` | `0`; set `1` to install the D3D11 observer |
| `RSF_OBSERVE_FORMAT` | `35`, `R16G16_UNORM` |
| `RSF_OBSERVE_MIN_WIDTH` | `512` |
| `RSF_OBSERVE_CAPACITY` | `8` retained textures |
| `RSF_VIEW_CB_MIN`, `RSF_VIEW_CB_MAX` | `1024`, `8192` bytes |
| `RSF_RENDERDOC_DLL` | Explicit Windows RenderDoc DLL path |
| `RSF_CAPTURE_PREFIX` | RenderDoc output prefix |
| `RSF_UI_CLASSIFY` | `1`; name pipeline objects as the game creates them and classify the draws made from them. Reports through the `ui:` lines and changes nothing |
| `RSF_UI_WIDGET_WIDTH`, `RSF_UI_WIDGET_HEIGHT` | `1920`, `1080`; the size a widget converter rasterizes the interface at |
| `RSF_ENABLE_JITTER` | `0` off; `1` patches the AA gate but opens it only while a reconstruction runs; `2` opens it from decryption to exit, which is what every result before this used |
| `RSF_JITTER_RVA` | Default address `0x112b1f3` |
| `RSF_TRANSLUCENT_VELOCITY` | `0`; set `1` to let translucent draws reach the velocity pass |
| `RSF_TRANSLUCENT_VELOCITY_RVA` | Default address `0x11823de` |
| `RSF_SCREEN_PERCENTAGE` | `50`; applied by F8/F9 and maintained during the run |
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
| `RSF_TRANSLUCENCY_TARGET`, `RSF_TRANSLUCENCY_TARGET_HEAVY` | `0` (match the scene), `100` (native): the layer's resolution as a percentage of the presented size, derived against the render scale in effect |
| `RSF_TRANSLUCENCY_HEAVY_INDICES` | `100000`; a frame whose layer draws more indices than this is heavy (the briefing) |
| `RSF_TRANSLUCENCY_SCALE` | `0`; a direct multiplier in percent overriding both targets |
| `RSF_REINSERT_DEPTH` | `0`; what F6 does when a promoted target meets the game's render-resolution depth: drop, keep, refuse |

The keys the [representation plan](../docs/representation-plan.md) introduces (`RSF_UI_*`, `RSF_POLICY_*`, `RSF_PRESENTATION`, `RSF_FG*`, `RSF_SR_VENDOR`, vendor runtime directories) are documented here as each milestone lands, not before.

Quality selection does not choose `RSF_SCREEN_PERCENTAGE` automatically; the [representation plan](../docs/representation-plan.md) derives the render scale from the vendor's plan per quality level.

## Startup and diagnostics

RenderDoc loads during attach to precede device creation. The observer installs early enough to see allocations. A worker waits for code entropy to fall before dumping the decrypted module and applying requested patches. Console writes wait until the engine has created its manager.

The jitter patch checks expected bytes at the default address. A custom RVA currently bypasses that check. These addresses are specific to the researched executable; the proxy does not yet use the plugin's build-recognition gate.

GPU readbacks and evaluation run from Present. `rsf-dump.log` records each readback step before it runs, making the last entry useful when diagnosing a crash. These synchronous captures can stall rendering.

Plain Wine needs more graphics overrides than an existing Proton setup; see [backend notes](../runtime/backends/README.md). Hook rollback, state preservation, and lifecycle problems are recorded in [the review](../docs/review.md).
