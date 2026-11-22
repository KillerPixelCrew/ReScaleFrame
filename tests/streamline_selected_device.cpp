// SPDX-License-Identifier: MIT
#include "../runtime/vulkan/streamline/sdk-fixes/selected-device-config.h"
// Native DLSS-G layout assertions for the Rust Streamline callers.
#include "../runtime/vulkan/streamline/sdk_fg_options_checks.h"
#include "vulkan-fg/streamline_abi.h"

static_assert(sl::kFeatureDLSS == 0 && sl::kFeatureDLSS_G == 1000 &&
    sl::kFeatureReflex == 3 && sl::kFeaturePCL == 4);

static sl::Result find_adapter(const sl::AdapterInfo& info, uint32_t mask) {
    switch (rsf_fixture_sl_find_adapter(info.vkPhysicalDevice != nullptr,
        info.deviceLUID, info.deviceLUIDSizeInBytes, mask)) {
    case 0: return sl::Result::eOk;
    case 1: return sl::Result::eErrorInvalidParameter;
    default: return sl::Result::eErrorAdapterNotSupported;
    }
}

extern "C" uint32_t rsf_fixture_sl_selected_mask(const uint8_t* luid, uint32_t length, uint32_t has_lookup) {
    return rsf::streamline::selectedAdapterMask(luid, length, has_lookup ? find_adapter : nullptr);
}
extern "C" uint32_t rsf_fixture_sl_config_applies(uint32_t supported, uint32_t selected) {
    return rsf::streamline::configApplies(supported, selected);
}
extern "C" uint32_t rsf_fixture_sl_device_config_applies(uint32_t feature, uint32_t supported,
    uint32_t selected, uint32_t vendor) {
    return rsf::streamline::deviceConfigApplies(static_cast<sl::Feature>(feature), supported, selected, vendor);
}
int main() { return rsf_fixture_sl_selected_run(); }
