"""Generate the core-only driver packet ABI for the SCons engine module."""
import os
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[1]
module = Path("integrations/godot/modules/rescaleframe")
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Use bindgen-cli 0.72.1")

bindings = subprocess.check_output([
    bindgen, str(module / "packet.h"),
    "--allowlist-type", "rsf_(godot_(packet_sources|driver_packet|packet_calls|swapchain_calls|present_calls|graph_calls|graph_split_fn)|fgvk_setup)",
    "--allowlist-var", "RSF_((GODOT|FGVK|SRVK)_ABI_VERSION|GODOT_CHAIN_.*|GODOT_VK_HANDOFF_ABI_VERSION)",
    "--blocklist-type", "rsf_camera_frame",
    "--raw-line", "#![allow(non_camel_case_types, dead_code)]",
    "--raw-line", "use crate::frame_abi::rsf_camera_frame;",
    "--use-core", "--no-doc-comments", "--no-derive-debug", "--with-derive-default",
    # The C ABI is also consumed by the engine module's other Windows architectures.
    "--no-layout-tests", "--rust-edition", "2024", "--rust-target", "1.85",
    "--", "--target=x86_64-pc-windows-msvc", "-Iintegrations/godot/native",
    "-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
], cwd=root)
(root / module / "packet_abi.rs").write_bytes(bindings.replace(b"\r\n", b"\n"))
