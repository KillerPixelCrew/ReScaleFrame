# Loader

The standalone launcher or WSGM will arrange early loading of this bootstrap. The bootstrap will
load the orchestrator; the orchestrator selects and prepares the game plugin before activating the
pipeline.

`rsf_bootstrap` still exports its version only. Injection, startup interception, the runtime
handshake and the DXGI shim fallback are pending. Keep heavy initialization and waits outside
`DllMain`.

## What is here now

| Directory | Contents |
| --- | --- |
| `diagnostics/` | Entropy measurement, module capture, import map, code patching, console variables |
| `proxy/` | The research carrier, built as `dinput8.dll` |

These are research tools. They read the process, write files, and apply patches that are asked for
explicitly. Nothing here is part of the shipping load path, and none of it touches a graphics
object beyond the observer in `runtime/graphics`.

## The research carrier

Built as `dinput8.dll` and deployed beside the game executable, selected with
`WINEDLLOVERRIDES="dinput8=n,b"` under Proton. AC7 imports exactly one function from that DLL and
nothing else in the process imports from it, so the forwarding surface is a single export and DXVK
is left alone. It loads as a static import, which puts it in place before the executable's entry
point runs.

It is inert unless `RSF_DUMP_DIR` is set.

Running DLSS needs the vendor runtime somewhere the game can reach. Put it in the game folder
rather than pointing at a checkout: under Proton the game runs in a container, and a path into a
source tree is one more thing that can be wrong for a reason that looks like DLSS failing. So the
deployed layout is:

```
ACE COMBAT 7/
  dinput8.dll                        the research carrier
  ReScaleFrame/streamline/           sl.interposer.dll, sl.common.dll, sl.dlss.dll,
                                     sl.pcl.dll, nvngx_dlss.dll
```

Those five are NVIDIA's, under the NVIDIA RTX SDKs License, and this project neither ships nor
redistributes them: copying them there is the user's own act under that license. Reverting the
whole thing is deleting `dinput8.dll` and the `ReScaleFrame` directory. Nothing else in the game
folder is written to, and no game file is modified.

| Variable | Effect |
| --- | --- |
| `RSF_DUMP_DIR` | Output directory. Required; created if missing |
| `RSF_OBSERVE=1` | Install the D3D11 observer |
| `RSF_OBSERVE_FORMAT` | DXGI format to retain, default 35 (`R16G16_UNORM`) |
| `RSF_OBSERVE_MIN_WIDTH` | Ignore targets narrower than this, default 1024 |
| `RSF_VIEW_CB_MIN` / `RSF_VIEW_CB_MAX` | Constant buffer size range to retain, default 1024 to 8192 |
| `RSF_RENDERDOC_DLL` | Path to a Windows `renderdoc.dll`; enables capture |
| `RSF_CAPTURE_PREFIX` | Where captures are written |
| `RSF_ENABLE_JITTER=1` | Patch the temporal jitter gate |
| `RSF_JITTER_RVA` | Override the gate address, default `0x112b1f3` |
| `RSF_SCREEN_PERCENTAGE` | Value F9 applies, default 50 |
| `RSF_CONSOLE_SINGLETON_RVA` | Console manager singleton, default `0x3a8b290` |
| `RSF_CONSOLE_FIND_SLOT` | `FindConsoleVariable` vtable offset, default `0x90` |

| Key | Action |
| --- | --- |
| F8 | Start DLSS, or report its counters if it is already running |
| F9 | Set `r.ScreenPercentage`, and `r.TemporalAASamples` to match |
| F10 | Dump velocity targets raw and decoded, view constant buffers, and a buffer size histogram |
| F11 | Trigger a RenderDoc capture |

Each F10 writes its own `captureNN_*` set. The index resets per process, so a short run overwrites
the low numbered files of a longer earlier one.

Alongside each raw target it writes a `_decoded` pair, produced by running the motion decode pass a
backend depends on. That pass exists because no backend can read Unreal's storage directly: they
take a scale factor for motion and nothing to subtract a bias with. Comparing the two images is how
the decode gets checked against the game's own buffer rather than only against values a test made
up. In the decoded image, blue marks the sentinel the pass wrote where the source held its clear
value, and mid grey is zero motion, the same convention the raw velocity view uses.
`RSF_DECODE_MOTION=0` turns it off.

## Running DLSS

The order matters, and each step is a key rather than automatic because each one wants the game to
be somewhere in particular.

| Step | Key | Why then |
| --- | --- | --- |
| 1 | F9, in flight | Revives the jitter path and halves the render scale. Without a jitter there are no extra sub-pixel samples, and the pipeline refuses the frame rather than producing something quietly soft. |
| 2 | F8 | Starts DLSS. The game's device has to exist, and by the time you can press a key it does. |
| 3 | F10 | Writes the inputs and, when DLSS is running, its output beside them. |

As Steam launch options, with the deployed layout above:

