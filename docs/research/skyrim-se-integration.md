# Skyrim Special Edition integration research

Initial investigation: 15 November 2026. Scope: installed executable fingerprint,
current public source review, integration design and validation plan. The initial
read-only findings below are historical. Later sections record exact-build hooks,
bounded game runs and experimental native-resolution SR evaluation.

## Question and approach

Determine how RSF can integrate SR, FG/MFG and latency with Skyrim's Creation
Engine renderer while preserving native post-processing and full-resolution UI.
Community Shaders (CS) is the primary reference because it exposes engine-owned
render targets, jitter, dynamic resolution, post-processing and UI entry points.
CommonLib supplies reverse-engineered layouts and relocation machinery. These
are source leads, not proof of behavior in our installed executable.

This is a Skyrim SE investigation, not a universal Bethesda adapter. Fallout 4,
older Skyrim, VR and Starfield need their own evidence and layouts.

## Installed target

Read-only inspection with `tools/inspect-game.py` and Windows version metadata:

| Field | Observation |
| --- | --- |
| Executable | `SkyrimSE.exe` |
| File version | `1.7.104.0` |
| Steam app / manifest build | `489830` / `24914197` |
| Bytes | `37910440` |
| SHA-256 | `846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f` |
| Architecture / preferred base | x64 / `0x140000000` |
| Entry RVA | `0x38ef310` |
| Graphics imports | `d3d11!D3D11CreateDeviceAndSwapChain`, `dxgi!CreateDXGIFactory` |

The root inventory has no SKSE loader, and Data has no SKSE directory. This is
an on-disk inventory, not a claim about mod-manager virtual deployments. Active
graphics API and loaded modules remain unobserved. The ignored fingerprint is
`.local/skyrim-research/fingerprint.json`.

