"""Regenerate the repository C ABI bindings that stay committed, using bindgen-cli 0.72.1.

Every other binding is generated at build time by the build.rs of the crate that owns its header
(tools/bindgen). The outputs here stay committed, each for the reason given next to it. Pass
--check to compare them instead of writing; an output whose external input is not given is
skipped with a note.
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from committed_bindings import bindgen, emit, finish  # noqa: E402

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--minhook", type=Path, default=root / "build/windows-x64/_deps/minhook-src",
                    help="MinHook v1.3.4 source tree for the HDE64 records")
args = parser.parse_args()


def generate(output, header, types, constants, clang_args, extra=()):
    emit(root / output, bindgen(header, "--allowlist-type", types, "--allowlist-var", constants,
                                "--raw-line", "#![allow(non_camel_case_types)]", *extra,
                                clang=clang_args, cwd=root))


# runtime/vulkan/xess-fg/build.ps1 compiles session.rs, which includes these, with bare rustc.
generate("runtime/vulkan/xess-fg/bridge_abi.rs", "runtime/vulkan/xess-fg/bridge_api.h",
         "rsf_xess_bridge_(create|start|frame|present_status)|rsf_vk_xess_fg_frame", "RSF_XESS_BRIDGE_.*", [],
         ["--blocklist-type", "rsf_xess_bridge_legacy_reserved", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "type rsf_xess_bridge_legacy_reserved = ::core::mem::MaybeUninit<u32>;"])
generate("runtime/vulkan/xess-fg/session_records_abi.rs", "runtime/vulkan/xess-fg/session_records.h",
         "rsf_xess_(capture_desc|bridge_resource_desc|bridge_modules|bridge_shared_image_desc)",
         "RSF_XESS_BRIDGE_(PLATFORM_ABI_VERSION|ALLOCATOR_COUNT)", [],
         ["--with-derive-default", "--with-derive-custom-struct", "rsf_xess_bridge_resource_desc=PartialEq,Eq"])

# The fixtures built from these test-only headers (rsf-ac7 with its fixture features and the
# rsf-ac7-render-scope-fixture crate) include the committed records as modules.
generate("tests/ac7_render_scope_fixture_abi.rs", "tests/ac7_render_scope_fixture.h",
         "rsf_ac7_fixture_lease", "NOTHING", ["-Isdk/game/include", "-Igames/ac7/include"])
generate("tests/ac7_graph_replay_abi.rs", "tests/ac7_graph_replay.h",
         "rsf_ac7_graph_(particle_replay|check_fn|screen_fn|sky_(apply|restore)_fn)",
         "RSF_SCREEN_(UNKNOWN|FLIGHT|REPLAY)", ["-Isdk/game/include"])

# engine_sharpening.rs (tests/process) loads the Godot and Unity DLLs. Their owner crates cannot be
# linked there (their C imports would come along), so it keeps these two settings records; the
# shared records come from rsf_game_sys.
generate("tests/engine_sharpening_abi.rs", "integrations/unity/native/package_state.h",
         "rsf_(godot_settings|unity_package_settings)",
         "RSF_(GODOT_ABI_VERSION|GODOT_SETTINGS_ABI_VERSION|UNITY_SETTINGS_ABI_VERSION)",
         ["-include", "integrations/godot/native/api.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include", "-Iintegrations/unity/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use rsf_game_sys::rsf_sharpening_options;",
          "--with-derive-custom-struct", "rsf_(godot_settings|unity_package_settings)=PartialEq"])

# hde64.h comes from the MinHook checkout CMake fetches (third_party/minhook/minhook.lock.json pins v1.3.4).
hde = args.minhook / "src/hde/hde64.h"
if hde.exists():
    emit(root / "third_party/minhook/src/hde.rs", bindgen(
        hde, "--allowlist-type", "hde64s",
        "--allowlist-var", "^F_(MODRM|IMM(8|16|32|64)|DISP32|RELATIVE|ERROR|PREFIX_67)$",
        "--allowlist-function", "hde64_disasm", "--with-derive-default"))
else:
    print(f"skipped third_party/minhook/src/hde.rs: no {hde}")
finish()
