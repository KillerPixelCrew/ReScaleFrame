# Fallout 4 site discovery

The adapter no longer recognizes one build. `games/fallout4/src/discovery.rs` finds every
Fallout4.exe site it hooks, patches, calls or reads at preparation, with the shared
`rsf_game_sys::signature` scanner, and Rule 4 of AGENTS.md applies: a missing or ambiguous site
refuses and names itself, and the executable's SHA-256 is logged and passed to the host setup as
evidence only.

## How sites are found

- **Patterns (114).** Each is unique in the executable sections, with rel32 branches and RIP
  displacements as wildcards. Hooked functions and evidence instructions are matched at the site;
  call sites whose return address selects a callback are matched from the call and resolve to the
  return address; imagespace is found through its first call, with its own entry checked; 26
  globals are the RIP-relative operand of a unique instruction that uses them, and the two
  constants among them (viewport unit 1.0, jitter scale 0.001) have their value checked. The
  patterns were derived from 1.11.240.0 with `tools/derive-site-patterns.py` and
  `docs/research/evidence/fallout4-sites-1.11.240.spec`; each covers at least the bytes the old
  exact-build guard checked at that address. Four were then chosen by hand: the world graph is
  found by the unique load that also reads its camera at +140, the creation call pattern runs on
  through the renderer field reads after the call, the Effects stride pattern starts 105 bytes
  before its site at an instruction boundary of the same function, and imagespace (whose entry a
  sibling shares) is found through its first call.
- **Names.** The ServingThread (job-list worker), PlayerControls, ImageSpaceEffectTemporalAA and
  PlayerCamera vtables come from their RTTI type names: the type descriptor, the complete object
  locator of offset 0 that names it, and the vtable slot after the pointer to that locator. The
  worker's destructor and entry are its vtable slots 0 and 1 and the TAA active query is TAA slot
  8, each with its entry bytes checked. Three job lists end in a `JobListEnd` job; the worker's is
  found by its own pattern and its descriptor is the only data slot that holds it, whose name slot
  must point at `JobListEnd`.
- **Imports.** PeekMessageA, WaitForSingleObjectEx, XInputGetState (ordinal 2 of
  XINPUT1_3.dll) and D3D11CreateDeviceAndSwapChain are import address table slots read from the
  import directory; the creation thunk is the only `FF 25` jump through its slot.
- **Inside a discovered caller.** Generic code around three calls has no unique pattern, so each is
  the only call of its discovered callee in the first 0x200 bytes of its discovered caller: the
  wrapper call in the JobListEnd job, the `Effects(manager, 0, 13, 1, 1)` call and the HUDless
  viewport selection in imagespace.
- **Menu queries.** The six menu-name queries share one shape; each is the only match whose
  `lea rdx` literal is its name (LoadingMenu, MainMenu, PauseMenu, PipboyMenu, MapMenu,
  VATSMenu), and its name object is the RIP operand it returns.

## Links

The old guards fixed many relations through call and RIP displacements, which the patterns now
wildcard. Each such relation is a link in `LINKS` (73): the displacement at an offset from one site
must address another site, plus a field offset where the code addresses a field (the DR disabled
flag at targets + FE8, the viewport selector at targets + FE4, the native frame at graphics state +
98, the second near-interval word, the delta at its global - 10). Examples: every caller site
calls the hook it selects, the idle and main-poll calls load Main and the input device manager,
the PeekMessageA calls go through the import slot, the outer Present loads the renderer window
cell twice, the camera cache reads load the near and world cameras into the graphics state, the
creation call reaches the thunk and then reads the renderer pointer, TLS index and window cell.
Helper calls inside evidence patterns (locks, matrix helpers) stay wildcards. Links are checked
when sites are discovered and again in live memory, with the vtable and job slots, whenever a
site is verified.

## Verification and refusal

Discovery runs once per process at preparation, on the loaded image. Every site is matched again
in live memory (`discovery::verify`) where the old guards ran: CPU, render, resolution, screen,
depth-domain and render-input guards at prepare and start, creation at preflight and start. This
adapter's features prepare together, so a site that is missing, ambiguous, unlinked or changed
refuses preparation; the log names the site (`Fallout4 native sites refused: site 'X' was not
found.`) and the game keeps running natively.

