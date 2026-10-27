# RDR2 stock graphics options: bounded feasibility research

Current decision, 26 October: stock RSF row insertion was explicitly cancelled. RSF owns native
DLSS/FSR/Reflex internally; user controls live in the shared Rust overlay. The native graphics
settings transaction and dimension producers now support quality requests from the ScriptHook
game fiber. See [renderer research](../../docs/research/rdr2-renderer.md) and
[current continuation](RESEARCH-HANDOFF.md). The feasibility findings below are historical.

26 October 2026. Static investigation only, against the modified `main` tree based on
`9699b4d5f1eae006b1c2793fd1778dac2022ff34`. No game, process, installation, options,
cache, renderer source or shared build output was touched.

## Finding

**Stock-menu replacement has a concrete native-code route, but a complete ScriptHook-only
route is not established.** ScriptHook can call databinding and UI lifecycle/event natives;
databinding can insert and remove list items, not just supply scalar values. However, those
operations require an authored list and template/context contract. Neither the inspected
examples nor this build's static evidence establish a script-owned graphics-row collection
or a native that registers a new persistent graphics setting with apply/revert semantics.

The useful exact-build result is a **metadata-driven settings row enumerator with an actual
`DontShow` branch**, and its row-binding helper. This is stronger than a string match and
is a viable place to investigate selective replacement. It is not yet a validated DLSS/FSR
hook: the corresponding live metadata identities, event handlers and ownership remain open.
Prefer a small guarded game adapter plus a ScriptHook script fiber unless those missing
contracts demonstrate that existing script bindings suffice. Do not implement an overlay
and call it stock Settings > Graphics integration.

## Sources and what they demonstrate

All source access was read-only; reference code was not copied into tracked source.

