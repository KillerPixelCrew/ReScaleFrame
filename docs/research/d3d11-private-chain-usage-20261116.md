# Private presentation-chain usage investigation, 16 November 2026

The question is why a successfully admitted virtual D3D11 chain fails when its
first physical D3D12 owner is created. The owned Fallout 4 run
`.local/fallout4-research/native-raster-20261116-093321-487` reached plugin
prepare/start 0, canonical scoped admission and host binding 0, then exited with
`C0000005`. Both private creation attempts returned `887a0001`. These are
observations, not proof of the exception's cause. The coordinator reports the
process exited and deployment restoration hashes matched.

## Exact descriptor distinction

The matched Fallout4.exe 1.11.240.0 SHA256 is
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The retained `creation-disassembly-20261116.txt` shows its original creation
descriptor at `RBP-80`. Instruction `1824ccb` writes `BufferUsage=430`:
shader input, render-target output and unordered access. `1824ced` sets sample
count 1 and quality 0. `1824d27` sets Windowed 1 and SwapEffect 0;
`1824d2f` sets flags 2. The original call at `1824d40` reaches the guarded
creation thunk `22c4cd9`. No descriptor field is changed by the adapter observer.

The generic `Bridge::create_physical(0)` copies the admitted application
description, then changes only swap effect to FLIP_DISCARD, D3D11-side buffer
count to 3, and flags to supported tearing. It calls the retained original
`IDXGIFactory::CreateSwapChain` continuation with the private D3D12 direct queue.
Thus the original usage flags reach that private creation request. This is not
the vendor `CreateSwapChainForHwnd` path. Two logged failures follow from
`ensure_physical` trying the selected Off backend and then its Off fallback.

The observed physical descriptor is 1280x720, format 28, three buffers, effect 4,
flags 800, windowed, with refresh 165/1. The existing passing scoped device
fixture instead requests usage 20. The old failure log omitted usage, sample,
refresh, scanline and scaling fields; the existing bounded failure receipt now
includes those fields. No descriptor translation or presentation policy was
changed in this diagnostic patch.

