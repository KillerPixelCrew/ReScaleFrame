# Optional D3D11-on-12 loading, 15 November 2026

Question: why does the ENB 0.505 compatibility run fail before creating a game
window, even though the adapter does not require a D3D12 overlay at startup?

The coordinator's owned run of commit `0ecfee5`, PID52892, is recorded under
`.local/skyrim-research/input-acceptance-20261115-234756`. Noninvasive CDB
`startup-stacks.txt` shows the loader's import-resolution worker waiting in
`ntdll!NtRaiseHardError`, through `LdrpReportError` and `LdrpSnapModule`.
`loader-error.txt` records RCX `0xc0000139` (STATUS_ENTRYPOINT_NOT_FOUND), with
the stack string spelling `D3D11On12CreateDevice`. This is a startup import
failure, not an SR input, shader or pacing failure. `exit.json` records the
coordinator's forced stop, no window, and `observation_completed=false`.
`restoration.json` hash-verifies restoration of all four backed-up files;
the coordinator also removed the temporary custom INI.

Read-only PE inspection of existing Release binaries confirms the Runtime
statically imports `D3D11CreateDeviceAndSwapChain` and `D3D11On12CreateDevice`
from `d3d11.dll`. The local ENB archive's WrapperVersion DLL exports the former
but not the latter; the installed Windows System32 DLL exports both. ENB's
installation receipt is
`.local/skyrim-compat-mods-20261115/install-enb-20261115-234532/install.json`:
its D3D11 proxy SHA256 is
`bfb7ff22a962fff9a0faa6f07f5a43b96e4305cc5c9efb0eedcec8d218846677`.
No ENB or game code was copied into this repository.

Source search identifies one production call: `overlay_d3d12.cpp`, in
`rsf_orchestrator_core`. The Runtime links all these object files; static
embedding also spreads this import to other outputs. Existing Release PE
inspection found it in the runtime, shared Loader, AC7 carrier, Godot native,
Minecraft, Unity and Unity.Native binaries. These are pre-change artifacts,
not proof of newly built imports. Following the Runtime's static imports among
adjacent first-party Release DLLs finds no second first-party DLL dependency;
the offending import is directly in the Runtime. Vendor libraries loaded later
and third-party game binaries are outside that bounded static-closure check.

The narrow repair resolves only `D3D11On12CreateDevice`, on first use of the
optional D3D12 overlay. It constructs an absolute path from GetSystemDirectoryW,
loads that system D3D11 module with system-only dependency search, and obtains
the entry point dynamically. It retains one successful module reference for
the process lifetime, keeping cached pointers and returned COM vtables valid
across overlay stop/start. A failed export lookup releases its module reference.
Unavailable loading/export produces a single existing overlay refusal and
does not retry per frame. Resolution runs from the draw path outside DllMain.
No generic device-creation imports, game hooks, wrapper chaining or ENB policy
are changed. The type-only decltype reference does not call the imported symbol.

An existing CPU-only domain fixture now rejects a static On12 import in the
loaded Runtime. This directly checks the startup dependency contract, without
requiring D3D12 devices or an ENB installation. No test/build/GPU/game execution
was performed for this patch; `git diff --check` is the local check. Coordinator
checks still required: `eng/verify.ps1`, the domain fixture, rebuilt PE imports
for Runtime and affected embedded outputs, then the separately owned bounded
ENB rerun. Removing this demonstrated loader failure does not establish ENB
rendering compatibility or visual acceptance.

## Coordinator validation and subsequent startup fault

The coordinator subsequently ran `eng/verify.ps1 -Configuration Release -VS2026`:
`.local/skyrim-research/verify-enb-on12-20261115.log` records exit0, 220 passed,
14 skipped, and clean formatting/Clippy. Rebuilt import evidence at
`.local/skyrim-research/enb-on12-rebuilt-imports-20261115.json` confirms no static
On12 import in Runtime, Loader, AC7, Minecraft and current Unity.Native outputs.
The initial directory scan also found a stale optional Godot artifact dated
13 November and obsolete Unity.dll dated 18 October; neither was rebuilt by
that gate. They must not be included in a claim of validated new imports.