| Primary source | Positive evidence and limit |
| --- | --- |
| [Alexander Blade's ScriptHookRDR2](https://dev-c.enbdev.com/rdr2/scripthookrdr2/) | Exposes script natives to ASI plugins. Does not document arbitrary stock settings schema registration. Existing `src/camera_probe.cpp` already registers a script fiber and resolves native exports. Reuse that lifecycle, without its diagnostics. |
| [alloc8or NativeDB, `e8558ab99c7e990edd7200a348379d6880c14f96`](https://github.com/alloc8or/rdr3-nativedb-data/blob/e8558ab99c7e990edd7200a348379d6880c14f96/natives.json) | Hashes/signatures below. Searching all named natives for settings/preferences finds no general graphics setting registration/apply interface. Unknown hashes and incomplete names prevent treating this absence as proof of impossibility. |
| [femga stock leaderboard example, `477aaaccadb7f0c042286611d9939009636cff12`](https://github.com/femga/rdr3_discoveries/blob/477aaaccadb7f0c042286611d9939009636cff12/databindings/uiapps/leaderboard/leaderboard.lua) | Clears/fills `PostMatchAndLeaderboard.LeaderboardList` with authored `LeaderboardHeader`/item aliases. Proves native list composition, not graphics schema editing. |
| [RedM-NPM bindings, `1ca8e6da61db3c632d9d346d7d1f84da8cddab75`](https://github.com/mikethemadkiwi/RedM-NPM/blob/1ca8e6da61db3c632d9d346d7d1f84da8cddab75/DataBindings.lua) and [app lifecycle](https://github.com/mikethemadkiwi/RedM-NPM/blob/1ca8e6da61db3c632d9d346d7d1f84da8cddab75/npmClient.lua) | Changes existing player-menu `*_visible`/`*_enabled` fields and launches the player app. These are authored player-menu controls, not the stock graphics menu. Its broad close/clear operations are unsuitable for borrowing the game's settings app. |
| [femga UI apps reference at the same revision](https://github.com/femga/rdr3_discoveries/blob/477aaaccadb7f0c042286611d9939009636cff12/databindings/uiapps/README.md) | Names `settings_menu`, including `graphics` and `pause_graphics` entries. Entry names identify navigation, not extensibility. |
| [RedM UI research, `1ebba3e49af4c47f8f26e0323af9ecbc3eeee273`](https://github.com/draobrehtom/redm-uiapps/blob/1ebba3e49af4c47f8f26e0323af9ecbc3eeee273/docs/research.md), [templates](https://github.com/draobrehtom/redm-uiapps/blob/1ebba3e49af4c47f8f26e0323af9ecbc3eeee273/docs/templates.md), [datastore](https://github.com/draobrehtom/redm-uiapps/blob/1ebba3e49af4c47f8f26e0323af9ecbc3eeee273/docs/datastore.md), [flows](https://github.com/draobrehtom/redm-uiapps/blob/1ebba3e49af4c47f8f26e0323af9ecbc3eeee273/docs/flows.md) | Public extraction research identifies the actual settings templates, paths and apply/backout flows below. It reports new custom flows/scenes working in RedM while two stock-file replacement tests were ignored. That is evidence about its RedM streaming tests, not a universal ScriptHook limitation. Its extracted game build is not fingerprinted here. |
| [Cfx parser bridge, `46de79bc1d4dd7c64f16c5cfbadf94612b5ab3d9`](https://github.com/citizenfx/fivem/blob/46de79bc1d4dd7c64f16c5cfbadf94612b5ab3d9/code/components/gta-core-rdr3/src/RageParser.cpp) and [custom text hook](https://github.com/citizenfx/fivem/blob/46de79bc1d4dd7c64f16c5cfbadf94612b5ab3d9/code/components/gta-core-rdr3/src/CustomText.cpp) | Native parser structures are obtainable by hashed type; custom localization uses a game-code hook. These are Cfx implementation techniques, not additional ScriptHook natives or offsets valid automatically in 1491.50. |

Mor3a `bc853d0b4a9e7ba23ee1fe00c2c107092ccd596d` uses its own Insert-key overlay.
It remains useful renderer research and supplies no stock-menu replacement precedent.

### Relevant script native contracts

Signatures retain NativeDB's `Any` where the type is unresolved. A script handle is not
permission to pass a native C++ collection pointer. All calls belong on the ScriptHook fiber.

| Native | Hash | Recorded signature |
| --- | --- | --- |
| `_DATABINDING_GET_DATA_CONTAINER_FROM_PATH` | `0x0C827D175F1292F2` | `Any(const char* path)` |
| `DATABINDING_IS_ENTRY_VALID` | `0x1E7130793AAAAB8D` | `BOOL(Hash entryId)` |
| `_DATABINDING_GET_ITEM_CONTEXT_BY_INDEX` | `0xE96D7F9FEFCC105F` | `Any(Any list, int index)` |
| `_DATABINDING_ADD_UI_ITEM_LIST` | `0xFE74FA57E0CE6824` | `Any(Any parent, const char* name)` |
| `_DATABINDING_INSERT_UI_ITEM_TO_LIST_FROM_CONTEXT_STRING_ALIAS` | `0x5859E970794D92F3` | `void(Any list, int index, const char* alias, Any context)` |
| `_DATABINDING_REMOVE_UI_ITEM_FROM_LIST_BY_INDEX` | `0x6318FB3BE37E11B3` | `void(Hash entryId, int index)` |
| `_DATABINDING_WRITE_DATA_INT` / `_BOOL` / `_STRING` | `0x335C3F6B3766B8D9` / `0xAB888B4B91046770` / `0xE1BD342F2872AEE9` | `void(Any entry, int/BOOL/const char* value)` |
| `IS_UIAPP_ACTIVE_BY_HASH` | `0x25B7A0206BDFAC76` | `BOOL(Hash app)` |
| `LAUNCH_UIAPP_BY_HASH_WITH_ENTRY` | `0xC1BCF31E975B3195` | `int(Hash app, Hash entry)` |
| `EVENTS_UI_PEEK_MESSAGE` / `GET_MESSAGE` | `0x90237103F27F7937` / `0xE24E957294241444` | `BOOL(Hash channel, Any* eventData)` |
| `EVENTS_UI_POP_MESSAGE` | `0x8E8A2369F48EC839` | `void(Hash channel)` |
| `_UIFLOWBLOCK_REQUEST` / `_IS_LOADED` / `_ENTER` / `_RELEASE` | `0xC0081B34E395CE48` / `0x10A93C057B6BD944` / `0x3B7519720C9DCB45` / `0xF320A77DD5F781DF` | `Any(Any hash)` / `Any(Any block)` / `Any(Any block, Any entry)` / `void(Any block)` |

UI events are four 8-byte-aligned slots: event type, integer parameter, hash parameter,
datastore parameter. No Settings > Graphics event channel or apply/cancel event identity
was verified. Do not pop a channel the game owns; that can consume its events.
UI flow natives enter authored resources; they do not define new row schemas or handlers.

## Exact-build static findings

Executable: RDR2 1.0.1491.50, SHA256
`b56c9548f670654a9b73bf25def3cd73af12e269f6e47dba28a34079adaf465e`, timestamp
`0x65eec92e`, SizeOfImage `0x7397600`. Dump: `dumps/rdr2-20261025/RDR2.exe.dump`,
base `0x7ff7b9de0000`, RVA equals file offset. Dump SHA256 was recomputed as
`496fa1bd3cdfb9ba77841f17a3242a554e929bb704da1f6b309e94b39281b7cb`.

Method: narrow ASCII/pointer/RIP-reference searches, then Capstone instruction inspection
and existing `.pdata` unwind ranges. No full-binary decompilation or live read was performed.
Semantic names below are research descriptions, not recovered symbols or callable ABI declarations.
Raw evidence and reproducible helpers are under `.local/rdr2-menu-research/`, especially
`static-evidence.json`, `static-disassembly.txt` and `scan.py`.

| Proven site | Evidence and runtime role | Expected bytes |
| --- | --- | --- |
| `+0x29774` | Registers literal `uiSystemSettingsGroup` at `+0x3325bd0`; stores type ID at `+0x3eb939c`. Initialization, not row insertion. | `48 83 ec 28 48 8d 15 51 c4 2f 03 b9 02 00 00 00` |
| `+0x20989c` and vtable `+0x3326428` | Constructor installs this table; table `+0xb0` points to leaf `+0x24d64c`, which returns the registered type ID. Establishes the native settings-group owner. | Constructor: `40 53 48 83 ec 20 48 8b d9 e8 a6 4a a5 02 48 8d`; getter, complete 12 bytes: `8b 05 4a bd c6 03 89 02 48 8b c2 c3` |
| `+0x22aad4`, same vtable `+0x200` | On `EDX==1`, loads `platform:/data/ui/settings/settingsInit` (`+0x3326e30`). Path LEA `+0x22ac29`, loader call `+0x22ac35 -> +0x25dd50c`. Establishes an authored settings-definition load, not an arbitrary row API. | Entry: `83 fa 01 0f 85 36 02 00 00 48 8b c4 48 89 58 08`; path LEA: `48 8d 15 00 c2 0f 03` |
| `+0x2c90420` | Registers `rage__fwuiGraphicsSettings` parser metadata, descriptor `+0x3cec758`. Field names at `+0x3679858..+0x3679878`: `dlssIndex`, `dlssQuality`, `dlssSharpen`, `fsr2Index`, `fsr2Sharpen`. Persistence schema, not UI row registration. | `48 83 ec 28 48 83 3d 3c c3 05 01 00 75 5d ba 04` |
| `+0x2c7f044` | Recursive settings metadata enumerator. Loads root pointer from global `+0x3ebdee8`, resolves a group, iterates item-pointer array `group+0x30` with count `+0x38`; tests each item's display field and binds admitted rows. Also walks subgroup array `+0x18`, count `+0x20`. Candidate selective visibility/registration boundary. | `48 8b c4 48 89 58 08 48 89 70 18 48 89 78 20 48` |
| `+0x2c7f0ca` within it | With `R13D==0`, compares `[R8+0x1c]`; zero skips the row-binding call. This is a proven visibility gate, not an identified DLSS-specific branch. | `45 39 68 1c 74 0b 49 8b d4 48 8b cf e8 05 36 fe ff` |
| `+0x2c626e0` | Called at `+0x2c7f0d6` and `+0x2c7f12a`. Observed inputs: RCX destination object, RDX binding context/path, R8 metadata. Reads metadata `+8`, converts display enum at `+0x1c` through table `+0x3ce7420`, then calls destination virtual `+0x130` to add a binding. Candidate row-admission hook; semantic argument types/ownership are incomplete. | `48 89 5c 24 10 48 89 7c 24 18 55 48 8d 6c 24 d0` |

The enumerator, binding helper, constructor, loader and parser registrar have matched
`.pdata` entries. The type getter is a 12-byte leaf without a `.pdata` entry, established
through its vtable and complete instructions. Raw E8/E9 matches in intervening data were
not accepted as callers or function boundaries.

Display enum descriptor `+0x3ce7420` links names at `+0x3ce7300` and hash/value pairs at
`+0x3ce7360`: `kSettingDisplay_DontShow=0`, StandardGroup=1, Slider=2, Stepper=3,
Stepper_Raw=4, Dropdown=5, Toggle=6, Button=7, GeneralInfo=8, HudElement=9,
GraphicsPreset=10. `DontShow`'s case-sensitive hash is `0x3bdaba5b`.

The graphics persistence field hashes are case-sensitive: `dlssIndex=0x4de87d5b`
at descriptor `+0x3cea018`, `dlssQuality=0xdb17b778` at `+0x3cea040`,
`fsr2Index=0x58e23159` at `+0x3cea090`. **These identify persisted fields, not proven
metadata item IDs at `item+8`.** Do not equate the two or patch the XML schema to hide rows.

### Labels, options and handlers: established leads versus gaps

Public extracted resources identify `settings_menu` hash `0x72417e17`, stock templates
`settings_list_item` (`0xd74b07e7`), `settings_list_item_raw` (`0x0aa6c076`),
`settings_group_item` (`0x0b18c7f7`) and `settings_graphics_preset` (`0x138217a6`).
The list templates expose `settingsLabel` and `settingsOption`. Public datastore paths include
`ui_apps.settings_menu.currentGroup`, `.currentSelection`, `.graphicsPreset` and memory
usage fields. The documented `.inputMapping.dynamicList` belongs to input mappings;
it is not evidence of a graphics-row list. The public types are inferred from bindings.

Stock flows include `settings_main_flow=0x48f8bc6c`,
`settings_apply_flow=0x9ddd8243`, `settings_backout_flow=0x5cd6e989` and
`settings_restore_default_flow=0xcf38999d`. Their existence does not establish script events.
The current dump also contains conversion resource paths `pause/settings/on_off`,
`pause/settings/setting_level`, `pause/settings/scale_mode`, `pause/settings/api`,
`pause/settings/aspect_ratio` and `settings_menu/value_conversions/reflex` at
`+0x367ce68..+0x367cef8`. These locate the options-conversion mechanism; actual DLSS/FSR
label hashes, choices and bound callbacks are not yet decoded.

Still unproven: exact graphics group and DLSS/FSR metadata IDs; whether its collection is
script-accessible; row-to-persistence accessors; label/option converter ABI; focus/rebuild
ownership; apply/revert/cancel handlers and their call signatures. The executable dump
alone does not contain the referenced heap objects or the complete authored UI resources.
No safe finished replacement detour is claimed. Obtain the matching `settingsInit`, settings
flow/widget/conversion definitions and inspect those missing owners before implementation.

## Recommended implementation

Use ScriptHook for the script-thread lifecycle, supported data/event calls and handoff to
RSF's settings service. Use a narrowly scoped expected-byte hook at the proven native
metadata/binding boundary if graphics rows are code-owned. Suppress native vendor **rows**
only after RSF replacements register successfully; preserve internal temporal input production
and native fallback. Do not zero native DLSS/FSR fields merely to make their rows disappear.
Mutating shared metadata temporarily during recursive enumeration is unsafe without proving
reentrancy and lifetime; prefer a selective admission hook or correctly owned replacement data.

| Control/behavior | Required implementation contract |
| --- | --- |
| SR enabled and backend | Bind actual RSF settings; derive choices from runtime capabilities and installed providers. Existing IDs are DLSS=1, FSR3=3, FSR4=4, XeSS=5. Preserve stored preference separately from effective/refused provider. |
| Quality | Omitted `SRQuality` follows game input ratio. Current override changes vendor mode only. A working quality row requires ownership of the game's render-resolution producer; do not advertise that implementation yet. |
| DLSS preset | Use shared preset IDs and the backend's required drained recreation. Show/enable it only for applicable supported providers. |
| FG, multiplier and latency | Add only with real renderer/presentation/marker implementations and runtime-reported capacity. No FG or latency implementation exists at this checkpoint; a native Reflex schema/converter is not RSF latency integration. |
| Apply, revert, cancel, defaults | Keep pending, committed and effective values. Stock Apply commits a validated transaction; reject/refuse restores display and prior effective configuration. Back/cancel and timeout rollback must include RSF values. Persist through the existing RSF INI owner after successful application, preserving unrelated keys. Do not commit every arrow press or borrow the game's XML for unsupported new fields. |
| Controller and capabilities | Retain authored row templates, stock focus/selection and input routing. Rebuild at the owner's boundary and preserve focus by row identity when capability changes hide options. Do not delete/reinsert focused rows each script tick. |
| Mode changes/teardown | Treat API/output/resize/loading changes as settings/resource transitions. Forced DX12 remains this adapter's scope; unsupported modes must refuse truthfully. Keep borrowed app/collection handles scoped to their generation; never destroy the game's app or clear its event queue. Native calls stay off render workers. |
| Build refusal and interoperability | Fingerprint executable and guard complete instructions plus relevant metadata identity. Unknown builds or failed registration leave native UI functional. Restore only owned hooks/data at safe teardown. RTSS and the existing queue/Present/Social Club chains remain intact. |

The current SR adapter uses DLSS helper `+0x26f3a3c` and corrected FSR thunk/IAT
`+0x2ea0af5/+0x6032a98`, per the handoff's top checkpoint. Older `34a0af5/6632a98`
values are superseded. Menu controls must reach this existing runtime owner, not execute a
second renderer or vendor context from the ScriptHook fiber.

### Next coherent slice and one future validation pass

1. Decode the matching authored settings definitions and connect graphics row IDs to the
   proven enumerator, converters, value accessors and stock commit/backout callbacks.
   This is the concrete prerequisite; do not begin a generic custom-menu framework.
2. Implement one guarded stock graphics replacement group for SR on/off, supported backend
   and applicable DLSS preset, preserving native input selection internally. Wire stock
   pending/apply/cancel/defaults and durable RSF values. Quality remains game-owned until
   its producer is implemented; FG/latency retain their separate implementation dependencies.
3. Complete those producer/FG/latency integrations before exposing their working stock rows.
   Check refusal, partial initialization and teardown together in the same implementation.
4. After preparation/build and authorized deployment with the game closed, use **one
   consolidated future game pass**: open both graphics entry paths; keyboard/controller
   navigation and repeated reopen; every advertised provider/preset and supported quality;
   Apply, Back/Cancel, revert timeout and defaults; capability-dependent focus; INI persistence
   after normal exit/relaunch; warm provider changes and cold model-load timing separately;
   resize/fullscreen/loading/pause and graphics-API refusal; implemented FG/latency transitions;
   RTSS coexistence and native fallback after refusal. Correlate committed/effective values
   with real SR dispatch/input dimensions and presentation/latency evidence, not labels or FPS
   alone. Only test advertised implemented features; no current live experiment was performed.

This task delivered static feasibility and specific admission/visibility sites. It did not
deliver menu replacement, a build, deployment, controller acceptance or game-tested hooks.

## Live coordinator follow-up, 26 October, run AC

After the worker completed and was released, a bounded **read-only** copy of the running
settings metadata resolved the previously unknown row identities. PID126008, image base
`0x7ff62a0b0000`, singleton global `+3ebdee8` points to `0x7ff62dfab0e0`; its root group is
inline at singleton+8. The snapshot has 21 groups and 221 items. Groups have subgroup pointer
array `+18`/uint16 count `+20`, item pointer array `+30`/uint16 count `+38`, and an inline group
item at `+40` whose key is `+48`. These offsets follow the enumerator's actual instructions.

The graphics path is **system `8c463422` -> graphics `bc098ed9`**. Its 29 metadata items include:

| Item | Metadata key `+8` | Display `+1c` | Metadata vtable RVA |
| --- | --- | --- | --- |
| dlssIndex | `4de87d5b` | 3, Stepper | `367c380` |
| dlssQuality | `db17b778` | 0, hidden | `367c380` |
| dlssSharpen | `3420e356` | 2, Slider | `367c600` |
| fsr2Index | `58e23159` | 3, Stepper | `367c4c0` |
| fsr2Sharpen | `0cd9de94` | 2, Slider | `367c600` |

Thus these persistence hashes **also match the live graphics metadata item keys in this
build**; the earlier warning against assuming that equality was appropriate until this check.
The system video group is `66647092` (17 items), advancedGraphics `d662dadd` (49 items).
ASCII names and case-sensitive Jenkins hashes were matched against the dumped executable.

Evidence and reproducible readers: `.local/rdr2-menu-research/live-ac-20261026/settings-tree.json`,
`settings-tree-named.json`, `names.txt`; `.local/rdr2-tools/live-settings-metadata.py` and
`.local/rdr2-menu-research/name-live-settings.py`. The raw objects and selected pointees are
private research evidence, not public pointer contracts. No game calls or writes were used.

The human's screenshot is retained as `live-ac-20261026/greyed-graphics.png`: DLSS, its
sharpening, FSR2, its sharpening, TAA, FXAA and MSAA are all greyed out. No RSF stock-menu
mutation has been applied. Current XML has both `dlssIndex=3` and `fsr2Index=1`, while the
pre-admin backup had `dlssIndex=3/fsr2Index=0`; the earlier probe's selector restoration remains
to reconcile while preserving HDR=false on this non-HDR display. The two native upscalers'
mutual enable conditions are a lead, **not yet a decoded cause**. Their metadata at `+48`
points to two 24-byte condition-like records, with opposite zero/one fields at record+8 and
child pointers at record+10. Actual condition type, evaluation and settings value/apply APIs
still need matching. Do not treat this incidental disabled state as implemented menu replacement.

An attempted interpretation of the child node's pointer/count members as a uint32 hash path
was rejected: several resulting words are halves of ordinary heap pointers. Preserve the raw
condition data, identify its native value/reference types and virtual evaluation methods, and
only then assign semantics. The snapshot reader now labels these as untyped candidates.
