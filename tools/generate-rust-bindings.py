"""Regenerate the Windows x64 C ABI bindings using bindgen-cli 0.72.1."""
import os
import subprocess
from pathlib import Path
from tempfile import NamedTemporaryFile

root = Path(__file__).resolve().parents[1]
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Install bindgen-cli 0.72.1 to regenerate the SDK bindings")

def generate(output, header, types, constants, clang_args):
    bindings = subprocess.check_output([
        bindgen, header, "--allowlist-type", types, "--allowlist-var", constants,
        "--raw-line", "#![allow(non_camel_case_types)]",
        "--no-derive-debug", "--use-core", "--no-doc-comments", "--rust-edition", "2024",
        "--rust-target", "1.85", "--", "--target=x86_64-pc-windows-msvc", *clang_args,
    ], cwd=root).replace(b"\r\n", b"\n")
    output = root / output
    if not output.exists() or output.read_bytes() != bindings:
        with NamedTemporaryFile(dir=output.parent, delete=False) as temporary:
            temporary.write(bindings)
        Path(temporary.name).replace(output)


generate("sdk/game/src/lib.rs", "sdk/game/include/rescaleframe/game_api.h",
         "rsf_get_game_plugin_api_fn", "RSF_.*", ["-Isdk/game/include"])
generate("runtime/orchestrator/src/render_abi.rs", "runtime/orchestrator/include/rescaleframe/frame_assembly.h",
         "rsf_(frame_resources|pipeline_camera_frame|dlss_frame|fg_choice)",
         "RSF_(FRAME_ASSEMBLY_ABI_VERSION|DLSS_ABI_VERSION|FG_BACKEND_(AUTO|ALL))",
         ["-include", "rescaleframe/fg_choice.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/backends/dlss/include", "-Isdk/game/include"])
