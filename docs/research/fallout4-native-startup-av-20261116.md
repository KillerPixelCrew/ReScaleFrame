# Fallout4 native startup null-platform audit

## Question and method

Determine whether the new native startup AV proves an integration defect or a
controller starting-cell defect, and identify the smallest useful next control.
Production stays frozen. This worker used the durable passive exception trace,
matched PE imports, Capstone chained-unwind disassembly and native MSVC RTTI.
No dead REA/Ghidra session was reopened and no native null bypass was proposed.
No builds, tests, game launch, GPU fixture, deployment or controller edit ran.

Executable Fallout4.exe1.11.240.0 SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The trace is `.local/fallout4-research/native-raster-20261116-095900-661/trace.jsonl`,
PID58104, armed durably before the exception with that exact SHA. The observer
returned false and detached; coordinator reports21 files restored and no forced
kill. Exit was C0000005, not normal exit. The loader ended after the first
overlay drawing stage, before an SR callback.

## Exact fault and native owner

The AV reads address98 at Fallout4+bc83c1, bytes `8b9198000000`,
`mov edx,[rcx+98]`, with RCX0. Immediately before it, bc83ba loads RCX from
native singleton slot30e7108. The method containing the fault beginsbc81f0;
its imported _splitpath_s, strcat_s and _strnicmp calls and filename-table walk
establish path/name resolution work. This is not a demonstrated player actor,
camera, render-target or swapchain receiver fault.

Constructor code1ead9c4 publishes slot30e7108 and1ead9d4 assigns vtable28a4db0.
The vtable's native COL2ace4c8 has signature1, object offset0 and matching self;
its TypeDescriptor30bbcb8 contains
`.?AVBSBethesdaPlatform@BSPlatform@@`. Thus the absent owner is the native
BSPlatform::BSBethesdaPlatform instance. The correct TD is30bbcb8; an initial
worker message transcribed it incorrectly as30bc4b8, corrected by the saved
PE-derived receipt. Do not infer the meaning of member98's enum values from
this identification alone.

Retained JSON:
`.local/fallout4-research/startup-null-platform-proof-20261116.json`.
It contains the exact load, fault bytes, slot, vtable, COL fields and type name.
Native writes to this slot also occur in initialization/destruction code near
1ead94a,1ead9cd,1eadcfa and1eb6bf1. This does not yet explain whether the owner
was never initialized, destroyed early, or changed by earlier work.

The accurate native return stack is:

| Return RVA | Enclosing native entry | Observation |
| --- | --- | --- |
|bd3fe9|bd3e40|Calls bc81f0 atbd3fe4 while processing path strings |
|bd235d|bd2260|Native caller, retained full chained listing |
|bd1ff7|bd1e60|Builds data-relative path strings using `%s%s` |
|bce8a3|bce880|Invokes bd1e60 before native resource registration completion |
|c326fa|c323f0|Native DATA\ and ESM startup path |
|c3efc9|c3ec50|Calls c323f0 atc3efc4 |
|c310b2|c2fff0|Native startup caller |
|c2f927|c2f910|Native entry wrapper |
|22bf5aa|Not assigned a semantic name here|Runtime/native thread-entry chain |

All recorded stack frames are native game/system frames. That rules out an
SR callback on the captured stack, but does not exonerate earlier corruption,
startup-order changes or mod interaction. It is insufficient to assign cause
to RSF, F4SE, ENB, QASmoke or the absence of a saved player.

## Controller hypothesis and precise next control

The ignored validation controller writes `sStartingCell=QASmoke` into the
General section of Fallout4Custom.ini at lines124..127. Its no-RSF F4SE/ENB
baseline reached the ordinary main menu without that custom start. Those
observations differ in both integration presence and startup route, so they
are not a controlled attribution. Skyrim's use of QASmoke is not evidence
that this Fallout4 startup route creates the same initialized player/platform
state. The current native fault is in a platform-dependent path resolver;
no exact player/save prerequisite has been proven.

The smallest next root-owned trial keeps the current RSF/F4SE/ENB binaries,
render options, monitor/controller conditions and bounded observer unchanged,
but removes only this controller-injected sStartingCell override to reach the
ordinary native main menu. Preserve any backed-up user setting and record the
actual test value. Observe startup and close without assuming a gameplay save.

- Same null-platform fault would reject the hypothesis that QASmoke alone is
  required for this failure and focus the startup integration/order audit.
- Successful main-menu startup would establish sensitivity to the custom-start
  route in this artifact combination, not proof that the harness alone is bad.
- If cause attribution is still required, a separate bounded no-RSF QASmoke
  control distinguishes an independently failing native harness route from an
  RSF/mod interaction; root owns whether that additional run is useful.

No configuration change or launch is performed by this worker. A null check,
forced singleton creation, native resource-registration bypass or removing
teardown is unjustified. Startup acceptance also does not establish native-AA
SR, resource parity, generated FG, measured latency or visual quality.

Retained disassembly is `.local/fallout4-research/qa-startup-stack-*.txt` and
`qa-singleton-write-*.txt`. It follows .pdata chains from real entries rather
than decoding arbitrary stack addresses as function starts. The reduced-consumer
findings are frozen in the separate reduced-consumer note while root evaluates
this startup control.


## Main-menu control result, 16 November 2026

Coordinator run `native-raster-20261116-100638-894`, PID60620, requested the
main-menu route without controller starting-cell injection. The45-second
startup observation completed: status reported1,806 application Presents,
foreground1, effective SR backend0 and "Waiting for verified scene inputs".
There were no verified source frames or FG tags. Close completed with normal
exit0. The receipt files.json contains exactly20 entries, all20 restored;
result.json reports restored=true and no experiment/cleanup error.
custom-ini-preserved.json confirms identical before/after SHA256
`5b2749c23d3a95fca9ca850271b0be5f06b11e6f9487e223eae31e8cd2c58180`,
unchanged=true and controller_writes0. The custom INI was not staged as an
extra modified file, explaining20 rather than the earlier21-entry runs.

This proves startup-route sensitivity in the tested RSF/F4SE/ENB combination.
It does not prove an unmodded QASmoke defect, establish a player/save requirement,
or rule out an interaction confined to that route. Runtime foreground1 is a
status sample, not physical focus/scanout/latency acceptance. Effective SR0 is
appropriate without verified scene inputs; this is not SR evaluation or world
resource/quality acceptance. The exact null-platform observation remains valid
for the earlier traced run; its underlying cause is still unassigned.

The latest generic private-chain-usage Release gate
`verify-private-usage-20261116.log` passed221 tests with14 skipped and clean
fmt/Clippy, and the paired device fixture passed. Those generic device/build
results and this menu startup result are distinct from the still-required
native-AA SR/resource capture, reduced quality, generated FG and latency work.