Structure offsets that no unique code reference reveals stay documented constants in their
modules. Several are fixed by the patterns that use them: renderer device +48, context +50 and
window +58 and the render-context block +B18 by the code after the creation call; the block's
current context +B20 by the prepass and shadow-writer reads; the manager polling flag +35 by the
poll entry; worker mode +68 and completion +60 by the join; the queue sequence +1490 and cursor +8
by the consume and enqueue patterns; the render target fields +FE4, +FE5, +FE8 and the DR slots
+DF4 by the DR and viewport patterns; the UI freeze counters +1EC and +1F0 and the PlayerCamera
state slots +E0 by their queries; the world camera at graph + 140 by the world graph pattern.
The renderer chain +70 and the remaining Main, worker and imagespace manager fields are documented
in `games/fallout4/engine.json`.

`discovery::tests::every_site_resolves_in_the_installed_executable` reads the installed
executable (`RSF_FALLOUT4_EXE` names another copy), resolves every site, checks every pattern,
check and link, and compares each site with the RVA recorded for 1.11.240.0. On 2026-11-24 all 143
sites resolved to the recorded RVAs in the installed Steam build (14 November file,
`fdcef37a...646072f8`). Live-memory discovery, preparation and hook installation have not yet run
inside the game.

## Third-party modules

The Steam overlay forwarding the creation path uses (`integrations/steam`, shared with Skyrim SE)
finds the `gameoverlayrenderer64.dll` code it patches by signature too and logs the overlay's hash
only ([Steam overlay site discovery](steam-overlay-site-discovery.md)); a missing overlay site
refuses the forwarding, and with it Fallout 4 preparation, naming the site. F4SE and ENB are not
matched by this adapter.

