# Vulkan and DX12 presentation options

Updated 5 September 2026 after inspecting Community Shaders' Vulkan branch and its pinned Streamline fork.

## The XeSS distinction

**XeSS supports Vulkan.** The current native Vulkan API covers XeSS Super Resolution. Intel's published XeSS 3 matrix lists FG/MFG and XeLL under DX12, with SR alone under Vulkan. The SDK includes `xess_vk.h` and a Vulkan SR sample; its FG and XeLL graphics-specific headers are `xefg_swapchain_d3d12.h` and `xell_d3d12.h`.

| Native SDK feature | DX11 | DX12 | Vulkan |
| --- | --- | --- | --- |
| XeSS-SR | Supported Intel devices | Supported devices | Supported devices |
| XeSS-FG/MFG | No | Supported devices; higher interpolation counts are hardware-dependent | No current native API |
| XeLL | No | Supported devices and configurations | No current native API |

Sources: [Intel's current support matrix](https://www.intel.com/content/www/us/en/developer/topic-technology/gamedev/xess.html), [SR Vulkan header](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/inc/xess/xess_vk.h), and [Vulkan FG/XeLL request #60](https://github.com/intel/xess/issues/60). The issue contains an Intel acknowledgement, but no delivery date in the inspected discussion. An absent native API does not mean a mixed Vulkan/DX12 application is impossible.

## What Community Shaders is actually changing

[PR #2632](https://github.com/community-shaders/skyrim-community-shaders/pull/2632) targets its Vulkan migration. The inspected upstream branch is `codex/vulkan-clean` at `eeb28dda597d220b54e65b4a1007fbd638b5e63a`. The related cleanup proposal, [PR #2670](https://github.com/community-shaders/skyrim-community-shaders/pull/2670), describes a DXVK fork exposing Vulkan device/resources, command submission, synchronization, and FG presentation hooks. These PRs were open when inspected; they are not evidence of a completed release.

This is a translation of Skyrim's DX11 rendering into Vulkan with custom integration points, not merely a different final swap-chain object. When SR runs on the same underlying Vulkan device, it can operate on translated resources without the old native DX11/DX12 SR round trip. The synchronization and lifetime work still exists, as illustrated by [their asynchronous interop fixes](https://github.com/community-shaders/skyrim-community-shaders/pull/2662).

The branch pins `doodlum/Streamline` at `579442cb54615dddb8f34a78def6f5ca7d4ec659`, rather than NVIDIA's unmodified upstream SDK. That fork adds `sl.xess`, `sl.fsr`, and `sl.fsr_g`. Its `sl.xess` implementation records `xessVKExecute` through Streamline; the public header describes the feature as upscaling only. This is useful prior art for a shared vendor interface. It does not supply native Vulkan XeSS MFG.

This also qualifies the earlier Streamline conclusion: **stock NVIDIA Streamline and Community Shaders' extended Streamline are different integration surfaces**. The custom fork is a candidate backend dependency, subject to compatibility and maintenance review. It should not dictate the public Game SDK.

For [XeSS PR #2704](https://github.com/community-shaders/skyrim-community-shaders/pull/2704), the [maintainer's stated reason](https://github.com/community-shaders/skyrim-community-shaders/pull/2704#issuecomment-5545322652) was that the Vulkan update would break the DX11/DX12 implementation, with XeSS support planned in the update. This is a project migration decision, not a claim that native DX12 interop cannot work.

The current PR description also reports testing SR quality modes and XeSS FG/multiplier/UI settings on an Arc 140V in SDR. This is author-reported evidence for the hybrid approach; AC7 compatibility and independently measured performance remain unproven.

## Options for AC7

| Approach | Main benefit | Cost or limitation for our target |
| --- | --- | --- |
| Native DX11 rendering/SR with DX12 presentation | Retains AC7's renderer and the Claw's native DX11 XeSS path; uses Intel's native MFG API | Requires DX11/DX12 sharing, queue ordering, a swap-chain facade, and mixed-API XeLL validation |
| DXVK rendering with native Vulkan SR/presentation | Shared underlying Vulkan resources; a basis for native Vulkan vendor features and broader platform work | Translates the game's whole DX11 workload; needs suitable DXVK integration; current XeSS MFG/XeLL cannot remain Vulkan-only |
| DXVK/Vulkan SR with DX12 MFG presentation | Allows Vulkan rendering while retaining Intel MFG | Requires an additional Vulkan/DX12 interop path, compatible shared formats/handles, GPU identity matching, and explicit cross-API synchronization/presentation ownership |
| Native DX11 with only a Vulkan processing/presentation bridge | Could avoid translating the entire game | Still needs cross-API resource sharing; does not gain all same-device DXVK advantages or native XeSS MFG |

Vulkan is not inherently slower or faster here. A DXVK path may change CPU overhead, shader compilation behavior, driver scheduling, memory use, and power consumption. Those can offset or exceed bridge costs, in either direction. API names cannot establish the winner.

For **the first Windows AC7/Claw SR+MFG target**, retain native DX11 SR plus DX12 MFG as the initial implementation. Keep renderer interop and presentation as replaceable orchestrator components. The Game SDK describes resource API, identity, conventions, and lifetime so a Vulkan backend does not require rewriting the game plugin's semantic contract.

Before choosing Vulkan as the default, benchmark a controlled DXVK variant against the native path at matched resolution, quality, power, and refresh settings. If Vulkan improves the actual workload enough to justify the additional MFG bridge, it becomes a viable second path. Both approaches still require the AC7 plugin to provide correct depth, motion, jitter, camera resets, HUD boundaries, and frame IDs.

No AC7 Vulkan/DX12 comparison was run during this research. No backend was installed into the game.
