# Anomaly motion history in Rust

The cutover moves the draw-time history, motion eligibility and b13 packing
from the first-party C++ adapter at `fa07fec` into `rsf-anomaly`. The
[isolated-game C++ baseline](rust-anomaly-cpp-baseline.md) at `8322179a36c7`
supplied the comparison DLL. Executable fingerprints and matching engine
revision are recorded in [engine.json](../../games/anomaly/engine.json).
This cutover uses the existing producer and shader evidence. It introduces no
new engine addresses or shader variants.

At the native constant-cache flush, the guarded geometry feed supplies the
instance key, projection domain, world and local-space palette in shader
order. Rust keeps separate Main and Scope frames, with World and FirstPerson
camera slots in each. It takes the first camera sample per domain and marks a
contradictory sample as conflicted. A split phase with the same source frame,
reset generation and extent retains the current state. A forward source
counter may skip between draws of a view. An older counter, a different
generation or a different extent drops that view's previous frame.
Retirement drains from the native queue on the render thread and removes the
retired generation and its predecessors in both views. Shader or device
resets clear frame data and retain retirement watermarks and conflict counts.

The private static-library interface borrows native inputs for each call.
Rust copies a snapshot on its first capture. Policy and constant upload borrow
the previous world, camera and bone data for the draw. C++ keeps engine reads,
the retirement queue, foliage capture and instance remapping, shader binding
and D3D11 resource leases. The graphics leaves upload caller-owned byte
arrays. The plugin still resolves the single Runtime already loaded by the
carrier.

The coordinator merge connected motion history to the shared Rust geometry
key. Retirement batches also have a Rust owner on the merged branch and still
apply to motion history on the render thread.

| Shader buffer | Rust packing and policy |
| --- | --- |
| Core VS b13, 3824 bytes | WVP at 0, 234 affine palette rows at 64 and draw, palette, rigid and ABI lanes at 3808. Rigid and static world, SKIN_NONE and bone-composed SKIN_0 keep their separate eligibility rules. SKIN_1 through SKIN_4 require a previous palette of at most 78 bones. |
| Tessellation DS b13, 144 bytes | Previous WV at 0, projection at 64 and history at 128. Preserves the split used by the existing VS/HS/DS chain. [Tessellation evidence](anomaly-tess-motion.md) |
| Foliage VS b13, 5056 bytes | Previous unjittered clip transform, tree transform and wind registers at 0 through 159, history at 160, detail rows at 176 and per-item state at 4080. Tree identity and LOD checks and detail group validity are preserved. [Foliage evidence](anomaly-foliage-motion.md) |
| Core PS b13, 48 bytes | Current viewport origin and reciprocal extents at 0, current and previous jitter at 16 and ABI control at 32. Jitter stays in viewport UV with positive Y down. [Core shader contract](../../games/anomaly/shaders/core/README.md) |

Core matrices use column vectors and row-major packing. The motion stage
still supplies the same view, projection and world inputs. High, low and
tessellated model classification and the stale-palette distance counters
retain their diagnostic roles. [Far-character evidence](anomaly-far-character-motion.md)

The C++ pixel packer checked that viewport extents were positive and finite
before calculating their reciprocals. A positive subnormal extent can still
produce infinity. The pixel shader rejected the infinity, but the CPU packer
reported success and set the ABI lane. The Rust packer returns failure and
writes an all-zero invalid buffer in this case. The production-linked motion
contract checks both the return value and every lane.

## Verification

The baseline reciprocal probe, built from the C++ source, accepted the
subnormal input and wrote an infinite reciprocal with ABI 1. The new
production-linked fixture refused it. The motion contract passed 11,474
checks in each build, including shader layouts, a moving skinned vertex,
split frame and camera conflicts, retirement, foliage and tessellation.

Synthetic FXC wrappers compiled the first-party Core VS/PS, Tessellation DS
and Foliage VS headers. Reflection reported b13 and the expected offsets in
all four buffers. The rules-only shader packs from Debug, Release and the
staged baseline shared SHA-256
`d2aa077a561ee71c084f806edb2f7cdcb0bf4ccaa17907a788324043f8622bb2`.

MSVC dumpbin matched the baseline export names and ordinals in both builds.

| Ordinal | Export |
| --- | --- |
| 1 | `rsf_anomaly_observation` |
| 2 | `rsf_anomaly_start` |
| 3 | `rsf_get_game_plugin_api` |

Both adapters resolve Runtime dynamically through the existing bootstrap path.
The staged baseline adapter SHA-256 is
`b978d99e2506d4d4864d267a109cd354a31e834c8e420d27dba4d8e9031fdca5`.

MSVC Debug and Release builds each passed 23 focused CPU contracts. These
covered motion, temporal history, shader tooling, Standard and AVX geometry
and SR hook guards, jitter and restoration, Runtime SR API, plugin lifecycle
and bootstrap. The Runtime API fixture initially hit a stale NVAPI include
path in the local build configuration. It built and passed after switching
to the available read-only Streamline SDK headers.

Workspace formatting, locked Clippy with warnings denied and all 216 locked
Rust tests passed. Four source-corpus shader-tool cases skipped because the
licensed vanilla shader fixture was absent. The motion, far-character,
tessellation probe and chain, and SR jitter device fixtures built in both
configurations. Device execution and moving-game acceptance remain open.
