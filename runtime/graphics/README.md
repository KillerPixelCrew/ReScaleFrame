# Graphics and presentation

Resource ownership, conversions, synchronization, and presentation live here. The first planned path keeps AC7 and XeSS-SR on native DX11 and bridges to DX12 for XeSS MFG.

Keep that implementation replaceable. Vulkan/DXVK is another candidate rendering path, to be assessed through measurements. Game plugins supply engine-specific data and conventions; they do not own independent swap chains or vendor contexts.
