# Runtime shader patching: first clients

2026-11-12. Simplified design, following the user's latest direction. Core owns
shader processing; adapters supply game rules, source acquisition and draw data.
No game source or reconstructed shader bodies are distributed. This is a packaging
rule, not a security subsystem. No game launch, deployment or commit is authorized.

## Scope

Implement Anomaly's X-Ray source path. DXBC rewriting is synthetic-tested only;
no bytecode game client or 3Dmigoto subset is incorporated in this milestone. Keep the
existing Runtime discovery pattern, `rsf_get_shader_patch_api`, with a size/version-
checked C function table. No new loader, registry or second Runtime. Other shader
forms have no implementation or reserved ABI fields now.

Core `runtime/shader_patch/` owns pack loading, D3D11 shader creation interception,
patch application, compilation, reflection checks, one local variant cache and
replacement objects/resources. Anomaly owns verified compile-frame/engine include
access, original source allowlist, motion rules, scene eligibility, history and
G-buffer reinsertion. Core never reads game settings or chooses engine hooks.
Move reusable loader mechanics into core; do not leave an adapter compiler/cache
beside a renamed core copy. Preserve the current original-bytecode proof and
reflection/stage-linkage checks while migrating.

## Two input paths, one rule representation

Rules are JSON data in a pack. No second C++ table format or scripting language.
Source rules identify reviewed root/include hashes, then apply exact-count anchored
edits, wrapping, source-range extraction and identifier replacements. These are the
operations the existing Anomaly Python transforms actually need. Add only RSF-
authored text; clone skin/foliage expressions from acquired local input at runtime.
Missing/ambiguous anchors or changed hashes refuse, leaving stock shaders.

D3D11 bytecode rules match original bytecode SHA-256, then a minimal instruction
pattern with an exact expected count. Patch the native DXBC instruction stream;
do not require a full HLSL decompile/recompile round trip. The synthetic operation replaces one exact same-length DXBC instruction and
repairs the checksum using first-party code. Adding resources/signatures and
general assembly are not implemented; evaluate the approved 3Dmigoto subset only
when a real bytecode client needs it. Preserve original operand modifiers/masks, execution behavior and
untouched chunks. Rebuild lengths/offsets/checksum and affected declarations/
signatures; refuse unsupported edits rather than guessing. The first bytecode
client determines the concrete operations required. No structural signatures,
graph DSL, rule priorities or generalized helper-IR linker now.

API creation sees precompiled bytecode and compile-cache hits. Anomaly publishes
engine root/entry/target/flags/complete ordered defines plus an include provider
through its verified create_shader caller frame. Core recompiles the original
using the engine's compiler and include order and requires byte-for-byte equality
with engine DXBC. Only then compile the patched tree with the same inputs. Copy
provider bytes during their lease; do not infer a loose-file installation path or
fall back to shipped trees. Cached permutations still need this acquisition/proof.

Core checks reflection: preserve native resources/buffer layouts and VS inputs,
allow only the pack's declared b13/interpolants/MRT additions, and validate linked
VS/HS/DS/PS interfaces and patch constants. Anomaly provides per-draw history data
and eligibility; generic allocation/binding mechanics live in core. Existing
history/MRT policy stays in the adapter. Refuse collisions or missing companion
stages/data and keep stock behavior.

## Minimal `.rsfpatch` v1

One file per pack, raw sections initially. No signing, trust UI, ownership scanner,
content receipts or decompression dependency in this milestone. Zstd is approved
but unused unless actual pack size warrants compression.

All integers little endian. Header is 24 bytes: magic `RSFPATCH` (8), version u32
(1), section_count u32, section_table_offset u64. Each table record is 24 bytes:
type u32, reserved u32=0, offset u64, size u64. Section types: metadata JSON (1),
rules JSON (2), payload files (3). Require one of each, no overlapping/header/table
ranges, arithmetic overflow or truncated file. Unknown versions refuse; unknown
optional section types can be skipped after bounds checks. No reserved future
backend fields. Hash complete file bytes with SHA-256 for cache identity and reload
change detection; no separate signing or integrity tree. Parse invalid JSON and
missing required fields as pack errors rather than crashing.

