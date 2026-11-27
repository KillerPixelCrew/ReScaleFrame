"""Write, or with --check compare, a binding that stays committed.

Most C ABI bindings are generated at build time by their owner crate's build.rs (tools/bindgen).
The generators that import this module produce the few that cannot be: their header lives in a
gitignored SDK checkout, or a build without Cargo compiles them. `--check` regenerates every
output in memory and fails when a committed file differs.
"""
import os
import subprocess
import sys
from pathlib import Path

BINDGEN_VERSION = "bindgen 0.72.1"
COMMON = ["--no-derive-debug", "--no-doc-comments", "--use-core", "--rust-edition", "2024", "--rust-target", "1.85"]
CHECK = "--check" in sys.argv
if CHECK:
    sys.argv.remove("--check")
_stale = []


def bindgen(header, *flags, clang=(), cwd=None):
    """Run bindgen-cli on header with the flags every committed binding shares; returns its output."""
    exe = os.environ.get("BINDGEN", "bindgen")
    if subprocess.check_output([exe, "--version"], text=True).strip() != BINDGEN_VERSION:
        raise SystemExit(f"Install {BINDGEN_VERSION} to regenerate the committed bindings")
    return subprocess.check_output(
        [exe, str(header), *COMMON, *flags, "--", "--target=x86_64-pc-windows-msvc", *clang], cwd=cwd)


def emit(path, data):
    """Write `data` (bindgen output) to `path`, or record it as stale under --check."""
    path = Path(path)
    data = data.replace(b"\r\n", b"\n")
    if CHECK:
        if not path.exists() or path.read_bytes().replace(b"\r\n", b"\n") != data:
            _stale.append(path)
    elif not path.exists() or path.read_bytes() != data:
        path.write_bytes(data)


def finish():
    """Exit non-zero when --check found a stale committed binding."""
    if _stale:
        raise SystemExit("Stale committed bindings; regenerate without --check:\n"
                         + "\n".join(f"  {path}" for path in _stale))
