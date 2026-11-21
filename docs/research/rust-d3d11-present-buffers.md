# D3D11 presentation buffer ownership in Rust

This source port starts at `cc2c622` and follows the [bridge cutover](rust-d3d11-present-cutover.md).
The question was whether the existing Rust owner could hold the application buffers and each
Present's backbuffer reference while keeping the facade's COM ABI and native resource operations.
The scope is the shared bridge, including its native D3D12 path.

| Path | Ownership and failure handling |
| --- | --- |
| Create | Rust admits up to eight buffers and builds one complete batch before publishing it. D3D11 uses the existing Rust shared-surface factory with the native D3D11 and D3D12 devices, render-target view and the application's UAV usage. Native D3D12 with switching uses committed render targets on the application device in `PRESENT` state. Strict native D3D12 uses the physical chain and allocates no virtual buffers. The shared upload fence and command slots follow their prior creation order. |
| `GetBuffer` | Rust checks the published batch and holds a reference across the native `QueryInterface`. The caller owns the returned interface and can query virtual buffers during scoped creation before the physical chain exists. Strict native D3D12 forwards to the physical chain after the existing admission check. A refused lookup clears the output and physical initialization failures retain the existing four-report limit. |
| Copy and Present | D3D11 copies the D3D12 peer of slot zero from `COMMON`. Switching native D3D12 uses the current application slot from `PRESENT`. Strict native D3D12 performs no app-buffer copy. The native leaf acquires the current physical backbuffer from one chain reference. Rust adopts its COM reference for the frame, including submission, the SDK's borrowed prepare pointer and the retire callback. Native D3D12 releases it after the slot signal. D3D11 releases it before that signal, as before. Failed acquisition skips copy and callbacks and releases any reference the leaf returned. `DXGI_PRESENT_TEST` acquires no buffer. |
| Resize | Rust retains the previous batch if the drain, admission or physical resize fails. It checks application references using the existing native COM count probe. After physical success the old batch is released before allocating replacements. A zero count keeps the application count. D3D11 uses three physical buffers and switching native D3D12 keeps its existing physical count. A complete replacement commits the application description and resets the application index. |
| Drain and retire | Fullscreen and provider switching drain work without replacing the application batch. The provider retire service still joins vendor work on its queue. Successful destruction releases shared surfaces before the shared fence and physical chain. Direct render buffers follow provider and host teardown. On drain failure Rust deliberately retains the published batch, including its COM references and shared NT handles, alongside the native resources already retained by the facade. The Present-scoped reference ends before a later switch or destruction. |

In the old native D3D12 path `make_render_buffers` returned early after writing directly to the
facade array. A failure after slot zero succeeded left a prefix of replacement textures visible
through `GetBuffer` after failed resize, even though the facade refused further Presents. Rust
drops the staged prefix and any reference returned on failure, faults the facade after a failed
replacement, and leaves its last committed application description. Synthetic tests cover partial
creation and resize, prior-buffer retention on resize refusal, new count admission, application
and SDK reference lifetimes, shared-surface rollback, and deliberate retention after drain refusal.
The canonical COM identity leaf and post-lookup unpublication check from `cc2c622` remain in use.

## Verification

The build and verification use separate baseline and candidate CMake and Cargo target directories.
Both were configured for MSVC Release with Streamline enabled and Rust 1.94.1. The first baseline
configure hit a yyjson download hash mismatch. Both source trees then used copies of the three
local inputs verified against the repository's lock file.

- The public presentation, Runtime and game SDK headers match the baseline byte for byte.
- The independent PE comparison matches case-sensitive export names, ordinals and forwarding
  entries against the rebuilt `cc2c622` artifacts. Counts are Runtime (381), AC7 carrier (256),
  Loader and its version alias (273 each), thin carrier (6), Minecraft (267), Unity Native (275)
  and Godot Native (398).
- Locked workspace Cargo tests passed (377 total, 234 in the orchestrator), along with formatting
  and workspace Clippy with warnings denied.
- All 21 focused MSVC CPU and ABI contracts passed, including the C++ and C11 presentation and
  FG DLL contracts and the embedded FG12 and renderer frontends. The D3D11 and shared FG
  presentation fixtures and standalone shared-surface fixture linked successfully.

Device and vendor execution, real DXGI resize and fullscreen transitions, and in-game presentation
remain unverified. The device fixtures and full release verification suite were outside this run.
