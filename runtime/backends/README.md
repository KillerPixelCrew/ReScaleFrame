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
given `cameraMotionIncluded` and an invalid value) accepts AC7's object-only field directly. One
that wants a complete field (XeSS, FSR) needs a composition pass that does not exist yet, and
`check` says exactly that instead of producing a smeared image at runtime.

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

### How far this gets under Wine, and where it stops

Run inside the game's own Proton prefix, with DXVK and DXVK-NVAPI selected:

```bash
WINEPREFIX=~/.local/share/Steam/steamapps/compatdata/502500/pfx \
WINEDLLOVERRIDES="d3d11,dxgi,nvapi,nvapi64,nvofapi64,nvngx,_nvngx=n" \
RSF_STREAMLINE_BIN='Z:\...\vendor\streamline\bin\x64' \
prime-run /usr/share/steam/compatibilitytools.d/proton-cachyos-slr/files/bin/wine \
    build/linux-cross-x64/bin/rsf_dlss_backend.exe
```

Both halves of that matter. `prime-run` puts the work on the discrete GPU, and the overrides select
DXVK for `d3d11`/`dxgi`, which DXVK-NVAPI needs underneath it. With plain Wine's own D3D11 the
whole chain fails early at "NVAPI failed to initialize", which reads like a driver problem and is
not one.

With those in place the integration gets a long way. NVAPI reports the RTX 4070 Laptop and driver
610.57 against a required 512.15, NGX starts, and it loads `nvngx_dlss.dll` version 310.7.0. The
DLSS plugin loads for adapter mask 0x1 on Ada, architecture 0x190 against a required 0x160.

It stops in one specific place:

```
NGXCubinD3D11::CreateKernel: error: NvAPI_D3D11_CreateCubinComputeShaderEx failed - nvapi status -3
nvapi_QueryInterface (NvAPI_D3D11_CreateCubinComputeShaderExV2): Not implemented method
```

DLSS on D3D11 launches its kernels through an NVAPI extension rather than through D3D11 compute.
DXVK enables the Vulkan extensions it is built on, `VK_NVX_binary_import` and
`VK_NVX_image_view_handle`, and DXVK-NVAPI implements `CreateCubinComputeShaderEx` and
`CreateCubinComputeShaderWithName`. It does not implement the `ExV2` variant, which is the one this
NGX version calls. `slSetD3DDevice` then fails with an exception inside Streamline.

So the gap is one unimplemented entry point in DXVK-NVAPI, not this code, not the driver, and not
the hardware. Three ways past it, in order of cost: an older `nvngx_dlss.dll` that calls the
non-V2 entry point, a DXVK-NVAPI that implements `ExV2`, or Windows. Nothing here has produced an
upscaled pixel yet.

The matrices are the other open piece. They come from the view uniform buffer, which the loader
reads but whose layout is only partly mapped for this engine branch.

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
