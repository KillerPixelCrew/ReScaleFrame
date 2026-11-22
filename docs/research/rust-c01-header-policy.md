# C01 shared header policy

Base `9716068b0237b76cb6279c90cceaec39cbf00826`. The C headers now declare the policy queries, and their Rust owners provide the same C call signatures. Header consumers that call the queries resolve them from a linked archive or the Runtime import library.

| PORT item | Disposition and source evidence |
| --- | --- |
| `game_frame.h` | Ported to [GameSDK frame policy](../../sdk/game/src/frame_policy.rs). All three queries read only the leading size word before borrowing the record. Null and short records return zero. SR refuses video, loading and the SR veto. FG also refuses menu, unknown and the FG veto. Other screen values keep the previous default. Rendered provenance requires absent input QPC and both nonzero source timing fields. Frame ABI and layout stay the same. |
| `game_renderer.h` | Ported to [GameSDK renderer policy](../../sdk/game/src/renderer_policy.rs). It scans the full 240-byte record, including reserved words and both nested domains. Both layouts consist entirely of 32-bit words and have size assertions that guard against padding. Signed zero requests mapping. [Runtime region validation](../../runtime/orchestrator/src/native_regions.rs) and both Godot paths use this owner. |
| `frame_generation.h` | Ported to [Runtime FG choice](../../runtime/orchestrator/src/fg_choice.rs). The default vendor order, software refusal, backend IDs and choice mask preserve the original values. The provider lookup returns immutable static rows with the same strings and returns null for unknown IDs. The INI labels are legacy metadata. [D3D11 presentation](../../runtime/orchestrator/src/d3d11_present.rs) uses the same implemented-provider predicate as the C API. |
| `plugin_callbacks.c` | Converted from C++ to a C11 leaf, retained as the native exception boundary. Each guard is an SEH `__try` whose filter accepts only the MSVC C++ exception code `0xE06D7363` (what the former `catch (...)` stopped under `/EHsc`), so access violations still propagate. `build.rs` compiles it in its own C build (`/std:c11`) beside the remaining C++ frame-tap leaf. [Plugin session](../../runtime/orchestrator/src/plugin_session.rs) calls the guarded query, detect, prepare, start, control and status functions. The native log guard also contains logger exceptions. [The throwing fixture](../../tests/plugin_session_fixture_throw.c) is now C. It cannot throw a C++ object, so detect and prepare call `RaiseException(0xE06D7363, ...)` after the Rust callback returns. The guard keys on that code, so the same containment path runs, but a real object throw and C++ destructor unwinding are no longer exercised. |

## Native link paths

- [Runtime exports](../../runtime/orchestrator/src/rust_exports.c) expose the C query names for the Runtime DLL and embedded consumers. The [orchestrator target](../../runtime/orchestrator/CMakeLists.txt) links the Rust archive, which includes GameSDK. Its FG session, CPU platform tables and OpenGL pipeline call Rust directly. Other native platform tables keep their existing signatures.
- The [AC7 and shared carriers](../../loader/proxy/CMakeLists.txt), [Unity package](../../integrations/unity/CMakeLists.txt) and [Godot extension](../../integrations/godot/CMakeLists.txt) link the embedded core. The [standalone DLSS archive](../../runtime/backends/dlss/CMakeLists.txt), [presentation archive](../../runtime/presentation/CMakeLists.txt) and frame dump archive carry the Rust core dependency.
- [Fixture target wiring](../../tests/CMakeLists.txt) preserves the C-only header object, the standalone plugin contract executable with its own Rust fixture archive and GameSDK dependency, and the D3D11 host and FG choice paths through the Runtime import surface. The existing native throw cases are unchanged.

## Checks

Pinned Rust 1.94.1 formatting, bindgen 0.72.1 regeneration, locked offline Cargo metadata, header layout and constant comparisons, and source link-path review. Native link, fixture, device and game results are pending.

The integrated D04 native tables now call the same SDK predicates directly.
One Rust pass callback adapts the depth-domain query for both SR12 and FG12,
so the later D04 table remains wired after the C predicate object is removed.
Strict C11 syntax checks passed for the three retained public headers.
