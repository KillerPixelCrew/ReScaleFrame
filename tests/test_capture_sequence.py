# SPDX-License-Identifier: GPL-3.0-only
import contextlib
import importlib.util
import io
import itertools
import json
from pathlib import Path
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("capture_sequence", Path(__file__).parents[1] / "tools/capture-sr-sequence.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class SequenceHarness(unittest.TestCase):
    def test_automatic_collection_and_failure_report(self):
        for produce in (True, False):
            with self.subTest(produce=produce), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                closed = []
                class Remote:
                    memory = 4096
                    def __init__(self, *args): pass
                    def status(self):
                        return SimpleNamespace(foreground=1, sr_enabled=1, sr_result=0,
                            sr_effective_backend=1, render_width=100, render_height=100,
                            output_width=200, output_height=200)
                    def write(self, offset, data):
                        assert struct.unpack("<4I", data) == (16, 1, 1, 1)
                    def allow_export(self, name): return name
                    def _call(self, *args):
                        if not produce: return
                        for source in range(1, 5):
                            bundle = root / f"scene-test-42-{source}"
                            bundle.mkdir()
                            (bundle / "sequence_capture_status.json").write_text("{}")
                            (bundle / "image.bin").write_bytes(b"pixels")
                            (bundle / "frame.json").write_text(json.dumps({"session_id": 1,
                                "source_frame_id": source, "camera_available": True}))
                            (bundle / "manifest.json").write_text(json.dumps({"status": "complete",
                                "session_id": 1, "source_frame_id": source, "matched_present": True,
                                "present_result": 0, "sr_result": 0, "images": [
                                {"name": name, "status": "captured", "raw": "image.bin"}
                                for name in ("sr_backend_output", "final_color")]}))
                    def close(self): closed.append(True)
                fake = SimpleNamespace(Remote=Remote, Image=lambda *args: None, struct=struct, plain=vars)
                original = runner.load
                def load(name, filename):
                    return fake if filename == "runtime-action.py" else original(name, filename)
                argv = ["capture", "--pid", "42", "--module-base", "0x1000", "--runtime", "unused",
                        "--sha256", "unused", "--captures", str(root), "--report", str(root / "report.json"),
                        "--timeout", "20"]
                with patch.object(sys, "argv", argv), patch.object(runner, "load", load), \
                     patch.object(runner.time, "sleep"), patch.object(runner.time, "monotonic", side_effect=itertools.count()), \
                     contextlib.redirect_stdout(io.StringIO()):
                    result = runner.main()
                report = json.loads((root / "report.json").read_text())
                self.assertEqual(result, 0 if produce else 2)
                self.assertEqual(report["status"], "captured" if produce else "failed")
                self.assertEqual(len(report["captures"]), 4 if produce else 0)
                self.assertEqual(report["visual_acceptance"], "not-established")
                self.assertEqual(closed, [True])


if __name__ == "__main__":
    unittest.main()