Metadata contains stable pack id, version, display name, intended adapter/executable
hashes, API and licence/attribution. Adapter decides target eligibility. Payload
section is a raw file table: count u32 then entries (name_length u32, data_length
u64, UTF-8 name bytes, file bytes). Check lengths and duplicate names. Names are
virtual include/resource names, never automatically extracted to host paths.
Only RSF-authored additions/replacements/resources and minimal matching patterns
ship. No original/motion game trees, generated clones, captured DXBC or caches.
No host executable/script entrypoints in the format.

Anomaly registers `<carrier directory>/ReScaleFrame/patches/motion.rsfpatch`.
Core refuses a different pack claiming the same original source hash, while allowing
the same pack id during transactional reload. No folder dispatcher, dependency DAG,
priorities or conflict-resolution engine is needed for this one-pack client.
Enable/disable and manual Reload act on existing replacements at a safe command
boundary. Reload parses/builds a new copy before replacing the current one; errors
keep the working copy and show the error. No auto-watch option initially. Keep
objects referenced by submitted commands alive until graphics completion. Device
loss/reset discards device objects and permits rebuilding for the new device.

## Runtime and overlay

Compile synchronously on the calling thread first. No worker pool/process/IPC,
async job API, cancellation framework or queue settings. Measure stalls before
adding jobs. Keep source/compile callbacks scoped and serialize shared compiler/
cache updates correctly. Exceptions stop at the C ABI and failures keep stock.

One writable cache directory supplied by the host. Variant key covers pack hash,
original DXBC/source/dependency hashes, compiler identity, target/entry/flags/
ordered defines and declared interface. Validate cached bytes/reflection before
use; publish complete cache files via temporary file and rename. No cache knobs,
LRU service or driver-native PSO cache in this milestone.

Small status record: intercepted, variants built, cache hits, replacements used,
refusals by reason and latest bounded error. Overlay lists packs with saved on/off,
status and manual Reload. No shader browser, per-rule UI or trust marks. Any actual
pack option used by a client has a generic live saved descriptor (Rule 2); do not
invent options in anticipation. Existing Motion remains Anomaly's enable alias;
retire obsolete shader-tree path reads or expose retained paths consistently.

## Authoring and package integration

One CLI: build and inspect packs. Build reads metadata.json, rules.json and authored
payload files from an explicit source directory; inspect prints contents/hash.
No originality scanner, binary receipt checks or human-review approval gate. The
packager simply excludes game shader bodies and generated caches. D3D11 creation
inventory/dump writes the bytecode/hash and reflection needed to author the first
bytecode pack under ignored .local. No replay/diff tool until a second real client
needs it. Synthetic unit tests and the Anomaly parity test remain required checks,
not an authoring framework.

Anomaly's pack contains its source hashes, transform rules and authored core/
foliage headers. Keep 73-root eligibility, tessellation chain and recursive-root
refusal. Runtime acquires licensed source/includes from the engine. Compare native
output byte for byte against tools/anomaly_motion_deploy.py's merged Python output
on the ignored 449-file corpus, hash
`fdc260e0608f259d876c4ae0f11a6f2d8c03c001cf1fe82548794af1c6826a67`.

Package build must not require the corpus. tools/package-anomaly.py includes only
pack/runtime/notices; source ZIP includes committed authored sources/rules. Offline
deploy stays a development/parity aid. Update PROGRESS/research/release record and
implementation tracker honestly; old deployed acceptance is not new-service testing.
Other packagers need only the same explicit pack-file allowlist when their client
uses the service. No blanket repackaging work now.

## Dependencies and evidence

Approved for possible use: 3Dmigoto GPLv3 DXBC subset, yyjson MIT. Only yyjson
is incorporated now; 3Dmigoto and zstd downloads remain unincorporated. Downloads reside
under vendor/shader-patch/, with original licences, exact revision/hash manifests
and THIRD_PARTY/docs/dependencies.md notices before incorporation. Zstd BSD is
approved but not linked for raw v1. No Luma/RenoDX code copied. 3Dmigoto's shader
fixes are excluded from its GPL source grant and never used as payloads.