The [official SKSE site](https://skse.silverlock.org/) lists SKSE 2.3.1 for
1.7.104. The store name Special Edition does not imply the old 1.5.97 ABI.

## Updated references

Fetched on 15 November 2026; reference trees remain ignored and no code is copied.

| Reference | Revision used / purpose |
| --- | --- |
| [Community Shaders](https://github.com/community-shaders/skyrim-community-shaders) | `33d7896147b54ca72d1ee5ca938bb85739593167`, dev, 14 November; primary renderer source |
| CS-pinned CommonLib | `70c1acd5261210982bd52f6d4468a082fe04d798`; layouts matching CS's dependency selection |
| [Current CommonLib](https://github.com/alandtse/CommonLibSSE-NG) | `909517ba2416bc530abf1d2794b319664850e234`, 14 November; separate latest checkout for comparison |
| [OptiScaler](https://github.com/optiscaler/OptiScaler) | `ccf3b8c6a75824d0c9c7cc7156bf666afddde3cc`, 14 November; updated, secondary interop reference, not audited here |
| [Special K](https://github.com/SpecialKO/SpecialK) | `0e584561e1d22a4b9d4828680fd638896ca5d4a4`, 3 November; updated, secondary coexistence reference, not audited here |

Correction: the existing CS checkout at `dd2677fc` pointed to the old doodlum
remote and reported up to date. Canonical upstream had moved to `33d78961` with
divergent history. Its old local dev branch was preserved; the clean working tree
now selects canonical origin/dev detached. A separate current clone confirms the
same revision. Earlier RSF bridge research remains historical evidence, not a
current Skyrim hook map. CS's pinned dependency and latest CommonLib are recorded
separately rather than silently changing the submodule's dependency contract.

## Source findings

All paths below refer to the CS revision above. Runtime use describes upstream
code, not observed execution on this machine.

| Question | Source evidence and implication |
| --- | --- |
| Where to set render size and jitter? | `src/Features/Upscaling.cpp:621-646,1733`: Main_UpdateJitter wraps native update; ConfigureTAA precedes it and ConfigureUpscaling follows it. Versioned call offsets exist for newer builds. Use this engine boundary rather than Present-time viewport guesses. |
| Where are SR inputs? | `Upscaling.cpp:1440`: Upscale reads renderer-owned kMAIN colour, kMOTION_VECTOR, kTEMPORAL_AA_MASK and main depth; reactive/composition masks also use CS deferred normals. Vanilla must be examined independently because those CS resources are not automatically available. |
| Where does SR run? | `Upscaling.cpp:1756`: Main_PostProcessing prepares FG resources, performs SR, then calls the native image-space path with optional HDR framebuffer redirection. Establish the exact colour space and downstream native tone-map chain before adopting this placement. |
| What motion convention is proposed? | `package/Shaders/Common/MotionBlur.hlsli:8`: unjittered current and previous transforms produce `(-0.5,+0.5)*(currentNDC-previousNDC)`, equivalent to previous-minus-current UV. This describes CS shader output, not proof of complete vanilla object coverage. |
| What do SDK callers expect? | `Upscaling/FidelityFX.cpp:348-383`: render-size motion scale, negative pixel jitter, milliseconds from delta seconds, vertical FOV, near/far, preExposure 1 and reset from ShouldResetHistory. World-to-meters factor is 0.01428222656. `Upscaling/Streamline.cpp:330-370` supplies non-inverted depth, camera-inclusive, unjittered motion. Verify these against raw captures; do not transfer SDK constants blindly. |
| Where can UI be separated? | `Upscaling.cpp:1740` snapshots the clean scene before interface rendering; HDRDisplay::SetUIBuffer redirects the framebuffer. HDRDisplay owns blend/composition changes. Trace all menus, loading screens, cursor and overlays before treating that boundary as complete. |
| Where can input be reported? | `src/Hooks.cpp:677,1168`: an input-event dispatch call in PollInputDevices is hooked. This may be after physical device sampling; it establishes an input-received lead, not a verified pre-sampling latency boundary. |
| Can upstream pacing be reused directly? | No. The input hook calls UpdateReflex and upstream owns vendor policy. RSF must report engine facts here and retain sleep, markers, tokens, technology selection and deduplication in the orchestrator. |

Candidate relocation IDs, not executable RVAs: jitter `(75460,77245)`,
post-processing `(100430,107148)`, UI `(79947,82084)`, input `(67315,68617)`.
Resolve against the installed build's Address Library, then verify instructions,
calling convention, expected bytes and surrounding control flow in the binary.
No patch site is approved by this source review.

CS uses version-aware CommonLib layouts, with explicit 1.7.99-era changes. Its
README requires Address Library and SSE Engine Fixes. Those are CS requirements;
an independent RSF adapter has not established a dependency on either. CS carries
GPLv3 in COPYING.txt; dependencies and individual files need their own review
before any future linking or redistribution under docs/dependencies.md.

## Proposed RSF boundary

Create `games/skyrim-se` as the exact-build adapter. Keep fingerprints, relocation
resolution, native types, render sizing, shader permutations, camera/history reset
facts, input conventions and native reinsertion there. Extract reusable Creation
Engine mechanisms under integrations only after a second target proves them.

Use the existing versioned game renderer/frame/event ABI. Supply frame/view
identity, active versus allocated rectangles, colour/depth/motion leases, camera
constants and reset causes at the native pass that consumes them. AddRef alone
does not protect contents when the engine reuses targets. Capture or copy before
later writes, order GPU work, and retire through the owner. Audit existing ABI
fields before proposing append-only extensions.

The runtime owns vendor SR/FG contexts, interop, presentation and latency. Reuse
its existing D3D11/D3D12 services after checking their actual contracts; do not
embed CS's independent vendor dispatch or proxy swapchain in the adapter.
SR colour and final HUD-less FG colour require distinct leases and timing.

Two paths need separate validation: vanilla renderer and coexistence with CS.
For CS coexistence, exactly one component may own jitter/SR, UI composition,
vendor pacing and FG presentation. Hook stacking alone cannot establish this.
User requirements: RSF must load without SKSE and must support coexistence with
Community Shaders and ENB. Select a direct carrier after inspecting startup/imports
and proxy chaining. CS itself may require SKSE; that must not become an RSF
dependency. Validate ENB and CS configurations separately, plus their combination
where supported upstream; RSF cannot assume those two renderers coexist themselves.

While RSF owns processing, deactivate Community Shaders upscaling and frame
generation. This is mandatory integration policy, not a player setup instruction.
Preserve other CS effects and ENB processing. Coordinate jitter, native TAA,
render sizing, presentation and pacing too, since disabling dispatch alone may
leave a second owner. Prefer a supported public coordination API; otherwise
research a version/fingerprint-guarded adapter mechanism. Public control support
has not been established by this pass. The C++ methods in Upscaling.h are not
evidence of exported ABI services. Do not guess private object offsets or rely
on a startup INI edit which live CS settings can overwrite. Startup must prevent
conflicting swapchain creation before graphics activation; live handover needs
ordered context/resource retirement. Refuse activation with a clear status if
exclusive ownership cannot be established. Preserve user settings and define
restoration on RSF deactivation. Show ownership and any pending handover in the
shared overlay through generic adapter descriptors.

ENB requires its own live hook/order and colour-space investigation; no ENB
compatibility or API support is established here. A CS Effects11 preset path
must not be mistaken for coexistence with the actual ENB binary.
Every eventual option, including observer diagnostics, must have a live generic
overlay descriptor and saved value.

## Next experiments and rejection conditions

1. Resolve build-matched Address Library entries and decompile the candidate
   callers. Reject every mismatch; record RVAs, expected bytes, function names
   and source correspondence before a hook is implemented.
2. Build a pass-through observer for frame/view identity, allocation descriptors,
   post-process order, input dispatch and final Present ownership. Reject a
   main-view candidate if reflection/map/preview views share its classification.
3. Capture bounded raw colour, depth, velocity and current/previous camera data
   in a static scene, camera pan and moving/skinned actor scene. Establish units,
   signs, jitter removal, depth range, invalid vectors, coverage and resource reuse.
   Reject camera-only motion as sufficient object velocity.
4. Trace allocation and active viewport propagation before reduced-resolution
   SR. Check shadows, reflections, water, foliage, particles, first-person hands,
   snow, precipitation and native tone mapping. UI stays at display resolution.
5. Validate cuts, teleports, load doors, pause, menus, resize, alt-tab, backend
   switches and teardown. Repeat vanilla and CS paths separately.
6. Only after SR evidence, validate final HUD-less/UI leases, FG/MFG and latency
   event order. Present counts alone establish neither generated-frame quality
   nor reduced latency. Physics/high-FPS compatibility needs a separate check.

Current result: installed target identified and source integration leads recorded.
Binary hook matching, active-path capture, implementation, native build tests,
device tests and game acceptance are all pending.

## Resumed native preflight, 15 November 2026

The first Ghidra import exceeded its 330-second startup deadline. The restart
left no Java or Skyrim process running and no retained local analysis files.
The resumed import uses the already installed Ghidra 12.1.4 and JDK 25 with a
30-minute startup allowance. Import completion is not established yet.

Nexus presented a Cloudflare challenge to the isolated agent-browser session;
ModPub's matching package required login. The Nexus package was not obtained.
An alternative public reference was downloaded from
[marthofdoom/CommonLibSSE-NG](https://github.com/marthofdoom/CommonLibSSE-NG),
revision `add17c090cfa312d370bfcdd55d2eb989d086fec` (12 November).
Its independently generated MITIDTAB revision 5 is not the Nexus Address Library.
Its SHA-256 is `a75879011dce7f810992a86acf811daed2ed75813ecee9a3903aec1242c272ce`.
The table's timestamp/image-size identity matches our exact executable.

`docs/research/scripts/inspect-skyrim-hooks.py` checks the executable SHA-256, table identity,
record bounds/order and FNV checksum before resolving source leads. It reads
only files and creates a new report exclusively. The actual installed executable
and downloaded table produced `.local/skyrim-research/hook-preflight.json`:

| Lead | Native observation |
| --- | --- |
| Renderer::Begin, AE ID 77245 | RVA `0x1009D30`; call at `+0x133`, RVA `0x1009E63`, bytes `E8 F8 44 01 00`, target `0x101E360` |
| PollInputDevices, AE ID 68617 | RVA `0xCF95F0`; call at `+0x7B`, RVA `0xCF966B`, bytes `E8 E0 0F 00 00`, target `0xCFA650` |
| RenderTargetManager, AE ID 411451 | reference RVA `0x3335600`; owner/layout not yet runtime verified |
| Post-processing 107148, UI 82084, allocation 77303 | absent from this independent table; unresolved, not inferred from neighboring IDs |

Capstone disassembly of `0x101E360` provides stronger jitter evidence than
the table alone: it increments the index at state `+0xD0` modulo eight, reads
sample arrays at `+0xD4` and `+0xF4`, divides by dimensions at `+0x24/+0x28`,
and writes projection offsets at `+0x44/+0x48`. When the temporal setting is
disabled it writes zero to both offsets. These dimension/offset locations match
the current CommonLib State definition. The leaf has no pdata entry; the bounded
disassembly includes subsequent padding/code and is not a claimed function extent.
Renderer::Begin has multiple unwind regions, so the first pdata range alone
also does not describe its complete logical body.

At the input call, RCX contains the dispatcher and RDX addresses the queued event
list. Target `0xCFA650` accesses the event-source lock at `+0x48`, notifying flag
at `+0x50` and sink arrays. This supports the source's event-dispatch interpretation,
not a pre-device-sampling boundary. The full poll entry remains a separate latency
input-sampling lead. No vendor sleep or marker policy belongs in these hooks.

Disassembly is retained in `.local/skyrim-research/native-root-disassembly.json`.
These are static byte and instruction checks, not an injected observer or game
acceptance. UI, colour pass ownership and moving-object vector production still
need matching and capture before publishing GPU leases.

The shader-patch service was reviewed at its actual current ABI. Its source route
requires source/original-bytecode proof; its DXBC route supports same-length
instruction replacement, not arbitrary new shader outputs/resources. Missing
motion outputs may therefore require proven source replacement or an explicit
service extension. Do not advertise a bytecode token edit as sufficient to add
velocity MRTs or previous-bone inputs. Skyrim shader roots and history ownership
must be supplied by the adapter; the generic service must remain game-independent.

### Original shader corpus

Read-only archive extraction used Cutleast/sse-bsa revision
`1f45f5548e95bf1316a87ecc58b9a4be32e8cdf3` (1.1.0), isolated with uv.
The installed `Skyrim - Shaders.bsa` SHA-256 is
`fd1ba630f443353ed21ed4ca0380968f9c7091c8434518ea3cc79bc6d38c18e7`.
Its only file is `shadersfx/shaders011.fxp`, 67,768,128 bytes, SHA-256
`72cfc73eb0938d5949f0b3a8683722a33b671a34b41b750ba138de8d154c5fbc`.
Neither game file was changed; extraction stays under ignored local research.

`docs/research/scripts/inspect-skyrim-shaders.py` inventories bounded DXBC containers and OSGN
output signatures. The actual corpus contains 16,044 containers: 3,390 vertex,
12,635 pixel and 19 compute shaders, with no rejected marker candidates.
It records hashes, extents and signatures, but does not validate DXBC checksums
or map native shader-family/permutation identity yet. Multiple MRTs do not prove
motion output semantics, completeness or current use. The original-bytecode
inventory is `.local/skyrim-research/shader-dxbc-inventory.json`.

### Community Shaders release ownership evidence

The latest published GitHub release at this check is v1.9.1; current dev and
published release must be handled separately. Downloaded the official
`CommunityShaders-2026-09-24T12-32Z.zip` into local research, SHA-256
`21daeba464bcbc6ee90f5bbf6d8bd17a0ad2b024cc406ba8a6ee216fc7679fcb`.
The DLL SHA-256 is `10ba95e6b1cd6b675ce74d1d681fae41e667bdd8b7b5ba8c362e657511097827`.
Its source tag resolves to `265c28f5` and is checked out separately from dev.

The archive supplies CommunityShaders.pdb. Microsoft's installed DIA library
read it without system COM registration or loading the CS DLL. PDB GUID
`240F7D4A-CB53-466A-B55C-6BCF3E723338`, age 1, matches the DLL's RSDS record.
The shipped export table contains SKSE entry points and FidelityFX helpers,
but no SR/FG ownership handover API. This is an inspected export table, not
proof that every possible inter-mod coordination route has been excluded.

PDB layout and instruction cross-checks for this exact DLL:

| Item | Evidence |
| --- | --- |
| globals::features::upscaling | object RVA `0x13DCF40`, size `0x150` |
| Upscaling::settings | member `+0x58`, size `0x38` |
| primary/fallback SR selections | settings `+0/+4`; GetUpscaleMethod at `0x4F8EB0` selects object `+0x58/+0x5C` in instructions |
| FG selection | settings `+0x10`; IsFrameGenerationActive at `0x4FE340` checks global object `+0x68` in instructions |
| native jitter wrapper | `Main_UpdateJitter::thunk` RVA `0x4F3F20`; reads the same SR selection globals before enabling native temporal behavior |
| graphics creation wrapper | `hk_D3D11CreateDeviceAndSwapChainUpscaling` RVA `0x4FC700` |
| settings load / hook install | LoadSettings `0x4F9500`, PostPostLoad `0x4F8ED0` |

DIA type metadata is retained in `.local/skyrim-research/cs-pdb-evidence.json`;
bounded disassembly is in `cs-method-disassembly.json`. None of these addresses
has been used to write memory or install hooks. A takeover must suppress both
SR selections, FG creation/dispatch, CS pacing and jitter interference before
RSF activation. It must preserve desired settings separately from effective
ownership and restore them safely. A private-layout profile needs the exact DLL
fingerprint and instruction guards; it cannot be applied to current dev or a
different release by version text alone. ENB and live CS acceptance remain open.

After restart the connected REA server now reports the installed Ghidra/JDK as
available. The Skyrim target was opened there for persistent follow-up, without
starting a second import while the CLI REA import is still running.

## First native observer and game run, 15 November 2026

`games/skyrim-se/src/plugin.cpp` now implements the game ABI with exact executable
detection and a pass-through detour of jitter producer RVA `0x101E360`. It guards
the first 21 bytes, creates the hook during prepare and enables it at start.
It preserves the original call once and copies dimensions/projection offsets
after the producer returns. It reports no invented frame identity or latency
events. Callback leases drain before host services retire; the adapter and its
trampoline are pinned for process lifetime. It logs at most sixteen samples.
CS presence currently refuses activation rather than claiming coexistence.

`loader/proxy/src/plugin_carrier.cpp` is a generic XInput forwarding carrier.
It forwards the native names/ordinals and invokes the runtime plugin-session
API with the fixed packaged Entry.dll. It does not contain a game name, native
offset, vendor choice or graphics pipeline. No second runtime is loaded by
the game adapter. Existing carrier implementations are not used as game hosts.

Release MSVC 14.52.37025 builds produced the game DLL and carrier. The Skyrim
ABI contract, new carrier forwarding contract and existing Wingman forwarding
contract passed. The full `eng/verify.ps1 -VS2026 -Configuration Release` run
reached 222 tests with two failures: `anomaly_fg_dlss_sr5_device` did not observe
the FSR3 switch and `anomaly_fg_fsr3_sroff_device` observed no active FG. Both
reproduced in isolation. No FSR/runtime implementation was changed in this pass;
the cause remains unresolved, so the complete native gate is not green.
The new forwarding check was then added and passed in the focused five-test run
(three carrier/Skyrim checks passed, those same two device checks failed).
Formatting and Clippy passed separately after the native gate stopped at CTest.
The initial configure had a stale MSVC 14.51 cache; fresh configuration selected
the actually installed 14.52 compiler. No toolchain was installed or upgraded.

A bounded vanilla game run used the explicit development deployment recorded at
`.local/skyrim-research/deploy-backups/20261115-115519/receipt.json`. Three new
files were deployed: the XInput carrier, runtime DLL and game DLL as Entry.dll.
No existing file at those paths was replaced. The runtime prepared and started
the adapter successfully. The responsive Skyrim window produced sixteen readable
samples at 1711x1048, with an eight-frame repeating jitter sequence. The state
address minus the observed executable load base equals `0x33363A0`, independently
agreeing with the researched state global. Loaded-module inventory includes the
carrier, one RSF runtime, Entry.dll and D3D11, with no SKSE or CS modules.
The game was closed gracefully after the bounded run.

Runtime evidence is retained as `observer-game-20261115.log` and
`observer-live-modules.json` under ignored local research. This is loading and
native-state observation only, not gameplay image acceptance or SR/FG/latency
validation. The development observer remains deployed for further investigation.

The CLI Ghidra process has now exited and its temporary project is gone. The
saved REA snapshot `analysis.json` retains `binary_overview` Evidence
`ev_e84290cbbc819af2ac3ab87646d9fe4655bbabcd18329d90741c515adeaf0db2`,
176,589 analyzed procedures and 38,333 defined strings. It does not retain an
open disassembler database or matched function dossiers. IDA successfully opened
the CS DLL/PDB and decompiled its SR selector and FG predicate; its database was
saved and closed before starting Skyrim executable analysis in IDA. Native CS
Feature::Load was resolved at DLL RVA `0x3A4B30`; full function paging is pending.
Source review identifies a possible pre-feature-load ownership boundary, but no
CS memory writes or takeover hooks have been implemented.

### Native shader producer matching

IDA completed its Skyrim auto-analysis and exposed the actual shader readers.
The search started from the FXP marker `0x11223344`, then followed the stream
reads, allocation sizes and D3D11 device calls. It did not identify shaders by
creation order alone. Matched functions were named and commented in IDA; an
explicit database copy is saved at
`.local/skyrim-research/SkyrimSE-1.7.104-research.i64`.

| RVA | Native role and lifetime evidence |
| --- | --- |
| `0x100FC10` | VS reader: marker, byte count, descriptor, vertex description (8 bytes), constant table (20), group selectors (4), DXBC; allocation `0x68 + byte count`; device CreateVertexShader slot 12. Bytecode remains embedded at native shader `+0x68`. |
| `0x100FFE0` | PS reader: marker, byte count, descriptor, constant table (64), group selectors (4), DXBC; native shader size `0x80`; CreatePixelShader slot 15. Temporary bytecode is freed before return, so capture/proof must happen at stream/device creation. |
| `0x10102D0` | CS reader: marker, byte count, descriptor, constant table (32), group selectors (4), DXBC; allocation `0x90 + byte count`; CreateComputeShader slot 18. Bytecode remains at native shader `+0x90`. |
| `0x1578F50` | Packed BSShader load: VS/PS counts, native readers, insertion into shader descriptor maps. |
| `0x14EA2D0` | Package initialization: constructs named shader owners, opens Shaders%03d.fxp, then loads the first nine families in a fixed sequence. |

Constructor strings, shader-type stores and the sequential loader calls establish
the first nine families in the installed FXP. The inventory tool now validates
those native record boundaries against independent DXBC sizes/stages and rejects
duplicate per-family/stage descriptors:

| Family | VS | PS |
| --- | ---: | ---: |
| BloodSplatter | 2 | 2 |
| DistantTree | 4 | 4 |
| RunGrass | 18 | 18 |
| Particle | 6 | 6 |
| Sky | 9 | 9 |
| Effect | 606 | 3216 |
| Lighting | 127 | 6924 |
| Utility | 339 | 177 |
| Water | 2172 | 2172 |

This classifies 15,811 of 16,044 containers with family, stage, native descriptor
and original SHA-256. The remaining 233 image-space/compute containers stay
unclassified. This is a producer/identity map, not evidence that every permutation
runs or writes complete motion. In particular, output count/mask alone does not
assign motion semantics. The complete report is
`.local/skyrim-research/shader-native-family-inventory.json`.

RTTI-linked native vtables also identify ImageSpaceEffectHDR's Render at
`0x153D120` and the common image-space shader Render at `0x1564E30`. The latter
binds native render-target indices through the inline RenderTargetManager at
`0x3335600`; it is not a pointer-to-pointer singleton. This corroborates the
current CS correction to CommonLib's erroneous dereferencing getter. Their
producer/consumer chains still need full tracing and runtime capture before
selecting SR or final HUD-less FG leases. Decompilation pages and constructor
evidence are retained in local `ida-*.json` files.

### Native main post-processing boundary, 15 November 2026

Question: where can RSF access scene colour before Skyrim's own HDR chain,
without removing native bloom, exposure or tonemapping? The independent ID table
does not contain CS's post-processing ID 107148. Instead, traced the actual
ImageSpaceManager global, its effect dispatcher and callers in IDA. This resolves
the earlier missing-ID entry above by native instructions, not a neighboring ID.

For the exact executable fingerprint recorded above:

| RVA | Evidence and role |
| --- | --- |
| `0x33D40A0` | Pointer to ImageSpaceManager. Its effect-pointer array is at manager `+0x28`. |
| `0x153AC00` | Main post-processing caller; references the manager and inline render-target manager. |
| `0x153ADE7` | `E8 04 2E FB FF` calls `0x14EDBF0`. RCX is the manager, EDX is input target 1, R8D is the caller's output target, R9B is set to 1. The caller-relative displacement is `+0x1E7`, matching current CS dev's ID 107148 hook. |
| `0x14EDBF0` | Main effect processing. Consumes manager, input target and output target; scans active effects 0..7, dispatches HDR/copy effect 30 or 34, then native temporal AA at manager `+0x1F0`, deferred effects and dynamic-resolution output. |
| `0x14EE060` | Generic effect dispatcher. Constructs native input/output wrappers, binds them to the effect, calls its Render virtual and releases the wrappers. |
| `0x10155FE` | Native CopyResource call in target binding. Texture and copy pointers are indexed from renderer `+0xA58/+0xA60 + index*0x30`; this supports direct native texture lookup independently of CommonLib's comments. |

The manager processor's analyzed instructions consume three arguments. The
observer forwards a fourth register argument unchanged because the caller sets
R9B; CS's five-argument declaration alone does not establish this build's native
ABI. The detour guards the processor's first 20 bytes and the caller's five-byte
call in addition to the full executable hash. It preserves the original chain
once. Named functions and comments were saved in the explicit IDA database copy.

The new bounded observer records at most sixteen manager calls and reads texture
descriptors only for researched main-colour slot 1 and motion-candidate slot 7.
The pointers are borrowed synchronously at the consumer and never retained.
GetDesc does not copy pixels, mutate the pipeline or establish a GPU-content
lease. Neither slot identity nor format proves vector units, full object-motion
coverage, HUD exclusion or a valid vendor input. Frame/view identity, depth,
active rectangles, exposure and GPU lifetime still require separate evidence.

Evidence: `ida-sr-native-seam.json`, `ida-native-effect-dispatch.json`,
`ida-post-root-followup-0.json`, `ida-post-root-followup-1.json` and
`ida-native-seam-detail-0.json` under ignored local research. The complete HDR
function is retained across its original page and `ida-native-seam-detail-3.json`.

The first bounded run with the new DLL prepared and started successfully and
remained responsive at the main menu. It produced jitter samples but no manager
observations. This negative result is retained in `post-process-menu-20261115.log`;
it does not game-validate the new pass or texture lookup. Its deployment backup
and receipt are in `deploy-backups/20261115-121924-post-process/`.

A second bounded run temporarily supplied `sStartingCell=QASmoke` through a new
SkyrimCustom.ini. The native setting's string, value references and cell resolver
were traced first (`ida-starting-cell-setting.json` and `ida-start-depth-0.json`).
This is an experiment launch aid, not an RSF setting or engine-control policy.
No desktop input was used. The run remained responsive and produced sixteen
post-processing calls, with input target 1 and output target 0. Both allocations
were 1711x1048, single-sample, bind flags `0x28` (render target and shader resource).
Main colour format was 26, R11G11B10_FLOAT; motion candidate was 34,
R16G16_FLOAT. The pointers were readable at this native boundary. No pixels were
captured, so this does not validate scene contents or motion encoding.

The game was closed gracefully. The experiment-only custom INI was removed after
checking that its contents still matched the file created by this experiment.
The original INIs were unchanged. Logs and module inventory are
`post-process-starting-cell-20261115.log` and
`post-process-starting-cell-modules.json`; the config receipt is
`starting-cell-experiment-receipt.json`. The installed DLL remains a development
observer. Its later status-text edit changes no hook or rendering behavior.

Release MSVC build and the 223-test verification run completed. The same two
Anomaly FSR3 FG fixtures failed again; the remaining executed checks passed.
The gate remains failed. Formatting and Clippy passed separately because CTest
stops the verification script before those steps. Logs are
`verify-post-process-full.log` and `clippy-post-process.log`. No Rust behavior
changed. SR, FG, latency and CS/ENB coexistence remain unimplemented.

### Native camera, depth and original motion writers

The next question was how to associate temporal inputs with the actual main
camera instead of selecting the first cache entry or borrowing a reference
header's layout. Traced the scene caller into camera construction, history
advance and shadow-state upload in the installed executable. Named all six
functions in the saved IDA database:

| RVA | Evidence |
| --- | --- |
| `0x101CB50` | Camera cache lookup/allocation. State `+0xB8` points to entries, count is `+0xC8`, stride `0x290`; key is NiCamera at entry `+0` and jitter flag `+0x284`. |
| `0x101E4E0` | Builds view/projection, preserves unjittered copies, conditionally applies temporal jitter and records the camera origin. |
| `0x101D400` | Perspective/orthographic matrix construction from NiCamera frustum. Perspective depth is standard, with `P22=f/(f-n)`, `P23=1`, `P32=-nf/(f-n)`. |
| `0x101CA60` | Advances cached matrices and origin history. First-frame zero history must not be used for reprojection. |
| `0x101C660` / `0x101E7F0` | Selects the camera cache by identity/flags and copies its matrices and origins into rendering shadow state. |
| `0x15399DF` | Native CopyResource reads main-depth index 0 at renderer `+0x2018`, global `0x3333F58`, depth-entry stride `0x98`. |
| `0x15447A0` | Independent depth SRV consumer uses global `0x3333FE0`, the same entry's `+0x88` SRV. |

The cache's unjittered projection is `+0x1C0`, current unjittered VP `+0x140`,
previous VP `+0x180`, current origin `+0x26C`, previous origin `+0x278`.
NiCamera frustum near/far are `+0x160/+0x164`. Current dynamic ratios are state
`+0x114/+0x118`. These are native instruction matches, not imported C++ types.

Correction to reference material: this build's jitter producer writes floats to
state `+0x4C/+0x50`. CommonLib's State declarations label those positions as a
frame count and flag. Neither is a usable input-frame identity here. The
eight-way jitter index at `+0xD0` is also not a monotonic frame ID. The observer
continues to report observation numbers only.

The bounded native-input run matched the main camera global `0x33D3D28` to one
of five cache entries and read all four samples successfully. Near was 15 game
units, far 353840; projection diagonal was approximately 1.19175/2.11867, with
depth terms 1.00004/1/-15.0006, consistent with the native standard-depth formula.
Dynamic ratios were 1/1. The first logged previous-VP components and origin were zero; subsequent
samples contained the prior state. Only two VP components were logged. This
suggests empty initial history, but does not establish all sixteen components;
first-frame reprojection remains unproven and must reset. Main depth was 1711x1048, single-sample,
format 44, R24G8_TYPELESS. These are descriptor/CPU-state observations, not depth
pixels or temporal-quality acceptance. The current CS FidelityFX source uses
0.01428222656 metres per game unit; this is a source convention, not a measured
scene scale in this experiment.

The observer reads these values synchronously at the existing native pass, logs
four bounded samples, and retains no camera/cache/texture pointers. No render
pass is yet admitted to a vendor backend. The run was responsive and closed
gracefully; the experiment-only custom INI was removed after checking its exact
contents. Deployment backup: `deploy-backups/20261115-123652-native-inputs/`.
Evidence: `native-inputs-game-20261115.log`, `native-inputs-modules.json`,
`ida-camera-candidate-*.json`, `ida-camera-producer-*.json`,
`ida-scene-depth-owner.json` and `ida-native-depth-srv-consumer.json`.
Release verification again reached 223 checks with the same two Anomaly FSR3 FG
failures; formatting and Clippy passed separately (`verify-native-inputs-full.log`,
`clippy-native-inputs.log`). GPU processing readiness remains false.

Original packaged shaders were also disassembled read-only using the installed
System32 d3dcompiler_47 D3DDisassemble, against the exact FXP/hash inventory.
For Lighting PS descriptor 1, Grass PS 0 and Sky PS 0, instructions project
current/previous positions with cb12 registers 12..15/16..19, divide by W,
subtract previous NDC from current and multiply XY by `(-0.5,+0.5)`. Written
motion in these paths is therefore previous-minus-current UV with Y down,
without projection jitter. Canonical current-minus-previous UV would negate
both channels. This proves the selected shader paths' arithmetic, not runtime
coverage, object history or unwritten-sample detection.

Effect PS descriptor 0 writes literal `(1,0)` to target 1 instead of that formula.
Lighting PS 1 has a conditional literal write. The same shader may serve an
auxiliary pass, so neither is yet proof of a missing live motion sample.
An original-bytecode census found literal mov/movc target-1 candidates in
1673 Effect, 4 Particle and all 6924 Lighting permutations, with no disassembly
failures. This is a textual candidate search, not complete control-flow analysis.
Some families/permutations have no target-1 signature because they serve other
passes. Actual target bindings and draw eligibility must establish repair scope.
Zero must not become a blanket unwritten-motion rule: zero is valid static motion.

Original hashes and selected disassemblies are recorded in
`native-motion-shader-disassembly-receipt.json`; the bounded candidate census is
`original-motion-candidate-census.json`. This evidence makes the next shader work
specific: correlate family/descriptor/hash with live motion-target draws, acquire
previous transforms at their native producer, and extend the shared replacement
mechanism when an interface/resource change is required. The existing whole,
same-length DXBC instruction operation cannot add missing vertex interpolators
or previous skeletal/object data, and must not be presented as that repair.

### CPU scope, worker input and HDR reinsertion

Question: which native update owns the inputs and scene, and can shared SR output
return to the native R11G11B10 colour target without bypassing HDR processing?

The adapter now detours three additional exact-build entries with 19-byte guards:

| RVA | Fact reported |
| --- | --- |
| `0x658870` | Outer non-nested Main update, FRAME_BEGIN/FRAME_END with a monotonic CPU scope. Its incoming callers ignore the native SHORT return, which the detour preserves. |
| `0xCF95F0` | Actual six-device polling entry. INPUT_SAMPLE is reported only when the calling thread owns that CPU scope. |
| `0x6565B0` | Rendering entry after update work, before state reset, Renderer::Begin and scene/menu routing. Reports SIMULATION_END once inside the scope. |

These hooks report no invented simulation-begin, device activity, Windows message
or vendor marker. The runtime normalizes missing input/simulation boundaries.
The carrier forwards the existing CPU-event C ABI into the runtime's generic
fact cache, with bounded acceptance logs. No sleep, vendor selection or pacing
SDK call was added to the adapter or carrier. CPU callbacks drain before their
borrowed host services retire; disabled trampolines remain process-lived with
the pinned adapter. No primary GPU renderer is published yet, so render_expected
remains zero and these scopes do not authorize FG.

Two responsive no-SKSE starting-cell runs accepted the initial 32 reported CPU
facts and carried source IDs 1..16 into the corresponding pre-HDR observations.
The first run had no scoped INPUT_SAMPLE. A second bounded poll probe explains
why: startup polling ran on the main thread outside an update, then gameplay
polling ran on multiple worker threads while the update/render scope was active.
In the observed log, SIMULATION_END preceded that worker poll. Native callback
`0x6E6030` routes through `0x659DF0` to polling, and its callback/name record at
`0x20B5130` names it "Poll controls". Timing overlap, a latest-tick atomic and the
native counter at `0x33363F8` are diagnostics, not ownership proofs. The adapter
therefore leaves those polls unassigned, and the runtime derives a labeled
substitute instead of pacing at a late or mismatched input sample. The next
latency task is native job submission/join ownership and the relation between
polled controls and the following simulation, not another render counter.

Evidence: `ida-input-caller-*.json`, `ida-native-render-boundary.json`,
`ida-tick-incoming-*.json`, `ida-main-frame-owner.json`, `ida-input-job-*.json`,
`ida-frame-callback-*.json`, `cpu-facts-game-initial.log` and
`cpu-poll-route-game.log`. The loading-loop owner at `0x6E4C20` is distinct and
was not substituted for the actual update tick. The IDA names/comments are saved.
Deployment receipts/backups are in `20261115-125635-cpu-facts/` and
`20261115-130016-poll-route/` beneath the local deploy-backups directory.
Both games closed gracefully and their temporary custom INIs were removed.

The shared SR writeback previously rejected different output/native formats.
`runtime/orchestrator/src/native_reinsert.cpp` now preflights both destinations,
keeps matching-format copies bit exact, and converts floating RGB/RGBA output
through rect-local cached scratch targets before copying to the requested native
rectangle. This preserves surrounding pixels and also works for a native target
without an RTV bind flag. No tonemapping, transfer function or game switch is
introduced. The shared binding scope clears conflicting UAVs, inherited stages
and predication and restores them afterwards; conversion resources retire with
the existing SR pipeline resources. Engine-specific precision/allocation policy
remains adapter work. A higher-precision copy cannot repair precision already
lost in the native source.

The new WARP device fixture covers HDR values up to 32, differing per-pixel
colours, R32 RGBA to R11G11B10 and RGBA16F destinations, offset rectangles,
untouched borders, bit-exact matching formats, aliased mirrors, rejection before
partial native writes, resource reuse/retirement and a conflicting source UAV.
It first caught the existing plain blit's forced opaque alpha; the new explicit
COPY_RGBA mode preserves source alpha, and the existing fullscreen-pass fixture
still passes. This is generic device evidence, not an SR evaluation in Skyrim.
Actual SR host wiring, reduced-resolution ownership, motion contents/repairs,
worker input ownership, final HUD-less FG/UI/Present leases and CS/ENB ownership
remain incomplete. Code-level rendering readiness stays zero.

Final validation for this checkpoint: `eng/verify.ps1 -VS2026 -Configuration Release`
rebuilt the affected shared runtime, carriers and adapters and reached 224 checks.
The Skyrim contract, existing fullscreen pass and new reinsertion device fixture
passed; the same two Anomaly FSR3 FG device fixtures failed. The full gate remains
failed. Formatting and Clippy passed separately after CTest stopped the script.
Logs: `verify-cpu-reinsert-retry.log`, `clippy-cpu-reinsert-retry.log`.
A final start-failure rollback correction retains prepared CPU callback services
for retry. Its successful hook path is unchanged from the bounded game runs;
a forced MinHook enable failure was not exercised in-game.
The bounded poll log contains 32 accepted emitted facts, sixteen pre-HDR scopes
1..16, and sixteen unassigned polls across seven threads. These facts do not
prove actual input-to-render latency, FG eligibility or vendor SR evaluation.

### Native-resolution SR evaluation and camera conversion

Question: can the matched native main-view inputs drive an actual reconstruction
and return its result without losing the game's HDR chain? Start at native
resolution to separate temporal conventions and reinsertion from native extent
allocation. This deliberately admits only dynamic ratios `(1,1)` and quality
Native. No performance or reduced-resolution claim follows from this experiment.

The adapter now accepts only input 1/output 0 from return RVA `0x153ADEC`, the
guarded main call. It selects the unique main-camera cache entry with jitter
flag 1, validates the perspective and `view * projection == current VP`, and
constructs the SDK row-major matrices independently. For camera-relative native
coordinates, world-to-view is `Translation(-origin) * nativeView`. Current clip
to previous clip is `inverse(currentVP) * Translation(origin - previousOrigin) *
previousVP`. The reverse transform is inverted separately. Standard-Z endpoints,
origin translation, round-trip projection, jitter signs/pixels, invalid history,
nonfinite values and mismatched matrices have a dedicated MSVC contract fixture.
History resets on first use, camera/extent/counter discontinuity or disagreement
between native previous and last observed current data. An invertible garbage
initial matrix alone is not accepted as continuous history.

Native context is read at `0x3331F30`; main colour, motion and depth retain the
owners already matched above. The active window pointer at `0x3330198` holds
the native swapchain at +0x18, as consumed by actual Present at `0x100A012`.
`RendererEnd` also increments counter `0x33363F4` after Present. That counter is
render identity only: source_frame_id stays zero and the pass carries NO_FG.
Every admitted texture/context/chain receives a callback-local COM lease;
dimensions, samples, formats, device and immediate-context ownership are checked.
The camera callback interval is measured with QPC, not claimed as a native
simulation delta. The first interval is refused rather than invented.
The approximate units-to-metres value 0.0142875 comes from the pinned CS editor's
Skyrim-to-Havok conversion, not a new game-unit measurement.

The generic runtime D3D11 host copies configuration and identity, reuses the
existing settings/overlay services, owns vendor selection/SDKs, and evaluates
through shared native SR and format-converting reinsertion. The adapter does not
link or load another runtime. The initial prototype starts suspended; adapter
start/quiesce controls an atomic activity gate. Services and SDK resources remain
process-lived. Same-session prepare can reuse that owner; a different session,
device recreation and complete drained retirement remain explicit work.
Initial startup currently depends on DLSS/Streamline, including on a machine
where the desired later SR backend is different. Portable startup is incomplete.
FG and latency choices stay disabled. Native-only quality is the only offered
quality. SR enable/backend/preset, sharpening, colour transport/correction and
a momentary capture action have shared overlay controls. Colour options save to
the generic host's INI section and apply on the next graphics callback.

After successful complete SR reinsertion, the adapter temporarily bypasses
native TAA through its enable byte around the original processor and restores
that byte on return. Native HDR, exposure and bloom still execute. Failure keeps
the native temporal path. Native TAA at `0x1544DA0` checks clear-mask +0x30,
calls `0x1545360` and consumes the mask before evaluation. Bit 0 selects main
history targets 81/82; other bits include water history, motion and auxiliary
targets. Successful SR frames preserve other bits and set bit 0 so the next
native fallback cannot consume stale main history. Reader bytes at `0x1544DF4`
are additionally guarded:
`44 39 71 30 74 09 E8 61 05 00 00 44 89 73 30`.
This follows the native consumer, rather than persisting a game INI override.
IDA evidence is `ida-native-taa-evaluate.json` and `ida-native-taa-clear.json`.
The original IDA worker expired while idle; the saved database was reopened
without reanalysis as session `0044e197`, retaining the original input identity.

The first SR run reached the host but failed NGX initialization. Its project
identifier was ordinary text; Streamline's pinned `sl_core_types.h` documents a
GUID. Supplying a stable valid GUID allowed initialization and actual evaluation.
Do not attribute this failure to lack of SKSE, the driver or hook addresses.
Receipt: `20261115-133537-native-sr/`; logs `native-sr-first-game.log` and
`native-sr-retry-game.log`, with corresponding module inventories. The first
capture retained four correct raw snapshots but had an off-by-one render-ID
join and used a UE motion preview. Its "matched_present" field was premature
and its unwritten preview statistic was inapplicable. Retain that failed capture
as a bookkeeping correction, not evidence of input ownership or missing motion.

The corrected complete capture is
`scene-20261115-134423-927-8579609-34676-1`, under the game's development
`ReScaleFrame/captures/website/`. Native render 1068, source identity null,
1711x1048, standard Z, backend DLSS, native quality, colour transport enabled,
camera-inclusive unjittered motion, scale `(-1,-1)`, settled reset=0 and actual
Present S_OK join its frame/pass/backend metadata. Native/backend raw inputs,
reconstructed colour and final Present colour are present. Native motion is
finite and zero throughout this stationary scene. Native/output RGB is finite
in [0,1]; 98.575% of pixels differ, mean absolute RGB difference 0.008561.
This proves neither HDR highlight quality nor motion coverage. The reconstructed
scene preview was inspected. No moving-camera, actor or gameplay acceptance is
claimed. Evidence: `native-sr-present-game.log`, module inventory and
`native-sr-present-pixels.json`; deployment `20261115-134354-sr-present/`.

A subsequent run switched SR off and on through the shared settings API, without
desktop input. With SR active, native TAA remained enabled outside the callback
and clear mask was 1. With SR off, it remained enabled and the mask became 0,
confirming native consumption of the reset request. Resuming SR evaluated again
and produced another complete capture. This validates fallback routing/reset
consumption and live settings, not visual quality during the transition.
Evidence: `native-sr-fallback-game.log`, `native-sr-off-state.txt`,
`native-sr-resumed-state.txt` and deployment `20261115-135301-sr-fallback/`.
The sustained run also exposed Streamline's missing presentCommon bookkeeping
warning. The shared backend now resolves its common-plugin before/after Present
callbacks for this unwrapped native chain; wrapped/shared-host chains must not
call it again. A failure suspends SR while native Present continues.

The final sustained run observed two paired common-plugin callback successes,
remained responsive through native render 4526 and did not repeat the warning.
Its complete capture `scene-20261115-135811-092-9406765-23512-1` contains settled
reset=0, backend=DLSS, native quality, source identity null and matched Present
S_OK. The actual final-colour preview was inspected, including native compass
and crosshair. This is not a moving-game quality acceptance. Evidence:
`native-sr-bookkeeping-game.log` and module inventory; deployment backup
`20261115-135642-sr-fallback/`. The TAA/history native functions and Present
consumer have names/comments saved in the existing research IDA database.

Moving-camera and independent/skinned/transparent velocity, native extent
production/scaling, original-shader repair, worker input ownership, final
HUD-less/UI leases, FG, measured latency and CS/ENB coexistence remain pending.
CS is still refused until takeover is implemented. No SKSE was deployed or
loaded. All bounded experiments close gracefully and remove their exact
temporary QASmoke custom INI; deployment replacements have local backups.

Final checkpoint validation: Release MSVC/VS2026 gate rebuilt all affected
integrations and ran 226 discovered checks: 210 passed, 14 default opt-ins skipped
and two failed (`anomaly_fg_dlss_sr5_device`, `anomaly_fg_fsr3_sroff_device`).
These are the same failures recorded before this host work; their cause remains
unresolved, and the full gate is failed. Skyrim plugin, camera, D3D11 host ABI,
shared overlay, input capture and reinsertion fixtures passed. Formatting and
Clippy passed separately because the CTest failure stops the gate first.
No Rust behavior was changed. Receipts: `verify-native-sr-final.log` and
`clippy-native-sr-final.log`. This is a development checkpoint, not a release or
completed Skyrim integration.

### Native raster size production and restoration

Question: can backend-requested render dimensions control actual native raster
production, rather than cropping or resizing a completed full-resolution image?
The current CS source supplies useful leads, but its lock assignments depend on
its other hooks. The independent MIT table did not map the relevant private
updater/camera-update IDs. Native global references and call-chain matching were
therefore used instead of guessed Address Library relocations.

Main render entry `0x6565B0 + 0x2D` calls `0x101E130` with State. Bytes at
`0x6565DD`: `E8 4E 7B 9C 00`. This matches CS's original-DRS bypass lead.
The function advances State previous ratios +0x11C/+0x120 and may adjust current
ratios +0x114/+0x118 from native timing flags. The adapter now guards and detours
it, bypassing only while RSF owns the ratios, with entry bytes
`83 B9 28 01 00 00 00 48 8B D1 0F 85 7F 01 00 00`.
Original execution resumes after ownership release; no permanent NOP or game
INI override is installed.

`0x1009960` derives the native viewport from the active target dimensions and
those ratios when State +0x128 is zero. `0x100AE80` uploads a 720-byte per-frame
buffer: c43 holds current/previous ratios; c44 holds reciprocals and width
clamps. When locked, it supplies unit ratios. These are static producer matches,
not a new raw constant-buffer capture. The important correction is `0x101E2D0`:
the native "lock" is a nesting counter. False increments it; true decrements
when nonzero. Treating it as a boolean silently changes scoped native behavior.
The matched functions and consumers have saved IDA names/comments.

The shared D3D11 host now separates backend preparation from evaluation.
A graphics-owner RECONFIGURE callback may apply quality/backend changes and
publish planned render/output dimensions through render_config; it returns
nonzero and never claims a reconstructed output. A verified scene lease binds
the initial device/thread. The adapter requests subsequent preparation at its
native jitter boundary, before geometry. It applies the backend's actual integer
sizes, not a hard-coded quality ratio, carries previous ratios, and supplies
Halton pixel jitter using the real render extent. The render-only Present
counter selects phase; it is not an input/source identifier. Native updater
suppression, state layout, jitter and restoration remain inside the adapter.

The first early-preparation attempt initialized the overlay on the loading
render worker. The process stalled before the first main pre-HDR scene. An
installed WinDbg `cdb -pv` noninvasive snapshot found main-thread waiting through
`0x11B2671`/`0x651353`, and loading worker 15088 inside native SetWindowPos at
`0x1009941`/`0x1009DFB`. It does not establish an NGX or driver deadlock. Evidence
is `resolution-early-init-stacks.log`, game log and module inventory; deployment
`20261115-142307-sr-fallback/`. The test process was force-terminated after its
snapshot and failed close request. Its exact temporary INI was removed. Cold
RECONFIGURE requests are now refused before touching the graphics context, and
adapter preparation waits for the verified scene execution thread. A regression
fixture checks this refusal without a usable graphics pointer. No debugger was
installed and no desktop control was used.

The next probe set lock=1 at jitter, following the isolated CS source assignment.
That was wrong for the vanilla path: requested dimensions changed, but the actual
pre-HDR viewport remained 1711x1048 and raw depth contained geometry outside the
declared reduced rectangle. This failed experiment is retained as
`resolution-lock-one-game.log`, `resolution-first-depth-coverage.json` and
capture `scene-20261115-142842-042-11237718-23060-1`. The probe manifest is partial
because no reduced SR evaluation occurred. Reference code plus changed ratios
was not proof of reduced rendering.

With the native primary scope unlocked, a bounded Quality-mode run produced
1141x699 from DLSS's plan for a 1711x1048 output. State ratios were
0.66686147/0.66698474 and phase count 17. Actual pre-HDR viewport was 1141x699,
lock zero, while allocation descriptors remained full size. Raw depth contained
geometry throughout the active rectangle and clear depth everywhere to its
right and below. The native colour preview was inspected and shows the complete
scene inside that rectangle, with unused allocation outside. This establishes
native raster production at the lower extent, not vendor SR at that extent.
Capture `scene-20261115-143322-259-11517937-2804-1`, native frame 2121, carries
source_frame_id=0, PRIMARY|NO_FG|RECONFIGURE and an explicit SR refusal (-3).
Skyrim's native temporal/spatial path completed the frame. Evidence:
`resolution-unlocked-game.log`, module inventory, state snapshots and
`resolution-unlocked-depth-coverage.json`; deployment `20261115-143235-sr-fallback/`.

Live native-quality/off restored current and previous ratios to 1 and lock to 0.
On resumed native-resolution DLSS, with complete actual-Present capture
`scene-20261115-143559-729-11675406-2804-2` at native render 11357. The final game
closed gracefully, its temporary custom INI was removed, and saved RSF settings
were returned to SR on/native quality. Quiesce keeps the native callback enabled
until ratio restoration has drained; no control-thread graphics work was added.
Forced quiesce during a stopped render loop has not been game-tested.

Reduced modes are currently experimental input probes in the shared quality
control. They retain native evaluation and do not claim an SR output. Before SR
admission, full-resolution native depth/stencil and post-processing auxiliaries
must match the reconstructed colour, with the native camera/viewport constants
updated at that boundary. Face-generation, precipitation and other auxiliary
view exceptions still need native matching. Moving scenes and all requested
shader repairs, FG, latency and CS/ENB handover remain incomplete.

Resolution-owner validation: VS2026 Release gate rebuilt the native integrations
and reached 226 discovered checks: 209 passed, 15 skipped, and the same
two Anomaly FSR3 FG device fixtures failed. Fourteen skips are the default opt-ins;
the additional DLSS-G SR-off fixture could not obtain foreground ownership, so
its activity acceptance was not run. The full gate remains failed and
their cause remains unresolved. The expanded cold-planning host contract,
Skyrim plugin/camera and existing capture/overlay/reinsertion checks passed.
Formatting and Clippy passed separately. Logs: `verify-resolution-owner.log`
and `clippy-resolution-owner.log`. No Rust behavior changed. Auxiliary loading
views, device/thread recreation and complete host retirement are not accepted
by this bounded raster test.

### Native auxiliary consumers and reversible depth preparation

Question: which buffers must follow a reconstructed full-resolution colour image
when the native main scene was rasterized into a smaller rectangle? Source review
of CS dev `33d7896147b54ca72d1ee5ca938bb85739593167` identifies depth,
refraction normals, SAO camera-Z and the underwater mask as leads. Its
`DepthRefractionUpscalePS.hlsl` samples depth/normal data after removing jitter;
its depth state disables stencil. This does not establish stencil preservation
or the vanilla consumer order. No reference implementation was copied.

Static IDA analysis uses the same executable SHA256 and saved database as above.
The native call at `0x15399DF` copies selected main depth into depth slot 7,
POST_ZPREPASS_COPY: destination `0x3334380 = 0x3333F58 + 7 * 0x98`.
It does not copy into slot 1, MAIN_COPY. The earlier source-owner observation
was correct; interpreting this destination as MAIN_COPY would be wrong.
`0x15447A0` can sample an indexed depth SRV, but has only an unwind-data xref;
it is not evidence of an executed main-chain consumer.

Direct global xrefs alone were insufficient. SAO camera-Z has pre-handoff
consumers, but the post-processing effects also obtain it through indexed
ImageSpaceTexture inputs. Following native RTTI, vtables and setup calls gives:

| Consumer | Identification and input evidence | Native timing |
| --- | --- | --- |
| Refraction | TypeDescriptor `0x2197DB0`, COL `0x1C81820`, vtable `0x1B4A7B0`; setup `0x15655B0` calls ImageSpaceTexture colour setup with target 13 at `0x15658E8`; render `0x1565920` binds it as input 2 | ProcessMain effect 1, after RSF's pre-HDR handoff and before HDR |
| Depth of field | TypeDescriptor `0x2197850`, COL `0x1C80A20`, vtable `0x1B48018`; setup `0x1551820` supplies depth input from selector `0x1015790` (returns 0), target `49 + selector(0x153C8F0)`, and conditional underwater target 12 | ProcessMain effect 3; can run before HDR or be deferred until after native TAA according to manager +0x210 |

The depth selector feeds ImageSpaceTexture depth setup `0x1554F90`, which writes
the depth index at +0xC and invalidates its colour index. The SAO selector reads
global `0x3436358`; its live value, chosen allocation/mips, formats and active
rectangles still need capture. Target 13 is refraction normals, 12 underwater
mask, and 49 SAO camera-Z in independent MIT CommonLib
`add17c090cfa312d370bfcdd55d2eb989d086fec`. These names are source-assisted
static matches, not new runtime coverage or visual acceptance. Functions and
comments were saved in IDA. Ignored evidence: `depth-aux-native-consumers.json`,
`depth-aux-native-xrefs.json` and `depth-aux-post-effects.json`.

This evidence rules out admitting reduced SR after repairing main depth alone.
The native HDR chain must receive matching auxiliary images and full-resolution
viewport/constants. Native histories must retain their original reduced contents
and previous ratios for the next frame. A candidate lifetime is temporary
contents from successful SR through final native consumers, followed by exact
restoration on the graphics owner before the next producer. The frame-end hook
and all consumers in that interval remain to be guarded and validated.

The generic graphics library now has a C ABI `depth_scale` transaction, separate
from Skyrim policy. It accepts explicit source/output pixel bounds and source
jitter, requires an immediate context and a single-mip/slice/sample
R24G8_TYPELESS native depth target, and refuses overlapping or foreign-thread
operations. Prepare snapshots the full allocation and renders into separate
scratch. Commit copies scratch into the original resource without changing its
views. Restore copies the snapshot back and releases the native texture lease;
cancelling before commit makes no native writes. Destroy restores a committed
transaction. The caller must keep native contents unchanged between prepare and
commit and must restore after its final consumer, before the next native writer.
Same-context GPU ordering supplies the copy/draw dependency, without a CPU wait.

Sampling is nearest texel at `floor(sourceMin + outputUV * sourceSize - jitter)`,
clamped to the explicit source rectangle. The first depth draw clears stencil
only within the output viewport. Eight stencil-only draws then reconstruct each
bit using the X24_TYPELESS_G8_UINT SRV, avoiding optional stencil-reference shader
features. Outside pixels retain the original packed contents. All preparation
and copies save/restore native bindings, including predication and UAV counters.
Resource and rectangle checks precede native writes. This is a reusable
mechanism with no game identifiers, settings or vendor policy.

The Release WARP fixture validates all 256 stencil values, representative 24-bit
depths including clear and endpoint-adjacent values, offset/unequal rectangles,
both jitter signs, clamping, unchanged outside pixels, deferred preparation,
bit-for-bit restoration, cancellation/destruction, ABI/refusal behavior and
foreign-thread refusal. It also verifies inherited depth/UAV/predicate,
viewport/topology and append-counter restoration. This is synthetic device
evidence. The helper is not connected to the Skyrim callback or deployed and
does not change reduced-SR admission.

VS2026 Release verification rebuilt the native tree: 227 discovered, 210 passed,
15 skipped and the same two Anomaly FG device failures. Fourteen skips are
default opt-ins; DLSS-G SR-off additionally skipped after foreground ownership
was denied. The full gate remains failed. Depth scaling, Skyrim contracts and
existing graphics/host checks passed. Formatting and Clippy passed separately.
Logs: `verify-depth-scale.log` and `clippy-depth-scale.log`. No Rust behavior
changed and no game was launched or installation modified in this checkpoint.

### Live auxiliary allocations and depth units

The adapter now reports native auxiliary descriptors at the verified main
pre-HDR callback. A generic, size/version-checked D3D11 host capture export adds
adapter-selected textures to an existing requested bundle. It requires the
same session, complete view key, native frame and graphics owner, and refuses
before dereferencing graphics pointers without that lease. It starts no capture
and retains no borrowed native resources. The existing shared capture action
controls readback; there is no new diagnostic setting or adapter GPU processing.

Two bounded vanilla QASmoke deployments observed these inputs:

| Native input | Actual allocation and views |
| --- | --- |
| Underwater mask, target 12 | 855x524 R8_UNORM, one mip, RTV/SRV mip 0 |
| Refraction normals, target 13 | 1711x1048 R8G8B8A8_UNORM, one mip, RTV/SRV mip 0 |
| SAO camera-Z, target 49 | 1711x1048 R32_FLOAT, eleven mips, base SRV spans all eleven |
| DOF selected target 60, selector 11 | Native texture pointer is null; RTV and SRV alias target 49's allocation at mip 10, one mip, effective size 1x1 |

The first probe inspected only the selected slot's texture pointer and missed
the DOF allocation. Resolving GetResource through its native view established
the alias in the second probe. This corrects the earlier working assumption
that every post-handoff SAO input needed full-resolution spatial reconstruction.
The observed DOF average-depth input is a 1x1 view; resizing the whole SAO image
for that consumer would change unrelated data without matching its actual use.
Other selectors or post-processing paths still require their own evidence.

Complete native capture `scene-20261115-153111-244-14986921-39312-1` includes
the three base allocations and matched native Present. SAO camera-Z is finite
from 203.54524 to 3388.69287, whereas device depth is 0.926346 to 0.995616.
Linearizing the captured main depth using `projectionB / (deviceZ - projectionA)`
matches camera-Z with median relative error 1.20e-6 and 99th percentile 5.84e-6.
It is linear camera depth in projection units, not a copy of hardware depth.
The pinned CS ISSAOCameraZ source is consistent with this distinction. Its
separate upscaling shader's raw-depth assignment is not a substitute for the
vanilla buffer's measured meaning.

Earlier complete native capture `scene-20261115-152227-196-14462875-36016-2`
and reduced-input/native-fallback capture
`scene-20261115-152423-719-14579406-36016-3` include the underwater mask and
refraction normals. Both allocations are entirely zero in this dry room.
These establish readback and descriptors, not active water/refraction coverage
or their raster rectangles. At primary 1141x699, multiplying the half-size mask
allocation by primary ratios predicts a fractional 570.16657x349.5 span; this is
a calculation to investigate, not an observed producer viewport. Main depth's
stencil is also zero in this scene; nonzero stencil preservation remains WARP
fixture evidence. Reduced SR is still not admitted.

Evidence: `auxiliary-initial-game.log`, `auxiliary-view-native-game.log`, module
inventories, `auxiliary-initial-pixels.json`, `auxiliary-view-native-pixels.json`
and `auxiliary-camera-z-units.json`. Deployment backups are
`20261115-152137-sr-fallback/` and `20261115-152805-sr-fallback/`.
Both processes closed gracefully and their exact temporary custom INIs were
removed. Settings were returned to native quality. No SKSE, CS or ENB was loaded.

The two auxiliary builds passed their shared capture ABI/cold-pointer regression
and graphics/Skyrim fixtures. Each full gate had 228 discovered checks, with the
same two Anomaly FG failures. The final auxiliary-view run had 212 passed and
14 default opt-in skips; its DLSS-G SR-off activity check passed. Formatting and
Clippy passed separately. These receipts are `verify-auxiliary-capture.log`,
`verify-auxiliary-views.log` and their corresponding Clippy logs. The gate remains
failed; water, gameplay, FG, latency and CS/ENB coexistence are not accepted.

### Shared Insert fallback after the user's overlay report

The user reported that Insert did not open the overlay. The active Skyrim log
showed a successfully installed window subclass and cursor hooks, so repeating
installation was not the next step. The shared host used the window-message
toggle without a polling route. Anomaly already had an adapter-local polling
fallback, but the new generic D3D11 host did not. DirectInput paths that bypass
WM_KEYDOWN therefore had no equivalent route here. No physical-key trace was
captured from the user's failed press.

The shared input module now exposes polling of the configured toggle's high
key-state bit. The generic D3D11 callback host invokes it at Present, before the
overlay can skip a hidden idle panel. This call stays in the new host so older
adapters retain their existing polling controls, including Anomaly's
OverlayKeyPoll option. It accepts
foreground root-window ownership only. Window messages and polling share the
held key; a delayed message cannot reverse a polling transition, auto-repeat
does not toggle, and regaining focus with Insert held is not a fresh press.
The existing visibility debounce and cursor handling remain in that mechanism.
No Skyrim-specific input policy or new setting was added to the shared layer.

The new fixture mocks this process's GetAsyncKeyState and GetForegroundWindow,
without SendInput or desktop focus changes. It verifies opening/closing with no
window messages, a held key, delayed duplicate messages beyond the debounce,
message-first handling, auto-repeat, background presses and focus gain while
held. It passes alongside the existing actual-backbuffer overlay rendering
fixture. These establish the missing generic route without pretending a
synthetic key state is a user keyboard test.

Deployment backup `20261115-153837-sr-fallback/` contains the replaced Runtime
and entry DLLs and their hashes. In its QASmoke process, public diagnostics
initially reported started=1, input_installed=1, visible=0, at 1711x1048.
Insert down/up messages posted only to that process's verified HWND opened the
real panel. Diagnostics observed visible=1 and 429 collected visible frames,
then 3763 visible frames before the next Insert messages closed it. The closed
state was visible=0 while native Presents continued. A later read was visible=1
again, with 4227 visible frames; the probe had posted no further key messages.
That read failed the proposed closed-idle assertion. Its input source was not
logged, so it cannot prove stable idle or be attributed to a physical user press.
Route logging was added for the next experiment. The user's physical Insert
acceptance remains distinct from the controlled message test and polling fixture.
No GPU work was run on the diagnostic remote thread; only the documented
any-thread diagnostics API was queried. Native rendering stayed on its owner.

Evidence: `overlay-insert-before.json`, `overlay-insert-open.json`,
`overlay-insert-closed.json`, `overlay-insert-idle.json`, game log and modules.
Release VS2026 verification rebuilt affected adapters: 229 discovered,
213 passed, 14 default opt-in skips, and the same two Anomaly FG device failures.
The full gate remains failed. The overlay hotkey, existing overlay host/device,
Skyrim and capture contracts passed. Formatting and Clippy passed separately.
Receipts: `verify-overlay-insert.log` and `clippy-overlay-insert.log`.

### Startup window binding, DPI and pointer acceptance

The user then confirmed that RSF started only after loading a game. A normal
main-menu launch, without the temporary QASmoke setting, reproduced started=0,
input_installed=0 and zero accepted Presents. The prior valid-scene requirement
had incorrectly gated window/input/overlay startup together with SR.
Evidence: `overlay-menu-cold.json` and `overlay-menu-cold-game.log`.

The adapter now guards native RendererEnd at `0x1009F70` with the entry bytes
`48 83 EC 48 B9 01 00 00 00 E8 22 A9 58 00`. Its wrapper offers the native
chain from active window `0x3330198 +0x18` to a generic size/version-checked
presentation-binding service before forwarding the original exactly once.
This entry contains native sleep and profiler work as well as Present; it is
not used as a latency marker or a source-frame identity. Actual Present remains
observed through the shared DXGI observer. All game identity, bytes, native
pointer selection and hook lifecycle stay in the adapter.

Presentation binding configures window/input/UI observation without starting
an SR vendor SDK. A separate scene-bound flag still rejects early RECONFIGURE
requests, even after the menu UI starts. A valid scene lease is required to
prepare/evaluate SR. This does not repeat the earlier loading-jitter SDK startup
that stalled in native SetWindowPos.

The first binding attempt still failed after startup. Its log proved that UI
first drew on thread 31880, while subsequent main ticks/render callbacks ran on
39988. The host had permanently retained the loading presentation owner.
`overlay-menu-first-bind-failed.log` preserves that failure. The generic host now
serializes presentation, binding and SR callbacks and permits a same-chain
presentation-thread transfer before a scene/SR owner is admitted. The next run
recorded 34972 ->39608 ->32952 handoffs, and actual foreground-polled Insert
open/close transitions. Device/chain recreation and owner transfers after SR
admission remain incomplete; this is not a general retirement solution.

The user next reported an oversized offscreen image and unclickable controls.
The adapter now requests per-monitor-v2 process DPI awareness during its
fingerprinted startup, before window creation. This is game window policy, not
a shared runtime executable switch or an INI resolution override. In the accepted
run the request succeeded. The existing 1920x1080 game preference produced both
a 1920x1080 render target and a physical 1920x1080 client area. The real window
reported per-monitor awareness (2) at 144 DPI; its outer bounds 0,0,1942,1136
fit inside the monitor's 2560x1528 work area. Earlier 1711x1048 capture dimensions
are historical evidence from the unaware startup, not the current DPI-aware run.

The shared foreground input poll also now carries left/right/middle button
levels to the panel when window mouse messages are absent. Those levels are
merged with the existing message/touch input and cleared on focus/visibility
changes. Cursor coordinates continue through the existing presentation-space
mapping. No settings policy or engine input layout moved into the shared layer.
The accepted log records a foreground-polled Insert open followed by two real
panel settings changes (SR backend 2 and then 1), both applied and saved, and
an outside-click close. The user explicitly confirmed both DPI and mouse input
working and authorized continuing the integration.

Accepted deployment: `20261115-161716-sr-fallback/`; log
`overlay-dpi-pointer-accepted-game.log`; physical metrics
`window-dpi-startup.json`. Prior failed startup and thread-transfer deployments
remain backed up at `20261115-160218-sr-fallback/` and
`20261115-160750-sr-fallback/`. The user closed the menu test processes; no force
termination was needed. Normal-menu probes created no temporary custom INI.

The last full-suite checkpoint before final owner/DPI/pointer corrections was
229 discovered, 213 passed, 14 default opt-in skips and the same two Anomaly FG
failures (`verify-presentation-bind.log`). Final corrections received focused
Release MSVC builds and direct game/user validation, following the user's
instruction to stop rerunning full fixtures for small edits. The exact final
code has not had another full-suite run. Logs: `build-presentation-owner.log`
and `build-input-dpi.log`. No Rust behavior changed. This accepts the reported
startup/DPI/pointer path, not reduced SR, object motion, FG, latency or CS/ENB.

### Shader-input and motion coverage audit status

The following table separates the native archive/source audit from runtime
coverage. No shader repair has been implemented yet. The first nine native
families and their descriptors are mapped above; the remaining image-space and
compute containers and all moving-object paths still need producer/binding
evidence. The current adapter reports NO_FG and rendering_ready=0.

| Pass/material class | Required inputs and available producer/binding evidence | Gaps and validation |
| --- | --- | --- |
| Static/instanced lighting and terrain | Native Lighting family; main depth 0 and motion 7; selected original PS uses current/previous unjittered screen positions. Main camera matrices/origins are captured and converted independently. | Per-variant world/instance/terrain buffers, slot ranges and actual writes are not established. Static dry-room zero motion is not moving coverage. |
| Skinned/deformed actors | Native updater0x1015800 retains previous bone arrays, uploader0x1578c80 binds VS b9 previous/b10 current; object updater0xeebf40 retains previous world transform. Live CPU history and four uploaded GPU buffer pairs verified below; two actual Lighting VS bytecodes consume both histories. | Final indexed-vertex ranges, draw-time bindings/write masks and deformation output remain unverified. Current bones or camera reprojection will not stand in for previous deformation. |
| Distant trees and RunGrass | Both native families mapped; all18 grass VS read separate current/previous wind timers. Two tree VS export previous positions; native tree sample reuses model position with separate world transforms. | Timer/instance producers, wind/LOD transitions, alpha-tested coverage and moving captures remain unverified. CS tree wind is an additional deformation path, not proof of native coverage. |
| Effect and particle/translucent geometry | Native Effect/Particle families mapped; literal-output candidates found in bytecode. | A literal is not proof of a missing live vector. Need actual variants, depth/motion blend/write masks, transforms and particle history before targeted repair. |
| Water/refraction | Water family mapped; native depth, target 13 normals and conditional target 12 mask identified at their consumers. | Dry-room auxiliary buffers are zero. Active water displacement history, transparency/depth coverage, fractional viewport and vector writes need capture. |
| Sky | Native Sky family and selected PS motion formula inspected. | Rotation-only/infinite-depth history, native bindings and moving-pan capture remain unverified. |
| Volumetrics/clouds/GPU-driven paths | Camera, depth, storage/constant buffers, histories and dispatch resources must be traced at their producers. | Remaining compute/image-space containers are not fully classified; active paths, precision, identities and motion coverage are open. |
| Depth, HDR and exposure | R24G8 main depth captured; pre-HDR colour and original HDR chain retained; SAO linear-depth units and DOF mip alias measured. | HDR highlights, exposure producer/consumer behavior and post-SR auxiliary corrections remain unaccepted. |
| Blood splatter, UI, menu and camera-free paths | BloodSplatter family mapped; shared UI draws at actual final Present; menu UI now independent of scene admission. | Native HUD-less/UI leases and blood-splatter ordering require proof. These paths retain native rendering; no blanket camera or velocity shader replacement is applied. |

### Reduced SR auxiliary handoff and zoom correction, 15 November 2026

The question was why changing SR quality cropped the displayed image despite
correct output dimensions. The first connected reduced-SR experiment, deployment
`20261115-164643-sr-fallback`, completed DLSS at 1280x720 into 1920x1080.
Capture `scene-20261115-164811-431-19607109-21100-1` shows correct full-view
`sr_output` and a cropped `final_color`. Thus SDK success and a full viewport
were insufficient. Merely marking native per-frame constants dirty did not fix
the postprocessing chain, which never called their uploader at this handoff.

Matched executable analysis in IDA identifies uploader RVA `0x100ae80`, guarded
by `48 89 5c 24 08 55 48 8d ac 24 e0 fd ff ff 48 81 ec 20 03 00 00`.
It updates constant buffer global `0x3331f08` only when byte `0x3336400`
is set. Registers c43/c44 contain current/previous resolution ratios and
reciprocals/clamps. Callers are camera setup and native scene rendering, rather
than the later image-space chain. A guarded hook records flags from actual
uploads on the graphics thread; the adapter reuses those flags when explicitly
uploading output-size constants before native HDR and producer-size constants
after RendererEnd. The adapter also calls guarded viewport RVA `0x1009960`.
No remote thread performs these GPU operations.

The generic host now prepares all auxiliary scratch surfaces and native colour
rollback snapshots before SR, commits only after evaluation succeeds, and keeps
a session/native-frame/view/thread scope until the adapter reports completion.
Main depth/stencil, target13 refraction normals and target7 motion are reconstructed
from the verified active rectangle with jitter removal, then restored after
RendererEnd. Target12 uses its fractional half-size rectangle without scene
jitter. Its producer `0x1546e80`, called by `0x52ddf0`, draws native clip-space
screen-splitter geometry. DOF clears it at `0x1550cbb` and clears its active flag
at `0x1550cda`; retirement preserves that later native clear. SAO and its scalar
DOF mip remain unchanged. These are native sampling/lifetime facts, not vendor
policy in the adapter.

The first raw reduced capture matched reconstructed depth and all stencil bits
exactly to the expected point-sampled, jitter-corrected source mapping. A focused
Release WARP fixture checks fractional bounds, colour channels, depth/stencil,
rollback, frame identity, overlap refusal and preservation of later native clears.
Focused MSVC Release builds passed. No full fixture suite was repeated.

Corrected deployment `20261115-165952-sr-fallback`, PID39248, produced complete
captures `scene-20261115-170305-284-20500968-39248-1` (Quality) and
`scene-20261115-170335-553-20531234-39248-2` (Performance). In each capture,
visual comparison of same-frame SR output and final native colour preserves
landmark placement across the full image. Camera movement between captures
precludes treating them as a stationary quality comparison. Physical client
and output both measure 1920x1080, HWND awareness2 at DPI144. Off restored four
ratios to 1, lock0 and native TAA enabled with its clear mask consumed. Quality
was restored and SR enabled again. Logs: `build-native-post-constants.log` and
`auxiliary-sr-zoom-corrected-game.log` under ignored research material.

This establishes the reported cropping correction and bounded dry-room reduced
DLSS path. Active water/refraction, moving shader coverage, other backends,
device recreation, FG, latency and CS/ENB coexistence remain unaccepted.

### Temporal jitter sign correction and texture-detail gap

The user reported severe XeSS Ultra Performance smearing and blurry DLSS even
at Quality after the cropping fix. Those reports invalidate any broader visual
quality acceptance inferred from correctly fitted landmarks. Capture
`scene-20261115-171000-331-20916015-39248-3` preserves the reported XeSS
640x360 input/output path; it has reset1 and is not a settled quality comparison.

Native `State_BuildCameraCache`, RVA `0x101e4e0`, copies unjittered projection
to cache+0x1c0, then postmultiplies its raster projection at +0x80 by an NDC
translation built from State+0x44/+0x48 (alternate pair +0x4c/+0x50 for bit9).
The code at `0x101e6e7..0x101e751` and a read-only live camera-cache sample
confirm projection entries [8]/[9] carry those translations. Thus projected
geometry moves by `(nativeX*renderWidth/2, -nativeY*renderHeight/2)` in Y-down
pixels. The adapter had reported the opposite sign on both axes. Intel's
[XeSS guide](https://github.com/intel/xess/blob/main/doc/xess_sr_developer_guide_english.md#jitter-offset)
and the installed SDK's DX12 sample agree with the geometry-displacement
convention. This is a producer/API mismatch, not evidence of low vendor quality.

The adapter now reports the measured sign. Auxiliary reconstruction retains
its previous source mapping by negating the API jitter for its subtract-offset
sampler. A focused camera check independently projects a point through the
native clip translation and compares screen displacement against reported
jitter; it replaces the old check that merely asserted the mistaken sign.
Release plugin/camera builds and that check passed, with no full fixture rerun.
Deployment `20261115-171418-sr-fallback`, PID39896, contains the correction.
The user continued live backend switching. Captures
`scene-20261115-171628-054-21303734-39896-3` and
`scene-20261115-171740-464-21376140-39896-4` establish settled FSR4
640x360 evaluation with reset0, corrected jitter and native final Present.
They do not establish corrected XeSS/DLSS moving quality. No sharpening was
added to mask the defect. Evidence: `build-skyrim-jitter-sign.log`,
`jitter-projection-native-live.json`, `jitter-sign-before-game.log`.

After the user closed that process, fresh PID35796 retained the same corrected
deployment. Settled captures `scene-20261115-172016-537-21532218-35796-1`
(XeSS Quality1130x636) and `scene-20261115-172048-043-21563718-35796-2`
(XeSS Ultra Performance640x360) both have reset0 and complete final Present.
The inspected outputs retain fine chest edges and stone detail without the
earlier severe streaking. This is a static-room observation, not moving-scene
acceptance or an isolated numeric comparison against the reset1 earlier capture.
`scene-20261115-172139-222-21614906-35796-3` also establishes settled
DLSS Quality1280x720 with reset0 and visibly resolved stone/chest detail in the
inspected SR output. `jitter-sign-vendor-checks-game.log` retains the switches.

Texture mip selection remains another quality gap. The pinned CS State.cpp
computes log2(render/output) and its shaders use SampleBias; its DLSS policy
adds a further -1. RSF's native adapter currently has no equivalent verified
material sampler adjustment. Native setting string `fMipBias:Display` points
to setting name field RVA `0x20d6a68`, but no consumer or scene-only lifetime has
yet been proven. An INI write or global sampler bias is not an implemented
repair. Trace the material producer/binding and preserve UI/shadow sampling
before adding the appropriate shader/sampler correction.

### Native skeletal and object history producers

To determine whether skinned motion needs new history or only corrected writes,
the NiSkinInstance name registration led to factory RVA `0xf09a10`, constructor
`0xf0a690`, and its matched 0x88-byte layout. Fields +0x48/+0x50 are separate
current/previous bone arrays, +0x38 a frame stamp, +0x3c matrix count, +0x40
registers per matrix, +0x44 allocated bytes and +0x60 the critical section.
Following references to the already proven native renderer counter located
updater `0x1015800`. Under that lock, once per native counter, it copies old
current matrices to previous, computes new current matrices, and initializes
previous from current only after allocation changes. Each matrix occupies
48 bytes. This first-allocation initialization is a native history reset, not
a generally valid substitute for previous animation.

The direct caller `0x1578c80` updates the skin, uploads current and previous
arrays separately, and binds VS constant buffers b10 and b9 respectively.
It uses three float4 registers per bone and TLS+0x2a00 to skip repeated work
for the same skin. The actual capacity and partition/index mapping still need
draw-time verification; the reference shader's 240 float4 declarations do not
prove all native draws fit that range. Object updater `0xeebf40` preserves the
old 0x34-byte world transform at object+0xb0 before updating object+0x7c,
with native counter/initialization/reset branches. These functions were named
and saved in the matched IDA database. `native-bone-history-producer.json`
retains the decompilation. No live bone capture or shader repair is claimed.

### Native material sampler correction, 15 November 2026

The remaining texture-detail question was whether native dynamic resolution
already compensated material mip selection. Following the renderer's sampler
table references gave a concrete answer: creator RVA `0x102b910` creates six
address modes by five filter modes at global `0x3331990`, stride40 per address
and stride8 per filter, with descriptor MipLODBias hardcoded to zero. Filters0/1
have MaxLOD0; filter2 is mip-linear, filter3 anisotropic and filter4 comparison.
The `fMipBias:Display` name remains only an unconnected lead and is not used.

Native state flush RVA `0x1010de0` consumes PS sampler dirty mask `0x20cfc08`
and indexes the table using address/filter arrays `0x20cfcbc`/`0x20cfcfc`.
Its PSSetSamplers call is `0x10113ae`; the same routine flushes CS resources
and other state. Its argument controls input-layout preservation, not compute
ownership. Indexed/nonindexed native draw functions call this routine before
issuing D3D draws. The adapter detours its guarded entry bytes:
`4c 8b dc 41 56 48 81 ec a0 00 00 00 8b 15 0e ee 0b 01`.
The matched functions are named and saved in the IDA database. Decompilation:
`native-sampler-table-and-bindings.json` in ignored research material.

The integration now supplies the table's native PS baseline samplers to a
size/version-checked generic runtime service after native state flush. Only
native cached colour1/depth0, unlocked primary reduced raster and its verified
graphics owner request log2(renderWidth/outputWidth) bias for filters2/3.
Output-size postprocessing, other colour/depth targets, comparison and no-mip
filters request native bindings. RendererEnd also drains an owned replacement
before Present, and quiesce waits for that retirement. This is an automatic
resolution-derived correction; it adds no user setting or vendor-specific bias.
Explicit shader SampleLevel operations remain unaffected by sampler LOD bias.

The runtime owns a bounded32-entry cache and per-slot leases for currently
installed replacements, including entries evicted while bound. It changes
only MipLODBias and preserves every other descriptor field. A sampler foreign
to both the supplied native baseline and the service's own last replacement
is left untouched. No native sampler table or INI is overwritten. Device and
immediate-context/thread mismatches refuse. Device recreation remains unsupported.

Focused MSVC Release builds passed. The new WARP check passed additive bias,
descriptor preservation, exact baseline restoration after80 cache changes,
foreign-binding preservation, unselected comparison preservation and invalid
mask/context refusal. It ran with the game closed, then the focused check ran
again after making the deferred-context case independent of the invalid-mask
case. No full gate was rerun. Logs: `build-sampler-bias.log`,
`build-sampler-bias-focused.log`, `build-sampler-bias-final.log`.

Deployment `20261115-172918-sr-fallback`, PID11404, logs actual native scene
sampler replacement at1280x720 to1920x1080 with bias−0.584962 and mask0xffff.
Complete capture `scene-20261115-172956-834-22112515-11404-1` has settled
DLSS Quality reset0 and a full native final Present. Source/output inspection
shows retained material detail. The attempted cross-session source comparison
found a view-to-world delta0.05289, so its gradient increase is not an isolated
quality measurement (`mip-bias-source-comparison.json`). The camera movement
must not be omitted from interpreting that result. Active shadow/water/refraction,
all material families, moving temporal quality and CS/ENB remain unaccepted.
Off/on also restored four native ratios1, lock0 and TAA enabled/clearMask0.
The final adapter skips runtime sampler calls when no slot needs replacement
or draining; it was Release-built and redeployed at
`20261115-173439-sr-fallback`, PID36664. The sampled capture above predates
that no-op optimization; the correction and scope policy are unchanged.

### Direct DLSS motion-direction correction after third-person smear report

The user identified skin smearing during a third-person orbit after idling and
then grain/trailing across props as well. Their F10 captures are
`scene-20261115-173508-646-22424328-36664-1` and
`scene-20261115-173512-096-22427781-36664-2`, both settled DLSS Quality.
This remains a temporal-quality defect, not accepted moving rendering.

Raw native vectors in a right-wall region match previous-minus-current UV
computed from captured depth and clip-to-previous matrices. Subtracting the
actual geometry jitter from current raster positions gives camera motion
medians7.53/9.07 input pixels in the two frames, with residual p95 below0.009
pixels (`native-moving-wall-motion-check.json`). Thus the sampled native camera
path is coherent; this does not validate the final vendor conversion. Manual
skin/torso rectangles have nonzero motion and roughly0.24..0.27 pixel residual
from camera-only prediction. That is not proof of correct deformation history
or a missing-buffer repair (`third-person-skin-motion-regions.json`).

The requested comparison against pinned Community Shaders dev33d7896147b54ca72d1ee5ca938bb85739593167
resolves the dispatch direction: Common/MotionBlur.hlsli returns
`(-0.5,+0.5)*(currentNDC-previousNDC)`, previous-minus-current Y-down UV.
Its Streamline.cpp supplies cameraMotionIncluded=true, unjittered vectors,
mvecScale(1,1) and jitterOffset(-jitter.x,-jitter.y). Upscaling.cpp writes native
projection offsets(-2*jitter.x/renderWidth,+2*jitter.y/renderHeight), agreeing
with RSF's corrected geometry-displacement jitter. FidelityFX.cpp likewise
scales those native vectors by positive render dimensions. This was source
comparison, not a Community Shaders runtime coexistence test.

NVIDIA's [DLSS programming guide](https://github.com/NVIDIA/DLSS/blob/main/doc/DLSS_Programming_Guide_Release.pdf)
section3.6 defines vectors that, added to current pixel position, reach the
previous position. The latest main PDF fetched15 November2026 has SHA256
`dca4d2e68ccedc04a431803595867a893474b1cce033e65a41e9eab9f6b436f8`.
Streamline's inspected dlssEntry.cpp forwards mvecScale multiplied by render
dimensions to NGX without another direction flip.

RSF's adapter correctly declares native-to-canonical scale(-1,-1), but the
generic D3D11 pipeline passed that canonical direction directly to DLSS when
camera motion was already included. Its separate dense-resolve and alternate
backend paths already converted back to previous-minus-current. This explains
why the earlier native-buffer check could pass while DLSS received reversed
motion. The direct canonical DLSS path now negates its scale once. Already
resolved pixel motion and unspecified legacy encodings retain their scales;
no game or engine identity selects this correction in the runtime.

Focused Release runtime build and the frame-assembly contract passed. The
contract maps known current/previous feature locations through the actual
conversion helper and checks resolved motion is not flipped twice and legacy
scale is preserved. The initial build invocation used the nonexistent target
rsf_frame_assembly_test; the corrected rsf_frame_assembly target built and passed.
No vendor device fixture or full gate was rerun. Deployment
`20261115-174240-sr-fallback`, PID16972, has complete settled capture
`scene-20261115-174318-760-22914437-16972-1`: motion_scale(1,1), reset0,
1280x720 to1920x1080 and native final Present. This stationary capture verifies
the corrected dispatch parameters, not the reported idle/orbit acceptance.
The third-person smear and broad moving quality remain open pending that check.

### Live skeletal history and small-family audit, 15 November 2026

After the direct DLSS correction, the user reported that rendering looks much
better and requested continued auditing. This is positive user feedback for the
current deployment, not acceptance of every material, backend or motion path.
The remaining question is whether native independent deformation history reaches
the active shaders. Bounded REA probes avoid adding diagnostic options or changing
engine inputs. PID16972, executable base0x7ff6c7ed0000 and graphics thread43256
identify this observation; executable fingerprint and reference revision remain
those recorded above. Scripts and raw results stay in ignored research material.

The first probe guarded bone binder0x1578c80 and observed1997 samples across
12 skin instances. Every observed consecutive-frame previous array exactly
matched the preceding current array, with no history mismatches or read errors.
Gapped observations were excluded from that comparison. This tests CPU history
at its actual producer/binder lifetime, not actor identity or rendered coverage.
Evidence: `native-bone-history-live.json`, REA
`ev_a13b60e40a6d1cbf610772d85a3919246ee19809b7e961b9163139a331571488`.

A second probe read actual VS b9/b10 through the immediate context after native
binding, on its graphics owner. Staging copies were mapped and released within
that callback. Four instances with40,23,20,8 matrices had GPU previous/current
data bit-identical to their respective CPU arrays for all matrix bytes.
Each buffer has3840 bytes, matching240 float4 registers or80 matrices. Unused
trailing bytes are not history. These four partition palettes were identity
maps with matching counts; actual indexed vertices and weights were not read.
The attempted root-name read included invalid trailing bytes and is not reliable
actor identification. Evidence: `native-bone-binding-live-evidence.json`, REA
`ev_be9b1a2a766b63559e6d21f8e656ca404146f6b235cf87b38f04ff635f5c6b81`.

The two actual native VS bytecodes match packaged Lighting shaders:
descriptor514, SHA256
`f5ad2129e298a445bb6e887777e2290c3829876cdc38864ecc4d34e838e10db1`,
and83886599, SHA256
`691c1913a9fcc3344940f204796c96d8353afd62bb2af08ae06b7c6625f89bcf`.
Both dynamically read b9/b10[240] and export current/previous world positions.
Actual VS/PS COM objects matched their native wrappers. PS descriptors4194817
and88081927 identify associated archive candidates, but PS bytecode was not
recovered from the actual COM objects. Those candidates conditionally write
literal(1,0) when cb2[7].z exceeds1e-5. Pinned CS Lighting.hlsl identifies that
field as SSRParams.z, selecting auxiliary colour output. Without draw-time
flag, MRT binding and blend/write-mask evidence, it is not a missing-vector bug.
Pinned Common/Skinned.hlsli independently agrees on b9/b10 and240 registers.

The first final-draw probe failed on an unsupported UInt64 method and captured
no records; it detached cleanly. The corrected probe and a separate seven-draw-
method counter also captured no draws. The native frame counter subsequently
remained79991 across reads. This observation window provides no draw-coverage
evidence and does not establish a renderer failure. No graphics call was issued
from a remote diagnostic thread and no shader repair was made. Corrected probe:
`probe-skinned-draws-v2.js`, REA
`ev_b543d09d7dec631176fc879b1f70bb6a4379ffbcf9522c7ddb0a69a4f82e25c4`.

Offline D3DDisassemble review covered all74 containers in the four small native
families, using the previously fingerprinted shaders011.fxp. Reproduction script
`audit-small-family-inputs.py` and per-container hashes, constants and signatures
are in `small-family-inputs/manifest.json`.

| Family | Native VS / PS | Input/output evidence | Remaining scope |
| --- | --- | --- | --- |
| RunGrass | 18 / 18 | All18 VS read cb2[17].w and cb2[18].w and export previous position. Selected VS separately evaluate wind and transform previous vertices with cb2[12..14]. Fourteen PS signatures include target1. | Live timers, instance/fade buffers b7/b8, depth permutations, alpha coverage and final target writes. |
| DistantTree | 4 / 4 | Two VS export previous positions; two PS expose target1. Selected colour VS transforms the same model position through separate current/previous world matrices. | Native depth variants and LOD history. CS TREE_WIND additionally displaces current/previous model positions using separate wind values; vanilla bytecode does not establish that optional path. |
| Particle | 6 / 6 | All six PS expose target1 with fixed XYZ values and computed alpha; no VS previous-position semantic. CS Particle.hlsl names target1 Normal. | Actual target binding and ordering, particle history and reactive handling. Target1 alone does not identify motion. |
| Sky | 9 / 9 | All nine VS export previous position and all nine PS expose target1. | Rotation/infinite-depth semantics, live constants and moving-pan output. Signatures alone do not validate vectors. |

These are source/bytecode and bounded live-binding checks. They do not replace
moving scene captures. Grass, particles, water, sky, auxiliary views, material
variants and final indexed skinned draws remain open. No build or full fixture
suite was needed for this research-only checkpoint.

### Native interface boundary capture for FG research, 15 November 2026

The pre-HDR SR point is not the completed scene needed by FG. The native render
entry0x6565b0 calls world rendering through0x656d10/0x656e60; the latter reaches
the already verified postprocess caller at0x6578d1. RenderEntry later invokes
UI drawing at0x656921, then retains screenshot/output branches before its
RendererEnd call at0x656bb1. Thus neither an SR callback nor a render counter
alone establishes final HUD-less or Present ownership.

IDA identified UI drawing at RVA0x116ac80 by its menu stack at UI+0x110,
count+0x120, enabled byte+0x1c0, IMenu flags+0x1c and vfunc+0x30 calls.
CommonLib's IMenu names slot6 PostDisplay, slot7 PreDisplay and flag0x200
TopmostRenderedMenu. The earlier UI routine0x116b080 invokes slot7 only for
flag0x1000 RendersOffscreenTargets. These structural matches corroborate the
CS dev33d789 MenuManagerDrawInterfaceStartHook lead despite absent tableID82084.
CS calls PostDisplay before this function and redirects the framebuffer RTV
for its UI layer. That is a source comparison, not proof of our UI composition.
The native function can also copy main1 into framebuffer0 at0x116af95. Therefore
the pre-interface backbuffer remains a candidate until actual contents are read.

The exact-build adapter now hooks this native boundary with expected bytes
`4c 8b dc 56 57 41 54 41 55 41 57 48 83 ec 50`. Existing F10 captures gain
`native_before_interface` and `native_after_interface` backbuffer snapshots.
The pre-HDR callback copies only session/frame/view, chain identity and output
dimensions into graphics-thread-local storage. The UI callback checks native
frame and singleton identity, reads the current chain, borrows its backbuffer
and sends the snapshot synchronously to the existing generic capture service.
It retains no native texture or context across callbacks. The host still requires
its armed capture, matching graphics owner and frame/view lease before readback.
Quiesce stops new capture requests and stop disables the hook and waits for calls.
No extra setting or per-frame shader replacement is introduced. The generic
capture header now documents its existing same-frame lifetime through Present.

Focused MSVC Release adapter build and game_plugin_contract passed. No full gate
or vendor device fixtures were rerun. PID16972 remains on the preceding deployed
adapter; this new hook is built, not deployed or game-tested. IDA names
RSF_Research_UI_DrawInterface and RSF_Research_Main_RenderWorld and comments
were saved in the existing database. Full decompilation evidence is retained in
ignored `native-interface-boundary-20261115.json`. Next validation must compare
both snapshots with actual final Present in one F10 capture, including HUD and
menus, before declaring HUD-less coverage or designing UI extraction. Source
frame association, FG resource retirement and latency input-job ownership remain
required; the adapter continues to report NO_FG and readiness0.

### Native delivered input kinds and task ownership, 15 November 2026

The latency investigation now traces the source of worker polling rather than
assigning it the latest update counter. The executable's callback/name table
contains 75 records in the inspected range0x20b4d00..0x20b51c0. Group builder
RVA0x60ef30 reads16-byte callback/name pairs, recognizes signal/sync records,
adds ordinary callbacks with0xebb070 and stops at JobListEnd. The Main render
end group starts at0x20b50e0 and includes Post process at0x20b5120 and Poll
controls at0x20b5130. These are native labels, not claimed GPU pass identities.

During gameplay the outer update calls group submission0x60eaf0 at0x658b0f.
It wakes manager0x3220a90 through event+0x58; manager loop0x60f1e0 submits and
joins the main groups from0x3220a78. The outer update joins that manager through
0x60eb80 at0x65932f, after RenderEntry. Native list executor0xebc170 atomically
claims callback indices, so another worker may execute the next callback before
the previous one completes. Table order is not completion order. In particular,
Post process waits for event20 and signals22, while Poll controls is a separate
callback. The generation bracket is now identified, but the simulation which
consumes those controls and a safe pre-input pacing point remain unverified.
No source ID is assigned from the latest-tick atomic or native render counter.

A separate factual boundary is available without that association: after polling
six devices,0xcf95f0 passes an InputEvent** to0xcfa650, which calls event sinks.
It then clears the native queue at0xcfba10. The button producer0xcfb340 writes
device at event+8 and links next at+0x10. Constructors0x7b1350,0xcfa070 and
0x972660 confirm the header, button type0, mouse device/type1/1 and controller
thumbstick device/type2/3. Current CommonLib InputEvent/InputDevices agrees on
the0x18-byte header and keyboard0, mouse1, gamepad2, virtual keyboard3.

The adapter now guards and hooks dispatch0xcfa650 with19 expected bytes:
`48 8b c4 48 89 50 10 55 56 57 41 54 41 55 41 56 41 57 48`.
Before the native call it copies only event headers into a delivered-kind mask,
on the native callback thread. After the original dispatch returns, it emits the
existing versioned INPUT_EVENT with INPUT_UNFRAMED, source0 and the dispatch-entry
QPC timestamp. It retains no event pointers, text, keys, button values or deltas.
Buttons, mouse movement, characters and thumbsticks are classified only for
their known devices; connection/unknown types are not control input. Invalid,
cyclic or excessive lists refuse the entire observation. This reports delivered
event kinds, including held-button events if the engine emits them, not a
measurement of a new physical transition or final input-to-photon latency.

The existing runtime owns bounded association, marker mapping and pacing.
The adapter adds no sleep, vendor selection or extra setting. CPU lifecycle
draining includes this new hook. Actual worker input sampling remains unassigned;
these facts do not authorize FG or enable a latency technology.

Focused MSVC Release adapter/input builds passed. skyrim_input_contract passed
known kinds, mixed lists, unknown/mismatched types, connection exclusion, empty
lists, unreadable heads/nodes, cycles and bounded/excessive termination.
game_skyrim_se_plugin_contract also passed. No full gate or GPU fixtures were
rerun. At that checkpoint PID16972 still used the preceding deployed DLL.
The deployment and bounded capture below supersede that deployment status.
IDA names/comments were saved. Reproduction: ignored
`inspect-native-frame-tasks.py`, `native-frame-task-table.json`,
`native-event-signal-sites.json` and `native-input-ownership-20261115.json`.
The table scan and decompilation are static executable evidence, not a runtime
job-order experiment. Exact executable fingerprint remains the one above.

PID16972 subsequently exited. Adapter-only deployment backup
`20261115-182739-native-input-interface` preserves the previous Entry DLL, INI,
loader log and receipt; the existing Runtime DLL was not replaced. New PID39676
accepted plugin prepare/start, reached QASmoke and remained responsive. The
experiment-owned starting-cell Custom INI was removed after startup, only after
checking its exact contents. No pre-existing custom INI or user settings were
overwritten. Native dispatch hook startup is therefore game-observed, but no
deliberate physical-input experiment or positive delivered-kind emission has
been verified yet. That narrower acceptance must not be described as measured
latency or complete keyboard/controller coverage.

Complete capture `scene-20261115-182806-092-25601765-39676-1` has native frame897,
DLSS Quality1280x720 to1920x1080 and both interface snapshots. Visual inspection
shows a scene without HUD before the interface, with compass/crosshair/stamina
HUD afterwards. All8,294,400 bytes of the after-interface buffer exactly match
the actual pre-overlay Present buffer. The before/after difference is21,997
pixels:20,706 in the compass band,432 around the crosshair and859 in the lower
HUD rectangle[1400,1004..1816,1026]. The comparison is same-frame, so scene
animation and differing jitter phases do not contaminate it. Raw buffer SHA256:
before `fa4b8af1281e7833cad78ba85470b08d8359a0c353a6629753351344eb140cfa`,
after/final `1e9f207eddda495812468207fae182042c70305c4fc79791676b27fbd60932b6`.
Receipt: ignored `interface-boundary-live-comparison.json`.

This proves the HUD boundary for this vanilla dry-room frame. Menu transitions,
special output/screenshot branches, CS/ENB composition and a compositable UI
layer remain open. Source identity remains null and NO_FG remains set. A clean
HUD-less capture does not by itself make generation or pacing safe.

### Native update, final scene and Present association, 15 November 2026

The same-thread update-to-render association was being discarded: CPU hooks
already assigned an outer update identity, but the SR pass always published
source0. The adapter now copies that identity only inside the actual synchronous
RenderEntry invocation. Each invocation receives a distinct monotonic scope;
nested invocations restore their enclosing scope. No worker poll, latest-tick
atomic or native render counter supplies this source. It identifies the CPU
update which executes rendering, not the physical input consumed by simulation.
Worker-input association remains unproven. NO_FG remains set and screen unknown.

At the verified before-interface boundary the adapter publishes paired native
FINAL_SCENE facts with the current backbuffer as scene_surface, then opens a
WINDOW scope carrying the same copied source, native frame, view and native
window-owner keys. Its explicit swapchain COM lease lasts through RendererEnd
and actual Present; the scope closes after RendererEnd returns. No borrowed
SR texture/context survives its original callback. Quiesce can drain an already
open window scope; active-call and pending-scope state prevent premature stop.

The generic D3D11 host now supplies the existing render_pass callback for these
ownership facts. It checks session, graphics thread, context/device and scene
resource device, then uses the shared scene/window owners. Actual Present is
reported to the same window owner. This does not create a generation provider,
submit vendor latency markers or authorize pacing. An older host without this
callback keeps the adapter's previous source0 capture behavior.

Generic capture records now use a declared CPU source when available and retain
native frame separately for auxiliary/interface readbacks. Source-bearing final
capture requires a matching current window and completed scene before reading
the actual Present backbuffer. Engine metadata records submission and all window
keys. A later native frame/view/family/scope cannot overwrite an already selected
capture's metadata, even if its CPU source is the same. Render-only fallback
continues to carry an ambiguous source and NO_FG.

Focused MSVC Release runtime/adapter builds and d3d11_host_contract plus
game_skyrim_se_plugin_contract passed. The extended input_dump WARP check
passed source/native counter separation and stale-native refusal, including
an attempted metadata rebind before an invalid graphics pointer. It ran after
the bounded game had exited. No full gate or vendor device suite was rerun.

Deployment backup `20261115-183933-sr-fallback`, PID37560, accepted native prepare
and start. Runtime logs match source2/submission2/native172 and subsequent
source4/native174, source5/native175, source6/native176 to a completed scene,
direct backbuffer ownership and accepted actual Present. Complete settled
capture `scene-20261115-183956-518-26312203-37560-1` has source541,
submission/scope541 and native711 at DLSS Quality1280x720 to1920x1080.
Its record flags4 preserve NO_FG; engine pass flags22 preserve PRIMARY,
AFTER_SIMULATION and NO_FG. Both interface snapshots are present and the
after-interface buffer again exactly equals actual final Present bytes.
Evidence: ignored `native-source-present-game.log` and
`source-native-present-live-comparison.json`, plus the capture's engine_pass.json.

The experiment-owned starting-cell INI was removed by exact-content check. The
bounded PID37560 was closed gracefully after its capture. The final metadata
rebind guard and exception-safe scope retirement were then Release-built and
focused-tested; the live capture predates those two defensive changes. It is
not a game test of their refusal/exception branches. Generic runtime builds also
include concurrent local Vulkan work; no Vulkan behavior was tested here.
Final built DLLs were installed while Skyrim was closed, preserving both previous
DLLs and hashes in `20261115-184323-source-scope-final`. That final deployment
was not relaunched during this checkpoint.

This establishes update/render/scene/window/Present ownership for the sampled
vanilla path. Primary submission begin/end facts, retained normalized FG inputs,
HUD-less GPU copies, compositable UI, screen transitions, provider presentation
and the actual input-to-simulation boundary remain required for FG and latency.

### Cold presentation bridge experiment and SR rollback, 15 November 2026

The shared providers accept BACKBUFFER_HUDLESS with the native final backbuffer
and its pre-interface scene. A separately rasterized UI texture is therefore
not a prerequisite for this provider-supported path. The next experiment used
the existing shared D3D11-to-D3D12 presentation bridge before native swapchain
creation, rather than attempting to replace a chain after scene admission.

D3D11 host ABI2 appends an optional bridge capability and process-lived native
window-selection callback. Only the adapter recognizes the Skyrim window class.
The runtime owns bridge installation, SDK paths and engine metadata, plain
presentation at startup, HUD-less mode, shared FG callbacks and CPU normalization.
Bridge mode initially requests provider0, runtime switching enabled and FG/latency
Off. A bridge-enabled host normalizes CPU facts once through the shared FG sink;
it does not dispatch them again through the upstream normalizer. ABI2 short,
version and missing-window-filter checks precede startup mutations.

Native World entry0x656e60 is now guarded by19 bytes
`48 8b c4 89 50 10 55 56 57 41 54 41 55 41 56 41 57 48 8d`.
Only caller0x656e32 and output0, within an owned RenderEntry scope, publish the
SUBMISSION begin/end pair. The native camera comes from root0x3230740+296,
as in the matched World function. Completed primary calls increment the outer
update's render_expected count. FRAME surrounds the enclosing RenderEntry,
including UI and Present. These are native execution facts, not vendor marker
choices. Current passes still declare NO_FG and unknown screen.

Focused Release runtime/adapter builds and host, Skyrim plugin and input contracts
passed. The first link encountered an active rsf_anomaly_fg_device_test holding
the build DLL; that process was left untouched. Linking succeeded after its
observed exit. No full gate or broad GPU fixture suite was rerun.

First bridge deployment `20261115-185607-sr-fallback`, PID38936, exited during
initial presentation. A distinct bounded exception experiment, PID34964,
captured a null-read access violation in dxgi.dll+0x8e3a below RTSS and Steam
overlay frames. First-chance system exceptions also occurred and are not all
crashes. Evidence is retained in ignored `bridge-first-exception-evidence.json`
and `bridge-first-exit.log`; the probe detached after process exit.

The host was redundantly installing DXGI observation even though its own bridge
already publishes application Present callbacks. Bridge-owned chains now use
those callbacks directly; native chains retain the old DXGI observer. No RTSS
or Steam settings were changed. Deployment `20261115-185923-sr-fallback`,
PID37700, then reached the scene and stayed responsive during a second bounded
probe, with no observed exceptions in its window. That change is consistent
with the duplicate interception being involved, not proof that either external
overlay is defective. Evidence: `bridge-no-observer-evidence.json`.

However, DLSS SR failed with raw backend result-9 and pipeline result-10, then
stopped evaluations after120 consecutive failures. The user reported upscaling
unavailable. Stable presentation is not acceptance of combined SR/FG transfer.
The direct camera-inclusive SR path still bypasses the dense motion/depth
normalizer, while shared FG storage expects R32_FLOAT depth and render-pixel
vectors. This is an input-contract lead, not an isolated runtime diagnosis of
the failed call. Resource creation/copies and actual SDK evaluate need inspection
before the bridge can be admitted. No claim of working generation is made.

The failed DLLs/log were backed up in `20261115-190139-bridge-rollback` and both
installed DLLs restored from the pre-bridge backup. Their SHA256 values are
Runtime `8a485dafcdf55ede5c0bd4e157f00ddb21d7ab379c6be4e750909428da7f02bb`
and Entry `2c2977b1f8a5d2581d568469353c29c28b27fbf04d0308acdf3a0e2c8c5486c0`.
Restored PID10028 is responsive and has successful SR evaluations. Complete
capture `scene-20261115-190209-167-27644843-10028-1` verifies source872,
DLSS Quality1280x720 to1920x1080, result0 and actual final Present. The
experiment-owned Custom INI was removed by exact-content check.

The Skyrim source now leaves presentation_bridge=0 until combined SR transfer
is corrected and verified. Generic ABI2 preparation and the native frame/world
facts remain built work; the running game intentionally uses the restored
working ABI1 pair. This is a failed experiment with a verified recovery, not
FG or latency readiness. Next work must preserve working SR while fixing the
shared input contract, retirement, screen admission and missing input pacing.

### Shared DLSS input normalization correction, 15 November 2026

The next question was why direct SR worked while the same camera-inclusive
inputs failed through shared presentation. Inspection followed the resource
creation and whole-resource copies, rather than changing vendor presets.
The captured direct decode is R32G32_FLOAT (DXGI format16). The existing
motion resolver records an RTX4070 Laptop experiment from4 October where
creating that format as a shared D3D11 texture returned E_INVALIDARG.
The transfer was trying to share the producer format unchanged. Native Skyrim
depth is also R24G8_TYPELESS, incompatible with the transfer's R32_FLOAT depth
destination. These are concrete contract defects; the failed November call's
exact HRESULT was not captured, so this is not a new runtime attribution of
every backend-9 failure.

The shared DLSS route now runs the existing normalization pass even when
camera motion is already included. It emits R16G16_FLOAT previous-minus-current
pixel displacement and R32_FLOAT device depth in the active render rectangle.
The adapter's canonical current-minus-previous UV scales are converted once;
legacy unscaled NDC input follows the same half-range/Y conversion already
used by the alternate-backend adapter. Resolved vectors keep positive inverse
render-dimension DLSS scales, avoiding a second sign reversal.

The resolver receives the actual sentinel-presence flag and current pixel
jitter. Camera-inclusive inputs without a sentinel undergo format/unit
conversion only: zero vectors remain valid, and camera motion is not added.
The existing sparse path still reconstructs only explicitly unwritten samples.
No new option, game switch or SDK ownership change was introduced.

Release runtime, Skyrim adapter and focused test targets built with MSVC.
The motion_resolve WARP check now includes R32G32 canonical motion and native
R24G8_TYPELESS depth, a nonidentity camera matrix and a zero sentinel value
with sentinel detection disabled. Readback verifies zero preservation,
(-4,+4) pixel direction, R16G16/R32 output formats and depth within1e-6.
The existing sparse/layer checks and frame_assembly contract also pass.
No full gate or broad fixture suite was run. The working PID10028 and installed
ABI1 DLL pair were left untouched; this correction is built/synthetic-tested,
not a new game-tested bridge deployment. Combined SR/FG remains disabled until
an isolated shared evaluate and retirement experiment succeeds.

### Input consumers and final-scene metadata, 15 November 2026

The latency question is which rendered source consumes a worker poll, rather
than which update happens to be current when polling runs. A bounded Frida
observation attached to existing PID10028, without replacing the installed
DLLs, invoking input or changing engine/GPU state. The native input manager's
four registered sinks resolved through their actual vtables to ConsoleHotkeyManager,
MenuControls, PlayerControls and BGSMoviePlayer. PlayerControls vtable
RVA0x1935070 slot1 resolves to0x7ad900. Constructor0x7ad450 independently
stores that vtable and singleton0x31a5698.

Matched executable decompilation shows0x7ad900 resetting movement/look vectors
at object+0x24/+0x2c, dispatching handlers through0x7b0130 and filtering look
through0x7aeba0. It returns early for UI pause count+0x160. It copies current
movement/look into previous vectors at+0x34 at instruction0x7ade56. This
establishes a producer and actual consumers, not the consuming source-frame ID.
No event values, keys, text or movement deltas were captured.

A bounded scan of152 singleton references found16 nearby vector-use matches,
not an exhaustive data-flow analysis. The candidate camera update0x8fd590
reads the look vector and calls its virtual HandleLookInput slot+0x78. PE RTTI
and vtable slot3 identify it as ThirdPersonState::Update, also shared by
DragonCameraState and HorseCameraState. This is a concrete camera consumption
site for a future ordering experiment, not a generic simulation-begin hook.
Free-camera candidate0x8f62c0 also reads movement/look; its camera mode and
consuming-frame association still need independent verification.

The five-second job probe armed submit0x60eaf0, join0x60eb80, Poll controls
0x6e6030 and selected callbacks, bounded to32 generations/2400 records. It
observed zero submissions in that window and detached its hooks automatically.
REA then detached the session. Zero records establish neither gameplay order
nor the reason execution was absent. The process remained responsive. Evidence
is retained in ignored `native-input-camera-consumers-20261115.json`, including
REA observation `ev_8b178e0dd8f72d7dfe4d5a49cc51c5001f4f9481ab6a92598a24aed00f6f0b41`,
the exact probe source/digest and decompilations. IDA names/comments were saved
in the existing matching database. Pacing and input-source association remain open.

Source review also found that identify_presentation_inputs claimed to copy
camera metadata but omitted it and screen classification. The adapter now
copies the captured camera, validity, clip transform, render rectangle,
motion convention, pass/resource generation and screen along with identity.
The destination still starts zeroed and receives no borrowed SR texture,
context or frame-context pointers. Native final-scene/window callbacks supply
their own resource leases. This preserves declared facts for later FG admission;
it does not invent a gameplay class or clear NO_FG. Focused MSVC Release adapter
build passed. No fixtures were rerun for these scalar copies, and no deployment
was made into the restored working session.

### Shared SR transfer device regression, 15 November 2026

Before another cold game deployment, the normalization correction was exercised
through the production D3D11 host and its actual shared D3D12 DLSS route in an
isolated process. The explicit `rsf_d3d11_host_shared_sr_device` target is excluded
from default builds and CTest; it requires a scratch runtime root containing
the matched Streamline files. It creates a hidden window, never requests
foreground ownership and leaves FG/latency Off. No installed game files were
changed. The scratch SDK files match the working deployment and are recorded
in ignored `.local/shared-sr-device-20261115/sdk-sha256.json`.

The synthetic pass supplies full-output-size R11G11B10 colour, R16G16 native
motion, R24G8_TYPELESS depth, a640x360 active rectangle and960x540 output.
Motion includes camera motion, has no sentinel and contains valid zeros.
The actual pipeline therefore decodes to R32G32 and must normalize before
sharing, just as in the failed game path. Eight sequential CPU sources and
application Presents exercise the six-slot transfer ring across reuse.

The RTX4070 Laptop run succeeded on all eight evaluates with no refused
frames. D3D11 debug queue was available and reported no error/corruption
messages. Output centre RGB was(0.249390,0.499268,0.749512) for constant input
(0.25,0.5,0.75); the prefilled output marker was overwritten. Runtime status
reported FG requested0/effective0/tagged0. This proves actual shared evaluate
and output return for these inputs, not moving-image quality, generation,
camera sampling order, UI extraction or game/mod compatibility.

The identical executable and SDK root were then paired with the saved failing
Runtime DLL from `20261115-190139-bridge-rollback`. It reproduced backend-9,
pipeline-10 and zero successful evaluations on the first frame. Its SHA256 is
`278a12ec30bb1a24d1348ba7d4cd700463ac64d387a464300446248f00ce87df`.
The corrected positive run's Runtime SHA256 is
`1402d169dd313875ccd298a7e2085b203d1c52a86a18288b2601694ac4acc6df`.
Logs, executable/source hashes and the SDK receipt are retained under ignored
`.local/shared-sr-device-20261115/`. This positive/negative comparison upgrades
the normalization from WARP-only evidence to a reproduced device regression.
It does not retroactively recover the old call's uncaptured HRESULT.

An initial fixture check accidentally queried statically linked internal
presentation helpers, which have different state from the Runtime DLL.
The fixture now queries the public rsf_get_d3d11_present_api table and uses
only that DLL's owner/host predicates. That harness correction is distinct
from the game's transfer defect. Native startup's pre-scene Present is also
represented. The working PID10028 remained responsive and its installed
pre-bridge DLL pair was left unchanged. Skyrim's cold bridge remains disabled
until an actual game run validates the corrected shared route.

Reproduction uses the explicit target, not the routine gate:

```powershell
cmake --build build/windows-x64 --config Release --target rsf_d3d11_host_shared_sr_device
./build/windows-x64/bin/Release/rsf_d3d11_host_shared_sr_device.exe .local/shared-sr-device-20261115
```

## Site discovery instead of a pinned build, 24 November 2026

Question: can the adapter find every SkyrimSE.exe site it hooks, patches, calls or reads without
the whole-file SHA-256 gate and fixed RVAs, so a game update that only moves code keeps working
and a real change still refuses?

Approach: each site gets a byte pattern that is unique in the executable sections, with rel32
branch targets and RIP-relative displacements as `??` wildcards. Admission
(`plugin::admit_observers`, first step of prepare and start) copies the loaded executable's
sections once and resolves every pattern in one pass (`rsf_game_sys::signature`,
`games/skyrim-se/src/discovery.rs`). Immediates and struct displacements stay fixed bytes, so a
pattern also checks the offsets its instruction uses. Before a site is hooked or patched its
pattern is matched again against live memory; another hook or a wrong match refuses. A missing,
ambiguous or changed site refuses admission and the prepare log names it ("Skyrim native sites
refused: site 'jitter' matched more than once."). Detection now recognizes `SkyrimSE.exe` by
name; the file SHA-256 is computed once for the host setup and logs and gates nothing.

How the patterns were found: `tools/derive-site-patterns.py` (capstone, read-only) grew the
shortest unique pattern from each recorded RVA in the installed 1.7.104.0 executable, or for a
global from an instruction whose RIP-relative operand addresses it, trying further references
when a short one was not unique. Long or function-crossing results were replaced by a reference
through a caller (the CPU dispatch hook, whose own prologue is not unique, is found through its
only call and its prologue is checked separately) or by a more specific reference (`UI`, whose
pattern also fixes the pause counter at +0x160).

| Kind | Sites | How found |
| --- | --- | --- |
| Hooked or verified code | 24 entries and call sites (observers, creation call, flush, interface, menus, 11 CPU hooks) | pattern at the site |
| Return addresses | jobs submit/join, world, named poll | pattern at the call; site = call + 5 or + 3 |
| CPU dispatch | 1 | rel32 target of its only call, prologue checked |
| Globals | 27 (renderer owner, frame, window, render state, samplers, UI, singletons, ...) | RIP-relative operand of a using instruction |
| D3D11 creation thunk | 1 | the only `FF 25` through the `D3D11CreateDeviceAndSwapChain` IAT slot from the import directory |
| Named poll and its slot | 2 | the function stored before the pointer to `"Poll controls"` in the named callback table; its thunk must forward the discovered `main` |
| Render-target slots | renderer + 0xa58 | documented offset: no instruction addresses the field directly |

Evidence: `discovery::tests::every_site_resolves_in_the_installed_executable` resolves all 60
sites in the installed executable (found through the Steam library list) and, for the recorded
build (PE timestamp `0x6a8c7046`), compares them with the RVAs recorded before this change: all
match. It also checks every guard pattern at its site and that the unpatched creation call reaches
the discovered thunk. On another build it reports that the sites resolved and skips the RVA
comparison; without an installation it is skipped.

Not changed: the Community Shaders 1.9.1 and SSE Display Tweaks 0.5.25 profiles are exact
third-party module profiles (file SHA-256 and module RVAs). They describe another mod's internals
and still refuse any other version. Struct offsets the code does not address through a global
remain documented constants.

Uncertainty: discovery has not yet run inside the game. Live-memory section copies, admission
timing and the hook installation at discovered addresses need a game run. Patterns were derived
from one build, so their survival across an update is untested until one ships.