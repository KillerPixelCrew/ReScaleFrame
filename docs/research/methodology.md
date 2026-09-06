# Working out how a game renders

The procedure that produced the AC7 results, written down so the next game costs less than the
first one did. It assumes a shipped Unreal Engine title with no debug symbols, no console, and
possibly no readable code.

The order matters. Each step either answers a question or tells you the cheaper way to answer it,
and several steps here exist because an earlier attempt failed in an instructive way.

## 1. Ask whether the file can be read at all

Before any disassembly, measure the entropy of the code section.

```bash
python3 tools/inspect-game.py "<game>.exe" .local/fingerprint.json
```

Around 6 to 6.5 bits per byte is ordinary x86. Approaching 8.0 means the section is encrypted or
compressed and nothing static will work on it: no cross references, no call sites, no strings in
code. Check where the entry point lands too. If it sits in a trailing section rather than in
`.text`, a wrapper runs first.

AC7 measured 7.997 across all 38.9 MB with its entry point in a `.bind` section. Hours of Ghidra
analysis on that file produced 136,352 functions of noise. The entropy check takes a second and
would have said so immediately.

Read-only data usually stays plaintext even when code does not, so strings, interface IDs and
vtables remain available. That is worth exploiting before concluding the file is useless.

## 2. If the code is encrypted, capture it from the running process

The protection decrypts the code to run it. Load a small DLL into the process, wait until the code
stops looking like ciphertext, and write the image out.

- Carry the diagnostic on a DLL the game imports **one** function from, and one that nothing else
  in the process imports. Proxying a graphics DLL puts you in front of the renderer for no reason.
- Trigger on the same entropy measurement, not on a timer. It self-verifies: the number that said
  the file was encrypted says when it no longer is.
- Write section raw offsets equal to their virtual addresses and record the real load base, so
  addresses in the dump match the running process.
- Do not rebuild an import table. The dump never needs to run.

Verify the capture rather than trusting it. Count rip-relative calls through the import table: a
protected file has none, because there is no code to contain them. AC7's dump had 19,888.

## 3. Recover meaning from the image

With plaintext code, several cheap techniques beat disassembling by hand.

**Source file attribution.** Unreal's `check` and `ensure` macros embed `__FILE__`, so a shipped
binary keeps the source path of every file containing one, and the code referencing that path came
from that file. `tools/map-source-files.py` turns that into a map. On AC7 it attributed 462 code
regions and put address ranges on hook map entries that had none.

**String references.** `tools/find-string-refs.py` counts code references to a string. Console
variable names are the useful case: the code that registers or looks one up is the code that
implements the feature. This is how the anti-aliasing gate was found.

**Engine source as the key.** With a matching engine checkout, a distinctive call sequence in the
source can be matched to its disassembly directly. `FParse::Param(FCommandLine::Get(), TEXT(...))`
repeated six times is unmistakable, and finding it names two engine functions at once.

## 4. Capture frames, and read them without a Windows machine

In-application RenderDoc works: load `renderdoc.dll` from the diagnostic DLL and drive
`RENDERDOC_GetAPI`. Under Proton this hooks DXVK's D3D11 and produces normal captures.

Two things that are not obvious:

- Allow vendor extensions (`eRENDERDOC_Option_AllowUnsupportedVendorExtensions` with the NVIDIA
  vendor id). Otherwise `nvapi_QueryInterface` returns null and, on hybrid graphics, the device is
  removed at the first present and the game dies. Skyrim Community Shaders hit this and documented
  it; copying that one line saves a confusing session.
- The Linux RenderDoc build has no D3D11 support at all, but the Windows `renderdoccmd.exe` runs
  under Wine and its `convert -c xml` needs no replay device. A 111 MB capture becomes 4.4 MB of
  structured data locally. Only reading pixels needs Windows.

## 5. Identify passes by what they read

A shipping build emits no debug markers, so passes have to be recognised structurally.
`tools/parse-capture.py --timeline` reconstructs them from render target bindings and the draws
between them.

**Match on inputs, not outputs.** This is the single most useful lesson here. Output format and
size identified two candidate passes in AC7 and could not distinguish temporal AA from motion
blur; the guess was made and then unmade twice on that basis. What settled it was the input set:
colour, history, velocity, depth and a 1x1 exposure target is `FRCPassPostProcessTemporalAA` and
nothing else. `--reads <resource>` filters passes by what they sample.

