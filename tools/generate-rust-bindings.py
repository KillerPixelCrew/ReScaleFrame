"""Regenerate the Windows x64 C ABI bindings using bindgen-cli 0.72.1."""
import os
import subprocess
from pathlib import Path
from tempfile import NamedTemporaryFile

root = Path(__file__).resolve().parents[1]
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Install bindgen-cli 0.72.1 to regenerate the SDK bindings")

def generate(output, header, types, constants, clang_args, extra=()):
    bindings = subprocess.check_output([
        bindgen, header, "--allowlist-type", types, "--allowlist-var", constants,
        "--raw-line", "#![allow(non_camel_case_types)]",
        "--no-derive-debug", "--use-core", "--no-doc-comments", "--rust-edition", "2024",
        "--rust-target", "1.85", *extra, "--", "--target=x86_64-pc-windows-msvc", *clang_args,
    ], cwd=root).replace(b"\r\n", b"\n")
    output = root / output
    if not output.exists() or output.read_bytes() != bindings:
        with NamedTemporaryFile(dir=output.parent, delete=False) as temporary:
            temporary.write(bindings)
        Path(temporary.name).replace(output)


generate("sdk/game/src/lib.rs", "sdk/game/include/rescaleframe/game_api.h",
         "rsf_(get_game_plugin_api_fn|frame_record|game_cpu_source_event)", "RSF_.*", ["-Isdk/game/include"])
generate("runtime/orchestrator/src/render_abi.rs", "runtime/orchestrator/include/rescaleframe/frame_assembly.h",
         "rsf_(frame_resources|pipeline_camera_frame|dlss_frame|fg_choice|render_link|sr_session_setup|sr_session_status|sr_provider|fg_host|fg_session_status|generation_provider|render_settings|sharpening_options)",
         "RSF_(FRAME_ASSEMBLY_ABI_VERSION|DLSS_ABI_VERSION|FG_(BACKEND_(AUTO|ALL)|ABI_VERSION|FIXED|AUTO|DYNAMIC)|PACING_(NONE|REFLEX)|REFLEX_(OFF|BOOST)|RENDER_LINK_ABI_VERSION|SR_SESSION_ABI_VERSION|BACKEND_ABI_VERSION|API_D3D12|SR_(FSR2|XESS)|RENDER_FRONTEND_ABI_VERSION|SHARPENING_ABI_VERSION)",
         ["-include", "rescaleframe/fg_choice.h", "-include", "rescaleframe/render_links.h",
          "-include", "rescaleframe/render_frontend.h",
          "-include", "rescaleframe/sr_session.h", "-include", "rescaleframe/fg_session.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/backends/dlss/include", "-Isdk/game/include"],
         ["--blocklist-type", "rsf_(frame_record|frame_id|frame_phase|screen_class|camera_frame)",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;"])
generate("runtime/orchestrator/src/cpu_abi.rs", "runtime/orchestrator/include/rescaleframe/native_cpu.h",
         "rsf_native_cpu_(frame|reservation|reservation_counters)",
         "RSF_NATIVE_CPU_(RESERVATION_.*|DEPENDENCY_.*|PRESENT_.*)",
         ["-Iruntime/orchestrator/include", "-Isdk/game/include"])
generate("runtime/orchestrator/src/scene_abi.rs", "runtime/orchestrator/include/rescaleframe/native_window.h",
         "rsf_(native_present_record|observer_present_event|native_scene_identity)",
         "RSF_NATIVE_SCENE_PRESENT_.*",
         ["-include", "rescaleframe/native_scene.h", "-Iruntime/orchestrator/include",
          "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--with-derive-default"])
