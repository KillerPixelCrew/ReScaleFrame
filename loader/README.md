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

It is inert unless `RSF_DUMP_DIR` is set. Reverting is deleting that one file.

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
| F9 | Set `r.ScreenPercentage`, and `r.TemporalAASamples` to match |
| F10 | Dump velocity targets, view constant buffers, and a buffer size histogram |
| F11 | Trigger a RenderDoc capture |

Each F10 writes its own `captureNN_*` set. The index resets per process, so a short run overwrites
the low numbered files of a longer earlier one.

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
