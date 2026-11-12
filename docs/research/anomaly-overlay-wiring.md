# Anomaly shared-overlay wiring

Status: built and contract-tested with fakes, a WARP swap chain and the system DirectInput devices.
**Not game-tested.** SR and FG are not wired; rendering-readiness flags stay 0.

## Question

How does the Anomaly adapter show the Runtime's shared overlay at native output resolution, keep the
game from acting on mouse and keyboard while the panel is open, and report SR/FG honestly, without
engine policy entering the Runtime?

## Approach and evidence

- Present boundary. The engine presents with `IDXGISwapChain::Present` from `IRenderDeviceRender::End`
  (`docs/research/anomaly-native-hooks.md`, RVA `0x822eb0`), after `CHUDManager::RenderUI`. The adapter
  hooks the shared DXGI swap-chain class `Present` (and `Present1`, `ResizeBuffers`, `ResizeTarget`) from
  the vtable of a WARP dummy chain, so no new engine RVA is needed and the observer's guarded sites are
  not required for `Overlay=1`. Calling `api.present` there draws after the last UI draw, before the
  frame leaves the process. `DXGI_PRESENT_TEST` and chains on other windows are ignored. The window
  filter is the engine class `_XRAY_1.5` (`xrEngine/Device_Initialize.cpp`, read-only reference source).
- Input boundary. `xrEngine/xr_input.cpp` polls buffered `GetDeviceData` on both the mouse and the
  keyboard, calls `Acquire` on `DIERR_INPUTLOST`/`DIERR_NOTACQUIRED`, and keeps its own key table from the
  events, with `DISCL_EXCLUSIVE` for the mouse when it is exclusive. The adapter probes the system
  `dinput8.dll` the carrier forwards to, hooks the device `Acquire`, `Unacquire`, `GetDeviceState`,
  `GetDeviceData`, `SetCooperativeLevel` and `IDirectInput8::CreateDevice` (A and W classes), and
  registers `GUID_SysMouse`/`GUID_SysKeyboard` devices as they are created. COM slot numbers are
  pinned by the gate contract against a real `IDirectInputDevice8A` vtable.
- Gate policy (`games/anomaly/src/overlay_input_gate.*`). Closed panel: untouched pass-through.
  Captured: idle state and no events, with the real buffer drained so nothing piles up; buffers are
  flushed on both edges; keys and buttons the engine saw pressed get synthetic releases when capture
  begins, so its key table does not stick; an exclusive mouse is unacquired for the duration and
  reacquired on the falling edge if the engine still wanted it, and the engine's own `Acquire` is
  answered without acquiring; genuine device loss still reaches the engine.
- Insert normally arrives as a window message in the Runtime's input layer. The adapter also toggles on
  its own foreground `GetAsyncKeyState(VK_INSERT)` edge (`OverlayKeyPoll=0` disables it); the Runtime's
  250 ms debounce makes the two paths safe together.
- Reset. `ResizeBuffers`/`ResizeTarget` notify `RESIZE` before forwarding. A different chain or device
  than the panel drew with notifies `DEVICE_RESET` before the next draw (the host holds a device
  reference, so the old device stays valid until it is released).

## What the panel reports

`sr_backend_choices=0` (SR unavailable), `fg_backend_choices=0`, latency Off only, one generated frame,
no DLSS preset. A zero SR mask is valid in the generic overlay API: the panel offers no SR backend,
`settings_apply` refuses `sr_enabled=1` and any SR backend/quality change, saved SR keys fall back to the
defaults, and status says "Super resolution is not available in this adapter". The panel's "Enable
upscaling" checkbox is still drawn (the Rust panel has no hidden state for it); a click is refused and
the panel shows its refused-configuration line. An earlier revision offered a placeholder DLSS choice;
that was untruthful and was removed.

## Configuration

`ReScaleFrame.ini`, section `[Anomaly]`: `Overlay=1` (independent of `Mode=Observe`),
`OverlayKeyPoll=1|0`. The shared render keys (`SR`, `SRBackend`, `SRQuality`, `FG`, `FGBackend`,
`FGGeneratedFrames`, `Latency`, `FrameLimitUs`, `StatusOverlay`, `DLSSPreset`) are written by the
Runtime into the same section. Log: `ReScaleFrame.Anomaly.log` beside the INI.

## Uncertainty

Not exercised in the game: real exclusive-mouse behaviour under the engine, WM_MOUSE delivery after the
unacquire, Insert delivery, and composition on the engine's actual swap chain. Stale gate records are
possible if the engine frees a device and a different non-mouse object reuses its address (the engine
creates its input once). Hooks and trampolines live for the process.
