# RTX 40 MFG unlock

## Question and implementation

Can ReScaleFrame's existing Streamline 2.14.1 path generate more than one intermediate
frame on RTX 40 without replacing its presentation owner or requiring game restarts?

Source inspected on 18 October 2026: [MFG-Unlocker](https://github.com/dashdogy/MFG-Unlocker)
revision `2b52823e3769c31cf52d3b368c6963cff8fc5344`, specifically `midpoint_fix.cpp`,
`ngx_mfg_gate.h`, `universal_wrapper_profile.h` and the wrapper publication in `patcher.cpp`.
The MIT notice and selected sources are retained in
`runtime/backends/dlss/third_party/mfg_unlocker`. Optional experimental kernels,
RTX 30 ports, external shader caches, loaders, overlays and OptiScaler are excluded.

Increasing the reported maximum alone is insufficient. The RTX 40 temporal kernel uses
fixed midpoint weights. Upstream's production transform replaces those weights with
the requested interpolation interval and its complement. Its provider-version profiles,
source PTX/fatbin SHA-256 checks, output SHA-256 checks and immutable descriptor
publication are preserved. This creates program data from the installed NVIDIA provider
in memory; no vendor program data or modified DLL is stored in Git or deployed on disk.

The wrapper is primed before Streamline initializes its plugins; the temporal and
count-gate publications follow NGX initialization, before ReScaleFrame creates a
DLSS-G session or swap chain. CUDA compute capability must be
8.9 on the exact D3D12 device LUID. A unique loaded module must expose the DLSS-G
device-parameter population export and no DirectSR export, and have an explicitly
supported provider version. This checks the loaded NGX implementation rather than a
sibling SR DLL or assumed installation path.

After the temporal publication, the provider's verified count/index gate changes
`0f 84` to `eb 04` inside the unique 13-byte context. Upstream's unwind-function,
branch-target, alignment, page-protection and atomic two-byte publication checks remain.
The Streamline wrapper's unique `ba <1/3/5> 3b ca 0f 42 d1` profile identifies its
compiled generated-frame capacity; replacing the final three bytes with NOPs removes
the GPU clamp without increasing the compiled capacity. Unknown or ambiguous patterns
refuse the unlock. Runtime logs record admission and the selected provider.

## Ownership and controls

The existing D3D11/D3D12 presentation owner, same-frame constants, normalized depth,
motion and final HUD-less colour remain the inputs. Frame/view identity, queue fences,
resource leases and provider retirement are unchanged. Pacing remains with Reflex for
DLSS-G, and with the selected provider after a switch to FSR or XeSS.

The cloned temporal descriptor and its provider are retained for the process because
NVIDIA can cache the published pointer. The patched wrapper is leased by the Streamline
host until its shutdown. Repeated Off/On and drained provider changes reuse the same
publication. A late DLSS-G plugin load is prepared before its first allocation too.
An unrecognized provider or failed patch retains the ordinary path and limits the
RTX 40 admission to one generated frame. Other GPU architectures keep their SDK limits.

The existing overlay multiplier changes the generated count at runtime. Admission is
shown separately from SDK-confirmed activity. FG contract ABI 2 appends the admission
status; overlay ABI 12 appends the corresponding field. All consumers must be rebuilt
and deployed together. Selecting 2x uses one intermediate frame; 6x uses five.

The first implementation patched the wrapper after plugin initialization. Although its
bytes and temporal publication matched, Streamline had already cached a one-frame
maximum and rejected higher requests. Moving the wrapper publication before plugin
initialization corrects the cached SDK limit. ReScaleFrame uses the smaller of the
initialized SDK limit and independently discovered wrapper capacity, and never
fabricates presentation activity.

## Validation

The opt-in `rsf_fg_present_bridge_test` mode `12-mfg` exercises the real vendor backend
with synthetic scene inputs. It checks FSR/XeSS/Off/DLSS transitions, preserved engine
buffer identity, runtime 2x/3x/4x/6x requests, generation Off/On and a second DLSS
provider entry. It requires admitted unlock status and advancing SDK presentation
counts at each multiplier. This is device execution evidence, not game image quality,
physical display timing or input-to-display latency evidence.

Game launches and visual acceptance remain user-run.

The unfocused fixture accepted runtime count changes after the initialization repair,
but produced only source presents. A verbose diagnostic build reported
`DLSS-G disabled: window not focused` in `dlfgPresent.cpp:2544`. Visible but nonactivated
presentation and changing spatial detail did not remove this gate. A focused test is
therefore explicitly opt-in (`RSF_MFG_FOCUS_TEST=1`), limited to 20 seconds, restores the
previous window, and stops if the human changes focus. Do not count the unfocused runs
as proof of interpolated output. The diagnostic verbosity change is not shipped.

The human explicitly authorized one focused test. On the RTX 4070 Laptop, the test
passed with confirmed samples at 2x: 8, 3x: 15, 4x: 41 and 6x: 15. Individual SDK
presentation deltas were 2, 3, 4 and 6 respectively. At generation Off the delta was
one and activity zero; returning to 4x restored four presents. After a provider
Off/DLSS round trip the initialized feature again produced four presents per source.
FSR had 18 active samples and XeSS nine. Concurrent engine queries and retained buffer
identities passed. The complete fixture had 106 active samples and exited successfully.
Raw device log: `.local/mfg-focus.log`. It used logging-only instrumentation; shader
transforms, initialization, presentation and switching were the production code.

The final production build explicitly recompiled `dlss_generation.cpp` after removing
that diagnostic logging override. From isolated main baseline `3cc6af2`,
`eng/verify.ps1 -Configuration Release -VS2026` passed 44 native checks with eight opt-in
skips, Rust formatting and Clippy. `cargo test --workspace --locked` passed 59 tests,
including the real overlay control's unlocked 6x mapping and refused-unlock 2x limit.
The production overlay passed its ABI/input/pixel/resize fixture. The Minecraft JAR
was rebuilt with the production JNI DLL and upstream notice. No game execution,
game image quality, physical display cadence or input-to-display latency is claimed.

The production AC7 proxy/plugin/overlay, Unity loader/runtime/plugin/overlay and
Minecraft JAR were deployed with backups and installed SHA-256 verification.
`.local/deploy-backups/rtx40-mfg-20261018-094106-547/deployment.json` records the exact
sources and destinations. Existing preferences and all 277 original Unity game files
were preserved. Vendor runtime files remain unmodified on disk.
