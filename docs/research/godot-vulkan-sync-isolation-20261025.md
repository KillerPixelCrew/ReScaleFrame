# Godot Vulkan synchronization isolation, 25 October 2026

The recorded `PRESENT_AFTER_WRITE` validation failure is isolated to the legacy
injected-hook path in this environment. A creation-time process mitigation
excludes `RTSSHooks64.dll` and removes the reported present hazard on both the
official editor and the custom editor. RTSS stays running; no application profile,
global mitigation, service or user setting changed.

This resolves the missing isolation in the
[earlier synchronization investigation](godot-engine-integration.md). It does not
establish an actual GPU race or identify a faulty instruction inside RTSS. An
interaction between injected hooks and validation remains possible. No native
WSI synchronization change follows from this result.

## Question and approach

The earlier official-engine control also reported the hazard, while filtering
the RTSS Vulkan layer still left `RTSSHooks64.dll` injected. A failed attempt to
persist a scoped RTSS profile supplied no renderer evidence. That failed profile
procedure was not repeated and RTSS was not stopped.

Windows' documented
[Extension Point Disable creation policy](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
blocks legacy extension points, including global Windows hooks, while retaining
ordinary DLL loading. The new bounded control launches only its owned Godot
process with that policy, queries the effective policy before resuming it, and
records loaded modules throughout execution. A normal launch uses the same
creation helper with the policy deferred. Explicit inherited handles are limited
to the diagnostic stdin/stdout/stderr streams.

The same minimal project renders an unshaded moving cube with a camera and no
add-on. There is no ReScaleFrame native runtime, Streamline, SR, FG or PCL owner
in either process. Vulkan implicit layers are filtered identically. The portable
Khronos layer is explicitly loaded with synchronization validation enabled.
Both controls take `Enter-RsfGodotValidation`; no second GPU test runs concurrently.
There are no raw image dumps or copied SDK payloads.

## Device evidence

The device is an NVIDIA GeForce RTX 5080. The loaded validation-layer SHA256 is
`E86FA4F70676A120F00A32BEAC6979CEB3CAFCE70DE42FA9107C89F587AB206E`.
The injected `RTSSHooks64.dll` SHA256 is
`68C496DEA7DED5B8766092F0159F4E9BA3947DF0829F594EB6981D2B06352AD5`.
Each receipt records executable identity, process ID, effective mitigation,
module paths/hashes, validation state, log paths and CPU interval samples.

| Editor/control | Effective extension-point disable | RTSSHooks64 | Present hazards |
| --- | ---: | --- | ---: |
| Official 4.7.2, normal | 0 | Loaded | 10 reported |
| Official 4.7.2, isolated | 1 | Absent | 0 |
| Custom 4.7.2, normal | 0 | Loaded | 10 reported |
| Custom 4.7.2, isolated | 1 | Absent | 0 |

The duplicate-message cap stops reporting after ten matching hazards. It does not
mean only ten affected presents occurred. Synchronization validation is confirmed
enabled in all four runs. The complete module inventory differs only by
`RTSSHooks64.dll` in each editor's pair; the Vulkan driver and validation layer
remain loaded with the same identities.

Official-editor receipts are under
`.local/godot/sync-controls/20261025-190503-545`. That pair still reports the
official engine's known null Win32 surface `HINSTANCE` VUID, so it is not blanket
core-validation acceptance. The custom-editor pair at
`20261025-190638-253` has the existing surface fix. Its isolated run reports
neither the present hazard nor a VUID. The custom editor SHA256 is
`E60D28D55689EF5DA3E28A98023032E21C6EB8C5264B5BF224EAF41AEB0034EF`.
This is the pinned Godot 4.7.2 source
`ed1daf0bf001b61586d9930840f2f1394092c079` with the previously recorded engine
integration patches; no engine was rebuilt for this experiment.

CPU intervals remain approximately 252 ms median in both official-editor runs.
The process mitigation therefore does not establish a correction for the separate
500 ms cadence seen in provider captures. Module polling and validation also make
these controls unsuitable for performance or latency claims.

## Source audit and use

`RenderingDeviceDriverVulkan::command_queue_execute_and_present` records the
engine command buffers, signals the acquired image's present semaphore in that
graphics submission, and passes that semaphore to Present. The swapchain render
pass supplies its final layout transition before that submission's signal.
Source review did not expose a missing engine/host ordering edge that explains
the cross-editor failure. The process controls supply the previously missing
runtime isolation; a source review alone would not justify blaming a layer.

`eng/test-godot-sync-isolation.ps1` reproduces the two minimal process controls.
`eng/godot-process-isolation.ps1` supplies the creation helper.
`eng/capture-godot.ps1 -DisableLegacyExtensionPoints` applies the same optional
policy to its owned foreground image-capture process and writes
`process-isolation.json` with effective policy and observed modules. Its default
launch behavior and immutable runtime cache are unchanged. These are diagnostic
launch controls, not automatic changes to a user's game or RTSS configuration.

The helper compiled through `Add-Type`, all three scripts passed PowerShell
parsing and `git diff --check`, and the official/custom process pairs exercised
actual Vulkan windows. This evidence covers the minimal native WSI path. Real
provider captures, secondary windows, exported players and other injected tools
retain their own validation scope.
