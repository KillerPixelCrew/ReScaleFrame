# Backends

Super resolution and frame generation backends, and the vendor-neutral model they implement.

The orchestrator owns the graphics resources and the presentation path. A backend owns one vendor's
reconstruction: how it wants its inputs, what it can derive for itself, and what it refuses. The
line between them is the same one `AGENTS.md` draws between orchestrator and game plugin, applied a
level down.

## Why Rust here

The rest of the runtime is C and C++ because it lives inside the game process and speaks COM. These
crates do not. They hold rules, unit conversions and refusal conditions, none of which need a GPU or
Windows to test, and all of which are easier to get right with sum types and exhaustive matches.

When a backend does need to reach a vendor SDK, the boundary is a C ABI in the shape
`sdk/game/include/rescaleframe/game_api.h` already uses: `#[repr(C)]` structs leading with
`struct_size`, opaque handles, `catch_unwind` at every entry point so a panic never unwinds into
foreign code, and a thread-local last error rather than a returned string. No STL, no Rust types,
no ambiguous ownership across the line.

| Component | Contents |
| --- | --- |
| `rsf-upscaler` (Rust) | Vendor-neutral model: quality levels, motion vector conventions, viability checks |
| `dlss` (C++) | Streamline: load, device, support query, resource tags, per frame evaluate |

The DLSS side is C++ rather than Rust, and deliberately. Streamline's structures are versioned,
GUID tagged, and in one case abstract with a virtual operator; `FrameToken` cannot be expressed as
a `#[repr(C)]` struct at all. Hand transcribing those layouts is silent corruption waiting for a
version bump, so the vendor side stays where the vendor's own headers define it and
`rescaleframe/dlss.h` is the narrow C surface the rest of the project sees. The rules above the
line stay in Rust, which is where deciding what to do belongs.

## `rsf-upscaler`

Three questions, answered without any vendor SDK present.

**What are this game's motion vectors, really?** `motion.rs` carries the answer as data rather than
as an assumption. Ace Combat 7 writes object motion only, biased into a sixteen bit unsigned target
by `In * (0.499 * 0.5) + 32767/65535`, with zero reserved to mean "nothing wrote this pixel". The
scale constant comes from engine source, not from fitting captured data: in a frame where little
moves, the samples cannot determine it, and a fitted value came out wrong by four times while
looking entirely plausible. There is a test pinning that.

**Can this backend be driven from those inputs?** `check` says so, and returns every reason it
cannot rather than the first. A backend that reconstructs camera motion from depth (Streamline,
given `cameraMotionIncluded` and an invalid value) needs no composition pass for AC7's object-only
field. One that wants a complete field (XeSS, FSR) does, and `check` says exactly that instead of
producing a smeared image at runtime.

No backend, Streamline included, can read the raw target though, and that took a second look to
see. They all offer a scale factor for motion and nothing to subtract a bias with, and Unreal's
storage is biased, so the raw target reads as a large constant motion across a still image. A
decode pass is required whichever backend is used. Once it exists, adding camera motion to it is
the composition pass XeSS and FSR want, so the two stop being separate work.

**What are this frame's numbers, in the units a backend wants?** `frame.rs` does the conversions.
Unreal keeps jitter in clip space and motion in a screen space that spans two units across the
viewport, and every backend wants pixels. Each conversion carries a factor that comes out wrong by
two or four if it is guessed at, and the storage encoding already contains one such factor, so
getting one right and the other wrong produces motion that points the right way and is twice as
long as it should be. That reads as a slightly eager reconstruction rather than as a bug, which is
why the conversions live here with their reasoning and their tests rather than being written out
again at each call site.

The same module carries the jitter sequence length a backend wants, which is eight samples scaled
by the area ratio. Unreal 4.18 takes that count from `r.TemporalAASamples` and does not move it
with screen percentage, so the loader sets it alongside the render scale.

## `dlss`

Streamline, reached through `sl.interposer.dll` loaded by absolute path at runtime. Manual hooking
is what makes this integration possible: the regular mode expects to be in place before the swap
chain exists, while manual hooking allows the D3D11 device to already exist, which is the only
option when attaching to a game that is already rendering. D3D11 has no device proxy in Streamline
at all, so the game keeps rendering on its own device.

What is settled, checked by cross-building against the real headers with warnings as errors:
initialisation, handing over the game's device, the adapter support query, the render size DLSS
asks for at a quality level, the five resource tags, and the per frame constants. The constants
carry the pair that makes AC7's motion buffer usable directly, `cameraMotionIncluded` false with
`motionVectorsInvalidValue` at the clear value, which is why AC7 needs no composition pass here.

### Running it under Wine

DLSS reports supported and returns render sizes on this machine, under Wine, in the game's own
Proton prefix:

```bash
WINEPREFIX=~/.local/share/Steam/steamapps/compatdata/502500/pfx \
WINEDLLOVERRIDES="d3d11,d3d12,d3d12core,dxgi,nvapi,nvapi64,nvofapi64,nvngx,_nvngx=n" \
RSF_STREAMLINE_BIN='Z:\...\vendor\streamline\bin\x64' \
prime-run /usr/share/steam/compatibilitytools.d/proton-cachyos-slr/files/bin/wine \
    build/linux-cross-x64/bin/rsf_dlss_backend.exe
```

