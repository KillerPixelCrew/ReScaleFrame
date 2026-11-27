# Display Tweaks0.5.25 creation ownership

Question: why did the first SKSE/Display Tweaks coexistence startup prepare
successfully but refuse RSF start, and which creation boundary preserves both
mods? This audit uses the installed shipped binary and matching Address Library;
the old `references/skyrim-display-tweaks` checkout is0.5.12 and was used only
as a search lead. No reference code was copied.

Since 2026-11-24 the module RVAs below are those of the researched 0.5.25 build only: the wrapper,
its call of the retained thunk and the retained slot are discovered by signature in the loaded
module (`games/skyrim-se/src/discovery.rs`) and the file SHA-256 is logged, not compared.
The game is SkyrimSE1.7.104, SHA256
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`.

## Installation and observed refusal

The coordinator installed user-supplied SKSE2.3.1 from archive SHA256
`7bad616ed360823a027f8828801d91e3a4aa2eacd952023af7f3e31adb2ae250`.
The receipt `install-skse-20261116-073858/install.json` under
`.local/skyrim-compat-mods-20261115` records64 verified files and valid Ian
Patterson signatures for both native executables. Display Tweaks0.5.25 was
obtained from `https://www.playbuff.net/api/mods/sse-258174/download`;
its DLL SHA256 is
`df7f352a3f13736709c5f9d265ebf311d9d77d373a9a1b8cbe2aee7fdd8ba9f4`.
Its packaged configuration remains unchanged.

The matching format5 Address Library has header1.7.104.0 and SHA256
`8aab3dd251d135b849bd983f86a4a205c920fa3e81f8e30c0e63ccfef9423842`.
The coordinator verified its Git LFS content against the public Enderal repo
commit `c9ea2a850dd2bac1b90f02a32a00929f6e6a62d5`, path
`SKSE/Plugins/versionlib-1-7-104-0.bin`. The installation receipt is
`install-display-library-20261116-074007/install.json`. The supplied ENB archive
SHA256 `93fe8efc6ac354f0509d8f08dfe4547b8253d67235536ee757fc18d5600ba72d`
matches the already installed archive and native binaries; four existing
configuration differences were preserved. These are installation facts, not
rendering acceptance.

PID29080 (`startup-20261116-074039`) loaded SKSE and Display Tweaks successfully.
Only the final `rsf-loader.log` section beginning `Skyrim DPI startup` belongs
to this run: prepare0, then start4294967291, signed -5/native refusal. Earlier
SR/FG lines in this appended log are from older runs. Display's log confirms
its Present/ResizeBuffers hooks, pre-Present limiter, window and D3D setup.
`exit.json` records CloseMainWindow requested and process exited, with null exit
code. This is not evidence of exit0 or successful RSF rendering.

## Exact creation route

Ghidra analysis of the matched DLL resolves these module-relative sites:

| Site | Finding |
| --- | --- |
| 25c0 | Resolves Address Library ID77396 plus2c0 for the game D3D creation call |
| 25dc0 | Reads that E8 target into retained slot5e168, then installs call5 to27250 |
| 3a960 | Emits a14-byte `ff 25 00 00 00 00` relay with an absolute destination, and writes only the game's five-byte E8 call |
| 27250 | Full twelve-argument D3D11CreateDeviceAndSwapChain wrapper; descriptor/pre-events precede the retained native call, post-events follow |
| 272e9 | `4c 8b 15 78 6e 03 00`, loading the retained original from5e168 into R10 |
| 27331 | `41 ff d2`, CALL R10; exact return27334, followed by `8b d8` |

Format5 has a96-byte header followed by indexed32-bit RVAs. ID77396 resolves
game1012060, so the patched call is exactly1012320. Its original bytes
`e8 ed 70 5a 00` target game15b9412, the existing IAT thunk. The matched wrapper
copies all eight stack arguments and four register arguments to this call and
returns the same native HRESULT. It does not replace the resulting chain.

This proves two incompatibilities in the prior adapter contract: Display's
installed E8 displacement fails the fixed native-caller guard, and the thunk's
actual caller becomes DLL27334 instead of game1012325. Relaxing just the first
guard would still miss scoped creation. **It does not prove which start stage
first returned -5 in PID29080**: prepare/start have no per-stage receipt in that
run, and their ordering relative to mod patch publication was not captured.

## Small adapter repair

The adapter retains its existing thunk hook and direct native path. The alternate
route requires the exact Display file fingerprint, a pinned module, readable
executable module ranges, the game's E8 to the exact14-byte relay, destination
DLL27250, exact wrapper prologue and77 invocation/return bytes, and retained
slot5e168 equal to game15b9412. The only admitted alternate return is DLL27334.
The route is checked during prepare, immediately before enable, and at creation,
so an exact startup patch published after prepare can still be recognized.
Unknown fingerprints, relays, retained targets or callsites refuse admission
with a specific reason. No Display or SKSE code/global/settings are modified.

