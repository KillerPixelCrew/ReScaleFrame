"""Regenerate the Windows x64 plugin ABI using bindgen-cli 0.72.1."""
import os
import subprocess
from pathlib import Path
from tempfile import NamedTemporaryFile

sdk = Path(__file__).resolve().parent
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Install bindgen-cli 0.72.1 to regenerate the SDK bindings")
bindings = subprocess.check_output([
    bindgen, "include/rescaleframe/game_api.h",
    "--allowlist-type", "rsf_get_game_plugin_api_fn", "--allowlist-var", "RSF_.*",
    "--raw-line", "#![allow(non_camel_case_types)]",
    "--no-derive-debug", "--use-core", "--no-doc-comments", "--rust-edition", "2024",
    "--rust-target", "1.85",
    "--", "--target=x86_64-pc-windows-msvc", "-Iinclude",
], cwd=sdk).replace(b"\r\n", b"\n")
output = sdk / "src/lib.rs"
if not output.exists() or output.read_bytes() != bindings:
    with NamedTemporaryFile(dir=output.parent, delete=False) as temporary:
        temporary.write(bindings)
    Path(temporary.name).replace(output)
