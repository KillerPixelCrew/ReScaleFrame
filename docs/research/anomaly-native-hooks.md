# Anomaly standard DX11 native boundary hooks

## Scope and question

Can a DLL carrier enter before graphics creation and observe the shipped renderer,
ordinary UI, physical-PDA copy and scope copy without replacing the executable?
The implementation currently observes these boundaries. It does not yet change
render dimensions, replace shader bytecode, produce motion or dispatch SR/FG.
Recognition of the AVX binary does not authorize the standard binary's RVAs. Since 24 November 2026 no build is admitted by\nits hash or name: every site is discovered (see [Signature discovery](#signature-discovery-24-november-2026)).

## Build identity and discovery

- Executable: `AnomalyDX11.exe`, AMD64.
- SHA-256: `c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e`.
- Preferred image base: `0x140000000`; runtime addresses use the actual module base.
- Image size: `0xfca000`; PE timestamp: `0x66f12204`.
- Read-only source correspondence: xray-monolith `028ccf5f5189b393396480076b11e40852ba3ecd`.
- Evidence: local REA IDA decompilations under `.local/anomaly/rea-evidence/`,
  original-file signature manifest `.local/anomaly/native-sites-dx11.json`.

RTTI complete-object locators and virtual tables narrowed candidates. Decompilation
was checked against actual renderer control flow, copy targets, DXGI calls and
source assertions. A source declaration or vtable index alone was not accepted.
For example, an initially guessed RenderToTarget slot resolved to a detail-model
factory and was rejected; verified slot 60 performs the backbuffer-to-UI/scope copy.

## Active observation sites

| Boundary | RVA | Source correspondence / evidence |
| --- | --- | --- |
| IRenderDeviceRender::Begin | `0x822cb0` | Backend begin and state-manager setup |
| IRenderDeviceRender::End | `0x822eb0` | Backend end and guarded native swapchain Present |
| IRenderDeviceRender::SetCacheXform | `0x823250` | View then projection update of xforms at RVA `0xd63c18` |
| CRender::Render | `0x8977c0` | Main renderer world/combine work; not a proven HUD-less linear-colour seam |
| CHUDManager::RenderUI | `0x0f1e60` | Ordinary gameplay UI and pause text |
| CRender::RenderToTarget | `0x898640` | Target 1 copies physical PDA content; target 2 copies secondary-view content |
| CRender::reset_begin | `0x83a020` | Target destruction/reset preparation |
| CRender::reset_end | `0x83a1c0` | Target reallocation and reset completion |

The exact 16-byte guards are in `games/anomaly/src/native_profile.h`. The adapter
checked the running filename, file SHA-256, image size and all instruction guards
before preparation; discovery now replaces the filename and hash checks (below). HookSet revalidates the module mapping and every site before
creating any detour. A mismatch refuses rather than patching a nearby candidate.

The unary hooks are native `void(this*)` boundaries. SetCacheXform takes this,
borrowed view and projection float matrices; RenderToTarget takes this and a
32-bit target enum. Source virtual methods return void even where IDA infers an
unused integer result from the last native call. No inferred return is consumed.

## Data meaning and lifetime

Device width/height are observed at RVAs `0xd37668`/`0xd3766c`; engine frame is at
`0xd37680`. Matrices are copied during SetCacheXform's borrowed arguments, after
native processing. They retain native storage conventions: this observation is
not the column-vector temporal-history conversion or complete camera ABI.

Counters describe native calls, not input frames or displayed/generated frames.
End may suppress Present for secondary views. A PDA copy is UV-space content,
not an output-space HUD layer. The world renderer can contain embedded/menu UI.
No current observation establishes an SR pass, final source-frame ownership or
FG eligibility. Source view, first-person projection and geometry lifetimes still
need their own producers.

The adapter uses the existing Runtime plugin-session lifecycle. The generic
carrier starts synchronously at first DirectInput8Create, outside DllMain, before
forwarding to System32. Source startup orders InitInput before renderer/device
creation; actual process ordering remains a live acceptance item.