Every part of that line earns its place, and each one was a dead end first:

- **`prime-run`** puts the work on the discrete GPU.
- **`d3d11,dxgi=n`** selects DXVK. DXVK-NVAPI sits on top of it, so with Wine's own D3D11 the chain
  dies at "NVAPI failed to initialize", which reads like a driver problem and is not one.
- **`d3d12,d3d12core=n`** selects vkd3d-proton, and this is the one that is easy to miss in a D3D11
  integration. Streamline runs its own compute through a DX11-on-12 device, so without it
  `D3D12CreateDevice` fails, `computeDX11On12->init` fails after it, and DLSS ends up unsupported
  for a reason that has nothing to do with D3D12 being wanted by anything the game does.
- **`nvngx,_nvngx=n`** reaches the driver's NGX rather than a stub.

With those, on an RTX 4070 Laptop with driver 610.57 against a required 512.15:

```
DLSS supported: 1 (result 0)
performance quality renders 1024x576 for 2048x1152
```

That render size is the same 1024x576 the game was measured at under `r.ScreenPercentage 50`, which
is a useful coincidence rather than a result: it means the size DLSS asks for at performance quality
is one AC7 can already be made to render at.

Two things in the log are worth knowing about and neither is fatal. NGX reports
`NvAPI_D3D11_CreateCubinComputeShaderEx failed - nvapi status -3`, and `CreateCubinComputeShaderExV2`
comes back as not implemented: DXVK-NVAPI implements the plain and `WithName` cubin entry points but
neither `Ex` variant, and NGX falls back. Every DLSS snippet on this machine takes that path, from
2.3.11 to 310.7.0, so it is the NGX core rather than the snippet version. Separately, DXVK logs
`waitForIdle: Operation failed` twice while the test tears its device down after `slShutdown`; in a
game the device outlives us, so this may never come up, but it has not been explained.

### It reconstructs a mission frame

7 September 2026, in flight, with the render scale held at 1024x576 against a 2048x1152 output:
7917 frames evaluated, none refused, and the result is the whole scene. Sky, cloud layer, sun,
terrain and aircraft, reconstructed at twice the rendered resolution, with no smearing under
movement. That last part is the first real test of the decoded velocity and of `ClipToPrevClip`,
neither of which a static scene can exercise.

Three things got it there, and each was a wrong assumption first: the colour is taken at Present
rather than at the pass that identifies it, because twenty seven draws add the sky to that same
target afterwards; the render scale re-applies itself, because loading a mission puts the game's own
setting back and a backend fed the presented size is doing antialiasing while looking like a
success; and the output target is cleared once, because a reconstruction does not promise to write
every pixel of it and unwritten memory reads as an artifact of the reconstruction.

What remains is that the result is drawn over the game's frame rather than reinserted into its
pipeline, so it is ungraded and has no interface on it.

### The first run, before any of that

Run in the game on 6 September 2026: 2176 sets recognised at 1024x576, 2175 evaluated, none refused.
Dumping the scene colour it was given and the result it produced, from the same frame, and comparing
them at 1:1, the reconstruction is real. Aircraft stencil text that is pixelated in the source is
legible in the result, and panel lines that stair-step in the source resolve into straight edges.

What that run does not establish, and it matters:

- The camera orbited the aircraft, so the reprojection path was exercised and did hold:
  `clipToPrevClip`, the depth buffer and Streamline's reconstruction of camera motion from them all
  produced a stable image, and a wrong matrix among those smears in a way this run would have shown.
  What did not move is the scene. Ace Combat 7 writes object motion only, and with nothing moving in
  the world the velocity buffer stayed at its clear value across most of the frame, exactly as the
  capture research measured for a panning camera. So the decoded velocity's sign and axis direction
  are still unverified: they need something moving through the world, which is a mission rather than
  a hangar.
- The colour handed over is the output of the pass that binds depth and velocity together, which is
  the lighting or composite pass, not the pre-tonemap image the full resolution captures described.
  Whether that is the right input is a question about the insertion point.
- Nothing is reinserted. The game still presents its own image, and this runs beside it.

Everything below was written before that run and describes getting there. The matrices are the piece
that stands between this and one. They come from the view uniform buffer, which the loader reads but
whose layout is only partly mapped for this engine branch.

### An identity is not optional

Streamline will not start NGX without one, and DLSS is an NGX feature. With none supplied the
plugin loads and then refuses with "Missing NGX context - DLSSContext cannot run", which reads
exactly like unsupported hardware. An injected integration has no NVIDIA-issued application id of
its own, since that belongs to the game's publisher, so it identifies by engine instead. For Ace
Combat 7 that is Unreal 4.18, which is simply true.

The honesty rule from `AGENTS.md` applies to this crate the same way it applies to
`rsf_game_info.rendering_ready`: a backend reports what it can do, and a pairing that is not viable
is refused with a reason rather than attempted.

Nothing here talks to a GPU yet. `cargo test -p rsf-upscaler` runs anywhere.
