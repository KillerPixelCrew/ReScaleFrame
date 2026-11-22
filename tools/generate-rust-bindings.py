"""Regenerate the repository C ABI bindings that stay committed, using bindgen-cli 0.72.1.

Every other binding is generated at build time by the build.rs of the crate that owns its header
(tools/bindgen). The outputs here stay committed, each for the reason given next to it. Pass
--check to compare them instead of writing; an output whose external input is not given is
skipped with a note.
"""
import argparse
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from committed_bindings import emit, finish  # noqa: E402

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--vulkan-sdk", type=Path, default=os.environ.get("VULKAN_SDK"),
                    help="Vulkan SDK root for tests/vulkan-fg/src/wsi_abi.rs (default: VULKAN_SDK)")
parser.add_argument("--minhook", type=Path, default=root / "build/windows-x64/_deps/minhook-src",
                    help="MinHook v1.3.4 source tree for the HDE64 records")
args = parser.parse_args()
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Install bindgen-cli 0.72.1 to regenerate the committed bindings")


def generate(output, header, types, constants, clang_args, extra=()):
    emit(root / output, subprocess.check_output([
        bindgen, str(header), "--allowlist-type", types, "--allowlist-var", constants,
        "--raw-line", "#![allow(non_camel_case_types)]",
        "--no-derive-debug", "--use-core", "--no-doc-comments", "--rust-edition", "2024",
        "--rust-target", "1.85", *extra, "--", "--target=x86_64-pc-windows-msvc", *clang_args,
    ], cwd=root))


# runtime/vulkan/xess-fg/build.ps1 compiles session.rs, which includes these, with bare rustc.
generate("runtime/vulkan/xess-fg/bridge_abi.rs", "runtime/vulkan/xess-fg/bridge_api.h",
         "rsf_xess_bridge_(create|start|frame|present_status)|rsf_vk_xess_fg_frame", "RSF_XESS_BRIDGE_.*", [],
         ["--blocklist-type", "rsf_xess_bridge_legacy_reserved", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "type rsf_xess_bridge_legacy_reserved = ::core::mem::MaybeUninit<u32>;"])
generate("runtime/vulkan/xess-fg/session_records_abi.rs", "runtime/vulkan/xess-fg/session_records.h",
         "rsf_xess_(capture_desc|bridge_resource_desc|bridge_modules|bridge_shared_image_desc)",
         "RSF_XESS_BRIDGE_(PLATFORM_ABI_VERSION|ALLOCATOR_COUNT)", [],
         ["--with-derive-default", "--with-derive-custom-struct", "rsf_xess_bridge_resource_desc=PartialEq,Eq"])

# tests/CMakeLists.txt compiles these test-only headers' fixtures with bare rustc.
generate("tests/ac7_render_scope_fixture_abi.rs", "tests/ac7_render_scope_fixture.h",
         "rsf_ac7_fixture_lease", "NOTHING", ["-Isdk/game/include", "-Igames/ac7/include"])
generate("tests/ac7_graph_replay_abi.rs", "tests/ac7_graph_replay.h",
         "rsf_ac7_graph_(particle_replay|check_fn|screen_fn|sky_(apply|restore)_fn)",
         "RSF_SCREEN_(UNKNOWN|FLIGHT|REPLAY)", ["-Isdk/game/include"])

# tests/CMakeLists.txt links engine_sharpening.rs into an executable with bare rustc. The Unity
# and Godot owner rlibs cannot be linked there (their C imports would come along), so it keeps
# these two settings records; the shared records come from rsf_game_sys.
generate("tests/engine_sharpening_abi.rs", "integrations/unity/native/package_state.h",
         "rsf_(godot_settings|unity_package_settings)",
         "RSF_(GODOT_ABI_VERSION|GODOT_SETTINGS_ABI_VERSION|UNITY_SETTINGS_ABI_VERSION)",
         ["-include", "integrations/godot/native/api.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include", "-Igames/unity-mono/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use rsf_game_sys::rsf_sharpening_options;",
          "--with-derive-custom-struct", "rsf_(godot_settings|unity_package_settings)=PartialEq"])

# wsi_abi.h includes the Vulkan SDK headers, which the repository does not carry.
if args.vulkan_sdk:
    generate("tests/vulkan-fg/src/wsi_abi.rs", "tests/vulkan-fg/wsi_abi.h",
             "rsf_ffx_wsi_dispatch", "NOTHING",
             ["-x", "c++", "-std=c++17", "-Wno-pragma-once-outside-header", f"-I{args.vulkan_sdk / 'Include'}"],
             ["--allowlist-function", "(rsf_fixture_sdk_wsi_.*|rsfFfxSetVulkanWsiDispatch)",
              "--no-recursive-allowlist", "--blocklist-type", "Vk.*|PFN_vk.*",
              "--raw-line", "#![allow(non_snake_case)]",
              "--raw-line", "use ash::vk::{\n"
                            "    self, Device as VkDevice, Queue as VkQueue, Result as VkResult, SwapchainKHR as VkSwapchainKHR,\n};",
              "--raw-line", "type PFN_vkCreateSwapchainKHR = Option<vk::PFN_vkCreateSwapchainKHR>;",
              "--raw-line", "type PFN_vkDestroySwapchainKHR = Option<vk::PFN_vkDestroySwapchainKHR>;",
              "--raw-line", "type PFN_vkQueuePresentKHR = Option<vk::PFN_vkQueuePresentKHR>;",
              "--raw-line", "type PFN_vkAcquireNextImageKHR = Option<vk::PFN_vkAcquireNextImageKHR>;",
              "--raw-line", "type PFN_vkGetSwapchainImagesKHR = Option<vk::PFN_vkGetSwapchainImagesKHR>;"])
else:
    print("skipped tests/vulkan-fg/src/wsi_abi.rs: pass --vulkan-sdk or set VULKAN_SDK")

# hde64.h comes from the MinHook checkout CMake fetches (CMakeLists.txt pins v1.3.4).
hde = args.minhook / "src/hde/hde64.h"
if hde.exists():
    emit(root / "runtime/orchestrator/src/d3d11_observer_hooks/hde_abi.rs", subprocess.check_output([
        bindgen, str(hde), "--allowlist-type", "hde64s",
        "--allowlist-var", "^F_(MODRM|IMM(8|16|32|64)|DISP32|RELATIVE|ERROR|PREFIX_67)$",
        "--allowlist-function", "hde64_disasm", "--with-derive-default", "--no-derive-debug",
        "--no-doc-comments", "--use-core", "--rust-edition", "2024", "--rust-target", "1.85",
        "--", "--target=x86_64-pc-windows-msvc",
    ]))
else:
    print(f"skipped runtime/orchestrator/src/d3d11_observer_hooks/hde_abi.rs: no {hde}")
finish()
