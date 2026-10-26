"""CPU checks against the pinned local SDK source; no SDK build or GPU use."""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
FIXES = ROOT / "runtime/vulkan/streamline/sdk-fixes"
SDK = ROOT / "references/godot/streamline"
SPEC = importlib.util.spec_from_file_location("selected_device", FIXES / "selected_device.py")
PATCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PATCH)


@unittest.skipUnless((SDK / PATCH.WRAPPER).is_file(), "Pinned Streamline reference source required")
class SelectedDevicePatch(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="rsf-streamline-selected-")
        self.addCleanup(self.temporary.cleanup)
        self.source = Path(self.temporary.name)
        for name in (
            PATCH.WRAPPER, "source/platforms/sl.chi/vulkan.cpp",
            "source/platforms/sl.chi/vknvll2.cpp", "source/plugins/sl.common/commonEntry.cpp",
            "source/core/sl.security/secureLoadLibrary.cpp",
        ):
            target = self.source / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(SDK / name, target)

    def test_guard_precedes_all_device_requirements(self):
        receipt = PATCH.apply(self.source)
        text = (self.source / PATCH.WRAPPER).read_text(encoding="utf-8")
        device = text[text.index("    VkResult VKAPI_CALL vkCreateDevice("):]
        guard = device.index("if (!rsf::streamline::deviceConfigApplies(")
        for key in ("1.2_features", "1.3_features", "extensions", "optional_extensions",
                    "queues/graphics/count", "queues/compute/count", "opticalflowNV_features",
                    "queues/opticalflow/count"):
            self.assertLess(guard, device.index(f'/external/vk/device/{key}'))
        self.assertLess(guard, device.index('/external/vk/opticalflow/supported'))
        self.assertLess(device.index("s_vk.nativeOpticalFlowHWSupport = false;"), guard)
        compute = (self.source / PATCH.COMPUTE).read_text(encoding="utf-8")
        constructor_guard = compute.index("if (selectedLatencyProperties.properties.vendorID == 0x10de)")
        self.assertLess(constructor_guard, compute.index("m_reflex = CreateVkNvLowLatency2("))
        self.assertLess(constructor_guard, compute.index("m_reflex = CreateNvLowLatencyVk("))
        self.assertEqual(len(receipt["edits"]), 3)
        self.assertEqual(receipt, json.loads(
            (self.source / "rsf-selected-device-fixes.json").read_text(encoding="utf-8")))

    def test_changed_anchor_refuses_without_mutation(self):
        path = self.source / PATCH.WRAPPER
        path.write_text(path.read_text(encoding="utf-8").replace(
            "// similarly add support for Vk 1.1 features", "// changed source"), encoding="utf-8")
        before = path.read_bytes()
        with self.assertRaises(SystemExit):
            PATCH.apply(self.source)
        self.assertEqual(before, path.read_bytes())
        self.assertFalse((self.source / PATCH.HEADER).exists())

    def test_duplicate_application_refuses_without_mutation(self):
        PATCH.apply(self.source)
        before = (self.source / PATCH.WRAPPER).read_bytes()
        with self.assertRaises(SystemExit):
            PATCH.apply(self.source)
        self.assertEqual(before, (self.source / PATCH.WRAPPER).read_bytes())

    def test_composes_with_current_checked_recipe(self):
        result = subprocess.run([sys.executable, str(FIXES / "prepare.py"), str(self.source)],
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("rsf::streamline::deviceConfigApplies(",
                      (self.source / PATCH.WRAPPER).read_text(encoding="utf-8"))
        self.assertTrue((self.source / "vulkanstory-fixes.json").is_file())
        self.assertTrue((self.source / "rsf-selected-device-fixes.json").is_file())


if __name__ == "__main__":
    unittest.main()
