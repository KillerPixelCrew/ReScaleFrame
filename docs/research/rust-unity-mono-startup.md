# Unity Mono startup and configuration port

U01 uses the existing loader, Mono embedding and Runtime callback contracts from
`1150dcbb`. This is a source port. The checked-in managed helper, its native API
record and the Runtime callback-host V2 prefix retain their existing ABI.

| Finding | Source evidence |
| --- | --- |
| Callback admission during retirement | `rsf_unity_mono_try_close` samples the fence under the bridge guard. Rust checks the fence, packet store and callback count and closes admission in the same state transaction. The lifecycle retains ownership across a busy stop or failed prepare and reopens callbacks only when publishing fresh services. The managed registry lookup also takes a callback lease. The package retirement entry and V2 policy callbacks keep their signatures. |
| Auto FG with only Streamline configured | The plugin supplies the `ReScaleFrame` directory beside the startup INI as the Runtime root and sends the independent vendor directories including explicit empty values. The existing presentation owner copies these values and selects a provider after device discovery. The source fixture covers Streamline-only path preparation. |
| Graphics diagnostics in the shared overlay | Open. The callback host V2 publishes no adapter-defined controls, and the Advanced control ABI exposes toggles and numeric values. Live directory editing and persisted restart-labelled debug control need a generic callback-host option contract and a directory editor or picker in the shared overlay. The existing startup settings remain callable. |
| Late Mono arrival | The initial SR attempt runs once after the settle delay. A fake-clock fixture places Mono availability in the last polling interval and checks that the retryable result returns after one attempt. |
| Executable fingerprint on retries | The combined starter owns one name and SHA-256 fingerprint, populated at the first SR attempt through the existing `rsf-windows` digest helper. It lends that identity to each Runtime start call. The standalone SR entry starts with a fresh identity. |
| Rooted configuration paths | The shared resolver checks both initial separator forms before looking for a drive marker. The fixtures cover a single slash or backslash and rooted paths with leaf components. |
| Unused configuration exports | Removed the two C wrappers and their header after checking tracked callers. Rust config helpers serve both Unity routes. |

The plugin DLL still exports the four Windows loader entries plus
`rsf_get_game_plugin_api`. CMake builds the Mono plugin variant of the Rust
archive separately so package links do not acquire the plugin-specific DLL imports.
The public `unity_runtime_host.h` remains a C consumer contract and supplies the
identity constants used by the device fixture. The D3D12 fixture stays in C++ to
exercise the real Runtime imports, COM and Streamline paths. The isolated Mono
fixture moved to Rust and keeps its target name, three path arguments, Mono embedding
entry points, managed `Program.Run` dispatch and result propagation.

Source checks covered the loader lookup and domain fixture, Runtime import
declarations, V2 callback setup and the package bridge's renamed native entry
points. Regenerated the two changed binding modules with bindgen 0.72.1 and used
Rust 1.94.1 formatting on the changed Rust files. Cargo metadata and
`git diff --check` passed. Builds, fixtures, lint and device execution are pending
user verification.

The Mono startup now enables the optional D3D12 debug layer through typed
Rust bindings. DLSS preset admission uses the mask generated from the same
C header. The former native graphics file is removed and the D3D12 import
link stays in place. Binding regeneration and source checks passed.
