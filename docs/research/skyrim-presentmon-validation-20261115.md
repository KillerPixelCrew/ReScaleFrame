# Bounded Skyrim display validation preparation, 15 November 2026

Preparation only: no Skyrim launch, ETW capture, GPU fixture, build, deployment
or settings change occurred. This does not validate FSR FG or resolve SDK/header
readiness. `docs/tooling.md` and installed tools were checked first.

## Tool receipt

Existing WinGet CLI `presentmon.exe --help` reports PresentMon2.6.0.
Resolved executable:
`C:\Users\N1GHT\AppData\Local\Microsoft\WinGet\Packages\Intel.PresentMon.Console_Microsoft.Winget.Source_8wekyb3d8bbwe\presentmon.exe`.
SHA256 `b2a706bc6ad475749e3b7e3409263aa1e6906d45bdcf993f6dbc0f660188f1af`
matches the official x64 asset; Intel Authenticode is valid. The executable
has no file/product version resource, so version comes from its CLI and hash.
No binary download or installation was needed. [Official release](https://github.com/GameTechDev/PresentMon/releases/tag/v2.6.0).

Ignored `.local/presentmon-validation-20261115/` holds `receipt.json`, CLI help,
MIT copyright/permission text in `README.md`, and primary reference files pinned
to release commit `e13fce6acdb55a808fd8318175a56863e532d95f`.
Only `--help` was executed. No PresentMon process remained afterward.

## Run only when the bounded game test is authorized

From the repository root, supply the actual running SkyrimSE PID:

```powershell
./.local/presentmon-validation-20261115/capture-skyrim-10s.ps1 -SkyrimProcessId 12345 -Label fsr-on
```

The wrapper checks PID/name and the pinned executable hash, creates a unique
output directory and ETW session, and writes CSV, console output and a receipt
with QPC frequency/start/end and the process start time. It neither starts the
game nor installs a service. Replace12345; do not reuse an old PID.
Its exact CLI argument pattern is:

```powershell
presentmon.exe --process_id 12345 --output_file D:\Coding\ReScaleFrame\.local\presentmon-validation-20261115\frames.csv --session_name RSF_Skyrim_12345_unique --timed 10 --terminate_after_timed --terminate_on_proc_exit --qpc_time --write_display_metadata --track_frame_type --track_hybrid_present --no_track_input --no_track_gpu --no_console_stats
```

Display tracking stays enabled. Input collection and GPU execution-duration
tracking are disabled to keep the check focused. Dropped frames are retained.
The native timer stops capture and exits; process exit can end it sooner.
No hotkey watcher or delayed waiting process is requested. The direct command
expects its output directory to exist; the wrapper creates one.
These switches are verified against the installed help and pinned
[command parser](https://github.com/GameTechDev/PresentMon/blob/v2.6.0/PresentMon/CommandLine.cpp).

If ETW access is denied, record the error and use an already authorized elevated
shell for the later run; do not change group membership or silently restart
as admin. PresentMon documents administrator/Performance Log Users access.
[Access requirements](https://github.com/GameTechDev/PresentMon/tree/v2.6.0#user-access-denied).

## Interpret the evidence

| Evidence | Meaning and comparison |
| --- | --- |
| `MsBetweenPresents`, `MsInPresentAPI` | OS/runtime Present cadence and time in that API, not original engine source rate. FG may introduce additional presents. Compare with RSF native application Present and source counters. |
| `DisplayedTime` | ETW-attributed display duration; `NA` means not displayed. Count valid display changes within one continuous chain/display/layer segment, not every CSV row. |
| `MsBetweenDisplayChange`, `MsUntilDisplayed` | Display-change cadence and Present-to-display delay. Useful against app-present cadence; neither is a complete native input-to-photon measurement. |
| `CPUStartQPC` | Correlation clock. CPU-start semantics are inferred by PresentMon; do not equate it with Skyrim's input sample or RSF source ID. |
| `PresentMode`, `VidPnSourceId`, `LayerIndex`, `PresentId`, `SwapChainAddress` | Partition streams and retain path changes instead of combining all process rows into one FPS number. |

Metric definitions: [pinned CLI documentation](https://github.com/GameTechDev/PresentMon/blob/v2.6.0/README-ConsoleApplication.md#csv-columns).

`--track_frame_type` requires matching application/driver instrumentation.
The release's frame-type enum has Application, Repeated, Intel_XEFG and
AMD_AFMF, but no dedicated FSR3 SDK type. AFMF is not evidence of FSR SDK
generation. The command requests available labels; unlabelled or Application
rows must not be reclassified as original engine renders. Expect to infer an
FSR cadence increase by comparing RSF source counters with ETW display events,
and report explicit per-frame generated identification as unavailable unless
the actual trace proves matching instrumentation.
[Frame-type definitions](https://github.com/GameTechDev/PresentMon/blob/v2.6.0/PresentData/PresentEventEnums.hpp).

Preserve nondisplayed/discarded records and console loss warnings. An occluded
or minimized window can keep producing callbacks without corresponding display
changes; `NA` alone does not distinguish occlusion, discard or incomplete ETW
observation. Do not impute missing display timestamps or report no-loss evidence
without inspecting diagnostics. Boundary frames can lack a complete duration.

On the Intel/NVIDIA hybrid path, renderer adapter and scanout adapter can differ.
Request hybrid-copy metadata and record the actual PresentMode/display/layer
and external versus internal display. A composed frame is attributed through
DWM; a hardware flip is attributed using kernel flip/vblank events. Neither
is an optical measurement of panel pixels, and a cross-adapter transfer can
change the route being compared. Display metadata IDs are not by themselves
GPU adapter identity. [ETW attribution paths](https://github.com/GameTechDev/PresentMon/blob/v2.6.0/PresentData/PresentMonTraceConsumer.hpp).

Take one settled10s FG-off and one matched10s FSR-on sample, preserving display,
scene and presentation mode. Compare the same interior QPC interval with RSF
native source, application Present, generated-present callbacks and effective
provider counters. Report ETW display-change rate, app-present rate, dropped/NA
records, path transitions and event loss separately. Increased callbacks or
even ETW display cadence does not establish lower latency, unique image content,
smooth generation or physical scanout acceptance. No result is claimed here.

Preparation checks: script AST parsed successfully, official reference/hash and
signature receipts recorded, scoped documentation diff check passed. The script
has not run against a process; ETW permissions and actual CSV schema remain
runtime checks for the authorized experiment.