The synchronous Begin/End remains at the unchanged game thunk. Display's
descriptor and pre-event work run first; ENB construction runs inside the
scope; admission completes before Display's post-events. There is one scope,
and the existing game owner, HWND, chain/device/context output addresses and
returned device identities remain mandatory. Original arguments, current IAT,
native HRESULT and LastError are preserved. No SKSE imports, runtime dependency,
shared game policy, extra native reference release or settings workaround was
introduced. No claim is made for different Display builds or arbitrary added
post-create callbacks.

## Other start guards and checks

The matched resolver inventory contains198 constant address expressions plus
one version-dependent expression, separately resolved for1.7.104. The patch
writer callers were inspected to distinguish function pointers from writes.
The relevant comparison covers every game start byte/pointer guard:

| RSF guard group | Exact game RVAs | Matched Display result |
| --- | --- | --- |
| Main render/call/reset | 101e360,14edbf0,153ade7,1544df4,101e130 | No intersecting patch found |
| End/viewport/upload/sampler/HUD-less input | 1009f70,1009960,100ae80,1010de0,116ac80 | Present patch100a00f and resize patch1009624 lie outside guarded entries; nearby window/target patches also do not intersect |
| CPU tick/poll/render/dispatch/world | 658870,cf95f0,6565b0,cfa650,656e60 | No intersecting patch found; Display event/main-loop patches are at different callsites |
| Controls/camera/jobs/named poll/movement/player | 7ad900,8fd590,60eaf0,60eb80,6e6030,776290,745200,558e70 | Controls patches at7add1e/7add3d are interior, outside the19-byte controls entry; other control patches are separate functions |
| Screen queries/named callback | 1169530,1169590,20b5130 | Query1169530 resolves as an unused function pointer, not a patch destination; no named callback write found |
| Creation | 15b9412,1012320 | Only proven guarded overlap is the five-byte game call at1012320; thunk retained unchanged |

Steam's separately guarded load sites belong to the Steam module, not Display's
game patch destinations. Compatibility's CS initializer/HDR refusal and the
runtime's generic factory hooks are unchanged. This is a static intersection
audit of the matched installation, not proof that another writer cannot race
startup or that every behavioral interaction is compatible. Narrow render,
CPU, creation and MinHook-stage refusal logs now identify any remaining first
failure without weakening guards or changing admission policy.

Ignored receipts are `display-static-audit-20261116.json`,
`display-resolved-targets-20261116.json` and
`display-patch-writers-20261116.json` under the compatibility directory.
`check-display-route-20261116.py` passed exact-file byte comparison, single-byte
mutation rejection, Address Library site and synthetic call/relay/return
geometry checks. These checks execute no target code. Subsequent build and game
evidence follows; the earlier refusal remains a separate historical result.

## Bounded combined-mod validation and deployment

The coherent Windows Release VS2026 gate exited0:220 passed,14 skipped,
Cargo fmt/Clippy clean, recorded in
`.local/skyrim-research/verify-display-coexistence-20261116.log`.
The tested source is baseline `909e17e8b66761c1373a9e0d47fd49d3781f19a4`
plus the uncommitted working trial, not a committed release.

The coordinator ran `-SKSE -Timeline -SceneWaitSeconds45`, PID60008,
`.local/skyrim-research/input-acceptance-20261116-080226`. Prepare/start both
returned0; the exact Display route matched and scoped-end returned native
HRESULT0/admission0 with verified canonical outer/inner ownership. Retained
module paths include SKSE, Display, ENB, Steam and RTSS alongside the system
graphics modules. All three copied SKSE/Display logs are fresh since launch.

The final public status in `display-coexistence-game-20261116.log` reports
DLSS SR effective1,1280x720 to1920x1080; DLSS FG requested/effective/active/
generated1; latency requested/effective1; renderer/FG/SR/vendor results0;
outputFPSvalid1 with base116/output232. At source4196 it records693 tags,
4368 application Presents and1389 output Presents, reasonActive. Matching late
prepare rows retain flags6, continuous1 and valid1, with submitted,
render_complete, paced and prospective1, failed0 and cpu_mask15. Marker-order
violations are0. These are bounded runtime/vendor generation telemetry results,
not physical scanout or measured latency.

Runtime foreground1 is a point sample only. The controller Foreground switch
was false, and this run is not a controlled focus or Display comparison. Do not
attribute the earlier inactive generation conclusively to either factor.
CloseMainWindow completed and no Skyrim process remained; `exit.json` records
observation_completed=true but exit_code=null, so exit0 is not established.
All four temporary replacements passed restoration hash checks. Pickup visual
quality, physical scanout/latency and full coexistence remain unmeasured.

After that restoration, the coordinator permanently deployed the tested Entry
SHA256 `0a1129a5c0fd610d7f6e9200e518dfb586f376e085ff292a0bbab7c7a3b74fc3`,
retaining the prior Entry under
`.local/skyrim-compat-mods-20261115/deploy-display-repair-20261116-080404`.
Its `deploy.json` verifies Runtime and INI unchanged; Runtime SHA256 is
`1b7ce92a78b863ad7d37112585d7ef9e0118e851809884ea8fd4eb242820c15b`.
This deployment does not broaden the acceptance scope above.
