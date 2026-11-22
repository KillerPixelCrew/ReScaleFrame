// SPDX-License-Identifier: MIT
// Compile and call the enabled leaves without a graphics device or SDK.
#include "../runtime/orchestrator/src/native_dlss_vulkan.cpp"
#include <cstdio>

extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) {
    if (!call || call->abi_version != RSF_SLVK_ABI_VERSION || call->session_id || call->frame_id || !call->callback)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return RSF_BACKEND_ERROR_NOT_READY;
}

int main() {
    rsf_srvk_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.output_width = 640;
    setup.output_height = 360;
    void* native{};
    uint32_t width{}, height{};
    rsf_vkdlss_commands commands{};
    const auto* platform = rsf::dlss_vk::platform;
    if (platform && platform->open(&setup, &native, &width, &height, &commands) == RSF_BACKEND_ERROR_NOT_READY &&
        !native && !width && !height && !commands.barrier && !commands.copy && !commands.blit) return 0;
    std::fputs("The enabled DLSS leaves must preserve an SDK-lease refusal.\n", stderr);
    return 1;
}
