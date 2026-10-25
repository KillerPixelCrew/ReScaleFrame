# FSR4 SR through Vulkan/DX12 interop

The direct Godot Vulkan path needs an orchestrator-owned FSR4 service because the
installed signed ML upscaler is a DX12 provider. The existing first-party
`runtime/vulkan/fsr4/bridge.cpp` supplies the matching-adapter DX12 execution and
INT8 compatibility lease. The new private bridge API and Vulkan service extend
that implementation without changing the legacy 96-byte frame ABI or SR ABI 1.

## Contract and source corrections

`bridge_api.h` checks structure size/version, accepts an absolute runtime
directory and exact adapter LUID, preserves reverse/infinite-depth and motion
jitter flags, and accepts explicit per-frame motion scales/pre-exposure. The
new depth path accepts R32F rather than requiring a depth-stencil resource.
The bridge still forces the newest enumerated FSR4 provider and verifies the
effective provider identity. NVIDIA/Intel still require the existing pinned
signed-runtime INT8 compatibility lease. Refusal never selects an older provider.

The private `native_sr_fsr4_vulkan` service imports dedicated shared RGBA16F,
R32F and RG16F images plus a permanent D3D12-fence timeline semaphore. It verifies
capability/memory-type compatibility and exact adapter identity, retains COM
references through both graphics APIs, and closes imported NT handles after
reference import. The engine input/output images remain engine-owned. Input
copies, EXTERNAL ownership transfers and output reinsertion stay on the GPU.

The size/version-checked `split_record` callback records graph-owned section
boundaries. It does not submit the producer immediately. The normal engine
execute submits the old section with previous-done wait and ready signal, and the
new continuation with a done wait. The DX12 queue can already wait for that future
ready signal. After successful reconstruction the service records output acquire
and GPU copy in the continuation. An SDK failure returns that continuation for
spatial fallback; the bridge still queues ready-before-done handoff. Both graph
sections must be submitted even on provider failure. Abandoning them would leave
the DX12 queue waiting for unsubmitted work. Callback failure is terminal and
retains the owner because the original recording buffer may have been consumed.

One increasing primary source frame is admitted per context. Duplicate same-frame
evaluation is refused before recording or SDK work. This also prevents allocator
reuse while an earlier evaluation from the same unsubmitted graph is waiting for
its own future producer signal. Context destruction requires all leased graph
sections submitted and both graphics APIs drained; a failed drain retains owners.

## Device evidence

The bridge, service and standalone fixture compiled on Windows using the
installed LLVM-MinGW compiler with `-Wall -Wextra -Werror`. This is separate from
the main CMake/MSVC verification. The existing CMake MinHook source under
`build/windows-x64/_deps/minhook-src` was compiled into isolated ignored objects.
No cached DLL, SDK source, global package or installed application was changed.
CPU-only checks rejected short structures, mismatched ABI and a relative runtime
directory without loading a driver.

`eng/test-fsr4-vulkan-interop.ps1` ran the explicit device mode under the common
Godot GPU mutex, with process-only extension-point mitigation effective (`1`).
Khronos reported core and synchronization validation enabled. Six 426x240 to
640x360 evaluations completed through the deferred producer/consumer submission
path on NVIDIA vendor `4318`. The fixture used reverse and infinite depth, R32F
depth, RG16F zero UV motion, explicit render-pixel motion scales and RGBA16F
color/output. All six duplicate frame evaluations were refused. SDK dispatch,
Vulkan output reinsertion and checked destruction returned zero, with no VUID or
synchronization error.

The selected provider ID was `17700776140664213505`, and INT8 compatibility was
reported active. The signed upscaler SHA-256 was
`D0DCCCC74A43C44BA435B7A369B456E0970D8A4464E4BD683119B374F2C9FB46`, matching
the existing compatibility fingerprint for SDK 2.3.0 upscaler 4.1.1.2740. Fixture
SHA-256 was `05934E4D040A0F511D7A309832CC345A0CC49F66AF8B527222A2B8834CAA250B`.
Compact evidence is `.local/godot/fsr4-diagnostic/deferred-submit/receipt.json`,
`stdout.log` and `stderr.log`. The first run spent approximately 43 seconds
compiling kernels and exited normally; it was not a synchronization timeout.

This establishes actual GPU dispatch and interop ordering on this device. The
fixture uses synthetic constant images and does not read pixels back. It does
not establish Godot reconstruction quality, object/camera geometry, exported
player acceptance or broad readiness. Root owns the engine graph split,
generic SR routing, staging and subsequent actual Godot image validation.

Subsequent diagnostic instrumentation adds the existing FrameCaptureVk reader
with the verified provider ID. It records `sr-linear-color`, `sr-depth` and
`sr-motion-uv` in the producer section, and `sr-output-linear` from the owned
shared output after its exact whole-image copy into the engine destination. Mapping occurs
only after timeline completion, Vulkan idle and checked DX12 drain. The first
Godot capture found that the engine output promises transfer destination only;
recording it as a capture source correctly failed the usage guard. The corrected
hook captures the owned source of that same copy, whose allocation explicitly
supports transfer source, without changing the engine's usage promise. This
instrumentation object-compiled with warnings as errors; its actual Godot capture
test was still pending when these source files were frozen for the next gate.
