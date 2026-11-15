# Skyrim Special Edition integration research

Initial investigation: 15 November 2026. Scope: installed executable fingerprint,
current public source review, integration design and validation plan. No game was
launched, injected, patched or modified. No hooks have been verified in this binary.

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