## Sites
| Site | Found by | 1.11.240.0 RVA |
| --- | --- | --- |
| Swap | pattern at the site | `0xc33250` |
| keyboard RAWINPUT | pattern at the site | `0x1675140` |
| mouse RAWINPUT | pattern at the site | `0x16754d0` |
| Main idle | pattern at the site | `0xc339d0` |
| input device poll | pattern at the site | `0x1672540` |
| input event consume | pattern at the site | `0x1673ae0` |
| PlayerControls events | pattern at the site | `0xddbe50` |
| worker poll wrapper | pattern at the site | `0xc34eb0` |
| JobListEnd job | pattern at the site | `0xbd6850` |
| worker join | pattern at the site | `0xbd51c0` |
| Swap call | call-site pattern; the site is its return address | `0xc346b1` |
| worker join call | call-site pattern; the site is its return address | `0xc346c0` |
| keyboard call | call-site pattern; the site is its return address | `0xc3a992` |
| mouse call | call-site pattern; the site is its return address | `0xc3a970` |
| idle call | call-site pattern; the site is its return address (+12) | `0xc315c0` |
| main poll call | call-site pattern; the site is its return address (+12) | `0xc342ba` |
| consume call | call-site pattern; the site is its return address | `0xc342de` |
| wrapper poll call | call-site pattern; the site is its return address | `0xc34ec8` |
| worker poll call | call-site pattern; the site is its return address | `0xbd7653` |
| worker consume call | call-site pattern; the site is its return address | `0xc34eec` |
| job callback | call-site pattern; the site is its return address (+13) | `0xbd76df` |
| PlayerControls call | call-site pattern; the site is its return address (+11) | `0x1673b74` |
| PeekMessageA call 1 | call-site pattern; the site is its return address (+0x17) | `0xc338ea` |
| PeekMessageA call 2 | call-site pattern; the site is its return address (+0x17) | `0xc3391e` |
| PeekMessageA call 3 | call-site pattern; the site is its return address (+0x32) | `0xc3398b` |
| PeekMessageA call 4 | call-site pattern; the site is its return address (+0x18) | `0xc339c1` |
| PlayerControls event walk | pattern at the site | `0xddbffb` |
| PlayerControls event kind | pattern at the site | `0xddc44c` |
| event enqueue sequence | pattern at the site | `0x167465c` |
| device enqueue 1 | pattern at the site | `0x16778a2` |
| device enqueue 2 | pattern at the site | `0x16760df` |
| queued event device | pattern at the site | `0x1673d1c` |
| device enqueue 3 | pattern at the site | `0x1677a5f` |
| device enqueue 4 | pattern at the site | `0x1677b02` |
| queued event kind | pattern at the site | `0x1674205` |
| queued event append | pattern at the site | `0x16742a4` |
| gamepad state | pattern at the site | `0x1677bb7` |
| dynamic resolution writer | pattern at the site | `0x1839ed0` |
| depth prepass | pattern at the site | `0x21f38b0` |
| outer Present | pattern at the site | `0x1818550` |
| WorldBegin | pattern at the site | `0x21f0ac0` |
| viewport selector store | pattern at the site | `0x1839ec0` |
| imagespace | the call in its unique caller pattern; entry checked | `0x21fa200` |
| imagespace Effects range | pattern at the site | `0x2185420` |
| outer Present call | call-site pattern; the site is its return address (+12) | `0xc335ec` |
| prepass call | call-site pattern; the site is its return address (+15) | `0x21f9f78` |
| dynamic resolution call | call-site pattern; the site is its return address | `0x21f9f34` |
| ordinary WorldBegin call | call-site pattern; the site is its return address | `0xc382a5` |
| imagespace call 1 | call-site pattern; the site is its return address | `0xc383c7` |
| imagespace call 2 | call-site pattern; the site is its return address | `0xc39d6f` |
| imagespace call 3 | call-site pattern; the site is its return address | `0xc39e13` |
| camera builder call | pattern at the site | `0x182c02d` |
| camera setter call | pattern at the site | `0x182c597` |
| camera update | pattern at the site | `0x21f30b0` |
| camera cache read | pattern at the site | `0x182bf20` |
| camera builder | pattern at the site | `0x182d750` |
| temporal update | pattern at the site | `0x182d580` |
| viewport builder | pattern at the site | `0x1817ef0` |
| camera cache reads | pattern at the site | `0x21f3294` |
| depth camera matrices | pattern at the site | `0x21f3b36` |
| depth build call | pattern at the site | `0x21f3db6` |
| world depth ranges | pattern at the site | `0x21f0e25` |
| camera builder viewport | pattern at the site | `0x182d9d9` |
| camera builder scale | pattern at the site | `0x182da63` |
| camera builder owner | pattern at the site | `0x182db0f` |
| temporal size | pattern at the site | `0x182d647` |
| dynamic resolution slots | pattern at the site | `0x183985b` |
| dynamic resolution selection | pattern at the site | `0x1839bc9` |
| dynamic resolution store | pattern at the site | `0x1839c3c` |
| dynamic resolution index | pattern at the site | `0x1839cb1` |
| dynamic resolution entry | pattern at the site | `0x1839e00` |
| dynamic resolution ratio | pattern at the site | `0x183a278` |
| dynamic resolution count | pattern at the site | `0x183a2d4` |
| dynamic resolution viewport | pattern at the site | `0x183a4a0` |
| shadow viewport writer | pattern at the site | `0x1817f97` |
| render target selection | pattern at the site | `0x1823db7` |
| render target view | pattern at the site | `0x1823f14` |
| render target bind | pattern at the site | `0x182430d` |
| Effects vertex stride | pattern starting before the site (+105) | `0x21856df` |
| renderer initialisation | pattern at the site | `0x18a246` |
| native frame read | pattern at the site | `0x182e226` |
| frame delta read | pattern at the site | `0xc33a60` |
| jitter store | pattern at the site | `0x165ba9f` |
| menu open query | pattern at the site | `0x1a80320` |
| background freeze query | pattern at the site | `0x1a80480` |
| pause freeze query | pattern at the site | `0x1a804a0` |
| camera state query | pattern at the site | `0x102b5c0` |
| creation call | call-site pattern; the site is its return address | `0x1824d45` |
| Main | RIP operand of a unique instruction | `0x32dd130` |
| input device manager | RIP operand of a unique instruction | `0x3e75cb8` |
| input event queue | RIP operand of a unique instruction | `0x3273ae0` |
| PlayerControls | RIP operand of a unique instruction | `0x31f0b80` |
| job list worker | RIP operand of a unique instruction | `0x32a12f0` |
| job list worker object | RIP operand of a unique instruction | `0x32a1320` |
| renderer pointer | RIP operand of a unique instruction | `0x38e13a0` |
| renderer object | RIP operand of a unique instruction | `0x3a25d10` |
| immediate render context | RIP operand of a unique instruction | `0x38e1398` |
| renderer window | RIP operand of a unique instruction | `0x38e13b0` |
| Present renderer | RIP operand of a unique instruction | `0x3a25d00` |
| render context TLS index | RIP operand of a unique instruction | `0x3e73018` |
| graphics state | RIP operand of a unique instruction | `0x3d871a0` |
| render targets | RIP operand of a unique instruction | `0x2f4d750` |
| world scene graph | RIP operand of a unique instruction | `0x32dd338` |
| frame delta | RIP operand of a unique instruction | `0x339f430` |
| imagespace manager | RIP operand of a unique instruction | `0x3e5e4b0` |
| viewport unit | RIP operand of a unique instruction; value checked | `0x2929458` |
| jitter scale | RIP operand of a unique instruction; value checked | `0x29293f8` |
| depth range mode | RIP operand of a unique instruction | `0x3e62250` |
| near depth interval | RIP operand of a unique instruction | `0x3e62190` |
| world depth interval | RIP operand of a unique instruction | `0x2f97b30` |
| near camera | RIP operand of a unique instruction | `0x3e622d0` |
| world camera | RIP operand of a unique instruction | `0x3e62210` |
| UI | RIP operand of a unique instruction | `0x30e8930` |
| PlayerCamera | RIP operand of a unique instruction | `0x30e6e58` |
| JobListEnd name | pointer at descriptor + 8; "JobListEnd" checked | `0x255dd10` |
| JobListEnd descriptor | the only data slot holding the job | `0x2f30fc0` |
| job list worker vtable | RTTI `.?AVServingThread@JobListManager@@` | `0x255e3b8` |
| job list worker destructor | vtable slot 0; entry checked | `0xbd72e0` |
| job list worker entry | vtable slot 1; entry checked | `0xbd7430` |
| PlayerControls vtable | RTTI `.?AVPlayerControls@@` | `0x2571a30` |
| TAA vtable | RTTI `.?AVImageSpaceEffectTemporalAA@@` | `0x2915eb0` |
| TAA active query | vtable slot 8; body checked | `0x2214fc0` |
| PlayerCamera vtable | RTTI `.?AVPlayerCamera@@` | `0x25aa6d8` |
| PeekMessageA import | import directory, USER32.dll | `0x2440a98` |
| WaitForSingleObjectEx import | import directory, KERNEL32.dll | `0x2440318` |
| XInputGetState import | import directory, XINPUT1_3.dll ordinal 2 | `0x2440f10` |
| D3D11CreateDeviceAndSwapChain import | import directory, d3d11.dll | `0x2441758` |
| creation thunk | the only `FF 25` through the import slot | `0x22c4cd9` |
| wrapper call | the only call of the wrapper in the JobListEnd job | `0xbd6945` |
| Effects call | the only `Effects(0, 13, 1, 1)` call in imagespace | `0x21fa288` |
| HUDless viewport call | the only viewport selector call in imagespace | `0x21fa2ca` |
| LoadingMenu query | menu-name query shape whose literal is "LoadingMenu" | `0x1a87520` |
| MainMenu query | menu-name query shape whose literal is "MainMenu" | `0x1a879a0` |
| PauseMenu query | menu-name query shape whose literal is "PauseMenu" | `0x1a87e20` |
| PipboyMenu query | menu-name query shape whose literal is "PipboyMenu" | `0x1a87fd0` |
| MapMenu query | menu-name query shape whose literal is "MapMenu" | `0x1a87a30` |
| VATSMenu query | menu-name query shape whose literal is "VATSMenu" | `0x1a890b0` |
| LoadingMenu name | RIP operand of the query's returned name | `0x3dbd830` |
| MainMenu name | RIP operand of the query's returned name | `0x3dbd8b0` |
| PauseMenu name | RIP operand of the query's returned name | `0x3dbd930` |
| PipboyMenu name | RIP operand of the query's returned name | `0x3dbd960` |
| MapMenu name | RIP operand of the query's returned name | `0x3dbd8d0` |
| VATSMenu name | RIP operand of the query's returned name | `0x3dbdb40` |
