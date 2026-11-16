# Skyrim Steam Present continuation, 16 November 2026

Question: preserve ENB/Steam/RTSS application pre/post work when Steam registers
another native Present implementation during private DLSS-G chain creation.
This follows [scoped creation](scoped-d3d11-creation-20261116.md) and the
[ENB ownership audit](skyrim-enb-chain-ownership-20261116.md). The worker made
source/static checks only, with no build, game/GPU run, deployment or commit.

## Exact evidence and rejected installer interception

Steam GameOverlayRenderer64.dll SHA256 is
`21c0c1ddb4259c1156356bdfe8432dff504d52c7b96d19718688cff2d5a6e52f`.
Read-only objdump and REA/Ghidra12.1.4 inspect that installed image. The older
static audit is retained at
`.local/skyrim-research/input-acceptance-20261116-005605/steam-forwarding-audit.md`.
The later `input-acceptance-20261116-011617/trace.jsonl` proves helper95dd0
changes singleton1834b8 while the application facade's vtable, Present entry
detour and receiver remain unchanged. Its registration caller is9ca9f, under
sl.dlss_g's native candidate registration during factory creation; the explicit
upgrade branch was skipped because the returned proxy's native interface differed.
The subsequent application path passes the facade through Steam9c24f and
RTSS72ed1 into DXGI8e3a, which reads null. This is a typed-receiver forwarding
failure, not evidence that the facade was freed.

Helper95dd0 uses the x64 ABI: RCX target, RDX handler, R8 output slot, R9D
jump-follow count, fifth stack argument diagnostic label. Its caller9ca76..9ca9a
supplies vtable slot8, handler9c130, output1834b8, count1 and label13abf8.
Helper entry bytes are `48 89 5c 24 08 57 48 83 ec 30`;95de6 clears the supplied
output before lower installer95220. Lower95899 publishes the generated
continuation, then958b5 calls WriteProcessMemory to patch the target and958e9
flushes its instruction cache. The helper's mutex/list work at95e0f..95e6b
occurs afterward; it is not a lock protecting Present's earlier load or the
whole publication. REA IDs are
`ev_9fe3a2adbf99bd21031bdd6e8327dc63712fe21fe48e47379916e3ff3a7c5772`
and `ev_8cfe780b659d15ef1ed1367abebfc5b5a75d5689ebfcd7fc70b12c092604f056`.
The temporary analysis database was closed.

Redirecting the installer's output to a temporary and replacing the singleton
after return would introduce additional publication/reentry questions. No such
hook, global rewrite, saved native continuation or registration suppression was
implemented. The original installer, singleton and native continuation remain
Steam-owned and untouched.

## Two complete load hooks

All addresses below are RVAs in the exact Steam image, preferred base180000000.

| Site | Complete original instruction | Untouched continuation |
| --- | --- | --- |
| TEST9c154 | `48 8b 05 5d 73 0e 00`, MOV RAX,[1834b8] | 9c15b: `ff d0 e9 9b 01 00 00`, original CALL and branch |
| normal9c23e | `48 8b 05 73 72 0e 00`, MOV RAX,[1834b8] | 9c245: `44 8b c6 8b d5 49 8b cf ff d0`, original argument setup and CALL |

Both complete instructions and continuations are checked before creating hooks,
then checked again before queueing enable. Each 21-byte near relay reproduces
only MOV RAX,[stable cell] and uses `FF 25 00 00 00 00` plus an absolute address
to jump to site+7. RAX is the only changed register, exactly as in the original
load; flags and stack are untouched. Steam still executes the original CALL
with its original return address, shadow space and alignment. TEST retains its
own branch; normal Present retains Steam's argument adjustments and post-work.

The two relays and shared pointer cell are initialized on a fresh near page,
checked for rel32 reach from both sites, changed to executable/read-only and
instruction-cache flushed before publication. No direct instruction memcpy
patch or new suspension facility is used. Public MinHook creates the two
disabled hooks, relocates each complete seven-byte original load into its
library-owned trampoline, and applies both queued enables through its existing
Freeze/Unfreeze and instruction-pointer relocation. The original two-byte TEST
CALL is not hooked or reconstructed.

