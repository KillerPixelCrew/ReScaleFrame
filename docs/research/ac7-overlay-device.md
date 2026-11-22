# AC7 overlay device mismatch

7 September 2026. Investigated from RSF `c82772d` and the user's failing game run, then
reproduced with the unchanged host in a synthetic process. Game identity: `Ace7Game.exe`,
Steam build 9855922, recorded SHA-256
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`.
The game session used D3D11 through Proton/DXVK and RenderDoc, at 2048x1152.

## Question and evidence

Why did the overlay's `CreateRenderTargetView` fail before returning while the earlier,
game-tested `present_blit` path could make a back-buffer view at the same Present boundary?
The handoff had already ruled out concurrent overlay frames, stored versus freshly queried
context devices, GetBuffer itself, and a hook on the view-creation slot. Drawing with no
target returned success but produced no visible panel. These findings were accepted.

The next step was to inspect all wrappers' logs and reproduce the caller independently.
Another per-launch timing experiment would not establish device/resource ownership.
The existing `.local/observe/rsf-dump.log` ended at view creation using device `0x672970`.
In the matching `RenderDoc_app_2026.09.07_12.01.35.log`:

- At 12:01:38, RenderDoc registers devices `0x672970` and `0x672330`.
- The game window `0x200aa`, also named in the overlay input log, belongs to the frame
  capturer for `0x672330`.
- At 12:01:42, RenderDoc reports `Assertion failed: 'parent'` in
  `d3d11_device_wrap.cpp(1153)`.

These addresses identify objects only within that run. They are not signatures or hook sites.
The RenderDoc log identifies v1.45, revision `2fc0bc04cb95499635f63986a55bc6f67849dd9f`.
Its [view creation implementation](https://github.com/baldurk/renderdoc/blob/2fc0bc04cb95499635f63986a55bc6f67849dd9f/renderdoc/driver/d3d11/d3d11_device_wrap.cpp)
looks up a resource's parent record in the receiving device's resource manager. A resource
from another device can reach a missing parent after the underlying view call succeeds.
The web-rendered source line numbering differs from the binary log, so the parent lookup
and surrounding method, rather than that line number alone, are the source evidence.

`hooked_create_buffer` selects the first device that creates a constant buffer whenever
buffer observation is enabled, even when that buffer is outside the retained size range.
`hooked_present` only fills the device if none has been selected. The observer can therefore
retain a helper device before the game device presents. Querying its context's device again
returns the same wrong device. The overlay's atlas and shaders can work on that device;
its first back-buffer view introduces a resource from the presenting device.

## Reproduction and change

An untracked baseline harness loaded the same release egui DLL and RenderDoc DLL, installed
the observer, created a helper device with a 16-byte constant buffer, and then created a
second device with an 800x600 flip-discard swap chain. It called the original host through
the real observer Present callback using the observer's device/context, without any backend.
The observer and presenting device differed. The original host stopped at the same
view-creation step with exception `0xc0000005`. Logs and the baseline harness are in
`.local/overlay-diagnosis/`, notably `repro-two-devices.log`.

The host now selects its renderer device through the presenting swap chain. Its private API
no longer accepts an observer-selected device or context. Each visible Present:

1. Checks that the chain belongs to the renderer device by canonical COM `IUnknown` identity.
2. Gets that device's immediate context and the current back buffer.
3. Checks the buffer's owner before creating its view, and uses the buffer's pixel dimensions.
4. Binds the view, draws, restores the old targets, and releases all frame references before
   forwarding Present. Missing views close the panel rather than drawing into no target.

The first four frames log the chain, device, context, buffer, owner, dimensions, format,
bind flags, view HRESULT, and draw result. The unfinished worker-thread target declaration
was discarded: moving this invalid device/resource pairing to another thread does not fix it.

The earlier cache also retained a back-buffer texture and view indefinitely. Those references
are now scoped to the callback, matching `present_blit`. The earlier general claim that all
back-buffer references are forbidden across a flip-model Present was too strong: the
documented requirement is to release direct and indirect references before
[ResizeBuffers](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgiswapchain-resizebuffers).
Per-Present acquisition and release avoids needing a resize hook here.

## Validation and limits

`tests/overlay_host.cpp` creates the helper-device selection deliberately, then uses the
actual host and observer callback with no backend and no target initially bound. A native
panel fixture supplies known triangles. GPU readback must find colored pixels, target state
must be restored to null, and ResizeBuffers must succeed while the host remains active.
The test accepts a real panel DLL path for the same checks with egui. Optional
`RSF_TEST_RENDERDOC_DLL` loads capture support before installing the observer.

The patched test also passed with the actual release egui DLL, RenderDoc v1.45, and Proton
Experimental's DXVK `v3.1-1-gadeda6639a09ad1`, under system Wine. Fifteen frames after the
initial empty layout each changed 200,576 pixels, at 800x600 and then 960x720. Both resize
checks passed, on an NVIDIA GeForce RTX 4070 Laptop GPU with driver 610.57.4. This is
synthetic rendering/readback evidence, not an AC7 run or MSVC/Windows
validation. The deployed panel DLL's SHA-256 is
`15aab38203d869b0f07d7835da4660e53e4a34e38157a30cc23cf669e11cac75`.
The log is `.local/overlay-diagnosis/fixed-egui-renderdoc-dxvk.log`.

The native cross-build, all 15 Wine tests, version alignment, Cargo formatting, and Clippy
passed. The focused overlay test also passed after the final path-validation edit.
`eng/verify.ps1` could not execute because PowerShell is absent;
MSVC remains untested. No Rust behavior changed.

Deployed `dinput8.dll` SHA-256:
`36c13ef76f9a8eaf197dcdc0ee2f68a786a5fe81e7dbdd99232fbd5a1c889b1b`.
The replaced DLL is backed up at
`.local/overlay-diagnosis/backups/20260907-121613/dinput8.dll`; its SHA-256 is
`5748b2922b95605ba16f673b1b3ed9e7b66c248d4fe1a947331e9bd6a1ddf9ef`.

The earlier F7 success establishes that its own run had a usable device/resource pairing.
It does not establish that F8 repairs the observer's selection, and this experiment did not
test that hypothesis. The backend still consumes the observer-selected device; broader
multi-device observer selection is separate work. The next AC7 acceptance check is to open
F5 before F8, observe visible panel pixels and continued presentation, then close/reopen it.
The patch is not yet game-validated.
