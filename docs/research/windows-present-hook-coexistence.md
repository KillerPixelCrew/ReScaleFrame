# Hooking Present on Windows next to Steam and RivaTuner

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

Measured 26 September 2026 on Windows 11 Pro 26200, the first day this project ran in the game on
Windows rather than under Proton. Every claim below is from the proxy's own log, its crash
reporter, or a scratch probe linked against the built `rsf_graphics` library; none is from reading
the other programs' code.

## The question

The observer patched `IDXGISwapChain::Present` in the swap chain's vtable. On the first Windows
launch the game died before showing a frame, with nothing in the log after the overlay came up.
Windows Error Reporting is disabled on this machine, so there was no dump and no event. The
proxy gained a crash reporter (a vectored exception handler that writes the faulting module and
offset, a symbolised walk of the faulting thread through dbghelp, and a minidump beside the log,
from a thread of its own because a stack overflow leaves the faulting thread nothing to run on).

## What the crash reporter showed

Exception `0xC00000FD`, a stack overflow, 64 frames of two functions alternating:
`DINPUT8.dll!hooked_present+0x1e8` and `gameoverlayrenderer64.dll!OverlayHookD3D3+0x13e8f`. Each
was forwarding to the other. Steam's overlay hooks Present in the same table, notices when its
entry is displaced, re-hooks, and records whatever it displaced as its original. Our vtable hook
had already recorded Steam's hook as its original. Every present recursed until the stack ran out.

## What the probes showed about the entry

With the observer forwarding to the genuine function instead, resolved from `dxgi.dll` on disk,
the game presented nothing: the re-entry guard was refusing every inner call, so the genuine entry
was itself patched to jump back into Steam. Probes in a clean process, no Steam, then showed:

| Step | Bytes at the entry of dxgi's Present |
| --- | --- |
| file on disk | `48 89 5c 24 10 48 89 74 24 18 55 57 41 56 48 8d` |
| memory, before any hook of ours | `e9 cb 6a ff bf 48 89 74 24 18 55 57 41 56 48 8d` |
| after our detour | `e9 99 7a fc ff ...` |
| after one present | `e9 cb 6a ff bf ...` again; our hook was never entered again |

The near jump leads through a relay page to `RTSSHooks64.dll`, RivaTuner Statistics Server, which
hooks Present in every D3D process on this machine and restores its own bytes on its first
present. Steam re-asserts and takes what it displaced as its original; RivaTuner re-asserts and
drops what it displaced. Nothing patched at that entry can be trusted to stay, and both behaviours
are one hook re-checking its target's entry.

## What was done

`runtime/graphics/src/d3d11_observer.cpp`, with MinHook (BSD-2-Clause, fetched at configure
time, pinned to v1.3.4) for the detour mechanics:

- The observer never owns the entry. It follows the jump chain from the entry, through relay
  pages outside any module, to the first function that lives inside a module: the outermost
  hook's own function (Steam's, RivaTuner's), or dxgi's Present when nothing is patched. It
  detours that. Hooks re-assert their target's entry; none re-asserts its own function's
  prologue. In the game the log reads "detoured the outermost hook's function at ... in
  gameoverlayrenderer64.dll, 2 hops down the chain", and every present since has been observed.
- A trampoline of the observer's own, built with MinHook's length disassembler from the bytes the
  file has for the entry, reaches the genuine body without passing any hook. A re-entered call
  goes there, which is the way out of any loop a re-asserting hook can make.
- The vtable patch remains as the fallback when no target can be established, with the re-entry
  refusal as its only protection.

The observer test, `tests/d3d11_observer.cpp`, had been failing on this machine for the same
reason: RivaTuner dropped the hook after the first present and the second was never counted. It
passes now, and so does the overlay host test.

## The overlay's mouse

The panel's pointer was unusable in the game. It had been following raw mouse deltas around the
game's per-frame warp of the pointer to the centre, which was chunky at best and, under the
desktop's 150 percent DPI scale that the game does not opt into, confined to two thirds of the
panel. SpecialK, ReShade and OptiScaler all do the same thing instead (ReShade
`source/input_windows.cpp`, SpecialK `src/input/cursor.cpp` and `src/input/raw_input.cpp`, read
the same day): while the panel is open they detour `SetCursorPos`, `ClipCursor`, `GetCursorPos`,
`ShowCursor`, `SetCursor`, `GetRawInputData` and `GetRawInputBuffer` in user32. The game's warp is
swallowed, its clip lifted, the cursor shown, the game handed the position the pointer had when
the panel opened, and the mouse fields of the raw input it reads zeroed. The real cursor then moves
freely and the panel reads its absolute position each frame, scaled from client pixels to the
presented image. `runtime/graphics/src/overlay_input.cpp` does that now, and the log names how
many warps and hides were swallowed while the panel was open (4575 warps in one session).

## Two more things the first Windows run found

- The observer read the swap chain's size once, at the first present. AC7 creates its window at
  the desktop's DPI-scaled size, 1707x1067 on a 2560x1600 desktop at 150 percent, presents a few
  frames, then resizes to the 1600x900 its settings say. DLSS was created for an output that did
  not exist and every scene target failed the tap's size judgement. The size is read on every
  present now, and the log says when it changes.
- Every surface promotion replaces is typeless, as Unreal allocates them, and a view on a typeless
  texture has to be told its format. Every F6 attempt failed at "the composite replacement views
  could not be created" until the tail carried the view formats the tap sees at the bindings.
  `tests/scene_promote.cpp` now uses typeless surfaces.

## What is not covered

- A hook installed after ours that patches the function we detoured, rather than the entry, would
  chain through us normally; one that re-asserts that function's prologue would drop us, and
  nothing here would notice. No such program has been seen.
- The DirectInput route. AC7 imports `dinput8.dll`, which is this proxy, and a game reading the
  mouse through DirectInput rather than raw input would still steer under the panel. Not observed
  in this game, which holds still with the raw input zeroed.
- Two crash reports at game exit read "no thread could be made to describe it": an access
  violation during shutdown, which the reporter now describes inline. Whose it is remains open.