Microsoft's [DXGI_USAGE documentation](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/dxgi-usage)
defines the unordered-access bit and does not establish that this bit is invalid
for this call. Its explicitly forbidden creation inputs are other bits.
The [D3D12 swap-chain documentation](https://learn.microsoft.com/en-us/windows/win32/direct3d12/swap-chains)
establishes the direct-queue, flip-effect and buffer-count requirements, which
this logged call satisfies. Neither page proves the cause of `887a0001` here.
No repair is justified solely by that HRESULT or by the usage difference.

## Separate application-buffer contract

The bridge supplies application GetBuffer from shared D3D11 textures. The
current shared-surface allocator binds those as shader inputs and render
targets, with no unordered-access bind. This is a separate, concrete mismatch
with a requested application usage containing 400. A private physical-chain
usage change alone would not establish the requested application UAV contract.
It also would not prove that this game actually creates a UAV on that buffer.
Any later repair must keep original application description and application
resource bindings separate from private copy/presentation requirements.

## Focused next check, source frozen

The existing `rsf_d3d11_host_rendered_fg_device --scoped` fixture now requests
usage 430. Before outer-wrapper construction it reads the actual virtual-buffer
bind flags and attempts `CreateUnorderedAccessView`, recording its HRESULT.
It retains that result but continues through the same production first Present,
so one run can expose both the application-buffer mismatch and physical
creation result. Failure of either is a contract failure, not a successful
generation receipt. The normal fixture mode retains its original usage.

An optional system `dxgidebug.dll` query is installed before any creation and
prints at most 16 warning/error messages around initial Present. The fixture
does not substitute a debug factory or change creation API/flags. Queue absence
or zero stored messages is reported honestly and cannot prove absence of a
DXGI error. In that event, the next discriminating observation is the exact
DXGI validation message for the failed production call, rather than another
game run with a guessed flag change.

The coordinator can rebuild the existing explicit target and Runtime, then run:

```powershell
build/windows-x64/bin/Release/rsf_d3d11_host_rendered_fg_device.exe `
  .local/fg-provider-sdk-comparison-20261115-202429 --scoped
```

This source change received a scoped `git diff --check`. The worker performed
no build, device run, game launch or deployment. The preceding integrated gate
was 221 passed and 14 skipped; it predates this diagnostic fixture change.

## Reproduction and minimal repair follow-up

The coordinator rebuilt Runtime and the existing explicit fixture successfully.
`.local/fallout4-research/descriptor-probe-20261116.log` reproduces the two
contracts outside the game and external mods: application usage 430 produces a
virtual buffer with bind flags 28 and `CreateUnorderedAccessView=80070057`;
physical usage 430 at 960x540, sample 1/0, refresh 0/0 produces
`CreateSwapChain=887a0001`, followed by initial Present `80004005`. The optional
DXGI info queue was unavailable. This rules out the game's 165 Hz refresh and
particular HWND as necessary to this reproduction, but is not yet the paired
native-API usage attribution.

The authorized repair keeps D3D11 application usage in the original description
and gives only its private D3D12 copy/presentation chain render-target usage.
It does not change native-D3D12 application creation policy. Shared surface ABI 2
appends `unordered_access`; existing users default it to zero. The presentation
bridge supplies it from the original application's requested usage and the
allocator adds `D3D11_BIND_UNORDERED_ACCESS`. That same description feeds surface
recreation, so a resize does not silently lose the requested UAV binding. No
game name, format-specific usage workaround or vendor policy was introduced.

Consumer audit found the bridge and Unity's native bridge using aggregate
initializers, and SR bridge, native FG and shared-surface tests using zeroed
setups. The bridge sets the new field explicitly; Unity supplies explicit zero;
all consumers use the shared ABI macro. Camera, frame, game and host ABI versions
are unchanged. Allocation/open failures still refuse rather than pretending a
UAV-compatible shared allocation exists. Actual D3D11 shared-UAV creation and
D3D12 import must pass the focused device check.

The scoped fixture now creates a native D3D12 device/direct queue on its selected
adapter before any RSF interception. Two native `CreateSwapChain` calls differ in
usage 430 versus 20, with separate otherwise identical hidden HWNDs to exclude
deferred previous-chain destruction. Both HRESULTs are recorded. A successful
20 control and invalid-call 430 is the required usage attribution; a platform
that accepts 430 is reported without inventing a universal prohibition. The
fixture then retains application usage 430, checks its reported descriptor and
actual GetBuffer UAV, and runs production initial Present before the existing
rendered-source checks. It links D3D12 directly for this native control.

The implementation follow-up above was source-only when frozen. Subsequent
coordinator validation below supersedes its pending build/device checks; it
does not establish Fallout 4 scene or capture acceptance.

## Coordinator validation, 16 November 2026

The Release VS2026 gate in
`.local/fallout4-research/verify-private-usage-20261116.log` exits 0 with
221 tests passed, 14 skipped, and Cargo formatting/Clippy clean. The explicit
fixture was rebuilt separately and run with an absolute scratch SDK root.
The earlier relative-root trial did not load Streamline correctly and is not
the SDK-loading or passing-device receipt.

`.local/fallout4-research/private-usage-fixture-20261116.log` exits 0 on the
NVIDIA GeForce RTX 4070 Laptop GPU and records:

| Check | Actual result |
| --- | --- |
| Direct native D3D12 creation, usage 430 | `887a0001` |
| Paired direct native D3D12 creation, usage 20 | `00000000` |
| Application description | Original usage 430 retained |
| Actual shared GetBuffer allocation | Bind flags A8, including UAV |
| Application CreateUnorderedAccessView | `00000000` |
| Production first Present | `00000000` |
| Existing rendered-source regression | 53 sources, 39 captures, 37 SR evaluations, 33 tags and 33 generated callbacks |

This controlled pair attributes the invalid call to the larger inherited usage
set on this native D3D12 path. The repaired production path also demonstrates
successful shared UAV allocation/import and physical creation/presentation
while retaining application usage. This is stronger than the earlier inference
from a game HRESULT. It does not establish a universal API prohibition on every
platform, nor prove the precise later access-violation instruction in the failed
game trial. The DXGI info queue remained unavailable; no debug-layer message is
claimed.

These are synthetic device results, not physical scanout, visual quality,
measured latency or moving-game FG acceptance. The first old game trial's
21 changed files were restored and verified by the coordinator. A new bounded
game-controller run is pending as of this update; its startup, scene, capture
completion and restoration must be recorded separately when its result exists.

## Startup-route comparison, 16 November 2026

The repaired QASmoke trial
`.local/fallout4-research/native-raster-20261116-095134-037`, PID49032,
still exited `C0000005` after the first overlay draw. Physical-chain creation
refusals were absent and no pre-SR callback was logged. Subsequent adapter
analysis locates the fault at native `bc83c1`, reading a null Bethesda platform
singleton during DATA/ESM path registration. No SR or resource fault is proved
by that observation. The coordinator resumed deferred restoration after
confirming no game remained and verified all 21 changed files.

The opt-in main-menu trial
`.local/fallout4-research/native-raster-20261116-100638-894`, PID60620,
omitted only the controller's starting-cell injection and left the original
custom INI untouched. It completed the 45-second startup observation, reported
1,806 application Presents and foreground 1, exited normally with code 0, and
restored its staged files with the custom INI unchanged. Effective SR remained
0, waiting for inputs. This establishes the observed startup-route distinction;
it is not a native patch, world-scene acceptance or SR evaluation.

The ignored controller now also has opt-in `-InteractiveScene`. It preserves
the same custom INI, completes the same startup readiness checks, and writes
`scene-ready.json` with its owned PID/report. A root-written fresh
`scene-continue.json` must name that PID within 120 seconds. The controller
checks owned liveness and readiness throughout the wait, then performs the
existing world status/capture checks. It generates no menu keys, console input
or focus requests. A missing/stale/mismatched signal refuses capture and takes
the existing orderly-close/restoration path. This handoff has only AST/static
validation here; the coordinator owns its next launch and world observation.
