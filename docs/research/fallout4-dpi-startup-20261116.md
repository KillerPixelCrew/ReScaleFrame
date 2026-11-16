# Fallout4 early DPI startup, 16 November 2026

## Defect and bounded evidence

Coordinator InteractiveScene trial `native-raster-20261116-101156-903`,
PID61956, reached the menu without verified SR inputs. Physical outer bounds
were1944x1139, client1920x1080, GetDpiForWindow96 and native chain1280x720.
The retained `window-fit-menu.png` showed the image at upper left with large
black right/bottom areas. Coordinator input did not reliably align with the
visual controls. This is a menu/window scaling defect before SR, not world or
SR acceptance. Return advanced the intro to the menu/update modal, but no world
was entered. The controller's interactive handoff timed out; it closed normally
with exit0 and20/20 manifest entries restored. This worker did not operate or
stop that process.

The missing startup DPI request is a source-level difference from Skyrim's
accepted adapter mechanism. The observed96-DPI window and physical/native extent
ratio are consistent with virtualization, but the actual fit/input correction
still requires a measured next run. Do not attribute all possible scaling
causes to DPI or equate an API success with final fit acceptance.

## Narrow adapter change and timing

Only `games/fallout4/src/plugin.cpp` changes production code. After exact
executable and native creation preflight, `prepare_dpi` runs before reader/hook
preparation, generic host creation, renderer activation or native D3D creation.
It guarded-reads the matched RendererData pointer38e13a0, native device+48 and
HWND+58. A missing renderer is an early cold state; unreadable owner or existing
device/window refuses instead of claiming early setup.

The carrier runs plugin prepare/start from its startup worker, not a proven
synchronous game-entry barrier. Accordingly this source placement alone is not
a universal scheduling guarantee. The cold native fields are checked at the
request; before changing an incompatible default, EnumWindows must also
successfully find zero top-level windows owned by this process. This catches
an already-created native window even before its HWND is published in the
renderer. No window-class guess, window mutation, new hook or suspension
framework is introduced. A next live receipt must demonstrate the actual
cold/request/effective awareness facts and native window dimensions.

The adapter dynamically resolves SetProcessDpiAwarenessContext,
GetDpiAwarenessContextForProcess, GetThreadDpiAwarenessContext and
AreDpiAwarenessContextsEqual from user32. Target context is per-monitor-v2
(the documented-4 handle). Process default and caller thread must both compare
equal to that exact context after preparation. An already compatible caller/
process is preserved without calling the setter. No SetThreadDpiAwarenessContext
is used, no context is silently overridden, and a fixed incompatible awareness
or missing API refuses with RSF_ERROR_NATIVE_REFUSED. A successful setter whose
effective caller context is still incompatible also refuses. Prior process or
thread awareness is not described as corrected merely because a request ran.

The receipt logs requested, API accepted, prior_compatible, verified, error,
owner readability, native device/window absence, enumeration success/process
window count, and opaque before/after process/thread contexts. It is written
through the incoming host logger, before generic services exist. A separate
specific refusal reason follows if necessary. The caller's LastError is
preserved. The process default remains a process-lifetime startup setting;
shutdown does not attempt to change it after native windows exist. There is no
new option, runtime game policy or overlay setting.

Microsoft documents that process awareness must be set before DPI-dependent
operations/windows, already-fixed defaults reject changes, and a thread may
have its own awareness context. Sources:
[process DPI setter](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setprocessdpiawarenesscontext),
[process context query](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiawarenesscontextforprocess),
[thread context query](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getthreaddpiawarenesscontext).
These API contracts explain the explicit re-query/refusal behavior; they are
not evidence of this game's next window fitting correctly.

## Checks and remaining acceptance

Scoped source ordering and diff checks passed. No build, test, game, GPU,
deployment or commit was performed by this worker. Prior generic repair and
all other production files are untouched in this task. The coordinator owns
the focused Entry build against its last verified Runtime and the next bounded
DPI/world trial. Measure physical client bounds, GetDpiForWindow and actual
chain/output dimensions, then input alignment and world entry; retain SR0 when
verified inputs are absent. Full SR/FG/latency and moving quality remain
separate work.


## Measured DPI trial and validation-only held-key helper

Coordinator trial `native-raster-20261116-102329-435`, PID56736, logged PMv2
accepted/verified. Its physical client1280x720 matched the native1280x720 chain,
GetDpiForWindow was144, and the retained screenshots showed the frame filling
the window. Outer bounds were1302x776. This accepts the measured menu window
fit for this run, correcting the earlier96-DPI physical/native mismatch. It
does not establish world input alignment, render inputs or SR evaluation.
The world handoff timed out without capture; normal exit0 and20 restored
manifest entries were reported. Both screenshots and window.json remain in
the report. This worker did not operate or launch the game.

Orca Return/click briefly advanced intro/update UI, but instantaneous synthetic
keys were unreliable for native buffered polling. Literal backtick text did
not open the console; text is not proof of a physical scan-code event. The
ignored `.local/fallout4-research/send-owned-game-key.ps1` therefore provides a
narrow validation action, independent of production CPU/input policy. It takes
an explicit fresh owned GamePid, key and fresh receipt path. It checks the
exact installed path and executable SHA, rechecks process start/path to reject
PID reuse, and immediately gates foreground HWND ownership to that PID.
Unfocused/mismatched targets refuse without a press or any focus change.

Only Enter scan1c, Escape01 and Console29 are accepted. SendInput emits physical
scan-code down, holds150ms, then attempts key-up in finally even on failure or
focus loss. Down/up are independent INPUT values. The receipt records action,
scan, timestamps, foreground HWND/PID, API return counts/errors and any refusal;
ui_success_inferred is always false. A failed key-up is reported, never called
successful delivery. There is no text entry, keylogging, launch, forced close
or foreground manipulation. Root must compare screenshots before/after and
retain the controller's ordinary ownership/restoration rules.

Invocation for root, with current owned PID and a new receipt file:

```powershell
pwsh -NoProfile -File .local/fallout4-research/send-owned-game-key.ps1 `
  -GamePid <fresh-owned-PID> -Key Enter -ReportPath <new-receipt.json>
```

PowerShell AST parsing passed; the helper was not invoked and no input was sent
by this worker. The combined Release gate was running while this helper was
prepared; no result from that still-running gate is claimed here. Production
files remained untouched.
