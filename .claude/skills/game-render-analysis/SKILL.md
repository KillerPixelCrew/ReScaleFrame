---
name: game-render-analysis
description: Analyse how a shipped game renders, so upscaling or frame generation can be integrated. Use when working out a game's render passes, motion vectors, jitter, view matrices or hook points; when a game executable resists static analysis; when reading RenderDoc captures; when reading engine state from a running game; or when patching an engine feature back on. Covers the loader diagnostics, the Ghidra tooling and the capture tooling in this repository.
---

# Analysing how a game renders

The working procedure for this project, with the tools that implement it. Full reasoning and the
AC7 results are in `docs/research/methodology.md` and `docs/research/ac7-frame-capture.md`; this is
the operational version.

## Before anything else

Measure the code section's entropy. `python3 tools/inspect-game.py "<game>.exe" out.json`.

Near 8.0 means encrypted: no static analysis will work, and hours of Ghidra on it produce noise.
Around 6 to 6.5 is ordinary code. Also check whether the entry point lands outside `.text`; if it
does, a wrapper runs first. Read-only data usually stays plaintext either way, so strings,
interface IDs and vtables survive.

Never report a game as analysable, or a hook site as found, on the strength of a string match.
This repository's honesty rule applies to analysis as much as to code.

## Toolchain

Build and test from a Linux checkout:

```bash
cmake --preset linux-cross-x64
cmake --build --preset linux-cross-debug
ctest --preset linux-cross-debug          # runs under Wine
```

MSVC on Windows is the reference build and the only thing CI runs. A mingw and Wine run is real,
but report it as "cross-built and tested under Wine", never as verified on Windows.

## Tools, and what each answers

| Tool | Answers |
| --- | --- |
| `tools/inspect-game.py` | Is this file readable at all |
| `tools/ghidra/build-directx-types.py` | Ghidra has no DirectX types; this makes them, plus vtable slots and 6100 interface IDs |
| `tools/ghidra/find-graphics-entrypoints.py` | Where are the device and swap chain created, what do the COM calls resolve to |
| `tools/map-source-files.py` | Which engine source file did this code come from, via embedded `__FILE__` |
| `tools/find-string-refs.py` | What code references this string, for finding console variable handling |
| `tools/parse-capture.py` | What did the frame allocate, in what order, reading what |
| `tools/ue4-view-layout.py` | What offsets should the view uniform buffer have, and do they hold |
| `tools/analyze-view-buffers.py` | Which field is which, judged by behaviour across captures |
| `tools/ghidra/export-types.py` | Engine struct layouts from a build carrying debug information |

## Capturing from a protected game

The research proxy is `loader/proxy`, built as `dinput8.dll`. Deploy it beside the executable and
select it with `WINEDLLOVERRIDES="dinput8=n,b"`. It is inert unless `RSF_DUMP_DIR` is set.
Reverting is deleting that one file.

Carry the diagnostic on a DLL the game imports one function from, that nothing else imports.
Proxying a graphics DLL puts you in front of the renderer for nothing.

```
WINEDLLOVERRIDES="dinput8=n,b" RSF_DUMP_DIR="Z:\...\.local\observe" RSF_OBSERVE=1 prime-run %command%
```

| Variable | Effect |
| --- | --- |
| `RSF_DUMP_DIR` | Output directory. Required, created if missing |
| `RSF_OBSERVE=1` | Install the D3D11 observer |
| `RSF_OBSERVE_FORMAT` | DXGI format to retain, default 35 (`R16G16_UNORM`, Unreal velocity) |
| `RSF_VIEW_CB_MIN`/`MAX` | Constant buffer size range to retain, default 1024 to 8192 |
| `RSF_RENDERDOC_DLL` | Path to a Windows `renderdoc.dll`, enables F11 capture |
| `RSF_ENABLE_JITTER=1` | Apply the temporal jitter patch |

F10 dumps textures, view constant buffers and a size histogram; F11 takes a RenderDoc capture. The
capture index resets per process, so a shorter run overwrites a longer earlier one.

The module dump triggers on entropy, not a timer, so it self-verifies. Confirm a dump by counting
rip-relative calls through the import table: a protected file has none.

## Reading frame captures on Linux

The Linux RenderDoc build has no D3D11 support. The Windows `renderdoccmd.exe` runs under Wine and
its conversion needs no replay device:

```bash
wine renderdoccmd.exe convert -f capture.rdc -o capture.xml -c xml
python3 tools/parse-capture.py capture.xml --timeline
python3 tools/parse-capture.py capture.xml --timeline --reads 2163
```

Only reading pixels needs Windows.

**Identify passes by what they read, not what they write.** Output format and size identify
candidates; input sets identify passes. Colour plus history plus velocity plus depth plus a 1x1
exposure target is Unreal's temporal AA and nothing else. This project got that wrong twice by
reasoning from output format.

## Reading engine state

Match constant buffers by a size range, never one size: Unreal's D3D11 buffers land in
power-of-two pool buckets, so a 2640 byte structure is allocated as 4096. Count every size seen,
because when the guess misses the histogram identifies the right buffer next run. The same buffer
is reused for orthographic interface views, so check for a perspective projection before trusting
a snapshot.

Offsets from engine source are a hypothesis. Confirm each field by a property only it has: a
projection's diagonal ratio is the aspect ratio, camera vectors are unit length, a previous-frame
transform sits near identity when still, jitter is sub-pixel and straddles zero. Capture the game
in several states and let fields identify themselves.

