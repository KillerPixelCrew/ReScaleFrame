"""Generate the typed Godot creation ABI from the pinned engine Vulkan headers."""

import argparse
import os
import re
import subprocess
from pathlib import Path
from tempfile import TemporaryDirectory

root = Path(__file__).resolve().parents[1]
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Use bindgen-cli 0.72.1")
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("include", type=Path, help="Godot engine thirdparty/vulkan/include")
include = parser.parse_args().include.resolve()
core = include / "vulkan/vulkan_core.h"
if not re.search(r"^#define VK_HEADER_VERSION 335$", core.read_text(encoding="utf-8"), re.M):
    raise SystemExit("Use the Vulkan headers from the pinned Godot 4.7.2 checkout")

features = (
    "Features2", "Vulkan11Features", "Vulkan12Features", "Vulkan13Features",
    "16BitStorageFeatures", "MultiviewFeatures", "ShaderFloat16Int8Features",
    "BufferDeviceAddressFeatures", "VulkanMemoryModelFeatures", "TimelineSemaphoreFeatures",
    "PipelineCreationCacheControlFeatures", "Synchronization2Features",
    "FragmentDensityMapFeaturesEXT", "FragmentDensityMapOffsetFeaturesQCOM",
    "FragmentShadingRateFeaturesKHR", "FaultFeaturesEXT", "AccelerationStructureFeaturesKHR",
    "RayTracingPipelineFeaturesKHR", "RayTracingValidationFeaturesNV",
    "ScalarBlockLayoutFeatures", "ShaderIntegerDotProductFeatures", "AntiLagFeaturesAMD",
    "MutableDescriptorTypeFeaturesEXT",
)
tags = (
    "APPLICATION_INFO", "INSTANCE_CREATE_INFO", "DEVICE_CREATE_INFO", "DEVICE_QUEUE_CREATE_INFO",
    "PHYSICAL_DEVICE_FEATURES_2", "PHYSICAL_DEVICE_VULKAN_1_1_FEATURES",
    "PHYSICAL_DEVICE_VULKAN_1_2_FEATURES", "PHYSICAL_DEVICE_VULKAN_1_3_FEATURES",
    "PHYSICAL_DEVICE_16BIT_STORAGE_FEATURES", "PHYSICAL_DEVICE_MULTIVIEW_FEATURES",
    "PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES", "PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES",
    "PHYSICAL_DEVICE_VULKAN_MEMORY_MODEL_FEATURES", "PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES",
    "PHYSICAL_DEVICE_PIPELINE_CREATION_CACHE_CONTROL_FEATURES",
    "PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES", "PHYSICAL_DEVICE_FRAGMENT_DENSITY_MAP_FEATURES_EXT",
    "PHYSICAL_DEVICE_FRAGMENT_DENSITY_MAP_OFFSET_FEATURES_QCOM",
    "PHYSICAL_DEVICE_FRAGMENT_SHADING_RATE_FEATURES_KHR", "PHYSICAL_DEVICE_FAULT_FEATURES_EXT",
    "PHYSICAL_DEVICE_ACCELERATION_STRUCTURE_FEATURES_KHR",
    "PHYSICAL_DEVICE_RAY_TRACING_PIPELINE_FEATURES_KHR",
    "PHYSICAL_DEVICE_RAY_TRACING_VALIDATION_FEATURES_NV",
    "PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES",
    "PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES", "PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD",
    "PHYSICAL_DEVICE_MUTABLE_DESCRIPTOR_TYPE_FEATURES_EXT",
)
assert len(features) + 4 == len(tags)
with TemporaryDirectory() as directory:
    header = Path(directory) / "godot_vk_creation.h"
    header.write_text(
        "#define VK_NO_PROTOTYPES\n#include <vulkan/vulkan_core.h>\n"
        "#include <rescaleframe/native_latency_vulkan_requirements.h>\n"
        "enum rsf_vk_tag {\n"
        + "".join(f"    TAG_{tag} = VK_STRUCTURE_TYPE_{tag},\n" for tag in tags)
        + "};\n",
        encoding="ascii",
    )
    types = (
        "Vk(InstanceCreateInfo|DeviceCreateInfo|BaseInStructure|BaseOutStructure|"
        "ExtensionProperties|PhysicalDeviceProperties|PhysicalDevice(" + "|".join(features) + "))"
        "|PFN_vk(CreateInstance|CreateDevice|DestroyDevice|EnumerateDeviceExtensionProperties|"
        "GetPhysicalDeviceProperties)|rsf_vk_tag|"
        "rsf_latencyvk_(device_query|device_requirements|created_device)"
    )
    output = subprocess.check_output(
        [bindgen, str(header), "--allowlist-type", f"^({types})$",
         "--blocklist-type", "^VkStructureType$", "--no-prepend-enum-name",
         "--allowlist-var", "^RSF_LATENCYVK_DEVICE_ABI_VERSION$",
         "--raw-line", "#![allow(non_camel_case_types, non_snake_case, dead_code)]",
         "--raw-line", "pub type VkStructureType = ::core::ffi::c_int;", "--use-core",
         "--no-doc-comments", "--no-derive-debug", "--with-derive-default",
         "--rust-edition", "2024", "--rust-target", "1.85",
         "--", "--target=x86_64-pc-windows-msvc", f"-I{include}",
         "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
         "-Iruntime/graphics/include", "-Isdk/game/include"],
        cwd=root,
    )
(root / "integrations/godot/native/vulkan/creation/abi.rs").write_bytes(output.replace(b"\r\n", b"\n"))
