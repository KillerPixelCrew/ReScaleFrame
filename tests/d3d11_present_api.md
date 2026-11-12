# D3D11 presentation DLL access contract

The public entry point is `rsf_get_d3d11_present_api`, declared by
`rescaleframe/d3d11_present_api.h`. Initialize `struct_size` and `abi_version`
(`RSF_D3D11_PRESENT_API_VERSION`, currently 1). It returns the existing bridge's
22 function pointers. It does not create a second runtime or change bridge state.
The header states thread, callback, queue and resource lifetimes and the distinct
boolean, HRESULT and backend-result conventions of the underlying functions.

## Parent build integration

The runtime source registration is already appended to
`runtime/orchestrator/CMakeLists.txt`. Include this one line in the parent's
`tests/CMakeLists.txt`:

```cmake
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_present_api.cmake")
```

Alternatively include `${CMAKE_SOURCE_DIR}/tests/d3d11_present_api.cmake` at the
end of the root build, under `BUILD_TESTING`. Do not include it twice. It registers
`rsf_d3d11_present_api_test`, `rsf_d3d11_present_api_c11` and both CTest contracts.
Clients link the real DLL import library, not OrchestratorCore. This intentionally
avoids inheriting that object's public `RSF_RUNTIME_BUILD` definition. Both tests
have compile-time guards against accidentally building as the DLL owner.

The isolated build used an untracked file
`build/anomaly-runtime-worker/query-tests.cmake` containing:

```cmake
cmake_language(DEFER CALL include "${CMAKE_SOURCE_DIR}/tests/d3d11_present_api.cmake")
```

Reproduce before parent registration, from the repository root:

```sh
cmake -S D:/Coding/ReScaleFrame -B D:/Coding/ReScaleFrame/build/anomaly-runtime-worker -G "Visual Studio 18 2026" -A x64 -DBUILD_TESTING=ON -DCMAKE_PROJECT_ReScaleFrame_INCLUDE=D:/Coding/ReScaleFrame/build/anomaly-runtime-worker/query-tests.cmake
cmake --build build/anomaly-runtime-worker --config Debug --target rsf_d3d11_present_api_test rsf_d3d11_present_api_c11 --parallel 4
ctest --test-dir build/anomaly-runtime-worker -C Debug -R '^d3d11_present_api' -V --output-on-failure
cmake --build build/anomaly-runtime-worker --config Release --target rsf_d3d11_present_api_test rsf_d3d11_present_api_c11 --parallel 4
ctest --test-dir build/anomaly-runtime-worker -C Release -R '^d3d11_present_api' -V --output-on-failure
```

After parent registration, remove the `CMAKE_PROJECT_ReScaleFrame_INCLUDE` cache
value (`-U CMAKE_PROJECT_ReScaleFrame_INCLUDE`) to avoid duplicate registration.

## Executed evidence

Native Windows x64, Visual Studio 2026 / MSVC 19.51.36263.0, warnings-as-errors:

- First RED: real Runtime loaded, missing query export failed the test.
- First GREEN: exported null query returned `RSF_ERROR_INVALID_ARGUMENT`.
- Second RED: version refusal produced the wrong code; second GREEN passed after
  size-first/version-second validation was added.
- Third RED: compatible queries failed in both the C++ DLL-import and C11 clients;
  third GREEN passed after the bridge table was populated.
- Final Debug: 2/2 passed. Final Release: 2/2 passed.
- C++ checks every slot's exact existing bridge type and Runtime DLL module
  ownership, null/short/version refusal, failure byte preservation, protected-page
  header-only refusal, unchanged bridge observations, oversized-capacity trailing
  canaries, and concurrent stable queries.
- The C11 executable compiles the public header as C11 and queries the imported
  Runtime DLL. `dumpbin` independently confirms the C export and EXE import.

Logs under `build/anomaly-runtime-worker/`:

- `red-test.log`, `green-null-test.log`
- `red-refusal-test.log`, `green-refusal-test.log`
- `red-table-test.log`, `green-table-test.log`
- `final-debug-build.log`, `final-debug-tests.log`
- `final-release-build.log`, `final-release-tests.log`
- `native-imports.log`, `native-exports.log`

Runtime artifacts are `bin/Debug/ReScaleFrame.Runtime.dll` and
`bin/Release/ReScaleFrame.Runtime.dll` below that build directory. Import libraries
are under `lib/Debug` and `lib/Release`.

## Limits

No contract test invokes install/prepare/create, a graphics hook, a CPU marker or
a provider transition. Only query and read-only atomic bridge observations run.
This verifies ABI access, not graphics/device/vendor/game acceptance. No game was
launched or installation changed. The shared `eng/verify.ps1` gate was not run:
this worker was explicitly restricted to the isolated focused build.

The table does not add `frame.ui` transport to native FG. A separate UI-layer input
remains an integration gap, and exposing the existing UI-mode setting does not
resolve it. Callback-owning DLLs and Runtime must remain loaded for the bridge's
process-lifetime hooks; this access surface provides no uninstall operation.
