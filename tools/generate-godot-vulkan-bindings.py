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
        "#include <rescaleframe/native_latency_vulkan.h>\n"
        "#include <rescaleframe/native_latency_vulkan_requirements.h>\n"
        "#include <rescaleframe/native_streamline_vulkan.h>\n"
        "#include <rescaleframe/native_xess_vulkan_requirements.h>\n"
        "#include <rescaleframe/native_sr_split_vulkan.h>\n"
        "#include <rescaleframe/input_dump_vulkan.h>\n"
        "#include <rescaleframe/input_dump.h>\n"
        '#include "integrations/godot/native/api.h"\n'
        '#include "runtime/orchestrator/src/native_fg_fsr4_vulkan.h"\n'
        "enum rsf_vk_tag {\n"
        + "".join(f"    TAG_{tag} = VK_STRUCTURE_TYPE_{tag},\n" for tag in tags)
        + "};\n",
        encoding="ascii",
    )
    types = (
        "Vk(InstanceCreateInfo|DeviceCreateInfo|BaseInStructure|BaseOutStructure|"
        "ExtensionProperties|PhysicalDeviceProperties|SwapchainCreateInfoKHR|PresentInfoKHR|PhysicalDevice(" + "|".join(features) + "))"
        "|PFN_vk(CreateInstance|CreateDevice|DestroyDevice|EnumerateDeviceExtensionProperties|"
        "GetPhysicalDeviceProperties|GetPhysicalDeviceMemoryProperties|GetPhysicalDeviceSurfaceCapabilitiesKHR|"
        "GetSwapchainImagesKHR|DeviceWaitIdle)|rsf_vk_tag|"
        "rsf_(latencyvk_(device_query|device_requirements|created_device|status)|slvk_setup|xessvk_setup|"
        "fgvk_(setup|frame|status|host_options)|srvk_(setup|frame|image|fallback)|fsr4vk_handoff|"
        "fsr4fg_info|input_dump_vk_setup|godot_vk_(image|packet|handoff))"
    )
    functions = (
        "rsf_(srvk_(create|plan|evaluate|destroy|retire_commands|dlss_preset|spatial_fallback|"
        "fsr4_(create|plan|evaluate|destroy|retire_commands))|slvk_initialize|"
        "xessvk_(initialize|shutdown|instance_requirements|device_requirements)|"
        "latencyvk_(instance_proc|options|presentation|submit_end|render|get_status|shutdown|"
        "require_device|device_created|device_destroyed)|"
        "fgvk_(create_with_host|images|enable|enable_options|backend|image_layout|max_generated|"
        "acquire|prepare|hudless|submitted|present|get_status|drain|destroy)|"
        "input_dump_(cancel|poll|requested|wants|begin_record|complete|pass_metadata|result))"
    )
    def generate(*extra):
        return subprocess.check_output(
            [bindgen, str(header), "--allowlist-type", f"^({types})$", "--allowlist-function", f"^{functions}$",
             "--blocklist-type", "^VkStructureType$", "--no-prepend-enum-name",
             "--allowlist-var", "^RSF_(LATENCYVK_(DEVICE_)?ABI_VERSION|SLVK_(ABI_VERSION|DLSS_SR|DLSS_FG)|"
             "XESSVK_REQUIREMENTS_ABI_VERSION|SRVK_(SPLIT_)?ABI_VERSION|FGVK_(HOST_)?ABI_VERSION|"
             "FSR4FG_BRIDGE_ABI_VERSION|INPUT_DUMP_VK_DEVICE_LOST|GODOT_(ABI_VERSION|VK_GRAPH_HANDOFF|VK_HANDOFF_ABI_VERSION))$",
             "--blocklist-type", "^rsf_(frame_record|game_render_pass|sharpening_options)$",
             "--raw-line", "#![allow(non_camel_case_types, non_snake_case, dead_code)]",
             "--raw-line", "#![allow(non_upper_case_globals)]",
             "--raw-line", "use rsf_game_sys::sharpening_abi::rsf_sharpening_options;",
             "--raw-line", "use rsf_game_sys::{rsf_frame_record, rsf_game_render_pass};",
             "--raw-line", "pub type VkStructureType = ::core::ffi::c_int;", "--use-core",
             "--no-doc-comments", "--no-derive-debug", "--with-derive-default", *extra,
             "--rust-edition", "2024", "--rust-target", "1.85",
             "--", "--target=x86_64-pc-windows-msvc", f"-I{include}",
             "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
             "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Iruntime/backends/dlss/include", "-Isdk/game/include", f"-I{root}"],
            cwd=root,
        )

    # Every record holds the blocklisted VkStructureType or an SDK record, so bindgen cannot prove
    # it Copy. They are all plain C data: name each record bindgen left without Copy and derive it.
    plain = generate().replace(b"\r\n", b"\n").decode()
    uncopied = re.findall(r"^#\[repr\(C\)\]\n(?!#\[derive\([^)]*Copy)(?:#\[.*\]\n)*pub struct (\w+) \{",
                          plain, re.M)
    output = generate("--with-derive-custom-struct", f"^({'|'.join(uncopied)})$=Copy,Clone")
(root / "integrations/godot/native/vulkan/creation/abi.rs").write_bytes(output.replace(b"\r\n", b"\n"))