Take numeric constants from engine source, never fit them to captured data. Unreal's velocity
scale could not have been recovered from these frames; a fitted value would have been wrong by
four times and looked reasonable.

## Patching engine features back on

Prefer re-enabling the engine's own path to reimplementing it beside it. The engine then keeps
everything derived from the change consistent, which a later patch cannot.

`rsf_patch_code` requires the bytes already present to match what is expected and refuses
otherwise. Always supply them. Patch only after the code is decrypted.

## Finding the pass that has the inputs

A backend needs colour, depth and motion together. Finding where they are together is most of the
work, and it is the part that took the most wrong turns here.

Recognise a pass by the set it binds, not by a name. Naming it invites reasoning from what that pass
does in stock engine source, which is how three separate assumptions got made about a game running a
vendor branch.

Judge the set at the draw, never at the binding. Unreal's D3D11 backend binds shader resources one
slot at a time, so the state is always partway through being written when a binding changes. Shadow
what is bound across calls and look at the shadow from a hooked `Draw` and `DrawIndexed`: that is
the moment it is complete by definition, because the runtime is about to use it.

Watch every slot D3D11 allows, not as many as the set needs. An engine binds its scene textures
structure alongside the post process inputs, and that alone reaches past slot 16, so a small window
watches a pass read depth and never sees it.

Judge sizes against the presented size with the render size left unknown, accepting anything from
half of it upwards that keeps the frame's aspect ratio. Deriving a render size from the largest
bound texture makes it an exact requirement, and one full resolution texture in the set then rejects
every other member of it.

Pick each resource by what it is. Slot order is an engine convention and does not survive contact
with a vendor branch: in this game slot 0 of the set holds the GBuffer's normals, and taking it as
scene colour would have handed a backend the normal buffer and produced a wrong image rather than no
image. Prefer failing to recognise over recognising the wrong thing.

Require only what the backend requires. An exposure target is optional to any backend that can
derive its own, and requiring it meant waiting for a set that never arrives.

## Instrumenting a search like this

The counters are the tool, and how they are split decides whether a run answers anything.

Split a counter the moment it cannot distinguish two failures. One counter read zero for two
sessions while meaning both "the hook never ran" and "it ran and rejected everything", which are
opposite problems. Count every call and the subset that survives each filter.

Count each precondition separately. Motion, depth and exposure recognised tens of thousands of times
each and never together said, in one line, that the format rules were right and the moment was
wrong. No amount of reasoning had reached that.

When a guess fails twice, stop guessing and print the data. Describing the bound set on a near miss,
slot by slot with sizes and formats, answered in one run what three rounds of inference had not.

Never trust a counter read immediately after the thing starts. A report on the key press that
installs a hook describes microseconds of runtime and reads as a result. Report on a timer as well,
and stay quiet while nothing moves.

Edge trigger on the condition, not on the identity of what satisfied it. Keying on the textures
involved fired once for a whole session, because a game binds the same targets every frame.

## Seeing it, not just counting it

A reconstruction that only writes into a texture cannot be judged. Counters say it ran and a dumped
frame says the geometry is right; neither shows ghosting, a smear behind a moving object, or an
inverted motion vector, because those are temporal and a still image has no time in it. Draw the
result over the game's own frame from inside a Present hook.

Expect that debug view to look wrong. The image is scene colour from partway through the frame:
linear rather than graded, with no interface on it. Say so rather than reading the darkness as a
fault. A copy will not do it either, since the result and the swap chain rarely share a format, so
it is a full screen draw with every piece of touched state saved and restored.

A still scene proves less than it appears to. A camera orbiting a static world exercises the
reprojection matrices and the depth buffer thoroughly and object motion not at all, because an
engine that writes object motion only leaves the velocity buffer at its clear value throughout.
Know which of the two a given scene is testing.

## Failure modes that have actually happened here

- **Using a device context off the render thread.** It races the game's rendering, returns
  whatever the staging copy held, and can kill the process. Do the work inside present.
- **Calling D3D11 while holding your own lock.** A hook runs inside the code it hooks, so the call
  re-enters and deadlocks. Record under the lock, work outside it.
- **Hooking binding instead of creation.** Binding runs a hundred times a frame for no extra
  information.
- **Using capture resource identifiers at runtime.** They do not exist there; match by signature.
- **Marker colours sharing channels with data.** An unwritten-pixel marker in the red channel read
  as strong motion for several sessions.
- **Believing a menu over the frame.** AC7 offers no temporal AA setting, and a pass with temporal
  AA's inputs runs anyway. Note what the frame does; do not name it after the engine feature it
  resembles.
- **Analysing two runs as one set.** A capture directory held one run that overwrote the low
  numbered files of an earlier one. Pooled, the field being searched for looked absent in all of
  them; differenced, it was obvious. Check timestamps before treating a directory as one dataset.
- **Reading a backend's log for the reason.** Two dead ends were one line each in a vendor log: a
  required preference flag not set, and an environment missing an entry point. Read the whole log
  before theorising about the code.

## Checking the target SDK first

Read what the backend expects before building anything to feed it. Streamline's `sl::Constants`
has `cameraMotionIncluded` and `motionVectorsInvalidValue`, and with those set it reconstructs
camera motion itself, which removed a pass this project had already written down as mandatory.
