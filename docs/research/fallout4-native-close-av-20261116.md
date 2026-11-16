# Fallout 4 native close access violation, 16 November 2026

Question: which native owner and call path produced the loaded-world close
failure, and what can this establish before a controlled close without RSF?
This is read-only instruction, PE RTTI and supplied observer evidence. No
patch, build, game launch or shared analysis session was used by this worker.

The installed Fallout4.exe is 1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
All addresses below are RVAs. The observed ASLR base was `7ff6ce730000`.
The preferred PE base used for static disassembly is `140000000`.

## Observed failure

Coordinator receipt
`.local/fallout4-research/native-raster-20261116-110223-800/close-trace.jsonl`
records PID27344, exception thread54912, an access violation at `4cb273`
reading address `50`, with RAX=0 and RCX=0. The observer returned false to
exception handling. Its accurate unwind and subsequent NtTerminateProcess
status `c0000005` establish an unhandled access violation, not a requested
clean process exit. The coordinator reports all20 experiment files restored.

At the fault, `4cb273: 48 8b 78 50` is `mov rdi,[rax+50]`. The immediately
preceding `4cb26c: 48 8b 05 9d 00 c2 02` loads RAX from global `30eb310`.
Thus this instruction dereferences a null global owner, rather than the
method's current RCX. The enclosing function is `4cb1c0..4cb2d3`; its entry
copies the cell receiver to RSI. Observed RSI was `2680b6aa690`.

The branch reaches this fallback only after earlier candidates fail and cell
flag+40 bit0 is clear. It checks the returned field pointer after loading it,
but does not check the owner before reading owner+50. Do not identify the
field's semantic type or this method's name from that offset alone.

## Native owner and destruction path

The constructor at `da67d0` installs vtable `256e618` and publishes its receiver
to global `30eb310` at `da6805`. Its PE RTTI complete object locator `29d7748`
names type `PlayerRegionState` through descriptor `3009bf0`. The constructor
initializes owner+48 and owner+50 to null. These native facts identify the
global without requiring an address-library name or F4SE at runtime.

The top-level shutdown routine `c31700` sets R13=0 at `c317b9`. At `c319f2`
it loads `30eb310`, optionally invokes the object's slot0 destructor with
EDX=0 at `c31a03`, then clears the global at
`c31a05: 4c 89 2d 04 99 4b 02`. Later in that same routine it loads global
`32dd158` at `c31aaf` and calls slot0 with EDX=1 at `c31ac3`.
The observed unwind proves that latter call entered the TES destruction path.
This is a concrete native ordering hazard: cell teardown can request a
PlayerRegionState fallback after shutdown has cleared its global.

| Observed return RVA | Exact preceding native call and evidence |
| --- | --- |
| `d4d34d` | `d4d348` calls `4cb1c0`, passing nonnull RDX as the cell receiver; enclosing routine `d4d330` compares the result with its own field+18 |
| `4c235c` | `4c2357` calls `d4d330`, passing the dying cell in RDX and native object `31edc70` in RCX |
| `4e6254` | `4e624f` calls destructor body `4c22c0`; deleting wrapper `4e6240` conditionally frees an allocation of size `f0` |
| `5735d9` | `5735d7` calls the cell's slot0 with EDX=1 after removing it from worldspace bookkeeping |
| `2e4cf6` | `2e4cf1` calls `573520`, with RCX from cell+`c8` and RDX the cell |
| `2d9e59` | `2d9e54` calls `2e4aa0` for a cell being removed from an array |
| `2cd0ef` | `2cd0ea` calls `2d9db0` with EDX=0/R8D=0 inside TES destructor `2ccfe0` |
| `2dd594` | `2dd58f` calls `2ccfe0`; deleting wrapper `2dd580` conditionally frees size `138` |
| `c31ac5` | `c31ac3` calls global `32dd158` object's slot0 with EDX=1 |
| `c2f935` | `c2f930` calls shutdown `c31700` after the native main function returns |

TES destructor `2ccfe0` installs vtable `2473b58`, whose RTTI locator
`293ee60` names `TES` through descriptor `2fa2c90`. Cell destructor `4c22c0`
installs vtable `24a34d8`, whose RTTI locator `2957a08` names `TESObjectCELL`
through descriptor `2f9b548`. Its secondary vtable `24a3730` agrees. This
corroborates the actual owner chain independently of inferred function names.
The remaining reported outer return `22bf5aa` is retained in the observer
receipt; assigning it a CRT symbol is unnecessary for this owner finding.

Pinned CommonLibF4's TESObjectCELL layout corroborates the cell+40 flags,
cell+48 extra data and cell+`c8` worldspace union; its TES size is `138`.
Names for the function at `4cb1c0`, the native object at `31edc70`, and the
PlayerRegionState field+50 remain unverified. PE exception-directory entries
can split one destructor into chained fragments, so a fragment start is not
automatically a separate function entry.

## Causal limit and next comparison

This identifies the immediate null owner and its native teardown sequence.
It does not exonerate RSF, F4SE, ENB, the save, or earlier state changes:
an all-native fault stack cannot identify what caused a cell to survive until
this fallback or altered its state before destruction. No workaround or
null-check patch is justified from this run alone. Generic committed change
`e4d3f00` was not modified.

The coordinator's controlled comparison uses the same F4SE/ENB/save world
and observer with RSF absent. Compare fault RVA, owner load, exception code,
shutdown path and restoration receipts. A matching failure establishes
reproduction without RSF in that setup; a clean baseline requires further
isolation and does not by itself assign the integration defect.

The coordinator completed that comparison in
`.local/fallout4-research/baseline-world-20261116-110927-046`, PID55372.
`modules-world.json` contains no RSF modules, and the coordinator reports no
RSF artifacts staged. The same Continue/save/world was observed before the
passive observer was armed and CloseMainWindow requested. `exit.json` records
actual exit `-1073741819` (`c0000005`), same_world_observed=true,
rsf_modules_absent=true and no_forced_kill=true. The baseline observer records
the same `4cb273` read of address50 with RAX=0/RCX=0 and the complete matching
native unwind through `d4d34d`, `4c235c`, `4e6254`, `5735d9`, `2e4cf6`,
`2d9e59`, `2cd0ef`, `2dd594`, `c31ac5`, `c2f935` and `22bf5aa`.

This reproduces the shutdown failure without RSF present in the tested
F4SE/ENB/save setup. It is evidence against treating this specific failure as
an RSF-only regression and gives no reason to patch the generic bridge for
it. It does not isolate native game behavior from F4SE, ENB or save state,
nor establish how the player's native Quit path behaves: both matched close
trials used CloseMainWindow.

## Opt-in native Quit handoff

The existing ignored `run-native-raster-validation.ps1` now accepts
`-NativeQuitHandoff`. In its cleanup path, a still-live retained game process
must match the executable path, launch-time ownership and exact executable
hash before it writes `close-ready.json` with PID, report path and a fixed
120-second budget. The controller then only observes that retained process
for actual exit, allowing the coordinator to use Escape and visible native
Quit/confirmation. It sends no input or close request in this branch.
Actual exit follows the existing exit-code receipt and restoration path.
Expiry throws a cleanup error with restoration deferred and leaves the game
alive. The default CloseMainWindow branch is unchanged.

Only AST parsing and scalar/source checks cover this controller change; no
worker game run, native Quit acceptance or build is claimed.

Untracked static receipts: `close-owner-native.txt`, `close-global-native.txt`,
`cpu2-c31700.txt`, `cpu2-c319c0.txt` and `cpu2-da67d0.txt` under
`.local/fallout4-research/`. No production hooks or capabilities changed.