Detours preserve native calls. The adapter is pinned before activation and retains
trampolines for process lifetime; disabling a patch does not prove another thread
has retired from a trampoline. Quiesce disables observation and detours, and refuses
completion while callbacks remain in flight. The carrier never unloads successful
runtime/adapter loads. No executable file or reference engine source is patched.

## Geometry identity, draw-time and context sites

The geometry hooks (`games/anomaly/src/geometry_profile.h`, 16 detours and 5 layout guards) are a separate guarded
set installed after these boundary hooks. Besides the model lifecycle and the six visual `Render` overrides they hook
`CBackend::constants.flush` (the draw-time call site, `0x84b930`), `r_dsgraph_render_hud` (`0x84ffa0`),
`r_dsgraph_render_sorted` (`0x850cc0`), `r_dsgraph_render_emissive` (`0x84ec80`) and `CRender::rmNear`/`rmNormal`
(vtable slots 55 and 57), and read `CRender::phase` (`0xd64f90`) and `Device.m_SecondViewport` (`0xd37b30`) as data.
`CRender::Render` itself stays the observer's hook; the geometry view scope is opened from it. The question, source
reading, evidence and the coverage counters are in `docs/research/anomaly-geometry-coverage.md`.

## Verification boundaries

- Exact-build detection and foreign-process refusal were native DLL tests; since 24 November 2026 detection is site\n  discovery (discovery::tests).
- Native HookSet tests execute an independently authored x64 fixture detour,
  its original trampoline and instruction restoration; malformed guards and
  out-of-image sites refuse without modifying the first site.
- The bootstrap foreign-host contract went RED for the missing entry and GREEN
  after implementation. It does not load a runtime into the test process.
- Full Release `eng/verify.ps1 -VS2026 -Configuration Release` completed successfully,
  including native tests, Cargo formatting and Clippy. The Debug full build failed
  on a Godot/runtime CRT and iterator-debug-level mismatch; focused Anomaly Debug
  contracts passed. This is not a claim that the entire Debug build passed.
- In-game hook activation, camera capture, PDA/scope events and visual stability
  remain pending until backed by process logs/capture. SR and FG remain unimplemented
  by this observation layer.

## Signature discovery, 24 November 2026

**Question.** AGENTS.md Rule 4 requires that the adapter finds what it hooks in the executable that is running.
Until now it admitted only the two files whose SHA-256 it listed and read about 260 addresses from one table per
build. Can one description find every site in both 1.5.3 DX11 builds, so a rebuilt or updated executable is either
found or refused by name, without a hash gate?

**Approach.** The shared scanner (`rsf_game_sys::signature`: wildcard patterns, unique-or-refuse, RIP-relative
decoding), as the Skyrim and RDR2 adapters use it, over the live executable sections. Patterns were derived with
`tools/derive-site-patterns.py`, extended with `--also IMAGE` so a pattern must resolve uniquely to its recorded RVA
in every image given: instruction-by-instruction growth from the site in both builds, bytes that differ between them
wildcarded, growth stopped where an instruction's length differs (SSE against VEX). The recorded long guards
(D3DCompile setup, tessellation prologues and wrappers, GetCBuffer, the binder registration and reflected size,
OnFrame) were masked with capstone and merged the same way; all but two merged to one pattern. Read-only inputs:
`D:\Anomaly 1.5.3\bin\AnomalyDX11.exe` (SHA-256 `c857b6f4...2fb4e`, SizeOfImage `0xfca000`) and
`AnomalyDX11AVX.exe` (`2032acfc...92f12`, `0xfb8000`). No game run, no Ghidra or IDA job.

**How each site is found** (`games/anomaly/src/discovery.rs`, 122 sites):

