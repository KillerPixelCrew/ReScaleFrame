"""Generate the core-only driver packet and frame ABI for the SCons engine module.

Stays committed: SCons and the CMake graph fixture compile the module with bare rustc, for several
Windows targets, so neither build script nor crate dependency can supply these bindings. Pass
--check to compare the committed outputs instead of writing them.
"""
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from committed_bindings import emit, finish  # noqa: E402

root = Path(__file__).resolve().parents[1]
module = Path("integrations/godot/modules/rescaleframe")
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Use bindgen-cli 0.72.1")


def generate(output, header, types, constants, extra, clang_args=()):
    emit(root / module / output, subprocess.check_output([
        bindgen, str(header), "--allowlist-type", types, "--allowlist-var", constants, *extra,
        "--use-core", "--no-doc-comments", "--no-derive-debug", "--rust-edition", "2024",
        "--rust-target", "1.85", "--", "--target=x86_64-pc-windows-msvc", *clang_args,
    ], cwd=root))


frame_types = "rsf_(frame_record|frame_id|frame_phase|screen_class|camera_frame)"
generate("frame_abi.rs", "sdk/game/include/rescaleframe/game_frame.h", frame_types,
         "RSF_(GAME_(FRAME|CAMERA)_ABI_VERSION|FRAME_(FLAG|SOURCE)_.*)",
         ["--raw-line", "#![allow(non_camel_case_types)]", "--raw-line", "#![allow(dead_code)]"])
generate("frame_event_abi.rs", "sdk/game/include/rescaleframe/game_api.h", "rsf_game_cpu_event",
         "RSF_(GAME_CPU_(FRAME_BEGIN|INPUT_SAMPLE|SIMULATION_BEGIN|"
         "SIMULATION_END|FRAME_END|INPUT_EVENT|EVENT_VERSION|INPUT_KEYED|INPUT_UNFRAMED)|"
         "GAME_RENDER_(SR|FINAL_SCENE|RESET|PRIMARY))",
         ["--blocklist-type", frame_types, "--raw-line", "#![allow(non_camel_case_types)]"],
         ["-Isdk/game/include"])
generate("packet_abi.rs", module / "packet.h",
         "rsf_(godot_(packet_sources|driver_packet|packet_calls|swapchain_calls|swapchain_release_calls|"
         "present_calls|graph_calls|graph_split_fn)|fgvk_setup)",
         "RSF_((GODOT|FGVK|SRVK)_ABI_VERSION|GODOT_VK_HANDOFF_ABI_VERSION|GAME_RENDER_(PRESENT|SUBMISSION))",
         ["--blocklist-type", "rsf_camera_frame",
          "--raw-line", "#![allow(non_camel_case_types, dead_code)]",
          "--raw-line", "use crate::frame_abi::rsf_camera_frame;",
          "--with-derive-default",
          # The C ABI is also consumed by the engine module's other Windows architectures.
          "--no-layout-tests"],
         ["-Iintegrations/godot/native", "-Isdk/game/include", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include"])
finish()