The next owned ENB run, PID26628, under
`.local/skyrim-research/input-acceptance-20261115-235923`, reached prepare/start0
and successful outer facade wrapping, then exited C0000005 before a scene.
Runtime restoration needed transient file-lock cleanup. Follow-up receipts are
under `.local/skyrim-research/enb-startup-fault-20261116-0001`. Direct CDB launch
failed Steam validation T:0000065432. CDB attach attempts produced no useful AV.
An early Frida attempt did not arm before the window; its exact owned process
was stopped. None of these failures localizes the game fault.

The passive post-window trace for PID32932 did arm and persisted exceptions and
termination in `frida-window.jsonl`; `frida-window-loader.log` records successful
outer-return wrapping. REA evidence identifier is
`ev_5a0e77f7b4458119a45a43df42f6e4f4c20a07a35e9f8bf06cd1e2fc93c98e83`,
with clean session detachment. Exceptions3/4 and7/8 traverse DXGI, RTSS+72303,
Steam overlay+9d0ba, then Runtime+9a2f7/+9a743 (or +9a75f)/+97960.
All eight retained exceptions are system C++ exceptions, not an access violation.
NtTerminateProcess later receives C0000005. The original first-eight cap may
have omitted the fatal AV; the trace does not prove that the earlier C++ throws
are the fatal fault or that RTSS, Steam, ENB or RSF caused it.

### Address resolution and ownership review

The existing Release Runtime SHA256 is
`65d1e4fd59e6e7b6a7c807afb66f2d3e05d20cfea8826f67a5e8c7a96d94d16f`,
matching the trial receipt. Exact PDB resolution is unavailable for this image:
the generated Release project has GenerateDebugInformation=false, the PE debug
directory contains only a coffgrp record (no RSDS/PDB identity), and no matching
Release PDB exists. A local Windows DbgHelp attempt fails symbol lookup with
error126; the ignored `resolve_runtime.py` and JSON record that failure. The
available Debug PDB cannot be applied to optimized Release RVAs. The missing
comtypes package therefore is not the reason these addresses lack matching
PDB symbols; installing it would not supply a matching PDB.

Read-only MSVC dumpbin disassembly of this exact image, saved as
`runtime-physical-disasm.txt` and `runtime-getbuffer-disasm.txt` in the diagnostic
directory, supports these source mappings (not PDB-derived line information):

| Return RVA | Operation matched to current source |
| --- | --- |
| 9a2f7 | `Bridge::create_physical(0)`, return from factory CreateSwapChain at vtable offset50h, before HRESULT test |
| 9a743 | `Bridge::ensure_physical`, first create_physical(active_backend) return |
| 9a75f | ensure_physical plain-chain fallback return after setting active backend0 |
| 97960 | `Bridge::GetBuffer`, return from ensure_physical before HRESULT test |

The plain branch creates a flip-discard physical chain on the existing parent
factory and D3D12 queue. The host first validates device/context/chain identity,
publishes the facade, and releases its original native-chain reference. The
original wrapper can retain other references; that lifetime is not established
by this trace. Duplicate HWND chain ownership and wrapper-held stale objects
remain hypotheses requiring the actual fault location and HRESULT/ownership
evidence. Neither warrants an unmeasured change to shared ownership or global
wrapper bypass. No additional production bridge change is made here.

The coordinator's ENB-only baseline PID44944 under
`.local/skyrim-research/enb-only-20261116-0010` survived20s with a Skyrim window
and local ENB plus system D3D11, without RSF. CloseMainWindow was then followed
by C0000005. It separates successful ENB-only startup from the earlier RSF+ENB
startup failure, but does not establish normal shutdown or visual acceptance.
The carrier was hash-restored and no game process remains, per coordinator.

### Next discriminating capture

The existing ignored `frida.js` and `frida-window.js` now retain separate budgets:
eight access violations and eight other exceptions, with independent bucket
sequence metadata. Original first-eight-only sources are retained as
`frida.first8.js` and `frida-window.first8.js`. The same bounded30s observer,
exact-owned-PID guard, exclusive fresh receipt, flush-before-unwind and always
return-false disposition remain. No new hook or tracing mechanism was added.
Node syntax checks and a mocked observer check pass: twelve system exceptions
cannot consume the eight-AV budget, and all callbacks return false. This is
mock validation only, not a new runtime capture.

