# Anomaly native observer in Rust

The observer port starts from `9d53556` and uses the native boundary evidence
for Standard and AVX Anomaly 1.5.3, traced to xray-monolith revision
`028ccf5f5189b393396480076b11e40852ba3ecd`. The existing profile tables retain
both executable SHA-256 fingerprints, image sizes, eight hook RVAs and the
extended instruction guards. Prepare checks the running image and start
uses the existing MinHook preflight and process-lifetime trampolines.
[Standard evidence](anomaly-native-hooks.md), [AVX evidence](anomaly-avx-profile.md)

| Engine point | Observer use |
| --- | --- |
| After native Begin | Copy the verified `Device.dwFrame`, width and height once. Rust counts the callback and passes the same sample to its observation and FG. Dimensions are in output pixels. |
| Before and after native End | Run SR's pre-Present handoff before opening FG's End scope. Close that scope after native End, then emit the periodic observer and stage counters. Native End can skip Present. |
| After SetCacheXform | Copy the two 16-float engine matrices under SEH, preserve their row-vector layout, and publish the finite flag from the copy. FG borrows that copy for the duration of its call. |
| Around CRender::Render and before RenderUI | Keep the native geometry view scope around Render. Count scene and HUD entries and forward the verified pre-HUD boundary to FG. |
| After RenderToTarget | Count target 1 as a physical PDA copy and target 2 as a scope copy. Only target 2 reports the secondary viewport to FG. |
| Before reset_begin and through reset_end | Increment the reset count and generation and clear camera validity before reporting the reset to FG. SR and motion continue to read the same observation reset counter. Track reset_end through the original trampoline. |

The initial port moved the GameSDK lifecycle, the active and in-flight state,
the host log lease and the ABI 1 observation into Rust. It used native leaves
for engine reads, profile verification, trampolines, the geometry scope and
stage calls.
The observer counts native invocations. Frame reconstruction, texture
leases, readback, presentation and vendor policy remain with their current
owners. This port changes no shader or buffer producer.

The previous Begin read the three engine globals twice, once for observation
and again for FG. The new regression checks that both consumers receive one
sample and that a failed read forwards no source fact to FG. The observation
keeps its count and reports zero for unavailable frame fields. A failed
guarded camera copy clears the observation's finite flag and leaves the
existing FG camera state untouched until a subsequent camera callback.

The previous quiesce released SR and motion state before checking whether
an admitted observer callback was still using it. Rust closes admission and
drains observer and input invocations before retiring the stage resources.
The existing teardown order keeps the native geometry view scope available
until SR and motion finish. A second drain check follows detour restoration.
Stop holds the lifecycle lock through release of the host log context.
An input-enable refusal now attempts to unwind the partially started
observer and requires an outstanding drain to finish before a retry.
A CPU regression holds both kinds of invocation across stop and checks
the retained session and teardown order.

## Verification

MSVC 19.52 x64 Debug and Release both built the adapter and the FG, SR
temporal, SR round-trip and SR jitter device fixtures. Each configuration
passed 18 focused CPU contracts. They covered the plugin, cold bootstrap,
foreign-process lifecycle refusal, the 216-byte observation ABI, native
trampolines, both profiles' geometry, SR and input hooks, motion hooks, the
overlay input gate and API, and the real Runtime SR API.

Both DLLs matched the staged `8322179a36c7` DLL's export names and ordinals.
The first comparison script missed the Debug lines because MSVC appended
incremental linker thunks to their names. The corrected comparison found
`rsf_anomaly_observation` at 1, `rsf_anomaly_start` at 2 and
`rsf_get_game_plugin_api` at 3 in both builds. The PE imports and lookup
strings confirmed the adapter still resolves the session functions through
the carrier-loaded Runtime.

Cargo formatting, workspace Clippy with warnings denied, and all 222 locked
workspace Rust tests passed. Device execution, live callback retirement and
Standard/AVX gameplay acceptance remain open for this revision. The
prior game baseline applies to the C++ observer. [Game baseline](rust-anomaly-cpp-baseline.md)

## Prepare admission from `3c73e3c4`

The observer now calls the same Rust `running_build` used by cold start. It
checks the running filename before hashing the executable, then requires the
exact Standard or AVX SHA-256. The matching build index selects a borrowed
view of the existing C++ profile with its label, image size, eight sites and
two longer guards. The same index still selects the native detour and stage
tables. The verified identities, RVAs and guards stay in their source tables.

Rust checks the DOS signature and NT offset, PE signature, AMD64 machine,
PE32+ magic and exact profiled `SizeOfImage`. Native callbacks copy the typed
PE headers under the existing C++ SEH guard. Rust then uses the checked image
reader to compare each 16-byte prologue and any longer guard attached to its
RVA. The longer bytes are the recorded unique prefix at that RVA. The
uniqueness evidence remains in the Standard and AVX research. Prepare
publishes the session only after both checks pass, with the existing refusal
texts for build identity, image size and instructions.

Start still pins the adapter before the first HookSet installation. That
installation retains its mapped-region, executable-protection and NT-header
extent checks, and re-reads the 16-byte prologues before any MinHook
registration. Re-enabling an existing set uses retained trampolines. The
native functions still own SEH, the typed detours, the original calls and the
return-address capture in the motion path. [HookSet evidence](rust-anomaly-native-hook-set.md)

This follow-up was checked by source comparison, pinned Rust formatting,
locked offline Cargo metadata and whitespace review. A Rust regression case
covers a changed unique suffix with all 16 prologue bytes intact. Compilation,
linking, Clippy, tests and live device and game behavior for this follow-up
await verification.
