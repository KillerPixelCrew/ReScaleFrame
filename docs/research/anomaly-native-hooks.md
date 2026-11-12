# Anomaly standard DX11 native boundary hooks

## Scope and question

Can a DLL carrier enter before graphics creation and observe the shipped renderer,
ordinary UI, physical-PDA copy and scope copy without replacing the executable?
The implementation currently observes these boundaries. It does not yet change
render dimensions, replace shader bytecode, produce motion or dispatch SR/FG.
Recognition of the AVX binary does not authorize the standard binary's RVAs.

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
checks the running filename, file SHA-256, image size and all instruction guards
before preparation. HookSet revalidates the module mapping and every site before
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

## Verification boundaries

- Exact-build detection and foreign-process refusal are native DLL tests.
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