The coordinator should prefix the updated body with the next exact owned PID
and a fresh JSONL output path, then attach at the same confirmed window boundary.
The discriminating evidence is the first access-violation record's instruction
address, read/write/execute target, registers and stack before termination.
If no AV is observed despite the reserved budget, its absence remains explicit;
termination status alone cannot identify a crash site. A PDB-bearing future
binary would require its own exact hash/RVA capture, not reuse of these RVAs.
No build, game/GPU run, install change or commit was performed for this review.

## Captured AV and GetBuffer failure-output correction

The coordinator's next bounded trace, PID44036, is
`.local/skyrim-research/enb-av-20261116-0017/trace.jsonl`, with `trace.js`,
`loader.log`, `run.json` and `restoration.json` alongside. REA evidence is
`ev_dc9e814fd7e5983f2c106c8df73e7ff24f1bb39e28da30c76daa2d0d57f8775d`,
with clean detachment. This uses the same Runtime hash as PID32932. The separate
budget records exception9, access-violation1, at system D3D11+264a5, reading
`0x530000b000`. System D3D11 is based at `0x7ffac6ed0000`; the identically named
ENB module is based at `0x180000000`. The stack passes system D3D11+5e7a4/+5e722,
ENB+5c19c and Skyrim+10124de, then the main startup callers. The coordinator
restored the prior runtime hash, removed the custom INI and confirmed no game
process remained.

Read-only MSVC disassembly of the matched on-disk game establishes the API pair:

- Skyrim+10124b4 calls swap-chain vtable+48h: GetBuffer, index0, IID at
  +1a74388 (`6f15aaf2-d208-4e89-9ab4-489535d34f9c`, ID3D11Texture2D), with output
  at `[rsp+88h]`. Return address is +10124b7.
- Without an HRESULT test, it reloads that output into RDX, sets R8=null and
  R9 to the render-target table slot, then calls device vtable+48h at +10124da:
  CreateRenderTargetView. Return address is +10124de.
- ENB+5c170 forwards that operation to its underlying device at member+28h,
  through vtable+48h. System D3D11+264a5 dereferences `[rdx]` on the nonnull
  resource before querying its dimension through resource vtable+38h.

Thus the AV is an invalid resource pointer supplied to CreateRenderTargetView,
not an exception in On12 entry resolution. The pointer equals the captured
RDX/RBX, `0x530000b000`. The table output address in R14/RAX/R11 is
SkyrimBase+3331fc8. Static receipts `game-getbuffer-rtv-disasm.txt`,
`enb-rtv-disasm.txt` and `system-resource-disasm.txt` preserve these instructions.
The game SHA256 remains `846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`;
system D3D11 SHA256 is
`9253355cd78fbc6aacb72e91be264d04b145ef06d64cbf496c8de3e5f0e44edb`.
The IDA session inventory was empty; this mapping uses exact-file disassembly,
not invented IDA or PDB symbols.

The generic bridge has a demonstrable failure-output defect independent of the
unmeasured HRESULT: GetBuffer called ensure_physical before validating/clearing
its output. A failure there could return an untouched uninitialized resource
pointer to precisely this unchecked caller. The narrow correction checks the
output and initializes it to null before any lazy physical-chain work, for all
engines and both native graphics paths. It preserves failed HRESULTs, resource
ownership and vendor selection. It does not force success or discard ENB.
Successful resource queries still use the existing paths.

The trace does not record the actual GetBuffer or physical CreateSwapChain
HRESULT, so the cause of physical creation failure remains unproven. The original
native reference is released at successful host handoff; other wrapper-retained
references and HWND association lifetime are still unknown. No ownership rewrite,
delayed original release, special ENB branch or presentation fallback is justified
from this evidence. Clearing a failed output is a failure-path safety repair,
not proof of working ENB presentation or a complete fix for startup failure.