MH_ApplyQueued is not an atomic pair. Any create/guard/queue/apply failure
queues disables for created hooks and attempts rollback, then refuses startup
before admitting a new facade. Backing relays, cell, dispatch state and any
library trampolines remain valid even if rollback itself refuses. A failed
publication attempt requires process restart; the adapter does not silently
retry a partially installed pair.

## Receiver contract and lifetime

The downstream dispatcher calls generic Host ABI7
`rsf_d3d11_host_dispatch_present` with host.user, the exact raw receiver and
incoming interval/flags. RSF_OK/handled1 returns native_result, including a
known-owned-but-unavailable failure; a service error returns E_FAIL without
native fallthrough. Only RSF_OK/handled0 reads Steam's current original slot
through a guarded read and calls it with the unchanged receiver and arguments.
A null/unreadable continuation fails safely. There is no canonical-QI routing,
receiver dereference in the adapter, physical-chain route cache or per-frame
singleton write. The generic core invokes its separate nonvirtual Present body
for an exact registered inner receiver, avoiding reentry through Steam's detour.

The adapter requires no native game device before installation. It checks at
native_creation_prepare and again before the verified cold creation call;
fingerprint, byte, allocation or service refusal short-circuits scope Begin.
Steam absence is allowed and rechecked before cold creation; late installation
after device creation refuses. This is not a late-module-injection framework
or evidence of support for an overlay first loaded during later gameplay.

Steam, the adapter and the generic Runtime are pinned for process lifetime.
Relays/cell and published service/user pointers are immutable and process-lived.
Stop does not unpatch, disable these hooks, remove their trampolines or free the
page. Present can reenter through private native presentation without an adapter
installation lock; exact owned and unowned receivers take separate paths.
Unavailable owned receivers must continue to be recognized after quiescence,
so they cannot accidentally enter native DXGI. Game teardown releases its own
objects under the existing leases; it does not reclaim published forwarding code.

Normal MinHook thread-snapshot and competing-hook-writer limitations remain:
the exact-byte recheck narrows a foreign mutation race but cannot establish
cooperation with another writer changing those instructions concurrently.
Cold game ownership alone was not treated as exclusion of other process threads.
The approved paired MinHook publication replaces the initially considered
unguarded displacement writes; no instruction-fetch tearing gap is knowingly
accepted through a manual patch mechanism.

## Checks and next validation

`.local/skyrim-research/steam-present-relay-check-20261116.json` records offline
assertions against exact Steam bytes: both source loads resolve1834b8, both
continuations match, sample rel32 reach fits, and both synthetic 21-byte relays
resolve their cell and site+7 return. This is not execution or a measured
allocation address. Source review checks the MinHook x64 decoder's RIP-relative
load relocation and seven-byte displaced extent. Scoped whitespace checks pass.

Changed adapter manifest: `games/skyrim-se/src/steam_present.cpp`, its header,
`games/skyrim-se/src/native_creation.cpp`, and the Skyrim CMake source list,
plus this note. CS/CPU/ABI6 creation behavior is preserved; Host ABI7 is shared
core ownership. Coordinator must run one combined gate and bounded actual
ENB/Steam/RTSS SR/FG switch validation, checking both hook publication and
owned/unowned forwarding with normal and TEST Present. No success, latency,
visual quality or complete mod-coexistence acceptance is claimed by this source fix.

## Subsequent ABI8 validation

The complete Steam installer audit expanded this mechanism from two Present
loads to ten loads covering seven typed methods, including teardown Release
and SetFullscreenState. See [close-chain routing](skyrim-close-chain-routing-20261116.md)
for exact bytes, the corrected facade-receiver finding and ABI8 ownership rules.
The coordinator's combined Release gate passed219/15 with fmt/Clippy clean;
scoped creation and plain bridge final-Release checks passed separately.
PID20280 in `.local/skyrim-research/input-acceptance-20261116-020919`
completed bounded SR/provider-switch observation and closed normally, with
ExitProcess0, no observed AV and four hash-verified restorations. This validates
the exercised routing/close path, not every method branch or full coexistence.
That run exposed same-source adapter flags0x6 versus retained core flags22.
The subsequent explicit-provenance repair passed the219/15 production gate and
focused device regression; PID52400/PID47748 resumed tagging (317/316 tags)
and exited normally with four verified restorations each. Generated output
remained inactive. The close-chain note records these receipts and the failed
foreground experiment; generated FG, visual and full coexistence acceptance
remain unestablished.
