"""Checked selected-device requirement filtering for pinned Streamline 2.14.1.

Kept separate until the coordinated SDK rebuild can update prepare.py and its pin.
"""
import argparse
import hashlib
import json
from pathlib import Path

COMMIT = "2122257e0fce486f91b385aa63b9a09b0a34b363"
WRAPPER = "source/core/sl.interposer/vulkan/wrapper.cpp"
COMPUTE = "source/platforms/sl.chi/vulkan.cpp"
HEADER = "source/core/sl.interposer/vulkan/rsfSelectedDeviceConfig.h"


def apply(sdk: Path):
    sdk = sdk.resolve()
    sources = {file: (sdk / file).read_text(encoding="utf-8") for file in (WRAPPER, COMPUTE)}
    edits = []

    def replace(before, after, file=WRAPPER):
        source = sources[file]
        observed = source.count(before)
        if observed != 1:
            raise SystemExit(f"{file}: expected one selected-device anchor, found {observed}")
        sources[file] = source.replace(before, after)
        edits.append({"file": file, "before": before, "after": after, "count": 1})

    replace('#include "include/sl_helpers_vk.h"',
            '#include "include/sl_helpers_vk.h"\n#include "rsfSelectedDeviceConfig.h"')
    replace("""        pluginManager->getLoadedFeatureConfigs(configs);
        for (auto& cfg : configs)
        {
            // similarly add support for Vk 1.1 features, if any plugin requests such a feature.""",
            """        pluginManager->getLoadedFeatureConfigs(configs);
        VkPhysicalDeviceIDProperties selectedID{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
        VkPhysicalDeviceProperties2 selectedProperties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2, &selectedID};
        s_idt.GetPhysicalDeviceProperties2(physicalDevice, &selectedProperties);
        rsf::streamline::FindAdapter* findAdapter = nullptr;
        sl::param::getPointerParam(sl::param::getInterface(),
            sl::param::common::kPFunFindAdapter, &findAdapter, true);
        const uint32_t selectedAdapter = selectedID.deviceLUIDValid ?
            rsf::streamline::selectedAdapterMask(selectedID.deviceLUID, VK_LUID_SIZE, findAdapter) : 0;
        if (!selectedAdapter)
        {
            SL_LOG_ERROR("Cannot resolve selected Vulkan device to one Streamline adapter LUID");
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        // These are singleton SDK state, so do not inherit another device's OFA flag.
        s_vk.nativeOpticalFlowHWSupport = false;
        for (auto& cfg : configs)
        {
            const uint32_t supportedAdapters = cfg.at("supportedAdapters").get<uint32_t>();
            if (!rsf::streamline::deviceConfigApplies(cfg.at("id").get<uint32_t>(),
                supportedAdapters, selectedAdapter, selectedProperties.properties.vendorID))
            {
                SL_LOG_INFO("Skipping device requirements for plugin %u: supported mask 0x%x, selected 0x%x",
                    cfg.at("id").get<uint32_t>(), supportedAdapters, selectedAdapter);
                continue;
            }
            // similarly add support for Vk 1.1 features, if any plugin requests such a feature.""")
    replace("""    m_reflex = CreateVkNvLowLatency2(m_device, params, m_vk);
    #ifdef SL_WITH_NVLLVK
    if (!m_reflex)
    {
        m_reflex = CreateNvLowLatencyVk(m_device, params, m_vk);
    }
    #endif""", """    VkPhysicalDeviceProperties2 selectedLatencyProperties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2};
    m_idt.GetPhysicalDeviceProperties2(m_physicalDevice, &selectedLatencyProperties);
    // The plugin's statistics support mask includes non-NVIDIA adapters. Neither
    // NVIDIA latency backend may be initialized for those selected devices.
    if (selectedLatencyProperties.properties.vendorID == 0x10de)
    {
        m_reflex = CreateVkNvLowLatency2(m_device, params, m_vk);
        #ifdef SL_WITH_NVLLVK
        if (!m_reflex)
        {
            m_reflex = CreateNvLowLatencyVk(m_device, params, m_vk);
        }
        #endif
    }
    else
    {
        m_reflex = nullptr;
        SL_LOG_INFO("Selected Vulkan device has no NVIDIA latency backend; PCL statistics remain available");
    }""", COMPUTE)
    target = sdk / HEADER
    if target.exists():
        raise SystemExit(f"{HEADER}: already exists; use an unpatched source copy")
    header = Path(__file__).with_name("selected-device-config.h").read_bytes()
    # Validate every anchor before changing either source file.
    before = {file: (sdk / file).read_bytes() for file in sources}
    for file, source in sources.items():
        (sdk / file).write_text(source, encoding="utf-8", newline="\n")
    target.write_bytes(header)
    receipt = {
        "sdkCommit": COMMIT,
        "sources": {file: {
            "beforeSha256": hashlib.sha256(before[file]).hexdigest(),
            "afterSha256": hashlib.sha256((sdk / file).read_bytes()).hexdigest(),
        } for file in sources},
        "headerSha256": hashlib.sha256(header).hexdigest(),
        "edits": edits,
    }
    (sdk / "rsf-selected-device-fixes.json").write_text(
        json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sdk", type=Path)
    result = apply(parser.parse_args().sdk)
    print(f"Applied {len(result['edits'])} checked selected-device edits")
