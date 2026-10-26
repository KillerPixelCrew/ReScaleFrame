// SPDX-License-Identifier: GPL-3.0-only
#include "drivers/d3d12/rendering_device_driver_d3d12.h"

void *RenderingDeviceDriverD3D12::rescaleframe_begin(CommandBufferID id) {
    auto *commands = reinterpret_cast<CommandBufferInfo *>(id.id);
    _resource_transitions_flush(commands);
    return commands->cmd_list.Get();
}
void RenderingDeviceDriverD3D12::rescaleframe_end(CommandBufferID id) {
    auto *commands = reinterpret_cast<CommandBufferInfo *>(id.id);
    commands->graphics_pso = nullptr;
    commands->compute_pso = nullptr;
    commands->graphics_root_signature_crc = 0;
    commands->compute_root_signature_crc = 0;
    commands->nir_graphics_runtime_data_root_param_idx = UINT32_MAX;
    commands->nir_compute_runtime_data_root_param_idx = UINT32_MAX;
    commands->descriptor_heaps_set = false;
    commands->pending_dyn_params = true;
}