| Group | Sites | How |
| --- | --- | --- |
| Native observer | 8 detours, `Device.dwWidth/dwHeight/dwFrame` | entry patterns; globals from the `mov` that reads them |
| Engine, shared | CRender vtable, `r_dsgraph_render_hud/sorted/emissive` | vtable from the constructor's `lea`; hud through its only distinctive call, entry checked |
| | `rmNear`, `rmFar`, `rmNormal` | CRender vtable slots 55, 56, 57; common entry `40 53 48 83 EC ?? 48 8B 01` checked |
| Geometry | 11 own detours, 7 layout sites, phase, second viewport, `RCache.xforms`, 4 hierarchy vtables | patterns; the last_lod load is `call; mov r9d,[rbx+108h]` plus 5 |
| | `CSkeletonX_PM::Render` | slot 7 of the vtable its constructor stores (SSE and AVX entries differ in length) |
| | `m_aVertexConstants` | the flush's `lea rbx` (second stage table) minus `0x70` |
| | phase compare, `IsSVPFrame` | must also read the discovered phase and `Device.dwFrame` |
| SR | 14 function sites, 6 calls, `r_dsgraph_render_hud_ui`, GetCBuffer, binder registration, reflected size, 21 globals | patterns; `phase_blur` and `set_viewport` through their call, checked against one encoding per codegen |
| | position binder vtable and `cl_pos_decompress_params::setup` | registration `lea r8,[binder]`, the binder's statically initialised vptr, slot 0; the full body is checked, one encoding per codegen |
| | six first-person pairs | in each function, the first `call set_P` followed within `0x120` bytes by `call [rax+1B8h]` or `call rmNear` |
| | SSAO static viewport and guard | the `cmp [guard]; jg; mov rax,[rbx]; lea r8,[viewport]` pattern |
| Motion | 3 detours, 2 `create_shader` calls, shader_compile prologue, D3DCompile setup, tess dispatch, wrappers | the recorded guards merged across builds are the patterns; `pSourceName` is that setup's `lea r8` |
| | `create_shader<SHS/SDS>` | through their dispatch call; the 66-byte prologue through the object store is checked |
| Input | OnFrame, KeyUpdate, MouseUpdate, CInput vtable | OnFrame by its complete merged body; vtable from the constructor's `lea` |

Every site's expected code is matched at the found address; a detour's first 16 bytes are copied at discovery and
compared again by the hook preflight. A missing, ambiguous or mismatched site refuses its feature only (native,
geometry, SR, motion, input; SR and motion share the SR globals) and the refusal names the site in the adapter log
and the session log. The executable's SHA-256 is logged as evidence; detection recognizes a process whose native
sites are found. Structure offsets no instruction reveals remain documented constants in the profile files; the
shader_compile caller frame (`0x14f0`..`0x1510`, defines `0x250`) is fixed by bytes of the discovered prologue and
setup patterns.

**Failed approaches.** The 22-byte merged binder registration guard matches several registrations once its
displacements are wildcards; the longer derived pattern covering all four registrations is unique.
`CSkeletonX_PM::Render`, `rmNear/rmFar`, the position binder setup and the first-person calls in
`CParticleEffect::Render` and `accum_point` have no common unique pattern (VEX moves change instruction lengths
next to them), hence slots and decoded calls. `phase_blur`'s entries differ from the first instruction, so its check
carries both encodings.

**Evidence.** `discovery::tests::every_site_resolves_in_the_installed_executables` resolves every pattern in both
installed executables with no refusal and checks that the records built from the found sites equal the recorded
ones field by field (sites, expected bytes, globals, first-person pairs, guards, caller frame). The evidence scripts
resolve `discovery.rs` the same way (`tools/anomaly_discovery.py`) and require every recorded value to be the
discovered site; `tools/anomaly_profile_evidence.py` passes 74 checks, including 12 altered patterns and checks in a
copy of `discovery.rs`, each refused with the site's name.

**Uncertainty.** Discovery has not run inside the game yet: the live scan, its start-up cost and the logged
refusals need a game run on each build. The patterns are proven unique only in these two builds; another build
either resolves or refuses by name. The recorded per-build records stay as fixture data (`RECORDED`, feature
`recorded-profiles`).