Local references studied: Luma `80936e7822f70625d86dcb762be537b2420ad209`, RenoDX
`bfdb52d3c75a943690ab8eb9e6716bc2ed509492`, rdr2-renodx-dx12
`f20c3c3003c29fc17697a7ab2ddf1cdc9389b0b0`. Luma core.hpp registers create/init/
bind/destroy pipeline callbacks and checks D3D11-only support. RenoDX utils/pipeline.hpp
tracks original hashes and cloned pipelines; mods/shader.hpp handles cbuffer/
descriptor injection. Both carry full replacement bodies, so neither establishes
RSF's no-game-code packaging rule merely by intercepting API creation. The RDR2
reference embeds reconstructed replacements and explicitly separates build/load
checks from untested game results. Luma's Custom MIT commercial-use addendum means
it is a study reference, not an approved dependency.

[3Dmigoto COPYING](https://github.com/bo3b/3Dmigoto/blob/master/COPYING.txt),
[assembler](https://github.com/bo3b/3Dmigoto/blob/master/D3D_Shaders/Assembler.cpp),
[yyjson licence](https://github.com/ibireme/yyjson/blob/master/LICENSE),
[Zstd BSD licence](https://github.com/facebook/zstd/blob/dev/LICENSE).

Tracked-file audit accepted by coordinator: 1,243 git ls-files entries including
merge-stage duplicates; all tracked text and Python literals compared against
449 local inputs, no 24-token contiguous match. Manual review found no identified
GSC bodies beyond anchors in Anomaly shaders/embedded sources/tests/docs; generated
previous-skin bodies are untracked. No files removed. This was a one-off research
check, not a proposed packaging gate or proof of historical authorship. Details
remain in ignored .local/anomaly/tracked-shader-audit.json.

## Later forms, only when clients need them

D3D9 requires native SM2/3 register decoding/emission and constants/state-block/reset
hooks. D3D10 can reuse DXBC with its device calls. FX blobs should be handled at
contained shader creation first. No current code or ABI reservations for these.

D3D12 adds PSO/stream/library capture, root-layout binding and fence lifetimes.
DXBC reuses native edits; DXIL needs version-matched DXC LLVM transforms, rebuilt
metadata/container and Microsoft's dxil.dll validator. Approved for that phase:
DXC NCSA and Microsoft's redistributable validator, recording exact binary version,
hash and licence text in package notices. No validator bypass or current DXIL code.

Vulkan SPIR-V uses SPIRV-Tools parsing/validation and Cross reflection; GLSL can use
glslang. OpenGL adds source compile/link/uniform state mapping; ARB needs its own
small assembly path. Approved Apache-2.0 Tools/Cross and permissive glslang are for
those client phases, not linked now. Opaque GL binaries cannot be presumed portable
IR. Add versioned API fields when implementing a real client, not ahead of time.

## First gate and simplification record

Required: synthetic source/DXBC/pack parser checks, source-loader/cache/reflection
checks, opt-in full corpus parity, D3D11 creation/rewrite/binding device fixture,
clean-install pack membership tests, eng/verify.ps1 -Configuration Release -VS2026
and cargo test --workspace --locked. Report built/synthetic/device/game separately.
An authorized game run must show runtime-patched variants by stage/family, original
proof, repeat cache hits, actual replacement draws, refusals, and live pack on/off/
reload. No game run is authorized here.

Before: 610-line complete-system design with all-backend matrix, native-IR framework,
structural matching/DSL, full archive/signatures, conflict DAG, workers/IPC, broad
UI and inventory/replay tooling. After: one source-aware client and synthetic DXBC checks, JSON anchors/hash
rules, raw three-section container, later-pack refusal, synchronous compiler, one
cache, pack on/off/status and build/inspect CLI. Removed planned signing/crypto,
provenance scanner/receipts, structural matcher/IR-linker, job/process framework,
replay/diff, per-rule/browser UI and all unneeded backend files/ABI reservations.
The obsolete public C-table/regex prototype is removed rather than keeping two
rule representations. Only actual client requirements can add these pieces later.

## Implementation evidence, first milestone

Source engine and API live under runtime/shader_patch/. The sole shipped rule
representation is JSON in games/anomaly/patches/motion. Native output matches
all 454 files of the Python merged output byte for byte, including skin clones,
foliage clones, shared IO and hull/domain stages. There are 73 allowlisted roots,
20 transformed files and four authored header assets. Root tags are opaque adapter metadata exposed from the same JSON through the C
API; no second text manifest is packaged. Pack size 95,058 bytes; raw storage is sufficient.
SHA-256: `6d16974bbc2f34576e6f84978ba3ee802529a7764b7fad5c4b5e5119af673f70`.

Insert rules use a byte offset plus a short nearby anchor, after verifying the
entire input SHA-256. This intentionally differs from searching repeated native
text: shared IO has identical member blocks, so a globally unique short trailing
anchor does not exist. The reviewed hash fixes the offset; anchor mismatch still
refuses. Clone/extract boundaries require unique start matches. This changes no
shader output and is covered by full corpus parity.

The source provider uses the stateless ID3DInclude implementation from X-Ray
source revision 028ccf5f5189b393396480076b11e40852ba3ecd, r4.cpp:1170. It delegates
r3-first/shared-fallback lookup, filesystem archives and allocation/free to the
engine, copying bytes before Close. Forward slashes are converted to backslashes
in the adapter. The caller frame still supplies the engine root and exact inputs;
core recompiles the unmodified snapshot before even accepting a cache hit.

Read-only PE evidence for the two pinned executables:

| Build | Executable SHA-256 | Includer table RVA | Open / Close RVA |
| --- | --- | --- | --- |
| Standard | c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e | 0xb67398 | 0x8377a0 / 0x837320 |
| AVX | 2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12 | 0xb54508 | 0x82e9f0 / 0x82e570 |

The already-guarded D3DCompile setup starts at 0x83b9ce / 0x832cce; its +6
RIP-relative LEA selects the includer vtable. Open begins 40534881ec40020000;
Close begins 4883ec284885d274. The adapter checks these before creating a stack
object containing that table. Source review establishes that the includer has no
data members and ignores object state; the exact-build PE checks establish the
selected functions. tests/anomaly_shader_provider_evidence.py verifies both files
and hashes without execution. No game call or runtime FS acquisition is claimed
tested here. A source mismatch/provider failure leaves the shader stock.

Synthetic WARP fixture exercises source proof, hash refusal, original mismatch,
reflection, cache reuse, API interception, generic buffer allocation/upload and
an actual same-length DXBC immediate edit (red 0.25 -> 0.5) with readback. The
first-party checksum implementation was checked against the original FXC checksum
before rewriting; 3Dmigoto's algorithm was studied, not imported. General DXBC
interface rewriting is unavailable, not silently approximated.

Live on/off selects existing stock/variant pairs. Reload builds all known variants
into a candidate registry and commits only after every eligible one validates;
old generation survives failure, and successful reload resets motion history.
D3D11 driver command references retain shader/buffer resources after application
COM release; WRITE_DISCARD preserves in-flight constant data. No Present-count
retirement heuristic was added. The old offline loader constructor remains only
for development/device fixtures; production uses Runtime compilation/cache through
the queried C ABI. Game-specific reflection/linkage and history policy stay local.

Pack enable/status/reload and intercepted-shader dump are adapter-described shared
overlay rows. MotionShaderDir is no longer read. Installed pack path is
ReScaleFrame/patches/motion.rsfpatch; cache/dumps stay under patches/cache and are
never packaged. No game directory has been written in this work.

Native overlay service ABI 4 appends a live label callback, evaluated after option
values so pack status reaches the panel each Present. Rust panel ABI remains 17.
The device fixture checks callback order. This does not add any game policy to the
shared panel or Runtime.

The optional corpus test also compiled all 73 roots with System32 d3dcompiler_43,
using synthetic compile inputs (SM_5 and model SKIN_1), reproduced each original
and built/reflection-checked each patched variant. This exercises the core source
path over real local assets, not engine frame capture or game filesystem calls.

Final verification, file groups, dependency inputs and remaining game acceptance
are recorded in [the milestone gate report](runtime-shader-patching-gate.md).