Existing failure diagnostics now log the actual plain physical-creation HRESULT
with facade/factory/queue/window identities and native descriptor. The first four
GetBuffer initialization refusals per facade log their HRESULT, index, full IID,
null output and native12/pending state. The bounded count is atomic. No option,
callback ABI or tracing framework was introduced. Coordinator's next rebuilt run
can correlate these records with the existing outer-native/facade identities
and bounded AV observer. If physical creation still fails, the exact HRESULT
distinguishes invalid HWND/ownership or descriptor refusal from other causes
before any root-cause repair. Native resource lifetime still needs direct evidence.

Local validation is source/disassembly review and `git diff --check` only. No
build, GPU/game run, deployment or commit was performed for this correction.
Coordinator checks are the native verification gate and focused presentation
fixtures, then an owned ENB run collecting the new failure HRESULTs. The prior
220-pass gate predates this bridge change and does not validate it.

## Measured physical refusal and remaining native lifetime fact

The coordinator rebuilt only `rsf_orchestrator` for narrow diagnosis, then ran
PID34956 under `.local/skyrim-research/input-acceptance-20261116-001939`.
This is not a completed native verification gate. The trial Runtime SHA256 is
`57b8400afd0644227f93320014faf6143c7084a9e99e54e6b0e4268e6ec1b75e`.
Its loader log now records two identical physical chain creation refusals:
HRESULT `0x80070005` (E_ACCESSDENIED), facade `0x1f87d6c1940`, factory
`0x1f851373ab0`, native queue `0x1f754c2c440`, HWND `0x4709b0`,
1920x1080, format28, three buffers, effect4 (flip-discard), flags800 (tearing),
native12=0. The following GetBuffer(index0, ID3D11Texture2D) returns E_FAIL
with outputNULL. This directly verifies that the output-clearing correction ran;
it does not establish successful startup. The process still exits C0000005.
Four backed-up files were hash-restored after a transient post-exit mapping lock,
and the coordinator removed the owned custom INI.

The outer native chain was `0x1f75acefd50`, with canonical IUnknown
`0x1f87dcd7f30` and Present at ENB+6c690. The facade was the exact runtime owner,
and native/outer service result0 preceded the refusal. E_ACCESSDENIED proves
the attempted physical chain was refused, but does not alone prove which object
still owns the HWND association. Neither the prior trace nor this log contains
the original chain's final Release result or the lifetime of buffers/views held
by other components. A wrapper's canonical IUnknown differs here; a facade
publication or a single Release call is not evidence that its underlying native
chain and its buffers have been destroyed.

The next minimal generic receipt closes one part of that gap. In the existing
cold handoff, RSF releases its own diagnostic native-IUnknown reference before
its already-required release of the caller-owned native chain reference, then
logs that Release's reported remaining count. It does not consume any external
reference, query a possibly destroyed pointer, or change the creation/Present
path. The existing application descriptor line also includes its original
SwapEffect. Refcounts are observational: a positive count establishes retained
wrapper ownership, while zero does not prove that every underlying backbuffer,
view or HWND association has retired. The adapter's parallel ENB constructor,
QueryInterface and Release audit must supply wrapper-specific interpretation.

No shared ownership replacement is justified yet. Releasing references until
zero, replacing ENB's device creation call, or routing around its Present would
violate ownership and fail to preserve external wrapper work. An earlier
creation admission or another presentation mechanism would need evidence of
the actual wrapper/underlying-chain lifetime before implementation; disabling
SR/FG is not a compatibility solution. No such new mechanism is implemented
by this diagnostic change.

The next owned coordinator receipt should retain the handoff Release count,
original descriptor and the already added physical HRESULT/GetBuffer records
in one run, interpreted against the adapter's static ownership audit. If the
wrapper survives, its audited retained-chain member is the next fact to sample;
if it reports zero, investigate surviving buffer/view ownership rather than
assuming success. Source is frozen after `git diff --check`; this worker ran no
build, game/GPU experiment, install mutation or commit. The coordinator still
owns the integrated native gate and the single next discriminating experiment.

The completed adapter audit subsequently establishes an independent late-wrap
defect: it bypasses the external wrapper's normal Present work. The resulting
generic transaction implementation and its unverified boundaries are recorded
in [scoped D3D11 creation](scoped-d3d11-creation-20261116.md). This supersedes the
earlier pending-mechanism decision; it does not invent a retained backbuffer
reference or identify the outstanding HWND owner from a Release count.
