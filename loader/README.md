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
| F5 | Open or close the egui overlay |
| F6 | Reinsert the reconstruction into the game's own frame, or stop |
| F8 | Start DLSS after the game has a device; later presses report counters |
| F7 | Toggle the reconstructed debug image over the back buffer |
| F9 | Apply render scale and the corresponding jitter sequence length |
| F10 | Dump retained velocity/view data and, while DLSS runs, matched colour/output |
| F11 | Trigger RenderDoc when capture support is available |

Start with F8, wait a few frames, then press F8 again to inspect evaluation/refusal counts. F7 shows motion; F10 provides still comparisons. F7 uses a rough tonemap and replaces the visible game frame, so it hides the HUD and does not preserve game grading. It is not output reinsertion.

F5 opens the egui overlay. It comes up as soon as the game has a device, before and independently of
F8, because the state it is most useful in is the one where nothing is running and the panel can say
why. It draws over the finished frame after everything else and is never one of the reconstruction's
inputs. The panel reports what was clicked and does not yet apply it: the hotkeys remain the way to
change anything. The panel DLL is found through `RSF_OVERLAY_DLL`, or beside the proxy.

F6 is that reinsertion, and it is off until asked for because a wrong substitution corrupts the frame. It needs the frame's tail identified first, which the loader learns from the draw into the back buffer over the first few frames after F8, so an immediate press reports what is still missing. It refuses when the game renders at the presented size, which is also what a mission load looks like from inside the frame. With F6 on, the reconstruction runs before the game's tonemap rather than at Present, the game grades it and draws its own interface over it at output resolution, and the last draw into the back buffer becomes a copy. The result has not been looked at yet.

F10 writes `captureNN_*` TGA, JSON, and buffer files. The index restarts with the process and can overwrite earlier captures; use a new directory per run. Velocity previews show unwritten pixels in blue and zero motion in grey. The decoded dump should match the reference decode's range and unwritten fraction.

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
| `RSF_ENABLE_JITTER` | `0`; set `1` to patch the AA gate after decryption |
| `RSF_JITTER_RVA` | Default address `0x112b1f3` |
| `RSF_SCREEN_PERCENTAGE` | `50`; applied by F8/F9 and maintained during the run |
| `RSF_CONSOLE_SINGLETON_RVA` | Default address `0x3a8b290` |
| `RSF_CONSOLE_FIND_SLOT` | Default byte offset `0x90` |
| `RSF_STREAMLINE_BIN` | Directory containing the vendor runtime |
| `RSF_DLSS_QUALITY` | `3`: Performance; backend enum is Native=0, Quality=1, Balanced=2, Performance=3, Ultra Performance=4 |
| `RSF_DLSS_OUTPUT_WIDTH`, `RSF_DLSS_OUTPUT_HEIGHT` | Observer's presented dimensions |
| `RSF_DECODE_MOTION` | `1`; controls decoded diagnostic dumps |
| `RSF_OVERLAY_DLL` | Explicit path to `rescaleframe_overlay.dll`; otherwise looked for beside the proxy |

**Parser limitations:** numeric environment values currently accept positive decimal integers only. Zero falls back to the default, so quality `0` selects Performance and `RSF_DECODE_MOTION=0` does not disable decoding. Address overrides must be decimal, despite the hexadecimal defaults shown above. Quality selection also does not choose `RSF_SCREEN_PERCENTAGE` automatically. These are open [review findings](../docs/review.md).

## Startup and diagnostics

RenderDoc loads during attach to precede device creation. The observer installs early enough to see allocations. A worker waits for code entropy to fall before dumping the decrypted module and applying requested patches. Console writes wait until the engine has created its manager.

The jitter patch checks expected bytes at the default address. A custom RVA currently bypasses that check. These addresses are specific to the researched executable; the proxy does not yet use the plugin's build-recognition gate.

GPU readbacks and evaluation run from Present. `rsf-dump.log` records each readback step before it runs, making the last entry useful when diagnosing a crash. These synchronous captures can stall rendering.

Plain Wine needs more graphics overrides than an existing Proton setup; see [backend notes](../runtime/backends/README.md). Hook rollback, state preservation, and lifecycle problems are recorded in [the review](../docs/review.md).