```
PROTON_ENABLE_NVAPI=1 RSF_OBSERVE=1 RSF_ENABLE_JITTER=1
RSF_DUMP_DIR="Z:\home\n1ght\Projekte\ReScaleFrame\.local\observe"
RSF_STREAMLINE_BIN="Z:\home\n1ght\.local\share\Steam\steamapps\common\ACE COMBAT 7\ReScaleFrame\streamline"
WINEDLLOVERRIDES="dinput8=n,b" %command%
```

All on one line. `RSF_DLSS_QUALITY` picks a level, 0 native through 4 ultra performance, defaulting
to 3, and `RSF_DLSS_OUTPUT_WIDTH` and `_HEIGHT` override the presented size the observer reports.

Proton already installs DXVK, vkd3d-proton and DXVK-NVAPI into the prefix and selects them, so the
launch options only have to add `dinput8`. Running the same binaries under plain Wine does not, and
then the whole list is needed:

```
WINEDLLOVERRIDES="d3d11,d3d12,d3d12core,dxgi,nvapi,nvapi64,nvofapi64,nvngx,_nvngx=n,b;dinput8=n,b"
```

`d3d12` belongs in that list even though nothing here wants D3D12: Streamline runs its own compute
through a DX11-on-12 device, and without it DLSS reports unsupported for a reason that looks
nothing like the cause. See `runtime/backends/README.md`.

F8 prints counters, and they are the point. A run that produces no image should be able to say
which step it stopped at, and each of these has been the answer at some point:

```
frame tap: 41230 calls inspected, 118 passes matched, render 1024x576
bridge: 118 passes, 0 view reads failed, 6 not the main view, 0 without jitter, 112 evaluated, 0 refused
pipeline: running 1, dlss supported 1, render 1024x576, output 2048x1152, evaluated 112, refused 0
```

Passes matched but no evaluates means the camera never arrived: either the view buffer was not the
one bound at that pass, or every frame was a secondary view. Evaluates with no picture is a
different problem entirely, and the reason F10 writes the output out.

Nothing here reinserts the result into the game. DLSS evaluating is not the same as DLSS being
visible, and the second one is not built yet.

F9 sets the jitter sequence length as well as the render scale, because 4.18 does not tie the two
together and a reconstruction wants the sequence to grow with the area ratio: 8 samples at full
scale, 32 at half. The count is written at the offset the screen percentage variable reported,
since `TConsoleVariableData` puts its two thread copies in the same place for every variable of the
same element size, and only when both copies already hold the engine default of 8. Untested in the
game so far; the log line says which way it went.

The dump runs inside `Present`, on the game's own render thread, so a fault there is a closed game
and no result code. Every step is written to `rsf-dump.log` before it is taken, one line at a time
with the file closed between lines, which makes the last line the step that did not survive:

```
dump begin: 3 textures, 1 constant buffers
texture 1 of 3
dump ...\capture00_0: 1024x576 format 35 mips 1 slices 1 samples 1
dump: creating staging copy
dump: copying (whole resource)
dump: mapping
dump: decoding 576 rows
dump: writing ...\capture00_0.tga
dump: done, 8.3% unwritten
```

A run that ends at `dump begin` never reached the first texture; one that ends at `copying` names
the resource in the line above it. Two cases are refused rather than attempted, because both are
invalid D3D11 that faults instead of returning a failure: a resource belonging to a different
device than the one dumping it, and a `CopyResource` between a mip chain and the flat staging copy.

## Order of operations, and why it is that order

1. **RenderDoc loads on attach**, not on the worker, because it has to be in place before the
   graphics device is created.
2. **The observer installs next.** Its texture creation hook only sees textures made after it is
   in place, and the targets worth having are allocated during engine startup.
3. **The module dump waits for the code to decrypt**, measured by entropy rather than a timer, so
   it self-verifies.
4. **Patches apply after that.** Patching earlier writes into ciphertext about to be overwritten.
5. **Console variables come later still**, because the manager does not exist until the game asks
   the engine for one. This deliberately does not construct it: running engine initialization at a
   moment of our choosing is a worse bug than not setting a variable.

## Rules this code follows

- **Nothing calls D3D11 while holding the observer's lock.** A hook runs inside the code it hooks,
  so a call made from one can re-enter it, and a non-recursive lock then deadlocks the process.
- **Resource reads happen inside present.** A device context cannot be used from two threads at
  once; reading from a worker races the game's rendering, returns whatever the staging copy held,
  and can take the process down. Both happened during development.
- **Patches state the bytes they expect and refuse otherwise.** A patch aimed at the wrong address
  is far worse than no patch, and a game update lands exactly there.
- **Values are found, not assumed.** Console variable writes search the object for the value it is
  known to hold and replace every copy, because a guessed struct offset silently corrupts a
  neighbour.

See [methodology.md](../docs/research/methodology.md) for the procedure these implement, and
[ac7-frame-capture.md](../docs/research/ac7-frame-capture.md) for what they established.
