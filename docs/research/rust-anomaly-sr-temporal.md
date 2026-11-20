# Anomaly Stage 5 Rust driver

The Anomaly adapter at `fa07fec` used the C++ Stage 5 driver with the Rust-backed
Runtime. Revision `8322179a36c7` established the live baseline on the Standard
and AVX executables. The port keeps the calls from SR Stage and checks that the
same Runtime table receives the same frame data. [Live baseline](rust-anomaly-cpp-baseline.md)

## Calls and lifetime

| Boundary | Owner and evidence |
| --- | --- |
| Configure and bind | The C++ facade supplies the existing Anomaly engine identity, metres, standard depth and RTV2 decode constants. Rust owns the requested settings, copies the vendor directories, validates the full D3D11 SR API 4 table and checks pipeline ABI 5. The facade resolves the query and colour transport setter against the carrier's borrowed Runtime module. |
| Drained End boundary | Rust starts and stops the shared pipeline, forwards the user's quality, backend and preset to the Runtime, and follows device and output changes. The existing Stage guard serializes the render path. A backend's first selection is announced for one Present before calling the Runtime. The overlay reads a mutex-protected snapshot while Runtime calls and logging run outside that mutex. |
| H1 plan | Rust accepts only a running pipeline with matching output dimensions. The existing native render extent rule still admits the Runtime's render size or leaves Stage to use the fallback scale. |
| S-B seam | The native pass builder still uses the main-view frame index, installed session, twin generation, main view and history keys, row-vector camera matrices and render-pixel jitter. Rust adds the driver's reset facts, counts builder refusals and calls the Runtime with the resulting Game ABI pass. The native caller owns colour, output, depth, motion and exposure through the return from evaluate. |
| Recovery and teardown | Stage still owns native target allocation, input preparation, reinsertion and bilinear recovery. A refused pass leaves the driver's pending reset intact. Quiesce calls stop before Stage releases its temporal targets and the engine device goes away. |

The shader and buffer audit covers the FP16 scene-colour twin, R24G8 standard
depth, previous transform producers, RTV2 validity, first-person handling,
exposure texture, and downstream tonemap and UI. Stage and the existing
motion owners supply the textures for the main view. This work used source
and CPU evidence for these handoffs. [SR seam research](anomaly-sr-seam.md)

The previous validator allowed a null sharpening slot in an otherwise complete
table. The API fixture now rejects that table while exercising the real Runtime
export through both a direct query and the adapter's module lookup. Fake-table
binding used to leave the SR session at zero. Both binding paths now give passes
the installation identity from QPC, and test-table validation uses its own
argument for each call. A start failure is suppressed only for the same device,
output size and settings. Returning to an applied setting lets a
later request retry a previously refused choice. The overlay reports the quality
that produced the last accepted frame even when a boundary has already applied
the next quality. Focused API fixture assertions cover those cases.

## Validation

MSVC 19.52 x64 Debug and Release built the production adapter and the SR
temporal, round-trip, jitter and FG device fixtures. Both configurations passed
all 23 selected CPU checks, including the real Runtime SR API, Standard and AVX
hook and input contracts, the adapter lifecycle, overlay API, render plan,
pass and reconstruction. An early Debug API run linked before the new Rust
archive was rebuilt and failed the new retry assertion. Relinking passed the
assertion in both configurations.

Both DLLs matched the three export names and ordinals in the staged
`8322179a36c7` Anomaly adapter. The import audit confirmed adapter lookup
through the carrier-loaded Runtime module. Cargo formatting and Clippy passed,
and all 216 locked workspace Rust tests passed. Device execution, vendor output
quality and in-game behavior remain open for this revision.