Formats identify candidates. Inputs identify passes.

## 6. Read engine state from the running game

Matrices, jitter and camera parameters live in a constant buffer and never appear in a frame
capture's texture list. Retain the buffer at creation and copy it out.

- **Match a size range, not one size.** Unreal's D3D11 constant buffers land in power-of-two pool
  buckets, so a 2640 byte view structure is allocated as 4096. Matching the stock size exactly
  found nothing at all.
- **Count every size you see.** When the guess misses, the histogram of sizes is what identifies
  the right buffer on the next run.
- **The same buffer is reused for different view types.** Orthographic interface views share it
  with the scene view. One snapshot is not enough to tell which is which; a perspective projection
  in the first matrix is a usable discriminator.

## 7. Confirm offsets rather than trusting them

`tools/ue4-view-layout.py` computes member offsets from the engine's own macro table. Treat the
result as a hypothesis: a vendor branch diverges, and it will do so silently. AC7 matched stock
exactly up to `ClipToPrevClip` at `0x6e0` and diverged after it, by 0x90 in one place and 0xA0 in
another, so not even a constant shift.

Confirm each field by a property only the right field has:

| Field | Property that confirms it |
| --- | --- |
| projection | ratio of the first two diagonal entries is the aspect ratio |
| its inverse | exact reciprocals of the projection |
| camera basis vectors | unit length |
| previous-frame transform | near identity when still, departing when moving |
| viewport size | the actual render dimensions |
| jitter | sub-pixel, changes every frame, straddles zero |

`tools/analyze-view-buffers.py` does this across many captures at once. Capture the same game in
several states, menu, briefing, flight, and let fields identify themselves by behaviour. That
beats trusting source offsets and is the only method that survives a vendor branch.

## 8. Take constants from source, never fit them to data

Unreal encodes velocity as `In * (0.499 * 0.5) + 32767/65535`. Measurements confirmed the bias to
seven decimal places and could say nothing at all about the scale, because every velocity in the
sampled frames was tiny. A decode fitted to that data would have been wrong by a factor of four
and would have looked plausible.

Measurements confirm constants. Source supplies them.

## 9. Re-enable engine features rather than reimplementing them

Where a game ships a feature disabled, turning the engine's own path back on beats building a
replacement beside it.

AC7 exposes no temporal AA, so `PreVisibilityFrameSetup` clears the jitter and never recomputes
it. The whole of it is one branch:

```cpp
if (View.AntiAliasingMethod == AAM_TemporalAA && ViewState) { ...compute jitter... }
```

Stepping over that conditional jump, six bytes, restores it. The engine then applies the offset
through `HackAddTemporalAAProjectionJitter`, which touches the projection before any derived
matrix is built, so every matrix and the velocity buffer stay mutually consistent. Writing jitter
into the constant buffer afterwards would have desynchronised all of it.

Patch defensively: require the bytes already there to be the ones expected, and refuse otherwise.
A patch aimed at the wrong address is much worse than no patch, and a game update lands exactly
there. Patch only after the code is decrypted.

## 10. Check what the framework already does

Before building anything, read what the target SDK expects. `sl::Constants` carries
`cameraMotionIncluded` and `motionVectorsInvalidValue`, and with those set Streamline reconstructs
camera motion from `clipToPrevClip` and depth itself. A motion vector composition pass had been
written into this project's documentation as mandatory before anyone read that header.

## Things that will bite

**A device context is not thread safe.** Reading a resource from a worker thread races the game's
own rendering: it returns whatever the staging copy happened to hold and can take the process
down. Both happened here. Do the work inside present, where you are already on the rendering
thread at a defined point in the frame.

**A hook runs inside the code it hooks.** Any call made from a creation hook can re-enter that
hook, so holding a non-recursive lock across it deadlocks the process. Two deadlocks here, the
same mistake twice: record under the lock, do the work outside it.

**Hook creation, not binding.** Render target binding runs a hundred times a frame and carries no
information that creation does not.

**Capture resource identifiers do not exist at runtime.** Match by signature, format, size and
binding, which is the reason to establish those in the capture analysis.

**Marker colours share channels with data.** An unwritten-pixel marker written into the red
channel read as strong horizontal motion for several sessions.
