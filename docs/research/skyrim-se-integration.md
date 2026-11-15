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

`tools/inspect-skyrim-hooks.py` checks the executable SHA-256, table identity,
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

`tools/inspect-skyrim-shaders.py` inventories bounded DXBC containers and OSGN
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
